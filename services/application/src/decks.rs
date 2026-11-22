//! Saved library content is distinct from format legality and match admission.

use crate::amount::Amount;
use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::Uuid;
pub mod import;
pub mod reads;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum PlayMode {
    Open,
    Collection,
}
impl TryFrom<String> for PlayMode {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "open" => Ok(Self::Open),
            "collection" => Ok(Self::Collection),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Named sections remain format-specific, with a bounded canonical wire spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct DeckSection(String);

impl DeckSection {
    /// Canonical section key captured with a selected revision.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for DeckSection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty()
            || text.len() > 32
            || !text.as_bytes()[0].is_ascii_lowercase()
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(de::Error::custom("invalid deck section"));
        }
        Ok(Self(text))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckEntry {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub definition_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub printing_id: Option<Uuid>,
    pub section: DeckSection,
    pub quantity: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDeck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub name: String,
    pub mode: PlayMode,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDeck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub deck_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub expected_revision_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub entries: Vec<DeckEntry>,
}

/// Mutable deck settings remain separate from immutable card-content revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckSettings {
    pub name: String,
    pub mode: PlayMode,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub appearance: crate::cosmetics::AppearanceSelection,
    /// The card in the deck shown on its box. Only a change may name one; the
    /// observed baseline never does, and omitting it keeps the current card.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::identifiers::present_uuid"
    )]
    pub face_card_id: Option<Uuid>,
}

impl DeckSettings {
    /// Validate settings shape; entitlement and format availability belong to SQL.
    ///
    /// # Errors
    /// Rejects invalid names, nil format IDs and invalid appearance IDs.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.format_id.is_nil()
            || !(1..=120).contains(&self.name.trim().chars().count())
            || self.name.contains('\0')
        {
            return Err("invalid deck settings");
        }
        self.appearance.validate()
    }
}

/// Full observed values prevent overwriting settings changed in another window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeDeckSettings {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub deck_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub expected: DeckSettings,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub settings: DeckSettings,
}

impl ChangeDeckSettings {
    /// Validate an immutable settings intent before submission or recovery.
    ///
    /// # Errors
    /// Returns an error for invalid identities, settings or a body over 4KiB.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil() || self.deck_id.is_nil() {
            return Err("invalid deck settings identity");
        }
        // The face card is a preference outside the compared baseline.
        if self.expected.face_card_id.is_some() {
            return Err("invalid deck settings");
        }
        self.expected.validate()?;
        self.settings.validate()?;
        if serde_json::to_vec(self)
            .map_err(|_| "invalid deck settings")?
            .len()
            > 4096
        {
            return Err("deck settings request is too large");
        }
        Ok(())
    }
}

/// Recovery submits the exact stored body, never the current unsaved form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DeckCommand {
    Create(CreateDeck),
    Save(SaveDeck),
    // Boxed: with its optional face card the settings change is far larger than the others.
    Settings(Box<ChangeDeckSettings>),
}
impl<'de> Deserialize<'de> for DeckCommand {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let wire: crate::provider_json::CommandObject = crate::provider_json::object(input)?;
        match wire.kind.as_str() {
            "create" => wire.payload().map(Self::Create),
            "save" => wire.payload().map(Self::Save),
            "settings" => wire
                .payload()
                .map(|command| Self::Settings(Box::new(command))),
            _ => Err(serde::de::Error::custom("invalid command kind")),
        }
    }
}

impl DeckCommand {
    /// Validate the original command before querying its bound receipt.
    ///
    /// # Errors
    /// Returns the same shape validation error as the original writer input.
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Create(input) => input.validate(),
            Self::Save(input) => input.validate(),
            Self::Settings(input) => input.validate(),
        }
    }
}

impl CreateDeck {
    pub fn validate(&self) -> Result<(), &'static str> {
        let length = self.name.trim().chars().count();
        if self.request_id.is_nil()
            || self.format_id.is_nil()
            || !(1..=120).contains(&length)
            || self.name.contains('\0')
        {
            return Err("deck name must contain 1 to 120 characters");
        }
        Ok(())
    }
}

impl SaveDeck {
    /// Validate shape-level bounds before the database rechecks references and ownership.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || self.deck_id.is_nil()
            || self.expected_revision_id.is_some_and(|id| id.is_nil())
        {
            return Err("invalid deck command identity");
        }
        validate_entries(&self.entries)
    }
}

fn validate_entries(entries: &[DeckEntry]) -> Result<(), &'static str> {
    if entries.len() > 1000
        || entries.iter().any(|entry| {
            entry.quantity.is_zero()
                || entry.definition_id.is_nil()
                || entry.printing_id.is_some_and(|id| id.is_nil())
        })
    {
        return Err("deck requires at most 1000 entries with positive quantities");
    }
    let mut identities = std::collections::HashSet::new();
    for entry in entries {
        if !identities.insert((
            entry.definition_id,
            entry.printing_id,
            entry.section.0.as_str(),
        )) {
            return Err("duplicate deck entry");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deck_input_uses_exact_positive_quantities_and_distinct_entries() {
        let entry = json!({"definitionId":Uuid::new_v4(),"printingId":null,"section":"main","quantity":"4"});
        let mut payload = json!({"requestId":Uuid::new_v4(),"deckId":Uuid::new_v4(),"expectedRevisionId":null,"entries":[entry.clone()]});
        assert!(
            serde_json::from_value::<SaveDeck>(payload.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        payload["entries"][0]["quantity"] = json!(4);
        assert!(serde_json::from_value::<SaveDeck>(payload.clone()).is_err());
        payload["entries"][0]["quantity"] = json!("0");
        assert!(
            serde_json::from_value::<SaveDeck>(payload.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        payload["entries"] = json!([entry.clone(), entry]);
        assert!(
            serde_json::from_value::<SaveDeck>(payload.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        payload["actorId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<SaveDeck>(payload).is_err());
        let absent_revision =
            json!({"requestId":Uuid::new_v4(),"deckId":Uuid::new_v4(),"entries":[]});
        assert!(serde_json::from_value::<SaveDeck>(absent_revision).is_err());
        assert!(
            serde_json::from_value::<DeckEntry>(
                json!({"definitionId":Uuid::new_v4(),"section":"main","quantity":"1"})
            )
            .is_err()
        );
    }

    #[test]
    fn full_sized_draft_fits_the_bounded_command_envelope() {
        let entries: Vec<_> = (0..1000)
            .map(|index| {
                json!({
                    "definitionId":Uuid::new_v4(),"printingId":Uuid::new_v4(),
                    "section":format!("main_{index:027}"),"quantity":"9".repeat(78)
                })
            })
            .collect();
        let payload = json!({"requestId":Uuid::new_v4(),"deckId":Uuid::new_v4(),"expectedRevisionId":null,"entries":entries});
        let draft: SaveDeck = serde_json::from_value(payload).unwrap();
        assert!(draft.validate().is_ok());
        let command = json!({"expectedRevision":null,"entries":draft.entries});
        // JSONB inserts spaces between keys/values and array items. Every entry
        // has four fields with ASCII-only canonical values, at most eight spaces.
        assert!(serde_json::to_vec(&command).unwrap().len() + 8003 <= 262144);
    }

    #[test]
    fn settings_recovery_requires_both_full_snapshots_and_explicit_nullable_appearance() {
        let settings = json!({"name":"Deck","mode":"collection","formatId":Uuid::new_v4(),"appearance":{"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null}});
        let payload = json!({"kind":"settings","command":{"requestId":Uuid::new_v4(),"deckId":Uuid::new_v4(),"expected":settings,"settings":settings}});
        let parsed: DeckCommand = serde_json::from_value(payload.clone()).unwrap();
        assert!(parsed.validate().is_ok());
        for field in ["expected", "settings"] {
            let mut missing = payload.clone();
            missing["command"][field]["appearance"]
                .as_object_mut()
                .unwrap()
                .remove("playmatId");
            assert!(serde_json::from_value::<DeckCommand>(missing).is_err());
            let mut nil = payload.clone();
            nil["command"][field]["appearance"]["backId"] = json!(Uuid::nil());
            assert!(serde_json::from_value::<DeckCommand>(nil).is_err());
        }
        let mut spoof = payload;
        spoof["command"]["settings"]["appearance"]["owned"] = json!(true);
        assert!(serde_json::from_value::<DeckCommand>(spoof).is_err());
    }

    #[test]
    fn a_face_card_may_only_be_named_by_the_change_and_never_as_null() {
        let settings = json!({"name":"Deck","mode":"collection","formatId":Uuid::new_v4(),"appearance":{"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null}});
        let face = Uuid::new_v4();
        let mut change = settings.clone();
        change["faceCardId"] = json!(face);
        let payload = json!({"kind":"settings","command":{"requestId":Uuid::new_v4(),"deckId":Uuid::new_v4(),"expected":settings,"settings":change}});
        let parsed: DeckCommand = serde_json::from_value(payload.clone()).unwrap();
        assert!(parsed.validate().is_ok());
        let DeckCommand::Settings(command) = &parsed else {
            panic!("expected a settings command");
        };
        assert_eq!(command.settings.face_card_id, Some(face));
        // The stored body round-trips exactly, so recovery terms stay bound.
        assert_eq!(serde_json::to_value(&parsed).unwrap(), payload);
        // Without a face card the original wire shape is unchanged.
        let plain = json!({"kind":"settings","command":{"requestId":Uuid::new_v4(),"deckId":Uuid::new_v4(),"expected":settings,"settings":settings}});
        let parsed_plain: DeckCommand = serde_json::from_value(plain.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed_plain).unwrap(), plain);
        let mut baseline = payload.clone();
        baseline["command"]["expected"]["faceCardId"] = json!(face);
        let parsed_baseline: DeckCommand = serde_json::from_value(baseline).unwrap();
        assert!(parsed_baseline.validate().is_err());
        let mut null = payload.clone();
        null["command"]["settings"]["faceCardId"] = json!(null);
        assert!(serde_json::from_value::<DeckCommand>(null).is_err());
        let mut nil = payload;
        nil["command"]["settings"]["faceCardId"] = json!(Uuid::nil());
        assert!(serde_json::from_value::<DeckCommand>(nil).is_err());
    }

    #[test]
    fn section_is_canonical_without_claiming_format_legality() {
        for section in ["main", "sideboard", "commander", "companion"] {
            assert!(serde_json::from_value::<DeckSection>(json!(section)).is_ok());
        }
        for section in ["", "Main", "main deck", "main/side", "_main"] {
            assert!(serde_json::from_value::<DeckSection>(json!(section)).is_err());
        }
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<PlayMode>(&["open", "collection"]);
    }
}
