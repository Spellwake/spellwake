//! Client mutation payloads never contain a trusted actor, owner or provider principal.

use serde::{Deserialize, Deserializer, Serialize, de};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Minimal durable acknowledgment; clients query current authorized resource state separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandAck {
    pub resource_id: Uuid,
}

impl<'de> Deserialize<'de> for CommandAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WireAck {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
        }
        let value: WireAck = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: value.resource_id,
        })
    }
}

/// A resource acknowledgment preserves the exact canonical resulting revision.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionAck {
    pub resource_id: Uuid,
    pub revision: crate::amount::Amount,
}
impl<'de> Deserialize<'de> for RevisionAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            revision: crate::amount::Amount,
        }
        let value: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: value.resource_id,
            revision: value.revision,
        })
    }
}
/// Decode directly from response bytes so duplicate keys survive until typed validation.
#[derive(Deserialize)]
#[serde(untagged, bound(deserialize = "A: Deserialize<'de>"))]
pub(crate) enum CommandResponse<A = CommandAck> {
    Ack(#[serde(deserialize_with = "crate::provider_json::object")] A),
    Disposition(CommandDisposition<A>),
}

/// An unknown acknowledgment is not assumed accepted and does not authorize a new request ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "WireDisposition<A>",
    into = "WireDisposition<A>",
    bound(
        deserialize = "A: Deserialize<'de>",
        serialize = "A: Serialize + Clone"
    )
)]
pub enum CommandDisposition<A = CommandAck> {
    Unknown,
    Pending,
    Accepted { ack: A },
    Rejected { errors: BTreeMap<String, String> },
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(deserialize = "A: Deserialize<'de>")
)]
enum WireDisposition<A> {
    Unknown {
        response: (),
    },
    Pending {
        response: (),
    },
    Accepted {
        #[serde(deserialize_with = "crate::provider_json::object_response")]
        response: crate::ApiResponse<A>,
    },
    Rejected {
        #[serde(deserialize_with = "crate::provider_json::object_response")]
        response: crate::ApiResponse<A>,
    },
}

impl<A> TryFrom<WireDisposition<A>> for CommandDisposition<A> {
    type Error = &'static str;

    fn try_from(value: WireDisposition<A>) -> Result<Self, Self::Error> {
        match value {
            WireDisposition::Unknown { .. } => Ok(Self::Unknown),
            WireDisposition::Pending { .. } => Ok(Self::Pending),
            WireDisposition::Accepted {
                response: crate::ApiResponse::Success { data },
            } => Ok(Self::Accepted { ack: data }),
            WireDisposition::Rejected {
                response: crate::ApiResponse::Fail { data },
            } => Ok(Self::Rejected { errors: data }),
            _ => Err("command state and response do not agree"),
        }
    }
}

impl<A> From<CommandDisposition<A>> for WireDisposition<A> {
    fn from(value: CommandDisposition<A>) -> Self {
        match value {
            CommandDisposition::Unknown => Self::Unknown { response: () },
            CommandDisposition::Pending => Self::Pending { response: () },
            CommandDisposition::Accepted { ack } => Self::Accepted {
                response: crate::ApiResponse::Success { data: ack },
            },
            CommandDisposition::Rejected { errors } => Self::Rejected {
                response: crate::ApiResponse::Fail { data: errors },
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum FriendAction {
    Request,
    Accept,
    Remove,
}
impl TryFrom<String> for FriendAction {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "request" => Ok(Self::Request),
            "accept" => Ok(Self::Accept),
            "remove" => Ok(Self::Remove),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FriendCommand {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub other_account_id: Uuid,
    pub action: FriendAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockCommand {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub other_account_id: Uuid,
    pub blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenDirectConversation {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub other_account_id: Uuid,
}

/// Keep the submitted text intact while bounding decoded Unicode characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct MessageBody(String);

impl MessageBody {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for MessageBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.trim().is_empty() || text.chars().count() > 4000 || text.contains('\0') {
            return Err(de::Error::custom(
                "message must contain 1 to 4000 characters",
            ));
        }
        Ok(Self(text))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendDirectMessage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub conversation_id: Uuid,
    pub body: MessageBody,
}

/// Mark only the caller's notifications for these exact immutable conversation messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkDirectRead {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub conversation_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub message_ids: Vec<Uuid>,
}
impl MarkDirectRead {
    /// # Errors
    /// Rejects nil identities, duplicate messages or a set outside the page bound.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || self.conversation_id.is_nil()
            || self.message_ids.is_empty()
            || self.message_ids.len() > crate::social::DIRECT_MESSAGE_PAGE_LIMIT
            || self.message_ids.iter().any(Uuid::is_nil)
            || self
                .message_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.message_ids.len()
        {
            return Err("read acknowledgment identities are required");
        }
        Ok(())
    }
}

/// One explicit social operation, retaining the canonical domain command body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SocialCommand {
    Friendship(FriendCommand),
    Block(BlockCommand),
    OpenDm(OpenDirectConversation),
    SendDm(SendDirectMessage),
}
impl<'de> Deserialize<'de> for SocialCommand {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let wire: crate::provider_json::CommandObject = crate::provider_json::object(input)?;
        match wire.kind.as_str() {
            "friendship" => wire.payload().map(Self::Friendship),
            "block" => wire.payload().map(Self::Block),
            "open_dm" => wire.payload().map(Self::OpenDm),
            "send_dm" => wire.payload().map(Self::SendDm),
            _ => Err(serde::de::Error::custom("invalid command kind")),
        }
    }
}

impl SocialCommand {
    /// Reject nil identities before invoking session-bound persistence.
    ///
    /// # Errors
    /// Returns an error when the request or target has no usable identity.
    pub fn validate(&self) -> Result<(), &'static str> {
        let (request, target) = match self {
            Self::Friendship(command) => (command.request_id, command.other_account_id),
            Self::Block(command) => (command.request_id, command.other_account_id),
            Self::OpenDm(command) => (command.request_id, command.other_account_id),
            Self::SendDm(command) => (command.request_id, command.conversation_id),
        };
        if request.is_nil() || target.is_nil() {
            return Err("social request and target identities are required");
        }
        Ok(())
    }
}

/// Only known business rejections cross the public social boundary.
pub(crate) fn social_failure(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && matches!(
            errors
                .iter()
                .next()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            Some(
                ("contact", "contact unavailable")
                    | (
                        "friendship",
                        "no incoming request to accept" | "friendship unavailable"
                    )
                    | ("conversation", "conversation unavailable")
                    | ("body", "message must contain 1 to 4000 characters")
                    | ("action", "invalid friend action")
            )
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn commands_reject_actor_injection_and_invalid_actions() {
        let mut payload =
            json!({"requestId":Uuid::new_v4(),"otherAccountId":Uuid::new_v4(),"action":"request"});
        assert!(serde_json::from_value::<FriendCommand>(payload.clone()).is_ok());
        payload["actorId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<FriendCommand>(payload.clone()).is_err());
        payload.as_object_mut().unwrap().remove("actorId");
        payload["action"] = json!("merge");
        assert!(serde_json::from_value::<FriendCommand>(payload).is_err());
    }

    #[test]
    fn disposition_cannot_turn_unknown_or_rejected_into_success() {
        assert_eq!(
            serde_json::from_value::<CommandDisposition>(
                json!({"state":"unknown","response":null})
            )
            .unwrap(),
            CommandDisposition::Unknown
        );
        let success = json!({"status":"success","data":{"resourceId":Uuid::new_v4()}});
        assert!(
            serde_json::from_value::<CommandDisposition>(
                json!({"state":"unknown","response":success})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<CommandDisposition>(
                json!({"state":"rejected","response":success})
            )
            .is_err()
        );
        let accepted = serde_json::from_value::<CommandDisposition>(
            json!({"state":"accepted","response":success}),
        )
        .unwrap();
        assert!(matches!(accepted, CommandDisposition::Accepted { .. }));
        assert_eq!(serde_json::to_value(accepted).unwrap()["state"], "accepted");
    }

    #[test]
    fn message_bound_counts_characters_and_preserves_content() {
        let body = " 🐉\n";
        assert_eq!(
            serde_json::from_value::<MessageBody>(json!(body))
                .unwrap()
                .as_str(),
            body
        );
        assert!(serde_json::from_value::<MessageBody>(json!("🐉".repeat(4000))).is_ok());
        assert!(serde_json::from_value::<MessageBody>(json!("🐉".repeat(4001))).is_err());
        assert!(serde_json::from_value::<MessageBody>(json!(" \n\t")).is_err());
        assert!(serde_json::from_value::<MessageBody>(json!(10)).is_err());
        assert!(serde_json::from_value::<MessageBody>(json!("hello\u{0}")).is_err());
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<FriendAction>(&["request", "accept", "remove"]);
    }
}
