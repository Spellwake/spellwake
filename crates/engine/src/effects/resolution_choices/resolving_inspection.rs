//! Regression for complete observation of an actually resolving copied offer.
//! Synthetic native mechanism only; ordinary R21 separately exercises its reader.
use super::*;
use spellwake_contracts::names::{EnglishCardName, ObjectNames};

#[test]
fn resolving_copy_retains_its_complete_payload_after_original_entry_departure() {
    let mut original = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(BodyTarget::None, offer(1)),
    );
    original.names =
        ObjectNames::single(EnglishCardName::new("Native Payment Spell".into()).unwrap());
    original.supertypes.insert(crate::program::Supertype::Snow);
    let mut mana = Program::artifact(None, vec![]);
    mana.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::ChooseColor {
            amount: 1.into(),
            colors: BTreeSet::from([ManaType::Blue, ManaType::Black]),
        },
    });
    let state = world(
        vec![A, B],
        A,
        vec![
            seed(1, A, ObjectDestination::Hand, original.clone()),
            hand(
                2,
                B,
                BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
                vec![Instruction::CopySpell],
            ),
            hand(
                3,
                A,
                BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
                vec![Instruction::CounterSpell],
            ),
            seed(4, B, ObjectDestination::Battlefield { controller: B }, mana),
        ],
    );
    let announced = cast(&state, A, 1, None);
    let original_entry = announced.state().execution().window().entries()[0].id();
    let priority = candidate(announced.state(), A, GameCommand::Pass);
    let copier = cast(
        priority.state(),
        B,
        2,
        Some(TargetRef::Stack(original_entry)),
    );
    let copied = resolve_top(copier.state());
    assert!(copied.state().pending_decision().is_none());
    let copied_entry = copied
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert!(matches!(copied_entry.kind(), StackKind::SpellCopy { .. }));
    assert_eq!(copied_entry.controller(), B);
    let copy_entry = copied_entry.id();

    // Remove the original through a real response before resolving the copy.
    let counter = cast(copied.state(), A, 3, Some(TargetRef::Stack(original_entry)));
    let removed = resolve_top(counter.state());
    assert_eq!(removed.state().execution().window().entries().len(), 1);
    assert_eq!(
        removed.state().execution().window().entries()[0].id(),
        copy_entry
    );
    assert!(
        removed
            .state()
            .execution()
            .stack_copiable_names(original_entry)
            .is_none()
    );
    let queued_payload = projection::stack_payloads(removed.state().execution())
        .pop()
        .unwrap();
    let offered = resolve_top(removed.state());
    let state = offered.state();
    let execution = state.execution();
    assert_eq!(payment_frame(state).payer(), B);
    assert_eq!(payment_frame(state).entry(), copy_entry);
    // Starting resolution retains the spell on the stack until its final exit
    // (CR 608.2n). Removing the original did not remove this distinct copy.
    assert_eq!(execution.window().entries().len(), 1);
    assert_eq!(execution.window().entries()[0].id(), copy_entry);
    assert_eq!(execution.window().resolving().unwrap().id(), copy_entry);
    assert_eq!(
        execution.stack_copiable_names(copy_entry),
        Some(&original.names)
    );
    assert_eq!(
        execution.stack_copiable_supertypes(copy_entry),
        Some(&original.supertypes)
    );
    assert_eq!(
        execution.stack_copiable_card_types(copy_entry),
        Some(&original.card_types)
    );
    assert_eq!(
        execution.stack_copiable_creature_subtypes(copy_entry),
        Some(&original.creature_subtypes)
    );
    assert_eq!(
        execution.stack_copiable_triggered_abilities(copy_entry),
        Some(original.triggered.as_slice())
    );
    let resolving = projection::resolving_payload(execution).unwrap();
    assert_eq!(resolving, queued_payload);
    assert_eq!(resolving["controller"], B.0);
    assert_eq!(resolving["kind"]["tag"], "SpellCopy");
    assert_eq!(resolving["targets"], serde_json::json!([]));
    let crate::program::ProgramKind::Spell(spell) = &original.kind else {
        unreachable!()
    };
    assert_eq!(
        resolving["body"],
        crate::test_support::program::body(&spell.body)
    );
    assert_eq!(resolving["bindings"], serde_json::json!({"trigger":null}));
    let aliases = AliasContext::from_card_aliases(&[
        (CardId(1), "original".into()),
        (CardId(2), "copier".into()),
        (CardId(3), "counter".into()),
        (CardId(4), "color-source".into()),
    ])
    .unwrap();
    let full = projection::state(state, &aliases).unwrap();
    assert_eq!(full["resolving"], resolving);
    assert_eq!(full["pending_work"]["tag"], "ResolutionPayment");
    assert_eq!(full["pending_work"]["work"]["payer"], B.0);

    let color = candidate(
        state,
        B,
        GameCommand::ActivateMana {
            source: object(state, 4),
            ability: ABILITY,
        },
    );
    assert!(matches!(
        color.state().pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    let suspended = projection::state(color.state(), &aliases).unwrap();
    assert_eq!(suspended["resolving"], resolving);
    assert_eq!(suspended["pending_work"]["tag"], "ManaColor");
    assert_eq!(suspended["pending_work"]["parent"]["tag"], "Payment");
    assert_eq!(suspended["pending_work"]["parent"]["work"]["payer"], B.0);
    let funded = reply(color.state(), DecisionReply::ManaColor(ManaType::Blue));
    assert_eq!(
        projection::resolving_payload(funded.state().execution()),
        Some(resolving)
    );

    let paid = reply(
        funded.state(),
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
            spend: vec![ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            }],
        })),
    );
    assert!(paid.state().pending_decision().is_none());
    assert!(projection::resolving_payload(paid.state().execution()).is_none());
    assert!(
        paid.state()
            .execution()
            .stack_copiable_names(copy_entry)
            .is_none()
    );
    assert_eq!(paid.state().execution().life_total(B), Some(&20.into()));
}
