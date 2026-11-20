//! Actual root lifecycle checks for continuous-control history, not attack legality.
//! Prepared ending boundaries cover rotation while combat remains unsupported.
use spellwake_engine::combat::CombatRules;
use spellwake_engine::{
    effects::{Program, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate,
        OpeningSetup, RuleFrame,
    },
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectZone},
    opening::MulliganChoice,
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    turn::{Step, TurnStage, TurnStart, TurnWork},
};
use std::collections::BTreeMap;

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);

fn context() -> GameContext {
    GameContext::new(
        "control-history-test".into(),
        "cr-302.6".into(),
        "literal-fixtures".into(),
        "partial-control-history".into(),
        "ordinary-individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn creature(id: u64, owner: SeatId, destination: ObjectDestination, toughness: i64) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::creature(
            Some(ManaCost::free()),
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}

fn permanents() -> Vec<SeedCard> {
    vec![
        creature(1, A, ObjectDestination::Battlefield { controller: A }, 3),
        SeedCard {
            card: CardId(2),
            owner: B,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::artifact(None, vec![]),
        },
        SeedCard {
            card: CardId(3),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::land(
                std::collections::BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(1),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
                }],
            ),
        },
        creature(4, A, ObjectDestination::Battlefield { controller: B }, 3),
        creature(5, A, ObjectDestination::Hand, 3),
        creature(6, B, ObjectDestination::LibraryBottom, 3),
    ]
}

fn prepared(number: u64, step: Step, cards: Vec<SeedCard>) -> NativeCandidate {
    GameState::initialize_scheduled(
        GameId(3026),
        context(),
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
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart { number, step },
    )
    .unwrap()
}

fn object(game: &GameState, id: u64) -> &GameObject {
    let cards = game.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(id)).unwrap())
        .unwrap()
}

fn act(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .unwrap()
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder, .. }) =
        game.execution().window().phase()
    else {
        panic!("expected ready priority")
    };
    holder
}

fn pass_round(game: &GameState) -> NativeCandidate {
    let first = act(game, holder(game), GameCommand::Pass).into_state();
    act(&first, holder(&first), GameCommand::Pass)
}

fn keep(game: &GameState) -> NativeCandidate {
    let decision = game.pending_decision().unwrap();
    act(
        game,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::Mulligan(MulliganChoice::Keep),
        },
    )
}

#[test]
fn actual_initial_untap_stamps_every_active_controlled_permanent_but_no_other_zone() {
    let initial = prepared(1, Step::Untap, permanents());
    let game = initial.state();
    assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(holder(game), A);
    for id in [1, 2, 3] {
        let card = object(game, id);
        assert!(card.control_epoch().is_some());
        assert_eq!(card.has_continuous_control_since_turn_start(), Some(true));
    }
    // Control, rather than ownership, selects both the stamped artifact and
    // the unstamped opponent-controlled creature.
    assert_eq!(object(game, 2).owner(), B);
    assert_eq!(
        object(game, 4).has_continuous_control_since_turn_start(),
        Some(false)
    );
    for id in [5, 6] {
        assert_eq!(object(game, id).control_epoch(), None);
        assert_eq!(
            object(game, id).has_continuous_control_since_turn_start(),
            None
        );
    }
    assert_eq!(
        initial
            .turn_transitions()
            .iter()
            .map(|transition| transition.to.step)
            .collect::<Vec<_>>(),
        vec![Step::Untap, Step::Upkeep]
    );
}

#[test]
fn prepared_midsteps_and_later_turn_numbers_do_not_invent_prior_control_history() {
    for (number, step) in [
        (1, Step::Upkeep),
        (23, Step::PrecombatMain),
        (23, Step::BeginningCombat),
        (23, Step::End),
    ] {
        let initial = prepared(number, step, permanents());
        assert_eq!(initial.state().turn().unwrap().position().step, step);
        for id in [1, 2, 3, 4] {
            assert_eq!(
                object(initial.state(), id).has_continuous_control_since_turn_start(),
                Some(false),
                "prepared {number:?}/{step:?}, card {id}"
            );
        }
    }
}

#[test]
fn ordinary_supported_steps_preserve_the_registry_without_restamping() {
    let mut game = prepared(1, Step::Untap, permanents()).into_state();
    let original = game.execution().window().cards().clone();
    for step in [Step::PrecombatMain, Step::BeginningCombat] {
        game = pass_round(&game).into_state();
        assert_eq!(game.turn().unwrap().position().step, step);
        assert_eq!(game.execution().window().cards(), &original);
    }
    let stopped = pass_round(&game);
    assert_eq!(
        stopped.state().pending_turn_work(),
        Some(TurnWork::DeclareAttackers)
    );
    let pending = stopped.state().pending_decision().unwrap();
    assert_eq!(pending.chooser(), A);
    let RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Attackers(frame)) =
        pending.frame()
    else {
        panic!("expected actual attacker choice")
    };
    assert_eq!(
        frame.eligible_attackers(),
        &[object(stopped.state(), 1).id()]
    );
    assert_eq!(stopped.state().execution().window().cards(), &original);
}

#[test]
fn creature_resolved_after_the_actual_turn_start_has_fresh_unstamped_history() {
    let initial = prepared(1, Step::Untap, permanents()).into_state();
    let main = pass_round(&initial).into_state();
    let established = object(&main, 1).clone();
    let hand = object(&main, 5).id();
    let cast = act(
        &main,
        A,
        GameCommand::CastUntargeted {
            spell: hand,
            payment: None,
        },
    )
    .into_state();
    assert_eq!(object(&cast, 5).zone(), ObjectZone::Stack);
    assert_eq!(object(&cast, 5).control_epoch(), None);
    let resolved = pass_round(&cast);
    let entrant = object(resolved.state(), 5);
    assert_eq!(entrant.zone(), ObjectZone::Battlefield);
    assert_ne!(entrant.id(), hand);
    assert_eq!(
        entrant.has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(object(resolved.state(), 1), &established);
    assert_eq!(
        resolved.state().turn().unwrap().position().step,
        Step::PrecombatMain
    );
    let entry = resolved.permanent_entry().unwrap();
    assert_eq!(entry.after, *entrant);
    assert_eq!(
        entry.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
}

#[test]
fn prepared_end_wrap_stamps_only_the_newly_active_controller_once() {
    // This fixture starts at End; it does not pretend native combat ran.
    let game = prepared(12, Step::End, permanents()).into_state();
    let revision = game.execution().window().cards().revision();
    let epoch = object(&game, 4).control_epoch();
    let next = pass_round(&game);
    assert_eq!(next.state().turn().unwrap().position().number, 13);
    assert_eq!(next.state().turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(next.state().execution().window().turn_owner(), B);
    assert_eq!(object(next.state(), 4).control_epoch(), epoch);
    assert_eq!(
        object(next.state(), 4).has_continuous_control_since_turn_start(),
        Some(true)
    );
    for id in [1, 2, 3] {
        assert_eq!(
            object(next.state(), id).has_continuous_control_since_turn_start(),
            Some(false)
        );
    }
    assert_eq!(
        next.state().execution().window().cards().revision(),
        revision + 1
    );
}

#[test]
fn prepared_cleanup_exception_and_repeat_do_not_stamp_the_ending_controller() {
    let mut cards = permanents();
    cards.push(creature(
        7,
        B,
        ObjectDestination::Battlefield { controller: B },
        0,
    ));
    let initial = prepared(8, Step::Cleanup, cards);
    let game = initial.state();
    assert_eq!(game.turn().unwrap().stage(), TurnStage::Priority);
    assert_eq!(game.turn().unwrap().position().cleanup_iteration, 0);
    assert_eq!(object(game, 7).zone(), ObjectZone::Graveyard(B));
    assert_eq!(
        object(game, 1).has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(
        object(game, 4).has_continuous_control_since_turn_start(),
        Some(false)
    );
    let revision = game.execution().window().cards().revision();
    let next = pass_round(game);
    assert_eq!(next.turn_transitions()[0].to.step, Step::Cleanup);
    assert_eq!(next.turn_transitions()[0].to.cleanup_iteration, 1);
    assert_eq!(next.state().turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(next.state().turn().unwrap().position().number, 9);
    assert_eq!(
        object(next.state(), 1).has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(
        object(next.state(), 4).has_continuous_control_since_turn_start(),
        Some(true)
    );
    assert_eq!(
        next.state().execution().window().cards().revision(),
        revision + 1
    );
}

#[test]
fn opening_handoff_records_one_actual_start_without_fabricating_battlefield_objects() {
    let mut cards = vec![];
    for (owner, base) in [(A, 100), (B, 200)] {
        cards.extend(
            (0..12).map(|index| creature(base + index, owner, ObjectDestination::LibraryBottom, 3)),
        );
    }
    let opening = GameState::initialize_opening(
        GameId(1033026),
        context(),
        OpeningSetup {
            seed: {
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
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            entropy: EntropyTape::new(vec![0; 1000]),
        },
    )
    .unwrap()
    .into_state();
    let first_keep = keep(&opening).into_state();
    assert_eq!(first_keep.pending_decision().unwrap().chooser(), B);
    let before = first_keep.execution().window().cards();
    let physical = before.iter().cloned().collect::<Vec<_>>();
    let revision = before.revision();
    let handoff = keep(&first_keep);
    let game = handoff.state();
    assert!(game.pending_decision().is_none());
    assert!(game.opening().unwrap().complete());
    assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(holder(game), A);
    assert_eq!(game.execution().window().cards().revision(), revision + 1);
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        physical
    );
    assert!(game.execution().window().cards().iter().all(|card| {
        card.control_epoch().is_none() && card.has_continuous_control_since_turn_start().is_none()
    }));
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-302.6")
}
