use super::*;
use crate::keyword_creatures::tests::current_catalogue;
use flate2::read::MultiGzDecoder;
use spellwake_content::CatalogFace;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
};

fn synthetic_vocab() -> CreatureTypeVocabulary {
    let mut members = vec![
        "Army".into(),
        "Beast".into(),
        "Construct".into(),
        "Druid".into(),
        "Elemental".into(),
        "Elf".into(),
        "Goblin".into(),
        "Human".into(),
        "Soldier".into(),
        "Spirit".into(),
        "Warrior".into(),
        "Zombie".into(),
    ];
    members.sort();
    CreatureTypeVocabulary::new("test-vocab".into(), [0u8; 32], members).expect("vocab")
}

fn synthetic_card(
    name: &str,
    type_line: &str,
    mana_cost: Option<&str>,
    pt: Option<(&str, &str)>,
    oracle_text: &str,
) -> InventoryCard {
    InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
        oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
        name: name.into(),
        layout: "normal".into(),
        faces: vec![CatalogFace {
            name: name.into(),
            type_line: Some(type_line.into()),
            oracle_text: Some(oracle_text.into()),
            mana_cost: mana_cost.map(str::to_string),
            power: pt.map(|(p, _)| p.to_string()),
            toughness: pt.map(|(_, t)| t.to_string()),
        }],
        legalities: BTreeMap::new(),
        eligible_formats: Vec::new(),
        required_by_base_formats: true,
        program_evidence: "test",
    }
}

#[test]
fn lowers_core_magic_archetypes_and_rejects_unsupported_clauses() {
    let vocab = synthetic_vocab();

    // 1. Creature with ETB draw
    let elvish = synthetic_card(
        "Elvish Visionary",
        "Creature \u{2014} Elf",
        Some("{1}{G}"),
        Some(("1", "1")),
        "When Elvish Visionary enters, draw a card.",
    );
    let p = compile_with_reason(&elvish, &vocab).expect("elvish visionary");
    assert_eq!(p.triggered.len(), 1);

    // 2. Burn spell with damage to any target
    let bolt = synthetic_card(
        "Lightning Bolt",
        "Instant",
        Some("{R}"),
        None,
        "Lightning Bolt deals 3 damage to any target.",
    );
    let p = compile_with_reason(&bolt, &vocab).expect("bolt");
    assert!(matches!(p.kind, ProgramKind::Spell(_)));

    // 3. Pump + keyword fusion (Aerial Maneuver)
    let trick = synthetic_card(
        "Aerial Maneuver",
        "Instant",
        Some("{1}{W}"),
        None,
        "Target creature gets +1/+1 and gains flying and first strike until end of turn.",
    );
    let p = compile_with_reason(&trick, &vocab).expect("aerial maneuver");
    let ProgramKind::Spell(spell) = p.kind else {
        panic!("expected spell");
    };
    assert_eq!(spell.body.instructions.len(), 1);

    // 4. Gatecrash Battalion trigger
    let battalion = synthetic_card(
        "Ordruun Veteran",
        "Creature \u{2014} Human Soldier",
        Some("{2}{R}{W}"),
        Some(("3", "1")),
        "Battalion \u{2014} Whenever Ordruun Veteran and at least two other creatures attack, Ordruun Veteran gains double strike until end of turn.",
    );
    let p = compile_with_reason(&battalion, &vocab).expect("ordruun veteran");
    assert_eq!(p.triggered.len(), 1);

    // 5. Gatecrash Bloodrush (targeted Channel from hand)
    let bloodrush = synthetic_card(
        "Viashino Shanktail",
        "Creature \u{2014} Warrior",
        Some("{3}{R}"),
        Some(("3", "1")),
        "First strike\nBloodrush \u{2014} {2}{R}, Discard Viashino Shanktail: Target attacking creature gets +3/+1 and gains first strike until end of turn.",
    );
    let p = compile_with_reason(&bloodrush, &vocab).expect("viashino shanktail");
    assert_eq!(p.activated.len(), 1);
    assert_eq!(
        p.activated[0].source_cost,
        SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel)
    );

    // 6. Spark Trooper end-step self-sacrifice
    let spark = synthetic_card(
        "Spark Trooper",
        "Creature \u{2014} Elemental Soldier",
        Some("{1}{R}{R}{W}"),
        Some(("6", "1")),
        "Trample, lifelink, haste\nAt the beginning of the end step, sacrifice Spark Trooper.",
    );
    let p = compile_with_reason(&spark, &vocab).expect("spark trooper");
    assert_eq!(p.triggered.len(), 1);

    // 7. Aura with continuous +2/+2 and flying
    let aura = synthetic_card(
        "Angelic Blessing Aura",
        "Enchantment \u{2014} Aura",
        Some("{1}{W}"),
        None,
        "Enchant creature\nEnchanted creature gets +2/+2 and has flying.",
    );
    let p = compile_with_reason(&aura, &vocab).expect("aura");
    assert!(p.continuous.is_some());

    // 8. Mana artifact
    let rock = synthetic_card(
        "Emerald Relic",
        "Artifact",
        Some("{2}"),
        None,
        "{T}: Add {G}.",
    );
    let p = compile_with_reason(&rock, &vocab).expect("mana rock");
    assert_eq!(p.mana.len(), 1);

    // 9. Strict rejection: never silently drop an unrecognized clause
    let partial = synthetic_card(
        "Unparseable Card",
        "Creature \u{2014} Human",
        Some("{1}{U}"),
        Some(("2", "2")),
        "Flying\nFlibbertigibbet the multiverse twice.",
    );
    assert!(compile_with_reason(&partial, &vocab).is_err());
}

#[test]
fn stage_a_and_b_fidelity_regressions() {
    let vocab = synthetic_vocab();

    // 1. Bytecode jump relocation via prepend_instruction_at preserves all jump destinations
    let mut jump_ops = vec![
        Instruction::FlipCoins {
            player: ResolvingPlayer::Controller,
            mode: spellwake_engine::program::CoinFlipMode::Single,
            win_body: None,
            lose_body: None,
            lose_destination: Some(2),
        },
        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
            ValueExpr::literal(1),
        ),
        Instruction::Jump { destination: 3 },
    ];
    prepend_instruction_at(
        &mut jump_ops,
        0,
        Instruction::OptionalEffect {
            chooser: ResolvingPlayer::Controller,
        },
    );
    assert!(matches!(jump_ops[0], Instruction::OptionalEffect { .. }));
    assert!(matches!(
        jump_ops[1],
        Instruction::FlipCoins {
            lose_destination: Some(3),
            ..
        }
    ));
    assert!(matches!(jump_ops[3], Instruction::Jump { destination: 4 }));

    // And an intervening-if optional trigger compiles cleanly with valid PC bounds
    let cond_opt = synthetic_card(
        "Conditional Optional",
        "Creature \u{2014} Human",
        Some("{1}{U}"),
        Some(("2", "2")),
        "When Conditional Optional enters, if you control an artifact, you may draw a card.",
    );
    let p = compile_with_reason(&cond_opt, &vocab).expect("conditional optional trigger");
    assert!(p.validate(&vocab).is_ok());
    assert_eq!(p.triggered.len(), 1);
    assert!(p.triggered[0].condition.intervening_if.is_some());
    let ops = &p.triggered[0].body.instructions;
    for (idx, op) in ops.iter().enumerate() {
        match op {
            Instruction::Jump { destination }
            | Instruction::BranchIfFalse { destination, .. }
            | Instruction::FlipCoins {
                lose_destination: Some(destination),
                ..
            } => {
                assert!(
                    *destination > idx && *destination <= ops.len(),
                    "corrupted jump destination {destination} at pc {idx} in {ops:?}"
                );
            }
            _ => {}
        }
    }

    // 2. SelfObject "untapped" and "+1/+1 counter" static conditions set StaticSourceCondition
    let untapped_lord = synthetic_card(
        "Untapped Sentinel",
        "Creature \u{2014} Soldier",
        Some("{1}{W}"),
        Some(("2", "2")),
        "As long as Untapped Sentinel is untapped, creatures you control get +1/+1.",
    );
    let p = compile_with_reason(&untapped_lord, &vocab).expect("untapped static condition");
    let has_untapped_source_cond = p.continuous.iter().any(|ab| {
        matches!(
            ab,
            StaticContinuousAbility::Battlefield { selector, .. }
                if selector.source_condition
                    == Some(spellwake_engine::characteristics::StaticSourceCondition::SourceIsUntapped)
                    && !selector.requires_untapped
                    && !selector.requires_tapped
        )
    });
    assert!(
        has_untapped_source_cond,
        "untapped self condition must set SourceIsUntapped, not selector.tapped_filter"
    );

    // 3. Multiplayer drain (EachOpponent + PriorResult) must NOT collapse to single-opponent gain
    let multi_drain = synthetic_card(
        "Multiplayer Drain",
        "Sorcery",
        Some("{2}{B}"),
        None,
        "Each opponent loses 2 life. You gain life equal to the life lost this way.",
    );
    if let Ok(prog) = compile_with_reason(&multi_drain, &vocab)
        && let ProgramKind::Spell(sp) = &prog.kind
    {
        assert!(
            !sp.body.instructions.iter().any(|op| matches!(
                op,
                Instruction::AdjustLife(spec)
                    if matches!(
                        spec.op,
                        spellwake_engine::toolbox::LifeOp::Drain { per_loss: false, .. }
                    )
            )),
            "must not miscompile total multiplayer life lost to single-opponent gain"
        );
    }

    // 4. ForEachObject referencing "it" must desugar cleanly to a group subject (never AbilitySource)
    //    when loop-independent, and fail closed when per-object dependent.
    let group_foreach = synthetic_card(
        "ForEach It",
        "Sorcery",
        Some("{2}{G}"),
        None,
        "For each creature you control, put a +1/+1 counter on it.",
    );
    let foreach_prog = compile_with_reason(&group_foreach, &vocab)
        .expect("loop-independent ForEachObject with 'it' should lower to a group op");
    if let ProgramKind::Spell(sp) = &foreach_prog.kind {
        assert!(
            sp.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::MutateObject(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::ObjectMutation::AddCounters { .. }
                        ) && (matches!(
                            &spec.source,
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(subject)
                                if matches!(
                                    subject.as_ref(),
                                    spellwake_engine::program::EffectSubject::Matching(_)
                                )
                        ) || matches!(
                            &spec.source,
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Battlefield,
                                ..
                            }
                        ))
                )
            }),
            "ForEachObject with 'it' must target controlled creatures group, never source: {:?}",
            sp.body.instructions
        );
    } else {
        panic!("expected Spell program for ForEach It");
    }
    let dependent_foreach = synthetic_card(
        "ForEach Dependent It",
        "Sorcery",
        Some("{2}{G}"),
        None,
        "For each creature you control, put a number of +1/+1 counters on it equal to its power.",
    );
    assert!(
        compile_with_reason(&dependent_foreach, &vocab).is_err(),
        "loop-dependent ForEachObject with 'its power' must fail closed"
    );

    // 5. AdditionalCombat must reject extra upkeep phase text
    let extra_upkeep = synthetic_card(
        "Extra Upkeep",
        "Sorcery",
        Some("{3}{U}"),
        None,
        "After this phase, there is an additional upkeep phase.",
    );
    assert!(
        compile_with_reason(&extra_upkeep, &vocab).is_err(),
        "extra upkeep must not compile to AdditionalCombat"
    );

    // 6. NoMaximumHandSize must reject "Each player has no maximum hand size"
    let global_hand = synthetic_card(
        "Global Hand",
        "Enchantment",
        Some("{2}{U}"),
        None,
        "Each player has no maximum hand size.",
    );
    assert!(
        compile_with_reason(&global_hand, &vocab).is_err(),
        "global no-max-hand-size must not compile to controller-only NoMaximumHandSize"
    );

    // 7. "Each player sacrifices all creatures" lowers to SacrificeSubject(Matching(Creature)),
    //    never to Sacrifice { amount: 1 }; "Sacrifice two creatures" lowers with amount = 2
    let sac_all = synthetic_card(
        "Sac All",
        "Sorcery",
        Some("{1}{B}"),
        None,
        "Each player sacrifices all creatures.",
    );
    let p_all = compile_with_reason(&sac_all, &vocab)
        .expect("sacrifice all creatures should lower via SacrificeSubject");
    let ProgramKind::Spell(sp_all) = &p_all.kind else {
        panic!("expected spell");
    };
    assert!(
        matches!(
            sp_all.body.instructions.as_slice(),
            [Instruction::ZoneOp(spec)]
                if matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(subject)
                        if matches!(
                            subject.as_ref(),
                            spellwake_engine::program::EffectSubject::Matching(filter)
                                if *filter == spellwake_engine::program::ObjectFilter::creature()
                        )
                ) || matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(_),
                        filter,
                        ..
                    } if **filter == spellwake_engine::program::ObjectFilter::creature()
                ) && matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                    }
                )
        ),
        "sacrifice all creatures must lower to ZoneOp(Sacrifice, Matching(Creature)), not sacrifice 1; got {:?}",
        sp_all.body.instructions
    );
    let sac_two = synthetic_card(
        "Sac Two",
        "Sorcery",
        Some("{2}{B}"),
        None,
        "Target player sacrifices two creatures.",
    );
    let p = compile_with_reason(&sac_two, &vocab).expect("sacrifice two creatures");
    let ProgramKind::Spell(sp) = &p.kind else {
        panic!("expected spell");
    };
    assert!(
        matches!(
            sp.body.instructions.as_slice(),
            [Instruction::Sacrifice { amount, .. }] if *amount == ValueExpr::literal(2)
        ),
        "expected Sacrifice amount=2, got {:?}",
        sp.body.instructions
    );

    // 8. DelayedTrigger honors YourNext (own_step_only=true) vs Next (own_step_only=false)
    //    and rejects ThatPlayersNext
    let your_end = synthetic_card(
        "Your End Draw",
        "Instant",
        Some("{U}"),
        None,
        "At the beginning of your next end step, draw a card.",
    );
    let p_your = compile_with_reason(&your_end, &vocab).expect("your next end step");
    let ProgramKind::Spell(sp_your) = &p_your.kind else {
        panic!("expected spell");
    };
    assert!(matches!(
        sp_your.body.instructions.as_slice(),
        [Instruction::ScheduleDelayedStepTrigger {
            step: Step::End,
            own_step_only: true,
            ..
        }]
    ));
    let next_end = synthetic_card(
        "Next End Draw",
        "Instant",
        Some("{U}"),
        None,
        "At the beginning of the next end step, draw a card.",
    );
    let p_next = compile_with_reason(&next_end, &vocab).expect("next end step");
    let ProgramKind::Spell(sp_next) = &p_next.kind else {
        panic!("expected spell");
    };
    assert!(matches!(
        sp_next.body.instructions.as_slice(),
        [Instruction::ScheduleDelayedStepTrigger {
            step: Step::End,
            own_step_only: false,
            ..
        }]
    ));
    let their_end = synthetic_card(
        "Their End Draw",
        "Instant",
        Some("{U}"),
        None,
        "Target player draws a card at the beginning of that player's next end step.",
    );
    assert!(
        compile_with_reason(&their_end, &vocab).is_err(),
        "that player's next end step must fail closed until per-player delayed triggers exist"
    );
}

// Recurse through control-flow and container AST variants that oracle_lowering
// lowers recursively (in body.rs or instructions.rs) so Gate 4 blocker
// attribution surfaces the real leaf blocker instead of the wrapper.
// Unimplemented container combinators (for example TemporaryTrigger,
// TemporarySpecialAction, ForEachColorAmong, WhenYouNext, ManaSpendRider,
// SequentialChoose, ReflexiveEventTrigger, Simultaneous, ForEachCategory) stay
// as leaves so Gate 4 keeps reporting them as named unsupported instructions
// instead of collapsing them into generic *-body buckets.
fn collect_leaf_ins<'a>(ins: &'a [InstructionAst], out: &mut Vec<&'a InstructionAst>) {
    for item in ins {
        match item {
            InstructionAst::Optional(inner)
            | InstructionAst::OptionalPlayer { body: inner, .. }
            | InstructionAst::IfCondition {
                then_body: inner, ..
            }
            | InstructionAst::Otherwise(inner)
            | InstructionAst::ReplaceEffect(inner)
            | InstructionAst::IfPreviousSucceeded(inner)
            | InstructionAst::IfPreviousFailed(inner)
            | InstructionAst::BindX { body: inner, .. }
            | InstructionAst::BindXY { body: inner, .. }
            | InstructionAst::ReflexiveTrigger(inner)
            | InstructionAst::DelayedTrigger { body: inner, .. }
            | InstructionAst::DelayedEventTrigger { body: inner, .. }
            | InstructionAst::ForEachPlayer { body: inner, .. }
            | InstructionAst::ForEachSubjectPlayer { body: inner, .. }
            | InstructionAst::ForEachObject { body: inner, .. }
            | InstructionAst::ForEachCount { body: inner, .. }
            | InstructionAst::ForAsLongAs { body: inner, .. }
            | InstructionAst::SpreeMode { body: inner, .. } => {
                collect_leaf_ins(inner, out);
            }
            InstructionAst::Unless {
                body,
                unless_action,
                ..
            } => {
                collect_leaf_ins(body, out);
                if let Some(action) = unless_action {
                    collect_leaf_ins(action, out);
                }
            }
            InstructionAst::Choice(branches)
            | InstructionAst::InteractiveChoice(InteractiveChoiceKind::VillainousChoice {
                options: branches,
                ..
            }) => {
                for branch in branches {
                    collect_leaf_ins(branch, out);
                }
            }
            InstructionAst::Modal { modes, .. } => {
                for mode in modes {
                    collect_leaf_ins(&mode.body, out);
                }
            }
            _ => out.push(item),
        }
    }
}

#[test]
fn collect_leaf_ins_recurses_into_lowered_containers() {
    let sample = vec![
        InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Metalcraft),
            then_body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::Proliferate,
            ])],
        },
        InstructionAst::SpreeMode {
            cost: Vec::new(),
            body: vec![InstructionAst::EndTurn],
        },
        InstructionAst::BindXY {
            x_expr: Box::new(ValueExprAst::Const(1)),
            y_expr: Box::new(ValueExprAst::Const(2)),
            body: vec![InstructionAst::ClaimPrize],
        },
        InstructionAst::DelayedEventTrigger {
            trigger: TriggerSpecAst {
                events: Vec::new(),
                intervening_if: None,
            },
            body: vec![InstructionAst::Planeswalk],
        },
        InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::Player(PlayerRefAst::EachPlayer)),
            body: vec![InstructionAst::Proliferate],
        },
        InstructionAst::ForAsLongAs {
            condition: Box::new(ConditionAst::Marker(GameMarker::RemainsExiled)),
            body: vec![InstructionAst::EndTurn],
        },
        InstructionAst::Unless {
            body: vec![InstructionAst::ClaimPrize],
            unless_player: None,
            unless_cost: None,
            unless_action: Some(vec![InstructionAst::Planeswalk]),
            unless_condition: None,
        },
        InstructionAst::Choice(vec![
            vec![InstructionAst::Proliferate],
            vec![InstructionAst::EndTurn],
        ]),
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::VillainousChoice {
            chooser: PlayerRefAst::EachOpponent,
            options: vec![
                vec![InstructionAst::ClaimPrize],
                vec![InstructionAst::Planeswalk],
            ],
        }),
        InstructionAst::Modal {
            choice: "Choose one".to_string(),
            modes: vec![spellwake_oracle_compiler::ast::ModalModeAst {
                body: vec![InstructionAst::ReplaceEffect(vec![
                    InstructionAst::Proliferate,
                ])],
                label: None,
                granted: None,
            }],
        },
        // Unimplemented container stays as a leaf so it remains a named blocker.
        InstructionAst::Simultaneous(vec![InstructionAst::EndTurn]),
    ];

    let mut leaves = Vec::new();
    collect_leaf_ins(&sample, &mut leaves);

    assert_eq!(leaves.len(), 14);
    assert!(
        !leaves
            .iter()
            .any(|ins| matches!(ins, InstructionAst::ReplaceEffect(_)))
    );
    assert!(matches!(leaves[0], InstructionAst::Proliferate));
    assert!(matches!(leaves[1], InstructionAst::EndTurn));
    assert!(matches!(leaves[2], InstructionAst::ClaimPrize));
    assert!(matches!(leaves[3], InstructionAst::Planeswalk));
    assert!(matches!(leaves[4], InstructionAst::Proliferate));
    assert!(matches!(leaves[5], InstructionAst::EndTurn));
    assert!(matches!(leaves[6], InstructionAst::ClaimPrize));
    assert!(matches!(leaves[7], InstructionAst::Planeswalk));
    assert!(matches!(leaves[8], InstructionAst::Proliferate));
    assert!(matches!(leaves[9], InstructionAst::EndTurn));
    assert!(matches!(leaves[10], InstructionAst::ClaimPrize));
    assert!(matches!(leaves[11], InstructionAst::Planeswalk));
    assert!(matches!(leaves[12], InstructionAst::Proliferate));
    assert!(matches!(leaves[13], InstructionAst::Simultaneous(_)));
}

#[test]
#[ignore = "uses saved dated Scryfall bulk corpus and creature vocabulary"]
fn oracle_lowering_corpus_coverage_audit() {
    let (inventory, vocabulary, registry) = current_catalogue();

    let total_required = inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
        .count();
    let total_bound = registry
        .coverage()
        .cards
        .iter()
        .filter(|entry| matches!(entry.binding, crate::BindingStatus::Partial { .. }))
        .count();

    // Count how many required cards compile strictly via oracle_lowering alone,
    // and tally drop reasons.
    let mut oracle_ok = 0usize;
    let mut reasons: HashMap<&'static str, usize> = HashMap::new();
    let mut oracle_ok_ids: BTreeSet<String> = BTreeSet::new();
    let mut oracle_errors = BTreeMap::new();

    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let hybrid_face = (card.faces.len() == 1)
            .then(|| super::super::split_hybrid_cost(card.faces[0].mana_cost.as_deref()?))
            .flatten();
        let normalized = hybrid_face.as_ref().map(|(cost, _)| {
            let mut normal = card.clone();
            normal.faces[0].mana_cost = Some(cost.clone());
            normal
        });
        let input = normalized.as_ref().unwrap_or(card);
        match compile_with_inventory_reason(input, inventory, &vocabulary) {
            Ok(_) => {
                oracle_ok += 1;
                if let Some(oid) = &card.oracle_id {
                    oracle_ok_ids.insert(oid.clone());
                }
            }
            Err(r) => {
                *reasons.entry(r).or_insert(0) += 1;
                if let Some(id) = &card.oracle_id {
                    oracle_errors.insert(id.clone(), r);
                }
            }
        }
    }

    // Load set codes from the gzipped oracle-cards input for benchmark sets
    let raw_path =
        std::env::var_os("SPELLWAKE_CATALOGUE_INPUT").expect("SPELLWAKE_CATALOGUE_INPUT");
    let file = File::open(raw_path).expect("open catalogue");
    let reader = BufReader::new(MultiGzDecoder::new(file));
    let mut set_oracles: HashMap<String, BTreeSet<String>> = HashMap::new();
    let target_sets = ["gtc", "isd", "dka", "avr", "cmm"];
    for line in reader.lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line).expect("json");
        let Some(set_code) = v.get("set").and_then(|s| s.as_str()) else {
            continue;
        };
        if !target_sets.contains(&set_code) {
            continue;
        }
        if let Some(oid) = v.get("oracle_id").and_then(|s| s.as_str()) {
            set_oracles
                .entry(set_code.to_string())
                .or_default()
                .insert(oid.to_string());
        }
    }

    let bound_ids: BTreeSet<String> = registry
        .coverage()
        .cards
        .iter()
        .filter_map(|e| match &e.binding {
            crate::BindingStatus::Partial { .. } => e.oracle_id.clone(),
            crate::BindingStatus::Blocked { .. } | crate::BindingStatus::Missing => None,
        })
        .collect();

    eprintln!("=== ORACLE LOWERING CORPUS AUDIT ===");
    eprintln!(
        "Profile::Current total bound: {total_bound} / {total_required} ({:.2}%)",
        (total_bound as f64 / total_required as f64) * 100.0
    );
    eprintln!(
        "oracle_lowering standalone valid: {oracle_ok} / {total_required} ({:.2}%)",
        (oracle_ok as f64 / total_required as f64) * 100.0
    );

    let mut card_by_oid: HashMap<&str, &InventoryCard> = HashMap::new();
    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        if let Some(oid) = card.oracle_id.as_deref() {
            card_by_oid.insert(oid, card);
        }
    }

    for set_code in target_sets {
        let oids = set_oracles.get(set_code).cloned().unwrap_or_default();
        let req_in_set: Vec<&str> = oids
            .iter()
            .map(String::as_str)
            .filter(|id| card_by_oid.contains_key(id))
            .collect();
        let bound_in_set = req_in_set
            .iter()
            .filter(|id| bound_ids.contains(**id))
            .count();
        let oracle_in_set = req_in_set
            .iter()
            .filter(|id| oracle_ok_ids.contains(**id))
            .count();
        eprintln!(
            "Set {set_code}: Current bound = {bound_in_set}/{} ({:.2}%), oracle_lowering = {oracle_in_set}/{} ({:.2}%)",
            req_in_set.len(),
            if req_in_set.is_empty() {
                0.0
            } else {
                (bound_in_set as f64 / req_in_set.len() as f64) * 100.0
            },
            req_in_set.len(),
            if req_in_set.is_empty() {
                0.0
            } else {
                (oracle_in_set as f64 / req_in_set.len() as f64) * 100.0
            },
        );
    }

    let mut sorted_reasons: Vec<(&'static str, usize)> = reasons.into_iter().collect();
    sorted_reasons.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    eprintln!(
        "Top 80 oracle_lowering drop reasons across 32,155 cards ({} distinct reasons):",
        sorted_reasons.len()
    );
    for (reason, count) in sorted_reasons.iter().take(80) {
        eprintln!("  {count:6}  {reason}");
    }

    // Breakdown of Stage 1 AST parse failures across all 32,155 Base Constructed cards
    let mut ast_total_failed_cards = 0usize;
    let mut ast_fail_by_reason: HashMap<String, Vec<String>> = HashMap::new();
    let compiler_vocab_diag = cached_compiler_vocab(&vocabulary);
    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let face_inputs: Vec<FaceInput<'_>> = card
            .faces
            .iter()
            .map(|f| FaceInput {
                name: &f.name,
                type_line: f.type_line.as_deref().unwrap_or(""),
                oracle_text: f.oracle_text.as_deref().unwrap_or(""),
            })
            .collect();
        let ast = compile_card(
            &CardInput {
                oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                name: &card.name,
                layout: &card.layout,
                faces: face_inputs,
            },
            &compiler_vocab_diag,
        );
        if !ast.strictly_compiled {
            ast_total_failed_cards += 1;
            for face in &ast.faces {
                for fail in &face.failures {
                    let norm_reason = if let Some((head, _)) = fail.reason.split_once(':') {
                        head
                    } else {
                        fail.reason.as_str()
                    };
                    let words: Vec<&str> = fail.text.split_whitespace().take(3).collect();
                    let prefix = words.join(" ").to_ascii_lowercase();
                    let key = format!("{}:{norm_reason}:[{prefix}]", fail.stage);
                    let entry = ast_fail_by_reason.entry(key).or_default();
                    if entry.len() < 4 {
                        entry.push(format!("[{}] {}", card.name, fail.text));
                    } else {
                        entry.push(String::new());
                    }
                }
            }
        }
    }
    let mut ast_fail_sorted: Vec<(String, Vec<String>)> = ast_fail_by_reason.into_iter().collect();
    ast_fail_sorted.sort_by_key(|e| std::cmp::Reverse(e.1.len()));
    eprintln!(
        "--- Stage 1 AST parse failures ({ast_total_failed_cards} / {total_required} cards incomplete) ---"
    );
    for (key, samples) in ast_fail_sorted.iter().take(40) {
        let ex: Vec<&str> = samples
            .iter()
            .filter(|s| !s.is_empty())
            .take(3)
            .map(String::as_str)
            .collect();
        eprintln!("  {:5}  {key}  e.g. {:?}", samples.len(), ex);
    }

    // Sub-breakdown of engine-validation-failed errors
    let mut val_errors: HashMap<String, Vec<&str>> = HashMap::new();
    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let [face] = card.faces.as_slice() else {
            continue;
        };
        if card.layout != "normal" || face.name != card.name {
            continue;
        }
        let Some(tl) = face.type_line.as_deref() else {
            continue;
        };
        let compiler_vocab = cached_compiler_vocab(&vocabulary);
        let ast = compile_card(
            &CardInput {
                oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                name: &card.name,
                layout: &card.layout,
                faces: vec![FaceInput {
                    name: &face.name,
                    type_line: tl,
                    oracle_text: face.oracle_text.as_deref().unwrap_or(""),
                }],
            },
            &compiler_vocab,
        );
        if !ast.strictly_compiled {
            continue;
        }
        if let Ok(prog) = lower_face(
            card,
            face,
            &ast.faces[0],
            &vocabulary,
            FaceLoweringOptions::default(),
        ) && let Err(e) = prog.validate(&vocabulary)
        {
            val_errors
                .entry(format!("{e:?}"))
                .or_default()
                .push(card.name.as_str());
        }
    }
    let mut val_sorted: Vec<(String, Vec<&str>)> = val_errors.into_iter().collect();
    val_sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1.len()));
    eprintln!("Engine validation failure breakdown:");
    for (err, names) in val_sorted.iter().take(12) {
        let sample: Vec<&str> = names.iter().copied().take(4).collect();
        eprintln!("  {:5}  {err}  (e.g. {:?})", names.len(), sample);
    }

    // Gatecrash-specific drop reasons on cards not yet bound
    let mut gtc_reasons: HashMap<&'static str, Vec<&str>> = HashMap::new();
    if let Some(gtc_oids) = set_oracles.get("gtc") {
        for oid in gtc_oids {
            if bound_ids.contains(oid) {
                continue;
            }
            if let Some(card) = card_by_oid.get(oid.as_str()) {
                let r = compile_with_reason(card, &vocabulary).err().unwrap_or("ok");
                gtc_reasons.entry(r).or_default().push(card.name.as_str());
            }
        }
    }
    let mut gtc_sorted: Vec<(&'static str, Vec<&str>)> = gtc_reasons.into_iter().collect();
    gtc_sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1.len()));
    eprintln!("Gatecrash (gtc) unbound card drop reasons:");
    for (reason, names) in gtc_sorted.iter().take(20) {
        let sample: Vec<&str> = names.iter().copied().take(5).collect();
        eprintln!("  {:4}  {reason}  (e.g. {:?})", names.len(), sample);
    }

    // Check how many avacyn_restored and innistrad cards oracle_lowering already covers
    let mut avr_total = 0usize;
    let mut avr_covered = 0usize;
    let mut isd_total = 0usize;
    let mut isd_covered = 0usize;
    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        if super::super::avacyn_restored::compile(card, &vocabulary).is_some() {
            avr_total += 1;
            if compile_with_reason(card, &vocabulary).is_ok() {
                avr_covered += 1;
            }
        }
        if super::super::innistrad::compile(card, &vocabulary).is_some() {
            isd_total += 1;
            if compile_with_reason(card, &vocabulary).is_ok() {
                isd_covered += 1;
            }
        }
    }
    // Sub-bucket telemetry for the biggest drop reasons so every Lowering Wave targets exact top gaps
    fn variant_name<T: std::fmt::Debug>(v: &T) -> String {
        let s = format!("{v:?}");
        s.split(['{', '(', ' ']).next().unwrap_or("?").to_string()
    }
    let mut unsup_ins: HashMap<String, usize> = HashMap::new();
    let mut unsup_kw: HashMap<String, usize> = HashMap::new();
    let mut unsup_static: HashMap<String, usize> = HashMap::new();
    let mut unsup_rule: HashMap<String, usize> = HashMap::new();
    let mut unsup_rep: HashMap<String, usize> = HashMap::new();
    let mut unsup_trig_ev: HashMap<String, usize> = HashMap::new();
    let mut unsup_trig_cond: HashMap<String, usize> = HashMap::new();
    let mut unsup_static_cond: HashMap<String, usize> = HashMap::new();
    let mut unsup_sub: HashMap<String, usize> = HashMap::new();
    let mut unsup_dmg_src: HashMap<String, usize> = HashMap::new();
    let mut unsup_pt_subj: HashMap<String, usize> = HashMap::new();
    let mut unsup_pat_action: HashMap<String, usize> = HashMap::new();
    let mut val_fail_errs: HashMap<String, Vec<String>> = HashMap::new();
    let mut val_fail_shapes: HashMap<String, usize> = HashMap::new();
    let mut unsup_ab_res: HashMap<String, usize> = HashMap::new();
    let mut unsup_enchant: HashMap<String, usize> = HashMap::new();
    let mut unsup_act_cost: HashMap<String, usize> = HashMap::new();
    let mut unsup_exile_gy: HashMap<String, usize> = HashMap::new();
    let mut unsup_ret_hand: HashMap<String, usize> = HashMap::new();
    let mut unsup_move_zone: HashMap<String, usize> = HashMap::new();
    let mut unsup_eff_mana: HashMap<String, usize> = HashMap::new();
    let mut unsup_val_expr: HashMap<String, usize> = HashMap::new();
    let mut unsup_if_cond: HashMap<String, usize> = HashMap::new();
    let mut unsup_grant_kw: HashMap<String, usize> = HashMap::new();
    let mut multi_face_layout: HashMap<String, usize> = HashMap::new();
    let mut unsup_leaf_ins: HashMap<String, usize> = HashMap::new();
    let mut unsup_kw_action: HashMap<String, usize> = HashMap::new();
    let mut unsup_pay_cost: HashMap<String, usize> = HashMap::new();
    let mut unsup_marker: HashMap<String, usize> = HashMap::new();

    let compiler_vocab = cached_compiler_vocab(&vocabulary);
    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let reason = match compile_with_inventory_reason(card, inventory, &vocabulary) {
            Ok(_) => continue,
            Err(r) => r,
        };
        if reason == "oracle:multi-face" || reason == "oracle:non-normal-layout" {
            *multi_face_layout.entry(card.layout.clone()).or_insert(0) += 1;
            continue;
        }
        let [face] = card.faces.as_slice() else {
            continue;
        };
        if card.layout != "normal" || face.name != card.name {
            continue;
        }
        let Some(tl) = face.type_line.as_deref() else {
            continue;
        };
        if reason == "oracle:unsupported-subtype" {
            if let Some((_, subs)) = tl.split_once(" \u{2014} ") {
                for tok in subs.split_whitespace() {
                    *unsup_sub.entry(tok.to_string()).or_insert(0) += 1;
                }
            }
            continue;
        }
        let ast = compile_card(
            &CardInput {
                oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                name: &card.name,
                layout: &card.layout,
                faces: vec![FaceInput {
                    name: &face.name,
                    type_line: tl,
                    oracle_text: face.oracle_text.as_deref().unwrap_or(""),
                }],
            },
            &compiler_vocab,
        );
        if !ast.strictly_compiled {
            continue;
        }
        let f = &ast.faces[0];
        let mut all_ins: Vec<&InstructionAst> = f.spell_effects.iter().collect();
        let mut leaf_ins: Vec<&InstructionAst> = Vec::new();
        collect_leaf_ins(&f.spell_effects, &mut leaf_ins);
        for ab in &f.abilities {
            match ab {
                AbilityAst::Activated { body, .. }
                | AbilityAst::Triggered { body, .. }
                | AbilityAst::SagaChapter { body, .. }
                | AbilityAst::Loyalty { body, .. } => {
                    all_ins.extend(body.iter());
                    collect_leaf_ins(body, &mut leaf_ins);
                }
                _ => {}
            }
        }
        match reason {
            "oracle:engine-validation-failed" => {
                if let Ok(program) =
                    lower_face(card, face, f, &vocabulary, FaceLoweringOptions::default())
                    && let Err(e) = program.validate(&vocabulary)
                {
                    let key = format!("{e:?}");
                    val_fail_errs
                        .entry(key.clone())
                        .or_default()
                        .push(card.name.clone());
                    let fmt_ops = |ops: &[Instruction]| -> Vec<String> {
                        ops.iter()
                            .take(4)
                            .map(|op| {
                                let s = format!("{op:?}");
                                s.split(&['(', '{', ' '][..])
                                    .next()
                                    .unwrap_or("?")
                                    .to_string()
                            })
                            .collect()
                    };
                    let shape = if let ProgramKind::Spell(sp) = &program.kind {
                        format!(
                            "{key}:spell target={:?} ops={:?}",
                            sp.body.target,
                            fmt_ops(&sp.body.instructions)
                        )
                    } else if let Some(t) = program.triggered.iter().find(|t| {
                        let mut probe = Program::enchantment(None);
                        probe.triggered.push((*t).clone());
                        probe.validate(&vocabulary).is_err()
                    }) {
                        format!(
                            "{key}:trig target={:?} ops={:?}",
                            t.body.target,
                            fmt_ops(&t.body.instructions)
                        )
                    } else if let Some(a) = program.activated.iter().find(|a| {
                        let mut probe = Program::enchantment(None);
                        probe.activated.push((*a).clone());
                        if let Some(res) = program.activation_restrictions.get(&a.id) {
                            probe.activation_restrictions.insert(a.id, res.clone());
                        }
                        if let Some(ec) = program.extra_activation_costs.get(&a.id) {
                            probe.extra_activation_costs.insert(a.id, ec.clone());
                        }
                        if let Some(&lc) = program.loyalty_costs.get(&a.id) {
                            probe.loyalty_costs.insert(a.id, lc);
                        }
                        probe.validate(&vocabulary).is_err()
                    }) {
                        format!(
                            "{key}:act target={:?} ops={:?}",
                            a.body.target,
                            fmt_ops(&a.body.instructions)
                        )
                    } else {
                        format!("{key}:other")
                    };
                    *val_fail_shapes.entry(shape).or_insert(0) += 1;
                }
            }
            "oracle:complex-enchant-restriction" | "oracle:unsupported-enchant-target" => {
                for st in &f.statics {
                    if let StaticAbilityAst::AttachmentRestriction {
                        filter: Some(ef), ..
                    } = st
                    {
                        *unsup_enchant.entry(format!("{ef:?}")).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-instruction" => {
                let mut b = BodyBuilder::new();
                let mut tmp_ids = LoweringIds::default();
                for ins in &all_ins {
                    if let InstructionAst::AbilityRestriction { restriction, .. } = ins {
                        *unsup_ab_res.entry(restriction.clone()).or_insert(0) += 1;
                    }
                    if lower_instruction(ins, &mut b, &vocabulary, &mut tmp_ids, false, false, None)
                        == Err("oracle:unsupported-instruction")
                    {
                        *unsup_ins.entry(variant_name(ins)).or_insert(0) += 1;
                    }
                }
                for ins in &leaf_ins {
                    let mut b2 = BodyBuilder::new();
                    let mut tmp_ids2 = LoweringIds::default();
                    if lower_instruction(
                        ins,
                        &mut b2,
                        &vocabulary,
                        &mut tmp_ids2,
                        false,
                        false,
                        None,
                    ) == Err("oracle:unsupported-instruction")
                    {
                        *unsup_leaf_ins.entry(variant_name(ins)).or_insert(0) += 1;
                        if let InstructionAst::PayCost(atom) = ins {
                            *unsup_pay_cost.entry(variant_name(atom)).or_insert(0) += 1;
                        }
                    }
                }
            }
            "oracle:unsupported-activation-cost" => {
                for ab in &f.abilities {
                    if let AbilityAst::Activated { cost, .. } = ab {
                        for atom in cost {
                            *unsup_act_cost.entry(variant_name(atom)).or_insert(0) += 1;
                        }
                    }
                }
            }
            "oracle:unsupported-exile-gy" => {
                for ins in &leaf_ins {
                    if let InstructionAst::ExileGraveyard { whose } = ins {
                        *unsup_exile_gy.entry(whose.clone()).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-return-hand" => {
                for ins in &leaf_ins {
                    if let InstructionAst::ReturnZone {
                        subject,
                        from_zone,
                        to,
                        ..
                    } = ins
                    {
                        let key = format!(
                            "{} from={:?} to={}",
                            variant_name(subject.as_ref()),
                            from_zone,
                            to
                        );
                        *unsup_ret_hand.entry(key).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-move-zone" => {
                for ins in &leaf_ins {
                    if let InstructionAst::MoveZone { subject, to } = ins {
                        let key = format!("{} -> {}", variant_name(subject.as_ref()), to);
                        *unsup_move_zone.entry(key).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-effect-mana" => {
                for ins in &leaf_ins {
                    if let InstructionAst::AddMana { mana, .. } = ins {
                        *unsup_eff_mana.entry(variant_name(mana)).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-value-expr" => {
                for ins in &leaf_ins {
                    let dbg = format!("{ins:?}");
                    for tok in [
                        "Add(",
                        "Sub(",
                        "Mul(",
                        "Devotion",
                        "DomainCount",
                        "OpponentsCount",
                        "ColorsOfManaSpent",
                        "ColorsAmong",
                        "CardTypesAmong",
                        "SubtypesAmong",
                        "RollDie",
                        "PartySize",
                        "TotalCardsInAllHands",
                        "ManaSpentToCast",
                        "AggregateOverPlayers",
                        "StartingLifeTotal",
                        "SelfColorsCount",
                        "CounterCount",
                        "ObjectStat",
                        "PlayerStat",
                    ] {
                        if dbg.contains(tok) {
                            *unsup_val_expr.entry(tok.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            "oracle:unsupported-if-condition" => {
                for ins in &all_ins {
                    if let InstructionAst::IfCondition { condition, .. } = ins {
                        *unsup_if_cond
                            .entry(variant_name(condition.as_ref()))
                            .or_insert(0) += 1;
                        if let ConditionAst::Marker(m) = condition.as_ref() {
                            *unsup_marker.entry(variant_name(m)).or_insert(0) += 1;
                        }
                    }
                }
            }
            "oracle:unsupported-granted-keyword" => {
                for st in &f.statics {
                    if let StaticAbilityAst::GrantKeywords { keywords, .. } = st {
                        for k in keywords {
                            *unsup_grant_kw.entry(k.to_ascii_lowercase()).or_insert(0) += 1;
                        }
                    }
                }
                for ins in &leaf_ins {
                    if let InstructionAst::GrantKeywords { keywords, .. } = ins {
                        for k in keywords {
                            *unsup_grant_kw.entry(k.to_ascii_lowercase()).or_insert(0) += 1;
                        }
                    }
                }
            }
            "oracle:unsupported-keyword" => {
                for (kw_idx, kw) in f.keywords.iter().enumerate() {
                    let mut p = Program::enchantment(None);
                    let mut tmp_ids = LoweringIds::default();
                    if lower_keyword(kw, kw_idx, f, &mut p, &vocabulary, &mut tmp_ids, false)
                        == Err("oracle:unsupported-keyword")
                    {
                        *unsup_kw.entry(kw.name.to_ascii_lowercase()).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-static" => {
                for st in &f.statics {
                    *unsup_static.entry(variant_name(st)).or_insert(0) += 1;
                }
            }
            "oracle:unsupported-rule-restriction" => {
                for st in &f.statics {
                    if let StaticAbilityAst::RuleOrCombatRestriction { rule, .. } = st {
                        *unsup_rule.entry(rule.clone()).or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-static-cond" => {
                for st in &f.statics {
                    match st {
                        StaticAbilityAst::ContinuousModifyPT {
                            condition: Some(c), ..
                        }
                        | StaticAbilityAst::GrantKeywords {
                            condition: Some(c), ..
                        }
                        | StaticAbilityAst::GrantAbilityText {
                            condition: Some(c), ..
                        } => {
                            *unsup_static_cond
                                .entry(variant_name(c.as_ref()))
                                .or_insert(0) += 1;
                            if let ConditionAst::PlayerActionThisTurn { action, .. } = c.as_ref() {
                                *unsup_pat_action.entry(action.clone()).or_insert(0) += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            "oracle:unsupported-replacement" => {
                for rep in &f.replacements {
                    *unsup_rep.entry(variant_name(rep)).or_insert(0) += 1;
                }
            }
            "oracle:unsupported-trigger-event" => {
                for ab in &f.abilities {
                    if let AbilityAst::Triggered { trigger, .. } = ab {
                        for ev in &trigger.events {
                            *unsup_trig_ev.entry(variant_name(ev)).or_insert(0) += 1;
                            if let TriggerEventAst::KeywordActionPerformed { action, .. } = ev {
                                *unsup_kw_action
                                    .entry(action.to_ascii_lowercase())
                                    .or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
            "oracle:unsupported-trigger-condition" => {
                for ab in &f.abilities {
                    if let AbilityAst::Triggered { trigger, .. } = ab
                        && let Some(c) = &trigger.intervening_if
                    {
                        *unsup_trig_cond.entry(variant_name(c)).or_insert(0) += 1;
                        if let ConditionAst::Marker(m) = c {
                            *unsup_marker.entry(variant_name(m)).or_insert(0) += 1;
                        }
                        if let ConditionAst::PlayerActionThisTurn { action, .. } = c {
                            *unsup_pat_action.entry(action.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
            "oracle:non-self-damage-source" => {
                for ins in all_ins {
                    if let InstructionAst::DealDamage { source, .. } = ins {
                        *unsup_dmg_src
                            .entry(variant_name(source.as_ref()))
                            .or_insert(0) += 1;
                    }
                }
            }
            "oracle:unsupported-modify-pt-subject" => {
                for ins in all_ins {
                    if let InstructionAst::ModifyPT { subject, .. } = ins {
                        *unsup_pt_subj
                            .entry(variant_name(subject.as_ref()))
                            .or_insert(0) += 1;
                    }
                }
            }
            _ => {}
        }
    }
    fn print_top<K: std::fmt::Display>(title: &str, map: HashMap<K, usize>, n: usize) {
        let mut v: Vec<(K, usize)> = map.into_iter().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.1));
        eprintln!("--- {title} ---");
        for (k, c) in v.into_iter().take(n) {
            eprintln!("  {c:5}  {k}");
        }
    }
    print_top("unsupported-instruction by AST variant", unsup_ins, 30);
    print_top(
        "unsupported-instruction LEAF by AST variant",
        unsup_leaf_ins,
        30,
    );
    print_top("unsupported PayCost atoms", unsup_pay_cost, 15);
    print_top(
        "unsupported KeywordActionPerformed actions",
        unsup_kw_action,
        20,
    );
    print_top("unsupported GameMarker conditions", unsup_marker, 20);
    print_top("multi-face / non-normal by layout", multi_face_layout, 15);
    print_top(
        "unsupported-activation-cost by CostAtomAst",
        unsup_act_cost,
        15,
    );
    print_top("unsupported-exile-gy by whose", unsup_exile_gy, 15);
    print_top("unsupported-return-hand by subject/to", unsup_ret_hand, 15);
    print_top("unsupported-move-zone by subject/to", unsup_move_zone, 15);
    print_top("unsupported-effect-mana by ManaSpecAst", unsup_eff_mana, 12);
    print_top("unsupported-value-expr by token", unsup_val_expr, 15);
    print_top(
        "unsupported-if-condition by ConditionAst",
        unsup_if_cond,
        15,
    );
    print_top("unsupported-granted-keyword by keyword", unsup_grant_kw, 20);
    print_top("unsupported-keyword by name", unsup_kw, 25);
    print_top("unsupported-static by AST variant", unsup_static, 15);
    print_top("unsupported-rule-restriction by rule", unsup_rule, 15);
    print_top(
        "unsupported-static-cond by AST variant",
        unsup_static_cond,
        15,
    );
    print_top("unsupported-replacement by AST variant", unsup_rep, 15);
    print_top(
        "unsupported-trigger-event by AST variant",
        unsup_trig_ev,
        15,
    );
    print_top(
        "unsupported-trigger-condition by AST variant",
        unsup_trig_cond,
        15,
    );
    print_top("unsupported-subtype by token", unsup_sub, 15);
    print_top("non-self-damage-source by SubjectRefAst", unsup_dmg_src, 10);
    print_top(
        "unsupported-modify-pt-subject by SubjectRefAst",
        unsup_pt_subj,
        10,
    );
    print_top(
        "PlayerActionThisTurn top unsupported actions",
        unsup_pat_action,
        25,
    );
    print_top("AbilityRestriction top labels", unsup_ab_res, 20);
    print_top("complex-enchant top filters", unsup_enchant, 15);
    let mut val_sorted: Vec<(String, Vec<String>)> = val_fail_errs.into_iter().collect();
    val_sorted.sort_by_key(|e| std::cmp::Reverse(e.1.len()));
    eprintln!("--- engine-validation-failed by DefinitionError ---");
    for (err, names) in val_sorted.into_iter().take(15) {
        let sample: Vec<&str> = names.iter().take(6).map(String::as_str).collect();
        eprintln!("  {:5}  {err}  (e.g. {:?})", names.len(), sample);
    }
    print_top("engine-validation-failed top shapes", val_fail_shapes, 25);

    eprintln!(
        "Legacy table overlap: avacyn_restored {avr_covered}/{avr_total}, innistrad {isd_covered}/{isd_total}"
    );

    // === GATE 4: MULTI-BLOCKER WHOLE-CARD DEPENDENCY QUEUE ===
    // Walk EVERY face and EVERY keyword/static/replacement/ability/instruction on every
    // unbound Base Constructed card to rank candidate capabilities and shared prerequisites.
    // Leaf failures can be handled by sequence lowering; only a full reaudit measures gains.
    let mut strict_ast_cards = 0usize;
    let mut solo_blocker_cards: HashMap<String, Vec<String>> = HashMap::new();
    let mut shared_blocker_cards: HashMap<String, usize> = HashMap::new();
    let mut blocker_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut candidate_blockers = BTreeMap::new();
    let mut ast_incomplete_unbound = 0usize;

    for card in inventory
        .cards
        .iter()
        .filter(|c| c.required_by_base_formats)
    {
        let face_inputs: Vec<FaceInput<'_>> = card
            .faces
            .iter()
            .map(|f| FaceInput {
                name: &f.name,
                type_line: f.type_line.as_deref().unwrap_or(""),
                oracle_text: f.oracle_text.as_deref().unwrap_or(""),
            })
            .collect();
        let ast = compile_card(
            &CardInput {
                oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                name: &card.name,
                layout: &card.layout,
                faces: face_inputs,
            },
            &compiler_vocab,
        );
        if ast.strictly_compiled {
            strict_ast_cards += 1;
        }
        let is_bound = card
            .oracle_id
            .as_deref()
            .is_some_and(|oid| bound_ids.contains(oid));
        if is_bound {
            continue;
        }

        let mut blockers: BTreeSet<String> = BTreeSet::new();
        if !matches!(
            card.layout.as_str(),
            "normal" | "saga" | "mutate" | "transform" | "split" | "adventure" | "meld"
        ) {
            blockers.insert(format!("layout:{}", card.layout));
        }
        if !ast.strictly_compiled {
            ast_incomplete_unbound += 1;
            for face in &ast.faces {
                for fail in &face.failures {
                    blockers.insert(format!("ast:{}:{}", fail.stage, fail.reason));
                }
            }
        }
        for (face_idx, ast_face) in ast.faces.iter().enumerate() {
            let face_tl = card
                .faces
                .get(face_idx)
                .and_then(|f| f.type_line.as_deref())
                .unwrap_or("");
            let is_spell_face = face_tl.contains("Instant") || face_tl.contains("Sorcery");
            let is_aura_face = face_tl.contains("Aura");
            let mut tmp_ids = LoweringIds::default();
            for (kw_idx, kw) in ast_face.keywords.iter().enumerate() {
                if card.layout == "transform"
                    && face_idx == 0
                    && kw.name.eq_ignore_ascii_case("disturb")
                {
                    continue;
                }
                if card.layout == "split"
                    && face_idx == 1
                    && kw.name.eq_ignore_ascii_case("aftermath")
                    && kw.parameter.is_none()
                {
                    continue;
                }
                let mut p = if is_spell_face {
                    Program::new(
                        BTreeSet::from([CardType::Sorcery]),
                        BTreeSet::new(),
                        BTreeSet::new(),
                        ProgramKind::Spell(SpellDefinition {
                            cost: ManaCost::free(),
                            uncastable_mana_cost: false,
                            body: ExecutableBody::new(BodyTarget::None, Vec::new()),
                            modes: None,
                        }),
                    )
                } else {
                    Program::enchantment(None)
                };
                if let Err(err) = lower_keyword(
                    kw,
                    kw_idx,
                    ast_face,
                    &mut p,
                    &vocabulary,
                    &mut tmp_ids,
                    is_spell_face,
                ) {
                    if err == "oracle:unsupported-keyword" {
                        blockers.insert(format!("kw:{}", kw.name.to_ascii_lowercase()));
                    } else {
                        blockers.insert(format!("kw:{}:{err}", kw.name.to_ascii_lowercase()));
                    }
                }
            }
            for st in &ast_face.statics {
                let mut p = if is_aura_face {
                    Program::aura(None, EnchantRestriction::Creature)
                } else {
                    Program::enchantment(None)
                };
                if let Err(err) = lower_static(st, &mut p, &vocabulary, &mut tmp_ids, is_spell_face)
                {
                    let detail = match st {
                        StaticAbilityAst::RuleOrCombatRestriction { rule, .. } => {
                            format!("static:rule:{rule}")
                        }
                        _ => format!("static:{}:{err}", variant_name(st)),
                    };
                    blockers.insert(detail);
                }
            }
            for rep in &ast_face.replacements {
                let mut p = Program::enchantment(None);
                if let Err(err) = lower_replacement(rep, &mut p, &vocabulary, &mut tmp_ids) {
                    blockers.insert(format!("rep:{}:{err}", variant_name(rep)));
                }
            }
            for ab in &ast_face.abilities {
                match ab {
                    AbilityAst::Activated {
                        cost, body, zone, ..
                    } => {
                        if try_lower_mana_ability(cost, body, &vocabulary).is_none() {
                            if let Err(err) =
                                lower_activation_cost_full(cost, zone.as_deref(), &vocabulary)
                            {
                                blockers.insert(format!("act-cost:{err}"));
                            }
                            let filtered_slice: Vec<InstructionAst> =
                                match strip_activated_restrictions(body, &vocabulary) {
                                    Ok((f, _)) => f,
                                    Err(err) => {
                                        blockers.insert(format!("act-restriction:{err}"));
                                        body.clone()
                                    }
                                };
                            if !(body.is_empty() && !ast.strictly_compiled)
                                && let Err(err) =
                                    lower_activated_body(body, &vocabulary, &mut tmp_ids)
                            {
                                let mut found_leaf = false;
                                let mut leaves = Vec::new();
                                collect_leaf_ins(&filtered_slice, &mut leaves);
                                for leaf in leaves {
                                    let mut b = BodyBuilder::new();
                                    if let Err(lerr) = lower_instruction(
                                        leaf,
                                        &mut b,
                                        &vocabulary,
                                        &mut tmp_ids,
                                        false,
                                        true,
                                        None,
                                    ) {
                                        found_leaf = true;
                                        blockers
                                            .insert(format!("ins:{}:{lerr}", variant_name(leaf)));
                                    }
                                }
                                if !found_leaf {
                                    blockers.insert(format!("act-body:{err}"));
                                }
                            }
                        }
                    }
                    AbilityAst::Triggered { trigger, body, .. } => {
                        let (mut dummy_spec, mut dummy_grouping) =
                            match lower_trigger_spec(trigger, &vocabulary) {
                                Ok((s, g, _)) => (s, g),
                                Err(err) => {
                                    blockers.insert(format!("trig-spec:{err}"));
                                    (TriggerSpec::self_enters(), TriggerGrouping::EachOccurrence)
                                }
                            };
                        let stripped = strip_triggered_restrictions(
                            body,
                            &mut dummy_spec,
                            &mut dummy_grouping,
                            &vocabulary,
                        );
                        let self_pronoun = trigger_subject_is_self(trigger);
                        let modal_ok = if let Some((pre, choice, modes, post, allow_repeat)) =
                            extract_modal_with_wrap(&stripped)
                            && !allow_repeat
                            && (pre.is_empty() && post.is_empty()
                                || matches!(parse_modal_choice(choice), Ok((1, 1))))
                            && parse_modal_choice(choice).is_ok()
                            && modes.len() >= 2
                        {
                            let mut all_ok = true;
                            for m in modes {
                                let m_stripped = strip_triggered_restrictions(
                                    &m.body,
                                    &mut dummy_spec,
                                    &mut dummy_grouping,
                                    &vocabulary,
                                );
                                let mut full =
                                    Vec::with_capacity(pre.len() + m_stripped.len() + post.len());
                                full.extend_from_slice(pre);
                                full.extend(m_stripped);
                                full.extend_from_slice(post);
                                if let Err(err) = lower_effect_body(
                                    &full,
                                    &vocabulary,
                                    &mut tmp_ids,
                                    true,
                                    self_pronoun,
                                ) {
                                    all_ok = false;
                                    let mut found_leaf = false;
                                    let mut leaves = Vec::new();
                                    collect_leaf_ins(&full, &mut leaves);
                                    for leaf in leaves {
                                        let mut b = BodyBuilder::new();
                                        if let Err(lerr) = lower_instruction(
                                            leaf,
                                            &mut b,
                                            &vocabulary,
                                            &mut tmp_ids,
                                            true,
                                            self_pronoun,
                                            None,
                                        ) {
                                            found_leaf = true;
                                            blockers.insert(format!(
                                                "ins:{}:{lerr}",
                                                variant_name(leaf)
                                            ));
                                        }
                                    }
                                    if !found_leaf {
                                        blockers.insert(format!("trig-body:{err}"));
                                    }
                                }
                            }
                            Some(all_ok)
                        } else {
                            None
                        };
                        if modal_ok.is_none()
                            && !(body.is_empty() && !ast.strictly_compiled)
                            && let Err(err) = lower_effect_body(
                                &stripped,
                                &vocabulary,
                                &mut tmp_ids,
                                true,
                                self_pronoun,
                            )
                        {
                            let mut found_leaf = false;
                            let mut leaves = Vec::new();
                            collect_leaf_ins(&stripped, &mut leaves);
                            for leaf in leaves {
                                let mut b = BodyBuilder::new();
                                if let Err(lerr) = lower_instruction(
                                    leaf,
                                    &mut b,
                                    &vocabulary,
                                    &mut tmp_ids,
                                    true,
                                    self_pronoun,
                                    None,
                                ) {
                                    found_leaf = true;
                                    blockers.insert(format!("ins:{}:{lerr}", variant_name(leaf)));
                                }
                            }
                            if !found_leaf {
                                blockers.insert(format!("trig-body:{err}"));
                            }
                        }
                    }
                    AbilityAst::Loyalty { body, .. } => {
                        let stripped = match strip_activated_restrictions(body, &vocabulary) {
                            Ok((f, _)) => f,
                            Err(err) => {
                                blockers.insert(format!("loyalty-restriction:{err}"));
                                body.clone()
                            }
                        };
                        if let Some((pre, choice, modes, post, _)) =
                            extract_modal_with_wrap(&stripped)
                            && matches!(parse_modal_choice(choice), Ok((1, 1)))
                            && modes.len() >= 2
                        {
                            for m in modes {
                                let m_stripped = strip_activated_restrictions(&m.body, &vocabulary)
                                    .map(|(f, _)| f)
                                    .unwrap_or_else(|_| m.body.clone());
                                let mut full =
                                    Vec::with_capacity(pre.len() + m_stripped.len() + post.len());
                                full.extend_from_slice(pre);
                                full.extend(m_stripped);
                                full.extend_from_slice(post);
                                if let Err(err) =
                                    lower_effect_body(&full, &vocabulary, &mut tmp_ids, false, true)
                                {
                                    let mut found_leaf = false;
                                    let mut leaves = Vec::new();
                                    collect_leaf_ins(&full, &mut leaves);
                                    for leaf in leaves {
                                        let mut b = BodyBuilder::new();
                                        if let Err(lerr) = lower_instruction(
                                            leaf,
                                            &mut b,
                                            &vocabulary,
                                            &mut tmp_ids,
                                            false,
                                            true,
                                            None,
                                        ) {
                                            found_leaf = true;
                                            blockers.insert(format!(
                                                "ins:{}:{lerr}",
                                                variant_name(leaf)
                                            ));
                                        }
                                    }
                                    if !found_leaf {
                                        blockers.insert(format!("special-body:{err}"));
                                    }
                                }
                            }
                        } else if let Err(err) =
                            lower_effect_body(&stripped, &vocabulary, &mut tmp_ids, false, true)
                        {
                            let mut found_leaf = false;
                            let mut leaves = Vec::new();
                            collect_leaf_ins(&stripped, &mut leaves);
                            for leaf in leaves {
                                let mut b = BodyBuilder::new();
                                if let Err(lerr) = lower_instruction(
                                    leaf,
                                    &mut b,
                                    &vocabulary,
                                    &mut tmp_ids,
                                    false,
                                    true,
                                    None,
                                ) {
                                    found_leaf = true;
                                    blockers.insert(format!("ins:{}:{lerr}", variant_name(leaf)));
                                }
                            }
                            if !found_leaf {
                                blockers.insert(format!("special-body:{err}"));
                            }
                        }
                    }
                    AbilityAst::SagaChapter { body, .. } => {
                        let mut dummy_spec = TriggerSpec::counters_changed(
                            ObjectFilter::self_object(),
                            CounterTriggerKind::SagaChapter(1),
                        );
                        let mut dummy_grouping = TriggerGrouping::EachOccurrence;
                        let stripped = strip_triggered_restrictions(
                            body,
                            &mut dummy_spec,
                            &mut dummy_grouping,
                            &vocabulary,
                        );
                        if let Some((pre, choice, modes, post, allow_repeat)) =
                            extract_modal_with_wrap(&stripped)
                            && !allow_repeat
                            && (pre.is_empty() && post.is_empty()
                                || matches!(parse_modal_choice(choice), Ok((1, 1))))
                            && parse_modal_choice(choice).is_ok()
                            && modes.len() >= 2
                        {
                            for m in modes {
                                let m_stripped = strip_triggered_restrictions(
                                    &m.body,
                                    &mut dummy_spec,
                                    &mut dummy_grouping,
                                    &vocabulary,
                                );
                                let mut full =
                                    Vec::with_capacity(pre.len() + m_stripped.len() + post.len());
                                full.extend_from_slice(pre);
                                full.extend(m_stripped);
                                full.extend_from_slice(post);
                                if let Err(err) =
                                    lower_effect_body(&full, &vocabulary, &mut tmp_ids, true, true)
                                {
                                    let mut found_leaf = false;
                                    let mut leaves = Vec::new();
                                    collect_leaf_ins(&full, &mut leaves);
                                    for leaf in leaves {
                                        let mut b = BodyBuilder::new();
                                        if let Err(lerr) = lower_instruction(
                                            leaf,
                                            &mut b,
                                            &vocabulary,
                                            &mut tmp_ids,
                                            true,
                                            true,
                                            None,
                                        ) {
                                            found_leaf = true;
                                            blockers.insert(format!(
                                                "ins:{}:{lerr}",
                                                variant_name(leaf)
                                            ));
                                        }
                                    }
                                    if !found_leaf {
                                        blockers.insert(format!("special-body:{err}"));
                                    }
                                }
                            }
                        } else if let Err(err) =
                            lower_effect_body(&stripped, &vocabulary, &mut tmp_ids, true, true)
                        {
                            let mut found_leaf = false;
                            let mut leaves = Vec::new();
                            collect_leaf_ins(&stripped, &mut leaves);
                            for leaf in leaves {
                                let mut b = BodyBuilder::new();
                                if let Err(lerr) = lower_instruction(
                                    leaf,
                                    &mut b,
                                    &vocabulary,
                                    &mut tmp_ids,
                                    true,
                                    true,
                                    None,
                                ) {
                                    found_leaf = true;
                                    blockers.insert(format!("ins:{}:{lerr}", variant_name(leaf)));
                                }
                            }
                            if !found_leaf {
                                blockers.insert(format!("special-body:{err}"));
                            }
                        }
                    }
                    AbilityAst::ClassLevel { .. } => {
                        blockers.insert(format!("ability:{}", variant_name(ab)));
                    }
                }
            }
            if !ast_face.spell_effects.is_empty() {
                let mut filtered_spell = Vec::with_capacity(ast_face.spell_effects.len());
                for ins in &ast_face.spell_effects {
                    if let InstructionAst::AbilityRestriction {
                        restriction,
                        condition,
                    } = ins
                        && ((condition.is_none()
                            && matches!(
                                restriction.as_str(),
                                "DuringOwnCombat" | "DuringYourCombat"
                            ))
                            || parse_cast_timing_restriction(
                                restriction,
                                condition.as_deref(),
                                &vocabulary,
                            )
                            .is_some())
                    {
                        continue;
                    }
                    filtered_spell.push(ins.clone());
                }
                if let Err(err) = lower_spell_effects(&filtered_spell, &vocabulary, &mut tmp_ids) {
                    let mut found_leaf = false;
                    let mut leaves = Vec::new();
                    if let Some((pre, _, modes, post, _)) = extract_modal_with_wrap(&filtered_spell)
                        && modes.len() >= 2
                    {
                        collect_leaf_ins(pre, &mut leaves);
                        for m in modes {
                            collect_leaf_ins(&m.body, &mut leaves);
                        }
                        collect_leaf_ins(post, &mut leaves);
                    } else {
                        collect_leaf_ins(&filtered_spell, &mut leaves);
                    }
                    for leaf in leaves {
                        let mut b = BodyBuilder::new();
                        if let Err(lerr) = lower_instruction(
                            leaf,
                            &mut b,
                            &vocabulary,
                            &mut tmp_ids,
                            false,
                            false,
                            None,
                        ) {
                            found_leaf = true;
                            blockers.insert(format!("ins:{}:{lerr}", variant_name(leaf)));
                        }
                    }
                    if !found_leaf {
                        blockers.insert(format!("spell-body:{err}"));
                    }
                }
            }
            if blockers.is_empty()
                && let Some(face) = card.faces.get(face_idx)
            {
                let oid = card.oracle_id.as_deref();
                let printed_loyalty = if face_tl.contains("Planeswalker") {
                    oid.and_then(|id| inventory.source_loyalty(id, face_idx))
                        .and_then(|s| s.parse::<u16>().ok())
                } else {
                    None
                };
                let printed_defense = if face_tl.contains("Battle") {
                    oid.and_then(|id| inventory.source_defense(id, face_idx))
                        .and_then(|s| s.parse::<u16>().ok())
                } else {
                    None
                };
                let allow_empty_mana_cost =
                    face_idx == 1 && matches!(card.layout.as_str(), "transform" | "flip");
                let allow_aftermath = face_idx == 1 && card.layout == "split";
                let allow_disturb = face_idx == 0 && card.layout == "transform";
                match lower_face(
                    card,
                    face,
                    ast_face,
                    &vocabulary,
                    FaceLoweringOptions {
                        allow_empty_mana_cost,
                        allow_aftermath,
                        allow_disturb,
                        printed_loyalty,
                        printed_defense,
                    },
                ) {
                    Ok(prog) => {
                        if let Err(err) = prog.validate(&vocabulary) {
                            blockers.insert(format!("validate:{err:?}"));
                        }
                    }
                    Err(err) => {
                        blockers.insert(format!("face:{err}"));
                    }
                }
            }
        }
        if blockers.is_empty() {
            if let Err(err) = compile_with_inventory_reason(card, inventory, &vocabulary) {
                blockers.insert(format!("card:{err}"));
            } else {
                blockers.insert("profile:not-wired-in-current".to_string());
            }
        }
        if let Some(id) = &card.oracle_id {
            candidate_blockers.insert(id.clone(), blockers.clone());
        }
        *blocker_hist.entry(blockers.len()).or_insert(0) += 1;
        for b in &blockers {
            *shared_blocker_cards.entry(b.clone()).or_insert(0) += 1;
        }
        if blockers.len() == 1
            && let Some(only) = blockers.into_iter().next()
        {
            solo_blocker_cards
                .entry(only)
                .or_default()
                .push(card.name.clone());
        }
    }

    // Optional deterministic evidence for before/after whole-card set comparisons.
    if let Some(path) = std::env::var_os("SPELLWAKE_COVERAGE_SNAPSHOT") {
        let cards: BTreeMap<_, _> = inventory
            .cards
            .iter()
            .filter(|card| card.required_by_base_formats)
            .map(|card| {
                let id = card.oracle_id.as_ref().expect("required card Oracle ID");
                (
                    id.clone(),
                    serde_json::json!({
                        "name": card.name,
                        "bound": bound_ids.contains(id),
                        "standalone_valid": oracle_ok_ids.contains(id),
                        "standalone_error": oracle_errors.get(id),
                        "candidate_blockers": candidate_blockers.get(id),
                    }),
                )
            })
            .collect();
        assert_eq!(
            cards.len(),
            total_required,
            "one row per required Oracle ID"
        );
        serde_json::to_writer_pretty(
            File::create(path).expect("create coverage snapshot"),
            &cards,
        )
        .expect("write coverage snapshot");
    }

    eprintln!("=== GATE 4: HONEST TIERS & MULTI-BLOCKER WHOLE-CARD QUEUE ===");
    eprintln!(
        "Tier 0 (Stage 1 strict AST parse):  {strict_ast_cards} / {total_required} ({:.2}%)",
        (strict_ast_cards as f64 / total_required as f64) * 100.0
    );
    eprintln!(
        "Tier 1 (oracle_lowering valid):     {oracle_ok} / {total_required} ({:.2}%)",
        (oracle_ok as f64 / total_required as f64) * 100.0
    );
    eprintln!(
        "Tier 2 (Profile::Current bound):    {total_bound} / {total_required} ({:.2}%)",
        (total_bound as f64 / total_required as f64) * 100.0
    );
    eprintln!(
        "Unbound cards: {} (AST-incomplete: {}) | Blocker cardinality histogram: {:?}",
        total_required.saturating_sub(total_bound),
        ast_incomplete_unbound,
        blocker_hist
    );

    let mut solo_sorted: Vec<(String, Vec<String>)> = solo_blocker_cards.into_iter().collect();
    solo_sorted.sort_by_key(|e| std::cmp::Reverse(e.1.len()));
    eprintln!(
        "--- TOP 45 SINGLE-BLOCKER CAPABILITIES (CANDIDATE WHOLE CARDS; GAINS REQUIRE REAUDIT) ---"
    );
    for (cap, names) in solo_sorted.iter().take(45) {
        let total_hits = shared_blocker_cards.get(cap).copied().unwrap_or(0);
        let sample: Vec<&str> = names.iter().take(4).map(String::as_str).collect();
        eprintln!(
            "  solo={:5}  total={:5}  {cap}  e.g. {:?}",
            names.len(),
            total_hits,
            sample
        );
    }

    let mut shared_sorted: Vec<(String, usize)> = shared_blocker_cards.into_iter().collect();
    shared_sorted.sort_by_key(|e| std::cmp::Reverse(e.1));
    eprintln!(
        "--- TOP 45 SHARED PREREQUISITE CAPABILITIES (TOTAL UNBOUND CARDS TOUCHING BLOCKER) ---"
    );
    for (cap, total_hits) in shared_sorted.iter().take(45) {
        eprintln!("  total={total_hits:5}  {cap}");
    }
}

#[test]
fn pass_e12_filter_and_chooser_fidelity() {
    let vocab = synthetic_vocab();

    // 1. KEYWORDS_MATCH_ANY lowers two-keyword OR to ObjectFilter::Either, while AND stays fail-closed.
    let or_kw = synthetic_card(
        "Sky Swatter",
        "Instant",
        Some("{1}{G}"),
        None,
        "Destroy target creature with flying or reach.",
    );
    let or_prog = compile_with_reason(&or_kw, &vocab).expect("flying or reach should lower");
    let ProgramKind::Spell(or_spell) = or_prog.kind else {
        panic!("expected spell");
    };
    let BodyTarget::FilteredPermanent(filter) = or_spell.body.target else {
        panic!("expected filtered permanent target");
    };
    let (c1, c2) = filter.clauses();
    let c2 = c2.expect("two-keyword OR must lower to ObjectFilter::Either");
    assert_eq!(c1.keyword, Some(KeywordAbility::Flying));
    assert_eq!(c2.keyword, Some(KeywordAbility::Reach));

    let and_kw = synthetic_card(
        "Dual Keyword Bane",
        "Instant",
        Some("{1}{B}"),
        None,
        "Destroy target creature with flying and vigilance.",
    );
    assert!(
        compile_with_reason(&and_kw, &vocab).is_err(),
        "AND multi-keyword filter must fail closed"
    );

    // 2. SubtypesMatchAny lowers OR subtypes while AND multi-subtypes stay fail-closed.
    let or_sub = synthetic_card(
        "Tribal Snipe",
        "Instant",
        Some("{1}{B}"),
        None,
        "Destroy target Elf or Goblin.",
    );
    assert!(
        compile_with_reason(&or_sub, &vocab).is_ok(),
        "OR multi-subtype filter should lower"
    );

    let and_sub = synthetic_card(
        "Specific Snipe",
        "Instant",
        Some("{1}{B}"),
        None,
        "Destroy target Human Warrior.",
    );
    assert!(
        compile_with_reason(&and_sub, &vocab).is_err(),
        "AND multi-subtype filter must fail closed"
    );

    // 3. ChosenByPlayer matches actor on sacrifice, while mismatched chooser stays fail-closed.
    let self_choice_sac = synthetic_card(
        "Fair Edict",
        "Sorcery",
        Some("{1}{B}"),
        None,
        "Each opponent sacrifices a creature of their choice they control.",
    );
    assert!(
        compile_with_reason(&self_choice_sac, &vocab).is_ok(),
        "matching chooser on sacrifice should lower"
    );

    let foreign_choice_sac = synthetic_card(
        "Foreign Edict",
        "Sorcery",
        Some("{1}{B}"),
        None,
        "Target opponent sacrifices a creature of your choice they control.",
    );
    assert!(
        compile_with_reason(&foreign_choice_sac, &vocab).is_err(),
        "foreign chooser on sacrifice must fail closed"
    );

    // 4. EntersTappedAndWithCounters lowers both EntryTap::Always and entry counters.
    let tapped_with_counter = synthetic_card(
        "Slumbering Construct",
        "Artifact Creature \u{2014} Construct",
        Some("{2}"),
        Some(("1", "1")),
        "Slumbering Construct enters tapped with a +1/+1 counter on it.",
    );
    let twc_prog = compile_with_reason(&tapped_with_counter, &vocab)
        .expect("enters tapped with a +1/+1 counter should lower");
    let ProgramKind::Permanent(def) = twc_prog.kind else {
        panic!("expected permanent");
    };
    assert_eq!(
        def.entry_tap,
        Some(spellwake_engine::land::EntryTap::Always)
    );
    assert!(!twc_prog.entry_counter_abilities.is_empty());
}

#[test]
#[ignore = "uses saved dated Scryfall corpus and vocabulary"]
fn wave10_source_action_catalogue_regression() {
    let (inventory, vocabulary, _) = current_catalogue();
    for name in [
        "Nezumi Graverobber // Nighteyes the Desecrator",
        "Dormant Grove // Gnarled Grovestrider",
        "Hadana's Climb // Winged Temple of Orazca",
        "Mysterious Tome // Chilling Chronicle",
        "Nezumi Shortfang // Stabwhisker the Odious",
    ] {
        let card = inventory
            .cards
            .iter()
            .find(|card| card.name == name)
            .expect("pinned card exists");
        let result = compile_with_inventory_reason(card, inventory, &vocabulary);
        assert!(result.is_ok(), "{name}: {:?}", result.err());
    }
}
