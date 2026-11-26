//! Multitarget damage, bolster, goad, and attacking-player payment.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_acidic_slime_destroys_artifact_enchantment_or_land_on_entry_and_kills_a_larger_blocker_with_deathtouch()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Green, 2)];
    let cards = vec![
        named("Acidic Slime", 1, A, ObjectDestination::Hand),
        named("Sol Ring", 2, B, battlefield(B)),
        seed(3, B, battlefield(B), Program::enchantment(None)),
        named("Island", 4, B, battlefield(B)),
        creature(5, B, 4, 8, &[]),
    ];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entry = cast_card(&initial, 1, &price);
    invalid(&entry, A, DecisionReply::TriggerTarget(marker(&entry, 5)));
    for n in [2, 3, 4] {
        let chosen = reply(&entry, DecisionReply::TriggerTarget(marker(&entry, n))).into_state();
        let done = round(chosen).into_state();
        assert_eq!(physical(&done, n).zone(), ObjectZone::Graveyard(B));
        assert!(sheet(&done, physical(&done, 1).id()).has_keyword(KeywordAbility::Deathtouch));
        if n == 2 {
            let combat = combat_on(done, 3);
            let swing = reply(&combat, attacks(&combat, &[(1, B, None)])).into_state();
            let block = blockers(swing);
            let spell = reply(&block, blocks(&block, &[(5, 1)])).into_state();
            let after = postcombat(spell);
            assert_eq!(physical(&after, 1).zone(), ObjectZone::Graveyard(A));
            assert_eq!(physical(&after, 5).zone(), ObjectZone::Graveyard(B));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_drakuseth_requires_one_distinct_primary_target_and_allows_zero_one_or_two_secondary_targets()
{
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 3)];
    let cards = vec![
        named("Drakuseth, Maw of Flames", 1, A, ObjectDestination::Hand),
        creature(2, B, 1, 5, &[]),
        named("Jace Beleren", 3, A, ObjectDestination::Hand),
        named("Sol Ring", 4, B, battlefield(B)),
        super::super::super::cmm_gameplay::remove_spell(5, A),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 5), (Red, 3), (Blue, 2)],
        Step::PrecombatMain,
    );
    let walker = cast_card(&initial, 3, &[(Colourless, 1), (Blue, 2)]);
    let field = cast_card(&walker, 1, &price);
    assert!(sheet(&field, physical(&field, 1).id()).has_keyword(KeywordAbility::Flying));
    let combat = combat_on(field, 3);
    let target = reply(&combat, attacks(&combat, &[(1, B, None)])).into_state();
    let first = TargetRef::Player(B);
    let victim = marker(&target, 2);
    let jace = marker(&target, 3);
    let reply_to = |choices| DecisionReply::TriggerPermanentTargets(choices);
    for illegal in [
        vec![],
        vec![first, first],
        vec![first, victim, victim],
        vec![marker(&target, 4)],
    ] {
        invalid(&target, A, reply_to(illegal));
    }
    for second in [vec![], vec![victim], vec![victim, jace]] {
        let mut selected = vec![first];
        selected.extend(&second);
        let stack = reply(&target, reply_to(selected)).into_state();
        if second.len() == 2 {
            let gone = round(targeted(&stack, A, 5, victim).into_state()).into_state();
            let rest = round(gone).into_state();
            assert_eq!(rest.execution().life_total(B), Some(&16.into()));
            assert_eq!(physical(&rest, 3).zone(), ObjectZone::Graveyard(A));
        }
        let hit = round(stack);
        assert_eq!(hit.damage_journal().applied.len(), 1 + second.len());
        assert!(
            hit.damage_journal()
                .applied
                .iter()
                .all(|r| r.source.id() == DamageSourceId::Object(physical(hit.state(), 1).id()))
        );
        let done = hit.into_state();
        assert_eq!(done.execution().life_total(B), Some(&16.into()));
        assert_eq!(
            done.execution()
                .battlefield_marked_damage(physical(&done, 2).id()),
            Some((if second.is_empty() { 0 } else { 3 }).into())
        );
        assert_eq!(
            physical(&done, 3).zone(),
            if second.len() == 2 {
                ObjectZone::Graveyard(A)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_disrupt_decorum_goads_the_existing_creatures_of_every_opponent_until_the_casters_next_turn()
{
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 2)];
    let mut cards = vec![
        named("Disrupt Decorum", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 3, &[KeywordAbility::Haste]),
        creature(3, B, 1, 3, &[]),
        creature(4, C, 1, 2, &[]),
        seed(
            5,
            B,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::free()),
                1.into(),
                1.into(),
                Default::default(),
            ),
        ),
    ];
    for (i, owner) in [(10, A), (11, B), (12, C)] {
        cards.push(named("Island", i, owner, ObjectDestination::LibraryBottom));
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let done = cast_card(&initial, 1, &price);
    let own = combat_on(done, 1);
    let skip = reply(&own, attacks(&own, &[])).into_state();
    let b = next_main(skip, 2);
    let new = round(
        command(
            &b,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&b, 5).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    let turn = combat_on(new, 2);
    invalid(&turn, B, attacks(&turn, &[]));
    invalid(&turn, B, attacks(&turn, &[(3, A, None)]));
    let attack = reply(&turn, attacks(&turn, &[(3, C, None)])).into_state();
    let other = combat_on(attack, 3);
    invalid(&other, C, attacks(&other, &[]));
    invalid(&other, C, attacks(&other, &[(4, A, None)]));
    let swings = reply(&other, attacks(&other, &[(4, B, None)])).into_state();
    let expired = combat_on(swings, 5);
    let _ = reply(&expired, attacks(&expired, &[]));
    let _ = reply(&expired, attacks(&expired, &[(3, A, None)]));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_elite_scaleguard_bolsters_a_least_toughness_creature_and_taps_only_a_defenders_creature_when_a_countered_attacker_attacks()
 {
    use ManaType::*;
    let price = [(Colourless, 4), (White, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Elite Scaleguard", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 1, &[KeywordAbility::Haste]),
            creature(3, A, 1, 1, &[KeywordAbility::Haste]),
            creature(4, B, 2, 1, &[]),
            creature(5, C, 1, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entry = cast_card(&initial, 1, &price);
    let choice = round(entry).into_state();
    for n in [1, 4, 5] {
        invalid(
            &choice,
            A,
            DecisionReply::SelectResolution(vec![physical(&choice, n).id()]),
        );
    }
    for host in [2, 3] {
        let other = if host == 2 { 3 } else { 2 };
        let boosted = reply(
            &choice,
            DecisionReply::SelectResolution(vec![physical(&choice, host).id()]),
        )
        .into_state();
        assert_eq!(
            sheet(&boosted, physical(&boosted, host).id()).plus_one_counters,
            2.into()
        );
        let step = combat_on(boosted, 1);
        let plain = reply(&step, attacks(&step, &[(other, B, None)])).into_state();
        assert!(plain.execution().window().entries().is_empty());
        let target = reply(&step, attacks(&step, &[(host, B, None)])).into_state();
        invalid(
            &target,
            A,
            DecisionReply::TriggerPermanentTargets(vec![marker(&target, 5)]),
        );
        let selected = reply(
            &target,
            DecisionReply::TriggerPermanentTargets(vec![marker(&target, 4)]),
        )
        .into_state();
        let tapped = round(selected).into_state();
        assert_eq!(physical(&tapped, 4).tapped(), Some(true));
        assert_eq!(physical(&tapped, 5).tapped(), Some(false));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_kazuul_offers_the_attacking_controller_one_payment_per_attacker_at_it_or_its_planeswalker() {
    use ManaType::*;
    use spellwake_engine::effects::ResolutionPaymentReply;
    let price = [(Colourless, 3), (Red, 2)];
    let blue = [(Colourless, 1), (Blue, 2)];
    let mut land = super::super::loyalty::mana_source(6, &[(Colourless, 3)]);
    land.owner = B;
    land.destination = battlefield(B);
    let cards = vec![
        named(
            "Kazuul, Tyrant of the Cliffs",
            1,
            A,
            ObjectDestination::Hand,
        ),
        creature(2, B, 1, 3, &[]),
        creature(3, B, 2, 3, &[]),
        creature(4, B, 1, 1, &[]),
        named("Jace Beleren", 5, A, ObjectDestination::Hand),
        land,
    ];
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 4), (Red, 2), (Blue, 2)],
        Step::PrecombatMain,
    );
    let jace = cast_card(&initial, 5, &blue);
    let field = cast_card(&jace, 1, &price);
    let step = combat_on(field, 2);
    let third = reply(&step, attacks(&step, &[(4, C, None)])).into_state();
    assert!(third.execution().window().entries().is_empty());
    let response = reply(
        &step,
        attacks(&step, &[(2, A, None), (3, A, Some(5)), (4, C, None)]),
    )
    .into_state();
    let ordered = trigger_order(response);
    assert_eq!(ordered.execution().window().entries().len(), 2);
    let funded = command(
        &ordered,
        B,
        GameCommand::ActivateMana {
            source: physical(&ordered, 6).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let first = round(funded).into_state();
    assert_eq!(first.pending_decision().unwrap().chooser(), B);
    let paid = reply(
        &first,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(Colourless, 3)]))),
    )
    .into_state();
    assert_eq!(paid.execution().mana(B, Colourless), 0.into());
    assert!(
        !paid
            .execution()
            .window()
            .cards()
            .iter()
            .any(|c| c.is_token())
    );
    let second = round(paid).into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), B);
    let made = reply(
        &second,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    let ogre = tokens(&made, A, 1)[0];
    let view = sheet(made.state(), ogre);
    assert_eq!(view.current_colors, [Color::Red].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Ogre").unwrap()].into()
    );
    stats(made.state(), ogre, 3, 3);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_arachnogenesis_counts_only_creatures_attacking_the_caster_and_prevents_only_nonspider_combat_damage_for_the_turn()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Green, 1)];
    let blue = [(Colourless, 1), (Blue, 2)];
    let mut spider = creature(2, B, 3, 3, &[]);
    spider
        .program
        .creature_subtypes
        .insert(current_catalogue().1.resolve_label("Spider").unwrap());
    let cards = vec![
        named("Arachnogenesis", 1, A, ObjectDestination::Hand),
        spider,
        creature(3, B, 3, 3, &[]),
        creature(4, B, 3, 2, &[]),
        creature(5, B, 2, 2, &[]),
        named("Jace Beleren", 6, A, ObjectDestination::Hand),
        super::super::loyalty::mana_source(7, &price),
    ];
    let initial = game(vec![A, B, C], cards, &blue, Step::PrecombatMain);
    let jace = cast_card(&initial, 6, &blue);
    let attacks_step = combat_on(jace, 2);
    let declared = reply(
        &attacks_step,
        attacks(
            &attacks_step,
            &[(2, A, None), (3, A, None), (4, A, Some(6)), (5, C, None)],
        ),
    )
    .into_state();
    let priority = pass(&pass(&declared).into_state()).into_state();
    let funded = command(
        &priority,
        A,
        GameCommand::ActivateMana {
            source: physical(&priority, 7).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let made = super::super::equipment::order_tokens(round(
        cast_untargeted(&funded, 1, &price).into_state(),
    ));
    let spiders = tokens(&made, A, 2);
    for id in &spiders {
        let view = sheet(made.state(), *id);
        assert_eq!(view.current_colors, [Color::Green].into());
        assert_eq!(
            view.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Spider").unwrap()].into()
        );
        stats(made.state(), *id, 1, 2);
        assert!(view.has_keyword(KeywordAbility::Reach));
    }
    let blocks = blockers(made.into_state());
    let defend = reply(
        &blocks,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: spiders[0],
            attacker: physical(&blocks, 3).id(),
        }]),
    )
    .into_state();
    let done = postcombat(defend);
    assert_eq!(done.execution().life_total(A), Some(&17.into()));
    assert_eq!(done.execution().life_total(C), Some(&20.into()));
    assert_eq!(loyalty(&done, 6), 3);
    assert_eq!(
        done.execution()
            .battlefield_marked_damage(physical(&done, 3).id()),
        Some(1.into())
    );
    assert_eq!(
        done.execution().battlefield_marked_damage(spiders[0]),
        Some(0.into())
    );
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_captain_ripley_vance_triggers_on_exactly_its_controllers_third_spell_each_turn_and_deals_its_power_after_the_counter()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 1)];
    let mut cards = vec![
        named("Captain Ripley Vance", 1, A, ObjectDestination::Hand),
        creature(2, B, 1, 6, &[]),
    ];
    for n in 3..10 {
        cards.push(seed(
            n,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ),
        ));
    }
    for n in 20..30 {
        cards.push(named("Island", n, A, ObjectDestination::LibraryBottom));
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = cast_card(&initial, 1, &price);
    let second = cast_card(&field, 3, &[]);
    let target = cast_untargeted(&second, 4, &[]).into_state();
    let cap = physical(&target, 1).id();
    let hit = |s: &GameState, t: TargetRef, n: i64| {
        let armed = reply(s, DecisionReply::TriggerTarget(t)).into_state();
        let damage = round(armed);
        let row = &damage.damage_journal().applied[0];
        assert_eq!(row.source.id(), DamageSourceId::Object(cap));
        assert_eq!(row.amount, n.into());
        let st = damage.into_state();
        assert_eq!(sheet(&st, cap).plus_one_counters, (n - 3).into());
        round(st).into_state()
    };
    let third = hit(&target, TargetRef::Player(B), 4);
    assert_eq!(third.execution().life_total(B), Some(&16.into()));
    let fourth = cast_card(&third, 5, &[]);
    assert_eq!(sheet(&fourth, cap).plus_one_counters, 1.into());
    let mut turn = next_main(fourth, 2);
    for n in 6..=8 {
        let priority = pass(&turn).into_state();
        let spell = cast_untargeted(&priority, n, &[]).into_state();
        if n < 8 {
            assert!(spell.pending_decision().is_none());
            turn = round(spell).into_state();
        } else {
            turn = hit(&spell, marker(&spell, 2), 5);
        }
    }
    assert_eq!(
        turn.execution()
            .battlefield_marked_damage(physical(&turn, 2).id()),
        Some(5.into())
    );
}
