//! Private member-source primitives for current membership refresh.
use super::{PatreonError, identifier};
use crate::{
    backend::{BackendClient, BackendError},
    provider_json::Object,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::error::Error;
use uuid::Uuid;

pub(super) enum Failure {
    Backend(BackendError),
    Provider(PatreonError),
    Contract,
}
impl Failure {
    pub(super) fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            Self::Provider(e) => Some(e),
            Self::Contract => None,
        }
    }
}
pub(super) fn parse_id(value: &str) -> Result<Uuid, &'static str> {
    crate::identifiers::parse(value).map_err(|_| "invalid canonical identifier")
}
pub(super) fn canonical_id<'de, D: Deserializer<'de>>(input: D) -> Result<Uuid, D::Error> {
    let value = String::deserialize(input)?;
    parse_id(&value).map_err(de::Error::custom)
}
pub(super) fn optional_id<'de, D: Deserializer<'de>>(input: D) -> Result<Option<Uuid>, D::Error> {
    Option::<String>::deserialize(input)?
        .as_deref()
        .map(parse_id)
        .transpose()
        .map_err(de::Error::custom)
}
pub(super) fn valid_context(
    version: u8,
    user: &str,
    campaign: &str,
    member: &str,
    generation: &str,
) -> bool {
    version == 1
        && identifier(user)
        && identifier(campaign)
        && identifier(member)
        && generation
            .parse::<i64>()
            .is_ok_and(|n| n > 0 && n.to_string() == generation)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireTask {
    #[serde(deserialize_with = "canonical_id")]
    association_id: Uuid,
    #[serde(deserialize_with = "canonical_id")]
    request_id: Uuid,
}
/// Only application identity crosses a task boundary; its public wrappers remain distinct domains.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemberTask {
    association_id: Uuid,
    request_id: Uuid,
}
impl MemberTask {
    pub(super) fn new(association_id: Uuid, request_id: Uuid) -> Option<Self> {
        (!association_id.is_nil() && !request_id.is_nil()).then_some(Self {
            association_id,
            request_id,
        })
    }
    pub(super) fn association_id(&self) -> Uuid {
        self.association_id
    }
    pub(super) fn request_id(&self) -> Uuid {
        self.request_id
    }
}
impl<'de> Deserialize<'de> for MemberTask {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let value = Object::<WireTask>::deserialize(input)?.0;
        Ok(Self {
            association_id: value.association_id,
            request_id: value.request_id,
        })
    }
}

/// One maintained bounded candidate parser. Selection is not source admission.
pub(super) fn read_candidate(
    backend: &BackendClient,
    campaign: &str,
    after: Option<Uuid>,
) -> Result<Option<Uuid>, Failure> {
    if after.is_some_and(|id| id.is_nil()) {
        return Err(Failure::Contract);
    }
    let rows: Vec<String> = backend
        .rpc(
            "read_patreon_refresh_work",
            &serde_json::json!({"p_campaign":campaign,"p_after":after,"p_limit":1}),
        )
        .map_err(Failure::Backend)?;
    if rows.len() > 1 {
        return Err(Failure::Contract);
    }
    let Some(raw) = rows.first() else {
        return Ok(None);
    };
    let association = parse_id(raw).map_err(|_| Failure::Contract)?;
    if after.is_some_and(|id| association <= id) {
        return Err(Failure::Contract);
    }
    Ok(Some(association))
}
