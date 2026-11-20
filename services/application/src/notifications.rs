//! Recipient-owned routing metadata; source references never confer action authority.
use crate::{amount::Amount, progression::validate_page};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;
/// Preserve the original ordered exact-ID set in command receipts.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadNotifications {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub notification_ids: Vec<Uuid>,
}
impl ReadNotifications {
    /// # Errors
    /// Rejects nil, duplicate, empty or oversized acknowledgment sets.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || !(1..=100).contains(&self.notification_ids.len())
            || self.notification_ids.iter().any(Uuid::is_nil)
            || self.notification_ids.iter().collect::<BTreeSet<_>>().len()
                != self.notification_ids.len()
        {
            return Err("invalid notification set");
        }
        Ok(())
    }
}
/// Existing producer text and source references, with the first acknowledgment time.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notification {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub source_kind: String,
    pub source_ref: String,
    pub body: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub read_at: Option<DateTime<Utc>>,
}
/// The unread count covers all own records, independently of this UUID page.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    pub unread_count: Amount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<Notification>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl NotificationPage {
    /// # Errors
    /// Rejects foreign account context, bad keyset metadata or inconsistent unread counts.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        validate_page(
            &self.items.iter().map(|n| n.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        if owner.is_nil()
            || self.account_id != owner
            || Amount::from(self.items.iter().filter(|n| n.read_at.is_none()).count() as u64)
                > self.unread_count
            || self.items.iter().any(|n| {
                [&n.source_kind, &n.source_ref, &n.body]
                    .iter()
                    .any(|s| s.contains('\0'))
            })
        {
            return Err("invalid notification context");
        }
        Ok(())
    }
}
