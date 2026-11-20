//! Strict compiled C03 source-input grammar.
//! Native signatures come from the guarded Registry8 owner; pending nulls reject.
//! Reviewed recipe prose remains input intent, independent of native observations.
use super::{super::catalogue::DefinitionInput, OptionalCase};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Inputs {
    pub(super) schema_version: u32,
    pub(super) profile: Profile,
    pub(super) epochs: Epochs,
    pub(super) authorities: Vec<Receipt>,
    pub(super) definitions: Vec<DefinitionInput>,
    pub(super) decks: Vec<Deck>,
    pub(super) physical_inventory: Vec<PhysicalRow>,
    pub(super) token_templates: Vec<TokenTemplateRef>,
    pub(super) full_game_recipes: FullGameRecipes,
}
#[derive(Debug, Clone, Copy, Deserialize)]
pub(super) enum Profile {
    #[serde(rename = "optional-payment-v1")]
    OptionalPayment,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Epochs {
    pub(super) registry: u32,
    pub(super) report: u32,
    pub(super) aggregate: u32,
    pub(super) current: u32,
    pub(super) witness: u32,
    pub(super) record_codec: u32,
    pub(super) vocabulary: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
}
// DefinitionInput is imported from the existing catalogue owner. Its String
// signature field rejects preparation nulls; materialize_rows remains the only
// native source/physical validation and Registry::seed owner.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Deck {
    pub(super) role: Role,
    pub(super) card_count: u64,
    pub(super) entries: Vec<DeckEntry>,
    pub(super) sideboard: Vec<()>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeckEntry {
    pub(super) name: String,
    pub(super) oracle_id: String,
    pub(super) count: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhysicalRow {
    pub(super) alias: String,
    pub(super) card_id: String,
    pub(super) owner: u64,
    pub(super) role: Role,
    pub(super) oracle_id: String,
    pub(super) name: String,
    pub(super) copy: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TokenTemplateRef {
    pub(super) source_oracle_id: String,
    pub(super) triggered_ability_id: Option<u64>,
    pub(super) instruction: usize,
    pub(super) creation: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(super) enum Role {
    P0,
    P1,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Roles<T> {
    #[serde(rename = "P0")]
    pub(super) p0: T,
    #[serde(rename = "P1")]
    pub(super) p1: T,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FullGameRecipes {
    pub(super) kind: String,
    pub(super) candidate_sha256: String,
    pub(super) participants: Vec<Participant>,
    pub(super) first_player: Role,
    pub(super) life_each: String,
    pub(super) format: String,
    pub(super) opening: String,
    pub(super) shuffle_basis: String,
    pub(super) card_order_basis: String,
    pub(super) policy_defaults: Vec<String>,
    pub(super) checkpoint_policy: Vec<String>,
    pub(super) expected_worlds: (),
    pub(super) limits: String,
    pub(super) cases: Vec<FullGameCase>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Participant {
    pub(super) role: Role,
    pub(super) seat: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FullGameCase {
    pub(super) id: OptionalCase,
    pub(super) decks: Vec<PhysicalDeck>,
    pub(super) turns: Vec<Turn>,
    pub(super) draw_plan: Roles<Vec<String>>,
    pub(super) responses_and_choices: Vec<Response>,
    pub(super) primary_expected: PrimaryExpected,
    pub(super) global_entropy_words: Vec<u64>,
    pub(super) budgets: Budgets,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhysicalDeck {
    pub(super) role: Role,
    pub(super) physical_cards: Vec<PhysicalCard>,
    pub(super) before_card_ids: Vec<String>,
    pub(super) after_card_ids: Vec<String>,
    pub(super) after_aliases_top_first: Vec<String>,
    pub(super) opening_hand_aliases: Vec<String>,
    pub(super) entropy_words: Vec<u64>,
    pub(super) algorithm: String,
    pub(super) entropy_words_consumed: u64,
    pub(super) qualification: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhysicalCard {
    pub(super) alias: String,
    pub(super) card_id: String,
    pub(super) owner: u64,
    pub(super) name: String,
    pub(super) oracle_id: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Turn {
    pub(super) role: Role,
    pub(super) own_turn: u64,
    pub(super) global_turn: u64,
    pub(super) precombat_main_land: String,
    pub(super) precombat_main_casts: Vec<Cast>,
    pub(super) attackers: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cast {
    pub(super) spell: String,
    pub(super) funding_taps_in_order: Vec<String>,
    pub(super) allocation: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Response {
    pub(super) when: String,
    pub(super) required: String,
    pub(super) funding_taps_in_order: Option<Vec<String>>,
    pub(super) casting_spend: Option<Allocation>,
    pub(super) cost: Option<String>,
    pub(super) result: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Allocation {
    #[serde(rename = "White")]
    pub(super) white: Option<String>,
    #[serde(rename = "Blue")]
    pub(super) blue: Option<String>,
    #[serde(rename = "Black")]
    pub(super) black: Option<String>,
    #[serde(rename = "Red")]
    pub(super) red: Option<String>,
    #[serde(rename = "Green")]
    pub(super) green: Option<String>,
    #[serde(rename = "Colourless")]
    pub(super) colourless: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrimaryExpected {
    pub(super) winner: Role,
    pub(super) loser: Role,
    pub(super) terminal_global_turn: u64,
    pub(super) terminal_step: String,
    pub(super) life: Roles<String>,
    #[serde(rename = "combat_damage_to_P1")]
    pub(super) combat_damage_to_p1: String,
    #[serde(rename = "mandatory_life_loss_P1")]
    pub(super) mandatory_life_loss_p1: Option<String>,
    pub(super) unblocked_vanguard_hits: u64,
    pub(super) casts: u64,
    pub(super) ordinary_trigger_entries: u64,
    pub(super) resolution_payments: Payments,
    pub(super) resolution_paid_mana: Roles<String>,
    pub(super) optional_outcomes: OptionalOutcomes,
    pub(super) mana_color_replies: u64,
    pub(super) successful_draws_excluding_opening: Roles<u64>,
    pub(super) remaining_library_cards: Roles<u64>,
    pub(super) physical_graveyard_aliases: Vec<String>,
    pub(super) draw_failures: u64,
    pub(super) mulligans: u64,
    pub(super) cleanup_discards: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Payments {
    #[serde(rename = "Paid")]
    pub(super) paid: u64,
    #[serde(rename = "Declined")]
    pub(super) declined: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OptionalOutcomes {
    #[serde(rename = "Take")]
    pub(super) take: u64,
    #[serde(rename = "Decline")]
    pub(super) decline: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Budgets {
    pub(super) accepted_inputs_max: u64,
    pub(super) reached_rejection_probes_max: u64,
    pub(super) global_turns_max: u64,
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;
