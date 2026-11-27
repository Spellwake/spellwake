use super::*;
use spellwake_engine::land::EntryChoice;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_multiplayer_sower_links_exactly_two_seats_and_loses_life_only_once() {
    let bolt = || {
        free_instant(
            BodyTarget::SINGLE_ANY_TARGET,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                spellwake_engine::program::ValueExpr::literal(3),
                false,
            ),
        )
    };
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Sower of Discord"),
            seed(2, A, ObjectDestination::Hand, bolt()),
            seed(3, A, ObjectDestination::Hand, bolt()),
            seed(4, A, ObjectDestination::Hand, bolt()),
        ],
        &[(ManaType::Colourless, 4), (ManaType::Black, 2)],
        Step::PrecombatMain,
    );
    let pending = round(
        cast_untargeted(
            &initial,
            1,
            &[(ManaType::Colourless, 4), (ManaType::Black, 2)],
        )
        .into_state(),
    )
    .into_state();
    illegal(
        &pending,
        A,
        combat_declaration(
            &pending,
            DecisionReply::EntryChoice(EntryChoice::Players([B, B])),
        ),
    );
    let mut resolved = reply(
        &pending,
        DecisionReply::EntryChoice(EntryChoice::Players([B, C])),
    )
    .into_state();
    let dragon = physical(&resolved, 1).id();
    let sheet = resolved
        .execution()
        .current_characteristics(dragon)
        .unwrap()
        .unwrap();
    assert!(sheet.keywords.contains(&Flying));
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Battlefield);
    for (card, target, other, life) in [(2, B, C, 17), (3, C, B, 14)] {
        let hit = round(targeted(&resolved, A, card, TargetRef::Player(target)).into_state())
            .into_state();
        assert_eq!(hit.execution().life_total(target), Some(&life.into()));
        assert_eq!(hit.execution().life_total(other), Some(&(life + 3).into()));
        resolved = round(hit).into_state();
        assert_eq!(resolved.execution().life_total(other), Some(&life.into()));
        assert!(resolved.execution().window().entries().is_empty());
    }
    let unaffected =
        round(targeted(&resolved, A, 4, TargetRef::Player(A)).into_state()).into_state();
    assert_eq!(unaffected.execution().life_total(A), Some(&17.into()));
    assert_eq!(unaffected.execution().life_total(B), Some(&14.into()));
    assert_eq!(unaffected.execution().life_total(C), Some(&14.into()));
    assert!(unaffected.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_predation_fights_each_opposing_creature_in_one_damage_event() {
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Ezuri's Predation"),
            creature(2, B, 1, 1, &[Lifelink]),
            creature(3, C, 4, 4, &[]),
            creature(4, B, 5, 5, &[]),
            creature(5, A, 8, 8, &[]),
        ],
        &[(ManaType::Colourless, 5), (ManaType::Green, 3)],
        Step::PrecombatMain,
    );
    let ordering = round(
        cast_untargeted(
            &initial,
            1,
            &[(ManaType::Colourless, 5), (ManaType::Green, 3)],
        )
        .into_state(),
    )
    .into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("Beast timestamp order");
    };
    let resolved = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    let batches: Vec<_> = events(&resolved)
        .map(|event| {
            event
                .occurrences
                .iter()
                .filter(|row| matches!(row.value, RulesOccurrence::DamageDealt { .. }))
                .count()
        })
        .filter(|&count| count > 0)
        .collect();
    assert_eq!(batches, [6]);
    let result = resolved.into_state();
    let beast = result
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .unwrap();
    assert_eq!(beast.controller(), Some(A));
    let survivors = result
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .count();
    assert_eq!(survivors, 1);
    assert_eq!(
        result.execution().battlefield_marked_damage(beast.id()),
        Some(1.into())
    );
    assert_eq!(
        result
            .execution()
            .battlefield_marked_damage(physical(&result, 4).id()),
        Some(4.into())
    );
    assert_eq!(
        result
            .execution()
            .battlefield_marked_damage(physical(&result, 5).id()),
        Some(0.into())
    );
    assert_eq!(physical(&result, 2).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&result, 3).zone(), ObjectZone::Graveyard(C));
    assert_eq!(result.execution().life_total(B), Some(&21.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_harsh_mercy_collects_all_choices_before_destroying_unchosen_types() {
    let (_, vocabulary, _) = current_catalogue();
    let make = |id, owner, label, keywords: &[KeywordAbility]| {
        let mut card = creature(id, owner, 2, 2, keywords);
        card.program
            .creature_subtypes
            .insert(vocabulary.resolve_label(label).unwrap());
        card
    };
    let mut shielded = make(5, A, "Giant", &[]);
    shielded.owner = C;
    use spellwake_engine::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    shielded.program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_source()],
        ),
    });
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Harsh Mercy"),
            make(2, A, "Elf", &[]),
            make(3, B, "Goblin", &[]),
            make(4, C, "Zombie", &[]),
            shielded,
            make(6, B, "Giant", &[Indestructible]),
        ],
        &[(ManaType::Colourless, 2), (ManaType::White, 1)],
        Step::PrecombatMain,
    );
    let regenerate = GameCommand::ActivateUntargetedAbility {
        source: physical(&initial, 5).id(),
        ability: ActivatedAbilityId(0),
        payment: None,
    };
    let protected = round(command(&initial, A, regenerate).into_state()).into_state();
    assert!(
        protected
            .execution()
            .regeneration_shields()
            .contains_key(&physical(&protected, 5).id())
    );
    let mut choosing = round(
        cast_untargeted(
            &protected,
            1,
            &[(ManaType::Colourless, 2), (ManaType::White, 1)],
        )
        .into_state(),
    )
    .into_state();
    for (player, kind) in [(A, "Elf"), (B, "Goblin"), (C, "Zombie")] {
        assert_eq!(choosing.pending_decision().unwrap().chooser(), player);
        assert_eq!(physical(&choosing, 5).zone(), ObjectZone::Battlefield);
        choosing = reply(
            &choosing,
            DecisionReply::PromptChoice(
                spellwake_engine::toolbox::PromptChoiceReply::CreatureType(
                    vocabulary.resolve_label(kind).unwrap(),
                ),
            ),
        )
        .into_state();
    }
    for survivor in [2, 3, 4, 6] {
        assert_eq!(
            physical(&choosing, survivor).zone(),
            ObjectZone::Battlefield
        );
    }
    assert_eq!(physical(&choosing, 5).zone(), ObjectZone::Graveyard(C));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_titan_of_littjara_counts_each_matching_creature_once_then_discards() {
    use spellwake_engine::effects::OptionalEffectChoice;
    let (_, vocabulary, _) = current_catalogue();
    let sliver = vocabulary.resolve_label("Sliver").unwrap();
    let illusion = vocabulary.resolve_label("Illusion").unwrap();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let typed = |id, owner, types: &[spellwake_contracts::creature_types::CreatureTypeId]| {
        let mut seed = creature(id, owner, 2, 2, &[]);
        seed.program.creature_subtypes.extend(types);
        seed
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Titan of Littjara"),
            typed(2, A, &[sliver, illusion]),
            typed(3, A, &[illusion]),
            typed(4, A, &[elf]),
            typed(5, B, &[sliver]),
            named("Forest", 6, A, ObjectDestination::LibraryBottom),
            named("Island", 7, A, ObjectDestination::LibraryBottom),
            named("Swamp", 8, A, ObjectDestination::LibraryBottom),
            named("Plains", 9, A, ObjectDestination::LibraryBottom),
        ],
        &[(ManaType::Colourless, 4), (ManaType::Blue, 2)],
        Step::PrecombatMain,
    );
    let entry = round(
        cast_untargeted(
            &initial,
            1,
            &[(ManaType::Colourless, 4), (ManaType::Blue, 2)],
        )
        .into_state(),
    )
    .into_state();
    let entered = reply(
        &entry,
        DecisionReply::EntryChoice(EntryChoice::CreatureType(sliver)),
    )
    .into_state();
    let titan = physical(&entered, 1).id();
    let view = entered
        .execution()
        .current_characteristics(titan)
        .unwrap()
        .unwrap();
    assert_eq!(view.current_creature_subtypes, [illusion, sliver].into());
    let may = round(entered).into_state();
    let decline = reply(
        &may,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&decline, 6).zone(), ObjectZone::Library(A));
    let drew = reply(
        &may,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    for card in 6..=7 {
        assert_eq!(physical(&drew, card).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(physical(&drew, 8).zone(), ObjectZone::Library(A));
    let done = reply(
        &drew,
        DecisionReply::SelectResolution(vec![physical(&drew, 7).id()]),
    )
    .into_state();
    assert_eq!(physical(&done, 7).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&done, 6).zone(), ObjectZone::Hand(A));

    let attack_start = next_main(done, 3);
    let hand_size = |state: &GameState| {
        state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|object| object.zone() == ObjectZone::Hand(A))
            .count()
    };
    let before = hand_size(&attack_start);
    let attacking = attack(&attack_start, 1, B);
    let attack_trigger = round(attacking).into_state();
    let during_attack = reply(
        &attack_trigger,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(hand_size(&during_attack), before);
    assert_eq!(
        during_attack
            .execution()
            .current_characteristics(titan)
            .unwrap()
            .unwrap()
            .current_creature_subtypes,
        [illusion, sliver].into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rukarumel_changes_owned_creature_cards_and_controlled_creatures_not_other_tokens() {
    let (_, vocabulary, _) = current_catalogue();
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let sliver = vocabulary.resolve_label("Sliver").unwrap();
    let types = |state: &GameState, object| {
        state
            .execution()
            .current_characteristics(object)
            .unwrap()
            .unwrap()
            .current_creature_subtypes
    };
    let price = [
        (ManaType::White, 1),
        (ManaType::Blue, 1),
        (ManaType::Black, 1),
        (ManaType::Red, 1),
        (ManaType::Green, 1),
    ];
    let initial = game(
        vec![A, B],
        vec![
            hand("Rukarumel, Biologist"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            named("Fencing Ace", 4, A, ObjectDestination::Graveyard),
            named("Fencing Ace", 5, B, ObjectDestination::Graveyard),
            named("Fencing Ace", 6, A, ObjectDestination::Hand),
            named("Dragon Fodder", 7, A, ObjectDestination::Hand),
            named(
                "Forest",
                8,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Forest",
                9,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Forest",
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[
            (ManaType::White, 2),
            (ManaType::Blue, 1),
            (ManaType::Black, 1),
            (ManaType::Red, 2),
            (ManaType::Green, 1),
            (ManaType::Colourless, 4),
        ],
        Step::PrecombatMain,
    );
    let entry = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let active = reply(
        &entry,
        DecisionReply::EntryChoice(EntryChoice::CreatureType(zombie)),
    )
    .into_state();
    for id in [1, 2, 4, 6] {
        assert!(types(&active, physical(&active, id).id()).contains(&zombie));
    }
    for id in [3, 5, 7] {
        assert!(!types(&active, physical(&active, id).id()).contains(&zombie));
    }
    let ace = cast_untargeted(
        &active,
        6,
        &[(ManaType::Colourless, 1), (ManaType::White, 1)],
    )
    .into_state();
    assert_eq!(physical(&ace, 6).zone(), ObjectZone::Stack);
    assert!(types(&ace, physical(&ace, 6).id()).contains(&zombie));
    let entered = round(ace).into_state();
    let ordering = round(
        cast_untargeted(
            &entered,
            7,
            &[(ManaType::Colourless, 1), (ManaType::Red, 1)],
        )
        .into_state(),
    )
    .into_state();
    let RuleFrame::TimestampOrder(frame) = ordering.pending_decision().unwrap().frame() else {
        panic!("Goblin timestamp order");
    };
    let goblins = reply(
        &ordering,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    for goblin in tokens(&goblins, A, 2) {
        assert!(!types(goblins.state(), goblin).contains(&zombie));
    }

    let mut untapped = next_main(goblins.into_state(), 3);
    for land in 8..=10 {
        untapped = command(
            &untapped,
            A,
            GameCommand::ActivateMana {
                source: physical(&untapped, land).id(),
                ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Forest),
            },
        )
        .into_state();
    }
    let activated = command(
        &untapped,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&untapped, 1).id(),
            ability: spellwake_engine::activated::ActivatedAbilityId(0),
            payment: Some(spend(&[(ManaType::Green, 3)])),
        },
    )
    .into_state();
    assert!(physical(&activated, 1).tapped().unwrap());
    let created = round(activated);
    let sliver_tokens: Vec<_> = events(&created)
        .flat_map(|event| &event.occurrences)
        .filter_map(|occurrence| match &occurrence.value {
            RulesOccurrence::TokenCreated { after, .. } => Some(after.object.id()),
            _ => None,
        })
        .collect();
    let [token] = sliver_tokens.as_slice() else {
        panic!("one created Sliver");
    };
    assert_eq!(types(created.state(), *token), [sliver, zombie].into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_akiri_draws_once_per_attacked_player_then_protects_the_equipment_host() {
    use spellwake_engine::activated::ActivatedAbilityId;
    let equipment = |id| {
        named(
            "Lightning Greaves",
            id,
            A,
            ObjectDestination::Battlefield { controller: A },
        )
    };
    let mana_land = named(
        "Plains",
        10,
        A,
        ObjectDestination::Battlefield { controller: A },
    );
    let cost = [
        (ManaType::Colourless, 1),
        (ManaType::White, 1),
        (ManaType::Red, 1),
    ];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Akiri, Fearless Voyager"),
            creature(2, A, 2, 2, &[]),
            creature(3, A, 2, 2, &[]),
            creature(4, A, 2, 2, &[]),
            equipment(5),
            equipment(6),
            equipment(7),
            named("Forest", 8, A, ObjectDestination::LibraryBottom),
            named("Island", 9, A, ObjectDestination::LibraryBottom),
            mana_land,
        ],
        &cost,
        Step::PrecombatMain,
    );
    let mut game = cast_named(&initial, &cost);
    for (item, host) in [(5, 2), (6, 3), (7, 4)] {
        let equip = GameCommand::ActivateAbility {
            source: physical(&game, item).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(physical(&game, host).id()),
            payment: None,
        };
        game = round(command(&game, A, equip).into_state()).into_state();
        assert_eq!(
            game.execution().attached_to(physical(&game, item).id()),
            Some(TargetRef::Object(physical(&game, host).id()))
        );
    }
    let attackers = combat_frame(&game, true);
    let declarations = [(2, B), (3, B), (4, C)]
        .into_iter()
        .map(|(id, defender)| AttackDeclaration {
            attacker: physical(&attackers, id).id(),
            defender,
            defending_planeswalker: None,
        })
        .collect();
    let declared = reply(&attackers, DecisionReply::DeclareAttackers(declarations)).into_state();
    let stacked = if let Some(pending) = declared.pending_decision() {
        let RuleFrame::Triggers(frame) = pending.frame() else {
            panic!("trigger order");
        };
        assert_eq!(frame.instances().len(), 2);
        reply(
            &declared,
            DecisionReply::OrderTriggers(frame.instances().to_vec()),
        )
        .into_state()
    } else {
        declared
    };
    assert_eq!(stacked.execution().window().entries().len(), 2);
    let drawn = round(round(stacked).into_state()).into_state();
    for id in [8, 9] {
        assert_eq!(physical(&drawn, id).zone(), ObjectZone::Hand(A));
    }
    let funded = command(
        &drawn,
        A,
        GameCommand::ActivateMana {
            source: physical(&drawn, 10).id(),
            ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Plains),
        },
    )
    .into_state();
    let activate = GameCommand::ActivateUntargetedAbility {
        source: physical(&funded, 1).id(),
        ability: ActivatedAbilityId(0),
        payment: Some(spend(&[(ManaType::White, 1)])),
    };
    let optional = round(command(&funded, A, activate).into_state()).into_state();
    let equipment = physical(&optional, 5).id();
    let host = physical(&optional, 2).id();
    let unchanged = reply(&optional, DecisionReply::SelectResolution(vec![])).into_state();
    assert_eq!(
        unchanged.execution().attached_to(equipment),
        Some(TargetRef::Object(host))
    );
    let detached = reply(&optional, DecisionReply::SelectResolution(vec![equipment])).into_state();
    assert_eq!(detached.execution().attached_to(equipment), None);
    assert_eq!(physical(&detached, 2).tapped(), Some(true));
    assert!(
        detached
            .execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap()
            .keywords
            .contains(&Indestructible)
    );
    let later = next_main(detached, 2);
    assert!(
        !later
            .execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap()
            .keywords
            .contains(&Indestructible)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_shirei_restores_its_owners_small_creature_only_if_same_source_survives() {
    use spellwake_engine::effects::OptionalEffectChoice;
    let destroy = |id| {
        seed(
            id,
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
        )
    };
    let cost = [(ManaType::Colourless, 4), (ManaType::Black, 1)];
    let mut stolen = creature(2, B, 1, 1, &[]);
    stolen.owner = A;
    let start = game(
        vec![A, B],
        vec![
            hand("Shirei, Shizo's Caretaker"),
            stolen,
            creature(3, A, 2, 2, &[]),
            destroy(4),
            destroy(5),
            destroy(6),
            named("Ghostly Flicker", 7, A, ObjectDestination::Hand),
            named(
                "Forest",
                8,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[
            (ManaType::Colourless, 6),
            (ManaType::Black, 1),
            (ManaType::Blue, 1),
        ],
        Step::PrecombatMain,
    );
    let mut battlefield = cast_named(&start, &cost);
    for (spell, target) in [(4, 3), (5, 2)] {
        let object = physical(&battlefield, target).id();
        battlefield =
            round(targeted(&battlefield, A, spell, TargetRef::Object(object)).into_state())
                .into_state();
    }
    assert_eq!(physical(&battlefield, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&battlefield, 3).zone(), ObjectZone::Graveyard(A));
    let scheduled = round(battlefield).into_state();
    let end_offer = |state: GameState| {
        exile_hand::until(state, |s| {
            s.turn().unwrap().position().step == Step::End
                && (s.pending_decision().is_some() || !s.execution().window().entries().is_empty())
        })
    };
    let gone = round(
        targeted(
            &scheduled,
            A,
            6,
            TargetRef::Object(physical(&scheduled, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    let next = next_main(gone, 2);
    assert_eq!(physical(&next, 2).zone(), ObjectZone::Graveyard(A));
    let original = physical(&scheduled, 1).id();
    let blink = command(
        &scheduled,
        A,
        GameCommand::CastWithResources {
            spell: physical(&scheduled, 7).id(),
            target: None,
            payment: CastingPayment {
                permanent_targets: [original, physical(&scheduled, 8).id()]
                    .map(TargetRef::Object)
                    .to_vec(),
                mana: Some(spend(&[(ManaType::Colourless, 2), (ManaType::Blue, 1)])),
                ..Default::default()
            },
        },
    )
    .into_state();
    let mut new_source = round(blink).into_state();
    if let Some(RuleFrame::LinkedReturnOrder(frame)) = new_source
        .pending_decision()
        .map(|decision| decision.frame())
    {
        new_source = reply(
            &new_source,
            DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
        )
        .into_state();
    }
    assert_eq!(physical(&new_source, 1).zone(), ObjectZone::Battlefield);
    assert_ne!(physical(&new_source, 1).id(), original);
    let after_blink = next_main(new_source, 2);
    assert_eq!(physical(&after_blink, 2).zone(), ObjectZone::Graveyard(A));
    let offer = end_offer(scheduled);
    let may = round(offer).into_state();
    let declined = reply(
        &may,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&declined, 2).zone(), ObjectZone::Graveyard(A));
    let restored = reply(
        &may,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(physical(&restored, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&restored, 2).controller(), Some(A));
    assert_eq!(physical(&restored, 3).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_pollenbright_druid_chooses_targeted_counter_or_targetless_proliferate() {
    use spellwake_engine::mana::CastMode;
    let cost = [(ManaType::Colourless, 1), (ManaType::Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Pollenbright Druid"),
            named("Pollenbright Druid", 2, A, ObjectDestination::Hand),
            creature(3, A, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
        ],
        &[(ManaType::Colourless, 2), (ManaType::Green, 2)],
        Step::PrecombatMain,
    );
    let first = round(cast_untargeted(&initial, 1, &cost).into_state()).into_state();
    let target = physical(&first, 3).id();
    let select = |mode, target| {
        DecisionReply::TriggerModes(vec![CastMode {
            mode,
            target,
            targets: vec![],
        }])
    };
    let RuleFrame::TriggerTarget(frame) = first.pending_decision().unwrap().frame() else {
        panic!("mode decision");
    };
    assert_eq!(frame.modal().unwrap().modes.len(), 2);
    illegal(&first, A, combat_declaration(&first, select(0, None)));
    let counter = reply(&first, select(0, Some(TargetRef::Object(target)))).into_state();
    let countered = round(counter).into_state();
    let power = |state: &GameState, id| {
        state
            .execution()
            .power_toughness(id)
            .unwrap()
            .unwrap()
            .derived
            .power
    };
    assert_eq!(power(&countered, target), 3.into());
    assert_eq!(power(&countered, physical(&countered, 4).id()), 2.into());

    let second = round(cast_untargeted(&countered, 2, &cost).into_state()).into_state();
    illegal(
        &second,
        A,
        combat_declaration(&second, select(1, Some(TargetRef::Object(target)))),
    );
    let queued = reply(&second, select(1, None)).into_state();
    let proliferate = round(queued).into_state();
    let declined = reply(&proliferate, DecisionReply::Proliferate(vec![])).into_state();
    assert_eq!(power(&declined, target), 3.into());
    let increased = reply(
        &proliferate,
        DecisionReply::Proliferate(vec![TargetRef::Object(target)]),
    )
    .into_state();
    assert_eq!(power(&increased, target), 4.into());
    assert_eq!(power(&increased, physical(&increased, 4).id()), 2.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_kaldra_living_weapon_stabilizes_the_germ_and_exiles_a_surviving_blocker() {
    use spellwake_engine::activated::ActivatedAbilityId;
    let initial = game(
        vec![A, B],
        vec![
            hand("Kaldra Compleat"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 1, 12, &[]),
        ],
        &[(ManaType::Colourless, 14)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &[(ManaType::Colourless, 7)]);
    let equipment = physical(&entered, 1).id();
    let germ_made = round(entered);
    let germ = tokens(&germ_made, A, 1)[0];
    let armed = germ_made.into_state();
    let living = armed
        .execution()
        .current_characteristics(germ)
        .unwrap()
        .unwrap();
    assert_eq!(
        living.keywords,
        [FirstStrike, Trample, Indestructible, Haste].into()
    );
    assert_eq!(
        living.power_toughness.unwrap().derived,
        PowerToughness {
            power: 5.into(),
            toughness: 5.into()
        }
    );
    assert_eq!(
        armed.execution().attached_to(equipment),
        Some(TargetRef::Object(germ))
    );
    assert!(
        armed
            .execution()
            .current_characteristics(equipment)
            .unwrap()
            .unwrap()
            .keywords
            .contains(&Indestructible)
    );

    let equip = GameCommand::ActivateAbility {
        source: equipment,
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(physical(&armed, 2).id()),
        payment: Some(spend(&[(ManaType::Colourless, 7)])),
    };
    let transfer = round(command(&armed, A, equip).into_state()).into_state();
    assert!(transfer.execution().window().cards().get(germ).is_none());
    assert_eq!(
        transfer.execution().attached_to(equipment),
        Some(TargetRef::Object(physical(&transfer, 2).id()))
    );

    let declare = combat_frame(&armed, true);
    let attacks = reply(
        &declare,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: germ,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let blocks = blockers(&attacks);
    let blocked = reply(
        &blocks,
        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: physical(&blocks, 3).id(),
            attacker: germ,
        }]),
    )
    .into_state();
    let first = round(blocked).into_state();
    assert_eq!(
        first
            .execution()
            .battlefield_marked_damage(physical(&first, 3).id()),
        Some(5.into())
    );
    assert_eq!(
        first.execution().battlefield_marked_damage(germ),
        Some(0.into())
    );
    let exiled = round(first).into_state();
    assert_eq!(physical(&exiled, 3).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ghoulish_impetus_returns_from_its_owners_graveyard_and_reattaches() {
    let price = [(ManaType::Black, 1), (ManaType::Colourless, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Ghoulish Impetus"),
            creature(2, B, 2, 2, &[]),
            creature(3, C, 3, 3, &[]),
            remove_spell(4, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastPaid {
            spell: physical(&initial, 1).id(),
            target: TargetRef::Object(physical(&initial, 2).id()),
            payment: spend(&price),
        },
    )
    .into_state();
    let enchanted = round(cast).into_state();
    let aura = physical(&enchanted, 1).id();
    let host = physical(&enchanted, 2).id();
    let assert_host = |state: &GameState, aura, host, power| {
        assert_eq!(
            state.execution().attached_to(aura),
            Some(TargetRef::Object(host))
        );
        let view = state
            .execution()
            .current_characteristics(host)
            .unwrap()
            .unwrap();
        assert_eq!(view.power_toughness.unwrap().derived.power, power);
        assert!(view.keywords.contains(&Deathtouch));
    };
    assert_host(&enchanted, aura, host, 3.into());
    let stalled = next_main(enchanted.clone(), 2);
    let attackers = combat_frame(&stalled, true);
    illegal(
        &attackers,
        B,
        combat_declaration(&attackers, DecisionReply::DeclareAttackers(vec![])),
    );
    illegal(
        &attackers,
        B,
        combat_declaration(
            &attackers,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: host,
                defender: A,
                defending_planeswalker: None,
            }]),
        ),
    );
    reply(
        &attackers,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: host,
            defender: C,
            defending_planeswalker: None,
        }]),
    );

    let dead = round(targeted(&enchanted, A, 4, TargetRef::Object(host)).into_state()).into_state();
    assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&dead, 2).zone(), ObjectZone::Graveyard(B));
    let scheduled = round(dead).into_state();
    let end_step = exile_hand::until(scheduled, |state| {
        state.turn().unwrap().position().step == Step::End
            && !state.execution().window().entries().is_empty()
    });
    let return_offer = round(end_step).into_state();
    let new_host = physical(&return_offer, 3).id();
    let returned = reply(
        &return_offer,
        DecisionReply::LinkedAuraChoice(TargetRef::Object(new_host)),
    )
    .into_state();
    let new_aura = physical(&returned, 1).id();
    assert_ne!(new_aura, aura);
    assert_host(&returned, new_aura, new_host, 4.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rukarumel_changes_permanent_spell_copies_but_does_not_copy_the_type_grant() {
    use ManaType::*;
    let (vocabulary, price) = (
        current_catalogue().1,
        [(White, 1), (Blue, 1), (Black, 1), (Red, 1), (Green, 1)],
    );
    let chosen = vocabulary.resolve_label("Sliver").unwrap();
    let setup = game(
        vec![A, B],
        vec![
            hand("Rukarumel, Biologist"),
            named("Fencing Ace", 2, A, ObjectDestination::Hand),
            named(
                "Hatchery Sliver",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[
            (Colourless, 2),
            (White, 3),
            (Blue, 1),
            (Black, 1),
            (Red, 1),
            (Green, 1),
        ],
        Step::PrecombatMain,
    );
    let entering = round(cast_untargeted(&setup, 1, &price).into_state()).into_state();
    let ruled = reply(
        &entering,
        DecisionReply::EntryChoice(EntryChoice::CreatureType(chosen)),
    )
    .into_state();
    let replicate = GameCommand::CastWithResources {
        spell: physical(&ruled, 2).id(),
        target: None,
        payment: CastingPayment {
            additional: [(0, 1.into())].into(),
            mana: Some(spend(&[(Colourless, 2), (White, 2)])),
            ..Default::default()
        },
    };
    let cast = command(&ruled, A, replicate).into_state();
    let copied = round(cast).into_state();
    let mut copies = copied.execution().window().spell_copies().values();
    let copy = copies.next().unwrap();
    assert!(copies.next().is_none());
    let copy_id = copy.permanent_object.unwrap();
    let copy_view = copied
        .execution()
        .current_characteristics(copy_id)
        .unwrap()
        .unwrap();
    assert_eq!(copy_view.zone, ObjectZone::Stack);
    assert!(copy_view.current_creature_subtypes.contains(&chosen));
    assert!(!copy_view.defined.creature_subtypes.contains(&chosen));

    let token_entered = round(copied).into_state();
    let token = token_entered
        .execution()
        .window()
        .cards()
        .iter()
        .find(|object| object.zone() == ObjectZone::Battlefield && object.is_token())
        .unwrap();
    let token_view = token_entered
        .execution()
        .current_characteristics(token.id())
        .unwrap()
        .unwrap();
    assert!(!token_view.current_creature_subtypes.contains(&chosen));

    let original_entered = round(token_entered).into_state();
    let original = original_entered
        .execution()
        .current_characteristics(physical(&original_entered, 2).id())
        .unwrap()
        .unwrap();
    assert_eq!(original.zone, ObjectZone::Battlefield);
    assert!(original.current_creature_subtypes.contains(&chosen));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_predatory_rampage_enforces_only_possible_blocking_requirements() {
    let price = [(ManaType::Colourless, 3), (ManaType::Green, 2)];
    for two_blockers in [false, true] {
        let mut cards = vec![
            hand("Predatory Rampage"),
            creature(2, A, 2, 2, &[Haste, Menace]),
            creature(3, B, 1, 3, &[]),
        ];
        if two_blockers {
            cards.push(creature(4, B, 1, 3, &[]));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let resolved = cast_named(&initial, &price);
        let attacker = physical(&resolved, 2).id();
        assert_eq!(
            resolved
                .execution()
                .power_toughness(attacker)
                .unwrap()
                .unwrap()
                .derived
                .power,
            5.into()
        );
        let attacking = attack(&resolved, 2, B);
        let none = DecisionReply::DeclareBlockers(vec![]);
        if two_blockers {
            let blocking = blockers(&attacking);
            illegal(&blocking, B, combat_declaration(&blocking, none));
            illegal(
                &blocking,
                B,
                combat_declaration(&blocking, block_choice(&blocking, 3, 2)),
            );
            let both = [3, 4]
                .into_iter()
                .map(|id| BlockDeclaration {
                    blocker: physical(&blocking, id).id(),
                    attacker,
                })
                .collect();
            reply(&blocking, DecisionReply::DeclareBlockers(both));
        } else {
            let next = exile_hand::until(attacking, |state| {
                state.pending_decision().is_some()
                    || state.turn().unwrap().position().step == Step::CombatDamage
            });
            if let Some(decision) = next.pending_decision() {
                assert!(matches!(
                    decision.frame(),
                    RuleFrame::Combat(CombatDecision::Blockers(_))
                ));
                reply(&next, none);
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tempt_with_vengeance_creates_simultaneous_opponent_tokens_then_separate_rewards() {
    use spellwake_engine::effects::OptionalEffectChoice::{Decline, Take};
    let price = [(ManaType::Colourless, 2), (ManaType::Red, 1)];
    let setup = game(
        vec![A, B, C],
        vec![hand("Tempt with Vengeance")],
        &price,
        Step::PrecombatMain,
    );
    let announced = GameCommand::CastWithResources {
        spell: physical(&setup, 1).id(),
        target: None,
        payment: CastingPayment {
            chosen_x: Some(2.into()),
            mana: Some(spend(&price)),
            ..Default::default()
        },
    };
    let initial = round(command(&setup, A, announced).into_state()).into_state();
    let order = |state: &GameState| {
        let RuleFrame::TimestampOrder(frame) = state.pending_decision().unwrap().frame() else {
            panic!("simultaneous token timestamp order");
        };
        reply(
            state,
            DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
        )
    };
    let after_initial = order(&initial).into_state();
    assert_eq!(after_initial.pending_decision().unwrap().chooser(), B);
    let counts = |state: &GameState| {
        let mut result = [0; 3];
        for token in state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        {
            let controller = token.controller().unwrap();
            result[usize::try_from(controller.0).unwrap()] += 1;
            let view = state
                .execution()
                .current_characteristics(token.id())
                .unwrap()
                .unwrap();
            assert!(view.has_keyword(Haste));
            assert_eq!(view.current_colors, [Color::Red].into());
            assert_eq!(
                view.current_creature_subtypes,
                [current_catalogue().1.resolve_label("Elemental").unwrap()].into()
            );
            assert_eq!(
                view.power_toughness.unwrap().derived,
                PowerToughness {
                    power: 1.into(),
                    toughness: 1.into()
                }
            );
        }
        result
    };
    for (decisions, expected) in [
        ([Take, Take], [6, 2, 2]),
        ([Take, Decline], [4, 2, 0]),
        ([Decline, Decline], [2, 0, 0]),
    ] {
        let mut state = after_initial.clone();
        let mut votes = Vec::new();
        for (chooser, decision) in [B, C].into_iter().zip(decisions) {
            let pending = state.pending_decision().unwrap();
            assert_eq!(pending.chooser(), chooser);
            let RuleFrame::OptionalEffect(frame) = pending.frame() else {
                panic!("tempting offer decision");
            };
            let (amount, previous) = frame.token_offer().unwrap();
            assert_eq!(*amount, 2.into());
            assert_eq!(previous, votes.as_slice());
            assert_eq!(counts(&state), [2, 0, 0]);
            state = reply(&state, DecisionReply::OptionalEffect(decision)).into_state();
            votes.push((chooser, decision == Take));
        }
        let mut batches = vec![];
        while state.pending_decision().is_some() {
            let progress = order(&state);
            batches.extend(events(&progress).filter_map(|event| {
                let count = event
                    .occurrences
                    .iter()
                    .filter(|o| matches!(o.value, RulesOccurrence::TokenCreated { .. }))
                    .count();
                (count > 0).then_some(count)
            }));
            state = progress.into_state();
        }
        assert_eq!(counts(&state), expected);
        assert_eq!(physical(&state, 1).zone(), ObjectZone::Graveyard(A));
        if decisions == [Take, Take] {
            assert_eq!(batches, [4, 2, 2]);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_hamza_reduces_its_own_cast_then_creature_spells_per_counter_bearing_creature() {
    use ManaType::*;
    let setup = game(
        vec![A, B],
        vec![
            hand("Hamza, Guardian of Arashin"),
            creature(2, A, 2, 2, &[]),
            creature(3, A, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
            named("Pollenbright Druid", 5, A, ObjectDestination::Hand),
            named("Pollenbright Druid", 6, A, ObjectDestination::Hand),
            named("Palladium Myr", 7, A, ObjectDestination::Hand),
            named("Mind Stone", 8, A, ObjectDestination::Hand),
            remove_spell(9, A),
        ],
        &[(Colourless, 5), (White, 1), (Green, 3)],
        Step::PrecombatMain,
    );
    let grow = |state: &GameState, druid, recipient| {
        let entering =
            round(cast_untargeted(state, druid, &[(Colourless, 1), (Green, 1)]).into_state())
                .into_state();
        let selected = reply(
            &entering,
            DecisionReply::TriggerModes(vec![spellwake_engine::mana::CastMode {
                mode: 0,
                target: Some(TargetRef::Object(physical(&entering, recipient).id())),
                targets: vec![],
            }]),
        )
        .into_state();
        round(selected).into_state()
    };
    let one = grow(&setup, 5, 2);
    let two = grow(&one, 6, 3);
    let doubled = grow(&one, 6, 2);
    let opposing = grow(&one, 6, 4);
    let reduced = [(Colourless, 2), (White, 1), (Green, 1)];
    let cast = |state: &GameState, id, price| GameCommand::CastUntargeted {
        spell: physical(state, id).id(),
        payment: Some(spend(price)),
    };
    for fewer in [doubled, opposing] {
        illegal(&fewer, A, cast(&fewer, 1, &reduced));
    }
    let hamza = cast_named(&two, &reduced);
    illegal(&hamza, A, cast(&hamza, 8, &[(Colourless, 1)]));
    let removed =
        round(targeted(&hamza, A, 9, TargetRef::Object(physical(&hamza, 1).id())).into_state())
            .into_state();
    illegal(&removed, A, cast(&removed, 7, &[(Colourless, 1)]));
    let myr = round(cast_untargeted(&hamza, 7, &[(Colourless, 1)]).into_state()).into_state();
    assert_eq!(physical(&myr, 7).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_goreclaw_discounts_large_creature_spells_and_buffs_controlled_creatures_at_threshold() {
    use ManaType::*;
    let setup = game(
        vec![A, B],
        vec![
            hand("Goreclaw, Terror of Qal Sisma"),
            named("Myriad Construct", 2, A, ObjectDestination::Hand),
            named("Fencing Ace", 3, A, ObjectDestination::Hand),
            creature(4, A, 3, 3, &[]),
            creature(5, B, 4, 4, &[]),
            named("Forest", 6, B, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 5), (Green, 1), (White, 1)],
        Step::PrecombatMain,
    );
    let bear = cast_named(&setup, &[(Colourless, 3), (Green, 1)]);
    let cheap_ace = GameCommand::CastUntargeted {
        spell: physical(&bear, 3).id(),
        payment: Some(spend(&[(White, 1)])),
    };
    illegal(&bear, A, cheap_ace);
    let joined = round(cast_untargeted(&bear, 2, &[(Colourless, 2)]).into_state()).into_state();
    assert_eq!(physical(&joined, 2).zone(), ObjectZone::Battlefield);
    let main = next_main(joined, 3);
    let attacking = attack(&main, 1, B);
    let boosted = round(attacking).into_state();
    for (id, power, trample) in [(1, 5, true), (2, 5, true), (4, 3, false), (5, 4, false)] {
        let now = boosted
            .execution()
            .current_characteristics(physical(&boosted, id).id())
            .unwrap()
            .unwrap();
        assert_eq!(now.power_toughness.unwrap().derived.power, power.into());
        assert_eq!(now.keywords.contains(&Trample), trample);
    }
    let cleanup = next_main(boosted, 4);
    for id in [1, 2] {
        let now = cleanup
            .execution()
            .current_characteristics(physical(&cleanup, id).id())
            .unwrap()
            .unwrap();
        assert_eq!(now.power_toughness.unwrap().derived.power, 4.into());
        assert!(!now.keywords.contains(&Trample));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_heartless_hidetsugu_deals_rounded_down_damage_to_all_players_simultaneously() {
    use ManaType::*;
    use spellwake_engine::activated::ActivatedAbilityId;
    let price = [(Colourless, 3), (Red, 2)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Heartless Hidetsugu"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PLAYER,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(3),
                        false,
                    ),
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = cast_named(&initial, &price);
    let activate = |state: &GameState| GameCommand::ActivateUntargetedAbility {
        source: physical(state, 1).id(),
        ability: ActivatedAbilityId(0),
        payment: None,
    };
    illegal(&cast, A, activate(&cast));
    let unequal = round(targeted(&cast, A, 2, TargetRef::Player(B)).into_state()).into_state();
    let next = next_main(unequal, 4);
    let activation = command(&next, A, activate(&next)).into_state();
    assert_eq!(physical(&activation, 1).tapped(), Some(true));
    assert_eq!(activation.execution().life_total(B), Some(&17.into()));
    let result = round(activation);
    for (seat, total) in [(A, 10), (B, 9), (C, 10)] {
        assert_eq!(
            result.state().execution().life_total(seat),
            Some(&total.into())
        );
    }
    let events: Vec<_> = events(&result)
        .map(|event| {
            event
                .occurrences
                .iter()
                .filter(|row| matches!(row.value, RulesOccurrence::DamageDealt { .. }))
                .count()
        })
        .filter(|count| *count > 0)
        .collect();
    assert_eq!(events, [3]);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tromokratis_exposes_itself_in_combat_and_requires_every_defending_creature_to_block() {
    use ManaType::*;
    let price = [(Colourless, 5), (Blue, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Tromokratis"),
            creature(2, B, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            remove_spell(4, B),
        ],
        &price,
        Step::PrecombatMain,
    );
    let ready = cast_named(&initial, &price);
    let remove = |state: &GameState| GameCommand::Cast {
        spell: physical(state, 4).id(),
        target: TargetRef::Object(physical(state, 1).id()),
    };
    let after_priority = pass(&ready).into_state();
    illegal(&after_priority, B, remove(&after_priority));
    let defending = next_main(ready.clone(), 2);
    let hostile = attack(&defending, 2, A);
    let block = blockers(&hostile);
    let defending_kraken = reply(&block, block_choice(&block, 1, 2)).into_state();
    let killed_as_blocker =
        round(command(&defending_kraken, B, remove(&defending_kraken)).into_state()).into_state();
    assert_eq!(
        physical(&killed_as_blocker, 1).zone(),
        ObjectZone::Graveyard(A)
    );
    let our_main = next_main(ready, 3);
    let attacking = attack(&our_main, 1, B);
    let opponent = pass(&attacking).into_state();
    let destroyed = round(command(&opponent, B, remove(&opponent)).into_state()).into_state();
    assert_eq!(physical(&destroyed, 1).zone(), ObjectZone::Graveyard(A));

    let blocking = blockers(&attacking);
    illegal(
        &blocking,
        B,
        combat_declaration(&blocking, block_choice(&blocking, 2, 1)),
    );
    illegal(
        &blocking,
        B,
        combat_declaration(&blocking, block_choice(&blocking, 3, 1)),
    );
    let together = [2, 3]
        .into_iter()
        .map(|id| BlockDeclaration {
            blocker: physical(&blocking, id).id(),
            attacker: physical(&blocking, 1).id(),
        })
        .collect();
    reply(&blocking, DecisionReply::DeclareBlockers(together));
    let open = reply(&blocking, DecisionReply::DeclareBlockers(vec![])).into_state();
    let damage = round(open).into_state();
    assert_eq!(damage.execution().life_total(B), Some(&12.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sower_replaces_an_old_link_when_it_returns_to_the_battlefield() {
    use ManaType::*;
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Sower of Discord"),
            named("Ghostly Flicker", 2, A, ObjectDestination::Hand),
            named(
                "Forest",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Serrated Scorpion", 4, A, ObjectDestination::Hand),
            remove_spell(5, A),
        ],
        &[(Colourless, 6), (Black, 3), (Blue, 1)],
        Step::PrecombatMain,
    );
    let entering = cast_named(&initial, &[(Colourless, 4), (Black, 2)]);
    let linked = reply(
        &entering,
        DecisionReply::EntryChoice(EntryChoice::Players([B, C])),
    )
    .into_state();
    let trigger_damage = |state: &GameState| {
        let entered = round(cast_untargeted(state, 4, &[(Black, 1)]).into_state()).into_state();
        let target = TargetRef::Object(physical(&entered, 4).id());
        let dead = round(targeted(&entered, A, 5, target).into_state()).into_state();
        round(dead).into_state()
    };
    let mut both = trigger_damage(&linked);
    if let Some(decision) = both.pending_decision() {
        let RuleFrame::Triggers(frame) = decision.frame() else {
            panic!("simultaneous linked damage triggers");
        };
        assert_eq!(frame.instances().len(), 2);
        both = reply(
            &both,
            DecisionReply::OrderTriggers(frame.instances().to_vec()),
        )
        .into_state();
    }
    assert_eq!(both.execution().window().entries().len(), 2);
    let threatened = round(round(both).into_state()).into_state();
    assert_eq!(threatened.execution().life_total(B), Some(&16.into()));
    assert_eq!(threatened.execution().life_total(C), Some(&16.into()));
    let old = physical(&linked, 1).id();
    let targets = vec![
        TargetRef::Object(old),
        TargetRef::Object(physical(&linked, 3).id()),
    ];
    let cast = GameCommand::CastWithResources {
        spell: physical(&linked, 2).id(),
        target: None,
        payment: CastingPayment {
            permanent_targets: targets,
            mana: Some(spend(&[(Colourless, 2), (Blue, 1)])),
            ..Default::default()
        },
    };
    let mut returned = round(command(&linked, A, cast).into_state()).into_state();
    while let Some(current) = returned.pending_decision() {
        let response = match current.frame() {
            RuleFrame::LinkedReturnOrder(frame) => {
                DecisionReply::LinkedReturnOrder(frame.objects().to_vec())
            }
            RuleFrame::EntryChoice(_) => DecisionReply::EntryChoice(EntryChoice::Players([A, B])),
            frame => panic!("unexpected blink decision {frame:?}"),
        };
        returned = reply(&returned, response).into_state();
    }
    assert_ne!(physical(&returned, 1).id(), old);

    let damage = trigger_damage(&returned);
    assert_eq!(damage.execution().life_total(A), Some(&22.into()));
    assert_eq!(damage.execution().window().entries().len(), 1);
    let loss = round(damage).into_state();
    assert_eq!(loss.execution().life_total(A), Some(&20.into()));
    assert_eq!(loss.execution().life_total(B), Some(&18.into()));
    assert_eq!(loss.execution().life_total(C), Some(&18.into()));
}
