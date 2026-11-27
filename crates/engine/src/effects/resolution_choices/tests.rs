//! Synthetic native-root mechanisms, without Oracle/ordinary-game qualification.
use super::*;
use crate::{
    characteristics::PermanentSelector,
    game::{
        GameCommand, GameContext, GameError, GameId, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef},
    occurrence::{RulesOccurrence, RulesRecord},
    program::{BodyTarget, CardType, CreatureTypeSlot, ExecutableBody},
    test_support::{creature_types, packet::AliasContext, projection},
};
use std::sync::Arc;

mod departure;
#[path = "native_mechanisms.rs"]
mod native_mechanisms;
mod selection_boundary_tests;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const ABILITY: ManaAbilityRef = ManaAbilityRef::Printed(ManaAbilityId(1));

fn gain(amount: i64) -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(amount.into()),
    )))
}
fn offer(cost: i64) -> Vec<Instruction> {
    vec![
        Instruction::OfferDynamicManaPayment {
            payer: ResolvingPlayer::Controller,
            fixed: ManaCost::generic(cost.into()),
            extra_generic: crate::program::ValueExpr::Literal(0.into()),
            paid_destination: 2,
            declined_destination: 1,
        },
        gain(2),
    ]
}
fn candidate(state: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    crate::test_support::current_capture::advance(
        state,
        GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        },
    )
    .unwrap()
}
fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn initialize(body: Vec<Instruction>) -> NativeCandidate {
    initialize_for_target(BodyTarget::None, body)
}
fn initialize_for_target(target: BodyTarget, body: Vec<Instruction>) -> NativeCandidate {
    initialize_for_target_paid(target, body, false)
}
fn initialize_for_target_paid(
    target: BodyTarget,
    body: Vec<Instruction>,
    with_paid: bool,
) -> NativeCandidate {
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(ManaCost::free(), ExecutableBody::new(target, body)),
    }];
    for (id, output) in [
        (
            10,
            ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
        ),
        (
            11,
            ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
        ),
        (
            12,
            ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Blue, ManaType::Red]),
            },
        ),
        (
            13,
            ManaOutput::Counted {
                mana_type: Some(ManaType::Blue),
                permanents: PermanentSelector {
                    required_types: BTreeSet::from([CardType::Land]),
                    ..PermanentSelector::default()
                },
            },
        ),
    ] {
        let mut program = Program::artifact(None, vec![]);
        program.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output,
        });
        cards.push(SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program,
        });
    }
    if with_paid {
        let mut program = Program::artifact(None, vec![]);
        program.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([
                (ManaType::White, 1.into()),
                (ManaType::Blue, 1.into()),
            ]))
            .paid(ManaCost::generic(1.into())),
        });
        cards.push(SeedCard {
            card: CardId(14),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program,
        });
        let mut hybrid = Program::artifact(None, vec![]);
        hybrid.mana.push(ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::SpecialColor(crate::mana::SpecialColorYield::HybridFilter {
                colors: [ManaType::White, ManaType::Blue],
            }),
        });
        cards.push(SeedCard {
            card: CardId(15),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: hybrid,
        });
    }
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "resolution-choice-unit".into(),
        vocabulary.rules_revision().into(),
        "synthetic-native-only".into(),
        "unit".into(),
        "unit".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        if with_paid {
            vec![
                CardId(10),
                CardId(11),
                CardId(12),
                CardId(13),
                CardId(14),
                CardId(15),
            ]
        } else {
            vec![CardId(10), CardId(11), CardId(12), CardId(13)]
        },
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let initialized =
        crate::test_support::current_capture::initialize(GameId(7710), context, seed).unwrap();
    native_mechanisms::native_packets::retain_root(&initialized, "tests.initialize");
    initialized
}
fn pending(body: Vec<Instruction>) -> GameState {
    let mut state = initialize(body).into_state();
    state = candidate(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    state = candidate(&state, A, GameCommand::Pass).into_state();
    candidate(&state, B, GameCommand::Pass).into_state()
}
fn reply(state: &GameState, choice: DecisionReply) -> NativeCandidate {
    candidate(
        state,
        state.pending_decision().unwrap().chooser(),
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice,
        },
    )
}
fn tap(state: &GameState, card: u64) -> NativeCandidate {
    candidate(
        state,
        A,
        GameCommand::ActivateMana {
            source: object(state, card),
            ability: ABILITY,
        },
    )
}
fn payment_frame(state: &GameState) -> ResolutionPaymentFrame {
    let RuleFrame::ResolutionPayment(frame) = state.pending_decision().unwrap().frame() else {
        panic!("payment expected")
    };
    frame.clone()
}
fn assert_old_payment_rejected(
    state: &GameState,
    old: &ResolutionPaymentFrame,
    id: crate::game::DecisionId,
) {
    let saved = state.clone();
    assert_eq!(
        native_mechanisms::native_packets::resume_rule(
            state.execution(),
            state.execution().revision(),
            &EffectDecision::ResolutionPayment(old.clone()),
            A,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            None
        ),
        Err(EffectError::WrongResolutionPaymentFrame)
    );
    assert_eq!(
        crate::test_support::current_capture::advance(
            state,
            GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: id,
                    choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
                }
            }
        ),
        Err(GameError::WrongDecision)
    );
    assert_eq!(*state, saved);
}

#[test]
fn optional_outcome_executes_or_skips_one_primitive_before_the_same_tail() {
    for (choice, life) in [
        (OptionalEffectChoice::Take, 23),
        (OptionalEffectChoice::Decline, 22),
    ] {
        let state = pending(vec![
            Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            },
            gain(1),
            gain(2),
        ]);
        let result = reply(&state, DecisionReply::OptionalEffect(choice));
        assert_eq!(result.state().execution().life_total(A), Some(&life.into()));
        assert_eq!(result.rules_journal().records.iter().filter(|record| matches!(record,
            RulesRecord::OptionalEffectOutcome { outcome, next, .. } if *outcome == choice && *next == if choice == OptionalEffectChoice::Take {1} else {2})).count(), 1);
        assert!(result.state().pending_decision().is_none());
    }
}

#[test]
fn explicit_zero_payment_is_paid_without_a_debit_and_differs_from_decline() {
    for (choice, life, paid) in [
        (
            ResolutionPaymentReply::Pay(ManaPayment::default()),
            20,
            true,
        ),
        (ResolutionPaymentReply::Decline, 22, false),
    ] {
        let state = pending(offer(0));
        let result = reply(&state, DecisionReply::ResolutionPayment(choice));
        assert_eq!(result.state().execution().life_total(A), Some(&life.into()));
        assert!(result.mana_payment().is_none());
        let outcomes: Vec<_> = result
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| match record {
                RulesRecord::ResolutionPaymentOutcome { outcome, .. } => Some(outcome),
                _ => None,
            })
            .collect();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(
            matches!(outcomes[0], ResolutionPaymentOutcome::Paid(payment) if payment.cost == ManaCost::free() && payment.spend.is_empty()),
            paid
        );
        assert!(!result.rules_journal().records.iter().any(|record| matches!(record,
            RulesRecord::Event(event) if event.occurrences.iter().any(|row| matches!(row.value, RulesOccurrence::ManaDebited { .. })))));
    }
}

#[test]
fn payment_allows_fixed_color_fixed_and_restores_exact_parent_before_pay() {
    let state = pending(offer(3));
    let old = payment_frame(&state);
    let old_id = state.pending_decision().unwrap().id();
    let first = tap(&state, 10).into_state();
    assert_old_payment_rejected(&first, &old, old_id);
    let before_bad = first.clone();
    assert!(
        first
            .advance(GameInput {
                basis: first.basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: first.pending_decision().unwrap().id(),
                    choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(
                        ManaPayment {
                            from_lots: Default::default(),
                            spend: vec![ManaSpend {
                                mana_type: ManaType::Blue,
                                amount: 3.into()
                            }]
                        }
                    ))
                }
            })
            .is_err()
    );
    assert_eq!(first, before_bad);
    let color = tap(&first, 12).into_state();
    assert!(matches!(
        color.pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    assert_eq!(color.execution().mana(A, ManaType::Blue), 1.into());
    assert!(matches!(
        color.execution().resolution_choice_state(),
        Some(ResolutionChoiceState::Payment {
            suspended_for_mana_color: true,
            ..
        })
    ));
    let inspected = projection::state(&color, &AliasContext::default()).unwrap();
    assert_eq!(inspected["pending_work"]["parent"]["tag"], "Payment");
    assert_eq!(
        inspected["pending_work"]["parent"]["work"]["cost"]["generic"],
        "3"
    );
    let restored = reply(&color, DecisionReply::ManaColor(ManaType::Blue)).into_state();
    assert_eq!(restored.execution().mana(A, ManaType::Blue), 2.into());
    let funded = tap(&restored, 11).into_state();
    assert_eq!(payment_frame(&funded).entry(), old.entry());
    assert_eq!(payment_frame(&funded).instruction(), old.instruction());
    assert!(payment_frame(&funded).revision() > old.revision());
    let result = reply(
        &funded,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 3.into(),
            }],
        })),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(result.state().execution().life_total(A), Some(&20.into()));
    assert!(result.mana_payment().is_none());
    assert!(result.rules_journal().records.iter().any(|record| matches!(record,
        RulesRecord::ResolutionPaymentOutcome { outcome: ResolutionPaymentOutcome::Paid(payment), .. }
            if payment.source == old.entry() && payment.payer == A && payment.spend[0].amount == 3.into())));
}

#[test]
fn declining_after_an_accepted_mana_prefix_preserves_tap_and_pool() {
    let state = pending(offer(3));
    let source = object(&state, 10);
    let funded = tap(&state, 10).into_state();
    let result = reply(
        &funded,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(result.state().execution().life_total(A), Some(&22.into()));
}

#[test]
fn zero_count_production_refreshes_payment_authority_without_mana_produced() {
    let state = pending(offer(3));
    let old = payment_frame(&state);
    let id = state.pending_decision().unwrap().id();
    let result = tap(&state, 13);
    assert!(result.mana_production().unwrap().produced.is_empty());
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 0.into());
    assert_old_payment_rejected(result.state(), &old, id);
    assert!(!result.rules_journal().records.iter().any(|record| matches!(record,
        RulesRecord::Event(event) if event.occurrences.iter().any(|row| matches!(row.value, RulesOccurrence::ManaProduced { .. })))));
    assert!(result.rules_journal().records.iter().any(|record| matches!(record,
        RulesRecord::Event(event) if event.occurrences.iter().any(|row| matches!(row.value, RulesOccurrence::ManaAbility { .. })))));
    let declined = reply(
        result.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_eq!(declined.state().execution().life_total(A), Some(&22.into()));
}

#[test]
fn taking_optional_type_choice_preserves_the_actual_child_and_tail() {
    let state = pending(vec![
        Instruction::OptionalEffect {
            chooser: ResolvingPlayer::Controller,
        },
        Instruction::ChooseCreatureType {
            into: CreatureTypeSlot::new(0),
            excluding: BTreeSet::new(),
        },
        gain(1),
    ]);
    let taken = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert!(matches!(
        taken.pending_decision().unwrap().frame(),
        RuleFrame::CreatureType(_)
    ));
    assert_eq!(taken.execution().instruction_index(), Some(1));
    let human = taken
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    let result = reply(&taken, DecisionReply::CreatureType(human));
    assert_eq!(result.state().execution().life_total(A), Some(&21.into()));
    assert!(
        !result
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record, RulesRecord::OptionalEffectOutcome { .. }))
    );
}

#[test]
fn paid_mana_ability_works_inside_resolution_payment_and_rejects_atomically() {
    let mut state = initialize_for_target_paid(BodyTarget::None, offer(2), true).into_state();
    state = candidate(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    state = candidate(&state, A, GameCommand::Pass).into_state();
    state = candidate(&state, B, GameCommand::Pass).into_state();
    let source = object(&state, 14);
    let payment = |kind| ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: kind,
            amount: 1.into(),
        }],
    };
    let paid = |kind| GameCommand::ActivateManaWithPayment {
        source,
        ability: ABILITY,
        payment: payment(kind),
    };
    let saved = state.clone();
    assert!(
        crate::test_support::current_capture::advance(
            &state,
            GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: paid(ManaType::Blue)
            }
        )
        .is_err()
    );
    assert_eq!(state, saved);
    state = tap(&state, 10).into_state();
    state = candidate(&state, A, paid(ManaType::Blue)).into_state();
    assert_eq!(state.execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(state.execution().mana(A, ManaType::White), 1.into());
    assert!(
        state
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped()
            .unwrap()
    );
    assert!(
        crate::test_support::current_capture::advance(
            &state,
            GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: paid(ManaType::Blue)
            }
        )
        .is_err()
    );
    let result = reply(
        &state,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![
                ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: 1.into(),
                },
                ManaSpend {
                    mana_type: ManaType::White,
                    amount: 1.into(),
                },
            ],
        })),
    );
    assert_eq!(result.state().execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(
        result.state().execution().mana(A, ManaType::White),
        0.into()
    );
}

#[test]
fn hybrid_mana_ability_preserves_resolution_payment_parent_and_credits_exact_bundle() {
    let mut state = initialize_for_target_paid(BodyTarget::None, offer(2), true).into_state();
    state = candidate(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    state = candidate(&state, A, GameCommand::Pass).into_state();
    state = candidate(&state, B, GameCommand::Pass).into_state();
    state = tap(&state, 10).into_state();
    let source = object(&state, 15);
    let paid = GameCommand::ActivateHybridMana {
        source,
        ability: ABILITY,
        payment: ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            }],
        },
        bundle: BTreeMap::from([(ManaType::White, 2.into())]),
    };
    state = candidate(&state, A, paid).into_state();
    assert_eq!(state.execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(state.execution().mana(A, ManaType::White), 2.into());
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    let result = reply(
        &state,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::White,
                amount: 2.into(),
            }],
        })),
    );
    assert_eq!(
        result.state().execution().mana(A, ManaType::White),
        0.into()
    );
}
