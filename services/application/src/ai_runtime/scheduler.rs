//! Deterministic pass-delegation and wake scheduler for AI seats.
//!
//! An AI seat only delegates priority passes under its own explicit same-seat,
//! same-turn grant. Required decisions, missing grants, turn changes, skip-cap
//! exhaustion, and any requested state change immediately wake the model.
use crate::player_wire::{PlayerObjectIdentity, PlayerSnapshot, PlayerZoneKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Maximum number of consecutive delegated passes allowed per model grant.
pub const MAX_DELEGATED_SKIPS: u32 = 32;

/// State changes that can wake a sleeping AI pass grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeCondition {
    StackChanged,
    BoardChanged,
    HandChanged,
    LifeChanged,
    OpponentAction,
    OwnMain,
    Combat,
}

impl WakeCondition {
    /// Parse a wake label emitted by the compact LLM reply.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "stack_changed" => Some(Self::StackChanged),
            "board_changed" => Some(Self::BoardChanged),
            "hand_changed" => Some(Self::HandChanged),
            "life_changed" => Some(Self::LifeChanged),
            "opponent_action" => Some(Self::OpponentAction),
            "own_main" => Some(Self::OwnMain),
            "combat" => Some(Self::Combat),
            _ => None,
        }
    }
}

/// Reason the scheduler decided to invoke the model rather than delegate a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeReason {
    RequiredDecision,
    NoGrant,
    TurnChanged,
    SkipCapReached,
    ConditionTriggered(WakeCondition),
}

/// Scheduler outcome for one AI boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleDecision {
    DelegatePass,
    CallModel(WakeReason),
}

/// Deterministic projection summary used to compare current state against the
/// post-accepted baseline of a pass grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSummary {
    pub turn_number: String,
    pub phase: String,
    pub step: String,
    pub active_seat_ids: Vec<Uuid>,
    pub stack_sig: Vec<String>,
    pub board_sig: Vec<String>,
    pub has_creatures: bool,
    pub hand_sig: Vec<String>,
    pub life_sig: BTreeMap<Uuid, String>,
    pub through_event_sequence: String,
}

impl StateSummary {
    /// Build a deterministic state summary from a seat-filtered `PlayerSnapshot`.
    pub fn from_snapshot(snapshot: &PlayerSnapshot) -> Self {
        let (turn_number, phase, step, active_seat_ids) = match &snapshot.turn {
            Some(t) => (
                String::from(t.number.clone()),
                serde_json::to_string(&t.phase).unwrap_or_default(),
                serde_json::to_string(&t.step).unwrap_or_default(),
                t.active_seat_ids.clone(),
            ),
            None => (String::new(), String::new(), String::new(), Vec::new()),
        };

        let stack_sig: Vec<String> = snapshot
            .stack
            .iter()
            .map(|entry| {
                format!(
                    "{}:{}:{}",
                    entry.controller_seat_id,
                    entry.label,
                    entry.targets.len()
                )
            })
            .collect();

        let zone_kinds: BTreeMap<&str, PlayerZoneKind> = snapshot
            .zones
            .iter()
            .map(|z| (z.handle().as_str(), z.zone_type()))
            .collect();

        let mut board_sig = Vec::new();
        let mut has_creatures = false;
        let mut hand_sig = Vec::new();
        for obj in &snapshot.visible_objects {
            let name = match &obj.identity {
                PlayerObjectIdentity::Known {
                    characteristics, ..
                } => characteristics.name.as_str(),
                PlayerObjectIdentity::Concealed {} => "concealed",
            };
            let zone_kind = zone_kinds.get(obj.zone.as_str()).copied();
            match zone_kind {
                Some(PlayerZoneKind::Battlefield) => {
                    if obj.public_properties.as_ref().is_some_and(|p| {
                        p.card_types
                            .contains(&crate::player_wire::PlayerCardType::Creature)
                    }) {
                        has_creatures = true;
                    }
                    let pt = obj.public_properties.as_ref().map(|p| {
                        let pw = p.power.clone().map(String::from).unwrap_or_default();
                        let tg = p.toughness.clone().map(String::from).unwrap_or_default();
                        format!("{pw}/{tg}")
                    });
                    board_sig.push(format!(
                        "{}:{}:{}:{}",
                        obj.controller_seat_id
                            .map_or_else(|| "none".to_owned(), |id| id.to_string()),
                        name,
                        obj.tapped,
                        pt.unwrap_or_default(),
                    ));
                }
                Some(PlayerZoneKind::Hand) if obj.owner_seat_id == snapshot.viewer_seat_id => {
                    hand_sig.push(name.to_owned());
                }
                _ => {}
            }
        }
        board_sig.sort();
        hand_sig.sort();

        let life_sig: BTreeMap<Uuid, String> = snapshot
            .players
            .iter()
            .map(|p| (p.seat_id, String::from(p.life.clone())))
            .collect();

        Self {
            turn_number,
            phase,
            step,
            active_seat_ids,
            stack_sig,
            board_sig,
            has_creatures,
            hand_sig,
            life_sig,
            through_event_sequence: snapshot.through_event_sequence.to_string(),
        }
    }

    fn is_own_main(&self, viewer: Uuid) -> bool {
        self.active_seat_ids.contains(&viewer)
            && (self.phase.contains("precombat-main") || self.phase.contains("postcombat-main"))
    }

    fn is_combat(&self) -> bool {
        self.phase == "\"combat\""
    }
}

/// Active model-authored pass delegation grant for one AI seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassGrant {
    pub turn_number: String,
    pub max_skips: u32,
    pub used_skips: u32,
    pub wake: BTreeSet<WakeCondition>,
    pub baseline: StateSummary,
}

/// Per-seat scheduled pass-delegation controller.
#[derive(Debug, Default, Clone)]
pub struct AiScheduler {
    grant: Option<PassGrant>,
}

impl AiScheduler {
    /// Create a fresh scheduler with no active pass grant.
    pub fn new() -> Self {
        Self { grant: None }
    }

    /// Revoke any active pass grant (e.g., on recovery, error, or model call).
    pub fn revoke(&mut self) {
        self.grant = None;
    }

    /// Evaluate whether the AI can delegate a priority `pass` or must wake the model.
    pub fn evaluate(&self, snapshot: &PlayerSnapshot) -> ScheduleDecision {
        if !snapshot.decisions.is_empty() {
            return ScheduleDecision::CallModel(WakeReason::RequiredDecision);
        }
        let Some(grant) = &self.grant else {
            return ScheduleDecision::CallModel(WakeReason::NoGrant);
        };
        let current = StateSummary::from_snapshot(snapshot);
        if grant.used_skips >= grant.max_skips {
            return ScheduleDecision::CallModel(WakeReason::SkipCapReached);
        }
        // Allow delegation to skip empty upkeep/draw windows at the start of a new turn
        // when the stack is empty; wake immediately once main/combat/stack/board/life changes.
        let in_empty_beginning = current.phase == "\"beginning\"" && current.stack_sig.is_empty();
        if current.turn_number != grant.turn_number && !in_empty_beginning {
            return ScheduleDecision::CallModel(WakeReason::TurnChanged);
        }
        // Defer non-stack wakes during own empty upkeep/draw until own precombat main.
        if in_empty_beginning && current.active_seat_ids.contains(&snapshot.viewer_seat_id) {
            return ScheduleDecision::DelegatePass;
        }
        for &cond in &grant.wake {
            let triggered = match cond {
                WakeCondition::StackChanged => {
                    !current.stack_sig.is_empty() && current.stack_sig != grant.baseline.stack_sig
                }
                WakeCondition::BoardChanged => current.board_sig != grant.baseline.board_sig,
                WakeCondition::HandChanged => current.hand_sig != grant.baseline.hand_sig,
                WakeCondition::LifeChanged => current.life_sig != grant.baseline.life_sig,
                WakeCondition::OpponentAction => {
                    // Only wake when an opponent actually altered meaningful game state
                    // (stack, board, or life). Empty phase/step transitions advance
                    // `through_event_sequence` and must NOT falsely wake pass delegation.
                    (!current.stack_sig.is_empty() && current.stack_sig != grant.baseline.stack_sig)
                        || current.board_sig != grant.baseline.board_sig
                        || current.life_sig != grant.baseline.life_sig
                }
                WakeCondition::OwnMain => {
                    current.is_own_main(snapshot.viewer_seat_id)
                        && (current.phase != grant.baseline.phase
                            || current.step != grant.baseline.step
                            || current.turn_number != grant.turn_number)
                }
                WakeCondition::Combat => {
                    current.is_combat() && !grant.baseline.is_combat() && current.has_creatures
                }
            };
            if triggered {
                return ScheduleDecision::CallModel(WakeReason::ConditionTriggered(cond));
            }
        }
        ScheduleDecision::DelegatePass
    }

    /// Record that one delegated pass was accepted.
    pub fn record_delegated_pass(&mut self) {
        if let Some(grant) = &mut self.grant {
            grant.used_skips = grant.used_skips.saturating_add(1);
        }
    }

    /// Install or clear the seat's pass grant after a model-authored action is accepted.
    pub fn record_model_acceptance(
        &mut self,
        delegate_pass: bool,
        wake: BTreeSet<WakeCondition>,
        max_skips: u32,
        post_snapshot: &PlayerSnapshot,
    ) {
        if !delegate_pass || max_skips == 0 {
            self.grant = None;
            return;
        }
        let baseline = StateSummary::from_snapshot(post_snapshot);
        self.grant = Some(PassGrant {
            turn_number: baseline.turn_number.clone(),
            max_skips: max_skips.min(MAX_DELEGATED_SKIPS),
            used_skips: 0,
            wake,
            baseline,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_summary(turn: &str, phase: &str, stack: &[&str], seq: &str) -> StateSummary {
        StateSummary {
            turn_number: turn.into(),
            phase: format!("\"{phase}\""),
            step: format!("\"{phase}\""),
            active_seat_ids: vec![Uuid::from_u128(1)],
            stack_sig: stack.iter().map(|s| (*s).to_owned()).collect(),
            board_sig: vec![],
            has_creatures: false,
            hand_sig: vec![],
            life_sig: BTreeMap::from([(Uuid::from_u128(1), "20".into())]),
            through_event_sequence: seq.into(),
        }
    }

    #[test]
    fn wake_condition_parsing_is_exact() {
        assert_eq!(
            WakeCondition::from_wire("stack_changed"),
            Some(WakeCondition::StackChanged)
        );
        assert_eq!(
            WakeCondition::from_wire("own_main"),
            Some(WakeCondition::OwnMain)
        );
        assert_eq!(WakeCondition::from_wire("unknown_cond"), None);
    }

    #[test]
    fn summary_helpers_detect_main_and_combat() {
        let viewer = Uuid::from_u128(1);
        let main = sample_summary("1", "precombat-main", &[], "0");
        assert!(main.is_own_main(viewer));
        assert!(!main.is_combat());

        let combat = sample_summary("1", "combat", &[], "1");
        assert!(!combat.is_own_main(viewer));
        assert!(combat.is_combat());
    }
}
