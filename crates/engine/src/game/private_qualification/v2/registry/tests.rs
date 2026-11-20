use super::*;

fn selected(
    cards: Value,
    bindings: Value,
    actions: Value,
    definition: crate::program::Program,
    initial_battlefield_order: &[&str],
) -> packet::SelectedInput {
    packet::SelectedInput {
        fixture: json!({"scope":"trusted_registry","seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"game_id":1,"context":["unit","cr","cards","programs","private"],"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"TrustedUnitPrecheck"},"floating_mana":[],"initial_world_cohorts":[],"initial_battlefield_order":initial_battlefield_order,"library_order":{"A":[],"B":[]},"cards":cards},"actions":actions,"bindings":bindings,"stop":{"kind":"AfterTrustedCall","after_action_ordinal":actions.as_array().unwrap().len(),"no_extra_pass":true}}),
        definitions: BTreeMap::from([(
            "P".into(),
            json!({"native_program":program::view(&definition)}),
        )]),
        metadata: Value::Null,
        vocabulary: crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
    }
}

#[test]
fn primitive_move_then_stale_probe_retains_real_incarnation_and_no_publisher() {
    let input = selected(
        json!([{"alias":"H","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"P","tapped":null}]),
        json!([{"name":"old","join":{"card_id":1}}]),
        json!([{"op":"trusted_call","owner":"GameObjects","method":"apply_zone_move","card":{"captured_object":"old"},"destination":"Graveyard","semantic_cause":null},{"op":"trusted_call","owner":"GameObjects","method":"apply_zone_move","card":{"captured_object":"old"},"destination":"Hand","adopt":false}]),
        crate::program::Program::creature(
            None,
            1.into(),
            1.into(),
            std::collections::BTreeSet::new(),
        ),
        &[],
    );
    let (report, V2Witness::Registry(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    assert_eq!(
        report["records"][1]["result"]["error_path"],
        json!(["UnknownObject"])
    );
    let seeded = witness.initialization.unwrap();
    assert_eq!(seeded.waiting_triggers().count(), 0);
    let old = seeded.window().cards().current_identity(CardId(1)).unwrap();
    let final_registry = witness.final_registry.unwrap();
    let new = final_registry.current_identity(CardId(1)).unwrap();
    assert_ne!(old, new);
    assert_eq!(
        final_registry.get(new).unwrap().zone(),
        ObjectZone::Graveyard(SeatId(0))
    );
    assert_eq!(
        seeded.window().cards().get(old).unwrap().zone(),
        ObjectZone::Hand(SeatId(0))
    );
}

#[test]
fn unexpected_adoption_error_keeps_first_primitive_tap_and_failed_attempt() {
    let action = json!({"op":"trusted_call","owner":"GameObjects","method":"set_tapped","object":{"captured_object":"old"},"tapped":true});
    let input = selected(
        json!([{"alias":"P","card_id":1,"owner":"A","controller":"A","zone":"Battlefield","definition":"P","tapped":false}]),
        json!([{"name":"old","join":{"card_id":1}}]),
        json!([action, action]),
        crate::program::Program::creature(
            None,
            1.into(),
            1.into(),
            std::collections::BTreeSet::new(),
        ),
        &["P"],
    );
    let (report, V2Witness::Registry(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "failed");
    assert_eq!(report["failed_action_ordinal"], 2);
    assert_eq!(
        report["records"][1]["result"]["error_path"],
        json!(["StatusUnchanged"])
    );
    assert_eq!(witness.attempts.len(), 2);
    let actual = witness.final_registry.unwrap();
    assert_eq!(actual.iter().next().unwrap().tapped(), Some(true));
    assert_eq!(&actual, witness.attempts[0].1.successor().unwrap());
}

#[test]
fn empty_shuffle_has_separate_primitive_receipt_and_real_executor_event() {
    let input = selected(
        json!([]),
        json!([]),
        json!([{"op":"trusted_call","owner":"GameObjects","method":"shuffle_library","player":"A","entropy_words":[],"adopt":true},{"op":"separate_effect_window_opening_branch","source_snapshot":"fresh_original_fixture_seed_before_any_primitive_mutation","entropy_words":[],"sequence":[{"op":"trusted_call","owner":"EffectWindow","method":"begin_opening"},{"op":"trusted_call","owner":"EffectWindow","method":"opening_shuffle","player":"A","adopt":true}]}]),
        crate::program::Program::creature(
            None,
            1.into(),
            1.into(),
            std::collections::BTreeSet::new(),
        ),
        &[],
    );
    let (report, V2Witness::Registry(witness)) = run(&input).unwrap() else {
        panic!()
    };
    assert_eq!(report["status"], "completed_recipe");
    assert!(report["records"][0]["canonical_rules_append"].is_null());
    let branch = witness.opening.unwrap();
    let transition = branch.shuffle.unwrap().unwrap();
    let events = transition
        .append()
        .records
        .iter()
        .filter(|value| matches!(value, RulesRecord::Event(_)))
        .count();
    assert_eq!(events, 1);
    assert_eq!(transition.state().operational_counters().next_event, 2);
    assert_eq!(
        witness
            .initialization
            .unwrap()
            .operational_counters()
            .next_event,
        1
    );
}
