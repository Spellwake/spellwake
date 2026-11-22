//! Card-specific token and constellation branches.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_nesting_dragon_landfall_creates_an_egg_whose_dragon_can_pay_for_power() {
    use ManaType::*;
    use spellwake_engine::activated::ActivatedAbilityId;
    let price = [(Colourless, 3), (Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Nesting Dragon"),
            named("Forest", 2, A, ObjectDestination::Hand),
            named("Forest", 3, B, ObjectDestination::Hand),
            remove_spell(4, A),
        ],
        &[(Colourless, 3), (Red, 3)],
        Step::PrecombatMain,
    );
    let dragon = cast_named(&initial, &price);
    assert!(
        dragon
            .execution()
            .current_characteristics(physical(&dragon, 1).id())
            .unwrap()
            .unwrap()
            .has_keyword(Flying)
    );
    let their_turn = next_main(dragon.clone(), 2);
    let other_land = command(
        &their_turn,
        B,
        GameCommand::PlayLand {
            land: physical(&their_turn, 3).id(),
        },
    )
    .into_state();
    assert!(other_land.execution().window().entries().is_empty());

    let landfall = command(
        &dragon,
        A,
        GameCommand::PlayLand {
            land: physical(&dragon, 2).id(),
        },
    )
    .into_state();
    let created = round(landfall);
    let egg = tokens(&created, A, 1)[0];
    let view = |state: &GameState, token| {
        state
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap()
    };
    let egg_view = view(created.state(), egg);
    assert!(egg_view.has_keyword(Defender));
    assert_eq!(egg_view.current_colors, [Color::Red].into());
    let vocabulary = current_catalogue().1;
    assert_eq!(
        egg_view.current_creature_subtypes,
        [
            vocabulary.resolve_label("Dragon").unwrap(),
            vocabulary.resolve_label("Egg").unwrap()
        ]
        .into()
    );
    assert_eq!(
        egg_view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 0.into(),
            toughness: 2.into()
        }
    );

    let dead =
        round(targeted(created.state(), A, 4, TargetRef::Object(egg)).into_state()).into_state();
    let born = round(dead);
    let token = tokens(&born, A, 1)[0];
    let baby = view(born.state(), token);
    assert!(baby.has_keyword(Flying));
    assert_eq!(baby.current_colors, [Color::Red].into());
    assert_eq!(
        baby.power_toughness.unwrap().derived,
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        }
    );
    let pay = GameCommand::ActivateUntargetedAbility {
        source: token,
        ability: ActivatedAbilityId(0),
        payment: Some(spend(&[(Red, 1)])),
    };
    let pumped = round(command(born.state(), A, pay).into_state()).into_state();
    assert_eq!(
        view(&pumped, token).power_toughness.unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 2.into()
        }
    );
    let later = next_main(pumped, 2);
    assert_eq!(
        view(&later, token).power_toughness.unwrap().derived.power,
        2.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_boon_gains_blessing_from_its_own_and_allied_enchantment_entries() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 2)];
    let mut flash = Program::enchantment(Some(ManaCost::free()));
    flash.printed_keywords.insert(Flash);
    let initial = game(
        vec![A, B],
        vec![
            hand("Boon of the Spirit Realm"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::enchantment(Some(ManaCost::free())),
            ),
            seed(5, B, ObjectDestination::Hand, flash),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let arrived = cast_named(&initial, &price);
    let stamp = |state: &GameState, card| {
        state
            .execution()
            .current_characteristics(physical(state, card).id())
            .unwrap()
            .unwrap()
    };
    assert_eq!(stamp(&arrived, 1).blessing_counters, 0.into());
    let one = round(arrived).into_state();
    assert_eq!(stamp(&one, 1).blessing_counters, 1.into());
    assert_eq!(
        stamp(&one, 2).power_toughness.unwrap().derived.power,
        3.into()
    );
    assert_eq!(
        stamp(&one, 3).power_toughness.unwrap().derived.power,
        2.into()
    );

    let their_priority = pass(&one).into_state();
    let theirs = GameCommand::CastUntargeted {
        spell: physical(&their_priority, 5).id(),
        payment: None,
    };
    let excluded = round(command(&their_priority, B, theirs).into_state()).into_state();
    assert_eq!(stamp(&excluded, 1).blessing_counters, 1.into());
    assert!(excluded.execution().window().entries().is_empty());
    let another = round(cast_untargeted(&one, 4, &[]).into_state()).into_state();
    let two = round(another).into_state();
    assert_eq!(stamp(&two, 1).blessing_counters, 2.into());
    assert_eq!(
        stamp(&two, 2).power_toughness.unwrap().derived.power,
        4.into()
    );
    let gone = round(targeted(&two, A, 6, TargetRef::Object(physical(&two, 1).id())).into_state())
        .into_state();
    assert_eq!(
        stamp(&gone, 2).power_toughness.unwrap().derived.power,
        2.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ondu_copies_an_aura_once_per_turn_and_attaches_to_a_nontargeted_host() {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice::{Decline, Take};
    let price = [(Colourless, 4), (White, 1)];
    let enchant = |id| {
        seed(
            id,
            A,
            ObjectDestination::Hand,
            Program::enchantment(Some(ManaCost::free())),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Ondu Spiritdancer"),
            creature(2, A, 2, 2, &[Shroud]),
            creature(3, B, 2, 2, &[]),
            enchant(4),
            named("All That Glitters", 5, A, ObjectDestination::Hand),
            enchant(6),
            enchant(7),
        ],
        &[(Colourless, 5), (White, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let free_entry =
        |state: &GameState, id| round(cast_untargeted(state, id, &[]).into_state()).into_state();
    let first = round(free_entry(&entered, 4)).into_state();
    let declined = reply(&first, DecisionReply::OptionalEffect(Decline)).into_state();
    let aura = GameCommand::CastPaid {
        spell: physical(&declined, 5).id(),
        target: TargetRef::Object(physical(&declined, 3).id()),
        payment: spend(&[(Colourless, 1), (White, 1)]),
    };
    let arrived = round(command(&declined, A, aura).into_state()).into_state();
    let offered = round(arrived).into_state();
    let accepted = reply(&offered, DecisionReply::OptionalEffect(Take)).into_state();
    let shrouded = TargetRef::Object(physical(&accepted, 2).id());
    let copied = reply(&accepted, DecisionReply::TokenAuraChoice(shrouded));
    let copy = tokens(&copied, A, 1)[0];
    assert_eq!(copied.state().execution().attached_to(copy), Some(shrouded));
    assert_eq!(
        copied
            .state()
            .execution()
            .attached_to(physical(copied.state(), 5).id()),
        Some(TargetRef::Object(physical(copied.state(), 3).id()))
    );
    let view = copied
        .state()
        .execution()
        .current_characteristics(copy)
        .unwrap()
        .unwrap();
    assert!(
        view.current_names
            .iter()
            .any(|name| name.as_str() == "All That Glitters")
    );
    let flush = |mut state: GameState| {
        while !state.execution().window().entries().is_empty() {
            assert!(state.pending_decision().is_none());
            state = round(state).into_state();
        }
        state
    };
    let no_recursion = flush(copied.into_state());
    let same_turn = flush(free_entry(&no_recursion, 6));
    let token_count = |state: &GameState| {
        state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|obj| obj.is_token() && obj.zone() == ObjectZone::Battlefield)
            .count()
    };
    assert_eq!(token_count(&same_turn), 1);
    let later = next_main(same_turn, 3);
    let renewed = round(free_entry(&later, 7)).into_state();
    let copy_again = reply(&renewed, DecisionReply::OptionalEffect(Take)).into_state();
    assert_eq!(token_count(&copy_again), 2);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_narci_draws_for_the_sacrificed_saga_and_drains_for_its_final_chapter() {
    use ManaType::*;
    let narci_price = [(Colourless, 1), (White, 1), (Black, 1), (Green, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Narci, Fable Singer"),
            named("The Eldest Reborn", 2, A, ObjectDestination::Hand),
            named("Fencing Ace", 3, B, ObjectDestination::Graveyard),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
            named("Island", 5, A, ObjectDestination::LibraryBottom),
            named("Swamp", 6, A, ObjectDestination::LibraryBottom),
            named("Plains", 7, B, ObjectDestination::LibraryBottom),
            named("Plains", 8, C, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 5), (White, 1), (Black, 2), (Green, 1)],
        Step::PrecombatMain,
    );
    let bard = cast_named(&initial, &narci_price);
    let saga =
        round(cast_untargeted(&bard, 2, &[(Colourless, 4), (Black, 1)]).into_state()).into_state();
    let chapter_three = next_main(saga, 7);
    assert_eq!(physical(&chapter_three, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(chapter_three.execution().life_total(B), Some(&20.into()));
    let selected = reply(
        &chapter_three,
        DecisionReply::TriggerTarget(TargetRef::Object(physical(&chapter_three, 3).id())),
    )
    .into_state();
    let mut final_chapter = round(selected).into_state();
    assert_eq!(physical(&final_chapter, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&final_chapter, 3).controller(), Some(A));
    if let Some(decision) = final_chapter.pending_decision() {
        let RuleFrame::Triggers(frame) = decision.frame() else {
            panic!("order Narci's two triggers");
        };
        assert_eq!(frame.instances().len(), 2);
        final_chapter = reply(
            &final_chapter,
            DecisionReply::OrderTriggers(frame.instances().to_vec()),
        )
        .into_state();
    }
    assert_eq!(final_chapter.execution().window().entries().len(), 2);
    let resolved = round(round(final_chapter).into_state()).into_state();
    assert_eq!(physical(&resolved, 6).zone(), ObjectZone::Hand(A));
    assert_eq!(resolved.execution().life_total(A), Some(&25.into()));
    assert_eq!(resolved.execution().life_total(B), Some(&15.into()));
    assert_eq!(resolved.execution().life_total(C), Some(&15.into()));

    let attacks = attack(&resolved, 1, B);
    let combat = exile_hand::until(attacks, |state| {
        state.execution().life_total(B) == Some(&12.into())
    });
    assert_eq!(combat.execution().life_total(A), Some(&28.into()));
    assert_eq!(combat.execution().life_total(C), Some(&15.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_grave_pact_makes_both_opponents_sacrifice_and_ignores_their_creatures_dying() {
    use ManaType::*;
    let price = [(Colourless, 1), (Black, 3)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Grave Pact"),
            creature(2, A, 1, 1, &[]),
            creature(3, A, 1, 1, &[]),
            creature(4, B, 1, 1, &[]),
            creature(5, B, 1, 1, &[Indestructible]),
            creature(6, C, 1, 1, &[]),
            creature(7, C, 1, 1, &[Indestructible]),
            remove_spell(8, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let pact = cast_named(&initial, &price);
    let target = TargetRef::Object(physical(&pact, 2).id());
    let dead = round(targeted(&pact, A, 8, target).into_state()).into_state();
    assert_eq!(physical(&dead, 2).zone(), ObjectZone::Graveyard(A));
    let first = round(dead).into_state();
    let selection =
        |state: &GameState, id| DecisionReply::SelectResolution(vec![physical(state, id).id()]);
    assert_eq!(first.pending_decision().unwrap().chooser(), B);
    illegal(&first, B, combat_declaration(&first, selection(&first, 6)));
    let second = reply(&first, selection(&first, 5)).into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), C);
    let finished = reply(&second, selection(&second, 7)).into_state();
    for (id, zone) in [
        (3, ObjectZone::Battlefield),
        (4, ObjectZone::Battlefield),
        (5, ObjectZone::Graveyard(B)),
        (6, ObjectZone::Battlefield),
        (7, ObjectZone::Graveyard(C)),
    ] {
        assert_eq!(physical(&finished, id).zone(), zone);
    }
    assert!(finished.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_bastion_and_teysa_create_a_vigilant_lifelink_soldier_and_double_friendly_death_triggers() {
    use ManaType::*;
    let bastion_price = [(Colourless, 2), (Black, 1)];
    let teysa_price = [(Colourless, 2), (White, 1), (Black, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Bastion of Remembrance"),
            named("Teysa Karlov", 2, A, ObjectDestination::Hand),
            creature(3, B, 1, 1, &[]),
            creature(4, A, 1, 1, &[]),
            remove_spell(5, A),
            remove_spell(6, A),
        ],
        &[(Colourless, 4), (Black, 2), (White, 1)],
        Step::PrecombatMain,
    );
    let teysa = round(cast_untargeted(&initial, 2, &teysa_price).into_state()).into_state();
    let bastion = cast_named(&teysa, &bastion_price);
    let created = round(bastion);
    assert!(created.state().execution().window().entries().is_empty());
    let soldier = tokens(&created, A, 1)[0];
    let view = created
        .state()
        .execution()
        .current_characteristics(soldier)
        .unwrap()
        .unwrap();
    let vocabulary = current_catalogue().1;
    assert_eq!(
        view.current_creature_subtypes,
        ["Human", "Soldier"]
            .map(|label| vocabulary.resolve_label(label).unwrap())
            .into()
    );
    assert_eq!(view.current_colors, [Color::White].into());
    assert_eq!(
        view.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    for keyword in [Vigilance, Lifelink] {
        assert!(view.has_keyword(keyword));
        assert!(
            !created
                .state()
                .execution()
                .current_characteristics(physical(created.state(), 4).id())
                .unwrap()
                .unwrap()
                .has_keyword(keyword)
        );
    }
    let enemy = TargetRef::Object(physical(created.state(), 3).id());
    let controlled = round(targeted(created.state(), A, 5, enemy).into_state()).into_state();
    assert!(controlled.execution().window().entries().is_empty());
    let dead =
        round(targeted(&controlled, A, 6, TargetRef::Object(soldier)).into_state()).into_state();
    let stacked = match dead.pending_decision().map(|decision| decision.frame()) {
        Some(RuleFrame::Triggers(frame)) => reply(
            &dead,
            DecisionReply::OrderTriggers(frame.instances().to_vec()),
        )
        .into_state(),
        None => dead,
        Some(frame) => panic!("unexpected death trigger frame {frame:?}"),
    };
    assert_eq!(stacked.execution().window().entries().len(), 2);
    let mut resolved = stacked;
    for gain in 1_i64..=2 {
        resolved = round(resolved).into_state();
        assert_eq!(
            resolved.execution().life_total(A),
            Some(&(20 + gain).into())
        );
        for opponent in [B, C] {
            assert_eq!(
                resolved.execution().life_total(opponent),
                Some(&(20 - gain).into())
            );
        }
    }
    assert!(resolved.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_havoc_jester_treats_the_sacrifice_of_an_artifact_as_a_damage_trigger() {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 1)];
    let sphere = named(
        "Commander's Sphere",
        2,
        A,
        ObjectDestination::Battlefield { controller: A },
    );
    let draw = sphere.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            hand("Havoc Jester"),
            sphere,
            creature(3, A, 1, 1, &[]),
            creature(4, B, 1, 1, &[]),
            remove_spell(5, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let killed = round(
        targeted(
            &entered,
            A,
            5,
            TargetRef::Object(physical(&entered, 3).id()),
        )
        .into_state(),
    )
    .into_state();
    assert!(killed.execution().window().entries().is_empty());
    let sacrifice = command(
        &killed,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&killed, 2).id(),
            ability: draw,
            payment: None,
        },
    )
    .into_state();
    assert_eq!(physical(&sacrifice, 2).zone(), ObjectZone::Graveyard(A));
    let resolve_for = |target| {
        let announce = reply(&sacrifice, DecisionReply::TriggerTarget(target)).into_state();
        round(announce).into_state()
    };
    let player = resolve_for(TargetRef::Player(B));
    assert_eq!(player.execution().life_total(B), Some(&19.into()));
    assert_eq!(player.execution().window().entries().len(), 1);
    let creature = resolve_for(TargetRef::Object(physical(&sacrifice, 4).id()));
    assert_eq!(physical(&creature, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(creature.execution().life_total(B), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ogre_slumlord_offers_a_rat_for_either_players_nontoken_but_never_the_rat_itself() {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice;
    let price = [(Colourless, 3), (Black, 2)];
    let rat_type = current_catalogue().1.resolve_label("Rat").unwrap();
    let initial = game(
        vec![A, B],
        vec![
            hand("Ogre Slumlord"),
            creature(2, A, 1, 1, &[]),
            creature(3, B, 1, 1, &[]),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::creature(None, 1.into(), 1.into(), [rat_type].into()),
            ),
            remove_spell(5, A),
            remove_spell(6, A),
            remove_spell(7, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let kill = |state: &GameState, spell, object| {
        round(targeted(state, A, spell, TargetRef::Object(object)).into_state()).into_state()
    };
    let opponent = round(kill(&entered, 5, physical(&entered, 3).id())).into_state();
    let refused = reply(
        &opponent,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert!(refused.state().execution().window().entries().is_empty());
    let own = round(kill(refused.state(), 6, physical(refused.state(), 2).id())).into_state();
    let gained = reply(
        &own,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    let rat = tokens(&gained, A, 1)[0];
    let sheet = |id| {
        gained
            .state()
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap()
    };
    let profile = sheet(rat);
    assert_eq!(profile.current_creature_subtypes, [rat_type].into());
    assert_eq!(profile.current_colors, [Color::Black].into());
    assert_eq!(
        profile.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    assert!(profile.has_keyword(Deathtouch));
    assert!(!sheet(physical(gained.state(), 4).id()).has_keyword(Deathtouch));
    let gone = kill(gained.state(), 7, rat);
    assert!(gone.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_judith_strengthens_other_creatures_and_pings_only_for_nontoken_deaths() {
    use ManaType::*;
    let price = [(Colourless, 1), (Black, 1), (Red, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Judith, the Scourge Diva"),
            named("Dragon Fodder", 2, A, ObjectDestination::Hand),
            creature(3, A, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
            remove_spell(5, A),
            remove_spell(6, A),
        ],
        &[(Colourless, 2), (Black, 1), (Red, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let making =
        round(cast_untargeted(&entered, 2, &[(Colourless, 1), (Red, 1)]).into_state()).into_state();
    let RuleFrame::TimestampOrder(frame) = making.pending_decision().unwrap().frame() else {
        panic!("simultaneous Goblins");
    };
    let made = reply(
        &making,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let goblin = tokens(&made, A, 2)[0];
    let state = made.state();
    for (id, size) in [
        (physical(state, 1).id(), 2),
        (physical(state, 3).id(), 3),
        (physical(state, 4).id(), 2),
        (goblin, 2),
    ] {
        assert_eq!(
            state
                .execution()
                .power_toughness(id)
                .unwrap()
                .unwrap()
                .derived
                .power,
            size.into()
        );
    }
    let lost_token =
        round(targeted(state, A, 5, TargetRef::Object(goblin)).into_state()).into_state();
    assert!(lost_token.execution().window().entries().is_empty());
    let lost_card = round(
        targeted(
            &lost_token,
            A,
            6,
            TargetRef::Object(physical(&lost_token, 3).id()),
        )
        .into_state(),
    )
    .into_state();
    let hits = reply(
        &lost_card,
        DecisionReply::TriggerTarget(TargetRef::Player(B)),
    )
    .into_state();
    let resolved = round(hits).into_state();
    assert_eq!(resolved.execution().life_total(B), Some(&19.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_yahenni_attacks_immediately_grows_on_opponent_deaths_and_sacrifices_to_survive() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let price = [(Colourless, 2), (Black, 1)];
    let yahenni = hand("Yahenni, Undying Partisan");
    let ability = yahenni.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            yahenni,
            creature(2, B, 2, 1, &[]),
            creature(3, A, 2, 2, &[]),
            remove_spell(4, A),
            remove_spell(5, A),
            remove_spell(6, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let attacked = attack(&entered, 1, B);
    assert_eq!(physical(&attacked, 1).tapped(), Some(true));
    let kill = |state: &GameState, spell, victim| {
        round(
            targeted(
                state,
                A,
                spell,
                TargetRef::Object(physical(state, victim).id()),
            )
            .into_state(),
        )
        .into_state()
    };
    let grown = round(kill(&entered, 4, 2)).into_state();
    let source = physical(&grown, 1).id();
    let counters = |state: &GameState| {
        state
            .execution()
            .counters_of_kind(source, CounterKind::PlusOne)
    };
    assert_eq!(counters(&grown), 1.into());
    let sacrifice = |id| GameCommand::ActivateWithCostObjects {
        source,
        ability,
        granting_effect: None,
        target: None,
        objects: vec![physical(&grown, id).id()],
        payment: None,
    };
    illegal(&grown, A, sacrifice(1));
    let paid = command(&grown, A, sacrifice(3)).into_state();
    assert_eq!(physical(&paid, 3).zone(), ObjectZone::Graveyard(A));
    assert_eq!(paid.execution().window().entries().len(), 1);
    let protected = round(paid).into_state();
    let lives = kill(&protected, 5, 1);
    assert_eq!(physical(&lives, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(counters(&lives), 1.into());
    let later = next_main(lives, 3);
    let dead = kill(&later, 6, 1);
    assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_archfiend_prevents_opponent_gains_and_counts_each_opponents_actual_loss() {
    use ManaType::*;
    use spellwake_engine::life::LifeAdjustment;
    let price = [(Colourless, 6), (Black, 2)];
    let create_spell = |id, instruction| {
        seed(
            id,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePlayer, instruction),
        )
    };
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Archfiend of Despair"),
            create_spell(2, Instruction::Damage { amount: 3.into() }),
            create_spell(
                3,
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Lose(2.into()),
                },
            ),
            create_spell(
                4,
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(7.into()),
                },
            ),
            create_spell(
                5,
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let current = entered
        .execution()
        .current_characteristics(physical(&entered, 1).id())
        .unwrap()
        .unwrap();
    assert!(current.has_keyword(Flying));
    let shift = |state: GameState, card, player| {
        round(targeted(&state, A, card, TargetRef::Player(player)).into_state()).into_state()
    };
    let changes = [(2, B), (3, C), (4, B), (5, A)]
        .into_iter()
        .fold(entered, |state, (card, player)| shift(state, card, player));
    assert_eq!(changes.execution().life_total(A), Some(&21.into()));
    assert_eq!(changes.execution().life_total(B), Some(&17.into()));
    assert_eq!(changes.execution().life_total(C), Some(&18.into()));
    let triggered = exile_hand::until(changes, |state| {
        state.turn().unwrap().position().step == Step::End
            && !state.execution().window().entries().is_empty()
    });
    let drained = round(triggered);
    assert_eq!(drained.state().execution().life_total(A), Some(&21.into()));
    assert_eq!(drained.state().execution().life_total(B), Some(&14.into()));
    assert_eq!(drained.state().execution().life_total(C), Some(&16.into()));
    let life_events = events(&drained)
        .filter_map(|event| {
            let affected = event
                .occurrences
                .iter()
                .filter(|row| matches!(row.value, RulesOccurrence::LifeChanged { .. }))
                .count();
            (affected != 0).then_some(affected)
        })
        .collect::<Vec<_>>();
    assert_eq!(life_events, [2]);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_gisela_doubles_opponent_damage_and_prevents_the_rounded_up_half_of_friendly_damage() {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 1), (White, 2)];
    let mut cards = vec![
        hand("Gisela, Blade of Goldnight"),
        creature(2, A, 1, 10, &[]),
        creature(3, B, 1, 10, &[]),
    ];
    cards.extend((4..=7).map(|id| {
        seed(
            id,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleAnyTarget,
                Instruction::Damage { amount: 3.into() },
            ),
        )
    }));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entered = cast_named(&initial, &price);
    let profile = entered
        .execution()
        .current_characteristics(physical(&entered, 1).id())
        .unwrap()
        .unwrap();
    assert!(profile.has_keyword(Flying));
    assert!(profile.has_keyword(FirstStrike));
    let destinations = [
        TargetRef::Player(A),
        TargetRef::Player(B),
        TargetRef::Object(physical(&entered, 2).id()),
        TargetRef::Object(physical(&entered, 3).id()),
    ];
    let damage = (4..=7)
        .zip(destinations)
        .fold(entered, |state, (card, target)| {
            round(targeted(&state, A, card, target).into_state()).into_state()
        });
    assert_eq!(damage.execution().life_total(A), Some(&19.into()));
    assert_eq!(damage.execution().life_total(B), Some(&14.into()));
    for (id, expected) in [(2, 1), (3, 6)] {
        assert_eq!(
            damage
                .execution()
                .battlefield_marked_damage(physical(&damage, id).id()),
            Some(expected.into()),
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_slimefoot_builds_a_saproling_and_its_death_damages_every_opponent() {
    use ManaType::*;
    let price = [(Colourless, 1), (Black, 1), (Green, 1)];
    let slimefoot = hand("Slimefoot, the Stowaway");
    let grow = slimefoot.program.activated[0].id;
    let initial = game(
        vec![A, B, C],
        vec![
            slimefoot,
            creature(2, A, 1, 1, &[]),
            remove_spell(3, A),
            remove_spell(4, A),
        ],
        &[(Colourless, 5), (Black, 1), (Green, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let cast = |amount| GameCommand::ActivateUntargetedAbility {
        source: physical(&entered, 1).id(),
        ability: grow,
        payment: Some(spend(&[(Colourless, amount)])),
    };
    illegal(&entered, A, cast(3));
    let paid = command(&entered, A, cast(4)).into_state();
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    let born = round(paid);
    let saproling = tokens(&born, A, 1)[0];
    let view = born
        .state()
        .execution()
        .current_characteristics(saproling)
        .unwrap()
        .unwrap();
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Saproling").unwrap()].into()
    );
    assert_eq!(view.current_colors, [Color::Green].into());
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let ordinary = TargetRef::Object(physical(born.state(), 2).id());
    let died = round(targeted(born.state(), A, 3, ordinary).into_state()).into_state();
    assert!(died.execution().window().entries().is_empty());
    let sap_died =
        round(targeted(&died, A, 4, TargetRef::Object(saproling)).into_state()).into_state();
    assert_eq!(sap_died.execution().window().entries().len(), 1);
    let recovered = round(sap_died);
    assert_eq!(
        recovered.state().execution().life_total(A),
        Some(&21.into())
    );
    assert_eq!(
        recovered.state().execution().life_total(B),
        Some(&19.into())
    );
    assert_eq!(
        recovered.state().execution().life_total(C),
        Some(&19.into())
    );
    assert_eq!(
        events(&recovered)
            .flat_map(|event| &event.occurrences)
            .filter(|row| matches!(row.value, RulesOccurrence::DamageDealt { .. }))
            .count(),
        2
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_taborax_gains_lifelink_at_five_counters_and_offers_a_card_for_a_cleric() {
    use ManaType::*;
    use spellwake_engine::effects::{CounterKind, OptionalEffectChoice};
    let price = [(Colourless, 2), (Black, 1)];
    let cleric = current_catalogue().1.resolve_label("Cleric").unwrap();
    let mut cards = vec![
        hand("Taborax, Hope's Demise"),
        creature(2, B, 1, 1, &[]),
        seed(
            7,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::creature(None, 1.into(), 1.into(), [cleric].into()),
        ),
        named("Forest", 8, A, ObjectDestination::LibraryTop),
    ];
    cards.extend((3..=6).map(|id| creature(id, A, 1, 1, &[])));
    cards.extend((10..=15).map(|id| remove_spell(id, A)));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let mut state = cast_named(&initial, &price);
    let source = physical(&state, 1).id();
    let kill = |state: &GameState, spell, target| {
        round(
            targeted(
                state,
                A,
                spell,
                TargetRef::Object(physical(state, target).id()),
            )
            .into_state(),
        )
        .into_state()
    };
    let counters = |state: &GameState| {
        state
            .execution()
            .counters_of_kind(source, CounterKind::PlusOne)
    };
    let view = |state: &GameState| {
        state
            .execution()
            .current_characteristics(source)
            .unwrap()
            .unwrap()
    };
    state = kill(&state, 10, 2);
    assert!(state.execution().window().entries().is_empty());
    assert!(view(&state).has_keyword(Flying));
    for (index, victim) in (3..=6).enumerate() {
        state = round(kill(&state, 11 + index as u64, victim)).into_state();
        assert_eq!(counters(&state), (index as i64 + 1).into());
        assert!(!view(&state).has_keyword(Lifelink));
        assert_eq!(physical(&state, 8).zone(), ObjectZone::Library(A));
    }
    let offer = round(kill(&state, 15, 7)).into_state();
    assert_eq!(counters(&offer), 5.into());
    assert!(view(&offer).has_keyword(Lifelink));
    for (decision, zone, life) in [
        (OptionalEffectChoice::Decline, ObjectZone::Library(A), 20),
        (OptionalEffectChoice::Take, ObjectZone::Hand(A), 19),
    ] {
        let final_state = reply(&offer, DecisionReply::OptionalEffect(decision)).into_state();
        assert_eq!(physical(&final_state, 8).zone(), zone);
        assert_eq!(final_state.execution().life_total(A), Some(&life.into()));
        assert_eq!(counters(&final_state), 5.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_demons_disciple_allows_a_planeswalker_and_checks_each_player() {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 1)];
    let walker_cost = [(Colourless, 1), (Blue, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Demon's Disciple"),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            creature(3, B, 1, 1, &[Indestructible]),
            named(
                "Forest",
                4,
                C,
                ObjectDestination::Battlefield { controller: C },
            ),
        ],
        &[(Colourless, 3), (Blue, 2), (Black, 1)],
        Step::PrecombatMain,
    );
    let walker = round(cast_untargeted(&initial, 2, &walker_cost).into_state()).into_state();
    assert_eq!(physical(&walker, 2).zone(), ObjectZone::Battlefield);
    let entered = cast_named(&walker, &price);
    let choosing = round(entered).into_state();
    let selection =
        |state: &GameState, id| DecisionReply::SelectResolution(vec![physical(state, id).id()]);
    assert_eq!(choosing.pending_decision().unwrap().chooser(), A);
    for chosen in [1, 2] {
        let opponents = reply(&choosing, selection(&choosing, chosen)).into_state();
        let finished = if let Some(decision) = opponents.pending_decision() {
            assert_eq!(decision.chooser(), B);
            reply(&opponents, selection(&opponents, 3)).into_state()
        } else {
            opponents
        };
        assert_eq!(physical(&finished, chosen).zone(), ObjectZone::Graveyard(A));
        assert_eq!(
            physical(&finished, 3 - chosen).zone(),
            ObjectZone::Battlefield
        );
        assert_eq!(physical(&finished, 3).zone(), ObjectZone::Graveyard(B));
        assert_eq!(physical(&finished, 4).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_goremand_pays_a_creature_to_cast_then_makes_each_opponent_sacrifice() {
    use ManaType::*;
    let price = [(Colourless, 4), (Black, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Goremand"),
            creature(2, A, 1, 1, &[]),
            creature(3, A, 1, 1, &[]),
            creature(4, B, 2, 2, &[]),
            creature(5, C, 2, 2, &[Indestructible]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = |objects| GameCommand::CastWithResources {
        spell: physical(&initial, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&price)),
            objects,
            ..Default::default()
        },
    };
    illegal(&initial, A, cast(vec![]));
    illegal(
        &initial,
        A,
        cast(vec![CastResource::SacrificeForSpell(
            physical(&initial, 4).id(),
        )]),
    );
    let paid = command(
        &initial,
        A,
        cast(vec![CastResource::SacrificeForSpell(
            physical(&initial, 2).id(),
        )]),
    )
    .into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Stack);
    let battlefield = round(paid).into_state();
    let mut result = round(battlefield).into_state();
    while let Some(decision) = result.pending_decision() {
        let chosen = match decision.chooser() {
            B => 4,
            C => 5,
            actor => panic!("unexpected actor {actor:?}"),
        };
        result = reply(
            &result,
            DecisionReply::SelectResolution(vec![physical(&result, chosen).id()]),
        )
        .into_state();
    }
    assert_eq!(physical(&result, 3).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&result, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&result, 5).zone(), ObjectZone::Graveyard(C));
    let view = result
        .execution()
        .current_characteristics(physical(&result, 1).id())
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(Flying));
    assert!(view.has_keyword(Trample));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_anax_counts_red_devotion_and_creates_one_or_two_nonblocking_satyrs() {
    use ManaType::*;
    use spellwake_engine::characteristics::BlockingRestriction;
    let price = [(Colourless, 1), (Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Anax, Hardened in the Forge"),
            named(
                "Striking Sliver",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, A, 4, 4, &[]),
            creature(4, B, 4, 4, &[]),
            remove_spell(5, A),
            remove_spell(6, A),
            remove_spell(7, A),
            remove_spell(8, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let stats = |state: &GameState, id| {
        state
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap()
    };
    let source = physical(&entered, 1).id();
    assert_eq!(
        stats(&entered, source)
            .power_toughness
            .unwrap()
            .derived
            .power,
        3.into()
    );
    let kill = |state: &GameState, spell, victim| {
        round(targeted(state, A, spell, TargetRef::Object(victim)).into_state()).into_state()
    };
    let enemy_died = kill(&entered, 5, physical(&entered, 4).id());
    assert!(enemy_died.execution().window().entries().is_empty());
    let giant_died = kill(&enemy_died, 6, physical(&enemy_died, 3).id());
    let ordered = round(giant_died).into_state();
    let RuleFrame::TimestampOrder(frame) = ordered.pending_decision().unwrap().frame() else {
        panic!("simultaneous Satyrs");
    };
    let two = reply(
        &ordered,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let satyrs = tokens(&two, A, 2);
    let satyr_subtype = current_catalogue().1.resolve_label("Satyr").unwrap();
    for satyr in satyrs {
        let view = stats(two.state(), satyr);
        assert_eq!(view.current_creature_subtypes, [satyr_subtype].into());
        assert_eq!(view.current_colors, [Color::Red].into());
        assert!(
            view.blocking_restrictions
                .contains(&BlockingRestriction::CannotBlock)
        );
        assert_eq!(
            view.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
    }
    let small_died = kill(two.state(), 7, physical(two.state(), 2).id());
    let one = round(small_died);
    let check_one_new_satyr = |candidate: &NativeCandidate| {
        let births = events(candidate)
            .flat_map(|event| &event.occurrences)
            .filter_map(|occurrence| match &occurrence.value {
                RulesOccurrence::TokenCreated { creator, after } => Some((*creator, after)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [(creator, birth)] = births.as_slice() else {
            panic!("exactly one new Satyr");
        };
        assert_eq!(*creator, A);
        assert_eq!(birth.object.controller(), Some(A));
        assert_eq!(
            stats(candidate.state(), birth.object.id()).current_creature_subtypes,
            [satyr_subtype].into()
        );
    };
    check_one_new_satyr(&one);
    assert_eq!(
        stats(one.state(), source)
            .power_toughness
            .unwrap()
            .derived
            .power,
        2.into()
    );
    let anax_died = kill(one.state(), 8, source);
    let last = round(anax_died);
    check_one_new_satyr(&last);
    assert_eq!(physical(last.state(), 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sekkuar_creates_a_hasty_graveborn_for_another_friendly_nontoken() {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 1), (Red, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Sek'Kuar, Deathkeeper"),
            creature(2, B, 1, 1, &[]),
            creature(3, A, 1, 1, &[]),
            remove_spell(4, A),
            remove_spell(5, A),
            remove_spell(6, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let kill = |state: &GameState, spell, target| {
        round(
            targeted(
                state,
                A,
                spell,
                TargetRef::Object(physical(state, target).id()),
            )
            .into_state(),
        )
        .into_state()
    };
    let enemy_died = kill(&entered, 4, 2);
    assert!(enemy_died.execution().window().entries().is_empty());
    let own_died = kill(&enemy_died, 5, 3);
    let born = round(own_died);
    let graveborn = tokens(&born, A, 1)[0];
    let view = born
        .state()
        .execution()
        .current_characteristics(graveborn)
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(Haste));
    assert_eq!(view.current_colors, [Color::Black, Color::Red].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Graveborn").unwrap()].into()
    );
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 1.into()
        }
    );
    let declaring = combat_frame(born.state(), true);
    let attacked = reply(
        &declaring,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: graveborn,
            defender: B,
            defending_planeswalker: None,
        }]),
    );
    assert_eq!(
        attacked
            .state()
            .execution()
            .window()
            .cards()
            .get(graveborn)
            .unwrap()
            .tapped(),
        Some(true)
    );
    let source_died = kill(born.state(), 6, 1);
    assert!(source_died.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_felidar_retreat_resolves_both_landfall_modes_with_doubling_season_and_intangible_virtue() {
    use ManaType::*;
    use spellwake_engine::{effects::CounterKind, mana::CastMode};
    let price = [(Colourless, 3), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Felidar Retreat"),
            named("Doubling Season", 2, A, ObjectDestination::Hand),
            named("Intangible Virtue", 3, A, ObjectDestination::Hand),
            named("Forest", 4, A, ObjectDestination::Hand),
            named("Forest", 5, B, ObjectDestination::Hand),
            creature(6, A, 3, 3, &[]),
            creature(7, B, 3, 3, &[]),
        ],
        &[(Colourless, 8), (White, 2), (Green, 1)],
        Step::PrecombatMain,
    );
    let virtue = round(cast_untargeted(&initial, 3, &[(Colourless, 1), (White, 1)]).into_state())
        .into_state();
    let season = round(cast_untargeted(&virtue, 2, &[(Colourless, 4), (Green, 1)]).into_state())
        .into_state();
    let entered = cast_named(&season, &price);
    let play = |state: &GameState, actor, land| {
        command(
            state,
            actor,
            GameCommand::PlayLand {
                land: physical(state, land).id(),
            },
        )
        .into_state()
    };
    let their_turn = next_main(entered.clone(), 2);
    let opponent = play(&their_turn, B, 5);
    assert!(opponent.execution().window().entries().is_empty());
    let allied = play(&entered, A, 4);
    let choose = |mode| {
        DecisionReply::TriggerModes(vec![CastMode {
            mode,
            target: None,
            targets: vec![],
        }])
    };
    illegal(&allied, A, combat_declaration(&allied, choose(2)));
    let making = round(reply(&allied, choose(0)).into_state());
    let ordering = making.state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("two Cats create a timestamp choice");
    };
    let cats = reply(
        ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let vocabulary = current_catalogue().1;
    let view = |state: &GameState, id| {
        state
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap()
    };
    for cat in tokens(&cats, A, 2) {
        let profile = view(cats.state(), cat);
        assert_eq!(profile.current_colors, [Color::White].into());
        assert_eq!(
            profile.current_creature_subtypes,
            ["Cat", "Beast"]
                .map(|kind| vocabulary.resolve_label(kind).unwrap())
                .into()
        );
        assert!(profile.has_keyword(Vigilance));
        assert_eq!(
            profile.power_toughness.unwrap().derived,
            PowerToughness {
                power: 3.into(),
                toughness: 3.into()
            }
        );
    }
    let allied_creature = physical(cats.state(), 6).id();
    assert!(!view(cats.state(), allied_creature).has_keyword(Vigilance));
    assert_eq!(
        view(cats.state(), allied_creature)
            .power_toughness
            .unwrap()
            .derived
            .power,
        3.into()
    );

    let strengthened = round(reply(&allied, choose(1)).into_state()).into_state();
    let opposing_creature = physical(&strengthened, 7).id();
    for (id, count, vigilance) in [(allied_creature, 2, true), (opposing_creature, 0, false)] {
        assert_eq!(
            strengthened
                .execution()
                .counters_of_kind(id, CounterKind::PlusOne),
            count.into()
        );
        assert_eq!(view(&strengthened, id).has_keyword(Vigilance), vigilance);
    }
    let next_turn = next_main(strengthened, 2);
    assert_eq!(
        next_turn
            .execution()
            .counters_of_kind(allied_creature, CounterKind::PlusOne),
        2.into()
    );
    assert!(!view(&next_turn, allied_creature).has_keyword(Vigilance));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_anafenza_bolsters_only_the_least_toughness_after_another_allied_nontoken_enters() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let price = [(White, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Anafenza, Kin-Tree Spirit"),
            named("Elvish Mystic", 2, A, ObjectDestination::Hand),
            named("Dragon Fodder", 3, A, ObjectDestination::Hand),
            creature(4, A, 2, 1, &[]),
            creature(5, A, 3, 3, &[]),
            creature(6, B, 1, 1, &[]),
        ],
        &[(White, 2), (Green, 1), (Colourless, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert!(entered.execution().window().entries().is_empty());
    let elf = round(cast_untargeted(&entered, 2, &[(Green, 1)]).into_state()).into_state();
    let bolster = round(elf).into_state();
    let RuleFrame::ResolutionSelect(frame) = bolster.pending_decision().unwrap().frame() else {
        panic!("Bolster chooses between allied creatures tied for least toughness");
    };
    for id in [2, 4] {
        assert!(frame.domain().contains(&physical(&bolster, id).id()));
    }
    assert_eq!(frame.domain().len(), 2);
    let choose = |id| DecisionReply::SelectResolution(vec![physical(&bolster, id).id()]);
    illegal(&bolster, A, combat_declaration(&bolster, choose(6)));
    let growing = reply(&bolster, choose(4)).into_state();
    let goblin_order =
        round(cast_untargeted(&growing, 3, &[(Colourless, 1), (Red, 1)]).into_state()).into_state();
    let RuleFrame::TimestampOrder(order) = goblin_order.pending_decision().unwrap().frame() else {
        panic!("simultaneous Goblins");
    };
    let goblins = reply(
        &goblin_order,
        DecisionReply::TimestampOrder(order.members().iter().map(|member| member.member).collect()),
    );
    tokens(&goblins, A, 2);
    let result = goblins.state();
    assert!(result.execution().window().entries().is_empty());
    assert_eq!(
        result
            .execution()
            .counters_of_kind(physical(result, 4).id(), CounterKind::PlusOne),
        1.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_dreadhorde_invasion_amasses_one_army_and_grants_lifelink_at_six_power() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let price = [(Colourless, 1), (Black, 1)];
    let mut cards = vec![hand("Dreadhorde Invasion")];
    for (owner, first) in [(A, 10), (B, 30)] {
        cards.extend(
            (first..first + 8)
                .map(|id| named("Forest", id, owner, ObjectDestination::LibraryBottom)),
        );
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entered = cast_named(&initial, &price);
    let upkeep = exile_hand::until(entered, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::Upkeep
    });
    let created = round(upkeep);
    let army = tokens(&created, A, 1)[0];
    let vocabulary = current_catalogue().1;
    let view = |state: &GameState| {
        state
            .execution()
            .current_characteristics(army)
            .unwrap()
            .unwrap()
    };
    let profile = view(created.state());
    assert_eq!(profile.current_colors, [Color::Black].into());
    assert_eq!(
        profile.current_creature_subtypes,
        ["Zombie", "Army"]
            .map(|kind| vocabulary.resolve_label(kind).unwrap())
            .into()
    );
    assert_eq!(created.state().execution().life_total(A), Some(&19.into()));
    assert_eq!(
        created
            .state()
            .execution()
            .counters_of_kind(army, CounterKind::PlusOne),
        1.into()
    );

    let army_attack = |state: &GameState| {
        let declaration = combat_frame(state, true);
        reply(
            &declaration,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: army,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state()
    };
    let low = next_main(created.into_state(), 5);
    let too_small = army_attack(&low);
    assert!(too_small.execution().window().entries().is_empty());
    assert!(!view(&too_small).has_keyword(Lifelink));

    let large = next_main(low, 13);
    assert_eq!(large.execution().life_total(A), Some(&14.into()));
    assert_eq!(
        large
            .execution()
            .counters_of_kind(army, CounterKind::PlusOne),
        6.into()
    );
    assert!(!view(&large).has_keyword(Lifelink));
    let gains = round(army_attack(&large)).into_state();
    assert!(view(&gains).has_keyword(Lifelink));
    let damage = exile_hand::until(gains, |state| {
        state.execution().life_total(B) == Some(&14.into())
    });
    assert_eq!(damage.execution().life_total(A), Some(&20.into()));
    let expired = next_main(damage, 14);
    assert!(!view(&expired).has_keyword(Lifelink));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_omen_of_the_sun_flashes_in_two_soldiers_then_sacrifices_to_scry() {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 1)];
    let omen = hand("Omen of the Sun");
    let ability = omen.program.activated[0].id;
    let initial = game(
        vec![A, B],
        vec![
            omen,
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named("Island", 3, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 4), (White, 2)],
        Step::Upkeep,
    );
    let entered = cast_named(&initial, &price);
    assert_eq!(entered.turn().unwrap().position().step, Step::Upkeep);
    let ordering = round(entered).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("simultaneous Soldiers");
    };
    let born = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    let vocabulary = current_catalogue().1;
    for soldier in tokens(&born, A, 2) {
        let view = born
            .state()
            .execution()
            .current_characteristics(soldier)
            .unwrap()
            .unwrap();
        assert_eq!(view.current_colors, [Color::White].into());
        assert_eq!(
            view.current_creature_subtypes,
            ["Human", "Soldier"]
                .map(|kind| vocabulary.resolve_label(kind).unwrap())
                .into()
        );
        assert_eq!(
            view.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
    }
    let state = born.state();
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    let activation = |payment| GameCommand::ActivateUntargetedAbility {
        source: physical(state, 1).id(),
        ability,
        payment: Some(spend(payment)),
    };
    illegal(state, A, activation(&[(Colourless, 1), (White, 1)]));
    let paid = command(state, A, activation(&price)).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    let scry = round(paid).into_state();
    let RuleFrame::LibraryTop(frame) = scry.pending_decision().unwrap().frame() else {
        panic!("scry 2");
    };
    let top = physical(&scry, 2).id();
    let second = physical(&scry, 3).id();
    assert_eq!(frame.domain_top_first(), [top, second]);
    let finished = reply(
        &scry,
        DecisionReply::LibraryTop(LibraryTopReply::Scry {
            top_first: vec![second],
            bottom_top_first: vec![top],
        }),
    )
    .into_state();
    let library = finished.execution().window().cards().library(A);
    assert_eq!(library.first(), Some(&second));
    assert_eq!(library.last(), Some(&top));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_alharu_has_partner_and_creates_spirits_only_for_an_allied_countered_nontoken() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let catalogue = current_catalogue().0;
    let oracle = |name: &str| {
        OracleId::try_from(
            catalogue
                .cards
                .iter()
                .find(|card| card.name == name)
                .unwrap()
                .oracle_id
                .clone()
                .unwrap(),
        )
        .unwrap()
    };
    let names = spellwake_contracts::names::NameRules::literal_names();
    let commanders = [
        oracle("Alharu, Solemn Ritualist"),
        oracle("Keleth, Sunmane Familiar"),
    ];
    let deck = crate::commander::prepare_deck(
        catalogue,
        &names,
        &commanders,
        &vec![oracle("Wastes"); 98],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(deck.commanders(), commanders);

    let price = [(Colourless, 4), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Alharu, Solemn Ritualist"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            creature(4, A, 2, 2, &[]),
            remove_spell(5, A),
            remove_spell(6, A),
            remove_spell(7, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let selection = |ids: &[u64]| {
        DecisionReply::TriggerPermanentTargets(
            ids.iter()
                .map(|&id| TargetRef::Object(physical(&entered, id).id()))
                .collect(),
        )
    };
    illegal(&entered, A, combat_declaration(&entered, selection(&[1])));
    let empty = round(reply(&entered, selection(&[])).into_state()).into_state();
    assert_eq!(
        empty
            .execution()
            .counters_of_kind(physical(&empty, 2).id(), CounterKind::PlusOne),
        0.into()
    );
    let marked = round(reply(&entered, selection(&[2, 3])).into_state()).into_state();
    for id in [2, 3] {
        assert_eq!(
            marked
                .execution()
                .counters_of_kind(physical(&marked, id).id(), CounterKind::PlusOne),
            1.into()
        );
    }
    let kill = |state: &GameState, spell, victim| {
        let target = TargetRef::Object(physical(state, victim).id());
        round(targeted(state, A, spell, target).into_state()).into_state()
    };
    let opposing = kill(&marked, 5, 3);
    assert!(opposing.execution().window().entries().is_empty());
    let unmarked = kill(&opposing, 6, 4);
    assert!(unmarked.execution().window().entries().is_empty());
    let result = round(kill(&unmarked, 7, 2));
    let spirit = tokens(&result, A, 1)[0];
    let view = result
        .state()
        .execution()
        .current_characteristics(spirit)
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(Flying));
    assert_eq!(view.current_colors, [Color::White].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Spirit").unwrap()].into()
    );
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
}
