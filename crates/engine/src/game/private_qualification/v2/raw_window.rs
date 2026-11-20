//! Raw allocation units have actual window receipts and no executor body claim.
use super::*;
use crate::stack_window::{OrderedTrigger, OrderedTriggerReceipt, StackWindow, StackWindowError};
mod resolution;
pub(super) use resolution::{RawResolutionWitness, resolution_branch};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RawWindowWitness {
    initialization: Box<Result<EffectWindow, EffectError>>,
    attempts: Vec<(
        StackWindow,
        Result<OrderedTriggerReceipt, StackWindowError>,
        bool,
    )>,
    final_window: Option<StackWindow>,
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if !matches!(prepared.initial, packet::Initial::TrustedUnitPrecheck {})
        || !prepared.trusted_precheck_overrides.is_empty()
        || prepared.initial_status.iter().any(|(_, value)| *value)
    {
        return Err("raw window cannot hide preparation".into());
    }
    let initialization = crate::test_support::native_calls::effect_initialize(prepared.seed);
    let mut witness = RawWindowWitness {
        initialization: Box::new(initialization.clone()),
        attempts: vec![],
        final_window: None,
    };
    let seeded = match initialization {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","initialization":projection::effect_error(&error)}),
                V2Witness::RawWindow(Box::new(witness)),
            ));
        }
    };
    let joins = Joins::seed(&selected.fixture, &seeded)?;
    let mut window = seeded.window().clone();
    let mut records = Vec::new();
    let mut failed = None;
    let actions = rows(&selected.fixture, "actions")?;
    if selected.fixture["stop"]["after_action_ordinal"].as_u64() != Some(actions.len() as u64)
        || selected.fixture["stop"]["no_extra_pass"] != true
    {
        return Err("unsupported raw window stop".into());
    }
    for (index, action) in actions.iter().enumerate() {
        if action["op"] != "trusted_call"
            || action["owner"] != "StackWindow"
            || action["method"] != "record_ordered_triggers"
        {
            return Err("unsupported raw window operation".into());
        }
        let triggers = rows(action, "triggers")?
            .iter()
            .map(|row| {
                Ok(OrderedTrigger {
                    controller: packet::seat(string(row, "controller")?)?,
                    source: seeded
                        .window()
                        .cards()
                        .get(joins.object(&row["source"], &selected.fixture, &seeded)?)
                        .ok_or("unknown raw trigger source")?
                        .clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let source = window.clone();
        let expected = source.revision();
        let adopt = action["adopt"].as_bool().unwrap_or(true);
        let result = capture::window(
            &source,
            projection::StackWindowCall::RecordOrderedTriggersWithReceipt {
                expected,
                triggers: &triggers,
            },
            |source| source.record_ordered_triggers_with_receipt(expected, triggers.clone()),
            |result| {
                Ok(match result {
                    Ok(value) => {
                        let (window, entries) = value.clone().into_parts();
                        json!({"tag":"Ok","state":projection::stack_window(&window),"allocated_entries":entries.iter().map(projection::opaque).collect::<Vec<_>>()})
                    }
                    Err(error) => projection::stack_window_error(error),
                })
            },
            |value| Some(value.clone().into_parts().0),
            adopt,
        )?;
        let actual = match &result {
            Ok(value) => {
                let (next, entries) = value.clone().into_parts();
                json!({"tag":"Ok","state":projection::stack_window(&next),"allocated_entries":entries.iter().map(projection::opaque).collect::<Vec<_>>()})
            }
            Err(error) => projection::stack_window_error(error),
        };
        records.push(capture::window_point(json!({"ordinal":index+1,"operation":action,"source":projection::stack_window(&source),"result":actual,"adopt_requested":adopt,"retained_source_full_state_eq":window==source,"raw_source_allocation_only_no_executor_body_claim":true}),&source,"raw_window actual source",Some(&format!("/observation/records/{}/source",records.len())))?);
        if adopt {
            if let Ok(value) = &result {
                window = value.clone().into_parts().0;
            } else {
                failed = Some(index + 1);
            }
        }
        witness.attempts.push((source, result, adopt));
        if failed.is_some() {
            break;
        }
    }
    witness.final_window = Some(window.clone());
    Ok((
        capture::window_point(
            json!({"status":if failed.is_none(){"completed_recipe"}else{"failed"},"scope":"trusted_stack_window","records":records,"failed_action_ordinal":failed,"stop_reached":failed.is_none(),"final_window":projection::stack_window(&window),"initial_executor":projection::effect(&seeded,&prepared.aliases)?,"executor_window_not_replaced":true,"stop":selected.fixture["stop"]}),
            &window,
            "raw_window actual final window",
            Some("/observation/final_window"),
        )?,
        V2Witness::RawWindow(Box::new(witness)),
    ))
}
