//! Self-contained owner-path controls, independent of local frozen packets.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

#[test]
fn matcher_requires_both_current_named_and_numeric_epochs() {
    for named in 1..=9 {
        for numeric in 1..=9 {
            if named == 9 && numeric == 9 {
                continue;
            }
            let old = json!({"schema":format!("spellwake.ordinary-trigger-matcher-inputs.v{named}"),"schema_version":numeric});
            assert_eq!(
                preflight(&old, &BTreeMap::new()),
                Err("wrong matcher matrix schema".into())
            );
            assert!(
                matches!(run(&old,"unit",&BTreeMap::new()),Err(error) if error == "unsupported matcher matrix schema")
            );
        }
    }
}

fn matrix(
    condition: Value,
    source_zone: &str,
    source_controller: Value,
    initial_battlefield_order: &[&str],
    active: &str,
    operation: Value,
) -> Value {
    json!({"schema":"spellwake.ordinary-trigger-matcher-inputs.v9","schema_version":9,"creature_vocabulary":projection::vocabulary(&crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr")),"shared_seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"life":{"A":"20","B":"20"},"floating_mana":[],"combat_rules":"UnrestrictedFreeForAll","libraries":{"A":[],"B":[]},"initial_stack":[]},"cases":[{"id":"unit","scope":"trusted native actual-mutation/common-publisher unit; no root command/reference trajectory","participants":["A","B"],"active":active,"cards":[{"alias":"S_A","card_id":101,"owner":"B","controller":source_controller,"zone":source_zone,"kind":"Creature","names":[],"supertypes":[],"creature_subtypes":[],"power":"1","toughness":"1","trigger_id":1,"grouping":"EachOccurrence","body":{"target":{"tag":"None"},"instructions":[{"op":"DrawController","amount":{"tag":"Literal","value":"1"}}]},"condition":condition}],"initial_world_cohorts":[],"initial_battlefield_order":initial_battlefield_order,"capture_seed_handles":true,"operation":operation,"stop":"after accepted single publisher transition before checks/placement/body execution"}]})
}

#[test]
fn entry_receipt_binds_the_new_identity_and_actual_controller_without_checks() {
    let input = matrix(
        json!({"tag":"Enters","object":"SelfObject"}),
        "Hand",
        Value::Null,
        &[],
        "A",
        json!({"method":"actual_canonical_zone_move_then_common_owning_publisher","object_alias":"S_A","from":"Hand","to":"Battlefield","new_controller":"A"}),
    );
    let (report, V2Witness::Matcher(witness)) = run(&input, "unit", &BTreeMap::new()).unwrap()
    else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let MatcherWitness::Moves { source, result } = *witness else {
        panic!()
    };
    let transition = result.unwrap();
    let movement = &transition.result()[0];
    assert_eq!(
        source.window().cards().current_identity(CardId(101)),
        Some(movement.before.id())
    );
    assert_ne!(movement.before.id(), movement.after.id());
    let instances = transition.state().waiting_triggers().collect::<Vec<_>>();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].controller, SeatId(0));
    assert_eq!(instances[0].source.object.id(), movement.after.id());
    assert!(transition.state().window().entries().is_empty());
}

#[test]
fn actual_discard_preserves_nonactive_player_and_uses_rule_reason() {
    let input = matrix(
        json!({"tag":"Discarded","player":"Opponent"}),
        "Battlefield",
        json!("A"),
        &["S_A"],
        "A",
        json!({"method":"actual_hand_to_Graveyard_discard_then_owning_publisher","discarding_player":"B","discarded_card":{"alias":"H_B","card_id":202,"owner":"B","zone":"Hand","definition":"creature_1_1"}}),
    );
    let definition = crate::program::Program::creature(
        None,
        1.into(),
        1.into(),
        std::collections::BTreeSet::new(),
    );
    let definitions = BTreeMap::from([(
        "creature_1_1".into(),
        json!({"native_program":program::view(&definition)}),
    )]);
    let (_, V2Witness::Matcher(witness)) = run(&input, "unit", &definitions).unwrap() else {
        panic!()
    };
    let MatcherWitness::Moves { source, result } = *witness else {
        panic!()
    };
    assert_eq!(source.window().active_player(), SeatId(0));
    let transition = result.unwrap();
    let event = transition
        .append()
        .records
        .iter()
        .find_map(|r| {
            if let RulesRecord::Event(e) = r {
                Some(e)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(event.cause, EventCause::MechanismUnit);
    assert!(
        matches!(&event.occurrences[0].value,RulesOccurrence::ZoneChange(change) if change.reason==crate::occurrence::ZoneReason::Discard {player:SeatId(1)})
    );
    assert_eq!(transition.state().waiting_triggers().count(), 1);
}

#[test]
fn step_preparation_has_no_retroactive_entry_and_genuine_upkeep_uses_active_b() {
    let input = matrix(
        json!({"tag":"StepBegan","step":"Upkeep","player":"Opponent"}),
        "Battlefield",
        json!("A"),
        &["S_A"],
        "B",
        json!({"method":"actual_turn_scheduler_Untap_to_Upkeep_then_entry_publisher","active":"B","turn":1}),
    );
    let (report, V2Witness::Matcher(witness)) = run(&input, "unit", &BTreeMap::new()).unwrap()
    else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let MatcherWitness::Step {
        source,
        stages,
        final_state,
    } = *witness
    else {
        panic!()
    };
    assert_eq!(source.window().active_player(), SeatId(1));
    let transitions = stages
        .iter()
        .filter_map(|stage| {
            if let step::StepResult::Transition(value) = stage {
                Some(value.as_ref().as_ref().unwrap())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(transitions.len(), 3);
    assert!(transitions[0].append().records.is_empty());
    assert!(transitions[1].append().records.is_empty());
    let events = transitions[2]
        .append()
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::Event(e) = r {
                Some(e)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0].occurrences[0].value,
        RulesOccurrence::StepBegan {
            active: SeatId(1),
            position: crate::turn::TurnPosition {
                step: crate::turn::Step::Upkeep,
                ..
            }
        }
    ));
    assert_eq!(final_state.waiting_triggers().count(), 1);
    assert!(final_state.window().entries().is_empty());
}

#[test]
fn grouping_component_is_required_with_a_trigger_and_old_current_epoch_rejects() {
    let input = matrix(
        json!({"tag":"Enters","object":"SelfObject"}),
        "Hand",
        Value::Null,
        &[],
        "A",
        json!({"method":"actual_canonical_zone_move_then_common_owning_publisher","object_alias":"S_A","from":"Hand","to":"Battlefield","new_controller":"A"}),
    );
    let mut empty = input.clone();
    for field in ["trigger_id", "condition", "grouping", "body"] {
        empty["cases"][0]["cards"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
    }
    let (_, V2Witness::Matcher(witness)) = run(&empty, "unit", &BTreeMap::new()).unwrap() else {
        panic!("matcher");
    };
    let MatcherWitness::Moves { result, .. } = *witness else {
        panic!("moves");
    };
    assert_eq!(result.unwrap().state().waiting_triggers().count(), 0);
    let mut missing = input.clone();
    missing["cases"][0]["cards"][0]
        .as_object_mut()
        .unwrap()
        .remove("grouping");
    assert!(run(&missing, "unit", &BTreeMap::new()).is_err());
    let mut wrong = input.clone();
    wrong["cases"][0]["cards"][0]["grouping"] = json!("OncePerTurn");
    assert!(run(&wrong, "unit", &BTreeMap::new()).is_err());
    let mut old = input;
    old["schema"] = json!("spellwake.ordinary-trigger-matcher-inputs.v5");
    old["schema_version"] = json!(5);
    assert!(run(&old, "unit", &BTreeMap::new()).is_err());
}
