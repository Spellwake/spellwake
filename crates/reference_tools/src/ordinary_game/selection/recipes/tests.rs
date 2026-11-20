use super::*;
use crate::ordinary_game::{
    fixture::{ObservationScope, ReportEpoch},
    policy::Policy,
    record, view,
};
use serde_json::{Value, json};
use spellwake_engine::{
    game::GameState,
    mana::ManaType,
    object_identity::{ObjectDestination, ObjectZone},
};
use std::{path::PathBuf, sync::OnceLock};
fn raw() -> Value {
    packet::unique_json(INPUTS).unwrap()
}
fn parsed(value: Value) -> Result<SelectionInputs, String> {
    parse(&serde_json::to_vec(&value).unwrap())
}
#[test]
fn closed_twenty_identity_and_shared_generic_grammar() {
    let inputs = parse(INPUTS).unwrap();
    assert_eq!(inputs.cases.len(), 20);
    assert_eq!(
        inputs
            .cases
            .iter()
            .map(|c| c.decision_plans.len())
            .sum::<usize>(),
        33
    );
    for (i, case) in SelectionCase::ALL.into_iter().enumerate() {
        assert_eq!(SelectionCase::parse(case.as_str()).unwrap(), case);
        assert_eq!(case.game_id(), 72001 + u128::try_from(i).unwrap());
        let contract = CaseId::Selection(case).observation_contract();
        assert_eq!(
            contract.observation_scope,
            ObservationScope::ResolutionSelection
        );
        assert_eq!(contract.report_epoch, ReportEpoch::Report5);
    }
    for invalid in ["SD17", "SD20", "sd01", " SD01", "SD01 "] {
        assert!(SelectionCase::parse(invalid).is_err());
    }
    assert!(serde_json::from_value::<Case>(raw()["cases"][0].clone()).is_err());
    assert!(
        serde_json::from_value::<DecisionPlan>(raw()["cases"][0]["decision_plans"][0].clone())
            .is_err()
    );
    assert!(
        serde_json::from_value::<Instruction>(raw()["cases"][0]["instructions"][0].clone())
            .is_err()
    );
    let mut legacy_plan = raw();
    legacy_plan["cases"][0]["decision_plans"][0] = json!({"kind":"optional","id":"x","entry":{"kind":"cast_entry","operation":"cast.primary"},"instruction":0,"chooser":"P0","options":["take","decline"],"choice":"take"});
    assert!(parsed(legacy_plan).is_err());
    let old: Inputs =
        serde_json::from_slice(include_bytes!("../../mandatory-inputs-v1.json")).unwrap();
    assert_eq!(old.cases.len(), 23);
    assert!(
        old.cases
            .iter()
            .all(|c| c.decision_plans.is_empty() && c.reached_probe_limit.is_none())
    );
}
#[test]
fn explicit_plan_presence_and_positive_cap_required_even_for_zero_frame_cases() {
    for index in [0, 1, 4, 12] {
        for field in ["decision_plans", "reached_probe_limit"] {
            let mut v = raw();
            v["cases"][index].as_object_mut().unwrap().remove(field);
            assert!(parsed(v).unwrap_err().contains("explicit"));
            let mut v = raw();
            v["cases"][index][field] = Value::Null;
            assert!(parsed(v).is_err());
        }
        for cap in [json!(0), json!(-1), json!("64"), json!(true)] {
            let mut v = raw();
            v["cases"][index]["reached_probe_limit"] = cap;
            assert!(parsed(v).is_err());
        }
    }
}
#[test]
fn duplicate_boundary_and_closed_headers_reject_before_typed_decode() {
    let s = String::from_utf8(INPUTS.to_vec()).unwrap();
    for duplicate in [
        s.replacen(
            "\"schema_version\": 1",
            "\"schema_version\": 1,\"schema_version\": 1",
            1,
        ),
        s.replacen(
            "\"reached_probe_limit\": 64",
            "\"reached_probe_limit\": 64,\"reached_probe_limit\": 64",
            1,
        ),
        s.replacen(
            "\"alias\": \"CASE-SPELL\"",
            "\"alias\": \"CASE-SPELL\",\"alias\": \"CASE-SPELL\"",
            1,
        ),
    ] {
        assert!(
            parse(duplicate.as_bytes())
                .unwrap_err()
                .contains("inputs JSON")
        );
    }
    for (pointer, value) in [
        ("/profile", json!("resolution-selection-natural-mana-v1")),
        ("/epochs/registry", json!(10)),
        ("/epochs/report", json!(4)),
        ("/seats/0/native_seat", json!(3)),
        ("/cases/0/id", json!("SD17")),
        ("/cases/0/extra", json!(true)),
        ("/cases/0/initialization/kind", json!("scheduled")),
        ("/cases/0/initialization/zones/0/role", json!("P2")),
        ("/cases/0/decision_plans/0/kind", json!("optional")),
        ("/cases/0/decision_plans/0/required_count", json!("01")),
    ] {
        let mut v = raw();
        if pointer.ends_with("/extra") {
            v["cases"][0]["extra"] = value;
        } else {
            *v.pointer_mut(pointer).unwrap() = value;
        }
        assert!(parsed(v).is_err(), "{pointer}");
    }
}
#[test]
fn references_plan_counts_and_nonempty_probe_authority_reject() {
    for (pointer, value) in [
        ("/cases/0/inventory/0/alias", json!(" ")),
        ("/cases/0/instructions/0/source/alias", json!("foreign")),
        (
            "/cases/0/causal_operations/0/entry/operation",
            json!("unknown"),
        ),
        (
            "/cases/0/decision_plans/0/entry/operation",
            json!("unknown"),
        ),
        ("/cases/0/decision_plans/0/required_count", json!("0")),
        ("/cases/0/decision_plans/0/required_count", json!("-1")),
        ("/cases/0/decision_plans/0/selected", json!([])),
        ("/cases/0/decision_plans/0/domain", json!([])),
    ] {
        let mut v = raw();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(parsed(v).is_err(), "{pointer}");
    }
    let mut v = raw();
    v["cases"][0]["decision_plans"][0]["probes"] = json!([{"kind":"execute","obligation":{"kind":"supplemental","id":"selection_wrong_reply_kind"},"basis":{"kind":"current"},"actor":"P0","attempt":{"kind":"reply","decision":{"kind":"current"},"choice":{"kind":"select_resolution","objects":[]}},"rejection":"wrong_decision_kind"}]);
    assert!(
        parsed(v)
            .unwrap_err()
            .contains("compiled probe template mismatch")
    );
}
#[test]
fn meaningful_probe_templates_are_strict_and_preserve_deliberately_invalid_vectors() {
    let valid = json!({"kind":"execute","obligation":{"kind":"original","id":"SD04-V3"},"basis":{"kind":"initializer"},"actor":"P0","attempt":{"kind":"reply","decision":{"kind":"current"},"choice":{"kind":"select_resolution","objects":[{"kind":"physical_current","alias":"x"},{"kind":"physical_current","alias":"x"}]}},"rejection":"duplicate_selection"});
    let decoded: SelectionProbeDeclaration = serde_json::from_value(valid.clone()).unwrap();
    let SelectionProbeDeclaration::Execute { attempt, .. } = decoded else {
        panic!("expected typed execute declaration");
    };
    let SelectionProbeAttempt::Reply {
        choice: SelectionProbeReply::SelectResolution { objects },
        ..
    } = *attempt
    else {
        panic!("expected typed attempted reply");
    };
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0], objects[1]);
    for pointer in ["/basis", "/actor", "/attempt", "/obligation", "/rejection"] {
        let mut v = valid.clone();
        v.as_object_mut().unwrap().remove(&pointer[1..]);
        assert!(serde_json::from_value::<SelectionProbeDeclaration>(v).is_err());
    }
    for (pointer, value) in [
        ("/rejection", json!("anything")),
        ("/obligation/id", json!("SD04-V15")),
        ("/basis", json!({"kind":"literal","id":1})),
        ("/attempt/decision", json!({"kind":"current","id":1})),
        (
            "/attempt/choice",
            json!({"kind":"select_resolution","objects":[],"fix":true}),
        ),
    ] {
        let mut v = valid.clone();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(serde_json::from_value::<SelectionProbeDeclaration>(v).is_err());
    }
    let pending = json!({"kind":"pending_exact_original_history","obligation":{"kind":"original","id":"SD04-V13"},"reason":"original_prefix_has_no_consumed_decision"});
    assert!(serde_json::from_value::<SelectionProbeDeclaration>(pending.clone()).is_ok());
    let mut v = pending;
    v["attempt"] = json!({});
    assert!(serde_json::from_value::<SelectionProbeDeclaration>(v).is_err());
}
fn required(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine source input required: {key}"))
}
fn genuine() -> &'static SelectionSource {
    static SOURCE: OnceLock<SelectionSource> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let vocabulary = crate::creature_vocabulary::load_vocabulary(&required(
            "SPELLWAKE_CREATURE_VOCABULARY_INPUT",
        ))
        .unwrap();
        SelectionSource::load(
            &required("SPELLWAKE_CATALOGUE_METADATA"),
            &required("SPELLWAKE_CATALOGUE_INPUT"),
            vocabulary,
        )
        .unwrap()
    })
}
fn initialize(fixture: &Fixture) -> spellwake_engine::game::NativeCandidate {
    let FixtureInitialization::Scheduled(start) = fixture.initialization else {
        panic!("selection must be scheduled")
    };
    GameState::initialize_scheduled(
        fixture.game,
        fixture.context.clone(),
        fixture.setup.seed.clone(),
        start,
    )
    .unwrap()
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary; initializer only"]
fn all_twenty_public_fixtures_preserve_exact_partitions_resources_and_actual_initialization() {
    let source = genuine();
    let inputs = inputs(source.definitions()).unwrap();
    let mut observations = vec![];
    for recipe in &inputs.cases {
        let fixture = source
            .fixture(recipe.id, source.vocabulary().clone())
            .unwrap();
        assert_eq!(fixture.aliases.len(), recipe.inventory.len());
        assert_eq!(fixture.setup.seed.cards.len(), recipe.inventory.len());
        assert_eq!(fixture.game, CaseId::Selection(recipe.id).game_id());
        assert!(fixture.shuffles.is_empty());
        assert_eq!(fixture.setup.entropy.remaining(), 0);
        assert_eq!(fixture.setup.seed.seats, vec![A, B]);
        for (i, row) in recipe.inventory.iter().enumerate() {
            let alias = fixture
                .alias(CardId(u64::try_from(i).unwrap() + 1))
                .unwrap();
            assert_eq!(
                (&alias.alias, alias.owner, &alias.definition_ref),
                (&row.alias, row.owner.seat(), &row.oracle_id)
            );
        }
        let candidate = initialize(&fixture);
        let state = candidate.state();
        assert_eq!(state.basis().game, fixture.game);
        assert_eq!(state.basis().context, fixture.context);
        assert_eq!(state.basis().revision, 0);
        assert_eq!(state.basis().epoch, 0);
        assert!(state.opening().is_none());
        assert!(state.outcome().is_none());
        let FixtureInitialization::Scheduled(start) = fixture.initialization else {
            unreachable!()
        };
        let position = state.turn().unwrap().position();
        assert_eq!(position.number, start.number);
        assert_eq!(position.step, start.step);
        assert_eq!(
            state.execution().window().turn_owner(),
            fixture.setup.seed.active
        );
        for life in &fixture.setup.seed.life {
            assert_eq!(state.execution().life_total(life.player), Some(&life.total));
        }

        let cards = state.execution().window().cards();
        assert_eq!(cards.iter().count(), fixture.aliases.len());
        for seed in &fixture.setup.seed.cards {
            let object = cards
                .get(cards.current_identity(seed.card).unwrap())
                .unwrap();
            assert_eq!(object.owner(), seed.owner);
            assert_eq!(
                state.execution().base_program(object.id()),
                Some(&seed.program)
            );
            let zone = match seed.destination {
                ObjectDestination::LibraryTop | ObjectDestination::LibraryBottom => {
                    ObjectZone::Library(seed.owner)
                }
                ObjectDestination::Hand => ObjectZone::Hand(seed.owner),
                ObjectDestination::Graveyard => ObjectZone::Graveyard(seed.owner),
                ObjectDestination::Battlefield { controller } => {
                    assert_eq!(object.controller(), Some(controller));
                    ObjectZone::Battlefield
                }
                ObjectDestination::Exile => ObjectZone::Exile,
                _ => panic!("unsupported selected seed"),
            };
            assert_eq!(object.zone(), zone);
        }
        let super::super::prepared::SelectionInitialization::SelectionPrepared {
            zones,
            floating_mana,
            battlefield_oldest_first,
            ..
        } = &recipe.initialization;
        for zone in zones {
            let aliases = |ids: &[spellwake_engine::object_identity::ObjectId]| {
                ids.iter()
                    .map(|id| {
                        fixture
                            .alias(cards.get(*id).unwrap().card_id().unwrap())
                            .unwrap()
                            .alias
                            .clone()
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                aliases(cards.library(zone.role.seat())),
                zone.library_top_first
            );
            assert_eq!(
                aliases(cards.graveyard(zone.role.seat())),
                zone.graveyard_top_first
            );
        }
        assert_eq!(
            fixture
                .setup
                .seed
                .initial_battlefield_order
                .iter()
                .map(|id| fixture.alias(*id).unwrap().alias.clone())
                .collect::<Vec<_>>(),
            battlefield_oldest_first
                .iter()
                .map(|r| r.alias.clone())
                .collect::<Vec<_>>()
        );
        for seat in [A, B] {
            for mana in ManaType::ALL {
                let amount = floating_mana
                    .iter()
                    .filter(|r| r.player.seat() == seat && r.mana_type.native() == mana)
                    .map(|r| r.amount.clone())
                    .next()
                    .unwrap_or_else(|| 0.into());
                assert_eq!(state.execution().mana(seat, mana), amount);
            }
        }
        observations.push(json!({"case":recipe.id.as_str(),"game_id":fixture.game.0.to_string(),"seed":crate::projection::game_seed_input(&fixture.setup.seed),"actual_basis":format!("{:?}",state.basis()),"actual_card_count":cards.iter().count(),"native_initialization":"accepted","scope":"prepared initializer only; no gameplay or Report5"}));
    }
    assert_eq!(observations.len(), 20);
    assert_eq!(source.registry().coverage().required_partial_bindings, 9);
    assert_eq!(source.registry().coverage().fully_qualified_cards, 0);
    if let Some(path) = std::env::var_os("SPELLWAKE_SELECTION_FIXTURE_OBSERVATIONS_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
    }
}
#[test]
#[ignore = "requires genuine source; rejected factory attempts leave source and later fixture unchanged"]
fn source_fixture_rejections_are_atomic_without_alias_slug_assumptions() {
    let source = genuine();
    let recipe = inputs(source.definitions()).unwrap().cases.remove(0);
    let baseline = source
        .fixture(recipe.id, source.vocabulary().clone())
        .unwrap();
    for mutation in 0..11 {
        let mut bad = recipe.clone();
        match mutation {
            0 => bad.inventory[1].alias = bad.inventory[0].alias.clone(),
            1 => bad.inventory[0].oracle_id = "00000000-0000-0000-0000-000000000000".into(),
            2 => bad.inventory[0].copy = 0,
            3 => bad.inventory[1].copy = 99,
            4 => bad.reached_probe_limit = None,
            5 => bad.reached_probe_limit = Some(0),
            6 => {
                let super::super::prepared::SelectionInitialization::SelectionPrepared {
                    turn, ..
                } = &mut bad.initialization;
                *turn = 0;
            }
            7 => {
                let super::super::prepared::SelectionInitialization::SelectionPrepared {
                    zones,
                    ..
                } = &mut bad.initialization;
                zones[0].hand.clear();
            }
            8 => {
                let super::super::prepared::SelectionInitialization::SelectionPrepared {
                    floating_mana,
                    ..
                } = &mut bad.initialization;
                floating_mana[0].amount = (-1).into();
            }
            9 => {
                let SelectionDecisionPlan::ResolutionSelect { instruction, .. } =
                    &mut bad.decision_plans[0]
                else {
                    unreachable!()
                };
                *instruction = u64::MAX;
            }
            10 => {
                let SelectionDecisionPlan::ResolutionSelect { operation, .. } =
                    &mut bad.decision_plans[0]
                else {
                    unreachable!()
                };
                *operation = SelectionOperation::DiscardCards;
            }
            _ => unreachable!(),
        };
        assert!(
            source
                .fixture_from_recipe(&bad, source.vocabulary().clone())
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(
            source
                .fixture(recipe.id, source.vocabulary().clone())
                .unwrap(),
            baseline
        );
    }
    let foreign = Arc::new(
        CreatureTypeVocabulary::new("negative-vocabulary".into(), [0; 32], vec!["Cat".into()])
            .unwrap(),
    );
    assert!(source.fixture(recipe.id, foreign).is_err());
    assert_eq!(
        source
            .fixture(recipe.id, source.vocabulary().clone())
            .unwrap(),
        baseline
    );
}
#[test]
#[ignore = "requires genuine fixture; only old-route refusal/capture instrumentation"]
fn selected_scope_is_refused_before_old_recorder_capture_and_views() {
    let source = genuine();
    let fixture = source
        .fixture(SelectionCase::Sd02, source.vocabulary().clone())
        .unwrap();
    let unbound = Policy::new(&fixture).unwrap();
    assert!(record::static_input_bytes_for_test(&fixture).is_err());
    let candidate = initialize(&fixture);
    assert!(
        unbound
            .prepare(candidate.state())
            .unwrap_err()
            .contains("initializer authority")
    );
    let history = view::History::default();
    assert!(view::world(&fixture, &history, candidate.state()).is_err());
    assert!(view::header(&fixture, &history, candidate.state()).is_err());
    assert!(view::facts(&fixture, &history, None, &candidate).is_err());
    crate::current_capture::with_selection_observer_for_test(source.vocabulary(), || {
        let before = crate::current_capture::caller_delivery_for_test();
        for report in [
            record::run_fixture_at(fixture.clone(), 0),
            record::run_case(
                CaseId::Selection(SelectionCase::Sd02),
                source.vocabulary().clone(),
            ),
        ] {
            assert!(!report.completed());
            assert_eq!(report.accepted_inputs(), 0);
            let value = serde_json::to_value(report).unwrap();
            assert_eq!(value["schema_version"], 5);
            assert!(value["failure"].as_str().unwrap().contains("unsupported"));
            for key in ["static_inputs", "initialization", "final", "rerun"] {
                assert!(value[key].is_null());
            }
            for key in ["accepted", "observations", "rejection_probes"] {
                assert_eq!(value[key], json!([]));
            }
        }
        assert_eq!(crate::current_capture::caller_delivery_for_test(), before);
    });
}
