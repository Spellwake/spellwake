//! Minimal social projections, independent of preview presence and private Auth data.
use crate::account::valid_username as username;
use crate::amount::Amount;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Bound history pages under the1MiB RPC envelope, including escaped Unicode bodies.
pub const DIRECT_MESSAGE_PAGE_LIMIT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum Relationship {
    None,
    Friend,
    IncomingRequest,
    OutgoingRequest,
}
impl TryFrom<String> for Relationship {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "none" => Ok(Self::None),
            "friend" => Ok(Self::Friend),
            "incoming_request" => Ok(Self::IncomingRequest),
            "outgoing_request" => Ok(Self::OutgoingRequest),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SocialPerson {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub username: String,
    pub is_deleted: bool,
    pub relationship: Relationship,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub friendship_id: Option<Uuid>,
    pub is_blocked: bool,
    pub can_contact: bool,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub conversation_id: Option<Uuid>,
    pub unread_messages: Amount,
    pub supporter: bool,
}

impl SocialPerson {
    /// Validate public identity and the internal consistency of private relationship facts.
    ///
    /// # Errors
    /// Rejects missing identities, unmasked deleted identity or inconsistent relationships.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil()
            || self.friendship_id.is_some_and(|id| id.is_nil())
            || self.conversation_id.is_some_and(|id| id.is_nil())
            || (self.relationship == Relationship::None) != self.friendship_id.is_none()
            || (self.is_blocked && self.can_contact)
            || (self.conversation_id.is_none() && !self.unread_messages.is_zero())
            || (self.is_deleted
                && (self.username != "Deleted player"
                    || self.relationship != Relationship::None
                    || self.can_contact
                    || self.supporter))
            || (!self.is_deleted && !username(&self.username))
        {
            return Err("invalid social person projection");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SocialPeoplePage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<SocialPerson>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}

/// Current selected identity and the full caller unread count, independent of loaded pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SocialStatus {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    pub unread_messages: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub person_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub person: Option<SocialPerson>,
}

impl SocialStatus {
    /// Check the requested identity, anonymity and consistency of the full unread count.
    ///
    /// # Errors
    /// Rejects nil/foreign identities, a person without a matching request or inconsistent counts.
    pub fn validate(&self, requested: Option<Uuid>) -> Result<(), &'static str> {
        if self.account_id.is_nil()
            || requested.is_some_and(|id| id.is_nil())
            || self.person_id != requested
        {
            return Err("invalid social status projection");
        }
        if let Some(person) = &self.person {
            person.validate()?;
            if Some(person.id) != requested
                || person.id == self.account_id
                || person.unread_messages > self.unread_messages
            {
                return Err("invalid social status projection");
            }
        }
        Ok(())
    }
}

impl SocialPeoplePage {
    /// Check exact UUID paging and, for directory searches, the literal current-name filter.
    ///
    /// # Errors
    /// Rejects malformed, self-containing, stale-cursor or query-inconsistent pages.
    pub fn validate(
        &self,
        query: Option<&str>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if self.account_id.is_nil()
            || !(1..=100).contains(&limit)
            || self.items.len() > limit
            || self.items.windows(2).any(|pair| pair[0].id >= pair[1].id)
            || self.items.iter().any(|person| {
                person.validate().is_err()
                    || person.id == self.account_id
                    || after.is_some_and(|id| person.id <= id)
                    || query.is_some_and(|query| {
                        person.is_deleted
                            || !person
                                .username
                                .to_lowercase()
                                .contains(&query.trim_matches(' ').to_ascii_lowercase())
                    })
            })
            || self.next_cursor
                != if self.items.len() == limit {
                    self.items.last().map(|person| person.id)
                } else {
                    None
                }
        {
            return Err("invalid social people page");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageCursor {
    pub sent_at: DateTime<Utc>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectMessage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub sender_id: Uuid,
    pub author: String,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

impl DirectMessage {
    fn cursor(&self) -> MessageCursor {
        MessageCursor {
            sent_at: self.sent_at,
            id: self.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectMessagePage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub conversation_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub person: SocialPerson,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<DirectMessage>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub next_cursor: Option<MessageCursor>,
}

impl DirectMessagePage {
    /// Validate immutable descending history, exact conversation and authorized sender domain.
    ///
    /// # Errors
    /// Rejects foreign senders, stale/tied cursor replays, duplicate messages and bad labels.
    pub fn validate(
        &self,
        conversation: Uuid,
        before: Option<&MessageCursor>,
        limit: usize,
    ) -> Result<(), &'static str> {
        self.person.validate()?;
        validate_message_page(&self.items, self.next_cursor.as_ref(), before, limit)?;
        if self.account_id.is_nil()
            || self.account_id == self.person.id
            || self.conversation_id != conversation
            || conversation.is_nil()
            || self.person.conversation_id != Some(conversation)
            || self.items.iter().any(|message| {
                if message.sender_id == self.person.id {
                    message.author != self.person.username
                } else {
                    message.sender_id != self.account_id || !username(&message.author)
                }
            })
        {
            return Err("invalid direct message page");
        }
        Ok(())
    }
}

/// Shared immutable descending history checks for direct and admitted game chat.
pub(crate) fn validate_message_page(
    items: &[DirectMessage],
    cursor: Option<&MessageCursor>,
    before: Option<&MessageCursor>,
    limit: usize,
) -> Result<(), &'static str> {
    let mut ids = BTreeSet::new();
    if !(1..=DIRECT_MESSAGE_PAGE_LIMIT).contains(&limit)
        || items.len() > limit
        || before.is_some_and(|value| value.id.is_nil())
        || items.iter().any(|message| {
            message.id.is_nil()
                || !ids.insert(message.id)
                || message.sender_id.is_nil()
                || message.body.trim().is_empty()
                || message.body.contains('\0')
                || message.body.chars().count() > 4000
                || before
                    .is_some_and(|value| (message.sent_at, message.id) >= (value.sent_at, value.id))
        })
        || items
            .windows(2)
            .any(|pair| (pair[0].sent_at, pair[0].id) <= (pair[1].sent_at, pair[1].id))
        || cursor.cloned()
            != if items.len() == limit {
                items.last().map(DirectMessage::cursor)
            } else {
                None
            }
    {
        return Err("invalid message page");
    }
    Ok(())
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<Relationship>(&[
            "none",
            "friend",
            "incoming_request",
            "outgoing_request",
        ]);
    }
}
