//! Bind literal private operations to existing native input types and actual receipts.
use super::*;

#[cfg(test)]
#[path = "cast_join_tests.rs"]
mod cast_join_tests;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LegendReplyEvent {
    started_revision: u64,
    pass: u64,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LegendReplyConflict {
    event: LegendReplyEvent,
    controller: u32,
    ordinal: u64,
}
use crate::test_support::receipt_joins::TokenCreationSelector as TokenCreationJoin;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BirthDiscoveryJoin {
    source_object_ref: String,
    triggered_ability_id: u64,
    related_token_after_ref: String,
    require_unique: bool,
}

pub(super) fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing string {key}"))
}
pub(super) fn rows<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing array {key}"))
}
pub(super) fn integer(value: &Value) -> Result<u64, String> {
    let text = value.as_str().ok_or("expected decimal u64 string")?;
    let result: u64 = text
        .parse()
        .map_err(|error| format!("invalid u64: {error}"))?;
    if result.to_string() != text {
        return Err("noncanonical decimal u64 string".into());
    }
    Ok(result)
}
pub(super) fn quantity(value: &Value) -> Result<spellwake_contracts::RulesInteger, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

#[derive(Default)]
pub(super) struct Joins {
    objects: BTreeMap<String, ObjectId>,
    instances: BTreeMap<String, TriggerInstanceId>,
    entries: BTreeMap<String, crate::stack_window::StackEntryId>,
    opening_draws: BTreeMap<(usize, SeatId), Vec<ObjectId>>,
}
impl Joins {
    pub(super) fn entry(&self, name: &str) -> Option<crate::stack_window::StackEntryId> {
        self.entries.get(name).copied()
    }
    pub(super) fn bind_entry(
        &mut self,
        name: &str,
        entry: crate::stack_window::StackEntryId,
    ) -> Result<(), String> {
        if self.entries.contains_key(name) {
            return Err("duplicate actual entry join".into());
        }
        self.entries.insert(name.into(), entry);
        Ok(())
    }
    pub(super) fn require_token_bindings(&self, fixture: &Value) -> Result<(), String> {
        for declaration in rows(fixture, "bindings")? {
            let join = &declaration["join"];
            let name = string(declaration, "name")?;
            let bound = if join.get("token_created_by_entry_ref").is_some() {
                self.objects.contains_key(name)
            } else if join.get("related_token_after_ref").is_some() {
                self.instances.contains_key(name)
            } else {
                continue;
            };
            if !bound {
                return Err(format!("unbound exact token receipt {name}"));
            }
        }
        Ok(())
    }
    pub(super) fn target(
        &self,
        value: &Value,
        fixture: &Value,
        execution: &EffectWindow,
    ) -> Result<TargetRef, String> {
        self.target_with_missing_entry_error(value, fixture, execution, "unbound actual entry")
    }
    /// Shared lowering with each caller's established missing-entry diagnostic.
    pub(super) fn target_with_missing_entry_error(
        &self,
        value: &Value,
        fixture: &Value,
        execution: &EffectWindow,
        missing_entry_error: &'static str,
    ) -> Result<TargetRef, String> {
        if value.get("player").is_some() {
            if value.as_object().map(serde_json::Map::len) != Some(1) {
                return Err("Player target requires exactly one player field".into());
            }
            // Bind recognized aliases even when absent from this game. The
            // actual native target gate owns participant and target legality.
            return packet::seat(string(value, "player")?).map(TargetRef::Player);
        }
        if let Some(name) = value["entry_ref"].as_str() {
            return Ok(TargetRef::Stack(
                *self.entries.get(name).ok_or(missing_entry_error)?,
            ));
        }
        Ok(TargetRef::Object(self.object(value, fixture, execution)?))
    }
    pub(super) fn observe_root(
        &mut self,
        ordinal: usize,
        action: Option<&Value>,
        candidate: &NativeCandidate,
    ) -> Result<(), String> {
        if let Some(name) = action.and_then(|value| value["bind_entry"].as_str()) {
            let entry = candidate
                .mana_payment()
                .map(|value| value.source)
                .or_else(|| candidate.ability_activation().map(|value| value.entry))
                .ok_or("no actual receipt for entry binding")?;
            self.bind_entry(name, entry)?;
        }
        for event in &candidate.draw_journal().events {
            if let crate::draw::DrawEvent::Drawn { player, after, .. } = event {
                self.opening_draws
                    .entry((ordinal, *player))
                    .or_default()
                    .push(after.id());
            }
        }
        Ok(())
    }
    pub(super) fn seed_from_candidate(
        fixture: &Value,
        candidate: &NativeCandidate,
    ) -> Result<Self, String> {
        let mut joins = Self::seed(fixture, candidate.state().execution())?;
        for binding in rows(fixture, "bindings")? {
            let Some(card) = binding["join"]["card_id"].as_u64() else {
                continue;
            };
            if let Some(before) =
                candidate
                    .draw_journal()
                    .events
                    .iter()
                    .find_map(|event| match event {
                        crate::draw::DrawEvent::Drawn { before, .. }
                            if before.card_id() == Some(CardId(card)) =>
                        {
                            Some(before)
                        }
                        _ => None,
                    })
            {
                joins
                    .objects
                    .insert(string(binding, "name")?.into(), before.id());
            }
        }
        joins.observe_root(0, None, candidate)?;
        Ok(joins)
    }
    pub(super) fn bind_object(&mut self, name: &str, object: ObjectId) -> Result<(), String> {
        if self.objects.insert(name.into(), object).is_some() {
            return Err("duplicate object receipt binding".into());
        }
        Ok(())
    }
    pub(super) fn seed(fixture: &Value, execution: &EffectWindow) -> Result<Self, String> {
        let mut result = Self::default();
        for binding in rows(fixture, "bindings")? {
            if let Some(card) = binding["join"]["card_id"].as_u64() {
                let object = execution
                    .window()
                    .cards()
                    .current_identity(CardId(card))
                    .ok_or_else(|| format!("missing seed CardId {card}"))?;
                if result
                    .objects
                    .insert(string(binding, "name")?.into(), object)
                    .is_some()
                {
                    return Err("duplicate seed join".into());
                }
            }
        }
        Ok(result)
    }
    pub(super) fn object(
        &self,
        reference: &Value,
        fixture: &Value,
        execution: &EffectWindow,
    ) -> Result<ObjectId, String> {
        if let Some(join) = reference.get("actual_draw_from_latest_opening_batch") {
            if join["ordinal_basis"] != "one_based" {
                return Err("unsupported opening draw ordinal basis".into());
            }
            let ordinal = usize::try_from(join["ordinal"].as_u64().ok_or("missing draw ordinal")?)
                .map_err(|error| error.to_string())?
                .checked_sub(1)
                .ok_or("draw ordinal starts at1")?;
            let action = usize::try_from(
                join["capture_from_actual_draw_journal_for_action_ordinal"]
                    .as_u64()
                    .ok_or("missing actual draw action join")?,
            )
            .map_err(|error| error.to_string())?;
            return self
                .opening_draws
                .get(&(action, packet::seat(string(join, "player")?)?))
                .and_then(|values| values.get(ordinal))
                .copied()
                .ok_or("missing actual DrawJournal receipt binding".into());
        }
        if let Some(name) = reference["captured_object"].as_str() {
            return self
                .objects
                .get(name)
                .copied()
                .ok_or_else(|| format!("unbound object {name}"));
        }
        let name = string(reference, "current_owned_action_alias")?;
        let found: Vec<_> = rows(&fixture["seed"], "cards")?
            .iter()
            .filter(|card| card["alias"] == name)
            .collect();
        let [card] = found.as_slice() else {
            return Err(format!("nonunique action alias {name}"));
        };
        let card = card["card_id"].as_u64().ok_or("invalid physical CardId")?;
        execution
            .window()
            .cards()
            .current_identity(CardId(card))
            .ok_or_else(|| format!("missing current physical CardId {card}"))
    }
    fn observe_token_creations(
        &mut self,
        fixture: &Value,
        append: &crate::occurrence::RulesJournal,
    ) -> Result<(), String> {
        for declaration in rows(fixture, "bindings")? {
            let join = &declaration["join"];
            if join.get("token_created_by_entry_ref").is_some() {
                let selector: TokenCreationJoin =
                    serde_json::from_value(join.clone()).map_err(|error| error.to_string())?;
                if declaration["when"] != "matching_actual_token_created" {
                    return Err("unsupported token birth receipt binding".into());
                }
                let instruction = selector.instruction;
                let ordinal = selector.birth_ordinal;
                let name = string(declaration, "name")?;
                if self.objects.contains_key(name) {
                    continue;
                }
                let Some(entry) = self.entries.get(&selector.token_created_by_entry_ref) else {
                    continue;
                };
                if let Some(object) = crate::test_support::receipt_joins::token_creation_at(
                    append,
                    *entry,
                    instruction,
                    ordinal,
                )? {
                    self.objects.insert(name.into(), object);
                }
                continue;
            }
        }
        Ok(())
    }
    pub(super) fn observe(
        &mut self,
        fixture: &Value,
        append: &crate::occurrence::RulesJournal,
    ) -> Result<(), String> {
        self.observe_token_creations(fixture, append)?;
        for declaration in rows(fixture, "bindings")? {
            let join = &declaration["join"];
            if join.get("token_created_by_entry_ref").is_some() {
                continue;
            }
            if join.get("related_token_after_ref").is_some() {
                let selector: BirthDiscoveryJoin =
                    serde_json::from_value(join.clone()).map_err(|error| error.to_string())?;
                if !selector.require_unique
                    || declaration["when"] != "first_matching_actual_birth_discovery_in_this_recipe"
                {
                    return Err("unsupported token birth discovery binding".into());
                }
                let name = string(declaration, "name")?;
                if self.instances.contains_key(name) {
                    continue;
                }
                let Some(source) = self.objects.get(&selector.source_object_ref) else {
                    continue;
                };
                let Some(related) = self.objects.get(&selector.related_token_after_ref) else {
                    continue;
                };
                if let Some(instance) = crate::test_support::receipt_joins::birth_discovery(
                    append,
                    *source,
                    selector.triggered_ability_id,
                    *related,
                )? {
                    self.instances.insert(name.into(), instance);
                }
                continue;
            }
            let Some(source_name) = join["source_object_ref"].as_str() else {
                continue;
            };
            let name = string(declaration, "name")?;
            let source = *self
                .objects
                .get(source_name)
                .ok_or("unknown discovery source join")?;
            let ability = join["triggered_ability_id"]
                .as_u64()
                .ok_or("missing triggered ability ID")?;
            let related = join["related_object_ref"]
                .as_str()
                .map(|name| {
                    self.objects
                        .get(name)
                        .copied()
                        .ok_or("unknown related join")
                })
                .transpose()?;
            let found = crate::test_support::receipt_joins::discovery(
                append,
                source,
                ability,
                &format!("nonunique discovery join {name}"),
                |relation| {
                    let actual_related = match relation {
                        TriggerRelated::GraveyardReturnPaid { .. }
                        | TriggerRelated::ReflexiveDiscardDamage { .. }
                        | TriggerRelated::CombatDamageToPlaneswalker { .. }
                        | TriggerRelated::SubtypeCombatDamage { .. }
                        | TriggerRelated::AttachedCreatureDamaged { .. }
                        | TriggerRelated::DamageToOpponent { .. }
                        | TriggerRelated::DamageDealtToPermanent { .. }
                        | TriggerRelated::DamageReceived { .. }
                        | TriggerRelated::CombatDamageToPlayer { .. }
                        | TriggerRelated::ManaSource(_)
                        | TriggerRelated::AttackGroup { .. }
                        | TriggerRelated::CardDrawn { .. }
                        | TriggerRelated::PlayerLost(_)
                        | TriggerRelated::SagaChapter { .. }
                        | TriggerRelated::CounterSource(_)
                        | TriggerRelated::Combatant(_)
                        | TriggerRelated::DeclaredSubtypeAttackers(_)
                        | TriggerRelated::AttackedWithTotalPower(_)
                        | TriggerRelated::AttackedAlone(_) => {
                            return Err(
                                "historical qualifier excludes combat keyword triggers".into()
                            );
                        }
                        TriggerRelated::Phased(_) | TriggerRelated::Mutated(_) => {
                            return Err("historical qualifier excludes phasing triggers".into());
                        }
                        TriggerRelated::LibraryAction(_) => {
                            return Err(
                                "historical qualifier excludes library action triggers".into()
                            );
                        }
                        TriggerRelated::SpellCast(value) => Some(value.spell().object.id()),
                        TriggerRelated::TokenCreated { .. } => return Ok(false),
                        TriggerRelated::ZoneChange { before, .. }
                        | TriggerRelated::Discard { before, .. } => Some(before.object.id()),
                        TriggerRelated::StatusChange(change) => Some(change.after.id()),
                        TriggerRelated::AbilityForCopy(_)
                        | TriggerRelated::SpellCopied(_)
                        | TriggerRelated::TargetingEntry(_)
                        | TriggerRelated::TargetedPermanent(_)
                        | TriggerRelated::DamageSource(_)
                        | TriggerRelated::StateTriggerSource(_)
                        | TriggerRelated::FinalSagaChapter { .. }
                        | TriggerRelated::Step { .. }
                        | TriggerRelated::ReflexiveTrigger { .. }
                        | TriggerRelated::LifeChanged(_) => None,
                        TriggerRelated::MiracleRevealed(object)
                        | TriggerRelated::LeftGame(object) => Some(*object),
                    };
                    Ok(actual_related == related)
                },
            )?;
            if let Some(instance) = found
                && self.instances.insert(name.into(), instance).is_some()
            {
                return Err(format!("nonunique discovery join {name}"));
            }
        }
        Ok(())
    }
}

pub(super) fn reply(
    state: &GameState,
    action: &Value,
    fixture: &Value,
    joins: &Joins,
) -> Result<GameInput, String> {
    let frame = state
        .pending_decision()
        .ok_or("reply without actual pending decision")?;
    let actor = action["actor"]
        .as_str()
        .map(packet::seat)
        .transpose()?
        .unwrap_or(frame.frame().chooser());
    let kind = action["reply_kind"]
        .as_str()
        .or_else(|| action["kind"].as_str())
        .or_else(|| action.get("copy_choice").map(|_| "CopyTargets"))
        .or_else(|| {
            action
                .get("order_trigger_instance_refs")
                .map(|_| "OrderTriggers")
        })
        .ok_or("reply without selected kind")?;
    let items = action
        .get("items")
        .or_else(|| action.get("order_trigger_instance_refs"))
        .map(|value| {
            value
                .as_array()
                .map(Vec::as_slice)
                .ok_or("reply items must be array")
        })
        .transpose()?
        .unwrap_or(&[]);
    let object = |item: &Value| joins.object(item, fixture, &state.execution);
    let choice = match kind {
        "TimestampOrder" => DecisionReply::TimestampOrder(
            items
                .iter()
                .map(program::token_member)
                .collect::<Result<_, _>>()?,
        ),
        "CopyTargets" => {
            let choice = &action["copy_choice"];
            DecisionReply::CopyTargets(match string(choice, "tag")? {
                "Retain" => crate::effects::CopyTargetChoice::Retain,
                "Change" => crate::effects::CopyTargetChoice::Change(joins.target(
                    &choice["target"],
                    fixture,
                    &state.execution,
                )?),
                _ => return Err("unsupported copy target reply".into()),
            })
        }
        "Mulligan" => {
            let [item] = items else {
                return Err("Mulligan requires one literal choice".into());
            };
            DecisionReply::Mulligan(match item.as_str() {
                Some("TakeMulligan") => MulliganChoice::Mulligan,
                Some("Keep") => MulliganChoice::Keep,
                _ => return Err("unsupported mulligan choice".into()),
            })
        }
        "Bottom" => DecisionReply::Bottom(items.iter().map(object).collect::<Result<_, _>>()?),
        "Discard" => DecisionReply::Discard(items.iter().map(object).collect::<Result<_, _>>()?),
        "ManaColor" => {
            let [item] = items else {
                return Err("ManaColor requires one literal color".into());
            };
            DecisionReply::ManaColor(program::mana(item.as_str().ok_or("nonstring mana color")?)?)
        }
        "KeepLegend" => {
            if action.get("items").is_some() {
                return Err("KeepLegend requires conflict and keep, not flat items".into());
            }
            let id: LegendReplyConflict = serde_json::from_value(action["conflict"].clone())
                .map_err(|e| format!("invalid legend conflict: {e}"))?;
            DecisionReply::KeepLegend {
                conflict: crate::checks::LegendConflictId {
                    event: crate::checks::CheckEventId {
                        started_revision: id.event.started_revision,
                        pass: id.event.pass,
                    },
                    controller: SeatId(id.controller),
                    ordinal: id.ordinal,
                },
                keep: object(&action["keep"])?,
            }
        }
        "MutateOnTop" => DecisionReply::MutateOnTop(
            action["on_top"]
                .as_bool()
                .ok_or("missing mutation placement")?,
        ),
        "ReturnCommanders" => {
            DecisionReply::ReturnCommanders(items.iter().map(object).collect::<Result<_, _>>()?)
        }
        "OrderGraveyard" => {
            DecisionReply::OrderGraveyard(items.iter().map(object).collect::<Result<_, _>>()?)
        }
        "TriggerTarget" => DecisionReply::TriggerTarget(joins.target(
            &action["target"],
            fixture,
            &state.execution,
        )?),
        "OrderTriggers" => DecisionReply::OrderTriggers(
            items
                .iter()
                .map(|item| {
                    let name = item.as_str().ok_or("nonstring instance alias")?;
                    joins
                        .instances
                        .get(name)
                        .copied()
                        .ok_or_else(|| format!("unbound instance {name}"))
                })
                .collect::<Result<_, _>>()?,
        ),
        "DeclareAttackers" | "DeclareBandedAttackers" => {
            let attackers = items
                .iter()
                .map(|row| {
                    Ok(AttackDeclaration {
                        attacker: object(&row["attacker"])?,
                        defender: packet::seat(string(row, "defender")?)?,
                        defending_planeswalker: None,
                    })
                })
                .collect::<Result<_, String>>()?;
            if kind == "DeclareAttackers" {
                DecisionReply::DeclareAttackers(attackers)
            } else {
                let bands = action["bands"]
                    .as_array()
                    .ok_or("bands must be an array")?
                    .iter()
                    .map(|band| {
                        band.as_array()
                            .ok_or("a band must be an array".to_string())?
                            .iter()
                            .map(object)
                            .collect::<Result<Vec<_>, String>>()
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                DecisionReply::DeclareBandedAttackers { attackers, bands }
            }
        }
        "DeclareBlockers" => DecisionReply::DeclareBlockers(
            items
                .iter()
                .map(|row| {
                    Ok(BlockDeclaration {
                        blocker: object(&row["blocker"])?,
                        attacker: object(&row["attacker"])?,
                    })
                })
                .collect::<Result<_, String>>()?,
        ),
        _ => return Err(format!("unsupported private root reply {kind}")),
    };
    Ok(GameInput {
        basis: state.basis.clone(),
        actor,
        command: GameCommand::Reply {
            decision: frame.id(),
            choice,
        },
    })
}
pub(super) fn command(
    state: &GameState,
    action: &Value,
    fixture: &Value,
    joins: &Joins,
) -> Result<GameInput, String> {
    let actor = packet::seat(string(action, "actor")?)?;
    let command = match string(action, "op")? {
        "Pass" | "pass" => GameCommand::Pass,
        "activate_mana" => {
            if action.get("ability_id").is_some() {
                return Err("obsolete numeric mana selection field".into());
            }
            GameCommand::ActivateMana {
                source: joins.object(&action["source"], fixture, &state.execution)?,
                ability: crate::test_support::mana_ref::decode(action["ability"].clone())?,
            }
        }
        "cast" => {
            let spell = joins.object(&action["source"], fixture, &state.execution)?;
            let payment = ManaPayment {
                from_lots: Default::default(),
                spend: rows(action, "payment")?
                    .iter()
                    .map(|row| {
                        Ok(ManaSpend {
                            mana_type: program::mana(string(row, "type")?)?,
                            amount: quantity(&row["amount"])?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
            };
            if action["target"].is_null() {
                GameCommand::CastUntargeted {
                    spell,
                    payment: Some(payment),
                }
            } else {
                GameCommand::CastPaid {
                    spell,
                    target: joins.target(&action["target"], fixture, &state.execution)?,
                    payment,
                }
            }
        }
        op => return Err(format!("unsupported private root command {op}")),
    };
    Ok(GameInput {
        basis: state.basis.clone(),
        actor,
        command,
    })
}
