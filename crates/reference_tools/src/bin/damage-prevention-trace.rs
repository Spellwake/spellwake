//! Actual native observations for synthetic creature damage, finite shields and multipliers.
//! Reference expectations are never loaded; durations and full cards are excluded.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    characteristics::{PowerToughness, PowerToughnessOperation},
    damage::{DamageEffectCreation, DamageEffectId, DamageJournal, DamageOutcome},
    effects::{EffectError, Instruction, Program, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
};
use std::{collections::BTreeMap, process::ExitCode};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const WARDEN: CardId = CardId(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum TraceProfile {
    Prevention,
    Multipliers,
    Sequences,
}
impl TraceProfile {
    fn multipliers(self) -> bool {
        self == Self::Multipliers
    }
    fn sequences(self) -> bool {
        self == Self::Sequences
    }
    fn detailed(self) -> bool {
        self != Self::Prevention
    }
}

#[derive(Serialize)]
struct Observation {
    #[serde(skip_serializing_if = "Option::is_none")]
    resolving: Option<bool>,
    checkpoint: &'static str,
    power: Option<RulesInteger>,
    toughness: Option<RulesInteger>,
    zone: &'static str,
    damage: Option<RulesInteger>,
    owner: &'static str,
    controller: Option<&'static str>,
    stack_size: usize,
    source_zones: BTreeMap<&'static str, &'static str>,
    shield_remaining: BTreeMap<&'static str, RulesInteger>,
}

#[derive(Serialize)]
struct SourceDescription {
    alias: &'static str,
    owner_and_caster: &'static str,
    cost: &'static str,
    instruction: String,
    scripted_prevention_order: &'static [&'static str],
}

#[derive(Serialize)]
struct AttemptView {
    attempt: String,
    source: String,
    target: String,
    amount: RulesInteger,
}

#[derive(Serialize)]
struct PreventionView {
    attempt: String,
    shield: String,
    shield_source: String,
    target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<u64>,
    amount: RulesInteger,
    damage_remaining: RulesInteger,
    shield_remaining: RulesInteger,
}

#[derive(Serialize)]
struct MultiplicationView {
    attempt: String,
    multiplier: String,
    multiplier_source: String,
    target: String,
    stage: u64,
    before: RulesInteger,
    factor: RulesInteger,
    after: RulesInteger,
}

#[derive(Serialize)]
struct ApplicationView {
    source: &'static str,
    effect: &'static str,
    kind: &'static str,
    attempt: String,
    stage: u64,
    before: RulesInteger,
    after: RulesInteger,
}

#[derive(Serialize)]
struct CompletionView {
    attempt: String,
    outcome: &'static str,
    dealt: Option<RulesInteger>,
}

#[derive(Serialize)]
struct CreationView {
    effect: &'static str,
    kind: &'static str,
    identity: String,
    source: String,
    instruction: usize,
    target: String,
    amount: RulesInteger,
}

#[derive(Serialize)]
struct InputJournal {
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<Vec<CreationView>>,
    input: &'static str,
    actor: Option<&'static str>,
    source: Option<&'static str>,
    game_revision: u64,
    attempts: Vec<AttemptView>,
    preventions: Vec<PreventionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    multiplications: Option<Vec<MultiplicationView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applications: Option<Vec<ApplicationView>>,
    applied: Vec<AttemptView>,
    completions: Vec<CompletionView>,
}

impl InputJournal {
    fn capture(
        input: &'static str,
        actor: Option<SeatId>,
        source: Option<&'static str>,
        state: &GameState,
        journal: &DamageJournal,
        effects: &BTreeMap<&'static str, TrackedEffect>,
        profile: TraceProfile,
    ) -> Result<Self, GameError> {
        let mut applications = Vec::new();
        if profile.detailed() {
            for event in &journal.preventions {
                applications.push((
                    event.attempt,
                    ApplicationView {
                        source: source.ok_or_else(inconsistent)?,
                        effect: effect_alias(effects, event.shield.into())?,
                        kind: "prevention",
                        attempt: format!("{:?}", event.attempt),
                        stage: event.stage,
                        before: event.damage_remaining.plus(&event.amount),
                        after: event.damage_remaining.clone(),
                    },
                ));
            }
            for event in &journal.multiplications {
                applications.push((
                    event.attempt,
                    ApplicationView {
                        source: source.ok_or_else(inconsistent)?,
                        effect: effect_alias(effects, event.multiplier.into())?,
                        kind: "multiply",
                        attempt: format!("{:?}", event.attempt),
                        stage: event.stage,
                        before: event.before.clone(),
                        after: event.after.clone(),
                    },
                ));
            }
            applications.sort_by_key(|(attempt, event)| (*attempt, event.stage));
        }
        Ok(Self {
            created: profile
                .sequences()
                .then(|| {
                    journal
                        .created
                        .iter()
                        .map(|creation| {
                            let (id, source, target, instruction, kind, amount) =
                                creation_parts(creation);
                            Ok(CreationView {
                                effect: effect_alias(effects, id)?,
                                kind,
                                identity: format!("{id:?}"),
                                source: format!("{source:?}"),
                                instruction,
                                target: format!("{target:?}"),
                                amount,
                            })
                        })
                        .collect::<Result<Vec<_>, GameError>>()
                })
                .transpose()?,
            input,
            actor: actor.map(seat_name).transpose()?,
            source,
            game_revision: state.basis().revision,
            attempts: journal
                .attempts
                .iter()
                .map(|event| AttemptView {
                    attempt: format!("{:?}", event.id),
                    source: format!("{:?}", event.source),
                    target: format!("{:?}", event.target),
                    amount: event.amount.clone(),
                })
                .collect(),
            preventions: journal
                .preventions
                .iter()
                .map(|event| PreventionView {
                    attempt: format!("{:?}", event.attempt),
                    shield: format!("{:?}", event.shield),
                    shield_source: format!("{:?}", event.shield_source),
                    target: format!("{:?}", event.target),
                    stage: profile.detailed().then_some(event.stage),
                    amount: event.amount.clone(),
                    damage_remaining: event.damage_remaining.clone(),
                    shield_remaining: event.shield_remaining.clone(),
                })
                .collect(),
            multiplications: profile.detailed().then(|| {
                journal
                    .multiplications
                    .iter()
                    .map(|event| MultiplicationView {
                        attempt: format!("{:?}", event.attempt),
                        multiplier: format!("{:?}", event.multiplier),
                        multiplier_source: format!("{:?}", event.multiplier_source),
                        target: format!("{:?}", event.target),
                        stage: event.stage,
                        before: event.before.clone(),
                        factor: event.factor.clone(),
                        after: event.after.clone(),
                    })
                    .collect()
            }),
            applications: profile
                .detailed()
                .then(|| applications.into_iter().map(|(_, event)| event).collect()),
            applied: journal
                .applied
                .iter()
                .map(|event| AttemptView {
                    attempt: format!("{:?}", event.attempt),
                    source: format!("{:?}", event.source),
                    target: format!("{:?}", event.target),
                    amount: event.amount.clone(),
                })
                .collect(),
            completions: journal
                .completions
                .iter()
                .map(|event| {
                    let (outcome, dealt) = match &event.outcome {
                        DamageOutcome::Dealt(amount) => ("dealt", Some(amount.clone())),
                        DamageOutcome::FullyPrevented => ("fully-prevented", None),
                    };
                    CompletionView {
                        attempt: format!("{:?}", event.attempt),
                        outcome,
                        dealt,
                    }
                })
                .collect(),
        })
    }
}

#[derive(Serialize)]
struct NativeDecision {
    #[serde(skip_serializing_if = "Option::is_none")]
    instruction: Option<usize>,
    source: &'static str,
    domain: Vec<&'static str>,
    chosen: &'static str,
    damage_before: RulesInteger,
    pending_damage: RulesInteger,
    chooser: &'static str,
    target: &'static str,
    source_controller: &'static str,
    stack_size: usize,
    source_zone: &'static str,
    decision_id: String,
    attempt_id: String,
    attempt_stage: u64,
    game_revision: u64,
    executor_revision: u64,
}

#[derive(Serialize)]
struct ShieldView {
    source: &'static str,
    shield_id: String,
    source_entry: String,
    target: String,
    active: bool,
    stored_remaining: Option<RulesInteger>,
}

#[derive(Serialize)]
struct MultiplierView {
    source: &'static str,
    multiplier_id: String,
    source_entry: String,
    target: String,
    active: bool,
    factor: Option<RulesInteger>,
}

#[derive(Serialize)]
struct NativeCheckpoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    subject_object: Option<String>,
    checkpoint: &'static str,
    game_revision: u64,
    executor_revision: u64,
    shields: Vec<ShieldView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    multipliers: Option<Vec<MultiplierView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_source_zones: Option<BTreeMap<&'static str, &'static str>>,
}

#[derive(Default, Serialize)]
struct NativeEvidence {
    source_entries: BTreeMap<&'static str, String>,
    decisions: Vec<NativeDecision>,
    input_journals: Vec<InputJournal>,
    checkpoints: Vec<NativeCheckpoint>,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: &'static str,
    native_driver: &'static str,
    program_source: &'static str,
    numeric_encoding: &'static str,
    scope: &'static str,
    decision_scope: &'static str,
    shield_projection: &'static str,
    subject: &'static str,
    subject_owner_and_controller: &'static str,
    full_game_comparisons: u32,
    fully_qualified_cards: u32,
    catalogue_bindings: u32,
    scenario_programs: BTreeMap<&'static str, Vec<SourceDescription>>,
    scenario_observations: BTreeMap<&'static str, Vec<Observation>>,
    native_only_evidence: BTreeMap<&'static str, NativeEvidence>,
}

struct Source {
    card: CardId,
    alias: &'static str,
    checkpoint: &'static str,
    instructions: Vec<Instruction>,
    creation_aliases: Vec<(usize, &'static str)>,
    prepared: bool,
    choices: &'static [&'static str],
}

struct TrackedEffect {
    instruction: usize,
    id: DamageEffectId,
    source: StackEntryId,
    target: CardObjectId,
}

struct ScenarioResult {
    observations: Vec<Observation>,
    programs: Vec<SourceDescription>,
    evidence: NativeEvidence,
}

fn inconsistent() -> GameError {
    EffectError::InconsistentProgram.into()
}

fn seat_name(seat: SeatId) -> Result<&'static str, GameError> {
    match seat {
        A => Ok("A"),
        B => Ok("B"),
        _ => Err(GameError::UnknownActor),
    }
}

fn identity(state: &GameState, card: CardId) -> Result<CardObjectId, GameError> {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .ok_or(EffectError::UnknownCard.into())
}

fn zone(state: &GameState, card: CardId) -> Result<CardZone, GameError> {
    state
        .execution()
        .window()
        .cards()
        .get(identity(state, card)?)
        .map(|object| object.zone())
        .ok_or(EffectError::UnknownCard.into())
}

fn zone_name(zone: CardZone) -> &'static str {
    match zone {
        CardZone::Library(_) => "LIBRARY",
        CardZone::Hand(_) => "HAND",
        CardZone::Graveyard(_) => "GRAVEYARD",
        CardZone::Battlefield => "BATTLEFIELD",
        CardZone::Stack => "STACK",
        CardZone::Exile => "EXILE",
        CardZone::Command => "COMMAND",
    }
}

fn observe(
    state: &GameState,
    checkpoint: &'static str,
    sources: &[Source],
    effects: &BTreeMap<&'static str, TrackedEffect>,
    evidence: &mut NativeEvidence,
    profile: TraceProfile,
) -> Result<Observation, GameError> {
    if state.pending_decision().is_some() || state.execution().pending_damage().is_some() {
        return Err(GameError::DecisionRequired);
    }
    let object = identity(state, WARDEN)?;
    let execution = state.execution();
    let card = execution
        .window()
        .cards()
        .get(object)
        .ok_or_else(inconsistent)?;
    let creature = execution.creature(object);
    let raw_source_zones: BTreeMap<_, _> = sources
        .iter()
        .map(|source| Ok((source.alias, zone_name(zone(state, source.card)?))))
        .collect::<Result<_, GameError>>()?;
    let source_zones = raw_source_zones
        .iter()
        .filter(|(alias, _)| {
            !profile.detailed()
                || !sources
                    .iter()
                    .any(|source| source.alias == **alias && is_multiplier(source))
        })
        .map(|(&alias, &zone)| (alias, zone))
        .collect();
    let mut shield_remaining = BTreeMap::new();
    let mut instances = Vec::new();
    let mut multiplier_instances = Vec::new();
    for (&alias, tracked) in effects {
        let id = match tracked.id {
            DamageEffectId::Prevention(id) => id,
            DamageEffectId::Multiplier(id) => {
                let active = execution.damage_multiplier(id);
                multiplier_instances.push(MultiplierView {
                    source: alias,
                    multiplier_id: format!("{id:?}"),
                    source_entry: format!("{:?}", tracked.source),
                    target: format!("{:?}", tracked.target),
                    active: active.is_some(),
                    factor: active.map(|effect| effect.factor.clone()),
                });
                continue;
            }
        };
        let active = execution.prevention_shield(id);
        // Absent observed instances have zero active protection, without inferred subtraction.
        shield_remaining.insert(
            alias,
            active.map_or_else(|| 0.into(), |shield| shield.remaining.clone()),
        );
        instances.push(ShieldView {
            source: alias,
            shield_id: format!("{id:?}"),
            source_entry: format!("{:?}", tracked.source),
            target: format!("{:?}", tracked.target),
            active: active.is_some(),
            stored_remaining: active.map(|shield| shield.remaining.clone()),
        });
    }
    evidence.checkpoints.push(NativeCheckpoint {
        subject_object: profile.sequences().then(|| format!("{object:?}")),
        checkpoint,
        game_revision: state.basis().revision,
        executor_revision: execution.revision(),
        shields: instances,
        multipliers: profile.detailed().then_some(multiplier_instances),
        raw_source_zones: profile.detailed().then_some(raw_source_zones),
    });
    Ok(Observation {
        resolving: profile
            .sequences()
            .then_some(execution.window().resolving().is_some()),
        checkpoint,
        power: creature.as_ref().map(|creature| creature.power.clone()),
        toughness: creature.as_ref().map(|creature| creature.toughness.clone()),
        zone: zone_name(card.zone()),
        damage: creature.map(|creature| creature.damage),
        owner: seat_name(card.owner())?,
        controller: card.controller().map(seat_name).transpose()?,
        stack_size: execution.window().entries().len(),
        source_zones,
        shield_remaining,
    })
}

fn advance(
    state: GameState,
    actor: SeatId,
    command: GameCommand,
    input_and_source: (&'static str, &Source),
    evidence: &mut NativeEvidence,
    effects: &mut BTreeMap<&'static str, TrackedEffect>,
    profile: TraceProfile,
) -> Result<GameState, GameError> {
    let candidate = state.advance(GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    })?;
    register_creations(
        candidate.damage_journal(),
        input_and_source.1,
        identity(&state, WARDEN)?,
        evidence,
        effects,
    )?;
    evidence.input_journals.push(InputJournal::capture(
        input_and_source.0,
        Some(actor),
        Some(input_and_source.1.alias),
        candidate.state(),
        candidate.damage_journal(),
        effects,
        profile,
    )?);
    Ok(candidate.into_state())
}

fn resolve_source(
    mut state: GameState,
    source: &Source,
    effects: &mut BTreeMap<&'static str, TrackedEffect>,
    evidence: &mut NativeEvidence,
    profile: TraceProfile,
) -> Result<GameState, GameError> {
    let spell = identity(&state, source.card)?;
    let target = TargetRef::Card(identity(&state, WARDEN)?);
    state = advance(
        state,
        A,
        GameCommand::Cast { spell, target },
        ("cast", source),
        evidence,
        effects,
        profile,
    )?;
    let entry = state
        .execution()
        .window()
        .entries()
        .last()
        .ok_or_else(inconsistent)?;
    if entry.kind() != &StackKind::CardSpell(identity(&state, source.card)?) {
        return Err(inconsistent());
    }
    let entry_id = entry.id();
    evidence
        .source_entries
        .insert(source.alias, format!("{entry_id:?}"));
    let mut chosen_count = 0;
    let input_limit = state.execution().window().participants().len() + source.choices.len();
    for _ in 0..input_limit {
        if let Some(pending) = state.pending_decision() {
            let RuleFrame::DamageOrder(frame) = pending.frame() else {
                return Err(inconsistent());
            };
            let active = state
                .execution()
                .pending_damage()
                .ok_or_else(inconsistent)?;
            let resolving = state
                .execution()
                .window()
                .resolving()
                .ok_or_else(inconsistent)?;
            if pending.chooser() != B
                || frame.source() != entry_id
                || resolving.id() != entry_id
                || active.attempt().source != entry_id
                || active.attempt().id != frame.attempt()
                || active.attempt().target != identity(&state, WARDEN)?
                || state.execution().instruction_index() != Some(frame.instruction_index())
            {
                return Err(inconsistent());
            }
            let alias = *source.choices.get(chosen_count).ok_or_else(inconsistent)?;
            let selected = effects.get(alias).ok_or_else(inconsistent)?.id;
            if !frame.options().contains(&selected) {
                return Err(inconsistent());
            }
            let mut domain = frame
                .options()
                .iter()
                .map(|id| effect_alias(effects, *id))
                .collect::<Result<Vec<_>, _>>()?;
            domain.sort_unstable();
            evidence.decisions.push(NativeDecision {
                instruction: profile.sequences().then_some(frame.instruction_index()),
                source: source.alias,
                domain,
                chosen: alias,
                damage_before: state
                    .execution()
                    .creature(active.attempt().target)
                    .ok_or_else(inconsistent)?
                    .damage,
                pending_damage: active.remaining().clone(),
                chooser: seat_name(pending.chooser())?,
                target: "warden",
                source_controller: seat_name(resolving.controller())?,
                stack_size: state.execution().window().entries().len(),
                source_zone: zone_name(zone(&state, source.card)?),
                decision_id: format!("{:?}", pending.id()),
                attempt_id: format!("{:?}", active.attempt().id),
                attempt_stage: active.stage(),
                game_revision: state.basis().revision,
                executor_revision: state.execution().revision(),
            });
            let command = GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::DamageOrder(selected),
            };
            state = advance(
                state,
                B,
                command,
                ("prevention-reply", source),
                evidence,
                effects,
                profile,
            )?;
            chosen_count += 1;
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                return Err(GameError::UnexpectedKernelPhase);
            };
            state = advance(
                state,
                holder,
                GameCommand::Pass,
                ("pass", source),
                evidence,
                effects,
                profile,
            )?;
        }
        if state
            .execution()
            .window()
            .entries()
            .iter()
            .all(|entry| entry.id() != entry_id)
        {
            if chosen_count != source.choices.len()
                || zone(&state, source.card)? != CardZone::Graveyard(A)
            {
                return Err(inconsistent());
            }
            return Ok(state);
        }
    }
    Err(GameError::UnexpectedKernelPhase)
}

fn is_multiplier(source: &Source) -> bool {
    matches!(
        source.instructions.as_slice(),
        [Instruction::CreateDamageMultiplier { .. }]
    )
}

fn effect_alias(
    effects: &BTreeMap<&'static str, TrackedEffect>,
    id: DamageEffectId,
) -> Result<&'static str, GameError> {
    effects
        .iter()
        .find(|(_, effect)| effect.id == id)
        .map(|(&alias, _)| alias)
        .ok_or_else(inconsistent)
}

fn creation_parts(
    creation: &DamageEffectCreation,
) -> (
    DamageEffectId,
    StackEntryId,
    CardObjectId,
    usize,
    &'static str,
    RulesInteger,
) {
    match creation {
        DamageEffectCreation::Prevention {
            shield,
            instruction,
        } => (
            shield.id.into(),
            shield.source,
            shield.target,
            *instruction,
            "prevention",
            shield.remaining.clone(),
        ),
        DamageEffectCreation::Multiplier {
            multiplier,
            instruction,
        } => (
            multiplier.id.into(),
            multiplier.source,
            multiplier.target,
            *instruction,
            "multiply",
            multiplier.factor.clone(),
        ),
    }
}

/// Register immutable birth identities before any same-input application is projected.
/// An exhausted/pruned effect need not remain in the returned active registry.
fn register_creations(
    journal: &DamageJournal,
    source: &Source,
    target: CardObjectId,
    evidence: &NativeEvidence,
    effects: &mut BTreeMap<&'static str, TrackedEffect>,
) -> Result<(), GameError> {
    for creation in &journal.created {
        let (id, creator, actual_target, instruction, _, _) = creation_parts(creation);
        let alias = source
            .creation_aliases
            .iter()
            .find(|(index, _)| *index == instruction)
            .map(|(_, alias)| *alias)
            .ok_or_else(inconsistent)?;
        if evidence.source_entries.get(source.alias) != Some(&format!("{creator:?}"))
            || actual_target != target
            || effects.values().any(|effect| {
                effect.id == id || (effect.source == creator && effect.instruction == instruction)
            })
            || effects
                .insert(
                    alias,
                    TrackedEffect {
                        id,
                        source: creator,
                        target,
                        instruction,
                    },
                )
                .is_some()
        {
            return Err(inconsistent());
        }
    }
    Ok(())
}

fn run_scenario(
    game: GameId,
    scenario: &'static str,
    sources: Vec<Source>,
    profile: TraceProfile,
) -> Result<ScenarioResult, GameError> {
    let mut cards = vec![SeedCard {
        card: WARDEN,
        owner: B,
        destination: CardDestination::Battlefield { controller: B },
        program: Program::VanillaCreature {
            power: 1.into(),
            toughness: 3.into(),
        },
    }];
    let mut mana: RulesInteger = 0.into();
    for source in &sources {
        cards.push(SeedCard {
            card: source.card,
            owner: A,
            destination: CardDestination::Hand,
            program: Program::Instant {
                cost: ManaCost::one(ManaType::Red),
                instructions: source.instructions.clone(),
            },
        });
        mana = mana.plus(&1.into());
    }
    let context = GameContext::new(
        if profile.sequences() {
            "native-ordered-resolution-v1"
        } else if profile.multipliers() {
            "native-damage-multiplication-v1"
        } else {
            "native-damage-prevention-v1"
        }
        .into(),
        if profile.sequences() {
            "magic-cr-20260925-ordered-primitive-resolution-subset"
        } else if profile.multipliers() {
            "magic-cr-20260925-literal-creature-damage-modifiers-subset"
        } else {
            "magic-cr-20260925-finite-creature-prevention-subset"
        }
        .into(),
        "no-catalogue-synthetic-seeds".into(),
        if profile.sequences() {
            "synthetic-ordered-resolution-programs-v1"
        } else if profile.multipliers() {
            "synthetic-literal-damage-multiplication-v1"
        } else {
            "synthetic-literal-damage-prevention-v1"
        }
        .into(),
        scenario.into(),
    )?;
    let candidate = GameState::initialize(
        game,
        context,
        vec![A, B],
        A,
        cards,
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::Red,
            amount: mana,
        }],
    )?;
    let mut evidence = NativeEvidence::default();
    let mut effects = BTreeMap::new();
    evidence.input_journals.push(InputJournal::capture(
        "initialize",
        None,
        None,
        candidate.state(),
        candidate.damage_journal(),
        &effects,
        profile,
    )?);
    let mut state = candidate.into_state();
    for source in sources.iter().filter(|source| source.prepared) {
        state = resolve_source(state, source, &mut effects, &mut evidence, profile)?;
    }
    let mut observations = vec![observe(
        &state,
        "initial",
        &sources,
        &effects,
        &mut evidence,
        profile,
    )?];
    for source in sources.iter().filter(|source| !source.prepared) {
        state = resolve_source(state, source, &mut effects, &mut evidence, profile)?;
        observations.push(observe(
            &state,
            source.checkpoint,
            &sources,
            &effects,
            &mut evidence,
            profile,
        )?);
    }
    let programs = sources
        .iter()
        .map(|source| SourceDescription {
            alias: source.alias,
            owner_and_caster: "A",
            cost: "one red mana, synthetic homogeneous cost; A seeded one per source",
            instruction: match source.instructions.as_slice() {
                [single] => format!("{single:?}"),
                sequence => format!("{sequence:?}"),
            },
            scripted_prevention_order: source.choices,
        })
        .collect();
    Ok(ScenarioResult {
        observations,
        programs,
        evidence,
    })
}

fn shield(card: u64, alias: &'static str, checkpoint: &'static str, amount: i64) -> Source {
    Source {
        card: CardId(card),
        alias,
        checkpoint,
        instructions: vec![Instruction::CreatePreventionShield {
            amount: amount.into(),
        }],
        creation_aliases: vec![(0, alias)],
        prepared: false,
        choices: &[],
    }
}

fn damage(
    card: u64,
    alias: &'static str,
    checkpoint: &'static str,
    amount: i64,
    choices: &'static [&'static str],
) -> Source {
    Source {
        card: CardId(card),
        alias,
        checkpoint,
        instructions: vec![Instruction::Damage {
            amount: amount.into(),
        }],
        creation_aliases: vec![],
        prepared: false,
        choices,
    }
}

fn multiplier(card: u64, alias: &'static str) -> Source {
    Source {
        card: CardId(card),
        alias,
        checkpoint: "prepared-multiplier",
        instructions: vec![Instruction::CreateDamageMultiplier { factor: 2.into() }],
        creation_aliases: vec![(0, alias)],
        prepared: true,
        choices: &[],
    }
}

fn sequence(card: u64, instructions: Vec<Instruction>, choices: &'static [&'static str]) -> Source {
    Source {
        card: CardId(card),
        alias: "probe",
        checkpoint: "settled",
        instructions,
        creation_aliases: vec![(0, "probe")],
        prepared: false,
        choices,
    }
}

fn sequence_scenarios() -> Vec<(&'static str, Vec<Source>)> {
    let mut blessing = shield(2, "blessing", "prepared-blessing", 3);
    blessing.prepared = true;
    let mut mending = shield(3, "mending", "prepared-mending", 4);
    mending.prepared = true;
    vec![
        (
            "zero-then-recover",
            vec![sequence(
                2,
                vec![
                    Instruction::ContinuousPowerToughness {
                        operation: PowerToughnessOperation::Set(PowerToughness {
                            power: 1.into(),
                            toughness: 0.into(),
                        }),
                    },
                    Instruction::ContinuousPowerToughness {
                        operation: PowerToughnessOperation::Modify(PowerToughness {
                            power: 0.into(),
                            toughness: 2.into(),
                        }),
                    },
                ],
                &[],
            )],
        ),
        (
            "lethal-then-bounce",
            vec![sequence(
                2,
                vec![
                    Instruction::Damage { amount: 3.into() },
                    Instruction::ReturnToOwnersHand,
                ],
                &[],
            )],
        ),
        (
            "shield-then-damage",
            vec![sequence(
                2,
                vec![
                    Instruction::CreatePreventionShield { amount: 4.into() },
                    Instruction::Damage { amount: 3.into() },
                ],
                &[],
            )],
        ),
        (
            "sequential-damage-ordering",
            vec![
                blessing,
                mending,
                sequence(
                    4,
                    vec![
                        Instruction::Damage { amount: 5.into() },
                        Instruction::Damage { amount: 3.into() },
                    ],
                    &["blessing"],
                ),
            ],
        ),
        (
            "bounce-then-damage",
            vec![sequence(
                2,
                vec![
                    Instruction::ReturnToOwnersHand,
                    Instruction::Damage { amount: 3.into() },
                ],
                &[],
            )],
        ),
    ]
}

fn run(profile: TraceProfile) -> Result<Report, GameError> {
    let scenarios = if profile.sequences() {
        sequence_scenarios()
    } else if profile.multipliers() {
        vec![
            (
                "multiplier-first",
                vec![
                    multiplier(2, "furnace"),
                    shield(3, "mending", "after-mending", 4),
                    damage(4, "bolt", "after-bolt", 3, &["furnace"]),
                ],
            ),
            (
                "prevention-first",
                vec![
                    multiplier(2, "furnace"),
                    shield(3, "mending", "after-mending", 4),
                    damage(4, "bolt", "after-bolt", 3, &["mending"]),
                ],
            ),
            (
                "two-doublers",
                vec![
                    multiplier(2, "furnace-a"),
                    multiplier(3, "furnace-b"),
                    damage(4, "bolt", "after-bolt", 3, &["furnace-a"]),
                ],
            ),
            (
                "mixed-recomputation",
                vec![
                    multiplier(2, "furnace-a"),
                    multiplier(3, "furnace-b"),
                    shield(4, "mending", "after-mending", 4),
                    damage(5, "impact", "after-impact", 5, &["furnace-a", "mending"]),
                ],
            ),
            (
                "persistent-reuse",
                vec![
                    multiplier(2, "furnace"),
                    shield(3, "mending", "after-mending", 4),
                    damage(4, "bolt-one", "after-bolt-one", 3, &["furnace"]),
                    damage(5, "bolt-two", "after-bolt-two", 3, &[]),
                ],
            ),
        ]
    } else {
        vec![
            (
                "single-shield-reuse",
                vec![
                    shield(2, "mending", "after-mending", 4),
                    damage(3, "bolt-one", "after-bolt-one", 3, &[]),
                    damage(4, "bolt-two", "after-bolt-two", 3, &[]),
                ],
            ),
            (
                "mending-first",
                vec![
                    shield(2, "mending", "after-mending", 4),
                    shield(3, "blessing", "after-blessing", 3),
                    damage(4, "bolt", "after-bolt", 3, &["mending"]),
                ],
            ),
            (
                "blessing-first",
                vec![
                    shield(2, "mending", "after-mending", 4),
                    shield(3, "blessing", "after-blessing", 3),
                    damage(4, "bolt", "after-bolt", 3, &["blessing"]),
                ],
            ),
            (
                "repeated-choice",
                vec![
                    shield(2, "mending", "after-mending", 4),
                    shield(3, "blessing-a", "after-blessing-a", 3),
                    shield(4, "blessing-b", "after-blessing-b", 3),
                    damage(
                        5,
                        "impact",
                        "after-impact",
                        5,
                        &["blessing-a", "blessing-b"],
                    ),
                ],
            ),
        ]
    };
    let mut scenario_programs = BTreeMap::new();
    let mut scenario_observations = BTreeMap::new();
    let mut native_only_evidence = BTreeMap::new();
    for (id, (name, sources)) in (1_u128..).zip(scenarios) {
        let result = run_scenario(GameId(id), name, sources, profile)?;
        scenario_programs.insert(name, result.programs);
        scenario_observations.insert(name, result.observations);
        native_only_evidence.insert(name, result.evidence);
    }
    Ok(Report {
        schema_version: 1,
        kind: if profile.sequences() {
            "restricted-ordered-resolution-programs"
        } else if profile.multipliers() {
            "restricted-damage-multiplication-interactions"
        } else {
            "restricted-damage-prevention-interactions"
        },
        native_driver: "game-state-initialize-advance-v1",
        program_source: "synthetic-authored-programs-v1; no Oracle registry bindings",
        numeric_encoding: "canonical decimal strings for rules integers",
        scope: if profile.sequences() {
            "Five authored ordered primitive programs; ten actual initial/settled GameState points. Native candidate journals prove attempted/applied amounts and old target identity. Forge pre-SBA/inter-instruction events remain reference-only; no native stepping API, real card-program, duration, casting/payment, full-game or performance qualification."
        } else if profile.multipliers() {
            "Resolved fixed-incarnation persistent literal multipliers and finite shields; fifteen immediate settled points and ordered application arithmetic. Synthetic creators differ from global static Furnace sources, whose source zones are excluded from common projections. No duration, casting/payment, full-card/game or performance parity."
        } else {
            "Positive literal fixed-incarnation creature shields and sequential single-source damage, initial and settled immediate checkpoints before cleanup. Synthetic homogeneous costs and seeded mana; no duration/expiry, real-card binding, complete-card/full-game or performance qualification."
        },
        decision_scope: if profile.sequences() {
            "One actual typed multiple-shield decision at instruction zero; singleton applications are automatic. Four synchronous Forge callbacks are distinct from native suspension protocol."
        } else if profile.multipliers() {
            "Six typed mixed-effect decisions; singleton modifications apply automatically. Eleven actual staged application quantities are compared separately; Forge synchronous callbacks are not native suspension protocol parity."
        } else {
            "Native-only typed prevention decisions: singleton applicable shields apply automatically. Forge's forced singleton callbacks have no native decision counterpart; shared checkpoint results do not establish protocol parity."
        },
        shield_projection: if profile.sequences() {
            "Bind actual typed birth IDs by creator entry and program instruction before projecting same-input applications, including effects consumed before return. Read active capacity or zero for an absent bound ID; raw checkpoints retain presence and exact incarnation."
        } else {
            "Bind actual shield IDs by the resolved creator stack entry. Read active remaining quantities; a previously observed ID absent from the active store projects zero active protection. Native checkpoint evidence retains presence and actual stored values separately."
        },
        subject: "synthetic vanilla creature alias warden, defined/copiable power 1 and toughness 3",
        subject_owner_and_controller: "B",
        full_game_comparisons: 0,
        fully_qualified_cards: 0,
        catalogue_bindings: 0,
        scenario_programs,
        scenario_observations,
        native_only_evidence,
    })
}

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let profile = match arguments.as_slice() {
        [] => Ok(TraceProfile::Prevention),
        [flag] if flag == "--multipliers" => Ok(TraceProfile::Multipliers),
        [flag] if flag == "--sequences" => Ok(TraceProfile::Sequences),
        _ => Err(GameError::Effect(EffectError::InvalidSetup)),
    };
    let (response, success): (ApiResponse<Report>, bool) = match profile.and_then(run) {
        Ok(data) => (ApiResponse::Success { data }, true),
        Err(error) => {
            eprintln!("native damage-prevention interaction failed: {error:?}");
            (
                ApiResponse::Error {
                    message: "Native damage-prevention interaction unavailable".into(),
                    code: "native-damage-prevention-error".into(),
                },
                false,
            )
        }
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
        eprintln!("native damage-prevention trace output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_source_creations_bind_by_index_even_when_both_are_consumed() {
        let source = Source {
            card: CardId(2),
            alias: "probe",
            checkpoint: "settled",
            instructions: vec![
                Instruction::CreatePreventionShield { amount: 1.into() },
                Instruction::CreatePreventionShield { amount: 2.into() },
                Instruction::Damage { amount: 3.into() },
            ],
            creation_aliases: vec![(0, "first"), (1, "second")],
            prepared: false,
            choices: &["first"],
        };
        let result = run_scenario(
            GameId(100),
            "consumed-creations",
            vec![source],
            TraceProfile::Sequences,
        )
        .unwrap();
        let births: Vec<_> = result
            .evidence
            .input_journals
            .iter()
            .flat_map(|journal| journal.created.as_ref().unwrap())
            .collect();
        assert_eq!(births.len(), 2);
        assert_eq!((births[0].effect, births[0].instruction), ("first", 0));
        assert_eq!((births[1].effect, births[1].instruction), ("second", 1));
        assert_eq!(births[0].source, births[1].source);
        assert_ne!(births[0].identity, births[1].identity);
        let checkpoint = result.evidence.checkpoints.last().unwrap();
        assert_eq!(checkpoint.shields.len(), 2);
        assert!(
            checkpoint
                .shields
                .iter()
                .all(|shield| !shield.active && shield.stored_remaining.is_none())
        );
        let terminal = result.evidence.input_journals.last().unwrap();
        assert!(terminal.applied.is_empty());
        assert_eq!(terminal.completions[0].outcome, "fully-prevented");
    }

    #[test]
    fn application_projection_orders_ten_real_attempts_numerically() {
        let mut instructions = vec![Instruction::CreateDamageMultiplier { factor: 1.into() }];
        instructions.extend((0..10).map(|_| Instruction::Damage { amount: 1.into() }));
        let source = Source {
            card: CardId(2),
            alias: "probe",
            checkpoint: "settled",
            instructions,
            creation_aliases: vec![(0, "scale")],
            prepared: false,
            choices: &[],
        };
        let result = run_scenario(
            GameId(101),
            "ten-attempts",
            vec![source],
            TraceProfile::Sequences,
        )
        .unwrap();
        let terminal = result.evidence.input_journals.last().unwrap();
        let applications = terminal.applications.as_ref().unwrap();
        assert_eq!(terminal.attempts.len(), 10);
        assert_eq!(applications.len(), 10);
        for (attempt, application) in terminal.attempts.iter().zip(applications) {
            assert_eq!(attempt.attempt, application.attempt);
            assert_eq!(application.stage, 1);
        }
        assert_eq!(applications[8].attempt, "DamageAttemptId(9)");
        assert_eq!(applications[9].attempt, "DamageAttemptId(10)");
    }
}
