//! Structural maps over real compiled old recipes; no native/source qualification.
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[test]
fn complete_compiled_cases_roundtrip_all_parameters_and_instruction_variants() {
    let raw: Value = serde_json::from_str(include_str!("../../mandatory-inputs-v1.json")).unwrap();
    let original: Inputs = serde_json::from_value(raw.clone()).unwrap();
    let kinds = raw["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|case| case["instructions"].as_array().unwrap())
        .map(|row| row["kind"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(kinds.len(), 9);
    for case in original.cases {
        let expected = case.clone();
        let mut seen = Vec::new();
        let mapped = case
            .map_execution(
                |init| (init,),
                |plan| (plan,),
                |kind| {
                    seen.push(kind);
                    (kind,)
                },
            )
            .map_id(|id| (id,));
        let expected_kinds = expected
            .instructions
            .iter()
            .flat_map(|instruction| match instruction {
                Instruction::CastTargeted { payment, .. }
                | Instruction::CastUntargeted { payment, .. } => {
                    payment.iter().map(|row| row.mana_type).collect::<Vec<_>>()
                }
                _ => vec![],
            })
            .collect::<Vec<_>>();
        assert_eq!(seen, expected_kinds);
        let restored = mapped
            .map_execution(|(init,)| init, |(plan,)| plan, |(kind,)| kind)
            .map_id(|(id,)| id);
        assert_eq!(restored, expected);
    }
}

#[test]
fn optional_defaults_and_non_default_plan_type_keep_existing_wire_contract() {
    use crate::ordinary_game::optional::OptionalCase;
    let raw: Value = serde_json::from_str(include_str!("../../optional-recipes-v2.json")).unwrap();
    let parsed: Inputs<OptionalCase> = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(parsed.cases.len(), 26);
    for case in parsed.cases {
        let expected = case.clone();
        let actual = case
            .map_execution(|init| (init,), |plan| (plan,), |mana| (mana,))
            .map_execution(|(init,)| init, |(plan,)| plan, |(mana,)| mana);
        assert_eq!(actual, expected);
    }
    // DecisionPlan deliberately has no Default implementation. Vec's existing
    // field default must not introduce such a generic bound or weaken enums.
    let mut first = raw["cases"][0].clone();
    first.as_object_mut().unwrap().remove("decision_plans");
    first.as_object_mut().unwrap().remove("reached_probe_limit");
    let old: Case<OptionalCase> = serde_json::from_value(first.clone()).unwrap();
    assert!(old.decision_plans.is_empty());
    assert!(old.reached_probe_limit.is_none());
    first["decision_plans"] = Value::Null;
    assert!(serde_json::from_value::<Case<OptionalCase>>(first).is_err());
}

#[test]
fn old_wire_rejects_foreign_fields_kinds_and_sixth_mana_name() {
    let raw: Value = serde_json::from_str(include_str!("../../mandatory-inputs-v1.json")).unwrap();
    let mut bad = raw["cases"][0].clone();
    bad["foreign"] = json!(true);
    assert!(serde_json::from_value::<Case>(bad).is_err());
    let duplicate = serde_json::to_string(&raw["cases"][0]).unwrap();
    let duplicate = duplicate.replacen("\"id\":", "\"id\":\"MT01\",\"id\":", 1);
    assert!(serde_json::from_str::<Case>(&duplicate).is_err());
    let row = raw["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|case| case["instructions"].as_array().unwrap())
        .find(|row| {
            row.get("payment")
                .and_then(Value::as_array)
                .is_some_and(|p| !p.is_empty())
        })
        .unwrap();
    let mut row = row.clone();
    row["payment"][0]["mana_type"] = json!("Colourless");
    assert!(serde_json::from_value::<Instruction>(row).is_err());
    let mut bad = raw["cases"][0].clone();
    bad["initialization"]["kind"] = json!("selection_prepared");
    assert!(serde_json::from_value::<Case>(bad).is_err());
    let mut bad = raw["cases"][0].clone();
    bad["decision_plans"] = json!([{ "kind":"resolution_select" }]);
    assert!(serde_json::from_value::<Case>(bad).is_err());
}
