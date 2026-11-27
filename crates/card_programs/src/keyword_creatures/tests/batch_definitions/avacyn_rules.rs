use super::{
    cmm_gameplay::{creature, named},
    *,
};
use spellwake_engine::{activated::ActivatedAbilityId, effects::OptionalEffectChoice};

fn put_into_library(spell: &str, cost: &[(ManaType, i64)], top: bool) {
    // The target's controller and owner differ.
    let state = game(
        vec![A, B],
        vec![
            named(spell, 1, A, ObjectDestination::Hand),
            named(
                "Thraben Valiant",
                2,
                B,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        cost,
        Step::PrecombatMain,
    );
    let cast = command(
        &state,
        A,
        GameCommand::CastWithResources {
            spell: physical(&state, 1).id(),
            target: Some(TargetRef::Object(physical(&state, 2).id())),
            payment: CastingPayment {
                mana: Some(spend(cost)),
                ..Default::default()
            },
        },
    );
    let state = round(cast.into_state()).into_state();
    let target = physical(&state, 2);
    assert_eq!(target.zone(), ObjectZone::Library(B));
    let library = state.execution().window().cards().library(B);
    assert_eq!(
        if top { library.first() } else { library.last() },
        Some(&target.id())
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_targeted_library_placement_uses_the_owner_and_correct_end() {
    put_into_library(
        "Vanishment",
        &[(ManaType::Blue, 1), (ManaType::Colourless, 4)],
        true,
    );
    put_into_library(
        "Banishing Stroke",
        &[(ManaType::White, 1), (ManaType::Colourless, 5)],
        false,
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_terminus_orders_physical_creatures_under_each_owners_library() {
    let price = [(ManaType::White, 2), (ManaType::Colourless, 4)];
    let initial = game(
        vec![A, B],
        vec![
            named("Terminus", 1, A, ObjectDestination::Hand),
            named(
                "Thraben Valiant",
                2,
                B,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(3, B, 3, 3, &[]),
            creature(4, A, 3, 3, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let offered = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let RuleFrame::ResolutionGraveyardOrder(frame) = offered.pending_decision().unwrap().frame()
    else {
        panic!("expected library order");
    };
    assert_eq!(frame.destination(), PublicZoneDestination::Library);
    assert_eq!(frame.chooser(), B);
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        [physical(&offered, 2).id(), physical(&offered, 3).id()].into()
    );
    let end = reply(
        &offered,
        DecisionReply::OrderResolutionGraveyard(vec![
            physical(&offered, 3).id(),
            physical(&offered, 2).id(),
        ]),
    )
    .into_state();
    let library = end.execution().window().cards().library(B);
    assert_eq!(
        library[1..],
        [physical(&end, 3).id(), physical(&end, 2).id()]
    );
    assert_eq!(physical(&end, 4).zone(), ObjectZone::Library(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_chaplain_rejects_duplicate_humans_and_taps_valid_costs_atomically() {
    let battlefield = ObjectDestination::Battlefield { controller: A };
    let initial = game(
        vec![A, B],
        vec![
            named("Devout Chaplain", 1, A, battlefield),
            named("Thraben Valiant", 2, A, battlefield),
            named("Thraben Valiant", 3, A, battlefield),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(None, vec![]),
            ),
        ],
        &[],
        Step::Untap,
    );
    let human = |id| physical(&initial, id).id();
    let activate = |objects| GameCommand::ActivateWithCostObjects {
        source: human(1),
        ability: ActivatedAbilityId(0),
        granting_effect: None,
        target: Some(TargetRef::Object(human(4))),
        objects,
        payment: None,
    };
    let bad = initial.advance(GameInput {
        basis: initial.basis().clone(),
        actor: A,
        command: activate(vec![human(2), human(2)]),
    });
    assert!(matches!(
        bad,
        Err(GameError::Effect(EffectError::InvalidCastResource))
    ));
    assert_eq!(physical(&initial, 1).tapped(), Some(false));
    let activated = command(&initial, A, activate(vec![human(2), human(3)])).into_state();
    for id in 1..=3 {
        assert_eq!(physical(&activated, id).tapped(), Some(true));
    }
    let end = round(activated).into_state();
    assert_eq!(physical(&end, 4).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source, creature vocabulary and Avacyn Restored set manifest"]
fn avacyn_rules_validate_compiled_set_definitions() {
    let manifest: Value = serde_json::from_reader(
        std::fs::File::open(std::env::var_os("SPELLWAKE_AVACYN_RESTORED_SET").unwrap()).unwrap(),
    )
    .unwrap();
    let ids: BTreeSet<_> = manifest["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["oracle_id"].as_str().unwrap())
        .collect();
    let (inventory, vocabulary) = dated_sources();
    let failures: Vec<_> = inventory
        .cards
        .iter()
        .filter(|card| card.oracle_id.as_deref().is_some_and(|id| ids.contains(id)))
        .filter_map(|card| {
            crate::keyword_creatures::compile(
                card,
                &vocabulary,
                crate::keyword_creatures::Profile::Current,
            )
            .ok()
            .and_then(|program| program.validate(&vocabulary).err())
            .map(|error| (&card.name, error))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_demonic_rising_rechecks_the_creature_count_at_resolution() {
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Demonic Rising",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 2, 2, &[]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ),
            ),
        ],
        &[],
        Step::PostcombatMain,
    );
    let offered = round(initial).into_state();
    assert_eq!(offered.turn().unwrap().position().step, Step::End);
    assert_eq!(offered.execution().window().entries().len(), 1);
    let accepted = round(offered.clone());
    tokens(&accepted, A, 1);

    let removal = targeted(
        &offered,
        A,
        3,
        TargetRef::Object(physical(&offered, 2).id()),
    );
    let removed = round(removal.into_state()).into_state();
    let declined = round(removed);
    assert!(
        events(&declined).any(|event| event.occurrences.iter().any(|occurrence| matches!(
            occurrence.value,
            RulesOccurrence::EntryCompleted {
                completion: spellwake_engine::occurrence::EntryCompletion::InterveningIfFalse,
                ..
            }
        )))
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_arcane_melee_reduces_spells_for_both_players() {
    for caster in [A, B] {
        let initial = game(
            vec![A, B],
            vec![
                named(
                    "Arcane Melee",
                    1,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                seed(
                    2,
                    caster,
                    ObjectDestination::Hand,
                    Program::instant(
                        ManaCost::generic(2.into()),
                        ExecutableBody::new(
                            BodyTarget::None,
                            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(spellwake_engine::program::ValueExpr::Literal(0.into()))],
                        ),
                    ),
                ),
            ],
            &[],
            Step::PrecombatMain,
        );
        let priority = if caster == A {
            initial
        } else {
            pass(&initial).into_state()
        };
        let spell = physical(&priority, 2).id();
        let cast = command(
            &priority,
            caster,
            GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        );
        let end = round(cast.into_state()).into_state();
        assert_eq!(physical(&end, 2).zone(), ObjectZone::Graveyard(caster));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_peel_from_reality_returns_the_remaining_legal_target() {
    let price = [(ManaType::Blue, 1), (ManaType::Colourless, 1)];
    let initial = game(
        vec![A, B],
        vec![
            named("Peel from Reality", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            seed(
                4,
                B,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let offered = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                fixed_targets: vec![
                    TargetRef::Object(physical(&initial, 2).id()),
                    TargetRef::Object(physical(&initial, 3).id()),
                ],
                ..Default::default()
            },
        },
    )
    .into_state();
    let priority = pass(&offered).into_state();
    let response = targeted(
        &priority,
        B,
        4,
        TargetRef::Object(physical(&priority, 3).id()),
    );
    let resolved = round(response.into_state()).into_state();
    let end = round(resolved).into_state();
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&end, 3).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_essence_harvest_retains_both_life_changes_to_the_same_player() {
    let price = [(ManaType::Black, 1), (ManaType::Colourless, 2)];
    let initial = game(
        vec![A, B],
        vec![
            named("Essence Harvest", 1, A, ObjectDestination::Hand),
            creature(2, A, 4, 3, &[]),
            creature(3, B, 8, 8, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Player(A)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let result = round(cast.into_state());
    let life_events: Vec<_> = events(&result)
        .filter_map(|event| {
            let changes: Vec<_> = event
                .occurrences
                .iter()
                .filter_map(|occurrence| {
                    if let RulesOccurrence::LifeChanged(change) = &occurrence.value {
                        Some(change)
                    } else {
                        None
                    }
                })
                .collect();
            (!changes.is_empty()).then_some(changes)
        })
        .collect();
    let [changes] = life_events.as_slice() else {
        panic!("expected one simultaneous event");
    };
    let [loss, gain] = changes.as_slice() else {
        panic!("expected both life changes");
    };
    assert_eq!(loss.kind, spellwake_engine::life::LifeChangeKind::Loss);
    assert_eq!(gain.kind, spellwake_engine::life::LifeChangeKind::Gain);
    for change in [loss, gain] {
        assert_eq!(change.player, A);
        assert_eq!(change.before, 20.into());
        assert_eq!(change.after, 20.into());
        assert_eq!(change.amount, 4.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_amass_selects_from_the_hand_after_all_three_draws() {
    let price = [(ManaType::Blue, 1), (ManaType::Colourless, 3)];
    let mut cards = vec![named("Amass the Components", 1, A, ObjectDestination::Hand)];
    cards.extend((2..=4).map(|id| {
        seed(
            id,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    }));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let offered = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let RuleFrame::ResolutionSelect(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("expected a choice from hand");
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.minimum(), 1);
    assert_eq!(frame.domain().len(), 3);
    assert_eq!(offered.face_down_reveals().count(), 0);
    let selected = physical(&offered, 3).id();
    let final_state = reply(&offered, DecisionReply::SelectResolution(vec![selected])).into_state();
    let in_library = physical(&final_state, 3);
    assert_eq!(in_library.zone(), ObjectZone::Library(A));
    assert_eq!(
        final_state.execution().window().cards().library(A).last(),
        Some(&in_library.id())
    );
    assert_eq!(physical(&final_state, 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&final_state, 4).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_cavern_of_souls_protects_only_the_spell_that_used_its_mana() {
    use spellwake_engine::{
        land::EntryChoice,
        mana::{ManaAbilityId, ManaAbilityRef, ManaRestriction, RestrictedManaSpend},
    };
    let elf = current_catalogue().1.resolve_label("Elf").unwrap();
    for protected in [false, true] {
        let initial = game(
            vec![A, B],
            vec![
                named("Cavern of Souls", 1, A, ObjectDestination::Hand),
                seed(
                    2,
                    A,
                    ObjectDestination::Hand,
                    Program::creature(
                        Some(ManaCost::generic(1.into())),
                        1.into(),
                        1.into(),
                        [elf].into(),
                    ),
                ),
                seed(
                    3,
                    B,
                    ObjectDestination::Hand,
                    free_instant(
                        BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
                        Instruction::StackOp(Box::new(
                            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                spellwake_engine::toolbox::CounterDestination::Graveyard,
                            ),
                        )),
                    ),
                ),
            ],
            &[(ManaType::Red, 1)],
            Step::PrecombatMain,
        );
        let entry = command(
            &initial,
            A,
            GameCommand::PlayLand {
                land: physical(&initial, 1).id(),
            },
        )
        .into_state();
        let played = reply(
            &entry,
            DecisionReply::EntryChoice(EntryChoice::CreatureType(elf)),
        )
        .into_state();
        let mana = command(
            &played,
            A,
            GameCommand::ActivateMana {
                source: physical(&played, 1).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        )
        .into_state();
        let funded = reply(&mana, DecisionReply::ManaColor(ManaType::Red)).into_state();
        let allocation = RestrictedManaSpend {
            mana_type: ManaType::Red,
            restriction: ManaRestriction::CreatureSpellOfTypeUncounterable(Some(elf)),
            amount: 1.into(),
        };
        let cast = command(
            &funded,
            A,
            GameCommand::CastWithResources {
                spell: physical(&funded, 2).id(),
                target: None,
                payment: CastingPayment {
                    mana: Some(spend(&[(ManaType::Red, 1)])),
                    restricted_mana: protected.then_some(allocation).into_iter().collect(),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let spell = cast.execution().window().entries().last().unwrap();
        assert_eq!(spell.protected_by_spent_mana(), protected);
        let entry = spell.id();
        let opposing_priority = pass(&cast).into_state();
        let counter = targeted(&opposing_priority, B, 3, TargetRef::Stack(entry)).into_state();
        let after_counter = round(counter).into_state();
        assert_eq!(
            after_counter.execution().window().entries().len(),
            usize::from(protected)
        );
        if protected {
            let resolved = round(after_counter).into_state();
            assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
        } else {
            assert_eq!(physical(&after_counter, 2).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_bonfire_damages_only_the_target_player_and_their_creatures() {
    let price = [(ManaType::Red, 1), (ManaType::Colourless, 4)];
    let initial = game(
        vec![A, B, C],
        vec![
            named("Bonfire of the Damned", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            creature(4, C, 2, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                chosen_x: Some(2.into()),
                ..Default::default()
            },
        },
    );
    let final_state = round(cast.into_state()).into_state();
    assert_eq!(final_state.execution().life_total(A), Some(&20.into()));
    assert_eq!(final_state.execution().life_total(B), Some(&18.into()));
    assert_eq!(final_state.execution().life_total(C), Some(&20.into()));
    assert_eq!(physical(&final_state, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&final_state, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&final_state, 4).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_appetite_reveals_the_opponents_hand_and_the_caster_chooses() {
    let price = [(ManaType::Black, 1)];
    let state = game(
        vec![A, B],
        vec![
            named("Appetite for Brains", 1, A, ObjectDestination::Hand),
            named("Spitebellows", 2, B, ObjectDestination::Hand),
            named("Swamp", 3, B, ObjectDestination::Hand),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &state,
        A,
        GameCommand::CastWithResources {
            spell: physical(&state, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let offered = round(cast.into_state()).into_state();
    let RuleFrame::ResolutionSelect(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("expected the caster to choose a card from the revealed hand");
    };
    let chosen = physical(&offered, 2).id();
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.domain(), &[chosen]);
    assert_eq!(offered.face_down_reveals().count(), 2);
    let finished = reply(&offered, DecisionReply::SelectResolution(vec![chosen])).into_state();
    assert_eq!(physical(&finished, 2).zone(), ObjectZone::Exile);
    assert_eq!(physical(&finished, 3).zone(), ObjectZone::Hand(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_lair_delve_takes_the_land_and_bottoms_the_noncreature() {
    let price = [(ManaType::Green, 1), (ManaType::Colourless, 2)];
    let state = game(
        vec![A, B],
        vec![
            named("Lair Delve", 1, A, ObjectDestination::Hand),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named("Pillar of Flame", 3, A, ObjectDestination::LibraryBottom),
            named("Spitebellows", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&state, 1, &price);
    let finished = round(cast.into_state()).into_state();
    assert_eq!(physical(&finished, 2).zone(), ObjectZone::Hand(A));
    let library = finished.execution().window().cards().library(A);
    assert_eq!(library.first(), Some(&physical(&finished, 4).id()));
    assert_eq!(library.last(), Some(&physical(&finished, 3).id()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_vexing_devil_stops_at_the_first_accepting_opponent() {
    let price = [(ManaType::Red, 1)];
    let state = game(
        vec![A, B, C],
        vec![named("Vexing Devil", 1, A, ObjectDestination::Hand)],
        &price,
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&state, 1, &price).into_state();
    let awaiting_trigger = round(cast).into_state();
    let first = round(awaiting_trigger).into_state();
    assert_eq!(first.pending_decision().unwrap().chooser(), B);
    let second = reply(
        &first,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), C);
    let finished = reply(
        &second,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(
        finished.execution().life_total(C).unwrap(),
        &state.execution().life_total(C).unwrap().minus(&4.into()),
    );
    assert_eq!(
        finished.execution().life_total(B),
        state.execution().life_total(B)
    );
    assert_eq!(physical(&finished, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_subtype_protection_prevents_targeting_and_damage_from_kindred_spells() {
    let vampire = dated_sources().1.resolve_label("Vampire").unwrap();
    let vampire_spell = |target, instruction| {
        let mut spell = free_instant(target, instruction);
        spell
            .card_types
            .insert(spellwake_engine::program::CardType::Kindred);
        spell.creature_subtypes.insert(vampire);
        spell
    };
    let state = game(
        vec![A, B],
        vec![
            named(
                "Midnight Duelist",
                1,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                vampire_spell(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(2),
                        false,
                    ),
                ),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                vampire_spell(
                    BodyTarget::None,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::Creatures,
                            ),
                        )),
                        spellwake_engine::program::ValueExpr::literal(2),
                        false,
                    ),
                ),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let illegal = state.advance(GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: GameCommand::CastWithResources {
            spell: physical(&state, 2).id(),
            target: Some(TargetRef::Object(physical(&state, 1).id())),
            payment: CastingPayment::default(),
        },
    });
    assert!(matches!(
        illegal,
        Err(GameError::Effect(EffectError::IllegalTarget))
    ));
    let cast = cast_untargeted(&state, 3, &[]).into_state();
    let finished = round(cast).into_state();
    assert_eq!(physical(&finished, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(
        finished
            .execution()
            .battlefield_marked_damage(physical(&finished, 1).id()),
        Some(0.into())
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_descent_exiles_hand_and_battlefield_choices_at_the_same_cut() {
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Descent into Madness",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            named("Defy Death", 4, A, ObjectDestination::Hand),
            named("Defy Death", 5, B, ObjectDestination::Hand),
        ],
        &[],
        Step::Untap,
    );
    let first = round(initial).into_state();
    assert_eq!(first.pending_decision().unwrap().chooser(), A);
    let source = physical(&first, 1).id();
    assert_eq!(
        first
            .execution()
            .counters_of_kind(source, spellwake_engine::effects::CounterKind::Despair),
        1.into()
    );
    let second = reply(
        &first,
        DecisionReply::SelectResolution(vec![physical(&first, 4).id()]),
    )
    .into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), B);
    assert_eq!(physical(&second, 4).zone(), ObjectZone::Hand(A));
    let done = reply(
        &second,
        DecisionReply::SelectResolution(vec![physical(&second, 3).id()]),
    );
    assert_eq!(physical(done.state(), 4).zone(), ObjectZone::Exile);
    assert_eq!(physical(done.state(), 3).zone(), ObjectZone::Exile);
    assert_eq!(physical(done.state(), 5).zone(), ObjectZone::Hand(B));
    assert!(events(&done).any(|event| {
        event
            .occurrences
            .iter()
            .filter(|row| matches!(row.value, RulesOccurrence::ZoneChange(_)))
            .count()
            == 2
    }));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_killing_wave_collects_payments_before_committing_sacrifices() {
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 1)];
    let initial = game(
        vec![A, B],
        vec![
            named("Killing Wave", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: None,
            payment: CastingPayment {
                chosen_x: Some(2.into()),
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let first = round(cast.into_state()).into_state();
    let saved = physical(&first, 2).id();
    let second = reply(&first, DecisionReply::SelectResolution(vec![saved])).into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), B);
    assert_eq!(second.execution().prior_vow_choices(), vec![(A, saved)]);
    assert_eq!(second.execution().life_total(A), Some(&20.into()));
    let done = reply(
        &second,
        DecisionReply::SelectResolution(vec![physical(&second, 3).id()]),
    );
    assert_eq!(done.state().execution().life_total(A), Some(&18.into()));
    assert_eq!(done.state().execution().life_total(B), Some(&18.into()));
    assert_eq!(physical(done.state(), 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(done.state(), 3).zone(), ObjectZone::Battlefield);
    assert!(events(&done).any(|event| {
        let changes = &event.occurrences;
        changes
            .iter()
            .filter(|row| matches!(row.value, RulesOccurrence::LifeChanged(_)))
            .count()
            == 2
            && changes
                .iter()
                .any(|row| matches!(row.value, RulesOccurrence::ZoneChange(_)))
    }));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_blessings_of_nature_enforces_and_applies_the_announced_shares() {
    let price = [(ManaType::Colourless, 4), (ManaType::Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            named("Blessings of Nature", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 2, &[]),
            creature(3, A, 1, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let first = physical(&initial, 2).id();
    let second = physical(&initial, 3).id();
    let cast = |shares| GameCommand::CastWithResources {
        spell: physical(&initial, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&price)),
            permanent_targets: vec![TargetRef::Object(first), TargetRef::Object(second)],
            target_division: shares,
            ..Default::default()
        },
    };
    for invalid in [vec![4, 0], vec![2, 1], vec![4]] {
        assert!(
            initial
                .advance(GameInput {
                    basis: initial.basis().clone(),
                    actor: A,
                    command: cast(invalid),
                })
                .is_err()
        );
    }
    let done = round(command(&initial, A, cast(vec![3, 1])).into_state());
    let engine = done.state().execution();
    let kind = spellwake_engine::effects::CounterKind::PlusOne;
    assert_eq!(engine.counters_of_kind(first, kind), 3.into());
    assert_eq!(engine.counters_of_kind(second, kind), 1.into());
    assert!(events(&done).any(|event| {
        event
            .occurrences
            .iter()
            .filter(|row| matches!(row.value, RulesOccurrence::CountersPlaced { .. }))
            .count()
            == 2
    }));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_bruna_attaches_auras_from_hand_and_graveyard_as_they_enter() {
    let initial = game(
        vec![A, B],
        vec![
            named(
                "Bruna, Light of Alabaster",
                1,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Defang", 2, A, ObjectDestination::Hand),
            named("Spectral Prison", 3, A, ObjectDestination::Graveyard),
            creature(4, B, 1, 2, &[]),
        ],
        &[],
        Step::Untap,
    );
    let declare = combat_frame(&initial, true);
    let bruna = physical(&declare, 1).id();
    let attacked = reply(
        &declare,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: bruna,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let pending = round(attacked).into_state();
    let chosen = vec![physical(&pending, 2).id(), physical(&pending, 3).id()];
    let order = reply(&pending, DecisionReply::SelectResolution(chosen.clone())).into_state();
    let done = reply(&order, DecisionReply::LinkedReturnOrder(chosen));
    for id in [2, 3] {
        let aura = physical(done.state(), id);
        assert_eq!(aura.zone(), ObjectZone::Battlefield);
        assert_eq!(
            done.state().execution().attached_to(aura.id()),
            Some(TargetRef::Object(bruna))
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_infinite_reflection_copies_existing_and_entering_nontokens() {
    let price = [(ManaType::Colourless, 5), (ManaType::Blue, 1)];
    let budget = [
        (ManaType::Colourless, 6),
        (ManaType::Blue, 1),
        (ManaType::Green, 1),
    ];
    let initial = game(
        vec![A, B],
        vec![
            named("Infinite Reflection", 1, A, ObjectDestination::Hand),
            named(
                "Seraph of Dawn",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(3, A, 1, 1, &[]),
            named("Timberland Guide", 4, A, ObjectDestination::Hand),
        ],
        &budget,
        Step::PrecombatMain,
    );
    let host = physical(&initial, 2).id();
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Object(host)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let aura = round(cast.into_state());
    assert_eq!(
        aura.state()
            .execution()
            .attached_to(physical(aura.state(), 1).id()),
        Some(TargetRef::Object(host))
    );
    let reflected = round(aura.into_state()).into_state();
    let check = |state: &GameState, card| {
        let object = physical(state, card).id();
        let view = state
            .execution()
            .current_characteristics(object)
            .unwrap()
            .unwrap();
        assert_eq!(
            state
                .execution()
                .creature(object)
                .unwrap()
                .unwrap()
                .toughness,
            4.into()
        );
        assert!(view.has_keyword(KeywordAbility::Flying));
        assert!(view.has_keyword(KeywordAbility::Lifelink));
        assert!(!view.is_token);
    };
    check(&reflected, 3);
    let entrant = cast_untargeted(
        &reflected,
        4,
        &[(ManaType::Colourless, 1), (ManaType::Green, 1)],
    );
    let done = round(entrant.into_state()).into_state();
    check(&done, 4);
    assert!(done.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn avacyn_rules_divine_deflection_shares_capacity_across_controller_and_creatures() {
    use spellwake_engine::{
        damage::{DamageAllocation, DamageOrigin},
        effects::DamageDecision,
    };

    let price = [(ManaType::Colourless, 3), (ManaType::White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            named("Divine Deflection", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 4, &[]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PLAYER,
                    Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec::target_controllers_creatures(
                            spellwake_engine::program::ValueExpr::Literal(3.into()),
                            true,
                            false,
                        ),
                    )),
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                chosen_x: Some(3.into()),
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let resolved = round(cast.into_state()).into_state();
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Graveyard(A));
    let damaged = round(targeted(&resolved, A, 3, TargetRef::Player(A)).into_state()).into_state();
    let RuleFrame::Damage(DamageDecision::Allocation(frame)) =
        damaged.pending_decision().unwrap().frame()
    else {
        panic!("expected one shared prevention allocation");
    };
    let creature = physical(&damaged, 2).id();
    assert_eq!(frame.eligible().len(), 2);
    assert_eq!(frame.required(), &3.into());
    let allocations = frame
        .eligible()
        .iter()
        .map(|row| DamageAllocation {
            attempt: row.attempt(),
            amount: if row.target() == DamageRecipient::Player(A) {
                2.into()
            } else {
                1.into()
            },
        })
        .collect();
    let done = reply(&damaged, DecisionReply::AllocatePrevention(allocations));
    assert_eq!(done.state().execution().life_total(A), Some(&19.into()));
    assert_eq!(done.state().execution().life_total(B), Some(&17.into()));
    assert_eq!(
        done.state().execution().battlefield_marked_damage(creature),
        Some(2.into())
    );
    assert!(
        done.damage_journal()
            .batches
            .iter()
            .any(|batch| matches!(batch.origin, DamageOrigin::PreventionFollowup { .. }))
    );
}
