//! Card-specific Sliver permission and activation scenarios.
use super::*;

fn sliver(id: u64, owner: SeatId) -> SeedCard {
    let mut result = creature(id, owner, 2, 2, &[]);
    result
        .program
        .creature_subtypes
        .insert(current_catalogue().1.resolve_label("Sliver").unwrap());
    result
}

pub(super) fn sheet(
    state: &GameState,
    id: u64,
) -> spellwake_engine::characteristics::ObjectCharacteristics {
    state
        .execution()
        .current_characteristics(physical(state, id).id())
        .unwrap()
        .unwrap()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sliver_mana_grants_include_each_printed_color_and_the_correct_players() {
    let cost = [(ManaType::Colourless, 1), (ManaType::Green, 1)];
    for (name, opposing_access) in [("Gemhide Sliver", true), ("Manaweft Sliver", false)] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                sliver(2, A),
                sliver(3, B),
                creature(4, A, 2, 2, &[]),
                remove_spell(5, A),
            ],
            &cost,
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &cost);
        assert_eq!(sheet(&entered, 1).granted_any_color_mana.len(), 1);
        let own_key = *sheet(&entered, 2).granted_any_color_mana.first().unwrap();
        let opposing = sheet(&entered, 3).granted_any_color_mana;
        assert_eq!(opposing.contains(&own_key), opposing_access);
        assert!(sheet(&entered, 4).granted_any_color_mana.is_empty());
        let mana = |state: &GameState, id, key| GameCommand::ActivateMana {
            source: physical(state, id).id(),
            ability: ManaAbilityRef::GrantedAnyColor(key),
        };
        illegal(&entered, A, mana(&entered, 1, own_key));
        let after_source_left = round(
            targeted(
                &entered,
                A,
                5,
                TargetRef::Object(physical(&entered, 1).id()),
            )
            .into_state(),
        )
        .into_state();
        assert!(
            sheet(&after_source_left, 2)
                .granted_any_color_mana
                .is_empty()
        );

        let colors = [
            ManaType::White,
            ManaType::Blue,
            ManaType::Black,
            ManaType::Red,
            ManaType::Green,
        ];
        let check = |state: &GameState, id, owner| {
            let activated = command(state, owner, mana(state, id, own_key)).into_state();
            let RuleFrame::ManaColor(frame) = activated.pending_decision().unwrap().frame() else {
                panic!("printed any color");
            };
            assert_eq!(frame.colors(), &colors.into());
            for color in colors {
                let produced = reply(&activated, DecisionReply::ManaColor(color)).into_state();
                assert_eq!(produced.execution().mana(owner, color), 1.into());
                assert_eq!(physical(&produced, id).tapped(), Some(true));
                assert!(produced.execution().window().entries().is_empty());
            }
        };
        let opposing_main = next_main(entered, 2);
        if opposing_access {
            check(&opposing_main, 3, B);
        } else {
            illegal(&opposing_main, B, mana(&opposing_main, 3, own_key));
        }
        let own_main = next_main(opposing_main, 3);
        check(&own_main, 2, A);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_firewake_sliver_grants_both_abilities_and_pays_the_bearers_sacrifice() {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            hand("Firewake Sliver"),
            sliver(2, A),
            sliver(3, B),
            creature(4, B, 2, 2, &[]),
        ],
        &[(Colourless, 2), (Red, 1), (Green, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &[(Colourless, 1), (Red, 1), (Green, 1)]);
    for id in [1, 2, 3] {
        assert!(sheet(&entered, id).has_keyword(Haste));
        assert_eq!(sheet(&entered, id).granted_activated.len(), 1);
    }
    assert!(!sheet(&entered, 4).has_keyword(Haste));
    assert!(sheet(&entered, 4).granted_activated.is_empty());
    let granted = *sheet(&entered, 2)
        .granted_activated
        .first_key_value()
        .unwrap()
        .0;
    let activate = |id| GameCommand::ActivateGranted {
        source: physical(&entered, 2).id(),
        effect: granted,
        target: Some(TargetRef::Object(physical(&entered, id).id())),
        payment: Some(spend(&[(Colourless, 1)])),
    };
    illegal(&entered, A, activate(4));
    let paid = command(&entered, A, activate(3)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    let resolved = round(paid).into_state();
    let boosted = |state: &GameState| sheet(state, 3).power_toughness.unwrap().derived;
    assert_eq!(
        boosted(&resolved),
        PowerToughness {
            power: 4.into(),
            toughness: 4.into()
        }
    );
    assert_eq!(
        boosted(&next_main(resolved, 2)),
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hibernation_sliver_offers_a_paid_return_to_the_bearers_owner() {
    use ManaType::*;
    let mut borrowed = sliver(2, B);
    borrowed.owner = A;
    let initial = game(
        vec![A, B],
        vec![
            hand("Hibernation Sliver"),
            borrowed,
            sliver(3, A),
            creature(4, A, 2, 2, &[]),
        ],
        &[(Blue, 1), (Black, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &[(Blue, 1), (Black, 1)]);
    for id in [1, 2, 3] {
        assert_eq!(sheet(&entered, id).granted_activated.len(), 1);
    }
    assert!(sheet(&entered, 4).granted_activated.is_empty());
    let available = pass(&entered).into_state();
    let granted = *sheet(&available, 2)
        .granted_activated
        .first_key_value()
        .unwrap()
        .0;
    let paid = command(
        &available,
        B,
        GameCommand::ActivateGranted {
            source: physical(&available, 2).id(),
            effect: granted,
            target: None,
            payment: None,
        },
    )
    .into_state();
    assert_eq!(paid.execution().life_total(B), Some(&18.into()));
    assert_eq!(paid.execution().life_total(A), Some(&20.into()));
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Battlefield);
    let returned = round(paid).into_state();
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&returned, 1).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hatchery_sliver_copies_its_own_spell_and_grants_a_new_sliver_its_printed_price() {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            hand("Hatchery Sliver"),
            named("Gemhide Sliver", 2, A, ObjectDestination::Hand),
            named("Fencing Ace", 3, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 6), (Green, 5), (White, 1)],
        Step::PrecombatMain,
    );
    let cast = |state: &GameState, id, repeats, costs: &[(ManaType, i64)]| {
        GameCommand::CastWithResources {
            spell: physical(state, id).id(),
            target: None,
            payment: CastingPayment {
                additional: [(0, repeats)].into(),
                mana: Some(spend(costs)),
                ..Default::default()
            },
        }
    };
    let first = command(
        &initial,
        A,
        cast(&initial, 1, 2.into(), &[(Colourless, 3), (Green, 3)]),
    )
    .into_state();
    let mut game = round(first).into_state();
    assert_eq!(game.execution().window().spell_copies().len(), 2);
    for _ in 0..3 {
        game = round(game).into_state();
    }
    assert_eq!(physical(&game, 1).zone(), ObjectZone::Battlefield);
    let tokens = |state: &GameState| {
        state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|obj| obj.is_token() && obj.zone() == ObjectZone::Battlefield)
            .count()
    };
    assert_eq!(tokens(&game), 2);

    illegal(
        &game,
        A,
        cast(&game, 3, 1.into(), &[(Colourless, 1), (White, 1)]),
    );
    let paid = command(
        &game,
        A,
        cast(&game, 2, 1.into(), &[(Colourless, 2), (Green, 2)]),
    )
    .into_state();
    let copied = round(paid).into_state();
    assert_eq!(copied.execution().window().spell_copies().len(), 1);
    let token_in_play = round(copied).into_state();
    assert_eq!(tokens(&token_in_play), 3);
    assert_eq!(physical(&token_in_play, 2).zone(), ObjectZone::Stack);
    let resolved = round(token_in_play).into_state();
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(tokens(&resolved), 3);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_clot_and_crypt_offer_regeneration_with_the_printed_costs_and_subjects() {
    use ManaType::*;
    let cast_cost = [(Colourless, 1), (Black, 1)];
    for (name, subject, tap_cost) in [("Clot Sliver", 2, false), ("Crypt Sliver", 3, true)] {
        let mut bearer = sliver(2, A);
        bearer.program.printed_keywords.insert(Haste);
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                bearer,
                sliver(3, B),
                creature(4, B, 2, 2, &[]),
                remove_spell(5, A),
                remove_spell(6, A),
            ],
            &[(Colourless, 3), (Black, 1)],
            Step::PrecombatMain,
        );
        let lord = cast_named(&initial, &cast_cost);
        let key = *sheet(&lord, 2)
            .granted_activated
            .first_key_value()
            .unwrap()
            .0;
        assert!(sheet(&lord, 3).granted_activated.contains_key(&key));
        assert!(sheet(&lord, 4).granted_activated.is_empty());
        let grant = |target| GameCommand::ActivateGranted {
            source: physical(&lord, 2).id(),
            effect: key,
            target,
            payment: (!tap_cost).then(|| spend(&[(Colourless, 2)])),
        };
        let selected = tap_cost.then(|| TargetRef::Object(physical(&lord, subject).id()));
        if tap_cost {
            illegal(
                &lord,
                A,
                grant(Some(TargetRef::Object(physical(&lord, 4).id()))),
            );
        } else {
            illegal(
                &lord,
                A,
                grant(Some(TargetRef::Object(physical(&lord, 3).id()))),
            );
        }
        let paid = command(&lord, A, grant(selected)).into_state();
        assert_eq!(physical(&paid, 2).tapped(), Some(tap_cost));
        assert_eq!(
            paid.execution().mana(A, Colourless),
            if tap_cost { 2.into() } else { 0.into() }
        );
        let ready = round(paid).into_state();
        let protected = physical(&ready, subject).id();
        assert!(
            ready
                .execution()
                .regeneration_shields()
                .contains_key(&protected)
        );
        let kill = |state: &GameState, spell| {
            round(targeted(state, A, spell, TargetRef::Object(protected)).into_state()).into_state()
        };
        let survived = kill(&ready, 5);
        assert_eq!(physical(&survived, subject).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&survived, subject).tapped(), Some(true));
        assert!(
            !survived
                .execution()
                .regeneration_shields()
                .contains_key(&protected)
        );
        let died = kill(&survived, 6);
        assert_eq!(
            physical(&died, subject).zone(),
            ObjectZone::Graveyard(if tap_cost { B } else { A })
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_brood_and_synapse_slivers_offer_the_reward_to_the_damaging_slivers_controller() {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice::{Decline, Take};
    for (name, color, create) in [
        ("Brood Sliver", Green, true),
        ("Synapse Sliver", Blue, false),
    ] {
        let price = [(Colourless, 4), (color, 1)];
        let mut attacker = sliver(2, A);
        attacker.program.printed_keywords.insert(Haste);
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                attacker,
                sliver(3, B),
                creature(4, A, 2, 2, &[Haste]),
                named("Forest", 5, A, ObjectDestination::LibraryBottom),
                named("Forest", 6, B, ObjectDestination::LibraryBottom),
                named("Island", 7, B, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let on_field = cast_named(&initial, &price);
        let hit = |state: &GameState, attacking_card, defender| {
            let attacked = attack(state, attacking_card, defender);
            let blocker = blockers(&attacked);
            let open = reply(&blocker, DecisionReply::DeclareBlockers(vec![])).into_state();
            round(open).into_state()
        };
        let no_reward = hit(&on_field, 4, B);
        assert!(no_reward.execution().window().entries().is_empty());

        let own = hit(&on_field, 2, B);
        let choice = round(own).into_state();
        assert_eq!(choice.pending_decision().unwrap().chooser(), A);
        let declined = reply(&choice, DecisionReply::OptionalEffect(Decline)).into_state();
        assert_eq!(physical(&declined, 5).zone(), ObjectZone::Library(A));
        assert!(
            !declined
                .execution()
                .window()
                .cards()
                .iter()
                .any(|o| o.is_token())
        );
        let accepted = reply(&choice, DecisionReply::OptionalEffect(Take));
        if create {
            let token = tokens(&accepted, A, 1)[0];
            let view = accepted
                .state()
                .execution()
                .current_characteristics(token)
                .unwrap()
                .unwrap();
            assert_eq!(
                view.current_creature_subtypes,
                [current_catalogue().1.resolve_label("Sliver").unwrap()].into()
            );
            assert!(view.current_colors.is_empty());
            assert_eq!(
                view.power_toughness.unwrap().derived,
                PowerToughness {
                    power: 1.into(),
                    toughness: 1.into()
                }
            );
        } else {
            assert_eq!(physical(accepted.state(), 5).zone(), ObjectZone::Hand(A));
        }

        let opposing = next_main(declined, 2);
        let dealt = hit(&opposing, 3, A);
        let offer = round(dealt).into_state();
        assert_eq!(offer.pending_decision().unwrap().chooser(), B);
        let collected = reply(&offer, DecisionReply::OptionalEffect(Take));
        if create {
            tokens(&collected, B, 1);
        } else {
            assert_eq!(physical(collected.state(), 7).zone(), ObjectZone::Hand(B));
            assert_eq!(
                physical(collected.state(), 5).zone(),
                ObjectZone::Library(A)
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_harmonic_sliver_gives_both_players_the_mandatory_destroy_trigger() {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1), (White, 1)];
    let opponent = free_sliver_in_hand(2, B);
    let initial = game(
        vec![A, B],
        vec![
            hand("Harmonic Sliver"),
            opponent,
            creature(3, B, 2, 2, &[]),
            named(
                "Mind Stone",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::enchantment(None),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = cast_named(&initial, &price);
    let choose = |state: &GameState, id| {
        DecisionReply::TriggerTarget(TargetRef::Object(physical(state, id).id()))
    };
    illegal(&cast, A, combat_declaration(&cast, choose(&cast, 3)));
    let picked = reply(&cast, choose(&cast, 4)).into_state();
    let destroyed = round(picked).into_state();
    assert_eq!(physical(&destroyed, 4).zone(), ObjectZone::Graveyard(B));

    let their_turn = next_main(destroyed, 2);
    let cast = GameCommand::CastUntargeted {
        spell: physical(&their_turn, 2).id(),
        payment: None,
    };
    let offered = round(command(&their_turn, B, cast).into_state()).into_state();
    assert_eq!(offered.pending_decision().unwrap().chooser(), B);
    let chosen = reply(&offered, choose(&offered, 5)).into_state();
    let removed = round(chosen).into_state();
    assert_eq!(physical(&removed, 5).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_lavabelly_sliver_grants_an_entry_ping_and_life_to_own_slivers_only() {
    use ManaType::*;
    let cast_cost = [(Colourless, 1), (Red, 1), (White, 1)];
    let opponent = free_sliver_in_hand(2, B);
    let initial = game(
        vec![A, B],
        vec![
            hand("Lavabelly Sliver"),
            opponent,
            creature(3, B, 2, 2, &[]),
            named("Jace Beleren", 4, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 2), (Red, 1), (White, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let walker = round(cast_untargeted(&initial, 4, &[(Colourless, 1), (Blue, 2)]).into_state())
        .into_state();
    let entered = cast_named(&walker, &cast_cost);
    let target_creature =
        DecisionReply::TriggerTarget(TargetRef::Object(physical(&entered, 3).id()));
    illegal(&entered, A, combat_declaration(&entered, target_creature));
    for target in [
        TargetRef::Player(B),
        TargetRef::Object(physical(&entered, 4).id()),
    ] {
        let chosen = reply(&entered, DecisionReply::TriggerTarget(target)).into_state();
        let pinged = round(chosen).into_state();
        assert_eq!(pinged.execution().life_total(A), Some(&21.into()));
        let player = target == TargetRef::Player(B);
        assert_eq!(
            pinged.execution().life_total(B),
            Some(&(if player { 19_i64 } else { 20 }).into())
        );
        assert_eq!(
            pinged
                .execution()
                .loyalty_counters()
                .get(&physical(&pinged, 4).id()),
            Some(&(if player { 3_i64 } else { 2 }).into())
        );
    }
    let pinged =
        round(reply(&entered, DecisionReply::TriggerTarget(TargetRef::Player(B))).into_state())
            .into_state();
    let opposing_turn = next_main(pinged, 2);
    let their_cast = GameCommand::CastUntargeted {
        spell: physical(&opposing_turn, 2).id(),
        payment: None,
    };
    let untargeted = round(command(&opposing_turn, B, their_cast).into_state()).into_state();
    assert!(untargeted.pending_decision().is_none());
    assert!(untargeted.execution().window().entries().is_empty());
    assert_eq!(untargeted.execution().life_total(B), Some(&19.into()));
}

fn free_sliver_in_hand(id: u64, owner: SeatId) -> SeedCard {
    let mut card = sliver(id, owner);
    card.destination = ObjectDestination::Hand;
    let ProgramKind::Permanent(definition) = &mut card.program.kind else {
        unreachable!("the fixture is a creature");
    };
    definition.casting_cost = Some(ManaCost::free());
    card
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_constricting_sliver_optionally_exiles_a_creature_until_the_actual_sliver_leaves() {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice::{Decline, Take};
    let price = [(Colourless, 5), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Constricting Sliver"),
            creature(2, B, 2, 2, &[]),
            creature(3, A, 2, 2, &[]),
            remove_spell(4, A),
            creature(5, B, 2, 2, &[]),
            free_sliver_in_hand(6, A),
            remove_spell(7, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let offered = cast_named(&initial, &price);
    let select = |id| DecisionReply::TriggerTarget(TargetRef::Object(physical(&offered, id).id()));
    illegal(&offered, A, combat_declaration(&offered, select(3)));
    let chosen = reply(&offered, select(2)).into_state();
    let resolving = round(chosen).into_state();
    let declined = reply(&resolving, DecisionReply::OptionalEffect(Decline)).into_state();
    assert_eq!(physical(&declined, 2).zone(), ObjectZone::Battlefield);
    let exiled = reply(&resolving, DecisionReply::OptionalEffect(Take)).into_state();
    assert_eq!(physical(&exiled, 2).zone(), ObjectZone::Exile);
    let second = round(cast_untargeted(&exiled, 6, &[]).into_state()).into_state();
    let selected = reply(
        &second,
        DecisionReply::TriggerTarget(TargetRef::Object(physical(&second, 5).id())),
    )
    .into_state();
    let selected = round(selected).into_state();
    let exiled = reply(&selected, DecisionReply::OptionalEffect(Take)).into_state();
    let victim = physical(&exiled, 2).id();
    let lord = physical(&exiled, 1).id();
    let moved = round(targeted(&exiled, A, 4, TargetRef::Object(lord)).into_state()).into_state();
    assert_eq!(physical(&moved, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&moved, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&moved, 2).controller(), Some(B));
    assert_ne!(physical(&moved, 2).id(), victim);
    assert_eq!(physical(&moved, 5).zone(), ObjectZone::Exile);
    let other = TargetRef::Object(physical(&moved, 6).id());
    let returned = round(targeted(&moved, A, 7, other).into_state()).into_state();
    assert_eq!(physical(&returned, 5).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&returned, 5).controller(), Some(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spiteful_sliver_uses_actual_damage_to_the_sliver_even_when_it_dies() {
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Spiteful Sliver"),
            sliver(2, A),
            sliver(3, B),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(3),
                        false,
                    ),
                ),
            ),
            named("Jace Beleren", 5, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 3), (Red, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let walker = round(cast_untargeted(&initial, 5, &[(Colourless, 1), (Blue, 2)]).into_state())
        .into_state();
    let entered = cast_named(&walker, &price);
    let targeted_sliver = physical(&entered, 2).id();
    let hit = round(targeted(&entered, A, 4, TargetRef::Object(targeted_sliver)).into_state())
        .into_state();
    assert_eq!(physical(&hit, 2).zone(), ObjectZone::Graveyard(A));
    let wrong_target = DecisionReply::TriggerTarget(TargetRef::Object(physical(&hit, 3).id()));
    illegal(&hit, A, combat_declaration(&hit, wrong_target));
    let back = reply(&hit, DecisionReply::TriggerTarget(TargetRef::Player(B))).into_state();
    let again = round(back).into_state();
    assert_eq!(again.execution().life_total(B), Some(&17.into()));
    let jace = TargetRef::Object(physical(&hit, 5).id());
    let back = reply(&hit, DecisionReply::TriggerTarget(jace)).into_state();
    let removed = round(back).into_state();
    assert_eq!(physical(&removed, 5).zone(), ObjectZone::Graveyard(A));

    let opposing = TargetRef::Object(physical(&entered, 3).id());
    let no_trigger = round(targeted(&entered, A, 4, opposing).into_state()).into_state();
    assert!(no_trigger.pending_decision().is_none());
    assert!(no_trigger.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_regal_sliver_becomes_monarch_then_buffs_its_controllers_slivers_on_later_entries() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Regal Sliver"),
            free_sliver_in_hand(2, A),
            sliver(3, B),
            creature(4, A, 2, 2, &[]),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let settling = cast_named(&initial, &price);
    let crown = round(settling).into_state();
    assert_eq!(crown.execution().monarch(), Some(A));
    assert_eq!(
        sheet(&crown, 1).power_toughness.unwrap().derived.power,
        3.into()
    );
    let next = round(cast_untargeted(&crown, 2, &[]).into_state()).into_state();
    let pumped = round(next).into_state();
    for (id, power) in [(1, 4), (2, 3), (3, 2), (4, 2)] {
        assert_eq!(
            sheet(&pumped, id).power_toughness.unwrap().derived.power,
            power.into()
        );
    }
    let later = next_main(pumped, 2);
    assert_eq!(
        sheet(&later, 1).power_toughness.unwrap().derived.power,
        3.into()
    );
    assert_eq!(
        sheet(&later, 2).power_toughness.unwrap().derived.power,
        2.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_quick_sliver_casts_off_turn_and_grants_the_permission_to_an_opponent() {
    use ManaType::*;
    use spellwake_engine::land::BasicLandType::{Forest, Island, Plains};
    let land = |id, owner, name| {
        named(
            name,
            id,
            owner,
            ObjectDestination::Battlefield { controller: owner },
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Quick Sliver"),
            named("Gemhide Sliver", 2, B, ObjectDestination::Hand),
            named("Fencing Ace", 3, A, ObjectDestination::Hand),
            land(4, A, "Forest"),
            land(5, A, "Plains"),
            land(6, B, "Forest"),
            land(7, B, "Island"),
        ],
        &[],
        Step::PrecombatMain,
    );
    let their_main = next_main(initial, 2);
    let mut after_pass = pass(&their_main).into_state();
    let tap = |state: &GameState, id, kind, actor| {
        command(
            state,
            actor,
            GameCommand::ActivateMana {
                source: physical(state, id).id(),
                ability: ManaAbilityRef::BasicLand(kind),
            },
        )
        .into_state()
    };
    after_pass = tap(&after_pass, 4, Forest, A);
    after_pass = tap(&after_pass, 5, Plains, A);
    let no_flash = GameCommand::CastUntargeted {
        spell: physical(&after_pass, 3).id(),
        payment: Some(spend(&[(Green, 1), (White, 1)])),
    };
    illegal(&after_pass, A, no_flash);
    let entranced =
        round(cast_untargeted(&after_pass, 1, &[(Green, 1), (White, 1)]).into_state()).into_state();
    assert_eq!(physical(&entranced, 1).zone(), ObjectZone::Battlefield);
    assert!(sheet(&entranced, 1).has_keyword(Flash));
    assert_eq!(entranced.execution().window().active_player(), B);

    let our_main = next_main(entranced, 3);
    let mut opposing_priority = pass(&our_main).into_state();
    opposing_priority = tap(&opposing_priority, 6, Forest, B);
    opposing_priority = tap(&opposing_priority, 7, Island, B);
    let opponent_cast = GameCommand::CastUntargeted {
        spell: physical(&opposing_priority, 2).id(),
        payment: Some(spend(&[(Green, 1), (Blue, 1)])),
    };
    let resolved = round(command(&opposing_priority, B, opponent_cast).into_state()).into_state();
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(resolved.execution().window().active_player(), A);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_taunting_sliver_goads_only_an_opposing_creature_on_its_controllers_sliver_entry() {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Taunting Sliver"),
            creature(2, B, 2, 2, &[]),
            creature(3, A, 2, 2, &[]),
            free_sliver_in_hand(4, B),
        ],
        &price,
        Step::PrecombatMain,
    );
    let offered = cast_named(&initial, &price);
    let choice = |id| DecisionReply::TriggerTarget(TargetRef::Object(physical(&offered, id).id()));
    illegal(&offered, A, combat_declaration(&offered, choice(3)));
    let goaded = round(reply(&offered, choice(2)).into_state()).into_state();
    let next = next_main(goaded, 2);
    let play = GameCommand::CastUntargeted {
        spell: physical(&next, 4).id(),
        payment: None,
    };
    let foreign = round(command(&next, B, play).into_state()).into_state();
    assert!(foreign.pending_decision().is_none());
    assert!(foreign.execution().window().entries().is_empty());

    let attackers = combat_frame(&foreign, true);
    let declare = |opponent| {
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attackers, 2).id(),
            defender: opponent,
            defending_planeswalker: None,
        }])
    };
    illegal(
        &attackers,
        B,
        combat_declaration(&attackers, DecisionReply::DeclareAttackers(vec![])),
    );
    illegal(&attackers, B, combat_declaration(&attackers, declare(A)));
    reply(&attackers, declare(C));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_necrotic_sliver_pays_the_bearers_sacrifice_then_destroys_any_legal_permanent() {
    use ManaType::*;
    let price = [(Colourless, 1), (White, 1), (Black, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Necrotic Sliver"),
            sliver(2, A),
            sliver(3, B),
            named(
                "Forest",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &[(Colourless, 4), (White, 1), (Black, 1)],
        Step::PrecombatMain,
    );
    let settled = cast_named(&initial, &price);
    let grant = *sheet(&settled, 2)
        .granted_activated
        .first_key_value()
        .unwrap()
        .0;
    assert!(sheet(&settled, 3).granted_activated.contains_key(&grant));
    let sacrifice = |bearer| GameCommand::ActivateGranted {
        source: physical(&settled, bearer).id(),
        effect: grant,
        target: Some(TargetRef::Object(physical(&settled, 4).id())),
        payment: Some(spend(&[(Colourless, 3)])),
    };
    illegal(&settled, A, sacrifice(3));
    let paid = command(&settled, A, sacrifice(2)).into_state();
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&paid, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    let resolved = round(paid).into_state();
    assert_eq!(physical(&resolved, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hollowhead_sliver_taps_the_bearer_and_discards_before_drawing() {
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 1)];
    let mut bearer = sliver(2, A);
    bearer.program.printed_keywords.insert(Haste);
    let initial = game(
        vec![A, B],
        vec![
            hand("Hollowhead Sliver"),
            bearer,
            sliver(3, B),
            named("Forest", 4, A, ObjectDestination::Hand),
            named("Forest", 5, B, ObjectDestination::Hand),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let sliver = cast_named(&initial, &price);
    assert!(sheet(&sliver, 3).granted_activated.is_empty());
    let (effect, ability) = sheet(&sliver, 2)
        .granted_activated
        .into_iter()
        .next()
        .unwrap();
    let loot = |discard: u64| GameCommand::ActivateWithCostObjects {
        source: physical(&sliver, 2).id(),
        ability: ability.id,
        granting_effect: Some(effect),
        target: None,
        objects: vec![physical(&sliver, discard).id()],
        payment: None,
    };
    illegal(&sliver, A, loot(5));
    let paid = command(&sliver, A, loot(4)).into_state();
    assert_eq!(physical(&paid, 2).tapped(), Some(true));
    assert_eq!(physical(&paid, 4).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&paid, 6).zone(), ObjectZone::Library(A));
    let drew = round(paid).into_state();
    assert_eq!(physical(&drew, 6).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_diffusion_sliver_taxes_the_opposing_spell_controller_and_accepts_its_exact_payment() {
    use ManaType::*;
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        effects::ResolutionPaymentReply,
        land::BasicLandType,
    };
    let mut source = Program::artifact(None, vec![]);
    source.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(3),
                    false,
                ),
            ],
        ),
    });
    let initial = game(
        vec![A, B],
        vec![
            hand("Diffusion Sliver"),
            sliver(2, A),
            sliver(3, B),
            seed(
                4,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(3),
                        false,
                    ),
                ),
            ),
            named(
                "Forest",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Island",
                6,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                7,
                B,
                ObjectDestination::Battlefield { controller: B },
                source,
            ),
        ],
        &[(Colourless, 1), (Blue, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &[(Colourless, 1), (Blue, 1)]);
    let mut ready = pass(&entered).into_state();
    for (land, basic) in [(5, BasicLandType::Forest), (6, BasicLandType::Island)] {
        ready = command(
            &ready,
            B,
            GameCommand::ActivateMana {
                source: physical(&ready, land).id(),
                ability: ManaAbilityRef::BasicLand(basic),
            },
        )
        .into_state();
    }
    let aim =
        |id| targeted(&ready, B, 4, TargetRef::Object(physical(&ready, id).id())).into_state();
    let safe = aim(3);
    assert_eq!(safe.execution().window().entries().len(), 1);
    let demanded = aim(2);
    assert_eq!(demanded.execution().window().entries().len(), 2);
    let quote = round(demanded).into_state();
    assert_eq!(quote.pending_decision().unwrap().chooser(), B);
    let pay = |mana: &[(ManaType, i64)]| {
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(mana)))
    };
    illegal(&quote, B, combat_declaration(&quote, pay(&[(Green, 1)])));
    let countered = reply(
        &quote,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    assert_eq!(physical(&countered, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&countered, 2).zone(), ObjectZone::Battlefield);
    let funded = reply(&quote, pay(&[(Green, 1), (Blue, 1)])).into_state();
    let hit = round(funded).into_state();
    assert_eq!(physical(&hit, 2).zone(), ObjectZone::Graveyard(A));

    let activated = command(
        &ready,
        B,
        GameCommand::ActivateAbility {
            source: physical(&ready, 7).id(),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(physical(&ready, 2).id()),
            payment: None,
        },
    )
    .into_state();
    assert_eq!(activated.execution().window().entries().len(), 2);
    let fee = round(activated).into_state();
    assert_eq!(fee.pending_decision().unwrap().chooser(), B);
    let countered = reply(
        &fee,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    assert!(countered.execution().window().entries().is_empty());
    assert_eq!(physical(&countered, 7).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&countered, 2).zone(), ObjectZone::Battlefield);
    let paid = reply(&fee, pay(&[(Green, 1), (Blue, 1)])).into_state();
    assert_eq!(
        physical(&round(paid).into_state(), 2).zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_capricious_sliver_grants_playing_an_exiled_land_to_a_damaging_sliver_for_one_turn() {
    use ManaType::*;
    let price = [(Colourless, 3), (Red, 1)];
    let mut attacker = sliver(2, A);
    attacker.program.printed_keywords.insert(Haste);
    let initial = game(
        vec![A, B],
        vec![
            hand("Capricious Sliver"),
            attacker,
            named("Forest", 3, A, ObjectDestination::LibraryTop),
            named("Island", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let melee = attack(&entered, 2, B);
    let after_damage = exile_hand::until(melee, |state| {
        state.execution().life_total(B) == Some(&18.into())
    });
    assert_eq!(physical(&after_damage, 3).zone(), ObjectZone::Library(A));
    assert_eq!(after_damage.execution().window().entries().len(), 1);
    let granted = round(after_damage).into_state();
    assert_eq!(physical(&granted, 3).zone(), ObjectZone::Exile);
    let play = |state: &GameState| GameCommand::PlayLand {
        land: physical(state, 3).id(),
    };
    illegal(&granted, A, play(&granted));
    let postcombat = exile_hand::until(granted, |state| {
        state.turn().unwrap().position().step == Step::PostcombatMain
    });
    let played = command(&postcombat, A, play(&postcombat)).into_state();
    assert_eq!(physical(&played, 3).zone(), ObjectZone::Battlefield);
    let expired = next_main(postcombat, 3);
    assert_eq!(physical(&expired, 3).zone(), ObjectZone::Exile);
    illegal(&expired, A, play(&expired));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_lazotep_sliver_grants_afflict_and_amasses_after_an_allied_nontoken_sliver_dies() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let price = [(Colourless, 3), (Black, 1)];
    let mut attacking_sliver = sliver(2, A);
    attacking_sliver.program.printed_keywords.insert(Haste);
    let initial = game(
        vec![A, B],
        vec![
            hand("Lazotep Sliver"),
            attacking_sliver,
            sliver(3, B),
            creature(4, B, 1, 4, &[]),
            remove_spell(5, A),
            remove_spell(6, A),
            remove_spell(7, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let block = blockers(&attack(&entered, 2, B));
    let blocking = reply(&block, block_choice(&block, 4, 2)).into_state();
    assert_eq!(blocking.execution().life_total(B), Some(&20.into()));
    let afflicted = round(blocking).into_state();
    assert_eq!(afflicted.execution().life_total(B), Some(&18.into()));

    let kill = |state: &GameState, spell, victim| {
        round(targeted(state, A, spell, TargetRef::Object(victim)).into_state()).into_state()
    };
    let opposing = kill(&entered, 5, physical(&entered, 3).id());
    assert!(opposing.execution().window().entries().is_empty());
    let sacrificed = kill(&opposing, 6, physical(&opposing, 2).id());
    let born = round(sacrificed);
    let army = tokens(&born, A, 1)[0];
    let view = born
        .state()
        .execution()
        .current_characteristics(army)
        .unwrap()
        .unwrap();
    let vocabulary = current_catalogue().1;
    assert_eq!(view.current_colors, [Color::Black].into());
    assert_eq!(
        view.current_creature_subtypes,
        ["Army", "Sliver"]
            .map(|kind| vocabulary.resolve_label(kind).unwrap())
            .into()
    );
    assert_eq!(
        born.state()
            .execution()
            .counters_of_kind(army, CounterKind::PlusOne),
        2.into()
    );
    let token_died = kill(born.state(), 7, army);
    assert!(token_died.execution().window().entries().is_empty());
}
