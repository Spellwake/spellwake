//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual registry primitives and separately labeled scheduled GameState draws.
//! No expectation/reference files are consumed by this runner.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    draw::{DrawEvent, DrawJournal, GameOutcome},
    effects::{Program, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState},
    object_identity::{CardId, GameObjects, ObjectDestination, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    turn::{Step, TurnStart},
};
use std::{collections::BTreeMap, process::ExitCode};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn id(name: &str) -> Result<CardId, String> {
    Ok(CardId(match name {
        "a" => 90,
        "b" => 1,
        "c" => 8,
        "d" => 6,
        "e" => 3,
        "f" => 4,
        "z" => 77,
        _ => return Err("unknown fixture alias".into()),
    }))
}
fn alias(card: CardId) -> Result<&'static str, String> {
    match card.0 {
        90 => Ok("a"),
        1 => Ok("b"),
        8 => Ok("c"),
        6 => Ok("d"),
        3 => Ok("e"),
        4 => Ok("f"),
        77 => Ok("z"),
        _ => Err("unknown actual card instance".into()),
    }
}
fn seat(player: SeatId) -> &'static str {
    match player.0 {
        0 => "A",
        1 => "B",
        _ => "C",
    }
}
fn zone(value: ObjectZone) -> &'static str {
    match value {
        ObjectZone::Library(_) => "Library",
        ObjectZone::Hand(_) => "Hand",
        ObjectZone::Graveyard(_) => "Graveyard",
        ObjectZone::Battlefield => "Battlefield",
        ObjectZone::Stack => "Stack",
        ObjectZone::Exile => "Exile",
        ObjectZone::Command => "Command",
    }
}
#[derive(Serialize)]
struct Player {
    library: Vec<&'static str>,
    hand: Vec<&'static str>,
}
#[derive(Serialize)]
struct Object {
    id: String,
    zone: &'static str,
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
    mode: &'static str,
    players: BTreeMap<&'static str, Player>,
    objects: BTreeMap<&'static str, Object>,
    outcome: Option<&'static str>,
    pending_eliminations: Vec<u32>,
    drawn: Vec<&'static str>,
    failed_attempts: RulesInteger,
    priority: Option<&'static str>,
    step: Option<&'static str>,
}
fn observe_cards(
    scenario: &'static str,
    label: &'static str,
    cards: &GameObjects,
    seats: &[SeatId],
    journal: &DrawJournal,
) -> Result<Snapshot, String> {
    let mut players = BTreeMap::new();
    for &player in seats {
        let library = cards
            .library(player)
            .iter()
            .map(|object| {
                cards
                    .get(*object)
                    .ok_or("index object absent".into())
                    .and_then(|card| {
                        alias(card.card_id().expect("source-authored card-only trace"))
                    })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut hand = cards
            .iter()
            .filter(|card| card.zone() == ObjectZone::Hand(player))
            .map(|card| alias(card.card_id().expect("source-authored card-only trace")))
            .collect::<Result<Vec<_>, String>>()?;
        hand.sort();
        players.insert(seat(player), Player { library, hand });
    }
    let objects = cards
        .iter()
        .map(|card| {
            Ok((
                alias(card.card_id().expect("source-authored card-only trace"))?,
                Object {
                    id: format!("{:?}", card.id()),
                    zone: zone(card.zone()),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut drawn = vec![];
    let mut failed = RulesInteger::from(0);
    for event in &journal.events {
        match event {
            DrawEvent::Drawn { before, .. } => drawn.push(alias(
                before.card_id().expect("source-authored card-only trace"),
            )?),
            DrawEvent::EmptyLibrary { attempts, .. } => failed = failed.plus(attempts),
        }
    }
    Ok(Snapshot {
        capture_point: None,
        scenario,
        label,
        mode: "registry-primitive",
        players,
        objects,
        outcome: None,
        pending_eliminations: vec![],
        drawn,
        failed_attempts: failed,
        priority: None,
        step: None,
    })
}
fn observe(
    scenario: &'static str,
    label: &'static str,
    cards: &GameObjects,
    seats: &[SeatId],
    journal: &DrawJournal,
) -> Result<Snapshot, String> {
    let mut snapshot = observe_cards(scenario, label, cards, seats, journal)?;
    snapshot.capture_point = spellwake_reference_tools::capture_points::token(
        concat!(file!(), "::observe"),
        spellwake_reference_tools::current_profile::SnapshotOwner::GameObjects {
            cards,
            participants: seats,
        },
        &spellwake_reference_tools::packet::AliasContext::default(),
    )
    .expect("current native observation point");
    Ok(snapshot)
}
fn observe_game(
    scenario: &'static str,
    label: &'static str,
    state: &GameState,
    journal: &DrawJournal,
) -> Result<Snapshot, String> {
    let mut snapshot = observe_cards(
        scenario,
        label,
        state.execution().window().cards(),
        state.execution().window().participants(),
        journal,
    )?;
    snapshot.capture_point = spellwake_reference_tools::capture_points::token(
        concat!(file!(), "::observe_game"),
        spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
        &spellwake_reference_tools::packet::AliasContext::default(),
    )
    .expect("current native observation point");
    snapshot.mode = "game-state";
    snapshot.outcome = state.outcome().map(|result| match result {
        GameOutcome::Winner(player) => seat(player),
        GameOutcome::Draw => "draw",
    });
    snapshot.pending_eliminations = state
        .pending_eliminations()
        .iter()
        .map(|seat| seat.0)
        .collect();
    snapshot.priority = match state.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(seat(holder)),
        _ => None,
    };
    snapshot.step = state.turn().map(|schedule| match schedule.position().step {
        Step::Draw => "DRAW",
        Step::PrecombatMain => "MAIN1",
        Step::Upkeep => "UPKEEP",
        _ => "OTHER",
    });
    Ok(snapshot)
}
fn seed(
    cards: GameObjects,
    name: &str,
    owner: SeatId,
    dest: ObjectDestination,
) -> Result<GameObjects, String> {
    spellwake_reference_tools::current_capture::card_seed_card(
        &cards,
        cards.revision(),
        id(name)?,
        owner,
        dest,
        &[A, B],
    )
    .map(|change| change.next)
    .map_err(|e| format!("{e:?}"))
}
fn prepared(
    seats: Vec<SeatId>,
    active: SeatId,
    libraries: Vec<(SeatId, Vec<&str>)>,
    start: TurnStart,
) -> Result<spellwake_engine::game::NativeCandidate, String> {
    let mut cards = vec![];
    for (owner, names) in libraries {
        for name in names {
            cards.push(SeedCard {
                card: id(name)?,
                owner,
                destination: ObjectDestination::LibraryBottom,
                program: Program::creature(
                    None,
                    1.into(),
                    1.into(),
                    std::collections::BTreeSet::new(),
                ),
            });
        }
    }
    let context = GameContext::new(
        "fixture".into(),
        spellwake_reference_tools::creature_types::selected_vocabulary()?
            .rules_revision()
            .to_owned(),
        "authored".into(),
        "library-v1".into(),
        "individual-turns".into(),
        spellwake_reference_tools::creature_types::selected_vocabulary()?.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .map_err(|e| format!("{e:?}"))?;
    spellwake_reference_tools::current_capture::initialize_scheduled(
        GameId(1000),
        context,
        GameSeed::with_uniform_life(
            seats,
            active,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            spellwake_reference_tools::creature_types::selected_vocabulary()?,
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
        start,
    )
    .map_err(|e| format!("{e:?}"))
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    scope: &'static str,
    snapshots: Vec<Snapshot>,
}
fn run() -> Result<Report, String> {
    let mut snapshots = vec![];
    for (scenario, names, count) in [
        ("ordered-single", vec!["a", "b", "c"], 1),
        ("ordered-multiple", vec!["a", "b", "c"], 2),
        ("last-card-safe", vec!["a"], 1),
        ("empty-draw-deferred", vec![], 1),
        ("partial-batch-deferred", vec!["a"], 3),
    ] {
        let mut cards = seed(
            spellwake_reference_tools::current_capture::card_new(0, &[A, B]),
            "z",
            B,
            ObjectDestination::LibraryBottom,
        )?;
        for name in names {
            cards = seed(cards, name, A, ObjectDestination::LibraryBottom)?;
        }
        snapshots.push(observe(
            scenario,
            "before",
            &cards,
            &[A, B],
            &DrawJournal::default(),
        )?);
        let result = spellwake_reference_tools::current_capture::card_draw(
            &cards,
            cards.revision(),
            A,
            count.into(),
            &[A, B],
        )
        .map_err(|e| format!("{e:?}"))?;
        snapshots.push(observe(
            scenario,
            "after-draw-before-sba",
            &result.next,
            &[A, B],
            &result.journal,
        )?);
    }
    for (scenario, names) in [
        ("last-card-safe", vec!["a"]),
        ("empty-draw-deferred", vec![]),
    ] {
        let candidate = prepared(
            vec![A, B],
            A,
            vec![(A, names), (B, vec!["z"])],
            TurnStart {
                number: 2,
                step: Step::Draw,
            },
        )?;
        snapshots.push(observe_game(
            scenario,
            "after-sba",
            candidate.state(),
            candidate.draw_journal(),
        )?);
    }
    let first = prepared(
        vec![A, B],
        A,
        vec![(A, vec!["a", "b"]), (B, vec!["c", "d"])],
        TurnStart::default(),
    )?;
    let mut state = first.into_state();
    let mut latest = DrawJournal::default();
    for player in [A, B] {
        let candidate = spellwake_reference_tools::current_capture::advance(
            &state,
            GameInput {
                basis: state.basis().clone(),
                actor: player,
                command: GameCommand::Pass,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
        latest = candidate.draw_journal().clone();
        state = candidate.into_state();
    }
    snapshots.push(observe_game(
        "first-draw-two-player",
        "A-first-main-priority",
        &state,
        &latest,
    )?);
    // A separate prepared turn-two fixture reaches the same B draw state. Native
    // combat still suspends; this is not a complete native first-turn traversal.
    let second = prepared(
        vec![A, B],
        B,
        vec![(A, vec!["a", "b"]), (B, vec!["c", "d"])],
        TurnStart {
            number: 2,
            step: Step::Draw,
        },
    )?;
    snapshots.push(observe_game(
        "first-draw-two-player",
        "draw-priority",
        second.state(),
        second.draw_journal(),
    )?);
    let mut state = prepared(
        vec![A, B, SeatId(2)],
        A,
        vec![
            (A, vec!["a", "b"]),
            (B, vec!["c", "d"]),
            (SeatId(2), vec!["e", "f"]),
        ],
        TurnStart::default(),
    )?
    .into_state();
    for player in [A, B, SeatId(2)] {
        let candidate = spellwake_reference_tools::current_capture::advance(
            &state,
            GameInput {
                basis: state.basis().clone(),
                actor: player,
                command: GameCommand::Pass,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
        latest = candidate.draw_journal().clone();
        state = candidate.into_state();
    }
    snapshots.push(observe_game(
        "first-draw-multiplayer",
        "draw-priority",
        &state,
        &latest,
    )?);
    let lost = prepared(
        vec![A, B, SeatId(2)],
        A,
        vec![],
        TurnStart {
            number: 2,
            step: Step::Draw,
        },
    )?;
    snapshots.push(observe_game(
        "multiplayer-empty-loss",
        "blocked",
        lost.state(),
        lost.draw_journal(),
    )?);
    let mut cards = spellwake_reference_tools::current_capture::card_new(0, &[A, B]);
    for name in ["a", "b", "c"] {
        cards = seed(cards, name, A, ObjectDestination::LibraryBottom)?;
    }
    for name in ["d", "e"] {
        cards = seed(cards, name, A, ObjectDestination::Hand)?;
    }
    cards = seed(cards, "z", B, ObjectDestination::LibraryBottom)?;
    snapshots.push(observe(
        "library-positioning",
        "before",
        &cards,
        &[A, B],
        &DrawJournal::default(),
    )?);
    for (name, destination, label) in [
        ("d", ObjectDestination::LibraryTop, "top-inserted"),
        ("e", ObjectDestination::LibraryBottom, "bottom-inserted"),
    ] {
        let object = cards
            .current_identity(id(name)?)
            .ok_or("missing actual insertion object")?;
        cards = spellwake_reference_tools::current_capture::card_move(
            &cards,
            cards.revision(),
            object,
            destination,
            &[A, B],
        )
        .and_then(spellwake_engine::object_identity::ZoneMoveResult::require_moved)
        .map_err(|e| format!("{e:?}"))?
        .next;
        snapshots.push(observe(
            "library-positioning",
            label,
            &cards,
            &[A, B],
            &DrawJournal::default(),
        )?);
    }
    for (order, label) in [
        (vec!["c", "d", "a", "b", "e"], "same-library-top"),
        (vec!["d", "a", "b", "e", "c"], "same-library-bottom"),
    ] {
        let order = order
            .into_iter()
            .map(|name| {
                cards
                    .current_identity(id(name)?)
                    .ok_or("missing actual reorder member".into())
            })
            .collect::<Result<Vec<_>, String>>()?;
        cards = spellwake_reference_tools::current_capture::card_reorder(
            &cards,
            cards.revision(),
            A,
            order,
            &[A, B],
        )
        .map_err(|e| format!("{e:?}"))?;
        snapshots.push(observe(
            "library-positioning",
            label,
            &cards,
            &[A, B],
            &DrawJournal::default(),
        )?);
    }
    Ok(Report {
        schema_version: 1,
        scope: "Registry primitive rows are not SBA/player-state execution. Scheduled rows use actual GameState. B turn two is separately prepared; no native full-turn combat traversal, replacements/triggers, hidden-info projection or multiplayer elimination is claimed.",
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

    let profile_token = "library";
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
        Err(error) => {
            eprintln!("native library trace failed: {error}");
            (
                ApiResponse::Error {
                    message: "Native library trace unavailable".into(),
                    code: "native-library-draw".into(),
                },
                ExitCode::FAILURE,
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
        eprintln!("library trace encoding failed: {error}");
        return ExitCode::FAILURE;
    }
    code
}
