//! Actual native observations for synthetic creature damage, finite shields and multipliers.
//! Reference expectations are never loaded; durations and full cards are excluded.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    characteristics::{PowerToughness, ResolvedPartSpec},
    damage::{
        DamageEffectCreation, DamageEffectId, DamageEffectTarget, DamageJournal, DamageOrigin,
        DamageRecipient, DamageSource, DamageSourceId,
    },
    effects::{DamageDecision, EffectError, Instruction, Program, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
};
use spellwake_reference_tools::damage_trace::{
    DamageApplicationView, DamageEffectTargetView, DamageJournalView, DamageOriginView,
    DamageSourceView, EffectIdentityView, damage_application_view, damage_effect_target_view,
    damage_effect_view, damage_journal_view, damage_origin_view, damage_source_view,
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
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

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
struct ApplicationView {
    source: &'static str,
    effect: &'static str,
    kind: &'static str,
    batch: String,
    attempt: String,
    stage: u64,
    before: RulesInteger,
    after: RulesInteger,
}

/// Fixture alias metadata joins the common typed creation record; it is not the
/// damage source or a second event authority.
#[derive(Serialize)]
struct CreationBinding {
    effect: &'static str,
    kind: &'static str,
    identity: EffectIdentityView,
    creator: String,
    instruction: usize,
    target: DamageEffectTargetView,
    amount: RulesInteger,
}

#[derive(Serialize)]
struct InputJournal {
    #[serde(skip_serializing_if = "Option::is_none")]
    creation_bindings: Option<Vec<CreationBinding>>,
    input: &'static str,
    actor: Option<&'static str>,
    source: Option<&'static str>,
    game_revision: u64,
    executor_revision: u64,
    #[serde(flatten)]
    journal: DamageJournalView,
    #[serde(skip_serializing_if = "Option::is_none")]
    applications: Option<Vec<ApplicationView>>,
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
                for allocation in &event.allocations {
                    let after = event
                        .damage_remaining
                        .get(&allocation.attempt)
                        .ok_or_else(inconsistent)?;
                    applications.push((
                        allocation.attempt,
                        ApplicationView {
                            source: source.ok_or_else(inconsistent)?,
                            effect: effect_alias(effects, event.shield.into())?,
                            kind: "prevention",
                            batch: format!("{:?}", event.batch),
                            attempt: format!("{:?}", allocation.attempt),
                            stage: event.stage,
                            before: after.plus(&allocation.amount),
                            after: after.clone(),
                        },
                    ));
                }
            }
            for event in &journal.multiplications {
                applications.push((
                    event.attempt,
                    ApplicationView {
                        source: source.ok_or_else(inconsistent)?,
                        effect: effect_alias(effects, event.multiplier.into())?,
                        kind: "multiply",
                        batch: format!("{:?}", event.batch),
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
            creation_bindings: profile
                .sequences()
                .then(|| {
                    journal
                        .created
                        .iter()
                        .map(|creation| {
                            let (id, creator, target, instruction, kind, amount) =
                                creation_parts(creation);
                            Ok(CreationBinding {
                                effect: effect_alias(effects, id)?,
                                kind,
                                identity: damage_effect_view(id),
                                creator: format!("{creator:?}"),
                                instruction,
                                target: damage_effect_target_view(target),
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
            executor_revision: state.execution().revision(),
            journal: damage_journal_view(journal),
            applications: profile
                .detailed()
                .then(|| applications.into_iter().map(|(_, event)| event).collect()),
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
    batch_id: String,
    origin: DamageOriginView,
    actual_source: DamageSourceView,
    application_domain: Vec<DamageApplicationView>,
    chosen_application: DamageApplicationView,
    attempt_id: String,
    attempt_stage: u64,
    game_revision: u64,
    executor_revision: u64,
}

#[derive(Serialize)]
struct ShieldView {
    source: &'static str,
    shield_id: String,
    creator_entry: String,
    target: DamageEffectTargetView,
    active: bool,
    stored_remaining: Option<RulesInteger>,
}

#[derive(Serialize)]
struct MultiplierView {
    source: &'static str,
    multiplier_id: String,
    creator_entry: String,
    target: DamageEffectTargetView,
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
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    source_entries: BTreeMap<&'static str, String>,
    source_objects: BTreeMap<&'static str, DamageSourceView>,
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
    creator: StackEntryId,
    target: DamageEffectTarget,
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

fn identity(state: &GameState, card: CardId) -> Result<ObjectId, GameError> {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .ok_or(EffectError::UnknownObject.into())
}

fn zone(state: &GameState, card: CardId) -> Result<ObjectZone, GameError> {
    state
        .execution()
        .window()
        .cards()
        .get(identity(state, card)?)
        .map(|object| object.zone())
        .ok_or(EffectError::UnknownObject.into())
}

fn zone_name(zone: ObjectZone) -> &'static str {
    match zone {
        ObjectZone::Library(_) => "LIBRARY",
        ObjectZone::Hand(_) => "HAND",
        ObjectZone::Graveyard(_) => "GRAVEYARD",
        ObjectZone::Battlefield => "BATTLEFIELD",
        ObjectZone::Stack => "STACK",
        ObjectZone::Exile => "EXILE",
        ObjectZone::Command => "COMMAND",
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
    let creature = execution.creature(object)?;
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
            // This tool tracks shield and multiplier objects created by its authored spell recipes.
            DamageEffectId::EmblemCap { .. }
            | DamageEffectId::StaticPrevention(_)
            | DamageEffectId::StaticPreventionWithExile(_, _)
            | DamageEffectId::CombatDamageToMill(_, _)
            | DamageEffectId::RemovePlusOneToPreventDamage(_, _)
            | DamageEffectId::ShieldCounter(_)
            | DamageEffectId::StaticDouble(_)
            | DamageEffectId::StaticModify(_, _)
            | DamageEffectId::TemporaryStaticModify(_)
            | DamageEffectId::TemporaryPrevention(_)
            | DamageEffectId::DoubleDamageFromEquippedCreature(_)
            | DamageEffectId::DoubleDamageToEquippedCreature(_)
            | DamageEffectId::StaticHalfPrevention { .. }
            | DamageEffectId::ProtectionMulticolored
            | DamageEffectId::ProtectionSubtype(_)
            | DamageEffectId::ProtectionFrom(_)
            | DamageEffectId::Protection(_)
            | DamageEffectId::UntilEndOfTurn { .. } => continue,
            DamageEffectId::Prevention(id) => id,
            DamageEffectId::Multiplier(id) => {
                let active = execution.damage_multiplier(id);
                multiplier_instances.push(MultiplierView {
                    source: alias,
                    multiplier_id: format!("{id:?}"),
                    creator_entry: format!("{:?}", tracked.creator),
                    target: damage_effect_target_view(tracked.target),
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
            creator_entry: format!("{:?}", tracked.creator),
            target: damage_effect_target_view(tracked.target),
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
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::observe"),
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        resolving: profile
            .sequences()
            .then_some(execution.window().resolving().is_some()),
        checkpoint,
        power: creature.as_ref().map(|creature| creature.power.clone()),
        toughness: creature.as_ref().map(|creature| creature.toughness.clone()),
        zone: zone_name(card.zone()),
        damage: execution.battlefield_marked_damage(object),
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
    let candidate = spellwake_reference_tools::current_capture::advance(
        &state,
        GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        },
    )?;
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
    let target = TargetRef::Object(identity(&state, WARDEN)?);
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
    let source_object = state
        .execution()
        .window()
        .cards()
        .get(identity(&state, source.card)?)
        .ok_or_else(inconsistent)?
        .clone();
    evidence.source_objects.insert(
        source.alias,
        damage_source_view(&DamageSource::Object {
            object: source_object,
            controller: entry.controller(),
        }),
    );
    let mut chosen_count = 0;
    let input_limit = state.execution().window().participants().len() + source.choices.len();
    for _ in 0..input_limit {
        if let Some(pending) = state.pending_decision() {
            let RuleFrame::Damage(DamageDecision::Order(frame)) = pending.frame() else {
                return Err(inconsistent());
            };
            let active = state
                .execution()
                .pending_damage()
                .ok_or_else(inconsistent)?;
            let mut components = active.components();
            let component = components.next().ok_or_else(inconsistent)?;
            if components.next().is_some() {
                return Err(inconsistent());
            }
            drop(components);
            let resolving = state
                .execution()
                .window()
                .resolving()
                .ok_or_else(inconsistent)?;
            let DamageOrigin::Resolution {
                entry: origin_entry,
                instruction,
            } = *frame.origin()
            else {
                return Err(inconsistent());
            };
            let source_object = identity(&state, source.card)?;
            let DamageSource::Object { object, controller } = &component.attempt().source else {
                return Err(inconsistent());
            };
            if pending.chooser() != B
                || origin_entry != entry_id
                || resolving.id() != entry_id
                || active.id() != frame.batch()
                || active.origin() != *frame.origin()
                || active.stage() != frame.stage()
                || frame.revision() != state.execution().revision()
                || component.attempt().source.id() != DamageSourceId::Object(source_object)
                || object.id() != source_object
                || *controller != resolving.controller()
                || component.attempt().target
                    != DamageRecipient::Creature(identity(&state, WARDEN)?)
                || state.execution().instruction_index() != Some(instruction)
            {
                return Err(inconsistent());
            }
            let alias = *source.choices.get(chosen_count).ok_or_else(inconsistent)?;
            let selected_effect = effects.get(alias).ok_or_else(inconsistent)?.id;
            let selected = *frame
                .applications()
                .iter()
                .find(|application| application.effect == selected_effect)
                .ok_or_else(inconsistent)?;
            let mut domain = frame
                .applications()
                .iter()
                .map(|application| effect_alias(effects, application.effect))
                .collect::<Result<Vec<_>, _>>()?;
            domain.sort_unstable();
            let DamageRecipient::Creature(target) = component.attempt().target else {
                return Err(inconsistent());
            };
            evidence.decisions.push(NativeDecision {
                instruction: profile.sequences().then_some(instruction),
                source: source.alias,
                domain,
                chosen: alias,
                damage_before: state
                    .execution()
                    .battlefield_marked_damage(target)
                    .ok_or_else(inconsistent)?,
                pending_damage: component.remaining().clone(),
                chooser: seat_name(pending.chooser())?,
                target: "warden",
                source_controller: seat_name(component.attempt().source.controller())?,
                stack_size: state.execution().window().entries().len(),
                source_zone: zone_name(zone(&state, source.card)?),
                decision_id: format!("{:?}", pending.id()),
                batch_id: format!("{:?}", active.id()),
                origin: damage_origin_view(*frame.origin()),
                actual_source: damage_source_view(&component.attempt().source),
                application_domain: frame
                    .applications()
                    .iter()
                    .copied()
                    .map(damage_application_view)
                    .collect(),
                chosen_application: damage_application_view(selected),
                attempt_id: format!("{:?}", component.attempt().id),
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
                || zone(&state, source.card)? != ObjectZone::Graveyard(A)
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
    DamageEffectTarget,
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
            shield.creator,
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
            multiplier.creator,
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
    target: ObjectId,
    evidence: &NativeEvidence,
    effects: &mut BTreeMap<&'static str, TrackedEffect>,
) -> Result<(), GameError> {
    for creation in &journal.created {
        let (id, creator, created_target, instruction, _, _) = creation_parts(creation);
        let alias = source
            .creation_aliases
            .iter()
            .find(|(index, _)| *index == instruction)
            .map(|(_, alias)| *alias)
            .ok_or_else(inconsistent)?;
        if evidence.source_entries.get(source.alias) != Some(&format!("{creator:?}"))
            || created_target != DamageEffectTarget::Permanent(target)
            || effects.values().any(|effect| {
                effect.id == id || (effect.creator == creator && effect.instruction == instruction)
            })
            || effects
                .insert(
                    alias,
                    TrackedEffect {
                        id,
                        creator,
                        target: DamageEffectTarget::Permanent(target),
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
    vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Result<ScenarioResult, GameError> {
    let mut cards = vec![SeedCard {
        card: WARDEN,
        owner: B,
        destination: ObjectDestination::Battlefield { controller: B },
        program: Program::creature(None, 1.into(), 3.into(), std::collections::BTreeSet::new()),
    }];
    let mut mana: RulesInteger = 0.into();
    for source in &sources {
        cards.push(SeedCard {
            card: source.card,
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(ManaType::Red),
                ExecutableBody::new(BodyTarget::SingleCreature, source.instructions.clone()),
            ),
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
        vocabulary.rules_revision().to_owned(),
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
        vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )?;
    let candidate = spellwake_reference_tools::current_capture::initialize(game, context, {
        // Fixture premise: seeded BF timestamps follow this authored card sequence.
        let initial_battlefield_order = cards
            .iter()
            .filter(|card| {
                matches!(
                    card.destination,
                    spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                )
            })
            .map(|card| card.card)
            .collect();
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            initial_battlefield_order,
            vec![FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: mana,
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })?;
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
    let actual = serde_json::to_value(&evidence).expect("actual native evidence serialization");
    evidence.capture_point = spellwake_reference_tools::capture_points::token(
        concat!(file!(), "::run_scenario::native_evidence"),
        spellwake_reference_tools::current_profile::SnapshotOwner::PureValue(&actual),
        &spellwake_reference_tools::packet::AliasContext::default(),
    )
    .expect("actual native evidence point");
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
                    Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                                ResolvedPartSpec::SetPT(PowerToughness {
                                    power: 1.into(),
                                    toughness: 0.into(),
                                }),
                            ]),
                            spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                        ),
                    )),
                    Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                                ResolvedPartSpec::ModifyPT(PowerToughness {
                                    power: 0.into(),
                                    toughness: 2.into(),
                                }),
                            ]),
                            spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                        ),
                    )),
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
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Hand,
                    ))),
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
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Hand,
                    ))),
                    Instruction::Damage { amount: 3.into() },
                ],
                &[],
            )],
        ),
    ]
}

fn run(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    profile: TraceProfile,
) -> Result<Report, GameError> {
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
        let result = run_scenario(GameId(id), name, sources, profile, vocabulary.clone())?;
        scenario_programs.insert(name, result.programs);
        scenario_observations.insert(name, result.observations);
        native_only_evidence.insert(name, result.evidence);
    }
    Ok(Report {
        schema_version: 3,
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
    let vocabulary = match spellwake_reference_tools::creature_types::selected_vocabulary() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            std::process::exit(1);
        }
    };

    let arguments = match spellwake_reference_tools::creature_types::qualification_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            return ExitCode::FAILURE;
        }
    };
    let profile = match arguments.as_slice() {
        [] => Ok(TraceProfile::Prevention),
        [flag] if flag == "--multipliers" => Ok(TraceProfile::Multipliers),
        [flag] if flag == "--sequences" => Ok(TraceProfile::Sequences),
        _ => Err(GameError::Effect(EffectError::InvalidSetup)),
    };
    let profile_token = match &profile {
        Ok(TraceProfile::Prevention) => "prevention",
        Ok(TraceProfile::Multipliers) => "multipliers",
        Ok(TraceProfile::Sequences) => "programs",
        Err(_) => {
            eprintln!("unsupported current profile arguments");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = spellwake_reference_tools::current_capture::start(
        profile_token,
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    ) {
        eprintln!("current capture source unavailable: {error}");
        return ExitCode::FAILURE;
    }
    let (response, success): (ApiResponse<Report>, bool) =
        match profile.and_then(|profile| run(&vocabulary, profile)) {
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
    let response = match spellwake_reference_tools::current_capture::finish_serializable(
        profile_token,
        &response,
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("current capture incomplete: {error}");
            return ExitCode::FAILURE;
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
            spellwake_reference_tools::creature_types::synthetic_vocabulary(),
        )
        .unwrap();
        let births: Vec<_> = result
            .evidence
            .input_journals
            .iter()
            .flat_map(|journal| journal.creation_bindings.as_ref().unwrap())
            .collect();
        assert_eq!(births.len(), 2);
        assert_eq!((births[0].effect, births[0].instruction), ("first", 0));
        assert_eq!((births[1].effect, births[1].instruction), ("second", 1));
        assert_eq!(births[0].creator, births[1].creator);
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
        assert!(terminal.journal.applied.is_empty());
        assert_eq!(terminal.journal.completions[0].outcome, "fully-prevented");
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
            spellwake_reference_tools::creature_types::synthetic_vocabulary(),
        )
        .unwrap();
        let terminal = result.evidence.input_journals.last().unwrap();
        let applications = terminal.applications.as_ref().unwrap();
        assert_eq!(terminal.journal.attempts.len(), 10);
        assert_eq!(applications.len(), 10);
        for (attempt, application) in terminal.journal.attempts.iter().zip(applications) {
            assert_eq!(attempt.attempt, application.attempt);
            assert_eq!(application.stage, 1);
        }
        assert_eq!(applications[8].attempt, "DamageAttemptId(9)");
        assert_eq!(applications[9].attempt, "DamageAttemptId(10)");
    }
}
