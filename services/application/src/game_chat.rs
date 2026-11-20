//! Human game chat reuses message history without applying direct-contact eligibility.
use crate::{
    account::PublicAccountIdentity,
    commands::MessageBody,
    social::{DirectMessage, MessageCursor, validate_message_page},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Original submitted text and replay identity; the match comes from the path.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendGameMessage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub body: MessageBody,
}
/// One consistent participant roster, policy hint and immutable message page.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameChatPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub conversation_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub participants: Vec<PublicAccountIdentity>,
    pub can_send: bool,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<DirectMessage>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub next_cursor: Option<MessageCursor>,
}
impl GameChatPage {
    /// Bind the response to the caller, match and exact descending request cursor.
    /// # Errors
    /// Rejects foreign/malformed rosters, author labels, identifiers or paging metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        match_id: Uuid,
        before: Option<&MessageCursor>,
        limit: usize,
    ) -> Result<(), &'static str> {
        validate_message_page(&self.items, self.next_cursor.as_ref(), before, limit)?;
        if owner.is_nil()
            || self.account_id != owner
            || match_id.is_nil()
            || self.match_id != match_id
            || self.conversation_id.is_nil()
            || !(1..=6).contains(&self.participants.len())
            || !self.participants.iter().any(|p| p.id == owner)
            || self.participants.iter().any(|p| p.validate().is_err())
            || self
                .participants
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || self.items.iter().any(|m| {
                !self
                    .participants
                    .iter()
                    .any(|p| p.id == m.sender_id && p.username == m.author)
            })
        {
            return Err("invalid game chat context");
        }
        Ok(())
    }
}
