//! Input-only structural stops for the finite trusted trajectories.
use super::*;
use crate::{effects::DamageParentView, program::CreatureTypeSlot};

fn entry(caller: &Caller<'_>, name: &str) -> Result<StackEntryId, String> {
    caller
        .joins
        .entry(name)
        .ok_or_else(|| format!("unbound structural entry {name}"))
}
fn locals(
    caller: &Caller<'_>,
    rows: &Value,
) -> Result<BTreeMap<CreatureTypeSlot, CreatureTypeId>, String> {
    let mut result = BTreeMap::new();
    for row in rows.as_array().ok_or("missing structural locals")? {
        let slot = creature_types::slot(row["slot"].clone())?;
        let id = creature_types::id(row["value"].clone(), caller.state.creature_vocabulary())?;
        if result.insert(slot, id).is_some() {
            return Err("duplicate structural local slot".into());
        }
    }
    Ok(result)
}
pub(super) fn before(caller: &Caller<'_>, action: &Value) -> Result<(), String> {
    if action["ordinal"].as_u64() != Some(caller.ordinal as u64) {
        return Err("literal action ordinal differs".into());
    }
    if !action["required_source_phase"].is_null()
        && (action["required_source_phase"] != "Priority::RequiresChecks"
            || !matches!(
                caller.state.window().phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            ))
    {
        return Err("actual source is not selected RequiresChecks boundary".into());
    }
    Ok(())
}
pub(super) fn after(caller: &Caller<'_>, action: &Value) -> Result<(), String> {
    if !action["required_result"].is_null()
        && !matches!(caller.completion, Some(RuleCompletion::Checks(_)))
    {
        return Err("actual checks did not complete at selected boundary".into());
    }
    let stop = &action["stop"];
    if stop["pending_kind"] == "CreatureType" || stop["kind"] == "CreatureType" {
        let Some(EffectDecision::CreatureType(frame)) = caller.frame.as_ref() else {
            return Err("actual CreatureType boundary not reached".into());
        };
        let excluding = rows(stop, "excluding")?
            .iter()
            .cloned()
            .map(|id| creature_types::id(id, caller.state.creature_vocabulary()))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if frame.entry() != entry(caller, string(stop, "entry_ref")?)?
            || frame.instruction().to_string() != string(stop, "instruction")?
            || frame.slot() != creature_types::slot(stop["slot"].clone())?
            || frame.chooser() != packet::seat(string(stop, "chooser")?)?
            || frame.vocabulary().to_string() != string(stop, "vocabulary")?
            || frame.excluding() != &excluding
        {
            return Err(
                "actual CreatureType frame differs from selected structural boundary".into(),
            );
        }
        if !stop["locals"].is_null()
            && caller
                .state
                .resolution_state()
                .ok_or("missing actual cursor")?
                .locals()
                != &locals(caller, &stop["locals"])?
        {
            return Err("actual cursor locals differ at selected frame boundary".into());
        }
    }
    if !stop["actual_cursor_entry"].is_null() {
        let cursor = caller
            .state
            .resolution_state()
            .ok_or("missing actual one-step cursor")?;
        if cursor.entry() != entry(caller, string(stop, "actual_cursor_entry")?)?
            || cursor.instruction().to_string() != string(stop, "actual_cursor_instruction")?
            || cursor.locals() != &locals(caller, &stop["actual_locals"])?
        {
            return Err("actual one-step helper cursor differs from selected boundary".into());
        }
    }
    let pending = &action["required_pending"];
    if !pending.is_null() {
        let Some(EffectDecision::Damage(crate::effects::DamageDecision::Allocation(frame))) =
            caller.frame.as_ref()
        else {
            return Err("actual allocation boundary not reached".into());
        };
        let Some(DamageParentView::Resolution(cursor)) = caller.state.damage_parent() else {
            return Err("actual moved Resolution parent absent".into());
        };
        let shield = caller
            .creations
            .get(&(
                entry(caller, string(&pending["shield_join"], "creator_entry")?)?,
                integer(&pending["shield_join"]["instruction"])? as usize,
            ))
            .ok_or("no actual selected shield creation")?;
        if pending["kind"] != "Damage::Allocation"
            || frame.chooser() != packet::seat(string(pending, "chooser")?)?
            || frame.required() != &quantity(&pending["required"])?
            || frame.eligible().len().to_string() != string(pending, "eligible_count")?
            || frame.application().effect != *shield
            || cursor.entry() != entry(caller, string(pending, "parent_entry")?)?
            || cursor.instruction().to_string() != string(pending, "parent_instruction")?
            || cursor.locals() != &locals(caller, &pending["parent_locals"])?
        {
            return Err("actual allocation/parent boundary differs from selected input".into());
        }
    }
    if action["required_parent"] == "Combat"
        && (!matches!(caller.state.damage_parent(), Some(DamageParentView::Combat))
            || caller.state.resolution_state().is_some())
    {
        return Err("actual Combat parent retained a resolution cursor".into());
    }
    Ok(())
}
