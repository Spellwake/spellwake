#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::{
    attachment::EnchantRestriction,
    characteristics::{
        AbilityChange, KeywordAbility, NumericExpression, NumericPair, PermanentSelector,
        StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
    },
};
fn boon(mut program: Program, amount: i64) -> Program {
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(2),
        selector: PermanentSelector {
            attached_to_source: true,
            ..Default::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(amount.into()),
            toughness: NumericExpression::Literal(0.into()),
        })]),
    });
    program
}
fn added(id: u64, owner: SeatId, program: Program, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        program,
        destination,
    }
}
fn aura(restriction: EnchantRestriction) -> Program {
    boon(Program::aura(Some(ManaCost::free()), restriction), 2)
}
fn equip(game: &GameState, equipment: u64, creature: u64) -> GameCommand {
    GameCommand::ActivateAbility {
        source: object(game, equipment),
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(object(game, creature)),
        payment: None,
    }
}
fn power(game: &GameState, card: u64) -> RulesInteger {
    game.execution()
        .power_toughness(object(game, card))
        .unwrap()
        .unwrap()
        .derived
        .power
}
#[test]
fn aura_targets_on_the_stack_enters_attached_and_dies_after_its_host_leaves() {
    let mut cards = seeds();
    cards.push(added(
        100,
        A,
        aura(EnchantRestriction::Creature),
        ObjectDestination::Hand,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    reject(
        &game,
        A,
        untargeted(&game, 100, None),
        EffectError::TargetRequired,
    );
    let cast = targeted(&game, A, 100, TargetRef::Object(object(&game, 90)));
    let entered = resolve(cast.state()).into_state();
    let host = object(&entered, 90);
    let enchantment = object(&entered, 100);
    assert_eq!(
        entered.execution().attached_to(enchantment),
        Some(TargetRef::Object(host))
    );
    assert_eq!(power(&entered, 90), 4.into());
    let bounce = targeted(&entered, A, 13, TargetRef::Object(host));
    let result = resolve(bounce.state()).into_state();
    assert_eq!(physical(&result, 90).zone(), ObjectZone::Hand(B));
    assert_eq!(physical(&result, 100).zone(), ObjectZone::Graveyard(A));
    assert_eq!(result.execution().attached_to(enchantment), None);
}
#[test]
fn aura_rechecks_target_and_does_not_enter_when_the_target_is_gone() {
    let mut cards = seeds();
    cards.push(added(
        100,
        A,
        aura(EnchantRestriction::Creature),
        ObjectDestination::Hand,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = targeted(&game, A, 100, TargetRef::Object(object(&game, 90))).into_state();
    let response = targeted(&cast, A, 13, TargetRef::Object(object(&cast, 90)));
    let moved = resolve(response.state());
    let done = resolve(moved.state());
    assert!(done.permanent_entry().is_none());
    assert_eq!(physical(done.state(), 100).zone(), ObjectZone::Graveyard(A));
}
#[test]
fn equip_is_a_sorcery_speed_targeted_stack_ability_and_reassignment_moves_the_bonus() {
    let mut cards = seeds();
    cards.push(added(
        100,
        A,
        boon(
            Program::equipment(Some(ManaCost::free()), ManaCost::one(ManaType::Green)),
            2,
        ),
        ObjectDestination::Hand,
    ));
    for id in [101, 102] {
        cards.push(creature(
            id,
            A,
            ObjectDestination::Battlefield { controller: A },
            None,
            3,
        ));
    }
    let game = setup(Some(Step::PrecombatMain), cards);
    let equipment = cast(&game, A, 100, None);
    let game = resolve(equipment.state()).into_state();
    let item = object(&game, 100);
    assert_eq!(game.execution().attached_to(item), None);
    reject(&game, A, equip(&game, 100, 90), EffectError::IllegalTarget);
    let start = accept(&game, A, equip(&game, 100, 101));
    assert!(start.ability_activation().is_some());
    reject(
        start.state(),
        A,
        equip(start.state(), 100, 102),
        EffectError::Timing(TimingError::StackNotEmpty),
    );
    let equipped = resolve(start.state()).into_state();
    assert_eq!(power(&equipped, 101), 4.into());
    let recast = accept(&equipped, A, equip(&equipped, 100, 102));
    let moved = resolve(recast.state()).into_state();
    assert_eq!(power(&moved, 101), 2.into());
    assert_eq!(power(&moved, 102), 4.into());
    let bounce = targeted(&moved, A, 13, TargetRef::Object(object(&moved, 102)));
    let result = resolve(bounce.state()).into_state();
    assert_eq!(physical(&result, 100).zone(), ObjectZone::Battlefield);
    assert_eq!(result.execution().attached_to(item), None);
}
#[test]
fn shroud_gained_after_attachment_keeps_the_aura_and_opponent_creatures_fail_own_enchant() {
    let mut cards = seeds();
    let mut aura = Program::aura(
        Some(ManaCost::free()),
        EnchantRestriction::CreatureYouControl,
    );
    aura.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(2),
        selector: PermanentSelector {
            attached_to_source: true,
            ..Default::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [KeywordAbility::Shroud],
        ))]),
    });
    cards.push(added(100, A, aura, ObjectDestination::Hand));
    cards.push(creature(
        101,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        3,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let target = |id| GameCommand::Cast {
        spell: object(&game, 100),
        target: TargetRef::Object(object(&game, id)),
    };
    reject(&game, A, target(90), EffectError::IllegalTarget);
    let cast = accept(&game, A, target(101));
    let done = resolve(cast.state()).into_state();
    assert_eq!(
        done.execution().attached_to(object(&done, 100)),
        Some(TargetRef::Object(object(&done, 101)))
    );
    reject(
        &done,
        A,
        GameCommand::Cast {
            spell: object(&done, 13),
            target: TargetRef::Object(object(&done, 101)),
        },
        EffectError::IllegalTarget,
    );
}

#[test]
fn gaining_protection_during_resolution_removes_only_attachments_of_the_matching_color() {
    use spellwake_engine::characteristics::Color;
    let mut cards = seeds();
    cards.push(creature(
        101,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        4,
    ));
    let mut red_aura = aura(EnchantRestriction::Creature);
    red_aura.color_indicator = Some([Color::Red].into());
    let mut red_equipment = Program::equipment(Some(ManaCost::free()), ManaCost::free());
    red_equipment.color_indicator = Some([Color::Red].into());
    let mut protection = Program::aura(Some(ManaCost::free()), EnchantRestriction::Creature);
    protection.color_indicator = Some([Color::White].into());
    protection.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            attached_to_source: true,
            ..Default::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [KeywordAbility::ProtectionFromRed],
        ))]),
    });
    for (id, program) in [(100, red_aura), (102, red_equipment), (103, protection)] {
        cards.push(added(id, A, program, ObjectDestination::Hand));
    }
    let game = setup(Some(Step::PrecombatMain), cards);
    let host = TargetRef::Object(object(&game, 101));
    let first = targeted(&game, A, 100, host);
    let aura_state = resolve(first.state());
    let item = cast(aura_state.state(), A, 102, None);
    let equipped_source = resolve(item.state());
    let equipped = accept(
        equipped_source.state(),
        A,
        equip(equipped_source.state(), 102, 101),
    );
    let together = resolve(equipped.state());
    for id in [100, 102] {
        assert_eq!(
            together
                .state()
                .execution()
                .attached_to(object(together.state(), id)),
            Some(host)
        );
    }
    let grant = targeted(together.state(), A, 103, host);
    let protected = resolve(grant.state());
    assert_eq!(
        physical(protected.state(), 100).zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        physical(protected.state(), 102).zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        protected
            .state()
            .execution()
            .attached_to(object(protected.state(), 102)),
        None
    );
    assert_eq!(
        protected
            .state()
            .execution()
            .attached_to(object(protected.state(), 103)),
        Some(host)
    );
}
