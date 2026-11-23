//! Deterministic priority affordance probing, optimal auto-tapping, and safe
//! boundary fallbacks for Spellwake's native Rust AI runtime.
//!
//! Runs in `<1 ms` directly against a `NativeScope` to:
//! 1. Identify every legal land play, untapped mana source (with color and
//!    land-vs-creature priority), castable spell in hand, and activatable ability.
//! 2. Enable zero-LLM `<1 ms` fast-passes whenever no non-pass priority play exists.
//! 3. Auto-tap lands (preferring lands over mana creatures) and synthesize exact
//!    `PlayerPayment` values when the model emits a cast with `payment: null` or
//!    without pre-tapping lands in a `cast-batch`.
use super::compact::AiActionPlan;
use crate::{
    native_game::NativeScope,
    player_wire::{
        ManaColor, ManaSpend, PlayerAbilityKind, PlayerAction, PlayerBasis, PlayerCardType,
        PlayerCommand, PlayerHandle, PlayerObjectIdentity, PlayerPayment, PlayerSnapshot,
        PlayerTarget, PlayerZoneKind, RulesCount,
    },
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// One verified legal untapped mana source on the viewer's battlefield.
#[derive(Clone, PartialEq, Eq)]
pub struct ManaSourceInfo {
    pub source: PlayerHandle,
    pub ability: PlayerHandle,
    pub color: Option<ManaColor>,
    pub is_creature: bool,
}

/// One hand spell verified legal to cast right now, along with its verified legal targets.
#[derive(Clone)]
pub struct CastableSpell {
    pub source: PlayerHandle,
    pub untargeted: bool,
    pub legal_targets: Vec<PlayerTarget>,
}

/// Verified legal priority affordances computed on the exact current `NativeScope`.
#[derive(Default, Clone)]
pub struct PriorityAffordances {
    /// Legal `PlayLand` and `ActivateMana` basic actions (compatible with `compact_prompt`).
    pub basic_actions: Vec<PlayerAction>,
    /// Verified untapped mana sources annotated with produced color and land/creature priority.
    pub mana_sources: Vec<ManaSourceInfo>,
    /// Hand object handles verified legal to cast right now (with current pool + auto-tap).
    pub castable_hand: Vec<PlayerHandle>,
    /// Detailed castable spells including verified legal targets for targeted spells.
    pub castable_spells: Vec<CastableSpell>,
    /// Own battlefield `(source, ability)` non-mana abilities verified legal to activate right now.
    pub activatable_abilities: Vec<(PlayerHandle, PlayerHandle)>,
}

impl PriorityAffordances {
    /// Returns `true` if at least one legal non-pass priority play exists right now.
    ///
    /// Note: tapping lands for mana alone is NOT a meaningful standalone play when no
    /// land play, castable spell, or non-mana activated ability is available.
    pub fn has_non_pass_play(&self) -> bool {
        let has_land_play = self
            .basic_actions
            .iter()
            .any(|a| matches!(a, PlayerAction::PlayLand { .. }));
        has_land_play || !self.castable_hand.is_empty() || !self.activatable_abilities.is_empty()
    }

    /// Aggregate currently available mana (pool + verified untapped mana sources) by color.
    pub fn available_mana_counts(&self, snapshot: &PlayerSnapshot) -> BTreeMap<&'static str, u32> {
        let mut counts = BTreeMap::new();
        for (color, amt) in current_mana_pool(snapshot) {
            if amt > 0 {
                *counts.entry(mana_color_name(color)).or_insert(0) += amt;
            }
        }
        for src in &self.mana_sources {
            let key = src.color.map_or("any", mana_color_name);
            *counts.entry(key).or_insert(0) += 1;
        }
        counts
    }
}

/// Parsed simple mana cost (colored pips + generic amount).
#[derive(Default, Clone, PartialEq, Eq)]
pub struct ParsedManaCost {
    pub colored: BTreeMap<ManaColor, u32>,
    pub generic: u32,
    pub has_x: bool,
}

impl ParsedManaCost {
    /// Parse standard Scryfall/Oracle mana cost strings such as `"{1}{R}"`, `"{G}{G}{G}"`, `"{3}{W}{W}"`.
    pub fn parse(raw: &str) -> Self {
        let mut out = Self::default();
        let mut rest = raw;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                break;
            };
            let sym = &after[..close];
            match sym {
                "W" => *out.colored.entry(ManaColor::White).or_insert(0) += 1,
                "U" => *out.colored.entry(ManaColor::Blue).or_insert(0) += 1,
                "B" => *out.colored.entry(ManaColor::Black).or_insert(0) += 1,
                "R" => *out.colored.entry(ManaColor::Red).or_insert(0) += 1,
                "G" => *out.colored.entry(ManaColor::Green).or_insert(0) += 1,
                "C" => *out.colored.entry(ManaColor::Colourless).or_insert(0) += 1,
                "X" => out.has_x = true,
                other => {
                    if let Ok(n) = other.parse::<u32>() {
                        out.generic = out.generic.saturating_add(n);
                    }
                }
            }
            rest = &after[close + 1..];
        }
        out
    }

    pub fn total(&self) -> u32 {
        let colored_sum: u32 = self.colored.values().copied().sum();
        colored_sum.saturating_add(self.generic)
    }
}

/// Probe every verified legal priority affordance on the exact current `NativeScope`.
pub fn probe_priority_affordances(
    scope: &NativeScope<'_>,
    snapshot: &PlayerSnapshot,
) -> PriorityAffordances {
    let mut out = PriorityAffordances::default();
    if !snapshot.decisions.is_empty() {
        return out;
    }

    let zone_kinds: HashMap<&str, PlayerZoneKind> = snapshot
        .zones
        .iter()
        .map(|z| (z.handle().as_str(), z.zone_type()))
        .collect();

    // Pass 1: probe PlayLand and ActivateMana (recording mana color + land/creature priority).
    for object in &snapshot.visible_objects {
        if object.owner_seat_id != snapshot.viewer_seat_id
            && object.controller_seat_id != Some(snapshot.viewer_seat_id)
        {
            continue;
        }
        let is_land = object
            .public_properties
            .as_ref()
            .is_some_and(|p| p.card_types.contains(&PlayerCardType::Land));
        let is_creature = object
            .public_properties
            .as_ref()
            .is_some_and(|p| p.card_types.contains(&PlayerCardType::Creature));

        if is_land {
            let act = PlayerAction::PlayLand {
                source: object.handle.clone(),
            };
            let cmd = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: act.clone(),
            };
            if scope.propose(&cmd).is_ok() {
                out.basic_actions.push(act);
            }
        }

        for ability in &object.abilities {
            if ability.kind == PlayerAbilityKind::Mana {
                let act = PlayerAction::ActivateMana {
                    source: object.handle.clone(),
                    ability: ability.handle.clone(),
                };
                let cmd = PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action: act.clone(),
                };
                if scope.propose(&cmd).is_ok() {
                    let color = infer_mana_color(&object.identity);
                    out.basic_actions.push(act);
                    out.mana_sources.push(ManaSourceInfo {
                        source: object.handle.clone(),
                        ability: ability.handle.clone(),
                        color,
                        is_creature,
                    });
                }
            }
        }
    }

    // Fast timing check: sorcery-speed cards can only be cast in own main phase with empty stack.
    let sorcery_timing_open = snapshot.stack.is_empty()
        && snapshot.turn.as_ref().is_some_and(|t| {
            let phase = serde_json::to_string(&t.phase).unwrap_or_default();
            t.active_seat_ids.contains(&snapshot.viewer_seat_id)
                && (phase.contains("precombat-main") || phase.contains("postcombat-main"))
        });

    // Collect candidate targets once for probing targeted spells/abilities.
    let candidate_targets = collect_candidate_targets(snapshot, &zone_kinds);

    // Pass 2: probe castable non-land spells in hand using fast single-tap probing.
    for object in &snapshot.visible_objects {
        if object.owner_seat_id != snapshot.viewer_seat_id {
            continue;
        }
        if zone_kinds.get(object.zone.as_str()).copied() != Some(PlayerZoneKind::Hand) {
            continue;
        }
        let is_land = object
            .public_properties
            .as_ref()
            .is_some_and(|p| p.card_types.contains(&PlayerCardType::Land));
        if is_land {
            continue;
        }
        if !sorcery_timing_open
            && !can_cast_at_instant_speed(&object.identity, object.public_properties.as_ref())
        {
            continue;
        }

        let cost = object_mana_cost(&object.identity);
        let Some((taps, payment)) = solve_mana_for_cost(&cost, snapshot, &out.mana_sources) else {
            continue;
        };

        // Build candidate list: index 0 = untargeted, indices 1.. = each candidate target.
        let mut candidates = Vec::with_capacity(1 + candidate_targets.len());
        candidates.push(PlayerAction::CastUntargeted {
            source: object.handle.clone(),
            payment: Some(payment.clone()),
        });
        for target in &candidate_targets {
            candidates.push(PlayerAction::CastTargeted {
                source: object.handle.clone(),
                target: target.clone(),
                payment: Some(payment.clone()),
            });
        }

        let accepted = scope.probe_cast_candidates(&taps, &candidates);
        if accepted.is_empty() {
            continue;
        }
        let untargeted = accepted.contains(&0);
        let legal_targets: Vec<PlayerTarget> = accepted
            .into_iter()
            .filter(|&i| i > 0)
            .map(|i| candidate_targets[i - 1].clone())
            .collect();
        out.castable_hand.push(object.handle.clone());
        out.castable_spells.push(CastableSpell {
            source: object.handle.clone(),
            untargeted,
            legal_targets,
        });
    }

    // Pass 3: probe own non-mana activated abilities on the battlefield.
    for object in &snapshot.visible_objects {
        if object.controller_seat_id != Some(snapshot.viewer_seat_id) {
            continue;
        }
        if zone_kinds.get(object.zone.as_str()).copied() != Some(PlayerZoneKind::Battlefield) {
            continue;
        }
        for ability in &object.abilities {
            if ability.kind != PlayerAbilityKind::Activated {
                continue;
            }
            let mut candidates = Vec::with_capacity(1 + candidate_targets.len());
            candidates.push(PlayerAction::ActivateUntargetedAbility {
                source: object.handle.clone(),
                ability: ability.handle.clone(),
                payment: None,
            });
            for target in &candidate_targets {
                candidates.push(PlayerAction::ActivateAbility {
                    source: object.handle.clone(),
                    ability: ability.handle.clone(),
                    target: target.clone(),
                    payment: None,
                });
            }
            if !scope.probe_cast_candidates(&[], &candidates).is_empty() {
                out.activatable_abilities
                    .push((object.handle.clone(), ability.handle.clone()));
            }
        }
    }

    out
}

fn can_cast_at_instant_speed(
    identity: &PlayerObjectIdentity,
    props: Option<&crate::player_wire::PlayerPublicProperties>,
) -> bool {
    if props.is_some_and(|p| p.card_types.contains(&PlayerCardType::Instant)) {
        return true;
    }
    if let PlayerObjectIdentity::Known {
        characteristics, ..
    } = identity
        && (characteristics.type_line.contains("Instant")
            || characteristics.rules_text.contains("Flash"))
    {
        return true;
    }
    false
}

/// Automatically tap lands/mana sources and populate `PlayerPayment` when the AI emits
/// a `CastUntargeted` / `CastTargeted` (or a `CastBatch` missing `payment`) without
/// having pre-tapped enough mana into `mana_pool`.
pub fn auto_tap_plan(
    plan: AiActionPlan,
    snapshot: &PlayerSnapshot,
    mana_sources: &[ManaSourceInfo],
    basis: &PlayerBasis,
) -> AiActionPlan {
    match plan {
        AiActionPlan::Single(mut cmd) => {
            let source_handle = match &cmd.action {
                PlayerAction::CastUntargeted { source, .. }
                | PlayerAction::CastTargeted { source, .. } => Some(source.clone()),
                _ => None,
            };
            let Some(source_handle) = source_handle else {
                return AiActionPlan::Single(cmd);
            };
            let Some(obj) = snapshot
                .visible_objects
                .iter()
                .find(|o| o.handle == source_handle)
            else {
                return AiActionPlan::Single(cmd);
            };
            let cost = object_mana_cost(&obj.identity);
            if cost.total() == 0 {
                return AiActionPlan::Single(cmd);
            }
            let Some((taps, payment)) = solve_mana_for_cost(&cost, snapshot, mana_sources) else {
                return AiActionPlan::Single(cmd);
            };
            match &mut cmd.action {
                PlayerAction::CastUntargeted { payment: p, .. }
                | PlayerAction::CastTargeted { payment: p, .. } => {
                    *p = Some(payment);
                }
                _ => {}
            }
            if taps.is_empty() {
                AiActionPlan::Single(cmd)
            } else {
                let mut cmds = Vec::with_capacity(taps.len() + 1);
                for tap in taps {
                    cmds.push(PlayerCommand {
                        request_id: Uuid::new_v4(),
                        basis: basis.clone(),
                        action: tap,
                    });
                }
                cmds.push(*cmd);
                AiActionPlan::CastBatch(cmds)
            }
        }
        AiActionPlan::CastBatch(mut cmds) => {
            // If the model emitted an explicit cast-batch with `payment: null`, synthesize payment.
            if let Some((last, taps)) = cmds.split_last_mut() {
                let needs_payment = matches!(
                    &last.action,
                    PlayerAction::CastUntargeted { payment: None, .. }
                        | PlayerAction::CastTargeted { payment: None, .. }
                );
                if needs_payment {
                    let src_handle = match &last.action {
                        PlayerAction::CastUntargeted { source, .. }
                        | PlayerAction::CastTargeted { source, .. } => Some(source.clone()),
                        _ => None,
                    };
                    if let Some(sh) = src_handle
                        && let Some(obj) = snapshot.visible_objects.iter().find(|o| o.handle == sh)
                    {
                        let cost = object_mana_cost(&obj.identity);
                        // Build payment from existing mana pool + the tapped sources in the batch.
                        let mut pool = current_mana_pool(snapshot);
                        for tap_cmd in taps.iter() {
                            if let PlayerAction::ActivateMana { source, ability } = &tap_cmd.action
                                && let Some(info) = mana_sources
                                    .iter()
                                    .find(|m| &m.source == source && &m.ability == ability)
                                && let Some(color) = info.color
                            {
                                *pool.entry(color).or_insert(0) += 1;
                            }
                        }
                        if let Some(payment) = build_payment_from_pool(&cost, &pool) {
                            match &mut last.action {
                                PlayerAction::CastUntargeted { payment: p, .. }
                                | PlayerAction::CastTargeted { payment: p, .. } => {
                                    *p = Some(payment);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            AiActionPlan::CastBatch(cmds)
        }
    }
}

/// Produce an actionable, model-readable diagnostic when an AI action is rejected
/// by the native engine (for example, pointing out exact legal targets for a targeted spell).
pub fn diagnose_rejection(
    plan: &AiActionPlan,
    affordances: &PriorityAffordances,
    codec: &super::compact::HandleCodec,
    raw_err: &str,
) -> String {
    let final_action = match plan {
        AiActionPlan::Single(cmd) => &cmd.action,
        AiActionPlan::CastBatch(cmds) => match cmds.last() {
            Some(c) => &c.action,
            None => return format!("native engine rejected empty batch ({raw_err})"),
        },
    };
    if let PlayerAction::CastTargeted { source, target, .. } = final_action {
        let short_src = codec.short_of(source.as_str()).unwrap_or(source.as_str());
        if let Some(sp) = affordances
            .castable_spells
            .iter()
            .find(|s| &s.source == source)
        {
            if !sp.legal_targets.contains(target) {
                let legal: Vec<String> = sp
                    .legal_targets
                    .iter()
                    .filter_map(|t| match t {
                        PlayerTarget::Seat { seat_id } => codec
                            .short_of(&seat_id.to_string())
                            .map(|s| format!("\"{s}\"")),
                        PlayerTarget::Object { handle } => {
                            codec.short_of(handle.as_str()).map(|s| format!("\"{s}\""))
                        }
                        PlayerTarget::Stack { handle } => {
                            codec.short_of(handle.as_str()).map(|s| format!("\"{s}\""))
                        }
                    })
                    .collect();
                return format!(
                    "ILLEGAL TARGET for {short_src}: pass one of its verified legal target handles ONLY: [{}].",
                    legal.join(", ")
                );
            }
        } else {
            return format!(
                "{short_src} is NOT in can_cast right now (cannot be cast here). Choose a card from can_cast or pass."
            );
        }
    }
    if let PlayerAction::CastUntargeted { source, .. } = final_action {
        let short_src = codec.short_of(source.as_str()).unwrap_or(source.as_str());
        if let Some(sp) = affordances
            .castable_spells
            .iter()
            .find(|s| &s.source == source)
            && !sp.untargeted
        {
            return format!(
                "{short_src} requires a target; use kind \"cast-targeted\" with one of its targets from can_cast."
            );
        }
    }
    format!(
        "native engine rejected action ({raw_err}). Check can_cast/can_play_lands, target legality, and decisions[0].domain; pass if no legal play."
    )
}

/// Build a safe, minimal deterministic fallback command when all retries exhaust so a live
/// match can never soft-lock.
pub fn fallback_command(snapshot: &PlayerSnapshot) -> Option<PlayerCommand> {
    if snapshot.decisions.is_empty() {
        return Some(PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: snapshot.basis.clone(),
            action: PlayerAction::Pass {},
        });
    }
    let raw = serde_json::to_value(&snapshot.decisions[0]).ok()?;
    let decision = raw
        .get("decision")
        .or_else(|| raw.get("handle"))
        .and_then(Value::as_str)?;
    let domain = raw.get("domain")?;
    let domain_kind = domain.get("kind").and_then(Value::as_str).unwrap_or("");
    let response = match domain_kind {
        "options" | "option" => {
            let opt = domain
                .get("options")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(|o| o.get("handle"))
                .and_then(Value::as_str)?;
            json!({ "kind": "option", "option": opt })
        }
        "attackers" => json!({ "kind": "attackers", "assignments": [] }),
        "blockers" => json!({ "kind": "blockers", "assignments": [] }),
        "objects" => {
            let min = domain
                .get("minimum")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let objs: Vec<Value> = domain
                .get("candidates")
                .or_else(|| domain.get("objects"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .take(min)
                        .filter_map(|v| {
                            v.as_str()
                                .or_else(|| v.get("handle").and_then(Value::as_str))
                        })
                        .map(|s| Value::String(s.to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            json!({ "kind": "objects", "objects": objs })
        }
        "payment" | "optional-payment" => json!({ "kind": "decline" }),
        _ => return None,
    };
    let action_val = json!({
        "kind": "choose",
        "decision": decision,
        "response": response,
    });
    let action: PlayerAction = serde_json::from_value(action_val).ok()?;
    let cmd = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: snapshot.basis.clone(),
        action,
    };
    cmd.validate().ok()?;
    Some(cmd)
}

fn collect_candidate_targets(
    snapshot: &PlayerSnapshot,
    zone_kinds: &HashMap<&str, PlayerZoneKind>,
) -> Vec<PlayerTarget> {
    let mut targets = Vec::new();
    // Opponent seats first (burn / player-targeted spells).
    for p in &snapshot.players {
        if p.seat_id != snapshot.viewer_seat_id {
            targets.push(PlayerTarget::Seat { seat_id: p.seat_id });
        }
    }
    // Opponent battlefield objects, then own battlefield objects.
    for obj in &snapshot.visible_objects {
        if zone_kinds.get(obj.zone.as_str()).copied() == Some(PlayerZoneKind::Battlefield)
            && obj.controller_seat_id != Some(snapshot.viewer_seat_id)
        {
            targets.push(PlayerTarget::Object {
                handle: obj.handle.clone(),
            });
        }
    }
    for obj in &snapshot.visible_objects {
        if zone_kinds.get(obj.zone.as_str()).copied() == Some(PlayerZoneKind::Battlefield)
            && obj.controller_seat_id == Some(snapshot.viewer_seat_id)
        {
            targets.push(PlayerTarget::Object {
                handle: obj.handle.clone(),
            });
        }
    }
    // Stack entries (counterspells).
    for entry in &snapshot.stack {
        targets.push(PlayerTarget::Stack {
            handle: entry.handle.clone(),
        });
    }
    // Own seat last (e.g. Sign in Blood targeting self).
    targets.push(PlayerTarget::Seat {
        seat_id: snapshot.viewer_seat_id,
    });
    targets
}

fn current_mana_pool(snapshot: &PlayerSnapshot) -> BTreeMap<ManaColor, u32> {
    let mut pool = BTreeMap::new();
    if let Some(me) = snapshot
        .players
        .iter()
        .find(|p| p.seat_id == snapshot.viewer_seat_id)
    {
        for entry in &me.mana_pool {
            let amt: u32 = String::from(entry.amount.clone()).parse().unwrap_or(0);
            if amt > 0 {
                *pool.entry(entry.color).or_insert(0) += amt;
            }
        }
    }
    pool
}

fn object_mana_cost(identity: &PlayerObjectIdentity) -> ParsedManaCost {
    match identity {
        PlayerObjectIdentity::Known {
            characteristics, ..
        } => characteristics
            .mana_cost
            .as_deref()
            .map(ParsedManaCost::parse)
            .unwrap_or_default(),
        PlayerObjectIdentity::Concealed {} => ParsedManaCost::default(),
    }
}

fn infer_mana_color(identity: &PlayerObjectIdentity) -> Option<ManaColor> {
    let PlayerObjectIdentity::Known {
        characteristics, ..
    } = identity
    else {
        return None;
    };
    let name = characteristics.name.as_str();
    match name {
        "Plains" => return Some(ManaColor::White),
        "Island" => return Some(ManaColor::Blue),
        "Swamp" => return Some(ManaColor::Black),
        "Mountain" => return Some(ManaColor::Red),
        "Forest" => return Some(ManaColor::Green),
        "Wastes" => return Some(ManaColor::Colourless),
        _ => {}
    }
    let text = characteristics.rules_text.as_str();
    if text.contains("Add {W}") {
        Some(ManaColor::White)
    } else if text.contains("Add {U}") {
        Some(ManaColor::Blue)
    } else if text.contains("Add {B}") {
        Some(ManaColor::Black)
    } else if text.contains("Add {R}") {
        Some(ManaColor::Red)
    } else if text.contains("Add {G}") {
        Some(ManaColor::Green)
    } else if text.contains("Add {C}") {
        Some(ManaColor::Colourless)
    } else {
        None
    }
}

/// Solve an optimal mana tap sequence (lands before mana creatures) + `PlayerPayment`
/// to pay `cost` from `snapshot`'s current mana pool and `mana_sources`.
pub fn solve_mana_for_cost(
    cost: &ParsedManaCost,
    snapshot: &PlayerSnapshot,
    mana_sources: &[ManaSourceInfo],
) -> Option<(Vec<PlayerAction>, PlayerPayment)> {
    let mut pool = current_mana_pool(snapshot);
    let mut spend: BTreeMap<ManaColor, u32> = BTreeMap::new();
    let mut used_sources = vec![false; mana_sources.len()];
    let mut taps = Vec::new();

    // Order indices so non-creature sources (lands) are always tapped before creatures.
    let mut ordered_indices: Vec<usize> = (0..mana_sources.len()).collect();
    ordered_indices.sort_by_key(|&idx| mana_sources[idx].is_creature);

    // Step 1: satisfy colored requirements from pool, then tapping matching sources.
    for (&color, &needed) in &cost.colored {
        let mut remaining = needed;
        if let Some(avail) = pool.get_mut(&color) {
            let take = (*avail).min(remaining);
            *avail -= take;
            remaining -= take;
            if take > 0 {
                *spend.entry(color).or_insert(0) += take;
            }
        }
        while remaining > 0 {
            let idx = ordered_indices
                .iter()
                .copied()
                .find(|&i| !used_sources[i] && mana_sources[i].color == Some(color))?;
            used_sources[idx] = true;
            taps.push(PlayerAction::ActivateMana {
                source: mana_sources[idx].source.clone(),
                ability: mana_sources[idx].ability.clone(),
            });
            *spend.entry(color).or_insert(0) += 1;
            remaining -= 1;
        }
    }

    // Step 2: satisfy generic requirement from remaining pool, then tapping remaining sources.
    let mut generic_left = cost.generic;
    for (&color, avail) in &mut pool {
        if generic_left == 0 {
            break;
        }
        let take = (*avail).min(generic_left);
        *avail -= take;
        generic_left -= take;
        if take > 0 {
            *spend.entry(color).or_insert(0) += take;
        }
    }
    while generic_left > 0 {
        let idx = ordered_indices
            .iter()
            .copied()
            .find(|&i| !used_sources[i] && mana_sources[i].color.is_some())?;
        let color = mana_sources[idx].color?;
        used_sources[idx] = true;
        taps.push(PlayerAction::ActivateMana {
            source: mana_sources[idx].source.clone(),
            ability: mana_sources[idx].ability.clone(),
        });
        *spend.entry(color).or_insert(0) += 1;
        generic_left -= 1;
    }

    // `prepare_cast_batch` supports at most 8 mana activations.
    if taps.len() > 8 {
        return None;
    }

    let spend_vec: Option<Vec<ManaSpend>> = spend
        .into_iter()
        .filter(|(_, amt)| *amt > 0)
        .map(|(color, amt)| {
            Some(ManaSpend {
                color,
                amount: RulesCount::try_from(amt.to_string()).ok()?,
            })
        })
        .collect();
    let spend_vec = spend_vec?;

    let payment = PlayerPayment {
        from_lots: BTreeMap::new(),
        graveyard_permission: None,
        use_granted_flashback: false,
        exile_cast_type: None,
        alternative_cost_source: None,
        spend: spend_vec,
        restricted_mana: Vec::new(),
    };
    Some((taps, payment))
}

fn build_payment_from_pool(
    cost: &ParsedManaCost,
    pool: &BTreeMap<ManaColor, u32>,
) -> Option<PlayerPayment> {
    let mut rem_pool = pool.clone();
    let mut spend: BTreeMap<ManaColor, u32> = BTreeMap::new();

    for (&color, &needed) in &cost.colored {
        let avail = rem_pool.get_mut(&color)?;
        if *avail < needed {
            return None;
        }
        *avail -= needed;
        *spend.entry(color).or_insert(0) += needed;
    }

    let mut generic_left = cost.generic;
    for (&color, avail) in &mut rem_pool {
        if generic_left == 0 {
            break;
        }
        let take = (*avail).min(generic_left);
        *avail -= take;
        generic_left -= take;
        if take > 0 {
            *spend.entry(color).or_insert(0) += take;
        }
    }
    if generic_left > 0 {
        return None;
    }

    let spend_vec: Option<Vec<ManaSpend>> = spend
        .into_iter()
        .filter(|(_, amt)| *amt > 0)
        .map(|(color, amt)| {
            Some(ManaSpend {
                color,
                amount: RulesCount::try_from(amt.to_string()).ok()?,
            })
        })
        .collect();
    let spend_vec = spend_vec?;
    Some(PlayerPayment {
        from_lots: BTreeMap::new(),
        graveyard_permission: None,
        use_granted_flashback: false,
        exile_cast_type: None,
        alternative_cost_source: None,
        spend: spend_vec,
        restricted_mana: Vec::new(),
    })
}

const fn mana_color_name(color: ManaColor) -> &'static str {
    match color {
        ManaColor::White => "white",
        ManaColor::Blue => "blue",
        ManaColor::Black => "black",
        ManaColor::Red => "red",
        ManaColor::Green => "green",
        ManaColor::Colourless => "colourless",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_mana_costs() {
        let c = ParsedManaCost::parse("{2}{R}{R}");
        assert_eq!(c.generic, 2);
        assert_eq!(c.colored.get(&ManaColor::Red), Some(&2));
        assert_eq!(c.total(), 4);

        let g = ParsedManaCost::parse("{G}");
        assert_eq!(g.generic, 0);
        assert_eq!(g.colored.get(&ManaColor::Green), Some(&1));
        assert_eq!(g.total(), 1);
    }
}
