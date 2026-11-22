//! Named combat games for continuous grants, triggered attacks, blockers and damage.
use super::declarations::{attacks, blocks, combat_on, invalid};
use super::equipment::{cast_card, marker, sheet, stats, wield};
use super::*;
mod last_combat;
mod triggered_combat;

fn blockers(state: GameState) -> GameState {
    exile_hand::until(state, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
        )
    })
}
fn postcombat(state: GameState) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    })
}
fn next_main(state: GameState, turn: u64) -> GameState {
    exile_hand::until(state, |s| {
        s.turn().unwrap().position().number == turn
            && s.turn().unwrap().position().step == Step::PrecombatMain
    })
}
fn trigger_order(state: GameState) -> GameState {
    if let Some(p) = state.pending_decision()
        && let RuleFrame::Triggers(f) = p.frame()
    {
        return reply(&state, DecisionReply::OrderTriggers(f.instances().to_vec())).into_state();
    }
    state
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heart_piercer_bow_and_sunspear_shikari_require_attachment_to_trigger_damage_or_grant_first_strike_and_lifelink()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (White, 1)];
    let cards = vec![
        named("Sunspear Shikari", 1, A, ObjectDestination::Hand),
        named("Heart-Piercer Bow", 2, A, ObjectDestination::Hand),
        creature(3, A, 1, 1, &[KeywordAbility::Haste]),
        creature(4, B, 2, 2, &[]),
        creature(5, C, 1, 1, &[]),
    ];
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 5), (White, 1)],
        Step::PrecombatMain,
    );
    let source = cast_card(&initial, 1, &price);
    let cat = physical(&source, 1).id();
    for k in [KeywordAbility::FirstStrike, KeywordAbility::Lifelink] {
        assert!(!sheet(&source, cat).has_keyword(k));
    }
    let entry = cast_card(&source, 2, &[(Colourless, 2)]);
    let bow = physical(&entry, 2).id();
    let first = wield(&entry, bow, 1, 1);
    for k in [KeywordAbility::FirstStrike, KeywordAbility::Lifelink] {
        assert!(sheet(&first, cat).has_keyword(k));
    }
    let transferred = wield(&first, bow, 3, 1);
    for k in [KeywordAbility::FirstStrike, KeywordAbility::Lifelink] {
        assert!(!sheet(&transferred, cat).has_keyword(k));
    }
    let combat = combat_on(first, 4);
    let other = reply(&combat, attacks(&combat, &[(3, B, None)])).into_state();
    assert!(other.execution().window().entries().is_empty() && other.pending_decision().is_none());
    let declaration = reply(&combat, attacks(&combat, &[(1, B, None)])).into_state();
    invalid(
        &declaration,
        A,
        DecisionReply::TriggerTarget(marker(&declaration, 5)),
    );
    let stack = reply(
        &declaration,
        DecisionReply::TriggerTarget(marker(&declaration, 4)),
    )
    .into_state();
    let hit = round(stack);
    assert!(
        hit.damage_journal()
            .applied
            .iter()
            .any(|h| h.source.id() == DamageSourceId::Object(bow) && h.amount == 1.into())
    );
    let block = blockers(hit.into_state());
    let duel = reply(&block, blocks(&block, &[(4, 1)])).into_state();
    let done = postcombat(duel);
    assert_eq!(done.execution().life_total(A), Some(&22.into()));
    assert_eq!(physical(&done, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&done, 1).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tetsuko_prevents_blocks_for_either_low_stat_only_on_its_controllers_creatures() {
    use ManaType::*;
    let price = [(Colourless, 1), (Blue, 1)];
    let cards = vec![
        named("Tetsuko Umezawa, Fugitive", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 4, &[KeywordAbility::Haste]),
        creature(3, A, 3, 1, &[KeywordAbility::Haste]),
        creature(4, A, 2, 2, &[KeywordAbility::Haste]),
        creature(5, B, 1, 2, &[]),
        creature(6, C, 1, 3, &[]),
    ];
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let entered = cast_card(&initial, 1, &price);
    let combat = combat_on(entered, 1);
    let declared = reply(
        &combat,
        attacks(&combat, &[(2, B, None), (3, B, None), (4, B, None)]),
    )
    .into_state();
    let block = blockers(declared);
    for n in [2, 3] {
        invalid(&block, B, blocks(&block, &[(5, n)]));
    }
    let legal = reply(&block, blocks(&block, &[(5, 4)])).into_state();
    let done = postcombat(legal);
    assert_eq!(done.execution().life_total(B), Some(&16.into()));
    let turn = combat_on(done, 3);
    let away = reply(&turn, attacks(&turn, &[(6, A, None)])).into_state();
    let block = blockers(away);
    let valid = reply(&block, blocks(&block, &[(1, 6)])).into_state();
    assert!(valid.pending_decision().is_none());
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_stonehoof_chieftain_grants_other_controlled_attackers_trample_and_indestructible_only_until_end_of_turn()
 {
    use ManaType::*;
    let price = [(Colourless, 7), (Green, 1)];
    let cards = vec![
        named("Stonehoof Chieftain", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 3, &[KeywordAbility::Haste]),
        creature(3, A, 1, 1, &[]),
        creature(4, B, 1, 2, &[]),
        super::super::cmm_gameplay::remove_spell(5, A),
    ];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entered = cast_card(&initial, 1, &price);
    for k in [KeywordAbility::Trample, KeywordAbility::Indestructible] {
        assert!(sheet(&entered, physical(&entered, 1).id()).has_keyword(k));
    }
    let step = combat_on(entered, 1);
    let att = reply(&step, attacks(&step, &[(2, B, None)])).into_state();
    let boosted = round(att).into_state();
    for n in [2, 3, 4] {
        for k in [KeywordAbility::Trample, KeywordAbility::Indestructible] {
            assert_eq!(
                sheet(&boosted, physical(&boosted, n).id()).has_keyword(k),
                n == 2
            );
        }
    }
    let safe = round(targeted(&boosted, A, 5, marker(&boosted, 2)).into_state()).into_state();
    assert_eq!(physical(&safe, 2).zone(), ObjectZone::Battlefield);
    let next = next_main(safe, 2);
    for k in [KeywordAbility::Trample, KeywordAbility::Indestructible] {
        assert!(!sheet(&next, physical(&next, 2).id()).has_keyword(k));
    }
    let opposing = combat_on(next, 2);
    let enemy = reply(&opposing, attacks(&opposing, &[(4, A, None)])).into_state();
    assert!(enemy.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_fog_bank_can_block_flyers_and_prevents_both_directions_of_combat_damage_but_allows_noncombat_damage()
 {
    use ManaType::*;
    let cards = vec![
        named("Fog Bank", 1, A, ObjectDestination::Hand),
        named("Bonesplitter", 2, A, battlefield(A)),
        creature(3, B, 3, 3, &[KeywordAbility::Flying]),
        creature(4, A, 1, 2, &[KeywordAbility::Haste]),
        seed(
            5,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleCreature,
                Instruction::Damage { amount: 2.into() },
            ),
        ),
    ];
    let initial = game(
        vec![A, B],
        cards,
        &[(Colourless, 2), (Blue, 1)],
        Step::PrecombatMain,
    );
    let ready = cast_card(&initial, 1, &[(Colourless, 1), (Blue, 1)]);
    let armed = wield(&ready, physical(&ready, 2).id(), 1, 1);
    let id = physical(&armed, 1).id();
    stats(&armed, id, 2, 2);
    for k in [KeywordAbility::Flying, KeywordAbility::Defender] {
        assert!(sheet(&armed, id).has_keyword(k));
    }
    let combat = combat_on(armed, 1);
    invalid(&combat, A, attacks(&combat, &[(1, B, None)]));
    let skipped = reply(&combat, attacks(&combat, &[])).into_state();
    let second = combat_on(skipped, 2);
    let swing = reply(&second, attacks(&second, &[(3, A, None)])).into_state();
    let block = blockers(swing);
    let blocked = reply(&block, blocks(&block, &[(1, 3)])).into_state();
    let after = postcombat(blocked);
    for n in [1, 3] {
        assert_eq!(
            after
                .execution()
                .battlefield_marked_damage(physical(&after, n).id()),
            Some(0.into())
        );
    }
    assert_eq!(after.execution().life_total(A), Some(&20.into()));
    let kill = round(targeted(&after, B, 5, marker(&after, 1)).into_state()).into_state();
    assert_eq!(physical(&kill, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zilortha_uses_power_only_for_lethal_damage_to_controlled_creatures_and_rechecks_original_lethality_when_it_leaves()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Red, 1), (Green, 1)];
    let cards = vec![
        named(
            "Zilortha, Strength Incarnate",
            1,
            A,
            ObjectDestination::Hand,
        ),
        creature(2, A, 7, 2, &[]),
        creature(3, A, 2, 7, &[]),
        creature(4, B, 2, 7, &[]),
        creature(5, B, 7, 2, &[]),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleCreature,
                Instruction::Damage { amount: 3.into() },
            ),
        ),
        super::super::cmm_gameplay::remove_spell(7, A),
    ];
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = cast_card(&initial, 1, &price);
    assert!(sheet(&field, physical(&field, 1).id()).has_keyword(KeywordAbility::Trample));
    for (n, survive, owner) in [
        (2, true, A),
        (3, false, A),
        (4, true, B),
        (5, false, B),
        (1, true, A),
    ] {
        let hit = round(targeted(&field, A, 6, marker(&field, n)).into_state()).into_state();
        assert_eq!(
            physical(&hit, n).zone(),
            if survive {
                ObjectZone::Battlefield
            } else {
                ObjectZone::Graveyard(owner)
            }
        );
        if n == 2 {
            let removed = round(targeted(&hit, A, 7, marker(&hit, 1)).into_state()).into_state();
            assert_eq!(physical(&removed, 2).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rafiqs_two_attacks_alone_triggers_give_a_single_attacker_exalted_and_double_strike() {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1), (White, 1), (Blue, 1)];
    let cards = vec![
        named("Rafiq of the Many", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 3, &[KeywordAbility::Haste]),
        creature(3, A, 1, 1, &[KeywordAbility::Haste]),
        creature(4, B, 2, 2, &[]),
    ];
    let before = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = cast_card(&before, 1, &price);
    let step = combat_on(field, 1);
    let group = reply(&step, attacks(&step, &[(2, B, None), (3, B, None)])).into_state();
    assert!(group.pending_decision().is_none() && group.execution().window().entries().is_empty());
    let alone = reply(&step, attacks(&step, &[(2, B, None)])).into_state();
    let ordered = trigger_order(alone);
    assert_eq!(ordered.execution().window().entries().len(), 2);
    let pair = round(round(ordered).into_state()).into_state();
    let hero = physical(&pair, 2).id();
    stats(&pair, hero, 3, 4);
    assert!(sheet(&pair, hero).has_keyword(KeywordAbility::DoubleStrike));
    let done = postcombat(pair);
    assert_eq!(done.execution().life_total(B), Some(&14.into()));
    let second = combat_on(done, 2);
    stats(&second, hero, 2, 3);
    assert!(!sheet(&second, hero).has_keyword(KeywordAbility::DoubleStrike));
    let enemy = reply(&second, attacks(&second, &[(4, A, None)])).into_state();
    assert!(enemy.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_craterhoof_counts_its_controllers_creatures_at_resolution_and_grants_them_trample_and_the_same_temporary_bonus()
 {
    use ManaType::*;
    let price = [(Colourless, 5), (Green, 3)];
    let cards = vec![
        named("Craterhoof Behemoth", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 1, &[]),
        creature(3, A, 2, 3, &[]),
        creature(4, B, 2, 3, &[]),
        super::super::cmm_gameplay::remove_spell(5, A),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::free()),
                1.into(),
                1.into(),
                Default::default(),
            ),
        ),
    ];
    let before = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = cast_card(&before, 1, &price);
    let chief = physical(&field, 1).id();
    assert!(sheet(&field, chief).has_keyword(KeywordAbility::Haste));
    let gone = round(targeted(&field, A, 5, marker(&field, 2)).into_state()).into_state();
    let less = round(gone).into_state();
    stats(&less, chief, 7, 7);
    let all = round(field).into_state();
    stats(&all, chief, 8, 8);
    stats(&all, physical(&all, 2).id(), 4, 4);
    stats(&all, physical(&all, 3).id(), 5, 6);
    stats(&all, physical(&all, 4).id(), 2, 3);
    for n in [1, 2, 3, 4] {
        assert_eq!(
            sheet(&all, physical(&all, n).id()).has_keyword(KeywordAbility::Trample),
            n != 4
        );
    }
    let new = cast_card(&all, 6, &[]);
    stats(&new, physical(&new, 6).id(), 1, 1);
    let expired = next_main(new, 2);
    stats(&expired, chief, 5, 5);
    assert!(!sheet(&expired, chief).has_keyword(KeywordAbility::Trample));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jazal_grants_attacking_creatures_the_current_attack_count_and_keeps_first_strike() {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 2)];
    let lease = [(Colourless, 3), (White, 2)];
    let subject = named("Jazal Goldmane", 1, A, ObjectDestination::Hand);
    let ability = subject.program.activated[0].id;
    let initial = game(
        vec![A, B, C],
        vec![
            subject,
            creature(2, A, 2, 3, &[KeywordAbility::Haste]),
            creature(3, A, 1, 1, &[KeywordAbility::Haste]),
            creature(4, B, 1, 4, &[]),
            super::loyalty::mana_source(5, &lease),
        ],
        &price,
        Step::PrecombatMain,
    );
    let ready = cast_card(&initial, 1, &price);
    let chief = physical(&ready, 1).id();
    assert!(sheet(&ready, chief).has_keyword(KeywordAbility::FirstStrike));
    let combat = combat_on(ready, 1);
    for attackers in [vec![(2, B, None)], vec![(2, B, None), (3, C, None)]] {
        let n = i64::try_from(attackers.len()).unwrap();
        let declared = reply(&combat, attacks(&combat, &attackers)).into_state();
        let mana = command(
            &declared,
            A,
            GameCommand::ActivateMana {
                source: physical(&declared, 5).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        let result = round(
            command(
                &mana,
                A,
                GameCommand::ActivateUntargetedAbility {
                    source: chief,
                    ability,
                    payment: Some(spend(&lease)),
                },
            )
            .into_state(),
        )
        .into_state();
        stats(&result, chief, 4, 4);
        stats(&result, physical(&result, 2).id(), 2 + n, 3 + n);
        stats(
            &result,
            physical(&result, 3).id(),
            if n == 2 { 3 } else { 1 },
            if n == 2 { 3 } else { 1 },
        );
        stats(&result, physical(&result, 4).id(), 1, 4);
        let next = next_main(result, 2);
        stats(&next, physical(&next, 2).id(), 2, 3);
        stats(&next, physical(&next, 3).id(), 1, 1);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_surrak_tests_formidable_at_combat_and_resolution_and_grants_haste_to_a_legal_owned_creature()
{
    use ManaType::*;
    let price = [(Colourless, 2), (Green, 2)];
    for buildup in [2, 3] {
        let initial = game(
            vec![A, B],
            vec![
                named("Surrak, the Hunt Caller", 1, A, ObjectDestination::Hand),
                creature(2, A, buildup, 4, &[]),
                creature(3, B, 9, 9, &[]),
                super::super::cmm_gameplay::remove_spell(4, A),
            ],
            &price,
            Step::PrecombatMain,
        );
        let field = cast_card(&initial, 1, &price);
        let begin = exile_hand::until(field, |s| {
            s.turn().unwrap().position().step == Step::BeginningCombat
        });
        if buildup == 2 {
            assert!(
                begin.pending_decision().is_none()
                    && begin.execution().window().entries().is_empty()
            );
            continue;
        }
        invalid(&begin, A, DecisionReply::TriggerTarget(marker(&begin, 3)));
        let stack = reply(&begin, DecisionReply::TriggerTarget(marker(&begin, 1))).into_state();
        let gone = round(targeted(&stack, A, 4, marker(&stack, 2)).into_state()).into_state();
        let missed = round(gone).into_state();
        let chief = physical(&missed, 1).id();
        assert!(!sheet(&missed, chief).has_keyword(KeywordAbility::Haste));
        let hit = round(stack).into_state();
        assert!(sheet(&hit, chief).has_keyword(KeywordAbility::Haste));
        let combat = combat_on(hit, 1);
        let attacked = reply(&combat, attacks(&combat, &[(1, B, None)])).into_state();
        let next = next_main(attacked, 2);
        assert!(!sheet(&next, chief).has_keyword(KeywordAbility::Haste));
        let their_combat = combat_on(next, 2);
        assert!(their_combat.execution().window().entries().is_empty());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_balefire_dragon_damages_only_the_hit_players_creatures_for_the_damage_actually_dealt() {
    use ManaType::*;
    let price = [(Colourless, 5), (Red, 2)];
    let cards = vec![
        named("Balefire Dragon", 1, A, ObjectDestination::Hand),
        creature(2, B, 3, 7, &[]),
        creature(3, B, 1, 1, &[KeywordAbility::Flying]),
        creature(4, C, 1, 2, &[]),
        creature(5, A, 1, 1, &[]),
    ];
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let field = cast_card(&initial, 1, &price);
    let dragon = physical(&field, 1).id();
    assert!(sheet(&field, dragon).has_keyword(KeywordAbility::Flying));
    let step = combat_on(field, 4);
    let swing = reply(&step, attacks(&step, &[(1, B, None)])).into_state();
    let block = blockers(swing);
    let prevent = reply(&block, blocks(&block, &[(3, 1)])).into_state();
    let after = postcombat(prevent);
    assert_eq!(after.execution().life_total(B), Some(&20.into()));
    assert_eq!(
        after
            .execution()
            .battlefield_marked_damage(physical(&after, 2).id()),
        Some(0.into())
    );
    let clear = reply(&block, blocks(&block, &[])).into_state();
    let hit = exile_hand::until(clear, |s| !s.execution().window().entries().is_empty());
    assert_eq!(hit.execution().life_total(B), Some(&14.into()));
    let fire = round(hit);
    assert_eq!(fire.damage_journal().applied.len(), 2);
    for row in &fire.damage_journal().applied {
        assert_eq!(row.source.id(), DamageSourceId::Object(dragon));
        assert_eq!(row.amount, 6.into());
    }
    let done = fire.into_state();
    assert_eq!(physical(&done, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(
        done.execution()
            .battlefield_marked_damage(physical(&done, 2).id()),
        Some(6.into())
    );
    for n in [4, 5] {
        assert_eq!(
            done.execution()
                .battlefield_marked_damage(physical(&done, n).id()),
            Some(0.into())
        );
    }
}
