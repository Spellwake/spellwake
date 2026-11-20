use super::*;
use crate::{
    activated::{ActivatedAbility, SourceCost},
    mana::ManaCost,
    program::{Instruction, Program},
};

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
fn actual_mixed_body_draws_and_bounces_without_a_followon_check_or_fake_ready() {
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(1.into()),
                },
                Instruction::ReturnToOwnersHand,
                Instruction::Damage { amount: 1.into() },
            ],
        ),
    );
    let input = selected(program, json!([body_call()]), None, &["T"]);
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let state = witness.final_state.unwrap();
    assert!(matches!(
        state.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
    ));
    assert_eq!(
        state
            .window()
            .cards()
            .get(state.window().cards().current_identity(CardId(1)).unwrap())
            .unwrap()
            .zone(),
        ObjectZone::Hand(SeatId(1))
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(state.window().cards().current_identity(CardId(3)).unwrap())
            .unwrap()
            .zone(),
        ObjectZone::Hand(SeatId(0))
    );
    let (_, EffectAttempt::Progress(result), _) = witness.attempts.last().unwrap() else {
        panic!()
    };
    let RuleResult::Complete { draws, journal, .. } = result.as_ref().as_ref().unwrap().result()
    else {
        panic!()
    };
    assert_eq!(draws.events.len(), 1);
    assert!(journal.attempts.is_empty());
}

#[test]
fn effect_creation_receipts_bind_real_offered_application_and_wrong_chooser_is_nonadopted() {
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::CreatePreventionShield { amount: 3.into() },
                Instruction::CreateDamageMultiplier { factor: 2.into() },
                Instruction::Damage { amount: 2.into() },
            ],
        ),
    );
    let input = selected(
        program,
        json!([body_call(),{"op":"attempt_rule_reply_without_adoption","actor":"A","kind":"DamageOrder","application_ref":{"created_effect_at_instruction":1}},{"op":"reply_actual_damage_order","actor":"B","application_ref":{"created_effect_at_instruction":1}}]),
        None,
        &["T"],
    );
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let rejected = witness
        .attempts
        .iter()
        .find(|(_, _, adopt)| !*adopt)
        .unwrap();
    let EffectAttempt::Progress(result) = &rejected.1 else {
        panic!()
    };
    assert_eq!(result.as_ref(), &Err(EffectError::WrongChooser));
    let state = witness.final_state.unwrap();
    let target = state.window().cards().current_identity(CardId(1)).unwrap();
    assert_eq!(state.creature(target).unwrap().unwrap().damage, 1.into());
    assert!(state.pending_damage().is_none());
}

#[test]
fn private_controller_receipt_preserves_outer_revisions_and_entry_draw_controller() {
    let ability = ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                Instruction::Damage { amount: 1.into() },
                Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(1.into()),
                },
            ],
        ),
    };
    let actions = json!([{"op":"trusted_call","owner":"EffectWindow","method":"begin_checks","completion_required":"Complete"},{"op":"trusted_activate_ability","actor":"A","source":{"captured_object":"S_seed_object"},"ability_id":1,"target":{"captured_object":"T_seed_object"},"payment":[],"bind_entry":"ability_entry"},{"op":"trusted_registry_successor_inside_unit_only","method":"apply_controller","object":{"captured_object":"S_seed_object"},"controller":"B","retain_exact_incarnation":true,"adopt_into_same_private_executor":true},{"op":"trusted_resolve_current_entry_through_existing_owned_gate_and_driver","entry_ref":"ability_entry","pass_order":["A","B"]}]);
    let input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(0.into()),
                }],
            ),
        ),
        actions,
        Some(Program::artifact(None, vec![ability])),
        &["T", "S"],
    );
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let (source, EffectAttempt::Controller(result), _) = witness
        .attempts
        .iter()
        .find(|(_, value, _)| matches!(value, EffectAttempt::Controller(_)))
        .unwrap()
    else {
        panic!()
    };
    let (after, receipt) = result.as_ref().as_ref().unwrap();
    assert_eq!(source.revision(), after.revision());
    assert_eq!(source.window().revision(), after.window().revision());
    assert_eq!(
        source.window().cards().revision() + 1,
        after.window().cards().revision()
    );
    assert_eq!(receipt.before.as_ref().unwrap().id(), receipt.after.id());
    let (_, EffectAttempt::Progress(result), _) = witness.attempts.last().unwrap() else {
        panic!()
    };
    let RuleResult::Complete { draws, journal, .. } = result.as_ref().as_ref().unwrap().result()
    else {
        panic!()
    };
    assert!(matches!(
        draws.events[0],
        crate::draw::DrawEvent::Drawn {
            player: SeatId(0),
            ..
        }
    ));
    assert_eq!(journal.attempts[0].source.controller(), SeatId(1));
}

#[test]
fn actual_gate_then_private_cursor_runs_one_shared_batch_with_exact_physical_sources() {
    let actions = json!([{"op":"trusted_prepare_actual_resolving_damage_origin","source_alias":"P","target":{"captured_object":"T_seed_object"},"instruction_index":0,"bind_entry":"origin","actor":"A","payment":[],"pass_order":["A","B"],"target_assessment":"actual_legal_target_gate"},{"op":"trusted_call","owner":"EffectWindow","method":"begin_damage_batch","origin":{"Resolution":{"entry_ref":"origin","instruction":0}},"kind":"Noncombat","components":[{"source_object":{"captured_object":"S_seed_object"},"recipient_creature":{"captured_object":"T_seed_object"},"amount":"2"},{"source_object":{"captured_object":"T_seed_object"},"recipient_creature":{"captured_object":"T_seed_object"},"amount":"3"},{"source_object":{"captured_object":"S_seed_object"},"recipient_player":"B","amount":"4"}]}]);
    let input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
        actions,
        Some(Program::creature(
            None,
            1.into(),
            100.into(),
            std::collections::BTreeSet::new(),
        )),
        &["T", "S"],
    );
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let (source, EffectAttempt::ResolutionPreparation(result), _) = witness
        .attempts
        .iter()
        .find(|(_, value, _)| matches!(value, EffectAttempt::ResolutionPreparation(_)))
        .unwrap()
    else {
        panic!()
    };
    let (prepared, gate) = result.as_ref().as_ref().unwrap();
    assert_eq!(prepared.revision(), source.revision());
    assert_eq!(gate.revision(), source.window().revision() + 1);
    assert_eq!(
        gate.target_assessment().unwrap().permission(),
        crate::targets::ResolutionPermission::SomeTargetsLegal
    );
    let (_, EffectAttempt::Progress(result), _) = witness.attempts.last().unwrap() else {
        panic!()
    };
    let RuleResult::Complete { journal, .. } = result.as_ref().as_ref().unwrap().result() else {
        panic!()
    };
    assert_eq!(journal.batches.len(), 1);
    assert_eq!(journal.attempts.len(), 3);
    let state = witness.final_state.unwrap();
    assert_eq!(
        state
            .creature(state.window().cards().current_identity(CardId(1)).unwrap())
            .unwrap()
            .unwrap()
            .damage,
        5.into()
    );
    assert_eq!(state.life_total(SeatId(1)), Some(&16.into()));
}

#[test]
fn disclosed_combat_preparation_uses_live_epoch_and_prunes_departed_membership() {
    let actions = json!([{"op":"private_prepare_fields","pt_counters":[{"object":{"captured_object":"T_seed_object"},"plus_one":"1","minus_one":"0"}],"marked_damage":[{"object":{"captured_object":"T_seed_object"},"amount":"1"}],"combat":{"attacker":{"captured_object":"T_seed_object"},"defender":"A","blocked":false,"control_epoch":"actual_current_epoch"}},body_call(),{"op":"trusted_call","owner":"EffectWindow","method":"prune_non_battlefield_state"}]);
    let input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
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
fn observation_join_failure_keeps_actual_accepted_check_result_and_state() {
    let mut input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
        json!([body_call()]),
        None,
        &["T"],
    );
    input.fixture["bindings"].as_array_mut().unwrap().push(json!({"name":"invalid_observation_join","join":{"source_object_ref":"missing_object","triggered_ability_id":1}}));
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "failed");
    assert_eq!(witness.attempts.len(), 1);
    let (_, EffectAttempt::Progress(result), true) = &witness.attempts[0] else {
        panic!()
    };
    let result = result.as_ref().as_ref().unwrap();
    assert_eq!(witness.final_state.as_deref(), Some(result.state()));
    assert!(matches!(
        result.state().window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { .. })
    ));
    assert_eq!(report["records"][0]["result"]["tag"], "Ok");
}

#[test]
fn prepared_cleanup_defers_waiting_until_same_iteration_priority_without_entry_event() {
    let actions = json!([{"op":"private_prepare_fields","marked_damage":[{"object":{"captured_object":"T_seed_object"},"amount":"1"}]},{"op":"trusted_prepare_cleanup_context","turn":1,"active":"A","iteration":0},{"op":"trusted_call","owner":"EffectWindow","method":"begin_cleanup"},{"op":"attempt_rule_reply_without_adoption","actor":"A","kind":"Discard","items":[{"captured_object":"H8_seed_object"},{"captured_object":"H7_seed_object"}]},{"op":"reply_exact_current_frame","actor":"A","kind":"Discard","items":[{"captured_object":"H8_seed_object"}]},{"op":"trusted_complete_same_cleanup_priority_handoff","empty_stack_required":true},{"op":"trusted_call","owner":"EffectWindow","method":"begin_checks"}]);
    let mut input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: crate::program::InstructionAmount::Literal(0.into()),
                }],
            ),
        ),
        actions,
        None,
        &["T", "W"],
    );
    let cards = input.fixture["seed"]["cards"].as_array_mut().unwrap();
    cards.retain(|value| value["alias"] != "P");
    input.fixture["bindings"]
        .as_array_mut()
        .unwrap()
        .retain(|value| value["name"] != "P_seed_object");
    for index in 1..=8 {
        input.fixture["seed"]["cards"].as_array_mut().unwrap().push(json!({"alias":format!("H{index}"),"card_id":10+index,"owner":"A","controller":null,"zone":"Hand","definition":"T"}));
        input.fixture["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":format!("H{index}_seed_object"),"join":{"card_id":10+index}}));
    }
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(crate::program::TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: crate::program::TriggeredAbilityId(1),
        condition: crate::program::OrdinaryCondition::Discarded(
            crate::program::PlayerRelation::Controller,
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::InstructionAmount::Literal(1.into()),
            }],
        ),
    });
    input.definitions.insert(
        "W".into(),
        json!({"native_program":program::view(&watcher)}),
    );
    input.fixture["seed"]["cards"].as_array_mut().unwrap().push(json!({"alias":"W","card_id":20,"owner":"A","controller":"A","zone":"Battlefield","definition":"W","tapped":false}));
    input.fixture["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"W_seed_object","join":{"card_id":20}}));
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let state = witness.final_state.unwrap();
    assert_eq!(state.window().entries().len(), 1);
    assert_eq!(state.waiting_triggers().count(), 0);
    assert_eq!(state.window().cards().library(SeatId(0)).len(), 1);
    let calendar = witness.calendars.last().unwrap().1.as_ref().unwrap();
    assert_eq!(calendar.position().step, Step::Cleanup);
    assert_eq!(calendar.position().cleanup_iteration, 0);
    assert_eq!(calendar.stage(), TurnStage::Priority);
    assert!(witness.attempts.iter().filter_map(|(_,result,_)|result.append()).flat_map(|journal|&journal.records).all(|record|!matches!(record,RulesRecord::Event(event) if matches!(event.cause,crate::occurrence::EventCause::StepBegan {..}))));
    let rejected = witness
        .attempts
        .iter()
        .find(|(_, _, adopt)| !*adopt)
        .unwrap();
    let EffectAttempt::Progress(result) = &rejected.1 else {
        panic!()
    };
    assert!(result.is_err());
}

#[test]
fn independent_raw_branch_does_not_transplant_its_exile_into_consumed_executor() {
    let actions = json!([{"op":"trusted_interaction_inputs","sequence":[{"op":"cast","actor":"A","source":"O","target":{"captured_object":"T_seed_object"},"payment":[],"bind_entry":"O_entry"},{"op":"pass","actor":"A"},{"op":"cast","actor":"B","source":"R","target":{"captured_object":"T_seed_object"},"payment":[],"bind_entry":"R_entry"},{"op":"resolve_one","pass_order":["B","A"]},{"op":"resolve_one","pass_order":["A","B"]}],"stop_before_followon_checks":true,"exact_unlisted_instruction_reply_table":[]},{"op":"separate_stack_window_branch","source_snapshot":"fresh_initial_fixture_construction","scope":"independent_raw_window_branch_from_original_Hand_O_not_the_consumed_first_branch","sequence":[{"op":"record_card_spell","actor":"A","source_current_alias":"O","bind_entry":"own_entry"},{"op":"real_pass_and_target_gate","pass_order":["A","B"]},{"op":"record_resolution_move","card_ref":"own_entry.original_card_object","destination":"Exile"},{"op":"finish_resolution","entry_ref":"own_entry","exit":null}]}]);
    let mut input = selected(
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
        actions,
        None,
        &["T"],
    );
    input.fixture["seed"]["cards"][1]["alias"] = json!("O");
    input.fixture["bindings"][1]["name"] = json!("O_seed_object");
    input.definitions.insert("R".into(),json!({"native_program":program::view(&Program::instant(ManaCost::free(),ExecutableBody::new(BodyTarget::SingleCreature,vec![Instruction::ReturnToOwnersHand])))}));
    input.fixture["seed"]["cards"].as_array_mut().unwrap().push(json!({"alias":"R","card_id":5,"owner":"B","controller":null,"zone":"Hand","definition":"R"}));
    input.fixture["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"R_seed_object","join":{"card_id":5}}));
    let (report, V2Witness::Body(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    assert_eq!(witness.raw_branches.len(), 1);
    let state = witness.final_state.unwrap();
    let original = state.window().cards().current_identity(CardId(2)).unwrap();
    assert_eq!(
        state.window().cards().get(original).unwrap().zone(),
        ObjectZone::Graveyard(SeatId(0))
    );
    let raw = &report["records"][report["records"].as_array().unwrap().len() - 1]["independent_raw_branch"];
    assert_eq!(raw["status"], "completed_recipe");
    assert_eq!(raw["authored_body_execution"], false);
    assert!(
        raw["final_window"]["cards"]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["physical_card"] == 2 && value["zone"]["kind"] == "exile")
    );
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
