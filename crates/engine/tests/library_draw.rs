use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::{CombatDeclarationRecord, CombatRules, DeclarationOrigin};
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    activated::ActivatedAbilityId,
    draw::{DrawEvent, GameOutcome},
    effects::{Program, SeedCard},
    game::{
        DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState,
    },
    mana::{ManaAbilityId, ManaPayment, ManaType},
    object_identity::{CardId, GameObjects, ObjectDestination, ObjectIdentityError, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn seed(
    mut cards: GameObjects,
    card: u64,
    owner: SeatId,
    destination: ObjectDestination,
) -> GameObjects {
    cards = cards
        .seed_card(cards.revision(), CardId(card), owner, destination)
        .unwrap()
        .next;
    cards
}
fn names(cards: &GameObjects, owner: SeatId) -> Vec<u64> {
    cards
        .library(owner)
        .iter()
        .map(|id| {
            cards
                .get(*id)
                .unwrap()
                .card_id()
                .expect("physical fixture card")
                .0
        })
        .collect()
}
fn assert_index(cards: &GameObjects, owners: &[SeatId]) {
    for &owner in owners {
        let expected: std::collections::BTreeSet<_> = cards
            .iter()
            .filter(|c| c.zone() == ObjectZone::Library(owner))
            .map(|c| c.id())
            .collect();
        let actual: std::collections::BTreeSet<_> = cards.library(owner).iter().copied().collect();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), cards.library(owner).len());
    }
}
#[test]
fn order_is_explicit_per_owner_and_independent_of_instance_numbers() {
    let cards = seed(
        seed(
            seed(
                seed(GameObjects::new(0), 90, A, ObjectDestination::LibraryBottom),
                2,
                B,
                ObjectDestination::LibraryBottom,
            ),
            1,
            A,
            ObjectDestination::LibraryBottom,
        ),
        70,
        A,
        ObjectDestination::LibraryTop,
    );
    assert_eq!(names(&cards, A), [70, 90, 1]);
    assert_eq!(names(&cards, B), [2]);
    assert_index(&cards, &[A, B]);
}
#[test]
fn every_departure_and_explicit_reentry_keeps_order_in_sync() {
    for destination in [
        ObjectDestination::Hand,
        ObjectDestination::Graveyard,
        ObjectDestination::Battlefield { controller: B },
        ObjectDestination::Stack { controller: B },
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ] {
        let cards = seed(
            seed(
                seed(GameObjects::new(0), 9, A, ObjectDestination::LibraryBottom),
                8,
                A,
                ObjectDestination::LibraryBottom,
            ),
            7,
            A,
            ObjectDestination::LibraryBottom,
        );
        let original = cards.current_identity(CardId(8)).unwrap();
        let moved = cards
            .apply_zone_move(cards.revision(), original, destination)
            .unwrap()
            .require_moved()
            .unwrap();
        assert_eq!(names(&moved.next, A), [9, 7]);
        assert_index(&moved.next, &[A, B]);
        assert_ne!(moved.after.id(), original);
        for (position, expected) in [
            (ObjectDestination::LibraryTop, vec![8, 9, 7]),
            (ObjectDestination::LibraryBottom, vec![9, 7, 8]),
        ] {
            let returned = moved
                .next
                .apply_zone_move(moved.next.revision(), moved.after.id(), position)
                .unwrap()
                .require_moved()
                .unwrap();
            assert_eq!(names(&returned.next, A), expected);
            assert_index(&returned.next, &[A, B]);
            assert_ne!(returned.after.id(), moved.after.id());
        }
    }
}
#[test]
fn exact_reorder_preserves_incarnations_and_rejects_stale_foreign_or_duplicate_members() {
    let cards = seed(
        seed(
            seed(GameObjects::new(0), 5, A, ObjectDestination::LibraryBottom),
            4,
            A,
            ObjectDestination::LibraryBottom,
        ),
        3,
        B,
        ObjectDestination::LibraryBottom,
    );
    let ids = cards.library(A).to_vec();
    let foreign = cards.library(B)[0];
    let reordered = cards
        .reorder_library(cards.revision(), A, vec![ids[1], ids[0]])
        .unwrap();
    assert_eq!(names(&reordered, A), [4, 5]);
    for id in &ids {
        assert_eq!(cards.get(*id), reordered.get(*id));
    }
    for order in [
        vec![],
        vec![ids[0]],
        vec![ids[0], ids[0]],
        vec![ids[0], foreign],
    ] {
        assert_eq!(
            cards.reorder_library(cards.revision(), A, order),
            Err(ObjectIdentityError::InvalidLibraryOrder)
        );
    }
    let change = cards
        .apply_zone_move(cards.revision(), ids[0], ObjectDestination::Hand)
        .unwrap()
        .require_moved()
        .unwrap();
    assert_eq!(
        change.next.reorder_library(change.next.revision(), A, ids),
        Err(ObjectIdentityError::InvalidLibraryOrder)
    );
    assert_eq!(
        cards.reorder_library(cards.revision() + 1, A, vec![]),
        Err(ObjectIdentityError::StaleRevision)
    );
    assert_eq!(
        cards.apply_zone_move(
            cards.revision(),
            cards.library(A)[0],
            ObjectDestination::LibraryTop
        ),
        Err(ObjectIdentityError::SameZone)
    );
}
#[test]
fn multiple_draws_move_one_top_card_at_a_time_with_fresh_identity() {
    let cards = seed(
        seed(
            seed(GameObjects::new(0), 90, A, ObjectDestination::LibraryBottom),
            1,
            A,
            ObjectDestination::LibraryBottom,
        ),
        8,
        A,
        ObjectDestination::LibraryBottom,
    );
    let before = cards.clone();
    let drawn = cards.draw_cards(cards.revision(), A, 2.into()).unwrap();
    assert_eq!(cards, before);
    assert_eq!(names(&drawn.next, A), [8]);
    assert_eq!(drawn.journal.events.len(), 2);
    for (event, expected) in drawn.journal.events.iter().zip([90, 1]) {
        let DrawEvent::Drawn {
            player,
            before,
            after,
        } = event
        else {
            panic!("expected actual draw");
        };
        assert_eq!(*player, A);
        assert_eq!(
            before.card_id().expect("physical fixture card"),
            CardId(expected)
        );
        assert_eq!(before.zone(), ObjectZone::Library(A));
        assert_eq!(after.zone(), ObjectZone::Hand(A));
        assert_ne!(before.id(), after.id());
        assert!(drawn.next.get(before.id()).is_none());
    }
    assert_index(&drawn.next, &[A, B]);
}
#[test]
fn final_card_is_safe_and_later_empty_attempts_are_exact_and_bounded() {
    let cards = seed(GameObjects::new(0), 1, A, ObjectDestination::LibraryBottom);
    let last = cards.draw_cards(cards.revision(), A, 1.into()).unwrap();
    assert!(matches!(
        last.journal.events.as_slice(),
        [DrawEvent::Drawn { .. }]
    ));
    let huge: RulesInteger = "900719925474099312345678901234567890"
        .to_owned()
        .try_into()
        .unwrap();
    let result = last
        .next
        .draw_cards(last.next.revision(), A, huge.clone())
        .unwrap();
    assert_eq!(result.next, last.next);
    assert_eq!(
        result.journal.events,
        vec![DrawEvent::EmptyLibrary {
            player: A,
            attempts: huge
        }]
    );
    let partial = cards.draw_cards(cards.revision(), A, 3.into()).unwrap();
    assert!(
        matches!(partial.journal.events.as_slice(),[DrawEvent::Drawn{..},DrawEvent::EmptyLibrary{player:A,attempts}] if *attempts==2.into())
    );
    let huge: RulesInteger = "900719925474099312345678901234567890"
        .to_owned()
        .try_into()
        .unwrap();
    let partial = cards.draw_cards(cards.revision(), A, huge.clone()).unwrap();
    assert!(
        matches!(partial.journal.events.as_slice(),[DrawEvent::Drawn{..},DrawEvent::EmptyLibrary{player:A,attempts}] if *attempts==huge.minus(&1.into()))
    );
}
#[test]
fn zero_negative_and_stale_draw_requests_do_not_touch_cards() {
    let cards = seed(GameObjects::new(0), 1, A, ObjectDestination::LibraryBottom);
    let before = cards.clone();
    let zero = cards.draw_cards(cards.revision(), A, 0.into()).unwrap();
    assert_eq!(zero.next, cards);
    assert!(zero.journal.events.is_empty());
    assert_eq!(
        cards.draw_cards(cards.revision(), A, (-1).into()),
        Err(ObjectIdentityError::NegativeDrawCount)
    );
    assert_eq!(
        cards.draw_cards(cards.revision() + 1, A, 1.into()),
        Err(ObjectIdentityError::StaleRevision)
    );
    assert_eq!(cards, before);
}
fn card(id: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::instant(
            spellwake_engine::mana::ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![spellwake_engine::program::Instruction::DealDamage(
                    Box::new(spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                        exile_if_dies: false,
                    }),
                )],
            ),
        ),
    }
}
fn game(
    seats: Vec<SeatId>,
    active: SeatId,
    cards: Vec<SeedCard>,
    start: TurnStart,
) -> spellwake_engine::game::NativeCandidate {
    GameState::initialize_scheduled(
        GameId(900),
        GameContext::new(
            "r".into(),
            "cr".into(),
            "catalogue".into(),
            "authored".into(),
            "individual".into(),
            test_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
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
                seats,
                active,
                cards,
                initial_battlefield_order,
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        start,
    )
    .unwrap()
}
fn holder(game: &GameState) -> SeatId {
    match game.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => holder,
        _ => panic!("no priority"),
    }
}
fn pass(game: &GameState) -> spellwake_engine::game::NativeCandidate {
    let before = game.clone();
    let candidate = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor: holder(game),
            command: GameCommand::Pass,
        })
        .unwrap();
    assert_eq!(*game, before);
    candidate
}
#[test]
fn two_player_first_draw_is_skipped_and_other_first_turns_draw_before_priority() {
    for count in [2, 3, 6] {
        let seats = (0..count).map(SeatId).collect::<Vec<_>>();
        let initial = game(
            seats.clone(),
            A,
            vec![
                card(10, A, ObjectDestination::LibraryBottom),
                card(9, A, ObjectDestination::LibraryBottom),
            ],
            TurnStart::default(),
        );
        let original = initial
            .state()
            .execution()
            .window()
            .cards()
            .current_identity(CardId(10))
            .unwrap();
        let mut state = initial.into_state();
        let mut last = None;
        for _ in 0..count {
            let candidate = pass(&state);
            state = candidate.state().clone();
            last = Some(candidate);
        }
        let candidate = last.unwrap();
        assert_eq!(holder(&state), A);
        assert!(state.outcome().is_none());
        assert!(state.pending_eliminations().is_empty());
        if count == 2 {
            assert_eq!(state.turn().unwrap().position().step, Step::PrecombatMain);
            assert!(candidate.draw_journal().events.is_empty());
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(original)
                    .unwrap()
                    .zone(),
                ObjectZone::Library(A)
            );
        } else {
            assert_eq!(state.turn().unwrap().position().step, Step::Draw);
            assert_eq!(candidate.draw_journal().events.len(), 1);
            assert!(state.execution().window().cards().get(original).is_none());
            assert_eq!(names(state.execution().window().cards(), A), [9]);
        }
    }
    let second = game(
        vec![A, B],
        B,
        vec![card(11, B, ObjectDestination::LibraryBottom)],
        TurnStart {
            number: 2,
            step: Step::Draw,
        },
    );
    assert_eq!(holder(second.state()), B);
    assert_eq!(second.draw_journal().events.len(), 1);
    assert!(second.state().outcome().is_none());
}
#[test]
fn drawn_card_can_be_cast_and_later_inputs_do_not_repeat_draw_journal() {
    let mut cards = vec![card(10, A, ObjectDestination::LibraryBottom)];
    cards.push(SeedCard {
        card: CardId(99),
        owner: B,
        destination: ObjectDestination::Battlefield { controller: B },
        program: Program::creature(
            None,
            2.into(),
            100.into(),
            std::collections::BTreeSet::new(),
        ),
    });
    let initial = game(
        vec![A, B],
        A,
        cards,
        TurnStart {
            number: 2,
            step: Step::Draw,
        },
    );
    let state = initial.into_state();
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(10))
        .unwrap();
    let target = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(99))
        .unwrap();
    let cast = state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::Cast {
                spell: source,
                target: TargetRef::Object(target),
            },
        })
        .unwrap();
    assert!(cast.draw_journal().events.is_empty());
    let first = pass(cast.state());
    let resolved = pass(first.state());
    assert_eq!(holder(resolved.state()), A);
    assert_eq!(resolved.state().turn().unwrap().position().step, Step::Draw);
    assert!(resolved.draw_journal().events.is_empty());
    assert!(resolved.state().outcome().is_none());
}
#[test]
fn empty_draw_ends_two_player_game_and_departs_multiplayer_loser_before_priority() {
    for count in [2, 3, 6] {
        let candidate = game(
            (0..count).map(SeatId).collect(),
            A,
            vec![],
            TurnStart {
                number: 2,
                step: Step::Draw,
            },
        );
        let state = candidate.into_state();
        if count == 2 {
            assert_eq!(
                state
                    .execution()
                    .state_based_losses()
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
                [A]
            );
            assert!(!matches!(
                state.execution().window().phase(),
                WindowPhase::Priority(PriorityPhase::Ready { .. })
            ));
            assert_eq!(state.outcome(), Some(GameOutcome::Winner(B)));
            assert!(state.pending_eliminations().is_empty());
        } else {
            assert!(state.execution().state_based_losses().is_empty());
            assert_eq!(state.outcome(), None);
            assert!(state.pending_eliminations().is_empty());
            assert_eq!(
                state.execution().window().participants(),
                &(1..count).map(SeatId).collect::<Vec<_>>()
            );
            assert_eq!(state.execution().window().active_participant(), None);
            assert_eq!(holder(&state), B);
        }
        let rejected_actors: Vec<SeatId> = if count == 2 { vec![A, B] } else { vec![A] };
        for actor in rejected_actors {
            // Handles are irrelevant after the result gate, but use a separate
            // ordinary fixture's actual incarnations rather than forging one.
            let fixture = game(
                vec![A, B],
                A,
                vec![card(1, A, ObjectDestination::Hand)],
                TurnStart {
                    number: 2,
                    step: Step::Upkeep,
                },
            )
            .into_state();
            let id = fixture
                .execution()
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            for command in [
                GameCommand::Pass,
                GameCommand::Cast {
                    spell: id,
                    target: TargetRef::Object(id),
                },
                GameCommand::CastPaid {
                    spell: id,
                    target: TargetRef::Object(id),
                    payment: ManaPayment::default(),
                },
                GameCommand::ActivateAbility {
                    source: id,
                    ability: ActivatedAbilityId(1),
                    target: TargetRef::Object(id),
                    payment: None,
                },
                GameCommand::ActivateMana {
                    source: id,
                    ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
                },
                GameCommand::Reply {
                    decision: DecisionId(1),
                    choice: DecisionReply::ManaColor(ManaType::Blue),
                },
            ] {
                let before = state.clone();
                assert_eq!(
                    state.advance(GameInput {
                        basis: state.basis().clone(),
                        actor,
                        command
                    }),
                    Err(if count == 2 {
                        GameError::GameEnded
                    } else {
                        GameError::UnknownActor
                    })
                );
                assert_eq!(state, before);
            }
        }
    }
}
#[test]
fn successful_draw_reaches_main_and_completes_actual_forced_empty_attack_work() {
    let initial = game(
        vec![A, B],
        A,
        vec![card(1, A, ObjectDestination::LibraryBottom)],
        TurnStart {
            number: 2,
            step: Step::Draw,
        },
    );
    assert_eq!(initial.draw_journal().events.len(), 1);
    let registry = initial.state().execution().window().cards().clone();
    let drawn = registry.current_identity(CardId(1)).unwrap();
    assert_eq!(registry.get(drawn).unwrap().zone(), ObjectZone::Hand(A));
    let mut state = initial.into_state();
    for expected in [
        Step::PrecombatMain,
        Step::BeginningCombat,
        Step::DeclareAttackers,
    ] {
        state = pass(&state).into_state();
        let accepted = pass(&state);
        assert_eq!(accepted.state().turn().unwrap().position().step, expected);
        assert!(accepted.draw_journal().events.is_empty());
        assert_eq!(accepted.state().execution().window().cards(), &registry);
        if expected == Step::DeclareAttackers {
            assert!(
                matches!(accepted.combat_declarations(), [CombatDeclarationRecord::Attackers {
                player: A, origin: DeclarationOrigin::ForcedEmpty, attackers, taps,
            }] if attackers.is_empty() && taps.is_empty())
            );
        } else {
            assert!(accepted.combat_declarations().is_empty());
        }
        state = accepted.into_state();
    }
    assert_eq!(state.pending_turn_work(), None);
    assert!(state.pending_decision().is_none());
    assert_eq!(holder(&state), A);
    assert!(!state.execution().combat().unwrap().had_attackers());
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr")
}
