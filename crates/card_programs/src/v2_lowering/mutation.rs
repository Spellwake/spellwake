//! Strictly-typed `v2::ir` object and player mutation lowering.
//!
//! Lowers `v2::ObjectMutation` and `v2::PlayerMutation` into native
//! `spellwake_engine` toolbox instructions (`Instruction::MutateObject`,
//! `Instruction::MutatePlayer`, `Instruction::ApplyContinuous`,
//! `Instruction::AdjustLife`, `Instruction::DealDamage`, `Instruction::StackOp`,
//! `Instruction::SideDeckOp`, `Instruction::CreateTokens`).

use std::collections::BTreeSet;
use std::num::NonZeroU16;

use spellwake_contracts::creature_types::CreatureTypeId;
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, ActivationSuppression, BlockingRestriction, ColorOperation,
        InstructionSubtypeOperand, KeywordAbility, PowerToughness, ResolvedDuration,
        ResolvedPartSpec, ResolvedPartsSpec, StaticContinuousAbility, StaticDrawRule,
        SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    effects::CounterKind as EngineCounterKind,
    emblem::EmblemDefinition,
    land::LandType,
    program::{
        ArtifactSubtype, BodyTarget, CoinFlipMode, ColorMask as EngineColorMask, CombatUntapGroup,
        DieRollMode, EnchantmentSubtype, ExecutableBody, Instruction,
        ObjectFilter as EngineObjectFilter, ProtectionRecipients, ResolvingPlayer, TargetZone,
        TemporaryRuleKind, ValueExpr as EngineValueExpr,
    },
    token::{TokenCreation, TokenRecipients},
    toolbox::{
        AdjustLifeSpec, ApplyContinuousSpec, AttachOp, ChoiceCategory, ControlOp, CounterOp,
        DealDamageSpec, GraveyardSemantics, LibraryTopAction, MutateObjectSpec, MutatePlayerSpec,
        ObjectCombatOp, ObjectMutation as EngineObjectMutation, ObjectQuery, ObjectStateMarkerOp,
        OrientationOp, PlayerDesignationOp, PlayerKeywordAction,
        PlayerMutation as EnginePlayerMutation, PlayerSelector, QuerySlice, QueryZone,
        SelectionConstraint, SideDeckKind, SideDeckOpKind, SideDeckOpSpec, StackOpSpec,
        TapStateOp, ZoneDest as EngineZoneDest, ZoneOpSpec, ZoneSelection,
    },
};
use spellwake_oracle_compiler::v2::ir as v2;

use crate::v2_lowering::{
    atoms, card,
    ctx::{BodyAcc, LowerCtx},
    effect,
    error::{LowerResult, LoweringError},
    filter, keywords, token, value,
};

/// Lower `Effect::MutateObject` into one or more toolbox instructions on `acc`.
pub(crate) fn lower_object_mutation(
    subject: &v2::SubjectRef,
    mutation: &v2::ObjectMutation,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match mutation {
        v2::ObjectMutation::Tap => {
            require_instant_duration(duration, "v2:tap-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::TapState(TapStateOp::Set(true)),
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::Untap => {
            require_instant_duration(duration, "v2:untap-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::TapState(TapStateOp::Set(false)),
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::ModifyPT {
            power,
            toughness,
            scale,
        } => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            if let Some(s) = scale {
                effect::register_value_expr_player_targets(s, acc, ctx)?;
            }
            let p_expr = value::lower_pt_delta(*power, scale.as_deref(), ctx)?;
            let t_expr = value::lower_pt_delta(*toughness, scale.as_deref(), ctx)?;
            let part = match (&p_expr, &t_expr) {
                (EngineValueExpr::Literal(p), EngineValueExpr::Literal(t)) => {
                    ResolvedPartSpec::ModifyPT(PowerToughness {
                        power: p.clone(),
                        toughness: t.clone(),
                    })
                }
                _ => ResolvedPartSpec::ModifyPTExpr {
                    power: Box::new(p_expr),
                    toughness: Box::new(t_expr),
                },
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(vec![part]), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::SetDynamicBasePT {
            power,
            toughness,
            add_types,
            keep_existing_types,
        } => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            effect::register_value_expr_player_targets(power, acc, ctx)?;
            effect::register_value_expr_player_targets(toughness, acc, ctx)?;
            let p_expr = value::lower_value_expr(power, ctx)?;
            let t_expr = value::lower_value_expr(toughness, ctx)?;
            let mut parts = Vec::new();
            if !add_types.is_empty() {
                let types_set = atoms::lower_card_type_set(*add_types);
                let type_op = if *keep_existing_types {
                    TypeOperation::Add(types_set)
                } else {
                    TypeOperation::Set(types_set)
                };
                parts.push(ResolvedPartSpec::Type(TypeChangeSpec {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: type_op,
                    subtypes: SubtypeOperation::Keep,
                }));
            }
            let pt_part = match (&p_expr, &t_expr) {
                (EngineValueExpr::Literal(p), EngineValueExpr::Literal(t)) => {
                    ResolvedPartSpec::SetPT(PowerToughness {
                        power: p.clone(),
                        toughness: t.clone(),
                    })
                }
                _ => ResolvedPartSpec::SetPTExpr {
                    power: Box::new(p_expr),
                    toughness: Box::new(t_expr),
                },
            };
            parts.push(pt_part);
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::GrantKeywords(kws) => {
            if kws.len() == 1
                && kws[0].kind == v2::KeywordKind::Flashback
                && kws[0].param.is_none()
                && matches!(duration, v2::Duration::Instant | v2::Duration::UntilEndOfTurn)
            {
                let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
                return emit_object_mutation_selected(
                    source,
                    selection,
                    EngineObjectMutation::StateMarker(ObjectStateMarkerOp::GrantFlashback),
                    acc,
                    ctx,
                );
            }
            if kws.len() == 1
                && kws[0].kind == v2::KeywordKind::Protection
                && matches!(kws[0].param, Some(v2::KeywordParam::ChosenColor))
            {
                let dur = atoms::lower_duration(duration)?;
                let recipients = lower_chosen_color_protection_recipients(subject, acc, ctx)?;
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::chosen_color_protection(recipients, dur),
                )));
                return Ok(());
            }
            let dur = atoms::lower_duration(duration)?;
            if kws.is_empty() {
                return Err(LoweringError::Unsupported("v2:grant-keywords-empty"));
            }
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let granted = lower_granted_keywords_preserving_ctx(kws, ctx)?;
            let mut parts = Vec::new();
            append_granted_keyword_parts(&mut parts, granted);
            if parts.is_empty() {
                return Err(LoweringError::Unsupported("v2:grant-keywords-empty"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::RemoveKeywords(kws) => {
            let dur = atoms::lower_duration(duration)?;
            if kws.is_empty() {
                return Err(LoweringError::Unsupported("v2:remove-keywords-empty"));
            }
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let mut suppressed = BTreeSet::new();
            for kw in kws {
                suppressed.insert(atoms::lower_keyword_ability(kw, ctx)?);
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange {
                        suppressed_keywords: suppressed,
                        ..AbilityChange::default()
                    })]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectMutation::RemoveAllAbilities => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectMutation::GrantAbility(face) => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let granted = card::lower_granted_face(face, ctx)?;
            if !granted.statics.is_empty() {
                return Err(LoweringError::Unsupported(
                    "v2:grant-ability-static-unsupported",
                ));
            }
            let mut parts = Vec::new();
            if !granted.keywords.is_empty() {
                parts.push(ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
                    granted.keywords,
                )));
            }
            for act in granted.activated {
                parts.push(ResolvedPartSpec::GrantActivated(Box::new(act)));
            }
            for trig in granted.triggered {
                parts.push(ResolvedPartSpec::GrantTrigger(Box::new(trig)));
            }
            for mana in granted.mana {
                parts.push(ResolvedPartSpec::GrantMana(Box::new(mana.output)));
            }
            if parts.is_empty() {
                return Err(LoweringError::Unsupported("v2:grant-ability-empty"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::PutCounters { kind, count } => {
            require_instant_duration(duration, "v2:put-counters-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            let engine_kind = atoms::lower_counter_kind(kind)?;
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            let amount = value::lower_value_expr(count, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Counters(CounterOp::Add {
                    kind: engine_kind,
                    amount,
                }),
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::RemoveCounters { kind, count, all } => {
            require_instant_duration(duration, "v2:remove-counters-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            let engine_kind = match kind {
                v2::CounterKind::Any => None,
                other => Some(atoms::lower_counter_kind(other)?),
            };
            let op = if *all {
                CounterOp::Clear { kind: engine_kind }
            } else {
                let amount = match count {
                    Some(expr) => {
                        effect::register_value_expr_player_targets(expr, acc, ctx)?;
                        value::lower_value_expr(expr, ctx)?
                    }
                    None => EngineValueExpr::literal(1),
                };
                CounterOp::Remove {
                    kind: engine_kind,
                    amount,
                }
            };
            emit_object_mutation_selected(source, selection, EngineObjectMutation::Counters(op), acc, ctx)
        }
        v2::ObjectMutation::MoveCounters {
            from,
            to,
            kind,
            count,
        } => {
            require_instant_duration(duration, "v2:move-counters-unexpected-duration")?;
            let engine_kind = match kind {
                v2::CounterKind::Any => None,
                other => Some(atoms::lower_counter_kind(other)?),
            };
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            let amount = value::lower_value_expr(count, ctx)?;
            if !acc.target_set
                && try_emit_two_target_pair(from, to, acc, ctx)?
            {
                return emit_object_mutation(
                    ObjectQuery::LinkedTarget { index: 0 },
                    EngineObjectMutation::Counters(CounterOp::Move {
                        to: ObjectQuery::LinkedTarget { index: 1 },
                        kind: engine_kind,
                        amount: Some(amount),
                    }),
                    acc,
                    ctx,
                );
            }
            let from_q = lower_mutation_subject_query(from, acc, ctx)?;
            let to_q = lower_mutation_subject_query(to, acc, ctx)?;
            emit_object_mutation(
                from_q,
                EngineObjectMutation::Counters(CounterOp::Move {
                    to: to_q,
                    kind: engine_kind,
                    amount: Some(amount),
                }),
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::DistributeCounters { kind, count } => {
            require_instant_duration(duration, "v2:distribute-counters-unexpected-duration")?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            if !matches!(query, ObjectQuery::Targets) {
                return Err(LoweringError::Unsupported(
                    "v2:distribute-counters-non-target",
                ));
            }
            let engine_kind = atoms::lower_counter_kind(kind)?;
            let Some(val) = value::lower_value_const(count) else {
                return Err(LoweringError::Unsupported("v2:distribute-counters-dynamic"));
            };
            let total = u16::try_from(val)
                .map_err(|_| LoweringError::Unsupported("v2:distribute-counters-range"))?;
            if total == 0 {
                return Err(LoweringError::Unsupported("v2:distribute-counters-zero"));
            }
            acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::distribute_counters(engine_kind, total),
            )));
            Ok(())
        }
        v2::ObjectMutation::DoubleCounters { kind } => {
            require_instant_duration(duration, "v2:double-counters-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            let engine_kind = match kind {
                None | Some(v2::CounterKind::Any) => None,
                Some(k) => Some(atoms::lower_counter_kind(k)?),
            };
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Counters(CounterOp::Double { kind: engine_kind }),
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::DoubleStat { power, toughness } => {
            if !*power && !*toughness {
                return Err(LoweringError::Unsupported("v2:double-stat-neither"));
            }
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::double_pt(query, *power, *toughness, dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::BecomeCopy { source, exceptions } => {
            let dur = atoms::lower_duration(duration)?;
            let receiver = lower_mutation_subject_query(subject, acc, ctx)?;
            let donor = lower_mutation_subject_query(source, acc, ctx)?;
            let exc = token::lower_copy_exceptions(exceptions, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::become_copy(receiver, donor, dur, exc),
            )));
            Ok(())
        }
        v2::ObjectMutation::BecomeColors {
            colors,
            keep_existing_colors,
        } => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let color_set = atoms::lower_color_set(*colors);
            let op = if *keep_existing_colors {
                ColorOperation::Add(color_set)
            } else {
                ColorOperation::Set(color_set)
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Color(op)]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectMutation::BecomeCreatureTypes {
            subtypes,
            add_types,
            keep_existing_types,
            base_pt,
            keywords,
        } => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            lower_become_types(
                query,
                subtypes,
                *add_types,
                *keep_existing_types,
                *base_pt,
                keywords,
                dur,
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::BecomeChosen(kind) => {
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let part = match kind {
                v2::CharacteristicKind::Color => ResolvedPartSpec::ChosenColor { add: false },
                v2::CharacteristicKind::BasicLandType => {
                    ResolvedPartSpec::ChosenBasicLandType { add: false }
                }
                v2::CharacteristicKind::CreatureType => {
                    return Err(LoweringError::Unsupported(
                        "v2:become-chosen-creature-type",
                    ));
                }
                v2::CharacteristicKind::CardType => {
                    return Err(LoweringError::Unsupported("v2:become-chosen-card-type"));
                }
                v2::CharacteristicKind::CardName | v2::CharacteristicKind::NonlandCardName => {
                    return Err(LoweringError::Unsupported("v2:become-chosen-card-name"));
                }
                v2::CharacteristicKind::BasicLandTypes(_) => {
                    return Err(LoweringError::Unsupported(
                        "v2:become-chosen-basic-land-types",
                    ));
                }
                v2::CharacteristicKind::OddOrEven => {
                    return Err(LoweringError::Unsupported("v2:become-chosen-odd-or-even"));
                }
                v2::CharacteristicKind::BinaryMode => {
                    return Err(LoweringError::Unsupported("v2:become-chosen-binary-mode"));
                }
                v2::CharacteristicKind::Number => {
                    return Err(LoweringError::Unsupported("v2:become-chosen-number"));
                }
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(vec![part]), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::GainControl { controller } => {
            let dur = atoms::lower_duration(duration)?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            let ctrl = atoms::lower_player_ref(*controller, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Control(ControlOp::Gain {
                    controller: ctrl,
                    duration: dur,
                }),
                acc,
                ctx,
            )
        }
        v2::ObjectMutation::AttachTo(host_subject) => {
            require_instant_duration(duration, "v2:attach-to-unexpected-duration")?;
            if !acc.target_set
                && try_emit_two_target_pair(subject, host_subject, acc, ctx)?
            {
                return emit_object_mutation(
                    ObjectQuery::LinkedTarget { index: 0 },
                    EngineObjectMutation::Attach(AttachOp::ToObject {
                        host: Box::new(ObjectQuery::LinkedTarget { index: 1 }),
                        ability: None,
                    }),
                    acc,
                    ctx,
                );
            }
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let attach_op = match host_subject.as_ref() {
                v2::SubjectRef::Player(player_ref) => AttachOp::ToPlayer {
                    player: atoms::lower_player_ref(*player_ref, acc, ctx)?,
                },
                v2::SubjectRef::TargetPlayer {
                    opponent_only,
                    count,
                } => {
                    acc.register_target(
                        BodyTarget::Player {
                            opponents_only: *opponent_only,
                        },
                        *count,
                    )?;
                    AttachOp::ToPlayer {
                        player: PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
                    }
                }
                other => AttachOp::ToObject {
                    host: Box::new(lower_mutation_subject_query(other, acc, ctx)?),
                    ability: None,
                },
            };
            emit_object_mutation(query, EngineObjectMutation::Attach(attach_op), acc, ctx)
        }
        v2::ObjectMutation::ApplyRule(rule) => {
            lower_apply_rule_mutation(subject, rule, duration, acc, ctx)
        }
        v2::ObjectMutation::CantBeRegenerated => {
            let dur = duration_default_eot(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CannotBeRegenerated,
                    )]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectMutation::StateAction(action) => {
            lower_object_state_action(subject, *action, duration, acc, ctx)
        }
        v2::ObjectMutation::KeywordAction { kind, amount } => {
            lower_object_keyword_action(subject, *kind, amount, duration, acc, ctx)
        }
        v2::ObjectMutation::CanBlockAdditional(extra) => {
            if *extra == 0 {
                return Err(LoweringError::Unsupported("v2:can-block-additional-zero"));
            }
            let dur = duration_default_eot(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let parts = vec![
                ResolvedPartSpec::BlockingRestriction(BlockingRestriction::AdditionalBlock);
                usize::from(*extra)
            ];
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::Fight(other) => {
            require_instant_duration(duration, "v2:fight-unexpected-duration")?;
            if !acc.target_set
                && try_emit_two_target_pair(subject, other, acc, ctx)?
            {
                acc.push(Instruction::DealDamage(Box::new(DealDamageSpec::fight(
                    ObjectQuery::LinkedTarget { index: 0 },
                    ObjectQuery::LinkedTarget { index: 1 },
                ))));
                return Ok(());
            }
            let first = lower_mutation_subject_query(subject, acc, ctx)?;
            let second = lower_mutation_subject_query(other, acc, ctx)?;
            acc.push(Instruction::DealDamage(Box::new(DealDamageSpec::fight(
                first, second,
            ))));
            Ok(())
        }
        v2::ObjectMutation::RetainTypes(mask) => {
            if mask.is_empty() {
                return Err(LoweringError::Unsupported("v2:retain-types-empty"));
            }
            let dur = atoms::lower_duration(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let types_set = atoms::lower_card_type_set(*mask);
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Add(types_set),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectMutation::MustBlockThisTurn(blocker_target) => {
            let dur = duration_default_eot(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let part = match blocker_target {
                None => ResolvedPartSpec::BlockingRestriction(
                    BlockingRestriction::MustBlockEachCombatIfAble,
                ),
                Some(target_subj) => match target_subj.as_ref() {
                    v2::SubjectRef::SelfObject => {
                        ResolvedPartSpec::BlockingRestriction(BlockingRestriction::MustBlockSource)
                    }
                    other => {
                        let target_q = lower_mutation_subject_query(other, acc, ctx)?;
                        let Some(move_ref) = target_q.to_move_subject() else {
                            return Err(LoweringError::Unsupported(
                                "v2:must-block-complex-subject",
                            ));
                        };
                        ResolvedPartSpec::MustBlockSubject(move_ref)
                    }
                },
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(vec![part]), dur),
            )));
            Ok(())
        }
        v2::ObjectMutation::ChangeText(_) => {
            Err(LoweringError::Unsupported("v2:object-mutation-change-text"))
        }
        v2::ObjectMutation::RetargetTo(_) => {
            Err(LoweringError::Unsupported("v2:object-mutation-retarget-to"))
        }
    }
}

/// Lower `Effect::MutatePlayer` into one or more toolbox instructions on `acc`.
pub(crate) fn lower_player_mutation(
    player: v2::PlayerRef,
    mutation: &v2::PlayerMutation,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match mutation {
        v2::PlayerMutation::AddMana(spec) => {
            require_instant_duration(duration, "v2:add-mana-unexpected-duration")?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            let yield_kind = atoms::lower_mana_spec(spec, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::add_mana(rp, yield_kind),
            )));
            Ok(())
        }
        v2::PlayerMutation::ManaRule(_) => {
            Err(LoweringError::Unsupported("v2:player-mutation-mana-rule"))
        }
        v2::PlayerMutation::AddManaSpendRider { .. } => Err(LoweringError::Unsupported(
            "v2:player-mutation-mana-spend-rider",
        )),
        v2::PlayerMutation::GainCounters { kind, count } => {
            require_instant_duration(duration, "v2:player-gain-counters-unexpected-duration")?;
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let engine_kind = atoms::lower_counter_kind(kind)?;
            let amount = value::lower_value_expr(count, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::add_counter(ps, engine_kind, amount),
            )));
            Ok(())
        }
        v2::PlayerMutation::Action(action) => {
            lower_player_action(player, *action, duration, acc, ctx)
        }
        v2::PlayerMutation::ScaledAction { kind, amount } => {
            lower_scaled_player_action(player, *kind, amount, duration, acc, ctx)
        }
        v2::PlayerMutation::Amass { subtype, amount } => {
            require_instant_duration(duration, "v2:amass-unexpected-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let army = ctx
                .lookup_creature_type("Army")
                .ok_or(LoweringError::Unsupported("v2:amass-missing-army-type"))?;
            let sub_id = ctx
                .lookup_creature_type(subtype)
                .ok_or(LoweringError::Unsupported("v2:amass-unknown-subtype"))?;
            let Some(val) = value::lower_value_const(amount) else {
                return Err(LoweringError::Unsupported("v2:amass-dynamic-amount"));
            };
            let n = u16::try_from(val)
                .map_err(|_| LoweringError::Unsupported("v2:amass-amount-range"))?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::KeywordAction(PlayerKeywordAction::Amass {
                    army,
                    subtype: sub_id,
                    amount: n,
                }),
            ))));
            Ok(())
        }
        v2::PlayerMutation::Retarget {
            subject,
            new_target,
        } => {
            require_instant_duration(duration, "v2:retarget-unexpected-duration")?;
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:retarget-non-controller"));
            }
            if new_target.is_some() {
                return Err(LoweringError::Unsupported("v2:retarget-fixed-new-target"));
            }
            let single_target_only = matches!(
                subject.as_ref(),
                v2::SubjectRef::TargetObject { filter, .. }
                    if filter.flags.contains(v2::FilterFlags::SINGLE_TARGET)
            );
            let (target, t_count) = atoms::lower_subject_to_target(subject, ctx)?;
            acc.register_target(target, t_count)?;
            acc.push(Instruction::StackOp(Box::new(StackOpSpec::retarget(
                single_target_only,
            ))));
            Ok(())
        }
        v2::PlayerMutation::RollDie { sides, count } => {
            require_instant_duration(duration, "v2:roll-die-unexpected-duration")?;
            let Some(nz_sides) = NonZeroU16::new(u16::from(*sides)) else {
                return Err(LoweringError::Unsupported("v2:roll-die-zero-sides"));
            };
            if *count == 0 {
                return Err(LoweringError::Unsupported("v2:roll-die-zero-count"));
            }
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::roll_die(
                    rp,
                    nz_sides,
                    EngineValueExpr::literal(i64::from(*count)),
                    DieRollMode::Sum,
                    Vec::new(),
                ),
            )));
            Ok(())
        }
        v2::PlayerMutation::SkipNextStep(step_ast) => {
            require_instant_duration(duration, "v2:skip-next-step-unexpected-duration")?;
            let step = atoms::lower_step(*step_ast)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::skip_step(ps, step),
            )));
            Ok(())
        }
        v2::PlayerMutation::AdditionalLandPlay(extra) => {
            if *extra == 0 {
                return Err(LoweringError::Unsupported("v2:additional-land-play-zero"));
            }
            let dur = duration_default_eot(duration)?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::AdditionalLandPlay {
                        player: rp,
                        count: *extra,
                    },
                    dur,
                ),
            )));
            Ok(())
        }
        v2::PlayerMutation::AdditionalCombat { followed_by_main } => {
            if !matches!(
                duration,
                v2::Duration::Instant
                    | v2::Duration::UntilEndOfTurn
                    | v2::Duration::UntilEndOfCombat
            ) {
                return Err(LoweringError::Unsupported(
                    "v2:additional-combat-unexpected-duration",
                ));
            }
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported(
                    "v2:additional-combat-non-controller",
                ));
            }
            if !*followed_by_main {
                return Err(LoweringError::Unsupported(
                    "v2:additional-combat-without-main",
                ));
            }
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::additional_combat(CombatUntapGroup::None),
            )));
            Ok(())
        }
        v2::PlayerMutation::PayCost(cost) => {
            require_instant_duration(duration, "v2:pay-cost-unexpected-duration")?;
            lower_inline_pay_cost(player, cost, acc, ctx)
        }
        v2::PlayerMutation::GrantPlayPermission {
            cast_only,
            subject,
            free,
        } => {
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let impulse = atoms::lower_impulse_duration(duration)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::impulse_play_permission(
                    query, ps, impulse, *cast_only, *free, false,
                ),
            )));
            Ok(())
        }
        v2::PlayerMutation::CreateEmblem(face) => {
            require_instant_duration(duration, "v2:create-emblem-unexpected-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let granted = card::lower_granted_face(face, ctx)?;
            if !granted.keywords.is_empty()
                || !granted.activated.is_empty()
                || !granted.mana.is_empty()
            {
                return Err(LoweringError::Unsupported(
                    "v2:emblem-unsupported-ability-kind",
                ));
            }
            if granted.statics.is_empty() && granted.triggered.is_empty() {
                return Err(LoweringError::Unsupported("v2:emblem-empty"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::emblem(
                    ps,
                    EmblemDefinition::Authored {
                        label: ctx.face_name.to_string(),
                        continuous: granted.statics,
                        triggered: granted.triggered,
                    },
                ),
            )));
            Ok(())
        }
        v2::PlayerMutation::Behold(obj_filter) => {
            require_instant_duration(duration, "v2:behold-unexpected-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::Behold {
                    filter: Box::new(ef),
                    optional: false,
                },
            ))));
            Ok(())
        }
        v2::PlayerMutation::GameIsDraw => {
            require_instant_duration(duration, "v2:game-is-draw-unexpected-duration")?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::draw_game(),
            )));
            Ok(())
        }
        v2::PlayerMutation::Empower { .. } => {
            Err(LoweringError::Unsupported("v2:player-mutation-empower"))
        }
        v2::PlayerMutation::XMaxBound(_) => {
            Err(LoweringError::Unsupported("v2:player-mutation-x-max-bound"))
        }
    }
}

// Parameters mirror the orthogonal fields of `v2::ObjectMutation::BecomeCreatureTypes`.
#[allow(clippy::too_many_arguments)]
fn lower_become_types(
    query: ObjectQuery,
    subtypes: &[String],
    add_types: spellwake_oracle_compiler::ast::CardTypeMask,
    keep_existing_types: bool,
    base_pt: Option<(i32, i32)>,
    keywords: &[v2::KeywordSpec],
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let mut creature_subs: BTreeSet<CreatureTypeId> = BTreeSet::new();
    let mut land_subs: BTreeSet<LandType> = BTreeSet::new();
    let mut artifact_subs: BTreeSet<ArtifactSubtype> = BTreeSet::new();
    let mut enchantment_subs: BTreeSet<EnchantmentSubtype> = BTreeSet::new();

    for raw in subtypes {
        if let Some(ct) = ctx.lookup_creature_type(raw) {
            creature_subs.insert(ct);
        } else if let Some(lt) = LowerCtx::lookup_land_type(raw) {
            land_subs.insert(lt);
        } else if let Some(at) = LowerCtx::lookup_artifact_subtype(raw) {
            artifact_subs.insert(at);
        } else if let Some(et) = LowerCtx::lookup_enchantment_subtype(raw) {
            enchantment_subs.insert(et);
        } else {
            return Err(LoweringError::Unsupported("v2:become-types-unknown-subtype"));
        }
    }

    let types_op = if add_types.is_empty() {
        TypeOperation::Keep
    } else {
        let set = atoms::lower_card_type_set(add_types);
        if keep_existing_types {
            TypeOperation::Add(set)
        } else {
            TypeOperation::Set(set)
        }
    };
    let creature_op = if creature_subs.is_empty() {
        SubtypeOperation::Keep
    } else if keep_existing_types {
        SubtypeOperation::Add(InstructionSubtypeOperand::Literal(creature_subs))
    } else {
        SubtypeOperation::Set(InstructionSubtypeOperand::Literal(creature_subs))
    };
    let land_op = if land_subs.is_empty() {
        SubtypeOperation::Keep
    } else if keep_existing_types {
        SubtypeOperation::Add(land_subs)
    } else {
        SubtypeOperation::Set(land_subs)
    };

    let base_change = TypeChangeSpec {
        land_subtypes: land_op,
        supertypes: TypeOperation::Keep,
        types: types_op,
        subtypes: creature_op,
    };

    let mut parts = Vec::new();
    if artifact_subs.is_empty() && enchantment_subs.is_empty() {
        parts.push(ResolvedPartSpec::Type(base_change));
    } else {
        let art_op = if artifact_subs.is_empty() {
            SubtypeOperation::Keep
        } else if keep_existing_types {
            SubtypeOperation::Add(artifact_subs)
        } else {
            SubtypeOperation::Set(artifact_subs)
        };
        let ench_op = if enchantment_subs.is_empty() {
            SubtypeOperation::Keep
        } else if keep_existing_types {
            SubtypeOperation::Add(enchantment_subs)
        } else {
            SubtypeOperation::Set(enchantment_subs)
        };
        parts.push(ResolvedPartSpec::TypeWithSubtypes {
            base: base_change,
            artifact_subtypes: art_op,
            enchantment_subtypes: ench_op,
        });
    }

    if let Some((p, t)) = base_pt {
        parts.push(ResolvedPartSpec::SetPT(PowerToughness {
            power: spellwake_contracts::RulesInteger::from(i64::from(p)),
            toughness: spellwake_contracts::RulesInteger::from(i64::from(t)),
        }));
    }

    if !keywords.is_empty() {
        let granted = lower_granted_keywords_preserving_ctx(keywords, ctx)?;
        append_granted_keyword_parts(&mut parts, granted);
    }

    acc.push(Instruction::ApplyContinuous(Box::new(
        ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration),
    )));
    Ok(())
}

fn lower_granted_keywords_preserving_ctx(
    kws: &[v2::KeywordSpec],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<keywords::GrantedKeywordParts> {
    let saved_has_obj = ctx.has_object_target;
    let saved_last_player = ctx.last_player_target;
    let saved_dmg_event = ctx.trigger_damage_event;
    let res = keywords::lower_granted_keywords(kws, ctx);
    ctx.has_object_target = saved_has_obj;
    ctx.last_player_target = saved_last_player;
    ctx.trigger_damage_event = saved_dmg_event;
    res
}

fn append_granted_keyword_parts(
    parts: &mut Vec<ResolvedPartSpec>,
    granted: keywords::GrantedKeywordParts,
) {
    if granted.has_ability_change() {
        parts.push(ResolvedPartSpec::Abilities(granted.ability_change));
    }
    for trig in granted.extra_triggers {
        parts.push(ResolvedPartSpec::GrantTrigger(Box::new(trig)));
    }
    for (act, restriction) in granted.extra_activated {
        match restriction {
            Some(restriction) => parts.push(ResolvedPartSpec::GrantActivatedRestricted {
                ability: Box::new(act),
                restriction: Box::new(restriction),
            }),
            None => parts.push(ResolvedPartSpec::GrantActivated(Box::new(act))),
        }
    }
}

fn lower_chosen_color_protection_recipients(
    subject: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ProtectionRecipients> {
    match subject {
        v2::SubjectRef::SelfObject => Ok(ProtectionRecipients::AbilitySource),
        v2::SubjectRef::TargetObject { .. } => {
            let (bt, tc) = atoms::lower_subject_to_target(subject, ctx)?;
            if !matches!(
                bt,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) || !matches!(tc, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
            {
                return Err(LoweringError::Unsupported(
                    "v2:chosen-color-protection-target",
                ));
            }
            acc.register_target(bt, tc)?;
            ctx.has_object_target = true;
            Ok(ProtectionRecipients::TargetPermanent)
        }
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::SameTarget,
        ) if acc.target_set
            && matches!(
                acc.target,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            )
            && acc.target_bounds.is_none()
            && !acc.x_target_count
            && !acc.up_to_x_target_count =>
        {
            Ok(ProtectionRecipients::TargetPermanent)
        }
        v2::SubjectRef::Group(flt)
        | v2::SubjectRef::Each(flt)
        | v2::SubjectRef::All(flt) => {
            effect::register_filter_player_targets(flt, acc, ctx)?;
            let ef = filter::lower_object_filter(flt, ctx)?;
            Ok(ProtectionRecipients::Matching(Box::new(ef)))
        }
        _ => Err(LoweringError::Unsupported(
            "v2:chosen-color-protection-subject",
        )),
    }
}

fn lower_apply_rule_mutation(
    subject: &v2::SubjectRef,
    rule: &v2::RuleRestrictionKind,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let dur = duration_default_eot(duration)?;
    let query = lower_mutation_subject_query(subject, acc, ctx)?;

    let parts: Vec<ResolvedPartSpec> = match rule {
        v2::RuleRestrictionKind::CantAttack => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotAttack,
        )],
        v2::RuleRestrictionKind::CantBlock => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotBlock,
        )],
        v2::RuleRestrictionKind::CantAttackOrBlock => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotAttackOrBlock,
        )],
        v2::RuleRestrictionKind::CantBeBlocked => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotBeBlocked,
        )],
        v2::RuleRestrictionKind::CantBeBlockedBy(obj_filter) => {
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotBeBlockedByMatching(ef),
            )]
        }
        v2::RuleRestrictionKind::CantBeBlockedExceptBy(obj_filter) => {
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::OnlyBlockedByMatching(ef),
            )]
        }
        v2::RuleRestrictionKind::CantBeBlockedByMoreThan(n) => {
            vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotBeBlockedByMoreThan(*n),
            )]
        }
        v2::RuleRestrictionKind::CantBeBlockedExceptByCount(2) => {
            vec![ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                KeywordAbility::Menace,
            ]))]
        }
        v2::RuleRestrictionKind::CantBeBlockedExceptByCount(3) => {
            vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::AtLeastThreeBlockers,
            )]
        }
        v2::RuleRestrictionKind::CantBeBlockedExceptByCount(_) => {
            return Err(LoweringError::Unsupported(
                "v2:rule-cant-be-blocked-except-count",
            ));
        }
        v2::RuleRestrictionKind::CanBlockOnly(obj_filter) => {
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CanBlockOnlyMatching(ef),
            )]
        }
        v2::RuleRestrictionKind::CantAttackAlone => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotAttackAlone,
        )],
        v2::RuleRestrictionKind::CantBlockAlone => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotBlockAlone,
        )],
        v2::RuleRestrictionKind::CantAttackOrBlockAlone => vec![
            ResolvedPartSpec::BlockingRestriction(BlockingRestriction::CannotAttackAlone),
            ResolvedPartSpec::BlockingRestriction(BlockingRestriction::CannotBlockAlone),
        ],
        v2::RuleRestrictionKind::MustBeBlocked => vec![ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::MustBeBlocked,
        )],
        v2::RuleRestrictionKind::CantBecomeUntapped => {
            vec![ResolvedPartSpec::Abilities(AbilityChange {
                doesnt_untap: true,
                ..AbilityChange::default()
            })]
        }
        v2::RuleRestrictionKind::ActivatedAbilitiesCantBeActivated => {
            vec![ResolvedPartSpec::Abilities(AbilityChange {
                suppressed_activations: Some(ActivationSuppression::All),
                ..AbilityChange::default()
            })]
        }
        v2::RuleRestrictionKind::AreGoaded => {
            let permanent = matches!(
                duration,
                v2::Duration::Permanent | v2::Duration::ForTheRestOfTheGame
            );
            return emit_object_mutation(
                query,
                EngineObjectMutation::Goad {
                    by: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    permanent,
                },
                acc,
                ctx,
            );
        }
        v2::RuleRestrictionKind::CantAttackUnlessDefendingControls(obj_filter) => {
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotAttackUnlessDefenderControls(ef),
            )]
        }
        v2::RuleRestrictionKind::CantBlockSubject(target_subj) => match target_subj.as_ref() {
            v2::SubjectRef::SelfObject => vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotBlockSource,
            )],
            _ => {
                return Err(LoweringError::Unsupported("v2:rule-cant-block-subject"));
            }
        },
        v2::RuleRestrictionKind::DamageCantBePrevented => {
            vec![ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                KeywordAbility::DamageCannotBePrevented,
            ]))]
        }
        v2::RuleRestrictionKind::CantBeTargetedBy(obj_filter) => {
            if is_pure_color_filter(obj_filter) {
                let mask = atoms::lower_color_mask(obj_filter.any_colors);
                if obj_filter.any_spell {
                    vec![ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(mask)]
                } else {
                    vec![ResolvedPartSpec::CannotBeTargetedByColors(mask)]
                }
            } else {
                return Err(LoweringError::Unsupported("v2:rule-cant-be-targeted-by"));
            }
        }
        v2::RuleRestrictionKind::CantBeCountered => {
            if matches!(subject, v2::SubjectRef::SelfObject) {
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(
                        TemporaryRuleKind::CantBeCountered {
                            colors: EngineColorMask::EMPTY,
                            filter: None,
                            controller_only: true,
                            next_only: true,
                        },
                        dur,
                    ),
                )));
                return Ok(());
            }
            return Err(LoweringError::Unsupported("v2:rule-cant-be-countered"));
        }
        v2::RuleRestrictionKind::CantBlockOrBeBlockedBy(_) => {
            return Err(LoweringError::Unsupported(
                "v2:rule-cant-block-or-be-blocked-by",
            ));
        }
        v2::RuleRestrictionKind::CantBeCast => {
            return Err(LoweringError::Unsupported("v2:rule-cant-be-cast"));
        }
        v2::RuleRestrictionKind::CantAttackTarget(_) => {
            return Err(LoweringError::Unsupported("v2:rule-cant-attack-target"));
        }
        v2::RuleRestrictionKind::CantAttackUnless(_) => {
            return Err(LoweringError::Unsupported("v2:rule-cant-attack-unless"));
        }
        v2::RuleRestrictionKind::CantBlockUnless(_) => {
            return Err(LoweringError::Unsupported("v2:rule-cant-block-unless"));
        }
        v2::RuleRestrictionKind::CantAttackOrBlockUnless(_) => {
            return Err(LoweringError::Unsupported(
                "v2:rule-cant-attack-or-block-unless",
            ));
        }
        v2::RuleRestrictionKind::CantBeEnchantedBy(_) => {
            return Err(LoweringError::Unsupported("v2:rule-cant-be-enchanted-by"));
        }
        v2::RuleRestrictionKind::CantPhaseOut => {
            return Err(LoweringError::Unsupported("v2:rule-cant-phase-out"));
        }
    };

    acc.push(Instruction::ApplyContinuous(Box::new(
        ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), dur),
    )));
    Ok(())
}

fn lower_object_state_action(
    subject: &v2::SubjectRef,
    action: v2::ObjectStateAction,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match action {
        v2::ObjectStateAction::Transform => {
            require_instant_duration(duration, "v2:transform-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Orientation(OrientationOp::Transform),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::TurnFaceUp => {
            require_instant_duration(duration, "v2:turn-face-up-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Orientation(OrientationOp::TurnFaceUp),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::TurnFaceDown => {
            require_instant_duration(duration, "v2:turn-face-down-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Orientation(OrientationOp::TurnFaceDown),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::Goad => {
            let permanent = match duration {
                v2::Duration::Instant
                | v2::Duration::UntilYourNextTurn
                | v2::Duration::UntilEndOfTurn => false,
                v2::Duration::Permanent | v2::Duration::ForTheRestOfTheGame => true,
                _ => return Err(LoweringError::Unsupported("v2:goad-unexpected-duration")),
            };
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Goad {
                    by: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    permanent,
                },
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::Regenerate => {
            if !matches!(
                duration,
                v2::Duration::Instant | v2::Duration::UntilEndOfTurn
            ) {
                return Err(LoweringError::Unsupported(
                    "v2:regenerate-unexpected-duration",
                ));
            }
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Regenerate,
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::DoesntUntapNextUntap => {
            if !matches!(
                duration,
                v2::Duration::Instant | v2::Duration::UntilYourNextTurn
            ) {
                return Err(LoweringError::Unsupported(
                    "v2:skip-next-untap-unexpected-duration",
                ));
            }
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::TapState(TapStateOp::SkipNextUntap),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::MustAttackEachCombat => {
            let dur = duration_default_eot(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::MustAttackEachCombatIfAble,
                    )]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectStateAction::Exert => {
            require_instant_duration(duration, "v2:exert-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::TapState(TapStateOp::Exert),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::PhaseOut => {
            require_instant_duration(duration, "v2:phase-out-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Orientation(OrientationOp::PhaseOut),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::Prepare => {
            require_instant_duration(duration, "v2:prepare-unexpected-duration")?;
            if !matches!(subject, v2::SubjectRef::SelfObject) {
                return Err(LoweringError::Unsupported("v2:prepare-non-self"));
            }
            acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::set_prepared(),
            )));
            Ok(())
        }
        v2::ObjectStateAction::AssignsNoCombatDamage => {
            let dur = duration_default_eot(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::AssignNoCombatDamage,
                    )]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectStateAction::SwitchPowerAndToughness => {
            let dur = duration_default_eot(duration)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::ObjectStateAction::RemoveFromCombat => {
            require_instant_duration(duration, "v2:remove-from-combat-unexpected-duration")?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::Combat(ObjectCombatOp::RemoveFromCombat),
                acc,
                ctx,
            )
        }
        v2::ObjectStateAction::Flip => Err(LoweringError::Unsupported("v2:state-action-flip")),
        v2::ObjectStateAction::Suspect => {
            Err(LoweringError::Unsupported("v2:state-action-suspect"))
        }
        v2::ObjectStateAction::Detain => Err(LoweringError::Unsupported("v2:state-action-detain")),
        v2::ObjectStateAction::PutSticker => {
            Err(LoweringError::Unsupported("v2:state-action-put-sticker"))
        }
    }
}

fn lower_object_keyword_action(
    subject: &v2::SubjectRef,
    kind: v2::ObjectKeywordAction,
    amount: &v2::ValueExpr,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    require_instant_duration(duration, "v2:object-keyword-action-duration")?;
    match kind {
        v2::ObjectKeywordAction::Monstrosity => {
            if !matches!(subject, v2::SubjectRef::SelfObject) {
                return Err(LoweringError::Unsupported("v2:monstrosity-non-self"));
            }
            effect::register_value_expr_player_targets(amount, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::monstrosity(amt),
            )));
            Ok(())
        }
        v2::ObjectKeywordAction::Explore => {
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            if value::lower_value_const(amount) == Some(1) {
                acc.push(Instruction::MutateObject(Box::new(
                    MutateObjectSpec::explore(query),
                )));
                Ok(())
            } else if !query.uses_target() {
                effect::register_value_expr_player_targets(amount, acc, ctx)?;
                let count = value::lower_value_expr(amount, ctx)?;
                let body = ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::explore(query),
                    ))],
                );
                acc.push(Instruction::RepeatLoop {
                    count,
                    body: Box::new(body),
                });
                Ok(())
            } else {
                Err(LoweringError::Unsupported("v2:explore-multi-targeted"))
            }
        }
        v2::ObjectKeywordAction::Adapt => {
            Err(LoweringError::Unsupported("v2:object-keyword-action-adapt"))
        }
        v2::ObjectKeywordAction::Endure => {
            Err(LoweringError::Unsupported("v2:object-keyword-action-endure"))
        }
    }
}

fn lower_player_action(
    player: v2::PlayerRef,
    action: v2::PlayerActionKind,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match action {
        v2::PlayerActionKind::Proliferate => {
            require_instant_duration(duration, "v2:proliferate-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::KeywordAction(PlayerKeywordAction::Proliferate),
            ))));
            Ok(())
        }
        v2::PlayerActionKind::Populate => {
            require_instant_duration(duration, "v2:populate-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::KeywordAction(PlayerKeywordAction::Populate),
            ))));
            Ok(())
        }
        v2::PlayerActionKind::Manifest => {
            require_instant_duration(duration, "v2:manifest-duration")?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::manifest_top(rp, EngineValueExpr::literal(1)),
            )));
            Ok(())
        }
        v2::PlayerActionKind::ManifestDread => {
            require_instant_duration(duration, "v2:manifest-dread-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::LibraryTop(LibraryTopAction::ManifestDread),
            ))));
            Ok(())
        }
        v2::PlayerActionKind::VentureIntoDungeon => {
            require_instant_duration(duration, "v2:venture-duration")?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::venture_into_dungeon(rp, false),
            )));
            Ok(())
        }
        v2::PlayerActionKind::RingTemptsYou => {
            require_instant_duration(duration, "v2:ring-tempts-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::Designation(PlayerDesignationOp::RingTemptsYou),
            ))));
            Ok(())
        }
        v2::PlayerActionKind::FlipCoin => {
            require_instant_duration(duration, "v2:flip-coin-duration")?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::flip_coins(rp, CoinFlipMode::Single, None, None, None),
            )));
            Ok(())
        }
        v2::PlayerActionKind::Clash => {
            require_instant_duration(duration, "v2:clash-duration")?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::clash(rp),
            )));
            Ok(())
        }
        v2::PlayerActionKind::BecomeMonarch => {
            require_instant_duration(duration, "v2:monarch-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::become_monarch(ps),
            )));
            Ok(())
        }
        v2::PlayerActionKind::ExtraTurn => {
            require_instant_duration(duration, "v2:extra-turn-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::take_extra_turn(ps),
            )));
            Ok(())
        }
        v2::PlayerActionKind::SkipNextTurn => {
            require_instant_duration(duration, "v2:skip-next-turn-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::skip_turns(ps, EngineValueExpr::literal(1)),
            )));
            Ok(())
        }
        v2::PlayerActionKind::WinGame => {
            require_instant_duration(duration, "v2:win-game-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::win_game(ps),
            )));
            Ok(())
        }
        v2::PlayerActionKind::LoseGame => {
            require_instant_duration(duration, "v2:lose-game-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::lose_game(ps),
            )));
            Ok(())
        }
        v2::PlayerActionKind::TakeInitiative => {
            require_instant_duration(duration, "v2:take-initiative-duration")?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::take_initiative(rp),
            )));
            Ok(())
        }
        v2::PlayerActionKind::OpenAttraction => {
            require_instant_duration(duration, "v2:open-attraction-duration")?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            acc.push(Instruction::SideDeckOp(Box::new(SideDeckOpSpec {
                owner: Some(ps),
                kind: SideDeckKind::Attraction,
                op: SideDeckOpKind::AdvanceTop,
                bind_succeeded: None,
            })));
            Ok(())
        }
        v2::PlayerActionKind::NoMaxHandSize => {
            let dur = atoms::lower_duration(duration)?;
            let draw_rule = match player {
                v2::PlayerRef::You => StaticDrawRule::NoMaximumHandSize,
                v2::PlayerRef::EachPlayer => StaticDrawRule::AllPlayersNoMaximumHandSize,
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:no-max-hand-size-player-scope",
                    ));
                }
            };
            let id = ctx.alloc_static_id();
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::temporary_static(StaticContinuousAbility::DrawRule {
                        id,
                        rule: draw_rule,
                    }),
                    dur,
                ),
            )));
            Ok(())
        }
        v2::PlayerActionKind::Forage => Err(LoweringError::Unsupported("v2:player-action-forage")),
        v2::PlayerActionKind::Learn => Err(LoweringError::Unsupported("v2:player-action-learn")),
        v2::PlayerActionKind::KeepManaAcrossSteps => Err(LoweringError::Unsupported(
            "v2:player-action-keep-mana-across-steps",
        )),
        v2::PlayerActionKind::RepeatProcessOnce => Err(LoweringError::Unsupported(
            "v2:player-action-repeat-process-once",
        )),
        v2::PlayerActionKind::Recruit => {
            Err(LoweringError::Unsupported("v2:player-action-recruit"))
        }
        v2::PlayerActionKind::FlashbackCostEqualsManaCost => Err(LoweringError::Unsupported(
            "v2:player-action-flashback-cost-equals-mana-cost",
        )),
        v2::PlayerActionKind::SpendManaAsAnyType => Err(LoweringError::Unsupported(
            "v2:player-action-spend-mana-as-any-type",
        )),
        v2::PlayerActionKind::ChooseSameModeMultiple => Err(LoweringError::Unsupported(
            "v2:player-action-choose-same-mode-multiple",
        )),
        v2::PlayerActionKind::XCantBeZero => {
            Err(LoweringError::Unsupported("v2:player-action-x-cant-be-zero"))
        }
        v2::PlayerActionKind::LifeTotalCantChange => Err(LoweringError::Unsupported(
            "v2:player-action-life-total-cant-change",
        )),
    }
}

fn lower_scaled_player_action(
    player: v2::PlayerRef,
    kind: v2::ScaledPlayerAction,
    amount: &v2::ValueExpr,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    require_instant_duration(duration, "v2:scaled-player-action-duration")?;
    effect::register_value_expr_player_targets(amount, acc, ctx)?;
    match kind {
        v2::ScaledPlayerAction::Scry => {
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::LibraryTop(LibraryTopAction::Scry(amt)),
            ))));
            Ok(())
        }
        v2::ScaledPlayerAction::Surveil => {
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::LibraryTop(LibraryTopAction::Surveil(amt)),
            ))));
            Ok(())
        }
        v2::ScaledPlayerAction::Investigate => {
            let clue_def = token::lower_predefined_token_def(v2::PredefinedToken::Clue, ctx.vocab)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            let recipients = match player {
                v2::PlayerRef::You => TokenRecipients::Controller,
                v2::PlayerRef::EachPlayer => TokenRecipients::EachPlayer,
                v2::PlayerRef::EachOpponent => TokenRecipients::EachOpponent,
                other => TokenRecipients::Player(atoms::lower_resolving_player(other, acc, ctx)?),
            };
            acc.push(Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    enters_tapped: false,
                    definition: clue_def,
                    amount: amt,
                    base_size: None,
                    recipients,
                    attacking_defender: None,
                }],
            });
            Ok(())
        }
        v2::ScaledPlayerAction::Incubate => {
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::KeywordAction(PlayerKeywordAction::Incubate { count: amt }),
            ))));
            Ok(())
        }
        v2::ScaledPlayerAction::Discover => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:discover-non-controller"));
            }
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::StackOp(Box::new(StackOpSpec::discover(amt))));
            Ok(())
        }
        v2::ScaledPlayerAction::Bolster => {
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let Some(val) = value::lower_value_const(amount) else {
                return Err(LoweringError::Unsupported("v2:bolster-dynamic-amount"));
            };
            let n = u16::try_from(val)
                .map_err(|_| LoweringError::Unsupported("v2:bolster-amount-range"))?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::KeywordAction(PlayerKeywordAction::Bolster(n)),
            ))));
            Ok(())
        }
        v2::ScaledPlayerAction::CollectEvidence => {
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::collect_evidence(rp, amt),
            )));
            Ok(())
        }
        v2::ScaledPlayerAction::Blight => {
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::blight(
                rp, amt,
            ))));
            Ok(())
        }
        v2::ScaledPlayerAction::Connive => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:player-action-connive"));
            }
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                ObjectQuery::Source,
                EngineObjectMutation::Connive { count: Some(amt) },
            ))));
            Ok(())
        }
        v2::ScaledPlayerAction::Support => {
            Err(LoweringError::Unsupported("v2:player-action-support"))
        }
        v2::ScaledPlayerAction::Fateseal => {
            Err(LoweringError::Unsupported("v2:player-action-fateseal"))
        }
        v2::ScaledPlayerAction::Earthbend => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:earthbend-non-controller"));
            }
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.register_target(
                BodyTarget::filtered_permanent(EngineObjectFilter::land().controlled()),
                v2::TargetCount::Exact(1),
            )?;
            acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::earthbend(amt),
            )));
            Ok(())
        }
    }
}

fn lower_inline_pay_cost(
    player: v2::PlayerRef,
    cost: &v2::Cost,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match cost {
        v2::Cost::PayLife(amount) => {
            effect::register_value_expr_player_targets(amount, acc, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::AdjustLife(Box::new(AdjustLifeSpec::pay(
                ps, amt,
            ))));
            Ok(())
        }
        v2::Cost::PayEnergy(amount) => {
            effect::register_value_expr_player_targets(amount, acc, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::pay_counter(ps, EngineCounterKind::Energy, amt),
            )));
            Ok(())
        }
        v2::Cost::Mana(mc)
            if !mc.symbols.is_empty()
                && mc
                    .symbols
                    .iter()
                    .all(|sym| matches!(sym, v2::ManaSymbol::Energy)) =>
        {
            let count = i32::try_from(mc.symbols.len())
                .map_err(|_| LoweringError::Unsupported("v2:pay-energy-count-overflow"))?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = EngineValueExpr::literal(i64::from(count));
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::pay_counter(ps, EngineCounterKind::Energy, amt),
            )));
            Ok(())
        }
        v2::Cost::DiscardHand => {
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let mut spec = ZoneOpSpec::move_all(
                ObjectQuery::zone(QueryZone::Hand(Box::new(ps)), EngineObjectFilter::any()),
                EngineZoneDest::Graveyard {
                    semantics: GraveyardSemantics::Discard,
                },
            );
            let ok = ctx.alloc_binding()?;
            let moved = ctx.alloc_binding()?;
            spec.bind_moved = Some(moved);
            spec.bind_succeeded = Some(ok);
            ctx.last_moved_binding = Some(moved);
            ctx.last_succeeded_binding = Some(ok);
            acc.push(Instruction::ZoneOp(Box::new(spec)));
            Ok(())
        }
        v2::Cost::SacrificeSelf => {
            let mut spec = ZoneOpSpec::move_all(
                ObjectQuery::Source,
                EngineZoneDest::Graveyard {
                    semantics: GraveyardSemantics::Sacrifice,
                },
            );
            let ok = ctx.alloc_binding()?;
            let moved = ctx.alloc_binding()?;
            spec.bind_moved = Some(moved);
            spec.bind_succeeded = Some(ok);
            ctx.last_moved_binding = Some(moved);
            ctx.last_succeeded_binding = Some(ok);
            acc.push(Instruction::ZoneOp(Box::new(spec)));
            Ok(())
        }
        v2::Cost::ExileSelf => {
            let mut spec = ZoneOpSpec::move_all(ObjectQuery::Source, EngineZoneDest::exile());
            let ok = ctx.alloc_binding()?;
            let moved = ctx.alloc_binding()?;
            spec.bind_moved = Some(moved);
            spec.bind_succeeded = Some(ok);
            ctx.last_moved_binding = Some(moved);
            ctx.last_succeeded_binding = Some(ok);
            acc.push(Instruction::ZoneOp(Box::new(spec)));
            Ok(())
        }
        v2::Cost::TapSelf => emit_object_mutation(
            ObjectQuery::Source,
            EngineObjectMutation::TapState(TapStateOp::Set(true)),
            acc,
            ctx,
        ),
        v2::Cost::UntapSelf => emit_object_mutation(
            ObjectQuery::Source,
            EngineObjectMutation::TapState(TapStateOp::Set(false)),
            acc,
            ctx,
        ),
        v2::Cost::Mill(amount) => {
            effect::register_value_expr_player_targets(amount, acc, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            let mut spec = ZoneOpSpec::move_all(
                ObjectQuery::ZoneScan {
                    zone: QueryZone::Library(Box::new(ps)),
                    filter: Box::new(EngineObjectFilter::any()),
                    slice: QuerySlice::Top(Box::new(amt)),
                },
                EngineZoneDest::Graveyard {
                    semantics: GraveyardSemantics::Mill,
                },
            );
            let ok = ctx.alloc_binding()?;
            let moved = ctx.alloc_binding()?;
            spec.bind_moved = Some(moved);
            spec.bind_succeeded = Some(ok);
            ctx.last_moved_binding = Some(moved);
            ctx.last_succeeded_binding = Some(ok);
            acc.push(Instruction::ZoneOp(Box::new(spec)));
            Ok(())
        }
        v2::Cost::ExertSelf => emit_object_mutation(
            ObjectQuery::Source,
            EngineObjectMutation::TapState(TapStateOp::Exert),
            acc,
            ctx,
        ),
        v2::Cost::UnattachSelf => emit_object_mutation(
            ObjectQuery::Source,
            EngineObjectMutation::Attach(AttachOp::Unattach),
            acc,
            ctx,
        ),
        v2::Cost::CollectEvidence(amount) => {
            effect::register_value_expr_player_targets(amount, acc, ctx)?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::collect_evidence(rp, amt),
            )));
            Ok(())
        }
        v2::Cost::Blight(amount) => {
            effect::register_value_expr_player_targets(amount, acc, ctx)?;
            let rp = atoms::lower_resolving_player(player, acc, ctx)?;
            let amt = value::lower_value_expr(amount, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::blight(
                rp, amt,
            ))));
            Ok(())
        }
        v2::Cost::Behold(obj_filter) => {
            effect::register_filter_player_targets(obj_filter, acc, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
                ps,
                EnginePlayerMutation::Behold {
                    filter: Box::new(ef),
                    optional: false,
                },
            ))));
            Ok(())
        }
        v2::Cost::Sacrifice {
            count,
            filter: obj_filter,
        } => {
            let (f1, chosen_player) = strip_of_chosen_player_local(obj_filter);
            let (f_clean, is_random) = strip_at_random_local(&f1);
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            effect::register_filter_player_targets(&f_clean, acc, ctx)?;
            let ef = filter::lower_object_filter(&f_clean, ctx)?;
            let amt = value::lower_value_expr(count, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let chooser = match chosen_player {
                None | Some(v2::PlayerRef::ThatPlayer) => ps.clone(),
                Some(cp) if cp == player => ps.clone(),
                Some(cp) => atoms::lower_player_ref(cp, acc, ctx)?,
            };
            let source = ObjectQuery::zone(
                QueryZone::BattlefieldControlledBy(Box::new(ps)),
                ef,
            );
            let selection = if is_random {
                ZoneSelection::Random {
                    count: Box::new(amt),
                    chooser,
                }
            } else {
                ZoneSelection::Prompt {
                    chooser,
                    min: Box::new(amt.clone()),
                    max: Box::new(amt),
                    constraint: selection_constraint_local(&f_clean),
                    category: ChoiceCategory::Generic,
                    hidden: false,
                }
            };
            emit_cost_zone_op(
                source,
                selection,
                EngineZoneDest::Graveyard {
                    semantics: GraveyardSemantics::Sacrifice,
                },
                acc,
                ctx,
            )
        }
        v2::Cost::Discard {
            count,
            filter: opt_filter,
            random,
        } => {
            let (clean_filter, chosen_player, filter_random) = match opt_filter {
                Some(f) => {
                    let (f1, cp) = strip_of_chosen_player_local(f);
                    let (f2, rnd) = strip_at_random_local(&f1);
                    (Some(f2), cp, rnd)
                }
                None => (None, None, false),
            };
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            if let Some(f) = clean_filter.as_ref() {
                effect::register_filter_player_targets(f, acc, ctx)?;
            }
            let ef = match clean_filter.as_ref() {
                Some(f) => filter::lower_object_filter(f, ctx)?,
                None => EngineObjectFilter::any(),
            };
            let amt = value::lower_value_expr(count, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let chooser = match chosen_player {
                None | Some(v2::PlayerRef::ThatPlayer) => ps.clone(),
                Some(cp) if cp == player => ps.clone(),
                Some(cp) => atoms::lower_player_ref(cp, acc, ctx)?,
            };
            let hidden = chooser == ps;
            let source = ObjectQuery::zone(QueryZone::Hand(Box::new(ps)), ef);
            let selection = if *random || filter_random {
                ZoneSelection::Random {
                    count: Box::new(amt),
                    chooser,
                }
            } else {
                ZoneSelection::Prompt {
                    chooser,
                    min: Box::new(amt.clone()),
                    max: Box::new(amt),
                    constraint: clean_filter.as_ref().and_then(selection_constraint_local),
                    category: ChoiceCategory::Generic,
                    hidden,
                }
            };
            emit_cost_zone_op(
                source,
                selection,
                EngineZoneDest::Graveyard {
                    semantics: GraveyardSemantics::Discard,
                },
                acc,
                ctx,
            )
        }
        v2::Cost::Exile {
            count,
            filter: obj_filter,
            from,
        } => {
            let (f1, chosen_player) = strip_of_chosen_player_local(obj_filter);
            let (f_clean, is_random) = strip_at_random_local(&f1);
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            effect::register_filter_player_targets(&f_clean, acc, ctx)?;
            let ef = filter::lower_object_filter(&f_clean, ctx)?;
            let amt = value::lower_value_expr(count, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let chooser = match chosen_player {
                None | Some(v2::PlayerRef::ThatPlayer) => ps.clone(),
                Some(cp) if cp == player => ps.clone(),
                Some(cp) => atoms::lower_player_ref(cp, acc, ctx)?,
            };
            let zones: &[v2::ZoneRef] = if !from.is_empty() {
                from
            } else {
                &f_clean.zones
            };
            let source = if zones.is_empty() {
                if f_clean.any_card && f_clean.all_types.is_empty() && f_clean.any_types.is_empty()
                {
                    return Err(LoweringError::Unsupported("v2:pay-cost-exile-zoneless"));
                }
                ObjectQuery::zone(QueryZone::BattlefieldControlledBy(Box::new(ps)), ef)
            } else if zones.len() == 1 {
                let qz = atoms::lower_zone_ref(zones[0], acc, ctx)?;
                ObjectQuery::zone(qz, ef)
            } else {
                let mut parts = Vec::with_capacity(zones.len());
                for z in zones {
                    let qz = atoms::lower_zone_ref(*z, acc, ctx)?;
                    parts.push(ObjectQuery::zone(qz, ef));
                }
                ObjectQuery::Union(parts)
            };
            let selection = if is_random {
                ZoneSelection::Random {
                    count: Box::new(amt),
                    chooser,
                }
            } else {
                ZoneSelection::Prompt {
                    chooser,
                    min: Box::new(amt.clone()),
                    max: Box::new(amt),
                    constraint: selection_constraint_local(&f_clean),
                    category: ChoiceCategory::Generic,
                    hidden: false,
                }
            };
            emit_cost_zone_op(source, selection, EngineZoneDest::exile(), acc, ctx)
        }
        v2::Cost::TapUntapped {
            count,
            filter: obj_filter,
        } => {
            let mut f = obj_filter.clone();
            if f.tapped.is_none() {
                f.tapped = Some(false);
            }
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            effect::register_filter_player_targets(&f, acc, ctx)?;
            let ef = filter::lower_object_filter(&f, ctx)?;
            let amt = value::lower_value_expr(count, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let source = ObjectQuery::zone(
                QueryZone::BattlefieldControlledBy(Box::new(ps.clone())),
                ef,
            );
            let selection = ZoneSelection::Prompt {
                chooser: ps,
                min: Box::new(amt.clone()),
                max: Box::new(amt),
                constraint: selection_constraint_local(&f),
                category: ChoiceCategory::Generic,
                hidden: false,
            };
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::TapState(TapStateOp::Set(true)),
                acc,
                ctx,
            )
        }
        v2::Cost::UntapTapped {
            count,
            filter: obj_filter,
        } => {
            let mut f = obj_filter.clone();
            if f.tapped.is_none() {
                f.tapped = Some(true);
            }
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            effect::register_filter_player_targets(&f, acc, ctx)?;
            let ef = filter::lower_object_filter(&f, ctx)?;
            let amt = value::lower_value_expr(count, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let source = ObjectQuery::zone(
                QueryZone::BattlefieldControlledBy(Box::new(ps.clone())),
                ef,
            );
            let selection = ZoneSelection::Prompt {
                chooser: ps,
                min: Box::new(amt.clone()),
                max: Box::new(amt),
                constraint: selection_constraint_local(&f),
                category: ChoiceCategory::Generic,
                hidden: false,
            };
            emit_object_mutation_selected(
                source,
                selection,
                EngineObjectMutation::TapState(TapStateOp::Set(false)),
                acc,
                ctx,
            )
        }
        v2::Cost::ExileTop { zone, count } => {
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            let qz = atoms::lower_zone_ref(*zone, acc, ctx)?;
            let amt = value::lower_value_expr(count, ctx)?;
            let source = ObjectQuery::ZoneScan {
                zone: qz,
                filter: Box::new(EngineObjectFilter::any()),
                slice: QuerySlice::Top(Box::new(amt)),
            };
            emit_cost_zone_op(source, ZoneSelection::All, EngineZoneDest::exile(), acc, ctx)
        }
        v2::Cost::RevealFromHand {
            count,
            filter: obj_filter,
        } => {
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            effect::register_filter_player_targets(obj_filter, acc, ctx)?;
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            let amt = value::lower_value_expr(count, ctx)?;
            let ps = atoms::lower_player_ref(player, acc, ctx)?;
            let source = ObjectQuery::zone(QueryZone::Hand(Box::new(ps.clone())), ef);
            let selection = ZoneSelection::Prompt {
                chooser: ps,
                min: Box::new(amt.clone()),
                max: Box::new(amt),
                constraint: selection_constraint_local(obj_filter),
                category: ChoiceCategory::Generic,
                hidden: true,
            };
            emit_cost_zone_op(source, selection, EngineZoneDest::RevealOnly, acc, ctx)
        }
        v2::Cost::PutZone { subject, dest } => {
            let destination = atoms::lower_zone_dest(dest, &v2::EntryModifiers::default(), acc, ctx)?;
            let (source, selection) = lower_mutation_source_and_selection(subject, acc, ctx)?;
            emit_cost_zone_op(source, selection, destination, acc, ctx)
        }
        v2::Cost::ReturnToHand { subject } => {
            if let v2::SubjectRef::Counted { filter: f, count } = subject {
                let (f1, chosen_player) = strip_of_chosen_player_local(f);
                let (f_clean, is_random) = strip_at_random_local(&f1);
                effect::register_filter_player_targets(&f_clean, acc, ctx)?;
                let ef = filter::lower_object_filter(&f_clean, ctx)?;
                let ps = atoms::lower_player_ref(player, acc, ctx)?;
                let chooser = match chosen_player {
                    None | Some(v2::PlayerRef::ThatPlayer) => ps.clone(),
                    Some(cp) if cp == player => ps.clone(),
                    Some(cp) => atoms::lower_player_ref(cp, acc, ctx)?,
                };
                let source = if f_clean.zones.is_empty() {
                    ObjectQuery::zone(QueryZone::BattlefieldControlledBy(Box::new(ps)), ef)
                } else if f_clean.zones.len() == 1 {
                    ObjectQuery::zone(atoms::lower_zone_ref(f_clean.zones[0], acc, ctx)?, ef)
                } else {
                    let mut parts = Vec::with_capacity(f_clean.zones.len());
                    for z in &f_clean.zones {
                        parts.push(ObjectQuery::zone(atoms::lower_zone_ref(*z, acc, ctx)?, ef));
                    }
                    ObjectQuery::Union(parts)
                };
                let selection = lower_count_selection(
                    count,
                    chooser,
                    is_random,
                    selection_constraint_local(&f_clean),
                    acc,
                    ctx,
                )?;
                return emit_cost_zone_op(source, selection, EngineZoneDest::Hand, acc, ctx);
            }
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            emit_cost_zone_op(query, ZoneSelection::All, EngineZoneDest::Hand, acc, ctx)
        }
        v2::Cost::PutCounters {
            kind,
            count,
            subject,
        } => {
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            let query = lower_mutation_subject_query(subject, acc, ctx)?;
            let engine_kind = atoms::lower_counter_kind(kind)?;
            let amount = value::lower_value_expr(count, ctx)?;
            emit_object_mutation(
                query,
                EngineObjectMutation::Counters(CounterOp::Add {
                    kind: engine_kind,
                    amount,
                }),
                acc,
                ctx,
            )
        }
        v2::Cost::RemoveCounters {
            kind,
            count,
            subject,
        } => {
            effect::register_value_expr_player_targets(count, acc, ctx)?;
            let query = match subject {
                Some(subj) => lower_mutation_subject_query(subj, acc, ctx)?,
                None => ObjectQuery::Source,
            };
            let engine_kind = match kind {
                v2::CounterKind::Any => None,
                other => Some(atoms::lower_counter_kind(other)?),
            };
            let amount = value::lower_value_expr(count, ctx)?;
            emit_object_mutation(
                query,
                EngineObjectMutation::Counters(CounterOp::Remove {
                    kind: engine_kind,
                    amount,
                }),
                acc,
                ctx,
            )
        }
        _ => Err(LoweringError::Unsupported("v2:player-pay-cost-unsupported")),
    }
}

fn emit_object_mutation(
    query: ObjectQuery,
    op: EngineObjectMutation,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    emit_object_mutation_selected(query, ZoneSelection::All, op, acc, ctx)
}

fn emit_object_mutation_selected(
    source: ObjectQuery,
    selection: ZoneSelection,
    op: EngineObjectMutation,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let bind_as_last_moved = matches!(
        source,
        ObjectQuery::ZoneScan { .. }
            | ObjectQuery::Union(_)
            | ObjectQuery::Difference(_, _)
            | ObjectQuery::Intersect(_, _)
            | ObjectQuery::Filtered { .. }
            | ObjectQuery::Take { .. }
    ) || ctx.last_moved_binding.is_none();
    let mut spec = MutateObjectSpec::with_selection(source, selection, op);
    let ok = ctx.alloc_binding()?;
    if bind_as_last_moved {
        let mutated = ctx.alloc_binding()?;
        spec.bind_mutated = Some(mutated);
        ctx.last_moved_binding = Some(mutated);
    }
    spec.bind_succeeded = Some(ok);
    ctx.last_succeeded_binding = Some(ok);
    acc.push(Instruction::MutateObject(Box::new(spec)));
    Ok(())
}

fn emit_cost_zone_op(
    source: ObjectQuery,
    selection: ZoneSelection,
    destination: EngineZoneDest,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let ok = ctx.alloc_binding()?;
    let moved = ctx.alloc_binding()?;
    let unchosen = if matches!(
        selection,
        ZoneSelection::Prompt { .. } | ZoneSelection::Random { .. }
    ) {
        Some(ctx.alloc_binding()?)
    } else {
        None
    };
    let mut spec = ZoneOpSpec::simple(source, selection, destination);
    spec.bind_moved = Some(moved);
    spec.bind_succeeded = Some(ok);
    spec.bind_unchosen = unchosen;
    ctx.last_moved_binding = Some(moved);
    ctx.last_succeeded_binding = Some(ok);
    if let Some(uid) = unchosen {
        ctx.last_unchosen_binding = Some(uid);
    }
    acc.push(Instruction::ZoneOp(Box::new(spec)));
    Ok(())
}

fn lower_mutation_subject_query(
    subject: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    effect::register_subject_filter_player_targets(subject, acc, ctx)?;
    if matches!(subject, v2::SubjectRef::Player(v2::PlayerRef::ThatPlayer)) {
        if let Some(slot) = ctx.in_loop_object.or(ctx.last_moved_binding) {
            return Ok(ObjectQuery::Bound(slot));
        }
        if acc.target_set
            && matches!(
                acc.target,
                BodyTarget::Object { .. }
                    | BodyTarget::AnyTarget(_)
                    | BodyTarget::Spell { .. }
                    | BodyTarget::StackEntry(_)
            )
        {
            return Ok(ObjectQuery::Targets);
        }
        if let Some(Instruction::ApplyContinuous(prev)) = acc.instructions.last()
            && let Some(prev_q) = &prev.selector
        {
            return Ok(prev_q.clone());
        }
        if ctx.in_trigger {
            return Ok(ObjectQuery::TriggerSubject);
        }
    }
    atoms::lower_subject_to_query(subject, acc, ctx)
}

fn lower_mutation_source_and_selection(
    subject: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(ObjectQuery, ZoneSelection)> {
    if let v2::SubjectRef::Counted { filter: f, count } = subject {
        let (f1, chosen_player) = strip_of_chosen_player_local(f);
        let (f_clean, is_random) = strip_at_random_local(&f1);
        effect::register_filter_player_targets(&f_clean, acc, ctx)?;
        let ef = filter::lower_object_filter(&f_clean, ctx)?;
        let source = if f_clean.zones.is_empty() {
            if f_clean.any_spell {
                ObjectQuery::zone(QueryZone::Stack, ef)
            } else if f_clean.any_card
                && f_clean.all_types.is_empty()
                && f_clean.any_types.is_empty()
            {
                return Err(LoweringError::Unsupported("v2:subject:zoneless-card-group"));
            } else {
                ObjectQuery::zone(QueryZone::Battlefield, ef)
            }
        } else if f_clean.zones.len() == 1 {
            ObjectQuery::zone(atoms::lower_zone_ref(f_clean.zones[0], acc, ctx)?, ef)
        } else {
            let mut parts = Vec::with_capacity(f_clean.zones.len());
            for z in &f_clean.zones {
                parts.push(ObjectQuery::zone(atoms::lower_zone_ref(*z, acc, ctx)?, ef));
            }
            ObjectQuery::Union(parts)
        };
        let chooser = match chosen_player {
            None => PlayerSelector::Resolving(ResolvingPlayer::Controller),
            Some(cp) => atoms::lower_player_ref(cp, acc, ctx)?,
        };
        let selection = lower_count_selection(
            count,
            chooser,
            is_random,
            selection_constraint_local(&f_clean),
            acc,
            ctx,
        )?;
        return Ok((source, selection));
    }
    Ok((lower_mutation_subject_query(subject, acc, ctx)?, ZoneSelection::All))
}

fn lower_count_selection(
    count: &v2::CountSpec,
    chooser: PlayerSelector,
    is_random: bool,
    constraint: Option<SelectionConstraint>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ZoneSelection> {
    if !count.any_number {
        effect::register_value_expr_player_targets(&count.count, acc, ctx)?;
    }
    if is_random {
        if count.up_to || count.any_number {
            return Err(LoweringError::Unsupported("v2:mutation-random-up-to"));
        }
        let amt = value::lower_value_expr(&count.count, ctx)?;
        return Ok(ZoneSelection::Random {
            count: Box::new(amt),
            chooser,
        });
    }
    let (min, max) = if count.any_number {
        (
            EngineValueExpr::literal(0),
            EngineValueExpr::literal(i64::from(u16::MAX)),
        )
    } else {
        let max_e = value::lower_value_expr(&count.count, ctx)?;
        let min_e = if count.up_to {
            EngineValueExpr::literal(0)
        } else {
            max_e.clone()
        };
        (min_e, max_e)
    };
    Ok(ZoneSelection::Prompt {
        chooser,
        min: Box::new(min),
        max: Box::new(max),
        constraint,
        category: ChoiceCategory::Generic,
        hidden: false,
    })
}

fn try_emit_two_target_pair(
    first: &v2::SubjectRef,
    second: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<bool> {
    if !matches!(first, v2::SubjectRef::TargetObject { .. })
        || !matches!(second, v2::SubjectRef::TargetObject { .. })
    {
        return Ok(false);
    }
    let (bt1, tc1) = atoms::lower_subject_to_target(first, ctx)?;
    let (bt2, tc2) = atoms::lower_subject_to_target(second, ctx)?;
    if !matches!(
        bt1,
        BodyTarget::Object {
            zone: TargetZone::Battlefield,
            ..
        }
    ) || !matches!(
        bt2,
        BodyTarget::Object {
            zone: TargetZone::Battlefield,
            ..
        }
    ) || bt1 == bt2
        || !matches!(tc1, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
        || !matches!(tc2, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
    {
        return Ok(false);
    }
    let mut c1 = BodyAcc::new();
    c1.register_target(bt1, tc1)?;
    c1.push(Instruction::MutateObject(Box::new(
        MutateObjectSpec::remember_permanent_target(),
    )));
    let mut c2 = BodyAcc::new();
    c2.register_target(bt2, tc2)?;
    c2.push(Instruction::MutateObject(Box::new(
        MutateObjectSpec::remember_permanent_target(),
    )));
    let b1 = c1.into_body()?;
    let b2 = c2.into_body()?;
    acc.push(Instruction::FixedTargetClause(Box::new(b1)));
    acc.push(Instruction::FixedTargetClause(Box::new(b2)));
    Ok(true)
}

fn strip_of_chosen_player_local(
    f: &v2::ObjectFilter,
) -> (v2::ObjectFilter, Option<v2::PlayerRef>) {
    let mut found: Option<v2::PlayerRef> = None;
    for pred in &f.predicates {
        if let v2::FilterPredicate::OfChosenPlayer(p) = pred {
            if let Some(prev) = found {
                if prev != *p {
                    return (f.clone(), None);
                }
            } else {
                found = Some(*p);
            }
        }
    }
    let Some(player) = found else {
        return (f.clone(), None);
    };
    let mut cleaned = f.clone();
    cleaned
        .predicates
        .retain(|p| !matches!(p, v2::FilterPredicate::OfChosenPlayer(_)));
    (cleaned, Some(player))
}

fn strip_at_random_local(f: &v2::ObjectFilter) -> (v2::ObjectFilter, bool) {
    if !f.flags.contains(v2::FilterFlags::AT_RANDOM) {
        return (f.clone(), false);
    }
    let mut cleaned = f.clone();
    cleaned.flags = v2::FilterFlags(cleaned.flags.0 & !v2::FilterFlags::AT_RANDOM.0);
    (cleaned, true)
}

fn selection_constraint_local(f: &v2::ObjectFilter) -> Option<SelectionConstraint> {
    if f.flags.contains(v2::FilterFlags::DIFFERENT_NAMES) {
        Some(SelectionConstraint::DistinctNames)
    } else if f.flags.contains(v2::FilterFlags::DIFFERENT_POWERS) {
        Some(SelectionConstraint::DistinctPowers)
    } else {
        None
    }
}

fn require_instant_duration(duration: &v2::Duration, code: &'static str) -> LowerResult<()> {
    match duration {
        v2::Duration::Instant | v2::Duration::Permanent => Ok(()),
        _ => Err(LoweringError::Unsupported(code)),
    }
}

fn duration_default_eot(duration: &v2::Duration) -> LowerResult<ResolvedDuration> {
    match duration {
        v2::Duration::Instant => Ok(ResolvedDuration::UntilEndOfTurn),
        other => atoms::lower_duration(other),
    }
}

fn is_pure_color_filter(f: &v2::ObjectFilter) -> bool {
    !f.any_colors.is_empty()
        && f.all_types.is_empty()
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.owner.is_none()
        && f.zones.is_empty()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
        && !f.any_permanent
        && !f.any_card
        && !f.is_ability
}
