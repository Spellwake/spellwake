//! Devotion and other abilities of the set's enchantment creatures.
use super::*;

fn characteristics(
    state: &GameState,
    id: u64,
) -> spellwake_engine::characteristics::ObjectCharacteristics {
    state
        .execution()
        .current_characteristics(physical(state, id).id())
        .unwrap()
        .unwrap()
}

fn white_devotion() -> Vec<SeedCard> {
    ["Balan, Wandering Knight", "Puresteel Paladin"]
        .into_iter()
        .zip(2..=3)
        .map(|(name, id)| {
            named(
                name,
                id,
                A,
                ObjectDestination::Battlefield { controller: A },
            )
        })
        .collect()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heliod_god_of_the_sun_becomes_a_creature_at_five_and_creates_vigilant_clerics() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let heliod = hand("Heliod, God of the Sun");
    let ability = heliod.program.activated[0].id;
    let mut cards = white_devotion();
    cards.extend([heliod, creature(4, B, 2, 2, &[]), remove_spell(5, A)]);
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 5), (White, 3)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let god = characteristics(&entered, 1);
    assert!(god.current_types.contains(&CardType::Creature));
    assert!(god.current_types.contains(&CardType::Enchantment));
    assert!(god.has_keyword(Indestructible));
    assert!(!god.has_keyword(Vigilance));
    assert!(characteristics(&entered, 2).has_keyword(Vigilance));
    assert!(!characteristics(&entered, 4).has_keyword(Vigilance));
    let paladin = TargetRef::Object(physical(&entered, 3).id());
    let depleted = round(targeted(&entered, A, 5, paladin).into_state()).into_state();
    assert_eq!(physical(&depleted, 1).zone(), ObjectZone::Battlefield);
    assert!(
        !characteristics(&depleted, 1)
            .current_types
            .contains(&CardType::Creature)
    );
    let made = round(
        command(
            &depleted,
            A,
            GameCommand::ActivateUntargetedAbility {
                source: physical(&depleted, 1).id(),
                ability,
                payment: Some(spend(&[(Colourless, 2), (White, 2)])),
            },
        )
        .into_state(),
    );
    let cleric = tokens(&made, A, 1)[0];
    let token = made
        .state()
        .execution()
        .current_characteristics(cleric)
        .unwrap()
        .unwrap();
    assert_eq!(
        token.current_types,
        [CardType::Creature, CardType::Enchantment].into()
    );
    assert_eq!(token.current_colors, [Color::White].into());
    assert_eq!(
        token.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Cleric").unwrap()].into()
    );
    assert!(token.has_keyword(Vigilance));
    assert_eq!(
        token.power_toughness.unwrap().derived,
        PowerToughness {
            power: 2.into(),
            toughness: 1.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heliod_sun_crowned_gives_another_creature_lifelink_and_rewards_life_gain() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let heliod = hand("Heliod, Sun-Crowned");
    let ability = heliod.program.activated[0].id;
    let price = [(Colourless, 2), (White, 1)];
    let mut cards = white_devotion();
    cards.extend([
        heliod,
        creature(4, A, 3, 3, &[Haste]),
        named(
            "Bastion of Remembrance",
            5,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 3), (White, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert!(
        characteristics(&entered, 1)
            .current_types
            .contains(&CardType::Creature)
    );
    assert!(characteristics(&entered, 1).has_keyword(Indestructible));
    let activate = |id| GameCommand::ActivateAbility {
        source: physical(&entered, 1).id(),
        ability,
        target: TargetRef::Object(physical(&entered, id).id()),
        payment: Some(spend(&[(Colourless, 1), (White, 1)])),
    };
    illegal(&entered, A, activate(1));
    let linked = round(command(&entered, A, activate(4)).into_state()).into_state();
    assert!(characteristics(&linked, 4).has_keyword(Lifelink));
    let attacked = attack(&linked, 4, B);
    let trigger = exile_hand::until(attacked, |state| {
        matches!(
            state.pending_decision().map(|decision| decision.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    assert_eq!(trigger.execution().life_total(A), Some(&23.into()));
    assert_eq!(trigger.execution().life_total(B), Some(&17.into()));
    for id in [4, 5] {
        let target = physical(&trigger, id).id();
        let stacked = reply(
            &trigger,
            DecisionReply::TriggerTarget(TargetRef::Object(target)),
        )
        .into_state();
        let grown = round(stacked).into_state();
        assert_eq!(
            grown
                .execution()
                .counters_of_kind(target, CounterKind::PlusOne),
            1.into()
        );
        let expired = next_main(grown, 2);
        assert!(!characteristics(&expired, 4).has_keyword(Lifelink));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_purphoros_counts_devotion_damages_each_opponent_for_another_entrant_and_pumps_allies() {
    use ManaType::*;
    let purphoros = hand("Purphoros, God of the Forge");
    let ability = purphoros.program.activated[0].id;
    let price = [(Colourless, 3), (Red, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            purphoros,
            named(
                "Nesting Dragon",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Inferno Titan", 3, A, ObjectDestination::Hand),
            creature(4, A, 2, 2, &[]),
            creature(5, B, 2, 2, &[]),
        ],
        &[(Colourless, 9), (Red, 4)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert!(entered.execution().window().entries().is_empty());
    assert!(characteristics(&entered, 1).has_keyword(Indestructible));
    assert!(
        !characteristics(&entered, 1)
            .current_types
            .contains(&CardType::Creature)
    );
    let with_titan =
        round(cast_untargeted(&entered, 3, &[(Colourless, 4), (Red, 2)]).into_state()).into_state();
    let RuleFrame::Triggers(frame) = with_titan.pending_decision().unwrap().frame() else {
        panic!("order opposing damage and Titan's damage triggers");
    };
    let stacked = reply(
        &with_titan,
        DecisionReply::OrderTriggers(frame.instances().to_vec()),
    )
    .into_state();
    let aimed = reply(
        &stacked,
        DecisionReply::TriggerPermanentTargets(vec![TargetRef::Player(A)]),
    )
    .into_state();
    let resolved = round(round(aimed).into_state()).into_state();
    assert!(
        characteristics(&resolved, 1)
            .current_types
            .contains(&CardType::Creature)
    );
    assert_eq!(resolved.execution().life_total(A), Some(&17.into()));
    for opponent in [B, C] {
        assert_eq!(resolved.execution().life_total(opponent), Some(&18.into()));
    }
    let boosted = round(
        command(
            &resolved,
            A,
            GameCommand::ActivateUntargetedAbility {
                source: physical(&resolved, 1).id(),
                ability,
                payment: Some(spend(&[(Colourless, 2), (Red, 1)])),
            },
        )
        .into_state(),
    )
    .into_state();
    for (id, power) in [(1, 7), (3, 7), (4, 3), (5, 2)] {
        assert_eq!(
            characteristics(&boosted, id)
                .power_toughness
                .unwrap()
                .derived
                .power,
            power.into()
        );
    }
    let later = next_main(boosted, 2);
    assert_eq!(
        characteristics(&later, 4)
            .power_toughness
            .unwrap()
            .derived
            .power,
        2.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_anikthea_makes_black_zombie_copies_of_owned_non_aura_enchantments() {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 1), (Black, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Anikthea, Hand of Erebos"),
            named("Fervor", 2, A, ObjectDestination::Graveyard),
            named("Intangible Virtue", 3, A, ObjectDestination::Graveyard),
            named("Darksteel Mutation", 4, A, ObjectDestination::Graveyard),
            named(
                "Boon of the Spirit Realm",
                5,
                B,
                ObjectDestination::Graveyard,
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert!(characteristics(&entered, 1).has_keyword(Menace));
    let select = |state: &GameState, id| {
        DecisionReply::TriggerPermanentTargets(vec![TargetRef::Object(physical(state, id).id())])
    };
    for illegal_target in [4, 5] {
        illegal(
            &entered,
            A,
            combat_declaration(&entered, select(&entered, illegal_target)),
        );
    }
    let empty = reply(&entered, DecisionReply::TriggerPermanentTargets(vec![])).into_state();
    assert_eq!(physical(&empty, 2).zone(), ObjectZone::Graveyard(A));
    let targeted = reply(&entered, select(&entered, 2)).into_state();
    let made = round(targeted);
    assert_eq!(physical(made.state(), 2).zone(), ObjectZone::Exile);
    let prison = tokens(&made, A, 1)[0];
    let copy = |state: &GameState| {
        state
            .execution()
            .current_characteristics(prison)
            .unwrap()
            .unwrap()
    };
    let characteristics = copy(made.state());
    assert_eq!(
        characteristics
            .current_names
            .iter()
            .next()
            .unwrap()
            .as_str(),
        "Fervor"
    );
    assert_eq!(
        characteristics.current_types,
        [CardType::Creature, CardType::Enchantment].into()
    );
    assert_eq!(characteristics.current_colors, [Color::Black].into());
    assert_eq!(
        characteristics.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Zombie").unwrap()].into()
    );
    assert!(characteristics.has_keyword(Menace));
    assert!(characteristics.has_keyword(Haste));
    assert_eq!(
        characteristics.power_toughness.unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 3.into()
        }
    );
    let later = next_main(made.into_state(), 3);
    let attacking = attack(&later, 1, B);
    let targeted_on_attack = reply(&attacking, select(&attacking, 3)).into_state();
    let double = round(targeted_on_attack).into_state();
    assert_eq!(physical(&double, 3).zone(), ObjectZone::Exile);
    let strengthened = copy(&double);
    assert!(strengthened.has_keyword(Vigilance));
    assert_eq!(
        strengthened.power_toughness.unwrap().derived.power,
        4.into()
    );
}
