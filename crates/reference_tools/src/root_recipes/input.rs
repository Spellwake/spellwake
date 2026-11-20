//! Input-only schema and selected-case admission. No expected/reference result is read.
pub(super) use crate::packet::seat;
use crate::{
    packet::{AliasContext, DefinitionPolicy, Initial, Packet, Seed, SeedMode},
    program,
};
use serde::Deserialize;
use serde_json::Value;
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    game::{GameContext, GameError, GameId, NativeCandidate, OpeningSetup},
    setup::GameSeed,
    shuffle::EntropyTape,
    turn::TurnStart,
};
use std::collections::{BTreeMap, BTreeSet};

pub use super::policy::StagePolicy;
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fixture {
    pub id: String,
    pub scenario: String,
    pub scope: String,
    pub forge_route: String,
    pub seed: Seed,
    pub actions: Vec<Action>,
    pub bindings: Vec<Binding>,
    pub choice_tables: Vec<ChoiceTable>,
    pub stop: Stop,
    pub notes: Vec<String>,
    pub card_id_base: u64,
    pub engine_recipes: Recipes,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recipes {
    pub native: NativeRecipe,
    pub forge: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeRecipe {
    pub action_ordinals: Vec<usize>,
    pub bindings: Option<String>,
    pub budget: Budget,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Budget {
    pub max_root_inputs: usize,
    pub max_native_auto_work_returns: Option<usize>,
    pub max_private_owner_calls: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChoiceTable {
    pub context: String,
    pub source: Reference,
    pub actor: String,
    pub reply: String,
    pub forge_callback: String,
    pub allowed_color_set: Vec<String>,
    pub retain_actual_offered_mask_and_order: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Stop {
    pub kind: String,
    pub step: Option<String>,
    pub turn: Option<u64>,
    pub cleanup_iteration: Option<u64>,
    pub after_action_ordinal: usize,
    pub no_extra_pass: bool,
    pub ready_guard: Option<ReadyGuard>,
    pub outcome_guard: Option<OutcomeGuard>,
    pub pending_guard: Option<FrameGuard>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Reference {
    Current { current_owned_action_alias: String },
    Captured { captured_object: String },
    Entry { entry_ref: String },
    Player { player: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Spend {
    #[serde(rename = "type")]
    pub kind: String,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Action {
    PassRound {
        actors: Vec<String>,
        role: String,
    },
    RepeatBounded {
        count: usize,
        sequence: Vec<Action>,
    },
    Pass {
        actor: String,
    },
    PlayLand {
        actor: String,
        source: Reference,
    },
    ActivateMana {
        actor: String,
        source: Reference,
        #[serde(deserialize_with = "crate::mana_ref::deserialize")]
        ability: spellwake_engine::mana::ManaAbilityRef,
        bind_production: Option<String>,
    },
    Cast {
        actor: String,
        source: Reference,
        target: Option<Reference>,
        payment: Vec<Spend>,
        bind_entry: Option<String>,
    },
    ActivateAbility {
        actor: String,
        source: Reference,
        ability_id: u64,
        target: Reference,
        payment: Vec<Spend>,
        bind_entry: Option<String>,
    },
    ReplyExactCurrentFrame {
        actor: String,
        kind: String,
        items: Vec<Value>,
        token: String,
    },
    SaveSnapshot {
        name: String,
        combat_guard: Option<CombatGuard>,
        frame_guard: Option<FrameGuard>,
    },
    AttemptWithoutAdoption {
        snapshot: Option<String>,
        command: Box<Action>,
    },
    AttemptReplyWithoutAdoption {
        snapshot: Option<String>,
        mutation: Mutation,
        combat_guard: Option<CombatGuard>,
        retain_main_source: Option<String>,
    },
    ReadOnlyOperations {
        methods: Vec<String>,
        repeat_count: usize,
    },
    EvaluateSameInputOnTwoForks {
        snapshot: String,
        input: Box<Action>,
        adopt_neither: bool,
    },
    SupplyEntropy {
        basis: String,
        words: Vec<u64>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mutation {
    pub actor: Option<String>,
    pub reply_kind: Option<String>,
    pub items: Option<Vec<Value>>,
    pub decision_id: Option<DecisionReference>,
    pub color: Option<String>,
    pub copy_choice: Option<CopyChoice>,
    pub basis_revision: Option<String>,
    pub token: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum DecisionReference {
    Mutation(String),
    Snapshot { from_snapshot: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadyGuard {
    pub priority: String,
    pub stack_entry_refs_bottom_to_top: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OutcomeGuard {
    pub winner: String,
    pub state_based_losses: Vec<String>,
    pub priority_forbidden: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalRow {
    pub slot: String,
    pub value: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum ParentGuard {
    Resolution {
        entry_ref: String,
        instruction: String,
        locals: Vec<LocalRow>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum FrameGuard {
    TriggerTarget {
        chooser: String,
        instance_ref: String,
        entry_ref: String,
        domain: Vec<Reference>,
    },
    CreatureType {
        entry_ref: String,
        instruction: String,
        slot: String,
        chooser: String,
        vocabulary: String,
        excluding: Vec<Value>,
        domain: String,
    },
    OrderTriggers {
        chooser: String,
        instance_refs: Vec<String>,
    },
    CopyTargets {
        entry_ref: String,
        instruction: String,
        chooser: String,
        original_entry_ref: String,
    },
    DamageOrder {
        entry_ref: String,
        instruction: String,
        chooser: String,
        application_effect_refs: Vec<String>,
        parent: ParentGuard,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CombatGuard {
    pub frame: String,
    pub actor: String,
    pub source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
pub(super) enum CopyChoice {
    Retain { target: Option<Reference> },
    Change { target: Reference },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub name: String,
    pub when: String,
    pub join: Join,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Join {
    TokenCreation(crate::receipt_joins::TokenCreationSelector),
    BirthDiscovery {
        source_object_ref: String,
        triggered_ability_id: u64,
        related_token_after_ref: String,
        require_unique: bool,
    },
    Seed {
        card_id: u64,
        #[serde(alias = "require_exact_original_incarnation")]
        require_exact_current_incarnation: bool,
    },
    Discovery {
        source_object_ref: String,
        triggered_ability_id: u64,
        related_object_ref: Option<String>,
        step: Option<StepJoin>,
        require_unique: bool,
    },
    ResolutionExit {
        resolution_entry_ref: String,
        actual_zone_move_before_ref: String,
        field: String,
        require_unique: bool,
    },
    PermanentEntry {
        actual_permanent_entry_of: String,
        field: String,
        require_unique: bool,
    },
    TriggerPlacement {
        trigger_placed_instance_ref: String,
        field: String,
        require_unique: bool,
    },
    CopyCreation {
        copy_created_by_entry_ref: String,
        instruction: String,
        original_entry_ref: String,
        field: String,
        require_unique: bool,
    },
    DamageEffectCreation {
        damage_effect_created_by_entry_ref: String,
        instruction: String,
        effect_kind: String,
        field: String,
        require_unique: bool,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StepJoin {
    pub step: String,
    pub active: String,
    pub turn: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub(super) fixture: Fixture,
    pub raw_fixture: Value,
    pub definitions: BTreeMap<String, Value>,
    pub packet_metadata: Value,
    pub aliases: AliasContext,
    pub vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    mode: SeedMode,
}

pub fn parse(bytes: &[u8], case: &str, policy: StagePolicy) -> Result<Selected, String> {
    let selected = Packet::parse(bytes)?.select(case)?;
    let raw_fixture = selected.fixture;
    let mode = policy.select(case, &raw_fixture)?;
    lower_selected(
        selected.definitions,
        selected.metadata,
        selected.vocabulary,
        raw_fixture,
        mode,
    )
}
fn lower_selected(
    definitions: BTreeMap<String, Value>,
    packet_metadata: Value,
    vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    raw_fixture: Value,
    mode: SeedMode,
) -> Result<Selected, String> {
    let fixture: Fixture =
        serde_json::from_value(raw_fixture.clone()).map_err(|error| error.to_string())?;
    if !fixture.stop.no_extra_pass
        || fixture.stop.after_action_ordinal != fixture.actions.len()
        || fixture.engine_recipes.native.action_ordinals
            != (1..=fixture.actions.len()).collect::<Vec<_>>()
    {
        return Err("inconsistent finite root action/stop declaration".into());
    }
    let mut names = BTreeSet::new();
    for binding in &fixture.bindings {
        if !names.insert(&binding.name) {
            return Err("duplicate runtime binding name".into());
        }
        let valid = match &binding.join {
            Join::TokenCreation(_) => binding.when == "matching_actual_token_created",
            Join::BirthDiscovery { require_unique, .. } => {
                *require_unique
                    && binding.when == "first_matching_actual_birth_discovery_in_this_recipe"
            }
            Join::Seed {
                require_exact_current_incarnation,
                ..
            } => {
                *require_exact_current_incarnation
                    && matches!(
                        binding.when.as_str(),
                        "after_seed_before_any_turn_work"
                            | "actual_seed_receipt_or_unchanged_initialization_object"
                    )
            }
            Join::Discovery {
                related_object_ref,
                step,
                require_unique,
                ..
            } => {
                *require_unique
                    && related_object_ref.is_some() != step.is_some()
                    && binding.when == "first_matching_actual_discovery_in_this_recipe"
            }
            Join::PermanentEntry {
                field,
                require_unique,
                ..
            } => {
                *require_unique
                    && matches!(field.as_str(), "before.id" | "after.id")
                    && binding.when == "after_first_permanent_resolution"
            }
            Join::TriggerPlacement {
                field,
                require_unique,
                ..
            } => {
                *require_unique
                    && field == "entry"
                    && binding.when == "matching_actual_trigger_placement"
            }
            Join::CopyCreation {
                field,
                require_unique,
                instruction,
                ..
            } => {
                *require_unique
                    && field == "entry"
                    && binding.when == "matching_actual_copy_created"
                    && canonical_usize(instruction).is_ok()
            }
            Join::DamageEffectCreation {
                field,
                require_unique,
                instruction,
                effect_kind,
                ..
            } => {
                *require_unique
                    && field == "effect"
                    && binding.when == "matching_actual_damage_effect_creation"
                    && canonical_usize(instruction).is_ok()
                    && matches!(effect_kind.as_str(), "Prevention" | "Multiplier")
            }
            Join::ResolutionExit {
                resolution_entry_ref,
                field,
                require_unique,
                ..
            } => {
                *require_unique
                    && field == "after.id"
                    && binding.when == format!("after_{resolution_entry_ref}_resolution")
            }
        };
        if !valid {
            return Err("unsupported literal binding contract".into());
        }
    }
    for choice in &fixture.choice_tables {
        if choice.context != "ManaColor"
            || !matches!(
                choice.source,
                Reference::Current { .. } | Reference::Captured { .. }
            )
        {
            return Err("mana choice needs a current/captured object source reference".into());
        }
    }
    let aliases = AliasContext::from_seed(&fixture.seed)?;
    Ok(Selected {
        fixture,
        raw_fixture,
        definitions,
        packet_metadata,
        aliases,
        vocabulary,
        mode,
    })
}

impl Selected {
    pub fn prepare(&self) -> Result<(GameId, GameContext, GameSeed), String> {
        match (
            &self.fixture.seed.initial_boundary,
            self.fixture.scope.as_str(),
        ) {
            (Initial::Interaction {}, "root_unscheduled")
            | (Initial::Scheduled { .. }, "root_unscheduled" | "root_scheduled")
            | (Initial::Opening { hand_size: 7, .. }, "root_opening") => {}
            _ => return Err("scope and exact initializer disagree".into()),
        }
        let prepared = self.fixture.seed.prepare(
            &self.definitions,
            self.mode,
            DefinitionPolicy::ValidateSelected,
            &self.vocabulary,
        )?;
        Ok((prepared.game, prepared.context, prepared.seed))
    }
    pub fn initialize(&self) -> Result<Result<NativeCandidate, GameError>, String> {
        Ok(self.initializer()?.execute())
    }
    pub(super) fn initializer(&self) -> Result<Initializer, String> {
        let (game, context, seed) = self.prepare()?;
        Ok(match &self.fixture.seed.initial_boundary {
            Initial::Interaction {} => Initializer::Interaction {
                game,
                context,
                seed,
            },
            Initial::Scheduled { turn, step } => Initializer::Scheduled {
                game,
                context,
                seed,
                start: TurnStart {
                    number: *turn,
                    step: program::step(step)?,
                },
            },
            Initial::Opening {
                hand_size: 7,
                entropy_words,
            } => Initializer::Opening {
                game,
                context,
                setup: OpeningSetup {
                    seed,
                    entropy: EntropyTape::new(entropy_words.clone()),
                },
                words: entropy_words.clone(),
            },
            _ => return Err("scope and exact initializer disagree".into()),
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Initializer {
    Interaction {
        game: GameId,
        context: GameContext,
        seed: GameSeed,
    },
    Scheduled {
        game: GameId,
        context: GameContext,
        seed: GameSeed,
        start: TurnStart,
    },
    Opening {
        game: GameId,
        context: GameContext,
        setup: OpeningSetup,
        words: Vec<u64>,
    },
}
impl Initializer {
    pub(super) fn execute(&self) -> Result<NativeCandidate, GameError> {
        match self {
            Self::Interaction {
                game,
                context,
                seed,
            } => crate::current_capture::initialize(*game, context.clone(), seed.clone()),
            Self::Scheduled {
                game,
                context,
                seed,
                start,
            } => crate::current_capture::initialize_scheduled(
                *game,
                context.clone(),
                seed.clone(),
                *start,
            ),
            Self::Opening {
                game,
                context,
                setup,
                words,
            } => crate::current_capture::initialize_opening(
                *game,
                context.clone(),
                setup.clone(),
                words.clone(),
            ),
        }
    }
    pub(super) fn seed(&self) -> &GameSeed {
        match self {
            Self::Interaction { seed, .. } | Self::Scheduled { seed, .. } => seed,
            Self::Opening { setup, .. } => &setup.seed,
        }
    }
    pub(super) fn context(&self) -> &GameContext {
        match self {
            Self::Interaction { context, .. }
            | Self::Scheduled { context, .. }
            | Self::Opening { context, .. } => context,
        }
    }
    pub(super) fn call(&self) -> crate::projection::QualificationCall<'_> {
        use crate::projection::QualificationCall;
        match self {
            Self::Interaction {
                game,
                context,
                seed,
            } => QualificationCall::GameInitialize {
                game: *game,
                context,
                seed,
            },
            Self::Scheduled {
                game,
                context,
                seed,
                start,
            } => QualificationCall::GameInitializeScheduled {
                game: *game,
                context,
                seed,
                start: *start,
            },
            Self::Opening {
                game,
                context,
                setup,
                words,
            } => QualificationCall::GameInitializeOpening {
                game: *game,
                context,
                setup,
                entropy_words: words,
            },
        }
    }
}

pub(super) fn canonical_usize(raw: &str) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|_| "expected canonical unsigned usize".to_owned())?;
    if value.to_string() != raw {
        return Err("expected canonical unsigned usize".into());
    }
    Ok(value)
}

/// Explicit synthetic unit admission, absent from production callers and stage policies.
#[cfg(test)]
pub(super) fn parse_synthetic(
    bytes: &[u8],
    case: &str,
    mode: SeedMode,
) -> Result<Selected, String> {
    let selected = Packet::parse(bytes)?.select(case)?;
    if !matches!(
        selected.fixture["scope"].as_str(),
        Some("root_scheduled" | "root_unscheduled" | "root_opening")
    ) {
        return Err("unsupported_scope: synthetic unit has nonroot scope".into());
    }
    lower_selected(
        selected.definitions,
        selected.metadata,
        selected.vocabulary,
        selected.fixture,
        mode,
    )
}
