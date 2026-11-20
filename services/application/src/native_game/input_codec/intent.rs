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
fn choice<'a>(value: &'a PlayerChoice, out: &mut BTreeSet<&'a PlayerHandle>) {
    match value {
        PlayerChoice::LibraryPartition { top, other } => {
            out.extend(top);
            out.extend(other);
        }
        PlayerChoice::Option { option } => {
            out.insert(option);
        }
        PlayerChoice::Objects { objects } | PlayerChoice::Ordering { ordered: objects } => {
            out.extend(objects);
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
        PlayerChoice::Number { .. } | PlayerChoice::Pay { .. } | PlayerChoice::Decline {} => {}
    }
}
pub(super) fn handles(value: &PlayerAction) -> BTreeSet<&PlayerHandle> {
    let mut out = BTreeSet::new();
    match value {
        PlayerAction::CastUntargeted { source, .. } | PlayerAction::PlayLand { source } => {
            out.insert(source);
        }
        PlayerAction::CastTargeted {
            source,
            target: value,
            ..
        } => {
            out.insert(source);
            target(value, &mut out);
        }
        PlayerAction::ActivateAbility {
            source,
            ability,
            target: value,
            ..
        } => {
            out.insert(source);
            out.insert(ability);
            target(value, &mut out);
        }
        PlayerAction::ActivateMana { source, ability } => {
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
