use super::*;

pub(super) fn lower_zone_moves_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::Destroy {
            subject,
            random: false,
            except: Some(ex),
        } => {
            let patched = apply_except_to_subject(subject, ex)?;
            lower_instruction(
                &InstructionAst::Destroy {
                    subject: Box::new(patched),
                    random: false,
                    except: None,
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::Destroy {
            subject,
            random: false,
            except: None,
        } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                        ObjectFilter::self_object(),
                        true,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                        ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                        true,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(emit_destroy_target(false));
                Ok(())
            }
            SubjectRefAst::TargetOneOf {
                options,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_target_one_of_permanent_body_target(
                    options, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder.instructions.push(emit_destroy_target(false));
                Ok(())
            }
            SubjectRefAst::TargetOneOf {
                options,
                count: TargetCountAst::UpTo(1),
            } => {
                let bt = lower_target_one_of_permanent_body_target(
                    options, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: 1,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if *n >= 2 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(max),
            } if *max > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *max,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
            } if !in_trigger => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                if *tc == TargetCountAst::UpToX {
                    builder.up_to_x_target_count = true;
                } else {
                    builder.x_target_count = true;
                }
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: u16::MAX,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::AnyNumber,
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: u16::MAX,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder.instructions.push(emit_destroy_target(false));
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None && builder.last_group_filter.is_some() =>
            {
                let obj = builder
                    .last_group_filter
                    .ok_or("oracle:unsupported-destroy")?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                        obj, true,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::TriggerSubject | SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && matches!(
                        builder.trigger_hint,
                        TriggerHint::CombatBlock | TriggerHint::DealtDamage
                    ) =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ));
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_subject(
                        EffectSubject::TriggerSubject,
                        false,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if builder.target == BodyTarget::None
                    && in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_subject(
                        EffectSubject::TriggerSubject,
                        false,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::SelfObject => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                        ObjectFilter::self_object(),
                        true,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                        obj, true,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::Group(f) if f.zone.is_none() => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                ensure_target_power_consumed(builder, &obj);
                builder.last_group_filter = Some(obj);
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_chosen(
                        ResolvingPlayer::Controller,
                        obj,
                        ValueExpr::literal(1),
                        true,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for it in items {
                    lower_instruction(
                        &InstructionAst::Destroy {
                            subject: Box::new(it.clone()),
                            random: false,
                            except: None,
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                Ok(())
            }
            _ => Err("oracle:unsupported-destroy"),
        },
        InstructionAst::Destroy {
            subject,
            random: true,
            except,
        } => {
            let eff_subj = if let Some(ex) = except {
                apply_except_to_subject(subject, ex)?
            } else {
                (**subject).clone()
            };
            match &eff_subj {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut trial = builder.clone();
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    bind_filter_player_target(&mut trial, &obj)?;
                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                obj,
                            ),
                            spellwake_engine::toolbox::ZoneSelection::Random {
                                count: Box::new(ValueExpr::literal(1)),
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                            },
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ));
                    *builder = trial;
                    Ok(())
                }
                _ => Err("oracle:unsupported-destroy"),
            }
        }
        InstructionAst::ModifyPreviousDestroyNoRegen => {
            if builder
                .instructions
                .iter_mut()
                .rev()
                .any(try_upgrade_destroy_no_regen)
            {
                return Ok(());
            }
            if let Some(pos) = builder.instructions.iter().rposition(|op| {
                matches!(
                    op,
                    Instruction::AffectPermanentTargets {
                        action: PermanentTargetAction::Destroy,
                        ..
                    }
                )
            }) {
                builder.instructions.insert(
                    pos,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                            BlockingRestriction::CannotBeRegenerated,
                        )]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                );
                return Ok(());
            }
            let dmg_no_regen_query = builder
                .instructions
                .iter()
                .rev()
                .find_map(find_damage_instruction_for_no_regen)
                .map(|dmg_op| {
                    if in_trigger || self_pronoun_valid {
                        Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(
                            Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause {
                                    any_types: CardTypeMask::CREATURE,
                                    flags: ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                                    ..ObjectClause::any()
                                },
                            ))),
                        ))
                    } else {
                        damage_no_regen_object_query(dmg_op, builder.target)
                    }
                });
            if let Some(Some(no_regen_query)) = dmg_no_regen_query {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        no_regen_query,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                            BlockingRestriction::CannotBeRegenerated,
                        )]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                );
                return Ok(());
            }
            Err("oracle:unsupported-no-regen-modifier")
        }
        InstructionAst::Exile {
            actor: None | Some(PlayerRefAst::You),
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } => {
            match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone,
                } => {
                    match inner.as_ref() {
                        s if matches!(s, SubjectRefAst::SelfObject)
                            || is_self_pronoun(s, builder, self_pronoun_valid) =>
                        {
                            match zone {
                                ZoneFilter::Battlefield => {
                                    if !in_trigger && !self_pronoun_valid {
                                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_resolving_spell());
                                    } else {
                                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_ability_source());
                                    }
                                    Ok(())
                                }
                                ZoneFilter::YourGraveyard
                                | ZoneFilter::YourHand
                                | ZoneFilter::AnyGraveyard
                                | ZoneFilter::YourLibrary
                                | ZoneFilter::Stack
                                | ZoneFilter::CommandZone
                                    if in_trigger || self_pronoun_valid || builder.has_source =>
                                {
                                    builder.instructions.push(
                                        super::super::toolbox_emit::move_subject(
                                            MoveSubjectRef::AbilitySource,
                                            SubjectDestination::Exile {
                                                delayed_return: None,
                                            },
                                        ),
                                    );
                                    Ok(())
                                }
                                _ => Err("oracle:unsupported-exile"),
                            }
                        }
                        SubjectRefAst::TriggerSubject
                            if in_trigger && *zone != ZoneFilter::OutsideTheGame =>
                        {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TriggeringObject, SubjectDestination::Exile {
                            delayed_return: None,
                        }));
                            Ok(())
                        }
                        SubjectRefAst::Anaphora(raw)
                            if matches!(
                                zone,
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::ThatPlayerGraveyard
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::OpponentsGraveyards
                                    | ZoneFilter::AllGraveyards
                            ) =>
                        {
                            let low = raw.trim().to_ascii_lowercase();
                            let is_singular = !matches!(
                                low.as_str(),
                                "them"
                                    | "those"
                                    | "those cards"
                                    | "one of them"
                                    | "each of them"
                                    | "all of them"
                                    | "the rest"
                            );
                            let is_plural_last_moved = matches!(
                                low.as_str(),
                                "them" | "those" | "those cards" | "all of them"
                            );
                            if !is_singular && !is_plural_last_moved {
                                return Err("oracle:unsupported-exile");
                            }
                            if has_prior_moved_object(&builder.instructions)
                                && !is_permanent_body_target(builder.target)
                            {
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::LastMoved, SubjectDestination::Exile {
                                delayed_return: None,
                            }));
                                Ok(())
                            } else if is_singular
                                && in_trigger
                                && builder.target == BodyTarget::None
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                            {
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TriggeringObject, SubjectDestination::Exile {
                                delayed_return: None,
                            }));
                                Ok(())
                            } else {
                                Err("oracle:unsupported-exile")
                            }
                        }
                        _ if *zone == ZoneFilter::Battlefield => lower_instruction(
                            &InstructionAst::Exile {
                                actor: None,
                                subject: inner.clone(),
                                random: false,
                                face_down: false,
                                into_pile: false,
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        ),
                        _ => {
                            let unwrapped = unwrap_in_zone_subject(subject)
                                .ok_or("oracle:unsupported-exile")?;
                            lower_instruction(
                                &InstructionAst::Exile {
                                    actor: None,
                                    subject: Box::new(unwrapped),
                                    random: false,
                                    face_down: false,
                                    into_pile: false,
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )
                        }
                    }
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && has_checked_revealed_top_strict(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::RevealedCard,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    if !in_trigger && !self_pronoun_valid {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_resolving_spell());
                    } else {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_ability_source());
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_subject(subject, vocabulary)?;
                    if filter.zone.is_some() && filter.zone != Some(ZoneFilter::Battlefield) {
                        if matches!(
                            bt,
                            BodyTarget::SingleOwnGraveyardCard
                                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                        ) {
                            builder.bind_target(bt)?;
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, SubjectDestination::Exile {
                                delayed_return: None,
                            }));
                        } else if matches!(
                            bt,
                            BodyTarget::AnyGraveyardCard
                                | BodyTarget::AnyGraveyardCreature
                                | BodyTarget::FilteredOwnGraveyardCard(_)
                                | BodyTarget::FilteredAnyGraveyardCard { .. }
                        ) {
                            builder.bind_target(bt)?;
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target());
                        } else {
                            return Err("oracle:unsupported-exile-gy-target");
                        }
                    } else if matches!(bt, BodyTarget::SingleSpell(_)) {
                        builder.bind_target(bt)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                SubjectDestination::Exile {
                                    delayed_return: None,
                                },
                            ),
                        );
                    } else {
                        builder.bind_target(bt)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::UpTo(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::Exile,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                    let mut clean = filter.as_ref().clone();
                    clean.zone = None;
                    let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action: PermanentTargetAction::Exile,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *max > 0 => {
                    let mut clean = filter.as_ref().clone();
                    clean.zone = None;
                    let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action: PermanentTargetAction::Exile,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::AnyNumber,
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut clean = filter.as_ref().clone();
                    clean.zone = None;
                    let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action: PermanentTargetAction::Exile,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && !in_trigger => {
                    let mut clean = filter.as_ref().clone();
                    clean.zone = None;
                    let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action: PermanentTargetAction::Exile,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if filter.zone.is_some() && *n >= 2 => {
                    let bt = match lower_target_subject(
                        &SubjectRefAst::TargetObject {
                            filter: filter.clone(),
                            count: TargetCountAst::Exact(1),
                        },
                        vocabulary,
                    )? {
                        BodyTarget::SingleOwnGraveyardCard => {
                            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::any())
                        }
                        other @ (BodyTarget::AnyGraveyardCard
                        | BodyTarget::AnyGraveyardCreature
                        | BodyTarget::FilteredOwnGraveyardCard(_)
                        | BodyTarget::FilteredAnyGraveyardCard { .. }) => other,
                        _ => return Err("oracle:unsupported-exile-gy-target"),
                    };
                    builder.bind_target(bt)?;
                    builder.graveyard_target_bounds = Some((usize::from(*n), usize::from(*n)));
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if filter.zone.is_some() && *max > 0 => {
                    let bt = match lower_target_subject(
                        &SubjectRefAst::TargetObject {
                            filter: filter.clone(),
                            count: TargetCountAst::Exact(1),
                        },
                        vocabulary,
                    )? {
                        BodyTarget::SingleOwnGraveyardCard => {
                            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::any())
                        }
                        other @ (BodyTarget::AnyGraveyardCard
                        | BodyTarget::AnyGraveyardCreature
                        | BodyTarget::FilteredOwnGraveyardCard(_)
                        | BodyTarget::FilteredAnyGraveyardCard { .. }) => other,
                        _ => return Err("oracle:unsupported-exile-gy-target"),
                    };
                    builder.bind_target(bt)?;
                    builder.graveyard_target_bounds = Some((0, usize::from(*max)));
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: count @ (TargetCountAst::ChosenX | TargetCountAst::AnyNumber),
                } if filter.zone.is_some()
                    && (*count != TargetCountAst::ChosenX || !in_trigger) =>
                {
                    let bt = match lower_target_subject(
                        &SubjectRefAst::TargetObject {
                            filter: filter.clone(),
                            count: TargetCountAst::Exact(1),
                        },
                        vocabulary,
                    )? {
                        BodyTarget::SingleOwnGraveyardCard => {
                            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::any())
                        }
                        other @ (BodyTarget::AnyGraveyardCard
                        | BodyTarget::AnyGraveyardCreature
                        | BodyTarget::FilteredOwnGraveyardCard(_)
                        | BodyTarget::FilteredAnyGraveyardCard { .. }) => other,
                        _ => return Err("oracle:unsupported-exile-gy-target"),
                    };
                    builder.bind_target(bt)?;
                    builder.graveyard_target_bounds = Some((0, usize::MAX));
                    if *count == TargetCountAst::ChosenX {
                        builder.x_target_count = true;
                    }
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if raw.trim().eq_ignore_ascii_case(
                        "all cards from target player's hand and graveyard",
                    ) =>
                {
                    let player = lower_resolving_player_in_context(
                        PlayerRefAst::TargetPlayer,
                        builder,
                        in_trigger,
                    )?;
                    // Both zones belong to one target and move in one simultaneous batch.
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_group_from_zone(AffectedPlayers::One(player), SourceZoneFilter::HandOrGraveyard, ObjectFilter::any()));
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "your hand"
                            | "target player's hand"
                            | "target opponent's hand"
                            | "that player's hand"
                            | "their hand"
                            | "defending player's hand"
                            | "active player's hand"
                            | "enchanted player's hand"
                            | "its controller's hand"
                            | "its owner's hand"
                            | "each opponent's hand"
                            | "your opponents' hands"
                            | "opponents' hands"
                            | "each other player's hand"
                            | "each other player's hands"
                            | "each player's hand"
                            | "all hands"
                    ) =>
                {
                    let low = raw.trim().to_ascii_lowercase();
                    let players = match low.as_str() {
                        "your hand" => AffectedPlayers::One(ResolvingPlayer::Controller),
                        "target player's hand" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::TargetPlayer,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "target opponent's hand" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::TargetOpponent,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "that player's hand" | "their hand" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayer,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "defending player's hand" => {
                            AffectedPlayers::One(ResolvingPlayer::DefendingPlayer)
                        }
                        "active player's hand" => {
                            AffectedPlayers::One(ResolvingPlayer::ActivePlayer)
                        }
                        "enchanted player's hand" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::EnchantedPlayer,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "its controller's hand" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::ItsController,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "its owner's hand" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::ItsOwner,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "each opponent's hand" | "your opponents' hands" | "opponents' hands" => {
                            AffectedPlayers::EachOpponent
                        }
                        "each other player's hand" | "each other player's hands" => {
                            AffectedPlayers::EachOtherPlayer
                        }
                        "each player's hand" | "all hands" => AffectedPlayers::EachPlayer,
                        _ => return Err("oracle:unsupported-exile"),
                    };
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::exile_group_from_zone(
                            players,
                            SourceZoneFilter::Hand,
                            ObjectFilter::any(),
                        ));
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "your graveyard"
                            | "target player's graveyard"
                            | "target opponent's graveyard"
                            | "that player's graveyard"
                            | "their graveyard"
                            | "defending player's graveyard"
                            | "active player's graveyard"
                            | "enchanted player's graveyard"
                            | "its controller's graveyard"
                            | "its owner's graveyard"
                            | "each opponent's graveyard"
                            | "your opponents' graveyards"
                            | "opponents' graveyards"
                            | "each other player's graveyard"
                            | "each other player's graveyards"
                            | "each player's graveyard"
                            | "all graveyards"
                    ) =>
                {
                    let low = raw.trim().to_ascii_lowercase();
                    match low.as_str() {
                        "each player's graveyard" | "all graveyards" => {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::exile_all_graveyards());
                        }
                        "your graveyard" => {
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(ResolvingPlayer::Controller),
                                ),
                            );
                        }
                        "target player's graveyard" => {
                            let p = lower_resolving_player_in_context(
                                PlayerRefAst::TargetPlayer,
                                builder,
                                in_trigger,
                            )?;
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(p),
                                ),
                            );
                        }
                        "target opponent's graveyard" => {
                            let p = lower_resolving_player_in_context(
                                PlayerRefAst::TargetOpponent,
                                builder,
                                in_trigger,
                            )?;
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(p),
                                ),
                            );
                        }
                        "that player's graveyard" | "their graveyard" => {
                            let p = lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayer,
                                builder,
                                in_trigger,
                            )?;
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(p),
                                ),
                            );
                        }
                        "defending player's graveyard" => {
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(ResolvingPlayer::DefendingPlayer),
                                ),
                            );
                        }
                        "active player's graveyard" => {
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(ResolvingPlayer::ActivePlayer),
                                ),
                            );
                        }
                        "enchanted player's graveyard" => {
                            let p = lower_resolving_player_in_context(
                                PlayerRefAst::EnchantedPlayer,
                                builder,
                                in_trigger,
                            )?;
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(p),
                                ),
                            );
                        }
                        "its controller's graveyard" => {
                            let p = lower_resolving_player_in_context(
                                PlayerRefAst::ItsController,
                                builder,
                                in_trigger,
                            )?;
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(p),
                                ),
                            );
                        }
                        "its owner's graveyard" => {
                            let p = lower_resolving_player_in_context(
                                PlayerRefAst::ItsOwner,
                                builder,
                                in_trigger,
                            )?;
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::One(p),
                                ),
                            );
                        }
                        "each opponent's graveyard"
                        | "your opponents' graveyards"
                        | "opponents' graveyards" => {
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::EachOpponent,
                                ),
                            );
                        }
                        "each other player's graveyard" | "each other player's graveyards" => {
                            builder.instructions.push(
                                super::super::toolbox_emit::exile_player_graveyards(
                                    AffectedPlayers::EachOtherPlayer,
                                ),
                            );
                        }
                        _ => return Err("oracle:unsupported-exile"),
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "your library"
                            | "target player's library"
                            | "target opponent's library"
                            | "that player's library"
                            | "their library"
                            | "defending player's library"
                            | "active player's library"
                            | "each opponent's library"
                            | "your opponents' libraries"
                            | "opponents' libraries"
                            | "each other player's library"
                            | "each player's library"
                            | "all libraries"
                    ) =>
                {
                    let low = raw.trim().to_ascii_lowercase();
                    let players = match low.as_str() {
                        "your library" => AffectedPlayers::One(ResolvingPlayer::Controller),
                        "target player's library" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::TargetPlayer,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "target opponent's library" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::TargetOpponent,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "that player's library" | "their library" => {
                            AffectedPlayers::One(lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayer,
                                builder,
                                in_trigger,
                            )?)
                        }
                        "defending player's library" => {
                            AffectedPlayers::One(ResolvingPlayer::DefendingPlayer)
                        }
                        "active player's library" => {
                            AffectedPlayers::One(ResolvingPlayer::ActivePlayer)
                        }
                        "each opponent's library"
                        | "your opponents' libraries"
                        | "opponents' libraries" => AffectedPlayers::EachOpponent,
                        "each other player's library" => AffectedPlayers::EachOtherPlayer,
                        "each player's library" | "all libraries" => AffectedPlayers::EachPlayer,
                        _ => return Err("oracle:unsupported-exile"),
                    };
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Group(players),
                                    )),
                                    ObjectFilter::any(),
                                ),
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePermanent
                            | BodyTarget::SingleCreature
                            | BodyTarget::SinglePlaneswalker
                            | BodyTarget::FilteredPermanent(_)
                    ) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::CreatedTokens,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::LastMoved,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if matches!(
                        builder.target,
                        BodyTarget::AnyGraveyardCard
                            | BodyTarget::AnyGraveyardCreature
                            | BodyTarget::FilteredOwnGraveyardCard(_)
                            | BodyTarget::FilteredAnyGraveyardCard { .. }
                    ) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if matches!(
                        builder.target,
                        BodyTarget::SingleOwnGraveyardCard
                            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                            | BodyTarget::SingleSpell(_)
                    ) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::TargetObject,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if has_checked_revealed_top_strict(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::RevealedCard,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::TriggeringObject,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::TriggeringObject,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                            ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                            PublicZoneDestination::Exile,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if try_lower_move_last_moved_filtered(
                        f,
                        SubjectDestination::Exile {
                            delayed_return: None,
                        },
                        builder,
                        vocabulary,
                    )? =>
                {
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if f.zone.is_none()
                        && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0 =>
                {
                    let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                            obj,
                            PublicZoneDestination::Exile,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    let mut inner = f.as_ref().clone();
                    let is_linked_exile = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                    let (players, zone) =
                        if is_linked_exile && matches!(f.zone, None | Some(ZoneFilter::Exile)) {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                            (
                                AffectedPlayers::One(ResolvingPlayer::Controller),
                                SourceZoneFilter::LinkedExile,
                            )
                        } else {
                            match f.zone {
                                Some(ZoneFilter::YourHand) => (
                                    AffectedPlayers::One(ResolvingPlayer::Controller),
                                    SourceZoneFilter::Hand,
                                ),
                                Some(ZoneFilter::TargetPlayerHand) => (
                                    AffectedPlayers::One(lower_resolving_player_in_context(
                                        PlayerRefAst::TargetPlayer,
                                        builder,
                                        in_trigger,
                                    )?),
                                    SourceZoneFilter::Hand,
                                ),
                                Some(ZoneFilter::TargetOpponentHand) => (
                                    AffectedPlayers::One(lower_resolving_player_in_context(
                                        PlayerRefAst::TargetOpponent,
                                        builder,
                                        in_trigger,
                                    )?),
                                    SourceZoneFilter::Hand,
                                ),
                                Some(ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands) => {
                                    (AffectedPlayers::EachOpponent, SourceZoneFilter::Hand)
                                }
                                Some(ZoneFilter::EachPlayerHand | ZoneFilter::AnyHand) => {
                                    (AffectedPlayers::EachPlayer, SourceZoneFilter::Hand)
                                }
                                Some(ZoneFilter::ThatPlayerHand) => (
                                    AffectedPlayers::One(lower_resolving_player_in_context(
                                        PlayerRefAst::ThatPlayer,
                                        builder,
                                        in_trigger,
                                    )?),
                                    SourceZoneFilter::Hand,
                                ),
                                Some(ZoneFilter::YourGraveyard) => (
                                    AffectedPlayers::One(ResolvingPlayer::Controller),
                                    SourceZoneFilter::Graveyard,
                                ),
                                Some(
                                    ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                                ) => (
                                    AffectedPlayers::EachOpponent,
                                    SourceZoneFilter::OpponentGraveyard,
                                ),
                                Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                                    (AffectedPlayers::EachPlayer, SourceZoneFilter::AnyGraveyard)
                                }
                                Some(ZoneFilter::ThatPlayerGraveyard) => (
                                    AffectedPlayers::One(lower_resolving_player_in_context(
                                        PlayerRefAst::ThatPlayer,
                                        builder,
                                        in_trigger,
                                    )?),
                                    SourceZoneFilter::Graveyard,
                                ),
                                Some(ZoneFilter::TargetPlayerGraveyard) => (
                                    AffectedPlayers::One(lower_resolving_player_in_context(
                                        PlayerRefAst::TargetPlayer,
                                        builder,
                                        in_trigger,
                                    )?),
                                    SourceZoneFilter::Graveyard,
                                ),
                                Some(ZoneFilter::TargetOpponentGraveyard) => (
                                    AffectedPlayers::One(lower_resolving_player_in_context(
                                        PlayerRefAst::TargetOpponent,
                                        builder,
                                        in_trigger,
                                    )?),
                                    SourceZoneFilter::Graveyard,
                                ),
                                Some(ZoneFilter::DefendingPlayerGraveyard) => (
                                    AffectedPlayers::One(ResolvingPlayer::DefendingPlayer),
                                    SourceZoneFilter::Graveyard,
                                ),
                                Some(ZoneFilter::YourLibrary) => (
                                    AffectedPlayers::One(ResolvingPlayer::Controller),
                                    SourceZoneFilter::YourLibrary,
                                ),
                                Some(ZoneFilter::Battlefield) => {
                                    (AffectedPlayers::EachPlayer, SourceZoneFilter::Battlefield)
                                }
                                Some(ZoneFilter::Exile)
                                    if matches!(
                                        f.controller,
                                        Some(
                                            ControllerRelation::Opponent
                                                | ControllerRelation::Opponents
                                                | ControllerRelation::OwnerOpponent
                                        )
                                    ) =>
                                {
                                    inner.controller = None;
                                    (
                                        AffectedPlayers::EachOpponent,
                                        SourceZoneFilter::OpponentExile,
                                    )
                                }
                                Some(other_zone) => {
                                    let mut trial = builder.clone();
                                    inner.zone = None;
                                    let obj = if is_unrestricted_filter(&inner) {
                                        ObjectFilter::any()
                                    } else {
                                        lower_object_filter_in_context(
                                            &inner, &trial, in_trigger, vocabulary,
                                        )?
                                    };
                                    bind_filter_player_target(&mut trial, &obj)?;
                                    let query = zone_filter_to_object_query(
                                        other_zone, obj, &mut trial, in_trigger,
                                    )
                                    .map_err(|_| "oracle:unsupported-exile")?;
                                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                            query,
                                            spellwake_engine::toolbox::ZoneDest::exile(),
                                        ),
                                    ));
                                    *builder = trial;
                                    return Ok(());
                                }
                                None => return Err("oracle:unsupported-exile"),
                            }
                        };
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    bind_filter_player_target(builder, &obj)?;
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_group_from_zone(players, zone, obj));
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if f.zone.is_none()
                        && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0
                        && f.controller == Some(ControllerRelation::You) =>
                {
                    if let Ok(ptf) = lower_permanent_type_filter(f, vocabulary) {
                        builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_chosen_permanents(AffectedPlayers::One(ResolvingPlayer::Controller), ValueExpr::literal(1), ptf));
                    } else {
                        let mut inner = f.as_ref().clone();
                        inner.controller = None;
                        let obj = lower_object_filter(&inner, vocabulary)?;
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(AffectedPlayers::One(ResolvingPlayer::Controller), spellwake_engine::activated::CostExileZone::Battlefield, ValueExpr::literal(1), obj));
                    }
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if matches!(
                        f.zone,
                        Some(ZoneFilter::YourHand | ZoneFilter::YourGraveyard)
                    ) =>
                {
                    let zone = if f.zone == Some(ZoneFilter::YourHand) {
                        spellwake_engine::activated::CostExileZone::Hand
                    } else {
                        spellwake_engine::activated::CostExileZone::Graveyard
                    };
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            zone,
                            ValueExpr::literal(1),
                            obj,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if ((f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                        && matches!(f.zone, None | Some(ZoneFilter::Exile)))
                        || matches!(
                            f.zone,
                            Some(
                                ZoneFilter::TargetPlayerHand
                                    | ZoneFilter::TargetOpponentHand
                                    | ZoneFilter::OpponentHand
                                    | ZoneFilter::OpponentsHands
                                    | ZoneFilter::EachPlayerHand
                                    | ZoneFilter::AnyHand
                                    | ZoneFilter::ThatPlayerHand
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::OpponentsGraveyards
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::AllGraveyards
                                    | ZoneFilter::ThatPlayerGraveyard
                                    | ZoneFilter::TargetPlayerGraveyard
                                    | ZoneFilter::TargetOpponentGraveyard
                                    | ZoneFilter::DefendingPlayerGraveyard
                            )
                        ) =>
                {
                    let mut inner = f.as_ref().clone();
                    let (players, zone) = if (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0)
                        != 0
                        && matches!(f.zone, None | Some(ZoneFilter::Exile))
                    {
                        inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        (
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            SourceZoneFilter::LinkedExile,
                        )
                    } else {
                        match f.zone {
                            Some(ZoneFilter::TargetPlayerHand) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::TargetPlayer,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Hand,
                            ),
                            Some(ZoneFilter::TargetOpponentHand) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::TargetOpponent,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Hand,
                            ),
                            Some(ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands) => {
                                (AffectedPlayers::EachOpponent, SourceZoneFilter::Hand)
                            }
                            Some(ZoneFilter::EachPlayerHand | ZoneFilter::AnyHand) => {
                                (AffectedPlayers::EachPlayer, SourceZoneFilter::Hand)
                            }
                            Some(ZoneFilter::ThatPlayerHand) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::ThatPlayer,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Hand,
                            ),
                            Some(
                                ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                            ) => (
                                AffectedPlayers::EachOpponent,
                                SourceZoneFilter::OpponentGraveyard,
                            ),
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                                (AffectedPlayers::EachPlayer, SourceZoneFilter::AnyGraveyard)
                            }
                            Some(ZoneFilter::ThatPlayerGraveyard) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::ThatPlayer,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Graveyard,
                            ),
                            Some(ZoneFilter::TargetPlayerGraveyard) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::TargetPlayer,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Graveyard,
                            ),
                            Some(ZoneFilter::TargetOpponentGraveyard) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::TargetOpponent,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Graveyard,
                            ),
                            Some(ZoneFilter::DefendingPlayerGraveyard) => (
                                AffectedPlayers::One(ResolvingPlayer::DefendingPlayer),
                                SourceZoneFilter::Graveyard,
                            ),
                            Some(ZoneFilter::SingleGraveyard) => {
                                (AffectedPlayers::EachPlayer, SourceZoneFilter::AnyGraveyard)
                            }
                            Some(ZoneFilter::ItsOwnerGraveyard) => (
                                AffectedPlayers::One(lower_resolving_player_in_context(
                                    PlayerRefAst::ItsOwner,
                                    builder,
                                    in_trigger,
                                )?),
                                SourceZoneFilter::Graveyard,
                            ),
                            Some(other_zone) => {
                                inner.zone = None;
                                let obj = if is_unrestricted_filter(&inner) {
                                    ObjectFilter::any()
                                } else {
                                    lower_object_filter_in_context(
                                        &inner, builder, in_trigger, vocabulary,
                                    )?
                                };
                                bind_filter_player_target(builder, &obj)?;
                                let query = zone_filter_to_object_query(
                                    other_zone, obj, builder, in_trigger,
                                )
                                .map_err(|_| "oracle:unsupported-exile")?;
                                builder
                                    .instructions
                                    .push(super::super::toolbox_emit::zone_op(
                                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                            query,
                                            spellwake_engine::toolbox::ZoneDest::exile(),
                                        ),
                                    ));
                                return Ok(());
                            }
                            None => return Err("oracle:unsupported-exile"),
                        }
                    };
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    bind_filter_player_target(builder, &obj)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::exile_group_from_zone(
                            players, zone, obj,
                        ));
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if f.zone.is_none()
                        && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0
                        && f.controller != Some(ControllerRelation::You)
                        && !f.any_spell
                        && !f.is_ability =>
                {
                    let mut trial = builder.clone();
                    let obj = lower_object_filter_in_context(f, &trial, in_trigger, vocabulary)?;
                    bind_filter_player_target(&mut trial, &obj)?;
                    trial
                        .instructions
                        .push(super::super::toolbox_emit::select_zone_cards_to_zone(
                            ResolvingPlayer::Controller,
                            SourceZoneFilter::AnyBattlefield,
                            obj,
                            ValueExpr::literal(1),
                            ValueExpr::literal(1),
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                            false,
                        ));
                    *builder = trial;
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if f.zone.is_some()
                        && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0 =>
                {
                    let Some(z) = f.zone else {
                        return Err("oracle:unsupported-exile");
                    };
                    let mut trial = builder.clone();
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    bind_filter_player_target(&mut trial, &obj)?;
                    let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                        .map_err(|_| "oracle:unsupported-exile")?;
                    let hidden = is_hidden_zone_filter(z);
                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                            query,
                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                                min: Box::new(ValueExpr::literal(1)),
                                max: Box::new(ValueExpr::literal(1)),
                                constraint: None,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                hidden,
                            },
                            spellwake_engine::toolbox::ZoneDest::exile(),
                        ),
                    ));
                    *builder = trial;
                    Ok(())
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    let mut shared_player_target: Option<BodyTarget> = None;
                    for (idx, it) in items.iter().enumerate() {
                        let prev_target = builder.target;
                        let mut eff_it = it.clone();
                        if idx > 0
                            && let Some(bound_player_bt) = shared_player_target
                        {
                            let rewrite_zone =
                                |z: &mut Option<ZoneFilter>| match (*z, bound_player_bt) {
                                    (
                                        Some(ZoneFilter::TargetPlayerHand),
                                        BodyTarget::SinglePlayer,
                                    ) => {
                                        *z = Some(ZoneFilter::ThatPlayerHand);
                                    }
                                    (
                                        Some(ZoneFilter::TargetPlayerGraveyard),
                                        BodyTarget::SinglePlayer,
                                    ) => {
                                        *z = Some(ZoneFilter::ThatPlayerGraveyard);
                                    }
                                    (
                                        Some(ZoneFilter::TargetOpponentHand),
                                        BodyTarget::SingleOpponent,
                                    ) => {
                                        *z = Some(ZoneFilter::ThatPlayerHand);
                                    }
                                    (
                                        Some(ZoneFilter::TargetOpponentGraveyard),
                                        BodyTarget::SingleOpponent,
                                    ) => {
                                        *z = Some(ZoneFilter::ThatPlayerGraveyard);
                                    }
                                    _ => {}
                                };
                            match &mut eff_it {
                                SubjectRefAst::All(f)
                                | SubjectRefAst::Each(f)
                                | SubjectRefAst::Group(f) => rewrite_zone(&mut f.zone),
                                SubjectRefAst::Anaphora(raw) => {
                                    let l = raw.trim().to_ascii_lowercase();
                                    if bound_player_bt == BodyTarget::SinglePlayer
                                        && l == "target player's graveyard"
                                    {
                                        *raw = "that player's graveyard".to_string();
                                    } else if bound_player_bt == BodyTarget::SinglePlayer
                                        && l == "target player's hand"
                                    {
                                        *raw = "that player's hand".to_string();
                                    } else if bound_player_bt == BodyTarget::SingleOpponent
                                        && l == "target opponent's graveyard"
                                    {
                                        *raw = "that player's graveyard".to_string();
                                    } else if bound_player_bt == BodyTarget::SingleOpponent
                                        && l == "target opponent's hand"
                                    {
                                        *raw = "that player's hand".to_string();
                                    }
                                }
                                _ => {}
                            }
                        }
                        lower_instruction(
                            &InstructionAst::Exile {
                                actor: None,
                                subject: Box::new(eff_it),
                                random: false,
                                face_down: false,
                                into_pile: false,
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                        if idx == 0
                            && prev_target == BodyTarget::None
                            && matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            )
                        {
                            shared_player_target = Some(builder.target);
                        }
                    }
                    Ok(())
                }
                SubjectRefAst::Group(f) if f.zone.is_none() => {
                    let mut trial = builder.clone();
                    let mut inner = f.as_ref().clone();
                    let is_linked = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                    if is_linked {
                        inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                    }
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    bind_filter_player_target(&mut trial, &obj)?;
                    let query = if is_linked {
                        if is_unrestricted_filter(&inner) {
                            spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                        } else {
                            spellwake_engine::toolbox::ObjectQuery::filtered(
                                spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None },
                                obj,
                            )
                        }
                    } else {
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            obj,
                        )
                    };
                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                            query,
                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                                min: Box::new(ValueExpr::literal(1)),
                                max: Box::new(ValueExpr::literal(1)),
                                constraint: None,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                hidden: false,
                            },
                            spellwake_engine::toolbox::ZoneDest::exile(),
                        ),
                    ));
                    *builder = trial;
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw) => {
                    let mut trial = builder.clone();
                    if let Some(query) =
                        parse_whole_zone_anaphora_query(raw, &mut trial, in_trigger)?
                    {
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                query,
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                        *builder = trial;
                        Ok(())
                    } else {
                        Err("oracle:unsupported-exile")
                    }
                }
                _ => Err("oracle:unsupported-exile"),
            }
        }
        InstructionAst::Exile {
            actor: Some(actor_player),
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } => {
            if matches!(subject.as_ref(), SubjectRefAst::InZone { .. }) {
                let unwrapped =
                    unwrap_in_zone_subject(subject).ok_or("oracle:unsupported-exile")?;
                return lower_instruction(
                    &InstructionAst::Exile {
                        actor: Some(*actor_player),
                        subject: Box::new(unwrapped),
                        random: false,
                        face_down: false,
                        into_pile: false,
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && !items.is_empty()
            {
                for (idx, it) in items.iter().enumerate() {
                    let next_actor = if idx > 0
                        && matches!(
                            actor_player,
                            PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                        ) {
                        PlayerRefAst::ThatPlayer
                    } else {
                        *actor_player
                    };
                    lower_instruction(
                        &InstructionAst::Exile {
                            actor: Some(next_actor),
                            subject: Box::new(it.clone()),
                            random: false,
                            face_down: false,
                            into_pile: false,
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                return Ok(());
            }
            let players = lower_affected_players_in_context(*actor_player, builder, in_trigger)?;
            match subject.as_ref() {
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "their hand" | "your hand" | "that player's hand"
                    ) =>
                {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_group_from_zone(players, SourceZoneFilter::Hand, ObjectFilter::any()));
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "their graveyard" | "your graveyard" | "that player's graveyard"
                    ) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(players));
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw) => {
                    let low = raw.trim().to_ascii_lowercase();
                    if matches!(
                        low.as_str(),
                        "it" | "that card"
                            | "those cards"
                            | "them"
                            | "the chosen card"
                            | "the chosen cards"
                            | "that creature"
                            | "that permanent"
                            | "that"
                    ) {
                        if last_instruction_moved_object(&builder.instructions)
                            || has_prior_moved_object(&builder.instructions)
                        {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                    MoveSubjectRef::LastMoved,
                                    SubjectDestination::Exile {
                                        delayed_return: None,
                                    },
                                ),
                            );
                            Ok(())
                        } else if builder.target != BodyTarget::None
                            && !matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            )
                        {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                            );
                            Ok(())
                        } else if last_instruction_created_tokens(&builder.instructions) {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                    MoveSubjectRef::CreatedTokens,
                                    SubjectDestination::Exile {
                                        delayed_return: None,
                                    },
                                ),
                            );
                            Ok(())
                        } else if in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                    MoveSubjectRef::TriggeringObject,
                                    SubjectDestination::Exile {
                                        delayed_return: None,
                                    },
                                ),
                            );
                            Ok(())
                        } else if has_checked_revealed_top_strict(&builder.instructions) {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                    MoveSubjectRef::RevealedCard,
                                    SubjectDestination::Exile {
                                        delayed_return: None,
                                    },
                                ),
                            );
                            Ok(())
                        } else {
                            Err("oracle:unsupported-exile")
                        }
                    } else {
                        Err("oracle:unsupported-exile")
                    }
                }
                SubjectRefAst::Group(f)
                    if matches!(
                        f.zone,
                        Some(
                            ZoneFilter::YourHand
                                | ZoneFilter::ThatPlayerHand
                                | ZoneFilter::TargetPlayerHand
                                | ZoneFilter::TargetOpponentHand
                        )
                    ) =>
                {
                    let mut inner = strip_matching_chooser(f, *actor_player);
                    inner.zone = None;
                    if matches!(
                        inner.controller,
                        Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
                    ) {
                        inner.controller = None;
                    }
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(
                            players,
                            spellwake_engine::activated::CostExileZone::Hand,
                            ValueExpr::literal(1),
                            obj,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if matches!(
                        f.zone,
                        Some(
                            ZoneFilter::YourGraveyard
                                | ZoneFilter::ThatPlayerGraveyard
                                | ZoneFilter::TargetPlayerGraveyard
                                | ZoneFilter::TargetOpponentGraveyard
                                | ZoneFilter::DefendingPlayerGraveyard
                        )
                    ) =>
                {
                    let mut inner = strip_matching_chooser(f, *actor_player);
                    inner.zone = None;
                    if matches!(
                        inner.controller,
                        Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
                    ) {
                        inner.controller = None;
                    }
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(
                            players,
                            spellwake_engine::activated::CostExileZone::Graveyard,
                            ValueExpr::literal(1),
                            obj,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut inner = strip_matching_chooser(f, *actor_player);
                    inner.zone = None;
                    if matches!(
                        inner.controller,
                        Some(
                            ControllerRelation::You
                                | ControllerRelation::ThatPlayer
                                | ControllerRelation::DefendingPlayer
                                | ControllerRelation::ItsController
                        )
                    ) {
                        inner.controller = None;
                    }
                    if let Ok(ptf) = lower_permanent_type_filter(&inner, vocabulary) {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_chosen_permanents(players, ValueExpr::literal(1), ptf));
                    } else {
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(players, spellwake_engine::activated::CostExileZone::Battlefield, ValueExpr::literal(1), obj));
                    }
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    let zone = match f.zone {
                        Some(
                            ZoneFilter::YourHand
                            | ZoneFilter::ThatPlayerHand
                            | ZoneFilter::TargetPlayerHand
                            | ZoneFilter::TargetOpponentHand
                            | ZoneFilter::OpponentHand
                            | ZoneFilter::OpponentsHands
                            | ZoneFilter::EachPlayerHand
                            | ZoneFilter::AnyHand,
                        ) => SourceZoneFilter::Hand,
                        Some(
                            ZoneFilter::YourGraveyard
                            | ZoneFilter::ThatPlayerGraveyard
                            | ZoneFilter::TargetPlayerGraveyard
                            | ZoneFilter::TargetOpponentGraveyard
                            | ZoneFilter::DefendingPlayerGraveyard,
                        ) => SourceZoneFilter::Graveyard,
                        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
                            SourceZoneFilter::OpponentGraveyard
                        }
                        Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                            SourceZoneFilter::AnyGraveyard
                        }
                        Some(ZoneFilter::YourLibrary) => SourceZoneFilter::YourLibrary,
                        Some(ZoneFilter::SingleGraveyard) => SourceZoneFilter::AnyGraveyard,
                        Some(ZoneFilter::YourHandOrGraveyard) => SourceZoneFilter::HandOrGraveyard,
                        Some(ZoneFilter::CommandZone) => SourceZoneFilter::CommandZone,
                        None | Some(ZoneFilter::Battlefield) => SourceZoneFilter::Battlefield,
                        Some(other_zone) => {
                            let mut inner = strip_matching_chooser(f, *actor_player);
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter_in_context(
                                    &inner, builder, in_trigger, vocabulary,
                                )?
                            };
                            bind_filter_player_target(builder, &obj)?;
                            let query =
                                zone_filter_to_object_query(other_zone, obj, builder, in_trigger)
                                    .map_err(|_| "oracle:unsupported-exile")?;
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::zone_op(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        query,
                                        spellwake_engine::toolbox::ZoneDest::exile(),
                                    ),
                                ));
                            return Ok(());
                        }
                    };
                    let mut inner = strip_matching_chooser(f, *actor_player);
                    inner.zone = None;
                    if zone == SourceZoneFilter::Battlefield
                        && matches!(
                            inner.controller,
                            Some(
                                ControllerRelation::You
                                    | ControllerRelation::ThatPlayer
                                    | ControllerRelation::DefendingPlayer
                                    | ControllerRelation::ItsController
                            )
                        )
                    {
                        inner.controller = None;
                    }
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    bind_filter_player_target(builder, &obj)?;
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_group_from_zone(players, zone, obj));
                    Ok(())
                }
                SubjectRefAst::Group(f) if f.zone.is_some() => {
                    let Some(z) = f.zone else {
                        return Err("oracle:unsupported-exile");
                    };
                    let mut trial = builder.clone();
                    let mut inner = strip_matching_chooser(f, *actor_player);
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    bind_filter_player_target(&mut trial, &obj)?;
                    let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                        .map_err(|_| "oracle:unsupported-exile")?;
                    let hidden = is_hidden_zone_filter(z);
                    if let AffectedPlayers::One(rp) = players {
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                query,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        rp,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden,
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    } else {
                        const SEAT_BIND: spellwake_engine::toolbox::BindingId =
                            spellwake_engine::toolbox::BindingId(906);
                        let inner_op = super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                query,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Bound(
                                        SEAT_BIND,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden,
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        );
                        trial.instructions.push(Instruction::ForEachPlayerQuery {
                            selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                                players,
                            )),
                            bind_player: SEAT_BIND,
                            body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner_op])),
                        });
                    }
                    *builder = trial;
                    Ok(())
                }
                SubjectRefAst::Group(f) if f.zone.is_none() => {
                    let mut trial = builder.clone();
                    let inner = strip_matching_chooser(f, *actor_player);
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    bind_filter_player_target(&mut trial, &obj)?;
                    if let AffectedPlayers::One(rp) = players {
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Battlefield,
                                    obj,
                                ),
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        rp,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    } else {
                        const SEAT_BIND: spellwake_engine::toolbox::BindingId =
                            spellwake_engine::toolbox::BindingId(935);
                        let scoped_bf = if inner.controller.is_none() {
                            spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Bound(SEAT_BIND),
                            ))
                        } else {
                            spellwake_engine::toolbox::QueryZone::Battlefield
                        };
                        let inner_op = super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                spellwake_engine::toolbox::ObjectQuery::zone(scoped_bf, obj),
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Bound(
                                        SEAT_BIND,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        );
                        trial.instructions.push(Instruction::ForEachPlayerQuery {
                            selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                                players,
                            )),
                            bind_player: SEAT_BIND,
                            body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner_op])),
                        });
                    }
                    *builder = trial;
                    Ok(())
                }
                _ => Err("oracle:unsupported-exile"),
            }
        }
        InstructionAst::Exile {
            actor,
            subject,
            random,
            face_down,
            into_pile: false,
        } if *random || *face_down => lower_exile_random_or_face_down(
            *actor,
            subject,
            *random,
            *face_down,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
        InstructionAst::ExileUntilLeaves { subject } => {
            if !(in_trigger || self_pronoun_valid) {
                return Err("oracle:unsupported-target-subject");
            }
            let eff_subj = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } => inner.as_ref(),
                other => other,
            };
            let exile_until_dest = SubjectDestination::Exile {
                delayed_return: Some(
                    spellwake_engine::program::DelayedReturnTiming::UntilSourceLeaves,
                ),
            };
            match eff_subj {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::AbilitySource,
                            exile_until_dest,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::TriggeringObject,
                            exile_until_dest,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::AttachedHost,
                            exile_until_dest,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::exile_target_until_source_leaves());
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::CreatedTokens,
                            exile_until_dest,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::LastMoved,
                            exile_until_dest,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && builder.target == BodyTarget::None
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::TriggeringObject,
                            exile_until_dest,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::exile_target_until_source_leaves());
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::exile_target_until_source_leaves());
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::UpTo(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::MoveTo(exile_until_dest),
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                {
                    let (minimum, maximum, is_x) = match *count {
                        TargetCountAst::Exact(n) => (n, n, false),
                        TargetCountAst::UpTo(max) => (0, max, false),
                        TargetCountAst::AnyNumber => (0, u16::MAX, false),
                        TargetCountAst::ChosenX => (0, u16::MAX, true),
                        _ => return Err("oracle:unsupported-target-subject"),
                    };
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if is_x {
                        builder.x_target_count = true;
                    }
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum,
                            maximum,
                            action: PermanentTargetAction::MoveTo(exile_until_dest),
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_subject_until_source_leaves(EffectSubject::Matching(obj)));
                    Ok(())
                }
                SubjectRefAst::Compound(items) if items.len() == 2 => {
                    let first = match &items[0] {
                        SubjectRefAst::InZone {
                            subject: inner,
                            zone: ZoneFilter::Battlefield,
                        } => inner.as_ref(),
                        other => other,
                    };
                    let second = match &items[1] {
                        SubjectRefAst::InZone {
                            subject: inner,
                            zone: ZoneFilter::Battlefield,
                        } => inner.as_ref(),
                        other => other,
                    };
                    let (
                        SubjectRefAst::TargetObject {
                            filter: f1,
                            count: TargetCountAst::Exact(1),
                        },
                        SubjectRefAst::All(f2) | SubjectRefAst::Each(f2),
                    ) = (first, second)
                    else {
                        return Err("oracle:unsupported-target-subject");
                    };
                    if !matches!(f1.zone, None | Some(ZoneFilter::Battlefield))
                        || !matches!(f2.zone, None | Some(ZoneFilter::Battlefield))
                    {
                        return Err("oracle:unsupported-target-subject");
                    }
                    let same_name_target = matches!(
                        &f2.named,
                        Some(spellwake_oracle_compiler::ast::NameFilterAst::SameAsReferenced)
                    ) || matches!(
                        &f2.named,
                        Some(spellwake_oracle_compiler::ast::NameFilterAst::SameAs(ref_subj))
                            if matches!(
                                ref_subj.as_ref(),
                                SubjectRefAst::Anaphora(_) | SubjectRefAst::TargetObject { .. }
                            )
                    );
                    if !same_name_target {
                        return Err("oracle:unsupported-target-subject");
                    }
                    let mut f1_clean = f1.as_ref().clone();
                    f1_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f1_clean, builder, in_trigger, vocabulary,
                    )?;
                    let mut f2_clean = f2.as_ref().clone();
                    f2_clean.zone = None;
                    f2_clean.named = None;
                    f2_clean.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
                    let obj =
                        lower_object_filter_in_context(&f2_clean, builder, in_trigger, vocabulary)?;
                    if obj.has_target_player() {
                        return Err("oracle:unsupported-target-subject");
                    }
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_subject_until_source_leaves(EffectSubject::TargetAndSameNameMatching(obj)));
                    Ok(())
                }
                _ => Err("oracle:unsupported-target-subject"),
            }
        }
        InstructionAst::ExileGraveyard { whose } => {
            let w = whose.trim().to_ascii_lowercase();
            match w.as_str() {
                "all" | "each player" | "each player's" | "all players" | "all players'" => {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_all_graveyards());
                    Ok(())
                }
                "all creatures and" => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                            ObjectFilter::creature(),
                            PublicZoneDestination::Exile,
                        ),
                    );
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_all_graveyards());
                    Ok(())
                }
                "your" => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::Controller,
                        )));
                    Ok(())
                }
                "each opponent" | "each opponent's" | "all opponents" | "all opponents'"
                | "opponents" | "opponents'" | "your opponents" | "your opponents'" => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::EachOpponent,));
                    Ok(())
                }
                "defending player" | "defending player's" => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::DefendingPlayer,
                        )));
                    Ok(())
                }
                "target player" | "target player's" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "target opponent" | "target opponent's" => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "target card from a" => {
                    builder.bind_target(BodyTarget::AnyGraveyardCard)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target creature card from a" => {
                    builder.bind_target(BodyTarget::AnyGraveyardCreature)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target creature card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::creature(),
                    ))?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target card from your" => {
                    builder
                        .bind_target(BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::any()))?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target noncreature, nonland card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::from_clause(
                            ObjectClause::any()
                                .forbid_types(CardTypeMask::CREATURE.union(CardTypeMask::LAND)),
                        ),
                    ))?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target instant or sorcery card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::instant_or_sorcery(),
                    ))?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target artifact card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::artifact(),
                    ))?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "target enchantment card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::enchantment(),
                    ))?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
                "~ from your" => {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_ability_source());
                    Ok(())
                }
                "that card from your" => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Intersect(
                                    Box::new(
                                        spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                    ),
                                    Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Group(
                                                AffectedPlayers::EachPlayer,
                                            ),
                                        )),
                                        ObjectFilter::any(),
                                    )),
                                ),
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    Ok(())
                }
                "that player" | "that player's" if in_trigger => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::UniqueRelatedPlayer,
                        )));
                    Ok(())
                }
                "that player" | "that player's"
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "that player" | "that player's" | "their" | "their graveyard" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(rp)));
                    Ok(())
                }
                "its controller" | "its controller's" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ItsController,
                        builder,
                        in_trigger,
                    )?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(rp)));
                    Ok(())
                }
                "its owner" | "its owner's" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ItsOwner,
                        builder,
                        in_trigger,
                    )?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(rp)));
                    Ok(())
                }
                "the bottom card of your" => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Graveyard(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        ),
                                    ),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::Bottom(Box::new(
                                        ValueExpr::literal(1),
                                    )),
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    Ok(())
                }
                "the bottom card of target player's" | "the bottom card of target player" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Graveyard(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TargetPlayer,
                                            ),
                                        ),
                                    ),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::Bottom(Box::new(
                                        ValueExpr::literal(1),
                                    )),
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    Ok(())
                }
                "the bottom card of target opponent's" | "the bottom card of target opponent" => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Graveyard(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TargetPlayer,
                                            ),
                                        ),
                                    ),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::Bottom(Box::new(
                                        ValueExpr::literal(1),
                                    )),
                                },
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ));
                    Ok(())
                }
                "a creature they control and their"
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(
                            AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                            spellwake_engine::activated::CostExileZone::Battlefield,
                            ValueExpr::literal(1),
                            ObjectFilter::creature(),
                        ),
                    );
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "all cards from target player's hand and" | "target player's hand and" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(
                            AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                            spellwake_engine::activated::CostExileZone::Hand,
                            ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(
                                ResolvingPlayer::TargetPlayer,
                            )),
                            ObjectFilter::any(),
                        ),
                    );
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_player_graveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "a card from your" | "one card from your" => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_cards_from_graveyard(ResolvingPlayer::Controller, NonZeroU8::new(1).ok_or("oracle:unreachable")?));
                    Ok(())
                }
                "two cards from your" => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_cards_from_graveyard(ResolvingPlayer::Controller, NonZeroU8::new(2).ok_or("oracle:unreachable")?));
                    Ok(())
                }
                "three cards from your" => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_cards_from_graveyard(ResolvingPlayer::Controller, NonZeroU8::new(3).ok_or("oracle:unreachable")?));
                    Ok(())
                }
                "a card from their" | "one card from their" if in_trigger => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_cards_from_graveyard(ResolvingPlayer::UniqueRelatedPlayer, NonZeroU8::new(1).ok_or("oracle:unreachable")?));
                    Ok(())
                }
                other => {
                    // Non-targeted "a/one/two/three <filter> card(s) from your" -> ExileFromZone
                    if let Some(prefix) = other.strip_suffix(" from your") {
                        let p = prefix.trim();
                        let count_and_rest = if let Some(r) = p
                            .strip_prefix("a ")
                            .or_else(|| p.strip_prefix("an "))
                            .or_else(|| p.strip_prefix("one "))
                        {
                            Some((1i64, r))
                        } else if let Some(r) = p.strip_prefix("two ") {
                            Some((2i64, r))
                        } else if let Some(r) = p.strip_prefix("three ") {
                            Some((3i64, r))
                        } else {
                            p.strip_prefix("four ").map(|r| (4i64, r))
                        };
                        if let Some((cnt, rest)) = count_and_rest
                            && !rest.starts_with("target ")
                        {
                            let type_phrase = rest
                                .trim_end_matches(" cards")
                                .trim_end_matches(" card")
                                .trim();
                            if let Some(obj) = parse_look_top_pick_filter(type_phrase, vocabulary) {
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(AffectedPlayers::One(ResolvingPlayer::Controller), spellwake_engine::activated::CostExileZone::Graveyard, ValueExpr::literal(cnt), obj));
                                return Ok(());
                            }
                        }
                    }
                    // Non-targeted "all <filter> cards from <your / target player's / target opponent's>"
                    if let Some(after_all) = other.strip_prefix("all ") {
                        let player_and_rest = if let Some(r) = after_all.strip_suffix(" from your")
                        {
                            Some((None, ResolvingPlayer::Controller, r.trim()))
                        } else if let Some(r) = after_all
                            .strip_suffix(" from target player's")
                            .or_else(|| after_all.strip_suffix(" from target player"))
                        {
                            Some((
                                Some(BodyTarget::SinglePlayer),
                                ResolvingPlayer::TargetPlayer,
                                r.trim(),
                            ))
                        } else {
                            after_all
                                .strip_suffix(" from target opponent's")
                                .or_else(|| after_all.strip_suffix(" from target opponent"))
                                .map(|r| {
                                    (
                                        Some(BodyTarget::SingleOpponent),
                                        ResolvingPlayer::TargetPlayer,
                                        r.trim(),
                                    )
                                })
                        };
                        if let Some((bind, rp, rest)) = player_and_rest {
                            let type_phrase = rest
                                .trim_end_matches(" cards")
                                .trim_end_matches(" card")
                                .trim();
                            if let Some(obj) = parse_look_top_pick_filter(type_phrase, vocabulary) {
                                if let Some(bt) = bind {
                                    builder.bind_target(bt)?;
                                }
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(AffectedPlayers::One(rp), spellwake_engine::activated::CostExileZone::Graveyard, ValueExpr::count_graveyard(rp, obj), obj));
                                return Ok(());
                            }
                        }
                    }
                    let (prefix, zone) = if let Some(p) = other.strip_suffix(" from your") {
                        (p.trim(), ZoneFilter::YourGraveyard)
                    } else if let Some(p) = other
                        .strip_suffix(" from an opponent's")
                        .or_else(|| other.strip_suffix(" from an opponent"))
                        .or_else(|| other.strip_suffix(" from opponents'"))
                        .or_else(|| other.strip_suffix(" from your opponents'"))
                    {
                        (p.trim(), ZoneFilter::OpponentGraveyard)
                    } else if let Some(p) = other
                        .strip_suffix(" from a single")
                        .or_else(|| other.strip_suffix(" from a"))
                        .or_else(|| other.strip_suffix(" from any"))
                        .or_else(|| other.strip_suffix(" from all"))
                        .or_else(|| other.strip_suffix(" from"))
                    {
                        (p.trim(), ZoneFilter::AnyGraveyard)
                    } else {
                        return Err("oracle:unsupported-exile-gy");
                    };
                    let mut chosen_x_bounds = false;
                    let (bounds, rest) = if let Some(r) = prefix.strip_prefix("target ") {
                        (None, r)
                    } else if let Some(r) = prefix.strip_prefix("x target ") {
                        chosen_x_bounds = true;
                        (Some((0usize, usize::MAX)), r)
                    } else if let Some(r) = prefix.strip_prefix("up to x target ") {
                        chosen_x_bounds = true;
                        (Some((0usize, usize::MAX)), r)
                    } else if let Some(r) = prefix.strip_prefix("any number of target ") {
                        (Some((0usize, usize::MAX)), r)
                    } else if let Some(after) = prefix.strip_prefix("up to ") {
                        let (word, tail) =
                            after.split_once(' ').ok_or("oracle:unsupported-exile-gy")?;
                        let max = match word {
                            "one" | "1" => 1usize,
                            "two" | "2" => 2,
                            "three" | "3" => 3,
                            "four" | "4" => 4,
                            "five" | "5" => 5,
                            "six" | "6" => 6,
                            "seven" | "7" => 7,
                            "eight" | "8" => 8,
                            "nine" | "9" => 9,
                            "ten" | "10" => 10,
                            _ => return Err("oracle:unsupported-exile-gy"),
                        };
                        let r = tail
                            .trim()
                            .strip_prefix("target ")
                            .ok_or("oracle:unsupported-exile-gy")?;
                        (Some((0usize, max)), r)
                    } else if let Some((word, tail)) = prefix.split_once(" target ") {
                        let exact = match word {
                            "one" | "1" => 1usize,
                            "two" | "2" => 2,
                            "three" | "3" => 3,
                            "four" | "4" => 4,
                            "five" | "5" => 5,
                            "six" | "6" => 6,
                            "seven" | "7" => 7,
                            "eight" | "8" => 8,
                            _ => return Err("oracle:unsupported-exile-gy"),
                        };
                        (Some((exact, exact)), tail)
                    } else {
                        return Err("oracle:unsupported-exile-gy");
                    };
                    let type_phrase = rest
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    let obj = if type_phrase.is_empty()
                        || type_phrase == "card"
                        || type_phrase == "cards"
                    {
                        ObjectFilter::any()
                    } else if let Some(f) = parse_look_top_pick_filter(type_phrase, vocabulary) {
                        f
                    } else {
                        return Err("oracle:unsupported-exile-gy");
                    };
                    let bt = match zone {
                        ZoneFilter::YourGraveyard => BodyTarget::FilteredOwnGraveyardCard(obj),
                        ZoneFilter::OpponentGraveyard => BodyTarget::FilteredAnyGraveyardCard {
                            filter: obj,
                            opponents_only: true,
                        },
                        ZoneFilter::AnyGraveyard => BodyTarget::FilteredAnyGraveyardCard {
                            filter: obj,
                            opponents_only: false,
                        },
                        _ => return Err("oracle:unsupported-exile-gy"),
                    };
                    builder.bind_target(bt)?;
                    if let Some((min, max)) = bounds {
                        builder.graveyard_target_bounds = Some((min, max));
                    }
                    if chosen_x_bounds {
                        builder.x_target_count = true;
                    }
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    );
                    Ok(())
                }
            }
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to,
            random,
            except,
            count,
            up_to,
            any_number,
            actor,
        } if matches!(subject.as_ref(), SubjectRefAst::InZone { .. }) => {
            let unwrapped =
                unwrap_in_zone_subject(subject).ok_or("oracle:unsupported-return-zone")?;
            lower_instruction(
                &InstructionAst::ReturnZone {
                    subject: Box::new(unwrapped),
                    from_zone: from_zone.clone(),
                    to: to.clone(),
                    tapped: *tapped,
                    transformed: *transformed,
                    with_counter: with_counter.clone(),
                    attached_to: attached_to.clone(),
                    random: *random,
                    except: except.clone(),
                    count: count.clone(),
                    up_to: *up_to,
                    any_number: *any_number,
                    actor: *actor,
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to,
            random: false,
            except: Some(ex),
            count,
            up_to,
            any_number,
            actor,
        } => {
            let patched = apply_except_to_subject(subject, ex)?;
            lower_instruction(
                &InstructionAst::ReturnZone {
                    subject: Box::new(patched),
                    from_zone: from_zone.clone(),
                    to: to.clone(),
                    tapped: *tapped,
                    transformed: *transformed,
                    with_counter: with_counter.clone(),
                    attached_to: attached_to.clone(),
                    random: false,
                    except: None,
                    count: count.clone(),
                    up_to: *up_to,
                    any_number: *any_number,
                    actor: *actor,
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: true,
            except: None,
            count,
            up_to: false,
            any_number: false,
            actor: None | Some(PlayerRefAst::You),
        } if builder.target == BodyTarget::None => {
            let filter_ast = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => f,
                _ => return Err("oracle:unsupported-return-random"),
            };
            let is_own_gy = filter_ast.zone == Some(ZoneFilter::YourGraveyard)
                || (filter_ast.zone.is_none()
                    && from_zone
                        .as_deref()
                        .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard")));
            if !is_own_gy {
                return Err("oracle:unsupported-return-random");
            }
            let dest = to.trim().to_ascii_lowercase();
            let to_hand = is_hand_return_dest(&dest);
            let bf_flags = parse_battlefield_dest(&dest);
            let to_battlefield = if to_hand {
                false
            } else if let Some(bf) = bf_flags
                && !bf.tapped
                && !bf.attacking
                && !bf.transformed
                && !bf.under_owner_control
                && !bf.attach_to_self
            {
                true
            } else {
                return Err("oracle:unsupported-return-random");
            };
            let n_val = match count.as_deref() {
                None => 1u8,
                Some(c) => {
                    let expr = lower_value_expr_in_context(
                        resolve_x(c, bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )?;
                    let ValueExpr::Literal(n) = expr else {
                        return Err("oracle:unsupported-return-random");
                    };
                    n.to_usize()
                        .and_then(|v| u8::try_from(v).ok())
                        .ok_or("oracle:unsupported-return-random")?
                }
            };
            let nz = NonZeroU8::new(n_val).ok_or("oracle:unsupported-return-random")?;
            let mut inner = filter_ast.as_ref().clone();
            inner.zone = None;
            let obj_filter = if is_unrestricted_filter(&inner) {
                None
            } else {
                Some(Box::new(lower_object_filter(&inner, vocabulary)?))
            };
            if to_battlefield && obj_filter.as_deref() != Some(&ObjectFilter::creature()) {
                return Err("oracle:unsupported-return-random");
            }
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards((obj_filter).map(|b| *b), nz, to_battlefield));
            Ok(())
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to: None,
            random: false,
            except: None,
            count,
            up_to,
            any_number,
            actor: None,
        } if count.is_some() || *up_to || *any_number => {
            let filter_ast = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => f,
                _ => return Err("oracle:unsupported-return-counted"),
            };
            let dest = to.trim().to_ascii_lowercase();
            let to_hand = is_hand_return_dest(&dest);
            let bf_flags = parse_battlefield_dest(&dest);
            let enters_tapped = *tapped || bf_flags.is_some_and(|f| f.tapped);
            let has_bf_only_mods = *transformed || with_counter.is_some() || enters_tapped;
            let allow_your_lib = matches!(
                filter_ast.zone,
                Some(ZoneFilter::YourGraveyard | ZoneFilter::YourHand)
            ) || (filter_ast.zone.is_none()
                && matches!(
                    filter_ast.controller,
                    Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                ))
                || from_zone
                    .as_deref()
                    .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard"));
            let (destination, default_zone) = if to_hand && !has_bf_only_mods {
                (SubjectDestination::Hand, SourceZoneFilter::Battlefield)
            } else if let Some(bf_f) = bf_flags {
                if bf_f.attach_to_self && !(in_trigger || self_pronoun_valid) {
                    return Err("oracle:unsupported-return-attach");
                }
                let counter = parse_return_with_counter_local(with_counter.as_deref())?;
                (
                    SubjectDestination::Battlefield {
                        tapped: enters_tapped,
                        attacking: bf_f.attacking,
                        transformed: *transformed || bf_f.transformed,
                        under_owner_control: bf_f.under_owner_control,
                        with_counter: counter,
                        attach_to_self: bf_f.attach_to_self,
                    },
                    SourceZoneFilter::Graveyard,
                )
            } else if !has_bf_only_mods
                && let Some(pl) = parse_lib_placement_for_subject(&dest, allow_your_lib, subject)
            {
                (
                    SubjectDestination::Library(pl),
                    SourceZoneFilter::Battlefield,
                )
            } else {
                return Err("oracle:unsupported-return-dest");
            };
            let mut inner = filter_ast.as_ref().clone();
            let (sel_player, src_zone) = resolve_return_select_zone(
                &mut inner,
                from_zone.as_deref(),
                default_zone,
                builder,
                in_trigger,
            )?;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            let (min, max) = if *any_number {
                (
                    ValueExpr::literal(0),
                    ValueExpr::literal(i64::from(u16::MAX)),
                )
            } else if let Some(cnt_ast) = count {
                let max_expr = lower_value_expr_in_context(
                    resolve_x(cnt_ast, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?;
                let min_expr = if *up_to {
                    ValueExpr::literal(0)
                } else {
                    max_expr.clone()
                };
                (min_expr, max_expr)
            } else {
                (ValueExpr::literal(0), ValueExpr::literal(1))
            };
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(
                    sel_player,
                    src_zone,
                    obj,
                    min,
                    max,
                    destination,
                    false,
                ),
            );
            Ok(())
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to,
            random: false,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            actor: None,
        } => {
            let dest = to.trim().to_ascii_lowercase();
            let to_hand = matches!(
                dest.as_str(),
                "hand"
                    | "owner's hand"
                    | "its owner's hand"
                    | "his owner's hand"
                    | "her owner's hand"
                    | "their owner's hand"
                    | "their owners' hands"
                    | "that player's hand"
                    | "your hand"
                    | "their hand"
                    | "to your hand"
                    | "into your hand"
                    | "to their hand"
                    | "into their hand"
                    | "to that player's hand"
                    | "into that player's hand"
                    | "to its owner's hand"
                    | "to their owner's hand"
                    | "to their owners' hands"
            );
            let bf_flags = parse_battlefield_dest(&dest);
            let enters_tapped = *tapped || bf_flags.is_some_and(|f| f.tapped);
            let has_bf_only_mods =
                *transformed || with_counter.is_some() || attached_to.is_some() || enters_tapped;
            let allow_your_lib = matches!(
                subject.as_ref(),
                SubjectRefAst::TargetObject { filter, .. }
                    if matches!(filter.zone, Some(ZoneFilter::YourGraveyard | ZoneFilter::YourHand))
                        || (filter.zone.is_none()
                            && matches!(
                                filter.controller,
                                Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                            ))
            ) || from_zone
                .as_deref()
                .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard"));
            if !has_bf_only_mods
                && let Some(pl) = parse_lib_placement_for_subject(&dest, allow_your_lib, subject)
            {
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let mut patched = filter.as_ref().clone();
                        if patched.zone.is_none()
                            && let Some(fz) = from_zone.as_deref()
                        {
                            let fl = fz.to_ascii_lowercase();
                            if fl.contains("your graveyard") {
                                patched.zone = Some(ZoneFilter::YourGraveyard);
                            } else if fl.contains("opponent's graveyard") {
                                patched.zone = Some(ZoneFilter::OpponentGraveyard);
                            } else if fl.contains("graveyard") {
                                patched.zone = Some(ZoneFilter::AnyGraveyard);
                            }
                        }
                        let bt = lower_target_subject(
                            &SubjectRefAst::TargetObject {
                                filter: Box::new(patched),
                                count: TargetCountAst::Exact(1),
                            },
                            vocabulary,
                        )?;
                        if !is_valid_put_target_in_library_target(&bt) {
                            return Err("oracle:unsupported-return-dest");
                        }
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(pl));
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if filter.zone.is_none()
                            && from_zone.is_none()
                            && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                                || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                                || matches!(count, TargetCountAst::AnyNumber)
                                || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x) = match *count {
                            TargetCountAst::Exact(n) => (n, n, false),
                            TargetCountAst::UpTo(max) => (0, max, false),
                            TargetCountAst::AnyNumber => (0, u16::MAX, false),
                            TargetCountAst::ChosenX => (0, u16::MAX, true),
                            _ => return Err("oracle:unsupported-return-dest"),
                        };
                        let bt = lower_battlefield_body_target_in_context(
                            filter, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        if is_x {
                            builder.x_target_count = true;
                        }
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: min_t,
                                maximum: max_t,
                                action: PermanentTargetAction::MoveTo(SubjectDestination::Library(
                                    pl,
                                )),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if (matches!(
                            filter.zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) || (filter.zone.is_none()
                            && from_zone.as_deref().is_some_and(|z| {
                                z.to_ascii_lowercase().contains("graveyard")
                            })))
                            && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                                || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                                || matches!(count, TargetCountAst::AnyNumber)
                                || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x) = match *count {
                            TargetCountAst::Exact(n) => (usize::from(n), usize::from(n), false),
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true),
                            _ => return Err("oracle:unsupported-return-dest"),
                        };
                        let eff_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|fz| {
                                let fl = fz.to_ascii_lowercase();
                                if fl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if fl.contains("opponent's graveyard") {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if fl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let bt = match eff_zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                BodyTarget::FilteredOwnGraveyardCard(obj)
                            }
                            Some(ZoneFilter::OpponentGraveyard) => {
                                BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: true,
                                }
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::SingleGraveyard) => {
                                BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: false,
                                }
                            }
                            _ => return Err("oracle:unsupported-return-dest"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let mut inner = f.as_ref().clone();
                        if let Ok((sel_player, src_zone)) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Battlefield,
                            builder,
                            in_trigger,
                        ) {
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(sel_player, src_zone, obj, ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Library(pl), false));
                            return Ok(());
                        }
                        if let Some(z) = f.zone {
                            let mut trial = builder.clone();
                            let mut inner = f.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter_in_context(
                                    &inner, &trial, in_trigger, vocabulary,
                                )?
                            };
                            bind_filter_player_target(&mut trial, &obj)?;
                            let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                .map_err(|_| "oracle:unsupported-move-zone")?;
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    query,
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: is_hidden_zone_filter(z),
                                    },
                                    spellwake_engine::toolbox::ZoneDest::Library {
                                        position:
                                            spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                                pl,
                                            ),
                                    },
                                ),
                            ));
                            *builder = trial;
                            return Ok(());
                        }
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        let is_linked = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                        if is_linked {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        }
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = if is_linked {
                            if is_unrestricted_filter(&inner) {
                                spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                            } else {
                                spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                        group: None,
                                    },
                                    obj,
                                )
                            }
                        } else {
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                obj,
                            )
                        };
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                query,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                spellwake_engine::toolbox::ZoneDest::Library {
                                    position:
                                        spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                            pl,
                                        ),
                                },
                            ),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        if !in_trigger && !self_pronoun_valid {
                            let adj = normalize_resolving_spell_lib_placement(pl, builder.target);
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_resolving_spell_in_library(adj));
                        } else {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::AbilitySource, SubjectDestination::Library(pl)));
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_valid_put_target_in_library_target(&builder.target) =>
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(pl));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::CreatedTokens,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::LastMoved,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AttachedHost,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if has_checked_revealed_top_strict(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::RevealedCard,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if try_lower_move_last_moved_filtered(
                            f,
                            SubjectDestination::Library(pl),
                            builder,
                            vocabulary,
                        )? =>
                    {
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("exile"))
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        return lower_linked_exile_group_move(
                            f,
                            SubjectDestination::Library(pl),
                            false,
                            builder,
                            vocabulary,
                            in_trigger,
                            "oracle:unsupported-return-dest",
                        );
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("battlefield")) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = lower_object_filter_in_context(
                            &inner, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        if pl == LibraryPlacement::Bottom && builder.target == BodyTarget::None {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_group_on_library_bottom(obj));
                        } else {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group_to_library(obj, pl));
                        }
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(players) = resolve_graveyard_group_players(
                            f.zone,
                            from_zone.as_deref(),
                            builder,
                            in_trigger,
                        )? =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(players, obj, SubjectDestination::Library(pl)));
                        return Ok(());
                    }
                    _ => return Err("oracle:unsupported-return-dest"),
                }
            }
            if to_hand && !has_bf_only_mods {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        if from_zone
                            .as_deref()
                            .is_some_and(|z| z.to_ascii_lowercase().contains("graveyard"))
                        {
                            builder.instructions.push(
                                super::super::toolbox_emit::return_source_graveyard_to_hand(),
                            );
                        } else if !in_trigger && !self_pronoun_valid {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::return_resolving_spell_to_hand());
                        } else {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::return_source_to_hand());
                        }
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let effective_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|fz| {
                                let fl = fz.to_ascii_lowercase();
                                if fl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if fl.contains("opponent's graveyard") {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if fl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        match effective_zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                let mut inner = filter.as_ref().clone();
                                inner.zone = None;
                                let obj = if is_unrestricted_filter(&inner) {
                                    ObjectFilter::any()
                                } else {
                                    lower_object_filter(&inner, vocabulary)?
                                };
                                builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                                builder
                                    .instructions
                                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand());
                            }
                            Some(
                                ZoneFilter::AnyGraveyard
                                | ZoneFilter::AllGraveyards
                                | ZoneFilter::SingleGraveyard
                                | ZoneFilter::OpponentGraveyard
                                | ZoneFilter::OpponentsGraveyards,
                            ) => {
                                let mut inner = filter.as_ref().clone();
                                inner.zone = None;
                                let obj = if is_unrestricted_filter(&inner) {
                                    ObjectFilter::any()
                                } else {
                                    lower_object_filter(&inner, vocabulary)?
                                };
                                let opponents_only = matches!(
                                    effective_zone,
                                    Some(
                                        ZoneFilter::OpponentGraveyard
                                            | ZoneFilter::OpponentsGraveyards
                                    )
                                );
                                builder.bind_target(BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only,
                                })?;
                                builder
                                    .instructions
                                    .push(super::super::toolbox_emit::return_to_owners_hand());
                            }
                            None | Some(ZoneFilter::Battlefield) => {
                                let mut inner = filter.as_ref().clone();
                                inner.zone = None;
                                let bt = lower_battlefield_body_target(&inner, vocabulary)?;
                                builder.bind_target(bt)?;
                                builder
                                    .instructions
                                    .push(super::super::toolbox_emit::return_to_owners_hand());
                            }
                            _ => return Err("oracle:unsupported-return-hand-zone"),
                        }
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if *n >= 2 => {
                        let gy_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|z| {
                                let zl = z.to_ascii_lowercase();
                                if zl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if zl.contains("opponent's graveyard")
                                    || zl.contains("opponents' graveyards")
                                {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if zl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        if matches!(
                            gy_zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) {
                            let mut inner = filter.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            if gy_zone == Some(ZoneFilter::YourGraveyard) {
                                builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                                builder.graveyard_target_bounds =
                                    Some((usize::from(*n), usize::from(*n)));
                                builder
                                    .instructions
                                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand());
                            } else {
                                builder.bind_target(BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: gy_zone == Some(ZoneFilter::OpponentGraveyard),
                                })?;
                                builder.graveyard_target_bounds =
                                    Some((usize::from(*n), usize::from(*n)));
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, SubjectDestination::Hand));
                            }
                            Ok(())
                        } else if filter.zone.is_none() && from_zone.is_none() {
                            let bt = lower_battlefield_body_target(filter, vocabulary)?;
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: *n,
                                    maximum: *n,
                                    action: PermanentTargetAction::ReturnToHand,
                                });
                            Ok(())
                        } else {
                            Err("oracle:unsupported-return-hand")
                        }
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(max),
                    } if *max > 0 => {
                        let gy_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|z| {
                                let zl = z.to_ascii_lowercase();
                                if zl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if zl.contains("opponent's graveyard")
                                    || zl.contains("opponents' graveyards")
                                {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if zl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        if matches!(
                            gy_zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) {
                            let mut inner = filter.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            if gy_zone == Some(ZoneFilter::YourGraveyard) {
                                builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                                builder.graveyard_target_bounds = Some((0, usize::from(*max)));
                                builder
                                    .instructions
                                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand());
                            } else {
                                builder.bind_target(BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: gy_zone == Some(ZoneFilter::OpponentGraveyard),
                                })?;
                                builder.graveyard_target_bounds = Some((0, usize::from(*max)));
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, SubjectDestination::Hand));
                            }
                            Ok(())
                        } else if filter.zone.is_none() && from_zone.is_none() {
                            let bt = lower_battlefield_body_target(filter, vocabulary)?;
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: *max,
                                    action: PermanentTargetAction::ReturnToHand,
                                });
                            Ok(())
                        } else {
                            Err("oracle:unsupported-return-hand")
                        }
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count:
                            count @ (TargetCountAst::ChosenX
                            | TargetCountAst::UpToX
                            | TargetCountAst::AnyNumber),
                    } if !matches!(count, TargetCountAst::ChosenX | TargetCountAst::UpToX)
                        || !in_trigger =>
                    {
                        let gy_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|z| {
                                let zl = z.to_ascii_lowercase();
                                if zl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if zl.contains("opponent's graveyard")
                                    || zl.contains("opponents' graveyards")
                                {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if zl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        if matches!(
                            gy_zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) {
                            let mut inner = filter.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            if gy_zone == Some(ZoneFilter::YourGraveyard) {
                                builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                                builder.graveyard_target_bounds = Some((0, usize::MAX));
                                if *count == TargetCountAst::ChosenX {
                                    builder.x_target_count = true;
                                } else if *count == TargetCountAst::UpToX {
                                    builder.up_to_x_target_count = true;
                                }
                                builder
                                    .instructions
                                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand());
                            } else {
                                builder.bind_target(BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: gy_zone == Some(ZoneFilter::OpponentGraveyard),
                                })?;
                                builder.graveyard_target_bounds = Some((0, usize::MAX));
                                if *count == TargetCountAst::ChosenX {
                                    builder.x_target_count = true;
                                } else if *count == TargetCountAst::UpToX {
                                    builder.up_to_x_target_count = true;
                                }
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, SubjectDestination::Hand));
                            }
                            Ok(())
                        } else if filter.zone.is_none() && from_zone.is_none() {
                            let bt = lower_battlefield_body_target(filter, vocabulary)?;
                            builder.bind_target(bt)?;
                            if *count == TargetCountAst::ChosenX {
                                builder.x_target_count = true;
                            } else if *count == TargetCountAst::UpToX {
                                builder.up_to_x_target_count = true;
                            }
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: u16::MAX,
                                    action: PermanentTargetAction::ReturnToHand,
                                });
                            Ok(())
                        } else {
                            Err("oracle:unsupported-return-hand")
                        }
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target != BodyTarget::None
                            && !matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            ) =>
                    {
                        if is_permanent_body_target(builder.target)
                            || builder.target == BodyTarget::OwnedExiledFlashbackCard
                        {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::return_to_owners_hand());
                        } else {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::move_subject(
                                    MoveSubjectRef::TargetObject,
                                    SubjectDestination::Hand,
                                ));
                        }
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::LastMoved,
                                SubjectDestination::Hand,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(raw)
                        if is_linked_exile_anaphora(raw)
                            && (in_trigger || self_pronoun_valid)
                            && builder.target == BodyTarget::None
                            && !last_instruction_moved_object(&builder.instructions)
                            && !has_prior_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_linked_exile(SubjectDestination::Hand, is_singular_linked_exile_anaphora(raw)));
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                SubjectDestination::Hand,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile)) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let obj = if in_trigger || self_pronoun_valid {
                            obj.with_flags(ObjectFilterFlags::EXILED_WITH_SELF)
                        } else {
                            obj
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::LinkedExile, obj, ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Hand, false));
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if strip_last_moved_predicates(
                            &mut f.as_ref().clone(),
                            &builder.instructions,
                        ) =>
                    {
                        let mut inner = f.as_ref().clone();
                        strip_last_moved_predicates(&mut inner, &builder.instructions);
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::LastMoved, obj, ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Hand, false));
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if f.zone == Some(ZoneFilter::YourGraveyard)
                            || from_zone.as_deref().is_some_and(|z| {
                                z.to_ascii_lowercase().contains("your graveyard")
                            }) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::Graveyard, obj, ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Hand, false));
                        Ok(())
                    }
                    SubjectRefAst::Group(f) if f.zone.is_some() || from_zone.is_some() => {
                        let mut inner = f.as_ref().clone();
                        let mut trial = builder.clone();
                        let (sel_player, src_zone) = resolve_return_select_zone(
                            &mut inner,
                            from_zone.as_deref(),
                            SourceZoneFilter::Graveyard,
                            &mut trial,
                            in_trigger,
                        )?;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        trial.instructions.push(
                            super::super::toolbox_emit::select_zone_cards_to_zone(
                                sel_player,
                                src_zone,
                                obj,
                                ValueExpr::literal(1),
                                ValueExpr::literal(1),
                                SubjectDestination::Hand,
                                false,
                            ),
                        );
                        *builder = trial;
                        Ok(())
                    }
                    SubjectRefAst::Group(f) if f.zone.is_none() && from_zone.is_none() => {
                        let obj = if is_unrestricted_filter(f) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(f, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::Battlefield, obj, ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Hand, false));
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if f.zone.is_none()
                            && from_zone.is_none()
                            && f.controller == Some(ControllerRelation::You)
                            && f.flags == AstFilterFlags::EXCLUDE_SELF
                            && f.any_types == AstCardTypeMask::CREATURE
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    types: true,
                                    controller: true,
                                    flags: true,
                                    ..FilterAllow::NONE
                                },
                            )
                            && in_trigger
                            && builder.target == BodyTarget::None =>
                    {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Battlefield,
                                        ObjectFilter::Clause(
                                            ObjectClause::creature()
                                                .controlled()
                                                .other_than_ability_source(),
                                        ),
                                    ),
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: false,
                                    },
                                    spellwake_engine::toolbox::ZoneDest::Hand,
                                ),
                            ));
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if f.zone.is_none()
                            && from_zone.is_none()
                            && f.controller == Some(ControllerRelation::You)
                            && f.flags.0 == 0
                            && f.tapped.is_none()
                            && f.token.is_none()
                            && f.combat.is_none()
                            && f.counter_kind.is_none() =>
                    {
                        let ptf = lower_permanent_type_filter(f, vocabulary)?;
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_controlled_permanents(AffectedPlayers::One(ResolvingPlayer::Controller), ValueExpr::literal(1), ptf));
                        Ok(())
                    }
                    SubjectRefAst::AttachedHost(_) => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                                ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                                PublicZoneDestination::Hand,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if try_lower_move_last_moved_filtered(
                            f,
                            SubjectDestination::Hand,
                            builder,
                            vocabulary,
                        )? =>
                    {
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("exile"))
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        lower_linked_exile_group_move(
                            f,
                            SubjectDestination::Hand,
                            false,
                            builder,
                            vocabulary,
                            in_trigger,
                            "oracle:unsupported-return-hand",
                        )
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("battlefield")) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = lower_object_filter(&inner, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                                obj,
                                PublicZoneDestination::Hand,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if classify_graveyard_group(f.zone, from_zone.as_deref()).is_some() =>
                    {
                        let all_gy =
                            classify_graveyard_group(f.zone, from_zone.as_deref()).unwrap_or(false);
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        if !all_gy
                            && builder.target == BodyTarget::None
                            && inner.forbidden_types.is_empty()
                            && inner.required_supertypes.is_empty()
                            && inner.forbidden_supertypes.is_empty()
                            && inner.subtypes.is_empty()
                            && inner.forbidden_subtypes.is_empty()
                            && inner.any_colors.is_empty()
                            && inner.forbidden_colors.is_empty()
                            && inner.color_arity.is_none()
                            && inner.required_keywords.is_empty()
                            && inner.forbidden_keywords.is_empty()
                            && inner.stat_bounds.is_empty()
                            && inner.counter_kind.is_none()
                            && inner.flags.0 == 0
                            && let Some(ct) = (if inner.any_types == AstCardTypeMask::CREATURE {
                                Some(CardType::Creature)
                            } else if inner.any_types == AstCardTypeMask::ARTIFACT {
                                Some(CardType::Artifact)
                            } else if inner.any_types == AstCardTypeMask::ENCHANTMENT {
                                Some(CardType::Enchantment)
                            } else if inner.any_types == AstCardTypeMask::INSTANT {
                                Some(CardType::Instant)
                            } else if inner.any_types == AstCardTypeMask::SORCERY {
                                Some(CardType::Sorcery)
                            } else if inner.any_types == AstCardTypeMask::LAND {
                                Some(CardType::Land)
                            } else if inner.any_types == AstCardTypeMask::PLANESWALKER {
                                Some(CardType::Planeswalker)
                            } else {
                                None
                            })
                        {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_own_graveyard_cards_of_type_to_hand(ct));
                            return Ok(());
                        }
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group(obj, all_gy, false, false));
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(players) = resolve_graveyard_group_players(
                            f.zone,
                            from_zone.as_deref(),
                            builder,
                            in_trigger,
                        )? =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(players, obj, SubjectDestination::Hand));
                        Ok(())
                    }
                    SubjectRefAst::Compound(items) if !items.is_empty() => {
                        for it in items {
                            lower_instruction(
                                &InstructionAst::ReturnZone {
                                    subject: Box::new(it.clone()),
                                    from_zone: from_zone.clone(),
                                    to: to.clone(),
                                    tapped: false,
                                    transformed: false,
                                    with_counter: None,
                                    attached_to: None,
                                    random: false,
                                    except: None,
                                    count: None,
                                    up_to: false,
                                    any_number: false,
                                    actor: None,
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        Ok(())
                    }
                    _ => Err("oracle:unsupported-return-hand"),
                }
            } else if let Some(bf_flags) = bf_flags {
                let counter = parse_return_with_counter_local(with_counter.as_deref())?;
                let attach_to_self = (bf_flags.attach_to_self
                    && (in_trigger || self_pronoun_valid))
                    || attached_to.as_deref().is_some_and(|a| {
                        (in_trigger || self_pronoun_valid)
                            && (is_self_pronoun(a, builder, self_pronoun_valid)
                                || matches!(a, SubjectRefAst::Anaphora(_)))
                    });
                if (attached_to.is_some() || bf_flags.attach_to_self) && !attach_to_self {
                    return Err("oracle:unsupported-return-attach");
                }
                let is_transformed = *transformed || bf_flags.transformed;
                let has_extra_bf_mods = is_transformed
                    || counter.is_some()
                    || attach_to_self
                    || bf_flags.attacking
                    || bf_flags.under_owner_control;
                let bf_dest = SubjectDestination::Battlefield {
                    tapped: enters_tapped,
                    attacking: bf_flags.attacking,
                    transformed: is_transformed,
                    under_owner_control: bf_flags.under_owner_control,
                    with_counter: counter,
                    attach_to_self,
                };
                let resolve_effective_gy_zone =
                    |filter_zone: Option<ZoneFilter>| -> Option<ZoneFilter> {
                        filter_zone.or_else(|| {
                            from_zone.as_deref().and_then(|fz| {
                                let fl = fz.to_ascii_lowercase();
                                if fl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if fl.contains("opponent's graveyard") {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if fl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else if fl.contains("exile") {
                                    Some(ZoneFilter::Exile)
                                } else {
                                    None
                                }
                            })
                        })
                    };
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && !has_extra_bf_mods
                        && from_zone
                            .as_deref()
                            .is_some_and(|z| z.to_ascii_lowercase().contains("graveyard")) =>
                    {
                        builder.instructions.push(
                            super::super::toolbox_emit::return_source_graveyard_to_battlefield(
                                enters_tapped,
                            ),
                        );
                        Ok(())
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AbilitySource,
                                bf_dest,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let effective_zone = resolve_effective_gy_zone(filter.zone);
                        match effective_zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                let mut patched = filter.as_ref().clone();
                                patched.zone = Some(ZoneFilter::YourGraveyard);
                                let bt = lower_target_subject(
                                    &SubjectRefAst::TargetObject {
                                        filter: Box::new(patched),
                                        count: TargetCountAst::Exact(1),
                                    },
                                    vocabulary,
                                )?;
                                builder.bind_target(bt)?;
                                if !has_extra_bf_mods
                                    && !enters_tapped
                                    && bt
                                        == BodyTarget::FilteredOwnGraveyardCard(
                                            ObjectFilter::creature(),
                                        )
                                {
                                    builder
                                        .instructions
                                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(false));
                                } else if !has_extra_bf_mods
                                    && matches!(
                                        bt,
                                        BodyTarget::FilteredOwnGraveyardCard(f)
                                            if !f.allows_instant_or_sorcery()
                                    )
                                {
                                    builder.instructions.push(
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(enters_tapped),
                                    );
                                } else {
                                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, bf_dest));
                                }
                                Ok(())
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::OpponentGraveyard) => {
                                let mut patched = filter.as_ref().clone();
                                patched.zone = effective_zone;
                                let bt = lower_target_subject(
                                    &SubjectRefAst::TargetObject {
                                        filter: Box::new(patched),
                                        count: TargetCountAst::Exact(1),
                                    },
                                    vocabulary,
                                )?;
                                builder.bind_target(bt)?;
                                if has_extra_bf_mods {
                                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, bf_dest));
                                } else if !enters_tapped
                                    && bt == BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                                {
                                    builder.instructions.push(
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(false),
                                    );
                                } else if matches!(
                                    bt,
                                    BodyTarget::FilteredAnyGraveyardCard { filter: f, .. }
                                        if !f.allows_instant_or_sorcery()
                                ) || matches!(
                                    bt,
                                    BodyTarget::AnyGraveyardCreature
                                        | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                                ) {
                                    builder.instructions.push(
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(enters_tapped),
                                    );
                                } else {
                                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, bf_dest));
                                }
                                Ok(())
                            }
                            None if has_extra_bf_mods => {
                                let bt = lower_target_subject(subject, vocabulary)?;
                                builder.bind_target(bt)?;
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, bf_dest));
                                Ok(())
                            }
                            _ => Err("oracle:unsupported-reanimate-zone"),
                        }
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if matches!(
                            count,
                            TargetCountAst::Exact(n) if *n >= 2
                        ) || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger) =>
                    {
                        let effective_zone = resolve_effective_gy_zone(filter.zone);
                        let (min_t, max_t, is_x) = match *count {
                            TargetCountAst::Exact(n) => (usize::from(n), usize::from(n), false),
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true),
                            _ => return Err("oracle:unsupported-reanimate-zone"),
                        };
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let bt = match effective_zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                BodyTarget::FilteredOwnGraveyardCard(obj)
                            }
                            Some(ZoneFilter::OpponentGraveyard) => {
                                BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: true,
                                }
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::SingleGraveyard) => {
                                BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only: false,
                                }
                            }
                            _ => return Err("oracle:unsupported-reanimate-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                bf_dest,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if try_lower_move_last_moved_filtered(f, bf_dest, builder, vocabulary)? =>
                    {
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("exile"))
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        lower_linked_exile_group_move(
                            f,
                            bf_dest,
                            false,
                            builder,
                            vocabulary,
                            in_trigger,
                            "oracle:unsupported-return-bf",
                        )
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if !is_transformed
                            && counter.is_none()
                            && !attach_to_self
                            && !bf_flags.attacking
                            && classify_graveyard_group(f.zone, from_zone.as_deref())
                                .is_some_and(|all_gy| !bf_flags.under_owner_control || !all_gy) =>
                    {
                        let all_gy =
                            classify_graveyard_group(f.zone, from_zone.as_deref()).unwrap_or(false);
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group(obj, all_gy, true, enters_tapped));
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(players) = resolve_graveyard_group_players(
                            f.zone,
                            from_zone.as_deref(),
                            builder,
                            in_trigger,
                        )? =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(players, obj, bf_dest));
                        Ok(())
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                bf_dest,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(raw) => {
                        if is_linked_exile_anaphora(raw)
                            && (in_trigger || self_pronoun_valid)
                            && builder.target == BodyTarget::None
                            && !last_instruction_moved_object(&builder.instructions)
                            && !has_prior_moved_object(&builder.instructions)
                        {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_linked_exile(bf_dest, is_singular_linked_exile_anaphora(raw)));
                            return Ok(());
                        }
                        let subj_ref = if builder.target != BodyTarget::None
                            && !matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            ) {
                            MoveSubjectRef::TargetObject
                        } else if last_instruction_moved_object(&builder.instructions) {
                            MoveSubjectRef::LastMoved
                        } else if in_trigger {
                            MoveSubjectRef::TriggeringObject
                        } else if self_pronoun_valid {
                            MoveSubjectRef::AbilitySource
                        } else {
                            return Err("oracle:unsupported-return-bf");
                        };
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                subj_ref, bf_dest,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AttachedHost,
                                bf_dest,
                            ),
                        );
                        Ok(())
                    }
                    SubjectRefAst::Group(f) => {
                        let mut inner = f.as_ref().clone();
                        let (sel_player, src_zone) = if (inner.flags.0
                            & AstFilterFlags::EXILED_WITH_SELF.0)
                            != 0
                            && matches!(inner.zone, None | Some(ZoneFilter::Exile))
                        {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                            inner.zone = None;
                            (ResolvingPlayer::Controller, SourceZoneFilter::LinkedExile)
                        } else if strip_last_moved_predicates(&mut inner, &builder.instructions) {
                            (ResolvingPlayer::Controller, SourceZoneFilter::LastMoved)
                        } else if inner.zone.is_some() {
                            resolve_select_zone(
                                &mut inner,
                                SourceZoneFilter::Graveyard,
                                builder,
                                in_trigger,
                            )?
                        } else if let Some(fz) = from_zone.as_deref() {
                            let fl = fz.to_ascii_lowercase();
                            if fl.contains("that player's graveyard")
                                || fl.contains("their graveyard")
                            {
                                (
                                    lower_resolving_player_in_context(
                                        PlayerRefAst::ThatPlayer,
                                        builder,
                                        in_trigger,
                                    )?,
                                    SourceZoneFilter::Graveyard,
                                )
                            } else if fl.contains("opponent's graveyard") {
                                (
                                    ResolvingPlayer::Controller,
                                    SourceZoneFilter::OpponentGraveyard,
                                )
                            } else if fl.contains("all graveyards") || fl.contains("any graveyard")
                            {
                                (ResolvingPlayer::Controller, SourceZoneFilter::AnyGraveyard)
                            } else if fl.contains("hand or graveyard")
                                || fl.contains("hand and/or graveyard")
                            {
                                (
                                    ResolvingPlayer::Controller,
                                    SourceZoneFilter::HandOrGraveyard,
                                )
                            } else if fl.contains("graveyard") {
                                (ResolvingPlayer::Controller, SourceZoneFilter::Graveyard)
                            } else if fl.contains("exile") {
                                (ResolvingPlayer::Controller, SourceZoneFilter::Exile)
                            } else if fl.contains("that player's hand") || fl.contains("their hand")
                            {
                                (
                                    lower_resolving_player_in_context(
                                        PlayerRefAst::ThatPlayer,
                                        builder,
                                        in_trigger,
                                    )?,
                                    SourceZoneFilter::Hand,
                                )
                            } else if fl.contains("hand") {
                                (ResolvingPlayer::Controller, SourceZoneFilter::Hand)
                            } else if fl.contains("your library") {
                                (ResolvingPlayer::Controller, SourceZoneFilter::YourLibrary)
                            } else if fl.contains("command zone") {
                                (ResolvingPlayer::Controller, SourceZoneFilter::CommandZone)
                            } else {
                                return Err("oracle:unsupported-return-bf");
                            }
                        } else {
                            return Err("oracle:unsupported-return-bf");
                        };
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(sel_player, src_zone, obj, ValueExpr::literal(1), ValueExpr::literal(1), bf_dest, false));
                        Ok(())
                    }
                    SubjectRefAst::Compound(items) if !items.is_empty() => {
                        for it in items {
                            lower_instruction(
                                &InstructionAst::ReturnZone {
                                    subject: Box::new(it.clone()),
                                    from_zone: from_zone.clone(),
                                    to: to.clone(),
                                    tapped: *tapped,
                                    transformed: *transformed,
                                    with_counter: with_counter.clone(),
                                    attached_to: attached_to.clone(),
                                    random: false,
                                    except: None,
                                    count: None,
                                    up_to: false,
                                    any_number: false,
                                    actor: None,
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_some() => {
                        let Some(z) = f.zone else {
                            return Err("oracle:unsupported-return-bf");
                        };
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                            .map_err(|_| "oracle:unsupported-return-bf")?;
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                query,
                                super::super::toolbox_emit::subject_dest_to_zone_dest(bf_dest),
                            ),
                        ));
                        *builder = trial;
                        Ok(())
                    }
                    _ => Err("oracle:unsupported-return-bf"),
                }
            } else if !has_bf_only_mods
                && matches!(
                    dest.as_str(),
                    "graveyard"
                        | "your graveyard"
                        | "owner's graveyard"
                        | "its owner's graveyard"
                        | "their owner's graveyard"
                        | "their owners' graveyards"
                )
            {
                match subject.as_ref() {
                    SubjectRefAst::Anaphora(raw)
                        if is_linked_exile_anaphora(raw)
                            && (in_trigger || self_pronoun_valid)
                            && builder.target == BodyTarget::None
                            && !last_instruction_moved_object(&builder.instructions)
                            && !has_prior_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_linked_exile(SubjectDestination::Graveyard, is_singular_linked_exile_anaphora(raw)));
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("exile"))
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        lower_linked_exile_group_move(
                            f,
                            SubjectDestination::Graveyard,
                            false,
                            builder,
                            vocabulary,
                            in_trigger,
                            "oracle:unsupported-return-dest",
                        )
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_some() => {
                        let Some(z) = f.zone else {
                            return Err("oracle:unsupported-return-dest");
                        };
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                            .map_err(|_| "oracle:unsupported-return-dest")?;
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                query,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Move,
                                },
                            ),
                        ));
                        *builder = trial;
                        Ok(())
                    }
                    _ => Err("oracle:unsupported-return-dest"),
                }
            } else {
                Err("oracle:unsupported-return-dest")
            }
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to: None,
            random: false,
            except: None,
            count,
            up_to,
            any_number,
            actor: Some(actor_player),
        } => {
            if *actor_player == PlayerRefAst::You {
                return lower_instruction(
                    &InstructionAst::ReturnZone {
                        subject: subject.clone(),
                        from_zone: from_zone.clone(),
                        to: to.clone(),
                        tapped: *tapped,
                        transformed: *transformed,
                        with_counter: with_counter.clone(),
                        attached_to: None,
                        random: false,
                        except: None,
                        count: count.clone(),
                        up_to: *up_to,
                        any_number: *any_number,
                        actor: None,
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            let dest = to.trim().to_ascii_lowercase();
            let to_hand = is_hand_return_dest(&dest);
            let bf_flags = parse_battlefield_dest(&dest);
            let enters_tapped = *tapped || bf_flags.is_some_and(|f| f.tapped);
            let has_bf_only_mods = *transformed || with_counter.is_some() || enters_tapped;
            let destination = if to_hand && !has_bf_only_mods {
                SubjectDestination::Hand
            } else if let Some(bf_f) = bf_flags {
                if bf_f.attach_to_self {
                    return Err("oracle:unsupported-return-attach");
                }
                let counter = parse_return_with_counter_local(with_counter.as_deref())?;
                let under_owner = bf_f.under_owner_control || !dest.contains("under your control");
                SubjectDestination::Battlefield {
                    tapped: enters_tapped,
                    attacking: bf_f.attacking,
                    transformed: *transformed || bf_f.transformed,
                    under_owner_control: under_owner,
                    with_counter: counter,
                    attach_to_self: false,
                }
            } else {
                return Err("oracle:unsupported-return-dest");
            };
            let resolve_actor_source_zone = |zone_opt: Option<ZoneFilter>,
                                             fz: Option<&str>|
             -> Option<SourceZoneFilter> {
                match zone_opt {
                    Some(ZoneFilter::YourGraveyard | ZoneFilter::ThatPlayerGraveyard) => {
                        Some(SourceZoneFilter::Graveyard)
                    }
                    Some(ZoneFilter::TargetPlayerGraveyard)
                        if *actor_player == PlayerRefAst::TargetPlayer =>
                    {
                        Some(SourceZoneFilter::Graveyard)
                    }
                    Some(ZoneFilter::TargetOpponentGraveyard)
                        if *actor_player == PlayerRefAst::TargetOpponent =>
                    {
                        Some(SourceZoneFilter::Graveyard)
                    }
                    Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => {
                        Some(SourceZoneFilter::Hand)
                    }
                    Some(ZoneFilter::TargetPlayerHand)
                        if *actor_player == PlayerRefAst::TargetPlayer =>
                    {
                        Some(SourceZoneFilter::Hand)
                    }
                    Some(ZoneFilter::TargetOpponentHand)
                        if *actor_player == PlayerRefAst::TargetOpponent =>
                    {
                        Some(SourceZoneFilter::Hand)
                    }
                    Some(ZoneFilter::YourHandOrGraveyard) => {
                        Some(SourceZoneFilter::HandOrGraveyard)
                    }
                    Some(ZoneFilter::Battlefield) => Some(SourceZoneFilter::Battlefield),
                    None => {
                        let Some(raw) = fz else {
                            return to_hand.then_some(SourceZoneFilter::Battlefield);
                        };
                        let fl = raw.to_ascii_lowercase();
                        if fl.contains("hand or graveyard") || fl.contains("hand and/or graveyard")
                        {
                            Some(SourceZoneFilter::HandOrGraveyard)
                        } else if fl.contains("their graveyard")
                            || fl.contains("that player's graveyard")
                            || fl.contains("his or her graveyard")
                            || fl == "graveyard"
                        {
                            Some(SourceZoneFilter::Graveyard)
                        } else if fl.contains("their hand")
                            || fl.contains("that player's hand")
                            || fl.contains("his or her hand")
                            || fl == "hand"
                        {
                            Some(SourceZoneFilter::Hand)
                        } else if fl.contains("battlefield") {
                            Some(SourceZoneFilter::Battlefield)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };
            match subject.as_ref() {
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if count.is_none()
                        && !*up_to
                        && !*any_number
                        && *actor_player == PlayerRefAst::EachPlayer
                        && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                        && matches!(f.zone, None | Some(ZoneFilter::Exile))
                        && from_zone
                            .as_deref()
                            .is_none_or(|z| z.to_ascii_lowercase().contains("exile"))
                        && (in_trigger || self_pronoun_valid) =>
                {
                    let owner_dest = match destination {
                        SubjectDestination::Battlefield {
                            tapped,
                            attacking,
                            transformed,
                            with_counter,
                            attach_to_self,
                            ..
                        } => SubjectDestination::Battlefield {
                            tapped,
                            attacking,
                            transformed,
                            under_owner_control: true,
                            with_counter,
                            attach_to_self,
                        },
                        other => other,
                    };
                    lower_linked_exile_group_move(
                        f,
                        owner_dest,
                        true,
                        builder,
                        vocabulary,
                        in_trigger,
                        if to_hand {
                            "oracle:unsupported-return-hand"
                        } else {
                            "oracle:unsupported-return-bf"
                        },
                    )
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if count.is_none()
                        && !*up_to
                        && !*any_number
                        && matches!(
                            resolve_actor_source_zone(f.zone, from_zone.as_deref()),
                            Some(SourceZoneFilter::Graveyard)
                        ) =>
                {
                    let mut trial = builder.clone();
                    let players =
                        lower_affected_players_in_context(*actor_player, &mut trial, in_trigger)?;
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    bind_filter_player_target(&mut trial, &obj)?;
                    trial.instructions.push(
                        super::super::toolbox_emit::move_graveyard_group_owned(
                            players,
                            obj,
                            destination,
                        ),
                    );
                    *builder = trial;
                    Ok(())
                }
                SubjectRefAst::Group(f) => {
                    let Some(src_zone) = resolve_actor_source_zone(f.zone, from_zone.as_deref())
                    else {
                        return Err("oracle:unsupported-return-zone");
                    };
                    let mut trial = builder.clone();
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    if obj.has_target_player() {
                        return Err("oracle:unsupported-return-zone");
                    }
                    let (min, max) = if *any_number {
                        (
                            ValueExpr::literal(0),
                            ValueExpr::literal(i64::from(u16::MAX)),
                        )
                    } else if let Some(cnt_ast) = count {
                        let max_expr = lower_value_expr_in_context(
                            resolve_x(cnt_ast, bound_x),
                            &trial,
                            in_trigger,
                            vocabulary,
                        )?;
                        let min_expr = if *up_to {
                            ValueExpr::literal(0)
                        } else {
                            max_expr.clone()
                        };
                        (min_expr, max_expr)
                    } else {
                        (
                            ValueExpr::literal(if *up_to { 0 } else { 1 }),
                            ValueExpr::literal(1),
                        )
                    };
                    if matches!(
                        actor_player,
                        PlayerRefAst::EachPlayer
                            | PlayerRefAst::EachOpponent
                            | PlayerRefAst::EachOtherPlayer
                    ) {
                        let players = lower_affected_players_in_context(
                            *actor_player,
                            &mut trial,
                            in_trigger,
                        )?;
                        let loop_body = ExecutableBody::new(
                            BodyTarget::None,
                            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::LoopPlayer, src_zone, obj, min, max, destination, false)],
                        );
                        trial.instructions.push(Instruction::ForEachPlayerQuery {
                            selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                                players,
                            )),
                            bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                            body: Box::new(loop_body),
                        });
                    } else {
                        let rp = lower_resolving_player_in_context(
                            *actor_player,
                            &mut trial,
                            in_trigger,
                        )?;
                        trial.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(rp, src_zone, obj, min, max, destination, false));
                    }
                    *builder = trial;
                    Ok(())
                }
                _ => Err(if to_hand {
                    "oracle:unsupported-return-hand"
                } else {
                    "oracle:unsupported-return-bf"
                }),
            }
        }
        InstructionAst::MoveZone { subject, to }
            if matches!(subject.as_ref(), SubjectRefAst::InZone { .. }) =>
        {
            let unwrapped =
                unwrap_in_zone_subject(subject).ok_or("oracle:unsupported-move-zone")?;
            lower_instruction(
                &InstructionAst::MoveZone {
                    subject: Box::new(unwrapped),
                    to: to.clone(),
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::MoveZone { subject, to } => {
            let dest = to.trim().to_ascii_lowercase();
            if let Some(bf_flags) = parse_battlefield_dest(&dest) {
                if bf_flags.attach_to_self && !(in_trigger || self_pronoun_valid) {
                    return Err("oracle:unsupported-move-zone");
                }
                let enters_tapped = bf_flags.tapped;
                let attacking = bf_flags.attacking;
                let to_bf = !bf_flags.under_owner_control;
                let bf_dest = SubjectDestination::Battlefield {
                    tapped: enters_tapped,
                    attacking,
                    transformed: bf_flags.transformed,
                    under_owner_control: bf_flags.under_owner_control,
                    with_counter: None,
                    attach_to_self: bf_flags.attach_to_self,
                };
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if !attacking
                        && !bf_flags.transformed
                        && !bf_flags.attach_to_self
                        && (filter.zone == Some(ZoneFilter::YourGraveyard)
                            || (to_bf
                                && matches!(
                                    filter.zone,
                                    Some(ZoneFilter::AnyGraveyard | ZoneFilter::OpponentGraveyard)
                                ))) =>
                    {
                        return lower_instruction(
                            &InstructionAst::ReturnZone {
                                subject: subject.clone(),
                                from_zone: None,
                                to: to.clone(),
                                tapped: enters_tapped,
                                transformed: false,
                                with_counter: None,
                                attached_to: None,
                                random: false,
                                except: None,
                                count: None,
                                up_to: false,
                                any_number: false,
                                actor: None,
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        );
                    }
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                bf_dest,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if matches!(
                            filter.zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::OpponentsGraveyards
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::AllGraveyards
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(
                                count,
                                TargetCountAst::ChosenX | TargetCountAst::UpToX
                            ) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x, is_up_to_x) = match *count {
                            TargetCountAst::Exact(n) => {
                                (usize::from(n), usize::from(n), false, false)
                            }
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false, false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true, false),
                            TargetCountAst::UpToX => (0, usize::MAX, false, true),
                        };
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let bt = match filter.zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                BodyTarget::FilteredOwnGraveyardCard(obj)
                            }
                            Some(
                                ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                            ) => BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: true,
                            },
                            Some(
                                ZoneFilter::AnyGraveyard
                                | ZoneFilter::AllGraveyards
                                | ZoneFilter::SingleGraveyard,
                            ) => BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: false,
                            },
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        if is_up_to_x {
                            builder.up_to_x_target_count = true;
                        }
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                bf_dest,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                            && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                                || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                                || matches!(count, TargetCountAst::AnyNumber)
                                || (matches!(
                                    count,
                                    TargetCountAst::ChosenX | TargetCountAst::UpToX
                                ) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x, is_up_to_x) = match *count {
                            TargetCountAst::Exact(n) => (n, n, false, false),
                            TargetCountAst::UpTo(max) => (0, max, false, false),
                            TargetCountAst::AnyNumber => (0, u16::MAX, false, false),
                            TargetCountAst::ChosenX => (0, u16::MAX, true, false),
                            TargetCountAst::UpToX => (0, u16::MAX, false, true),
                        };
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        if is_x {
                            builder.x_target_count = true;
                        }
                        if is_up_to_x {
                            builder.up_to_x_target_count = true;
                        }
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: min_t,
                                maximum: max_t,
                                action: PermanentTargetAction::MoveTo(bf_dest),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw) => {
                        if is_linked_exile_anaphora(raw)
                            && (in_trigger || self_pronoun_valid)
                            && builder.target == BodyTarget::None
                            && !last_instruction_moved_object(&builder.instructions)
                            && !has_prior_moved_object(&builder.instructions)
                        {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::return_linked_exile(bf_dest, is_singular_linked_exile_anaphora(raw)));
                            return Ok(());
                        }
                        let subj_ref = if builder.target != BodyTarget::None
                            && !matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            ) {
                            MoveSubjectRef::TargetObject
                        } else if last_instruction_moved_object(&builder.instructions)
                            || (is_last_moved_card_anaphora(raw)
                                && has_prior_moved_object(&builder.instructions))
                        {
                            MoveSubjectRef::LastMoved
                        } else if in_trigger {
                            MoveSubjectRef::TriggeringObject
                        } else if self_pronoun_valid {
                            MoveSubjectRef::AbilitySource
                        } else if has_checked_revealed_top_strict(&builder.instructions) {
                            MoveSubjectRef::RevealedCard
                        } else {
                            return Err("oracle:unsupported-move-zone");
                        };
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                subj_ref, bf_dest,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                bf_dest,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AttachedHost,
                                bf_dest,
                            ),
                        );
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AbilitySource,
                                bf_dest,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if try_lower_move_last_moved_filtered(f, bf_dest, builder, vocabulary)? =>
                    {
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        return lower_linked_exile_group_move(
                            f,
                            bf_dest,
                            false,
                            builder,
                            vocabulary,
                            in_trigger,
                            "oracle:unsupported-move-zone",
                        );
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if !bf_flags.under_owner_control
                            && !bf_flags.attach_to_self
                            && f.zone == Some(ZoneFilter::YourHand) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        let hand_match_count = ValueExpr::Count {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(obj),
                            controller_scope: None,
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::Hand, obj, hand_match_count.clone(), hand_match_count, bf_dest, false));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if !attacking
                            && !bf_flags.transformed
                            && classify_graveyard_group(f.zone, None)
                                .is_some_and(|all_gy| to_bf || !all_gy) =>
                    {
                        let all_gy = classify_graveyard_group(f.zone, None).unwrap_or(false);
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group(obj, all_gy, true, enters_tapped));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(players) =
                            resolve_graveyard_group_players(f.zone, None, builder, in_trigger)? =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(players, obj, bf_dest));
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        if !attacking
                            && !bf_flags.transformed
                            && !bf_flags.under_owner_control
                            && !bf_flags.attach_to_self
                            && matches!(f.zone, Some(ZoneFilter::YourHand))
                        {
                            let simple_types = f.forbidden_types.is_empty()
                                && f.required_supertypes.is_empty()
                                && f.forbidden_supertypes.is_empty()
                                && f.subtypes.is_empty()
                                && f.forbidden_subtypes.is_empty()
                                && f.any_colors.is_empty()
                                && f.forbidden_colors.is_empty()
                                && f.color_arity.is_none()
                                && f.required_keywords.is_empty()
                                && f.forbidden_keywords.is_empty()
                                && f.stat_bounds.is_empty()
                                && f.counter_kind.is_none()
                                && f.flags.0 == 0;
                            if simple_types {
                                let types = if f.any_permanent {
                                    BTreeSet::from([
                                        CardType::Artifact,
                                        CardType::Creature,
                                        CardType::Enchantment,
                                        CardType::Land,
                                        CardType::Planeswalker,
                                    ])
                                } else {
                                    card_type_mask_to_set(f.any_types).unwrap_or_default()
                                };
                                let permanent_only = !types.is_empty()
                                    && types.iter().all(|t| {
                                        matches!(
                                            t,
                                            CardType::Artifact
                                                | CardType::Creature
                                                | CardType::Enchantment
                                                | CardType::Land
                                                | CardType::Planeswalker
                                        )
                                    });
                                if permanent_only {
                                    let mut mask = spellwake_engine::program::CardTypeMask::EMPTY;
                                    for ct in types {
                                        mask = mask.with(ct);
                                    }
                                    let obj = ObjectFilter::from_clause(
                                        spellwake_engine::program::ObjectClause::any()
                                            .any_types(mask),
                                    );
                                    builder.instructions.push(
                                        super::super::toolbox_emit::put_hand_permanent_filtered(
                                            ResolvingPlayer::Controller,
                                            obj,
                                            1,
                                            enters_tapped,
                                        ),
                                    );
                                    return Ok(());
                                }
                            } else {
                                let mut inner = f.as_ref().clone();
                                inner.zone = None;
                                if let Ok(obj) = lower_object_filter(&inner, vocabulary) {
                                    builder.instructions.push(
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_hand_permanent_filtered(ResolvingPlayer::Controller, obj, 1, enters_tapped),
                                    );
                                    return Ok(());
                                }
                            }
                        }
                        let mut inner = f.as_ref().clone();
                        if let Ok((sel_player, src_zone)) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Hand,
                            builder,
                            in_trigger,
                        ) {
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(sel_player, src_zone, obj, ValueExpr::literal(1), ValueExpr::literal(1), bf_dest, false));
                            return Ok(());
                        }
                        if let Some(z) = f.zone {
                            let mut trial = builder.clone();
                            let mut inner = f.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter_in_context(
                                    &inner, &trial, in_trigger, vocabulary,
                                )?
                            };
                            bind_filter_player_target(&mut trial, &obj)?;
                            let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                .map_err(|_| "oracle:unsupported-move-zone")?;
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    query,
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: is_hidden_zone_filter(z),
                                    },
                                    super::super::toolbox_emit::subject_dest_to_zone_dest(bf_dest),
                                ),
                            ));
                            *builder = trial;
                            return Ok(());
                        }
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0 {
                            let mut trial = builder.clone();
                            let mut inner = f.as_ref().clone();
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                            let query = if is_unrestricted_filter(&inner) {
                                spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                            } else {
                                let obj = lower_object_filter_in_context(
                                    &inner, &trial, in_trigger, vocabulary,
                                )?;
                                bind_filter_player_target(&mut trial, &obj)?;
                                spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                        group: None,
                                    },
                                    obj,
                                )
                            };
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    query,
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: false,
                                    },
                                    super::super::toolbox_emit::subject_dest_to_zone_dest(bf_dest),
                                ),
                            ));
                            *builder = trial;
                            return Ok(());
                        }
                        if has_prior_moved_object(&builder.instructions)
                            && let Some(filtered) = strip_move_zone_last_moved_filter(
                                f, builder, in_trigger, vocabulary,
                            )
                        {
                            builder.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    spellwake_engine::toolbox::ObjectQuery::filtered(
                                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                        filtered,
                                    ),
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: false,
                                    },
                                    super::super::toolbox_emit::subject_dest_to_zone_dest(bf_dest),
                                ),
                            ));
                            return Ok(());
                        }
                        return Err("oracle:unsupported-move-zone");
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_some() => {
                        let Some(z) = f.zone else {
                            return Err("oracle:unsupported-move-zone");
                        };
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                            .map_err(|_| "oracle:unsupported-move-zone")?;
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                query,
                                super::super::toolbox_emit::subject_dest_to_zone_dest(bf_dest),
                            ),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                    SubjectRefAst::Compound(items) if !items.is_empty() => {
                        for it in items {
                            lower_instruction(
                                &InstructionAst::MoveZone {
                                    subject: Box::new(it.clone()),
                                    to: to.clone(),
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let allow_your_lib = matches!(
                subject.as_ref(),
                SubjectRefAst::SelfObject
                    | SubjectRefAst::Anaphora(_)
                    | SubjectRefAst::All(_)
                    | SubjectRefAst::Each(_)
                    | SubjectRefAst::Group(_)
                    | SubjectRefAst::TriggerSubject
                    | SubjectRefAst::AttachedHost(_)
            ) || matches!(
                subject.as_ref(),
                SubjectRefAst::TargetObject { filter, .. }
                    if matches!(filter.zone, Some(ZoneFilter::YourGraveyard | ZoneFilter::YourHand))
                        || (filter.zone.is_none()
                            && matches!(
                                filter.controller,
                                Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                            ))
            );
            if dest.contains("after_shuffle") && dest.contains("library") {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        if !in_trigger && !self_pronoun_valid {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_resolving_spell_in_library(LibraryPlacement::BottomRandom,));
                        } else {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_ability_source_into_library());
                        }
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        if is_valid_put_target_in_library_target(&bt) {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_target_into_library());
                            return Ok(());
                        }
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_valid_put_target_in_library_target(&builder.target) =>
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_target_into_library());
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if let Some(offset_expr) = parse_dynamic_lib_from_top_offset(
                &dest,
                allow_your_lib,
                bound_x,
                builder,
                in_trigger,
                vocabulary,
            )? {
                if is_self_pronoun(subject, builder, self_pronoun_valid)
                    && !in_trigger
                    && !self_pronoun_valid
                {
                    if let ValueExpr::Literal(n) = &offset_expr
                        && let Some(idx) = n.to_usize().and_then(|v| u8::try_from(v).ok())
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_resolving_spell_in_library(LibraryPlacement::FromTop(idx),));
                        return Ok(());
                    }
                } else {
                    let subj_ref = match subject.as_ref() {
                        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                            Some(MoveSubjectRef::AbilitySource)
                        }
                        SubjectRefAst::TargetObject {
                            count: TargetCountAst::Exact(1),
                            ..
                        } => {
                            let bt = lower_target_subject(subject, vocabulary)?;
                            if is_valid_put_target_in_library_target(&bt) {
                                builder.bind_target(bt)?;
                                Some(MoveSubjectRef::TargetObject)
                            } else {
                                None
                            }
                        }
                        SubjectRefAst::Anaphora(_)
                            if is_valid_put_target_in_library_target(&builder.target) =>
                        {
                            Some(MoveSubjectRef::TargetObject)
                        }
                        SubjectRefAst::Anaphora(_)
                            if last_instruction_created_tokens(&builder.instructions) =>
                        {
                            Some(MoveSubjectRef::CreatedTokens)
                        }
                        SubjectRefAst::Anaphora(_)
                            if last_instruction_moved_object(&builder.instructions) =>
                        {
                            Some(MoveSubjectRef::LastMoved)
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some(MoveSubjectRef::TriggeringObject)
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                            Some(MoveSubjectRef::AttachedHost)
                        }
                        SubjectRefAst::Anaphora(_) if in_trigger => {
                            Some(MoveSubjectRef::TriggeringObject)
                        }
                        SubjectRefAst::Anaphora(_)
                            if has_checked_revealed_top_strict(&builder.instructions) =>
                        {
                            Some(MoveSubjectRef::RevealedCard)
                        }
                        _ => None,
                    };
                    if let Some(subj_ref) = subj_ref {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_subject_into_library_from_top(subj_ref, offset_expr));
                        return Ok(());
                    }
                }
            }
            if let Some(pl) = parse_lib_placement_for_subject(&dest, allow_your_lib, subject) {
                match subject.as_ref() {
                    SubjectRefAst::Group(f)
                        if pl == LibraryPlacement::Bottom
                            && f.zone == Some(ZoneFilter::YourHand)
                            && builder.target == BodyTarget::None =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        if is_unrestricted_filter(&inner) {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        )),
                                        ObjectFilter::any(),
                                    ),
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: true,
                                    },
                                    spellwake_engine::toolbox::ZoneDest::Library {
                                        position:
                                            spellwake_engine::toolbox::LibraryOrderPos::Bottom,
                                    },
                                ),
                            ));
                            return Ok(());
                        }
                    }
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        if is_valid_put_target_in_library_target(&bt) {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(pl));
                            return Ok(());
                        }
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: *n,
                                maximum: *n,
                                action: PermanentTargetAction::MoveTo(SubjectDestination::Library(
                                    pl,
                                )),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(max),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *max > 0 =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                maximum: *max,
                                action: PermanentTargetAction::MoveTo(SubjectDestination::Library(
                                    pl,
                                )),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                maximum: u16::MAX,
                                action: PermanentTargetAction::MoveTo(SubjectDestination::Library(
                                    pl,
                                )),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        if *tc == TargetCountAst::UpToX {
                            builder.up_to_x_target_count = true;
                        } else {
                            builder.x_target_count = true;
                        }
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                maximum: u16::MAX,
                                action: PermanentTargetAction::MoveTo(SubjectDestination::Library(
                                    pl,
                                )),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if matches!(
                            filter.zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::OpponentsGraveyards
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::AllGraveyards
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(
                                count,
                                TargetCountAst::ChosenX | TargetCountAst::UpToX
                            ) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x, is_up_to_x) = match *count {
                            TargetCountAst::Exact(n) => {
                                (usize::from(n), usize::from(n), false, false)
                            }
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false, false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true, false),
                            TargetCountAst::UpToX => (0, usize::MAX, false, true),
                        };
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let bt = match filter.zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                BodyTarget::FilteredOwnGraveyardCard(obj)
                            }
                            Some(
                                ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                            ) => BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: true,
                            },
                            Some(
                                ZoneFilter::AnyGraveyard
                                | ZoneFilter::AllGraveyards
                                | ZoneFilter::SingleGraveyard,
                            ) => BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: false,
                            },
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        if is_up_to_x {
                            builder.up_to_x_target_count = true;
                        }
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let mut inner = f.as_ref().clone();
                        if let Ok((sel_player, src_zone)) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Battlefield,
                            builder,
                            in_trigger,
                        ) {
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(sel_player, src_zone, obj, ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Library(pl), false));
                            return Ok(());
                        }
                        if let Some(z) = f.zone {
                            let mut trial = builder.clone();
                            let mut inner = f.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter_in_context(
                                    &inner, &trial, in_trigger, vocabulary,
                                )?
                            };
                            bind_filter_player_target(&mut trial, &obj)?;
                            let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                .map_err(|_| "oracle:unsupported-move-zone")?;
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    query,
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: is_hidden_zone_filter(z),
                                    },
                                    spellwake_engine::toolbox::ZoneDest::Library {
                                        position:
                                            spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                                pl,
                                            ),
                                    },
                                ),
                            ));
                            *builder = trial;
                            return Ok(());
                        }
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        let is_linked = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                        if is_linked {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        }
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = if is_linked {
                            if is_unrestricted_filter(&inner) {
                                spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                            } else {
                                spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                        group: None,
                                    },
                                    obj,
                                )
                            }
                        } else {
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                obj,
                            )
                        };
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                query,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                spellwake_engine::toolbox::ZoneDest::Library {
                                    position:
                                        spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                            pl,
                                        ),
                                },
                            ),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        if !in_trigger && !self_pronoun_valid {
                            let adj = normalize_resolving_spell_lib_placement(pl, builder.target);
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_resolving_spell_in_library(adj));
                        } else {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::AbilitySource, SubjectDestination::Library(pl)));
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_valid_put_target_in_library_target(&builder.target) =>
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(pl));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::CreatedTokens,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if last_instruction_moved_object(&builder.instructions)
                            || (is_last_moved_card_anaphora(raw)
                                && has_prior_moved_object(&builder.instructions)) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::LastMoved,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AttachedHost,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if has_checked_revealed_top_strict(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::RevealedCard,
                                SubjectDestination::Library(pl),
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if try_lower_move_last_moved_filtered(
                            f,
                            SubjectDestination::Library(pl),
                            builder,
                            vocabulary,
                        )? =>
                    {
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = lower_object_filter_in_context(
                            &inner, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        if pl == LibraryPlacement::Bottom && builder.target == BodyTarget::None {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_group_on_library_bottom(obj));
                        } else {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group_to_library(obj, pl));
                        }
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(players) =
                            resolve_graveyard_group_players(f.zone, None, builder, in_trigger)? =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(players, obj, SubjectDestination::Library(pl)));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_some() => {
                        let Some(z) = f.zone else {
                            return Err("oracle:unsupported-move-zone");
                        };
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                            .map_err(|_| "oracle:unsupported-move-zone")?;
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                query,
                                spellwake_engine::toolbox::ZoneDest::Library {
                                    position:
                                        spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                            pl,
                                        ),
                                },
                            ),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if let Some(query) =
                            parse_whole_zone_anaphora_query(raw, builder, in_trigger)? =>
                    {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                query,
                                spellwake_engine::toolbox::ZoneDest::Library {
                                    position:
                                        spellwake_engine::toolbox::LibraryOrderPos::from_placement(
                                            pl,
                                        ),
                                },
                            ),
                        ));
                        return Ok(());
                    }
                    SubjectRefAst::Compound(items) if !items.is_empty() => {
                        for it in items {
                            lower_instruction(
                                &InstructionAst::MoveZone {
                                    subject: Box::new(it.clone()),
                                    to: to.clone(),
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let pub_dest = if matches!(
                dest.as_str(),
                "hand"
                    | "the hand"
                    | "owner's hand"
                    | "its owner's hand"
                    | "his owner's hand"
                    | "her owner's hand"
                    | "their owner's hand"
                    | "their owners' hands"
                    | "their hands"
                    | "your hand"
                    | "their hand"
                    | "that player's hand"
                    | "its controller's hand"
                    | "to your hand"
                    | "to their hand"
                    | "to their hands"
                    | "to that player's hand"
                    | "to its owner's hand"
                    | "to its controller's hand"
                    | "to their owner's hand"
                    | "to their owners' hands"
                    | "into your hand"
                    | "into their hand"
                    | "into their hands"
                    | "into that player's hand"
                    | "into its owner's hand"
                    | "into its controller's hand"
                    | "into their owner's hand"
                    | "into their owners' hands"
                    | "back into your hand"
                    | "back to your hand"
                    | "back into its owner's hand"
                    | "back to its owner's hand"
                    | "back into their hand"
                    | "back into their owners' hands"
                    | "each player's hand"
                    | "target player's hand"
                    | "target opponent's hand"
            ) {
                Some(PublicZoneDestination::Hand)
            } else if matches!(dest.as_str(), "exile" | "into exile" | "to exile") {
                Some(PublicZoneDestination::Exile)
            } else if matches!(
                dest.as_str(),
                "graveyard"
                    | "the graveyard"
                    | "owner's graveyard"
                    | "its owner's graveyard"
                    | "his owner's graveyard"
                    | "her owner's graveyard"
                    | "their owner's graveyard"
                    | "their owners' graveyards"
                    | "their graveyards"
                    | "your graveyard"
                    | "their graveyard"
                    | "that player's graveyard"
                    | "to your graveyard"
                    | "to their graveyard"
                    | "to that player's graveyard"
                    | "to a graveyard"
                    | "to the graveyard"
                    | "into your graveyard"
                    | "into their graveyard"
                    | "into that player's graveyard"
                    | "into its owner's graveyard"
                    | "into their owner's graveyard"
                    | "into their owners' graveyards"
                    | "into a graveyard"
                    | "into the graveyard"
                    | "to its owner's graveyard"
                    | "to their owner's graveyard"
                    | "each player's graveyard"
                    | "target player's graveyard"
                    | "target opponent's graveyard"
            ) {
                Some(PublicZoneDestination::Graveyard)
            } else {
                None
            };
            if let Some(pd) = pub_dest {
                let subj_dest_from_pd = |pd: PublicZoneDestination| -> Option<SubjectDestination> {
                    match pd {
                        PublicZoneDestination::Hand => Some(SubjectDestination::Hand),
                        PublicZoneDestination::Graveyard => Some(SubjectDestination::Graveyard),
                        PublicZoneDestination::Exile => Some(SubjectDestination::Exile {
                            delayed_return: None,
                        }),
                        _ => None,
                    }
                };
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Hand =>
                    {
                        if !in_trigger && !self_pronoun_valid {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::return_resolving_spell_to_hand());
                        } else {
                            builder
                                .instructions
                                .push(super::super::toolbox_emit::return_source_to_hand());
                        }
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Exile =>
                    {
                        if !in_trigger && !self_pronoun_valid {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_resolving_spell());
                        } else {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_ability_source());
                        }
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Graveyard =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::AbilitySource,
                                SubjectDestination::Graveyard,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && pd == PublicZoneDestination::Hand =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::return_to_owners_hand());
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && pd == PublicZoneDestination::Exile =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } if let Some(sd) = subj_dest_from_pd(pd) => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *n >= 2
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        let action = match pd {
                            PublicZoneDestination::Hand => PermanentTargetAction::ReturnToHand,
                            PublicZoneDestination::Exile => PermanentTargetAction::Exile,
                            _ => PermanentTargetAction::MoveTo(SubjectDestination::Graveyard),
                        };
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: *n,
                                maximum: *n,
                                action,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(max),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *max > 0
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        let action = match pd {
                            PublicZoneDestination::Hand => PermanentTargetAction::ReturnToHand,
                            PublicZoneDestination::Exile => PermanentTargetAction::Exile,
                            _ => PermanentTargetAction::MoveTo(SubjectDestination::Graveyard),
                        };
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                maximum: *max,
                                action,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::ChosenX | TargetCountAst::AnyNumber),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && (*count != TargetCountAst::ChosenX || !in_trigger)
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let mut clean = filter.as_ref().clone();
                        clean.zone = None;
                        let bt = lower_battlefield_body_target(&clean, vocabulary)?;
                        builder.bind_target(bt)?;
                        if *count == TargetCountAst::ChosenX {
                            builder.x_target_count = true;
                        }
                        let action = match pd {
                            PublicZoneDestination::Hand => PermanentTargetAction::ReturnToHand,
                            PublicZoneDestination::Exile => PermanentTargetAction::Exile,
                            _ => PermanentTargetAction::MoveTo(SubjectDestination::Graveyard),
                        };
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                maximum: u16::MAX,
                                action,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if matches!(
                            filter.zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::OpponentsGraveyards
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::AllGraveyards
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) && matches!(
                            pd,
                            PublicZoneDestination::Hand | PublicZoneDestination::Exile
                        ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(
                                count,
                                TargetCountAst::ChosenX | TargetCountAst::UpToX
                            ) && !in_trigger))
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        let (min_t, max_t, is_x, is_up_to_x) = match *count {
                            TargetCountAst::Exact(n) => {
                                (usize::from(n), usize::from(n), false, false)
                            }
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false, false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true, false),
                            TargetCountAst::UpToX => (0, usize::MAX, false, true),
                        };
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let bt = match filter.zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                BodyTarget::FilteredOwnGraveyardCard(obj)
                            }
                            Some(
                                ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                            ) => BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: true,
                            },
                            Some(
                                ZoneFilter::AnyGraveyard
                                | ZoneFilter::AllGraveyards
                                | ZoneFilter::SingleGraveyard,
                            ) => BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: false,
                            },
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        if is_up_to_x {
                            builder.up_to_x_target_count = true;
                        }
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if pd == PublicZoneDestination::Hand
                            && matches!(
                                raw.trim().to_ascii_lowercase().as_str(),
                                "your commander" | "your commander from the command zone"
                            ) =>
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::CommandZone, ObjectFilter::any().with_flags(
                                    spellwake_engine::program::ObjectFilterFlags::COMMANDER_ONLY,
                                ), ValueExpr::literal(1), ValueExpr::literal(1), SubjectDestination::Hand, false));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if matches!(
                            builder.target,
                            BodyTarget::SinglePermanent
                                | BodyTarget::SingleCreature
                                | BodyTarget::SinglePlaneswalker
                                | BodyTarget::FilteredPermanent(_)
                        ) && matches!(
                            pd,
                            PublicZoneDestination::Hand | PublicZoneDestination::Exile
                        ) =>
                    {
                        builder
                            .instructions
                            .push(if pd == PublicZoneDestination::Hand {
                                super::super::toolbox_emit::return_to_owners_hand()
                            } else {
                                super::super::toolbox_emit::exile_target()
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target != BodyTarget::None
                            && !matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            )
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TargetObject,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::CreatedTokens,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if (last_instruction_moved_object(&builder.instructions)
                            || (is_last_moved_card_anaphora(raw)
                                && has_prior_moved_object(&builder.instructions)))
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::LastMoved,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if matches!(
                            builder.target,
                            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                        ) && has_checked_revealed_top_strict(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::RevealedCard,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject
                        if in_trigger && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                sd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Compound(items) if !items.is_empty() => {
                        for it in items {
                            lower_instruction(
                                &InstructionAst::MoveZone {
                                    subject: Box::new(it.clone()),
                                    to: to.clone(),
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                                ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                                pd,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(sd) = subj_dest_from_pd(pd)
                            && try_lower_move_last_moved_filtered(f, sd, builder, vocabulary)? =>
                    {
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if pd != PublicZoneDestination::Exile
                            && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && (in_trigger || self_pronoun_valid)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        return lower_linked_exile_group_move(
                            f,
                            sd,
                            false,
                            builder,
                            vocabulary,
                            in_trigger,
                            "oracle:unsupported-move-zone",
                        );
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if pd == PublicZoneDestination::Hand
                            && classify_graveyard_group(f.zone, None).is_some() =>
                    {
                        let all_gy = classify_graveyard_group(f.zone, None).unwrap_or(false);
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group(obj, all_gy, false, false));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if let Some(sd) = subj_dest_from_pd(pd)
                            && let Some(players) = resolve_graveyard_group_players(
                                f.zone, None, builder, in_trigger,
                            )? =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(players, obj, sd));
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let subj_dest = match pd {
                            PublicZoneDestination::Hand => SubjectDestination::Hand,
                            PublicZoneDestination::Graveyard => SubjectDestination::Graveyard,
                            PublicZoneDestination::Exile => SubjectDestination::Exile {
                                delayed_return: None,
                            },
                            PublicZoneDestination::Battlefield => SubjectDestination::Battlefield {
                                tapped: false,
                                attacking: false,
                                transformed: false,
                                under_owner_control: true,
                                with_counter: None,
                                attach_to_self: false,
                            },
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        let mut inner = f.as_ref().clone();
                        if let Ok((sel_player, src_zone)) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Battlefield,
                            builder,
                            in_trigger,
                        ) {
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder.instructions.push(
                                super::super::toolbox_emit::select_zone_cards_to_zone(
                                    sel_player,
                                    src_zone,
                                    obj,
                                    ValueExpr::literal(1),
                                    ValueExpr::literal(1),
                                    subj_dest,
                                    false,
                                ),
                            );
                            return Ok(());
                        }
                        if let Some(z) = f.zone {
                            let mut trial = builder.clone();
                            let mut inner = f.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter_in_context(
                                    &inner, &trial, in_trigger, vocabulary,
                                )?
                            };
                            bind_filter_player_target(&mut trial, &obj)?;
                            let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                .map_err(|_| "oracle:unsupported-move-zone")?;
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::simple(
                                    query,
                                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                                        chooser:
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        min: Box::new(ValueExpr::literal(1)),
                                        max: Box::new(ValueExpr::literal(1)),
                                        constraint: None,
                                        category:
                                            spellwake_engine::toolbox::ChoiceCategory::Generic,
                                        hidden: is_hidden_zone_filter(z),
                                    },
                                    super::super::toolbox_emit::subject_dest_to_zone_dest(
                                        subj_dest,
                                    ),
                                ),
                            ));
                            *builder = trial;
                            return Ok(());
                        }
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        let is_linked = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                        if is_linked {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        }
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = if is_linked {
                            if is_unrestricted_filter(&inner) {
                                spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                            } else {
                                spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                        group: None,
                                    },
                                    obj,
                                )
                            }
                        } else {
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                obj,
                            )
                        };
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                query,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                super::super::toolbox_emit::subject_dest_to_zone_dest(subj_dest),
                            ),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::move_group(group_obj, pd));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if pd != PublicZoneDestination::Exile
                            && is_linked_exile_anaphora(raw)
                            && (in_trigger || self_pronoun_valid)
                            && builder.target == BodyTarget::None
                            && !last_instruction_moved_object(&builder.instructions)
                            && !has_prior_moved_object(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::return_linked_exile(
                                sd,
                                is_singular_linked_exile_anaphora(raw),
                            ));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        let subj_dest = match pd {
                            PublicZoneDestination::Hand => SubjectDestination::Hand,
                            PublicZoneDestination::Graveyard => SubjectDestination::Graveyard,
                            PublicZoneDestination::Exile => SubjectDestination::Exile {
                                delayed_return: None,
                            },
                            PublicZoneDestination::Battlefield => SubjectDestination::Battlefield {
                                tapped: false,
                                attacking: false,
                                transformed: false,
                                under_owner_control: true,
                                with_counter: None,
                                attach_to_self: false,
                            },
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::move_subject(
                                MoveSubjectRef::TriggeringObject,
                                subj_dest,
                            ));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if self_pronoun_valid
                            && is_self_anaphora_label(raw)
                            && builder.target == BodyTarget::None =>
                    {
                        match pd {
                            PublicZoneDestination::Hand => {
                                builder
                                    .instructions
                                    .push(super::super::toolbox_emit::return_source_to_hand());
                            }
                            PublicZoneDestination::Exile => {
                                builder
                                    .instructions
                                    .push(super::super::toolbox_emit::exile_ability_source());
                            }
                            PublicZoneDestination::Graveyard => {
                                builder.instructions.push(
                                    super::super::toolbox_emit::move_subject(
                                        MoveSubjectRef::AbilitySource,
                                        SubjectDestination::Graveyard,
                                    ),
                                );
                            }
                            PublicZoneDestination::Battlefield => {
                                builder.instructions.push(
                                    super::super::toolbox_emit::move_subject(
                                        MoveSubjectRef::AbilitySource,
                                        SubjectDestination::Battlefield {
                                            tapped: false,
                                            attacking: false,
                                            transformed: false,
                                            under_owner_control: true,
                                            with_counter: None,
                                            attach_to_self: false,
                                        },
                                    ),
                                );
                            }
                            _ => return Err("oracle:unsupported-move-zone"),
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if has_checked_revealed_top_strict(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::move_subject(
                                MoveSubjectRef::RevealedCard,
                                sd,
                            ));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = lower_object_filter(&inner, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::move_group(obj, pd));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone.is_some()
                            && matches!(
                                pd,
                                PublicZoneDestination::Hand
                                    | PublicZoneDestination::Graveyard
                                    | PublicZoneDestination::Exile
                                    | PublicZoneDestination::Battlefield
                            ) =>
                    {
                        let Some(z) = f.zone else {
                            return Err("oracle:unsupported-move-zone");
                        };
                        let mut trial = builder.clone();
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                            .map_err(|_| "oracle:unsupported-move-zone")?;
                        let zone_dest = match pd {
                            PublicZoneDestination::Hand => {
                                spellwake_engine::toolbox::ZoneDest::Hand
                            }
                            PublicZoneDestination::Graveyard => {
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Move,
                                }
                            }
                            PublicZoneDestination::Exile => {
                                spellwake_engine::toolbox::ZoneDest::exile()
                            }
                            PublicZoneDestination::Battlefield => {
                                spellwake_engine::toolbox::ZoneDest::battlefield(
                                    spellwake_engine::toolbox::ZoneController::Owner,
                                )
                            }
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(query, zone_dest),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if let Some(sd) = subj_dest_from_pd(pd)
                            && let Some(query) =
                                parse_whole_zone_anaphora_query(raw, builder, in_trigger)? =>
                    {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    query,
                                    super::super::toolbox_emit::subject_dest_to_zone_dest(sd),
                                ),
                            ));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if dest.contains("battlefield")
                && !dest.contains("blocking")
                && !dest.contains("as a copy")
                && !dest.contains("as a ")
                && !dest.contains("melded")
                && !dest.contains(" with ")
                && !dest.contains("attached to")
                && (dest.contains("face down")
                    || dest.contains("face-down")
                    || dest.contains("under that player's control")
                    || dest.contains("under their control")
                    || dest.contains("under its controller's control"))
            {
                let face_down = dest.contains("face down") || dest.contains("face-down");
                let tapped = dest.contains("tapped") && !dest.contains("untapped");
                let transformed = dest.contains("transformed") || dest.contains("converted");
                let controller_opt = if dest.contains("owner's control")
                    || dest.contains("owners' control")
                {
                    Some(spellwake_engine::toolbox::ZoneController::Owner)
                } else if dest.contains("under your control") || !dest.contains("under ") {
                    Some(spellwake_engine::toolbox::ZoneController::Player(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    )))
                } else if dest.contains("under that player's control")
                    || dest.contains("under their control")
                {
                    lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)
                        .ok()
                        .map(|rp| {
                            spellwake_engine::toolbox::ZoneController::Player(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                            ))
                        })
                } else if dest.contains("under its controller's control") {
                    lower_resolving_player_in_context(
                        PlayerRefAst::ItsController,
                        builder,
                        in_trigger,
                    )
                    .ok()
                    .map(|rp| {
                        spellwake_engine::toolbox::ZoneController::Player(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        ))
                    })
                } else {
                    None
                };
                if let Some(controller) = controller_opt {
                    let zone_dest = spellwake_engine::toolbox::ZoneDest::Battlefield {
                        controller,
                        tapped,
                        attacking: false,
                        transformed,
                        face_down,
                        entering_counters: Vec::new(),
                        attach_to: None,
                    };
                    let mut trial = builder.clone();
                    let query_opt = match subject.as_ref() {
                        s if is_self_pronoun(s, &trial, self_pronoun_valid) => {
                            Some(spellwake_engine::toolbox::ObjectQuery::Source)
                        }
                        SubjectRefAst::TargetObject {
                            count: TargetCountAst::Exact(1),
                            ..
                        } => {
                            if let Ok(bt) = lower_target_subject(subject, vocabulary)
                                && trial.bind_target(bt).is_ok()
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                            } else {
                                None
                            }
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                            Some(spellwake_engine::toolbox::ObjectQuery::AttachedHost)
                        }
                        SubjectRefAst::Anaphora(raw) => {
                            if trial.target != BodyTarget::None
                                && !matches!(
                                    trial.target,
                                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                                )
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                            } else if last_instruction_moved_object(&trial.instructions)
                                || (is_last_moved_card_anaphora(raw)
                                    && has_prior_moved_object(&trial.instructions))
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::LastMoved)
                            } else if in_trigger
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                            } else if self_pronoun_valid {
                                Some(spellwake_engine::toolbox::ObjectQuery::Source)
                            } else if has_checked_revealed_top_strict(&trial.instructions) {
                                Some(spellwake_engine::toolbox::ObjectQuery::Revealed)
                            } else {
                                None
                            }
                        }
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                            if let Some(z) = f.zone {
                                let mut inner = f.as_ref().clone();
                                inner.zone = None;
                                if let Ok(obj) = if is_unrestricted_filter(&inner) {
                                    Ok(ObjectFilter::any())
                                } else {
                                    lower_object_filter_in_context(
                                        &inner, &trial, in_trigger, vocabulary,
                                    )
                                } && bind_filter_player_target(&mut trial, &obj).is_ok()
                                    && let Ok(q) =
                                        zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                {
                                    Some(q)
                                } else {
                                    None
                                }
                            } else if has_prior_moved_object(&trial.instructions)
                                && let Some(filtered) = strip_move_zone_last_moved_filter(
                                    f, &trial, in_trigger, vocabulary,
                                )
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                    filtered,
                                ))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(q) = query_opt {
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(q, zone_dest),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            if dest.contains("shuffle") && dest.contains("library") {
                let mut trial = builder.clone();
                let query_opt = match subject.as_ref() {
                    s if is_self_pronoun(s, &trial, self_pronoun_valid) => {
                        Some(spellwake_engine::toolbox::ObjectQuery::Source)
                    }
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => {
                        if let Ok(bt) = lower_target_subject(subject, vocabulary)
                            && is_valid_put_target_in_library_target(&bt)
                            && trial.bind_target(bt).is_ok()
                        {
                            Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                        } else {
                            None
                        }
                    }
                    SubjectRefAst::Anaphora(raw) => {
                        if let Ok(Some(q)) =
                            parse_whole_zone_anaphora_query(raw, &mut trial, in_trigger)
                        {
                            Some(q)
                        } else if is_valid_put_target_in_library_target(&trial.target) {
                            Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                        } else if last_instruction_moved_object(&trial.instructions)
                            || has_prior_moved_object(&trial.instructions)
                        {
                            Some(spellwake_engine::toolbox::ObjectQuery::LastMoved)
                        } else if in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        {
                            Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                        } else {
                            None
                        }
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                        let z = f.zone.unwrap_or(ZoneFilter::Battlefield);
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
                        if let Ok(obj) = if is_unrestricted_filter(&inner) {
                            Ok(ObjectFilter::any())
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)
                        } && bind_filter_player_target(&mut trial, &obj).is_ok()
                            && let Ok(q) =
                                zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                        {
                            Some(q)
                        } else {
                            None
                        }
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        Some(spellwake_engine::toolbox::ObjectQuery::AttachedHost)
                    }
                    _ => None,
                };
                if let Some(q) = query_opt {
                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            q,
                            spellwake_engine::toolbox::ZoneDest::Library {
                                position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto,
                            },
                        ),
                    ));
                    *builder = trial;
                    return Ok(());
                }
                if let SubjectRefAst::Group(f) = subject.as_ref() {
                    let z = f.zone.unwrap_or(ZoneFilter::Battlefield);
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    if let Ok(obj) = if is_unrestricted_filter(&inner) {
                        Ok(ObjectFilter::any())
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)
                    } && bind_filter_player_target(&mut trial, &obj).is_ok()
                        && let Ok(q) = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                    {
                        trial.instructions.push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                q,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: is_hidden_zone_filter(z),
                                },
                                spellwake_engine::toolbox::ZoneDest::Library {
                                    position:
                                        spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto,
                                },
                            ),
                        ));
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            if dest.contains("exile")
                && (dest.contains("face down") || dest.contains("face-down"))
                && lower_exile_random_or_face_down(
                    None,
                    subject,
                    false,
                    true,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .is_ok()
            {
                return Ok(());
            }
            if dest.contains("battlefield")
                && !dest.contains("as a copy")
                && !dest.contains("as a ")
                && !dest.contains("melded")
                && !dest.contains("blocking")
                && !dest.contains("attached")
                && (dest.contains("face down")
                    || dest.contains("face-down")
                    || dest.contains(" with "))
            {
                let mut trial = builder.clone();
                let mut entering_counters = Vec::new();
                let mut counters_ok = true;
                if dest.contains(" with ") {
                    counters_ok = false;
                    if let Some((_, after_with)) = dest.split_once(" with ")
                        && let Some((cnt_body, _)) = after_with.split_once(" counter")
                    {
                        let cb = cnt_body.trim();
                        let parsed = if let Some(rest) = cb
                            .strip_prefix("a ")
                            .or_else(|| cb.strip_prefix("an "))
                            .or_else(|| cb.strip_prefix("one "))
                        {
                            Some((1i64, rest.trim()))
                        } else if let Some(rest) = cb.strip_prefix("two ") {
                            Some((2i64, rest.trim()))
                        } else if let Some(rest) = cb.strip_prefix("three ") {
                            Some((3i64, rest.trim()))
                        } else if let Some((num_s, rest)) = cb.split_once(' ')
                            && let Ok(n) = num_s.parse::<i64>()
                            && n > 0
                        {
                            Some((n, rest.trim()))
                        } else {
                            None
                        };
                        if let Some((n, kind_s)) = parsed
                            && let Some(kind) =
                                parse_counter_kind(kind_s).filter(|k| k.is_permanent())
                        {
                            entering_counters.push((kind, Box::new(ValueExpr::literal(n))));
                            counters_ok = true;
                        }
                    }
                }
                let ctrl_opt =
                    if dest.contains("owner's control") || dest.contains("owners' control") {
                        Some(spellwake_engine::toolbox::ZoneController::Owner)
                    } else if !dest.contains("under ") || dest.contains("under your control") {
                        Some(spellwake_engine::toolbox::ZoneController::Player(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                        )))
                    } else if dest.contains("under that player's control")
                        || dest.contains("under their control")
                    {
                        lower_resolving_player_in_context(
                            PlayerRefAst::ThatPlayer,
                            &mut trial,
                            in_trigger,
                        )
                        .ok()
                        .map(|rp| {
                            spellwake_engine::toolbox::ZoneController::Player(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                            ))
                        })
                    } else {
                        None
                    };
                if counters_ok && let Some(controller) = ctrl_opt {
                    let zone_dest = spellwake_engine::toolbox::ZoneDest::Battlefield {
                        controller,
                        tapped: dest.contains("tapped") && !dest.contains("untapped"),
                        attacking: dest.contains("attacking"),
                        transformed: dest.contains("transformed") || dest.contains("converted"),
                        face_down: dest.contains("face down") || dest.contains("face-down"),
                        entering_counters,
                        attach_to: None,
                    };
                    let mut pushed = false;
                    match subject.as_ref() {
                        s if is_self_pronoun(s, &trial, self_pronoun_valid) => {
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    spellwake_engine::toolbox::ObjectQuery::Source,
                                    zone_dest,
                                ),
                            ));
                            pushed = true;
                        }
                        SubjectRefAst::TargetObject {
                            count: TargetCountAst::Exact(1),
                            ..
                        } => {
                            if let Ok(bt) = lower_target_subject(subject, vocabulary)
                                && trial.bind_target(bt).is_ok()
                            {
                                trial.instructions.push(super::super::toolbox_emit::zone_op(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        spellwake_engine::toolbox::ObjectQuery::Targets,
                                        zone_dest,
                                    ),
                                ));
                                pushed = true;
                            }
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            trial.instructions.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                    zone_dest,
                                ),
                            ));
                            pushed = true;
                        }
                        SubjectRefAst::Anaphora(raw) => {
                            let q_opt = if trial.target != BodyTarget::None
                                && !matches!(
                                    trial.target,
                                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                                ) {
                                Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                            } else if last_instruction_moved_object(&trial.instructions)
                                || (is_last_moved_card_anaphora(raw)
                                    && has_prior_moved_object(&trial.instructions))
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::LastMoved)
                            } else if in_trigger
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                            {
                                Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                            } else if has_checked_revealed_top_strict(&trial.instructions) {
                                Some(spellwake_engine::toolbox::ObjectQuery::Revealed)
                            } else {
                                None
                            };
                            if let Some(q) = q_opt {
                                trial.instructions.push(super::super::toolbox_emit::zone_op(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(q, zone_dest),
                                ));
                                pushed = true;
                            }
                        }
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_some() => {
                            if let Some(z) = f.zone {
                                let mut inner = f.as_ref().clone();
                                inner.zone = None;
                                if let Ok(obj) = if is_unrestricted_filter(&inner) {
                                    Ok(ObjectFilter::any())
                                } else {
                                    lower_object_filter_in_context(
                                        &inner, &trial, in_trigger, vocabulary,
                                    )
                                } && bind_filter_player_target(&mut trial, &obj).is_ok()
                                    && let Ok(q) =
                                        zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                {
                                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                            q, zone_dest,
                                        ),
                                    ));
                                    pushed = true;
                                }
                            }
                        }
                        SubjectRefAst::Group(f) if f.zone.is_some() => {
                            if let Some(z) = f.zone {
                                let mut inner = f.as_ref().clone();
                                inner.zone = None;
                                if let Ok(obj) = if is_unrestricted_filter(&inner) {
                                    Ok(ObjectFilter::any())
                                } else {
                                    lower_object_filter_in_context(
                                        &inner, &trial, in_trigger, vocabulary,
                                    )
                                } && bind_filter_player_target(&mut trial, &obj).is_ok()
                                    && let Ok(q) =
                                        zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                                {
                                    trial.instructions.push(super::super::toolbox_emit::zone_op(
                                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                                            q,
                                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                                chooser:
                                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                        ResolvingPlayer::Controller,
                                                    ),
                                                min: Box::new(ValueExpr::literal(1)),
                                                max: Box::new(ValueExpr::literal(1)),
                                                constraint: None,
                                                category:
                                                    spellwake_engine::toolbox::ChoiceCategory::Generic,
                                                hidden: is_hidden_zone_filter(z),
                                            },
                                            zone_dest,
                                        ),
                                    ));
                                    pushed = true;
                                }
                            }
                        }
                        _ => {}
                    }
                    if pushed {
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            if let SubjectRefAst::Anaphora(raw) = subject.as_ref()
                && has_prior_moved_object(&builder.instructions)
                && let Some(pick_n) = parse_last_moved_subset_count(raw)
            {
                let dest_opt = if let Some(bf_flags) = parse_battlefield_dest(&dest)
                    && !bf_flags.attach_to_self
                {
                    Some(super::super::toolbox_emit::subject_dest_to_zone_dest(
                        SubjectDestination::Battlefield {
                            tapped: bf_flags.tapped,
                            attacking: bf_flags.attacking,
                            transformed: bf_flags.transformed,
                            under_owner_control: bf_flags.under_owner_control,
                            with_counter: None,
                            attach_to_self: false,
                        },
                    ))
                } else if let Some(pl) = parse_lib_placement_for_subject(&dest, true, subject) {
                    Some(spellwake_engine::toolbox::ZoneDest::Library {
                        position: spellwake_engine::toolbox::LibraryOrderPos::from_placement(pl),
                    })
                } else if let Some(pd) = pub_dest {
                    match pd {
                        PublicZoneDestination::Hand => {
                            Some(spellwake_engine::toolbox::ZoneDest::Hand)
                        }
                        PublicZoneDestination::Graveyard => {
                            Some(spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Move,
                            })
                        }
                        PublicZoneDestination::Exile => {
                            Some(spellwake_engine::toolbox::ZoneDest::exile())
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(zone_dest) = dest_opt {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(pick_n)),
                                    max: Box::new(ValueExpr::literal(pick_n)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                zone_dest,
                            ),
                        ));
                    return Ok(());
                }
            }
            Err("oracle:unsupported-move-zone")
        }
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except,
            half,
            rounding,
            chooser: Some(c),
        } if chooser_matches_actor(*c, *player) => lower_instruction(
            &InstructionAst::Sacrifice {
                player: *player,
                count: count.clone(),
                subject: subject.clone(),
                except: except.clone(),
                half: *half,
                rounding: *rounding,
                chooser: None,
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except: Some(ex),
            half,
            rounding,
            chooser: None,
        } => {
            let patched = apply_except_to_subject(subject, ex)?;
            lower_instruction(
                &InstructionAst::Sacrifice {
                    player: *player,
                    count: count.clone(),
                    subject: Box::new(patched),
                    except: None,
                    half: *half,
                    rounding: *rounding,
                    chooser: None,
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::Sacrifice {
            player,
            count: None,
            subject,
            except: None,
            half: true,
            rounding,
            chooser: None,
        } if matches!(
            player,
            PlayerRefAst::You
                | PlayerRefAst::TargetPlayer
                | PlayerRefAst::TargetOpponent
                | PlayerRefAst::ThatPlayer
                | PlayerRefAst::EachPlayer
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachOtherPlayer
        ) =>
        {
            let f = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) if f.zone.is_none() => {
                    strip_matching_chooser(f, *player)
                }
                _ => return Err("oracle:unsupported-sacrifice"),
            };
            let f = &f;
            let group_players = try_group_affected_players(*player);
            let rp = if group_players.is_some() {
                ResolvingPlayer::LoopPlayer
            } else {
                lower_resolving_player_in_context(*player, builder, in_trigger)?
            };
            let ptf = lower_permanent_type_filter(f, vocabulary)?;
            let mut count_f = f.clone();
            if matches!(
                (player, count_f.controller),
                (PlayerRefAst::You, Some(ControllerRelation::You))
                    | (
                        PlayerRefAst::TargetPlayer,
                        Some(ControllerRelation::TargetPlayer | ControllerRelation::ThatPlayer)
                    )
                    | (
                        PlayerRefAst::TargetOpponent,
                        Some(ControllerRelation::TargetOpponent | ControllerRelation::ThatPlayer)
                    )
                    | (
                        PlayerRefAst::ThatPlayer
                            | PlayerRefAst::EachPlayer
                            | PlayerRefAst::EachOpponent
                            | PlayerRefAst::EachOtherPlayer,
                        Some(ControllerRelation::ThatPlayer)
                    )
            ) {
                count_f.controller = None;
            }
            let obj_filter = if is_unrestricted_filter(&count_f) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&count_f, vocabulary)?
            };
            let Some(two) = std::num::NonZeroU16::new(2) else {
                return Err("oracle:unsupported-sacrifice");
            };
            let half_expr = ValueExpr::DivConst {
                expr: Box::new(ValueExpr::count_player_battlefield(rp, obj_filter)),
                divisor: two,
                round_up: matches!(rounding, Some(spellwake_oracle_compiler::ast::Rounding::Up)),
            };
            let sac_op = Instruction::Sacrifice {
                players: AffectedPlayers::One(rp),
                amount: half_expr,
                filter: ptf,
            };
            if let Some(players) = group_players {
                push_for_each_player(players, vec![sac_op], builder);
            } else {
                builder.instructions.push(sac_op);
            }
            Ok(())
        }
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        } => match subject.as_ref() {
            s if count.is_none()
                && matches!(player, PlayerRefAst::You)
                && is_self_pronoun(s, builder, self_pronoun_valid) =>
            {
                if in_trigger || self_pronoun_valid {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_subject(
                            EffectSubject::AbilitySource,
                        ),
                    );
                    Ok(())
                } else {
                    Err("oracle:unsupported-sacrifice")
                }
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if count.is_none()
                && filter.zone.is_none()
                && matches!(
                    player,
                    PlayerRefAst::You
                        | PlayerRefAst::ThatObjectController
                        | PlayerRefAst::ItsController
                        | PlayerRefAst::ThatPlayer
                ) =>
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_subject(
                        EffectSubject::TargetObject,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::TriggerSubject if count.is_none() && in_trigger => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_subject(
                        EffectSubject::TriggerSubject,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::AttachedHost(_)
                if count.is_none() && (in_trigger || self_pronoun_valid) =>
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_subject(
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        )),
                    ),
                );
                Ok(())
            }
            SubjectRefAst::Anaphora(raw) if count.is_none() => {
                let low_raw = raw.trim().to_ascii_lowercase();
                let eff_subj = if is_permanent_body_target(builder.target) {
                    EffectSubject::TargetObject
                } else if last_instruction_created_tokens(&builder.instructions) {
                    EffectSubject::CreatedTokens
                } else if matches!(
                    low_raw.as_str(),
                    "this creature"
                        | "this permanent"
                        | "this artifact"
                        | "this enchantment"
                        | "this land"
                ) && (in_trigger || self_pronoun_valid)
                {
                    EffectSubject::AbilitySource
                } else if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) {
                    EffectSubject::TriggerSubject
                } else if self_pronoun_valid {
                    EffectSubject::AbilitySource
                } else if last_instruction_moved_object(&builder.instructions) {
                    EffectSubject::LastMoved
                } else if let Some(group_obj) = builder.last_group_filter {
                    EffectSubject::Matching(group_obj)
                } else {
                    return Err("oracle:unsupported-sacrifice");
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_subject(
                        eff_subj,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::All(f)
                if count.is_none() && matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
            {
                let mut inner = strip_matching_chooser(f, *player);
                inner.zone = None;
                let mut trial = builder.clone();
                let players = lower_affected_players_in_context(*player, &mut trial, in_trigger)
                    .map_err(|_| "oracle:sacrifice-all-unsupported")?;
                if inner.controller.is_some() {
                    let compatible = matches!(
                        (player, inner.controller),
                        (PlayerRefAst::You, Some(ControllerRelation::You))
                            | (
                                PlayerRefAst::EachOpponent,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::Opponent
                                        | ControllerRelation::Opponents,
                                ),
                            )
                            | (
                                PlayerRefAst::EachPlayer,
                                Some(ControllerRelation::ThatPlayer)
                            )
                            | (
                                PlayerRefAst::EachOtherPlayer,
                                Some(ControllerRelation::ThatPlayer | ControllerRelation::NotYou,),
                            )
                            | (
                                PlayerRefAst::TargetPlayer,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::TargetPlayer,
                                ),
                            )
                            | (
                                PlayerRefAst::TargetOpponent,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::TargetOpponent,
                                ),
                            )
                            | (
                                PlayerRefAst::ThatPlayer,
                                Some(ControllerRelation::ThatPlayer)
                            )
                            | (
                                PlayerRefAst::DefendingPlayer,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::DefendingPlayer,
                                ),
                            )
                            | (
                                PlayerRefAst::ActivePlayer
                                    | PlayerRefAst::EnchantedPlayer
                                    | PlayerRefAst::ItsController
                                    | PlayerRefAst::ItsOwner
                                    | PlayerRefAst::ThatObjectController
                                    | PlayerRefAst::ChosenPlayer,
                                Some(ControllerRelation::ThatPlayer),
                            )
                    );
                    if !compatible {
                        return Err("oracle:sacrifice-all-unsupported");
                    }
                    inner.controller = None;
                }
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                };
                bind_filter_player_target(&mut trial, &obj)?;
                trial.instructions.push(super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                            ),
                            obj,
                        ),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                        },
                    ),
                ));
                *builder = trial;
                Ok(())
            }
            SubjectRefAst::Group(f) | SubjectRefAst::Each(f) => {
                let f = strip_matching_chooser(f, *player);
                let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
                if count.is_none()
                    && matches!(player, PlayerRefAst::You)
                    && lower_sacrifice_selection(&f, vocabulary).is_some()
                    && let Ok(obj) = lower_object_filter(&f, vocabulary)
                    && !obj.has_target_player()
                    && !obj.uses_target_permanent()
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::simple(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        ),
                                    ),
                                    obj,
                                ),
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: false,
                                },
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                                },
                            ),
                        ));
                    return Ok(());
                }
                let amount = match count {
                    Some(c) => lower_value_expr_in_context(
                        resolve_x(c, bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )?,
                    None => ValueExpr::literal(1),
                };
                let ptf = lower_permanent_type_filter(&f, vocabulary)?;
                builder.instructions.push(Instruction::Sacrifice {
                    players,
                    amount,
                    filter: ptf,
                });
                Ok(())
            }
            SubjectRefAst::All(_) => Err("oracle:sacrifice-all-unsupported"),
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for (idx, it) in items.iter().enumerate() {
                    let next_player = if idx > 0
                        && matches!(
                            player,
                            PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                        ) {
                        PlayerRefAst::ThatPlayer
                    } else {
                        *player
                    };
                    lower_instruction(
                        &InstructionAst::Sacrifice {
                            player: next_player,
                            count: count.clone(),
                            subject: Box::new(it.clone()),
                            except: None,
                            half: false,
                            rounding: None,
                            chooser: None,
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                Ok(())
            }
            _ => Err("oracle:unsupported-sacrifice"),
        },
        InstructionAst::SacrificeAnyNumber { player, subject } => match subject.as_ref() {
            SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                if f.zone.is_none() =>
            {
                let f = strip_matching_chooser(f, *player);
                let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
                let ptf = lower_permanent_type_filter(&f, vocabulary)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_up_to(
                        players,
                        ValueExpr::literal(i64::from(u16::MAX)),
                        ptf,
                    ),
                );
                Ok(())
            }
            _ => Err("oracle:unsupported-sacrifice"),
        },
        InstructionAst::SacrificeUnlessPay {
            subject,
            cost: CostAtomAst::PayMana(raw),
        } if is_self_pronoun(subject, builder, self_pronoun_valid) => {
            let idx = builder.instructions.len();
            if let Some(cost) = parse_fixed_mana_cost(raw) {
                builder
                    .instructions
                    .push(Instruction::OfferDynamicManaPayment {
                        payer: ResolvingPlayer::Controller,
                        fixed: cost,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: idx + 2,
                        declined_destination: idx + 1,
                    });
            } else if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
                && x_cnt > 0
            {
                let x_val = match bound_x {
                    Some(expr) => {
                        lower_value_expr_in_context(expr, builder, in_trigger, vocabulary)?
                    }
                    None => ValueExpr::chosen_x(),
                };
                let extra_generic = scale_value_expr(x_val, i64::from(x_cnt));
                builder
                    .instructions
                    .push(Instruction::OfferDynamicManaPayment {
                        payer: ResolvingPlayer::Controller,
                        fixed,
                        extra_generic,
                        paid_destination: idx + 2,
                        declined_destination: idx + 1,
                    });
            } else {
                return Err("oracle:sac-unless-x");
            }
            builder
                .instructions
                .push(super::super::toolbox_emit::sacrifice_subject(
                    EffectSubject::AbilitySource,
                ));
            Ok(())
        }
        InstructionAst::DiscardUnless {
            player,
            count,
            unless_filter,
        } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let obj_filter = lower_object_filter(unless_filter, vocabulary)?;
            if let Some(players) = try_group_affected_players(*player) {
                let rp = ResolvingPlayer::LoopPlayer;
                push_for_each_player(
                    players,
                    vec![
                        Instruction::OptionalEffect { chooser: rp },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_filtered(
                            AffectedPlayers::One(rp),
                            ValueExpr::literal(1),
                            obj_filter,
                        ),
                        Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                                comparison: Comparison::Less,
                                right: ValueExpr::literal(1),
                            },
                            destination: 4,
                        },
                        Instruction::Discard {
                            players: AffectedPlayers::One(rp),
                            amount: amt,
                        },
                    ],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::OptionalEffect { chooser: rp });
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_filtered(
                    AffectedPlayers::One(rp),
                    ValueExpr::literal(1),
                    obj_filter,
                ),
            );
            let branch_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: Comparison::Less,
                    right: ValueExpr::literal(1),
                },
                destination: branch_idx + 2,
            });
            builder.instructions.push(Instruction::Discard {
                players: AffectedPlayers::One(rp),
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Discard {
            player,
            count: None,
            filter,
            anaphora: Some(a),
            random,
        } if matches!(
            a.trim().to_ascii_lowercase().as_str(),
            "that many cards" | "that many card" | "that many"
        ) && filter
            .as_deref()
            .is_none_or(|f| is_unrestricted_filter(&strip_matching_chooser(f, *player))) =>
        {
            let amt = ValueExpr::EventValue(EventValueKind::LastActionCount);
            if *random {
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                builder.instructions.push(Instruction::RepeatLoop {
                    count: amt,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(rp)],
                    )),
                });
                return Ok(());
            }
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Discard {
                players,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Discard {
            player: PlayerRefAst::You | PlayerRefAst::ThatPlayer,
            count: None,
            filter: None,
            anaphora: Some(_),
            random: false,
        } if has_checked_revealed_top(&builder.instructions) => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                    MoveSubjectRef::RevealedCard,
                    SubjectDestination::Graveyard,
                ),
            );
            Ok(())
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora: Some(a),
            random: false,
        } if has_prior_moved_object(&builder.instructions) => {
            let low = a.trim().to_ascii_lowercase();
            let mut trial = builder.clone();
            let filtered_source = match filter.as_deref() {
                None => spellwake_engine::toolbox::ObjectQuery::LastMoved,
                Some(f) => {
                    let stripped = strip_matching_chooser(f, *player);
                    if is_unrestricted_filter(&stripped) {
                        spellwake_engine::toolbox::ObjectQuery::LastMoved
                    } else {
                        let obj = lower_object_filter_in_context(
                            &stripped, &trial, in_trigger, vocabulary,
                        )?;
                        spellwake_engine::toolbox::ObjectQuery::filtered(
                            spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            obj,
                        )
                    }
                }
            };
            if count.is_none()
                && matches!(
                    low.as_str(),
                    "that card"
                        | "it"
                        | "those cards"
                        | "the chosen card"
                        | "the chosen cards"
                        | "that"
                        | "them"
                )
            {
                trial.instructions.push(super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        filtered_source,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                        },
                    ),
                ));
                *builder = trial;
                return Ok(());
            }
            let pick_count_opt = if let Some(c) = count {
                Some(lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    &trial,
                    in_trigger,
                    vocabulary,
                )?)
            } else {
                parse_last_moved_subset_count(&low).map(ValueExpr::literal)
            };
            if let Some(amt) = pick_count_opt {
                let chooser_rp =
                    lower_resolving_player_in_context(*player, &mut trial, in_trigger)?;
                trial.instructions.push(super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        filtered_source,
                        spellwake_engine::toolbox::ZoneSelection::Prompt {
                            chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                chooser_rp,
                            ),
                            min: Box::new(amt.clone()),
                            max: Box::new(amt),
                            constraint: None,
                            category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                            hidden: true,
                        },
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                        },
                    ),
                ));
                *builder = trial;
                return Ok(());
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::Discard {
            player,
            count,
            filter: Some(f),
            anaphora: None,
            random: true,
        } if !is_unrestricted_filter(&strip_matching_chooser(f, *player)) => {
            let stripped = strip_matching_chooser(f, *player);
            let amt = match count {
                Some(c) => lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                None => ValueExpr::literal(1),
            };
            let obj_filter = lower_object_filter(&stripped, vocabulary)?;
            if let Some(players) = try_group_affected_players(*player) {
                const RAND_DISC_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(916);
                let seat_sel = spellwake_engine::toolbox::PlayerSelector::Bound(RAND_DISC_BIND);
                let inner = super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Hand(Box::new(seat_sel.clone())),
                            obj_filter,
                        ),
                        spellwake_engine::toolbox::ZoneSelection::Random {
                            count: Box::new(amt),
                            chooser: seat_sel,
                        },
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                        },
                    ),
                );
                builder.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: RAND_DISC_BIND,
                    body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
                });
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                            )),
                            obj_filter,
                        ),
                        spellwake_engine::toolbox::ZoneSelection::Random {
                            count: Box::new(amt),
                            chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        },
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                        },
                    ),
                ));
            Ok(())
        }
        InstructionAst::Discard {
            player,
            count,
            filter: Some(f),
            anaphora: None,
            random: false,
        } if !is_unrestricted_filter(&strip_matching_chooser(f, *player)) => {
            let stripped = strip_matching_chooser(f, *player);
            let amt = match count {
                Some(c) => lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                None => ValueExpr::literal(1),
            };
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            let obj_filter = lower_object_filter(&stripped, vocabulary)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_filtered(
                    players, amt, obj_filter,
                ),
            );
            Ok(())
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora: None,
            random,
        } if filter
            .as_deref()
            .is_none_or(|f| is_unrestricted_filter(&strip_matching_chooser(f, *player))) =>
        {
            let amt = match count {
                Some(c) => lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                None => ValueExpr::literal(1),
            };
            if *random {
                if let Some(players) = try_group_affected_players(*player) {
                    const RAND_DISC_GRP_BIND: spellwake_engine::toolbox::BindingId =
                        spellwake_engine::toolbox::BindingId(917);
                    let seat_sel =
                        spellwake_engine::toolbox::PlayerSelector::Bound(RAND_DISC_GRP_BIND);
                    let inner = super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                    seat_sel.clone(),
                                )),
                                ObjectFilter::any(),
                            ),
                            spellwake_engine::toolbox::ZoneSelection::Random {
                                count: Box::new(amt),
                                chooser: seat_sel,
                            },
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                            },
                        ),
                    );
                    builder.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                            players,
                        )),
                        bind_player: RAND_DISC_GRP_BIND,
                        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
                    });
                    return Ok(());
                }
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                if amt == ValueExpr::literal(1) {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(rp));
                    return Ok(());
                }
                if let ValueExpr::Literal(n) = &amt
                    && let Some(k) = n.to_usize()
                    && (2..=4).contains(&k)
                {
                    for _ in 0..k {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(rp));
                    }
                    return Ok(());
                }
                builder.instructions.push(Instruction::RepeatLoop {
                    count: amt,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(rp)],
                    )),
                });
                return Ok(());
            }
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Discard {
                players,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::DiscardHand { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::Discard {
                        players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                        amount: ValueExpr::count_hand(ResolvingPlayer::LoopPlayer),
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Discard {
                players: AffectedPlayers::One(rp),
                amount: ValueExpr::count_hand(rp),
            });
            Ok(())
        }
        InstructionAst::DiscardUpTo {
            player,
            max_count,
            filter,
            any_number,
            all,
            random,
        } => {
            if *random && !*all {
                return Err("oracle:unsupported-discard-up-to-random");
            }
            let mut trial = builder.clone();
            let lowered_filter: Option<Box<ObjectFilter>> = match filter.as_deref() {
                None => None,
                Some(f) => {
                    let mut stripped = strip_matching_chooser(f, *player);
                    if matches!(
                        stripped.zone,
                        Some(
                            ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand | ZoneFilter::AnyHand
                        )
                    ) {
                        stripped.zone = None;
                    }
                    if is_unrestricted_filter(&stripped) {
                        None
                    } else {
                        let obj = lower_object_filter_in_context(
                            &stripped, &trial, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(&mut trial, &obj)?;
                        Some(Box::new(obj))
                    }
                }
            };
            if *all {
                if let Some(players) = try_group_affected_players(*player) {
                    let op = match lowered_filter {
                        None => Instruction::Discard {
                            players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                            amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(
                                ResolvingPlayer::LoopPlayer,
                            )),
                        },
                        Some(obj_filter) => crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_filtered(AffectedPlayers::One(ResolvingPlayer::LoopPlayer), ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(
                                ResolvingPlayer::LoopPlayer,
                            )), *(obj_filter)),
                    };
                    push_for_each_player(players, vec![op], &mut trial);
                } else {
                    let rp = lower_resolving_player_in_context(*player, &mut trial, in_trigger)?;
                    let op = match lowered_filter {
                        None => Instruction::Discard {
                            players: AffectedPlayers::One(rp),
                            amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(rp)),
                        },
                        Some(obj_filter) => crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_filtered(AffectedPlayers::One(rp), ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(rp)), *(obj_filter)),
                    };
                    trial.instructions.push(op);
                }
                *builder = trial;
                return Ok(());
            }
            let players = lower_affected_players_in_context(*player, &mut trial, in_trigger)?;
            let max_expr = if *any_number {
                ValueExpr::literal(0)
            } else {
                lower_value_expr_in_context(
                    resolve_x(max_count, bound_x),
                    &trial,
                    in_trigger,
                    vocabulary,
                )?
            };
            trial.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_up_to_ext(
                    players,
                    max_expr,
                    (lowered_filter).map(|b| *b),
                    *any_number,
                ),
            );
            *builder = trial;
            Ok(())
        }
        InstructionAst::Mill { player, count } => {
            let amt = rewrite_milled_card_mana_value(
                lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                builder,
            );
            if matches!(player, PlayerRefAst::EachPlayer) {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::mill_players(
                        AffectedPlayers::EachPlayer,
                        amt,
                    ));
                return Ok(());
            }
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::Mill {
                        player: ResolvingPlayer::LoopPlayer,
                        amount: amt,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Mill {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::ShuffleLibrary {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library(),
            );
            Ok(())
        }
        InstructionAst::ShuffleLibrary { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(ResolvingPlayer::LoopPlayer,)],
                    builder,
                );
                return Ok(());
            }
            let rp = if *player == PlayerRefAst::ThatPlayer {
                let prior_search_player =
                    builder.instructions.iter().rev().find_map(|op| match op {
                        Instruction::SearchLibrary { filter } => match filter.source {
                            spellwake_engine::program::SearchSource::OwnLibrary
                            | spellwake_engine::program::SearchSource::OwnLibraryOrGraveyard => {
                                Some(filter.searcher)
                            }
                            spellwake_engine::program::SearchSource::TargetPlayerLibrary
                            | spellwake_engine::program::SearchSource::TargetOpponentLibrary => {
                                Some(ResolvingPlayer::TargetPlayer)
                            }
                            spellwake_engine::program::SearchSource::PlayerLibrary(_) => None,
                        },
                        _ => None,
                    });
                if let Some(sp) = prior_search_player {
                    sp
                } else if let Ok(ctx_p) =
                    lower_resolving_player_in_context(*player, builder, in_trigger)
                {
                    ctx_p
                } else if let Some(prior_p) =
                    extract_prior_resolving_player_for_shuffle(&builder.instructions, in_trigger)
                {
                    prior_p
                } else if let Some(loop_players) =
                    builder.instructions.iter().rev().find_map(|op| match op {
                        Instruction::ForEachPlayerQuery { selector, body, .. } => {
                            match selector.as_ref() {
                                spellwake_engine::toolbox::PlayerSelector::Group(players)
                                    if !body.instructions.is_empty()
                                        || !matches!(players, AffectedPlayers::One(_)) =>
                                {
                                    Some(*players)
                                }
                                _ => None,
                            }
                        }
                        Instruction::DrawCards(spec) => match &spec.players {
                            spellwake_engine::toolbox::PlayerSelector::Group(players)
                                if !matches!(players, AffectedPlayers::One(_)) =>
                            {
                                Some(*players)
                            }
                            _ => None,
                        },
                        Instruction::Sacrifice { players, .. }
                        | Instruction::Discard { players, .. }
                            if !matches!(players, AffectedPlayers::One(_)) =>
                        {
                            Some(*players)
                        }
                        Instruction::ZoneOp(spec) => extract_zone_op_group_players(&spec.source),
                        _ => None,
                    })
                {
                    push_for_each_player(
                        loop_players,
                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(ResolvingPlayer::LoopPlayer,)],
                        builder,
                    );
                    return Ok(());
                } else if has_prior_moved_object(&builder.instructions) {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::shuffle(
                                spellwake_engine::toolbox::PlayerSelector::OwnersOf(Box::new(
                                    spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                )),
                            ),
                        ));
                    return Ok(());
                } else if let Some(any_p) =
                    builder.instructions.iter().rev().find_map(|op| match op {
                        Instruction::ZoneOp(spec) => extract_zone_op_resolving_player(&spec.source)
                            .or(match &spec.selection {
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser,
                                    ..
                                }
                                | spellwake_engine::toolbox::ZoneSelection::Random {
                                    chooser,
                                    ..
                                } => match chooser {
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(p)
                                    | spellwake_engine::toolbox::PlayerSelector::Group(
                                        AffectedPlayers::One(p),
                                    ) => Some(*p),
                                    _ => None,
                                },
                                _ => None,
                            }),
                        _ => None,
                    })
                {
                    any_p
                } else {
                    return Err("oracle:unbound-that-player");
                }
            } else {
                lower_resolving_player_in_context(*player, builder, in_trigger)?
            };
            if rp == ResolvingPlayer::Controller {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library(),
                );
            } else {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(
                        rp,
                    ),
                );
            }
            Ok(())
        }
        InstructionAst::RevealHand { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(
                            ResolvingPlayer::LoopPlayer,
                        ),
                    ],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(rp),
            );
            Ok(())
        }
        InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        } => {
            builder
                .instructions
                .push(emit_shuffle_own_graveyard_into_library());
            Ok(())
        }
        InstructionAst::ShuffleGraveyardIntoLibrary { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(AffectedPlayers::One(ResolvingPlayer::LoopPlayer), ObjectFilter::any(), SubjectDestination::Library(LibraryPlacement::Bottom)),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(ResolvingPlayer::LoopPlayer),
                    ],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_graveyard_group_owned(
                    AffectedPlayers::One(rp),
                    ObjectFilter::any(),
                    SubjectDestination::Library(LibraryPlacement::Bottom),
                ),
            );
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(rp),
            );
            Ok(())
        }
        InstructionAst::PhaseOut { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::phase_out_query(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                    ));
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::phase_out_query(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                    ));
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if (in_trigger || self_pronoun_valid || builder.has_source)
                    && builder.target == BodyTarget::None
                    && matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "he" | "she" | "him" | "her"
                    ) =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::phase_out_query(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                    ));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target_in_context(
                    filter, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(super::super::toolbox_emit::phase_out_query(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                    ));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(1),
            } if filter.zone.is_none() && builder.target == BodyTarget::None => {
                let bt = lower_battlefield_body_target_in_context(
                    filter, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder.up_to_one_target = true;
                builder
                    .instructions
                    .push(super::super::toolbox_emit::phase_out_query(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                    ));
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                if f.zone.is_none()
                    && f.controller == Some(ControllerRelation::You)
                    && f.any_types == AstCardTypeMask::LAND
                    && filter_only_has(
                        f,
                        FilterAllow {
                            types: true,
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    ) =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::phase_out_query(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            ObjectFilter::land().controlled(),
                        ),
                    ));
                Ok(())
            }
            _ => Err("oracle:unsupported-phase-out"),
        },
        InstructionAst::PutInLibraryAtPosition {
            player: _,
            subject,
            from_top,
        } if *from_top >= 1 => {
            let offset_u8 = from_top - 1;
            let eff_subj = unwrap_in_zone_subject(subject).unwrap_or_else(|| (**subject).clone());
            if is_self_pronoun(&eff_subj, builder, self_pronoun_valid)
                && !in_trigger
                && !self_pronoun_valid
            {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_resolving_spell_in_library(LibraryPlacement::FromTop(offset_u8),));
                return Ok(());
            }
            let subj_ref = lower_move_subject_ref(
                &eff_subj,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_subject_into_library_from_top(subj_ref, ValueExpr::literal(i64::from(offset_u8))));
            Ok(())
        }
        InstructionAst::MoveCounted {
            actor,
            count,
            up_to,
            any_number,
            subject,
            to,
        } => {
            let (SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)) =
                subject.as_ref()
            else {
                return Err("oracle:unsupported-move-counted-subject");
            };
            let dest = to.trim().to_ascii_lowercase();
            let subj_dest = if let Some(pl) = parse_lib_placement(&dest, true) {
                SubjectDestination::Library(pl)
            } else if dest.contains("exile") {
                SubjectDestination::Exile {
                    delayed_return: None,
                }
            } else if dest.contains("hand") {
                SubjectDestination::Hand
            } else if dest.contains("graveyard") {
                SubjectDestination::Graveyard
            } else if dest.contains("battlefield") && !dest.contains("attacking") {
                SubjectDestination::Battlefield {
                    tapped: dest.contains("tapped"),
                    attacking: false,
                    transformed: false,
                    under_owner_control: is_battlefield_owner_control_dest(&dest),
                    with_counter: None,
                    attach_to_self: false,
                }
            } else {
                return Err("oracle:unsupported-move-counted-dest");
            };
            let mut inner = f.as_ref().clone();
            let src_zone = if strip_last_moved_predicates(&mut inner, &builder.instructions) {
                SourceZoneFilter::LastMoved
            } else {
                let z = match inner.zone {
                    Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => {
                        SourceZoneFilter::Hand
                    }
                    Some(ZoneFilter::YourHandOrGraveyard) => SourceZoneFilter::HandOrGraveyard,
                    Some(ZoneFilter::YourGraveyard) => SourceZoneFilter::Graveyard,
                    Some(ZoneFilter::OpponentGraveyard) => SourceZoneFilter::OpponentGraveyard,
                    Some(ZoneFilter::AnyGraveyard) => SourceZoneFilter::AnyGraveyard,
                    Some(ZoneFilter::Exile) => SourceZoneFilter::Exile,
                    Some(ZoneFilter::Battlefield) | None => SourceZoneFilter::Battlefield,
                    _ => return Err("oracle:unsupported-move-counted-zone"),
                };
                inner.zone = None;
                z
            };
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            let group_players = actor.and_then(try_group_affected_players);
            let player = if group_players.is_some() {
                ResolvingPlayer::LoopPlayer
            } else {
                match actor {
                    Some(p) => lower_resolving_player_in_context(*p, builder, in_trigger)?,
                    None => ResolvingPlayer::Controller,
                }
            };
            let (min, max) = if *any_number {
                (
                    ValueExpr::literal(0),
                    ValueExpr::literal(i64::from(u16::MAX)),
                )
            } else if let Some(cnt_ast) = count.as_deref() {
                let c = lower_value_expr_in_context(
                    resolve_x(cnt_ast, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?;
                let min_v = if *up_to {
                    ValueExpr::literal(0)
                } else {
                    c.clone()
                };
                (min_v, c)
            } else {
                return Err("oracle:unsupported-move-counted-count");
            };
            let op =
                crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(
                    player, src_zone, obj, min, max, subj_dest, false,
                );
            if let Some(players) = group_players {
                push_for_each_player(players, vec![op], builder);
            } else {
                builder.instructions.push(op);
            }
            Ok(())
        }
        InstructionAst::ExileTopLibrary {
            actor,
            whose,
            count,
            face_down,
        } => {
            let w = whose.trim().to_ascii_lowercase();
            if w.ends_with("_bottom") {
                return Err("oracle:unsupported-exile-top-whose");
            }
            let (req_target, library_owner) = match w.as_str() {
                "your" | "own" => (None, AffectedPlayers::One(ResolvingPlayer::Controller)),
                "" => match actor {
                    None | Some(PlayerRefAst::You) => {
                        (None, AffectedPlayers::One(ResolvingPlayer::Controller))
                    }
                    Some(p) => (
                        None,
                        lower_affected_players_in_context(*p, builder, in_trigger)?,
                    ),
                },
                "their" | "their library" | "their libraries" | "his or her" => match actor {
                    Some(p) if *p != PlayerRefAst::You => (
                        None,
                        lower_affected_players_in_context(*p, builder, in_trigger)?,
                    ),
                    _ => (
                        None,
                        AffectedPlayers::One(lower_resolving_player_in_context(
                            PlayerRefAst::ThatPlayer,
                            builder,
                            in_trigger,
                        )?),
                    ),
                },
                "target player" | "target player's" => (
                    Some(BodyTarget::SinglePlayer),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                ),
                "target opponent" | "target opponent's" => (
                    Some(BodyTarget::SingleOpponent),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                ),
                "each opponent" | "each opponent's" => (None, AffectedPlayers::EachOpponent),
                "each player" | "each player's" => (None, AffectedPlayers::EachPlayer),
                "each other player" | "each other player's" => {
                    (None, AffectedPlayers::EachOtherPlayer)
                }
                "that player" | "that player's" => (
                    None,
                    AffectedPlayers::One(lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?),
                ),
                "active player" | "active player's" => {
                    (None, AffectedPlayers::One(ResolvingPlayer::ActivePlayer))
                }
                "defending player" | "defending player's" => {
                    (None, AffectedPlayers::One(ResolvingPlayer::DefendingPlayer))
                }
                "enchanted player" | "enchanted player's"
                    if in_trigger || self_pronoun_valid || builder.has_source =>
                {
                    (None, AffectedPlayers::One(ResolvingPlayer::EnchantedPlayer))
                }
                "its controller" | "its controller's" => (
                    None,
                    AffectedPlayers::One(lower_resolving_player_in_context(
                        PlayerRefAst::ThatObjectController,
                        builder,
                        in_trigger,
                    )?),
                ),
                _ => return Err("oracle:unsupported-exile-top-whose"),
            };
            if let Some(bt) = req_target {
                builder.bind_target(bt)?;
            }
            let cnt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if (in_trigger || self_pronoun_valid)
                && let AffectedPlayers::One(rp) = library_owner
            {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top_linked(rp, cnt, LinkedExileGroup::SourceLinked, *face_down));
            } else if !*face_down
                && let AffectedPlayers::One(rp) = library_owner
                && matches!(&cnt, ValueExpr::Literal(_))
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::exile_library_top(
                        rp, cnt, false,
                    ));
            } else {
                builder.instructions.push(Instruction::ExileTopPlay {
                    count: cnt,
                    library_owner,
                    duration: spellwake_engine::program::ImpulsePlayDuration::ThisTurn,
                    cast_only: false,
                    free: false,
                    any_color: false,
                    face_down: *face_down,
                    grant_play: false,
                });
            }
            Ok(())
        }
        InstructionAst::WheelShuffleHandAndGraveyard {
            player: PlayerRefAst::EachPlayer,
            draw,
        } => {
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_each_hand_and_graveyard());
            let amt = lower_value_expr_in_context(
                resolve_x(draw, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if amt != ValueExpr::literal(0) {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::draw_each_player(amt));
            }
            Ok(())
        }
        InstructionAst::ShuffleIntoLibrary { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            if !in_trigger && !self_pronoun_valid {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_resolving_spell_in_library(LibraryPlacement::BottomRandom,));
            } else {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_ability_source_into_library());
            }
            Ok(())
        }
        InstructionAst::ShuffleIntoLibrary { subject } => match subject.as_ref() {
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                let mut non_self = Vec::new();
                let mut self_items = Vec::new();
                for it in items {
                    if is_self_pronoun(it, builder, self_pronoun_valid) {
                        self_items.push(it);
                    } else {
                        non_self.push(it);
                    }
                }
                for it in non_self.into_iter().chain(self_items) {
                    lower_instruction(
                        &InstructionAst::ShuffleIntoLibrary {
                            subject: Box::new(it.clone()),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_target_into_library());
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if matches!(
                filter.zone,
                Some(
                    ZoneFilter::YourGraveyard
                        | ZoneFilter::OpponentGraveyard
                        | ZoneFilter::AnyGraveyard
                        | ZoneFilter::SingleGraveyard
                )
            ) =>
            {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_target_into_library());
                Ok(())
            }
            SubjectRefAst::TargetObject { filter, count }
                if matches!(
                    filter.zone,
                    Some(
                        ZoneFilter::YourGraveyard
                            | ZoneFilter::OpponentGraveyard
                            | ZoneFilter::AnyGraveyard
                            | ZoneFilter::SingleGraveyard
                    )
                ) && matches!(
                    count,
                    TargetCountAst::Exact(_) | TargetCountAst::UpTo(_) | TargetCountAst::AnyNumber
                ) =>
            {
                let (min_t, max_t) = match *count {
                    TargetCountAst::Exact(n) if n > 0 => (usize::from(n), usize::from(n)),
                    TargetCountAst::UpTo(n) if n > 0 => (0, usize::from(n)),
                    TargetCountAst::AnyNumber => (0, usize::from(u16::MAX)),
                    _ => return Err("oracle:unsupported-shuffle-into-library"),
                };
                let single_subj = SubjectRefAst::TargetObject {
                    filter: filter.clone(),
                    count: TargetCountAst::Exact(1),
                };
                let bt = lower_target_subject(&single_subj, vocabulary)?;
                builder.bind_target(bt)?;
                builder.graveyard_target_bounds = Some((min_t, max_t));
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_target_into_library());
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "your graveyard"
                        | "your hand"
                        | "your hand and graveyard"
                        | "your graveyard and hand"
                        | "their hand and graveyard"
                        | "their graveyard and hand"
                        | "their hands and graveyards"
                        | "all graveyards"
                        | "opponents' graveyards"
                        | "each opponent's graveyard"
                ) =>
            {
                match raw.trim().to_ascii_lowercase().as_str() {
                    "your graveyard" => {
                        builder
                            .instructions
                            .push(emit_shuffle_own_graveyard_into_library());
                    }
                    "your hand" => {
                        builder.instructions.push(
                            super::super::toolbox_emit::shuffle_subject_into_library(
                                EffectSubject::Matching(ObjectFilter::any()),
                                Some(SourceZoneFilter::Hand),
                            ),
                        );
                    }
                    "your hand and graveyard" | "your graveyard and hand" => {
                        builder.instructions.push(
                            super::super::toolbox_emit::shuffle_subject_into_library(
                                EffectSubject::Matching(ObjectFilter::any()),
                                Some(SourceZoneFilter::Hand),
                            ),
                        );
                        builder
                            .instructions
                            .push(emit_shuffle_own_graveyard_into_library());
                    }
                    "their hand and graveyard"
                    | "their graveyard and hand"
                    | "their hands and graveyards" => {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_each_hand_and_graveyard());
                    }
                    "all graveyards" => {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::Matching(ObjectFilter::any()), Some(SourceZoneFilter::AnyGraveyard)));
                    }
                    _ => {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::Matching(ObjectFilter::any()), Some(SourceZoneFilter::OpponentGraveyard)));
                    }
                }
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if last_instruction_created_tokens(&builder.instructions) =>
            {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::CreatedTokens, None));
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target != BodyTarget::None
                    && !matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
            {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::TargetObject, None));
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::LastMoved, None));
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if has_checked_revealed_top_strict(&builder.instructions) =>
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                        MoveSubjectRef::RevealedCard,
                        SubjectDestination::Library(LibraryPlacement::Bottom),
                    ),
                );
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library(),
                );
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::TriggerSubject, None));
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                    && builder.target == BodyTarget::None =>
            {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::TriggerSubject, None));
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        )), None));
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && let Some(group_obj) = builder.last_group_filter =>
            {
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::Matching(group_obj), None));
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                let mut inner = f.as_ref().clone();
                let zone = if (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0 {
                    if !self_pronoun_valid && !in_trigger {
                        return Err("oracle:unsupported-shuffle-into-library");
                    }
                    inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                    Some(SourceZoneFilter::LinkedExile)
                } else {
                    match f.zone {
                        None | Some(ZoneFilter::Battlefield) => None,
                        Some(ZoneFilter::YourGraveyard) => Some(SourceZoneFilter::Graveyard),
                        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
                            Some(SourceZoneFilter::OpponentGraveyard)
                        }
                        Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                            Some(SourceZoneFilter::AnyGraveyard)
                        }
                        Some(ZoneFilter::Exile)
                            if matches!(
                                inner.controller,
                                Some(
                                    ControllerRelation::Opponent
                                        | ControllerRelation::Opponents
                                        | ControllerRelation::OwnerOpponent
                                )
                            ) =>
                        {
                            inner.controller = None;
                            Some(SourceZoneFilter::OpponentExile)
                        }
                        Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
                        Some(ZoneFilter::YourHand) => Some(SourceZoneFilter::Hand),
                        _ => return Err("oracle:unsupported-shuffle-into-library"),
                    }
                };
                inner.zone = None;
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner, vocabulary)?
                };
                bind_filter_player_target(builder, &obj)?;
                if zone == Some(SourceZoneFilter::Graveyard) && obj == ObjectFilter::any() {
                    builder
                        .instructions
                        .push(emit_shuffle_own_graveyard_into_library());
                } else {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_subject_into_library(EffectSubject::Matching(obj), zone));
                }
                Ok(())
            }
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for it in items {
                    lower_instruction(
                        &InstructionAst::ShuffleIntoLibrary {
                            subject: Box::new(it.clone()),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                Ok(())
            }
            _ => Err("oracle:unsupported-shuffle-into-library"),
        },
        InstructionAst::SacrificeUnlessPay { subject, cost } => {
            let sac_ins = InstructionAst::Sacrifice {
                player: PlayerRefAst::You,
                count: None,
                subject: subject.clone(),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            };
            if let CostAtomAst::PayMana(raw) = cost
                && let Some(mana) = parse_fixed_mana_cost(raw)
            {
                let offer_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::OfferDynamicManaPayment {
                        payer: ResolvingPlayer::Controller,
                        fixed: mana,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: 0,
                        declined_destination: offer_idx + 1,
                    });
                lower_instruction(
                    &sac_ins,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::OfferDynamicManaPayment {
                    paid_destination, ..
                }) = builder.instructions.get_mut(offer_idx)
                {
                    *paid_destination = end_dest;
                }
                return Ok(());
            }
            lower_instruction_seq(
                &[
                    InstructionAst::Optional(vec![InstructionAst::PayCost(cost.clone())]),
                    InstructionAst::IfPreviousFailed(vec![sac_ins]),
                ],
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::RevealCards { subject } => match subject.as_ref() {
            SubjectRefAst::Player(p) => {
                if let Some(players) = try_group_affected_players(*p) {
                    push_for_each_player(
                        players,
                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(ResolvingPlayer::LoopPlayer)],
                        builder,
                    );
                    return Ok(());
                }
                let rp = lower_resolving_player_in_context(*p, builder, in_trigger)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(rp),
                );
                Ok(())
            }
            SubjectRefAst::Anaphora(raw) => {
                match raw.trim().to_ascii_lowercase().as_str() {
                    "your hand" => {
                        builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(ResolvingPlayer::Controller));
                        Ok(())
                    }
                    "their hand" | "that player's hand" => {
                        let rp = lower_resolving_player_in_context(
                            PlayerRefAst::ThatPlayer,
                            builder,
                            in_trigger,
                        )?;
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(rp));
                        Ok(())
                    }
                    "target player's hand" => {
                        builder.bind_target(BodyTarget::SinglePlayer)?;
                        builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(ResolvingPlayer::TargetPlayer));
                        Ok(())
                    }
                    "target opponent's hand" => {
                        builder.bind_target(BodyTarget::SingleOpponent)?;
                        builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(ResolvingPlayer::TargetPlayer));
                        Ok(())
                    }
                    _ => Err("oracle:unsupported-reveal-cards"),
                }
            }
            _ => Err("oracle:unsupported-reveal-cards"),
        },
        InstructionAst::ExileOrRevealTopUntil {
            exile,
            library,
            filter,
        } => {
            let lib_low = library.trim().to_ascii_lowercase();
            let lib_key = lib_low
                .strip_suffix(" library")
                .or_else(|| lib_low.strip_suffix(" libraries"))
                .unwrap_or(lib_low.as_str())
                .trim();
            let mut inner_f = filter.as_ref().clone();
            inner_f.zone = None;
            let obj = lower_object_filter(&inner_f, vocabulary)?;
            let group_players = match lib_key {
                "each opponent" | "each opponent's" | "opponents'" | "your opponents'" => {
                    Some(AffectedPlayers::EachOpponent)
                }
                "each player" | "each player's" => Some(AffectedPlayers::EachPlayer),
                "each other player" | "each other player's" => {
                    Some(AffectedPlayers::EachOtherPlayer)
                }
                _ => None,
            };
            if let Some(players) = group_players {
                const UNTIL_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(918);
                let seat_sel = spellwake_engine::toolbox::PlayerSelector::Bound(UNTIL_BIND);
                let dest = if *exile {
                    spellwake_engine::toolbox::ZoneDest::exile()
                } else {
                    spellwake_engine::toolbox::ZoneDest::RevealOnly
                };
                let inner = super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(seat_sel)),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::UntilMatch {
                                filter: Box::new(obj),
                                count: Box::new(ValueExpr::literal(1)),
                                include_non_matches: true,
                            },
                        },
                        dest,
                    ),
                );
                builder.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: UNTIL_BIND,
                    body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
                });
                return Ok(());
            }
            let (req_bt, library_owner) = match lib_key {
                "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" | "library" => {
                    (None, ResolvingPlayer::Controller)
                }
                "target player" | "target player's" | "targetplayer" => (
                    Some(BodyTarget::SinglePlayer),
                    ResolvingPlayer::TargetPlayer,
                ),
                "target opponent" | "target opponent's" | "targetopponent" => (
                    Some(BodyTarget::SingleOpponent),
                    ResolvingPlayer::TargetPlayer,
                ),
                "that player" | "that player's" | "thatplayer" | "their" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?,
                ),
                "its controller" | "its controller's" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ItsController,
                        builder,
                        in_trigger,
                    )?,
                ),
                "its owner" | "its owner's" => (
                    None,
                    lower_resolving_player_in_context(PlayerRefAst::ItsOwner, builder, in_trigger)?,
                ),
                "defending player" | "defending player's" | "defendingplayer" => {
                    (None, ResolvingPlayer::DefendingPlayer)
                }
                "enchanted player" | "enchanted player's" | "enchantedplayer"
                    if in_trigger || self_pronoun_valid || builder.has_source =>
                {
                    (None, ResolvingPlayer::EnchantedPlayer)
                }
                "active player" | "active player's" | "activeplayer" => {
                    (None, ResolvingPlayer::ActivePlayer)
                }
                _ => return Err("oracle:unsupported-reveal-until"),
            };
            if let Some(bt) = req_bt {
                builder.bind_target(bt)?;
            }
            if *exile {
                builder.instructions.push(Instruction::RevealUntilMatchExt {
                    library_owner,
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj),
                    count: ValueExpr::literal(1),
                    match_dest: spellwake_engine::program::LookTopKeepDest::Exile {
                        playable: false,
                        free: false,
                        cast_only: false,
                        any_color: false,
                        duration: ResolvedDuration::NoStatedDuration,
                    },
                    rest_dest: spellwake_engine::program::LookTopRestDest::Exile,
                    keep_all_revealed: false,
                    optional_keep: false,
                    exile_instead_of_reveal: false,
                });
            } else {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        library_owner,
                                    ),
                                )),
                                filter: Box::new(ObjectFilter::any()),
                                slice: spellwake_engine::toolbox::QuerySlice::UntilMatch {
                                    filter: Box::new(obj),
                                    count: Box::new(ValueExpr::literal(1)),
                                    include_non_matches: true,
                                },
                            },
                            spellwake_engine::toolbox::ZoneDest::RevealOnly,
                        ),
                    ));
            }
            Ok(())
        }
        InstructionAst::RevealByPlayer { player, subject } => match subject.as_ref() {
            SubjectRefAst::Anaphora(raw) => {
                let low = raw.trim().to_ascii_lowercase();
                if low == "the top card" {
                    let make_reveal =
                        |library_owner: ResolvingPlayer| Instruction::LookTopChooseExt {
                            library_owner,
                            chooser: ResolvingPlayer::Controller,
                            count: ValueExpr::literal(1),
                            filter: None,
                            max_picks: 0,
                            take_all_matching: false,
                            keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                            rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                            reveal_kept: false,
                            reveal_all: true,
                            reveal_only: true,
                            secondary: None,
                        };
                    if let Some(players) = try_group_affected_players(*player) {
                        push_for_each_player(
                            players,
                            vec![make_reveal(ResolvingPlayer::LoopPlayer)],
                            builder,
                        );
                        return Ok(());
                    }
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    builder.instructions.push(make_reveal(rp));
                    Ok(())
                } else if matches!(low.as_str(), "it" | "that card")
                    && has_checked_revealed_top_strict(&builder.instructions)
                {
                    if try_group_affected_players(*player).is_some() {
                        return Err("oracle:unsupported-reveal-by-player");
                    }
                    let prior_owner = builder.instructions.iter().rev().find_map(|op| match op {
                        Instruction::LookTopChooseExt {
                            count,
                            library_owner,
                            reveal_only: true,
                            ..
                        } => (*count == ValueExpr::literal(1)).then_some(Some(*library_owner)),
                        Instruction::MayRevealLibraryTop => Some(Some(ResolvingPlayer::Controller)),
                        Instruction::ZoneOp(spec)
                            if matches!(
                                spec.destination,
                                spellwake_engine::toolbox::ZoneDest::Exile { .. }
                            ) && matches!(
                                &spec.source,
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Library(_),
                                    ..
                                }
                            ) =>
                        {
                            Some(None)
                        }
                        _ => None,
                    });
                    let Some(Some(library_owner)) = prior_owner else {
                        return Err("oracle:unsupported-reveal-by-player");
                    };
                    let mut trial = builder.clone();
                    let actor = lower_resolving_player_in_context(*player, &mut trial, in_trigger)?;
                    if actor != library_owner && actor != ResolvingPlayer::Controller {
                        return Err("oracle:unsupported-reveal-by-player");
                    }
                    trial.instructions.push(Instruction::LookTopChooseExt {
                        library_owner,
                        chooser: ResolvingPlayer::Controller,
                        count: ValueExpr::literal(1),
                        filter: None,
                        max_picks: 0,
                        take_all_matching: false,
                        keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                        rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                        reveal_kept: false,
                        reveal_all: true,
                        reveal_only: true,
                        secondary: None,
                    });
                    *builder = trial;
                    Ok(())
                } else if matches!(
                    low.as_str(),
                    "your hand" | "their hand" | "that player's hand"
                ) {
                    if let Some(players) = try_group_affected_players(*player) {
                        push_for_each_player(
                            players,
                            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(ResolvingPlayer::LoopPlayer)],
                            builder,
                        );
                        return Ok(());
                    }
                    let effective = if *player == PlayerRefAst::You && low != "your hand" {
                        PlayerRefAst::ThatPlayer
                    } else {
                        *player
                    };
                    let rp = lower_resolving_player_in_context(effective, builder, in_trigger)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::reveal_player_hand(
                            rp,
                        ),
                    );
                    Ok(())
                } else {
                    Err("oracle:unsupported-reveal-by-player")
                }
            }
            _ => Err("oracle:unsupported-reveal-by-player"),
        },
        _ => Err("oracle:unsupported-instruction"),
    }
}

fn has_live_revealed_last_moved_producer(ops: &[Instruction]) -> bool {
    for op in ops.iter().rev() {
        match op {
            Instruction::LookTopChooseExt {
                reveal_only: true, ..
            } => continue,
            Instruction::RevealUntilMatchExt { .. }
            | Instruction::LookTopChooseExt { .. }
            | Instruction::ExileTopPlay { .. } => return true,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                ) && matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(_),
                        ..
                    }
                ) =>
            {
                return true;
            }
            Instruction::ChooseOneLastMoved { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::AdjustLife(_)
            | Instruction::MutatePlayer(_)
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::ChooseValue { .. }
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::AddManaForPlayer { .. } => continue,
            Instruction::ApplyContinuous(spec)
                if matches!(
                    spec.payload,
                    spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                        | spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                ) =>
            {
                continue;
            }
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::RevealOnly
                ) =>
            {
                continue;
            }
            _ => return false,
        }
    }
    false
}

fn is_last_moved_card_anaphora(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "it" | "them"
            | "that card"
            | "those cards"
            | "the card"
            | "the cards"
            | "the exiled card"
            | "the exiled cards"
            | "the revealed card"
            | "the revealed cards"
            | "the chosen card"
            | "the chosen cards"
            | "that creature"
            | "those creatures"
            | "that permanent"
            | "those permanents"
            | "that land"
            | "those lands"
            | "that artifact"
            | "that enchantment"
    )
}

fn try_lower_move_last_moved_filtered(
    f: &ObjectFilterAst,
    destination: SubjectDestination,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<bool, &'static str> {
    let mut inner = f.clone();
    if !strip_last_moved_predicates(&mut inner, &builder.instructions) {
        return Ok(false);
    }
    let filter = if is_unrestricted_filter(&inner) {
        None
    } else {
        let obj = lower_object_filter(&inner, vocabulary)?;
        if obj.has_target_player() || obj.uses_target_permanent() {
            return Err("oracle:unsupported-move-zone");
        }
        Some(Box::new(obj))
    };
    builder.instructions.push(
        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_last_moved_filtered(
            (filter).map(|b| *b),
            destination,
        ),
    );
    Ok(true)
}

pub(super) fn has_live_last_moved_producer(
    ops: &[Instruction],
    expected: LastMovedProducerKind,
) -> bool {
    for op in ops.iter().rev() {
        match op {
            Instruction::Mill { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Mill
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::ExileTopPlay { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any | LastMovedProducerKind::Exile
                );
            }
            Instruction::Discard { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Discard
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::Sacrifice { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Sacrifice
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
                if crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                    .is_some() =>
            {
                let spec =
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                        .unwrap();
                return match &spec.destination {
                    spellwake_engine::toolbox::ZoneDest::Exile { .. } => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Exile
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                    } => matches!(
                        expected,
                        LastMovedProducerKind::Any
                            | LastMovedProducerKind::Sacrifice
                            | LastMovedProducerKind::Graveyard
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. },
                    } => matches!(
                        expected,
                        LastMovedProducerKind::Any
                            | LastMovedProducerKind::Destroy
                            | LastMovedProducerKind::Graveyard
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                    } => matches!(
                        expected,
                        LastMovedProducerKind::Any
                            | LastMovedProducerKind::Discard
                            | LastMovedProducerKind::Graveyard
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Mill,
                    } => matches!(
                        expected,
                        LastMovedProducerKind::Any
                            | LastMovedProducerKind::Mill
                            | LastMovedProducerKind::Graveyard
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard { .. } => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Graveyard
                    ),
                    spellwake_engine::toolbox::ZoneDest::RevealOnly
                    | spellwake_engine::toolbox::ZoneDest::None => continue,
                    _ => matches!(expected, LastMovedProducerKind::Any),
                };
            }

            Instruction::LookTopChooseExt {
                reveal_only: true, ..
            } => continue,
            Instruction::LookTopChooseExt { keep_dest, .. }
            | Instruction::RevealUntilMatchExt {
                match_dest: keep_dest,
                ..
            } => {
                return match keep_dest {
                    spellwake_engine::program::LookTopKeepDest::Exile { .. }
                    | spellwake_engine::program::LookTopKeepDest::ExileLinked { .. } => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Exile
                    ),
                    spellwake_engine::program::LookTopKeepDest::Graveyard => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Graveyard
                    ),
                    _ => matches!(expected, LastMovedProducerKind::Any),
                };
            }
            Instruction::ChooseObjects { .. } => {
                return matches!(expected, LastMovedProducerKind::Any);
            }

            Instruction::SearchLibrary { .. }
            | Instruction::ManifestTop { .. }
            | Instruction::ManifestDread => {
                return matches!(expected, LastMovedProducerKind::Any);
            }
            Instruction::ChooseOneLastMoved { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::AdjustLife(_)
            | Instruction::MutatePlayer(_)
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::ChooseValue { .. }
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::AddManaForPlayer { .. } => continue,
            Instruction::ApplyContinuous(spec)
                if matches!(
                    spec.payload,
                    spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                        | spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                ) =>
            {
                continue;
            }
            _ => return false,
        }
    }
    false
}

pub(super) fn strip_last_moved_predicates(
    filter: &mut ObjectFilterAst,
    ops: &[Instruction],
) -> bool {
    if matches!(
        filter.zone,
        Some(
            ZoneFilter::OpponentHand
                | ZoneFilter::OpponentsHands
                | ZoneFilter::TargetOpponentHand
                | ZoneFilter::TargetPlayerHand
        )
    ) {
        return false;
    }
    #[derive(Clone, Copy)]
    enum MatchKind {
        Producer(LastMovedProducerKind),
        RevealedMoved,
    }
    let classify =
        |pred: &spellwake_oracle_compiler::ast::FilterPredicateAst| -> Option<MatchKind> {
            use spellwake_oracle_compiler::ast::{FilterPredicateAst, PriorActionRefKind};
            match pred {
                FilterPredicateAst::PriorActionZoneRef { kind } => {
                    match kind.trim().to_ascii_lowercase().as_str() {
                        "from_among_those"
                        | "from_among_those_cards"
                        | "among_them"
                        | "among them"
                        | "from_among_them"
                        | "from among them"
                        | "among_those"
                        | "among those"
                        | "among_those_cards"
                        | "among those cards" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Any))
                        }
                        "milled_this_way"
                        | "among_the_cards_milled_this_way"
                        | "among the cards milled this way"
                        | "among_cards_milled_this_way"
                        | "among cards milled this way"
                        | "cards_milled_this_way"
                        | "from_among_the_cards_milled_this_way"
                        | "from among the cards milled this way"
                        | "from_among_cards_milled_this_way"
                        | "from among cards milled this way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Mill))
                        }
                        "exiled_this_way"
                        | "among_the_cards_exiled_this_way"
                        | "among the cards exiled this way"
                        | "among_cards_exiled_this_way"
                        | "among cards exiled this way"
                        | "cards_exiled_this_way"
                        | "from_among_the_cards_exiled_this_way"
                        | "from among the cards exiled this way"
                        | "from_among_cards_exiled_this_way"
                        | "from among cards exiled this way"
                        | "from_among_those_cards_exiled_this_way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Exile))
                        }
                        "discarded_this_way"
                        | "among_the_cards_discarded_this_way"
                        | "among_cards_discarded_this_way"
                        | "from_among_the_cards_discarded_this_way"
                        | "from_among_cards_discarded_this_way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Discard))
                        }
                        "put_into_graveyard_this_way"
                        | "put_into_a_graveyard_this_way"
                        | "put_into_your_graveyard_this_way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Graveyard))
                        }
                        "revealed_this_way"
                        | "among_the_cards_revealed_this_way"
                        | "among_cards_revealed_this_way"
                        | "from_among_the_cards_revealed_this_way"
                        | "from_among_cards_revealed_this_way" => Some(MatchKind::RevealedMoved),
                        _ => None,
                    }
                }
                FilterPredicateAst::AffectedByPriorAction(kind) => match kind {
                    PriorActionRefKind::PutIntoGraveyardThisWay => {
                        Some(MatchKind::Producer(LastMovedProducerKind::Graveyard))
                    }
                    PriorActionRefKind::ExiledThisWay => {
                        Some(MatchKind::Producer(LastMovedProducerKind::Exile))
                    }
                    PriorActionRefKind::DiscardedThisWay => {
                        Some(MatchKind::Producer(LastMovedProducerKind::Discard))
                    }
                    PriorActionRefKind::SacrificedThisWay => {
                        Some(MatchKind::Producer(LastMovedProducerKind::Sacrifice))
                    }
                    PriorActionRefKind::DestroyedThisWay => {
                        Some(MatchKind::Producer(LastMovedProducerKind::Destroy))
                    }
                    PriorActionRefKind::RevealedThisWay => Some(MatchKind::RevealedMoved),
                    _ => None,
                },
                FilterPredicateAst::StateQualification(raw) => {
                    match raw.trim().to_ascii_lowercase().as_str() {
                        "milled this way" | "they milled this way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Mill))
                        }
                        "exiled this way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Exile))
                        }
                        "discarded this way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Discard))
                        }
                        "put into a graveyard this way" | "put into your graveyard this way" => {
                            Some(MatchKind::Producer(LastMovedProducerKind::Graveyard))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        };
    let mut matched_any = false;
    for pred in &filter.predicates {
        if let Some(kind) = classify(pred) {
            let live = match kind {
                MatchKind::Producer(pk) => has_live_last_moved_producer(ops, pk),
                MatchKind::RevealedMoved => has_live_revealed_last_moved_producer(ops),
            };
            if !live {
                return false;
            }
            matched_any = true;
        }
    }
    if !matched_any {
        return false;
    }
    filter.predicates.retain(|pred| classify(pred).is_none());
    filter.zone = None;
    true
}

pub(super) fn resolve_select_zone(
    filter: &mut ObjectFilterAst,
    default_zone: SourceZoneFilter,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<(ResolvingPlayer, SourceZoneFilter), &'static str> {
    if (filter.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
        && matches!(filter.zone, None | Some(ZoneFilter::Exile))
    {
        filter.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
        filter.zone = None;
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::LinkedExile));
    }
    if strip_last_moved_predicates(filter, &builder.instructions) {
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::LastMoved));
    }
    let zone = filter.zone.take();
    match zone {
        None => Ok((ResolvingPlayer::Controller, default_zone)),
        Some(ZoneFilter::Battlefield) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::Battlefield))
        }
        Some(ZoneFilter::YourHand) => Ok((ResolvingPlayer::Controller, SourceZoneFilter::Hand)),
        Some(ZoneFilter::YourHandOrGraveyard) => Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::HandOrGraveyard,
        )),
        Some(ZoneFilter::ThatPlayerHand) => Ok((
            lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?,
            SourceZoneFilter::Hand,
        )),
        Some(ZoneFilter::TargetPlayerHand) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok((ResolvingPlayer::TargetPlayer, SourceZoneFilter::Hand))
        }
        Some(ZoneFilter::TargetOpponentHand) => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok((ResolvingPlayer::TargetPlayer, SourceZoneFilter::Hand))
        }
        Some(ZoneFilter::YourGraveyard) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::Graveyard))
        }
        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::OpponentGraveyard,
        )),
        Some(
            ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
        ) => Ok((ResolvingPlayer::Controller, SourceZoneFilter::AnyGraveyard)),
        Some(ZoneFilter::ItsOwnerGraveyard) => Ok((
            lower_resolving_player_in_context(PlayerRefAst::ItsOwner, builder, in_trigger)?,
            SourceZoneFilter::Graveyard,
        )),
        Some(ZoneFilter::ThatPlayerGraveyard) => Ok((
            lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?,
            SourceZoneFilter::Graveyard,
        )),
        Some(ZoneFilter::TargetPlayerGraveyard) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok((ResolvingPlayer::TargetPlayer, SourceZoneFilter::Graveyard))
        }
        Some(ZoneFilter::TargetOpponentGraveyard) => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok((ResolvingPlayer::TargetPlayer, SourceZoneFilter::Graveyard))
        }
        Some(ZoneFilter::DefendingPlayerGraveyard) => Ok((
            ResolvingPlayer::DefendingPlayer,
            SourceZoneFilter::Graveyard,
        )),
        Some(ZoneFilter::YourLibrary) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::YourLibrary))
        }
        Some(ZoneFilter::Exile)
            if matches!(
                filter.controller,
                Some(
                    ControllerRelation::Opponent
                        | ControllerRelation::Opponents
                        | ControllerRelation::OwnerOpponent
                )
            ) =>
        {
            filter.controller = None;
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::OpponentExile))
        }
        Some(ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::OpponentHand))
        }
        Some(ZoneFilter::CommandZone) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::CommandZone))
        }
        Some(ZoneFilter::Exile) => Ok((ResolvingPlayer::Controller, SourceZoneFilter::Exile)),
        _ => Err("oracle:unsupported-move-zone"),
    }
}

pub(super) fn has_checked_revealed_top(ops: &[Instruction]) -> bool {
    ops.iter().rev().any(|op| {
        matches!(
            op,
            Instruction::LookTopChooseExt {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
                | Instruction::RevealUntilMatchExt { .. }
        ) || matches!(
            op,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                ) && matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(_),
                        ..
                    }
                )
        )
    })
}

/// True only when a prior instruction satisfies the engine's `definitely_checked_top`
/// invariant required to validate `MoveSubjectRef::RevealedCard`.
pub(super) fn has_checked_revealed_top_strict(ops: &[Instruction]) -> bool {
    ops.iter().rev().any(|op| {
        matches!(
            op,
            Instruction::LookTopChooseExt {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
        ) || matches!(
            op,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                ) && matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(_),
                        ..
                    }
                )
        )
    })
}

pub(super) fn parse_small_number_word(raw: &str) -> Option<u16> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "zero" => Some(0),
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        "twenty" => Some(20),
        other => other.parse::<u16>().ok(),
    }
}

pub(super) fn parse_vote_options(raw: &str) -> Result<Vec<String>, &'static str> {
    let cleaned = raw.trim().trim_end_matches('.');
    let mut parts: Vec<String> = Vec::new();
    for seg in cleaned.split(" or ") {
        for sub in seg.split(',') {
            let t = sub.trim().trim_start_matches("or ").trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
    }
    if parts.len() < 2 || parts.len() > 16 {
        return Err("oracle:unsupported-vote-options");
    }
    Ok(parts)
}

pub(super) fn lower_move_subject_ref(
    subject: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<MoveSubjectRef, &'static str> {
    if is_self_pronoun(subject, builder, self_pronoun_valid) {
        return Ok(MoveSubjectRef::AbilitySource);
    }
    match subject {
        SubjectRefAst::SelfObject => Ok(MoveSubjectRef::AbilitySource),
        SubjectRefAst::AttachedHost(_) => Ok(MoveSubjectRef::AttachedHost),
        SubjectRefAst::TriggerSubject if in_trigger => Ok(MoveSubjectRef::TriggeringObject),
        SubjectRefAst::TargetObject { .. } => {
            let bt = lower_target_subject(subject, vocabulary)?;
            builder.bind_target(bt)?;
            Ok(MoveSubjectRef::TargetObject)
        }
        SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
            Ok(MoveSubjectRef::TargetObject)
        }
        SubjectRefAst::Anaphora(_) if in_trigger => Ok(MoveSubjectRef::TriggeringObject),
        _ => Err("oracle:unsupported-move-subject"),
    }
}

pub(super) fn parse_return_with_counter_local(
    with_counter: Option<&str>,
) -> Result<Option<(CounterKind, u8)>, &'static str> {
    let Some(raw) = with_counter else {
        return Ok(None);
    };
    let low_raw = raw.trim().to_ascii_lowercase();
    let (qty, after_qty): (u8, &str) = if let Some(rest) = low_raw
        .strip_prefix("a ")
        .or_else(|| low_raw.strip_prefix("an "))
        .or_else(|| low_raw.strip_prefix("one "))
    {
        (1, rest.trim())
    } else if let Some(rest) = low_raw.strip_prefix("two ") {
        (2, rest.trim())
    } else if let Some(rest) = low_raw.strip_prefix("three ") {
        (3, rest.trim())
    } else if let Some(rest) = low_raw.strip_prefix("four ") {
        (4, rest.trim())
    } else if let Some(rest) = low_raw.strip_prefix("five ") {
        (5, rest.trim())
    } else if let Some((tok, rest)) = low_raw.split_once(' ')
        && let Some(n) = match tok {
            "six" => Some(6u8),
            "seven" => Some(7),
            "eight" => Some(8),
            "nine" => Some(9),
            "ten" => Some(10),
            _ => tok.parse::<u8>().ok().filter(|&v| v > 0),
        }
    {
        (n, rest.trim())
    } else {
        (1, low_raw.as_str())
    };
    let mut kind_str = after_qty;
    while let Some(rest) = kind_str
        .strip_prefix("additional ")
        .or_else(|| kind_str.strip_prefix("extra "))
    {
        kind_str = rest.trim_start();
    }
    kind_str = kind_str
        .strip_suffix(" counters")
        .or_else(|| kind_str.strip_suffix(" counter"))
        .unwrap_or(kind_str)
        .trim();
    let ck = parse_counter_kind(kind_str).ok_or("oracle:unsupported-counter-kind")?;
    Ok(Some((ck, qty)))
}

fn can_upgrade_destroy_no_regen(op: &Instruction) -> bool {
    match op {
        Instruction::ZoneOp(spec)
            if matches!(
                spec.destination,
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. }
                }
            ) =>
        {
            true
        }
        Instruction::FixedTargetClause(body) => body
            .instructions
            .iter()
            .rev()
            .any(can_upgrade_destroy_no_regen),
        Instruction::ForEachPlayerQuery { body, .. } | Instruction::RepeatLoop { body, .. } => body
            .instructions
            .iter()
            .rev()
            .any(can_upgrade_destroy_no_regen),
        Instruction::ChooseOption { options, .. } => {
            !options.is_empty()
                && options.iter().any(|branch| {
                    branch
                        .body
                        .instructions
                        .iter()
                        .rev()
                        .any(can_upgrade_destroy_no_regen)
                })
        }
        _ => false,
    }
}

fn try_upgrade_destroy_no_regen(op: &mut Instruction) -> bool {
    match op {
        Instruction::ZoneOp(spec)
            if matches!(
                spec.destination,
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. }
                }
            ) =>
        {
            if let spellwake_engine::toolbox::ZoneDest::Graveyard {
                semantics:
                    spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate },
            } = &mut spec.destination
            {
                *allow_regenerate = false;
            }
            true
        }
        Instruction::FixedTargetClause(body) => body
            .instructions
            .iter_mut()
            .rev()
            .any(try_upgrade_destroy_no_regen),
        Instruction::ForEachPlayerQuery { body, .. } | Instruction::RepeatLoop { body, .. } => body
            .instructions
            .iter_mut()
            .rev()
            .any(try_upgrade_destroy_no_regen),
        Instruction::ChooseOption { options, .. } => {
            if options.is_empty()
                || !options.iter().any(|branch| {
                    branch
                        .body
                        .instructions
                        .iter()
                        .rev()
                        .any(can_upgrade_destroy_no_regen)
                })
            {
                return false;
            }
            let mut upgraded = false;
            for branch in options {
                if branch
                    .body
                    .instructions
                    .iter_mut()
                    .rev()
                    .any(try_upgrade_destroy_no_regen)
                {
                    upgraded = true;
                }
            }
            upgraded
        }
        _ => false,
    }
}

fn is_damage_instruction_for_no_regen(op: &Instruction) -> bool {
    matches!(
        op,
        Instruction::DealDamage(_)
            | Instruction::AffectPermanentTargets {
                action: PermanentTargetAction::Damage(_),
                ..
            }
            | Instruction::DivideDamageExprAmongTargets { .. }
            | Instruction::CreatureDealsPowerDamageToOther { .. }
    )
}

fn find_damage_instruction_for_no_regen(op: &Instruction) -> Option<&Instruction> {
    if is_damage_instruction_for_no_regen(op) {
        return Some(op);
    }
    match op {
        Instruction::FixedTargetClause(body) => body
            .instructions
            .iter()
            .rev()
            .find_map(find_damage_instruction_for_no_regen),
        Instruction::ForEachPlayerQuery { body, .. } | Instruction::RepeatLoop { body, .. } => body
            .instructions
            .iter()
            .rev()
            .find_map(find_damage_instruction_for_no_regen),
        _ => None,
    }
}

fn damage_no_regen_object_query(
    op: &Instruction,
    target: BodyTarget,
) -> Option<spellwake_engine::toolbox::ObjectQuery> {
    use spellwake_engine::program::{DamageGroup, DamageRecipientsRef};
    use spellwake_engine::toolbox::{DamageTargetQuery, ObjectQuery, QueryZone};
    match op {
        Instruction::DealDamage(spec) => match &spec.recipients {
            DamageTargetQuery::Targets => {
                if is_permanent_body_target(target) {
                    Some(ObjectQuery::Targets)
                } else if matches!(
                    target,
                    BodyTarget::SingleAnyTarget
                        | BodyTarget::SingleCreatureOrPlayer
                        | BodyTarget::SinglePlayerOrPlaneswalker
                        | BodyTarget::SingleOpponentOrPlaneswalker
                ) {
                    Some(ObjectQuery::filtered(
                        ObjectQuery::Targets,
                        ObjectFilter::creature(),
                    ))
                } else {
                    None
                }
            }
            DamageTargetQuery::Objects(q) | DamageTargetQuery::Both { objects: q, .. } => {
                Some(q.clone())
            }
            DamageTargetQuery::Relative(r) => match r.as_ref() {
                DamageRecipientsRef::Target => {
                    if is_permanent_body_target(target) {
                        Some(ObjectQuery::Targets)
                    } else if matches!(
                        target,
                        BodyTarget::SingleAnyTarget
                            | BodyTarget::SingleCreatureOrPlayer
                            | BodyTarget::SinglePlayerOrPlaneswalker
                            | BodyTarget::SingleOpponentOrPlaneswalker
                    ) {
                        Some(ObjectQuery::filtered(
                            ObjectQuery::Targets,
                            ObjectFilter::creature(),
                        ))
                    } else {
                        None
                    }
                }
                DamageRecipientsRef::AbilitySource => Some(ObjectQuery::Source),
                DamageRecipientsRef::TriggeringObject => Some(ObjectQuery::TriggerSubject),
                DamageRecipientsRef::AttachedHost => Some(ObjectQuery::AttachedHost),
                DamageRecipientsRef::LastMoved => Some(ObjectQuery::LastMoved),
                DamageRecipientsRef::CreatedTokens => Some(ObjectQuery::CreatedTokens),
                DamageRecipientsRef::OtherCreaturesAndEachPlayer => Some(ObjectQuery::zone(
                    QueryZone::Battlefield,
                    ObjectFilter::Clause(ObjectClause::creature().other_than_ability_source()),
                )),
                DamageRecipientsRef::Group(g) => match g {
                    DamageGroup::Filtered(f)
                    | DamageGroup::FilteredAndPlayers { filter: f, .. } => {
                        Some(ObjectQuery::zone(QueryZone::Battlefield, *f))
                    }
                    DamageGroup::Creatures
                    | DamageGroup::CreaturesAndPlayers
                    | DamageGroup::CreaturesAndPlaneswalkers => Some(ObjectQuery::zone(
                        QueryZone::Battlefield,
                        ObjectFilter::creature(),
                    )),
                    DamageGroup::CreaturesWithoutFlying => Some(ObjectQuery::zone(
                        QueryZone::Battlefield,
                        ObjectFilter::Clause(ObjectClause {
                            any_types: CardTypeMask::CREATURE,
                            forbidden_keyword: Some(KeywordAbility::Flying),
                            ..ObjectClause::any()
                        }),
                    )),
                    DamageGroup::CreaturesOfSubtype(id) => Some(ObjectQuery::zone(
                        QueryZone::Battlefield,
                        ObjectFilter::Clause(ObjectClause {
                            any_types: CardTypeMask::CREATURE,
                            subtypes: SubtypeFilter::Has(*id),
                            ..ObjectClause::any()
                        }),
                    )),
                    DamageGroup::CreaturesExcept(id) => Some(ObjectQuery::zone(
                        QueryZone::Battlefield,
                        ObjectFilter::Clause(ObjectClause {
                            any_types: CardTypeMask::CREATURE,
                            subtypes: SubtypeFilter::Lacks(*id),
                            ..ObjectClause::any()
                        }),
                    )),
                    DamageGroup::ControllerAndTargetCreature => Some(ObjectQuery::Targets),
                    DamageGroup::AbilitySource => Some(ObjectQuery::Source),
                    _ => None,
                },
                _ => None,
            },
            DamageTargetQuery::Players(_) => None,
        },
        Instruction::AffectPermanentTargets {
            action: PermanentTargetAction::Damage(_),
            ..
        }
        | Instruction::DivideDamageExprAmongTargets { .. }
        | Instruction::CreatureDealsPowerDamageToOther { .. } => {
            if is_permanent_body_target(target) {
                Some(ObjectQuery::Targets)
            } else {
                Some(ObjectQuery::filtered(
                    ObjectQuery::Targets,
                    ObjectFilter::creature(),
                ))
            }
        }
        _ => None,
    }
}

fn extract_prior_resolving_player_for_shuffle(
    ops: &[Instruction],
    in_trigger: bool,
) -> Option<ResolvingPlayer> {
    let valid = |p: ResolvingPlayer| {
        p != ResolvingPlayer::Controller
            && (p != ResolvingPlayer::UniqueRelatedPlayer || in_trigger)
    };
    let from_sel = |sel: &spellwake_engine::toolbox::PlayerSelector| match sel {
        spellwake_engine::toolbox::PlayerSelector::Resolving(p)
        | spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::One(p))
            if valid(*p) =>
        {
            Some(*p)
        }
        _ => None,
    };
    ops.iter().rev().find_map(|op| match op {
        Instruction::ForEachPlayerQuery { selector, body, .. } => from_sel(selector)
            .or_else(|| extract_prior_resolving_player_for_shuffle(&body.instructions, in_trigger)),
        Instruction::FixedTargetClause(body) => {
            let from_bt = match body.target {
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                    Some(ResolvingPlayer::TargetPlayer)
                }
                BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::FilteredPermanent(_)
                | BodyTarget::SingleAnyTarget
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SinglePlayerOrPlaneswalker
                | BodyTarget::SingleOpponentOrPlaneswalker => {
                    Some(ResolvingPlayer::TargetController)
                }
                BodyTarget::SingleSpell(_)
                | BodyTarget::SingleStackObject(_)
                | BodyTarget::SingleStackEntry
                | BodyTarget::ControlledAbility { .. }
                | BodyTarget::StackEntryTargetingControlledPermanent => {
                    Some(ResolvingPlayer::TargetSpellController)
                }
                BodyTarget::FilteredAnyGraveyardCard { .. }
                | BodyTarget::AnyGraveyardCard
                | BodyTarget::AnyGraveyardCreature
                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker => {
                    Some(ResolvingPlayer::TargetOwner)
                }
                _ => None,
            };
            from_bt.filter(|p| valid(*p)).or_else(|| {
                extract_prior_resolving_player_for_shuffle(&body.instructions, in_trigger)
            })
        }
        Instruction::ZoneOp(spec) => extract_zone_op_resolving_player(&spec.source)
            .filter(|p| valid(*p))
            .or_else(|| spec.shuffle_library.as_ref().and_then(from_sel))
            .or_else(|| match &spec.selection {
                spellwake_engine::toolbox::ZoneSelection::Prompt { chooser, .. }
                | spellwake_engine::toolbox::ZoneSelection::Random { chooser, .. } => {
                    from_sel(chooser)
                }
                _ => None,
            }),
        Instruction::DrawCards(spec) => from_sel(&spec.players),
        Instruction::AdjustLife(spec) => from_sel(&spec.players),
        Instruction::OptionalEffect { chooser: p }
        | Instruction::OptionalSequence { chooser: p, .. }
        | Instruction::OfferDynamicManaPayment { payer: p, .. }
        | Instruction::RevealUntilMatchExt {
            library_owner: p, ..
        }
        | Instruction::Mill { player: p, .. }
        | Instruction::Scry { player: p, .. }
        | Instruction::Surveil { player: p, .. }
        | Instruction::LookTopChooseExt {
            library_owner: p, ..
        }
        | Instruction::ExileTopPlay {
            library_owner: AffectedPlayers::One(p),
            ..
        }
        | Instruction::Sacrifice {
            players: AffectedPlayers::One(p),
            ..
        }
        | Instruction::Discard {
            players: AffectedPlayers::One(p),
            ..
        } if valid(*p) => Some(*p),
        _ => None,
    })
}

fn is_linked_exile_anaphora(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "the exiled card"
            | "the exiled cards"
            | "that exiled card"
            | "those exiled cards"
            | "each exiled card"
            | "all exiled cards"
            | "all of the exiled cards"
            | "each of the exiled cards"
            | "the card exiled with ~"
            | "the cards exiled with ~"
            | "cards exiled with ~"
            | "all cards exiled with ~"
            | "each card exiled with ~"
    )
}

fn is_singular_linked_exile_anaphora(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "the exiled card" | "that exiled card" | "the card exiled with ~"
    )
}

fn lower_linked_exile_group_move(
    f: &ObjectFilterAst,
    destination: SubjectDestination,
    strip_owner_that_player: bool,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    err_tag: &'static str,
) -> Result<(), &'static str> {
    let mut inner = f.clone();
    inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
    inner.zone = None;
    if strip_owner_that_player {
        if matches!(
            inner.controller,
            Some(ControllerRelation::OwnerThatPlayer | ControllerRelation::ThatPlayer)
        ) {
            inner.controller = None;
        }
        if matches!(
            inner.owner,
            Some(ControllerRelation::OwnerThatPlayer | ControllerRelation::ThatPlayer)
        ) {
            inner.owner = None;
        }
    }
    if is_unrestricted_filter(&inner) {
        builder.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_linked_exile(
                destination,
                false,
            ),
        );
        return Ok(());
    }
    let obj = lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?;
    if obj.has_target_player() || obj.uses_target_permanent() {
        return Err(err_tag);
    }
    let linked_obj = obj.with_flags(ObjectFilterFlags::EXILED_WITH_SELF);
    let count_expr = ValueExpr::Count {
        zone: ValueZone::Exile(None),
        filter: Box::new(linked_obj),
        controller_scope: None,
    };
    builder.instructions.push(
        crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(
            ResolvingPlayer::Controller,
            SourceZoneFilter::LinkedExile,
            linked_obj,
            count_expr.clone(),
            count_expr,
            destination,
            false,
        ),
    );
    Ok(())
}

fn is_moved_object_instruction(op: &Instruction) -> bool {
    matches!(
        op,
        Instruction::ZoneOp(_)
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::DrawCards(_)
            | Instruction::ChooseOneLastMoved { .. }
            | Instruction::ManifestTop { .. }
            | Instruction::ChooseObjects { .. }
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::ReturnExiledTargetCohort { .. }
            | Instruction::ReturnCardsDestroyedThisWay
            | Instruction::RevealUntilMatchExt { .. }
            | Instruction::LookTopChooseExt { .. }
            | Instruction::RevealTopTwoPiles { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::Mill { .. }
            | Instruction::Discard { .. }
            | Instruction::ManifestDread
            | Instruction::Incubate { .. }
            | Instruction::SearchLibrary { .. }
    )
}

fn extract_zone_op_resolving_player(
    query: &spellwake_engine::toolbox::ObjectQuery,
) -> Option<ResolvingPlayer> {
    use spellwake_engine::toolbox::{ObjectQuery, PlayerSelector, QueryZone};
    match query {
        ObjectQuery::ZoneScan {
            zone:
                QueryZone::Hand(ps)
                | QueryZone::Library(ps)
                | QueryZone::Graveyard(ps)
                | QueryZone::ExileOwnedBy(ps)
                | QueryZone::BattlefieldControlledBy(ps),
            ..
        } => match ps.as_ref() {
            PlayerSelector::Resolving(p) | PlayerSelector::Group(AffectedPlayers::One(p)) => {
                Some(*p)
            }
            _ => None,
        },
        ObjectQuery::Union(parts) => parts.iter().find_map(extract_zone_op_resolving_player),
        ObjectQuery::Filtered { source, .. }
        | ObjectQuery::Take { source, .. }
        | ObjectQuery::Difference(source, _)
        | ObjectQuery::Intersect(source, _) => extract_zone_op_resolving_player(source),
        _ => None,
    }
}

fn extract_zone_op_group_players(
    query: &spellwake_engine::toolbox::ObjectQuery,
) -> Option<AffectedPlayers> {
    use spellwake_engine::toolbox::{ObjectQuery, PlayerSelector, QueryZone};
    match query {
        ObjectQuery::ZoneScan {
            zone:
                QueryZone::Hand(ps)
                | QueryZone::Library(ps)
                | QueryZone::Graveyard(ps)
                | QueryZone::ExileOwnedBy(ps)
                | QueryZone::BattlefieldControlledBy(ps),
            ..
        } => match ps.as_ref() {
            PlayerSelector::Group(players) if !matches!(players, AffectedPlayers::One(_)) => {
                Some(*players)
            }
            _ => None,
        },
        ObjectQuery::Union(parts) => parts.iter().find_map(extract_zone_op_group_players),
        ObjectQuery::Filtered { source, .. }
        | ObjectQuery::Take { source, .. }
        | ObjectQuery::Difference(source, _)
        | ObjectQuery::Intersect(source, _) => extract_zone_op_group_players(source),
        _ => None,
    }
}

fn parse_last_moved_subset_count(raw: &str) -> Option<i64> {
    let low = raw.trim().to_ascii_lowercase();
    match low.as_str() {
        "one of them" | "one of those cards" | "a card from among them" | "one of the cards" => {
            Some(1)
        }
        "two of them" | "two of those cards" | "two of the cards" => Some(2),
        "three of them" | "three of those cards" | "three of the cards" => Some(3),
        "four of them" | "four of those cards" => Some(4),
        _ => None,
    }
}

fn strip_move_zone_last_moved_filter(
    f: &ObjectFilterAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    use spellwake_oracle_compiler::ast::FilterPredicateAst;
    let has_prior_ref = f.predicates.iter().any(|p| {
        matches!(
            p,
            FilterPredicateAst::PriorActionZoneRef { .. }
                | FilterPredicateAst::AffectedByPriorAction(_)
        )
    });
    if !has_prior_ref {
        return None;
    }
    let mut cleaned = f.clone();
    cleaned.predicates.retain(|p| {
        !matches!(
            p,
            FilterPredicateAst::PriorActionZoneRef { .. }
                | FilterPredicateAst::AffectedByPriorAction(_)
        )
    });
    cleaned.zone = None;
    if is_unrestricted_filter(&cleaned) {
        Some(ObjectFilter::any())
    } else {
        lower_object_filter_in_context(&cleaned, builder, in_trigger, vocabulary).ok()
    }
}

pub(super) fn last_instruction_moved_object(ops: &[Instruction]) -> bool {
    ops.last().is_some_and(is_moved_object_instruction)
        || matches!(
            ops.last(),
            Some(Instruction::ApplyContinuous(spec))
                if matches!(
                    spec.payload,
                    spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { .. }
                        | spellwake_engine::toolbox::ContinuousPayload::CombatAction(_)
                        | spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(_)
                ) && (matches!(
                    spec.selector.as_ref(),
                    Some(spellwake_engine::toolbox::ObjectQuery::LastMoved)
                ) || matches!(
                    spec.selector.as_ref(),
                    Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(s))
                        if matches!(s.as_ref(), EffectSubject::LastMoved)
                ))
        )
        || matches!(
            ops.last(),
            Some(Instruction::MutateObject(spec))
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::ObjectMutation::AddCounters { .. }
                        | spellwake_engine::toolbox::ObjectMutation::RemoveCounters { .. }
                        | spellwake_engine::toolbox::ObjectMutation::SetTapped(_)
                        | spellwake_engine::toolbox::ObjectMutation::SkipNextUntap
                ) && (matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::LastMoved
                ) || matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(s)
                        if matches!(s.as_ref(), EffectSubject::LastMoved)
                ))
        )
}

pub(super) fn has_prior_moved_object(ops: &[Instruction]) -> bool {
    for op in ops.iter().rev() {
        if is_moved_object_instruction(op) {
            return true;
        }
        match op {
            Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::AdjustLife(_)
            | Instruction::MutatePlayer(_)
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::ChooseValue { .. }
            | Instruction::AddManaForPlayer { .. } => continue,
            Instruction::ApplyContinuous(spec)
                if matches!(
                    spec.payload,
                    spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                        | spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                        | spellwake_engine::toolbox::ContinuousPayload::DynamicBoost { .. }
                        | spellwake_engine::toolbox::ContinuousPayload::CombatAction(_)
                        | spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(_)
                ) =>
            {
                continue;
            }
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::ObjectMutation::AddCounters { .. }
                        | spellwake_engine::toolbox::ObjectMutation::RemoveCounters { .. }
                        | spellwake_engine::toolbox::ObjectMutation::SetTapped(_)
                        | spellwake_engine::toolbox::ObjectMutation::SkipNextUntap
                ) =>
            {
                continue;
            }
            _ => return false,
        }
    }
    false
}

pub(super) fn parse_lib_placement(
    dest: &str,
    allow_your_library: bool,
) -> Option<LibraryPlacement> {
    if dest.contains("after_shuffle") {
        return None;
    }
    let is_random = dest.contains("random");
    if dest == "owner_choice_top_or_bottom_of_library"
        || dest.contains("their choice of the top or bottom")
        || dest.contains("owner's choice of the top or bottom")
        || dest.contains("that player's choice of the top or bottom")
    {
        return if is_random {
            None
        } else {
            Some(LibraryPlacement::ChoiceTopOrBottom {
                chooser: ResolvingPlayer::TargetOwner,
            })
        };
    }
    if dest.contains("your choice of the top or bottom")
        || dest == "the top or bottom of your library"
        || dest == "top or bottom of your library"
    {
        return if is_random {
            None
        } else {
            Some(LibraryPlacement::ChoiceTopOrBottom {
                chooser: ResolvingPlayer::Controller,
            })
        };
    }
    if dest.contains("choice") || dest.contains("top or bottom") || dest.contains("bottom or top") {
        return None;
    }
    match dest {
        "top_of_library" => return Some(LibraryPlacement::Top),
        "bottom_of_library" => return Some(LibraryPlacement::Bottom),
        "bottom_of_library_random"
        | "the bottom in a random order"
        | "bottom in a random order" => {
            return Some(LibraryPlacement::BottomRandom);
        }
        _ => {}
    }
    if !dest.contains("library") && !dest.contains("libraries") {
        return None;
    }
    let owner_ok = dest.contains("owner's library")
        || dest.contains("owners' libraries")
        || dest.contains("their library")
        || dest.contains("their libraries")
        || dest.contains("that player's library")
        || dest.contains("an opponent's library")
        || dest.contains("target player's library")
        || dest.contains("target opponent's library")
        || dest.contains("of library")
        || dest.contains("of the library")
        || dest.contains("that library")
        || (allow_your_library && dest.contains("your library"));
    if !owner_ok {
        return None;
    }
    if is_random {
        return if dest.contains("bottom of") || dest.contains("the bottom") {
            Some(LibraryPlacement::BottomRandom)
        } else {
            None
        };
    }
    if dest.contains("second from the top") {
        Some(LibraryPlacement::FromTop(1))
    } else if dest.contains("third from the top") {
        Some(LibraryPlacement::FromTop(2))
    } else if dest.contains("fourth from the top") {
        Some(LibraryPlacement::FromTop(3))
    } else if dest.contains("fifth from the top") {
        Some(LibraryPlacement::FromTop(4))
    } else if dest.contains("sixth from the top") {
        Some(LibraryPlacement::FromTop(5))
    } else if dest.contains("seventh from the top") {
        Some(LibraryPlacement::FromTop(6))
    } else if dest.contains("eighth from the top") {
        Some(LibraryPlacement::FromTop(7))
    } else if dest.contains("ninth from the top") {
        Some(LibraryPlacement::FromTop(8))
    } else if dest.contains("tenth from the top") {
        Some(LibraryPlacement::FromTop(9))
    } else if dest.contains("top of") {
        Some(LibraryPlacement::Top)
    } else if dest.contains("bottom of") {
        Some(LibraryPlacement::Bottom)
    } else {
        None
    }
}

pub(super) fn is_single_card_subject(subject: &SubjectRefAst) -> bool {
    matches!(
        subject,
        SubjectRefAst::SelfObject
            | SubjectRefAst::TriggerSubject
            | SubjectRefAst::AttachedHost(_)
            | SubjectRefAst::TargetObject {
                count: TargetCountAst::Exact(1) | TargetCountAst::UpTo(1),
                ..
            }
    ) || matches!(
        subject,
        SubjectRefAst::Anaphora(a) if matches!(
            a.trim().to_ascii_lowercase().as_str(),
            "it" | "that card"
                | "that creature"
                | "that permanent"
                | "the card"
                | "that artifact"
                | "that enchantment"
                | "that land"
                | "that planeswalker"
                | "that spell"
                | "the exiled card"
                | "the revealed card"
        )
    )
}

pub(super) fn parse_lib_placement_for_subject(
    dest: &str,
    allow_your_library: bool,
    subject: &SubjectRefAst,
) -> Option<LibraryPlacement> {
    parse_lib_placement(dest, allow_your_library).or_else(|| {
        if is_single_card_subject(subject) && dest.contains("random") {
            let stripped = dest
                .replace(", in a random order", "")
                .replace(" in a random order", "")
                .replace(" in random order", "");
            parse_lib_placement(stripped.trim(), allow_your_library)
                .filter(|pl| *pl == LibraryPlacement::Bottom)
        } else {
            None
        }
    })
}

pub(super) fn is_valid_put_target_in_library_target(bt: &BodyTarget) -> bool {
    matches!(
        bt,
        BodyTarget::SingleCreature
            | BodyTarget::SinglePermanent
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::SingleAnyTarget
            | BodyTarget::SingleSpell(_)
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SingleOwnGraveyardCard
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
            | BodyTarget::FilteredOwnGraveyardCard(_)
            | BodyTarget::FilteredAnyGraveyardCard { .. }
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BattlefieldDestFlags {
    pub(super) tapped: bool,
    pub(super) attacking: bool,
    pub(super) transformed: bool,
    pub(super) under_owner_control: bool,
    pub(super) attach_to_self: bool,
}

pub(super) fn parse_dynamic_lib_from_top_offset(
    dest: &str,
    allow_your_library: bool,
    bound_x: Option<&ValueExprAst>,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<ValueExpr>, &'static str> {
    if dest.contains("after_shuffle") || dest.contains("random") {
        return Ok(None);
    }
    let owner_ok = dest.contains("owner's library")
        || dest.contains("owners' libraries")
        || dest.contains("their library")
        || dest.contains("that player's library")
        || (allow_your_library && dest.contains("your library"));
    if !owner_ok {
        return Ok(None);
    }
    if let Some((_, after)) = dest.split_once("just beneath the top ") {
        let count_raw = after
            .split_once(" card")
            .map_or(after.trim(), |(c, _)| c.trim());
        if count_raw == "x" {
            let expr = match bound_x {
                Some(e) => lower_value_expr_in_context(e, builder, in_trigger, vocabulary)?,
                None => ValueExpr::chosen_x(),
            };
            return Ok(Some(expr));
        }
        if let Some(n) = parse_small_number_word(count_raw) {
            return Ok(Some(ValueExpr::literal(i64::from(n))));
        }
        return Err("oracle:unsupported-move-zone");
    }
    if let Some(before_from) = dest.strip_suffix(" from the top") {
        let pos_tok = before_from.split_whitespace().last().unwrap_or("");
        if matches!(pos_tok, "xth" | "x") {
            let expr = match bound_x {
                Some(e) => lower_value_expr_in_context(e, builder, in_trigger, vocabulary)?,
                None => ValueExpr::chosen_x(),
            };
            return Ok(Some(ValueExpr::Sub(
                Box::new(expr),
                Box::new(ValueExpr::literal(1)),
            )));
        }
        if let Some(num_s) = pos_tok
            .strip_suffix("th")
            .or_else(|| pos_tok.strip_suffix("st"))
            .or_else(|| pos_tok.strip_suffix("nd"))
            .or_else(|| pos_tok.strip_suffix("rd"))
            && let Ok(n) = num_s.parse::<u16>()
            && n >= 1
        {
            return Ok(Some(ValueExpr::literal(i64::from(n - 1))));
        }
    }
    Ok(None)
}

pub(super) fn is_player_keyword(kw: &KeywordAbility) -> bool {
    matches!(
        kw,
        KeywordAbility::Shroud
            | KeywordAbility::Hexproof
            | KeywordAbility::HexproofFrom(_)
            | KeywordAbility::ProtectionFromWhite
            | KeywordAbility::ProtectionFromBlue
            | KeywordAbility::ProtectionFromBlack
            | KeywordAbility::ProtectionFromRed
            | KeywordAbility::ProtectionFromGreen
            | KeywordAbility::ProtectionFromMulticolored
            | KeywordAbility::ProtectionFromSubtype(_)
            | KeywordAbility::ProtectionFrom(_)
    )
}

pub(super) fn strip_trailing_temp_duration(
    low: &str,
    dur: ResolvedDuration,
) -> Option<(String, ResolvedDuration)> {
    let mut body = low.trim().trim_end_matches('.').trim().to_owned();
    let mut eff_dur = dur;
    for (suffix, implied) in [
        (
            " until your next turn",
            ResolvedDuration::UntilControllerNextTurn,
        ),
        (" until the end of turn", ResolvedDuration::UntilEndOfTurn),
        (" until end of turn", ResolvedDuration::UntilEndOfTurn),
        (
            " until the end of combat",
            ResolvedDuration::UntilEndOfCombat,
        ),
        (" until end of combat", ResolvedDuration::UntilEndOfCombat),
        (" this combat", ResolvedDuration::UntilEndOfCombat),
        (" this turn", ResolvedDuration::UntilEndOfTurn),
    ] {
        if let Some(stripped) = body.strip_suffix(suffix) {
            body = stripped.trim().to_owned();
            if eff_dur == ResolvedDuration::NoStatedDuration
                || (eff_dur == ResolvedDuration::UntilEndOfTurn
                    && implied != ResolvedDuration::UntilEndOfTurn)
            {
                eff_dur = implied;
            }
            break;
        }
    }
    (!body.is_empty()).then_some((body, eff_dur))
}

pub(super) fn parse_player_cant_activate_rule(rule_body: &str) -> Option<bool> {
    match rule_body {
        "cant_activate_abilities"
        | "cantactivateabilities"
        | "can't activate abilities"
        | "cannot activate abilities" => Some(false),
        "cant_activate_nonmana_abilities"
        | "cantactivatenonmanaabilities"
        | "can't activate abilities that aren't mana abilities"
        | "can't activate abilities unless they're mana abilities"
        | "cannot activate abilities that aren't mana abilities"
        | "cannot activate abilities unless they're mana abilities" => Some(true),
        _ => None,
    }
}

pub(super) fn resolve_return_select_zone(
    inner: &mut ObjectFilterAst,
    from_zone: Option<&str>,
    default_zone: SourceZoneFilter,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<(ResolvingPlayer, SourceZoneFilter), &'static str> {
    if (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
        && matches!(inner.zone, None | Some(ZoneFilter::Exile))
    {
        inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
        inner.zone = None;
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::LinkedExile));
    }
    if strip_last_moved_predicates(inner, &builder.instructions) {
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::LastMoved));
    }
    if inner.zone.is_some() {
        return resolve_select_zone(inner, default_zone, builder, in_trigger);
    }
    let Some(fz) = from_zone else {
        return Ok((ResolvingPlayer::Controller, default_zone));
    };
    let fl = fz.trim().to_ascii_lowercase();
    if matches!(
        fl.as_str(),
        "among them"
            | "from among them"
            | "them"
            | "among those cards"
            | "from among those cards"
            | "those cards"
            | "those"
    ) && has_live_last_moved_producer(&builder.instructions, LastMovedProducerKind::Any)
    {
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::LastMoved));
    }
    if fl.contains("command zone") {
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::CommandZone));
    }
    if fl.contains("target opponent's graveyard") {
        builder.bind_target(BodyTarget::SingleOpponent)?;
        return Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::TargetPlayerGraveyard,
        ));
    }
    if fl.contains("target player's graveyard") {
        builder.bind_target(BodyTarget::SinglePlayer)?;
        return Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::TargetPlayerGraveyard,
        ));
    }
    if fl.contains("target opponent's hand") {
        builder.bind_target(BodyTarget::SingleOpponent)?;
        return Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::TargetPlayerHand,
        ));
    }
    if fl.contains("target player's hand") {
        builder.bind_target(BodyTarget::SinglePlayer)?;
        return Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::TargetPlayerHand,
        ));
    }
    if fl.contains("opponent's hand") || fl.contains("opponents' hands") {
        return Ok((ResolvingPlayer::Controller, SourceZoneFilter::OpponentHand));
    }
    if fl.contains("that player's graveyard") || fl.contains("their graveyard") {
        let rp = lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
        Ok((rp, SourceZoneFilter::Graveyard))
    } else if fl.contains("opponent's graveyard") || fl.contains("opponents' graveyards") {
        Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::OpponentGraveyard,
        ))
    } else if fl.contains("all graveyards") || fl.contains("any graveyard") {
        Ok((ResolvingPlayer::Controller, SourceZoneFilter::AnyGraveyard))
    } else if fl.contains("hand or graveyard") || fl.contains("hand and/or graveyard") {
        Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::HandOrGraveyard,
        ))
    } else if fl.contains("graveyard") {
        Ok((ResolvingPlayer::Controller, SourceZoneFilter::Graveyard))
    } else if fl.contains("exile") {
        Ok((ResolvingPlayer::Controller, SourceZoneFilter::Exile))
    } else if fl.contains("that player's hand") || fl.contains("their hand") {
        let rp = lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
        Ok((rp, SourceZoneFilter::Hand))
    } else if fl.contains("hand") {
        Ok((ResolvingPlayer::Controller, SourceZoneFilter::Hand))
    } else if fl.contains("battlefield") {
        Ok((ResolvingPlayer::Controller, SourceZoneFilter::Battlefield))
    } else {
        Err("oracle:unsupported-return-zone")
    }
}

pub(super) fn fold_except_into_filter(
    base: &mut ObjectFilterAst,
    except: &SubjectRefAst,
) -> Result<(), &'static str> {
    match except {
        SubjectRefAst::SelfObject => {
            base.flags = base.flags.union(AstFilterFlags::EXCLUDE_SELF);
            Ok(())
        }
        s if is_self_anaphora_ref(s) => {
            base.flags = base.flags.union(AstFilterFlags::EXCLUDE_SELF);
            Ok(())
        }
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            for item in items {
                fold_except_into_filter(base, item)?;
            }
            Ok(())
        }
        SubjectRefAst::Group(ef) | SubjectRefAst::All(ef) | SubjectRefAst::Each(ef) => {
            if !ef.forbidden_types.is_empty()
                || !ef.forbidden_supertypes.is_empty()
                || !ef.forbidden_subtypes.is_empty()
                || !ef.forbidden_colors.is_empty()
                || ef.color_arity.is_some()
                || ef.controller.is_some()
                || ef.zone.is_some()
                || ef.combat.is_some()
                || ef.tapped.is_some()
                || !ef.required_keywords.is_empty()
                || !ef.forbidden_keywords.is_empty()
                || !ef.stat_bounds.is_empty()
                || ef.counter_kind.is_some()
                || ef.named.is_some()
                || !ef.flags.is_empty()
                || !ef.predicates.is_empty()
                || ef.any_permanent
                || ef.any_card
                || ef.any_spell
                || ef.is_ability
            {
                return Err("oracle:unsupported-except-filter");
            }
            let types_match_base = ef.any_types.is_empty()
                || (!base.any_types.is_empty() && ef.any_types == base.any_types);
            if !ef.subtypes.is_empty()
                && types_match_base
                && ef.any_colors.is_empty()
                && ef.required_supertypes.is_empty()
                && ef.token.is_none()
            {
                base.forbidden_subtypes.extend(ef.subtypes.iter().cloned());
                return Ok(());
            }
            if !ef.any_types.is_empty()
                && ef.subtypes.is_empty()
                && ef.any_colors.is_empty()
                && ef.required_supertypes.is_empty()
                && ef.token.is_none()
            {
                base.forbidden_types = base.forbidden_types.union(ef.any_types);
                return Ok(());
            }
            if !ef.any_colors.is_empty()
                && types_match_base
                && ef.subtypes.is_empty()
                && ef.required_supertypes.is_empty()
                && ef.token.is_none()
            {
                base.forbidden_colors = base.forbidden_colors.union(ef.any_colors);
                return Ok(());
            }
            if !ef.required_supertypes.is_empty()
                && types_match_base
                && ef.subtypes.is_empty()
                && ef.any_colors.is_empty()
                && ef.token.is_none()
            {
                base.forbidden_supertypes = base.forbidden_supertypes.union(ef.required_supertypes);
                return Ok(());
            }
            if let Some(tok_flag) = ef.token
                && types_match_base
                && ef.subtypes.is_empty()
                && ef.any_colors.is_empty()
                && ef.required_supertypes.is_empty()
                && base.token.is_none()
            {
                base.token = Some(!tok_flag);
                return Ok(());
            }
            Err("oracle:unsupported-except-filter")
        }
        _ => Err("oracle:unsupported-except-filter"),
    }
}

pub(super) fn apply_except_to_subject(
    subject: &SubjectRefAst,
    except: &SubjectRefAst,
) -> Result<SubjectRefAst, &'static str> {
    match subject {
        SubjectRefAst::All(f) => {
            let mut inner = f.as_ref().clone();
            fold_except_into_filter(&mut inner, except)?;
            Ok(SubjectRefAst::All(Box::new(inner)))
        }
        SubjectRefAst::Each(f) => {
            let mut inner = f.as_ref().clone();
            fold_except_into_filter(&mut inner, except)?;
            Ok(SubjectRefAst::Each(Box::new(inner)))
        }
        SubjectRefAst::Group(f) => {
            let mut inner = f.as_ref().clone();
            fold_except_into_filter(&mut inner, except)?;
            Ok(SubjectRefAst::Group(Box::new(inner)))
        }
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(apply_except_to_subject(it, except)?);
            }
            Ok(SubjectRefAst::Compound(out))
        }
        _ => Err("oracle:unsupported-except-filter"),
    }
}

// Mirrors the full InstructionAst::SearchLibrary fields plus lower_instruction context.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_search_library_instruction<'a>(
    library: &str,
    count: &'a ValueExprAst,
    filter: &ObjectFilterAst,
    reveal: bool,
    to_zone: &str,
    up_to: bool,
    any_number: bool,
    extra_zones: &'a [ZoneFilter],
    secondary_filter: Option<&'a ObjectFilterAst>,
    secondary_is_or: bool,
    where_x: Option<&'a ValueExprAst>,
    rest_to_zone: Option<&'a str>,
    modifier: Option<&'a spellwake_oracle_compiler::ast::SearchModifierAst>,
    follow_up: &[InstructionAst],
    optional_search: bool,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&'a ValueExprAst>,
) -> Result<(), &'static str> {
    let mut eff_reveal = reveal;
    let mut eff_to_zone = to_zone.to_owned();
    let mut post_entry: Option<spellwake_engine::program::SearchBattlefieldPostEntry> = None;
    let mut trailing: Vec<&InstructionAst> = Vec::new();

    if to_zone == "follow_up" {
        if follow_up.is_empty() {
            return Err("oracle:unsupported-search-follow-up");
        }
        let mut idx = 0usize;
        if let Some(InstructionAst::RevealCards { subject }) = follow_up.get(idx)
            && subject_is_anaphora(subject)
        {
            eff_reveal = true;
            idx += 1;
        }
        match follow_up.get(idx) {
            Some(InstructionAst::MoveZone { subject, to }) if subject_is_anaphora(subject) => {
                eff_to_zone = to.clone();
                idx += 1;
            }
            Some(InstructionAst::ReturnZone {
                subject,
                from_zone: None,
                to,
                tapped,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            }) if subject_is_anaphora(subject) => {
                let to_low = to.to_ascii_lowercase();
                eff_to_zone = if *tapped && !to_low.contains("tapped") {
                    format!("{to} tapped")
                } else {
                    to.clone()
                };
                idx += 1;
            }
            Some(InstructionAst::Exile {
                actor: None | Some(PlayerRefAst::You),
                subject,
                random: false,
                face_down: false,
                into_pile: false,
            }) if subject_is_anaphora(subject) => {
                eff_to_zone = "exile".to_owned();
                idx += 1;
            }
            _ => return Err("oracle:unsupported-search-follow-up"),
        }
        let remaining: Vec<&InstructionAst> = follow_up[idx..]
            .iter()
            .filter(|ins| {
                !matches!(
                    ins,
                    InstructionAst::ShuffleLibrary {
                        player: PlayerRefAst::You | PlayerRefAst::ThatPlayer
                    }
                ) && !matches!(
                    ins,
                    InstructionAst::IfCondition {
                        condition,
                        then_body,
                    } if matches!(
                        condition.as_ref(),
                        ConditionAst::PriorActionOccurred {
                            action,
                            subject_filter: None,
                            min_count: 1,
                        } if matches!(
                            action.trim().to_ascii_lowercase().as_str(),
                            "search"
                                | "searched"
                                | "search_library"
                                | "searched_library"
                                | "searched_your_library"
                                | "searched_a_library"
                        )
                    ) && matches!(
                        then_body.as_slice(),
                        [InstructionAst::ShuffleLibrary {
                            player: PlayerRefAst::You | PlayerRefAst::ThatPlayer
                        }]
                    )
                )
            })
            .collect();
        let is_anaphora_haste = |ins: &InstructionAst| -> bool {
            matches!(
                ins,
                InstructionAst::GrantKeywords {
                    subject,
                    keywords,
                    ..
                } if subject_is_anaphora(subject)
                    && keywords.len() == 1
                    && keywords[0].trim().eq_ignore_ascii_case("haste")
            )
        };
        let is_end_step_return_hand = |ins: &InstructionAst| -> bool {
            let InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                body,
                ..
            } = ins
            else {
                return false;
            };
            match body.as_slice() {
                [InstructionAst::MoveZone { subject, to }] if subject_is_anaphora(subject) => {
                    is_hand_return_dest(&to.trim().to_ascii_lowercase())
                }
                [
                    InstructionAst::ReturnZone {
                        subject,
                        to,
                        tapped: false,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        random: false,
                        except: None,
                        count: None,
                        up_to: false,
                        any_number: false,
                        ..
                    },
                ] if subject_is_anaphora(subject) => {
                    is_hand_return_dest(&to.trim().to_ascii_lowercase())
                }
                _ => false,
            }
        };
        let dest_is_bf = eff_to_zone.to_ascii_lowercase().contains("battlefield");
        if dest_is_bf
            && remaining.len() == 2
            && ((is_anaphora_haste(remaining[0]) && is_end_step_return_hand(remaining[1]))
                || (is_end_step_return_hand(remaining[0]) && is_anaphora_haste(remaining[1])))
        {
            if !(in_trigger || self_pronoun_valid) {
                return Err("oracle:unsupported-search-post-entry");
            }
            post_entry = Some(
                spellwake_engine::program::SearchBattlefieldPostEntry::HasteAndReturnAtEndStep,
            );
        } else {
            for ins in remaining {
                if instruction_references_anaphora(ins) {
                    return Err("oracle:unsupported-search-follow-up");
                }
                trailing.push(ins);
            }
        }
    } else if !follow_up.is_empty() {
        return Err("oracle:unsupported-search-follow-up");
    }

    let mut effective_filter_owned: Option<ObjectFilterAst> = None;
    if let Some(spellwake_oracle_compiler::ast::NameFilterAst::SameAs(ref_subj)) = &filter.named
        && let SubjectRefAst::TargetObject {
            filter: tf,
            count: TargetCountAst::Exact(1),
        } = ref_subj.as_ref()
        && tf.zone.is_none()
    {
        let bt = lower_battlefield_body_target(tf, vocabulary)?;
        builder.bind_target(bt)?;
        let mut cloned = filter.clone();
        cloned.named = Some(spellwake_oracle_compiler::ast::NameFilterAst::SameAsReferenced);
        effective_filter_owned = Some(cloned);
    }
    let resolved_where_x = where_x.map(|e| resolve_x(e, bound_x));
    let mut search = lower_library_search_ext(
        library,
        resolve_x(count, bound_x),
        effective_filter_owned.as_ref().unwrap_or(filter),
        eff_reveal,
        eff_to_zone.as_str(),
        vocabulary,
        optional_search,
        up_to,
        any_number,
        &SearchLibraryExtra {
            extra_zones,
            secondary_filter,
            secondary_is_or,
            where_x: resolved_where_x,
            rest_to_zone,
            modifier,
        },
    )?;
    if let spellwake_engine::program::SearchCardMatcher::SameNameAs { reference, .. } =
        &mut search.matcher
        && *reference == MoveSubjectRef::TriggeringObject
    {
        if matches!(
            builder.target,
            BodyTarget::SingleCreature
                | BodyTarget::SinglePermanent
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::FilteredPermanent(_)
        ) {
            *reference = MoveSubjectRef::TargetObject;
        } else if !in_trigger {
            return Err("oracle:unsupported-search-named");
        }
    }
    if let Some(pe) = post_entry {
        if let SearchDestination::Battlefield {
            post_entry: ref mut slot,
            ..
        } = search.destination
        {
            *slot = pe;
        } else {
            return Err("oracle:unsupported-search-follow-up");
        }
    }
    bind_search_target(builder, &search)?;
    if search.searcher == ResolvingPlayer::UniqueRelatedPlayer {
        search.searcher =
            lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
    }
    let start = builder.instructions.len();
    builder.instructions.push(Instruction::SearchLibrary {
        filter: Box::new(search),
    });
    for ins in trailing {
        lower_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
    }
    if optional_search {
        let emitted = builder.instructions.len() - start;
        if emitted == 1 {
            prepend_instruction_at(
                &mut builder.instructions,
                start,
                Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                },
            );
        } else {
            prepend_instruction_at(
                &mut builder.instructions,
                start,
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: emitted,
                },
            );
        }
    }
    Ok(())
}

pub(super) fn resolve_graveyard_group_players(
    filter_zone: Option<ZoneFilter>,
    from_zone: Option<&str>,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<Option<AffectedPlayers>, &'static str> {
    match filter_zone {
        Some(ZoneFilter::YourGraveyard) => {
            Ok(Some(AffectedPlayers::One(ResolvingPlayer::Controller)))
        }
        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
            Ok(Some(AffectedPlayers::EachOpponent))
        }
        Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
            Ok(Some(AffectedPlayers::EachPlayer))
        }
        Some(ZoneFilter::ThatPlayerGraveyard) => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok(Some(AffectedPlayers::One(rp)))
        }
        Some(ZoneFilter::TargetPlayerGraveyard) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(Some(AffectedPlayers::One(ResolvingPlayer::TargetPlayer)))
        }
        Some(ZoneFilter::TargetOpponentGraveyard) => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(Some(AffectedPlayers::One(ResolvingPlayer::TargetPlayer)))
        }
        Some(ZoneFilter::DefendingPlayerGraveyard) => {
            Ok(Some(AffectedPlayers::One(ResolvingPlayer::DefendingPlayer)))
        }
        Some(_) => Ok(None),
        None => {
            let Some(raw) = from_zone else {
                return Ok(None);
            };
            let fz = raw.trim().to_ascii_lowercase();
            if !fz.contains("graveyard") {
                return Ok(None);
            }
            if fz.contains("target opponent's graveyard") {
                builder.bind_target(BodyTarget::SingleOpponent)?;
                Ok(Some(AffectedPlayers::One(ResolvingPlayer::TargetPlayer)))
            } else if fz.contains("target player's graveyard") {
                builder.bind_target(BodyTarget::SinglePlayer)?;
                Ok(Some(AffectedPlayers::One(ResolvingPlayer::TargetPlayer)))
            } else if fz.contains("that player's graveyard") || fz.contains("their graveyard") {
                let rp = lower_resolving_player_in_context(
                    PlayerRefAst::ThatPlayer,
                    builder,
                    in_trigger,
                )?;
                Ok(Some(AffectedPlayers::One(rp)))
            } else if fz.contains("defending player's graveyard") {
                Ok(Some(AffectedPlayers::One(ResolvingPlayer::DefendingPlayer)))
            } else if fz.contains("opponent's graveyard") || fz.contains("opponents' graveyards") {
                Ok(Some(AffectedPlayers::EachOpponent))
            } else if fz.contains("your graveyard") {
                Ok(Some(AffectedPlayers::One(ResolvingPlayer::Controller)))
            } else if fz.contains("all graveyards")
                || fz.contains("any graveyard")
                || fz.contains("each graveyard")
                || fz.contains("graveyards")
            {
                Ok(Some(AffectedPlayers::EachPlayer))
            } else {
                Ok(None)
            }
        }
    }
}

fn emit_destroy_target(no_regeneration: bool) -> Instruction {
    super::super::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(
        spellwake_engine::toolbox::ObjectQuery::Targets,
        spellwake_engine::toolbox::ZoneDest::Graveyard {
            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                allow_regenerate: !no_regeneration,
            },
        },
    ))
}

fn emit_shuffle_own_graveyard_into_library() -> Instruction {
    let mut spec = spellwake_engine::toolbox::ZoneOpSpec::move_all(
        spellwake_engine::toolbox::ObjectQuery::zone(
            spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
            )),
            ObjectFilter::any(),
        ),
        spellwake_engine::toolbox::ZoneDest::Library {
            position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto,
        },
    );
    spec.shuffle_library = Some(spellwake_engine::toolbox::PlayerSelector::Resolving(
        ResolvingPlayer::Controller,
    ));
    super::super::toolbox_emit::zone_op(spec)
}

fn zone_filter_to_object_query(
    zone: ZoneFilter,
    obj: ObjectFilter,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<spellwake_engine::toolbox::ObjectQuery, &'static str> {
    use spellwake_engine::toolbox::{ObjectQuery, PlayerSelector, QueryZone};
    let ctrl = || Box::new(PlayerSelector::Resolving(ResolvingPlayer::Controller));
    let each = || Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer));
    let opps = || Box::new(PlayerSelector::Group(AffectedPlayers::EachOpponent));
    match zone {
        ZoneFilter::Battlefield => Ok(ObjectQuery::zone(QueryZone::Battlefield, obj)),
        ZoneFilter::Stack => Ok(ObjectQuery::zone(QueryZone::Stack, obj)),
        ZoneFilter::Exile => Ok(ObjectQuery::zone(QueryZone::Exile, obj)),
        ZoneFilter::CommandZone => Ok(ObjectQuery::zone(QueryZone::Command, obj)),
        ZoneFilter::YourGraveyard => Ok(ObjectQuery::zone(QueryZone::Graveyard(ctrl()), obj)),
        ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards => {
            Ok(ObjectQuery::zone(QueryZone::Graveyard(opps()), obj))
        }
        ZoneFilter::SingleGraveyard | ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards => {
            Ok(ObjectQuery::zone(QueryZone::Graveyard(each()), obj))
        }
        ZoneFilter::TargetPlayerGraveyard => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                obj,
            ))
        }
        ZoneFilter::TargetOpponentGraveyard => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                obj,
            ))
        }
        ZoneFilter::DefendingPlayerGraveyard => Ok(ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::DefendingPlayer,
            ))),
            obj,
        )),
        ZoneFilter::ThatPlayerGraveyard => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok(ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(rp))),
                obj,
            ))
        }
        ZoneFilter::ItsOwnerGraveyard => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ItsOwner, builder, in_trigger)?;
            Ok(ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(rp))),
                obj,
            ))
        }
        ZoneFilter::YourHand => Ok(ObjectQuery::zone(QueryZone::Hand(ctrl()), obj)),
        ZoneFilter::ThatPlayerHand => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok(ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Resolving(rp))),
                obj,
            ))
        }
        ZoneFilter::TargetPlayerHand => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                obj,
            ))
        }
        ZoneFilter::TargetOpponentHand => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                obj,
            ))
        }
        ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands => {
            Ok(ObjectQuery::zone(QueryZone::Hand(opps()), obj))
        }
        ZoneFilter::EachPlayerHand | ZoneFilter::AnyHand => {
            Ok(ObjectQuery::zone(QueryZone::Hand(each()), obj))
        }
        ZoneFilter::YourLibrary => Ok(ObjectQuery::zone(QueryZone::Library(ctrl()), obj)),
        ZoneFilter::TargetPlayerLibrary => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(ObjectQuery::zone(
                QueryZone::Library(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                obj,
            ))
        }
        ZoneFilter::TargetOpponentLibrary => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(ObjectQuery::zone(
                QueryZone::Library(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                obj,
            ))
        }
        ZoneFilter::ThatPlayerLibrary => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok(ObjectQuery::zone(
                QueryZone::Library(Box::new(PlayerSelector::Resolving(rp))),
                obj,
            ))
        }
        ZoneFilter::OpponentLibrary => Ok(ObjectQuery::zone(QueryZone::Library(opps()), obj)),
        ZoneFilter::EachPlayerLibrary | ZoneFilter::AnyLibrary => {
            Ok(ObjectQuery::zone(QueryZone::Library(each()), obj))
        }
        ZoneFilter::YourGraveyardOrExile => Ok(ObjectQuery::Union(vec![
            ObjectQuery::zone(QueryZone::Graveyard(ctrl()), obj),
            ObjectQuery::zone(QueryZone::ExileOwnedBy(ctrl()), obj),
        ])),
        ZoneFilter::AnyGraveyardOrExile => Ok(ObjectQuery::Union(vec![
            ObjectQuery::zone(QueryZone::Graveyard(each()), obj),
            ObjectQuery::zone(QueryZone::Exile, obj),
        ])),
        ZoneFilter::YourHandOrGraveyard => Ok(ObjectQuery::Union(vec![
            ObjectQuery::zone(QueryZone::Hand(ctrl()), obj),
            ObjectQuery::zone(QueryZone::Graveyard(ctrl()), obj),
        ])),
        ZoneFilter::YourLibraryOrGraveyard => Ok(ObjectQuery::Union(vec![
            ObjectQuery::zone(QueryZone::Library(ctrl()), obj),
            ObjectQuery::zone(QueryZone::Graveyard(ctrl()), obj),
        ])),
        ZoneFilter::YourGraveyardHandOrLibrary => Ok(ObjectQuery::Union(vec![
            ObjectQuery::zone(QueryZone::Graveyard(ctrl()), obj),
            ObjectQuery::zone(QueryZone::Hand(ctrl()), obj),
            ObjectQuery::zone(QueryZone::Library(ctrl()), obj),
        ])),
        ZoneFilter::OutsideTheGame => Err("oracle:unsupported-move-zone"),
    }
}

fn is_hidden_zone_filter(zone: ZoneFilter) -> bool {
    matches!(
        zone,
        ZoneFilter::YourHand
            | ZoneFilter::ThatPlayerHand
            | ZoneFilter::TargetPlayerHand
            | ZoneFilter::TargetOpponentHand
            | ZoneFilter::OpponentHand
            | ZoneFilter::OpponentsHands
            | ZoneFilter::EachPlayerHand
            | ZoneFilter::AnyHand
            | ZoneFilter::YourLibrary
            | ZoneFilter::TargetPlayerLibrary
            | ZoneFilter::TargetOpponentLibrary
            | ZoneFilter::ThatPlayerLibrary
            | ZoneFilter::OpponentLibrary
            | ZoneFilter::EachPlayerLibrary
            | ZoneFilter::AnyLibrary
            | ZoneFilter::YourHandOrGraveyard
            | ZoneFilter::YourLibraryOrGraveyard
            | ZoneFilter::YourGraveyardHandOrLibrary
    )
}

fn parse_whole_zone_anaphora_query(
    raw: &str,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<Option<spellwake_engine::toolbox::ObjectQuery>, &'static str> {
    use spellwake_engine::toolbox::{ObjectQuery, PlayerSelector, QueryZone};
    let low = raw.trim().to_ascii_lowercase();
    let (zone_kind, players) = match low.as_str() {
        "your hand" => (0u8, AffectedPlayers::One(ResolvingPlayer::Controller)),
        "target player's hand" => (
            0,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::TargetPlayer,
                builder,
                in_trigger,
            )?),
        ),
        "target opponent's hand" => (
            0,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::TargetOpponent,
                builder,
                in_trigger,
            )?),
        ),
        "that player's hand" | "their hand" => (
            0,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                builder,
                in_trigger,
            )?),
        ),
        "each opponent's hand" | "your opponents' hands" | "opponents' hands" => {
            (0, AffectedPlayers::EachOpponent)
        }
        "each player's hand" | "all hands" => (0, AffectedPlayers::EachPlayer),
        "your graveyard" => (1, AffectedPlayers::One(ResolvingPlayer::Controller)),
        "target player's graveyard" => (
            1,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::TargetPlayer,
                builder,
                in_trigger,
            )?),
        ),
        "target opponent's graveyard" => (
            1,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::TargetOpponent,
                builder,
                in_trigger,
            )?),
        ),
        "that player's graveyard" | "their graveyard" => (
            1,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                builder,
                in_trigger,
            )?),
        ),
        "each opponent's graveyard" | "your opponents' graveyards" | "opponents' graveyards" => {
            (1, AffectedPlayers::EachOpponent)
        }
        "each player's graveyard" | "all graveyards" => (1, AffectedPlayers::EachPlayer),
        "your library" => (2, AffectedPlayers::One(ResolvingPlayer::Controller)),
        "target player's library" => (
            2,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::TargetPlayer,
                builder,
                in_trigger,
            )?),
        ),
        "target opponent's library" => (
            2,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::TargetOpponent,
                builder,
                in_trigger,
            )?),
        ),
        "that player's library" | "their library" => (
            2,
            AffectedPlayers::One(lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                builder,
                in_trigger,
            )?),
        ),
        "each opponent's library" | "your opponents' libraries" | "opponents' libraries" => {
            (2, AffectedPlayers::EachOpponent)
        }
        "each player's library" | "all libraries" => (2, AffectedPlayers::EachPlayer),
        _ => return Ok(None),
    };
    let ps = Box::new(PlayerSelector::Group(players));
    let qz = match zone_kind {
        0 => QueryZone::Hand(ps),
        1 => QueryZone::Graveyard(ps),
        _ => QueryZone::Library(ps),
    };
    Ok(Some(ObjectQuery::zone(qz, ObjectFilter::any())))
}

#[allow(clippy::too_many_arguments)]
fn lower_exile_random_or_face_down(
    actor: Option<PlayerRefAst>,
    subject: &SubjectRefAst,
    random: bool,
    face_down: bool,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{
        BindingId, ChoiceCategory, ObjectQuery, PlayerSelector, QueryZone, ZoneDest, ZoneOpSpec,
        ZoneSelection,
    };

    if matches!(subject, SubjectRefAst::InZone { .. }) {
        let unwrapped = unwrap_in_zone_subject(subject).ok_or("oracle:unsupported-exile")?;
        return lower_exile_random_or_face_down(
            actor,
            &unwrapped,
            random,
            face_down,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        );
    }

    if let SubjectRefAst::Compound(items) = subject
        && !items.is_empty()
    {
        let mut trial = builder.clone();
        for it in items {
            lower_exile_random_or_face_down(
                actor,
                it,
                random,
                face_down,
                &mut trial,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
        }
        *builder = trial;
        return Ok(());
    }

    let exile_dest = ZoneDest::Exile {
        face_down,
        viewer: None,
        link_to_source: false,
        delayed_return: None,
        link_group: None,
    };

    let actor_ref = actor.unwrap_or(PlayerRefAst::You);
    let mut trial = builder.clone();
    let players = lower_affected_players_in_context(actor_ref, &mut trial, in_trigger)?;

    if random {
        if let SubjectRefAst::Anaphora(raw) = subject
            && let Some(query) = parse_whole_zone_anaphora_query(raw, &mut trial, in_trigger)?
            && let AffectedPlayers::One(rp) = players
        {
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                    query,
                    ZoneSelection::Random {
                        count: Box::new(ValueExpr::literal(1)),
                        chooser: PlayerSelector::Resolving(rp),
                    },
                    exile_dest,
                )));
            *builder = trial;
            return Ok(());
        }
        let (SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)) = subject
        else {
            return Err("oracle:unsupported-exile");
        };
        let mut inner = strip_matching_chooser(f, actor_ref);
        let z_opt = inner.zone.take();
        if matches!(
            inner.controller,
            Some(
                ControllerRelation::You
                    | ControllerRelation::ThatPlayer
                    | ControllerRelation::DefendingPlayer
            )
        ) && matches!(
            z_opt,
            Some(
                ZoneFilter::YourHand
                    | ZoneFilter::ThatPlayerHand
                    | ZoneFilter::YourGraveyard
                    | ZoneFilter::ThatPlayerGraveyard
            )
        ) {
            inner.controller = None;
        }
        let obj = if is_unrestricted_filter(&inner) {
            ObjectFilter::any()
        } else {
            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
        };
        bind_filter_player_target(&mut trial, &obj)?;
        if let AffectedPlayers::One(rp) = players {
            let query = match z_opt {
                Some(z) => zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                    .map_err(|_| "oracle:unsupported-exile")?,
                None => ObjectQuery::zone(QueryZone::Battlefield, obj),
            };
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                    query,
                    ZoneSelection::Random {
                        count: Box::new(ValueExpr::literal(1)),
                        chooser: PlayerSelector::Resolving(rp),
                    },
                    exile_dest,
                )));
        } else {
            const SEAT_BIND: BindingId = BindingId(907);
            let qz = match z_opt {
                Some(
                    ZoneFilter::YourHand
                    | ZoneFilter::ThatPlayerHand
                    | ZoneFilter::EachPlayerHand
                    | ZoneFilter::OpponentHand
                    | ZoneFilter::OpponentsHands
                    | ZoneFilter::AnyHand,
                ) => QueryZone::Hand(Box::new(PlayerSelector::Bound(SEAT_BIND))),
                Some(
                    ZoneFilter::YourGraveyard
                    | ZoneFilter::ThatPlayerGraveyard
                    | ZoneFilter::AnyGraveyard
                    | ZoneFilter::AllGraveyards
                    | ZoneFilter::OpponentGraveyard
                    | ZoneFilter::OpponentsGraveyards,
                ) => QueryZone::Graveyard(Box::new(PlayerSelector::Bound(SEAT_BIND))),
                None | Some(ZoneFilter::Battlefield) => {
                    QueryZone::BattlefieldControlledBy(Box::new(PlayerSelector::Bound(SEAT_BIND)))
                }
                _ => return Err("oracle:unsupported-exile"),
            };
            let inner_op = super::super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                ObjectQuery::zone(qz, obj),
                ZoneSelection::Random {
                    count: Box::new(ValueExpr::literal(1)),
                    chooser: PlayerSelector::Bound(SEAT_BIND),
                },
                exile_dest,
            ));
            trial.instructions.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(PlayerSelector::Group(players)),
                bind_player: SEAT_BIND,
                body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner_op])),
            });
        }
        *builder = trial;
        return Ok(());
    }

    // Non-random face-down exile
    match subject {
        s if is_self_pronoun(s, &trial, self_pronoun_valid) => {
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                    ObjectQuery::Source,
                    exile_dest,
                )));
        }
        SubjectRefAst::TargetObject {
            count: TargetCountAst::Exact(1),
            ..
        } => {
            let bt = lower_target_subject(subject, vocabulary)?;
            trial.bind_target(bt)?;
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                    ObjectQuery::Targets,
                    exile_dest,
                )));
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                    ObjectQuery::TriggerSubject,
                    exile_dest,
                )));
        }
        SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                    ObjectQuery::AttachedHost,
                    exile_dest,
                )));
        }
        SubjectRefAst::Anaphora(raw) => {
            if let Some(q) = parse_whole_zone_anaphora_query(raw, &mut trial, in_trigger)? {
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        q, exile_dest,
                    )));
            } else if trial.target != BodyTarget::None
                && !matches!(
                    trial.target,
                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                )
            {
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        ObjectQuery::Targets,
                        exile_dest,
                    )));
            } else if last_instruction_created_tokens(&trial.instructions) {
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        ObjectQuery::CreatedTokens,
                        exile_dest,
                    )));
            } else if last_instruction_moved_object(&trial.instructions)
                || (is_last_moved_card_anaphora(raw) && has_prior_moved_object(&trial.instructions))
            {
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        ObjectQuery::LastMoved,
                        exile_dest,
                    )));
            } else if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) {
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        ObjectQuery::TriggerSubject,
                        exile_dest,
                    )));
            } else if has_checked_revealed_top_strict(&trial.instructions) {
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        ObjectQuery::Revealed,
                        exile_dest,
                    )));
            } else {
                return Err("oracle:unsupported-exile");
            }
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
            let mut inner = strip_matching_chooser(f, actor_ref);
            let z = inner.zone.take().unwrap_or(ZoneFilter::Battlefield);
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
            };
            bind_filter_player_target(&mut trial, &obj)?;
            let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                .map_err(|_| "oracle:unsupported-exile")?;
            trial
                .instructions
                .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                    query, exile_dest,
                )));
        }
        SubjectRefAst::Group(f) => {
            let mut inner = strip_matching_chooser(f, actor_ref);
            let z = inner.zone.take().unwrap_or(ZoneFilter::Battlefield);
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
            };
            bind_filter_player_target(&mut trial, &obj)?;
            let hidden = is_hidden_zone_filter(z);
            if let AffectedPlayers::One(rp) = players {
                let query = zone_filter_to_object_query(z, obj, &mut trial, in_trigger)
                    .map_err(|_| "oracle:unsupported-exile")?;
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                        query,
                        ZoneSelection::Prompt {
                            chooser: PlayerSelector::Resolving(rp),
                            min: Box::new(ValueExpr::literal(1)),
                            max: Box::new(ValueExpr::literal(1)),
                            constraint: None,
                            category: ChoiceCategory::Generic,
                            hidden,
                        },
                        exile_dest,
                    )));
            } else {
                const SEAT_BIND: BindingId = BindingId(908);
                let qz = match z {
                    ZoneFilter::YourHand
                    | ZoneFilter::ThatPlayerHand
                    | ZoneFilter::EachPlayerHand
                    | ZoneFilter::OpponentHand
                    | ZoneFilter::OpponentsHands
                    | ZoneFilter::AnyHand => {
                        QueryZone::Hand(Box::new(PlayerSelector::Bound(SEAT_BIND)))
                    }
                    ZoneFilter::YourGraveyard
                    | ZoneFilter::ThatPlayerGraveyard
                    | ZoneFilter::AnyGraveyard
                    | ZoneFilter::AllGraveyards
                    | ZoneFilter::OpponentGraveyard
                    | ZoneFilter::OpponentsGraveyards => {
                        QueryZone::Graveyard(Box::new(PlayerSelector::Bound(SEAT_BIND)))
                    }
                    ZoneFilter::Battlefield => QueryZone::BattlefieldControlledBy(Box::new(
                        PlayerSelector::Bound(SEAT_BIND),
                    )),
                    _ => return Err("oracle:unsupported-exile"),
                };
                let inner_op = super::super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                    ObjectQuery::zone(qz, obj),
                    ZoneSelection::Prompt {
                        chooser: PlayerSelector::Bound(SEAT_BIND),
                        min: Box::new(ValueExpr::literal(1)),
                        max: Box::new(ValueExpr::literal(1)),
                        constraint: None,
                        category: ChoiceCategory::Generic,
                        hidden,
                    },
                    exile_dest,
                ));
                trial.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(PlayerSelector::Group(players)),
                    bind_player: SEAT_BIND,
                    body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner_op])),
                });
            }
        }
        _ => return Err("oracle:unsupported-exile"),
    }
    *builder = trial;
    Ok(())
}

#[cfg(test)]
mod wave10_zone_tests {
    use super::*;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new("wave10-zone".into(), [0; 32], vec!["Human".into()])
            .expect("test vocabulary")
    }

    fn exile(raw: &str) -> InstructionAst {
        InstructionAst::Exile {
            actor: None,
            subject: Box::new(SubjectRefAst::Anaphora(raw.into())),
            random: false,
            face_down: false,
            into_pile: false,
        }
    }

    #[test]
    fn wave10_identity_crisis_preserves_one_target_and_simultaneous_zones() {
        let card = InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("0abe0584-d876-4827-972b-589da53dbe96".into()),
            name: "Identity Crisis".into(),
            layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: "Identity Crisis".into(),
                type_line: Some("Sorcery".into()),
                oracle_text: Some(
                    "Exile all cards from target player's hand and graveyard.".into(),
                ),
                mana_cost: Some("{2}{W}{W}{B}{B}".into()),
                power: None,
                toughness: None,
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "test",
        };
        let program = compile_with_reason(&card, &vocabulary()).expect("Identity Crisis lowers");
        let ProgramKind::Spell(spell) = program.kind else {
            panic!("expected spell");
        };
        assert_eq!(spell.body.target, BodyTarget::SinglePlayer);
        assert_eq!(
            spell.body.instructions,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_group_from_zone(
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    SourceZoneFilter::HandOrGraveyard,
                    ObjectFilter::any()
                )
            ]
        );
    }

    #[test]
    fn wave10_combined_zone_exile_rejects_unrepresented_qualifiers() {
        for raw in [
            "all creature cards from target player's hand and graveyard",
            "all cards from target player's hand and graveyard except lands",
            "all cards from each player's hand and graveyard",
        ] {
            let mut builder = BodyBuilder::new();
            assert!(
                lower_zone_moves_instruction(
                    &exile(raw),
                    &mut builder,
                    &vocabulary(),
                    &mut LoweringIds::default(),
                    false,
                    false,
                    None,
                )
                .is_err(),
                "must preserve qualifier: {raw}"
            );
        }
    }
}
