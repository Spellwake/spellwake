//! Actual native scheduler control flow and separate GameState resource probes.
//! Scripted mandatory-work notifications do not implement drawing or combat.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{Instruction, Program, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{PriorityCompleted, Step, TurnStage, TurnStart, TurnWork, WorkCompleted},
};
use std::{collections::BTreeMap, process::ExitCode};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn step(step: Step) -> &'static str {
    match step {
        Step::Untap => "UNTAP",
        Step::Upkeep => "UPKEEP",
        Step::Draw => "DRAW",
        Step::PrecombatMain => "MAIN1",
        Step::BeginningCombat => "COMBAT_BEGIN",
        Step::DeclareAttackers => "COMBAT_DECLARE_ATTACKERS",
        Step::DeclareBlockers => "COMBAT_DECLARE_BLOCKERS",
        Step::CombatDamage => "COMBAT_DAMAGE",
        Step::EndCombat => "COMBAT_END",
        Step::PostcombatMain => "MAIN2",
        Step::End => "END_OF_TURN",
        Step::Cleanup => "CLEANUP",
    }
}
fn seat(value: SeatId) -> &'static str {
    if value == A { "A" } else { "B" }
}
#[derive(Serialize)]
struct Flow {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    scenario: &'static str,
    trusted_work_notifications: bool,
    priority_steps_turn1: Vec<&'static str>,
    priority_steps_turn2: Vec<&'static str>,
    cleanup_priority_iterations: Vec<u64>,
    cleanup_entries_turn1: usize,
}
fn flow(
    scenario: &'static str,
    attacking: bool,
    additional: bool,
    repeat_cleanup: bool,
) -> Result<Flow, String> {
    let mut scheduler =
        spellwake_reference_tools::current_capture::turn_new(TurnStart::default(), true)
            .map_err(|e| format!("{e:?}"))?;
    let mut result = Flow {
        capture_point: None,
        scenario,
        trusted_work_notifications: true,
        priority_steps_turn1: vec![],
        priority_steps_turn2: vec![],
        cleanup_priority_iterations: vec![],
        cleanup_entries_turn1: 0,
    };
    for _ in 0..100 {
        let pos = scheduler.position();
        if pos.number == 3 {
            let actual = serde_json::to_value(&result).map_err(|error| error.to_string())?;
            result.capture_point = spellwake_reference_tools::capture_points::token(
                concat!(file!(), "::flow"),
                spellwake_reference_tools::current_profile::SnapshotOwner::PureValue(&actual),
                &spellwake_reference_tools::packet::AliasContext::default(),
            )?;
            return Ok(result);
        }
        if pos.number == 1 && scheduler.stage() == TurnStage::Work(TurnWork::Cleanup) {
            result.cleanup_entries_turn1 += 1;
        }
        scheduler = match scheduler.stage() {
            TurnStage::Work(work) => spellwake_reference_tools::current_capture::turn_work(
                &scheduler,
                match work {
                    TurnWork::Untap => WorkCompleted::Untap,
                    TurnWork::Draw => WorkCompleted::Draw,
                    TurnWork::DeclareAttackers => WorkCompleted::DeclareAttackers,
                    TurnWork::DeclareBlockers => WorkCompleted::DeclareBlockers,
                    TurnWork::CombatDamage { additional: second } => WorkCompleted::CombatDamage {
                        additional_step_required: additional && !second,
                    },
                    TurnWork::Cleanup => WorkCompleted::Cleanup {
                        priority_required: repeat_cleanup
                            && pos.number == 1
                            && pos.cleanup_iteration == 0,
                    },
                },
            ),
            TurnStage::Priority => {
                let name = if pos.step == Step::CombatDamage && additional && !pos.additional_damage
                {
                    "COMBAT_FIRST_STRIKE_DAMAGE"
                } else {
                    step(pos.step)
                };
                if pos.number == 1 {
                    result.priority_steps_turn1.push(name);
                } else {
                    result.priority_steps_turn2.push(name);
                }
                if pos.step == Step::Cleanup {
                    result
                        .cleanup_priority_iterations
                        .push(pos.cleanup_iteration);
                }
                spellwake_reference_tools::current_capture::turn_priority(
                    &scheduler,
                    if pos.step == Step::DeclareAttackers {
                        PriorityCompleted::Attackers {
                            declared_or_put_attacking: attacking,
                        }
                    } else {
                        PriorityCompleted::Ordinary
                    },
                )
            }
        }
        .map_err(|e| format!("{e:?}"))?;
    }
    Err("scheduler failed to reach the third turn within bounded fixture steps".into())
}
#[derive(Serialize)]
struct Snapshot {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    scenario: &'static str,
    label: &'static str,
    turn: u64,
    active: &'static str,
    step: &'static str,
    priority: Option<&'static str>,
    stack_size: usize,
    pools: BTreeMap<&'static str, BTreeMap<&'static str, RulesInteger>>,
    target_zone: Option<&'static str>,
    response_zone: Option<&'static str>,
}
fn zone(state: &GameState, card: CardId) -> Option<&'static str> {
    let cards = state.execution().window().cards();
    match cards.get(cards.current_identity(card)?)?.zone() {
        ObjectZone::Hand(_) => Some("Hand"),
        ObjectZone::Graveyard(_) => Some("Graveyard"),
        ObjectZone::Battlefield => Some("Battlefield"),
        ObjectZone::Stack => Some("Stack"),
        _ => Some("Other"),
    }
}
fn observe(
    scenario: &'static str,
    label: &'static str,
    state: &GameState,
) -> Result<Snapshot, String> {
    let position = state.turn().ok_or("missing actual schedule")?.position();
    let pools = [A, B]
        .into_iter()
        .map(|player| {
            (
                seat(player),
                [
                    ("W", ManaType::White),
                    ("U", ManaType::Blue),
                    ("B", ManaType::Black),
                    ("R", ManaType::Red),
                    ("G", ManaType::Green),
                    ("C", ManaType::Colourless),
                ]
                .into_iter()
                .map(|(name, kind)| (name, state.execution().mana(player, kind)))
                .collect(),
            )
        })
        .collect();
    Ok(Snapshot {
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::snapshot"),
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        scenario,
        label,
        turn: position.number,
        active: seat(state.execution().window().active_player()),
        step: step(position.step),
        priority: match state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(seat(holder)),
            _ => None,
        },
        stack_size: state.execution().window().entries().len(),
        pools,
        target_zone: zone(state, CardId(1)),
        response_zone: zone(state, CardId(2)),
    })
}
fn setup(
    start: Step,
    pools: bool,
    payer_type: ManaType,
    cards: Vec<SeedCard>,
) -> Result<GameState, String> {
    let context = GameContext::new(
        "fixture".into(),
        spellwake_reference_tools::creature_types::selected_vocabulary()?
            .rules_revision()
            .to_owned(),
        "authored".into(),
        "turn-v1".into(),
        "individual-two-player".into(),
        spellwake_reference_tools::creature_types::selected_vocabulary()?.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .map_err(|e| format!("{e:?}"))?;
    let mana = if pools {
        vec![
            FloatingMana {
                player: A,
                mana_type: payer_type,
                amount: 2.into(),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Colourless,
                amount: 3.into(),
            },
        ]
    } else {
        vec![]
    };
    spellwake_reference_tools::current_capture::initialize_scheduled(
        GameId(800),
        context,
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
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                spellwake_reference_tools::creature_types::selected_vocabulary()?,
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart {
            number: 1,
            step: start,
        },
    )
    .map(|candidate| candidate.into_state())
    .map_err(|e| format!("{e:?}"))
}
fn advance(state: &GameState, actor: SeatId, command: GameCommand) -> Result<GameState, String> {
    spellwake_reference_tools::current_capture::advance(
        state,
        GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        },
    )
    .map(|candidate| candidate.into_state())
    .map_err(|e| format!("{e:?}"))
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    scope: &'static str,
    flows: Vec<Flow>,
    snapshots: Vec<Snapshot>,
}
fn run() -> Result<Report, String> {
    let mut snapshots = vec![];
    let initial = setup(Step::Untap, true, ManaType::White, vec![])?;
    snapshots.push(observe("boundary-mana", "first-upkeep", &initial)?);
    for (start, seeded, one, after) in [
        (
            Step::Upkeep,
            "upkeep-seeded",
            "upkeep-after-one-pass",
            "after-upkeep",
        ),
        (
            Step::PrecombatMain,
            "main-seeded",
            "main-after-one-pass",
            "after-main",
        ),
    ] {
        let mut state = setup(start, true, ManaType::White, vec![])?;
        snapshots.push(observe("boundary-mana", seeded, &state)?);
        state = advance(&state, A, GameCommand::Pass)?;
        snapshots.push(observe("boundary-mana", one, &state)?);
        state = advance(&state, B, GameCommand::Pass)?;
        snapshots.push(observe("boundary-mana", after, &state)?);
    }
    let mut state = setup(
        Step::Upkeep,
        true,
        ManaType::Blue,
        vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: Program::creature(
                    None,
                    1.into(),
                    3.into(),
                    std::collections::BTreeSet::new(),
                ),
            },
            SeedCard {
                card: CardId(2),
                owner: A,
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
            },
        ],
    )?;
    snapshots.push(observe("stack-resolution", "seeded", &state)?);
    let cards = state.execution().window().cards();
    let spell = cards
        .current_identity(CardId(2))
        .ok_or("missing actual instant")?;
    let target = cards
        .current_identity(CardId(1))
        .ok_or("missing actual target")?;
    state = advance(
        &state,
        A,
        GameCommand::Cast {
            spell,
            target: TargetRef::Object(target),
        },
    )?;
    snapshots.push(observe("stack-resolution", "after-cast", &state)?);
    state = advance(&state, A, GameCommand::Pass)?;
    snapshots.push(observe("stack-resolution", "after-first-pass", &state)?);
    state = advance(&state, B, GameCommand::Pass)?;
    snapshots.push(observe("stack-resolution", "after-resolution", &state)?);
    state = advance(&state, A, GameCommand::Pass)?;
    snapshots.push(observe(
        "stack-resolution",
        "after-resolution-first-pass",
        &state,
    )?);
    state = advance(&state, B, GameCommand::Pass)?;
    snapshots.push(observe("stack-resolution", "after-step", &state)?);
    Ok(Report {
        schema_version: 1,
        scope: "Pure scheduler control flow uses disclosed trusted work notifications. Physical snapshots use actual GameState. No native draw/combat/trigger/discard implementation or full-game qualification.",
        flows: vec![
            flow("turn-order", false, false, false)?,
            flow("combat-order", true, false, false)?,
            flow("additional-damage", true, true, false)?,
            flow("cleanup-repeat", false, false, true)?,
        ],
        snapshots,
    })
}
fn main() -> ExitCode {
    let native_arguments = spellwake_reference_tools::creature_types::qualification_args()
        .unwrap_or_else(|error| {
            eprintln!("invalid qualification input: {error}");
            std::process::exit(1)
        });
    if !native_arguments.is_empty() {
        eprintln!("unsupported qualification arguments");
        std::process::exit(1);
    }

    let profile_token = "scheduler";
    if let Err(error) = spellwake_reference_tools::current_capture::start(
        profile_token,
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    ) {
        eprintln!("current capture source unavailable: {error}");
        return ExitCode::FAILURE;
    }
    let (response, code): (ApiResponse<Report>, ExitCode) = match run() {
        Ok(data) => (ApiResponse::Success { data }, ExitCode::SUCCESS),
        Err(message) => (
            ApiResponse::Error {
                message,
                code: "native-turn-trace".into(),
            },
            ExitCode::FAILURE,
        ),
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
    match serde_json::to_string_pretty(&response) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("failed to encode native turn evidence: {error}");
            return ExitCode::FAILURE;
        }
    }
    code
}
