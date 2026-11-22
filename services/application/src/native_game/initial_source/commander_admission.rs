//! Signed admission for one explicitly published Commander release and profile.
use super::*;
use crate::{
    actor::ActorError,
    decks::PlayMode,
    match_history::ControllerKind,
    native_game::preparation_source::{NativeRoomSource, PreparationEntry, PreparationRelease},
    rooms::configuration::MatchStructure,
};
use num_traits::ToPrimitive;
use serde_json::{Value, json};
use spellwake_card_programs::commander::prepare_deck;
use std::{collections::BTreeMap, path::Path};

pub(in crate::native_game) const READY_PROOF: &str = "native-constructed-ready-v1/commander/";
pub(in crate::native_game) const COUNTDOWN_PROOF: &str =
    "native-constructed-ffa-candidate-v1/commander/";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CardApproval {
    definition_revision_id: Uuid,
    printing_id: Uuid,
    program: EvidenceHash,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommanderRelease {
    schema: String,
    pub(crate) release_id: Uuid,
    pub(crate) legality_revision_id: Uuid,
    pub(crate) format_id: Uuid,
    profile_id: Uuid,
    profile_qualification: EvidenceHash,
    cards: BTreeMap<Uuid, CardApproval>,
    #[serde(skip)]
    pin: Option<EvidenceHash>,
}

impl CommanderRelease {
    pub(crate) fn from_env() -> Result<Option<Self>, ActorError> {
        let (Some(file), Some(pin)) = (
            std::env::var_os("SPELLWAKE_COMMANDER_RELEASE_MANIFEST"),
            std::env::var_os("SPELLWAKE_COMMANDER_RELEASE_MANIFEST_SHA256"),
        ) else {
            if std::env::var_os("SPELLWAKE_COMMANDER_RELEASE_MANIFEST").is_some()
                || std::env::var_os("SPELLWAKE_COMMANDER_RELEASE_MANIFEST_SHA256").is_some()
            {
                return Err(ActorError::Configuration);
            }
            return Ok(None);
        };
        let bytes = crate::catalogue_source::read_bounded(Path::new(&file), 16 * 1024 * 1024)
            .map_err(|_| ActorError::Configuration)?;
        Self::verified(&bytes, pin.to_str().ok_or(ActorError::Configuration)?).map(Some)
    }

    fn verified(bytes: &[u8], expected: &str) -> Result<Self, ActorError> {
        let pin = digest(bytes);
        if pin.as_str() != expected {
            return Err(ActorError::Configuration);
        }
        let mut release: Self =
            crate::decode_wire_object(bytes).map_err(|_| ActorError::Configuration)?;
        if release.schema != "spellwake-accepted-commander-v1"
            || [
                release.release_id,
                release.legality_revision_id,
                release.format_id,
                release.profile_id,
            ]
            .iter()
            .any(Uuid::is_nil)
            || !(1..=50_000).contains(&release.cards.len())
            || release.cards.iter().any(|(oracle, card)| {
                oracle.is_nil() || card.definition_revision_id.is_nil() || card.printing_id.is_nil()
            })
        {
            return Err(ActorError::Configuration);
        }
        release.pin = Some(pin);
        Ok(release)
    }

    pub(in crate::native_game) fn binds(
        &self,
        source: &NativeProgramSource,
        release: &PreparationRelease,
        format: Uuid,
        profile: &crate::native_game::preparation_source::PreparationProfile,
        legality: Uuid,
    ) -> bool {
        release.id == self.release_id
            && format == self.format_id
            && profile.id == self.profile_id
            && profile.qualification_ref.as_str() == self.profile_qualification.as_str()
            && profile.planar_pool_ref.is_none()
            && profile.seat_capacity <= crate::amount::Amount::from(3)
            && legality == self.legality_revision_id
            && self
                .pin
                .as_ref()
                .is_some_and(|pin| release.evidence_ref.as_str() == pin.as_str())
            && release.kernel_ref.as_str() == format!("sha256:{}", native_runtime_identity())
            && release.rules_ref.as_str() == hash_text(source.vocabulary.rules_sha256()).as_str()
            && release.program_manifest_ref.as_str() == source.identity().as_str()
    }

    pub(in crate::native_game) fn validate_deck(
        &self,
        source: &NativeProgramSource,
        entries: &[PreparationEntry],
    ) -> bool {
        let Some(inventory) = &source.commander_inventory else {
            return false;
        };
        let mut main = Vec::new();
        let mut commanders = Vec::new();
        let mut count = 0;
        for entry in entries {
            let Ok(oracle) = OracleId::try_from(entry.oracle_ref.as_str().to_owned()) else {
                return false;
            };
            if !Uuid::parse_str(oracle.as_str())
                .ok()
                .and_then(|id| self.cards.get(&id))
                .is_some_and(|card| {
                    card.definition_revision_id == entry.definition_revision_id
                        && entry
                            .printing
                            .as_ref()
                            .is_some_and(|printing| printing.id == card.printing_id)
                        && card.program.as_str() == entry.program_ref.as_str()
                        && source.registry.resolve(&oracle).is_some()
                })
            {
                return false;
            }
            let Some(quantity) = entry
                .quantity
                .integer()
                .to_usize()
                .filter(|n| (1..=100).contains(n))
            else {
                return false;
            };
            count += quantity;
            if count > 100 {
                return false;
            }
            match entry.section.as_str() {
                "main" => main.extend(std::iter::repeat_n(oracle, quantity)),
                "commander" if quantity == 1 => commanders.push(oracle),
                _ => return false,
            }
        }
        prepare_deck(
            inventory,
            &source.names,
            &commanders,
            &main,
            &BTreeMap::new(),
        )
        .is_ok()
    }

    pub(in crate::native_game) fn prepare_room(
        &self,
        source: &NativeProgramSource,
        observation: &NativeRoomSource,
        actor: Uuid,
        countdown: bool,
    ) -> Option<Value> {
        let terms = observation.terms();
        let room = &terms.room;
        if !self.binds(
            source,
            &terms.release,
            room.format_id,
            &terms.profile,
            terms.legality_revision_id,
        ) || room.mode != PlayMode::Open
            || room.competitive
            || room.structure != MatchStructure::Bo1
            || terms.slots.iter().any(|slot| {
                slot.team_ref.is_some() || slot.controller_kind != ControllerKind::Human
            })
        {
            return None;
        }
        let actor_slot = terms
            .slots
            .iter()
            .find(|slot| slot.account_id == Some(actor))?;
        let selected = actor_slot.selected_revision_id?;
        let stamp = digest(observation.source_hash().as_bytes());
        let qualified_roster = || {
            terms.slots.iter().all(|slot| {
                slot.entries
                    .as_ref()
                    .is_some_and(|entries| self.validate_deck(source, entries))
            })
        };
        let mut prepared = json!({
            "sourceHash":observation.source_hash(), "releaseId":self.release_id,
            "legalityRevisionId":self.legality_revision_id
        });
        if countdown {
            if !qualified_roster() {
                return None;
            }
            prepared["version"] = json!("native-countdown-prepared-v1");
            prepared["qualificationRef"] = json!(format!("{COUNTDOWN_PROOF}{}", stamp.as_str()));
            return Some(prepared);
        }
        prepared["selectedRevisionId"] = json!(selected);
        let entries = actor_slot.entries.as_ref()?;
        if !self.validate_deck(source, entries) {
            prepared["version"] = json!("native-ready-rejected-v1");
            prepared["qualificationRef"] = json!("native-ready-deck-rejected-v1");
            return Some(prepared);
        }
        prepared["version"] = json!("native-ready-prepared-v1");
        prepared["qualificationRef"] = json!(format!("{READY_PROOF}{}", stamp.as_str()));
        if !actor_slot.ready
            && (room.state == "countdown"
                || terms
                    .slots
                    .iter()
                    .all(|slot| slot.ready || slot.slot_id == actor_slot.slot_id))
            && qualified_roster()
        {
            prepared["countdownQualificationRef"] =
                json!(format!("{COUNTDOWN_PROOF}{}", stamp.as_str()));
        }
        Some(prepared)
    }
}
