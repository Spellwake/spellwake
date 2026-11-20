//! Prepared paid public-root trajectories, not natural opening or ordinary capture.
use super::*;
use spellwake_engine::{
    combat::CombatRules,
    draw::{DrawEvent, GameOutcome},
    effects::{EffectError, LibraryAction, LibraryActionKind, LibraryTopReply},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend},
    object_identity::{ObjectId, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord},
    priority::PriorityPhase,
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
const A: SeatId = SeatId(19);
const B: SeatId = SeatId(7);
struct Run {
    initial: GameState,
    state: GameState,
    calls: Vec<(GameInput, Result<NativeCandidate, GameError>)>,
}
impl Run {
    fn new(oracle: &str, target: SeatId, count: u64) -> Self {
        let g = genuine();
        let mut cards = vec![
            g.registry
                .seed(&id(ISLAND), CardId(1), A, ObjectDestination::Hand)
                .unwrap(),
            g.registry
                .seed(&id(oracle), CardId(2), A, ObjectDestination::Hand)
                .unwrap(),
        ];
        for (owner, start, total) in [
            (A, 10, if target == A { count } else { 3 }),
            (B, 20, if target == B { count } else { 3 }),
        ] {
            for offset in 0..total {
                cards.push(
                    g.registry
                        .seed(
                            &id(ISLAND),
                            CardId(start + offset),
                            owner,
                            ObjectDestination::LibraryBottom,
                        )
                        .unwrap(),
                );
            }
        }
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            LIBRARY_TOP_PROFILE.into(),
            g.vocabulary.rules_revision().into(),
            format!("oracle-sha256:{}", crate::TEACHING_SOURCE_SHA256),
            "library-top-registry11".into(),
            "prepared-paid-only".into(),
            g.vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            g.vocabulary.clone(),
            names,
        );
        let initialized = GameState::initialize_scheduled(
            GameId(88412),
            context.clone(),
            seed.clone(),
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        );
        assert_eq!(
            initialized,
            GameState::initialize_scheduled(
                GameId(88412),
                context,
                seed,
                TurnStart {
                    number: 1,
                    step: Step::PrecombatMain
                }
            )
        );
        let state = initialized.unwrap().into_state();
        assert_eq!(state.execution().mana(A, ManaType::Blue), 0.into());
        Self {
            initial: state.clone(),
            state,
            calls: vec![],
        }
    }
    fn call(&mut self, actor: SeatId, command: GameCommand) -> Result<NativeCandidate, GameError> {
        let input = GameInput {
            basis: self.state.basis().clone(),
            actor,
            command,
        };
        let before = self.state.clone();
        let result = self.state.advance(input.clone());
        assert_eq!(self.state, before);
        assert_eq!(self.state.advance(input.clone()), result);
        if let Ok(candidate) = &result {
            self.state = candidate.state().clone();
        }
        self.calls.push((input, result.clone()));
        result
    }
    fn paid_cast(&mut self, oracle: &str, target: SeatId) {
        let command = |state: &GameState, payment: ManaPayment| {
            if oracle == SCOUR {
                GameCommand::CastPaid {
                    spell: object(state, 2),
                    target: TargetRef::Player(target),
                    payment,
                }
            } else {
                GameCommand::CastUntargeted {
                    spell: object(state, 2),
                    payment: Some(payment),
                }
            }
        };
        // A real rejected cast before the land/mana prefix cannot spend resources.
        assert!(
            self.call(A, command(&self.state, ManaPayment::default()))
                .is_err()
        );
        let played = self
            .call(
                A,
                GameCommand::PlayLand {
                    land: object(&self.state, 1),
                },
            )
            .unwrap();
        assert!(played.land_play().is_some());
        let produced = self
            .call(
                A,
                GameCommand::ActivateMana {
                    source: object(&self.state, 1),
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
                },
            )
            .unwrap();
        assert!(produced.mana_production().is_some());
        assert_eq!(self.state.execution().mana(A, ManaType::Blue), 1.into());
        let cast = self
            .call(
                A,
                command(
                    &self.state,
                    ManaPayment {
                        spend: vec![ManaSpend {
                            mana_type: ManaType::Blue,
                            amount: 1.into(),
                        }],
                    },
                ),
            )
            .unwrap();
        let payment = cast.mana_payment().unwrap();
        assert_eq!(payment.payer, A);
        assert_eq!(payment.cost, ManaCost::one(ManaType::Blue));
        assert_eq!(self.state.execution().mana(A, ManaType::Blue), 0.into());
        for _ in 0..2 {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                self.state.execution().window().phase()
            else {
                panic!("ready priority")
            };
            self.call(holder, GameCommand::Pass).unwrap();
        }
    }
    fn reply(
        &mut self,
        actor: SeatId,
        choice: DecisionReply,
    ) -> Result<NativeCandidate, GameError> {
        self.call(
            actor,
            GameCommand::Reply {
                decision: self.state.pending_decision().unwrap().id(),
                choice,
            },
        )
    }
    fn actions(&self) -> Vec<&LibraryAction> {
        self.calls
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .flat_map(|c| &c.rules_journal().records)
            .filter_map(|r| {
                if let RulesRecord::Event(event) = r {
                    Some(event)
                } else {
                    None
                }
            })
            .flat_map(|e| &e.occurrences)
            .filter_map(|o| {
                if let RulesOccurrence::LibraryAction(action) = &o.value {
                    Some(action)
                } else {
                    None
                }
            })
            .collect()
    }
    fn replay(&self) {
        let mut state = self.initial.clone();
        for (input, expected) in &self.calls {
            let before = state.clone();
            let actual = state.advance(input.clone());
            assert_eq!(&actual, expected);
            assert_eq!(state, before);
            if let Ok(candidate) = actual {
                state = candidate.into_state();
            }
        }
        assert_eq!(state, self.state);
    }
}
fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn hand_count(state: &GameState, owner: SeatId) -> usize {
    state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|object| object.zone() == ObjectZone::Hand(owner))
        .count()
}
fn pile(state: &GameState, owner: SeatId, graveyard: bool) -> Vec<CardId> {
    let cards = state.execution().window().cards();
    let ids = if graveyard {
        cards.graveyard(owner)
    } else {
        cards.library(owner)
    };
    ids.iter()
        .map(|o| cards.get(*o).unwrap().card_id().unwrap())
        .collect()
}
#[test]
#[ignore = "requires genuine Registry4; actual land play/mana/payment and native Result/state replay"]
fn paid_opt_and_consider_branch_choices_resume_actual_controller_draw() {
    for oracle in [OPT, CONSIDER] {
        for remove in [false, true] {
            let mut run = Run::new(oracle, A, 3);
            let original = pile(&run.state, A, false);
            run.paid_cast(oracle, A);
            let RuleFrame::LibraryTop(frame) = run.state.pending_decision().unwrap().frame() else {
                panic!("real singleton library choice")
            };
            assert_eq!(frame.chooser(), A);
            assert_eq!(frame.domain_top_first().len(), 1);
            let before = frame.domain_top_first()[0];
            assert_eq!(
                run.state
                    .execution()
                    .window()
                    .cards()
                    .get(before)
                    .unwrap()
                    .card_id(),
                Some(original[0])
            );
            let choice = |top: Vec<ObjectId>, rest: Vec<ObjectId>| {
                DecisionReply::LibraryTop(if oracle == OPT {
                    LibraryTopReply::Scry {
                        top_first: top,
                        bottom_top_first: rest,
                    }
                } else {
                    LibraryTopReply::Surveil {
                        top_first: top,
                        to_graveyard: rest,
                    }
                })
            };
            assert_eq!(
                run.reply(A, choice(vec![before], vec![before])),
                Err(GameError::Effect(EffectError::InvalidLibraryPartition))
            );
            assert_eq!(
                run.reply(B, choice(vec![before], vec![])),
                Err(GameError::WrongChooser)
            );
            let result = run
                .reply(
                    A,
                    if remove {
                        choice(vec![], vec![before])
                    } else {
                        choice(vec![before], vec![])
                    },
                )
                .unwrap();
            assert!(
                run.state.pending_decision().is_none(),
                "singleton arrival must not prompt owner ordering"
            );
            let drawn = if remove { original[1] } else { original[0] };
            assert!(
                matches!(run.state.execution().window().cards().get(object(&run.state,drawn.0)).unwrap().zone(),ObjectZone::Hand(owner) if owner==A)
            );
            assert_eq!(result.draw_journal().events.len(), 1);
            assert!(
                matches!(&result.draw_journal().events[0],DrawEvent::Drawn{player,after,..} if *player==A && after.card_id()==Some(drawn))
            );
            let action = run.actions();
            assert_eq!(action.len(), 1);
            assert_eq!(action[0].player, A);
            assert_eq!(action[0].requested, 1.into());
            assert_eq!(
                action[0].kind,
                if oracle == OPT {
                    LibraryActionKind::Scry
                } else {
                    LibraryActionKind::Surveil
                }
            );
            if oracle == CONSIDER && remove {
                assert!(pile(&run.state, A, true).contains(&original[0]));
                assert!(run.state.execution().window().cards().get(before).is_none());
            }
            if oracle == OPT && remove {
                assert_eq!(pile(&run.state, A, false).last(), Some(&original[0]));
            }
            assert!(pile(&run.state, A, true).contains(&CardId(2)));
            run.replay();
        }
    }
}
#[test]
#[ignore = "requires genuine Registry4; target milling, native owner order, controller draw and replay"]
fn paid_thought_scour_targets_self_or_opponent_and_clamps_short_libraries_without_failed_mill_draw()
{
    for target in [A, B] {
        for count in [0, 1, 2, 3] {
            let mut run = Run::new(SCOUR, target, count);
            let target_library = pile(&run.state, target, false);
            let before_hand = hand_count(&run.state, A);
            run.paid_cast(SCOUR, target);
            if count >= 2 {
                let pending = run.state.pending_decision().unwrap();
                assert_eq!(pending.chooser(), target);
                let RuleFrame::ResolutionGraveyardOrder(frame) = pending.frame() else {
                    panic!("actual mill arrivals owner order")
                };
                let mut order = frame.domain().to_vec();
                assert_eq!(order.len(), 2);
                order.reverse();
                let expected = order
                    .iter()
                    .map(|o| {
                        run.state
                            .execution()
                            .window()
                            .cards()
                            .get(*o)
                            .unwrap()
                            .card_id()
                            .unwrap()
                    })
                    .collect::<Vec<_>>();
                assert!(
                    run.reply(
                        target,
                        DecisionReply::OrderResolutionGraveyard(vec![order[0], order[0]])
                    )
                    .is_err()
                );
                run.reply(
                    target,
                    DecisionReply::OrderResolutionGraveyard(order.clone()),
                )
                .unwrap();
                assert_eq!(run.actions()[0].graveyard_before_top_first, order);
                let actual = pile(&run.state, target, true);
                let relative = actual
                    .into_iter()
                    .filter(|id| target_library.contains(id))
                    .collect::<Vec<_>>();
                assert_eq!(relative, expected);
                assert_eq!(run.actions()[0].graveyard_before_top_first.len(), 2);
            } else {
                assert!(run.state.pending_decision().is_none());
            }
            let actions = run.actions();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].kind, LibraryActionKind::Mill);
            assert_eq!(actions[0].player, target);
            assert_eq!(actions[0].requested, 2.into());
            assert_eq!(
                actions[0].graveyard_before_top_first.len(),
                usize::try_from(count.min(2)).unwrap()
            );
            let draws = run
                .calls
                .iter()
                .filter_map(|(_, r)| r.as_ref().ok())
                .flat_map(|c| &c.draw_journal().events)
                .collect::<Vec<_>>();
            assert_eq!(draws.len(), 1);
            if target == A && count <= 2 {
                assert_eq!(run.state.outcome(), Some(GameOutcome::Winner(B)));
                assert!(
                    matches!(draws[0],DrawEvent::EmptyLibrary{player,attempts} if *player==A && *attempts==1.into())
                );
            } else {
                assert!(matches!(draws[0],DrawEvent::Drawn{player,..} if *player==A));
                assert_eq!(hand_count(&run.state, A), before_hand - 1);
            }
            run.replay();
        }
    }
}
