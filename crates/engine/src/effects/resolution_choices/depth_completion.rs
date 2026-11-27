//! Additional DP02 assertions awaiting a new source-bound native execution.
//! This file does not establish ordinary, Oracle, Forge or complete-wave coverage.
use super::*;
use crate::token::{TokenBatchMember, TokenRecipients};

fn spend(mana_type: ManaType, amount: i64) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type,
            amount: amount.into(),
        }],
    }
}

fn payment_outcomes(result: &NativeCandidate) -> usize {
    result
        .rules_journal()
        .records
        .iter()
        .filter(|record| matches!(record, RulesRecord::ResolutionPaymentOutcome { .. }))
        .count()
}

#[test]
fn every_wrong_reply_kind_variant_retains_the_latest_prefix_before_its_own_valid_continuation() {
    let opened = pending(vec![
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::generic(3.into()),
                crate::program::ValueExpr::Literal(0.into()),
                4,
                1,
            ),
        )),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        gain(1),
        gain(2),
    ]);
    let fixed = tap(&opened, 10);
    let color = tap(fixed.state(), 12);
    let restored = reply(color.state(), DecisionReply::ManaColor(ManaType::Blue));
    let optional = reply(
        restored.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );

    for wrong in [
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment::default())),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        DecisionReply::ManaColor(ManaType::Blue),
    ] {
        reject(
            optional.state(),
            A,
            command_reply(optional.state(), wrong),
            GameError::Effect(EffectError::WrongDecisionKind),
        );
        let continued = reply(
            optional.state(),
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert_tail(&continued);
        assert_eq!(outcomes(&continued), 1);
        assert_eq!(
            continued.state().execution().mana(A, ManaType::Blue),
            2.into()
        );
    }

    for wrong in [
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        DecisionReply::ManaColor(ManaType::Blue),
    ] {
        reject(
            restored.state(),
            A,
            command_reply(restored.state(), wrong),
            GameError::Effect(EffectError::WrongDecisionKind),
        );
        let continued = reply(
            restored.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
        assert_eq!(payment_outcomes(&continued), 1);
        assert_eq!(continued.state(), optional.state());
        let finished = reply(
            continued.state(),
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert_tail(&finished);
        assert_eq!(
            finished.state().execution().mana(A, ManaType::Blue),
            2.into()
        );
    }

    for wrong in [
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(ManaType::Blue, 1))),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    ] {
        reject(
            color.state(),
            A,
            command_reply(color.state(), wrong),
            GameError::Effect(EffectError::WrongDecisionKind),
        );
        let continued = reply(color.state(), DecisionReply::ManaColor(ManaType::Blue));
        assert_eq!(continued.state(), restored.state());
        assert_eq!(continued.rules_journal(), restored.rules_journal());
        let declined = reply(
            continued.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
        let finished = reply(
            declined.state(),
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert_tail(&finished);
        assert_eq!(
            finished.state().execution().mana(A, ManaType::Blue),
            2.into()
        );
    }
    for state in [color.state(), restored.state(), optional.state()] {
        for id in [10, 12] {
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(object(state, id))
                    .unwrap()
                    .tapped(),
                Some(true)
            );
        }
    }
}

#[test]
pub(super) fn exact_total_unavailable_amount_and_required_colorless_denials_keep_the_funded_prefix()
{
    let funded = tap(&pending(offer(3)), 10);
    reject(
        funded.state(),
        A,
        command_reply(
            funded.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(ManaType::Blue, 3))),
        ),
        GameError::Effect(EffectError::InvalidManaPayment(
            ManaPaymentError::InsufficientMana,
        )),
    );
    let declined = reply(
        funded.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_tail(&declined);
    assert_eq!(payment_outcomes(&declined), 1);
    assert_eq!(
        declined.state().execution().mana(A, ManaType::Blue),
        1.into()
    );

    let mut colorless = Program::artifact(None, vec![]);
    colorless.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
    });
    let mut blue = colorless.clone();
    blue.mana[0].output = ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())]));
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(
                1,
                A,
                BodyTarget::None,
                vec![
                    Instruction::PromptChoice(Box::new(
                        crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::Controller,
                            ManaCost::specific(ManaType::Colourless, 1.into()),
                            crate::program::ValueExpr::Literal(0.into()),
                            2,
                            1,
                        ),
                    )),
                    gain(2),
                ],
            ),
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                blue,
            ),
            seed(
                11,
                A,
                ObjectDestination::Battlefield { controller: A },
                colorless,
            ),
        ],
    );
    let offered = resolve_top(cast(&initial, A, 1, None).state());
    let funded = tap(offered.state(), 10);
    reject(
        funded.state(),
        A,
        command_reply(
            funded.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(ManaType::Blue, 1))),
        ),
        GameError::Effect(EffectError::InvalidManaPayment(
            ManaPaymentError::MissingRequired,
        )),
    );
    let completed_funding = tap(funded.state(), 11);
    let paid = reply(
        completed_funding.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(
            ManaType::Colourless,
            1,
        ))),
    );
    assert_eq!(payment_outcomes(&paid), 1);
    assert!(paid.state().pending_decision().is_none());
    assert_eq!(paid.state().execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(
        paid.state().execution().mana(A, ManaType::Colourless),
        0.into()
    );
    assert_eq!(paid.state().execution().life_total(A), Some(&20.into()));
}

#[derive(Clone)]
struct TypedTrace {
    inputs: Vec<GameInput>,
    journals: Vec<RulesJournal>,
    states: Vec<GameState>,
}

fn record_input(trace: &mut TypedTrace, actor: SeatId, command: GameCommand) {
    let current = trace.states.last().unwrap();
    let input = GameInput {
        basis: current.basis().clone(),
        actor,
        command,
    };
    let next = current.advance(input.clone()).unwrap();
    trace.inputs.push(input);
    trace.journals.push(next.rules_journal().clone());
    trace.states.push(next.into_state());
}

fn record_choice(trace: &mut TypedTrace, choice: DecisionReply) {
    let current = trace.states.last().unwrap();
    let actor = current.pending_decision().unwrap().chooser();
    let command = command_reply(current, choice);
    record_input(trace, actor, command);
}

fn replay_body() -> Vec<Instruction> {
    vec![
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::Controller,
                ManaCost::generic(2.into()),
                crate::program::ValueExpr::Literal(0.into()),
                1,
                4,
            ),
        )),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        gain(1),
        gain(2),
    ]
}

fn record_branch(optional: Option<OptionalEffectChoice>) -> TypedTrace {
    let initial = initialize(replay_body());
    let mut trace = TypedTrace {
        inputs: vec![],
        journals: vec![initial.rules_journal().clone()],
        states: vec![initial.into_state()],
    };
    let spell = object(&trace.states[0], 1);
    record_input(
        &mut trace,
        A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    );
    record_input(&mut trace, A, GameCommand::Pass);
    record_input(&mut trace, B, GameCommand::Pass);
    for id in [10, 12] {
        let source = object(trace.states.last().unwrap(), id);
        record_input(
            &mut trace,
            A,
            GameCommand::ActivateMana {
                source,
                ability: ABILITY,
            },
        );
    }
    record_choice(&mut trace, DecisionReply::ManaColor(ManaType::Blue));
    record_choice(
        &mut trace,
        DecisionReply::ResolutionPayment(match optional {
            Some(_) => ResolutionPaymentReply::Pay(spend(ManaType::Blue, 2)),
            None => ResolutionPaymentReply::Decline,
        }),
    );
    if let Some(choice) = optional {
        record_choice(&mut trace, DecisionReply::OptionalEffect(choice));
    }
    trace
}

#[test]
fn original_typed_inputs_replay_all_three_payment_optional_branches_and_detect_distinct_tampers() {
    for (optional_choice, life, pool) in [
        (Some(OptionalEffectChoice::Take), 23, 0),
        (Some(OptionalEffectChoice::Decline), 22, 0),
        (None, 20, 2),
    ] {
        let trace = record_branch(optional_choice);
        let initial = initialize(replay_body());
        assert_eq!(initial.rules_journal(), &trace.journals[0]);
        assert_eq!(initial.state(), &trace.states[0]);
        let mut replay = initial.into_state();
        for (index, original) in trace.inputs.iter().enumerate() {
            assert_eq!(&replay, &trace.states[index]);
            let next = replay.advance(original.clone()).unwrap();
            assert_eq!(
                next.rules_journal(),
                &trace.journals[index + 1],
                "journal {index}"
            );
            assert_eq!(
                next.state(),
                &trace.states[index + 1],
                "whole state {index}"
            );
            replay = next.into_state();
        }
        assert!(replay.pending_decision().is_none());
        assert_eq!(replay.execution().life_total(A), Some(&life.into()));
        assert_eq!(replay.execution().mana(A, ManaType::Blue), pool.into());

        // Each mutation starts at its actual retained prefix. The unmodified
        // original input then reproduces that exact saved journal and state.
        let mut tampers = Vec::new();
        let mut wrong_actor = trace.inputs[4].clone();
        wrong_actor.actor = B;
        tampers.push((4, wrong_actor, GameError::WrongChooser));
        let mut stale_basis = trace.inputs[5].clone();
        stale_basis.basis = trace.inputs[4].basis.clone();
        tampers.push((5, stale_basis, GameError::StaleRevision));
        let mut old_decision = trace.inputs[5].clone();
        let GameCommand::Reply { decision, .. } = &mut old_decision.command else {
            unreachable!()
        };
        *decision = trace.states[3].pending_decision().unwrap().id();
        tampers.push((5, old_decision, GameError::WrongDecision));
        let mut wrong_kind = trace.inputs[5].clone();
        let GameCommand::Reply { choice, .. } = &mut wrong_kind.command else {
            unreachable!()
        };
        *choice = DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline);
        tampers.push((
            5,
            wrong_kind,
            GameError::Effect(EffectError::WrongDecisionKind),
        ));
        let mut bad_spend = trace.inputs[6].clone();
        let GameCommand::Reply { choice, .. } = &mut bad_spend.command else {
            unreachable!()
        };
        *choice =
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(ManaType::Blue, 1)));
        tampers.push((
            6,
            bad_spend,
            GameError::Effect(EffectError::InvalidManaPayment(
                ManaPaymentError::WrongTotal,
            )),
        ));
        for (index, tampered, error) in tampers {
            assert_ne!(tampered, trace.inputs[index]);
            let before = trace.states[index].clone();
            assert_eq!(before.advance(tampered), Err(error));
            assert_eq!(before, trace.states[index]);
            let next = before.advance(trace.inputs[index].clone()).unwrap();
            assert_eq!(next.rules_journal(), &trace.journals[index + 1]);
            assert_eq!(next.state(), &trace.states[index + 1]);
        }

        // Red is a legal reply to this color child. Exact replay detects it by
        // actual changed output, rather than requiring every tamper to be Err.
        let mut legal_changed_color = trace.inputs[5].clone();
        let GameCommand::Reply { choice, .. } = &mut legal_changed_color.command else {
            unreachable!()
        };
        *choice = DecisionReply::ManaColor(ManaType::Red);
        assert_ne!(legal_changed_color, trace.inputs[5]);
        let changed = trace.states[5].advance(legal_changed_color).unwrap();
        assert_ne!(changed.rules_journal(), &trace.journals[6]);
        assert_ne!(changed.state(), &trace.states[6]);
        assert_eq!(
            changed.state().execution().mana(A, ManaType::Blue),
            1.into()
        );
        assert_eq!(changed.state().execution().mana(A, ManaType::Red), 1.into());
        if optional_choice.is_some() {
            let index = 7;
            let mut changed_optional = trace.inputs[index].clone();
            let GameCommand::Reply { choice, .. } = &mut changed_optional.command else {
                unreachable!()
            };
            *choice = DecisionReply::OptionalEffect(
                if *choice == DecisionReply::OptionalEffect(OptionalEffectChoice::Take) {
                    OptionalEffectChoice::Decline
                } else {
                    OptionalEffectChoice::Take
                },
            );
            let changed = trace.states[index].advance(changed_optional).unwrap();
            assert_ne!(changed.rules_journal(), &trace.journals[index + 1]);
            assert_ne!(changed.state(), &trace.states[index + 1]);
        }
    }
}

fn token_creation() -> TokenCreation {
    TokenCreation::one(
        TokenDefinition::new(
            creature(),
            TokenName::Specified(
                spellwake_contracts::names::EnglishCardName::new("Depth Native Token".into())
                    .unwrap(),
            ),
            &creature_types::selected_vocabulary().unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn token_count_conversion_total_and_layout_overflow_reject_take_atomically_then_decline_once() {
    let maximum = RulesInteger::from_usize(usize::MAX);
    let layout_count = usize::MAX / 2;
    assert!(
        layout_count
            .checked_mul(std::mem::size_of::<crate::token::TokenEntryPreview>())
            .is_none_or(|bytes| bytes > usize::try_from(isize::MAX).unwrap())
    );
    for (amount, recipients) in [
        (maximum.plus(&1.into()), TokenRecipients::Controller),
        (maximum, TokenRecipients::EachPlayer),
        // Vec rejects this impossible element layout before an allocation can
        // be attempted. This is representational capacity, not an OOM test.
        (
            RulesInteger::from_usize(layout_count),
            TokenRecipients::Controller,
        ),
    ] {
        let mut creation = token_creation();
        creation.amount = crate::program::ValueExpr::Literal(amount);
        creation.recipients = recipients;
        let state = pending(wrapped(Instruction::CreateTokens {
            creations: vec![creation],
        }));
        let before = state.clone();
        reject(
            &state,
            A,
            command_reply(
                &state,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            ),
            GameError::Effect(EffectError::TokenCapacity),
        );
        assert_eq!(state, before);
        assert!(
            optional_frame(&state)
                .options()
                .contains(&OptionalEffectChoice::Take)
        );
        let declined = reply(
            &state,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert_tail(&declined);
        assert_eq!(outcomes(&declined), 1);
        assert!(
            !occurrences(&declined)
                .iter()
                .any(|row| matches!(row, RulesOccurrence::TokenCreated { .. }))
        );
        assert_eq!(
            declined.state().execution().window().cards().iter().count(),
            5
        );
    }
}

#[test]
fn optional_token_two_apnap_order_children_keep_locals_and_reject_each_bad_reply_before_one_birth_batch()
 {
    let mut creation = token_creation();
    creation.amount = crate::program::ValueExpr::Literal(2.into());
    creation.recipients = TokenRecipients::EachPlayer;
    let state = pending(vec![
        Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
            chooser: crate::toolbox::PlayerSelector::Resolving(
                crate::program::ResolvingPlayer::Controller,
            ),
            domain: crate::toolbox::PromptDomain::Subtype {
                kind: crate::toolbox::SubtypeChoiceKind::Creature,
                excluding: BTreeSet::new(),
                legacy_slot: Some(CreatureTypeSlot::new(0)),
            },
            category: crate::toolbox::ChoiceCategory::Generic,
            bind_to: crate::toolbox::BindingId(0),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
            option_branches: Vec::new(),
        })),
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        Instruction::CreateTokens {
            creations: vec![creation],
        },
        gain(2),
    ]);
    let human = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    let optional = reply(
        &state,
        DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(human)),
    );
    let declined = reply(
        optional.state(),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_tail(&declined);
    assert_eq!(outcomes(&declined), 1);
    assert!(
        !occurrences(&declined)
            .iter()
            .any(|row| matches!(row, RulesOccurrence::TokenCreated { .. }))
    );
    let old_optional = command_reply(
        optional.state(),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    let taken = reply(
        optional.state(),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(outcomes(&taken), 1);
    let mut child = taken.into_state();
    let mut previous_completion = None;
    for chooser in [A, B] {
        let RuleFrame::TimestampOrder(frame) = child.pending_decision().unwrap().frame() else {
            panic!("actual APNAP token-order child");
        };
        let frame = frame.clone();
        assert_eq!(frame.chooser(), chooser);
        assert_eq!(frame.instruction(), 2);
        assert_eq!(frame.members().len(), 2);
        assert_eq!(
            child.execution().resolution_state().unwrap().locals(),
            &BTreeMap::from([(CreatureTypeSlot::new(0), human)])
        );
        assert_eq!(child.execution().window().cards().iter().count(), 5);
        let order = frame
            .members()
            .iter()
            .rev()
            .map(|member| member.member)
            .collect::<Vec<_>>();
        let valid = command_reply(&child, DecisionReply::TimestampOrder(order.clone()));
        let mut denials = vec![
            (
                if chooser == A { B } else { A },
                valid.clone(),
                GameError::WrongChooser,
            ),
            (chooser, old_optional.clone(), GameError::WrongDecision),
            (
                chooser,
                command_reply(
                    &child,
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                ),
                GameError::Effect(EffectError::WrongDecisionKind),
            ),
            (
                chooser,
                command_reply(&child, DecisionReply::TimestampOrder(vec![])),
                GameError::Effect(EffectError::IllegalTimestampOrder),
            ),
            (
                chooser,
                command_reply(
                    &child,
                    DecisionReply::TimestampOrder(vec![order[0], order[0]]),
                ),
                GameError::Effect(EffectError::IllegalTimestampOrder),
            ),
            (
                chooser,
                command_reply(
                    &child,
                    DecisionReply::TimestampOrder(vec![order[0], TokenBatchMember::new(99)]),
                ),
                GameError::Effect(EffectError::IllegalTimestampOrder),
            ),
        ];
        if let Some(old) = previous_completion.take() {
            denials.push((chooser, old, GameError::WrongDecision));
        }
        for (actor, command, error) in denials {
            reject(&child, actor, command, error);
            let completed = candidate(&child, chooser, valid.clone());
            assert_eq!(outcomes(&completed), 0);
            if chooser == A {
                assert_eq!(
                    completed
                        .state()
                        .execution()
                        .window()
                        .cards()
                        .iter()
                        .count(),
                    5
                );
                let RuleFrame::TimestampOrder(next) =
                    completed.state().pending_decision().unwrap().frame()
                else {
                    panic!("next APNAP group")
                };
                assert_eq!(next.chooser(), B);
            } else {
                assert_tail(&completed);
                assert_eq!(
                    occurrences(&completed)
                        .iter()
                        .filter(|row| matches!(row, RulesOccurrence::TokenCreated { .. }))
                        .count(),
                    4
                );
            }
        }
        previous_completion = Some(valid.clone());
        let completed = candidate(&child, chooser, valid.clone());
        if chooser == B {
            let batches = completed.rules_journal().records.iter().filter(|record| matches!(record,
                RulesRecord::Event(event) if event.occurrences.iter().any(|row| matches!(row.value, RulesOccurrence::TokenCreated { .. }))
            )).count();
            assert_eq!(batches, 1);
            let objects = occurrences(&completed)
                .into_iter()
                .filter_map(|row| match row {
                    RulesOccurrence::TokenCreated { creator, after } => {
                        Some((*creator, after.object.id()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                objects
                    .iter()
                    .map(|(creator, _)| *creator)
                    .collect::<Vec<_>>(),
                vec![A, A, B, B]
            );
            let timestamps = objects
                .iter()
                .map(|(_, object)| {
                    completed
                        .state()
                        .execution()
                        .characteristics
                        .object_timestamp(*object)
                        .unwrap()
                })
                .collect::<Vec<_>>();
            assert!(timestamps[1] < timestamps[0]);
            assert!(timestamps[0] < timestamps[3]);
            assert!(timestamps[3] < timestamps[2]);
            assert_eq!(
                completed
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .count(),
                9
            );
            assert_tail(&completed);
            reject(completed.state(), B, valid, GameError::NoDecision);
        }
        child = completed.into_state();
    }
}

#[test]
fn both_optional_and_payment_actor_variants_reject_then_accept_the_same_authorized_choice() {
    let payment = tap(&tap(&pending(offer(2)), 10).into_state(), 11);
    for choice in [
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(ManaType::Blue, 2))),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    ] {
        reject(
            payment.state(),
            B,
            command_reply(payment.state(), choice.clone()),
            GameError::WrongChooser,
        );
        let continued = reply(payment.state(), choice);
        assert_eq!(payment_outcomes(&continued), 1);
        assert!(continued.state().pending_decision().is_none());
    }
    let optional = pending(wrapped(gain(1)));
    for (choice, life) in [
        (OptionalEffectChoice::Take, 23),
        (OptionalEffectChoice::Decline, 22),
    ] {
        let choice = DecisionReply::OptionalEffect(choice);
        reject(
            &optional,
            B,
            command_reply(&optional, choice.clone()),
            GameError::WrongChooser,
        );
        let continued = reply(&optional, choice);
        assert_eq!(outcomes(&continued), 1);
        assert_eq!(
            continued.state().execution().life_total(A),
            Some(&life.into())
        );
        assert!(continued.state().pending_decision().is_none());
    }
}

#[test]
fn consumed_pay_and_decline_reject_both_original_inputs_and_current_basis_reuse_without_a_second_outcome()
 {
    let funded = tap(&pending(offer(1)), 10);
    for choice in [
        ResolutionPaymentReply::Pay(spend(ManaType::Blue, 1)),
        ResolutionPaymentReply::Decline,
    ] {
        let original = GameInput {
            basis: funded.state().basis().clone(),
            actor: A,
            command: command_reply(funded.state(), DecisionReply::ResolutionPayment(choice)),
        };
        let completed = funded.state().advance(original.clone()).unwrap();
        let before = completed.state().clone();
        assert_eq!(
            before.advance(original.clone()),
            Err(GameError::StaleRevision)
        );
        assert_eq!(completed.state(), &before);
        reject(&before, A, original.command, GameError::NoDecision);
        let continued = candidate(&before, A, GameCommand::Pass);
        assert_eq!(payment_outcomes(&continued), 0);
        assert_eq!(
            continued.state().execution().life_total(A),
            before.execution().life_total(A)
        );
        assert_eq!(
            continued.state().execution().mana(A, ManaType::Blue),
            before.execution().mana(A, ManaType::Blue)
        );
    }
}

#[test]
fn stale_pay_variants_reject_at_fixed_and_color_prefixes_then_current_decline_keeps_every_tap() {
    let offered = pending(offer(3));
    let old_frame = payment_frame(&offered);
    let original = GameInput {
        basis: offered.basis().clone(),
        actor: A,
        command: command_reply(
            &offered,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(ManaType::Blue, 3))),
        ),
    };
    let fixed = tap(&offered, 10);
    let color = tap(fixed.state(), 12);
    let restored = reply(color.state(), DecisionReply::ManaColor(ManaType::Blue));
    for (state, pool) in [
        (fixed.state(), 1),
        (color.state(), 2),
        (restored.state(), 2),
    ] {
        let before = state.clone();
        assert_eq!(
            state.advance(original.clone()),
            Err(GameError::StaleRevision)
        );
        reject(state, A, original.command.clone(), GameError::WrongDecision);
        assert_eq!(
            state.execution().resume_rule(
                state.execution().revision(),
                &EffectDecision::ResolutionPayment(old_frame.clone()),
                A,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(
                    ManaType::Blue,
                    3
                ))),
                None,
            ),
            Err(EffectError::WrongResolutionPaymentFrame)
        );
        assert_eq!(state, &before);
        let current = if matches!(
            state.pending_decision().unwrap().frame(),
            RuleFrame::ManaColor(_)
        ) {
            reply(state, DecisionReply::ManaColor(ManaType::Blue)).into_state()
        } else {
            state.clone()
        };
        let continued = reply(
            &current,
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        );
        assert_tail(&continued);
        assert_eq!(payment_outcomes(&continued), 1);
        assert_eq!(
            continued.state().execution().mana(A, ManaType::Blue),
            pool.into()
        );
        assert_eq!(
            continued
                .state()
                .execution()
                .window()
                .cards()
                .get(object(state, 10))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
}

#[test]
fn optional_trigger_token_allocator_failures_preserve_owned_work_and_allow_decline_without_counter_reset()
 {
    // This real captured trigger has no physical spell exit to allocate after
    // Decline. A poisoned token allocator therefore leaves a valid alternative.
    let state = captured_after_source_change(
        false,
        wrapped(Instruction::CreateTokens {
            creations: vec![token_creation()],
        }),
    );
    for mode in 0..3 {
        let mut execution = state.execution().clone();
        let error = match mode {
            0 => {
                execution.set_test_object_allocator(u64::MAX, false);
                EffectError::Window(StackWindowError::Objects(
                    crate::object_identity::ObjectIdentityError::RevisionExhausted,
                ))
            }
            1 => {
                execution.set_test_object_allocator(u64::MAX, true);
                EffectError::Window(StackWindowError::Objects(
                    crate::object_identity::ObjectIdentityError::IdentityExhausted,
                ))
            }
            2 => {
                execution.set_test_characteristic_timestamp(u64::MAX);
                EffectError::Characteristics(
                    crate::characteristics::CharacteristicsError::TimestampExhausted,
                )
            }
            _ => unreachable!(),
        };
        let frame = execution.pending_resolution_decision().unwrap();
        let before = execution.clone();
        assert_eq!(
            execution.resume_rule(
                execution.revision(),
                &frame,
                A,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                None,
            ),
            Err(error)
        );
        assert_eq!(execution, before);
        assert_eq!(execution.life_total(A), Some(&20.into()));
        let declined = execution
            .resume_rule(
                execution.revision(),
                &frame,
                A,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
                None,
            )
            .unwrap();
        assert_eq!(declined.state().life_total(A), Some(&22.into()));
        assert!(matches!(declined.result(), RuleResult::Complete { .. }));
        assert_eq!(
            declined
                .append()
                .records
                .iter()
                .filter(|record| matches!(
                    record,
                    RulesRecord::OptionalEffectOutcome {
                        outcome: OptionalEffectChoice::Decline,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(!declined.append().records.iter().any(|record| matches!(record,
            RulesRecord::Event(event) if event.occurrences.iter().any(|row| matches!(row.value, RulesOccurrence::TokenCreated { .. }))
        )));
        assert_eq!(
            declined.state().window().cards().iter().count(),
            execution.window().cards().iter().count()
        );
    }
}

#[test]
fn decision_identity_exhaustion_after_a_fixed_prefix_still_allows_current_decline() {
    let fixed = tap(&pending(offer(3)), 10);
    let mut state = fixed.into_state();
    state.set_test_next_decision(u64::MAX);
    reject(
        &state,
        A,
        GameCommand::ActivateMana {
            source: object(&state, 11),
            ability: ABILITY,
        },
        GameError::DecisionIdentityExhausted,
    );
    assert_eq!(state.execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 11))
            .unwrap()
            .tapped(),
        Some(false)
    );
    let declined = reply(
        &state,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_tail(&declined);
    assert_eq!(payment_outcomes(&declined), 1);
    assert_eq!(
        declined.state().execution().mana(A, ManaType::Blue),
        1.into()
    );
    assert_eq!(state.next_decision_sequence(), u64::MAX);
}

#[test]
fn both_actual_optional_damage_orders_retain_locals_and_targets_through_denials_then_apply_once() {
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(
                1,
                A,
                BodyTarget::SINGLE_CREATURE,
                vec![
                    Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                        chooser: crate::toolbox::PlayerSelector::Resolving(
                            crate::program::ResolvingPlayer::Controller,
                        ),
                        domain: crate::toolbox::PromptDomain::Subtype {
                            kind: crate::toolbox::SubtypeChoiceKind::Creature,
                            excluding: BTreeSet::new(),
                            legacy_slot: Some(CreatureTypeSlot::new(0)),
                        },
                        category: crate::toolbox::ChoiceCategory::Generic,
                        bind_to: crate::toolbox::BindingId(0),
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial: false,
                        option_branches: Vec::new(),
                    })),
                    Instruction::ApplyContinuous(Box::new(crate::toolbox::ApplyContinuousSpec {
                        selector: None,
                        payload: crate::toolbox::ContinuousPayload::TargetPreventionShield {
                            amount: 1.into(),
                        },
                        duration: crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                        condition: None,
                        one_shot: true,
                        as_emblem: None,
                        emblem_label: None,
                    })),
                    Instruction::ApplyContinuous(Box::new(
                        crate::toolbox::ApplyContinuousSpec::damage_multiplier(2.into()),
                    )),
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    Instruction::DealDamage(Box::new(crate::toolbox::DealDamageSpec {
                        source: crate::toolbox::DamageSourceQuery::EntrySource,
                        recipients: crate::toolbox::DamageTargetQuery::Targets,
                        amount: crate::program::DamageAmountSpec::Expr(
                            crate::program::ValueExpr::Literal(3.into()),
                        ),
                        exile_if_dies: false,
                    })),
                    gain(2),
                ],
            ),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
        ],
    );
    let target = object(&initial, 2);
    let type_child = resolve_top(cast(&initial, A, 1, Some(TargetRef::Object(target))).state());
    let human = type_child
        .state()
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    let optional = reply(
        type_child.state(),
        DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(human)),
    );
    let payload = projection::resolving_payload(optional.state().execution());
    let declined = reply(
        optional.state(),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_tail(&declined);
    assert_eq!(outcomes(&declined), 1);
    assert!(declined.damage_journal().applied.is_empty());
    assert_eq!(
        declined
            .state()
            .execution()
            .battlefield_marked_damage(target),
        Some(0.into())
    );

    for multiplier_first in [false, true] {
        let taken = reply(
            optional.state(),
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
        assert_eq!(outcomes(&taken), 1);
        let child = taken.state();
        let RuleFrame::Damage(DamageDecision::Order(frame)) =
            child.pending_decision().unwrap().frame()
        else {
            panic!("actual optional damage Order child");
        };
        assert_eq!(frame.chooser(), B);
        assert_eq!(frame.applications().len(), 2);
        let cursor = child.execution().resolution_state().unwrap();
        assert_eq!(cursor.instruction(), 4);
        assert_eq!(
            cursor.locals(),
            &BTreeMap::from([(CreatureTypeSlot::new(0), human)])
        );
        assert_eq!(projection::resolving_payload(child.execution()), payload);
        let selected = *frame
            .applications()
            .iter()
            .find(|application| {
                matches!(
                    application.effect,
                    crate::damage::DamageEffectId::Multiplier(_)
                ) == multiplier_first
            })
            .unwrap();
        let valid = command_reply(child, DecisionReply::DamageOrder(selected));
        let mut foreign = selected;
        foreign.scope =
            crate::damage::DamageScope::Recipient(crate::damage::DamageRecipient::Player(A));
        let denials = [
            (A, valid.clone(), GameError::WrongChooser),
            (
                B,
                command_reply(
                    optional.state(),
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                ),
                GameError::WrongDecision,
            ),
            (
                B,
                command_reply(
                    child,
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                ),
                GameError::Effect(EffectError::WrongDecisionKind),
            ),
            (
                B,
                command_reply(child, DecisionReply::AllocatePrevention(vec![])),
                GameError::Effect(EffectError::WrongDecisionKind),
            ),
            (
                B,
                command_reply(child, DecisionReply::DamageOrder(foreign)),
                GameError::Effect(EffectError::Damage(
                    crate::damage::DamageError::InapplicableEffect,
                )),
            ),
        ];
        for (actor, command, error) in denials {
            reject(child, actor, command, error);
            let completed = candidate(child, B, valid.clone());
            assert_tail(&completed);
            assert_eq!(outcomes(&completed), 0);
            assert_eq!(
                completed
                    .state()
                    .execution()
                    .battlefield_marked_damage(target),
                Some(if multiplier_first { 5.into() } else { 4.into() })
            );
            assert_eq!(completed.damage_journal().applied.len(), 1);
            assert_eq!(completed.damage_journal().multiplications.len(), 1);
            assert_eq!(completed.damage_journal().preventions.len(), 1);
            assert!(completed.state().execution().pending_damage().is_none());
            reject(completed.state(), B, valid.clone(), GameError::NoDecision);
        }
    }
}

#[test]
fn optional_type_child_rejects_an_unexcluded_member_of_a_foreign_vocabulary_then_accepts_the_real_member()
 {
    let state = pending(wrapped(Instruction::PromptChoice(Box::new(
        crate::toolbox::PromptChoiceSpec {
            chooser: crate::toolbox::PlayerSelector::Resolving(
                crate::program::ResolvingPlayer::Controller,
            ),
            domain: crate::toolbox::PromptDomain::Subtype {
                kind: crate::toolbox::SubtypeChoiceKind::Creature,
                excluding: BTreeSet::new(),
                legacy_slot: Some(CreatureTypeSlot::new(0)),
            },
            category: crate::toolbox::ChoiceCategory::Generic,
            bind_to: crate::toolbox::BindingId(0),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
            option_branches: Vec::new(),
        },
    ))));
    let taken = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    let child = taken.state();
    // This vocabulary supplies only the adversarial ID. The actual game and
    // issued child continue to use the explicit genuine selected vocabulary.
    let foreign = creature_types::synthetic_vocabulary()
        .resolve_label("Elf")
        .unwrap();
    let real = child
        .execution()
        .creature_vocabulary()
        .resolve_label("Elf")
        .unwrap();
    assert_ne!(foreign.vocabulary(), real.vocabulary());
    reject(
        child,
        A,
        command_reply(
            child,
            DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(foreign)),
        ),
        GameError::Effect(EffectError::SelectionOutsideDomain),
    );
    let completed = reply(
        child,
        DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(real)),
    );
    assert_tail(&completed);
    assert_eq!(outcomes(&completed), 0);
    assert_eq!(occurrences(&completed).iter().filter(|row| matches!(row, RulesOccurrence::CreatureTypeChosen { value, .. } if *value == real)).count(), 1);
}

#[test]
fn optional_decline_cursor_increment_overflow_is_a_typed_atomic_owner_rejection() {
    let state = pending(wrapped(gain(1)));
    let mut execution = state.execution().clone();
    let Some(PendingWork::OptionalEffect(work)) = &mut execution.pending_work else {
        panic!("real optional owner")
    };
    work.cursor.instruction = usize::MAX - 1;
    let frame = execution.pending_resolution_decision().unwrap();
    let before = execution.clone();
    assert_eq!(
        execution.resume_rule(
            execution.revision(),
            &frame,
            A,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
            None,
        ),
        Err(EffectError::InconsistentProgram)
    );
    assert_eq!(execution, before);
    // This deliberately malformed cursor has no representable valid successor.
    // Keep that limit distinct from the untouched root's actual continuation.
    let continued = reply(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_tail(&continued);
    assert_eq!(outcomes(&continued), 1);
}

#[test]
fn payment_and_color_publication_exhaustion_keep_latest_executor_and_accepted_taps() {
    let fixed = tap(&pending(offer(3)), 10);
    let color = tap(fixed.state(), 12);
    for mode in 0..3 {
        let mut execution = fixed.state().execution().clone();
        let error = match mode {
            0 => {
                execution.set_test_window_revision(u64::MAX);
                EffectError::Window(StackWindowError::RevisionExhausted)
            }
            1 => {
                execution.set_test_object_allocator(u64::MAX, false);
                EffectError::Window(StackWindowError::Objects(
                    crate::object_identity::ObjectIdentityError::RevisionExhausted,
                ))
            }
            2 => {
                execution.set_test_publication_event(u64::MAX);
                EffectError::Trigger(crate::occurrence::TriggerError::EventIdentityExhausted)
            }
            _ => unreachable!(),
        };
        let before = execution.clone();
        assert_eq!(
            execution.begin_mana_activation(
                execution.revision(),
                A,
                object(fixed.state(), 11),
                ABILITY
            ),
            Err(error)
        );
        assert_eq!(execution, before);
        assert_eq!(execution.mana(A, ManaType::Blue), 1.into());
        assert_eq!(
            execution
                .window()
                .cards()
                .get(object(fixed.state(), 10))
                .unwrap()
                .tapped(),
            Some(true)
        );
        assert_eq!(
            execution
                .window()
                .cards()
                .get(object(fixed.state(), 11))
                .unwrap()
                .tapped(),
            Some(false)
        );
    }
    let mut execution = color.state().execution().clone();
    execution.set_test_publication_event(u64::MAX);
    let before = execution.clone();
    let frame = execution.pending_mana_color().unwrap().clone();
    assert_eq!(
        execution.resume_mana_color(execution.revision(), &frame, A, ManaType::Blue),
        Err(EffectError::Trigger(
            crate::occurrence::TriggerError::EventIdentityExhausted
        ))
    );
    assert_eq!(execution, before);
    assert_eq!(execution.mana(A, ManaType::Blue), 1.into());
    assert!(matches!(
        execution.resolution_choice_state(),
        Some(ResolutionChoiceState::Payment {
            suspended_for_mana_color: true,
            ..
        })
    ));
    // These injected allocator boundaries can also prevent physical spell exit.
    // Continuation is therefore checked on the untouched actual root prefix,
    // without resetting a poisoned counter and calling that normal gameplay.
    let restored = reply(color.state(), DecisionReply::ManaColor(ManaType::Blue));
    let continued = reply(
        restored.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_tail(&continued);
    assert_eq!(payment_outcomes(&continued), 1);
    assert_eq!(
        continued.state().execution().mana(A, ManaType::Blue),
        2.into()
    );
}
