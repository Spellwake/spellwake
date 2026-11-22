//! Instant-speed permissions and cast modifiers from source-bound permanents.
use super::*;
use spellwake_engine::characteristics::KeywordAbility::*;

fn untargeted_at(state: &GameState, number: u64, price: &[(ManaType, i64)]) -> GameCommand {
    GameCommand::CastUntargeted {
        spell: physical(state, number).id(),
        payment: (!price.is_empty()).then(|| spend(price)),
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_shimmer_myr_raff_capashen_and_skittering_cicada_flash_in_and_allow_exactly_their_printed_spell_classes_at_instant_speed()
 {
    for (name, price) in [
        ("Shimmer Myr", vec![(Colourless, 3)]),
        (
            "Raff Capashen, Ship's Mage",
            vec![(Colourless, 2), (White, 1), (Blue, 1)],
        ),
        ("Skittering Cicada", vec![(Colourless, 3)]),
    ] {
        let mut cards = draw_pile();
        cards.extend([
            named(name, 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::artifact(fixed_cost("{2}"), vec![]),
            ),
            named("Keleth, Sunmane Familiar", 4, A, ObjectDestination::Hand),
            named("The Binding of the Titans", 5, A, ObjectDestination::Hand),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{2}"), 1.into(), 3.into(), Default::default()),
            ),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{W}"), 1.into(), 1.into(), Default::default()),
            ),
        ]);
        let start = game(
            vec![A, B],
            cards,
            &[(Colourless, 10), (White, 3), (Green, 1), (Blue, 1)],
            Step::PrecombatMain,
        );
        let waiting = cast_untargeted(&start, 2, &[]).into_state();
        illegal(&waiting, A, untargeted_at(&waiting, 3, &[(Colourless, 2)]));
        let ready = round(cast_untargeted(&waiting, 1, &price).into_state()).into_state();
        let source = physical(&ready, 1).id();
        assert!(sheet(&ready, source).has_keyword(Flash));
        assert_eq!(
            sheet(&ready, source).has_keyword(Flying),
            name.starts_with("Raff")
        );
        for (n, cost, allowed) in [
            (3, vec![(Colourless, 2)], true),
            (
                4,
                vec![(Colourless, 1), (White, 1)],
                name.starts_with("Raff"),
            ),
            (
                5,
                vec![(Colourless, 1), (Green, 1)],
                name.starts_with("Raff"),
            ),
            (6, vec![(Colourless, 2)], name == "Skittering Cicada"),
            (7, vec![(White, 1)], false),
        ] {
            let cast = untargeted_at(&ready, n, &cost);
            if !allowed {
                illegal(&ready, A, cast);
                continue;
            }
            let stack = command(&ready, A, cast).into_state();
            assert_eq!(physical(&stack, n).zone(), ObjectZone::Stack);
            if name == "Skittering Cicada" {
                let buff = round(stack).into_state();
                assert_size(&buff, source, 4, 4);
                assert!(sheet(&buff, source).has_keyword(Trample));
                let next = exile_hand::until(buff, |s| {
                    s.turn().unwrap().position().number == 2
                        && s.turn().unwrap().position().step == Step::PrecombatMain
                });
                assert_size(&next, source, 2, 2);
                assert!(!sheet(&next, source).has_keyword(Trample));
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_thryx_flashes_in_and_reduces_and_protects_its_controllers_spells_at_mana_value_five() {
    let counter = free_instant(
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        Instruction::CounterSpell,
    );
    let mut cards = draw_pile();
    cards.extend([
        named("Thryx, the Sudden Storm", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
        seed(
            3,
            A,
            ObjectDestination::Hand,
            Program::artifact(fixed_cost("{4}"), vec![]),
        ),
        seed(
            4,
            A,
            ObjectDestination::Hand,
            Program::artifact(fixed_cost("{5}"), vec![]),
        ),
        seed(5, B, ObjectDestination::Hand, counter),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 8), (Blue, 2)],
        Step::PrecombatMain,
    );
    let waiting = cast_untargeted(&start, 2, &[]).into_state();
    let flashed = round(cast_untargeted(&waiting, 1, &[(Colourless, 3), (Blue, 2)]).into_state())
        .into_state();
    let source = sheet(&flashed, physical(&flashed, 1).id());
    assert!(source.has_keyword(Flash));
    assert!(source.has_keyword(Flying));
    let ready = round(flashed).into_state();
    illegal(&ready, A, untargeted_at(&ready, 3, &[(Colourless, 3)]));
    for (n, big) in [(3, false), (4, true)] {
        let cast = cast_untargeted(&ready, n, &[(Colourless, 4)]).into_state();
        let id = cast.execution().window().entries().last().unwrap().id();
        let other = pass(&cast).into_state();
        let after = round(targeted(&other, B, 5, TargetRef::Stack(id)).into_state()).into_state();
        if big {
            assert_eq!(physical(&after, n).zone(), ObjectZone::Stack);
            let resolved = round(after).into_state();
            assert_eq!(physical(&resolved, n).zone(), ObjectZone::Battlefield);
        } else {
            assert_eq!(physical(&after, n).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mizzix_awards_experience_only_for_instants_and_sorceries_above_the_threshold_and_discounts_later_spells()
 {
    let spell = |instant, price: &str| {
        let body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
            }],
        );
        if instant {
            Program::instant(fixed_cost(price).unwrap(), body)
        } else {
            Program::sorcery(fixed_cost(price).unwrap(), body)
        }
    };
    let mut cards = draw_pile();
    cards.extend([
        named("Mizzix of the Izmagnus", 1, A, ObjectDestination::Hand),
        seed(2, A, ObjectDestination::Hand, spell(true, "{1}")),
        seed(3, A, ObjectDestination::Hand, spell(false, "{1}")),
        seed(4, A, ObjectDestination::Hand, spell(false, "{2}")),
        seed(5, A, ObjectDestination::Hand, spell(true, "{2}{U}")),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            Program::artifact(fixed_cost("{3}"), vec![]),
        ),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 12), (Blue, 2), (Red, 1)],
        Step::PrecombatMain,
    );
    let mut state =
        round(cast_untargeted(&start, 1, &[(Colourless, 2), (Blue, 1), (Red, 1)]).into_state())
            .into_state();
    for (n, price, before, after) in [
        (2, vec![(Colourless, 1)], 0, 1),
        (3, vec![], 1, 1),
        (4, vec![(Colourless, 1)], 1, 2),
        (5, vec![(Blue, 1)], 2, 3),
    ] {
        assert_eq!(state.execution().experience_counters(A), before.into());
        let stack = cast_untargeted(&state, n, &price).into_state();
        assert_eq!(
            stack.execution().window().entries().len(),
            1 + usize::from(after > before)
        );
        let ability = if after > before {
            round(stack).into_state()
        } else {
            stack
        };
        assert_eq!(ability.execution().experience_counters(A), after.into());
        state = round(ability).into_state();
    }
    illegal(&state, A, untargeted_at(&state, 6, &[]));
    let normal = cast_untargeted(&state, 6, &[(Colourless, 3)]).into_state();
    assert_eq!(normal.execution().window().entries().len(), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_the_immortal_sun_locks_both_players_loyalty_reduces_only_its_casters_spell_price_and_anthems_and_draws_for_its_controller()
 {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput};
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 4.into()), (White, 2.into())].into()),
    });
    let mut cards = draw_pile();
    cards.extend((20..23).map(|n| {
        seed(
            n,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    cards.extend([
        named("The Immortal Sun", 1, A, ObjectDestination::Hand),
        named("Elspeth, Sun's Champion", 2, A, ObjectDestination::Hand),
        named("Elspeth, Sun's Champion", 3, B, ObjectDestination::Hand),
        creature(4, A, 2, 3, &[]),
        creature(5, B, 4, 3, &[]),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            Program::artifact(fixed_cost("{2}"), vec![]),
        ),
        seed(7, B, ObjectDestination::Battlefield { controller: B }, rock),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 11), (White, 2)],
        Step::PrecombatMain,
    );
    let walker =
        round(cast_untargeted(&start, 2, &[(Colourless, 4), (White, 2)]).into_state()).into_state();
    let enabled = command(&walker, A, activation(&walker, 2, 0, &[])).into_state();
    assert_eq!(enabled.execution().window().entries().len(), 1);
    let sun = round(cast_untargeted(&walker, 1, &[(Colourless, 6)]).into_state()).into_state();
    illegal(&sun, A, activation(&sun, 2, 0, &[]));
    assert_size(&sun, physical(&sun, 4).id(), 3, 4);
    assert_size(&sun, physical(&sun, 5).id(), 4, 3);
    let discount = round(cast_untargeted(&sun, 6, &[(Colourless, 1)]).into_state()).into_state();
    assert_eq!(discount.execution().mana(A, Colourless), 0.into());
    let before = hand_size(&discount, A);
    let theirs = exile_hand::until(discount, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(hand_size(&theirs, A), before);
    assert_eq!(hand_size(&theirs, B), 2);
    let funded = command(
        &theirs,
        B,
        GameCommand::ActivateMana {
            source: physical(&theirs, 7).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    illegal(
        &funded,
        B,
        untargeted_at(&funded, 3, &[(Colourless, 3), (White, 2)]),
    );
    let bought = round(
        command(
            &funded,
            B,
            untargeted_at(&funded, 3, &[(Colourless, 4), (White, 2)]),
        )
        .into_state(),
    )
    .into_state();
    illegal(&bought, B, activation(&bought, 3, 0, &[]));
    let ours = exile_hand::until(bought, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(hand_size(&ours, A), before + 2);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_inspiring_statuary_allows_only_nonartifact_spells_to_tap_own_untapped_artifacts_for_generic_mana()
 {
    let cards = vec![
        named("Inspiring Statuary", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Hand,
            Program::creature(fixed_cost("{2}{U}"), 1.into(), 1.into(), Default::default()),
        ),
        seed(
            3,
            A,
            ObjectDestination::Hand,
            Program::artifact(fixed_cost("{2}{U}"), vec![]),
        ),
        seed(
            4,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![]),
        ),
        seed(
            5,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::artifact(None, vec![]),
        ),
        seed(
            7,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![]),
        ),
    ];
    let opening = game(
        vec![A, B],
        cards,
        &[(Colourless, 3), (Blue, 1)],
        Step::PrecombatMain,
    );
    let field = round(cast_untargeted(&opening, 1, &[(Colourless, 3)]).into_state()).into_state();
    let apply = |n, ids: &[u64], mana| GameCommand::CastWithResources {
        spell: physical(&field, n).id(),
        target: None,
        payment: CastingPayment {
            mana,
            objects: ids
                .iter()
                .map(|id| CastResource::Improvise(physical(&field, *id).id()))
                .collect(),
            ..Default::default()
        },
    };
    illegal(&field, A, apply(3, &[1, 4], Some(spend(&[(Blue, 1)]))));
    illegal(&field, A, apply(2, &[1, 5], Some(spend(&[(Blue, 1)]))));
    illegal(&field, A, apply(2, &[1, 4, 7], None));
    let spell = command(&field, A, apply(2, &[1, 4], Some(spend(&[(Blue, 1)])))).into_state();
    for id in [1, 4] {
        assert_eq!(physical(&spell, id).tapped(), Some(true));
    }
    assert_eq!(spell.execution().mana(A, Blue), 0.into());
    let done = round(spell).into_state();
    assert_eq!(physical(&done, 2).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_grand_abolisher_prohibits_opposing_spells_and_artifact_creature_enchantment_activations_only_on_its_controllers_turn()
 {
    let gain = || {
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
            }],
        )
    };
    let ability = ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: gain(),
    };
    let mut creature = Program::creature(fixed_cost("{1}"), 1.into(), 1.into(), Default::default());
    creature.activated.push(ability.clone());
    let mut enchantment = Program::enchantment(fixed_cost("{1}"));
    enchantment.activated.push(ability.clone());
    let mut cards = draw_pile();
    cards.extend([
        named("Grand Abolisher", 1, A, ObjectDestination::Hand),
        seed(
            2,
            B,
            ObjectDestination::Hand,
            Program::instant(ManaCost::free(), gain()),
        ),
        seed(
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::artifact(None, vec![ability]),
        ),
        seed(
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature,
        ),
        seed(
            5,
            B,
            ObjectDestination::Battlefield { controller: B },
            enchantment,
        ),
        named(
            "Sol Ring",
            6,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        named(
            "Forest",
            7,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
    ]);
    let opening = game(vec![A, B], cards, &[(White, 2)], Step::PrecombatMain);
    let field = round(cast_untargeted(&opening, 1, &[(White, 2)]).into_state()).into_state();
    let other = pass(&field).into_state();
    let test = |s: &GameState, enabled| {
        let mut actions = vec![
            untargeted_at(s, 2, &[]),
            GameCommand::ActivateMana {
                source: physical(s, 6).id(),
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(
                    spellwake_engine::mana::ManaAbilityId(0),
                ),
            },
        ];
        actions.extend((3..=5).map(|id| activation(s, id, 0, &[])));
        for request in actions {
            if enabled {
                let _activated = command(s, B, request);
            } else {
                illegal(s, B, request);
            }
        }
        let land = command(
            s,
            B,
            GameCommand::ActivateMana {
                source: physical(s, 7).id(),
                ability: spellwake_engine::mana::ManaAbilityRef::BasicLand(
                    spellwake_engine::land::BasicLandType::Forest,
                ),
            },
        )
        .into_state();
        assert_eq!(land.execution().mana(B, Green), 1.into());
    };
    test(&other, false);
    let b = exile_hand::until(other, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    test(&b, true);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_destiny_spinner_protects_its_own_creature_and_enchantment_spells_and_turns_a_controlled_land_into_an_elemental_for_the_turn()
 {
    let counter = free_instant(
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        Instruction::CounterSpell,
    );
    let mut cards = draw_pile();
    cards.extend([
        named("Destiny Spinner", 1, A, ObjectDestination::Hand),
        named(
            "Forest",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Forest",
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        seed(
            4,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::free()),
                1.into(),
                1.into(),
                Default::default(),
            ),
        ),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::enchantment(Some(ManaCost::free())),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
        seed(7, B, ObjectDestination::Hand, counter),
        seed(
            8,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::enchantment(None),
        ),
        seed(
            9,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::enchantment(None),
        ),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 4), (Green, 2)],
        Step::PrecombatMain,
    );
    let field =
        round(cast_untargeted(&start, 1, &[(Colourless, 1), (Green, 1)]).into_state()).into_state();
    for (n, protected) in [(4, true), (5, true), (6, false)] {
        let cast = cast_untargeted(&field, n, &[]).into_state();
        let entry = cast.execution().window().entries().last().unwrap().id();
        let other = pass(&cast).into_state();
        let after =
            round(targeted(&other, B, 7, TargetRef::Stack(entry)).into_state()).into_state();
        assert_eq!(
            physical(&after, n).zone(),
            if protected {
                ObjectZone::Stack
            } else {
                ObjectZone::Graveyard(A)
            }
        );
    }
    let animate = |n| GameCommand::ActivateAbility {
        source: physical(&field, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&field, n).id()),
        payment: Some(spend(&[(Colourless, 3), (Green, 1)])),
    };
    illegal(&field, A, animate(3));
    let end = round(command(&field, A, animate(2)).into_state()).into_state();
    let id = physical(&end, 2).id();
    let animated = sheet(&end, id);
    assert_size(&end, id, 2, 2);
    assert!(animated.has_type(CardType::Land));
    assert!(animated.is_creature());
    assert!(
        animated
            .current_creature_subtypes
            .contains(&current_catalogue().1.resolve_label("Elemental").unwrap())
    );
    assert!(animated.has_keyword(Trample));
    assert!(animated.has_keyword(Haste));
    let expired = exile_hand::until(end, |s| s.turn().unwrap().position().number == 2);
    assert!(!sheet(&expired, id).is_creature());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_soul_of_new_phyrexia_grants_indestructible_to_its_controllers_permanents_from_the_battlefield_or_by_exiling_itself_from_the_graveyard()
 {
    let mut cards = draw_pile();
    cards.extend([
        named("Soul of New Phyrexia", 1, A, ObjectDestination::Hand),
        creature(2, A, 2, 2, &[]),
        named(
            "Forest",
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        creature(4, B, 2, 2, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
    ]);
    let start = game(vec![A, B], cards, &[(Colourless, 11)], Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &[(Colourless, 6)]).into_state()).into_state();
    assert!(sheet(&field, physical(&field, 1).id()).has_keyword(Trample));
    let yard =
        round(targeted(&field, A, 5, TargetRef::Object(physical(&field, 1).id())).into_state())
            .into_state();
    for (source, ability, wherefrom) in [
        (field, 0, ObjectZone::Battlefield),
        (yard, 1, ObjectZone::Exile),
    ] {
        let paid = command(
            &source,
            A,
            activation(&source, 1, ability, &[(Colourless, 5)]),
        )
        .into_state();
        assert_eq!(physical(&paid, 1).zone(), wherefrom);
        let ready = round(paid).into_state();
        for (n, ours) in [(2, true), (3, true), (4, false)] {
            assert_eq!(
                sheet(&ready, physical(&ready, n).id()).has_keyword(Indestructible),
                ours
            );
        }
        let failed =
            round(targeted(&ready, A, 6, TargetRef::Object(physical(&ready, 2).id())).into_state())
                .into_state();
        assert_eq!(physical(&failed, 2).zone(), ObjectZone::Battlefield);
        let expired = exile_hand::until(failed, |s| s.turn().unwrap().position().number == 2);
        assert!(!sheet(&expired, physical(&expired, 2).id()).has_keyword(Indestructible));
    }
}
