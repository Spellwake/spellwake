//! Additional enchantment and constellation gameplay.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_arasta_ignores_its_controllers_spell_and_makes_a_reach_spider_for_an_opponents_spell() {
    use ManaType::*;
    let price = [(Colourless, 2), (Green, 2)];
    let instant = |id, owner| {
        seed(
            id,
            owner,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_PLAYER,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Arasta of the Endless Web"),
            instant(2, A),
            instant(3, B),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert!(
        entered
            .execution()
            .current_characteristics(physical(&entered, 1).id())
            .unwrap()
            .unwrap()
            .has_keyword(Reach)
    );
    let mine = targeted(&entered, A, 2, TargetRef::Player(B)).into_state();
    assert_eq!(mine.execution().window().entries().len(), 1);
    let opposing = pass(&entered).into_state();
    let trigger = targeted(&opposing, B, 3, TargetRef::Player(A)).into_state();
    assert_eq!(trigger.execution().window().entries().len(), 2);
    let new = round(trigger);
    let spider = tokens(&new, A, 1)[0];
    assert_eq!(physical(new.state(), 3).zone(), ObjectZone::Stack);
    let view = new
        .state()
        .execution()
        .current_characteristics(spider)
        .unwrap()
        .unwrap();
    assert!(view.has_keyword(Reach));
    assert_eq!(view.current_colors, [Color::Green].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Spider").unwrap()].into()
    );
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 2.into()
        }
    );
    let resolved = round(new.into_state()).into_state();
    assert_eq!(resolved.execution().life_total(A), Some(&19.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_archon_of_suns_grace_makes_a_flying_pegasus_with_lifelink_for_an_allied_enchantment() {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 2)];
    let pegasus = current_catalogue().1.resolve_label("Pegasus").unwrap();
    let program = Program::creature(None, 3.into(), 3.into(), [pegasus].into());
    let initial = game(
        vec![A, B],
        vec![
            hand("Archon of Sun's Grace"),
            named("Fervor", 2, A, ObjectDestination::Hand),
            seed(
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
                program.clone(),
            ),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                program,
            ),
        ],
        &[(Colourless, 4), (White, 2), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert!(entered.execution().window().entries().is_empty());
    let view = |state: &GameState, id| {
        state
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap()
    };
    let archon = view(&entered, physical(&entered, 1).id());
    for keyword in [Flying, Lifelink] {
        assert!(archon.has_keyword(keyword));
    }
    assert!(view(&entered, physical(&entered, 3).id()).has_keyword(Lifelink));
    assert!(!view(&entered, physical(&entered, 4).id()).has_keyword(Lifelink));
    let enchantment =
        round(cast_untargeted(&entered, 2, &[(Colourless, 2), (Red, 1)]).into_state()).into_state();
    let made = round(enchantment);
    let token = tokens(&made, A, 1)[0];
    let sheet = view(made.state(), token);
    assert!(sheet.has_keyword(Flying));
    assert!(sheet.has_keyword(Lifelink));
    assert_eq!(sheet.current_colors, [Color::White].into());
    assert_eq!(sheet.current_creature_subtypes, [pegasus].into());
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: 2.into(),
            toughness: 2.into()
        }
    );
    let choosing = combat_frame(made.state(), true);
    let attack = reply(
        &choosing,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: token,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let damage = exile_hand::until(attack, |state| {
        state.execution().life_total(B) == Some(&18.into())
    });
    assert_eq!(damage.execution().life_total(A), Some(&22.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_doomwake_giant_shrinks_enemies_for_its_own_and_later_enchantment_entries() {
    use ManaType::*;
    let price = [(Colourless, 4), (Black, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Doomwake Giant"),
            named("Fervor", 2, A, ObjectDestination::Hand),
            creature(3, A, 1, 1, &[]),
            creature(4, B, 1, 1, &[Indestructible]),
            creature(5, B, 3, 3, &[]),
        ],
        &[(Colourless, 6), (Black, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    assert_eq!(physical(&entered, 4).zone(), ObjectZone::Battlefield);
    let smaller = round(entered).into_state();
    assert_eq!(physical(&smaller, 4).zone(), ObjectZone::Graveyard(B));
    let size = |state: &GameState, id| {
        state
            .execution()
            .power_toughness(physical(state, id).id())
            .unwrap()
            .unwrap()
            .derived
            .toughness
    };
    assert_eq!(size(&smaller, 5), 2.into());
    let second =
        round(cast_untargeted(&smaller, 2, &[(Colourless, 2), (Red, 1)]).into_state()).into_state();
    let even_smaller = round(second).into_state();
    assert_eq!(size(&even_smaller, 3), 1.into());
    assert_eq!(size(&even_smaller, 5), 1.into());
    let reset = next_main(even_smaller, 2);
    assert_eq!(size(&reset, 5), 3.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_cacophony_unleashed_destroys_on_cast_and_animates_on_an_enchantment_entry() {
    use ManaType::*;
    use spellwake_engine::program::Supertype;
    let price = [(Colourless, 5), (Black, 2)];
    let cast = game(
        vec![A, B],
        vec![
            hand("Cacophony Unleashed"),
            creature(2, A, 2, 2, &[]),
            named(
                "Doomwake Giant",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(4, B, 2, 2, &[Indestructible]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let uses = cast_named(&cast, &price);
    let RuleFrame::Triggers(order) = uses.pending_decision().unwrap().frame() else {
        panic!("cast entrance and animation triggers");
    };
    let stacked = reply(
        &uses,
        DecisionReply::OrderTriggers(order.instances().to_vec()),
    )
    .into_state();
    let mut resolving = round(round(stacked).into_state()).into_state();
    if let Some(RuleFrame::ResolutionGraveyardOrder(frame)) =
        resolving.pending_decision().map(|choice| choice.frame())
    {
        resolving = reply(
            &resolving,
            DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec()),
        )
        .into_state();
    }
    assert_eq!(physical(&resolving, 2).zone(), ObjectZone::Graveyard(A));
    for id in [1, 3, 4] {
        assert_eq!(physical(&resolving, id).zone(), ObjectZone::Battlefield);
    }
    let view = |state: &GameState, source| {
        state
            .execution()
            .current_characteristics(source)
            .unwrap()
            .unwrap()
    };
    let animated = |state: &GameState, source| {
        let profile = view(state, source);
        assert_eq!(
            profile.current_types,
            [CardType::Creature, CardType::Enchantment].into()
        );
        assert!(profile.current_supertypes.contains(&Supertype::Legendary));
        let vocab = current_catalogue().1;
        let subtype = ["Nightmare", "God"].map(|label| vocab.resolve_label(label).unwrap());
        for kind in subtype {
            assert!(profile.current_creature_subtypes.contains(&kind));
        }
        for keyword in [Menace, Deathtouch] {
            assert!(profile.has_keyword(keyword));
        }
        assert_eq!(
            profile.power_toughness.unwrap().derived,
            PowerToughness {
                power: 6.into(),
                toughness: 6.into()
            }
        );
    };
    let source = physical(&resolving, 1).id();
    animated(&resolving, source);
    let inactive = next_main(resolving, 2);
    assert_eq!(
        view(&inactive, source).current_types,
        [CardType::Enchantment].into()
    );

    let mut free_enchantment = named("Fervor", 4, A, ObjectDestination::Hand);
    let ProgramKind::Permanent(permanent) = &mut free_enchantment.program.kind else {
        unreachable!()
    };
    permanent.casting_cost = Some(ManaCost::free());
    let copied = game(
        vec![A, B],
        vec![
            hand("Anikthea, Hand of Erebos"),
            named("Cacophony Unleashed", 2, A, ObjectDestination::Graveyard),
            creature(3, B, 2, 2, &[]),
            free_enchantment,
        ],
        &[(Colourless, 2), (White, 1), (Black, 1), (Green, 1)],
        Step::PrecombatMain,
    );
    let target = cast_named(
        &copied,
        &[(Colourless, 2), (White, 1), (Black, 1), (Green, 1)],
    );
    let announced = reply(
        &target,
        DecisionReply::TriggerPermanentTargets(vec![TargetRef::Object(physical(&target, 2).id())]),
    )
    .into_state();
    let token = round(announced);
    let product = tokens(&token, A, 1)[0];
    let activities = token.into_state();
    assert_eq!(activities.execution().window().entries().len(), 1);
    let living = round(activities).into_state();
    animated(&living, product);
    assert_eq!(physical(&living, 3).zone(), ObjectZone::Battlefield);
    let after_cleanup = next_main(living, 3);
    assert_eq!(
        view(&after_cleanup, product)
            .power_toughness
            .unwrap()
            .derived
            .power,
        3.into()
    );
    let later_enchantment =
        round(cast_untargeted(&after_cleanup, 4, &[]).into_state()).into_state();
    let animated_again = round(later_enchantment).into_state();
    animated(&animated_again, product);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_endrek_makes_thrulls_when_creature_spells_are_cast_then_sacrifices_at_seven() {
    use ManaType::*;
    let price = [(Colourless, 4), (Black, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Endrek Sahr, Master Breeder"),
            named("Elvish Mystic", 2, A, ObjectDestination::Hand),
            named("Soul of New Phyrexia", 3, A, ObjectDestination::Hand),
            named("Sol Ring", 4, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 11), (Black, 1), (Green, 1)],
        Step::PrecombatMain,
    );
    let endrek = cast_named(&initial, &price);
    let artifact = cast_untargeted(&endrek, 4, &[(Colourless, 1)]).into_state();
    assert_eq!(artifact.execution().window().entries().len(), 1);
    let ready = round(artifact).into_state();
    let next = cast_untargeted(&ready, 2, &[(Green, 1)]).into_state();
    let first = round(next);
    let thrull = tokens(&first, A, 1)[0];
    assert_eq!(physical(first.state(), 2).zone(), ObjectZone::Stack);
    let view = first
        .state()
        .execution()
        .current_characteristics(thrull)
        .unwrap()
        .unwrap();
    assert_eq!(view.current_colors, [Color::Black].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Thrull").unwrap()].into()
    );
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let still_alive = round(first.into_state()).into_state();
    assert_eq!(physical(&still_alive, 1).zone(), ObjectZone::Battlefield);
    let casting = cast_untargeted(&still_alive, 3, &[(Colourless, 6)]).into_state();
    let ordering = round(casting).into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("six simultaneous Thrulls");
    };
    let seven = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|member| member.member).collect()),
    );
    assert_eq!(super::set_encore::new_tokens(&seven).len(), 6);
    assert_eq!(seven.state().execution().window().entries().len(), 2);
    let sacrificed = round(seven.into_state()).into_state();
    assert_eq!(physical(&sacrificed, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&sacrificed, 3).zone(), ObjectZone::Stack);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mikaeus_gives_only_other_friendly_nonhumans_undying_and_destroys_a_human_that_hits_him() {
    use ManaType::*;
    use spellwake_engine::effects::CounterKind;
    let price = [(Colourless, 3), (Black, 3)];
    let human = current_catalogue().1.resolve_label("Human").unwrap();
    let seed_human = |id, owner| {
        let mut program = Program::creature(None, 1.into(), 1.into(), [human].into());
        program.printed_keywords.insert(Haste);
        seed(
            id,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            program,
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Mikaeus, the Unhallowed"),
            creature(2, A, 2, 2, &[]),
            seed_human(3, A),
            seed_human(4, B),
            creature(5, B, 2, 2, &[]),
            remove_spell(6, A),
            remove_spell(7, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let view = |state: &GameState, id| {
        state
            .execution()
            .current_characteristics(physical(state, id).id())
            .unwrap()
            .unwrap()
    };
    assert!(view(&entered, 1).has_keyword(Intimidate));
    for (id, power) in [(1, 5), (2, 3), (3, 1), (4, 1), (5, 2)] {
        assert_eq!(
            view(&entered, id).power_toughness.unwrap().derived.power,
            power.into()
        );
    }
    let kill = |state: &GameState, spell| {
        round(targeted(state, A, spell, TargetRef::Object(physical(state, 2).id())).into_state())
            .into_state()
    };
    let dead = kill(&entered, 6);
    assert_eq!(physical(&dead, 2).zone(), ObjectZone::Graveyard(A));
    let returned = round(dead).into_state();
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(
        returned
            .execution()
            .counters_of_kind(physical(&returned, 2).id(), CounterKind::PlusOne),
        1.into()
    );
    let dead_again = kill(&returned, 7);
    assert_eq!(physical(&dead_again, 2).zone(), ObjectZone::Graveyard(A));
    assert!(dead_again.execution().window().entries().is_empty());
    let their_turn = next_main(dead_again, 2);
    let attacks = attack(&their_turn, 4, A);
    let impact = exile_hand::until(attacks, |state| {
        state.execution().life_total(A) == Some(&19.into())
    });
    assert_eq!(physical(&impact, 4).zone(), ObjectZone::Battlefield);
    let destroyed = round(impact).into_state();
    assert_eq!(physical(&destroyed, 4).zone(), ObjectZone::Graveyard(B));
}
