//! Session/receipt/provider coordinator. No automatic retry of a one-use code or uncertain claim.
use super::{
    PatreonError,
    association::{
        CompleteLinkRequest, InternalStatus, LinkPhase, LinkRequest, LinkStart, LinkStatus,
    },
    client::PatreonClient,
    oauth::OAuthConfig,
};
use crate::{
    ApiResponse,
    actor::{ActorState, RequestActor},
    backend::{BackendClient, BackendConfig, BackendError},
    commands::CommandAck,
    provider_json::fingerprint,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error, fmt, sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) enum ServiceError {
    Backend(BackendError),
    Provider(PatreonError),
    Contract,
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("patreon association is temporarily unresolved")
    }
}
impl fmt::Debug for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for ServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            Self::Provider(e) => Some(e),
            Self::Contract => None,
        }
    }
}
impl From<BackendError> for ServiceError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}
impl From<PatreonError> for ServiceError {
    fn from(e: PatreonError) -> Self {
        Self::Provider(e)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartAck {
    resource_id: Uuid,
    expires_at: DateTime<Utc>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimAck {
    may_exchange: bool,
}
/// No provider configuration is needed to recover an existing owned flow status.
pub(crate) struct AssociationService {
    backend: BackendClient,
    config: Option<Arc<OAuthConfig>>,
}
impl AssociationService {
    pub fn new(
        backend: BackendConfig,
        config: Option<Arc<OAuthConfig>>,
    ) -> Result<Self, ServiceError> {
        Ok(Self {
            backend: BackendClient::new(
                backend,
                "spellwake-patreon-association",
                Duration::from_secs(60),
            )?,
            config,
        })
    }
    fn rpc<T: DeserializeOwned>(
        &self,
        name: &str,
        body: &Value,
    ) -> Result<ApiResponse<T>, ServiceError> {
        let result = self
            .backend
            .rpc::<crate::provider_json::ObjectResponse<T>>(name, body)?
            .0;
        match &result {
            ApiResponse::Error { .. } => return Err(ServiceError::Contract),
            ApiResponse::Fail { data } if !allowed_failure(data) => {
                return Err(ServiceError::Contract);
            }
            _ => {}
        }
        Ok(result)
    }
    fn read(
        &self,
        actor: &RequestActor,
        request: &LinkRequest,
        code: Option<&str>,
    ) -> Result<ApiResponse<InternalStatus>, ServiceError> {
        let account = actor
            .account()
            .filter(|id| !id.is_nil())
            .ok_or(ServiceError::Contract)?;
        let state = request.state_hash().map_err(|_| ServiceError::Contract)?;
        let result:ApiResponse<InternalStatus>=self.rpc("read_patreon_link",&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request.request_id,"p_state_hash":state,"p_code_hash":code}))?;
        if let ApiResponse::Success { data } = &result {
            data.validate(account, request.request_id)?;
        }
        Ok(result)
    }
    pub fn status(
        &self,
        actor: &RequestActor,
        request: &LinkRequest,
    ) -> Result<ApiResponse<LinkStatus>, ServiceError> {
        match self.read(actor, request, None)? {
            ApiResponse::Success { data } => Ok(ApiResponse::Success {
                data: data.public(),
            }),
            ApiResponse::Fail { data } => Ok(ApiResponse::Fail { data }),
            ApiResponse::Error { .. } => Err(ServiceError::Contract),
        }
    }
    pub fn start(
        &self,
        actor: &RequestActor,
        request: &LinkRequest,
    ) -> Result<ApiResponse<LinkStart>, ServiceError> {
        let account = actor
            .account()
            .filter(|id| !id.is_nil())
            .ok_or(ServiceError::Contract)?;
        if actor.state() != ActorState::Active {
            return Ok(fail("account", "unavailable"));
        }
        let config = self.config.as_ref().ok_or(ServiceError::Contract)?;
        let state = request.state_hash().map_err(|_| ServiceError::Contract)?;
        let ack:StartAck=match self.rpc("start_patreon_link",&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request.request_id,"p_state_hash":state,"p_terms":config.terms}))?{
            ApiResponse::Success{data}=>data,ApiResponse::Fail{data}=>return Ok(ApiResponse::Fail{data}),ApiResponse::Error{..}=>return Err(ServiceError::Contract),
        };
        if ack.resource_id != request.request_id {
            return Err(ServiceError::Contract);
        }
        let status = match self.read(actor, request, None)? {
            ApiResponse::Success { data } => data,
            ApiResponse::Fail { data } => return Ok(ApiResponse::Fail { data }),
            ApiResponse::Error { .. } => return Err(ServiceError::Contract),
        };
        if status.phase != LinkPhase::Issued {
            return Ok(fail("request", phase_name(status.phase)));
        }
        let terms = status.server_terms.as_ref().ok_or(ServiceError::Contract)?;
        if !config.matches(terms) || status.expires_at != Some(ack.expires_at) {
            return Err(ServiceError::Contract);
        }
        Ok(ApiResponse::Success {
            data: LinkStart {
                account_id: account,
                request_id: request.request_id,
                expires_at: ack.expires_at,
                authorization_url: terms.authorization_url(&request.state_key)?,
            },
        })
    }
    pub fn complete(
        &self,
        actor: &RequestActor,
        request: &CompleteLinkRequest,
    ) -> Result<ApiResponse<LinkStatus>, ServiceError> {
        request.validate().map_err(|_| ServiceError::Contract)?;
        let flow = request.flow();
        let state = flow.state_hash().map_err(|_| ServiceError::Contract)?;
        let code = fingerprint(request.code.as_bytes());
        let status = match self.read(actor, &flow, Some(&code))? {
            ApiResponse::Success { data } => data,
            ApiResponse::Fail { data } => return Ok(ApiResponse::Fail { data }),
            ApiResponse::Error { .. } => return Err(ServiceError::Contract),
        };
        if status.phase != LinkPhase::Issued {
            return Ok(ApiResponse::Success {
                data: status.public(),
            });
        }
        if actor.state() != ActorState::Active {
            return Ok(fail("account", "unavailable"));
        }
        let config = self.config.as_ref().ok_or(ServiceError::Contract)?;
        let frozen = status.server_terms.as_ref().ok_or(ServiceError::Contract)?;
        if !config.matches(frozen) {
            return Err(ServiceError::Contract);
        }
        let claim = fingerprint(Uuid::new_v4().as_bytes());
        let permission:ClaimAck=match self.rpc("claim_patreon_link",&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request.request_id,"p_state_hash":state,"p_code_hash":code,"p_claim_hash":claim}))?{
            ApiResponse::Success{data}=>data,ApiResponse::Fail{data}=>return Ok(ApiResponse::Fail{data}),ApiResponse::Error{..}=>return Err(ServiceError::Contract),
        };
        if !permission.may_exchange {
            let observed = self.read(actor, &flow, Some(&code))?;
            if matches!(&observed,ApiResponse::Success{data} if matches!(data.phase,LinkPhase::Unknown|LinkPhase::Issued))
            {
                return Err(ServiceError::Contract);
            }
            return Ok(public_response(observed));
        }
        // Do not create a provider client or exchange a code until first-claim acknowledgment is known.
        let provider = PatreonClient::new(config.source.clone())?;
        let user = provider.exchange(config, frozen, &request.code)?;
        let identity = provider.linked_identity(&user)?;
        let Some(member_id) = identity.member_id() else {
            return Ok(fail("membership", "unavailable"));
        };
        let member = provider.member(member_id, identity.user_id())?;
        let proof = json!({"version":1,"source":"patreon-v2-identity-member-v1","campaignId":member.campaign_id,"userId":member.user_id,"memberId":member.member_id,
            "identityHash":identity.body_sha256(),"memberHash":member.body_sha256,"entitledTierIds":member.entitled_tier_ids});
        let ack:CommandAck=match self.rpc("complete_patreon_link",&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request.request_id,"p_state_hash":state,"p_code_hash":code,"p_claim_hash":claim,"p_proof":proof}))?{
            ApiResponse::Success{data}=>data,ApiResponse::Fail{data}=>return Ok(ApiResponse::Fail{data}),ApiResponse::Error{..}=>return Err(ServiceError::Contract),
        };
        let result = self.read(actor, &flow, Some(&code))?;
        match &result {
            ApiResponse::Success { data }
                if data.phase == LinkPhase::Linked
                    && data.association_id == Some(ack.resource_id)
                    && !ack.resource_id.is_nil() => {}
            ApiResponse::Fail { .. } => {}
            _ => return Err(ServiceError::Contract),
        }
        Ok(public_response(result))
    }
}
fn public_response(result: ApiResponse<InternalStatus>) -> ApiResponse<LinkStatus> {
    match result {
        ApiResponse::Success { data } => ApiResponse::Success {
            data: data.public(),
        },
        ApiResponse::Fail { data } => ApiResponse::Fail { data },
        ApiResponse::Error { message, code } => ApiResponse::Error { message, code },
    }
}
fn fail<T>(key: &str, value: &str) -> ApiResponse<T> {
    ApiResponse::Fail {
        data: BTreeMap::from([(key.into(), value.into())]),
    }
}
fn phase_name(phase: LinkPhase) -> &'static str {
    match phase {
        LinkPhase::Unknown => "unknown",
        LinkPhase::Issued => "issued",
        LinkPhase::Claimed => "claimed",
        LinkPhase::Linked => "linked",
        LinkPhase::Expired => "expired",
        LinkPhase::Superseded => "superseded",
    }
}
fn allowed_failure(data: &BTreeMap<String, String>) -> bool {
    data.len() == 1
        && data.iter().all(|(key, value)| match key.as_str() {
            "session" => value == "unauthorized",
            "account" | "membership" => value == "unavailable",

            "request" => matches!(
                value.as_str(),
                "conflict" | "unknown" | "expired" | "superseded" | "spent"
            ),
            _ => false,
        })
}
