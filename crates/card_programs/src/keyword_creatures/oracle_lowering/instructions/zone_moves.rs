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
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(
                        ObjectFilter::self_object(),
                    )));
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ))));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::DestroyTarget);
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
                builder.instructions.push(Instruction::DestroyTarget);
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
                count: TargetCountAst::ChosenX,
            } => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.x_target_count = true;
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
                builder.instructions.push(Instruction::DestroyTarget);
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None && builder.last_group_filter.is_some() =>
            {
                let obj = builder
                    .last_group_filter
                    .ok_or("oracle:unsupported-destroy")?;
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(obj)));
                Ok(())
            }
            SubjectRefAst::TriggerSubject | SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && builder.trigger_hint == TriggerHint::CombatBlock =>
            {
                builder
                    .instructions
                    .push(Instruction::DestroyCapturedCombatCreature {
                        no_regeneration: false,
                    });
                Ok(())
            }
            SubjectRefAst::TriggerSubject | SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && builder.trigger_hint == TriggerHint::DealtDamage =>
            {
                builder
                    .instructions
                    .push(Instruction::DestroyCapturedDamageSource);
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder.instructions.push(Instruction::DestroySubject {
                    subject: Box::new(EffectSubject::TriggerSubject),
                    no_regeneration: false,
                });
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if builder.target == BodyTarget::None
                    && in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
            {
                builder.instructions.push(Instruction::DestroySubject {
                    subject: Box::new(EffectSubject::TriggerSubject),
                    no_regeneration: false,
                });
                Ok(())
            }
            SubjectRefAst::SelfObject => {
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(
                        ObjectFilter::self_object(),
                    )));
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(obj)));
                Ok(())
            }
            SubjectRefAst::Group(f) if f.zone.is_none() => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                ensure_target_power_consumed(builder, &obj);
                builder.last_group_filter = Some(obj);
                builder.instructions.push(Instruction::DestroyChosen {
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj),
                    count: ValueExpr::literal(1),
                    allow_regeneration: true,
                });
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
        InstructionAst::ModifyPreviousDestroyNoRegen => {
            match builder.instructions.last_mut() {
                Some(last @ Instruction::DestroyTarget) => {
                    *last = Instruction::DestroyTargetWithoutRegeneration;
                    return Ok(());
                }
                Some(Instruction::DestroyCapturedCombatCreature { no_regeneration })
                | Some(Instruction::DestroySubject {
                    no_regeneration, ..
                }) => {
                    *no_regeneration = true;
                    return Ok(());
                }
                Some(Instruction::DestroyChosen {
                    allow_regeneration, ..
                }) => {
                    *allow_regeneration = false;
                    return Ok(());
                }
                Some(Instruction::DestroyGroup(filter)) => {
                    let cloned = filter.clone();
                    if let Some(last) = builder.instructions.last_mut() {
                        *last = Instruction::DestroyGroupWithoutRegeneration(cloned);
                        return Ok(());
                    }
                }
                _ => {}
            }
            Err("oracle:unsupported-no-regen-modifier")
        }
        InstructionAst::Exile {
            actor: None | Some(PlayerRefAst::You),
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } => match subject.as_ref() {
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && has_checked_revealed_top_strict(&builder.instructions) =>
            {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::RevealedCard,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                if !in_trigger && !self_pronoun_valid {
                    builder.instructions.push(Instruction::ExileResolvingSpell);
                } else {
                    builder.instructions.push(Instruction::ExileAbilitySource);
                }
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_target_subject(subject, vocabulary)?;
                if filter.zone.is_some() {
                    if matches!(
                        bt,
                        BodyTarget::SingleOwnGraveyardCard
                            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                    ) {
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        });
                    } else if matches!(
                        bt,
                        BodyTarget::AnyGraveyardCard
                            | BodyTarget::AnyGraveyardCreature
                            | BodyTarget::FilteredOwnGraveyardCard(_)
                            | BodyTarget::FilteredAnyGraveyardCard { .. }
                    ) {
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::ExileGraveyardTarget);
                    } else {
                        return Err("oracle:unsupported-exile-gy-target");
                    }
                } else if matches!(bt, BodyTarget::SingleSpell(_)) {
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::TargetObject,
                        destination: SubjectDestination::Exile {
                            delayed_return: None,
                        },
                    });
                } else {
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::ExileTarget);
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
                builder.instructions.push(Instruction::ExileTarget);
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
            } if filter.zone.is_none() && *n >= 2 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
            } if filter.zone.is_none() && *max > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                count: TargetCountAst::ChosenX,
            } if filter.zone.is_none() && !in_trigger => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.x_target_count = true;
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
                builder.instructions.push(Instruction::ExileGraveyardTarget);
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
                builder.instructions.push(Instruction::ExileGraveyardTarget);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: count @ (TargetCountAst::ChosenX | TargetCountAst::AnyNumber),
            } if filter.zone.is_some() && (*count != TargetCountAst::ChosenX || !in_trigger) => {
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
                builder.instructions.push(Instruction::ExileGraveyardTarget);
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
                        | "each opponent's hand"
                        | "your opponents' hands"
                        | "opponents' hands"
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
                    "each opponent's hand" | "your opponents' hands" | "opponents' hands" => {
                        AffectedPlayers::EachOpponent
                    }
                    "each player's hand" | "all hands" => AffectedPlayers::EachPlayer,
                    _ => return Err("oracle:unsupported-exile"),
                };
                builder.instructions.push(Instruction::ExileGroupFromZone {
                    players,
                    zone: SourceZoneFilter::Hand,
                    filter: Box::new(ObjectFilter::any()),
                });
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
                        | "each opponent's graveyard"
                        | "your opponents' graveyards"
                        | "opponents' graveyards"
                        | "each player's graveyard"
                        | "all graveyards"
                ) =>
            {
                let low = raw.trim().to_ascii_lowercase();
                match low.as_str() {
                    "each player's graveyard" | "all graveyards" => {
                        builder.instructions.push(Instruction::ExileAllGraveyards);
                    }
                    "your graveyard" => {
                        builder
                            .instructions
                            .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                                ResolvingPlayer::Controller,
                            )));
                    }
                    "target player's graveyard" => {
                        let p = lower_resolving_player_in_context(
                            PlayerRefAst::TargetPlayer,
                            builder,
                            in_trigger,
                        )?;
                        builder
                            .instructions
                            .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(p)));
                    }
                    "target opponent's graveyard" => {
                        let p = lower_resolving_player_in_context(
                            PlayerRefAst::TargetOpponent,
                            builder,
                            in_trigger,
                        )?;
                        builder
                            .instructions
                            .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(p)));
                    }
                    "that player's graveyard" | "their graveyard" => {
                        let p = lower_resolving_player_in_context(
                            PlayerRefAst::ThatPlayer,
                            builder,
                            in_trigger,
                        )?;
                        builder
                            .instructions
                            .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(p)));
                    }
                    "each opponent's graveyard"
                    | "your opponents' graveyards"
                    | "opponents' graveyards" => {
                        builder
                            .instructions
                            .push(Instruction::ExilePlayerGraveyards(
                                AffectedPlayers::EachOpponent,
                            ));
                    }
                    _ => return Err("oracle:unsupported-exile"),
                }
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
                builder.instructions.push(Instruction::ExileTarget);
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if last_instruction_created_tokens(&builder.instructions) =>
            {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::CreatedTokens,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::LastMoved,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
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
                builder.instructions.push(Instruction::ExileGraveyardTarget);
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
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::TargetObject,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if has_checked_revealed_top_strict(&builder.instructions) =>
            {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::RevealedCard,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::TriggeringObject,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                    && builder.target == BodyTarget::None =>
            {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::TriggeringObject,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder.instructions.push(Instruction::MoveGroup {
                    group: Box::new(ObjectFilter::Clause(ObjectClause::any().attached_to_self())),
                    destination: PublicZoneDestination::Exile,
                });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                if f.zone.is_none() && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0 =>
            {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::MoveGroup {
                    group: Box::new(obj),
                    destination: PublicZoneDestination::Exile,
                });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let mut inner = f.as_ref().clone();
                let is_linked_exile = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                let (players, zone) = if is_linked_exile
                    && matches!(f.zone, None | Some(ZoneFilter::Exile))
                {
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
                        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => (
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
                        _ => return Err("oracle:unsupported-exile"),
                    }
                };
                inner.zone = None;
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner, vocabulary)?
                };
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::ExileGroupFromZone {
                    players,
                    zone,
                    filter: Box::new(obj),
                });
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
                        .push(Instruction::ExileChosenPermanents {
                            players: AffectedPlayers::One(ResolvingPlayer::Controller),
                            amount: ValueExpr::literal(1),
                            filter: ptf,
                        });
                } else {
                    let mut inner = f.as_ref().clone();
                    inner.controller = None;
                    let obj = lower_object_filter(&inner, vocabulary)?;
                    builder.instructions.push(Instruction::ExileFromZone {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        zone: spellwake_engine::activated::CostExileZone::Battlefield,
                        amount: ValueExpr::literal(1),
                        filter: Box::new(obj),
                    });
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
                builder.instructions.push(Instruction::ExileFromZone {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    zone,
                    amount: ValueExpr::literal(1),
                    filter: Box::new(obj),
                });
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
                let (players, zone) = if (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
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
                        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => (
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
                        _ => return Err("oracle:unsupported-exile"),
                    }
                };
                inner.zone = None;
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner, vocabulary)?
                };
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::ExileGroupFromZone {
                    players,
                    zone,
                    filter: Box::new(obj),
                });
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
                        let rewrite_zone = |z: &mut Option<ZoneFilter>| match (*z, bound_player_bt)
                        {
                            (Some(ZoneFilter::TargetPlayerHand), BodyTarget::SinglePlayer) => {
                                *z = Some(ZoneFilter::ThatPlayerHand);
                            }
                            (Some(ZoneFilter::TargetPlayerGraveyard), BodyTarget::SinglePlayer) => {
                                *z = Some(ZoneFilter::ThatPlayerGraveyard);
                            }
                            (Some(ZoneFilter::TargetOpponentHand), BodyTarget::SingleOpponent) => {
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
            _ => Err("oracle:unsupported-exile"),
        },
        InstructionAst::Exile {
            actor: Some(actor_player),
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } => {
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
                    builder.instructions.push(Instruction::ExileGroupFromZone {
                        players,
                        zone: SourceZoneFilter::Hand,
                        filter: Box::new(ObjectFilter::any()),
                    });
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
                        .push(Instruction::ExilePlayerGraveyards(players));
                    Ok(())
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
                    builder.instructions.push(Instruction::ExileFromZone {
                        players,
                        zone: spellwake_engine::activated::CostExileZone::Hand,
                        amount: ValueExpr::literal(1),
                        filter: Box::new(obj),
                    });
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
                    builder.instructions.push(Instruction::ExileFromZone {
                        players,
                        zone: spellwake_engine::activated::CostExileZone::Graveyard,
                        amount: ValueExpr::literal(1),
                        filter: Box::new(obj),
                    });
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
                            .push(Instruction::ExileChosenPermanents {
                                players,
                                amount: ValueExpr::literal(1),
                                filter: ptf,
                            });
                    } else {
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(Instruction::ExileFromZone {
                            players,
                            zone: spellwake_engine::activated::CostExileZone::Battlefield,
                            amount: ValueExpr::literal(1),
                            filter: Box::new(obj),
                        });
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
                        None | Some(ZoneFilter::Battlefield) => SourceZoneFilter::Battlefield,
                        _ => return Err("oracle:unsupported-exile"),
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
                    builder.instructions.push(Instruction::ExileGroupFromZone {
                        players,
                        zone,
                        filter: Box::new(obj),
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-exile"),
            }
        }
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
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::AbilitySource,
                        destination: exile_until_dest,
                    });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::TriggeringObject,
                        destination: exile_until_dest,
                    });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::AttachedHost,
                        destination: exile_until_dest,
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    builder
                        .instructions
                        .push(Instruction::ExileTargetUntilSourceLeaves);
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::CreatedTokens,
                        destination: exile_until_dest,
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::LastMoved,
                        destination: exile_until_dest,
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && builder.target == BodyTarget::None
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::TriggeringObject,
                        destination: exile_until_dest,
                    });
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
                        .push(Instruction::ExileTargetUntilSourceLeaves);
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
                        .push(Instruction::ExileTargetUntilSourceLeaves);
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
                        .push(Instruction::ExileSubjectUntilSourceLeaves {
                            subject: Box::new(EffectSubject::Matching(obj)),
                        });
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
                        .push(Instruction::ExileSubjectUntilSourceLeaves {
                            subject: Box::new(EffectSubject::TargetAndSameNameMatching(obj)),
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-target-subject"),
            }
        }
        InstructionAst::ExileGraveyard { whose } => {
            let w = whose.trim().to_ascii_lowercase();
            match w.as_str() {
                "all" | "each player" | "each player's" | "all players" | "all players'" => {
                    builder.instructions.push(Instruction::ExileAllGraveyards);
                    Ok(())
                }
                "your" => {
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::Controller,
                        )));
                    Ok(())
                }
                "each opponent" | "each opponent's" | "all opponents" | "all opponents'"
                | "opponents" | "opponents'" | "your opponents" | "your opponents'" => {
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(
                            AffectedPlayers::EachOpponent,
                        ));
                    Ok(())
                }
                "defending player" | "defending player's" => {
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::DefendingPlayer,
                        )));
                    Ok(())
                }
                "target player" | "target player's" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "target opponent" | "target opponent's" => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "target card from a" => {
                    builder.bind_target(BodyTarget::AnyGraveyardCard)?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target creature card from a" => {
                    builder.bind_target(BodyTarget::AnyGraveyardCreature)?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target creature card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::creature(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target card from your" => {
                    builder
                        .bind_target(BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::any()))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target noncreature, nonland card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::from_clause(
                            ObjectClause::any()
                                .forbid_types(CardTypeMask::CREATURE.union(CardTypeMask::LAND)),
                        ),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target instant or sorcery card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::instant_or_sorcery(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target artifact card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::artifact(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target enchantment card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::enchantment(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "~ from your" => {
                    builder.instructions.push(Instruction::ExileAbilitySource);
                    Ok(())
                }
                "that card from your" => {
                    builder
                        .instructions
                        .push(Instruction::ExileCapturedGraveyardCard);
                    Ok(())
                }
                "that player" | "that player's" if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
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
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "a creature they control and their"
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    builder.instructions.push(Instruction::ExileFromZone {
                        players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        zone: spellwake_engine::activated::CostExileZone::Battlefield,
                        amount: ValueExpr::literal(1),
                        filter: Box::new(ObjectFilter::creature()),
                    });
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "all cards from target player's hand and" | "target player's hand and" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder.instructions.push(Instruction::ExileFromZone {
                        players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        zone: spellwake_engine::activated::CostExileZone::Hand,
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(
                            ResolvingPlayer::TargetPlayer,
                        )),
                        filter: Box::new(ObjectFilter::any()),
                    });
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "a card from your" | "one card from your" => {
                    builder
                        .instructions
                        .push(Instruction::ExileCardsFromGraveyard {
                            player: ResolvingPlayer::Controller,
                            count: NonZeroU8::new(1).ok_or("oracle:unreachable")?,
                        });
                    Ok(())
                }
                "two cards from your" => {
                    builder
                        .instructions
                        .push(Instruction::ExileCardsFromGraveyard {
                            player: ResolvingPlayer::Controller,
                            count: NonZeroU8::new(2).ok_or("oracle:unreachable")?,
                        });
                    Ok(())
                }
                "three cards from your" => {
                    builder
                        .instructions
                        .push(Instruction::ExileCardsFromGraveyard {
                            player: ResolvingPlayer::Controller,
                            count: NonZeroU8::new(3).ok_or("oracle:unreachable")?,
                        });
                    Ok(())
                }
                "a card from their" | "one card from their" if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::ExileCardsFromGraveyard {
                            player: ResolvingPlayer::UniqueRelatedPlayer,
                            count: NonZeroU8::new(1).ok_or("oracle:unreachable")?,
                        });
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
                                builder.instructions.push(Instruction::ExileFromZone {
                                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                    zone: spellwake_engine::activated::CostExileZone::Graveyard,
                                    amount: ValueExpr::literal(cnt),
                                    filter: Box::new(obj),
                                });
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
                                builder.instructions.push(Instruction::ExileFromZone {
                                    players: AffectedPlayers::One(rp),
                                    zone: spellwake_engine::activated::CostExileZone::Graveyard,
                                    amount: ValueExpr::count_graveyard(rp, obj),
                                    filter: Box::new(obj),
                                });
                                return Ok(());
                            }
                        }
                    }
                    let (prefix, zone) = if let Some(p) = other.strip_suffix(" from your") {
                        (p.trim(), ZoneFilter::YourGraveyard)
                    } else if let Some(p) = other
                        .strip_suffix(" from an opponent's")
                        .or_else(|| other.strip_suffix(" from an opponent"))
                    {
                        (p.trim(), ZoneFilter::OpponentGraveyard)
                    } else if let Some(p) = other
                        .strip_suffix(" from a single")
                        .or_else(|| other.strip_suffix(" from a"))
                        .or_else(|| other.strip_suffix(" from any"))
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
                            _ => return Err("oracle:unsupported-exile-gy"),
                        };
                        let r = tail
                            .trim()
                            .strip_prefix("target ")
                            .ok_or("oracle:unsupported-exile-gy")?;
                        (Some((0usize, max)), r)
                    } else if let Some((word, tail)) = prefix.split_once(" target ") {
                        let exact = match word {
                            "two" | "2" => 2usize,
                            "three" | "3" => 3,
                            "four" | "4" => 4,
                            "five" | "5" => 5,
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
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
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
                .push(Instruction::ReturnRandomGraveyardCards {
                    filter: obj_filter,
                    count: nz,
                    to_battlefield,
                });
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
                let counter = parse_return_with_counter(with_counter.as_deref())?;
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
            builder
                .instructions
                .push(Instruction::SelectZoneCardsToZone {
                    player: sel_player,
                    source_zone: src_zone,
                    filter: Box::new(obj),
                    min,
                    max,
                    destination,
                    reveal: false,
                });
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
                            .push(Instruction::PutTargetInLibrary(pl));
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let mut inner = f.as_ref().clone();
                        let (sel_player, src_zone) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Battlefield,
                            builder,
                            in_trigger,
                        )?;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(Instruction::SelectZoneCardsToZone {
                                player: sel_player,
                                source_zone: src_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Library(pl),
                                reveal: false,
                            });
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        if !in_trigger && !self_pronoun_valid {
                            let adj = normalize_resolving_spell_lib_placement(pl, builder.target);
                            builder
                                .instructions
                                .push(Instruction::PutResolvingSpellInLibrary(adj));
                        } else {
                            builder.instructions.push(Instruction::MoveSubject {
                                subject: MoveSubjectRef::AbilitySource,
                                destination: SubjectDestination::Library(pl),
                            });
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_valid_put_target_in_library_target(&builder.target) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::PutTargetInLibrary(pl));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::CreatedTokens,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::LastMoved,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AttachedHost,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if has_checked_revealed_top_strict(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::RevealedCard,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone.is_none() && from_zone.is_none() =>
                    {
                        let obj =
                            lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        if pl == LibraryPlacement::Bottom && builder.target == BodyTarget::None {
                            builder
                                .instructions
                                .push(Instruction::PutGroupOnLibraryBottom(Box::new(obj)));
                        } else {
                            builder.instructions.push(Instruction::MoveGroupToLibrary {
                                group: obj,
                                placement: pl,
                            });
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
                            .push(Instruction::MoveGraveyardGroupOwned {
                                players,
                                filter: Box::new(obj),
                                destination: SubjectDestination::Library(pl),
                            });
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
                            builder
                                .instructions
                                .push(Instruction::ReturnSourceGraveyardToHand);
                        } else if !in_trigger && !self_pronoun_valid {
                            builder
                                .instructions
                                .push(Instruction::ReturnResolvingSpellToHand);
                        } else {
                            builder.instructions.push(Instruction::ReturnSourceToHand);
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
                                    .push(Instruction::ReturnTargetGraveyardCreatureToHand);
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::OpponentGraveyard) => {
                                let mut inner = filter.as_ref().clone();
                                inner.zone = None;
                                let obj = if is_unrestricted_filter(&inner) {
                                    ObjectFilter::any()
                                } else {
                                    lower_object_filter(&inner, vocabulary)?
                                };
                                let opponents_only =
                                    effective_zone == Some(ZoneFilter::OpponentGraveyard);
                                builder.bind_target(BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only,
                                })?;
                                builder.instructions.push(Instruction::ReturnToOwnersHand);
                            }
                            None => {
                                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                                builder.bind_target(bt)?;
                                builder.instructions.push(Instruction::ReturnToOwnersHand);
                            }
                            _ => return Err("oracle:unsupported-return-hand-zone"),
                        }
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if *n >= 2 => {
                        let is_your_gy = filter.zone == Some(ZoneFilter::YourGraveyard)
                            || from_zone
                                .as_deref()
                                .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard"));
                        if is_your_gy {
                            let mut inner = filter.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                            builder.graveyard_target_bounds =
                                Some((usize::from(*n), usize::from(*n)));
                            builder
                                .instructions
                                .push(Instruction::ReturnTargetGraveyardCreatureToHand);
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
                        let is_your_gy = filter.zone == Some(ZoneFilter::YourGraveyard)
                            || from_zone
                                .as_deref()
                                .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard"));
                        if is_your_gy {
                            let mut inner = filter.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                            builder.graveyard_target_bounds = Some((0, usize::from(*max)));
                            builder
                                .instructions
                                .push(Instruction::ReturnTargetGraveyardCreatureToHand);
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
                        count: count @ (TargetCountAst::ChosenX | TargetCountAst::AnyNumber),
                    } if *count != TargetCountAst::ChosenX || !in_trigger => {
                        let is_your_gy = filter.zone == Some(ZoneFilter::YourGraveyard)
                            || from_zone
                                .as_deref()
                                .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard"));
                        if is_your_gy {
                            let mut inner = filter.as_ref().clone();
                            inner.zone = None;
                            let obj = if is_unrestricted_filter(&inner) {
                                ObjectFilter::any()
                            } else {
                                lower_object_filter(&inner, vocabulary)?
                            };
                            builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                            builder.graveyard_target_bounds = Some((0, usize::MAX));
                            if *count == TargetCountAst::ChosenX {
                                builder.x_target_count = true;
                            }
                            builder
                                .instructions
                                .push(Instruction::ReturnTargetGraveyardCreatureToHand);
                            Ok(())
                        } else if filter.zone.is_none() && from_zone.is_none() {
                            let bt = lower_battlefield_body_target(filter, vocabulary)?;
                            builder.bind_target(bt)?;
                            if *count == TargetCountAst::ChosenX {
                                builder.x_target_count = true;
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
                            builder.instructions.push(Instruction::ReturnToOwnersHand);
                        } else {
                            builder.instructions.push(Instruction::MoveSubject {
                                subject: MoveSubjectRef::TargetObject,
                                destination: SubjectDestination::Hand,
                            });
                        }
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::LastMoved,
                            destination: SubjectDestination::Hand,
                        });
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: SubjectDestination::Hand,
                        });
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
                        builder
                            .instructions
                            .push(Instruction::SelectZoneCardsToZone {
                                player: ResolvingPlayer::Controller,
                                source_zone: SourceZoneFilter::LinkedExile,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Hand,
                                reveal: false,
                            });
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
                            .push(Instruction::SelectZoneCardsToZone {
                                player: ResolvingPlayer::Controller,
                                source_zone: SourceZoneFilter::LastMoved,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Hand,
                                reveal: false,
                            });
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
                            .push(Instruction::SelectZoneCardsToZone {
                                player: ResolvingPlayer::Controller,
                                source_zone: SourceZoneFilter::Graveyard,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Hand,
                                reveal: false,
                            });
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
                            .push(Instruction::SelectZoneCardsToZone {
                                player: ResolvingPlayer::Controller,
                                source_zone: SourceZoneFilter::Battlefield,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Hand,
                                reveal: false,
                            });
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
                            .push(Instruction::ChooseOtherControlledCreature {
                                sacrifice: false,
                                or_exile_source: false,
                            });
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
                            .push(Instruction::ReturnControlledPermanents {
                                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                amount: ValueExpr::literal(1),
                                filter: ptf,
                            });
                        Ok(())
                    }
                    SubjectRefAst::AttachedHost(_) => {
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )),
                            destination: PublicZoneDestination::Hand,
                        });
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone.is_none() && from_zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(obj),
                            destination: PublicZoneDestination::Hand,
                        });
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
                                .push(Instruction::ReturnOwnGraveyardCardsOfTypeToHand(ct));
                            return Ok(());
                        }
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(Instruction::MoveGraveyardGroup {
                            filter: Box::new(obj),
                            all_graveyards: all_gy,
                            to_battlefield: false,
                            tapped: false,
                        });
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
                            .push(Instruction::MoveGraveyardGroupOwned {
                                players,
                                filter: Box::new(obj),
                                destination: SubjectDestination::Hand,
                            });
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
                let counter = match with_counter.as_deref() {
                    None => None,
                    Some(raw) => {
                        let low_raw = raw.trim().to_ascii_lowercase();
                        let (qty, kind_str): (u8, &str) = if let Some(rest) = low_raw
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
                        } else {
                            (1, low_raw.as_str())
                        };
                        let ck = parse_counter_kind(kind_str)
                            .ok_or("oracle:unsupported-counter-kind")?;
                        Some((ck, qty))
                    }
                };
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
                            Instruction::ReturnSourceGraveyardToBattlefield {
                                tapped: enters_tapped,
                            },
                        );
                        Ok(())
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: bf_dest,
                        });
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
                                        .push(Instruction::ReturnOwnGraveyardCreatureToBattlefield);
                                } else if !has_extra_bf_mods
                                    && matches!(
                                        bt,
                                        BodyTarget::FilteredOwnGraveyardCard(f)
                                            if !f.allows_instant_or_sorcery()
                                    )
                                {
                                    builder.instructions.push(
                                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                                            tapped: enters_tapped,
                                        },
                                    );
                                } else {
                                    builder.instructions.push(Instruction::MoveSubject {
                                        subject: MoveSubjectRef::TargetObject,
                                        destination: bf_dest,
                                    });
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
                                    builder.instructions.push(Instruction::MoveSubject {
                                        subject: MoveSubjectRef::TargetObject,
                                        destination: bf_dest,
                                    });
                                } else if !enters_tapped
                                    && bt == BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                                {
                                    builder.instructions.push(
                                        Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker,
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
                                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                                            tapped: enters_tapped,
                                        },
                                    );
                                } else {
                                    builder.instructions.push(Instruction::MoveSubject {
                                        subject: MoveSubjectRef::TargetObject,
                                        destination: bf_dest,
                                    });
                                }
                                Ok(())
                            }
                            None if has_extra_bf_mods => {
                                let bt = lower_target_subject(subject, vocabulary)?;
                                builder.bind_target(bt)?;
                                builder.instructions.push(Instruction::MoveSubject {
                                    subject: MoveSubjectRef::TargetObject,
                                    destination: bf_dest,
                                });
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: bf_dest,
                        });
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if !is_transformed
                            && counter.is_none()
                            && !attach_to_self
                            && !bf_flags.attacking
                            && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && from_zone
                                .as_deref()
                                .is_none_or(|z| z.to_ascii_lowercase().contains("exile"))
                            && builder.target == BodyTarget::None
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        inner.zone = None;
                        if !is_unrestricted_filter(&inner) {
                            return Err("oracle:unsupported-return-bf");
                        }
                        builder
                            .instructions
                            .push(Instruction::ReturnCardsExiledWithLinkExt {
                                group: spellwake_engine::program::LinkedExileGroup::SourceLinked,
                                under_controller_control: !bf_flags.under_owner_control,
                                tapped: enters_tapped,
                            });
                        Ok(())
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
                        builder.instructions.push(Instruction::MoveGraveyardGroup {
                            filter: Box::new(obj),
                            all_graveyards: all_gy,
                            to_battlefield: true,
                            tapped: enters_tapped,
                        });
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
                            .push(Instruction::MoveGraveyardGroupOwned {
                                players,
                                filter: Box::new(obj),
                                destination: bf_dest,
                            });
                        Ok(())
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: bf_dest,
                        });
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_) => {
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: subj_ref,
                            destination: bf_dest,
                        });
                        Ok(())
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AttachedHost,
                            destination: bf_dest,
                        });
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
                            .push(Instruction::SelectZoneCardsToZone {
                                player: sel_player,
                                source_zone: src_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: bf_dest,
                                reveal: false,
                            });
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
                    _ => Err("oracle:unsupported-return-bf"),
                }
            } else {
                Err("oracle:unsupported-return-dest")
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: bf_dest,
                        });
                        return Ok(());
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
                        ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x) = match *count {
                            TargetCountAst::Exact(n) => (usize::from(n), usize::from(n), false),
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true),
                            _ => return Err("oracle:unsupported-move-zone"),
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
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: bf_dest,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject { filter, count }
                        if filter.zone.is_none()
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
                            _ => return Err("oracle:unsupported-move-zone"),
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
                                action: PermanentTargetAction::MoveTo(bf_dest),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) => {
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
                        } else if has_checked_revealed_top_strict(&builder.instructions) {
                            MoveSubjectRef::RevealedCard
                        } else {
                            return Err("oracle:unsupported-move-zone");
                        };
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: subj_ref,
                            destination: bf_dest,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: bf_dest,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AttachedHost,
                            destination: bf_dest,
                        });
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: bf_dest,
                        });
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
                        builder.instructions.push(Instruction::MoveGraveyardGroup {
                            filter: Box::new(obj),
                            all_graveyards: all_gy,
                            to_battlefield: true,
                            tapped: enters_tapped,
                        });
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
                            .push(Instruction::MoveGraveyardGroupOwned {
                                players,
                                filter: Box::new(obj),
                                destination: bf_dest,
                            });
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
                                    builder.instructions.push(
                                        Instruction::PutHandPermanentsOntoBattlefield {
                                            player: ResolvingPlayer::Controller,
                                            types,
                                            maximum: 1,
                                            tapped: enters_tapped,
                                        },
                                    );
                                    return Ok(());
                                }
                            } else {
                                let mut inner = f.as_ref().clone();
                                inner.zone = None;
                                if let Ok(obj) = lower_object_filter(&inner, vocabulary) {
                                    builder.instructions.push(
                                        Instruction::PutHandPermanentFiltered {
                                            player: ResolvingPlayer::Controller,
                                            filter: Box::new(obj),
                                            maximum: 1,
                                            tapped: enters_tapped,
                                        },
                                    );
                                    return Ok(());
                                }
                            }
                        }
                        let mut inner = f.as_ref().clone();
                        let (sel_player, src_zone) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Hand,
                            builder,
                            in_trigger,
                        )?;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(Instruction::SelectZoneCardsToZone {
                                player: sel_player,
                                source_zone: src_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: bf_dest,
                                reveal: false,
                            });
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
                                .push(Instruction::PutResolvingSpellInLibrary(
                                    LibraryPlacement::BottomRandom,
                                ));
                        } else {
                            builder
                                .instructions
                                .push(Instruction::ShuffleAbilitySourceIntoLibrary);
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
                                .push(Instruction::ShuffleTargetIntoLibrary);
                            return Ok(());
                        }
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_valid_put_target_in_library_target(&builder.target) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::ShuffleTargetIntoLibrary);
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
                            .push(Instruction::PutResolvingSpellInLibrary(
                                LibraryPlacement::FromTop(idx),
                            ));
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
                            .push(Instruction::PutSubjectIntoLibraryFromTop {
                                subject: subj_ref,
                                offset: offset_expr,
                            });
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
                                .push(Instruction::PutOwnHandCardOnLibraryBottom);
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
                                .push(Instruction::PutTargetInLibrary(pl));
                            return Ok(());
                        }
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if filter.zone.is_none() && *n >= 2 => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                    } if filter.zone.is_none() && *max > 0 => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                        count: TargetCountAst::ChosenX,
                    } if filter.zone.is_none() && !in_trigger => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.x_target_count = true;
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
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                    {
                        let (min_t, max_t, is_x) = match *count {
                            TargetCountAst::Exact(n) => (usize::from(n), usize::from(n), false),
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true),
                            _ => return Err("oracle:unsupported-move-zone"),
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
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let mut inner = f.as_ref().clone();
                        let (sel_player, src_zone) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Battlefield,
                            builder,
                            in_trigger,
                        )?;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder
                            .instructions
                            .push(Instruction::SelectZoneCardsToZone {
                                player: sel_player,
                                source_zone: src_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Library(pl),
                                reveal: false,
                            });
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        if !in_trigger && !self_pronoun_valid {
                            let adj = normalize_resolving_spell_lib_placement(pl, builder.target);
                            builder
                                .instructions
                                .push(Instruction::PutResolvingSpellInLibrary(adj));
                        } else {
                            builder.instructions.push(Instruction::MoveSubject {
                                subject: MoveSubjectRef::AbilitySource,
                                destination: SubjectDestination::Library(pl),
                            });
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_valid_put_target_in_library_target(&builder.target) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::PutTargetInLibrary(pl));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::CreatedTokens,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::LastMoved,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AttachedHost,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if has_checked_revealed_top_strict(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::RevealedCard,
                            destination: SubjectDestination::Library(pl),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                        let obj =
                            lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        if pl == LibraryPlacement::Bottom && builder.target == BodyTarget::None {
                            builder
                                .instructions
                                .push(Instruction::PutGroupOnLibraryBottom(Box::new(obj)));
                        } else {
                            builder.instructions.push(Instruction::MoveGroupToLibrary {
                                group: obj,
                                placement: pl,
                            });
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
                            .push(Instruction::MoveGraveyardGroupOwned {
                                players,
                                filter: Box::new(obj),
                                destination: SubjectDestination::Library(pl),
                            });
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
                    | "to your hand"
                    | "to their hand"
                    | "to their hands"
                    | "to that player's hand"
                    | "to its owner's hand"
                    | "to their owner's hand"
                    | "to their owners' hands"
                    | "into your hand"
                    | "into their hand"
                    | "into their hands"
                    | "into that player's hand"
                    | "into its owner's hand"
                    | "into their owner's hand"
                    | "into their owners' hands"
            ) {
                Some(PublicZoneDestination::Hand)
            } else if matches!(dest.as_str(), "exile" | "into exile" | "to exile") {
                Some(PublicZoneDestination::Exile)
            } else if matches!(
                dest.as_str(),
                "graveyard"
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
                    | "into your graveyard"
                    | "into their graveyard"
                    | "into that player's graveyard"
                    | "into its owner's graveyard"
                    | "into their owner's graveyard"
                    | "into their owners' graveyards"
                    | "into a graveyard"
                    | "to its owner's graveyard"
                    | "to their owner's graveyard"
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
                                .push(Instruction::ReturnResolvingSpellToHand);
                        } else {
                            builder.instructions.push(Instruction::ReturnSourceToHand);
                        }
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Exile =>
                    {
                        if !in_trigger && !self_pronoun_valid {
                            builder.instructions.push(Instruction::ExileResolvingSpell);
                        } else {
                            builder.instructions.push(Instruction::ExileAbilitySource);
                        }
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Graveyard =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: SubjectDestination::Graveyard,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() && pd == PublicZoneDestination::Hand => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::ReturnToOwnersHand);
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() && pd == PublicZoneDestination::Exile => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::ExileTarget);
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } if let Some(sd) = subj_dest_from_pd(pd) => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: sd,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if filter.zone.is_none()
                        && *n >= 2
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                    } if filter.zone.is_none()
                        && *max > 0
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                    } if filter.zone.is_none()
                        && (*count != TargetCountAst::ChosenX || !in_trigger)
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
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
                                    | ZoneFilter::AnyGraveyard
                                    | ZoneFilter::SingleGraveyard
                            )
                        ) && matches!(
                            pd,
                            PublicZoneDestination::Hand | PublicZoneDestination::Exile
                        ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger))
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        let (min_t, max_t, is_x) = match *count {
                            TargetCountAst::Exact(n) => (usize::from(n), usize::from(n), false),
                            TargetCountAst::UpTo(max) => (0, usize::from(max), false),
                            TargetCountAst::AnyNumber => (0, usize::MAX, false),
                            TargetCountAst::ChosenX => (0, usize::MAX, true),
                            _ => return Err("oracle:unsupported-move-zone"),
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
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((min_t, max_t));
                        if is_x {
                            builder.x_target_count = true;
                        }
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: sd,
                        });
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
                                Instruction::ReturnToOwnersHand
                            } else {
                                Instruction::ExileTarget
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: sd,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::CreatedTokens,
                            destination: sd,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::LastMoved,
                            destination: sd,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if matches!(
                            builder.target,
                            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                        ) && has_checked_revealed_top_strict(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::RevealedCard,
                            destination: sd,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject
                        if in_trigger && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: sd,
                        });
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
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )),
                            destination: pd,
                        });
                        return Ok(());
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
                        builder.instructions.push(Instruction::MoveGraveyardGroup {
                            filter: Box::new(obj),
                            all_graveyards: all_gy,
                            to_battlefield: false,
                            tapped: false,
                        });
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
                            .push(Instruction::MoveGraveyardGroupOwned {
                                players,
                                filter: Box::new(obj),
                                destination: sd,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let mut inner = f.as_ref().clone();
                        let (sel_player, src_zone) = resolve_select_zone(
                            &mut inner,
                            SourceZoneFilter::Battlefield,
                            builder,
                            in_trigger,
                        )?;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        let subj_dest = match pd {
                            PublicZoneDestination::Hand => SubjectDestination::Hand,
                            PublicZoneDestination::Graveyard => SubjectDestination::Graveyard,
                            PublicZoneDestination::Exile => SubjectDestination::Exile {
                                delayed_return: None,
                            },
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder
                            .instructions
                            .push(Instruction::SelectZoneCardsToZone {
                                player: sel_player,
                                source_zone: src_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: subj_dest,
                                reveal: false,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(group_obj),
                            destination: pd,
                        });
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
                            _ => return Err("oracle:unsupported-move-zone"),
                        };
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: subj_dest,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if self_pronoun_valid
                            && is_self_anaphora_label(raw)
                            && builder.target == BodyTarget::None =>
                    {
                        match pd {
                            PublicZoneDestination::Hand => {
                                builder.instructions.push(Instruction::ReturnSourceToHand);
                            }
                            PublicZoneDestination::Exile => {
                                builder.instructions.push(Instruction::ExileAbilitySource);
                            }
                            PublicZoneDestination::Graveyard => {
                                builder.instructions.push(Instruction::MoveSubject {
                                    subject: MoveSubjectRef::AbilitySource,
                                    destination: SubjectDestination::Graveyard,
                                });
                            }
                            _ => return Err("oracle:unsupported-move-zone"),
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if has_checked_revealed_top_strict(&builder.instructions)
                            && let Some(sd) = subj_dest_from_pd(pd) =>
                    {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::RevealedCard,
                            destination: sd,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(obj),
                            destination: pd,
                        });
                        return Ok(());
                    }
                    _ => {}
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
                    builder.instructions.push(Instruction::SacrificeSubject {
                        subject: Box::new(EffectSubject::AbilitySource),
                    });
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
                builder.instructions.push(Instruction::SacrificeSubject {
                    subject: Box::new(EffectSubject::TargetObject),
                });
                Ok(())
            }
            SubjectRefAst::TriggerSubject if count.is_none() && in_trigger => {
                builder.instructions.push(Instruction::SacrificeSubject {
                    subject: Box::new(EffectSubject::TriggerSubject),
                });
                Ok(())
            }
            SubjectRefAst::AttachedHost(_)
                if count.is_none() && (in_trigger || self_pronoun_valid) =>
            {
                builder.instructions.push(Instruction::SacrificeSubject {
                    subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ))),
                });
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
                builder.instructions.push(Instruction::SacrificeSubject {
                    subject: Box::new(eff_subj),
                });
                Ok(())
            }
            SubjectRefAst::All(f) if count.is_none() && f.zone.is_none() => {
                let mut inner = f.as_ref().clone();
                match player {
                    PlayerRefAst::You => {
                        if inner.controller.is_none()
                            || inner.controller == Some(ControllerRelation::You)
                        {
                            inner.controller = Some(ControllerRelation::You);
                        } else {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                    }
                    PlayerRefAst::EachOpponent => {
                        if inner.controller.is_none()
                            || matches!(
                                inner.controller,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::Opponent
                                        | ControllerRelation::Opponents
                                )
                            )
                        {
                            inner.controller = Some(ControllerRelation::Opponent);
                        } else {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                    }
                    PlayerRefAst::EachPlayer => {
                        if inner.controller.is_none()
                            || inner.controller == Some(ControllerRelation::ThatPlayer)
                        {
                            inner.controller = None;
                        } else {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                    }
                    PlayerRefAst::EachOtherPlayer => {
                        if inner.controller.is_none()
                            || matches!(
                                inner.controller,
                                Some(ControllerRelation::ThatPlayer | ControllerRelation::NotYou)
                            )
                        {
                            inner.controller = Some(ControllerRelation::NotYou);
                        } else {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                    }
                    PlayerRefAst::TargetPlayer => {
                        if inner.controller.is_none()
                            || matches!(
                                inner.controller,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::TargetPlayer
                                )
                            )
                        {
                            inner.controller = Some(ControllerRelation::TargetPlayer);
                        } else {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                    }
                    PlayerRefAst::TargetOpponent => {
                        if inner.controller.is_none()
                            || matches!(
                                inner.controller,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::TargetOpponent
                                )
                            )
                        {
                            inner.controller = Some(ControllerRelation::TargetOpponent);
                        } else {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                    }
                    PlayerRefAst::ThatPlayer => {
                        if inner.controller.is_some()
                            && inner.controller != Some(ControllerRelation::ThatPlayer)
                        {
                            return Err("oracle:sacrifice-all-unsupported");
                        }
                        if builder.target == BodyTarget::SingleOpponent {
                            inner.controller = Some(ControllerRelation::TargetOpponent);
                        } else if builder.target == BodyTarget::SinglePlayer {
                            inner.controller = Some(ControllerRelation::TargetPlayer);
                        } else {
                            inner.controller = Some(ControllerRelation::ThatPlayer);
                        }
                    }
                    PlayerRefAst::DefendingPlayer
                        if inner.controller.is_none()
                            || matches!(
                                inner.controller,
                                Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::DefendingPlayer
                                )
                            ) =>
                    {
                        inner.controller = Some(ControllerRelation::DefendingPlayer);
                    }
                    _ => return Err("oracle:sacrifice-all-unsupported"),
                }
                let obj = lower_object_filter(&inner, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::SacrificeSubject {
                    subject: Box::new(EffectSubject::Matching(obj)),
                });
                Ok(())
            }
            SubjectRefAst::Group(f) | SubjectRefAst::Each(f) => {
                let f = strip_matching_chooser(f, *player);
                let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
                if count.is_none()
                    && matches!(player, PlayerRefAst::You)
                    && let Some(sel) = lower_sacrifice_selection(&f, vocabulary)
                {
                    builder
                        .instructions
                        .push(Instruction::SacrificeMatching(sel));
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
        InstructionAst::SacrificeUnlessPay {
            subject,
            cost: CostAtomAst::PayMana(raw),
        } if is_self_pronoun(subject, builder, self_pronoun_valid) => {
            let idx = builder.instructions.len();
            if let Some(cost) = parse_fixed_mana_cost(raw) {
                builder.instructions.push(Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::Controller,
                    cost,
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
                .push(Instruction::SacrificeStateTriggerSource);
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
                        Instruction::DiscardFiltered {
                            players: AffectedPlayers::One(rp),
                            amount: ValueExpr::literal(1),
                            filter: Box::new(obj_filter),
                        },
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
            builder.instructions.push(Instruction::DiscardFiltered {
                players: AffectedPlayers::One(rp),
                amount: ValueExpr::literal(1),
                filter: Box::new(obj_filter),
            });
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
            player: PlayerRefAst::You | PlayerRefAst::ThatPlayer,
            count: None,
            filter: None,
            anaphora: Some(_),
            random: false,
        } if has_checked_revealed_top(&builder.instructions) => {
            builder.instructions.push(Instruction::MoveSubject {
                subject: MoveSubjectRef::RevealedCard,
                destination: SubjectDestination::Graveyard,
            });
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
            builder.instructions.push(Instruction::DiscardFiltered {
                players,
                amount: amt,
                filter: Box::new(obj_filter),
            });
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
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                if amt == ValueExpr::literal(1) {
                    builder
                        .instructions
                        .push(Instruction::DiscardOneAtRandom(rp));
                    return Ok(());
                }
                if let ValueExpr::Literal(n) = &amt
                    && let Some(k) = n.to_usize()
                    && (2..=4).contains(&k)
                {
                    for _ in 0..k {
                        builder
                            .instructions
                            .push(Instruction::DiscardOneAtRandom(rp));
                    }
                    return Ok(());
                }
                builder.instructions.push(Instruction::RepeatLoop {
                    count: amt,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DiscardOneAtRandom(rp)],
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
                        Some(obj_filter) => Instruction::DiscardFiltered {
                            players: AffectedPlayers::One(ResolvingPlayer::LoopPlayer),
                            amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(
                                ResolvingPlayer::LoopPlayer,
                            )),
                            filter: obj_filter,
                        },
                    };
                    push_for_each_player(players, vec![op], &mut trial);
                } else {
                    let rp = lower_resolving_player_in_context(*player, &mut trial, in_trigger)?;
                    let op = match lowered_filter {
                        None => Instruction::Discard {
                            players: AffectedPlayers::One(rp),
                            amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(rp)),
                        },
                        Some(obj_filter) => Instruction::DiscardFiltered {
                            players: AffectedPlayers::One(rp),
                            amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(rp)),
                            filter: obj_filter,
                        },
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
            trial.instructions.push(Instruction::DiscardUpToExt {
                players,
                max_count: max_expr,
                filter: lowered_filter,
                any_number: *any_number,
            });
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
            if matches!(player, PlayerRefAst::EachPlayer)
                && let ValueExpr::Literal(n) = &amt
            {
                let n_u16 = u16::try_from(n.to_usize().ok_or("oracle:mill-ovf")?)
                    .map_err(|_| "oracle:mill-ovf")?;
                builder
                    .instructions
                    .push(Instruction::MillEachPlayer(n_u16));
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
            builder.instructions.push(Instruction::ShuffleOwnLibrary);
            Ok(())
        }
        InstructionAst::ShuffleLibrary { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::ShufflePlayerLibrary(
                        ResolvingPlayer::LoopPlayer,
                    )],
                    builder,
                );
                return Ok(());
            }
            let rp = if *player == PlayerRefAst::ThatPlayer
                && let Some(Instruction::SearchLibrary { filter }) = builder.instructions.last()
            {
                match filter.source {
                    spellwake_engine::program::SearchSource::OwnLibrary
                    | spellwake_engine::program::SearchSource::OwnLibraryOrGraveyard => {
                        filter.searcher
                    }
                    spellwake_engine::program::SearchSource::TargetPlayerLibrary
                    | spellwake_engine::program::SearchSource::TargetOpponentLibrary => {
                        ResolvingPlayer::TargetPlayer
                    }
                    spellwake_engine::program::SearchSource::PlayerLibrary(_) => {
                        lower_resolving_player_in_context(*player, builder, in_trigger)?
                    }
                }
            } else {
                lower_resolving_player_in_context(*player, builder, in_trigger)?
            };
            if rp == ResolvingPlayer::Controller {
                builder.instructions.push(Instruction::ShuffleOwnLibrary);
            } else {
                builder
                    .instructions
                    .push(Instruction::ShufflePlayerLibrary(rp));
            }
            Ok(())
        }
        InstructionAst::RevealHand { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::RevealPlayerHand(ResolvingPlayer::LoopPlayer)],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::RevealPlayerHand(rp));
            Ok(())
        }
        InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        } => {
            builder
                .instructions
                .push(Instruction::ShuffleOwnGraveyardIntoLibrary);
            Ok(())
        }
        InstructionAst::PhaseOut { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder
                    .instructions
                    .push(Instruction::PhaseOut(PhasingSubject::Source));
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
                    .push(Instruction::PhaseOut(PhasingSubject::Target));
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
                    .push(Instruction::PutResolvingSpellInLibrary(
                        LibraryPlacement::FromTop(offset_u8),
                    ));
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
                .push(Instruction::PutSubjectIntoLibraryFromTop {
                    subject: subj_ref,
                    offset: ValueExpr::literal(i64::from(offset_u8)),
                });
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
            let op = Instruction::SelectZoneCardsToZone {
                player,
                source_zone: src_zone,
                filter: Box::new(obj),
                min,
                max,
                destination: subj_dest,
                reveal: false,
            };
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
                    .push(Instruction::ExileLibraryTopLinked {
                        player: rp,
                        amount: cnt,
                        group: LinkedExileGroup::SourceLinked,
                        face_down: *face_down,
                    });
            } else if !*face_down
                && let AffectedPlayers::One(rp) = library_owner
                && let ValueExpr::Literal(n) = &cnt
                && let Some(u) = n.to_usize().and_then(|v| u16::try_from(v).ok())
            {
                builder.instructions.push(Instruction::ExileLibraryTop {
                    player: rp,
                    amount: u,
                });
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
                .push(Instruction::ShuffleEachHandAndGraveyard);
            let amt = lower_value_expr_in_context(
                resolve_x(draw, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if amt != ValueExpr::literal(0) {
                builder
                    .instructions
                    .push(Instruction::DrawEachPlayer { amount: amt });
            }
            Ok(())
        }
        InstructionAst::ShuffleIntoLibrary { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            if !in_trigger && !self_pronoun_valid {
                builder
                    .instructions
                    .push(Instruction::PutResolvingSpellInLibrary(
                        LibraryPlacement::BottomRandom,
                    ));
            } else {
                builder
                    .instructions
                    .push(Instruction::ShuffleAbilitySourceIntoLibrary);
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
                    .push(Instruction::ShuffleTargetIntoLibrary);
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
                    .push(Instruction::ShuffleTargetIntoLibrary);
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
                    .push(Instruction::ShuffleTargetIntoLibrary);
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
                            .push(Instruction::ShuffleOwnGraveyardIntoLibrary);
                    }
                    "your hand" => {
                        builder
                            .instructions
                            .push(Instruction::ShuffleSubjectIntoLibrary {
                                subject: Box::new(EffectSubject::Matching(ObjectFilter::any())),
                                zone: Some(SourceZoneFilter::Hand),
                            });
                    }
                    "your hand and graveyard" | "your graveyard and hand" => {
                        builder
                            .instructions
                            .push(Instruction::ShuffleSubjectIntoLibrary {
                                subject: Box::new(EffectSubject::Matching(ObjectFilter::any())),
                                zone: Some(SourceZoneFilter::Hand),
                            });
                        builder
                            .instructions
                            .push(Instruction::ShuffleOwnGraveyardIntoLibrary);
                    }
                    "their hand and graveyard"
                    | "their graveyard and hand"
                    | "their hands and graveyards" => {
                        builder
                            .instructions
                            .push(Instruction::ShuffleEachHandAndGraveyard);
                    }
                    "all graveyards" => {
                        builder
                            .instructions
                            .push(Instruction::ShuffleSubjectIntoLibrary {
                                subject: Box::new(EffectSubject::Matching(ObjectFilter::any())),
                                zone: Some(SourceZoneFilter::AnyGraveyard),
                            });
                    }
                    _ => {
                        builder
                            .instructions
                            .push(Instruction::ShuffleSubjectIntoLibrary {
                                subject: Box::new(EffectSubject::Matching(ObjectFilter::any())),
                                zone: Some(SourceZoneFilter::OpponentGraveyard),
                            });
                    }
                }
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if last_instruction_created_tokens(&builder.instructions) =>
            {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::CreatedTokens),
                        zone: None,
                    });
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
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::TargetObject),
                        zone: None,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::LastMoved),
                        zone: None,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if has_checked_revealed_top_strict(&builder.instructions) =>
            {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::RevealedCard,
                    destination: SubjectDestination::Library(LibraryPlacement::Bottom),
                });
                builder.instructions.push(Instruction::ShuffleOwnLibrary);
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::TriggerSubject),
                        zone: None,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                    && builder.target == BodyTarget::None =>
            {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::TriggerSubject),
                        zone: None,
                    });
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))),
                        zone: None,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && let Some(group_obj) = builder.last_group_filter =>
            {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::Matching(group_obj)),
                        zone: None,
                    });
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
                        .push(Instruction::ShuffleOwnGraveyardIntoLibrary);
                } else {
                    builder
                        .instructions
                        .push(Instruction::ShuffleSubjectIntoLibrary {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            zone,
                        });
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
                builder.instructions.push(Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::Controller,
                    cost: mana,
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
                if let Some(Instruction::OfferManaPayment {
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
                        vec![Instruction::RevealPlayerHand(ResolvingPlayer::LoopPlayer)],
                        builder,
                    );
                    return Ok(());
                }
                let rp = lower_resolving_player_in_context(*p, builder, in_trigger)?;
                builder.instructions.push(Instruction::RevealPlayerHand(rp));
                Ok(())
            }
            SubjectRefAst::Anaphora(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "your hand" => {
                    builder
                        .instructions
                        .push(Instruction::RevealPlayerHand(ResolvingPlayer::Controller));
                    Ok(())
                }
                "their hand" | "that player's hand" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    builder.instructions.push(Instruction::RevealPlayerHand(rp));
                    Ok(())
                }
                "target player's hand" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::RevealPlayerHand(ResolvingPlayer::TargetPlayer));
                    Ok(())
                }
                "target opponent's hand" => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::RevealPlayerHand(ResolvingPlayer::TargetPlayer));
                    Ok(())
                }
                _ => Err("oracle:unsupported-reveal-cards"),
            },
            _ => Err("oracle:unsupported-reveal-cards"),
        },
        InstructionAst::ExileOrRevealTopUntil {
            exile: true,
            library,
            filter,
        } => {
            let lib_low = library.trim().to_ascii_lowercase();
            let (req_bt, library_owner) = match lib_low.as_str() {
                "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" => {
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
                "that player" | "that player's" | "thatplayer" | "their" | "their library" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?,
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
            let mut inner_f = filter.as_ref().clone();
            inner_f.zone = None;
            let obj = lower_object_filter(&inner_f, vocabulary)?;
            builder.instructions.push(Instruction::RevealUntilMatchExt {
                library_owner,
                chooser: ResolvingPlayer::Controller,
                filter: Box::new(obj),
                count: ValueExpr::literal(1),
                match_dest: spellwake_engine::program::LookTopKeepDest::Exile {
                    playable: false,
                    duration: ResolvedDuration::NoStatedDuration,
                },
                rest_dest: spellwake_engine::program::LookTopRestDest::Exile,
            });
            Ok(())
        }
        InstructionAst::RevealByPlayer { player, subject } => match subject.as_ref() {
            SubjectRefAst::Anaphora(raw) => {
                let low = raw.trim().to_ascii_lowercase();
                if low == "the top card" {
                    let make_reveal = |library_owner: ResolvingPlayer| Instruction::LookTopChoose {
                        count: ValueExpr::literal(1),
                        library_owner,
                        filter: None,
                        reveal: true,
                        reveal_only: true,
                        to_battlefield: false,
                        enters_tapped: false,
                        take_all_matching: false,
                        max_picks: 0,
                        rest_to_graveyard: false,
                        rest_to_exile: false,
                        rest_to_top: false,
                        random_rest_order: false,
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
                        Instruction::LookTopChoose {
                            count,
                            library_owner,
                            reveal_only: true,
                            ..
                        } => (*count == ValueExpr::literal(1)).then_some(Some(*library_owner)),
                        Instruction::MayRevealLibraryTop => Some(Some(ResolvingPlayer::Controller)),
                        Instruction::ExileLibraryTop { .. }
                        | Instruction::ExileLibraryTopLinked { .. } => Some(None),
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
                    trial.instructions.push(Instruction::LookTopChoose {
                        count: ValueExpr::literal(1),
                        library_owner,
                        filter: None,
                        reveal: true,
                        reveal_only: true,
                        to_battlefield: false,
                        enters_tapped: false,
                        take_all_matching: false,
                        max_picks: 0,
                        rest_to_graveyard: false,
                        rest_to_exile: false,
                        rest_to_top: false,
                        random_rest_order: false,
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
                            vec![Instruction::RevealPlayerHand(ResolvingPlayer::LoopPlayer)],
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
                    builder.instructions.push(Instruction::RevealPlayerHand(rp));
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

pub(super) fn has_live_last_moved_producer(
    ops: &[Instruction],
    expected: LastMovedProducerKind,
) -> bool {
    for op in ops.iter().rev() {
        match op {
            Instruction::Mill { .. } | Instruction::MillEachPlayer(_) => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Mill
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::ExileLibraryTop { .. }
            | Instruction::ExileLibraryTopLinked { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::ExileFromZone { .. }
            | Instruction::ExileGroupFromZone { .. }
            | Instruction::ExileChosenPermanents { .. }
            | Instruction::ExileTarget
            | Instruction::ExileGraveyardTarget
            | Instruction::ExileSubjectUntilSourceLeaves { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any | LastMovedProducerKind::Exile
                );
            }
            Instruction::Discard { .. }
            | Instruction::DiscardFiltered { .. }
            | Instruction::DiscardOneAtRandom(_)
            | Instruction::DiscardUpTo { .. }
            | Instruction::DiscardUpToExt { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Discard
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::Sacrifice { .. }
            | Instruction::SacrificeSubject { .. }
            | Instruction::SacrificeMatching(_) => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Sacrifice
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::DestroyGroup(_)
            | Instruction::DestroyGroupWithoutRegeneration(_)
            | Instruction::DestroyChosen { .. } => {
                return matches!(
                    expected,
                    LastMovedProducerKind::Any
                        | LastMovedProducerKind::Destroy
                        | LastMovedProducerKind::Graveyard
                );
            }
            Instruction::MoveSubject { destination, .. }
            | Instruction::SelectZoneCardsToZone { destination, .. }
            | Instruction::MoveGraveyardGroupOwned { destination, .. } => {
                return match destination {
                    SubjectDestination::Exile { .. } => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Exile
                    ),
                    SubjectDestination::Graveyard => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Graveyard
                    ),
                    _ => matches!(expected, LastMovedProducerKind::Any),
                };
            }
            Instruction::MoveGroup { destination, .. } => {
                return match destination {
                    PublicZoneDestination::Exile => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Exile
                    ),
                    PublicZoneDestination::Graveyard => matches!(
                        expected,
                        LastMovedProducerKind::Any | LastMovedProducerKind::Graveyard
                    ),
                    _ => matches!(expected, LastMovedProducerKind::Any),
                };
            }
            Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::PayLife { .. }
            | Instruction::PayControllerLife(_)
            | Instruction::PayEnergy { .. }
            | Instruction::PayControllerEnergy(_)
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::CreateTemporaryRule { .. }
            | Instruction::ChooseValue { .. }
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::AdjustPlayersLife { .. }
            | Instruction::GainControllerLife { .. }
            | Instruction::AddMana(_)
            | Instruction::AddManaForPlayer { .. }
            | Instruction::RevealPlayerHand(_)
            | Instruction::LookTopChoose {
                reveal_only: true, ..
            } => continue,
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
    let classify = |pred: &spellwake_oracle_compiler::ast::FilterPredicateAst| -> Option<LastMovedProducerKind> {
        use spellwake_oracle_compiler::ast::{FilterPredicateAst, PriorActionRefKind};
        match pred {
            FilterPredicateAst::PriorActionZoneRef { kind } => match kind.as_str() {
                "from_among_those" | "from_among_those_cards" => Some(LastMovedProducerKind::Any),
                "milled_this_way" => Some(LastMovedProducerKind::Mill),
                "exiled_this_way" => Some(LastMovedProducerKind::Exile),
                _ => None,
            },
            FilterPredicateAst::AffectedByPriorAction(kind) => match kind {
                PriorActionRefKind::PutIntoGraveyardThisWay => {
                    Some(LastMovedProducerKind::Graveyard)
                }
                PriorActionRefKind::ExiledThisWay => Some(LastMovedProducerKind::Exile),
                PriorActionRefKind::DiscardedThisWay => Some(LastMovedProducerKind::Discard),
                PriorActionRefKind::SacrificedThisWay => Some(LastMovedProducerKind::Sacrifice),
                PriorActionRefKind::DestroyedThisWay => Some(LastMovedProducerKind::Destroy),
                _ => None,
            },
            FilterPredicateAst::StateQualification(raw) => {
                match raw.trim().to_ascii_lowercase().as_str() {
                    "milled this way" | "they milled this way" => {
                        Some(LastMovedProducerKind::Mill)
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
            if !has_live_last_moved_producer(ops, kind) {
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
        Some(ZoneFilter::YourGraveyard) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::Graveyard))
        }
        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => Ok((
            ResolvingPlayer::Controller,
            SourceZoneFilter::OpponentGraveyard,
        )),
        Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::AnyGraveyard))
        }
        Some(ZoneFilter::ThatPlayerGraveyard) => Ok((
            lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?,
            SourceZoneFilter::Graveyard,
        )),
        Some(ZoneFilter::TargetPlayerGraveyard) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok((ResolvingPlayer::TargetPlayer, SourceZoneFilter::Graveyard))
        }
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
        Some(ZoneFilter::Exile) => Ok((ResolvingPlayer::Controller, SourceZoneFilter::Exile)),
        _ => Err("oracle:unsupported-move-zone"),
    }
}

pub(super) fn has_checked_revealed_top(ops: &[Instruction]) -> bool {
    ops.iter().rev().any(|op| {
        matches!(
            op,
            Instruction::LookTopChoose {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
                | Instruction::ExileLibraryTop { .. }
                | Instruction::ExileLibraryTopLinked { .. }
                | Instruction::RevealUntilMatch { .. }
                | Instruction::RevealUntilMatchExt { .. }
        )
    })
}

/// True only when a prior instruction satisfies the engine's `definitely_checked_top`
/// invariant required to validate `MoveSubjectRef::RevealedCard`.
pub(super) fn has_checked_revealed_top_strict(ops: &[Instruction]) -> bool {
    ops.iter().rev().any(|op| {
        matches!(
            op,
            Instruction::LookTopChoose {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
                | Instruction::ExileLibraryTop { .. }
                | Instruction::ExileLibraryTopLinked { .. }
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

pub(super) fn last_instruction_moved_object(ops: &[Instruction]) -> bool {
    matches!(
        ops.last(),
        Some(
            Instruction::MoveSubject { .. }
                | Instruction::SelectZoneCardsToZone { .. }
                | Instruction::ChooseObjects { .. }
                | Instruction::MoveGroup { .. }
                | Instruction::MoveGroupToLibrary { .. }
                | Instruction::MoveGraveyardGroup { .. }
                | Instruction::MoveGraveyardGroupOwned { .. }
                | Instruction::ReturnOwnGraveyardCreatureToBattlefield
                | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
                | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { .. }
                | Instruction::ReturnChosenXGraveyardCreatures
                | Instruction::ReturnGraveyardTargetsTapped(_)
                | Instruction::ReturnRandomGraveyardCards { .. }
                | Instruction::ReturnSourceGraveyardToBattlefield { .. }
                | Instruction::PutHandPermanentFiltered { .. }
                | Instruction::PutHandPermanentsOntoBattlefield { .. }
                | Instruction::ReturnExiledTargetCohort { .. }
                | Instruction::ReturnCardsDestroyedThisWay
                | Instruction::ReturnCapturedDyingCreature
                | Instruction::ReturnTargetGraveyardCreatureToHand
                | Instruction::ReturnToOwnersHand
                | Instruction::RevealUntilMatch { .. }
                | Instruction::RevealUntilMatchExt { .. }
                | Instruction::LookTopChoose { .. }
                | Instruction::LookTopChooseExt { .. }
                | Instruction::RevealTopTwoPiles { .. }
                | Instruction::ExileGroupFromZone { .. }
                | Instruction::ExileFromZone { .. }
                | Instruction::ExileChosenPermanents { .. }
                | Instruction::ExileTarget
                | Instruction::ExileGraveyardTarget
                | Instruction::ExileLibraryTopLinked { .. }
                | Instruction::ManifestDread
                | Instruction::Incubate { .. }
                | Instruction::SearchLibrary { .. }
        )
    ) || matches!(
        ops.last(),
        Some(
            Instruction::ModifyGroupUntilEndOfTurn { group, .. }
                | Instruction::ChangeGroup { group, .. }
                | Instruction::ModifyGroupByAmount { group, .. }
                | Instruction::SuppressGroupKeywords { group, .. }
        ) if matches!(group.as_ref(), EffectSubject::LastMoved)
    ) || matches!(
        ops.last(),
        Some(
            Instruction::PutCountersOnSubject { subject, .. }
                | Instruction::RemoveCountersFromSubject { subject, .. }
                | Instruction::SetSubjectTapped { subject, .. }
                | Instruction::SkipNextUntap { subject }
                | Instruction::RequireCombatAction { subject, .. }
        ) if matches!(subject.as_ref(), EffectSubject::LastMoved)
    )
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
    let fl = fz.to_ascii_lowercase();
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
