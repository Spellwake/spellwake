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
fn fortify_targets_only_a_controlled_land_and_detaches_after_it_leaves() {
    let mut cards = seeds();
    cards.push(added(
        100,
        A,
        Program::fortification(Some(ManaCost::free()), ManaCost::free()),
        ObjectDestination::Hand,
    ));
    cards.push(land(
        101,
        ObjectDestination::Battlefield { controller: A },
        false,
    ));
    let mut foreign_land = land(102, ObjectDestination::Battlefield { controller: B }, false);
    foreign_land.owner = B;
    cards.push(foreign_land);
    cards.push(spell(
        103,
        A,
        Instruction::ReturnToOwnersHand,
        ManaCost::free(),
        BodyTarget::SinglePermanent,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast_item = cast(&game, A, 100, None);
    let game = resolve(cast_item.state()).into_state();
    reject(&game, A, equip(&game, 100, 90), EffectError::IllegalTarget);
    reject(&game, A, equip(&game, 100, 102), EffectError::IllegalTarget);
    let activation = accept(&game, A, equip(&game, 100, 101));
    let game = resolve(activation.state()).into_state();
    let item = object(&game, 100);
    assert_eq!(
        game.execution().attached_to(item),
        Some(TargetRef::Object(object(&game, 101)))
    );
    let bounce = targeted(&game, A, 103, TargetRef::Object(object(&game, 101)));
    let game = resolve(bounce.state()).into_state();
    assert_eq!(game.execution().attached_to(item), None);
    assert_eq!(physical(&game, 100).zone(), ObjectZone::Battlefield);
}

#[test]
fn when_the_equipment_leaves_before_its_trigger_the_token_still_enters_under_the_trigger_controllers_control()
 {
    use spellwake_engine::{
        program::{TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
        token::{TokenDefinition, TokenName},
    };
    let token = TokenDefinition::new(
        Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &test_vocabulary(),
    )
    .unwrap();
    let mut item = Program::equipment(Some(ManaCost::free()), ManaCost::free());
    item.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokenAndAttach {
                definition: Box::new(token),
            }],
        ),
    });
    let mut cards = seeds();
    cards.push(added(100, A, item, ObjectDestination::Hand));
    cards.push(spell(
        101,
        A,
        Instruction::ReturnToOwnersHand,
        ManaCost::free(),
        BodyTarget::SinglePermanent,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast_item = cast(&game, A, 100, None);
    let game = resolve(cast_item.state()).into_state();
    assert_eq!(game.execution().window().entries().len(), 1);
    let old_item = object(&game, 100);
    let bounce = targeted(&game, A, 101, TargetRef::Object(old_item));
    let game = resolve(bounce.state()).into_state();
    let game = resolve(&game).into_state();
    let tokens = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|object| object.is_token() && object.zone() == ObjectZone::Battlefield)
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].controller(), Some(A));
    assert_eq!(game.execution().attached_to(old_item), None);
    assert_eq!(physical(&game, 100).zone(), ObjectZone::Hand(A));
}

#[test]
fn reconfigure_type_effect_survives_losing_abilities_and_expires_when_the_host_leaves() {
    use spellwake_engine::characteristics::{
        AbilityRemoval, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    };
    let mut cards = seeds();
    cards.push(added(
        100,
        A,
        Program::reconfigure(
            Some(ManaCost::free()),
            2.into(),
            4.into(),
            BTreeSet::new(),
            ManaCost::free(),
        ),
        ObjectDestination::Hand,
    ));
    cards.push(creature(
        101,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        4,
    ));
    cards.push(spell(
        102,
        A,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
        ManaCost::free(),
        BodyTarget::SinglePermanent,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast_item = cast(&game, A, 100, None);
    let game = resolve(cast_item.state()).into_state();
    let source = object(&game, 100);
    reject(&game, A, equip(&game, 100, 100), EffectError::IllegalTarget);
    let activate = accept(&game, A, equip(&game, 100, 101));
    let game = resolve(activate.state()).into_state();
    let strip = targeted(&game, A, 102, TargetRef::Object(source));
    let game = resolve(strip.state()).into_state();
    let view = game
        .execution()
        .current_characteristics(source)
        .unwrap()
        .unwrap();
    assert!(!view.is_creature());
    assert!(view.abilities.activated.is_empty());
    assert_eq!(
        game.execution().attached_to(source),
        Some(TargetRef::Object(object(&game, 101)))
    );
    let bounce = targeted(&game, A, 13, TargetRef::Object(object(&game, 101)));
    let game = resolve(bounce.state()).into_state();
    let view = game
        .execution()
        .current_characteristics(source)
        .unwrap()
        .unwrap();
    assert!(view.is_creature());
    assert!(view.abilities.activated.is_empty());
    assert_eq!(game.execution().attached_to(source), None);
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

#[test]
fn bestow_uses_its_alternative_price_and_becomes_a_creature_if_target_or_attachment_fails() {
    use spellwake_engine::{
        characteristics::{ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
        mana::CastingPayment,
        program::{CardType, ConditionalCastCost},
    };
    let mut cards = seeds();
    let mut bestow = boon(
        Program::creature(
            Some(ManaCost::one(ManaType::White)),
            1.into(),
            2.into(),
            BTreeSet::new(),
        ),
        2,
    );
    bestow.card_types.insert(CardType::Enchantment);
    bestow.conditional_cost = Some(ConditionalCastCost::Bestow(ManaCost::generic(3.into())));
    cards.push(added(100, A, bestow, ObjectDestination::Hand));
    cards.push(spell(
        101,
        A,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                AbilityChange::grant_keywords([KeywordAbility::ProtectionFromWhite]),
            )]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
        ManaCost::free(),
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let who = object(&game, 90);
    let bestow = |target| GameCommand::CastWithResources {
        spell: object(&game, 100),
        target,
        payment: CastingPayment {
            use_alternative: true,
            mana: Some(payment(&[(ManaType::Colourless, 3)])),
            ..Default::default()
        },
    };
    reject(&game, A, bestow(None), EffectError::TargetRequired);
    reject(
        &game,
        A,
        bestow(Some(TargetRef::Object(object(&game, 9)))),
        EffectError::IllegalTarget,
    );
    let normal = cast(&game, A, 100, None);
    let ordinary = resolve(normal.state());
    assert!(
        !ordinary
            .state()
            .execution()
            .is_bestowed(object(ordinary.state(), 100))
    );
    assert_eq!(power(ordinary.state(), 100), 1.into());
    let cast = accept(&game, A, bestow(Some(TargetRef::Object(who))));
    assert_eq!(
        cast.mana_payment().unwrap().cost,
        ManaCost::generic(3.into())
    );
    let stack_id = object(cast.state(), 100);
    assert!(cast.state().execution().is_bestowed(stack_id));
    assert_eq!(
        cast.state()
            .execution()
            .current_characteristics(stack_id)
            .unwrap()
            .unwrap()
            .current_types,
        BTreeSet::from([CardType::Enchantment])
    );
    assert!(
        cast.state()
            .execution()
            .copiable_program(stack_id)
            .unwrap()
            .has_type(CardType::Creature)
    );
    let bounced = targeted(cast.state(), A, 13, TargetRef::Object(who));
    let before_resolution = resolve(bounced.state());
    let fallback = resolve(before_resolution.state());
    assert!(fallback.permanent_entry().is_some());
    assert_eq!(
        physical(fallback.state(), 100).zone(),
        ObjectZone::Battlefield
    );
    assert!(
        !fallback
            .state()
            .execution()
            .is_bestowed(object(fallback.state(), 100))
    );
    assert_eq!(power(fallback.state(), 100), 1.into());
    let legal = resolve(cast.state()).into_state();
    let aura = object(&legal, 100);
    assert!(legal.execution().is_bestowed(aura));
    assert_eq!(
        legal.execution().attached_to(aura),
        Some(TargetRef::Object(who))
    );
    assert_eq!(power(&legal, 90), 4.into());
    assert!(legal.execution().power_toughness(aura).unwrap().is_none());
    for command in [13, 101] {
        let response = targeted(&legal, A, command, TargetRef::Object(who));
        let detached = resolve(response.state()).into_state();
        assert_eq!(physical(&detached, 100).zone(), ObjectZone::Battlefield);
        assert_eq!(object(&detached, 100), aura);
        assert!(!detached.execution().is_bestowed(aura));
        assert!(detached.execution().attached_to(aura).is_none());
        assert_eq!(power(&detached, 100), 1.into());
    }
}
