use super::*;

fn selected(
    cards: Value,
    actions: Value,
    initial_battlefield_order: &[&str],
) -> packet::SelectedInput {
    let cards = cards.as_array().unwrap();
    let bindings=cards.iter().map(|card|json!({"name":format!("{}_seed_object",card["alias"].as_str().unwrap()),"join":{"card_id":card["card_id"]}})).collect::<Vec<_>>();
    let library = cards
        .iter()
        .filter(|card| card["zone"] == "Library" && card["owner"] == "A")
        .map(|card| card["alias"].clone())
        .collect::<Vec<_>>();
    packet::SelectedInput {
        fixture: json!({"scope":"trusted_executor_precheck","seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"game_id":1,"context":["unit","cr","cards","programs","private"],"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"TrustedUnitPrecheck"},"floating_mana":[],"cards":cards,"initial_world_cohorts":[],"initial_battlefield_order":initial_battlefield_order,"library_order":{"A":library,"B":[]}},"actions":actions,"bindings":bindings,"stop":{"after_action_ordinal":actions.as_array().unwrap().len(),"no_extra_pass":true}}),
        definitions: BTreeMap::from([(
            "P".into(),
            json!({"native_program":program::view(&crate::program::Program::creature(None,1.into(),1.into(), std::collections::BTreeSet::new()))}),
        )]),
        metadata: Value::Null,
        vocabulary: crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
    }
}

#[test]
fn compressed_huge_failed_draw_retains_actual_successful_prefix_and_stops_precheck() {
    let input = selected(
        json!([{"alias":"L","card_id":1,"owner":"A","controller":null,"zone":"Library","definition":"P"}]),
        json!([{"op":"trusted_call","owner":"EffectWindow","method":"enter_turn_step","active":"A","priority":false,"empty_mana":false},{"op":"trusted_call","owner":"EffectWindow","method":"draw_cards","player":"A","amount":"900719925474099312345678901234567890"}]),
        &[],
    );
    let (report, V2Witness::Executor(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    let EffectAttempt::Draw(result) = &witness.attempts[1].1 else {
        panic!()
    };
    let transition = result.as_ref().as_ref().unwrap();
    assert_eq!(transition.result().events.len(), 2);
    assert!(
        matches!(&transition.result().events[1],crate::draw::DrawEvent::EmptyLibrary {attempts,..} if *attempts==quantity(&json!("900719925474099312345678901234567889")).unwrap())
    );
    assert_eq!(
        transition
            .append()
            .records
            .iter()
            .filter(|record| matches!(record, RulesRecord::Event(_)))
            .count(),
        2
    );
    assert!(transition.state().state_based_losses().is_empty());
    assert_eq!(
        transition
            .state()
            .window()
            .cards()
            .iter()
            .filter(|card| card.zone() == ObjectZone::Hand(SeatId(0)))
            .count(),
        1
    );
    assert_eq!(transition.state().window().phase(), WindowPhase::TurnWork);
}

#[test]
fn late_primitive_batch_fault_cannot_leak_its_first_move_into_later_actual_sba() {
    let input = selected(
        json!([{"alias":"A","card_id":1,"owner":"A","controller":"A","zone":"Battlefield","definition":"P","tapped":false},{"alias":"B","card_id":2,"owner":"B","controller":"B","zone":"Battlefield","definition":"P","tapped":false}]),
        json!([{"op":"private_fault_clone","owner":"GameObjects","field":"next_identity","value":"18446744073709551614","bind":"fault"},{"op":"attempt_owner_call_without_adoption","snapshot":"fault","owner":"GameObjects","method":"apply_zone_moves","moves":[{"object":{"captured_object":"A_seed_object"},"destination":"Graveyard"},{"object":{"captured_object":"B_seed_object"},"destination":"Graveyard"}]},{"op":"private_prepare_fields","marked_damage":[{"object":{"captured_object":"A_seed_object"},"amount":"1"},{"object":{"captured_object":"B_seed_object"},"amount":"1"}]},{"op":"trusted_call","owner":"EffectWindow","method":"begin_checks"}]),
        &["A", "B"],
    );
    let (report, V2Witness::Executor(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    assert_eq!(
        report["records"][2]["result"]["error_path"],
        json!(["IdentityExhausted"])
    );
    let initial = witness.initialization.as_ref().as_ref().unwrap();
    let prepared = &witness.attempts[2].0;
    assert_eq!(initial.window().cards(), prepared.window().cards());
    let final_state = witness.final_state.unwrap();
    assert_eq!(final_state.window().cards().graveyard(SeatId(0)).len(), 1);
    assert_eq!(final_state.window().cards().graveyard(SeatId(1)).len(), 1);
    assert!(final_state.window().cards().next_identity_sequence() < u64::MAX);
}
