//! The activation enters the stack only after its actual price is paid.
use super::*;
use spellwake_engine::activated::ActivationRestriction;

#[test]
fn power_up_uses_entry_before_costs_and_resets_after_the_permanent_leaves() {
    let printed = ManaCost {
        generic: 2.into(),
        required: [(ManaType::Blue, 1.into()), (ManaType::Red, 1.into())].into(),
    };
    let permanent = ManaCost {
        generic: 1.into(),
        required: [(ManaType::Blue, 1.into()), (ManaType::Colourless, 1.into())].into(),
    };
    let mut source = creature(100, A, ObjectDestination::Hand, Some(permanent), 3);
    for n in [1, 2] {
        let id = ActivatedAbilityId(n);
        source.program.activated.push(ActivatedAbility {
            id,
            mana_cost: printed.clone(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PutCountersOnSource { amount: 1.into() }],
            ),
        });
        source
            .program
            .activation_restrictions
            .insert(id, ActivationRestriction::PowerUp);
    }
    let mut seeded = source.clone();
    seeded.card = CardId(101);
    seeded.destination = ObjectDestination::Battlefield { controller: A };
    let mut cards = seeds()
        .into_iter()
        .filter(|s| matches!(s.card, CardId(13 | 98 | 99)))
        .collect::<Vec<_>>();
    cards.extend([source, seeded]);
    let mut game = setup(Some(Step::PrecombatMain), cards);
    let pay_cast = || Some(payment(&[(ManaType::Blue, 1), (ManaType::Colourless, 2)]));
    let activate = |source, n, price| GameCommand::ActivateUntargetedAbility {
        source,
        ability: ActivatedAbilityId(n),
        payment: Some(price),
    };
    assert_eq!(
        game.execution()
            .power_up_mana_cost(object(&game, 101), &printed),
        printed
    );
    let casted = cast(&game, A, 100, pay_cast());
    game = resolve(casted.state()).into_state();
    let old = object(&game, 100);
    assert_eq!(
        game.execution().power_up_mana_cost(old, &printed),
        ManaCost::one(ManaType::Red)
    );
    assert!(
        game.advance(input(
            &game,
            A,
            activate(old, 1, payment(&[(ManaType::Blue, 1)]))
        ))
        .is_err()
    );
    assert!(
        !game
            .execution()
            .ability_exhausted(old, ActivatedAbilityId(1))
    );
    let paid = accept(&game, A, activate(old, 1, payment(&[(ManaType::Red, 1)])));
    assert_eq!(
        paid.mana_payment().unwrap().cost,
        ManaCost::one(ManaType::Red)
    );
    assert!(
        !paid
            .state()
            .execution()
            .ability_exhausted(old, ActivatedAbilityId(2))
    );
    reject(
        paid.state(),
        A,
        activate(old, 1, payment(&[(ManaType::Red, 1)])),
        EffectError::AlreadyExhausted,
    );
    game = resolve(paid.state()).into_state();
    assert_eq!(
        game.execution()
            .power_toughness_counters(old)
            .unwrap()
            .plus_one,
        1.into()
    );
    let bounced = targeted(&game, A, 13, TargetRef::Object(old));
    game = resolve(bounced.state()).into_state();
    let casted = cast(&game, A, 100, pay_cast());
    game = resolve(casted.state()).into_state();
    let new = object(&game, 100);
    assert_ne!(new, old);
    assert!(
        !game
            .execution()
            .ability_exhausted(new, ActivatedAbilityId(1))
    );
    for _ in 0..100 {
        if game.execution().window().active_player() == B {
            break;
        }
        if let Some(p) = game.pending_decision() {
            let RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Attackers(_)) =
                p.frame()
            else {
                panic!("unexpected {:?}", p.frame())
            };
            game = accept(
                &game,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice: DecisionReply::DeclareAttackers(vec![]),
                },
            )
            .into_state();
        } else {
            game = pass(&game).into_state();
        }
    }
    assert_eq!(game.execution().window().active_player(), B);
    assert_eq!(game.execution().power_up_mana_cost(new, &printed), printed);
}
