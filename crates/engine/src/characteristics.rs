//! Exact-world evaluation for admitted copy, control, text, type, color, ability
//! and PT buckets, including finite defining abilities in all physical zones.
//!
//! Canonical state owns physical counters, resolved records and one timestamp
//! clock. Evaluation, generated-operation probes and ordering use only scratch
//! values borrowed from the caller's actual physical/definition/state cut.
mod colors;
mod evaluation;
mod inheritance;
pub use inheritance::{ActivationOriginPath, ActivationTerms, InheritedAbilities};
#[cfg(test)]
#[path = "characteristics/tests.rs"]
pub(crate) mod frozen_tests;
mod ordering;
#[cfg(test)]
pub(crate) mod private_qualification;
mod static_rules;
mod text;
pub(crate) use text::rewrite_program as rewrite_program_creature_types;
mod types;
mod world;
mod world_eval;

pub use colors::{Color, ColorFilter, ColorOperation, StaticColorOperand};
pub(crate) use evaluation::{count_permanents, evaluate, selector_matches_permanent};
pub use static_rules::*;
pub use types::*;
pub(crate) use world_eval::{
    WorldEvalCtx, compute_party_size, eval_world_condition, eval_world_value_expr,
    matches_world_object_filter,
};

use crate::priority::SeatId;
pub use crate::program::CardType;
use crate::{
    object_identity::{CardId, GameObjects, ObjectId, ObjectZone},
    stack_window::StackEntryId,
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

/// Unlocked door state for a Duskmourn Room enchantment on the battlefield (CR 709.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoomDoors {
    pub left: bool,
    pub right: bool,
}

impl RoomDoors {
    #[must_use]
    pub const fn is_unlocked(self, half: crate::program::SplitHalf) -> bool {
        match half {
            crate::program::SplitHalf::Left => self.left,
            crate::program::SplitHalf::Right => self.right,
        }
    }

    #[must_use]
    pub const fn fully_unlocked(self) -> bool {
        self.left && self.right
    }
}

/// Canonical constructed dungeons (CR 309).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DungeonId {
    LostMineOfPhandelver,
    DungeonOfTheMadMage,
    TombOfAnnihilation,
    Undercity,
}

/// Per-player dungeon venture progress (CR 309, 701.46).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DungeonProgress {
    pub active: Option<(DungeonId, u8)>,
    pub completed: u16,
}

/// Nested canonical state under EffectWindow, never a final-stat cache.
/// Compact per-spell turn history record for filtered `CastSpellThisTurn` queries (e.g. Veil of Summer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CastSpellTurnRecord {
    pub(crate) types: crate::program::CardTypeMask,
    pub(crate) colors: crate::program::ColorMask,
    pub(crate) supertypes: crate::program::SupertypeMask,
    pub(crate) subtypes: BTreeSet<spellwake_contracts::creature_types::CreatureTypeId>,
    pub(crate) from_hand: bool,
    pub(crate) from_graveyard: bool,
    pub(crate) from_exile: bool,
}

/// Compact per-event turn history record for filtered graveyard-arrival and sacrifice queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZoneMoveTurnRecord {
    pub(crate) owner: SeatId,
    pub(crate) previous_controller: Option<SeatId>,
    pub(crate) from_battlefield: bool,
    pub(crate) types: crate::program::CardTypeMask,
    pub(crate) colors: crate::program::ColorMask,
    pub(crate) supertypes: crate::program::SupertypeMask,
    pub(crate) subtypes: BTreeSet<spellwake_contracts::creature_types::CreatureTypeId>,
    pub(crate) artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CharacteristicState {
    /// Physical commander designations used by rules that inspect the evaluated battlefield.
    pub(crate) commander_cards: BTreeSet<CardId>,
    pub(crate) entry_copies: BTreeMap<ObjectId, std::sync::Arc<crate::program::Program>>,
    /// Component additions that postdate an individual copy effect, for this permanent only.
    mutated_copies: BTreeMap<
        (ObjectId, ContinuousEffectId),
        (crate::program::Program, crate::program::Program),
    >,
    mutated_entry_faces: BTreeMap<ObjectId, crate::program::Program>,
    pub(crate) commander_chosen_colors: BTreeMap<CardId, Color>,
    pub(crate) exhausted: BTreeSet<(
        ObjectId,
        Option<CharacteristicTimestamp>,
        crate::activated::ActivatedAbilityId,
        ActivationOriginPath,
    )>,
    pub(crate) attachments: BTreeMap<ObjectId, crate::targets::TargetRef>,
    pub(crate) paired_creatures: BTreeMap<ObjectId, ObjectId>,
    pub(crate) suspend_haste:
        BTreeMap<ObjectId, (crate::priority::SeatId, Option<ContinuousEffectId>)>,
    pub(crate) foretold: BTreeMap<ObjectId, u64>,
    pub(crate) saddled: BTreeSet<ObjectId>,
    pub(crate) upkeep_cuts:
        Option<BTreeMap<crate::priority::SeatId, crate::object_identity::ControlEpoch>>,
    pub(crate) speed: BTreeMap<crate::priority::SeatId, u8>,
    pub(crate) city_blessing: BTreeSet<crate::priority::SeatId>,
    pub(crate) player_color_protection: BTreeMap<crate::priority::SeatId, BTreeSet<Color>>,
    pub(crate) experience: BTreeMap<crate::priority::SeatId, RulesInteger>,
    /// Generic non-poison player counters (such as Energy) keyed by counter kind and seat.
    pub(crate) player_counters:
        BTreeMap<crate::effects::CounterKind, BTreeMap<crate::priority::SeatId, RulesInteger>>,
    /// Game-wide monarch designation, independent of the permanent that created it.
    pub(crate) monarch: Option<crate::priority::SeatId>,
    pub(crate) lore: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) life_lost_this_turn: BTreeMap<SeatId, RulesInteger>,
    pub(crate) life_gained_this_turn: BTreeMap<SeatId, RulesInteger>,
    pub(crate) spells_cast_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) attacked_this_turn_players: BTreeSet<SeatId>,
    pub(crate) creatures_entered_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) permanents_left_battlefield_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) cards_discarded_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) permanents_sacrificed_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) cards_drawn_this_turn_counts: BTreeMap<SeatId, u16>,
    pub(crate) descended_this_turn: BTreeSet<SeatId>,
    pub(crate) committed_crime_this_turn: BTreeSet<SeatId>,
    pub(crate) entered_types_this_turn: BTreeMap<SeatId, BTreeMap<CardType, u16>>,
    pub(crate) permanents_entered_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) cast_spell_types_this_turn: BTreeMap<SeatId, BTreeMap<CardType, u16>>,
    pub(crate) noncreature_spells_cast_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) spells_cast_from_hand_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) lands_played_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) cards_left_graveyard_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) nonland_permanents_left_battlefield_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) any_spell_warped_this_turn: bool,
    pub(crate) creatures_died_this_turn_count: u16,
    pub(crate) controlled_creatures_died_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) tokens_created_this_turn: BTreeMap<SeatId, u16>,
    pub(crate) attacking_creatures_this_turn: BTreeMap<SeatId, BTreeSet<ObjectId>>,
    pub(crate) attacked_objects_this_turn: BTreeSet<ObjectId>,
    pub(crate) attacked_records_this_turn: BTreeMap<SeatId, Vec<ZoneMoveTurnRecord>>,
    pub(crate) blocked_objects_this_turn: BTreeSet<ObjectId>,
    pub(crate) entered_objects_this_turn: BTreeSet<ObjectId>,
    /// Number of combat phases that have begun this turn (1 during the first combat phase).
    pub(crate) combat_phases_this_turn: u8,
    /// Number of cards put into each player's graveyard from anywhere this turn.
    pub(crate) cards_entered_graveyard_this_turn: BTreeMap<SeatId, u16>,
    /// Number of counters placed on creatures by each player this turn.
    pub(crate) counters_placed_on_creatures_this_turn: BTreeMap<SeatId, u16>,
    /// Number of lands that entered the battlefield under each player's control this turn.
    pub(crate) lands_entered_this_turn: BTreeMap<SeatId, u16>,
    /// Number of dice rolled by each player this turn.
    pub(crate) dice_rolled_this_turn: BTreeMap<SeatId, u16>,
    /// Total mana value of spells cast by each player this turn.
    pub(crate) other_spells_mv_this_turn: BTreeMap<SeatId, u32>,
    /// Battlefield permanents that entered directly from a graveyard.
    pub(crate) entered_from_graveyard: BTreeSet<ObjectId>,
    /// Battlefield permanents cast via an alternative or conditional keyword cost.
    pub(crate) paid_alternative_objects: BTreeSet<ObjectId>,
    /// Graveyard object incarnations that died (moved battlefield -> graveyard) this turn.
    pub(crate) died_this_turn: BTreeSet<ObjectId>,
    /// Graveyard object incarnations put into a graveyard from anywhere this turn.
    pub(crate) put_into_gy_this_turn: BTreeSet<ObjectId>,
    /// Graveyard object incarnations put into a graveyard from a library this turn.
    pub(crate) put_into_gy_from_library_this_turn: BTreeSet<ObjectId>,
    /// Card names chosen via `PromptDomain::CardName` linked to a source permanent.
    pub(crate) named_cards: BTreeMap<ObjectId, String>,
    pub(crate) activated_sources_this_turn: BTreeMap<ObjectId, u16>,
    pub(crate) ability_resolutions_by_source_this_turn: BTreeMap<ObjectId, u16>,
    pub(crate) players_dealt_damage_this_turn: BTreeSet<SeatId>,
    pub(crate) players_dealt_noncombat_damage_this_turn: BTreeSet<SeatId>,
    pub(crate) object_damage_taken_this_turn: BTreeMap<ObjectId, RulesInteger>,
    /// Filtered spell-cast history per seat this turn (cleared at turn start).
    pub(crate) cast_spells_this_turn: BTreeMap<SeatId, Vec<CastSpellTurnRecord>>,
    /// Players who activated an ability of a card in a graveyard this turn.
    pub(crate) activated_graveyard_ability_this_turn: BTreeSet<SeatId>,
    /// Life lost by each seat during the previous turn (rotated on turn start).
    pub(crate) life_lost_last_turn: BTreeMap<SeatId, RulesInteger>,
    /// Number of spells cast by each seat during the previous turn (rotated on turn start).
    pub(crate) spells_cast_last_turn: BTreeMap<SeatId, RulesInteger>,
    /// Total cards put into exile this turn.
    pub(crate) cards_exiled_this_turn: u32,
    /// Filtered graveyard-arrival records this turn.
    pub(crate) cards_put_into_graveyard_this_turn: Vec<ZoneMoveTurnRecord>,
    /// Filtered sacrifice records per player this turn.
    pub(crate) sacrificed_this_turn: BTreeMap<SeatId, Vec<ZoneMoveTurnRecord>>,
    /// Whether any definition in this game queries per-source damage-dealt turn provenance.
    pub(crate) track_damage_dealt_by: bool,
    /// Total damage dealt by each source object this turn.
    pub(crate) damage_dealt_this_turn_by: BTreeMap<ObjectId, RulesInteger>,
    /// Opponents dealt damage by each source object this turn.
    pub(crate) damage_dealt_to_opponents_this_turn: BTreeMap<ObjectId, BTreeSet<SeatId>>,
    /// Source objects that dealt combat damage to a player this turn.
    pub(crate) combat_damage_dealt_to_player_this_turn: BTreeSet<ObjectId>,
    /// Permanent objects that received excess damage this turn.
    pub(crate) excess_damage_received_this_turn: BTreeSet<ObjectId>,
    /// Origin zone each battlefield permanent entered from.
    pub(crate) entered_from_zone: BTreeMap<ObjectId, crate::program::SourceZoneFilter>,
    /// Battlefield permanents cast by their current controller.
    pub(crate) cast_by_controller_objects: BTreeSet<ObjectId>,
    /// Battlefield permanents cast from hand.
    pub(crate) cast_from_hand_objects: BTreeSet<ObjectId>,
    /// Actual damage pairs retained until cleanup when a definition uses that history.
    pub(crate) creature_damage_pairs: Option<BTreeSet<(ObjectId, ObjectId)>>,
    pub(crate) vow_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) vow_protected_players: BTreeMap<ObjectId, BTreeSet<SeatId>>,
    pub(crate) corpse_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) blessing_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) despair_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) hatchling_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) named_counters:
        BTreeMap<crate::effects::CounterKind, BTreeMap<ObjectId, RulesInteger>>,
    pub(crate) imprinted_cards: BTreeMap<ObjectId, Vec<ObjectId>>,
    pub(crate) ability_donors_in_exile: BTreeMap<ObjectId, Vec<ObjectId>>,
    pub(crate) linked_exiles: BTreeMap<ObjectId, Vec<ObjectId>>,
    pub(crate) chosen_entry_players: BTreeMap<ObjectId, [crate::priority::SeatId; 2]>,
    pub(crate) entry_color_choices: BTreeMap<ObjectId, Color>,
    pub(crate) entry_option_choices: BTreeMap<ObjectId, u8>,
    pub(crate) entry_player_choices: BTreeMap<ObjectId, SeatId>,
    pub(crate) indestructible_counters: BTreeMap<ObjectId, RulesInteger>,
    indestructible_grants: BTreeMap<ObjectId, ContinuousEffectId>,
    keyword_counter_grants: BTreeMap<(ObjectId, crate::effects::CounterKind), ContinuousEffectId>,
    pub(crate) page_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) quest_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) verse_counters: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) suspect_counters: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) charge: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) storage: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) defense: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) battle_protectors: BTreeMap<ObjectId, SeatId>,
    pub(crate) loyalty: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) loyalty_used: BTreeMap<ObjectId, RulesInteger>,
    pub(crate) loyalty_activators: BTreeSet<SeatId>,
    pub(crate) extra_loyalty_activations: BTreeMap<SeatId, RulesInteger>,
    pub(crate) extra_loyalty_used: BTreeMap<(ObjectId, SeatId), RulesInteger>,
    pub(crate) emblems: BTreeMap<crate::emblem::EmblemId, crate::emblem::Emblem>,
    pub(crate) no_max_hand_size_players: BTreeSet<SeatId>,
    next_emblem: u64,
    pub(crate) time_counters: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) fade: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) age: BTreeMap<ObjectId, spellwake_contracts::RulesInteger>,
    pub(crate) renowned: BTreeSet<ObjectId>,
    /// Current Class level for Class enchantments on the battlefield (absent defaults to level 1, CR 716).
    pub(crate) class_levels: BTreeMap<ObjectId, u8>,
    /// Cases on the battlefield that have been solved (CR 719).
    pub(crate) solved_cases: BTreeSet<ObjectId>,
    /// Battlefield permanents currently in the prepared state.
    pub(crate) prepared: BTreeSet<ObjectId>,
    pub(crate) end_step_casts: BTreeMap<ObjectId, crate::program::EndStepCast>,
    /// Cast provenance and mana-spent summary carried from spell resolution onto the battlefield permanent (CR 400.7a).
    pub(crate) cast_summaries: BTreeMap<ObjectId, crate::mana::CastManaSummary>,
    /// Ring temptation level (0..=4) per player (CR 701.52).
    pub(crate) ring_level: BTreeMap<SeatId, u8>,
    /// Current Ring-bearer creature per player (CR 701.52).
    pub(crate) ring_bearer: BTreeMap<SeatId, ObjectId>,
    /// Game-wide initiative designation (CR 725).
    pub(crate) initiative: Option<SeatId>,
    /// Per-player dungeon venture progress (CR 309).
    pub(crate) dungeons: BTreeMap<SeatId, DungeonProgress>,
    /// Unlocked doors for Room enchantments on the battlefield (CR 709.5).
    pub(crate) unlocked_doors: BTreeMap<ObjectId, RoomDoors>,
    /// Battlefield permanents exerted this turn (CR 701.39).
    pub(crate) exerted_objects_this_turn: BTreeSet<ObjectId>,
    /// Battlefield permanents that are currently monstrous (CR 701.31).
    pub(crate) monstrous: BTreeSet<ObjectId>,
    /// Battlefield permanents whose tribute was paid on entry (CR 702.104).
    pub(crate) tribute_paid: BTreeSet<ObjectId>,
    pub(crate) evoked: BTreeSet<ObjectId>,
    pub(crate) kicked: BTreeSet<ObjectId>,
    /// Per-index kicker payment counts carried from stack spells onto entering permanents.
    pub(crate) kicker_counts: BTreeMap<ObjectId, BTreeMap<usize, RulesInteger>>,
    pub(crate) bestowed_spells: BTreeSet<ObjectId>,
    pub(crate) bestowed_auras: BTreeMap<ObjectId, ContinuousEffectId>,
    reconfigure_effects: BTreeMap<ObjectId, ContinuousEffectId>,
    next_effect: u64,
    next_timestamp: u64,
    object_timestamps: BTreeMap<ObjectId, CharacteristicTimestamp>,
    resolved: BTreeMap<ContinuousEffectId, ResolvedContinuousEffect>,
    pub(crate) temporary_statics: BTreeMap<u64, TemporaryBattlefieldStatic>,
    next_temp_static: u64,
    counters: BTreeMap<ObjectId, PowerToughnessCounters>,
    next_world_tenure: u64,
    world_tenures: BTreeMap<ObjectId, WorldTenure>,
}
impl Default for CharacteristicState {
    fn default() -> Self {
        Self {
            commander_cards: BTreeSet::new(),
            entry_copies: BTreeMap::new(),
            mutated_copies: BTreeMap::new(),
            mutated_entry_faces: BTreeMap::new(),
            commander_chosen_colors: BTreeMap::new(),
            exhausted: BTreeSet::new(),
            attachments: BTreeMap::new(),
            paired_creatures: BTreeMap::new(),
            saddled: BTreeSet::new(),
            foretold: BTreeMap::new(),
            suspend_haste: BTreeMap::new(),
            upkeep_cuts: None,
            speed: BTreeMap::new(),
            city_blessing: BTreeSet::new(),
            no_max_hand_size_players: BTreeSet::new(),
            player_color_protection: BTreeMap::new(),
            experience: BTreeMap::new(),
            player_counters: BTreeMap::new(),
            monarch: None,
            lore: BTreeMap::new(),
            life_lost_this_turn: BTreeMap::new(),
            life_gained_this_turn: BTreeMap::new(),
            spells_cast_this_turn: BTreeMap::new(),
            attacked_this_turn_players: BTreeSet::new(),
            creatures_entered_this_turn: BTreeMap::new(),
            permanents_left_battlefield_this_turn: BTreeMap::new(),
            cards_discarded_this_turn: BTreeMap::new(),
            permanents_sacrificed_this_turn: BTreeMap::new(),
            cards_drawn_this_turn_counts: BTreeMap::new(),
            descended_this_turn: BTreeSet::new(),
            committed_crime_this_turn: BTreeSet::new(),
            entered_types_this_turn: BTreeMap::new(),
            permanents_entered_this_turn: BTreeMap::new(),
            cast_spell_types_this_turn: BTreeMap::new(),
            noncreature_spells_cast_this_turn: BTreeMap::new(),
            spells_cast_from_hand_this_turn: BTreeMap::new(),
            lands_played_this_turn: BTreeMap::new(),
            cards_left_graveyard_this_turn: BTreeMap::new(),
            nonland_permanents_left_battlefield_this_turn: BTreeMap::new(),
            any_spell_warped_this_turn: false,
            creatures_died_this_turn_count: 0,
            controlled_creatures_died_this_turn: BTreeMap::new(),
            tokens_created_this_turn: BTreeMap::new(),
            attacking_creatures_this_turn: BTreeMap::new(),
            attacked_objects_this_turn: BTreeSet::new(),
            attacked_records_this_turn: BTreeMap::new(),
            blocked_objects_this_turn: BTreeSet::new(),
            entered_objects_this_turn: BTreeSet::new(),
            combat_phases_this_turn: 0,
            cards_entered_graveyard_this_turn: BTreeMap::new(),
            counters_placed_on_creatures_this_turn: BTreeMap::new(),
            lands_entered_this_turn: BTreeMap::new(),
            dice_rolled_this_turn: BTreeMap::new(),
            other_spells_mv_this_turn: BTreeMap::new(),
            entered_from_graveyard: BTreeSet::new(),
            paid_alternative_objects: BTreeSet::new(),
            died_this_turn: BTreeSet::new(),
            put_into_gy_this_turn: BTreeSet::new(),
            put_into_gy_from_library_this_turn: BTreeSet::new(),
            named_cards: BTreeMap::new(),
            activated_sources_this_turn: BTreeMap::new(),
            ability_resolutions_by_source_this_turn: BTreeMap::new(),
            players_dealt_damage_this_turn: BTreeSet::new(),
            cast_spells_this_turn: BTreeMap::new(),
            activated_graveyard_ability_this_turn: BTreeSet::new(),
            life_lost_last_turn: BTreeMap::new(),
            spells_cast_last_turn: BTreeMap::new(),
            cards_exiled_this_turn: 0,
            cards_put_into_graveyard_this_turn: Vec::new(),
            sacrificed_this_turn: BTreeMap::new(),
            track_damage_dealt_by: false,
            damage_dealt_this_turn_by: BTreeMap::new(),
            damage_dealt_to_opponents_this_turn: BTreeMap::new(),
            combat_damage_dealt_to_player_this_turn: BTreeSet::new(),
            excess_damage_received_this_turn: BTreeSet::new(),
            entered_from_zone: BTreeMap::new(),
            cast_by_controller_objects: BTreeSet::new(),
            cast_from_hand_objects: BTreeSet::new(),
            players_dealt_noncombat_damage_this_turn: BTreeSet::new(),
            object_damage_taken_this_turn: BTreeMap::new(),
            creature_damage_pairs: None,
            vow_counters: BTreeMap::new(),
            vow_protected_players: BTreeMap::new(),
            corpse_counters: BTreeMap::new(),
            blessing_counters: BTreeMap::new(),
            despair_counters: BTreeMap::new(),
            hatchling_counters: BTreeMap::new(),
            named_counters: BTreeMap::new(),
            imprinted_cards: BTreeMap::new(),
            ability_donors_in_exile: BTreeMap::new(),
            linked_exiles: BTreeMap::new(),
            chosen_entry_players: BTreeMap::new(),
            entry_color_choices: BTreeMap::new(),
            entry_option_choices: BTreeMap::new(),
            entry_player_choices: BTreeMap::new(),
            indestructible_counters: BTreeMap::new(),
            indestructible_grants: BTreeMap::new(),
            keyword_counter_grants: BTreeMap::new(),
            page_counters: BTreeMap::new(),
            quest_counters: BTreeMap::new(),
            verse_counters: BTreeMap::new(),
            suspect_counters: BTreeMap::new(),
            charge: BTreeMap::new(),
            storage: BTreeMap::new(),
            defense: BTreeMap::new(),
            battle_protectors: BTreeMap::new(),
            loyalty: BTreeMap::new(),
            loyalty_used: BTreeMap::new(),
            loyalty_activators: BTreeSet::new(),
            extra_loyalty_activations: BTreeMap::new(),
            extra_loyalty_used: BTreeMap::new(),
            emblems: BTreeMap::new(),
            next_emblem: 1,
            age: BTreeMap::new(),
            time_counters: BTreeMap::new(),
            fade: BTreeMap::new(),
            renowned: BTreeSet::new(),
            class_levels: BTreeMap::new(),
            solved_cases: BTreeSet::new(),
            prepared: BTreeSet::new(),
            end_step_casts: BTreeMap::new(),
            cast_summaries: BTreeMap::new(),
            ring_level: BTreeMap::new(),
            ring_bearer: BTreeMap::new(),
            initiative: None,
            dungeons: BTreeMap::new(),
            unlocked_doors: BTreeMap::new(),
            exerted_objects_this_turn: BTreeSet::new(),
            monstrous: BTreeSet::new(),
            tribute_paid: BTreeSet::new(),
            evoked: BTreeSet::new(),
            kicked: BTreeSet::new(),
            kicker_counts: BTreeMap::new(),
            bestowed_spells: BTreeSet::new(),
            bestowed_auras: BTreeMap::new(),
            reconfigure_effects: BTreeMap::new(),
            next_effect: 1,
            next_timestamp: 1,
            object_timestamps: BTreeMap::new(),
            resolved: BTreeMap::new(),
            temporary_statics: BTreeMap::new(),
            next_temp_static: 1,
            counters: BTreeMap::new(),
            next_world_tenure: 1,
            world_tenures: BTreeMap::new(),
        }
    }
}
impl CharacteristicState {
    pub(crate) fn attack_allocation_mark(&self) -> [u64; 4] {
        [
            self.next_effect,
            self.next_timestamp,
            self.next_world_tenure,
            self.next_emblem,
        ]
    }
    pub(crate) fn retain_attack_mark(&mut self, mark: [u64; 4]) {
        self.next_effect = self.next_effect.max(mark[0]);
        self.next_timestamp = self.next_timestamp.max(mark[1]);
        self.next_world_tenure = self.next_world_tenure.max(mark[2]);
        self.next_emblem = self.next_emblem.max(mark[3]);
    }

    pub(crate) fn copiable_definition<'a>(
        &'a self,
        physical: &'a crate::object_identity::GameObject,
        programs: &'a BTreeMap<CardId, crate::program::Program>,
    ) -> Option<(&'a crate::program::Program, Option<CharacteristicTimestamp>)> {
        let original = physical.base_program(programs)?;
        if physical.zone() == ObjectZone::Exile && self.foretold.contains_key(&physical.id()) {
            static CONCEALED: std::sync::LazyLock<crate::program::Program> =
                std::sync::LazyLock::new(|| {
                    let mut hidden = crate::program::Program::artifact(None, vec![]);
                    hidden.card_types.clear();
                    hidden
                });
            return Some((&CONCEALED, None));
        }

        if let Some(kind) = physical.face_down() {
            return Some((kind.definition(), None));
        }

        if physical.zone() == ObjectZone::Battlefield
            && let Some((program, timestamp)) = self.copied_definition(physical.id())
        {
            return Some((program, Some(timestamp)));
        }
        if physical.zone() == ObjectZone::Battlefield
            && let Some(program) = self.entry_copies.get(&physical.id())
        {
            return Some((
                program.as_ref(),
                self.object_timestamps.get(&physical.id()).copied(),
            ));
        }
        Some((original, None))
    }
    /// Last independent layer-1 snapshot and its creating timestamp. The caller
    /// supplies a live battlefield incarnation; no donor reference is retained.
    pub(crate) fn copied_definition(
        &self,
        object: ObjectId,
    ) -> Option<(&crate::program::Program, CharacteristicTimestamp)> {
        self.resolved
            .values()
            .filter(|effect| effect.affected.contains(&object))
            .filter_map(|effect| match effect.parts.0.as_slice() {
                [ResolvedPart::Copy(program)] => Some((
                    self.mutated_copies
                        .get(&(object, effect.id))
                        .map_or(program.as_ref(), |(merged, _)| merged),
                    effect.timestamp,
                )),
                _ => None,
            })
            .max_by_key(|(_, timestamp)| *timestamp)
    }
    /// A newly merged component applies after all active copy effects in layer 1a.
    /// Preserve each underlying copy so expiry can reveal its own merged result.
    pub(crate) fn update_copy_components<E>(
        &mut self,
        object: ObjectId,
        new_top: Option<&crate::program::Program>,
        merge: impl Fn(&crate::program::Program) -> Result<crate::program::Program, E>,
    ) -> Result<(), E> {
        if let Some(program) = self.entry_copies.get_mut(&object) {
            let face = new_top
                .or_else(|| self.mutated_entry_faces.get(&object))
                .unwrap_or(program)
                .clone();
            *program = std::sync::Arc::new(merge(program)?);
            self.mutated_entry_faces.insert(object, face);
        }
        for effect in self
            .resolved
            .values()
            .filter(|effect| effect.affected.contains(&object))
        {
            if let [ResolvedPart::Copy(program)] = effect.parts.0.as_slice() {
                let key = (object, effect.id);
                let previous = self.mutated_copies.get(&key);
                let merged = merge(previous.map_or(program.as_ref(), |(merged, _)| merged))?;
                let face = new_top
                    .or_else(|| previous.map(|(_, face)| face))
                    .unwrap_or(program)
                    .clone();
                self.mutated_copies.insert(key, (merged, face));
            }
        }
        Ok(())
    }
    pub(crate) fn displayed_copy_face(&self, object: ObjectId) -> Option<&crate::program::Program> {
        if let Some((effective, time)) = self.copied_definition(object) {
            let effect = self
                .resolved
                .values()
                .find(|effect| effect.timestamp == time)?;
            return Some(
                self.mutated_copies
                    .get(&(object, effect.id))
                    .map_or(effective, |(_, face)| face),
            );
        }
        self.mutated_entry_faces.get(&object)
    }
    /// Admit exactly the trusted initial BF order without deriving history from
    /// registry or definition IDs. Validate the complete batch before allocation.
    pub(crate) fn register_initial_battlefield(
        &mut self,
        cards: &GameObjects,
        order: &[CardId],
    ) -> Result<(), CharacteristicsError> {
        if !self.object_timestamps.is_empty() || !self.resolved.is_empty() {
            return Err(CharacteristicsError::InitialBattlefieldAlreadyRegistered);
        }
        let battlefield: BTreeMap<_, _> = cards
            .iter()
            .filter(|object| object.zone() == ObjectZone::Battlefield)
            .filter_map(|object| object.card_id().map(|card| (card, object.id())))
            .collect();
        let supplied: BTreeSet<_> = order.iter().copied().collect();
        if supplied.len() != order.len() || supplied != battlefield.keys().copied().collect() {
            return Err(CharacteristicsError::InvalidInitialBattlefieldOrder);
        }
        let order = order
            .iter()
            .map(|card| battlefield[card])
            .collect::<Vec<_>>();
        self.register_battlefield_entries(&order)
    }

    /// CR 613.7e gives an attachment a fresh timestamp when it attaches to a
    /// different object. Reattaching to its current host has no effect.
    pub(crate) fn attach(
        &mut self,
        source: ObjectId,
        host: crate::targets::TargetRef,
        entering: bool,
    ) -> Result<(), CharacteristicsError> {
        if self.attachments.get(&source) != Some(&host) {
            if !entering {
                let next = self
                    .next_timestamp
                    .checked_add(1)
                    .ok_or(CharacteristicsError::TimestampExhausted)?;
                self.object_timestamps
                    .insert(source, CharacteristicTimestamp(self.next_timestamp));
                self.next_timestamp = next;
            }
            self.attachments.insert(source, host);
        }
        Ok(())
    }
    /// End the continuous type change created when this exact Equipment attached.
    pub(crate) fn detach(&mut self, source: ObjectId) -> Option<crate::targets::TargetRef> {
        if let Some(effect) = self.bestowed_auras.remove(&source) {
            self.resolved.remove(&effect);
        }
        if let Some(effect) = self.reconfigure_effects.remove(&source) {
            self.resolved.remove(&effect);
        }
        self.attachments.remove(&source)
    }
    /// Start the bestowed permanent’s layer-4 type change as it enters (CR 702.103b).
    pub(crate) fn enter_bestowed(
        &mut self,
        entry: StackEntryId,
        source: ObjectId,
    ) -> Result<(), CharacteristicsError> {
        let effect = self.add_effect(
            entry,
            source,
            ResolvedParts(vec![ResolvedPart::Type(TypeChange {
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Set([CardType::Enchantment].into()),
                subtypes: SubtypeOperation::Keep,
                land_subtypes: SubtypeOperation::Keep,
            })]),
            ResolvedDuration::NoStatedDuration,
        )?;
        self.bestowed_auras.insert(source, effect.id);
        Ok(())
    }
    /// CR 702.151b creates a timestamped effect that lasts for this attachment.
    pub(crate) fn stop_being_creature_while_attached(
        &mut self,
        entry: StackEntryId,
        source: ObjectId,
    ) -> Result<(), CharacteristicsError> {
        let effect = self.add_effect(
            entry,
            source,
            ResolvedParts(vec![ResolvedPart::Type(TypeChange {
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                subtypes: SubtypeOperation::Keep,
                land_subtypes: SubtypeOperation::Keep,
            })]),
            ResolvedDuration::NoStatedDuration,
        )?;
        self.reconfigure_effects.insert(source, effect.id);
        Ok(())
    }
    /// Trusted single entry through the same complete-order allocator.
    pub(crate) fn restamp_transformations(
        &mut self,
        order: &[ObjectId],
    ) -> Result<(), CharacteristicsError> {
        for id in order {
            self.object_timestamps.remove(id);
        }
        self.register_battlefield_entries(order)
    }
    pub(crate) fn register_battlefield_entry(
        &mut self,
        object: ObjectId,
    ) -> Result<CharacteristicTimestamp, CharacteristicsError> {
        let timestamp = CharacteristicTimestamp(self.next_timestamp);
        self.register_battlefield_entries(&[object])?;
        Ok(timestamp)
    }

    /// Allocate distinct relative timestamps in the complete selected order.
    /// Duplicate members, prior registrations and overflow reject before mutation.
    /// World tenure is reconciled separately at the one actual publication cut.
    pub(crate) fn register_battlefield_entries(
        &mut self,
        order: &[ObjectId],
    ) -> Result<(), CharacteristicsError> {
        let mut seen = BTreeSet::new();
        if order
            .iter()
            .any(|id| !seen.insert(*id) || self.object_timestamps.contains_key(id))
        {
            return Err(CharacteristicsError::DuplicateBattlefieldTimestamp);
        }
        let count =
            u64::try_from(order.len()).map_err(|_| CharacteristicsError::TimestampExhausted)?;
        let next = self
            .next_timestamp
            .checked_add(count)
            .ok_or(CharacteristicsError::TimestampExhausted)?;
        for (id, value) in order.iter().zip(self.next_timestamp..next) {
            self.object_timestamps
                .insert(*id, CharacteristicTimestamp(value));
        }
        self.next_timestamp = next;
        Ok(())
    }

    pub(crate) fn create_emblem(
        &mut self,
        controller: crate::priority::SeatId,
        definition: crate::emblem::EmblemDefinition,
    ) -> Result<crate::emblem::Emblem, CharacteristicsError> {
        let next_id = self
            .next_emblem
            .checked_add(1)
            .ok_or(CharacteristicsError::EffectIdentityExhausted)?;
        let next_timestamp = self
            .next_timestamp
            .checked_add(1)
            .ok_or(CharacteristicsError::TimestampExhausted)?;
        let emblem = crate::emblem::Emblem {
            id: crate::emblem::EmblemId(self.next_emblem),
            controller,
            definition,
            timestamp: CharacteristicTimestamp(self.next_timestamp),
        };
        self.next_emblem = next_id;
        self.next_timestamp = next_timestamp;
        self.emblems.insert(emblem.id, emblem.clone());
        Ok(emblem)
    }

    pub(crate) fn clear_indestructible_counter_grant(&mut self, object: ObjectId) {
        if let Some(id) = self.indestructible_grants.remove(&object) {
            self.resolved.remove(&id);
        }
    }

    /// A fresh counter of the same kind gives the whole group a new timestamp (CR 613.7d).
    pub(crate) fn refresh_indestructible_counter_grant(
        &mut self,
        entry: StackEntryId,
        object: ObjectId,
    ) -> Result<(), CharacteristicsError> {
        let effect = self.add_effect(
            entry,
            object,
            ResolvedParts(vec![ResolvedPart::Abilities(
                AbilityChange::grant_keywords([KeywordAbility::Indestructible]),
            )]),
            ResolvedDuration::NoStatedDuration,
        )?;
        self.clear_indestructible_counter_grant(object);
        self.indestructible_grants.insert(object, effect.id);
        Ok(())
    }

    pub(crate) fn clear_keyword_counter_grant(
        &mut self,
        object: ObjectId,
        kind: crate::effects::CounterKind,
    ) {
        if kind == crate::effects::CounterKind::Indestructible {
            self.clear_indestructible_counter_grant(object);
            return;
        }
        if let Some(id) = self.keyword_counter_grants.remove(&(object, kind)) {
            self.resolved.remove(&id);
        }
    }

    /// CR 122.1b & CR 613.7d: keyword counters grant their keyword in Layer 6 with a fresh timestamp.
    pub(crate) fn refresh_keyword_counter_grant(
        &mut self,
        entry: StackEntryId,
        object: ObjectId,
        kind: crate::effects::CounterKind,
    ) -> Result<(), CharacteristicsError> {
        if kind == crate::effects::CounterKind::Indestructible {
            return self.refresh_indestructible_counter_grant(entry, object);
        }
        let Some(keyword) = kind.keyword_counter_ability() else {
            return Ok(());
        };
        let effect = self.add_effect(
            entry,
            object,
            ResolvedParts(vec![ResolvedPart::Abilities(
                AbilityChange::grant_keywords([keyword]),
            )]),
            ResolvedDuration::NoStatedDuration,
        )?;
        self.clear_keyword_counter_grant(object, kind);
        self.keyword_counter_grants
            .insert((object, kind), effect.id);
        Ok(())
    }

    pub(crate) fn add_effect(
        &mut self,
        creator: StackEntryId,
        target: ObjectId,
        parts: ResolvedParts,
        duration: ResolvedDuration,
    ) -> Result<ResolvedContinuousEffect, CharacteristicsError> {
        self.add_effect_for_objects(creator, BTreeSet::from([target]), parts, duration)
    }

    pub(crate) fn add_effect_for_objects(
        &mut self,
        creator: StackEntryId,
        affected: BTreeSet<ObjectId>,
        mut parts: ResolvedParts,
        duration: ResolvedDuration,
    ) -> Result<ResolvedContinuousEffect, CharacteristicsError> {
        parts
            .validate()
            .map_err(CharacteristicsError::InvalidParts)?;
        parts.0.sort_by_key(ResolvedPart::bucket);
        let next_effect = self
            .next_effect
            .checked_add(1)
            .ok_or(CharacteristicsError::EffectIdentityExhausted)?;
        let next_timestamp = self
            .next_timestamp
            .checked_add(1)
            .ok_or(CharacteristicsError::TimestampExhausted)?;
        let id = ContinuousEffectId(self.next_effect);
        let effect = ResolvedContinuousEffect {
            id,
            timestamp: CharacteristicTimestamp(self.next_timestamp),
            creator,
            affected,
            parts,
            duration,
        };
        self.resolved.insert(id, effect.clone());
        self.next_effect = next_effect;
        self.next_timestamp = next_timestamp;
        Ok(effect)
    }
    pub(crate) fn take_plus_one_counter(
        &mut self,
        target: ObjectId,
    ) -> Result<(), CharacteristicsError> {
        self.take_power_toughness_counters(
            target,
            &PowerToughnessCounters {
                plus_one: 1.into(),
                minus_one: 0.into(),
            },
        )?;
        Ok(())
    }
    pub(crate) fn take_power_toughness_counters(
        &mut self,
        target: ObjectId,
        removed: &PowerToughnessCounters,
    ) -> Result<PowerToughnessCounters, CharacteristicsError> {
        let current = self
            .counters
            .get_mut(&target)
            .ok_or(CharacteristicsError::InvalidCounterPlacement)?;
        if !removed.valid_placement()
            || removed.plus_one > current.plus_one
            || removed.minus_one > current.minus_one
        {
            return Err(CharacteristicsError::InvalidCounterPlacement);
        }
        current.plus_one = current.plus_one.minus(&removed.plus_one);
        current.minus_one = current.minus_one.minus(&removed.minus_one);
        let after = current.clone();
        if after == PowerToughnessCounters::default() {
            self.counters.remove(&target);
        }
        Ok(after)
    }
    pub(crate) fn place_counters(
        &mut self,
        target: ObjectId,
        placement: &PowerToughnessCounters,
    ) -> Result<(), CharacteristicsError> {
        if !placement.valid_placement() {
            return Err(CharacteristicsError::InvalidCounterPlacement);
        }
        let current = self.counters.entry(target).or_default();
        current.plus_one = current.plus_one.plus(&placement.plus_one);
        current.minus_one = current.minus_one.plus(&placement.minus_one);
        Ok(())
    }
    pub(crate) fn counters(&self, object: ObjectId) -> PowerToughnessCounters {
        self.counters.get(&object).cloned().unwrap_or_default()
    }
    pub(crate) fn next_effect_sequence(&self) -> u64 {
        self.next_effect
    }
    pub(crate) fn next_timestamp_sequence(&self) -> u64 {
        self.next_timestamp
    }
    pub(crate) fn object_timestamp(&self, object: ObjectId) -> Option<CharacteristicTimestamp> {
        self.object_timestamps.get(&object).copied()
    }
    pub(crate) fn object_timestamps(
        &self,
    ) -> impl Iterator<Item = (ObjectId, CharacteristicTimestamp)> + '_ {
        self.object_timestamps
            .iter()
            .map(|(object, stamp)| (*object, *stamp))
    }
    pub(crate) fn effects(&self) -> impl Iterator<Item = &ResolvedContinuousEffect> {
        self.resolved.values()
    }
    pub(crate) fn effect(&self, id: ContinuousEffectId) -> Option<&ResolvedContinuousEffect> {
        self.resolved.get(&id)
    }
    pub(crate) fn expire_departed_player_control(&mut self, players: &BTreeSet<SeatId>) {
        self.resolved.retain(|_, effect| {
            effect.parts.0.retain(|part| {
                !matches!(part, ResolvedPart::Control(controller) if players.contains(controller))
            });
            !effect.parts.0.is_empty()
        });
    }
    pub(crate) fn expire_at_cleanup(
        &mut self,
        active: crate::priority::SeatId,
        turn: u64,
    ) -> Vec<ContinuousEffectId> {
        self.player_color_protection.clear();
        self.expire_effects_matching(|duration| duration.expires_at_cleanup(active, turn))
    }

    pub(crate) fn expire_at_end_of_combat(&mut self) -> Vec<ContinuousEffectId> {
        self.expire_effects_matching(ResolvedDuration::expires_at_end_of_combat)
    }

    pub(crate) fn clear_extra_turn_history(&mut self) {
        self.combat_phases_this_turn = 0;
        self.cards_entered_graveyard_this_turn.clear();
        self.counters_placed_on_creatures_this_turn.clear();
        self.lands_entered_this_turn.clear();
        self.dice_rolled_this_turn.clear();
        self.other_spells_mv_this_turn.clear();
        self.attacked_records_this_turn.clear();
        self.put_into_gy_this_turn.clear();
        self.put_into_gy_from_library_this_turn.clear();
    }

    pub(crate) fn expire_at_turn_start(
        &mut self,
        active: crate::priority::SeatId,
        turn: u64,
    ) -> Vec<ContinuousEffectId> {
        self.clear_extra_turn_history();
        self.expire_effects_matching(|duration| duration.expires_at_turn_start(active, turn))
    }

    pub(crate) fn expire_at_skipped_turns(
        &mut self,
        skipped: &BTreeSet<SeatId>,
    ) -> Vec<ContinuousEffectId> {
        self.expire_effects_matching(|duration| {
            matches!(
                duration,
                ResolvedDuration::ThroughNextTurn { player, .. }
                    | ResolvedDuration::UntilPlayerNextTurn { player, .. }
                if skipped.contains(&player)
            )
        })
    }

    /// Register a duration-scoped battlefield static ability (CR 611.2a).
    pub(crate) fn add_temporary_static(
        &mut self,
        controller: SeatId,
        source_object: Option<ObjectId>,
        duration: ResolvedDuration,
        ability: StaticContinuousAbility,
    ) -> Result<u64, CharacteristicsError> {
        self.add_temporary_static_locked(controller, source_object, duration, ability, None)
    }

    pub(crate) fn add_temporary_static_locked(
        &mut self,
        controller: SeatId,
        source_object: Option<ObjectId>,
        duration: ResolvedDuration,
        ability: StaticContinuousAbility,
        locked_objects: Option<BTreeSet<ObjectId>>,
    ) -> Result<u64, CharacteristicsError> {
        let id = self.next_temp_static;
        let next_id = id
            .checked_add(1)
            .ok_or(CharacteristicsError::EffectIdentityExhausted)?;
        let timestamp = CharacteristicTimestamp(self.next_timestamp);
        let next_ts = self
            .next_timestamp
            .checked_add(1)
            .ok_or(CharacteristicsError::TimestampExhausted)?;
        self.next_temp_static = next_id;
        self.next_timestamp = next_ts;
        self.temporary_statics.insert(
            id,
            TemporaryBattlefieldStatic {
                id,
                controller,
                source_object,
                timestamp,
                duration,
                ability,
                locked_objects,
            },
        );
        Ok(id)
    }

    fn expire_effects_matching(
        &mut self,
        mut ends: impl FnMut(ResolvedDuration) -> bool,
    ) -> Vec<ContinuousEffectId> {
        let mut expired = Vec::new();
        self.resolved.retain(|&id, effect| {
            let ended = ends(effect.duration);
            if ended {
                expired.push(id);
            }
            !ended
        });
        self.temporary_statics
            .retain(|_, temp| !ends(temp.duration));
        expired
    }

    /// Remove effects whose exact source left, phased out, untapped, or changed controller.
    pub(crate) fn expire_source_control(
        &mut self,
        cards: &GameObjects,
        evaluated: &EvaluatedCharacteristics,
    ) -> Vec<ContinuousEffectId> {
        let mut expired = Vec::new();
        let still_ok = |duration: ResolvedDuration| -> bool {
            match duration {
                ResolvedDuration::WhileSourceControlled {
                    source,
                    controller,
                    epoch,
                } => {
                    cards.get(source).is_some_and(|card| {
                        card.zone() == crate::object_identity::ObjectZone::Battlefield
                            && cards.phased_out_under(source).is_none()
                            && card.control_epoch() == Some(epoch)
                    }) && evaluated.get(source).is_some_and(|object| {
                        object.controller == Some(controller)
                            && object.zone == crate::object_identity::ObjectZone::Battlefield
                    })
                }
                ResolvedDuration::WhileSourceOnBattlefield { source } => {
                    cards.get(source).is_some_and(|card| {
                        card.zone() == crate::object_identity::ObjectZone::Battlefield
                            && cards.phased_out_under(source).is_none()
                    })
                }
                ResolvedDuration::WhileSourceTapped { source } => {
                    cards.get(source).is_some_and(|card| {
                        card.zone() == crate::object_identity::ObjectZone::Battlefield
                            && cards.phased_out_under(source).is_none()
                            && card.tapped() == Some(true)
                    })
                }
                ResolvedDuration::WhileSourceControlledAndTapped {
                    source,
                    controller,
                    epoch,
                } => {
                    cards.get(source).is_some_and(|card| {
                        card.zone() == crate::object_identity::ObjectZone::Battlefield
                            && cards.phased_out_under(source).is_none()
                            && card.control_epoch() == Some(epoch)
                            && card.tapped() == Some(true)
                    }) && evaluated.get(source).is_some_and(|object| {
                        object.controller == Some(controller)
                            && object.zone == crate::object_identity::ObjectZone::Battlefield
                    })
                }
                _ => true,
            }
        };
        self.resolved.retain(|id, effect| {
            let keep = still_ok(effect.duration);
            if !keep {
                expired.push(*id);
            }
            keep
        });
        self.temporary_statics
            .retain(|_, temp| still_ok(temp.duration));
        expired
    }

    /// Compute from pre-check state and apply in the same SBA batch as death.
    pub(crate) fn counter_cancellations(&self) -> Vec<(ObjectId, PowerToughnessCounters)> {
        self.counters
            .iter()
            .filter(|(_, counts)| counts.plus_one.is_positive() && counts.minus_one.is_positive())
            .map(|(object, counts)| {
                let cancel = std::cmp::min(&counts.plus_one, &counts.minus_one);
                (
                    *object,
                    PowerToughnessCounters {
                        plus_one: counts.plus_one.minus(cancel),
                        minus_one: counts.minus_one.minus(cancel),
                    },
                )
            })
            .collect()
    }
    pub(crate) fn apply_counter_cancellations(
        &mut self,
        updates: Vec<(ObjectId, PowerToughnessCounters)>,
    ) {
        for (object, counts) in updates {
            if counts.plus_one.is_positive() || counts.minus_one.is_positive() {
                self.counters.insert(object, counts);
            } else {
                self.counters.remove(&object);
            }
        }
    }
    pub(crate) fn expire_suspend_haste(&mut self, cards: &GameObjects) -> bool {
        let mut removed = false;
        self.suspend_haste.retain(|object, (controller, effect)| {
            let keep = cards
                .get(*object)
                .is_some_and(|c| c.controller() == Some(*controller));
            if !keep && let Some(id) = effect {
                removed |= self.resolved.remove(id).is_some();
            }
            keep
        });
        removed
    }
    pub(crate) fn retain_battlefield(
        &mut self,
        live: &BTreeSet<ObjectId>,
        linked_counter_sources: &BTreeSet<ObjectId>,
    ) {
        self.entry_copies.retain(|id, _| live.contains(id));
        self.mutated_entry_faces
            .retain(|object, _| live.contains(object));
        self.mutated_copies
            .retain(|(object, id), _| live.contains(object) && self.resolved.contains_key(id));
        self.lore.retain(|id, _| live.contains(id));
        self.vow_counters.retain(|id, _| live.contains(id));
        self.vow_protected_players.retain(|id, _| live.contains(id));
        self.corpse_counters
            .retain(|object, _| live.contains(object));
        self.blessing_counters
            .retain(|object, _| live.contains(object));
        self.hatchling_counters
            .retain(|object, _| live.contains(object));
        self.despair_counters
            .retain(|object, _| live.contains(object));
        self.named_counters.retain(|_, counters| {
            counters.retain(|object, _| {
                live.contains(object) || linked_counter_sources.contains(object)
            });
            !counters.is_empty()
        });
        self.imprinted_cards
            .retain(|source, _| live.contains(source));
        self.ability_donors_in_exile
            .retain(|source, _| live.contains(source));
        self.chosen_entry_players
            .retain(|source, _| live.contains(source));
        self.entry_color_choices
            .retain(|source, _| live.contains(source));
        self.entry_option_choices
            .retain(|source, _| live.contains(source));
        self.entry_player_choices
            .retain(|source, _| live.contains(source));
        self.indestructible_counters
            .retain(|object, _| live.contains(object));
        self.indestructible_grants
            .retain(|object, _| live.contains(object));
        self.keyword_counter_grants
            .retain(|(object, _), _| live.contains(object));
        self.page_counters.retain(|id, _| live.contains(id));
        self.quest_counters.retain(|id, _| live.contains(id));
        self.verse_counters.retain(|id, _| live.contains(id));
        self.suspect_counters.retain(|id, _| live.contains(id));
        self.charge.retain(|id, _| live.contains(id));
        self.storage.retain(|id, _| live.contains(id));
        self.defense.retain(|id, _| live.contains(id));
        self.battle_protectors.retain(|id, _| live.contains(id));
        self.loyalty.retain(|id, _| live.contains(id));
        self.loyalty_used.retain(|id, _| live.contains(id));
        self.extra_loyalty_used
            .retain(|(id, _), _| live.contains(id));
        self.fade.retain(|id, _| live.contains(id));
        self.age.retain(|id, _| live.contains(id));
        self.renowned.retain(|id| live.contains(id));
        self.resolved.retain(|_, effect| {
            effect.affected.retain(|object| live.contains(object));
            !effect.affected.is_empty()
        });
        self.attachments.retain(|source, host| {
            live.contains(source)
                && match host {
                    crate::targets::TargetRef::Object(object) => live.contains(object),
                    crate::targets::TargetRef::Player(_) => true,
                    crate::targets::TargetRef::Stack(_) => false,
                }
        });
        self.bestowed_auras.retain(|source, id| {
            if self.attachments.contains_key(source) {
                true
            } else {
                self.resolved.remove(id);
                false
            }
        });
        self.reconfigure_effects.retain(|source, id| {
            if self.attachments.contains_key(source) {
                true
            } else {
                self.resolved.remove(id);
                false
            }
        });
        self.end_step_casts
            .retain(|object, _| live.contains(object));
        self.cast_summaries.retain(|object, _| {
            live.contains(object) || self.entered_objects_this_turn.contains(object)
        });
        self.kicked.retain(|object| live.contains(object));
        self.evoked.retain(|object| live.contains(object));
        self.entered_from_graveyard
            .retain(|object| live.contains(object));
        self.paid_alternative_objects
            .retain(|object| live.contains(object));
        self.saddled.retain(|object| live.contains(object));
        self.ring_bearer.retain(|_, object| live.contains(object));
        self.unlocked_doors
            .retain(|object, _| live.contains(object));
        self.monstrous.retain(|object| live.contains(object));
        self.exerted_objects_this_turn
            .retain(|object| live.contains(object));
        self.exhausted
            .retain(|(object, _, _, _)| live.contains(object));
        self.counters.retain(|object, _| live.contains(object));
        self.object_timestamps
            .retain(|object, _| live.contains(object));
        // World departures remain until the actual publisher reconciles history
        // and records Lost; ordinary battlefield pruning cannot erase that fact.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_exhaustion_and_invalid_counter_counts_never_change_owned_state() {
        use crate::{
            object_identity::{CardId, GameObjects, ObjectDestination},
            priority::SeatId,
            stack_window::StackWindow,
        };
        let objects = GameObjects::new(0)
            .seed_card(0, CardId(1), SeatId(0), ObjectDestination::Hand)
            .unwrap()
            .next;
        let card = objects.current_identity(CardId(1)).unwrap();
        let window = StackWindow::new(vec![SeatId(0), SeatId(1)], SeatId(0), objects).unwrap();
        let window = window.finish_checks(window.revision()).unwrap();
        let window = window
            .record_card_spell(window.revision(), SeatId(0), card)
            .unwrap();
        let source = window.entries()[0].id();
        let mut state = CharacteristicState {
            next_effect: u64::MAX,
            ..Default::default()
        };
        let saved = state.clone();
        assert_eq!(
            state.add_effect(
                source,
                card,
                ResolvedParts(vec![ResolvedPart::SwitchPT]),
                ResolvedDuration::NoStatedDuration
            ),
            Err(CharacteristicsError::EffectIdentityExhausted)
        );
        for (plus, minus) in [(-1, 2), (2, -1), (0, 0)] {
            assert_eq!(
                state.place_counters(
                    card,
                    &PowerToughnessCounters {
                        plus_one: plus.into(),
                        minus_one: minus.into()
                    }
                ),
                Err(CharacteristicsError::InvalidCounterPlacement)
            );
        }
        assert_eq!(state, saved);
    }
}
