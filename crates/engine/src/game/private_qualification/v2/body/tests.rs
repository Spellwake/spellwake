use super::*;
use crate::{mana::ManaCost, program::Program};

#[test]
fn private_body_token_timestamp_order_dispatch_preserves_pending_work_and_actual_birth_joins() {
    let actions = json!([
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"cast_on_turn","actor":"A","source":{"current_owned_action_alias":"P"},"target":null,"payment":[],"bind_entry":"producer","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"pass","actor":"A","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"pass","actor":"B","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_resolution","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"resume_rule","actor":"A","reply":{"kind":"TimestampOrder","items":[1,0]},"adopt":true}
    ]);
    let mut input = crate::game::private_qualification::token_batch_tests::fixture(
        "TrustedUnitPrecheck",
        actions,
    );
    input.fixture["id"] = json!("S2T-token-batch-private-regression");
    input.fixture["scope"] = json!("trusted_executor_precheck");
    for (index, action) in input.fixture["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        action["ordinal"] = json!(index + 1);
    }
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!("wrong actual body witness")
    };
    assert_eq!(report["status"], "completed_recipe", "{report}");
    let (pending, EffectAttempt::Progress(result), adopted) = witness.attempts.last().unwrap()
    else {
        panic!("missing actual resumed rule")
    };
    assert!(*adopted);
    assert_eq!(
        pending
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        0
    );
    let cursor = pending.resolution_state().unwrap();
    let candidate = result.as_ref().as_ref().unwrap();
    let births = crate::test_support::receipt_joins::token_creations(
        candidate.append(),
        cursor.entry(),
        cursor.instruction(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(births.len(), 2);
    let mut joins = Joins::default();
    joins.bind_entry("producer", cursor.entry()).unwrap();
    joins.observe(&input.fixture, candidate.append()).unwrap();
    assert_eq!(
        joins
            .object(
                &json!({"captured_object":"second"}),
                &input.fixture,
                candidate.state()
            )
            .unwrap(),
        births[1]
    );
    assert_eq!(
        witness
            .final_state
            .unwrap()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token())
            .count(),
        2
    );
    for bad_order in [json!(["1", "0"]), json!([1.5, 0]), json!([-1, 0])] {
        let mut bad = input.clone();
        bad.fixture["actions"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["reply"]["items"] = bad_order;
        let (report, V2Witness::Body(witness)) = run(&bad).unwrap() else {
            panic!()
        };
        assert_eq!(report["status"], "failed");
        assert!(
            report["failure"]
                .as_str()
                .unwrap()
                .contains("timestamp member")
        );
        let pending = witness.final_state.unwrap();
        assert_eq!(
            pending
                .window()
                .cards()
                .iter()
                .filter(|o| o.is_token())
                .count(),
            0
        );
        assert!(pending.resolution_state().is_some());
    }
}

fn selected(
    program: Program,
    actions: Value,
    artifact: Option<Program>,
    initial_battlefield_order: &[&str],
) -> packet::SelectedInput {
    let mut cards = vec![
        json!({"alias":"T","card_id":1,"owner":"B","controller":"B","zone":"Battlefield","definition":"T","tapped":false}),
        json!({"alias":"P","card_id":2,"owner":"A","controller":null,"zone":"Hand","definition":"P","tapped":null}),
        json!({"alias":"L","card_id":3,"owner":"A","controller":null,"zone":"Library","definition":"T"}),
    ];
    let mut definitions = BTreeMap::from([
        (
            "P".into(),
            json!({"native_program":program::view(&program)}),
        ),
        (
            "T".into(),
            json!({"native_program":program::view(&Program::creature(None,1.into(),100.into(), std::collections::BTreeSet::new()))}),
        ),
    ]);
    if let Some(artifact) = artifact {
        cards.push(json!({"alias":"S","card_id":4,"owner":"A","controller":"A","zone":"Battlefield","definition":"S","tapped":false}));
        definitions.insert(
            "S".into(),
            json!({"native_program":program::view(&artifact)}),
        );
    }
    let bindings=cards.iter().map(|card|json!({"name":format!("{}_seed_object",card["alias"].as_str().unwrap()),"join":{"card_id":card["card_id"]}})).collect::<Vec<_>>();
    packet::SelectedInput {
        fixture: json!({"scope":"trusted_executor_precheck","seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"game_id":1,"context":["unit","cr","cards","programs","private"],"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"TrustedUnitPrecheck"},"floating_mana":[],"cards":cards,"initial_world_cohorts":[],"initial_battlefield_order":initial_battlefield_order,"library_order":{"A":["L"],"B":[]}},"bindings":bindings,"actions":actions,"stop":{"after_action_ordinal":actions.as_array().unwrap().len(),"no_extra_pass":true}}),
        definitions,
        metadata: Value::Null,
        vocabulary: crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
    }
}

fn body_call() -> Value {
    json!({"op":"trusted_run_existing_body","source_alias":"P","actor":"A","target":{"captured_object":"T_seed_object"},"payment":[],"pass_order":["A","B"],"bind_entry":"P_entry","exact_instruction_reply_table":[],"stop_before_followon_checks":true,"return_at":"first_actual_Pending_or_Complete","no_unlisted_reply":true})
}

#[test]
fn disclosed_combat_preparation_uses_live_epoch_and_prunes_departed_membership() {
    let actions = json!([{"op":"private_prepare_fields","pt_counters":[{"object":{"captured_object":"T_seed_object"},"plus_one":"1","minus_one":"0"}],"marked_damage":[{"object":{"captured_object":"T_seed_object"},"amount":"1"}],"combat":{"attacker":{"captured_object":"T_seed_object"},"defender":"A","blocked":false,"control_epoch":"actual_current_epoch"}},body_call(),{"op":"trusted_call","owner":"EffectWindow","method":"prune_non_battlefield_state"}]);
    let input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand,
                    ),
                ))],
            ),
        ),
        actions,
        None,
        &["T"],
    );
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let state = witness.final_state.unwrap();
    assert_eq!(state.combat().unwrap().attackers().count(), 0);
    assert!(state.combat().unwrap().had_attackers());
    assert!(state.continuous_effects().next().is_none());
}

#[test]
fn private_body_grouped_births_bind_both_members_and_keep_complete_waiting_wire() {
    let actions = json!([
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"cast_on_turn","actor":"A","source":{"current_owned_action_alias":"P"},"target":null,"payment":[],"bind_entry":"producer","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"pass","actor":"A","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"pass","actor":"B","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"begin_resolution","adopt":true},
        {"op":"trusted_call","owner":"EffectWindow","method":"resume_rule","actor":"A","reply":{"kind":"TimestampOrder","items":[1,0]},"adopt":true}
    ]);
    let mut input = crate::game::private_qualification::token_batch_tests::grouped_fixture(
        "TrustedUnitPrecheck",
        actions,
    );
    input.fixture["id"] = json!("S2T-grouped-private-regression");
    input.fixture["scope"] = json!("trusted_executor_precheck");
    for (index, action) in input.fixture["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        action["ordinal"] = json!(index + 1);
    }
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!("body witness");
    };
    assert_eq!(report["status"], "completed_recipe", "{report}");
    let (pending, EffectAttempt::Progress(result), _) = witness.attempts.last().unwrap() else {
        panic!("actual resume");
    };
    let candidate = result.as_ref().as_ref().unwrap();
    let captures = candidate
        .append()
        .records
        .iter()
        .filter_map(|r| {
            if let crate::occurrence::RulesRecord::TriggerDiscovered(w) = r {
                Some(w.as_ref())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].causes.len(), 2);
    let wire = projection::waiting(captures[0]);
    assert_eq!(report["final_state"]["waiting"][0], wire);
    assert_eq!(
        projection::journal(candidate.append())
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["tag"] == "TriggerDiscovered")
            .unwrap()["value"],
        wire
    );
    let mut joins = Joins::seed(&input.fixture, pending).unwrap();
    joins
        .bind_entry("producer", pending.resolution_state().unwrap().entry())
        .unwrap();
    joins.observe(&input.fixture, candidate.append()).unwrap();
    joins.require_token_bindings(&input.fixture).unwrap();
}
