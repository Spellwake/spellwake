//! Safe owner-facing account and saved-library projections. These are not match admission.
use crate::{actor::ActorState, amount::Amount, cosmetics::AppearanceSelection, decks::PlayMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) fn valid_username(value: &str) -> bool {
    (3..=24).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Minimal historical/public identity, never the reserved name of a deleted player.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicAccountIdentity {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub username: String,
    pub is_deleted: bool,
}
impl PublicAccountIdentity {
    /// Check the existing username grammar and permanent anonymous representation.
    ///
    /// # Errors
    /// Rejects nil principals, malformed live names or an unmasked deleted identity.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil()
            || if self.is_deleted {
                self.username != "Deleted player"
            } else {
                !valid_username(&self.username)
            }
        {
            return Err("invalid public account identity");
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Wallet {
    pub owned: Amount,
    pub held: Amount,
    pub available: Amount,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    pub username: String,
    pub state: ActorState,
    pub appearance_revision: Amount,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub defaults: AppearanceSelection,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub wallet: Wallet,
}
fn valid_appearance(selection: &AppearanceSelection) -> bool {
    !selection.back_id.is_nil()
        && !selection.sleeve_id.is_nil()
        && !selection.dice_id.is_nil()
        && selection.playmat_id.is_none_or(|id| !id.is_nil())
}
impl AccountSummary {
    /// Check private-row identity, lifecycle and exact held/available arithmetic.
    ///
    /// # Errors
    /// Returns a static error for a malformed owner projection.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.account_id.is_nil()
            || !matches!(self.state, ActorState::Active | ActorState::DeletionPending)
            || !valid_username(&self.username)
            || !valid_appearance(&self.defaults)
            || self
                .wallet
                .owned
                .checked_sub(&self.wallet.held)
                .ok()
                .as_ref()
                != Some(&self.wallet.available)
        {
            return Err("invalid account projection");
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub key: String,
    pub name: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub name: String,
    pub mode: PlayMode,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub current_revision_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub format: FormatSummary,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub appearance: AppearanceSelection,
    /// The card shown on the deck box, or null for a deck without cards.
    #[serde(default, deserialize_with = "crate::provider_json::optional_object")]
    pub face_card: Option<DeckFaceCard>,
}
/// A deck's face card and its official art, where the catalogue has it.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckFaceCard {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub definition_id: Uuid,
    pub art_url: Option<String>,
}
impl DeckFaceCard {
    fn is_valid(&self) -> bool {
        !self.definition_id.is_nil()
            && self.art_url.as_deref().is_none_or(|url| {
                url.starts_with("https://")
                    && url.len() <= 2048
                    && !url.contains(char::is_whitespace)
            })
    }
}
impl DeckSummary {
    /// Validate saved metadata without asserting legality or match admission.
    ///
    /// # Errors
    /// Returns a static error for invalid identities, text or cosmetic slots.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil()
            || !(1..=120).contains(&self.name.chars().count())
            || self.name.trim_matches(' ') != self.name
            || self.current_revision_id.is_some_and(|id| id.is_nil())
            || self.format.id.is_nil()
            || self.format.key.is_empty()
            || self.format.name.is_empty()
            || !valid_appearance(&self.appearance)
            || self.face_card.as_ref().is_some_and(|face| !face.is_valid())
        {
            return Err("invalid deck metadata projection");
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckLibraryPage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<DeckSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl DeckLibraryPage {
    /// Validate page ordering against the requested limit and cursor.
    ///
    /// # Errors
    /// Returns a static error for invalid metadata, identities or pagination.
    pub fn validate_after(&self, limit: usize, after: Option<Uuid>) -> Result<(), &'static str> {
        if !(1..=100).contains(&limit)
            || self.items.len() > limit
            || self.items.windows(2).any(|pair| pair[0].id >= pair[1].id)
            || self.items.iter().any(|item| {
                item.validate().is_err() || after.is_some_and(|cursor| item.id <= cursor)
            })
            || self.next_cursor
                != if self.items.len() == limit {
                    self.items.last().map(|item| item.id)
                } else {
                    None
                }
        {
            return Err("invalid deck library projection");
        }
        Ok(())
    }
}
