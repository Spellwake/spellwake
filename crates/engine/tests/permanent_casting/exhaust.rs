use super::*;
use spellwake_engine::activated::ActivationRestriction;

#[test]
fn each_exhaust_ability_can_be_used_once_per_incarnation_even_before_the_first_resolves() {
    let mut creature =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    for (id, n) in [(1, 1), (2, 2)] {
        let id = ActivatedAbilityId(id);
        creature.activated.push(ActivatedAbility {
            id,
            source_cost: SourceCost::None,
            mana_cost: ManaCost::free(),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PutCountersOnSource { amount: n.into() }],
            ),
        });
        creature
            .activation_restrictions
            .insert(id, ActivationRestriction::Exhaust);
    }
    let mut cards = seeds();
    cards.push(SeedCard {
        card: CardId(100),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: creature,
    });
    let game = setup(Some(Step::PrecombatMain), cards);
    let old = object(&game, 100);
    let activate = |source, n| GameCommand::ActivateUntargetedAbility {
        source,
        ability: ActivatedAbilityId(n),
        payment: None,
    };
    let first = accept(&game, A, activate(old, 1));
    assert!(
        first
            .state()
            .execution()
            .ability_exhausted(old, ActivatedAbilityId(1))
    );
    assert!(
        !first
            .state()
            .execution()
            .ability_exhausted(old, ActivatedAbilityId(2))
    );
    reject(
        first.state(),
        A,
        activate(old, 1),
        EffectError::AlreadyExhausted,
    );
    let second = accept(first.state(), A, activate(old, 2));
    let game = resolve(second.state()).into_state();
    assert_eq!(
        game.execution()
            .power_toughness_counters(old)
            .unwrap()
            .plus_one,
        2.into()
    );
    let game = resolve(&game).into_state();
    assert_eq!(
        game.execution()
            .power_toughness_counters(old)
            .unwrap()
            .plus_one,
        3.into()
    );
    reject(&game, A, activate(old, 2), EffectError::AlreadyExhausted);
    let bounce = targeted(&game, A, 13, TargetRef::Object(old));
    let game = resolve(bounce.state()).into_state();
    let recast = cast(&game, A, 100, None);
    let game = resolve(recast.state()).into_state();
    let new = object(&game, 100);
    assert_ne!(old, new);
    assert!(
        !game
            .execution()
            .ability_exhausted(new, ActivatedAbilityId(1))
    );
    let activation = accept(&game, A, activate(new, 1));
    let game = resolve(activation.state()).into_state();
    assert_eq!(
        game.execution()
            .power_toughness_counters(new)
            .unwrap()
            .plus_one,
        1.into()
    );
}
