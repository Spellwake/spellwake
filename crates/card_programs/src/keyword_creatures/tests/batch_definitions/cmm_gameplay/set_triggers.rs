//! Named triggers whose setup shares the ordinary paid cast and resolution path.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_myr_sire_dies_into_the_printed_phyrexian_myr_artifact_creature() {
    let price = [(ManaType::Colourless, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Myr Sire"),
            remove_spell(2, A),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let sire = TargetRef::Object(physical(&entered, 1).id());
    let exiled = round(targeted(&entered, A, 3, sire).into_state()).into_state();
    assert_eq!(physical(&exiled, 1).zone(), ObjectZone::Exile);
    assert!(exiled.execution().window().entries().is_empty());
    let killed = round(targeted(&entered, A, 2, sire).into_state()).into_state();
    assert_eq!(physical(&killed, 1).zone(), ObjectZone::Graveyard(A));
    let made = round(killed);
    let myr = tokens(&made, A, 1)[0];
    let view = made
        .state()
        .execution()
        .current_characteristics(myr)
        .unwrap()
        .unwrap();
    assert_eq!(
        view.current_types,
        [CardType::Artifact, CardType::Creature].into()
    );
    let vocabulary = current_catalogue().1;
    assert_eq!(
        view.current_creature_subtypes,
        [
            vocabulary.resolve_label("Phyrexian").unwrap(),
            vocabulary.resolve_label("Myr").unwrap(),
        ]
        .into()
    );
    assert!(view.current_colors.is_empty());
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_serrated_scorpion_deals_damage_to_each_opponent_and_gains_two_life() {
    let price = [(ManaType::Black, 1)];
    let initial = game(
        vec![A, B, C],
        vec![hand("Serrated Scorpion"), remove_spell(2, A)],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let victim = TargetRef::Object(physical(&entered, 1).id());
    let dead = round(targeted(&entered, A, 2, victim).into_state()).into_state();
    assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(dead.execution().life_total(A), Some(&20.into()));
    let result = round(dead);
    assert_eq!(result.state().execution().life_total(A), Some(&22.into()));
    for opponent in [B, C] {
        assert_eq!(
            result.state().execution().life_total(opponent),
            Some(&18.into())
        );
    }
    let damages = events(&result)
        .filter_map(|event| {
            let count = event
                .occurrences
                .iter()
                .filter(|row| matches!(row.value, RulesOccurrence::DamageDealt { .. }))
                .count();
            (count > 0).then_some(count)
        })
        .collect::<Vec<_>>();
    assert_eq!(damages, [2]);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rot_shambler_grows_after_another_owned_creatures_death_and_not_an_opponents() {
    let price = [(ManaType::Colourless, 1), (ManaType::Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Rot Shambler"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            remove_spell(4, A),
            remove_spell(5, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let killed = |state: &GameState, spell, victim| {
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
    let no_trigger = killed(&entered, 4, 3);
    assert!(no_trigger.execution().window().entries().is_empty());
    let queued = killed(&no_trigger, 5, 2);
    let before = queued
        .execution()
        .power_toughness(physical(&queued, 1).id())
        .unwrap()
        .unwrap();
    assert_eq!(before.derived.power, 1.into());
    let grown = round(queued).into_state();
    let after = grown
        .execution()
        .power_toughness(physical(&grown, 1).id())
        .unwrap()
        .unwrap();
    assert_eq!(
        after.derived,
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_supply_runners_and_gavony_silversmith_follow_their_distinct_counter_domains() {
    use ManaType::*;
    let fixture = |name, cost| {
        game(
            vec![A, B],
            vec![
                hand(name),
                creature(2, A, 2, 2, &[]),
                creature(3, A, 3, 3, &[]),
                creature(4, B, 2, 2, &[]),
            ],
            &[(Colourless, cost), (White, 1)],
            Step::PrecombatMain,
        )
    };
    let counters = |state: &GameState, id| {
        state
            .execution()
            .power_toughness_counters(physical(state, id).id())
            .unwrap()
            .plus_one
    };
    let runners = fixture("Supply Runners", 4);
    let running = cast_named(&runners, &[(Colourless, 4), (White, 1)]);
    let spread = round(running).into_state();
    for (id, expected) in [(1, 0), (2, 1), (3, 1), (4, 0)] {
        assert_eq!(counters(&spread, id), expected.into());
    }

    let gavony = fixture("Gavony Silversmith", 3);
    let window = cast_named(&gavony, &[(Colourless, 3), (White, 1)]);
    let choose = |ids: &[u64]| {
        DecisionReply::TriggerPermanentTargets(
            ids.iter()
                .map(|&id| TargetRef::Object(physical(&window, id).id()))
                .collect(),
        )
    };
    illegal(&window, A, combat_declaration(&window, choose(&[2, 2])));
    for targets in [&[][..], &[2][..], &[2, 4][..]] {
        let queued = reply(&window, choose(targets)).into_state();
        let placed = round(queued).into_state();
        for id in 1..=4 {
            assert_eq!(
                counters(&placed, id),
                i64::from(targets.contains(&id)).into()
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_meteor_golem_targets_only_an_opponents_nonland_permanent() {
    let price = [(ManaType::Colourless, 7)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Meteor Golem"),
            named(
                "Forest",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Mind Stone",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Darksteel Ingot",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(5, B, 3, 3, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entering = cast_named(&initial, &price);
    let target = |id| DecisionReply::TriggerTarget(TargetRef::Object(physical(&entering, id).id()));
    for id in [2, 3] {
        illegal(&entering, A, combat_declaration(&entering, target(id)));
    }
    for (id, destination) in [(4, ObjectZone::Battlefield), (5, ObjectZone::Graveyard(B))] {
        let stacked = reply(&entering, target(id)).into_state();
        let result = round(stacked).into_state();
        assert_eq!(physical(&result, id).zone(), destination);
        assert_eq!(physical(&result, 1).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_eternal_witness_and_cadaver_imp_return_only_from_their_owners_graveyard() {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice::{Decline, Take};
    for (name, color, can_take_artifact) in [
        ("Eternal Witness", Green, true),
        ("Cadaver Imp", Black, false),
    ] {
        let price = [(Colourless, 1), (color, 2)];
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                named("Mind Stone", 2, A, ObjectDestination::Graveyard),
                named("Fencing Ace", 3, A, ObjectDestination::Graveyard),
                named("Fencing Ace", 4, B, ObjectDestination::Graveyard),
            ],
            &price,
            Step::PrecombatMain,
        );
        let entering = cast_named(&initial, &price);
        let select =
            |id| DecisionReply::TriggerTarget(TargetRef::Object(physical(&entering, id).id()));
        illegal(&entering, A, combat_declaration(&entering, select(4)));
        let id = if can_take_artifact { 2 } else { 3 };
        if !can_take_artifact {
            illegal(&entering, A, combat_declaration(&entering, select(2)));
            assert!(
                entering
                    .execution()
                    .current_characteristics(physical(&entering, 1).id())
                    .unwrap()
                    .unwrap()
                    .has_keyword(Flying)
            );
        }
        let triggered = reply(&entering, select(id)).into_state();
        let offered = round(triggered).into_state();
        let declined = reply(&offered, DecisionReply::OptionalEffect(Decline)).into_state();
        assert_eq!(physical(&declined, id).zone(), ObjectZone::Graveyard(A));
        let retrieved = reply(&offered, DecisionReply::OptionalEffect(Take)).into_state();
        assert_eq!(physical(&retrieved, id).zone(), ObjectZone::Hand(A));
        assert_eq!(physical(&retrieved, 4).zone(), ObjectZone::Graveyard(B));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_nadiers_nightblade_drains_for_a_controlled_token_even_when_it_is_exiled() {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Nadier's Nightblade"),
            named("Dragon Fodder", 2, A, ObjectDestination::Hand),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleCreature,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                ),
            ),
            creature(4, A, 1, 1, &[]),
            remove_spell(5, A),
        ],
        &[(Colourless, 3), (Black, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let blade = cast_named(&initial, &price);
    let revealed =
        round(cast_untargeted(&blade, 2, &[(Colourless, 1), (Red, 1)]).into_state()).into_state();
    let RuleFrame::TimestampOrder(frame) = revealed.pending_decision().unwrap().frame() else {
        panic!("Goblin timestamp order");
    };
    let made = reply(
        &revealed,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    let token = tokens(&made, A, 2)[0];
    let on_field = made.into_state();
    let destroyed = round(
        targeted(
            &on_field,
            A,
            5,
            TargetRef::Object(physical(&on_field, 4).id()),
        )
        .into_state(),
    )
    .into_state();
    assert!(destroyed.execution().window().entries().is_empty());
    let departed =
        round(targeted(&destroyed, A, 3, TargetRef::Object(token)).into_state()).into_state();
    assert_eq!(departed.execution().window().entries().len(), 1);
    assert_eq!(departed.execution().life_total(A), Some(&20.into()));
    let drained = round(departed).into_state();
    assert_eq!(drained.execution().life_total(A), Some(&21.into()));
    assert_eq!(drained.execution().life_total(B), Some(&19.into()));
    assert_eq!(drained.execution().life_total(C), Some(&19.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_palace_sentinels_starts_the_monarch_draw_then_transfers_it_on_combat_damage() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Palace Sentinels"),
            creature(2, B, 2, 2, &[Haste]),
            named("Forest", 3, A, ObjectDestination::LibraryBottom),
            named("Swamp", 4, B, ObjectDestination::LibraryBottom),
            named("Island", 5, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let queued = cast_named(&initial, &price);
    assert_eq!(queued.execution().monarch(), None);
    let crown = round(queued).into_state();
    assert_eq!(crown.execution().monarch(), Some(A));
    let first_end = |state: GameState| {
        exile_hand::until(state, |state| {
            state.turn().unwrap().position().step == Step::End
                && !state.execution().window().entries().is_empty()
        })
    };
    let after_end = round(first_end(crown)).into_state();
    assert_eq!(physical(&after_end, 3).zone(), ObjectZone::Hand(A));
    let opposing = next_main(after_end, 2);
    let attacking = attack(&opposing, 2, A);
    let blocks = blockers(&attacking);
    let open = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
    let hit = round(open).into_state();
    assert_eq!(hit.execution().life_total(A), Some(&18.into()));
    assert_eq!(hit.execution().monarch(), Some(A));
    let transferred = round(hit).into_state();
    assert_eq!(transferred.execution().monarch(), Some(B));
    let next_draw = round(first_end(transferred)).into_state();
    assert_eq!(physical(&next_draw, 5).zone(), ObjectZone::Hand(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_guttersnipe_observes_only_its_controllers_instant_or_sorcery_casts() {
    use ManaType::*;
    let draw = || {
        free_instant(
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ),
        )
    };
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Guttersnipe"),
            named("Dragon Fodder", 2, A, ObjectDestination::Hand),
            seed(3, A, ObjectDestination::Hand, draw()),
            seed(4, B, ObjectDestination::Hand, draw()),
            named("Fencing Ace", 5, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 3), (Red, 2), (White, 1)],
        Step::PrecombatMain,
    );
    let on_field = cast_named(&initial, &[(Colourless, 2), (Red, 1)]);
    let creature_only = cast_untargeted(&on_field, 5, &[(Colourless, 1), (White, 1)]).into_state();
    assert_eq!(creature_only.execution().window().entries().len(), 1);

    let after_priority = pass(&on_field).into_state();
    let their_spell = GameCommand::CastUntargeted {
        spell: physical(&after_priority, 4).id(),
        payment: None,
    };
    let opposing = command(&after_priority, B, their_spell).into_state();
    assert_eq!(opposing.execution().window().entries().len(), 1);
    for (id, cost) in [(2, &[(Colourless, 1), (Red, 1)][..]), (3, &[][..])] {
        let own = cast_untargeted(&on_field, id, cost).into_state();
        assert_eq!(own.execution().window().entries().len(), 2);
        let damage = round(own);
        let result = damage.state();
        assert_eq!(physical(result, id).zone(), ObjectZone::Stack);
        assert_eq!(result.execution().life_total(A), Some(&20.into()));
        assert_eq!(result.execution().life_total(B), Some(&18.into()));
        assert_eq!(result.execution().life_total(C), Some(&18.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_artifact_cast_watchers_make_their_printed_tokens_and_keep_optional_payment_separate() {
    use ManaType::*;
    use spellwake_engine::effects::ResolutionPaymentReply;
    for (name, price, pay, kind, flying) in [
        (
            "Efficient Construction",
            [(Colourless, 3), (Blue, 1)],
            false,
            "Thopter",
            true,
        ),
        (
            "Myrsmith",
            [(Colourless, 1), (White, 1)],
            true,
            "Myr",
            false,
        ),
    ] {
        let mut instant_artifact = Program::artifact(Some(ManaCost::free()), vec![]);
        instant_artifact.printed_keywords.insert(Flash);
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                named("Mind Stone", 2, A, ObjectDestination::Hand),
                named("Fencing Ace", 3, A, ObjectDestination::Hand),
                seed(4, B, ObjectDestination::Hand, instant_artifact),
            ],
            &[(Colourless, 6), (Blue, 1), (White, 2)],
            Step::PrecombatMain,
        );
        let field = cast_named(&initial, &price);
        let creature_only = cast_untargeted(&field, 3, &[(Colourless, 1), (White, 1)]).into_state();
        assert_eq!(creature_only.execution().window().entries().len(), 1);
        let their_priority = pass(&field).into_state();
        let opponent_spell = GameCommand::CastUntargeted {
            spell: physical(&their_priority, 4).id(),
            payment: None,
        };
        let opponent = command(&their_priority, B, opponent_spell).into_state();
        assert_eq!(opponent.execution().window().entries().len(), 1);

        let own_artifact = cast_untargeted(&field, 2, &[(Colourless, 2)]).into_state();
        assert_eq!(own_artifact.execution().window().entries().len(), 2);
        let resolving = round(own_artifact);
        let created = if pay {
            let quote = resolving.into_state();
            let missed = reply(
                &quote,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            );
            assert!(tokens(&missed, A, 0).is_empty());
            reply(
                &quote,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
                    Colourless, 1,
                )]))),
            )
        } else {
            resolving
        };
        assert_eq!(physical(created.state(), 2).zone(), ObjectZone::Stack);
        let token = tokens(&created, A, 1)[0];
        let view = created
            .state()
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap();
        assert_eq!(
            view.current_types,
            [CardType::Artifact, CardType::Creature].into()
        );
        assert!(view.current_colors.is_empty());
        assert_eq!(
            view.current_creature_subtypes,
            [current_catalogue().1.resolve_label(kind).unwrap()].into()
        );
        assert_eq!(view.has_keyword(Flying), flying);
        assert_eq!(
            view.power_toughness.unwrap().derived,
            PowerToughness {
                power: 1.into(),
                toughness: 1.into()
            }
        );
    }
}
