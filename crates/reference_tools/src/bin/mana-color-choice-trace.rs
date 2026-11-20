//! Actual GameState color-choice activations over authored land programs.
//! This runner never consumes expected or reference outputs; all physical views,
//! pending domains and production records come from actual native candidates.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{CopyTargetChoice, EffectError, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, DecisionReply, GameBasis, GameCommand, GameContext, GameError,
        GameId, GameInput, GameState, RuleFrame,
    },
    mana::{FloatingMana, ManaAbility, ManaAbilityId, ManaOutput, ManaProductionRecord, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    priority::SeatId,
};
use std::{collections::BTreeMap, process::ExitCode};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const COLORS: [ManaType; 5] = [
    ManaType::White,
    ManaType::Blue,
    ManaType::Black,
    ManaType::Red,
    ManaType::Green,
];
type Pool = BTreeMap<&'static str, RulesInteger>;

#[derive(Clone, Serialize)]
struct SourceView {
    card: &'static str,
    zone: String,
    owner: &'static str,
    controller: Option<&'static str>,
    tapped: Option<bool>,
}
#[derive(Clone, Serialize)]
struct Snapshot {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    label: &'static str,
    sources: BTreeMap<&'static str, SourceView>,
    pools: BTreeMap<&'static str, Pool>,
    stack_size: usize,
    accepted: Option<bool>,
}
#[derive(Serialize)]
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
struct SourceIdentity {
    object: String,
    physical_card: u64,
}
#[derive(Serialize)]
struct PendingView {
    decision: u64,
    basis: BasisView,
    chooser: &'static str,
    source: String,
    ability: serde_json::Value,
    amount: RulesInteger,
    colors: Vec<&'static str>,
    executor_frame_matches: bool,
}
#[derive(Serialize)]
struct NativeState {
    basis: BasisView,
    sources: BTreeMap<&'static str, SourceIdentity>,
    snapshot: Snapshot,
    pending: Option<PendingView>,
    executor_revision: u64,
    instruction_index: Option<usize>,
    stack_entries: Vec<String>,
    spell_copies: String,
    priority: String,
}
#[derive(Clone, Serialize)]
struct ProductionView {
    source: String,
    ability: serde_json::Value,
    controller: &'static str,
    produced: Pool,
}
impl From<&ManaProductionRecord> for ProductionView {
    fn from(value: &ManaProductionRecord) -> Self {
        Self {
            source: format!("{:?}", value.source),
            ability: spellwake_reference_tools::mana_ref::view(value.ability),
            controller: seat(value.controller),
            produced: amounts(&value.produced),
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
    production: Option<ProductionView>,
    payment_receipt_present: bool,
    input_unchanged: bool,
    error: Option<String>,
}
#[derive(Serialize)]
struct NativeProbe {
    id: &'static str,
    initialization_production_present: bool,
    initialization_payment_present: bool,
    initial: NativeState,
    steps: Vec<Step>,
    followup: Step,
}
#[derive(Clone, Serialize)]
struct AbilityView {
    produced: &'static str,
    amount: RulesInteger,
}
#[derive(Clone, Serialize)]
struct CallbackView {
    source: &'static str,
    chooser: &'static str,
    colors: Vec<&'static str>,
    selected: &'static str,
    amount: RulesInteger,
    snapshot_index: usize,
}
#[derive(Serialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}
fn one_or_many<T>(mut values: Vec<T>) -> OneOrMany<T> {
    if values.len() == 1 {
        OneOrMany::One(values.remove(0))
    } else {
        OneOrMany::Many(values)
    }
}
#[derive(Serialize)]
struct CommonProbe {
    id: &'static str,
    source_cards: BTreeMap<&'static str, &'static str>,
    selected_ability: OneOrMany<AbilityView>,
    chosen_color: OneOrMany<&'static str>,
    produced: OneOrMany<Pool>,
    snapshots: Vec<Snapshot>,
    callbacks: Vec<CallbackView>,
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
    scenarios: BTreeMap<&'static str, Vec<CommonProbe>>,
    native_observations: Vec<NativeProbe>,
}
struct Source {
    alias: &'static str,
    name: &'static str,
    card: CardId,
    owner: SeatId,
    ability: u64,
    amount: i64,
}
struct Activation {
    source: &'static str,
    color: ManaType,
    pending_label: &'static str,
    completed_label: &'static str,
}
struct Probe {
    group: &'static str,
    id: &'static str,
    sources: Vec<Source>,
    activations: Vec<Activation>,
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
fn identity(state: &GameState, card: CardId) -> Result<ObjectId, GameError> {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .ok_or(EffectError::UnknownObject.into())
}
fn snapshot(
    state: &GameState,
    sources: &[Source],
    label: &'static str,
    accepted: Option<bool>,
) -> Result<Snapshot, GameError> {
    Ok(Snapshot {
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::snapshot"),
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        label,
        sources: sources
            .iter()
            .map(|source| {
                let object = state
                    .execution()
                    .window()
                    .cards()
                    .get(identity(state, source.card)?)
                    .ok_or(EffectError::UnknownObject)?;
                Ok((
                    source.alias,
                    SourceView {
                        card: source.name,
                        zone: format!("{:?}", object.zone()),
                        owner: seat(object.owner()),
                        controller: object.controller().map(seat),
                        tapped: object.tapped(),
                    },
                ))
            })
            .collect::<Result<_, GameError>>()?,
        pools: [A, B]
            .into_iter()
            .map(|player| {
                (
                    seat(player),
                    ManaType::ALL
                        .into_iter()
                        .map(|kind| (symbol(kind), state.execution().mana(player, kind)))
                        .collect(),
                )
            })
            .collect(),
        stack_size: state.execution().window().entries().len(),
        accepted,
    })
}
fn native_state(
    state: &GameState,
    sources: &[Source],
    label: &'static str,
    accepted: Option<bool>,
) -> Result<NativeState, GameError> {
    let pending = state
        .pending_decision()
        .map(|decision| -> Result<PendingView, GameError> {
            let RuleFrame::ManaColor(frame) = decision.frame() else {
                return Err(EffectError::WrongDecisionKind.into());
            };
            Ok(PendingView {
                decision: decision.id().0,
                basis: decision.basis().into(),
                chooser: seat(decision.chooser()),
                source: format!("{:?}", frame.source()),
                ability: spellwake_reference_tools::mana_ref::view(frame.ability()),
                amount: frame.amount().clone(),
                colors: frame.colors().iter().copied().map(symbol).collect(),
                executor_frame_matches: state.execution().pending_mana_color() == Some(frame),
            })
        })
        .transpose()?;
    Ok(NativeState {
        basis: state.basis().into(),
        sources: sources
            .iter()
            .map(|source| {
                Ok((
                    source.alias,
                    SourceIdentity {
                        object: format!("{:?}", identity(state, source.card)?),
                        physical_card: source.card.0,
                    },
                ))
            })
            .collect::<Result<_, GameError>>()?,
        snapshot: snapshot(state, sources, label, accepted)?,
        pending,
        executor_revision: state.execution().revision(),
        instruction_index: state.execution().instruction_index(),
        stack_entries: state
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| format!("{entry:?}"))
            .collect(),
        spell_copies: format!("{:?}", state.execution().window().spell_copies()),
        priority: format!("{:?}", state.execution().window().phase()),
    })
}
fn input(state: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    }
}
/// Rejected requests are recorded against the actual unchanged accepted state.
fn execute(
    state: &mut GameState,
    sources: &[Source],
    label: &'static str,
    physical_label: &'static str,
    accepted: Option<bool>,
    request: GameInput,
) -> Result<Step, GameError> {
    let before = native_state(state, sources, "before", None)?;
    let saved = state.clone();
    let result = spellwake_reference_tools::current_capture::advance(state, request.clone());
    let input_unchanged = *state == saved;
    let (candidate_base, production, payment_receipt_present, error) = match result {
        Ok(candidate) => {
            let CandidateBase::Advance(base) = candidate.base() else {
                return Err(EffectError::InconsistentProgram.into());
            };
            let base = Some(base.into());
            let production = candidate.mana_production().map(ProductionView::from);
            let payment = candidate.mana_payment().is_some();
            *state = candidate.into_state();
            (base, production, payment, None)
        }
        Err(error) => (None, None, false, Some(format!("{error:?}"))),
    };
    Ok(Step {
        label,
        actor: seat(request.actor),
        command: format!("{:?}", request.command),
        input_basis: (&request.basis).into(),
        before,
        after: native_state(state, sources, physical_label, accepted)?,
        candidate_base,
        production,
        payment_receipt_present,
        input_unchanged,
        error,
    })
}
fn reply(decision: DecisionId, color: ManaType) -> GameCommand {
    GameCommand::Reply {
        decision,
        choice: DecisionReply::ManaColor(color),
    }
}
fn run_probe(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    index: u128,
    probe: Probe,
) -> Result<(CommonProbe, NativeProbe), GameError> {
    let initialized = spellwake_reference_tools::current_capture::initialize(
        GameId(index),
        GameContext::new(
            "native-mana-color-choice-v1".into(),
            vocabulary.rules_revision().to_owned(),
            "synthetic-no-catalogue-binding".into(),
            "authored-color-choice-land-v1".into(),
            probe.id.into(),
            vocabulary.id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )?,
        {
            let cards: Vec<SeedCard> = probe
                .sources
                .iter()
                .map(|source| SeedCard {
                    card: source.card,
                    owner: source.owner,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ManaAbility {
                            id: ManaAbilityId(source.ability),
                            output: ManaOutput::ChooseColor {
                                amount: source.amount.into(),
                                colors: COLORS.into_iter().collect(),
                            },
                        }],
                    ),
                })
                .collect();
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
                [
                    (A, ManaType::Black, 2),
                    (A, ManaType::Colourless, 1),
                    (B, ManaType::White, 1),
                ]
                .into_iter()
                .map(|(player, mana_type, amount)| FloatingMana {
                    player,
                    mana_type,
                    amount: amount.into(),
                })
                .collect(),
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
    )?;
    let initialization_production_present = initialized.mana_production().is_some();
    let initialization_payment_present = initialized.mana_payment().is_some();
    let mut state = initialized.into_state();
    let initial = native_state(&state, &probe.sources, "initial", None)?;
    let mut snapshots = vec![initial.snapshot.clone()];
    let mut steps = Vec::new();
    let mut callbacks = Vec::new();
    let mut selected_ability = Vec::new();
    let mut chosen_color = Vec::new();
    let mut produced = Vec::new();
    for activation in &probe.activations {
        let source = probe
            .sources
            .iter()
            .find(|source| source.alias == activation.source)
            .ok_or(EffectError::UnknownObject)?;
        let start_basis = state.basis().clone();
        let request = input(
            &state,
            A,
            GameCommand::ActivateMana {
                source: identity(&state, source.card)?,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(
                    source.ability,
                )),
            },
        );
        let begun = execute(
            &mut state,
            &probe.sources,
            "begin",
            activation.pending_label,
            None,
            request,
        )?;
        snapshots.push(begun.after.snapshot.clone());
        steps.push(begun);
        let pending = state.pending_decision().ok_or(GameError::NoDecision)?;
        let decision = pending.id();
        let RuleFrame::ManaColor(frame) = pending.frame() else {
            return Err(EffectError::WrongDecisionKind.into());
        };
        let callback = CallbackView {
            source: source.alias,
            chooser: seat(frame.chooser()),
            colors: frame.colors().iter().copied().map(symbol).collect(),
            selected: symbol(activation.color),
            amount: frame.amount().clone(),
            snapshot_index: snapshots.len() - 1,
        };
        selected_ability.push(AbilityView {
            produced: "Any",
            amount: frame.amount().clone(),
        });
        let rejected = [
            (
                "illegal-color",
                input(&state, A, reply(decision, ManaType::Colourless)),
            ),
            (
                "wrong-chooser",
                input(&state, B, reply(decision, activation.color)),
            ),
            (
                "wrong-decision",
                input(
                    &state,
                    A,
                    reply(DecisionId(decision.0 + 100), activation.color),
                ),
            ),
            (
                "wrong-kind",
                input(
                    &state,
                    A,
                    GameCommand::Reply {
                        decision,
                        choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
                    },
                ),
            ),
            ("pending-pass", input(&state, A, GameCommand::Pass)),
            (
                "stale-basis",
                GameInput {
                    basis: start_basis,
                    actor: A,
                    command: reply(decision, activation.color),
                },
            ),
        ];
        for (label, request) in rejected {
            steps.push(execute(
                &mut state,
                &probe.sources,
                label,
                activation.pending_label,
                None,
                request,
            )?);
        }
        let request = input(&state, A, reply(decision, activation.color));
        let completed = execute(
            &mut state,
            &probe.sources,
            "reply",
            activation.completed_label,
            Some(true),
            request,
        )?;
        let production = completed
            .production
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?;
        // Read the selected output from the actual record, rather than constructing
        // a pool or a result by applying the scripted color ourselves.
        let actual_selected = production
            .produced
            .iter()
            .find(|(_, amount)| amount.is_positive())
            .map(|(kind, _)| *kind)
            .ok_or(EffectError::InconsistentProgram)?;
        callbacks.push(CallbackView {
            selected: actual_selected,
            ..callback
        });
        chosen_color.push(actual_selected);
        produced.push(production.produced.clone());
        snapshots.push(completed.after.snapshot.clone());
        steps.push(completed);
        let request = input(&state, A, reply(decision, activation.color));
        steps.push(execute(
            &mut state,
            &probe.sources,
            "replayed-reply",
            activation.completed_label,
            Some(true),
            request,
        )?);
    }
    let request = input(&state, A, GameCommand::Pass);
    let followup = execute(
        &mut state,
        &probe.sources,
        "later-pass",
        "after-pass",
        Some(true),
        request,
    )?;
    Ok((
        CommonProbe {
            id: probe.id,
            source_cards: probe
                .sources
                .iter()
                .map(|source| (source.alias, source.name))
                .collect(),
            selected_ability: one_or_many(selected_ability),
            chosen_color: one_or_many(chosen_color),
            produced: one_or_many(produced),
            snapshots,
            callbacks,
        },
        NativeProbe {
            id: probe.id,
            initialization_production_present,
            initialization_payment_present,
            initial,
            steps,
            followup,
        },
    ))
}
fn source(
    alias: &'static str,
    name: &'static str,
    card: u64,
    owner: SeatId,
    ability: u64,
    amount: i64,
) -> Source {
    Source {
        alias,
        name,
        card: CardId(card),
        owner,
        ability,
        amount,
    }
}
fn run(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Result<Report, GameError> {
    let recipes = [
        (
            "gateway-colors",
            "Gateway Plaza",
            71,
            1,
            [
                "gateway-colors-w",
                "gateway-colors-u",
                "gateway-colors-b",
                "gateway-colors-r",
                "gateway-colors-g",
            ],
        ),
        (
            "lotus-vale-colors",
            "Lotus Vale",
            73,
            3,
            [
                "lotus-vale-colors-w",
                "lotus-vale-colors-u",
                "lotus-vale-colors-b",
                "lotus-vale-colors-r",
                "lotus-vale-colors-g",
            ],
        ),
        (
            "lotus-field-colors",
            "Lotus Field",
            79,
            3,
            [
                "lotus-field-colors-w",
                "lotus-field-colors-u",
                "lotus-field-colors-b",
                "lotus-field-colors-r",
                "lotus-field-colors-g",
            ],
        ),
    ];
    let mut probes = Vec::new();
    for (group, name, ability, amount, ids) in recipes {
        for (color, id) in COLORS.into_iter().zip(ids) {
            probes.push(Probe {
                group,
                id,
                sources: vec![source("source", name, 1, A, ability, amount)],
                activations: vec![Activation {
                    source: "source",
                    color,
                    pending_label: "color-choice",
                    completed_label: "activated",
                }],
            });
        }
    }
    probes.push(Probe {
        group: "controller-receives-mana",
        id: "stolen-lotus-field",
        sources: vec![source("source", "Lotus Field", 1, B, 79, 3)],
        activations: vec![Activation {
            source: "source",
            color: ManaType::Green,
            pending_label: "color-choice",
            completed_label: "activated",
        }],
    });
    probes.push(Probe {
        group: "sequential-sources",
        id: "plaza-field",
        sources: vec![
            source("plaza", "Gateway Plaza", 1, A, 71, 1),
            source("field", "Lotus Field", 2, A, 79, 3),
        ],
        activations: vec![
            Activation {
                source: "plaza",
                color: ManaType::Blue,
                pending_label: "plaza-color-choice",
                completed_label: "plaza-activated",
            },
            Activation {
                source: "field",
                color: ManaType::Red,
                pending_label: "field-color-choice",
                completed_label: "field-activated",
            },
        ],
    });
    let mut scenarios: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut native_observations = Vec::new();
    for (probe, index) in probes.into_iter().zip(1..) {
        let group = probe.group;
        let (common, native) = run_probe(vocabulary, index, probe)?;
        scenarios.entry(group).or_default().push(common);
        native_observations.push(native);
    }
    Ok(Report {
        schema_version: 1,
        kind: "tap-mana-color-choice",
        native_driver: "game-state-initialize-advance-v1",
        program_source: "synthetic-authored-color-choice-noncreature-land-v1; no Oracle registry bindings",
        scope: "Shared physical post-tap choice/completed snapshots and actual domain/chooser/output. Native asynchronous pending/reply, exact bases, immutable rejection, retained priority and current receipt lifetime are native-only; Forge uses a synchronous callback. No payment-time activations, real-card or full-game qualification.",
        numeric_encoding: "canonical decimal strings for rules integers",
        catalogue_bindings: 0,
        fully_qualified_cards: 0,
        full_game_comparisons: 0,
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

    let profile_token = "mana-color";
    if let Err(error) = spellwake_reference_tools::current_capture::start(
        profile_token,
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    ) {
        eprintln!("current capture source unavailable: {error}");
        return ExitCode::FAILURE;
    }
    let (response, success): (ApiResponse<Report>, bool) = match run(&vocabulary) {
        Ok(data) => (ApiResponse::Success { data }, true),
        Err(error) => {
            eprintln!("native mana color-choice trace failed: {error:?}");
            (
                ApiResponse::Error {
                    message: "Native mana color-choice trace unavailable".into(),
                    code: "native-mana-color-choice-error".into(),
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
        eprintln!("native mana color-choice output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
