//! Actual opening decisions and separate deterministic registry shuffle probes.
//! Never reads expected or reference data; private fixture entropy is not a
//! production entropy source, public view or whole-card/game qualification.
use serde::Serialize;
use spellwake_contracts::ApiResponse;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{Program, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, OpeningSetup,
        RuleFrame,
    },
    object_identity::{CardId, ObjectDestination, ObjectZone},
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    turn::Step,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::ExitCode,
};
fn seat(player: SeatId) -> &'static str {
    match player.0 {
        0 => "A",
        1 => "B",
        _ => "C",
    }
}
#[derive(Serialize)]
struct Choice {
    kind: &'static str,
    player: &'static str,
    hand_size: usize,
    charged: usize,
    count: usize,
    selected_count: usize,
    phase: String,
    priority_granted: bool,
}
#[derive(Serialize)]
struct Shuffle {
    player: &'static str,
    algorithm: &'static str,
    card_count: usize,
    entropy_start: usize,
    entropy_end: usize,
    identity_retained: bool,
}
#[derive(Serialize)]
struct Scenario {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    id: &'static str,
    players: usize,
    mulligans: BTreeMap<&'static str, u8>,
    final_hand_sizes: BTreeMap<&'static str, usize>,
    final_library_sizes: BTreeMap<&'static str, usize>,
    tuck_counts: BTreeMap<&'static str, Vec<usize>>,
    declaration_counts: BTreeMap<&'static str, usize>,
    choices: Vec<Choice>,
    shuffles: Vec<Shuffle>,
    opening_complete: bool,
    phase: &'static str,
    priority: Option<&'static str>,
    stack_size: usize,
    entropy_position: usize,
}
fn hand(state: &GameState, player: SeatId) -> usize {
    state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|card| card.zone() == ObjectZone::Hand(player))
        .count()
}
fn shuffles(rows: &[spellwake_engine::shuffle::ShuffleRecord]) -> Vec<Shuffle> {
    rows.iter()
        .map(|record| Shuffle {
            player: seat(record.player),
            algorithm: record.algorithm,
            card_count: record.before.len(),
            entropy_start: record.entropy_start,
            entropy_end: record.entropy_end,
            identity_retained: record.before.len() == record.after.len()
                && record.before.iter().copied().collect::<BTreeSet<_>>()
                    == record.after.iter().copied().collect(),
        })
        .collect()
}
fn scenario(id: &'static str, goals: &[u8]) -> Result<Scenario, String> {
    let seats = (0..goals.len())
        .map(|i| SeatId(i as u32))
        .collect::<Vec<_>>();
    let mut cards = vec![];
    for &player in &seats {
        for index in 0..12 {
            cards.push(SeedCard {
                card: CardId(u64::from(player.0) * 100 + index + 1),
                owner: player,
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
    let ctx = GameContext::new(
        "fixture".into(),
        spellwake_reference_tools::creature_types::selected_vocabulary()?
            .rules_revision()
            .to_owned(),
        "authored".into(),
        "opening-v1".into(),
        "ordinary-individual".into(),
        spellwake_reference_tools::creature_types::selected_vocabulary()?.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .map_err(|e| format!("{e:?}"))?;
    let initial = spellwake_reference_tools::current_capture::initialize_opening(
        GameId(1300),
        ctx,
        OpeningSetup {
            seed: GameSeed::with_uniform_life(
                seats.clone(),
                SeatId(0),
                cards,
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                spellwake_reference_tools::creature_types::selected_vocabulary()?,
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            entropy: EntropyTape::new(vec![0; 5000]),
        },
        vec![0; 5000],
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut records = shuffles(initial.shuffle_journal());
    let mut state = initial.into_state();
    let mut choices = vec![];
    let mut declarations = seats
        .iter()
        .map(|&p| (seat(p), 0))
        .collect::<BTreeMap<_, usize>>();
    let mut tucks = seats
        .iter()
        .map(|&p| (seat(p), vec![]))
        .collect::<BTreeMap<_, Vec<usize>>>();
    for _ in 0..200 {
        if state.opening().is_some_and(|opening| opening.complete()) {
            break;
        }
        let decision = state.pending_decision().ok_or("opening decision missing")?;
        let player = decision.chooser();
        let charged = state.opening().ok_or("opening missing")?.charged(player);
        let (kind, count, selected_count, reply) = match decision.frame() {
            RuleFrame::Opening(OpeningDecision::Declare { taken, .. }) => {
                *declarations
                    .get_mut(seat(player))
                    .ok_or("missing declaration seat")? += 1;
                (
                    "declare",
                    0,
                    0,
                    DecisionReply::Mulligan(if *taken < goals[player.0 as usize] {
                        MulliganChoice::Mulligan
                    } else {
                        MulliganChoice::Keep
                    }),
                )
            }
            RuleFrame::Opening(OpeningDecision::Bottom { count, hand, .. }) => {
                let selected = hand.iter().rev().take(*count).copied().collect::<Vec<_>>();
                (
                    "bottom",
                    *count,
                    selected.len(),
                    DecisionReply::Bottom(selected),
                )
            }
            _ => return Err("not an opening frame".into()),
        };
        choices.push(Choice {
            kind,
            player: seat(player),
            hand_size: hand(&state, player),
            charged,
            count,
            selected_count,
            phase: format!("{:?}", state.execution().window().phase()),
            priority_granted: matches!(
                state.execution().window().phase(),
                WindowPhase::Priority(PriorityPhase::Ready { .. })
            ),
        });
        let candidate = spellwake_reference_tools::current_capture::advance(
            &state,
            GameInput {
                basis: state.basis().clone(),
                actor: player,
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: reply,
                },
            },
        )
        .map_err(|e| format!("{e:?}"))?;
        for &seat_id in &seats {
            let before = state
                .opening()
                .ok_or("prior opening missing")?
                .mulligans_taken(seat_id);
            let after = candidate.state().opening().ok_or("next opening missing")?;
            if after.mulligans_taken(seat_id) > before {
                tucks
                    .get_mut(seat(seat_id))
                    .ok_or("missing tuck seat")?
                    .push(after.charged(seat_id));
            }
        }
        records.extend(shuffles(candidate.shuffle_journal()));
        state = candidate.into_state();
    }
    let opening = state.opening().ok_or("final opening missing")?;
    if !opening.complete() {
        return Err("opening fixture did not terminate".into());
    }
    Ok(Scenario {
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::scenario::final"),
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        id,
        players: seats.len(),
        mulligans: seats
            .iter()
            .map(|&p| (seat(p), opening.mulligans_taken(p)))
            .collect(),
        final_hand_sizes: seats.iter().map(|&p| (seat(p), hand(&state, p))).collect(),
        final_library_sizes: seats
            .iter()
            .map(|&p| (seat(p), state.execution().window().cards().library(p).len()))
            .collect(),
        tuck_counts: tucks,
        declaration_counts: declarations,
        choices,
        shuffles: records,
        opening_complete: opening.complete(),
        phase: if state
            .turn()
            .is_some_and(|turn| turn.position().step == Step::Upkeep)
        {
            "UPKEEP"
        } else {
            "OTHER"
        },
        priority: match state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(seat(holder)),
            _ => None,
        },
        stack_size: state.execution().window().entries().len(),
        entropy_position: state.entropy().ok_or("final entropy missing")?.position(),
    })
}
#[derive(Serialize)]
struct Small {
    size: usize,
    entropy_consumed: usize,
    record: bool,
    identity_retained: bool,
}
#[derive(Serialize)]
struct Properties {
    permutations: Vec<Vec<u64>>,
    identity_retained: Vec<bool>,
    entropy_consumed: Vec<usize>,
    small: Vec<Small>,
    rejection_words_consumed: usize,
}
fn shuffle_properties() -> Result<Properties, String> {
    let player = SeatId(0);
    let mut cards = spellwake_reference_tools::current_capture::card_new(0, &[player]);
    for id in [0, 1, 2] {
        cards = spellwake_reference_tools::current_capture::card_seed_card(
            &cards,
            cards.revision(),
            CardId(id),
            player,
            ObjectDestination::LibraryBottom,
            &[player],
        )
        .map_err(|e| format!("{e:?}"))?
        .next;
    }
    let mut permutations = vec![];
    let mut retained = vec![];
    let mut consumed = vec![];
    for first in 0..3 {
        for second in 0..2 {
            let result = spellwake_reference_tools::current_capture::card_shuffle(
                &cards,
                cards.revision(),
                player,
                &EntropyTape::new(vec![first, second]),
                &[first, second],
                &[player],
                false,
            )
            .map_err(|e| format!("{e:?}"))?;
            permutations.push(
                result
                    .next
                    .library(player)
                    .iter()
                    .map(|id| {
                        result
                            .next
                            .get(*id)
                            .and_then(|card| card.card_id().map(|id| id.0))
                            .ok_or("actual shuffle object absent".into())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            );
            retained.push(
                result
                    .record
                    .before
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    == result.record.after.iter().copied().collect(),
            );
            consumed.push(result.entropy.position());
        }
    }
    let mut small = vec![];
    for size in [0, 1] {
        let mut cards = spellwake_reference_tools::current_capture::card_new(0, &[player]);
        if size == 1 {
            cards = spellwake_reference_tools::current_capture::card_seed_card(
                &cards,
                cards.revision(),
                CardId(1),
                player,
                ObjectDestination::LibraryBottom,
                &[player],
            )
            .map_err(|e| format!("{e:?}"))?
            .next;
        }
        let result = spellwake_reference_tools::current_capture::card_shuffle(
            &cards,
            cards.revision(),
            player,
            &EntropyTape::new(vec![]),
            &[],
            &[player],
            false,
        )
        .map_err(|e| format!("{e:?}"))?;
        small.push(Small {
            size,
            entropy_consumed: result.entropy.position(),
            record: result.record.player == player,
            identity_retained: result.record.before == result.record.after,
        });
    }
    let rejection = spellwake_reference_tools::current_capture::card_shuffle(
        &cards,
        cards.revision(),
        player,
        &EntropyTape::new(vec![u64::MAX, 0, 0]),
        &[u64::MAX, 0, 0],
        &[player],
        false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let properties = Properties {
        permutations,
        identity_retained: retained,
        entropy_consumed: consumed,
        small,
        rejection_words_consumed: rejection.entropy.position(),
    };
    if spellwake_reference_tools::current_capture::is_active() {
        let actual = serde_json::to_value(&properties).map_err(|error| error.to_string())?;
        for key in [
            "permutations",
            "identity_retained",
            "entropy_consumed",
            "small",
        ] {
            for (index, value) in actual[key]
                .as_array()
                .ok_or("actual shuffle property array missing")?
                .iter()
                .enumerate()
            {
                let point = spellwake_reference_tools::current_capture::observation(
                    concat!(file!(), "::shuffle_properties"),
                    spellwake_reference_tools::current_profile::SnapshotOwner::PureValue(value),
                    &spellwake_reference_tools::packet::AliasContext::default(),
                )?;
                spellwake_reference_tools::current_capture::bind_observation(
                    &point,
                    &format!("/observations/data/shuffle_properties/{key}/{index}"),
                )?;
            }
        }
        let point = spellwake_reference_tools::current_capture::observation(
            concat!(file!(), "::shuffle_properties"),
            spellwake_reference_tools::current_profile::SnapshotOwner::PureValue(
                &actual["rejection_words_consumed"],
            ),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )?;
        spellwake_reference_tools::current_capture::bind_observation(
            &point,
            "/observations/data/shuffle_properties/rejection_words_consumed",
        )?;
    }
    Ok(properties)
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    scope: &'static str,
    scenarios: Vec<Scenario>,
    shuffle_properties: Properties,
}
fn run() -> Result<Report, String> {
    Ok(Report {
        schema_version: 1,
        scope: "Actual native GameState opening profiles and separate deterministic registry shuffle probes. Host entropy fixture is fixed, not a production random-source/security proof. Forge sequential protocol/eighth zero-hand prompt and pre-shuffle starting choice are explicit exclusions; no cross-engine random sequence, bottom identity/order, full card/game, startup-effect or public-view qualification.",
        scenarios: vec![
            scenario("initial-seven", &[0, 0])?,
            scenario("london-repeated", &[2, 0])?,
            scenario("multiplayer-free", &[2, 1, 0])?,
            scenario("keep-stops", &[0, 2])?,
            scenario("zero-hand-limit", &[7, 0])?,
        ],
        shuffle_properties: shuffle_properties()?,
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

    let profile_token = "opening";
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
            eprintln!("opening trace failed: {error}");
            (
                ApiResponse::Error {
                    message: "Native opening trace unavailable".into(),
                    code: "native-opening".into(),
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
        eprintln!("opening trace encoding failed: {error}");
        return ExitCode::FAILURE;
    }
    code
}
