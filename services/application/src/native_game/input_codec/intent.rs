//! Exhaustive original-intent handle traversal; ordered intent vectors stay intact.
use super::*;
use crate::player_wire::{PlayerAction, PlayerChoice, PlayerTarget};
fn target<'a>(value: &'a PlayerTarget, out: &mut BTreeSet<&'a PlayerHandle>) {
    match value {
        PlayerTarget::Object { handle } | PlayerTarget::Stack { handle } => {
            out.insert(handle);
        }
        PlayerTarget::Seat { .. } => {}
    }
}
fn cast_resources<'a>(
    resources: &'a [crate::player_wire::PlayerCastResource],
    out: &mut BTreeSet<&'a PlayerHandle>,
) {
    for resource in resources {
        out.insert(resource.source());
        if let crate::player_wire::PlayerCastResource::Splice {
            target: Some(selected),
            ..
        } = resource
        {
            target(selected, out);
        }
    }
}
fn payment_handle<'a>(
    value: Option<&'a crate::player_wire::PlayerPayment>,
    out: &mut BTreeSet<&'a PlayerHandle>,
) {
    if let Some(handle) = value.and_then(|payment| payment.alternative_cost_source.as_ref()) {
        out.insert(handle);
    }
    if let Some(handle) = value.and_then(|payment| payment.graveyard_permission.as_ref()) {
        out.insert(handle);
    }
}
fn choice<'a>(value: &'a PlayerChoice, out: &mut BTreeSet<&'a PlayerHandle>) {
    match value {
        PlayerChoice::TriggerModes { modes } => {
            for mode in modes {
                for selected in &mode.targets {
                    target(selected, out);
                }
                if let Some(selected) = &mode.target {
                    target(selected, out);
                }
            }
        }
        PlayerChoice::TargetList { targets } => {
            for value in targets {
                target(value, out);
            }
        }
        PlayerChoice::Option { option } => {
            out.insert(option);
        }
        PlayerChoice::Objects { objects } | PlayerChoice::Ordering { ordered: objects } => {
            out.extend(objects);
        }
        PlayerChoice::SearchWithEntryColors { objects, colors } => {
            out.extend(objects);
            out.extend(colors.iter().map(|entry| &entry.object));
        }
        PlayerChoice::SearchWithEntryChoices {
            objects,
            colors,
            reveals,
        } => {
            out.extend(objects);
            out.extend(colors.iter().map(|entry| &entry.object));
            for entry in reveals {
                out.insert(&entry.object);
                if let Some(card) = &entry.card {
                    out.insert(card);
                }
            }
        }
        PlayerChoice::SearchHandGraveyard { hand, graveyard } => {
            out.extend(hand.iter());
            out.extend(graveyard.iter());
        }
        PlayerChoice::SearchToDestination { object, .. } => {
            out.extend(object.iter());
        }
        PlayerChoice::ImpulseCast {
            target: selected,
            resources,
            mana_abilities,
            modes,
            permanent_targets,
            fixed_targets,
            payment,
            ..
        } => {
            payment_handle(payment.as_ref(), out);
            if let Some(t) = selected {
                target(t, out);
            }
            for mode in modes {
                for value in &mode.targets {
                    target(value, out);
                }
                if let Some(value) = &mode.target {
                    target(value, out);
                }
            }
            for value in permanent_targets {
                target(value, out);
            }
            for value in fixed_targets {
                target(value, out);
            }
            cast_resources(resources, out);
            for activation in mana_abilities {
                out.insert(&activation.source);
                out.insert(&activation.ability);
                out.extend(&activation.cost_objects);
            }
        }
        PlayerChoice::MadnessCast {
            target: selected,
            resources,
            mana_abilities,
            payment,
            ..
        } => {
            payment_handle(payment.as_ref(), out);
            if let Some(t) = selected {
                target(t, out);
            }
            cast_resources(resources, out);
            for activation in mana_abilities {
                out.insert(&activation.source);
                out.insert(&activation.ability);
                out.extend(&activation.cost_objects);
            }
        }
        PlayerChoice::CascadeCast {
            target: selected,
            resources,
            payment,
            ..
        }
        | PlayerChoice::ReboundCast {
            target: selected,
            resources,
            payment,
            ..
        }
        | PlayerChoice::SuspendCast {
            target: selected,
            resources,
            payment,
            ..
        } => {
            payment_handle(payment.as_ref(), out);
            if let Some(t) = selected {
                target(t, out);
            }
            cast_resources(resources, out);
        }
        PlayerChoice::Target { target: value } => target(value, out),
        PlayerChoice::Legend { conflict, keep } => {
            out.insert(conflict);
            out.insert(keep);
        }
        PlayerChoice::Prevention { allocations } => {
            out.extend(allocations.iter().map(|x| &x.attempt));
        }
        PlayerChoice::Distribution { allocations } => {
            for entry in allocations {
                target(&entry.target, out);
            }
        }
        PlayerChoice::Attackers { assignments } => {
            for entry in assignments {
                out.insert(&entry.attacker);
                target(&entry.defender, out);
            }
        }
        PlayerChoice::Blockers { assignments } => {
            for entry in assignments {
                out.insert(&entry.blocker);
                out.extend(&entry.attackers);
            }
        }
        PlayerChoice::CombatDamage { assignments } => {
            for entry in assignments {
                out.insert(&entry.source);
                target(&entry.target, out);
            }
        }
        PlayerChoice::Scry { top, bottom } => {
            out.extend(top);
            out.extend(bottom);
        }
        PlayerChoice::Surveil { top, graveyard } => {
            out.extend(top);
            out.extend(graveyard);
        }
        PlayerChoice::CounterRemoval { .. }
        | PlayerChoice::RedeclareKeepingMana {}
        | PlayerChoice::RedeclareReversingMana { .. }
        | PlayerChoice::PayAttackCost { .. }
        | PlayerChoice::Number { .. }
        | PlayerChoice::Pay { .. }
        | PlayerChoice::Decline {} => {}
    }
}
pub(super) fn handles(value: &PlayerAction) -> BTreeSet<&PlayerHandle> {
    let mut out = BTreeSet::new();
    match value {
        PlayerAction::CastFaceDown { source, .. } | PlayerAction::TurnFaceUp { source, .. } => {
            out.insert(source);
        }
        PlayerAction::CastWithResources {
            fixed_targets,
            permanent_targets,
            modes,
            source,
            target: choice,
            resources,
            payment,
            ..
        } => {
            payment_handle(payment.as_ref(), &mut out);
            for selected in permanent_targets {
                target(selected, &mut out);
            }
            for selected in fixed_targets {
                target(selected, &mut out);
            }
            for mode in modes {
                for choice in &mode.targets {
                    target(choice, &mut out);
                }
                if let Some(choice) = &mode.target {
                    target(choice, &mut out);
                }
            }
            out.insert(source);
            if let Some(choice) = choice {
                target(choice, &mut out);
            }
            cast_resources(resources, &mut out);
        }
        PlayerAction::Foretell { source, .. }
        | PlayerAction::Suspend { source, .. }
        | PlayerAction::Plot { source, .. }
        | PlayerAction::PlayLandWithEntryColor { source, .. }
        | PlayerAction::PlayLand { source } => {
            out.insert(source);
        }
        PlayerAction::PlayLandWithEntryReveal { source, reveal } => {
            out.insert(source);
            if let Some(card) = reveal {
                out.insert(card);
            }
        }
        PlayerAction::CastUntargeted { source, payment } => {
            out.insert(source);
            payment_handle(payment.as_ref(), &mut out);
        }
        PlayerAction::CastTargeted {
            source,
            payment,
            target: value,
        } => {
            payment_handle(payment.as_ref(), &mut out);
            out.insert(source);
            target(value, &mut out);
        }
        PlayerAction::ActivateEmblem {
            creature, payment, ..
        } => {
            out.insert(creature);
            payment_handle(payment.as_ref(), &mut out);
        }
        PlayerAction::ActivatePermanentTargets {
            source,
            ability,
            targets,
            ..
        } => {
            out.insert(source);
            out.insert(ability);
            for value in targets {
                target(value, &mut out);
            }
        }
        PlayerAction::ActivateModalAbility {
            source,
            ability,
            modes,
            ..
        } => {
            out.insert(source);
            out.insert(ability);
            for m in modes {
                if let Some(value) = &m.target {
                    target(value, &mut out);
                }
                for value in &m.targets {
                    target(value, &mut out);
                }
            }
        }
        PlayerAction::ActivateAbility {
            source,
            ability,
            target: value,
            ..
        }
        | PlayerAction::ActivateLoyaltyX {
            source,
            ability,
            target: value,
            ..
        } => {
            out.insert(source);
            out.insert(ability);
            target(value, &mut out);
        }
        PlayerAction::Ninjutsu {
            source,
            ability,
            attacker,
            ..
        } => {
            out.insert(source);
            out.insert(ability);
            out.insert(attacker);
        }
        PlayerAction::ActivateWithCreatures {
            source,
            ability,
            creatures,
        } => {
            out.insert(source);
            out.insert(ability);
            out.extend(creatures.iter());
        }
        PlayerAction::ActivateWithCostObjects {
            source,
            ability,
            target: selected,
            objects,
            ..
        } => {
            out.insert(source);
            out.insert(ability);
            out.extend(objects);
            if let Some(value) = selected {
                target(value, &mut out);
            }
        }
        PlayerAction::ActivateManaForPlayer {
            source, ability, ..
        } => {
            out.insert(source);
            out.insert(ability);
        }
        PlayerAction::ActivateManaWithCostObjects {
            source,
            ability,
            objects,
        } => {
            out.insert(source);
            out.insert(ability);
            out.extend(objects);
        }
        PlayerAction::ActivateUntargetedX {
            source, ability, ..
        }
        | PlayerAction::ActivateUntargetedAbility {
            source, ability, ..
        }
        | PlayerAction::Transmute {
            source, ability, ..
        }
        | PlayerAction::Cycle {
            source, ability, ..
        }
        | PlayerAction::ActivateMana { source, ability }
        | PlayerAction::ActivateManaWithPayment {
            source, ability, ..
        }
        | PlayerAction::ActivateHybridMana {
            source, ability, ..
        }
        | PlayerAction::ActivateManaWithBundle {
            source, ability, ..
        } => {
            out.insert(source);
            out.insert(ability);
        }
        PlayerAction::Choose { decision, response } => {
            out.insert(decision);
            choice(response, &mut out);
        }
        PlayerAction::Pass {} | PlayerAction::Concede {} => {}
    }
    out
}
