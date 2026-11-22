//! Actual definition-constructor attempts, without initialization/checks/body work.
use super::*;

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    let [action] = rows(&selected.fixture, "actions")? else {
        return Err("definition admission requires exactly one literal attempt".into());
    };
    if action
        != &json!({"op":"attempt_construction_without_adoption","owner":"EffectWindow","method":"new","seed":"this_fixture_complete_seed"})
        || selected.fixture["stop"]
            != json!({"kind":"AfterConstructionAttempt","after_action_ordinal":1,"no_extra_pass":true})
    {
        return Err("unsupported definition-construction operation/stop".into());
    }
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::DeferToOwner,
    )?;
    if prepared.initial_status.iter().any(|(_, tapped)| *tapped)
        || !prepared.trusted_precheck_overrides.is_empty()
        || !matches!(prepared.initial, packet::Initial::TrustedUnitPrecheck {})
    {
        return Err("construction attempt cannot hide preparatory mutations".into());
    }
    // Syntax is decoded canonically, while the actual constructor owns semantic
    // admission and its native InvalidSetup result. There is no source state yet.
    let (result, writer_point) =
        crate::test_support::native_calls::effect_initialize_recorded(prepared.seed, false);
    let actual = match &result {
        Ok(state) => json!({"tag":"Ok","state":projection::effect(state,&prepared.aliases)?}),
        Err(error) => projection::effect_error(error),
    };
    let record = capture::attach_writer_point(
        json!({"ordinal":1,"operation":action,"result":actual}),
        writer_point,
    );
    Ok((
        json!({"status":"completed_recipe","scope":"trusted_definition_admission","source_state":null,"adopt_requested":false,"records":[record],"stop":selected.fixture["stop"],"checks_or_body_execution":false}),
        V2Witness::Construction(Box::new(result)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::Program;

    fn selected(program: Program) -> packet::SelectedInput {
        packet::SelectedInput {
            fixture: json!({
                "scope":"trusted_definition_admission",
                "seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"creature_vocabulary":crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id().to_string(),"game_id":1,"context":["unit","cr","cards","programs","private"],"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"TrustedUnitPrecheck"},"floating_mana":[],"initial_world_cohorts":[],"initial_battlefield_order":[],"library_order":{"A":[],"B":[]},"cards":[{"alias":"P","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"P","tapped":null}]},
                "actions":[{"op":"attempt_construction_without_adoption","owner":"EffectWindow","method":"new","seed":"this_fixture_complete_seed"}],
                "stop":{"kind":"AfterConstructionAttempt","after_action_ordinal":1,"no_extra_pass":true}
            }),
            definitions: BTreeMap::from([(
                "P".into(),
                json!({"native_program":program::view(&program)}),
            )]),
            metadata: Value::Null,
            vocabulary: crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
        }
    }

    #[test]
    fn deferred_codec_returns_actual_late_body_admission_error_without_a_game() {
        let input = selected(Program::instant(
            crate::mana::ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    crate::program::Instruction::Damage { amount: 1.into() },
                    crate::program::Instruction::Damage { amount: 0.into() },
                ],
            ),
        ));
        let (report, witness) = run(&input).unwrap();
        assert_eq!(report["status"], "completed_recipe");
        assert_eq!(
            report["records"][0]["result"]["error_domain"],
            "EffectError"
        );
        assert_eq!(
            report["records"][0]["result"]["error_path"],
            json!(["InvalidSetup"])
        );
        assert!(
            matches!(witness, V2Witness::Construction(result) if *result==Err(EffectError::InvalidSetup))
        );
    }

    #[test]
    fn valid_zero_draw_construction_has_no_checks_seed_events_or_ready_reconstruction() {
        let input = selected(Program::instant(
            crate::mana::ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![crate::program::Instruction::DrawController {
                    amount: crate::program::ValueExpr::Literal(0.into()),
                }],
            ),
        ));
        let (_, witness) = run(&input).unwrap();
        let V2Witness::Construction(result) = witness else {
            panic!("wrong constructor witness")
        };
        let state = result.unwrap();
        assert!(matches!(
            state.window().phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ));
        assert_eq!(state.operational_counters().next_event, 1);
        assert_eq!(state.waiting_triggers().count(), 0);
        assert!(state.window().entries().is_empty());
    }
}
