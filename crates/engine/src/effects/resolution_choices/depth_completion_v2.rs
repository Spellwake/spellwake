//! Surgical DP02 variants identified by the independent v1 source review.
//! Source-only draft until the parent binds actual checks and native execution.
use super::*;
use crate::land::BasicLandType;

#[test]
fn payment_rejects_wrong_or_absent_intrinsic_ability_then_uses_the_same_sources_actual_ability() {
    let mut printed = Program::artifact(None, vec![]);
    printed.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
    });
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::None, offer(2)),
            seed(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                printed.clone(),
            ),
            seed(
                11,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::land(BTreeSet::from([BasicLandType::Island.land_type()]), vec![]),
            ),
            seed(
                12,
                A,
                ObjectDestination::Battlefield { controller: A },
                printed,
            ),
        ],
    );
    let offered = resolve_top(cast(&initial, A, 1, None).state());
    let prefix = tap(offered.state(), 10);
    for (card, absent, actual) in [
        (
            11,
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
            ManaAbilityRef::BasicLand(BasicLandType::Island),
        ),
        (
            12,
            ManaAbilityRef::BasicLand(BasicLandType::Island),
            ABILITY,
        ),
    ] {
        let state = prefix.state();
        let source = object(state, card);
        let quote = payment_frame(state);
        reject(
            state,
            A,
            GameCommand::ActivateMana {
                source,
                ability: absent,
            },
            GameError::Effect(EffectError::UnknownManaAbility),
        );
        assert_eq!(payment_frame(state), quote);
        assert_eq!(state.execution().mana(A, ManaType::Blue), 1.into());
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(false)
        );
        let funded = candidate(
            state,
            A,
            GameCommand::ActivateMana {
                source,
                ability: actual,
            },
        );
        let production = funded.mana_production().unwrap();
        assert_eq!(production.source, source);
        assert_eq!(production.ability, actual);
        assert_eq!(
            production.produced,
            BTreeMap::from([(ManaType::Blue, 1.into())])
        );
        assert_eq!(funded.state().execution().mana(A, ManaType::Blue), 2.into());
        assert_eq!(
            funded
                .state()
                .execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(true)
        );
        let paid = reply(
            funded.state(),
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: 2.into(),
                }],
            })),
        );
        assert!(paid.state().pending_decision().is_none());
        assert_eq!(paid.state().execution().mana(A, ManaType::Blue), 0.into());
        assert_eq!(paid.state().execution().life_total(A), Some(&20.into()));
        assert_eq!(paid.rules_journal().records.iter().filter(|record| matches!(record,
            RulesRecord::ResolutionPaymentOutcome { outcome: ResolutionPaymentOutcome::Paid(payment), .. }
                if payment.source == quote.entry() && payment.payer == A && payment.cost == *quote.cost()
        )).count(), 1);
    }
}

#[test]
fn consumed_color_rejects_the_complete_original_input_and_current_basis_old_id_then_declines_once()
{
    let fixed = tap(&pending(offer(3)), 10);
    let color = tap(fixed.state(), 12);
    let original = GameInput {
        basis: color.state().basis().clone(),
        actor: A,
        command: command_reply(color.state(), DecisionReply::ManaColor(ManaType::Blue)),
    };
    let restored = color.state().advance(original.clone()).unwrap();
    assert!(matches!(
        restored.state().pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    assert_eq!(
        restored.state().execution().mana(A, ManaType::Blue),
        2.into()
    );
    assert_eq!(
        restored.mana_production().unwrap().produced,
        BTreeMap::from([(ManaType::Blue, 1.into())])
    );
    let repeated_actual_transition = color.state().advance(original.clone()).unwrap();
    assert_eq!(
        repeated_actual_transition.rules_journal(),
        restored.rules_journal()
    );
    assert_eq!(repeated_actual_transition.state(), restored.state());

    let latest = restored.state().clone();
    assert_eq!(
        restored.state().advance(original.clone()),
        Err(GameError::StaleRevision)
    );
    assert_eq!(restored.state(), &latest);
    reject(
        restored.state(),
        A,
        original.command,
        GameError::WrongDecision,
    );
    let declined = reply(
        restored.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert_tail(&declined);
    assert_eq!(
        declined.state().execution().mana(A, ManaType::Blue),
        2.into()
    );
    for card in [10, 12] {
        assert_eq!(
            declined
                .state()
                .execution()
                .window()
                .cards()
                .get(object(declined.state(), card))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    assert_eq!(
        declined
            .rules_journal()
            .records
            .iter()
            .filter(|record| matches!(
                record,
                RulesRecord::ResolutionPaymentOutcome {
                    outcome: ResolutionPaymentOutcome::Declined,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(
        !occurrences(&declined)
            .iter()
            .any(|row| matches!(row, RulesOccurrence::ManaProduced { .. }))
    );
}

#[test]
fn retain_copy_reply_rejects_wrong_actor_and_both_reuse_forms_before_real_retained_target_resolution()
 {
    let initial = world(
        vec![A, B],
        A,
        vec![
            hand(1, A, BodyTarget::None, vec![gain(0)]),
            hand(
                2,
                A,
                BodyTarget::single_spell(crate::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::counter_targeted(
                        crate::toolbox::CounterDestination::Graveyard,
                    ),
                ))],
            ),
            hand(
                3,
                A,
                BodyTarget::single_spell(crate::program::SpellFilter::copyable_instruction()),
                wrapped(Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::copy_targeted_spell(),
                ))),
            ),
        ],
    );
    let base = cast(&initial, A, 1, None);
    let original_target = base.state().execution().window().entries()[0].id();
    let counter = cast(base.state(), A, 2, Some(TargetRef::Stack(original_target)));
    let donor = counter.state().execution().window().entries()[1].id();
    let copier = cast(counter.state(), A, 3, Some(TargetRef::Stack(donor)));
    let optional = resolve_top(copier.state());
    let taken = reply(
        optional.state(),
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(outcomes(&taken), 1);
    let child = taken.state();
    let RuleFrame::CopyTargets(frame) = child.pending_decision().unwrap().frame() else {
        panic!("actual targeted-copy child");
    };
    assert_eq!(frame.original(), donor);
    assert_eq!(frame.chooser(), A);
    let retained = GameInput {
        basis: child.basis().clone(),
        actor: A,
        command: command_reply(child, DecisionReply::CopyTargets(CopyTargetChoice::Retain)),
    };
    reject(child, B, retained.command.clone(), GameError::WrongChooser);
    let completed = child.advance(retained.clone()).unwrap();
    assert_tail(&completed);
    assert_eq!(outcomes(&completed), 0);
    assert_eq!(
        completed.state().execution().window().spell_copies().len(),
        1
    );
    let copy = completed
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert!(
        matches!(copy.kind(), StackKind::SpellCopy { copied_from, .. } if *copied_from == donor)
    );
    assert_eq!(copy.targets().targets().len(), 1);
    assert_eq!(
        copy.targets().targets()[0].choice().target,
        TargetRef::Stack(original_target)
    );
    assert_eq!(occurrences(&completed).iter().filter(|row| matches!(row,
        RulesOccurrence::CopyCreated { entry, original, .. } if *entry == copy.id() && *original == donor
    )).count(), 1);
    let replayed = child.advance(retained.clone()).unwrap();
    assert_eq!(replayed.rules_journal(), completed.rules_journal());
    assert_eq!(replayed.state(), completed.state());

    let latest = completed.state().clone();
    assert_eq!(
        completed.state().advance(retained.clone()),
        Err(GameError::StaleRevision)
    );
    assert_eq!(completed.state(), &latest);
    reject(
        completed.state(),
        A,
        retained.command,
        GameError::NoDecision,
    );
    let first_pass = candidate(completed.state(), A, GameCommand::Pass);
    let applied_copy = candidate(first_pass.state(), B, GameCommand::Pass);
    assert_eq!(
        applied_copy.state().execution().life_total(A),
        Some(&22.into())
    );
    assert_eq!(
        applied_copy
            .state()
            .execution()
            .window()
            .cards()
            .get(object(applied_copy.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    let entries = applied_copy.state().execution().window().entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id(), donor);
    assert_eq!(outcomes(&applied_copy), 0);
    assert!(
        !occurrences(&applied_copy)
            .iter()
            .any(|row| matches!(row, RulesOccurrence::CopyCreated { .. }))
    );
}
