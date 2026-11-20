//! Hash-only room capabilities with deterministic server-owned delivery and exact recovery.
use crate::{
    amount::Amount,
    commands::{CommandAck, CommandDisposition},
    progression::validate_page,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::Sha256;
use std::{collections::BTreeMap, fmt};
use uuid::Uuid;
/// A dedicated server secret. It is neither printable nor serializable.
pub struct InvitationKey([u8; 32]);
impl InvitationKey {
    /// # Errors
    /// Rejects aliases, padding and keys that are not32 decoded bytes.
    pub fn new(raw: &str) -> Result<Self, &'static str> {
        crate::capability::decode_key(raw).map(Self)
    }
    /// Recover the same capability across sessions/restarts while this dedicated key is retained.
    /// # Errors
    /// Rejects nil context and any unrepresentable framing or cryptographic initialization.
    pub fn derive(
        &self,
        actor: Uuid,
        room: Uuid,
        request: Uuid,
        basis: &Amount,
    ) -> Result<InvitationToken, &'static str> {
        if [actor, room, request].iter().any(Uuid::is_nil) {
            return Err("invalid invitation context");
        }
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.0).map_err(|_| "invalid invitation key")?;
        mac.update(b"spellwake-room-invitation-v1\0");
        let basis = basis.to_string();
        for value in [
            actor.as_bytes().as_slice(),
            room.as_bytes().as_slice(),
            request.as_bytes().as_slice(),
            basis.as_bytes(),
        ] {
            let length = u16::try_from(value.len()).map_err(|_| "invalid invitation context")?;
            mac.update(&length.to_be_bytes());
            mac.update(value);
        }
        Ok(InvitationToken(
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()),
        ))
    }
}
/// Serialize only for intentional API delivery. Diagnostic formatting always redacts the capability.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct InvitationToken(String);
impl fmt::Debug for InvitationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InvitationToken([REDACTED])")
    }
}
impl<'de> Deserialize<'de> for InvitationToken {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(input)?;
        crate::capability::decode_key(&raw).map_err(serde::de::Error::custom)?;
        Ok(Self(raw))
    }
}
impl InvitationToken {
    pub(crate) fn hash(&self) -> Result<String, &'static str> {
        crate::capability::key_hash(&self.0)
    }
}
/// Original host intent; the service derives the capability and stores only its hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueInvitation {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_revision: Amount,
}
/// A required nullable shared capability; the canonical source chooses the human slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinRoom {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "Option::deserialize")]
    pub token: Option<InvitationToken>,
}
/// Issuer-owned release remains available while account deletion is pending.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeInvitation {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
}
/// Delivery adds the token only after a validated accepted canonical receipt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedInvitation {
    pub resource_id: Uuid,
    pub token: InvitationToken,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinAck {
    pub resource_id: Uuid,
    pub slot_id: Uuid,
}
impl<'de> Deserialize<'de> for JoinAck {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            slot_id: Uuid,
        }
        let wire: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: wire.resource_id,
            slot_id: wire.slot_id,
        })
    }
}
impl JoinAck {
    /// Bind a parsed response to its original requested room and a real slot identity.
    pub fn valid(&self, room: Uuid) -> bool {
        !room.is_nil() && self.resource_id == room && !self.slot_id.is_nil()
    }
}
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum JoinResponse {
    Ack(JoinAck),
    Disposition(CommandDisposition<JoinAck>),
}
impl<'de> Deserialize<'de> for IssuedInvitation {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            token: InvitationToken,
        }
        let value: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: value.resource_id,
            token: value.token,
        })
    }
}
/// Safe issuer metadata never includes the hash or raw token.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvitationSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub room_id: Uuid,
    pub state: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvitationPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<InvitationSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl InvitationPage {
    /// # Errors
    /// Rejects foreign identity/filter, malformed safe metadata and mismatched pagination.
    pub fn validate(
        &self,
        owner: Uuid,
        room: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || room.is_some_and(|id| id.is_nil())
            || self.items.iter().any(|i| {
                i.room_id.is_nil()
                    || room.is_some_and(|id| id != i.room_id)
                    || !matches!(i.state.as_str(), "active" | "canceled")
            })
        {
            return Err("invalid invitation page");
        }
        validate_page(
            &self.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )
    }
}
pub(crate) fn issue_failure(errors: &BTreeMap<String, String>) -> bool {
    known_failure(
        errors,
        &[
            ("account", "unavailable"),
            ("room", "unavailable"),
            ("revision", "conflict"),
        ],
    )
}
pub(crate) fn join_failure(errors: &BTreeMap<String, String>) -> bool {
    known_failure(
        errors,
        &[
            ("room", "unavailable"),
            ("invitation", "unavailable"),
            ("profile", "unqualified"),
            ("capacity", "full"),
        ],
    )
}
pub(crate) fn revoke_failure(errors: &BTreeMap<String, String>) -> bool {
    known_failure(
        errors,
        &[("account", "unavailable"), ("invitation", "unavailable")],
    )
}
fn known_failure(errors: &BTreeMap<String, String>, allowed: &[(&str, &str)]) -> bool {
    errors.len() == 1
        && errors
            .iter()
            .next()
            .is_some_and(|(k, v)| allowed.contains(&(k.as_str(), v.as_str())))
}
pub(crate) fn deliver(ack: CommandAck, token: InvitationToken) -> Option<IssuedInvitation> {
    (!ack.resource_id.is_nil()).then_some(IssuedInvitation {
        resource_id: ack.resource_id,
        token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitation_derivation_matches_independent_hmac_vector_and_separates_context() {
        let key = InvitationKey::new("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8").unwrap();
        let actor: Uuid = "11111111-1111-1111-1111-111111111111".parse().unwrap();
        let room: Uuid = "22222222-2222-2222-2222-222222222222".parse().unwrap();
        let request: Uuid = "33333333-3333-3333-3333-333333333333".parse().unwrap();
        let basis: Amount = "9007199254740993".parse().unwrap();
        let token = key.derive(actor, room, request, &basis).unwrap();
        assert_eq!(
            serde_json::to_value(&token).unwrap(),
            serde_json::json!("hsG1dhm-yoA1Mb_-AO1SC3I0jVFjtCXfHAcHX7WWuGY")
        );
        assert_eq!(token, key.derive(actor, room, request, &basis).unwrap());
        for alternative in [
            key.derive(Uuid::new_v4(), room, request, &basis).unwrap(),
            key.derive(actor, Uuid::new_v4(), request, &basis).unwrap(),
            key.derive(actor, room, Uuid::new_v4(), &basis).unwrap(),
            key.derive(actor, room, request, &Amount::from(1)).unwrap(),
        ] {
            assert_ne!(token, alternative);
        }
        assert!(key.derive(Uuid::nil(), room, request, &basis).is_err());
    }
    #[test]
    fn private_capability_aliases_are_rejected_and_diagnostics_redact_delivery() {
        let raw = "A".repeat(43);
        let token: InvitationToken = serde_json::from_value(serde_json::json!(raw)).unwrap();
        for alias in [
            format!("{raw}="),
            format!("{}B", "A".repeat(42)),
            "A".repeat(42),
            "!".repeat(43),
        ] {
            assert!(serde_json::from_value::<InvitationToken>(serde_json::json!(alias)).is_err());
            assert!(InvitationKey::new(&alias).is_err());
        }
        let issued = IssuedInvitation {
            resource_id: Uuid::new_v4(),
            token: token.clone(),
        };
        let join = JoinRoom {
            request_id: Uuid::new_v4(),
            token: Some(token),
        };
        assert!(!format!("{issued:#?}").contains(&raw));
        assert!(!format!("{join:#?}").contains(&raw));
        assert_eq!(serde_json::to_value(issued).unwrap()["token"], raw);
    }
}
