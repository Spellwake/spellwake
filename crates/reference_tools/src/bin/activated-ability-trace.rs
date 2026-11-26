//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual native activation inputs and observations for isolated authored artifacts.
//! No expected or reference JSON is read. Already-tapped fixtures are reached by
//! a disclosed native-only free tap activation against an auxiliary creature.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    activated::{AbilityActivationRecord, ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, GameBasis, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
    },
    mana::{FloatingMana, ManaCost, ManaPayment, ManaPaymentRecord, ManaSpend, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectId, ObjectZone},
    priority::SeatId,
    stack_window::StackKind,
    targets::TargetRef,
};
use spellwake_reference_tools::damage_trace::{DamageJournalView, damage_journal_view};
use std::{collections::BTreeMap, process::ExitCode};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const SOURCE: CardId = CardId(1);
const TARGET: CardId = CardId(2);
const RESPONSE: CardId = CardId(3);
const AUXILIARY: CardId = CardId(4);
type Pool = BTreeMap<&'static str, RulesInteger>;

#[derive(Clone, Serialize)]
struct PhysicalView {
    owner: &'static str,
    controller: Option<&'static str>,
    zone: String,
    tapped: Option<bool>,
}
#[derive(Clone, Serialize)]
struct CreatureView {
    #[serde(flatten)]
    physical: PhysicalView,
    power: Option<RulesInteger>,
    toughness: Option<RulesInteger>,
    damage: Option<RulesInteger>,
}
#[derive(Clone, Serialize)]
struct Snapshot {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    label: &'static str,
    source: PhysicalView,
    target: CreatureView,
    pools: BTreeMap<&'static str, Pool>,
    stack_size: usize,
    stack_kind: Option<&'static str>,
    stack_controller: Option<&'static str>,
    source_on_stack: bool,
    accepted: Option<bool>,
    response: Option<PhysicalView>,
}
#[derive(Clone, Serialize)]
struct BasisView {
    game: String,
    context: serde_json::Value,
    epoch: u64,
    revision: u64,
}
impl From<&GameBasis> for BasisView {
    fn from(value: &GameBasis) -> Self {
        Self {
            game: format!("{:?}", value.game),
            context: spellwake_reference_tools::projection::context(&value.context),
            epoch: value.epoch,
            revision: value.revision,
        }
    }
}
#[derive(Serialize)]
struct IdentityView {
    object: String,
    physical_card: u64,
    physical: PhysicalView,
}
fn object_view(object: &GameObject) -> IdentityView {
    IdentityView {
        object: format!("{:?}", object.id()),
        physical_card: object.card_id().expect("source-authored card-only trace").0,
        physical: physical(object),
    }
}
#[derive(Serialize)]
struct EntryView {
    id: String,
    kind: &'static str,
    controller: &'static str,
    source: Option<IdentityView>,
    targets: String,
}
#[derive(Serialize)]
struct NativeState {
    basis: BasisView,
    snapshot: Snapshot,
    objects: BTreeMap<&'static str, IdentityView>,
    stack_entries: Vec<EntryView>,
    priority: String,
    pending: bool,
    instruction_index: Option<usize>,
}
#[derive(Serialize)]
struct CostView {
    generic: RulesInteger,
    required: Pool,
}
#[derive(Serialize)]
struct SpendView {
    mana_type: &'static str,
    amount: RulesInteger,
}
#[derive(Serialize)]
struct PaymentView {
    source: String,
    payer: &'static str,
    cost: CostView,
    spend: Vec<SpendView>,
}
impl From<&ManaPaymentRecord> for PaymentView {
    fn from(value: &ManaPaymentRecord) -> Self {
        Self {
            source: format!("{:?}", value.source),
            payer: seat(value.payer),
            cost: CostView {
                generic: value.cost.generic.clone(),
                required: amounts(&value.cost.required),
            },
            spend: value
                .spend
                .iter()
                .map(|entry| SpendView {
                    mana_type: symbol(entry.mana_type),
                    amount: entry.amount.clone(),
                })
                .collect(),
        }
    }
}
#[derive(Serialize)]
struct ActivationView {
    entry: String,
    ability: u64,
    controller: &'static str,
    source: IdentityView,
    source_cost: &'static str,
    resulting_source: String,
}
impl From<&AbilityActivationRecord> for ActivationView {
    fn from(value: &AbilityActivationRecord) -> Self {
        Self {
            entry: format!("{:?}", value.entry),
            ability: value.ability.0,
            controller: seat(value.controller),
            source: object_view(&value.source),
            source_cost: value.source_cost.trace_label(),
            resulting_source: format!("{:?}", value.resulting_source),
        }
    }
}
#[derive(Serialize)]
struct Step {
    label: &'static str,
    actor: &'static str,
    command: String,
    input_basis: BasisView,
    before: NativeState,
    after: NativeState,
    candidate_base: Option<BasisView>,
    activation: Option<ActivationView>,
    payment: Option<PaymentView>,
    production_present: bool,
    damage_journal: Option<DamageJournalView>,
    input_unchanged: bool,
    error: Option<String>,
}
#[derive(Serialize)]
struct NativeProbe {
    id: &'static str,
    initialization_activation_present: bool,
    initialization_payment_present: bool,
    initialization_production_present: bool,
    initialized: NativeState,
    setup: Vec<Step>,
    initial: NativeState,
    steps: Vec<Step>,
    followup: Step,
}
#[derive(Serialize)]
struct CommonProbe {
    id: &'static str,
    source_card: &'static str,
    target_card: &'static str,
    source_owner: &'static str,
    target_owner: &'static str,
    initial_tapped: bool,
    selected_spend: Vec<&'static str>,
    effect: &'static str,
    intervention: Option<&'static str>,
    comparison_scope: &'static str,
    response_card: Option<&'static str>,
    snapshots: Vec<Snapshot>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: &'static str,
    native_driver: &'static str,
    program_source: &'static str,
    scope: &'static str,
    numeric_encoding: &'static str,
    catalogue_bindings: u32,
    fully_qualified_cards: u32,
    full_game_comparisons: u32,
    excluded_reference_probes: Vec<&'static str>,
    scenarios: BTreeMap<&'static str, Vec<CommonProbe>>,
    native_observations: Vec<NativeProbe>,
}
struct Probe {
    group: &'static str,
    id: &'static str,
    source_card: &'static str,
    source_owner: SeatId,
    target_owner: SeatId,
    initially_tapped: bool,
    response: bool,
    ability: u64,
    cost: ManaCost,
    source_cost: SourceCost,
    instructions: Vec<Instruction>,
    spend: Vec<ManaType>,
    effect: &'static str,
}
fn seat(value: SeatId) -> &'static str {
    match value {
        A => "A",
        B => "B",
        _ => "unknown",
    }
}
fn symbol(value: ManaType) -> &'static str {
    match value {
        ManaType::White => "W",
        ManaType::Blue => "U",
        ManaType::Black => "B",
        ManaType::Red => "R",
        ManaType::Green => "G",
        ManaType::Colourless => "C",
    }
}
fn amounts(value: &BTreeMap<ManaType, RulesInteger>) -> Pool {
    ManaType::ALL
        .into_iter()
        .map(|kind| {
            (
                symbol(kind),
                value.get(&kind).cloned().unwrap_or_else(|| 0.into()),
            )
        })
        .collect()
}
fn zone(value: ObjectZone) -> String {
    match value {
        ObjectZone::Battlefield => "battlefield".into(),
        ObjectZone::Stack => "stack".into(),
        ObjectZone::Hand(owner) => format!("hand:{}", seat(owner)),
        ObjectZone::Graveyard(owner) => format!("graveyard:{}", seat(owner)),
        ObjectZone::Library(owner) => format!("library:{}", seat(owner)),
        ObjectZone::Exile => "exile".into(),
        ObjectZone::Command => "command".into(),
    }
}
fn physical(object: &GameObject) -> PhysicalView {
    PhysicalView {
        owner: seat(object.owner()),
        controller: object.controller().map(seat),
        zone: zone(object.zone()),
        tapped: object.tapped(),
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
fn object(state: &GameState, card: CardId) -> Result<&GameObject, GameError> {
    state
        .execution()
        .window()
        .cards()
        .get(identity(state, card)?)
        .ok_or(EffectError::UnknownObject.into())
}
fn kind(value: &StackKind) -> &'static str {
    match value {
        StackKind::CardSpell(_) => "card-spell",
        StackKind::ActivatedAbility { .. } => "activated-ability",
        StackKind::SpellCopy { .. } => "spell-copy",
        StackKind::Speed => "speed",
        StackKind::Monarch => "monarch",
        StackKind::DelayedCopy { .. } => "delayed-copy",
        StackKind::Emblem { .. } => "emblem",
        StackKind::TriggeredAbility { .. } => "triggered-ability",
    }
}
fn native_state(
    state: &GameState,
    probe: &Probe,
    label: &'static str,
    accepted: Option<bool>,
) -> Result<NativeState, GameError> {
    assert!(
        !state.pending_decision().is_some_and(|pending| matches!(
            pending.frame(),
            spellwake_engine::game::RuleFrame::Triggers(_)
        )),
        "bounded activated-ability projection excludes trigger-order frames"
    );
    let entries = state.execution().window().entries();
    let target_object = object(state, TARGET)?;
    let creature = state.execution().creature(target_object.id())?;
    let snapshot = Snapshot {
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::native_state"),
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        label,
        source: physical(object(state, SOURCE)?),
        target: CreatureView {
            physical: physical(target_object),
            power: creature.as_ref().map(|value| value.power.clone()),
            toughness: creature.as_ref().map(|value| value.toughness.clone()),
            damage: state
                .execution()
                .battlefield_marked_damage(target_object.id()),
        },
        pools: [A, B]
            .into_iter()
            .map(|player| {
                (
                    seat(player),
                    ManaType::ALL
                        .into_iter()
                        .map(|mana_type| {
                            (symbol(mana_type), state.execution().mana(player, mana_type))
                        })
                        .collect(),
                )
            })
            .collect(),
        stack_size: entries.len(),
        stack_kind: entries.last().map(|entry| kind(entry.kind())),
        stack_controller: entries.last().map(|entry| seat(entry.controller())),
        source_on_stack: object(state, SOURCE)?.zone() == ObjectZone::Stack,
        accepted,
        response: if probe.response {
            Some(physical(object(state, RESPONSE)?))
        } else {
            None
        },
    };
    let mut cards = vec![("source", SOURCE), ("target", TARGET)];
    if probe.response {
        cards.push(("response", RESPONSE));
    }
    if probe.initially_tapped {
        cards.push(("auxiliary", AUXILIARY));
    }
    Ok(NativeState {
        basis: state.basis().into(),
        snapshot,
        objects: cards
            .into_iter()
            .map(|(alias, card)| Ok((alias, object_view(object(state, card)?))))
            .collect::<Result<_, GameError>>()?,
        stack_entries: entries
            .iter()
            .map(|entry| EntryView {
                id: format!("{:?}", entry.id()),
                kind: kind(entry.kind()),
                controller: seat(entry.controller()),
                source: match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(object_view(source)),
                    _ => None,
                },
                targets: format!("{:?}", entry.targets()),
            })
            .collect(),
        priority: format!("{:?}", state.execution().window().phase()),
        pending: state.pending_decision().is_some(),
        instruction_index: state.execution().instruction_index(),
    })
}
fn input(state: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    }
}
fn execute(
    state: &mut GameState,
    probe: &Probe,
    label: &'static str,
    request: GameInput,
    accepted: Option<bool>,
) -> Result<Step, GameError> {
    let before = native_state(state, probe, "before", None)?;
    let saved = state.clone();
    let result = spellwake_reference_tools::current_capture::advance(state, request.clone());
    let input_unchanged = *state == saved;
    let (candidate_base, activation, payment, production_present, damage_journal, error) =
        match result {
            Ok(candidate) => {
                let CandidateBase::Advance(base) = candidate.base() else {
                    return Err(EffectError::InconsistentProgram.into());
                };
                let values = (
                    Some(base.into()),
                    candidate.ability_activation().map(ActivationView::from),
                    candidate.mana_payment().map(PaymentView::from),
                    candidate.mana_production().is_some(),
                    Some(damage_journal_view(candidate.damage_journal())),
                    None,
                );
                *state = candidate.into_state();
                values
            }
            Err(error) => (None, None, None, false, None, Some(format!("{error:?}"))),
        };
    let after_accepted = if matches!(label, "activated" | "rejected" | "response-cast") {
        Some(error.is_none())
    } else {
        accepted
    };
    Ok(Step {
        label,
        actor: seat(request.actor),
        command: format!("{:?}", request.command),
        input_basis: (&request.basis).into(),
        before,
        after: native_state(state, probe, label, after_accepted)?,
        candidate_base,
        activation,
        payment,
        production_present,
        damage_journal,
        input_unchanged,
        error,
    })
}
fn pass(
    state: &mut GameState,
    probe: &Probe,
    label: &'static str,
    actor: SeatId,
) -> Result<Step, GameError> {
    let request = input(state, actor, GameCommand::Pass);
    execute(state, probe, label, request, None)
}
fn activation(
    state: &GameState,
    ability: u64,
    target: CardId,
    payment: Option<ManaPayment>,
) -> Result<GameCommand, GameError> {
    Ok(GameCommand::ActivateAbility {
        source: identity(state, SOURCE)?,
        ability: ActivatedAbilityId(ability),
        target: TargetRef::Object(identity(state, target)?),
        payment,
    })
}
fn run_probe(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    game: u128,
    probe: Probe,
) -> Result<(CommonProbe, NativeProbe), GameError> {
    let mut abilities = vec![ActivatedAbility {
        id: ActivatedAbilityId(probe.ability),
        mana_cost: probe.cost.clone(),
        source_cost: probe.source_cost,
        body: ExecutableBody::new(BodyTarget::SingleCreature, probe.instructions.clone()),
    }];
    if probe.initially_tapped {
        abilities.push(ActivatedAbility {
            id: ActivatedAbilityId(90),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Tap,
            body: ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        });
    }
    let mut cards = vec![
        SeedCard {
            card: SOURCE,
            owner: probe.source_owner,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::artifact(None, abilities),
        },
        SeedCard {
            card: TARGET,
            owner: probe.target_owner,
            destination: ObjectDestination::Battlefield { controller: B },
            program: Program::creature(None, 3.into(), 3.into(), std::collections::BTreeSet::new()),
        },
    ];
    if probe.response {
        cards.push(SeedCard {
            card: RESPONSE,
            owner: B,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(ManaType::Blue),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Hand,
                        ),
                    ))],
                ),
            ),
        });
    }
    if probe.initially_tapped {
        cards.push(SeedCard {
            card: AUXILIARY,
            owner: B,
            destination: ObjectDestination::Battlefield { controller: B },
            program: Program::creature(None, 3.into(), 3.into(), std::collections::BTreeSet::new()),
        });
    }
    let initialized = spellwake_reference_tools::current_capture::initialize(
        GameId(game),
        GameContext::new(
            "native-activated-abilities-v1".into(),
            vocabulary.rules_revision().to_owned(),
            "synthetic-no-catalogue-binding".into(),
            "authored-noncreature-artifacts-v1".into(),
            probe.id.into(),
            vocabulary.id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )?,
        {
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
                [A, B]
                    .into_iter()
                    .flat_map(|player| {
                        ManaType::ALL
                            .into_iter()
                            .map(move |mana_type| FloatingMana {
                                player,
                                mana_type,
                                amount: (if player == A { 2 } else { 1 }).into(),
                            })
                    })
                    .collect(),
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
    )?;
    let initialization_activation_present = initialized.ability_activation().is_some();
    let initialization_payment_present = initialized.mana_payment().is_some();
    let initialization_production_present = initialized.mana_production().is_some();
    let mut state = initialized.into_state();
    let initialized = native_state(&state, &probe, "initialized", None)?;
    let mut setup = Vec::new();
    if probe.initially_tapped {
        let request = input(&state, A, activation(&state, 90, AUXILIARY, None)?);
        setup.push(execute(&mut state, &probe, "setup-tap", request, None)?);
        setup.push(pass(&mut state, &probe, "setup-pass-a", A)?);
        setup.push(pass(&mut state, &probe, "setup-resolved", B)?);
    }
    let initial = native_state(&state, &probe, "initial", None)?;
    let mut snapshots = vec![initial.snapshot.clone()];
    let requested_spend = probe
        .spend
        .iter()
        .map(|mana_type| ManaSpend {
            mana_type: *mana_type,
            amount: 1.into(),
        })
        .collect();
    let request = input(
        &state,
        A,
        activation(
            &state,
            probe.ability,
            TARGET,
            Some(ManaPayment {
                from_lots: Default::default(),
                spend: requested_spend,
            }),
        )?,
    );
    let mut steps = Vec::new();
    let step = execute(
        &mut state,
        &probe,
        if probe.id == "rod-tapped-rejected" {
            "rejected"
        } else {
            "activated"
        },
        request,
        None,
    )?;
    let admitted = step.error.is_none();
    snapshots.push(step.after.snapshot.clone());
    steps.push(step);
    if admitted {
        if probe.response {
            steps.push(pass(&mut state, &probe, "pass-to-response", A)?);
            let request = input(
                &state,
                B,
                GameCommand::Cast {
                    spell: identity(&state, RESPONSE)?,
                    target: TargetRef::Object(identity(&state, TARGET)?),
                },
            );
            let step = execute(&mut state, &probe, "response-cast", request, Some(true))?;
            snapshots.push(step.after.snapshot.clone());
            steps.push(step);
            steps.push(pass(&mut state, &probe, "response-pass-b", B)?);
            let step = pass(&mut state, &probe, "response-resolved", A)?;
            snapshots.push(step.after.snapshot.clone());
            steps.push(step);
        }
        steps.push(pass(&mut state, &probe, "ability-pass-a", A)?);
        let step = pass(&mut state, &probe, "resolved", B)?;
        snapshots.push(step.after.snapshot.clone());
        steps.push(step);
    }
    let followup = pass(&mut state, &probe, "followup-pass", A)?;
    let common = CommonProbe {
        id: probe.id,
        source_card: probe.source_card,
        target_card: "Hill Giant",
        source_owner: seat(probe.source_owner),
        target_owner: seat(probe.target_owner),
        initial_tapped: probe.initially_tapped,
        selected_spend: probe.spend.into_iter().map(symbol).collect(),
        effect: probe.effect,
        intervention: probe.response.then_some("target"),
        comparison_scope: "common",
        response_card: probe.response.then_some("Unsummon"),
        snapshots,
    };
    Ok((
        common,
        NativeProbe {
            id: probe.id,
            initialization_activation_present,
            initialization_payment_present,
            initialization_production_present,
            initialized,
            setup,
            initial,
            steps,
            followup,
        },
    ))
}
fn fixtures() -> Vec<Probe> {
    let mut result = Vec::new();
    for (id, owner, tapped, response) in [
        ("rod-normal", A, false, false),
        ("rod-stolen", B, false, false),
        ("rod-tapped-rejected", A, true, false),
        ("rod-target-departed", A, false, true),
    ] {
        result.push(Probe {
            group: "rod-of-ruin",
            id,
            source_card: "Rod of Ruin",
            source_owner: owner,
            target_owner: B,
            initially_tapped: tapped,
            response,
            ability: 31,
            cost: ManaCost::generic(3.into()),
            source_cost: SourceCost::Tap,
            instructions: vec![Instruction::Damage { amount: 1.into() }],
            spend: vec![ManaType::Blue, ManaType::Red, ManaType::Colourless],
            effect: "1",
        });
    }
    for (id, owner, tapped, response) in [
        ("pyrite-normal", A, false, false),
        ("pyrite-tapped-stolen", B, true, false),
        ("pyrite-target-departed", A, false, true),
    ] {
        result.push(Probe {
            group: "pyrite-spellbomb",
            id,
            source_card: "Pyrite Spellbomb",
            source_owner: owner,
            target_owner: B,
            initially_tapped: tapped,
            response,
            ability: 43,
            cost: ManaCost::one(ManaType::Red),
            source_cost: SourceCost::Sacrifice,
            instructions: vec![Instruction::Damage { amount: 2.into() }],
            spend: vec![ManaType::Red],
            effect: "2",
        });
    }
    for (id, source_owner, target_owner, response) in [
        ("aether-normal", A, B, false),
        ("aether-stolen-target-owner", B, A, false),
        ("aether-target-departed", A, B, true),
    ] {
        result.push(Probe {
            group: "aether-spellbomb",
            id,
            source_card: "Aether Spellbomb",
            source_owner,
            target_owner,
            initially_tapped: false,
            response,
            ability: 59,
            cost: ManaCost::one(ManaType::Blue),
            source_cost: SourceCost::Sacrifice,
            instructions: vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Hand,
                ),
            ))],
            spend: vec![ManaType::Blue],
            effect: "bounce",
        });
    }
    result
}
fn run(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Result<Report, GameError> {
    let mut scenarios: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut native_observations = Vec::new();
    for (index, probe) in fixtures().into_iter().enumerate() {
        let group = probe.group;
        let (common, native) = run_probe(vocabulary, index as u128 + 1, probe)?;
        scenarios.entry(group).or_default().push(common);
        native_observations.push(native);
    }
    Ok(Report {
        schema_version: 2,
        kind: "native-isolated-activated-ability-observations",
        native_driver: "GameState::initialize/advance",
        program_source: "explicit authored noncreature artifact fragments; no catalogue binding",
        scope: "10 common probes/35 physical checkpoints. Initial tapped status in two fixtures is reached by disclosed native-only free Tap damage against an auxiliary creature and real passes; this is not a reference setup protocol comparison. Rod source intervention remains reference-only. Native basis, incarnations, current activation/payment records and priority are separate native-only evidence. No whole cards, general costs, host protocol, full games or capacity qualification.",
        numeric_encoding: "canonical decimal strings for rules quantities",
        catalogue_bindings: 0,
        fully_qualified_cards: 0,
        full_game_comparisons: 0,
        excluded_reference_probes: vec!["rod-source-departed"],
        scenarios,
        native_observations,
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

    let native_arguments = spellwake_reference_tools::creature_types::qualification_args()
        .unwrap_or_else(|error| {
            eprintln!("invalid qualification input: {error}");
            std::process::exit(1)
        });
    if !native_arguments.is_empty() {
        eprintln!("unsupported qualification arguments");
        std::process::exit(1);
    }

    let profile_token = "activated";
    if let Err(error) = spellwake_reference_tools::current_capture::start(
        profile_token,
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    ) {
        eprintln!("current capture source unavailable: {error}");
        return ExitCode::FAILURE;
    }
    let result = match run(&vocabulary) {
        Ok(report) => ApiResponse::<Report>::Success { data: report },
        Err(error) => {
            eprintln!("native activation trace failed: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let result = match spellwake_reference_tools::current_capture::finish_serializable(
        profile_token,
        &result,
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("current capture incomplete: {error}");
            return ExitCode::FAILURE;
        }
    };
    match serde_json::to_string_pretty(&result) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("native activation trace serialization failed: {error}");
            ExitCode::FAILURE
        }
    }
}
