//! Explicit actual-source finite qualification. Synthetic mechanisms stay separately named.
use spellwake_reference_tools::{
    creature_types,
    ordinary_game::{
        fixture::{A, B, FixtureInitialization},
        mandatory::{MandatoryCase, MandatorySource},
    },
};
use std::{collections::BTreeSet, path::PathBuf};
fn source() -> (
    std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    MandatorySource,
) {
    let vocabulary =
        creature_types::selected_vocabulary().expect("explicit source-qualified vocabulary");
    let dir = PathBuf::from(
        std::env::var_os("SPELLWAKE_MANDATORY_CATALOGUE_DIR")
            .expect("explicit actual mandatory catalogue"),
    );
    let source = MandatorySource::load(
        &dir.join("oracle-metadata.json"),
        &dir.join("oracle-cards-20261029210158.jsonl.gz"),
        &vocabulary,
    )
    .unwrap();
    (vocabulary, source)
}
#[test]
fn mandatory_case_names_are_the_exact_finite_execution_domain() {
    for case in MandatoryCase::ALL {
        assert_eq!(MandatoryCase::parse(case.as_str()).unwrap(), case);
    }
    for id in [
        "T10",
        "T09.M.control-change",
        "MT03",
        "mt01",
        "T01",
        "MT01 ",
    ] {
        assert!(MandatoryCase::parse(id).is_err());
    }
    assert_eq!(MandatoryCase::ALL.len(), 23);
}
#[test]
#[ignore = "requires actual pinned catalogue and source-qualified vocabulary; run explicitly"]
fn mandatory_full_source_games_and_reduced_cases() {
    let (vocabulary, source) = source();
    let mut cases = vec![];
    let mut natural = 0;
    let mut bounded = 0;
    for case in MandatoryCase::ALL {
        let fixture = source.fixture(case, vocabulary.clone()).unwrap();
        assert_eq!(fixture.setup.seed.seats, vec![A, B]);
        assert!(fixture.setup.seed.mana.is_empty());
        let opening = matches!(fixture.initialization, FixtureInitialization::Opening);
        assert_eq!(opening, MandatoryCase::FULL_GAMES.contains(&case));
        assert_eq!(fixture.setup.seed.cards.len(), fixture.aliases.len());
        assert_eq!(
            fixture
                .setup
                .seed
                .cards
                .iter()
                .map(|s| s.card)
                .collect::<BTreeSet<_>>()
                .len(),
            fixture.aliases.len()
        );
        if opening {
            assert_eq!(fixture.aliases.len(), 120);
        }
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
        assert_eq!(run["schema_version"], 3);
        assert_eq!(run["rerun"]["initialization_equal"], true);
        assert_eq!(run["rerun"]["final_state_equal"], true);
        assert_eq!(run["rerun"]["all_inputs_consumed"], true);
        assert_eq!(
            run["rerun"]["compared_ordinals"].as_array().unwrap().len(),
            run["accepted"].as_array().unwrap().len()
        );
        assert!(run["accepted"].as_array().unwrap().iter().all(|a| {
            a["current_facts"].get("rules_journal").is_some()
                && a.get("token_changes").is_some()
                && a["header"].get("execution").is_some()
        }));
        if run["final"].get("outcome").is_some() {
            natural += 1;
        } else {
            bounded += 1;
            assert_eq!(run["final"]["kind"], "bounded_prefix");
        }
        for p in run["rejection_probes"].as_array().unwrap() {
            assert_eq!(p["no_candidate"], true);
            assert_eq!(p["state_retained"], true);
        }
        cases.push(serde_json::json!({"case":case.as_str(),"accepted":run["accepted"].as_array().unwrap().len(),"final":run["final"],"rerun":run["rerun"]}));
    }
    assert_eq!((natural, bounded), (6, 17));
    receipt(
        "SPELLWAKE_MANDATORY_CHECK_RECEIPT",
        serde_json::json!({"kind":"actual-source-bound-mandatory-game-tests","cases":cases,"natural":natural,"bounded":bounded}),
    );
}
#[test]
#[ignore = "requires actual pinned catalogue and source-qualified vocabulary; foreign-case instance is a separate same-crate test-only selector"]
fn mandatory_decision_authority_probes() {
    let (vocabulary, source) = source();
    let mut actual = BTreeSet::new();
    for case in [
        MandatoryCase::T01OwnThenOpponent,
        MandatoryCase::T02OwnMain,
        MandatoryCase::T06ArtistTop,
        MandatoryCase::T06SplitTargets,
        MandatoryCase::T08Self,
    ] {
        let report = source.run(&[case], vocabulary.clone()).unwrap();
        assert!(
            report.completed(),
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        let json = serde_json::to_value(report).unwrap();
        for probe in json["runs"][0]["rejection_probes"].as_array().unwrap() {
            let id = probe["expectation_ref"].as_str().unwrap();
            let ordinal = probe["attached_candidate_ordinal"].as_u64().unwrap();
            assert_eq!(
                probe["probe_id"],
                format!("{id}@{ordinal}"),
                "probe identity must retain actual attached candidate"
            );
            assert!(
                actual.insert(id.to_owned()),
                "duplicate T10 family attachment{id}"
            );
            assert_eq!(probe["no_candidate"], true);
            assert_eq!(probe["state_retained"], true);
            assert_eq!(probe["expected_error"], probe["actual_error"]);
        }
    }
    let expected = [
        "T10.wrong-actor",
        "T10.wrong-decision-kind",
        "T10.stale-outer-basis",
        "T10.stale-trigger-instance",
        "T10.omitted-order-member",
        "T10.duplicate-order-member",
        "T10.foreign-order-member",
        "T10.target-from-another-frame",
        "T10.creature-target-in-player-frame",
        "T10.stack-target-in-player-frame",
        "T10.repeated-consumed-reply",
        "T10.wrong-actor.timestamp",
        "T10.wrong-decision-kind.timestamp",
        "T10.omitted-order-member.timestamp",
        "T10.duplicate-order-member.timestamp",
        "T10.foreign-order-member.timestamp",
        "T10.wrong-actor.graveyard",
        "T10.wrong-decision-kind.graveyard",
        "T10.omitted-order-member.graveyard",
        "T10.duplicate-order-member.graveyard",
        "T10.foreign-order-member.graveyard",
    ]
    .map(str::to_owned)
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    receipt(
        "SPELLWAKE_MANDATORY_PROBE_RECEIPT",
        serde_json::json!({"kind":"actual-ordinary-recorded-replayed-mandatory-probes","probe_ids":actual,"separate_test_only_family":"T10.foreign-trigger-instance"}),
    );
}
fn receipt(variable: &str, value: serde_json::Value) {
    if let Some(path) = std::env::var_os(variable) {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "actual guarded source plus separately labeled synthetic control effect; explicitly qualify with --ignored --exact"]
fn mandatory_llanowar_control_change_is_a_separate_synthetic_mechanism() {
    use spellwake_engine::{
        characteristics::{
            ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        },
        effects::{EffectError, SeedCard},
        game::{
            DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
            RuleFrame,
        },
        mana::{ManaAbilityId, ManaAbilityRef, ManaCost, ManaType},
        object_identity::{CardId, ObjectDestination},
        priority::PriorityPhase,
        program::{BodyTarget, ExecutableBody, Instruction, Program},
        stack_window::{StackWindowError, WindowPhase},
        targets::TargetRef,
        turn::{Step, TurnStart},
    };
    let (vocabulary, source) = source();
    let fixture = source
        .fixture(MandatoryCase::T09BounceRecastStale, vocabulary.clone())
        .unwrap();
    let llanowar = fixture.card("P0.llanowar-visionary.1").unwrap();
    let island = fixture.card("P1.island.1").unwrap();
    let mut seed = fixture.setup.seed.clone();
    seed.cards
        .iter_mut()
        .find(|s| s.card == island)
        .unwrap()
        .destination = ObjectDestination::LibraryBottom;
    seed.initial_battlefield_order.retain(|c| *c != island);
    let synthetic = CardId(999);
    seed.cards.push(SeedCard {
        card: synthetic,
        owner: B,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::NoStatedDuration,
                }],
            ),
        ),
    });
    let context = GameContext::new(
        "synthetic-mandatory-control-test".into(),
        vocabulary.rules_revision().into(),
        "source-exact-Llanowar-plus-synthetic-effect".into(),
        "native-control-test-only".into(),
        "synthetic-outside-registry7-ordinary".into(),
        vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let mut game = GameState::initialize_scheduled(
        GameId(5999),
        context,
        seed,
        TurnStart {
            number: 2,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    let submit = |s: &GameState, actor, command| {
        s.advance(GameInput {
            basis: s.basis().clone(),
            actor,
            command,
        })
        .unwrap()
    };
    let pass = |s: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
        else {
            panic!("ready native holder")
        };
        submit(s, holder, GameCommand::Pass)
    };
    for _ in 0..20 {
        if game.turn().unwrap().position().step == Step::PrecombatMain {
            break;
        }
        assert!(game.pending_decision().is_none());
        game = pass(&game).into_state();
    }
    let object = game
        .execution()
        .window()
        .cards()
        .current_identity(llanowar)
        .unwrap();
    let before = game
        .execution()
        .window()
        .cards()
        .get(object)
        .unwrap()
        .clone();
    assert_eq!(before.has_continuous_control_since_turn_start(), Some(true));
    game = submit(&game, A, GameCommand::Pass).into_state();
    let spell = game
        .execution()
        .window()
        .cards()
        .current_identity(synthetic)
        .unwrap();
    game = submit(
        &game,
        B,
        GameCommand::Cast {
            spell,
            target: TargetRef::Object(object),
        },
    )
    .into_state();
    for _ in 0..4 {
        if game.execution().window().entries().is_empty() {
            break;
        }
        game = pass(&game).into_state();
    }
    let changed = game.execution().window().cards().get(object).unwrap();
    assert_eq!(changed.id(), before.id());
    assert_eq!(changed.controller(), Some(B));
    assert_ne!(changed.control_epoch(), before.control_epoch());
    assert_eq!(
        changed.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let epoch = changed.control_epoch();
    let input = |s: &GameState, actor| GameInput {
        basis: s.basis().clone(),
        actor,
        command: GameCommand::ActivateMana {
            source: object,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    };
    let saved = game.clone();
    assert_eq!(
        game.advance(input(&game, A)),
        Err(GameError::Effect(EffectError::Window(
            StackWindowError::WrongController
        )))
    );
    assert_eq!(game, saved);
    game = submit(&game, A, GameCommand::Pass).into_state();
    let saved = game.clone();
    assert_eq!(
        game.advance(input(&game, B)),
        Err(GameError::Effect(EffectError::CreatureTapRestricted))
    );
    assert_eq!(game, saved);
    for _ in 0..100 {
        if game.turn().unwrap().position().number == 3
            && game.turn().unwrap().position().step == Step::PrecombatMain
            && game.pending_decision().is_none()
        {
            break;
        }
        if let Some(decision) = game.pending_decision() {
            let choice = match decision.frame() {
                RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                other => panic!("unexpected synthetic continuation frame: {other:?}"),
            };
            game = submit(
                &game,
                decision.chooser(),
                GameCommand::Reply {
                    decision: decision.id(),
                    choice,
                },
            )
            .into_state();
        } else {
            game = pass(&game).into_state();
        }
    }
    assert_eq!(game.turn().unwrap().position().number, 3);
    let actual = game.execution().window().cards().get(object).unwrap();
    assert_eq!(actual.id(), before.id());
    assert_eq!(actual.control_epoch(), epoch);
    assert_eq!(actual.has_continuous_control_since_turn_start(), Some(true));
    let activated = game.advance(input(&game, B)).unwrap();
    assert_eq!(
        activated.state().execution().mana(B, ManaType::Green),
        1.into()
    );
    assert_eq!(activated.mana_production().unwrap().source, object);
    receipt(
        "SPELLWAKE_MANDATORY_SYNTHETIC_RECEIPT",
        serde_json::json!({"kind":"actual-source-Llanowar-plus-synthetic-control-effect","selector":"mandatory_llanowar_control_change_is_a_separate_synthetic_mechanism","registry7_ordinary_credit":false,"forge_credit":false,"same_object":true,"fresh_restricted":true,"old_controller_rejected":true,"next_controller_turn_mana":{"Green":"1"}}),
    );
}
