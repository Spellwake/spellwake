use super::*;
use crate::{
    mana::ManaCost,
    program::{Instruction, Program},
};

fn selected(
    cards: Vec<Value>,
    definitions: BTreeMap<String, Value>,
    initial: Value,
    actions: Value,
    mana: Value,
    participants: Value,
    active: &str,
) -> packet::SelectedInput {
    let mut libraries = serde_json::Map::new();
    for player in participants.as_array().unwrap() {
        libraries.insert(
            player.as_str().unwrap().into(),
            json!(
                cards
                    .iter()
                    .filter(|card| card["zone"] == "Library" && card["owner"] == *player)
                    .map(|card| card["alias"].clone())
                    .collect::<Vec<_>>()
            ),
        );
    }
    let bindings=cards.iter().map(|card|json!({"name":format!("{}_seed_object",card["alias"].as_str().unwrap()),"join":{"card_id":card["card_id"]}})).collect::<Vec<_>>();
    let life = participants
        .as_array()
        .unwrap()
        .iter()
        .map(|value| (value.as_str().unwrap().to_string(), json!("20")))
        .collect::<serde_json::Map<_, _>>();
    packet::SelectedInput {
        fixture: json!({"scope":"private-root-unit","seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"game_id":1,"context":["unit","cr","cards","programs","private"],"participants":participants,"active":active,"life":life,"combat_rules":"UnrestrictedFreeForAll","initial_boundary":initial,"floating_mana":mana,"cards":cards,"initial_world_cohorts":[],"library_order":libraries},"bindings":bindings,"actions":actions,"stop":{"after_action_ordinal":actions.as_array().unwrap().len(),"no_extra_pass":true}}),
        definitions,
        metadata: Value::Null,
        vocabulary: crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
    }
}

#[test]
fn actual_entry_joins_and_bounded_passes_reach_copy_pending_before_private_fault() {
    let definitions = BTreeMap::from([
        (
            "T".into(),
            json!({"native_program":program::view(&Program::creature(None,1.into(),100.into(), std::collections::BTreeSet::new()))}),
        ),
        (
            "O".into(),
            json!({"native_program":program::view(&Program::instant(ManaCost::free(),ExecutableBody::new(BodyTarget::SingleCreature,vec![Instruction::Damage {amount:1.into()}])))}),
        ),
        (
            "P".into(),
            json!({"native_program":program::view(&Program::instant(ManaCost::free(),ExecutableBody::new(BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),vec![Instruction::CopySpell])))}),
        ),
    ]);
    let cards = vec![
        json!({"alias":"T","card_id":1,"owner":"B","controller":"B","zone":"Battlefield","definition":"T","tapped":false}),
        json!({"alias":"O","card_id":2,"owner":"A","controller":null,"zone":"Hand","definition":"O"}),
        json!({"alias":"P","card_id":3,"owner":"B","controller":null,"zone":"Hand","definition":"P"}),
    ];
    let actions = json!([{"op":"cast","actor":"A","source":{"current_owned_action_alias":"O"},"target":{"captured_object":"T_seed_object"},"payment":[],"bind_entry":"O_entry"},{"op":"pass","actor":"A"},{"op":"cast","actor":"B","source":{"current_owned_action_alias":"P"},"target":{"entry_ref":"O_entry"},"payment":[],"bind_entry":"P_entry"},{"op":"repeat_bounded","count":1,"sequence":[{"op":"pass_round","actors":["B","A"]}]},{"op":"private_fault_clone","owner":"StackWindow","field":"next_entry","value":"18446744073709551615","bind":"fault"},{"op":"attempt_reply_without_adoption","snapshot":"fault","mutation":{"copy_choice":{"tag":"Retain"}}}]);
    let mut input = selected(
        cards,
        definitions,
        json!({"kind":"Interaction"}),
        actions,
        json!([]),
        json!(["A", "B"]),
        "A",
    );
    input.fixture["seed"]["initial_battlefield_order"] = json!(["T"]);
    let (
        report,
        Witness::Root {
            attempts,
            final_state,
            ..
        },
        extras,
    ) = run_extended(&input).unwrap()
    else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    assert!(extras.is_empty());
    let (source, _, result) = attempts.last().unwrap();
    assert!(matches!(
        source.pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    assert!(result.is_err());
    assert!(matches!(
        final_state.unwrap().pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
}

#[test]
fn actual_charged_bottom_cohort_keeps_staged_selection_through_invalid_and_late_fault() {
    let definitions = BTreeMap::from([(
        "L".into(),
        json!({"native_program":program::view(&Program::creature(None,1.into(),1.into(), std::collections::BTreeSet::new()))}),
    )]);
    let mut cards = Vec::new();
    for (seat, player) in [(0, "A"), (1, "B")] {
        for index in 1..=8 {
            cards.push(json!({"alias":format!("L_{player}_{index}"),"card_id":seat*100+index,"owner":player,"controller":null,"zone":"Library","definition":"L"}));
        }
    }
    let drawn = |player: &str| json!({"actual_draw_from_latest_opening_batch":{"player":player,"ordinal":7,"capture_from_actual_draw_journal_for_action_ordinal":2,"ordinal_basis":"one_based"}});
    let actions = json!([{"op":"reply_exact_current_frame","actor":"A","kind":"Mulligan","items":["TakeMulligan"]},{"op":"reply_exact_current_frame","actor":"B","kind":"Mulligan","items":["TakeMulligan"]},{"op":"reply_exact_current_frame","actor":"A","kind":"Bottom","items":[drawn("A")]},{"op":"save_snapshot","name":"staged"},{"op":"attempt_reply_without_adoption","snapshot":"staged","mutation":{"actor":"B","kind":"Bottom","items":[]}},{"op":"private_fault_clone","snapshot":"staged","owner":"GameObjects","field":"next_identity","value":"18446744073709551614","bind":"fault"},{"op":"attempt_reply_without_adoption","snapshot":"fault","mutation":{"actor":"B","kind":"Bottom","items":[drawn("B")]}},{"op":"reply_exact_current_frame","actor":"B","kind":"Bottom","items":[drawn("B")]},{"op":"reply_exact_current_frame","actor":"A","kind":"Mulligan","items":["Keep"]},{"op":"reply_exact_current_frame","actor":"B","kind":"Mulligan","items":["Keep"]}]);
    let mut input = selected(
        cards,
        definitions,
        json!({"kind":"Opening","hand_size":7,"entropy_words":[7,6,5,4,3,2,1,7,6,5,4,3,2,1,7,6,5,4,3,2,1,7,6,5,4,3,2,1]}),
        actions,
        json!([]),
        json!(["A", "B"]),
        "A",
    );
    input.fixture["seed"]["initial_battlefield_order"] = json!([]);
    let (
        report,
        Witness::Root {
            attempts,
            final_state,
            ..
        },
        _,
    ) = run_extended(&input).unwrap()
    else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let errors = attempts
        .iter()
        .filter(|(_, _, result)| result.is_err())
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 2);
    let mut exact_fault_clone = errors[0].0.clone();
    fault(
        &mut exact_fault_clone,
        "GameObjects",
        "next_identity",
        u64::MAX - 1,
    )
    .unwrap();
    assert_eq!(exact_fault_clone, errors[1].0);
    let final_state = final_state.unwrap();
    assert_eq!(final_state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(
        final_state
            .execution
            .window()
            .cards()
            .iter()
            .filter(|card| card.zone() == ObjectZone::Hand(SeatId(0)))
            .count(),
        6
    );
}

#[test]
fn prepared_calendar_clears_all_positive_players_in_old_active_order() {
    let mut input = selected(
        vec![],
        BTreeMap::new(),
        json!({"kind":"ScheduledPrivatePreparedResourceUnit","turn":1,"step":"Cleanup"}),
        json!([]),
        json!([{"player":"C","type":"Red","amount":"1"},{"player":"C","type":"Blue","amount":"2"},{"player":"B","type":"Green","amount":"3"},{"player":"B","type":"White","amount":"1"},{"player":"A","type":"Black","amount":"0"}]),
        json!(["B", "C", "A"]),
        "C",
    );
    input.fixture["seed"]["initial_battlefield_order"] = json!([]);
    let (
        report,
        Witness::Root {
            initialization,
            final_state,
            ..
        },
        _,
    ) = run_extended(&input).unwrap()
    else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let initial = initialization.unwrap();
    let values = initial
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .filter(|event| {
            matches!(
                event.cause,
                crate::occurrence::EventCause::ManaPoolsEmptied { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].occurrences.len(), 2);
    assert!(matches!(
        values[0].occurrences[0].value,
        crate::occurrence::RulesOccurrence::ManaPoolEmptied {
            player: SeatId(2),
            ..
        }
    ));
    assert!(matches!(
        values[0].occurrences[1].value,
        crate::occurrence::RulesOccurrence::ManaPoolEmptied {
            player: SeatId(1),
            ..
        }
    ));
    let final_state = final_state.unwrap();
    assert_eq!(final_state.execution.window().active_player(), SeatId(0));
    assert_eq!(final_state.turn().unwrap().position().number, 2);
}

#[test]
fn genuine_later_owner_order_keeps_earlier_placements_through_private_wrong_wave() {
    let mut program =
        Program::creature(None, 0.into(), 0.into(), std::collections::BTreeSet::new());
    program.triggered.push(crate::program::TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: crate::program::TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::dies(crate::program::ObjectFilter::self_object()),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(0.into()),
            }],
        ),
    });
    let definitions = BTreeMap::from([(
        "S".into(),
        json!({"native_program":program::view(&program)}),
    )]);
    let cards=(1..=4).map(|id|{let owner=if id<=2 {"A"}else{"B"};json!({"alias":format!("S{id}"),"card_id":id,"owner":owner,"controller":owner,"zone":"Battlefield","definition":"S","tapped":false})}).collect();
    let actions = json!([{"op":"reply_exact_current_frame","actor":"A","kind":"OrderGraveyard","items":[{"captured_object":"S2_seed_object"},{"captured_object":"S1_seed_object"}]},{"op":"reply_exact_current_frame","actor":"B","kind":"OrderGraveyard","items":[{"captured_object":"S3_seed_object"},{"captured_object":"S4_seed_object"}]},{"op":"reply_exact_current_frame","actor":"A","kind":"OrderTriggers","items":["S1_instance","S2_instance"]},{"op":"save_snapshot","name":"B_order"},{"op":"attempt_effect_frame_reply_without_adoption","snapshot":"B_order","mutation":{"wave":"actual_plus_one_checked"},"actor":"B","items":["S4_instance","S3_instance"]},{"op":"reply_exact_current_frame","actor":"B","kind":"OrderTriggers","items":["S4_instance","S3_instance"]}]);
    let mut input = selected(
        cards,
        definitions,
        json!({"kind":"Scheduled","turn":1,"step":"PrecombatMain"}),
        actions,
        json!([]),
        json!(["A", "B"]),
        "A",
    );
    input.fixture["seed"]["initial_battlefield_order"] = json!(["S1", "S2", "S3", "S4"]);
    for id in 1..=4 {
        input.fixture["bindings"].as_array_mut().unwrap().push(json!({"name":format!("S{id}_instance"),"join":{"source_object_ref":format!("S{id}_seed_object"),"triggered_ability_id":1,"related_object_ref":format!("S{id}_seed_object")}}));
    }
    let (report, Witness::Root { final_state, .. }, extras) = run_extended(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    assert_eq!(extras.len(), 1);
    assert_eq!(extras[0].source.execution.window().entries().len(), 2);
    assert_eq!(
        extras[0].result,
        Err(EffectError::Trigger(TriggerError::WrongOrderFrame))
    );
    assert_eq!(final_state.unwrap().execution.window().entries().len(), 4);
}
