//! Selected input operations compose existing owners, without interpreting bodies.
use super::*;

impl Caller<'_> {
    pub(super) fn action(&mut self, action: &Value) -> Result<(), String> {
        if self.fixture["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("S2T-"))
        {
            return resolution_values::action(self, action);
        }
        match string(action, "op")? {
            "separate_stack_window_branch" => {
                let (view, witness) = raw_window::resolution_branch(
                    self.original_seed,
                    self.fixture,
                    self.aliases,
                    action,
                )?;
                let complete = view["status"] == "completed_recipe";
                self.records.push(json!({"action_ordinal":self.ordinal,"operation":action,"independent_raw_branch":view,"first_executor_state":projection::effect(&self.state,self.aliases)?}));
                self.raw_branches.push(witness);
                if !complete {
                    return Err(
                        "independent raw branch failed; all actual raw Results retained".into(),
                    );
                }
                Ok(())
            }
            "trusted_prepare_cleanup_context" => {
                if action["iteration"] != 0
                    || packet::seat(string(action, "active")?)? != self.state.window().turn_owner()
                {
                    return Err("unsupported prepared cleanup context".into());
                }
                let result = crate::test_support::current_capture::turn_new(
                    TurnStart {
                        number: action["turn"].as_u64().ok_or("missing cleanup turn")?,
                        step: Step::Cleanup,
                    },
                    self.state.window().participants().len() == 2,
                );
                self.records.push(crate::test_support::capture_points::mark_value("actual prepared scheduler constructor result",crate::test_support::current_profile::SnapshotOwner::TurnScheduler(result.as_ref().map_err(|error|format!("actual scheduler constructor rejected: {error:?}"))?),self.aliases,json!({"action_ordinal":self.ordinal,"operation":action,"scheduler_result":match &result {Ok(value)=>json!({"tag":"Ok","actual_native_debug":format!("{value:?}"),"position":crate::test_support::damage_trace::turn_position_view(value.position())}),Err(error)=>projection::turn_error(error)},"scope":"explicit-prepared-calendar-no-retrospective-StepBegan"}))?);
                self.calendars.push((None, result.clone()));
                self.turn = Some(result.map_err(|error| {
                    format!("actual calendar construction rejected: {error:?}")
                })?);
                let active = self.state.window().turn_owner();
                self.invoke_with_input(json!({"owner":"EffectWindow","method":"enter_turn_step","role":"prepared-Cleanup-TurnWork-no-StepBegan"}),projection::QualificationCall::EffectCall(projection::EffectCall::EnterTurnStep{expected:self.state.revision(),active,priority:false,empty_mana:false}),|s|EffectAttempt::Unit(Box::new(s.enter_turn_step(s.revision(),active,false,false))),true)
            }
            "trusted_complete_same_cleanup_priority_handoff" => {
                if action["empty_stack_required"] != true
                    || !self.state.window().entries().is_empty()
                {
                    return Err("cleanup handoff requires literal empty stack".into());
                }
                let Some(RuleCompletion::Checks(completed)) = self.completion else {
                    return Err("cleanup handoff lacks actual completed check receipt".into());
                };
                let old = self
                    .turn
                    .clone()
                    .ok_or("cleanup handoff lacks actual calendar")?;
                let result = crate::test_support::current_capture::turn_work(
                    &old,
                    WorkCompleted::Cleanup {
                        priority_required: completed.performed() || completed.waiting_triggers(),
                    },
                );
                self.records.push(crate::test_support::capture_points::mark_value("actual same-cleanup scheduler source",crate::test_support::current_profile::SnapshotOwner::TurnScheduler(&old),self.aliases,json!({"action_ordinal":self.ordinal,"operation":action,"source_scheduler":format!("{old:?}"),"scheduler_result":match &result {Ok(value)=>json!({"tag":"Ok","actual_native_debug":format!("{value:?}"),"position":crate::test_support::damage_trace::turn_position_view(value.position())}),Err(error)=>projection::turn_error(error)}}))?);
                self.calendars.push((Some(old.clone()), result.clone()));
                let next = result
                    .map_err(|error| format!("actual cleanup handoff rejected: {error:?}"))?;
                let active = self.state.window().turn_owner();
                self.invoke_with_input(json!({"owner":"EffectWindow","method":"enter_calendar","role":"actual-same-Cleanup-priority-handoff"}),projection::QualificationCall::EffectCall(projection::EffectCall::EnterCalendar{expected:self.state.revision(),previous:Some(&old),next_turn:&next,active}),|s|EffectAttempt::Unit(Box::new(s.enter_calendar(s.revision(),Some(&old),&next,active))),true)?;
                self.turn = Some(next);
                Ok(())
            }
            "reply_exact_current_frame" => {
                if action["kind"] != "Discard" {
                    return Err("unsupported trusted exact rule reply".into());
                }
                let frame = self
                    .frame
                    .clone()
                    .ok_or("exact reply lacks actual Pending frame")?;
                let actor = packet::seat(string(action, "actor")?)?;
                let selected = rows(action, "items")?
                    .iter()
                    .map(|value| self.joins.object(value, self.fixture, &self.state))
                    .collect::<Result<Vec<_>, _>>()?;
                let turn = self.turn.clone();
                self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
                        expected: self.state.revision(),
                        frame: &frame,
                        actor,
                        reply: &DecisionReply::Discard(selected.clone()),
                        turn: turn.as_ref(),
                    }),
                    |s| {
                        EffectAttempt::Progress(Box::new(s.resume_rule(
                            s.revision(),
                            &frame,
                            actor,
                            DecisionReply::Discard(selected.clone()),
                            turn.as_ref(),
                        )))
                    },
                    true,
                )
            }
            "trusted_owned_batch_entry" => {
                let moves = rows(action, "moves")?
                    .iter()
                    .map(|value| {
                        Ok((
                            self.joins
                                .object(&value["card"], self.fixture, &self.state)?,
                            ObjectDestination::Battlefield {
                                controller: packet::seat(string(
                                    &value["destination"],
                                    "Battlefield",
                                )?)?,
                            },
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(
                        projection::EffectCall::QualificationZoneMoves {
                            expected: self.state.revision(),
                            moves: &moves,
                        },
                    ),
                    |s| {
                        EffectAttempt::Moves(Box::new(
                            s.qualification_zone_moves(s.revision(), &moves),
                        ))
                    },
                    true,
                )?;
                let Some((_, EffectAttempt::Moves(result), _)) = self.attempts.last() else {
                    return Err("missing batch movement receipt".into());
                };
                let movements = result
                    .as_ref()
                    .as_ref()
                    .map_err(|_| "batch rejected")?
                    .result();
                for row in rows(action, "moves")? {
                    let before = self.joins.object(&row["card"], self.fixture, &self.state)?;
                    let matches = movements
                        .iter()
                        .filter(|movement| movement.before.id() == before)
                        .collect::<Vec<_>>();
                    let [movement] = matches.as_slice() else {
                        return Err("batch row does not identify one actual before receipt".into());
                    };
                    self.joins
                        .bind_object(string(row, "bind_after")?, movement.after.id())?;
                }
                Ok(())
            }
            "trusted_prepare_actual_resolving_damage_origin" => {
                if action["instruction_index"] != 0
                    || action["target_assessment"] != "actual_legal_target_gate"
                {
                    return Err("unsupported private damage cursor boundary".into());
                }
                let mut cast = action.clone();
                cast["source"] = json!(string(action, "source_alias")?);
                self.cast(&cast)?;
                for actor in rows(action, "pass_order")? {
                    self.pass(actor.as_str().ok_or("invalid damage-origin pass actor")?)?;
                }
                let entry = self
                    .joins
                    .entry(string(action, "bind_entry")?)
                    .ok_or("missing actual damage-origin cast receipt")?;
                self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(
                        projection::EffectCall::QualificationPrepareResolutionCursor {
                            entry,
                            instruction: 0,
                        },
                    ),
                    |s| {
                        EffectAttempt::ResolutionPreparation(Box::new(
                            s.qualification_prepare_resolution_cursor(entry, 0),
                        ))
                    },
                    true,
                )
            }
            "private_prepare_fields" => self.preparation(action),
            "trusted_run_existing_body" => {
                if !rows(action, "exact_instruction_reply_table")?.is_empty()
                    || action["no_unlisted_reply"] != true
                    || action["return_at"] != "first_actual_Pending_or_Complete"
                {
                    return Err("unsupported body helper reply/return contract".into());
                }
                let mut cast = action.clone();
                cast["source"] = json!(string(action, "source_alias")?);
                self.cast(&cast)?;
                self.resolve(action)?;
                self.maybe_followon_checks(
                    action["stop_before_followon_checks"]
                        .as_bool()
                        .ok_or("missing body stop policy")?,
                )
            }
            "trusted_interaction_inputs" => {
                if !rows(action, "exact_unlisted_instruction_reply_table")?.is_empty() {
                    return Err("unlisted interaction replies unsupported".into());
                }
                let stop = action["stop_before_followon_checks"]
                    .as_bool()
                    .ok_or("missing interaction stop policy")?;
                for input in rows(action, "sequence")? {
                    match string(input, "op")? {
                        "cast" => self.cast(input)?,
                        "pass" => self.pass(string(input, "actor")?)?,
                        "resolve_one" => {
                            self.resolve(input)?;
                            self.maybe_followon_checks(stop)?;
                        }
                        "reply_copy" => {
                            self.copy_reply(input)?;
                            self.maybe_followon_checks(stop)?;
                        }
                        "trusted_owned_zone_move" => self.movement(input)?,
                        _ => return Err("unsupported finite interaction input".into()),
                    }
                }
                Ok(())
            }
            "trusted_owned_zone_move" => self.movement(action),
            "trusted_activate_ability" => {
                self.ready()?;
                let actor = packet::seat(string(action, "actor")?)?;
                let source = self
                    .joins
                    .object(&action["source"], self.fixture, &self.state)?;
                let target = self.target(&action["target"])?;
                let ability =
                    ActivatedAbilityId(action["ability_id"].as_u64().ok_or("missing ability ID")?);
                let payment = Self::payment(&action["payment"])?;
                self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(
                        projection::EffectCall::ActivateAbility {
                            expected: self.state.revision(),
                            actor,
                            source,
                            ability,
                            target,
                            payment: Some(&payment),
                        },
                    ),
                    |s| {
                        EffectAttempt::Activation(Box::new(s.activate_ability(
                            s.revision(),
                            actor,
                            source,
                            ability,
                            target,
                            Some(payment.clone()),
                        )))
                    },
                    true,
                )?;
                let Some((_, EffectAttempt::Activation(result), _)) = self.attempts.last() else {
                    return Err("missing activation receipt".into());
                };
                let entry = result
                    .as_ref()
                    .as_ref()
                    .map_err(|_| "activation rejected")?
                    .result()
                    .activation
                    .entry;
                self.bind_entry(action["bind_entry"].as_str(), entry)
            }
            "trusted_registry_successor_inside_unit_only" => {
                if action["method"] != "apply_controller"
                    || action["retain_exact_incarnation"] != true
                    || action["adopt_into_same_private_executor"] != true
                {
                    return Err("unsupported private registry successor".into());
                }
                let object = self
                    .joins
                    .object(&action["object"], self.fixture, &self.state)?;
                let controller = packet::seat(string(action, "controller")?)?;
                self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(
                        projection::EffectCall::QualificationControllerSuccessor {
                            object,
                            controller,
                        },
                    ),
                    |s| {
                        EffectAttempt::Controller(Box::new(
                            s.qualification_controller_successor(object, controller),
                        ))
                    },
                    true,
                )
            }
            "trusted_resolve_current_entry_through_existing_owned_gate_and_driver" => {
                self.resolve(action)
            }
            "trusted_call" => match (string(action, "owner")?, string(action, "method")?) {
                ("EffectWindow", "begin_checks") => {
                    self.checks("explicit finite recipe check")?;
                    if action["completion_required"] == "Complete" && self.frame.is_some() {
                        return Err(
                            "explicit Complete requirement reached actual Pending; frame retained"
                                .into(),
                        );
                    }
                    Ok(())
                }
                ("EffectWindow", "begin_cleanup") => {
                    let turn = self
                        .turn
                        .clone()
                        .ok_or("cleanup call lacks actual prepared calendar")?;
                    self.invoke_with_input(
                        action.clone(),
                        projection::QualificationCall::EffectCall(
                            projection::EffectCall::BeginCleanup {
                                expected: self.state.revision(),
                                turn: &turn,
                            },
                        ),
                        |s| EffectAttempt::Progress(Box::new(s.begin_cleanup(s.revision(), &turn))),
                        true,
                    )
                }
                ("EffectWindow", "begin_mana_activation") => {
                    if action.get("ability_id").is_some() {
                        return Err("obsolete numeric mana selection field".into());
                    }
                    let actor = packet::seat(string(action, "actor")?)?;
                    let source = self
                        .joins
                        .object(&action["source"], self.fixture, &self.state)?;
                    let ability = crate::test_support::mana_ref::decode(action["ability"].clone())?;
                    self.invoke_with_input(
                        action.clone(),
                        projection::QualificationCall::EffectCall(
                            projection::EffectCall::BeginManaActivation {
                                expected: self.state.revision(),
                                actor,
                                source,
                                ability,
                            },
                        ),
                        |s| {
                            EffectAttempt::Mana(Box::new(s.begin_mana_activation(
                                s.revision(),
                                actor,
                                source,
                                ability,
                            )))
                        },
                        true,
                    )
                }
                ("EffectWindow", "begin_damage_batch") => {
                    if action["kind"] != "Noncombat" {
                        return Err("unsupported shared damage kind".into());
                    }
                    let origin = &action["origin"]["Resolution"];
                    let entry = self
                        .joins
                        .entry(string(origin, "entry_ref")?)
                        .ok_or("unbound actual shared damage origin")?;
                    let instruction = usize::try_from(
                        origin["instruction"]
                            .as_u64()
                            .ok_or("missing shared instruction")?,
                    )
                    .map_err(|error| error.to_string())?;
                    let components=rows(action,"components")?.iter().map(|value|{
                            let recipient=match (value.get("recipient_creature"),value.get("recipient_player")) {
                                (Some(object),None)=>crate::damage::DamageRecipient::Creature(self.joins.object(object,self.fixture,&self.state)?),
                                (None,Some(player))=>crate::damage::DamageRecipient::Player(packet::seat(player.as_str().ok_or("invalid recipient player")?)?),
                                _=>return Err("ambiguous shared damage recipient".into()),
                            };
                            Ok(crate::effects::private_qualification::QualificationDamageComponent {source:self.joins.object(&value["source_object"],self.fixture,&self.state)?,recipient,amount:quantity(&value["amount"])?})
                        }).collect::<Result<Vec<_>,String>>()?;
                    self.invoke_with_input(
                        action.clone(),
                        projection::QualificationCall::EffectCall(
                            projection::EffectCall::QualificationBeginSharedDamage {
                                entry,
                                instruction,
                                components: &components
                                    .iter()
                                    .map(|value| projection::SharedDamageComponent {
                                        source: value.source,
                                        recipient: value.recipient,
                                        amount: &value.amount,
                                    })
                                    .collect::<Vec<_>>(),
                            },
                        ),
                        |s| {
                            EffectAttempt::Progress(Box::new(s.qualification_begin_shared_damage(
                                entry,
                                instruction,
                                &components,
                            )))
                        },
                        true,
                    )
                }
                ("EffectWindow", "prune_non_battlefield_state") => self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(
                        projection::EffectCall::QualificationPruneCreatureState,
                    ),
                    |s| {
                        EffectAttempt::Preparation(Box::new(Ok(
                            s.qualification_prune_non_battlefield_state()
                        )))
                    },
                    true,
                ),
                ("GameObjects", "apply_zone_move") => {
                    if action["adopt"] != false || action["destination"] != "Exile" {
                        return Err(
                            "only literal nonadopted primitive Exile control supported here".into(),
                        );
                    }
                    let object = self
                        .joins
                        .object(&action["card"], self.fixture, &self.state)?;
                    self.invoke_with_input(
                        action.clone(),
                        projection::QualificationCall::GameObjectsCall(
                            projection::GameObjectsCall::ApplyZoneMove {
                                expected: self.state.window().cards().revision(),
                                object,
                                destination: ObjectDestination::Exile,
                            },
                        ),
                        |s| {
                            EffectAttempt::PrimitiveChange(Box::new(
                                s.window().cards().apply_zone_move(
                                    s.window().cards().revision(),
                                    object,
                                    ObjectDestination::Exile,
                                ),
                            ))
                        },
                        false,
                    )
                }
                _ => Err("unsupported actual owner body call".into()),
            },
            "capture_actual_boundary" | "save_snapshot" => {
                let index = self.records.len();
                self.records.push(capture::effect_record_point(json!({"action_ordinal":self.ordinal,"operation":action,"state":projection::effect(&self.state,self.aliases)?,"frame":self.frame.as_ref().map(|frame|projection::effect_frame(frame,&self.state)).transpose()?}),&self.state,self.aliases,"body exact captured boundary",&format!("/observation/records/{index}/state"))?);
                Ok(())
            }
            "attempt_rule_reply_without_adoption" | "reply_actual_damage_order" => {
                if action["kind"] == "Discard" {
                    let frame = self
                        .frame
                        .clone()
                        .ok_or("negative Discard lacks actual Pending")?;
                    let actor = packet::seat(string(action, "actor")?)?;
                    let selected = rows(action, "items")?
                        .iter()
                        .map(|value| self.joins.object(value, self.fixture, &self.state))
                        .collect::<Result<Vec<_>, _>>()?;
                    let turn = self.turn.clone();
                    return self.invoke_with_input(
                        action.clone(),
                        projection::QualificationCall::EffectCall(
                            projection::EffectCall::ResumeRule {
                                expected: self.state.revision(),
                                frame: &frame,
                                actor,
                                reply: &DecisionReply::Discard(selected.clone()),
                                turn: turn.as_ref(),
                            },
                        ),
                        |s| {
                            EffectAttempt::Progress(Box::new(s.resume_rule(
                                s.revision(),
                                &frame,
                                actor,
                                DecisionReply::Discard(selected.clone()),
                                turn.as_ref(),
                            )))
                        },
                        false,
                    );
                }
                if action.get("kind").is_some_and(|kind| kind != "DamageOrder") {
                    return Err("unsupported exact rule reply kind".into());
                }
                let application = self.damage_application(&action["application_ref"])?;
                let frame = self.frame.clone().ok_or("no actual rule frame")?;
                let actor = packet::seat(string(action, "actor")?)?;
                let adopt = action["op"] == "reply_actual_damage_order";
                self.invoke_with_input(
                    action.clone(),
                    projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
                        expected: self.state.revision(),
                        frame: &frame,
                        actor,
                        reply: &DecisionReply::DamageOrder(application),
                        turn: None,
                    }),
                    |s| {
                        EffectAttempt::Progress(Box::new(s.resume_rule(
                            s.revision(),
                            &frame,
                            actor,
                            DecisionReply::DamageOrder(application),
                            None,
                        )))
                    },
                    adopt,
                )
            }
            _ => Err("unsupported finite body operation".into()),
        }
    }
}
