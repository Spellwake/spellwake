//! Actual-source finite interaction qualification. No synthetic catalogue fallback.
use spellwake_reference_tools::{
    creature_types,
    ordinary_game::{
        fixture::FixtureInitialization,
        interaction::{InteractionCase, InteractionSource},
    },
};
use std::{collections::BTreeSet, path::PathBuf};

#[test]
fn interaction_case_names_exclude_non_game_suite_obligations() {
    for case in InteractionCase::ALL {
        assert_eq!(InteractionCase::parse(case.as_str()).unwrap(), case);
    }
    for name in ["IF13", "IF14", "if01", "BI03", "BI01 ", "RD01"] {
        assert!(InteractionCase::parse(name).is_err());
    }
    assert_eq!(
        InteractionCase::FOCUSED.len() + InteractionCase::FULL_GAMES.len(),
        InteractionCase::ALL.len()
    );
}

#[test]
#[ignore = "requires actual pinned catalogue in SPELLWAKE_TEACHING_CATALOGUE_DIR and source-qualified vocabulary; run explicitly"]
fn actual_interaction_games_and_prepared_edges_reexecute_complete_journals_and_state() {
    let vocabulary =
        creature_types::selected_vocabulary().expect("explicit source-qualified vocabulary");
    let directory = PathBuf::from(
        std::env::var_os("SPELLWAKE_TEACHING_CATALOGUE_DIR")
            .expect("explicit actual catalogue directory"),
    );
    let source = InteractionSource::load(
        &directory.join("oracle-metadata.json"),
        &directory.join("oracle-cards-20261029210158.jsonl.gz"),
        &vocabulary,
    )
    .unwrap();
    let mut probe_evidence = std::collections::BTreeMap::<String, BTreeSet<String>>::new();
    let mut exact_probe_contexts =
        std::collections::BTreeMap::<String, Vec<serde_json::Value>>::new();
    let mut context_mutation_controls = BTreeSet::new();
    let mut case_receipts = vec![];
    for case in InteractionCase::ALL {
        let fixture = source.fixture(case, vocabulary.clone()).unwrap();
        assert_eq!(fixture.aliases.len(), 120);
        assert_eq!(fixture.setup.seed.cards.len(), 120);
        assert_eq!(
            fixture
                .setup
                .seed
                .cards
                .iter()
                .map(|s| s.card)
                .collect::<BTreeSet<_>>()
                .len(),
            120
        );
        let opening = matches!(fixture.initialization, FixtureInitialization::Opening);
        assert_eq!(opening, InteractionCase::FULL_GAMES.contains(&case));
        let report = source.run(&[case], vocabulary.clone()).unwrap();
        let json = serde_json::to_value(&report).unwrap();
        let run = &json["runs"][0];
        assert_eq!(
            run["completed"],
            true,
            "{}: {}",
            case.as_str(),
            run["failure"]
        );
        assert!(report.completed());
        assert_eq!(run["schema_version"], 2);
        assert_eq!(run["rerun"]["initialization_equal"], true);
        assert_eq!(run["rerun"]["final_state_equal"], true);
        assert_eq!(run["rerun"]["all_inputs_consumed"], true);
        assert_eq!(
            run["rerun"]["compared_ordinals"].as_array().unwrap().len(),
            run["accepted"].as_array().unwrap().len()
        );
        let natural = matches!(
            case,
            InteractionCase::Bi01
                | InteractionCase::Bi02
                | InteractionCase::If11
                | InteractionCase::If12
        );
        if natural {
            assert_eq!(run["final"]["outcome"]["kind"], "winner");
            assert_eq!(
                run["final"]["outcome"]["seat"],
                if case == InteractionCase::Bi02 { 41 } else { 3 }
            );
            assert_eq!(run["final"]["entropy_position"].is_null(), !opening);
        } else {
            assert_eq!(run["final"]["kind"], "bounded_prefix");
            assert_eq!(
                run["final"]["boundary_ref"],
                format!("{}.settled", case.as_str())
            );
            assert!(run["final"].get("outcome").is_none());
        }
        let final_ordinal = run["final"]["final_ordinal"].as_u64().unwrap().to_string();
        assert!(run["checkpoints"].get(&final_ordinal).is_some());
        for probe in run["rejection_probes"].as_array().unwrap() {
            assert_eq!(probe["state_retained"], true);
            assert_eq!(probe["no_candidate"], true);
            probe_evidence
                .entry(probe["expectation_ref"].as_str().unwrap().into())
                .or_default()
                .insert(case.as_str().into());
            if let Some(mut context) = checked_probe_context(run, probe) {
                context["caseId"] = case.as_str().into();
                exact_probe_contexts
                    .entry(probe["expectation_ref"].as_str().unwrap().into())
                    .or_default()
                    .push(context);
            }
            if let Some((id, changed)) = probe_context_mutation(probe)
                && !context_mutation_controls.contains(id)
            {
                assert!(
                    std::panic::catch_unwind(|| checked_probe_context(run, &changed)).is_err(),
                    "context matcher accepted invalid probe metadata: {id}"
                );
                context_mutation_controls.insert(id.to_owned());
            }
        }
        case_receipts.push(serde_json::json!({"case":case.as_str(),"completed":true,"accepted":run["accepted"].as_array().unwrap().len(),"rerun":run["rerun"],"final":run["final"]}));
        let accepted = run["accepted"].as_array().unwrap();
        assert!(accepted.iter().any(|a| {
            a["policy_entry_ref"]
                .as_str()
                .is_some_and(|id| id.starts_with(case.as_str()) && !id.contains("automatic"))
        }));
        assert!(
            accepted
                .iter()
                .any(|a| a["current_facts"].get("interaction_journal").is_some())
        );
    }
    for family in [
        "IF13.counter-insufficient-payment",
        "IF13.cancel-insufficient-payment",
        "IF13.unavailable-mana-color",
        "IF13.foreign-actor",
    ] {
        assert!(
            exact_probe_contexts
                .get(family)
                .is_some_and(|contexts| !contexts.is_empty()),
            "missing exact requested probe context: {family}"
        );
    }
    assert_eq!(context_mutation_controls.len(), 6);
    if let Some(path) = std::env::var_os("SPELLWAKE_INTERACTION_CHECK_RECEIPT") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"kind":"actual-development-test-receipt","cases":case_receipts,"executed_root_probe_families":probe_evidence,"checked_probe_contexts":exact_probe_contexts,"context_assertion_mutation_controls":context_mutation_controls})).unwrap()).unwrap();
    }
}

/// Deliberately inconsistent private reader inputs, never additional engine attempts.
fn probe_context_mutation(probe: &serde_json::Value) -> Option<(&'static str, serde_json::Value)> {
    let mut changed = probe.clone();
    let id = match probe["expectation_ref"].as_str().unwrap() {
        "IF13.counter-insufficient-pool" => {
            changed["expectation_ref"] = "IF13.counter-insufficient-payment".into();
            "counter-pool-mislabeled-as-short-spend"
        }
        "IF13.cancel-insufficient-pool" => {
            changed["expectation_ref"] = "IF13.cancel-insufficient-payment".into();
            "cancel-pool-mislabeled-as-short-spend"
        }
        "IF13.counter-insufficient-payment" => {
            changed["attempted_input"]["command"]["payment"]["spend"][0]["amount"] = "2".into();
            "counter-submitted-full-spend"
        }
        "IF13.cancel-insufficient-payment" => {
            changed["attempted_input"]["command"]["payment"]["spend"][0]["amount"] = "3".into();
            "cancel-submitted-full-spend"
        }
        "IF13.unavailable-mana-color" => {
            changed["attempted_input"]["command"]["payment"]["spend"][1]["mana_type"] =
                "red".into();
            "unavailable-color-red-instead-of-green"
        }
        "IF13.unknown-actor" if probe["attempted_input"]["command"]["kind"] == "pass" => {
            changed["expectation_ref"] = "IF13.foreign-actor".into();
            "unknown-seat-mislabeled-as-registered-priority"
        }
        _ => return None,
    };
    Some((id, changed))
}

/// Join a rejection to its actual retained source and successful continuation.
/// Family names alone cannot establish payment, color or registered-priority coverage.
fn checked_probe_context(
    run: &serde_json::Value,
    probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    use serde_json::json;
    let family = probe["expectation_ref"].as_str().unwrap();
    if !matches!(
        family,
        "IF13.counter-insufficient-payment"
            | "IF13.cancel-insufficient-payment"
            | "IF13.unavailable-mana-color"
            | "IF13.foreign-actor"
            | "IF13.unknown-actor"
            | "IF13.counter-insufficient-pool"
            | "IF13.cancel-insufficient-pool"
    ) {
        return None;
    }
    let ordinal = probe["attached_candidate_ordinal"].as_u64().unwrap() as usize;
    let source = if ordinal == 0 {
        &run["initialization"]
    } else {
        &run["accepted"][ordinal - 1]
    };
    assert_eq!(source["ordinal"].as_u64().unwrap() as usize, ordinal);
    let header = &source["header"];
    let attempted = &probe["attempted_input"];
    let command = &attempted["command"];
    assert_eq!(header["basis"], probe["original_basis"]);
    assert_eq!(attempted["basis"], probe["original_basis"]);
    assert_eq!(probe["expected_error"], probe["actual_error"]);
    assert_eq!(probe["state_retained"], true);
    assert_eq!(probe["no_candidate"], true);
    assert!(
        run["rerun"]["probe_ids_compared"]
            .as_array()
            .unwrap()
            .contains(&probe["probe_id"])
    );
    let continuation_ordinal = probe["continuation_ordinal"].as_u64().unwrap() as usize;
    let continuation = &run["accepted"][continuation_ordinal - 1];
    assert_eq!(continuation["input"]["basis"], probe["original_basis"]);
    let mut context = json!({"probeId":probe["probe_id"],"sourceOrdinal":ordinal,"sourceBasis":header["basis"],"participants":header["participants"],"priorityHolder":header["priority_holder"],"sourceMana":header["mana"],"attemptedInput":attempted,"error":probe["actual_error"],"noCandidate":true,"fullStateRetained":true,"rerunCompared":true,"continuationOrdinal":continuation_ordinal,"successfulContinuation":continuation["input"]});
    if family == "IF13.unknown-actor" {
        assert_eq!(attempted["actor"], 999);
        assert!(
            !header["participants"]
                .as_array()
                .unwrap()
                .contains(&attempted["actor"])
        );
        assert_eq!(probe["actual_error"]["path"], json!(["UnknownActor"]));
        context["requestedObligationMatch"] = false.into();
        return Some(context);
    }
    assert_eq!(header["phase"]["kind"], "priority");
    assert_eq!(header["phase"]["phase"]["kind"], "ready");
    assert!(header["pending"].is_null());
    assert!(
        header["participants"]
            .as_array()
            .unwrap()
            .contains(&attempted["actor"])
    );
    if family == "IF13.foreign-actor" {
        assert_ne!(attempted["actor"], header["priority_holder"]);
        assert_eq!(*command, json!({"kind":"pass"}));
        assert_eq!(continuation["input"]["actor"], header["priority_holder"]);
        assert_eq!(continuation["input"]["command"], json!({"kind":"pass"}));
        assert_eq!(
            probe["actual_error"]["path"],
            json!(["Effect", "Window", "Priority", "WrongPlayer"])
        );
        context["requestedObligationMatch"] = true.into();
        return Some(context);
    }
    assert_eq!(attempted["actor"], 41);
    assert_eq!(header["priority_holder"], 41);
    assert_eq!(command["kind"], "cast_paid");
    assert_eq!(command["target"]["kind"], "stack");
    let live = header["stack"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["entry"] == command["target"]["entry"])
        .expect("requested target is a real live entry");
    assert_eq!(live["kind"]["kind"], "card_spell");
    let pool = &header["mana"]["41"];
    for color in ["black", "green", "red", "white", "colorless"] {
        assert_eq!(pool[color], "0");
    }
    let spend = &command["payment"]["spend"];
    let (name, full_cost) = if family.starts_with("IF13.counter-") {
        ("counterspell", 2)
    } else {
        ("cancel", 3)
    };
    assert!(
        command["spell"]["alias"]
            .as_str()
            .unwrap()
            .starts_with(&format!("U.{name}."))
    );
    let extra_pool = family.ends_with("-pool");
    if extra_pool {
        assert!(pool["blue"].as_str().unwrap().parse::<u32>().unwrap() < full_cost);
        assert_eq!(
            *spend,
            json!([{"mana_type":"blue","amount":full_cost.to_string()}])
        );
        assert_eq!(
            probe["actual_error"]["path"],
            json!(["Effect", "InsufficientMana"])
        );
        assert_eq!(continuation["input"]["command"]["kind"], "activate_mana");
        context["requestedObligationMatch"] = false.into();
        return Some(context);
    }
    assert_eq!(pool["blue"], full_cost.to_string());
    let expected_path = match family {
        "IF13.counter-insufficient-payment" => {
            assert_eq!(*spend, json!([{"mana_type":"blue","amount":"1"}]));
            json!(["Effect", "InvalidManaPayment", "MissingRequired"])
        }
        "IF13.cancel-insufficient-payment" => {
            assert_eq!(*spend, json!([{"mana_type":"blue","amount":"2"}]));
            json!(["Effect", "InvalidManaPayment", "WrongTotal"])
        }
        "IF13.unavailable-mana-color" => {
            assert_eq!(
                *spend,
                json!([{"mana_type":"blue","amount":"2"},{"mana_type":"green","amount":"1"}])
            );
            json!(["Effect", "InsufficientMana"])
        }
        _ => unreachable!("closed selected probe vocabulary"),
    };
    assert_eq!(probe["actual_error"]["path"], expected_path);
    assert!(ordinal > 0);
    assert_eq!(source["input"]["command"]["kind"], "activate_mana");
    assert_eq!(source["input"]["actor"], 41);
    let tap = &source["input"]["command"]["source"];
    assert_eq!(source["current_facts"]["mana_production"]["source"], *tap);
    assert_eq!(
        source["current_facts"]["mana_production"]["produced"],
        json!({"blue":"1"})
    );
    let before_cards = run["checkpoints"][ordinal.to_string()]["cards"]
        .as_object()
        .unwrap();
    let tapped = before_cards
        .values()
        .find(|card| card["alias"] == tap["alias"])
        .unwrap();
    assert_eq!(tapped["object"]["object"], tap["object"]);
    assert_eq!(tapped["object"]["tapped"], true);
    assert_eq!(tapped["object"]["controller"], 41);
    assert_eq!(continuation["input"]["command"]["kind"], "cast_paid");
    assert_eq!(continuation["input"]["command"]["spell"], command["spell"]);
    assert_eq!(
        continuation["input"]["command"]["target"],
        command["target"]
    );
    assert_eq!(
        continuation["input"]["command"]["payment"]["spend"],
        json!([{"mana_type":"blue","amount":full_cost.to_string()}])
    );
    assert_eq!(continuation["header"]["mana"]["41"]["blue"], "0");
    let after_cards = run["checkpoints"][continuation_ordinal.to_string()]["cards"]
        .as_object()
        .unwrap();
    assert_eq!(
        after_cards
            .values()
            .find(|card| card["alias"] == tap["alias"])
            .unwrap()["object"]["tapped"],
        true
    );
    context["priorAcceptedTap"] = source["input"].clone();
    context["priorAcceptedManaProduction"] = source["current_facts"]["mana_production"].clone();
    context["priorTapRetainedAfterContinuation"] = true.into();
    context["requestedObligationMatch"] = true.into();
    Some(context)
}

#[test]
fn interaction_cli_requires_explicit_source_and_rejects_suite_ids() {
    for args in [
        vec!["--unknown"],
        vec!["--metadata"],
        vec!["--case", "IF13"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_interaction-game"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(
            message.contains("usage:")
                || message.contains("missing interaction")
                || message.contains("unknown interaction case"),
            "{message}"
        );
    }
}

/// A small fixed command prefix for negative probes, not another gameplay policy.
fn advance(
    state: &spellwake_engine::game::GameState,
    actor: spellwake_engine::priority::SeatId,
    command: spellwake_engine::game::GameCommand,
) -> spellwake_engine::game::GameState {
    state
        .advance(spellwake_engine::game::GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })
        .unwrap()
        .into_state()
}
fn payment(amount: i64) -> spellwake_engine::mana::ManaPayment {
    spellwake_engine::mana::ManaPayment {
        spend: vec![spellwake_engine::mana::ManaSpend {
            mana_type: spellwake_engine::mana::ManaType::Blue,
            amount: amount.into(),
        }],
    }
}
fn reject(
    state: &spellwake_engine::game::GameState,
    actor: spellwake_engine::priority::SeatId,
    command: spellwake_engine::game::GameCommand,
    expected: spellwake_engine::game::GameError,
) {
    let retained = state.clone();
    assert_eq!(
        state
            .advance(spellwake_engine::game::GameInput {
                basis: state.basis().clone(),
                actor,
                command
            })
            .unwrap_err(),
        expected
    );
    assert_eq!(*state, retained);
}

#[test]
#[ignore = "requires actual pinned catalogue in SPELLWAKE_TEACHING_CATALOGUE_DIR and source-qualified vocabulary; run explicitly"]
fn prepared_negative_timing_completed_target_and_creature_spell_probes_are_atomic() {
    use spellwake_engine::{
        actions::TimingError,
        effects::EffectError,
        game::{GameCommand, GameError, GameId, GameState},
        land::BasicLandType,
        mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
        object_identity::ObjectDestination,
        targets::TargetRef,
        turn::{Step, TurnStart},
    };
    use spellwake_reference_tools::ordinary_game::fixture::{A, B};
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let directory = PathBuf::from(
        std::env::var_os("SPELLWAKE_TEACHING_CATALOGUE_DIR")
            .expect("explicit actual catalogue directory"),
    );
    let source = InteractionSource::load(
        &directory.join("oracle-metadata.json"),
        &directory.join("oracle-cards-20261029210158.jsonl.gz"),
        &vocabulary,
    )
    .unwrap();
    // Explicit local negative premise: add Divination and another Counterspell to
    // the hand by moving their existing admitted instances, preserving all120cards.
    let mut fixture = source
        .fixture(InteractionCase::If06, vocabulary.clone())
        .unwrap();
    for alias in ["U.divination.1", "U.counterspell.2"] {
        let card = fixture.card(alias).unwrap();
        fixture
            .setup
            .seed
            .cards
            .iter_mut()
            .find(|s| s.card == card)
            .unwrap()
            .destination = ObjectDestination::Hand;
    }
    let (_, context) = fixture.probe_context().unwrap();
    let mut state = GameState::initialize_scheduled(
        GameId(5501),
        context,
        fixture.setup.seed.clone(),
        TurnStart {
            number: 19,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    for n in 1..=6 {
        state = advance(
            &state,
            A,
            GameCommand::ActivateMana {
                source: fixture
                    .current_object(&state, &format!("U.island.{n}"))
                    .unwrap(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        );
    }
    let unsummon = fixture.current_object(&state, "U.unsummon.1").unwrap();
    state = advance(
        &state,
        A,
        GameCommand::CastPaid {
            spell: unsummon,
            target: TargetRef::Object(fixture.current_object(&state, "G.grizzly-bears.1").unwrap()),
            payment: payment(1),
        },
    );
    let entry = state.execution().window().entries()[0].id();
    reject(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: fixture.current_object(&state, "U.divination.1").unwrap(),
            payment: Some(payment(3)),
        },
        GameError::Effect(EffectError::Timing(TimingError::StackNotEmpty)),
    );
    state = advance(
        &state,
        A,
        GameCommand::CastPaid {
            spell: fixture.current_object(&state, "U.counterspell.1").unwrap(),
            target: TargetRef::Stack(entry),
            payment: payment(2),
        },
    );
    state = advance(&state, A, GameCommand::Pass);
    state = advance(&state, B, GameCommand::Pass);
    assert!(state.execution().window().entries().is_empty());
    reject(
        &state,
        A,
        GameCommand::CastPaid {
            spell: fixture.current_object(&state, "U.counterspell.2").unwrap(),
            target: TargetRef::Stack(entry),
            payment: payment(2),
        },
        GameError::Effect(EffectError::IllegalTarget),
    );
    assert_eq!(state.execution().mana(A, ManaType::Blue), 3.into());

    let mut opponent = source
        .fixture(InteractionCase::If06, vocabulary.clone())
        .unwrap();
    opponent.setup.seed.active = B;
    let card = opponent.card("U.divination.1").unwrap();
    opponent
        .setup
        .seed
        .cards
        .iter_mut()
        .find(|s| s.card == card)
        .unwrap()
        .destination = ObjectDestination::Hand;
    let (_, context) = opponent.probe_context().unwrap();
    let mut state = GameState::initialize_scheduled(
        GameId(5502),
        context,
        opponent.setup.seed.clone(),
        TurnStart {
            number: 20,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    state = advance(&state, B, GameCommand::Pass);
    for n in 1..=3 {
        state = advance(
            &state,
            A,
            GameCommand::ActivateMana {
                source: opponent
                    .current_object(&state, &format!("U.island.{n}"))
                    .unwrap(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        );
    }
    reject(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: opponent.current_object(&state, "U.divination.1").unwrap(),
            payment: Some(payment(3)),
        },
        GameError::Effect(EffectError::Timing(TimingError::NotActivePlayer)),
    );
    assert_eq!(state.execution().mana(A, ManaType::Blue), 3.into());

    let mut creature = source.fixture(InteractionCase::If02, vocabulary).unwrap();
    let card = creature.card("U.unsummon.1").unwrap();
    creature
        .setup
        .seed
        .cards
        .iter_mut()
        .find(|s| s.card == card)
        .unwrap()
        .destination = ObjectDestination::Hand;
    let (_, context) = creature.probe_context().unwrap();
    let mut state = GameState::initialize_scheduled(
        GameId(5503),
        context,
        creature.setup.seed.clone(),
        TurnStart {
            number: 20,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    for n in 1..=2 {
        state = advance(
            &state,
            B,
            GameCommand::ActivateMana {
                source: creature
                    .current_object(&state, &format!("G.forest.{n}"))
                    .unwrap(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
            },
        );
    }
    state = advance(
        &state,
        B,
        GameCommand::CastUntargeted {
            spell: creature
                .current_object(&state, "G.grizzly-bears.1")
                .unwrap(),
            payment: Some(ManaPayment {
                spend: vec![ManaSpend {
                    mana_type: ManaType::Green,
                    amount: 2.into(),
                }],
            }),
        },
    );
    state = advance(&state, B, GameCommand::Pass);
    state = advance(
        &state,
        A,
        GameCommand::ActivateMana {
            source: creature.current_object(&state, "U.island.1").unwrap(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    );
    reject(
        &state,
        A,
        GameCommand::CastPaid {
            spell: creature.current_object(&state, "U.unsummon.1").unwrap(),
            target: TargetRef::Object(
                creature
                    .current_object(&state, "G.grizzly-bears.1")
                    .unwrap(),
            ),
            payment: payment(1),
        },
        GameError::Effect(EffectError::IllegalTarget),
    );
    if let Some(path) = std::env::var_os("SPELLWAKE_INTERACTION_NEGATIVE_RECEIPT") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"kind":"actual-source-prepared-negative-tests","root_command_rejections":["IF13.divination-in-response","IF13.counter-completed-entry","IF13.divination-opponent-turn","IF13.unsummon-creature-spell"],"all_no_candidate_and_full_state_retained":true,"prepared_source_cases":["IF06","IF02"],"changes":"existing source-admitted physical cards moved to hand; opponent case changes active role; no Program/control history/combat injection"})).unwrap()).unwrap();
    }
}
