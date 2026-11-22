//! Independent activations and actual spending dependencies within attack costs.
use super::*;
use spellwake_engine::{
    game::AttackManaId,
    mana::{ManaAbility, ManaCost, ManaOutput},
};

fn source(n: u64, output: ManaOutput) -> SeedCard {
    seed(
        n,
        A,
        battlefield(A),
        Program::land(
            Default::default(),
            vec![ManaAbility {
                id: ManaAbilityId(0),
                output,
            }],
        ),
    )
}

fn payment(mut cards: Vec<SeedCard>, budget: &[(ManaType, i64)]) -> GameState {
    cards.push(creature(1, A, 2, 3, &[KeywordAbility::Haste]));
    cards.push(named("Baird, Steward of Argive", 2, B, battlefield(B)));
    let game = game(vec![A, B], cards, budget, Step::DeclareAttackers);
    let frame = combat_frame(&game, true);
    let attacker = physical(&frame, 1).id();
    reply(
        &frame,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state()
}

fn tap(s: &GameState, n: u64) -> GameState {
    command(
        s,
        A,
        GameCommand::ActivateMana {
            source: physical(s, n).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state()
}
fn reverse(ids: &[u64]) -> DecisionReply {
    DecisionReply::RedeclareReversingMana(ids.iter().copied().map(AttackManaId).collect())
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_baird_selectively_reverses_mana_and_rejects_dangling_payment_dependencies() {
    use ManaType::*;
    let start = payment(
        vec![
            named("Sol Ring", 3, A, battlefield(A)),
            source(
                4,
                ManaOutput::Fixed([(Green, 1.into())].into()).paid(ManaCost::generic(1.into())),
            ),
            named("Sol Ring", 5, A, battlefield(A)),
        ],
        &[],
    );
    let one = tap(&start, 3);
    let two = command(
        &one,
        A,
        GameCommand::ActivateManaWithPayment {
            source: physical(&one, 4).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            payment: spend(&[(Colourless, 1)]),
        },
    )
    .into_state();
    let three = tap(&two, 5);
    let choices: Vec<_> = three.attack_mana_choices().collect();
    assert_eq!(choices.len(), 3);
    assert_eq!(choices[1].dependencies, [AttackManaId(0)].into());
    assert!(choices.iter().all(|choice| choice.reversible));
    for invalid in [vec![0], vec![3], vec![2, 2]] {
        illegal(
            &three,
            A,
            GameCommand::Reply {
                decision: three.pending_decision().unwrap().id(),
                choice: reverse(&invalid),
            },
        );
    }
    for (choice, kept, c, g) in [
        (DecisionReply::RedeclareKeepingMana, true, 3, 1),
        (DecisionReply::RedeclareAttackers, false, 0, 0),
    ] {
        let endpoint = reply(&three, choice);
        assert_eq!(endpoint.state().execution().mana(A, Colourless), c.into());
        assert_eq!(endpoint.state().execution().mana(A, Green), g.into());
        assert!(endpoint.state().execution().revision() > three.execution().revision());
        assert!(endpoint.rules_journal().records.iter().any(|r| matches!(r,
            RulesRecord::AttackDeclarationReversed {kept_mana_abilities, ..} if *kept_mana_abilities == kept)));
    }
    for (reversed, colours, tapped) in [
        (vec![0, 1], (2, 0), [false, false, true]),
        (vec![2], (1, 1), [true, true, false]),
        (vec![1, 2], (2, 0), [true, false, false]),
        (vec![0, 1, 2], (0, 0), [false, false, false]),
    ] {
        let rolled = reply(&three, reverse(&reversed));
        assert!(rolled.state().execution().revision() > three.execution().revision());
        assert_eq!(physical(rolled.state(), 1).tapped(), Some(false));
        for (n, expected) in [3, 4, 5].into_iter().zip(tapped) {
            assert_eq!(physical(rolled.state(), n).tapped(), Some(expected));
        }
        assert_eq!(
            rolled.state().execution().mana(A, Colourless),
            colours.0.into()
        );
        assert_eq!(rolled.state().execution().mana(A, Green), colours.1.into());
        assert!(rolled.state().attack_mana_choices().next().is_none());
        assert!(rolled.rules_journal().records.iter().any(|r| matches!(r, RulesRecord::AttackDeclarationReversed{reversed_mana,..} if *reversed_mana==reversed.iter().copied().map(AttackManaId).collect())));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_baird_reverses_a_creature_sacrifice_without_recomputing_retained_variable_mana() {
    use ManaType::*;
    let base = payment(
        vec![
            source(
                3,
                ManaOutput::Fixed([(Black, 1.into())].into())
                    .sacrifice(spellwake_engine::mana::ManaSacrificeCost::ControlledCreature),
            ),
            source(
                4,
                ManaOutput::Dynamic(
                    spellwake_engine::mana::DynamicManaYield::GreatestCreaturePower,
                )
                .paid(ManaCost::generic(1.into())),
            ),
            creature(5, A, 7, 7, &[]),
        ],
        &[(Green, 1)],
    );
    let victim = physical(&base, 5).id();
    let sac = command(
        &base,
        A,
        GameCommand::ActivateManaWithCostObjects {
            source: physical(&base, 3).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            objects: vec![victim],
        },
    )
    .into_state();
    assert_eq!(physical(&sac, 5).zone(), ObjectZone::Graveyard(A));
    let paid = command(
        &sac,
        A,
        GameCommand::ActivateManaWithRestrictedPayment {
            source: physical(&sac, 4).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            payment: spend(&[(Green, 1)]),
            restricted_mana: vec![],
            bundle: Some([(Blue, 2.into())].into()),
        },
    )
    .into_state();
    assert_eq!(paid.execution().mana(A, Blue), 2.into());
    let rollback = reply(&paid, reverse(&[0])).into_state();
    assert_eq!(physical(&rollback, 5).id(), victim);
    assert_eq!(physical(&rollback, 5).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&rollback, 1).tapped(), Some(false));
    assert_eq!(physical(&rollback, 4).tapped(), Some(true));
    assert_eq!(rollback.execution().mana(A, Black), 0.into());
    assert_eq!(rollback.execution().mana(A, Green), 0.into());
    assert_eq!(rollback.execution().mana(A, Blue), 2.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_baird_preserves_triggered_mana_choices_and_their_downstream_dependencies() {
    use ManaType::*;
    let mut filter = source(
        4,
        ManaOutput::Fixed([(Red, 1.into())].into()).paid(ManaCost::generic(2.into())),
    );
    // A nonland filter must not trigger Mirari's Wake when it is tapped.
    filter.program.card_types = [CardType::Artifact].into();
    let start = payment(
        vec![
            source(
                3,
                ManaOutput::Fixed([(Blue, 1.into()), (Green, 1.into())].into()),
            ),
            filter,
            named("Mirari's Wake", 5, A, battlefield(A)),
            named("Sol Ring", 6, A, battlefield(A)),
        ],
        &[],
    );
    let choice = tap(&start, 3);
    let RuleFrame::ManaColor(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("triggered mana choice");
    };
    assert_eq!(frame.source(), physical(&choice, 5).id());
    assert_eq!(frame.colors(), &[Blue, Green].into());
    let funded = reply(&choice, DecisionReply::ManaColor(Blue)).into_state();
    assert_eq!(funded.execution().mana(A, Blue), 2.into());
    let filtered = command(
        &funded,
        A,
        GameCommand::ActivateManaWithPayment {
            source: physical(&funded, 4).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            payment: spend(&[(Blue, 2)]),
        },
    )
    .into_state();
    let all = tap(&filtered, 6);
    let rows: Vec<_> = all.attack_mana_choices().collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].dependencies, [AttackManaId(0)].into());
    illegal(
        &all,
        A,
        GameCommand::Reply {
            decision: all.pending_decision().unwrap().id(),
            choice: reverse(&[0]),
        },
    );
    let retained = reply(&all, reverse(&[2])).into_state();
    assert_eq!(retained.execution().mana(A, Blue), 0.into());
    assert_eq!(retained.execution().mana(A, Green), 1.into());
    assert_eq!(retained.execution().mana(A, Red), 1.into());
    assert_eq!(retained.execution().mana(A, Colourless), 0.into());
    let producer_only = reply(&all, reverse(&[1, 2])).into_state();
    assert_eq!(producer_only.execution().mana(A, Blue), 2.into());
    assert_eq!(producer_only.execution().mana(A, Green), 1.into());
    assert_eq!(producer_only.execution().mana(A, Red), 0.into());
}
