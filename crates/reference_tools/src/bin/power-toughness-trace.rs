//! Native observations for synthetic, literal layer-7 programs, not complete cards.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    characteristics::{PowerToughness, PowerToughnessCounters, PowerToughnessOperation},
    effects::{EffectError, Instruction, Program, SeedCard},
    game::{GameCommand, GameContext, GameError, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
};
use std::{collections::BTreeMap, process::ExitCode};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const SUBJECT: CardId = CardId(1);

#[derive(Serialize)]
struct NumericPair {
    power: RulesInteger,
    toughness: RulesInteger,
}
impl From<PowerToughness> for NumericPair {
    fn from(values: PowerToughness) -> Self {
        Self {
            power: values.power,
            toughness: values.toughness,
        }
    }
}

#[derive(Serialize)]
struct EvaluationView {
    object: String,
    copiable: NumericPair,
    base: NumericPair,
    derived: NumericPair,
    applied_effect_ids: Vec<String>,
}

#[derive(Serialize)]
struct Observation {
    checkpoint: &'static str,
    power: RulesInteger,
    toughness: RulesInteger,
    plus_one_counters: RulesInteger,
    minus_one_counters: RulesInteger,
    zone: &'static str,
    damage: RulesInteger,
    stack_size: usize,
    source_zones: BTreeMap<&'static str, &'static str>,
    source_exit: BTreeMap<&'static str, bool>,
    native_evaluation: EvaluationView,
    native_game_revision: u64,
}

#[derive(Serialize)]
struct SourceDescription {
    alias: &'static str,
    owner_and_caster: &'static str,
    cost: &'static str,
    instruction: String,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: &'static str,
    native_driver: &'static str,
    program_source: &'static str,
    numeric_encoding: &'static str,
    scope: &'static str,
    subject: &'static str,
    subject_owner_and_controller: &'static str,
    initial_floating_mana: &'static str,
    full_game_comparisons: u32,
    fully_qualified_cards: u32,
    catalogue_bindings: u32,
    scenario_programs: BTreeMap<&'static str, Vec<SourceDescription>>,
    scenario_observations: BTreeMap<&'static str, Vec<Observation>>,
}

struct Source {
    card: CardId,
    alias: &'static str,
    checkpoint: &'static str,
    instruction: Instruction,
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
    sources: &[Source; 2],
) -> Result<Observation, GameError> {
    let object = identity(state, SUBJECT)?;
    let execution = state.execution();
    let evaluation = execution
        .power_toughness(object)
        .ok_or(EffectError::InconsistentProgram)?;
    let counters = execution
        .power_toughness_counters(object)
        .ok_or(EffectError::InconsistentProgram)?;
    let creature = execution
        .creature(object)
        .ok_or(EffectError::InconsistentProgram)?;
    let mut source_zones = BTreeMap::new();
    let mut source_exit = BTreeMap::new();
    for source in sources {
        let current_zone = zone(state, source.card)?;
        source_zones.insert(source.alias, zone_name(current_zone));
        // These checkpoints are initial or settled after successful resolution.
        source_exit.insert(source.alias, current_zone == CardZone::Graveyard(A));
    }
    Ok(Observation {
        checkpoint,
        power: evaluation.derived.power.clone(),
        toughness: evaluation.derived.toughness.clone(),
        plus_one_counters: counters.plus_one,
        minus_one_counters: counters.minus_one,
        zone: zone_name(zone(state, SUBJECT)?),
        damage: creature.damage,
        stack_size: execution.window().entries().len(),
        source_zones,
        source_exit,
        native_evaluation: EvaluationView {
            object: format!("{:?}", evaluation.object),
            copiable: evaluation.copiable.into(),
            base: evaluation.base.into(),
            derived: evaluation.derived.into(),
            applied_effect_ids: evaluation
                .applied_effects
                .iter()
                .map(|id| format!("{id:?}"))
                .collect(),
        },
        native_game_revision: state.basis().revision,
    })
}

fn advance(state: GameState, actor: SeatId, command: GameCommand) -> Result<GameState, GameError> {
    Ok(state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })?
        .into_state())
}

fn resolve_top(mut state: GameState) -> Result<GameState, GameError> {
    let entry = state
        .execution()
        .window()
        .entries()
        .last()
        .ok_or(GameError::UnexpectedKernelPhase)?
        .id();
    for _ in 0..state.execution().window().participants().len() {
        if state.pending_decision().is_some() {
            return Err(GameError::DecisionRequired);
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            return Err(GameError::UnexpectedKernelPhase);
        };
        state = advance(state, holder, GameCommand::Pass)?;
        if state
            .execution()
            .window()
            .entries()
            .iter()
            .all(|current| current.id() != entry)
        {
            return Ok(state);
        }
    }
    Err(GameError::UnexpectedKernelPhase)
}

fn run_scenario(
    game: GameId,
    profile: &'static str,
    operations: [(&'static str, &'static str, Instruction); 2],
) -> Result<(Vec<Observation>, Vec<SourceDescription>), GameError> {
    let [
        (first_alias, first_checkpoint, first),
        (second_alias, second_checkpoint, second),
    ] = operations;
    let sources = [
        Source {
            card: CardId(2),
            alias: first_alias,
            checkpoint: first_checkpoint,
            instruction: first,
        },
        Source {
            card: CardId(3),
            alias: second_alias,
            checkpoint: second_checkpoint,
            instruction: second,
        },
    ];
    let mut cards = vec![SeedCard {
        card: SUBJECT,
        owner: B,
        destination: CardDestination::Battlefield { controller: B },
        program: Program::VanillaCreature {
            power: 1.into(),
            toughness: 3.into(),
        },
    }];
    for source in &sources {
        cards.push(SeedCard {
            card: source.card,
            owner: A,
            destination: CardDestination::Hand,
            program: Program::Instant {
                cost: ManaCost::one(ManaType::Red),
                instructions: vec![source.instruction.clone()],
            },
        });
    }
    // Context labels describe this synthetic input, without claiming source qualification.
    let context = GameContext::new(
        "native-power-toughness-foundation-v1".into(),
        "magic-cr-20260925-literal-layer-7-subset".into(),
        "no-catalogue-synthetic-seeds".into(),
        "synthetic-literal-pt-programs-v1".into(),
        profile.into(),
    )?;
    let mut state = GameState::initialize(
        game,
        context,
        vec![A, B],
        A,
        cards,
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::Red,
            amount: 2.into(),
        }],
    )?
    .into_state();
    let target = TargetRef::Card(identity(&state, SUBJECT)?);
    let mut observations = vec![observe(&state, "initial", &sources)?];
    for source in &sources {
        let spell = identity(&state, source.card)?;
        state = advance(state, A, GameCommand::Cast { spell, target })?;
        state = resolve_top(state)?;
        observations.push(observe(&state, source.checkpoint, &sources)?);
    }
    let descriptions = sources
        .iter()
        .map(|source| SourceDescription {
            alias: source.alias,
            owner_and_caster: "A",
            cost: "one red mana (synthetic homogeneous cost)",
            instruction: format!("{:?}", source.instruction),
        })
        .collect();
    Ok((observations, descriptions))
}

fn continuous(operation: PowerToughnessOperation) -> Instruction {
    Instruction::ContinuousPowerToughness { operation }
}

fn pair(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

fn run() -> Result<Report, GameError> {
    let scenarios = [
        (
            "set-after-boost",
            [
                (
                    "growth",
                    "after-growth",
                    continuous(PowerToughnessOperation::Modify(pair(3, 3))),
                ),
                (
                    "diminish",
                    "after-diminish",
                    continuous(PowerToughnessOperation::Set(pair(1, 1))),
                ),
            ],
        ),
        (
            "switch-then-asymmetric-boost",
            [
                (
                    "switch",
                    "after-switch",
                    continuous(PowerToughnessOperation::Switch),
                ),
                (
                    "fists",
                    "after-fists",
                    continuous(PowerToughnessOperation::Modify(pair(4, 0))),
                ),
            ],
        ),
        (
            "counter-then-switch",
            [
                (
                    "counter",
                    "after-counter",
                    Instruction::PutPowerToughnessCounters {
                        counters: PowerToughnessCounters {
                            plus_one: 1.into(),
                            minus_one: 0.into(),
                        },
                    },
                ),
                (
                    "switch",
                    "after-switch",
                    continuous(PowerToughnessOperation::Switch),
                ),
            ],
        ),
    ];
    let mut scenario_observations = BTreeMap::new();
    let mut scenario_programs = BTreeMap::new();
    for (game, (name, operations)) in (1_u128..).zip(scenarios) {
        let (observations, programs) = run_scenario(GameId(game), name, operations)?;
        scenario_observations.insert(name, observations);
        scenario_programs.insert(name, programs);
    }
    Ok(Report {
        schema_version: 1,
        kind: "restricted-power-toughness-interactions",
        native_driver: "game-state-initialize-advance-v1",
        program_source: "synthetic-authored-programs-v1; no Oracle registry bindings",
        numeric_encoding: "canonical decimal strings for rules integers",
        scope: "Literal fixed-incarnation set/modify/switch and +1/+1 counter placement; settled native projections including source exits. Seeded mana, no printed durations/turn expiry, dynamic dependencies, other layers, real-card binding or full-card/full-game qualification.",
        subject: "synthetic vanilla creature, defined/copiable power 1 and toughness 3",
        subject_owner_and_controller: "B",
        initial_floating_mana: "A: two red; B: zero",
        full_game_comparisons: 0,
        fully_qualified_cards: 0,
        catalogue_bindings: 0,
        scenario_programs,
        scenario_observations,
    })
}

fn main() -> ExitCode {
    let (response, success): (ApiResponse<Report>, bool) = match run() {
        Ok(data) => (ApiResponse::Success { data }, true),
        Err(error) => {
            eprintln!("native power/toughness interaction failed: {error:?}");
            (
                ApiResponse::Error {
                    message: "Native power/toughness interaction unavailable".into(),
                    code: "native-power-toughness-error".into(),
                },
                false,
            )
        }
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
        eprintln!("native power/toughness trace output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
