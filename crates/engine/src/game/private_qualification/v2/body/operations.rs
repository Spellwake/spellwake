//! Existing owner operations used by the finite private recipes.
use super::*;

impl Caller<'_> {
    pub(super) fn cast(&mut self, action: &Value) -> Result<(), String> {
        self.ready()?;
        let actor = packet::seat(string(action, "actor")?)?;
        let source = if let Some(alias) = action["source"].as_str() {
            json!({"current_owned_action_alias":alias})
        } else {
            action["source"].clone()
        };
        let source = self.joins.object(&source, self.fixture, &self.state)?;
        let target = (!action["target"].is_null())
            .then(|| self.target(&action["target"]))
            .transpose()?;
        let payment = Self::payment(&action["payment"])?;
        self.invoke_with_input(
            action.clone(),
            projection::QualificationCall::EffectCall(projection::EffectCall::CastOnTurn {
                expected: self.state.revision(),
                actor,
                spell: source,
                target,
                payment: Some(&payment),
                turn: None,
            }),
            |s| {
                EffectAttempt::Cast(Box::new(s.cast_on_turn(
                    s.revision(),
                    actor,
                    source,
                    target,
                    Some(payment.clone()),
                    None,
                )))
            },
            true,
        )?;
        let Some((_, EffectAttempt::Cast(result), _)) = self.attempts.last() else {
            return Err("missing actual cast receipt".into());
        };
        let entry = result
            .as_ref()
            .as_ref()
            .map_err(|_| "cast rejected")?
            .result()
            .source;
        self.bind_entry(action["bind_entry"].as_str(), entry)
    }
    pub(super) fn pass(&mut self, actor: &str) -> Result<(), String> {
        self.ready()?;
        let actor = packet::seat(actor)?;
        self.invoke_with_input(json!({"owner":"EffectWindow","method":"pass","actor":actor.0,"metadata_only_no_append":true}),projection::QualificationCall::EffectCall(projection::EffectCall::Pass{expected:self.state.revision(),actor}),|s|EffectAttempt::Preparation(Box::new(s.pass(s.revision(),actor))),true)
    }
    pub(super) fn resolve(&mut self, action: &Value) -> Result<(), String> {
        if let Some(name) = action["entry_ref"].as_str()
            && self.state.window().entries().last().map(|entry| entry.id())
                != self.joins.entry(name)
        {
            return Err("bound entry is not actual current top".into());
        }
        for actor in rows(action, "pass_order")? {
            self.pass(actor.as_str().ok_or("nonstring pass actor")?)?;
        }
        self.invoke_with_input(json!({"owner":"EffectWindow","method":"begin_resolution","target_gate":"actual-owned-gate","return_at":"first_actual_Pending_or_Complete"}),projection::QualificationCall::EffectCall(projection::EffectCall::BeginResolution{expected:self.state.revision()}),|s|EffectAttempt::Progress(Box::new(s.begin_resolution(s.revision()))),true)
    }
    pub(super) fn maybe_followon_checks(&mut self, stop: bool) -> Result<(), String> {
        if !stop
            && matches!(
                self.state.window().phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            )
        {
            self.checks("authored helper permits follow-on checks")?;
        }
        Ok(())
    }
    pub(super) fn copy_reply(&mut self, action: &Value) -> Result<(), String> {
        let frame = self
            .frame
            .clone()
            .ok_or("copy reply has no actual Pending frame")?;
        if !matches!(frame, EffectDecision::CopyTargets(_)) {
            return Err("actual Pending frame is not CopyTargets".into());
        }
        let actor = packet::seat(string(action, "actor")?)?;
        let choice = match string(action, "choice")? {
            "Retain" => crate::effects::CopyTargetChoice::Retain,
            "Change" => crate::effects::CopyTargetChoice::Change(self.target(&action["target"])?),
            _ => return Err("unsupported exact copy reply".into()),
        };
        self.invoke_with_input(
            action.clone(),
            projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
                expected: self.state.revision(),
                frame: &frame,
                actor,
                reply: &DecisionReply::CopyTargets(choice),
                turn: None,
            }),
            |s| {
                EffectAttempt::Progress(Box::new(s.resume_rule(
                    s.revision(),
                    &frame,
                    actor,
                    DecisionReply::CopyTargets(choice),
                    None,
                )))
            },
            true,
        )?;
        if let Some(name) = action["bind_created_copy"].as_str() {
            let ids = self
                .last_append
                .records
                .iter()
                .flat_map(|record| match record {
                    RulesRecord::Event(event) => event.occurrences.as_slice(),
                    _ => &[],
                })
                .filter_map(|occurrence| {
                    if let RulesOccurrence::CopyCreated { entry, .. } = occurrence.value {
                        Some(entry)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let [entry] = ids.as_slice() else {
                return Err("copy receipt binding requires exactly one actual CopyCreated".into());
            };
            self.bind_entry(Some(name), *entry)?;
        }
        Ok(())
    }
    pub(super) fn movement(&mut self, action: &Value) -> Result<(), String> {
        let object = self
            .joins
            .object(&action["object"], self.fixture, &self.state)?;
        let destination = match string(action, "destination")? {
            "Hand" => ObjectDestination::Hand,
            "Graveyard" => ObjectDestination::Graveyard,
            _ => return Err("unsupported disclosed movement destination".into()),
        };
        self.invoke_with_input(
            action.clone(),
            projection::QualificationCall::EffectCall(
                projection::EffectCall::QualificationZoneMoves {
                    expected: self.state.revision(),
                    moves: &[(object, destination)],
                },
            ),
            |s| {
                EffectAttempt::Moves(Box::new(
                    s.qualification_zone_moves(s.revision(), &[(object, destination)]),
                ))
            },
            true,
        )?;
        if let Some(name) = action["bind_after"].as_str() {
            let Some((_, EffectAttempt::Moves(result), _)) = self.attempts.last() else {
                return Err("no movement receipt for after binding".into());
            };
            let movement = result
                .as_ref()
                .as_ref()
                .map_err(|_| "movement rejected")?
                .result()
                .first()
                .ok_or("no actual movement")?;
            self.joins.bind_object(name, movement.after.id())?;
        }
        Ok(())
    }
    pub(super) fn preparation(&mut self, action: &Value) -> Result<(), String> {
        let counters = action
            .get("pt_counters")
            .map(|_| rows(action, "pt_counters"))
            .transpose()?
            .unwrap_or(&[])
            .iter()
            .map(|row| {
                Ok((
                    self.joins
                        .object(&row["object"], self.fixture, &self.state)?,
                    PowerToughnessCounters {
                        plus_one: quantity(&row["plus_one"])?,
                        minus_one: quantity(&row["minus_one"])?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let marked = action
            .get("marked_damage")
            .map(|_| rows(action, "marked_damage"))
            .transpose()?
            .unwrap_or(&[])
            .iter()
            .map(|row| {
                Ok((
                    self.joins
                        .object(&row["object"], self.fixture, &self.state)?,
                    quantity(&row["amount"])?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.invoke_with_input(
            action.clone(),
            projection::QualificationCall::EffectCall(
                projection::EffectCall::QualificationPrepareCreatureFields {
                    counters: &counters,
                    marked: &marked,
                },
            ),
            |s| {
                EffectAttempt::Preparation(Box::new(
                    s.qualification_prepare_creature_fields(&counters, &marked),
                ))
            },
            true,
        )?;
        let life = action
            .get("life_totals")
            .map(|value| {
                value
                    .as_object()
                    .ok_or("private life totals must be explicit object")
            })
            .transpose()?
            .into_iter()
            .flat_map(|values| values.iter())
            .map(|(player, total)| Ok((packet::seat(player)?, quantity(total)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let combat = action
            .get("combat")
            .map(|value| -> Result<(ObjectId, SeatId), String> {
                if value["blocked"] != false || value["control_epoch"] != "actual_current_epoch" {
                    return Err("unsupported prepared combat membership".into());
                }
                Ok((
                    self.joins
                        .object(&value["attacker"], self.fixture, &self.state)?,
                    packet::seat(string(value, "defender")?)?,
                ))
            })
            .transpose()?;
        if !life.is_empty() || combat.is_some() {
            self.invoke_with_input(
                json!({"owner":"EffectWindow","private_context_preparation":action}),
                projection::QualificationCall::EffectCall(
                    projection::EffectCall::QualificationPrepareContextFields {
                        life: &life,
                        combat,
                    },
                ),
                |s| {
                    EffectAttempt::Preparation(Box::new(
                        s.qualification_prepare_context_fields(&life, combat),
                    ))
                },
                true,
            )?;
        }
        Ok(())
    }
    pub(super) fn damage_application(
        &self,
        reference: &Value,
    ) -> Result<DamageApplication, String> {
        let Some(EffectDecision::Damage(DamageDecision::Order(frame))) = &self.frame else {
            return Err("no actual damage order frame".into());
        };
        let DamageOrigin::Resolution { entry, .. } = *frame.origin() else {
            return Err("damage effect join needs actual resolution origin".into());
        };
        let instruction = usize::try_from(
            reference["created_effect_at_instruction"]
                .as_u64()
                .ok_or("missing authored instruction join")?,
        )
        .map_err(|error| error.to_string())?;
        let effect = self
            .creations
            .get(&(entry, instruction))
            .ok_or("no actual effect creation receipt for instruction")?;
        let values = frame
            .applications()
            .iter()
            .filter(|application| application.effect == *effect)
            .copied()
            .collect::<Vec<_>>();
        let [application] = values.as_slice() else {
            return Err(
                "effect creation join does not identify one actual offered application".into(),
            );
        };
        Ok(*application)
    }
}
