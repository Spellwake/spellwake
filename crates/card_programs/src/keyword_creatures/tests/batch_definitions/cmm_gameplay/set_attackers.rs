//! Named attack triggers exercised through paid casts and declared combat.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_lorthos_pays_during_resolution_and_skips_the_targets_own_next_untap() {
    use ManaType::*;
    use spellwake_engine::{effects::ResolutionPaymentReply, land::BasicLandType};
    let price = [(Colourless, 5), (Blue, 3)];
    let mut originals = vec![
        hand("Lorthos, the Tidemaker"),
        named(
            "Mind Stone",
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        named(
            "Island",
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        named("Island", 4, A, ObjectDestination::LibraryBottom),
        named("Island", 5, B, ObjectDestination::LibraryBottom),
        named("Island", 6, B, ObjectDestination::LibraryBottom),
    ];
    originals.extend((10..18).map(|id| {
        named(
            "Forest",
            id,
            A,
            ObjectDestination::Battlefield { controller: A },
        )
    }));
    let initial = game(vec![A, B], originals, &price, Step::PrecombatMain);
    let summoned = cast_named(&initial, &price);
    let main = next_main(summoned, 3);
    let attacked = attack(&main, 1, B);
    let choices = [2, 3]
        .into_iter()
        .map(|id| TargetRef::Object(physical(&attacked, id).id()))
        .collect();
    let selected = reply(&attacked, DecisionReply::TriggerPermanentTargets(choices)).into_state();
    let quoting = round(selected).into_state();
    let declined = reply(
        &quoting,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    for id in [2, 3] {
        assert_eq!(physical(&declined, id).tapped(), Some(false));
    }
    let mut funded = quoting;
    for id in 10..18 {
        funded = command(
            &funded,
            A,
            GameCommand::ActivateMana {
                source: physical(&funded, id).id(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
            },
        )
        .into_state();
    }
    let paid = reply(
        &funded,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(Green, 8)]))),
    )
    .into_state();
    for id in [2, 3] {
        assert_eq!(physical(&paid, id).tapped(), Some(true));
    }
    let skipped = next_main(paid, 4);
    for id in [2, 3] {
        assert_eq!(physical(&skipped, id).tapped(), Some(true));
    }
    let resumed = next_main(skipped, 6);
    for id in [2, 3] {
        assert_eq!(physical(&resumed, id).tapped(), Some(false));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_bane_of_bala_ged_makes_the_defending_player_choose_two_permanents() {
    let price = [(ManaType::Colourless, 7)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Bane of Bala Ged"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            named(
                "Forest",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Mind Stone",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let main = next_main(cast_named(&initial, &price), 3);
    let window = round(attack(&main, 1, B)).into_state();
    assert_eq!(window.pending_decision().unwrap().chooser(), B);
    let choice = |ids: &[u64]| {
        DecisionReply::SelectResolution(ids.iter().map(|&id| physical(&window, id).id()).collect())
    };
    illegal(&window, B, combat_declaration(&window, choice(&[4])));
    illegal(&window, B, combat_declaration(&window, choice(&[2, 4])));
    illegal(&window, A, combat_declaration(&window, choice(&[4, 5])));
    let chosen = reply(&window, choice(&[4, 5])).into_state();
    assert_eq!(physical(&chosen, 4).zone(), ObjectZone::Exile);
    assert_eq!(physical(&chosen, 5).zone(), ObjectZone::Exile);
    assert_eq!(physical(&chosen, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&chosen, 3).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_pianna_buffs_the_attacking_group_and_leaves_other_creatures_unchanged() {
    use ManaType::*;
    let price = [(Colourless, 1), (White, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Pianna, Nomad Captain"),
            creature(2, A, 3, 3, &[]),
            creature(3, A, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
            named("Forest", 5, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let choices = combat_frame(&next_main(cast_named(&initial, &price), 3), true);
    let attacks = [1, 2]
        .into_iter()
        .map(|id| AttackDeclaration {
            attacker: physical(&choices, id).id(),
            defender: B,
            defending_planeswalker: None,
        })
        .collect();
    let going = reply(&choices, DecisionReply::DeclareAttackers(attacks)).into_state();
    let buffed = round(going).into_state();
    let power = |state: &GameState, id| {
        state
            .execution()
            .power_toughness(physical(state, id).id())
            .unwrap()
            .unwrap()
            .derived
            .power
    };
    for (id, expected) in [(1, 3), (2, 4), (3, 2), (4, 2)] {
        assert_eq!(power(&buffed, id), expected.into());
    }
    let cleanup_done = next_main(buffed, 4);
    for (id, expected) in [(1, 2), (2, 3)] {
        assert_eq!(power(&cleanup_done, id), expected.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tuya_bearclaw_uses_the_largest_other_controlled_creature_at_resolution() {
    use ManaType::*;
    let price = [(Colourless, 1), (Red, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Tuya Bearclaw"),
            creature(2, A, 5, 5, &[]),
            creature(3, A, 3, 3, &[]),
            creature(4, B, 9, 9, &[]),
            remove_spell(5, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let main = next_main(cast_named(&initial, &price), 3);
    let attacking = attack(&main, 1, B);
    let unchanged = round(attacking.clone()).into_state();
    let removed = round(
        targeted(
            &attacking,
            A,
            5,
            TargetRef::Object(physical(&attacking, 2).id()),
        )
        .into_state(),
    )
    .into_state();
    let smaller = round(removed).into_state();
    for (state, size) in [(&unchanged, 7), (&smaller, 5)] {
        let result = state
            .execution()
            .power_toughness(physical(state, 1).id())
            .unwrap()
            .unwrap()
            .derived;
        assert_eq!(
            result,
            PowerToughness {
                power: size.into(),
                toughness: size.into()
            }
        );
    }
}

fn defender_sacrifices(attacked: &GameState, ids: &[u64]) -> GameState {
    let choosing = round(attacked.clone()).into_state();
    assert_eq!(choosing.pending_decision().unwrap().chooser(), B);
    let choice = |ids: &[u64]| {
        DecisionReply::SelectResolution(
            ids.iter().map(|&id| physical(&choosing, id).id()).collect(),
        )
    };
    illegal(
        &choosing,
        B,
        combat_declaration(&choosing, choice(&ids[..ids.len() - 1])),
    );
    let mut state = reply(&choosing, choice(ids)).into_state();
    while let Some(RuleFrame::ResolutionGraveyardOrder(frame)) =
        state.pending_decision().map(|decision| decision.frame())
    {
        state = reply(
            &state,
            DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec()),
        )
        .into_state();
    }
    state
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_pathrazer_requires_three_sacrifices_and_three_blockers() {
    let price = [(ManaType::Colourless, 11)];
    let mut cards = vec![hand("Pathrazer of Ulamog")];
    cards.extend((2..=4).map(|id| {
        named(
            "Forest",
            id,
            B,
            ObjectDestination::Battlefield { controller: B },
        )
    }));
    cards.extend((5..=8).map(|id| creature(id, B, 1, 2, &[])));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let main = next_main(cast_named(&initial, &price), 3);
    let sacrificed = defender_sacrifices(&attack(&main, 1, B), &[2, 3, 4]);
    for id in 2..=4 {
        assert_eq!(physical(&sacrificed, id).zone(), ObjectZone::Graveyard(B));
    }
    let block = blockers(&sacrificed);
    let selection = |ids: &[u64]| {
        DecisionReply::DeclareBlockers(
            ids.iter()
                .map(|&id| BlockDeclaration {
                    attacker: physical(&block, 1).id(),
                    blocker: physical(&block, id).id(),
                })
                .collect(),
        )
    };
    illegal(&block, B, combat_declaration(&block, selection(&[5, 6])));
    reply(&block, selection(&[5, 6, 7]));
    reply(&block, selection(&[]));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ulamogs_crusher_must_attack_and_the_defender_sacrifices_two_permanents() {
    let price = [(ManaType::Colourless, 8)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Ulamog's Crusher"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
            creature(5, B, 2, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let summoning_sick = combat_frame(&entered, true);
    reply(&summoning_sick, DecisionReply::DeclareAttackers(vec![]));
    let main = next_main(entered, 3);
    let choose = combat_frame(&main, true);
    let omit_crusher = DecisionReply::DeclareAttackers(vec![AttackDeclaration {
        attacker: physical(&choose, 2).id(),
        defender: B,
        defending_planeswalker: None,
    }]);
    illegal(&choose, A, combat_declaration(&choose, omit_crusher));
    let result = defender_sacrifices(&attack(&main, 1, B), &[3, 5]);
    assert_eq!(physical(&result, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&result, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&result, 5).zone(), ObjectZone::Graveyard(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_artisan_can_return_its_controllers_creature_before_it_resolves_and_annihilates_two() {
    use spellwake_engine::effects::OptionalEffectChoice;
    let price = [(ManaType::Colourless, 9)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Artisan of Kozilek"),
            named("Elvish Mystic", 2, A, ObjectDestination::Graveyard),
            named("Palladium Myr", 3, B, ObjectDestination::Graveyard),
            creature(4, B, 1, 1, &[]),
            creature(5, B, 1, 1, &[]),
            creature(6, B, 1, 1, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&initial, 1, &price).into_state();
    let target = |id| DecisionReply::TriggerTarget(TargetRef::Object(physical(&cast, id).id()));
    illegal(&cast, A, combat_declaration(&cast, target(3)));
    let stacked = reply(&cast, target(2)).into_state();
    let offer = round(stacked).into_state();
    for (choice, zone) in [
        (OptionalEffectChoice::Decline, ObjectZone::Graveyard(A)),
        (OptionalEffectChoice::Take, ObjectZone::Battlefield),
    ] {
        let result = reply(&offer, DecisionReply::OptionalEffect(choice)).into_state();
        assert_eq!(physical(&result, 1).zone(), ObjectZone::Stack);
        assert_eq!(physical(&result, 2).zone(), zone);
    }
    let accepted = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let main = next_main(round(accepted).into_state(), 3);
    let result = defender_sacrifices(&attack(&main, 1, B), &[4, 5]);
    assert_eq!(physical(&result, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&result, 5).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&result, 6).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_it_that_betrays_brings_back_the_opponents_sacrificed_card_but_not_their_token() {
    use ManaType::*;
    use spellwake_engine::land::BasicLandType;
    let price = [(Colourless, 12)];
    let mut cards = vec![
        hand("It That Betrays"),
        named("Dread Drone", 2, B, ObjectDestination::Hand),
        named("Forest", 3, B, ObjectDestination::LibraryBottom),
    ];
    cards.extend((10..15).map(|id| {
        named(
            "Swamp",
            id,
            B,
            ObjectDestination::Battlefield { controller: B },
        )
    }));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let mut their_main = next_main(cast_named(&initial, &price), 2);
    for id in 10..15 {
        their_main = command(
            &their_main,
            B,
            GameCommand::ActivateMana {
                source: physical(&their_main, id).id(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Swamp),
            },
        )
        .into_state();
    }
    let drone_cast = command(
        &their_main,
        B,
        GameCommand::CastUntargeted {
            spell: physical(&their_main, 2).id(),
            payment: Some(spend(&[(Black, 5)])),
        },
    )
    .into_state();
    let ordering = round(round(drone_cast).into_state()).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("simultaneous Eldrazi Spawn");
    };
    let created = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let spawn = tokens(&created, B, 2);
    let main = next_main(created.into_state(), 3);
    let attacked = attack(&main, 1, B);
    let offer = round(attacked).into_state();
    let victims = vec![spawn[0], physical(&offer, 10).id()];
    let sacrifice = reply(&offer, DecisionReply::SelectResolution(victims)).into_state();
    assert_eq!(physical(&sacrifice, 10).zone(), ObjectZone::Graveyard(B));
    assert!(
        sacrifice
            .execution()
            .window()
            .cards()
            .get(spawn[0])
            .is_none()
    );
    assert_eq!(sacrifice.execution().window().entries().len(), 1);
    let restored = round(sacrifice).into_state();
    let returned = physical(&restored, 10);
    assert_eq!(returned.zone(), ObjectZone::Battlefield);
    assert_eq!(returned.owner(), B);
    assert_eq!(returned.controller(), Some(A));
    assert!(
        restored
            .execution()
            .window()
            .cards()
            .get(spawn[1])
            .is_some()
    );
    assert!(restored.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ceaseless_hunger_exiles_two_on_cast_survives_destroy_and_exiles_twenty_on_attack() {
    let price = [(ManaType::Colourless, 10)];
    let mut cards = vec![
        hand("Ulamog, the Ceaseless Hunger"),
        creature(2, B, 3, 3, &[]),
        named(
            "Mind Stone",
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        remove_spell(4, A),
    ];
    cards.extend((10..34).map(|id| named("Forest", id, B, ObjectDestination::LibraryBottom)));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = cast_untargeted(&initial, 1, &price).into_state();
    let targets = [2, 3].map(|id| TargetRef::Object(physical(&cast, id).id()));
    illegal(
        &cast,
        A,
        combat_declaration(
            &cast,
            DecisionReply::TriggerPermanentTargets(vec![targets[0]; 2]),
        ),
    );
    let stacked = reply(
        &cast,
        DecisionReply::TriggerPermanentTargets(targets.to_vec()),
    )
    .into_state();
    let trigger_resolved = round(stacked).into_state();
    assert_eq!(physical(&trigger_resolved, 1).zone(), ObjectZone::Stack);
    for id in [2, 3] {
        assert_eq!(physical(&trigger_resolved, id).zone(), ObjectZone::Exile);
    }
    let entered = round(trigger_resolved).into_state();
    let target = TargetRef::Object(physical(&entered, 1).id());
    let survives = round(targeted(&entered, A, 4, target).into_state()).into_state();
    assert_eq!(physical(&survives, 1).zone(), ObjectZone::Battlefield);
    let main = next_main(survives, 3);
    let before = main.execution().window().cards();
    let library = before.library(B).to_vec();
    let exiling = library[..20]
        .iter()
        .map(|id| before.get(*id).unwrap().card_id().unwrap().0)
        .collect::<Vec<_>>();
    let done = round(attack(&main, 1, B)).into_state();
    let after = done.execution().window().cards();
    assert_eq!(after.library(B), &library[20..]);
    assert_eq!(after.library(A), before.library(A));
    for id in exiling {
        assert_eq!(physical(&done, id).zone(), ObjectZone::Exile);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_flayer_borrows_and_untaps_the_target_then_both_creatures_can_annihilate() {
    use ManaType::*;
    use spellwake_engine::land::BasicLandType;
    let price = [(Colourless, 2), (Green, 8)];
    let mut cards = vec![
        hand("Flayer of Loyalties"),
        named(
            "Palladium Myr",
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        named(
            "Sol Ring",
            9,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named("Forest", 19, A, ObjectDestination::LibraryBottom),
        named("Swamp", 20, B, ObjectDestination::LibraryBottom),
    ];
    cards.extend((3..=8).map(|id| creature(id, B, 1, 1, &[])));
    cards.extend((10..=17).map(|id| {
        named(
            "Forest",
            id,
            A,
            ObjectDestination::Battlefield { controller: A },
        )
    }));
    let initial = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let mut funded = next_main(initial, 3);
    for (id, ability) in [(9, ManaAbilityRef::Printed(ManaAbilityId(0)))]
        .into_iter()
        .chain((10..=17).map(|id| (id, ManaAbilityRef::BasicLand(BasicLandType::Forest))))
    {
        funded = command(
            &funded,
            A,
            GameCommand::ActivateMana {
                source: physical(&funded, id).id(),
                ability,
            },
        )
        .into_state();
    }
    let announce = cast_untargeted(&funded, 1, &price).into_state();
    let creature = TargetRef::Object(physical(&announce, 2).id());
    let trigger = reply(&announce, DecisionReply::TriggerTarget(creature)).into_state();
    let their_priority = pass(&trigger).into_state();
    let tapped = command(
        &their_priority,
        B,
        GameCommand::ActivateMana {
            source: physical(&their_priority, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(physical(&tapped, 2).tapped(), Some(true));
    let borrowed = round(tapped).into_state();
    let myr = physical(&borrowed, 2);
    assert_eq!(myr.controller(), Some(A));
    assert_eq!(myr.tapped(), Some(false));
    assert_eq!(physical(&borrowed, 1).zone(), ObjectZone::Stack);
    let size = |state: &GameState, id| {
        state
            .execution()
            .current_characteristics(physical(state, id).id())
            .unwrap()
            .unwrap()
    };
    let view = size(&borrowed, 2);
    assert!(view.has_keyword(Haste));
    assert!(view.has_keyword(Trample));
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 10.into(),
            toughness: 10.into()
        }
    );
    let entered = round(borrowed).into_state();
    assert!(size(&entered, 1).has_keyword(Trample));
    let first_attack = attack(&entered, 2, B);
    let first = defender_sacrifices(&first_attack, &[3, 4]);
    for id in [3, 4] {
        assert_eq!(physical(&first, id).zone(), ObjectZone::Graveyard(B));
    }
    let later = next_main(first, 5);
    assert_eq!(physical(&later, 2).controller(), Some(B));
    let view = size(&later, 2);
    assert!(!view.has_keyword(Haste));
    assert!(!view.has_keyword(Trample));
    assert_eq!(view.power_toughness.unwrap().derived.power, 2.into());
    let second = defender_sacrifices(&attack(&later, 1, B), &[5, 6]);
    for id in [5, 6] {
        assert_eq!(physical(&second, id).zone(), ObjectZone::Graveyard(B));
    }
}
