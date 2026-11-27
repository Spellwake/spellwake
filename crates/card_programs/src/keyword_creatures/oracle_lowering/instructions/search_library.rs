use super::*;

pub(super) fn lower_search_library_group(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::Draw {
            player,
            count,
            up_to: true,
        } if matches!(resolve_x(count, bound_x), ValueExprAst::Const(n) if (1..=10).contains(n)) => {
            let ValueExprAst::Const(n) = *resolve_x(count, bound_x) else {
                return Err("oracle:unsupported-draw-player");
            };
            let draw_one_ops = |rp: ResolvingPlayer| -> Vec<Instruction> {
                let draw_op =
                    super::super::toolbox_emit::draw_resolving_player(rp, ValueExpr::literal(1));
                if n == 1 {
                    vec![Instruction::OptionalEffect { chooser: rp }, draw_op]
                } else {
                    vec![Instruction::RepeatLoop {
                        count: ValueExpr::literal(i64::from(n)),
                        body: Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::OptionalEffect { chooser: rp }, draw_op],
                        )),
                    }]
                }
            };
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(players, draw_one_ops(ResolvingPlayer::LoopPlayer), builder);
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.extend(draw_one_ops(rp));
            Ok(())
        }
        InstructionAst::Draw {
            player,
            count,
            up_to: false,
        } => {
            let amount = rewrite_milled_card_mana_value(
                lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                builder,
            );
            match player {
                PlayerRefAst::You => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_controller(amount));
                    Ok(())
                }
                PlayerRefAst::EachPlayer => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_each_player(amount));
                    Ok(())
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_target_player(amount));
                    Ok(())
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_target_player(amount));
                    Ok(())
                }
                PlayerRefAst::EachOpponent => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_player(
                            AffectedPlayers::EachOpponent,
                            amount,
                        ));
                    Ok(())
                }
                PlayerRefAst::EachOtherPlayer => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_player(
                            AffectedPlayers::EachOtherPlayer,
                            amount,
                        ));
                    Ok(())
                }
                PlayerRefAst::ThatPlayer => {
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::draw_target_player(amount));
                        Ok(())
                    } else if let Ok(rp) =
                        lower_resolving_player_in_context(*player, builder, in_trigger)
                        && (rp != ResolvingPlayer::UniqueRelatedPlayer || in_trigger)
                    {
                        builder.instructions.push(
                            super::super::toolbox_emit::draw_resolving_player(rp, amount),
                        );
                        Ok(())
                    } else if in_trigger {
                        builder.instructions.push(
                            super::super::toolbox_emit::draw_resolving_player(
                                ResolvingPlayer::UniqueRelatedPlayer,
                                amount,
                            ),
                        );
                        Ok(())
                    } else {
                        Err("oracle:draw-related-multi")
                    }
                }
                PlayerRefAst::ItsController
                | PlayerRefAst::ThatObjectController
                | PlayerRefAst::ItsOwner
                | PlayerRefAst::ActivePlayer
                | PlayerRefAst::DefendingPlayer
                | PlayerRefAst::EnchantedPlayer
                | PlayerRefAst::ChosenPlayer
                | PlayerRefAst::ThatPlayerOrPlaneswalkerController
                | PlayerRefAst::TargetOpponentAtRandom => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::draw_resolving_player(
                            rp, amount,
                        ));
                    Ok(())
                }
                _ => Err("oracle:unsupported-draw-player"),
            }
        }
        InstructionAst::Scry { player, count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::Scry {
                        player: ResolvingPlayer::LoopPlayer,
                        amount: amt,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Scry {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Surveil { player, count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::Surveil {
                        player: ResolvingPlayer::LoopPlayer,
                        amount: amt,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Surveil {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::SearchLibrary {
            library,
            count,
            filter,
            reveal,
            to_zone,
            shuffle,
            up_to,
            any_number,
            extra_zones,
            secondary_filter,
            secondary_is_or,
            where_x,
            rest_to_zone,
            modifier,
            follow_up,
            exact,
        } => {
            let eff_dest = if to_zone.to_ascii_lowercase().contains("after_shuffle")
                && to_zone.to_ascii_lowercase().contains("top")
            {
                "top of your library"
            } else {
                to_zone.as_str()
            };
            let legacy_eligible =
                !*exact && (*shuffle || to_zone.to_ascii_lowercase().contains("after_shuffle"));
            let legacy_err = if legacy_eligible {
                match lower_search_library_instruction(
                    library,
                    count,
                    filter,
                    *reveal,
                    eff_dest,
                    *up_to,
                    *any_number,
                    extra_zones,
                    secondary_filter.as_deref(),
                    *secondary_is_or,
                    where_x.as_deref(),
                    rest_to_zone.as_deref(),
                    modifier.as_ref(),
                    follow_up,
                    false,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                ) {
                    Ok(()) => return Ok(()),
                    Err("oracle:dynamic-search-count")
                        if (*up_to || !is_unrestricted_filter(filter))
                            && !*any_number
                            && (secondary_filter.is_none() || *secondary_is_or)
                            && rest_to_zone.is_none()
                            && follow_up.is_empty() =>
                    {
                        let mut dyn_ok = false;
                        let dyn_ast_opt = match where_x.as_deref() {
                            Some(we) if matches!(count.as_ref(), ValueExprAst::ChosenX) => {
                                Some(resolve_x(we, bound_x))
                            }
                            None => Some(resolve_x(count, bound_x)),
                            Some(_) => None,
                        };
                        if let Some(dyn_ast) = dyn_ast_opt
                            && let Ok(dyn_expr) = lower_value_expr_in_context(
                                dyn_ast, builder, in_trigger, vocabulary,
                            )
                            && let Ok(mut search) = lower_library_search_ext(
                                library,
                                &ValueExprAst::Const(1),
                                filter,
                                *reveal,
                                eff_dest,
                                vocabulary,
                                false,
                                true,
                                false,
                                &SearchLibraryExtra {
                                    extra_zones: extra_zones.as_slice(),
                                    secondary_filter: secondary_filter.as_deref(),
                                    secondary_is_or: *secondary_is_or,
                                    where_x: None,
                                    rest_to_zone: None,
                                    modifier: modifier.as_ref(),
                                },
                            )
                            && matches!(
                                search.destination,
                                SearchDestination::Hand
                                    | SearchDestination::Graveyard
                                    | SearchDestination::Exile { .. }
                                    | SearchDestination::Battlefield {
                                        post_entry:
                                            spellwake_engine::program::SearchBattlefieldPostEntry::None,
                                        ..
                                    }
                            )
                        {
                            search.count = SearchCount::UpToExpr(Box::new(dyn_expr));
                            let mut trial = builder.clone();
                            if bind_search_target(&mut trial, &search).is_ok() {
                                let rp_ok = if search.searcher
                                    == ResolvingPlayer::UniqueRelatedPlayer
                                {
                                    if let Ok(rp) = lower_resolving_player_in_context(
                                        PlayerRefAst::ThatPlayer,
                                        &mut trial,
                                        in_trigger,
                                    ) {
                                        search.searcher = rp;
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                };
                                if rp_ok {
                                    trial.instructions.push(Instruction::SearchLibrary {
                                        filter: Box::new(search),
                                    });
                                    *builder = trial;
                                    dyn_ok = true;
                                }
                            }
                        }
                        if dyn_ok {
                            return Ok(());
                        }
                        "oracle:dynamic-search-count"
                    }
                    Err(e) => e,
                }
            } else {
                "oracle:unsupported-instruction"
            };
            if try_lower_search_library_toolbox(
                library,
                count,
                filter,
                *reveal,
                to_zone,
                *shuffle,
                *up_to,
                *any_number,
                *exact,
                extra_zones,
                secondary_filter.as_deref(),
                *secondary_is_or,
                where_x.as_deref(),
                rest_to_zone.as_deref(),
                modifier.as_ref(),
                follow_up,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
            {
                return Ok(());
            }
            Err(legacy_err)
        }
        InstructionAst::TapPermanent {
            subject,
            any_number: true,
        } => match subject.as_ref() {
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1) | TargetCountAst::AnyNumber,
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: u16::MAX,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                if f.zone.is_none() =>
            {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::UntapOrTapSelect {
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj),
                    count: ValueExpr::literal(i64::from(u16::MAX)),
                    up_to: true,
                    tap: true,
                });
                Ok(())
            }
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for it in items {
                    lower_instruction(
                        &InstructionAst::TapPermanent {
                            subject: Box::new(it.clone()),
                            any_number: true,
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
            _ => Err("oracle:unsupported-tap-perm"),
        },
        InstructionAst::TapPermanent {
            subject,
            any_number: false,
        } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::tap_ability_source());
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::AttachedHost,
                        spellwake_engine::toolbox::ObjectMutation::SetTapped(true),
                    ));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target_in_context(
                    filter, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target_in_context(
                    filter, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *n,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetOneOf {
                options,
                count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
            } => {
                let bt = lower_target_one_of_permanent_body_target(
                    options, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                let minimum = if *count == TargetCountAst::UpTo(1) {
                    0
                } else {
                    1
                };
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum,
                        maximum: 1,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::ChosenX,
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target_in_context(
                    filter, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                builder.x_target_count = true;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: u16::MAX,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::Group(f)
                if f.zone.is_none() && f.controller == Some(ControllerRelation::You) =>
            {
                let mut inner = (**f).clone();
                inner.controller = None;
                if inner.tapped == Some(false) {
                    inner.tapped = None;
                }
                let obj = lower_object_filter(&inner, vocabulary)?;
                let untapped_obj = match obj {
                    ObjectFilter::Clause(c) => ObjectFilter::Clause(c.untapped()),
                    ObjectFilter::Either(a, b) => ObjectFilter::Either(a.untapped(), b.untapped()),
                };
                builder
                    .instructions
                    .push(Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::with_selection(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                    Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    )),
                                ),
                                untapped_obj,
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
                            spellwake_engine::toolbox::ObjectMutation::SetTapped(true),
                        ),
                    )));
                Ok(())
            }
            SubjectRefAst::Group(f) if f.zone.is_none() => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::UntapOrTapSelect {
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj),
                    count: ValueExpr::literal(1),
                    up_to: false,
                    tap: true,
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
                let (minimum, maximum) = builder
                    .instructions
                    .iter()
                    .rev()
                    .find_map(|op| match op {
                        Instruction::AffectPermanentTargets {
                            minimum, maximum, ..
                        } => Some((*minimum, *maximum)),
                        _ => None,
                    })
                    .unwrap_or((1, 1));
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum,
                        maximum,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.last_group_filter.is_some() => {
                let obj = builder
                    .last_group_filter
                    .ok_or("oracle:unsupported-tap-perm")?;
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(obj, true));
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if last_instruction_created_tokens(&builder.instructions) =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::CreatedTokens,
                        true,
                    ));
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::LastMoved,
                        true,
                    ));
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::TriggerSubject,
                        true,
                    ));
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                    && builder.target == BodyTarget::None =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::TriggerSubject,
                        true,
                    ));
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
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(obj, true));
                Ok(())
            }
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for it in items {
                    lower_instruction(
                        &InstructionAst::TapPermanent {
                            subject: Box::new(it.clone()),
                            any_number: false,
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
            _ => Err("oracle:unsupported-tap-perm"),
        },
        InstructionAst::UntapPermanent {
            subject,
            count,
            up_to,
        } if count.is_some() || *up_to => {
            let filter_ast = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if f.zone.is_none() =>
                {
                    f
                }
                _ => return Err("oracle:unsupported-untap-perm"),
            };
            let obj = lower_object_filter_in_context(filter_ast, builder, in_trigger, vocabulary)?;
            bind_filter_player_target(builder, &obj)?;
            let cnt = match count {
                Some(c) => lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                None => ValueExpr::literal(1),
            };
            builder.instructions.push(Instruction::UntapOrTapSelect {
                chooser: ResolvingPlayer::Controller,
                filter: Box::new(obj),
                count: cnt,
                up_to: *up_to,
                tap: false,
            });
            Ok(())
        }
        InstructionAst::UntapPermanent { subject, .. } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::untap_ability_source());
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(
                        ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                        false,
                    ));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::ChosenX,
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.x_target_count = true;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: u16::MAX,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::Group(f)
                if f.zone.is_none() && f.controller == Some(ControllerRelation::You) =>
            {
                let mut inner = (**f).clone();
                inner.controller = None;
                if inner.tapped == Some(true) {
                    inner.tapped = None;
                }
                let obj = lower_object_filter(&inner, vocabulary)?;
                let tapped_obj = match obj {
                    ObjectFilter::Clause(c) => ObjectFilter::Clause(c.tapped()),
                    ObjectFilter::Either(a, b) => ObjectFilter::Either(a.tapped(), b.tapped()),
                };
                builder
                    .instructions
                    .push(Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::with_selection(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                    Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    )),
                                ),
                                tapped_obj,
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
                            spellwake_engine::toolbox::ObjectMutation::SetTapped(false),
                        ),
                    )));
                Ok(())
            }
            SubjectRefAst::Group(f) if f.zone.is_none() => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::UntapOrTapSelect {
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj),
                    count: ValueExpr::literal(1),
                    up_to: false,
                    tap: false,
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
                let (minimum, maximum) = builder
                    .instructions
                    .iter()
                    .rev()
                    .find_map(|op| match op {
                        Instruction::AffectPermanentTargets {
                            minimum, maximum, ..
                        } => Some((*minimum, *maximum)),
                        _ => None,
                    })
                    .unwrap_or((1, 1));
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum,
                        maximum,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.last_group_filter.is_some() => {
                let obj = builder
                    .last_group_filter
                    .ok_or("oracle:unsupported-untap-perm")?;
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(obj, false));
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if last_instruction_created_tokens(&builder.instructions) =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::CreatedTokens,
                        false,
                    ));
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::LastMoved,
                        false,
                    ));
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::TriggerSubject,
                        false,
                    ));
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if in_trigger
                    && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                    && builder.target == BodyTarget::None =>
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_subject_tapped(
                        EffectSubject::TriggerSubject,
                        false,
                    ));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *n,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetOneOf {
                options,
                count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
            } => {
                let bt = lower_target_one_of_permanent_body_target(
                    options, builder, in_trigger, vocabulary,
                )?;
                builder.bind_target(bt)?;
                let minimum = if *count == TargetCountAst::UpTo(1) {
                    0
                } else {
                    1
                };
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum,
                        maximum: 1,
                        action: PermanentTargetAction::Untap,
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
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if let Some((kind, maximum)) = parse_untap_up_to_phrase(raw) =>
            {
                builder
                    .instructions
                    .push(Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::with_selection(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::from_clause(
                                    ObjectClause::any()
                                        .any_types(CardTypeMask::from_card_type(kind)),
                                ),
                            ),
                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                                min: Box::new(ValueExpr::literal(0)),
                                max: Box::new(ValueExpr::literal(i64::from(maximum))),
                                constraint: None,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                hidden: false,
                            },
                            spellwake_engine::toolbox::ObjectMutation::SetTapped(false),
                        ),
                    )));
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                if f.controller == Some(ControllerRelation::You)
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
                let filter = ObjectFilter::land().controlled();
                builder.last_group_filter = Some(filter);
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(filter, false));
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                if f.controller == Some(ControllerRelation::You)
                    && f.any_types == AstCardTypeMask::CREATURE
                    && filter_only_has(
                        f,
                        FilterAllow {
                            types: true,
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    ) =>
            {
                let filter = ObjectFilter::creature().controlled();
                builder.last_group_filter = Some(filter);
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(filter, false));
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                if f.zone.is_none() =>
            {
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder
                    .instructions
                    .push(super::super::toolbox_emit::set_group_tapped(obj, false));
                Ok(())
            }
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for it in items {
                    lower_instruction(
                        &InstructionAst::UntapPermanent {
                            subject: Box::new(it.clone()),
                            count: None,
                            up_to: false,
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
            _ => Err("oracle:unsupported-untap-perm"),
        },
        InstructionAst::TapOrUntapPermanent { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        spellwake_engine::toolbox::ObjectMutation::ToggleTapped,
                    ));
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::AttachedHost,
                        spellwake_engine::toolbox::ObjectMutation::ToggleTapped,
                    ));
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
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action: PermanentTargetAction::ToggleTap,
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
                        action: PermanentTargetAction::ToggleTap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *n,
                        action: PermanentTargetAction::ToggleTap,
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
                        action: PermanentTargetAction::ToggleTap,
                    });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                let obj = lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder
                    .instructions
                    .push(super::super::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            obj,
                        ),
                        spellwake_engine::toolbox::ObjectMutation::ToggleTapped,
                    ));
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
                    .push(super::super::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ObjectMutation::ToggleTapped,
                    ));
                Ok(())
            }
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                        spellwake_engine::toolbox::ObjectMutation::ToggleTapped,
                    ));
                Ok(())
            }
            _ => Err("oracle:unsupported-toggle-tap"),
        },
        InstructionAst::SkipNextUntap { subject } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::exert_source());
                return Ok(());
            }
            if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) {
                if let Some(Instruction::AffectPermanentTargets {
                    action: action @ PermanentTargetAction::Tap,
                    ..
                }) = builder.instructions.last_mut()
                {
                    *action = PermanentTargetAction::TapAndSkipNextUntap;
                    return Ok(());
                }
                if matches!(
                    builder.target,
                    BodyTarget::SingleCreature
                        | BodyTarget::SinglePermanent
                        | BodyTarget::SinglePlaneswalker
                        | BodyTarget::FilteredPermanent(_)
                ) {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::skip_next_untap(
                            EffectSubject::TargetObject,
                        ));
                    return Ok(());
                }
                if in_trigger {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::skip_next_untap(
                            EffectSubject::TriggerSubject,
                        ));
                    return Ok(());
                }
            }
            match subject.as_ref() {
                SubjectRefAst::TargetObject { filter, count } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    let (min, max) = match count {
                        TargetCountAst::Exact(n) if *n > 0 => (*n, *n),
                        TargetCountAst::UpTo(n) if *n > 0 => (0, *n),
                        TargetCountAst::AnyNumber => (0, u16::MAX),
                        _ => return Err("oracle:unsupported-skip-untap"),
                    };
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: min,
                            maximum: max,
                            action: PermanentTargetAction::SkipNextUntap,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    let min = if *count == TargetCountAst::UpTo(1) {
                        0
                    } else {
                        1
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: min,
                            maximum: 1,
                            action: PermanentTargetAction::SkipNextUntap,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::skip_next_untap(
                            EffectSubject::Matching(obj),
                        ));
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::skip_next_untap(
                            EffectSubject::Matching(ObjectFilter::any().attached_to_self()),
                        ));
                    Ok(())
                }
                _ => Err("oracle:unsupported-skip-untap"),
            }
        }
        InstructionAst::Counter {
            target,
            unless_pay,
            unless_action: None,
        } => {
            if unless_pay.is_none()
                && let Some(ops) = try_lower_counter_matching_stack_objects(target, vocabulary)?
            {
                builder.instructions.extend(ops);
                return Ok(());
            }
            if in_trigger
                && builder.target == BodyTarget::None
                && (matches!(target.as_ref(), SubjectRefAst::TriggerSubject)
                    || matches!(target.as_ref(), SubjectRefAst::Anaphora(raw) if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "that spell" | "that spell or ability" | "that ability" | "it"
                    )))
            {
                if let Some(cost_atom) = unless_pay {
                    match cost_atom {
                        CostAtomAst::PayMana(raw) => {
                            let idx = builder.instructions.len();
                            if let Some(cost) = parse_fixed_mana_cost(raw) {
                                builder.instructions.push(Instruction::OfferManaPayment {
                                    payer: ResolvingPlayer::TriggerSubjectController,
                                    cost,
                                    paid_destination: idx + 2,
                                    declined_destination: idx + 1,
                                });
                            } else if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
                                && x_cnt > 0
                            {
                                let x_val = match bound_x {
                                    Some(expr) => lower_value_expr_in_context(
                                        expr, builder, in_trigger, vocabulary,
                                    )?,
                                    None => ValueExpr::chosen_x(),
                                };
                                let extra_generic = scale_value_expr(x_val, i64::from(x_cnt));
                                builder
                                    .instructions
                                    .push(Instruction::OfferDynamicManaPayment {
                                        payer: ResolvingPlayer::TriggerSubjectController,
                                        fixed,
                                        extra_generic,
                                        paid_destination: idx + 2,
                                        declined_destination: idx + 1,
                                    });
                            } else {
                                return Err("oracle:counter-unless-x");
                            }
                            builder
                                .instructions
                                .push(Instruction::CounterTriggeringStackObject);
                            return Ok(());
                        }
                        CostAtomAst::PayGenericManaExpr(_) | CostAtomAst::PayScaledMana { .. } => {
                            let (fixed, extra_generic) = lower_counter_unless_dynamic_mana(
                                cost_atom, builder, in_trigger, vocabulary, bound_x,
                            )?;
                            let idx = builder.instructions.len();
                            builder
                                .instructions
                                .push(Instruction::OfferDynamicManaPayment {
                                    payer: ResolvingPlayer::TriggerSubjectController,
                                    fixed,
                                    extra_generic,
                                    paid_destination: idx + 2,
                                    declined_destination: idx + 1,
                                });
                            builder
                                .instructions
                                .push(Instruction::CounterTriggeringStackObject);
                            return Ok(());
                        }
                        _ => return Err("oracle:counter-unless-non-mana"),
                    }
                }
                builder
                    .instructions
                    .push(Instruction::CounterTriggeringStackObject);
                return Ok(());
            }
            let reuse_bound_spell = matches!(
                builder.target,
                BodyTarget::SingleSpell(_)
                    | BodyTarget::SingleStackObject(_)
                    | BodyTarget::SingleStackEntry
            ) && matches!(
                target.as_ref(),
                SubjectRefAst::Anaphora(raw) if matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "that spell"
                        | "that spell or ability"
                        | "that ability"
                        | "the chosen spell"
                        | "the chosen spell or ability"
                        | "the chosen ability"
                        | "it"
                )
            );
            if !reuse_bound_spell {
                let bt = lower_target_subject(target, vocabulary)?;
                builder.bind_target(bt)?;
            }
            if let Some(cost_atom) = unless_pay {
                match cost_atom {
                    CostAtomAst::PayMana(raw) => {
                        let idx = builder.instructions.len();
                        if let Some(cost) = parse_fixed_mana_cost(raw) {
                            builder.instructions.push(Instruction::OfferManaPayment {
                                payer: ResolvingPlayer::TargetSpellController,
                                cost,
                                paid_destination: idx + 2,
                                declined_destination: idx + 1,
                            });
                        } else if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
                            && x_cnt > 0
                        {
                            let x_val = match bound_x {
                                Some(expr) => lower_value_expr_in_context(
                                    expr, builder, in_trigger, vocabulary,
                                )?,
                                None => ValueExpr::chosen_x(),
                            };
                            let extra_generic = scale_value_expr(x_val, i64::from(x_cnt));
                            builder
                                .instructions
                                .push(Instruction::OfferDynamicManaPayment {
                                    payer: ResolvingPlayer::TargetSpellController,
                                    fixed,
                                    extra_generic,
                                    paid_destination: idx + 2,
                                    declined_destination: idx + 1,
                                });
                        } else {
                            return Err("oracle:counter-unless-x");
                        }
                        builder.instructions.push(Instruction::CounterSpell);
                        Ok(())
                    }
                    CostAtomAst::PayGenericManaExpr(_) | CostAtomAst::PayScaledMana { .. } => {
                        let (fixed, extra_generic) = lower_counter_unless_dynamic_mana(
                            cost_atom, builder, in_trigger, vocabulary, bound_x,
                        )?;
                        let idx = builder.instructions.len();
                        builder
                            .instructions
                            .push(Instruction::OfferDynamicManaPayment {
                                payer: ResolvingPlayer::TargetSpellController,
                                fixed,
                                extra_generic,
                                paid_destination: idx + 2,
                                declined_destination: idx + 1,
                            });
                        builder.instructions.push(Instruction::CounterSpell);
                        Ok(())
                    }
                    other => {
                        let pay_ops = lower_resolution_pay_cost_ops(
                            ResolvingPlayer::TargetSpellController,
                            other,
                            vocabulary,
                            bound_x,
                        )
                        .map_err(|_| "oracle:counter-unless-non-mana")?;
                        if pay_ops.len() == 1 {
                            builder.instructions.push(Instruction::OptionalEffect {
                                chooser: ResolvingPlayer::TargetSpellController,
                            });
                        } else {
                            builder.instructions.push(Instruction::OptionalSequence {
                                chooser: ResolvingPlayer::TargetSpellController,
                                instructions: pay_ops.len(),
                            });
                        }
                        builder.instructions.extend(pay_ops);
                        let branch_idx = builder.instructions.len();
                        builder.instructions.push(Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                                comparison: Comparison::Less,
                                right: ValueExpr::literal(1),
                            },
                            destination: branch_idx + 2,
                        });
                        builder.instructions.push(Instruction::CounterSpell);
                        Ok(())
                    }
                }
            } else {
                builder.instructions.push(Instruction::CounterSpell);
                Ok(())
            }
        }
        InstructionAst::Counter {
            target,
            unless_pay: None,
            unless_action: Some(act),
        } => {
            let is_trigger_target = in_trigger
                && builder.target == BodyTarget::None
                && matches!(target.as_ref(), SubjectRefAst::Anaphora(raw) if matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "that spell" | "that spell or ability" | "that ability" | "it"
                ));
            let chooser = if is_trigger_target {
                ResolvingPlayer::TriggerSubjectController
            } else {
                let bt = lower_target_subject(target, vocabulary)?;
                builder.bind_target(bt)?;
                ResolvingPlayer::TargetSpellController
            };
            let act_slice: &[InstructionAst] = match act.as_slice() {
                [InstructionAst::Optional(inner)] => inner.as_slice(),
                [InstructionAst::OptionalPlayer { body, .. }] => body.as_slice(),
                other => other,
            };
            let mut act_builder = BodyBuilder::new();
            act_builder.target = builder.target;
            act_builder.has_source = builder.has_source || in_trigger || self_pronoun_valid;
            lower_instruction_seq(
                act_slice,
                &mut act_builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if act_builder.target != builder.target
                || act_builder.instructions.is_empty()
                || act_builder.graveyard_target_bounds.is_some()
                || act_builder.x_target_count
                || !act_builder
                    .instructions
                    .iter()
                    .all(is_allowed_inside_optional_effect)
            {
                return Err("oracle:counter-unless-action");
            }
            if act_builder.instructions.len() == 1 {
                builder
                    .instructions
                    .push(Instruction::OptionalEffect { chooser });
            } else {
                builder.instructions.push(Instruction::OptionalSequence {
                    chooser,
                    instructions: act_builder.instructions.len(),
                });
            }
            builder.instructions.extend(act_builder.instructions);
            let branch_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: Comparison::Less,
                    right: ValueExpr::literal(1),
                },
                destination: branch_idx + 2,
            });
            builder.instructions.push(if is_trigger_target {
                Instruction::CounterTriggeringStackObject
            } else {
                Instruction::CounterSpell
            });
            Ok(())
        }
        InstructionAst::AddMana { player, mana } => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            let mut trial = builder.clone();
            let group_players = try_group_affected_players(*player);
            let target_player: Option<ResolvingPlayer> = if *player == PlayerRefAst::You {
                None
            } else if group_players.is_some() {
                Some(ResolvingPlayer::LoopPlayer)
            } else {
                Some(lower_resolving_player_in_context(
                    *player, &mut trial, in_trigger,
                )?)
            };
            let make_add = |yield_kind: EffectManaYield| -> Instruction {
                match target_player {
                    None => Instruction::AddMana(yield_kind),
                    Some(rp) => Instruction::AddManaForPlayer {
                        player: rp,
                        yield_kind,
                    },
                }
            };
            let wubrg = || -> BTreeSet<ManaType> {
                [
                    ManaType::White,
                    ManaType::Blue,
                    ManaType::Black,
                    ManaType::Red,
                    ManaType::Green,
                ]
                .into_iter()
                .collect()
            };
            let emitted_op = match mana {
                ManaSpecAst::Fixed(raw) => {
                    let bundle = parse_mana_bundle(raw).ok_or("oracle:bad-mana-bundle")?;
                    make_add(EffectManaYield::Fixed(bundle))
                }
                ManaSpecAst::ScaledSymbol { symbol, amount } => {
                    let amt = lower_value_expr_in_context(
                        resolve_x(amount, bound_x),
                        &trial,
                        in_trigger,
                        vocabulary,
                    )?;
                    if let Some(kind) = parse_single_mana_type(symbol) {
                        make_add(EffectManaYield::Counted { kind, amount: amt })
                    } else if let Some(bundle) = parse_mana_bundle(symbol) {
                        Instruction::RepeatLoop {
                            count: amt,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![make_add(EffectManaYield::Fixed(bundle))],
                            )),
                        }
                    } else {
                        return Err("oracle:bad-mana-sym");
                    }
                }
                ManaSpecAst::CombinationOf { amount, symbols } if symbols.len() >= 2 => {
                    let amt = lower_value_expr_in_context(
                        resolve_x(amount, bound_x),
                        &trial,
                        in_trigger,
                        vocabulary,
                    )?;
                    if symbols.len() == 2 {
                        let first =
                            parse_single_mana_type(&symbols[0]).ok_or("oracle:bad-mana-sym")?;
                        let second =
                            parse_single_mana_type(&symbols[1]).ok_or("oracle:bad-mana-sym")?;
                        if first == second {
                            return Err("oracle:unsupported-effect-mana");
                        }
                        make_add(EffectManaYield::Split {
                            first,
                            second,
                            amount: amt,
                        })
                    } else {
                        let mut colors = BTreeSet::new();
                        for s in symbols {
                            colors.insert(parse_single_mana_type(s).ok_or("oracle:bad-mana-sym")?);
                        }
                        Instruction::RepeatLoop {
                            count: amt,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![make_add(EffectManaYield::Choice {
                                    colors,
                                    amount: ValueExpr::literal(1),
                                })],
                            )),
                        }
                    }
                }
                ManaSpecAst::Choice(opts) if opts.len() >= 2 => {
                    let mut flattened: Vec<String> = Vec::new();
                    for raw_opt in opts {
                        let cleaned = raw_opt.trim().trim_end_matches('.');
                        if cleaned.contains(" or ") {
                            for seg in cleaned.split(", or ").flat_map(|s| s.split(" or ")) {
                                for sub in seg.split(',') {
                                    let t = sub.trim().trim_start_matches("or ").trim();
                                    if !t.is_empty() {
                                        flattened.push(t.to_owned());
                                    }
                                }
                            }
                        } else if !cleaned.is_empty() {
                            flattened.push(cleaned.to_owned());
                        }
                    }
                    if flattened.len() < 2 {
                        return Err("oracle:bad-mana-sym");
                    }
                    let all_single: Option<BTreeSet<ManaType>> = flattened
                        .iter()
                        .map(|opt| parse_single_mana_type(opt))
                        .collect();
                    if let Some(colors) = all_single
                        && !colors.is_empty()
                    {
                        make_add(EffectManaYield::Choice {
                            colors,
                            amount: ValueExpr::literal(1),
                        })
                    } else {
                        let parse_counted_mana_opt =
                            |opt: &str| -> Option<(ManaType, ValueExprAst)> {
                                let trimmed = opt.trim();
                                let (qty_str, sym_str) = trimmed.rsplit_once(' ')?;
                                let mt = parse_single_mana_type(sym_str.trim())?;
                                let qty_low = qty_str.trim().to_ascii_lowercase();
                                if qty_low == "x" {
                                    return Some((mt, ValueExprAst::ChosenX));
                                }
                                let n = parse_small_number_word(&qty_low)?;
                                if n > 0 {
                                    Some((mt, ValueExprAst::Const(i32::from(n))))
                                } else {
                                    None
                                }
                            };
                        let all_counted: Option<Vec<(ManaType, ValueExprAst)>> = flattened
                            .iter()
                            .map(|opt| parse_counted_mana_opt(opt))
                            .collect();
                        if let Some(pairs) = all_counted
                            && let Some((_, first_amt)) = pairs.first()
                            && pairs.iter().all(|(_, a)| a == first_amt)
                        {
                            let mut colors = BTreeSet::new();
                            for (mt, _) in &pairs {
                                colors.insert(*mt);
                            }
                            let amount = lower_value_expr_in_context(
                                resolve_x(first_amt, bound_x),
                                &trial,
                                in_trigger,
                                vocabulary,
                            )?;
                            make_add(EffectManaYield::Choice { colors, amount })
                        } else {
                            let mut bundles: Vec<BTreeMap<ManaType, RulesInteger>> = Vec::new();
                            for opt in &flattened {
                                let low = opt.trim().to_ascii_lowercase();
                                if matches!(
                                    low.as_str(),
                                    "one mana of any color"
                                        | "a mana of any color"
                                        | "mana of any color"
                                        | "any color"
                                ) {
                                    for mt in wubrg() {
                                        let b = BTreeMap::from([(mt, RulesInteger::from(1))]);
                                        if !bundles.contains(&b) {
                                            bundles.push(b);
                                        }
                                    }
                                    continue;
                                }
                                if let Some((mt, ValueExprAst::Const(n))) =
                                    parse_counted_mana_opt(opt)
                                    && n > 0
                                {
                                    let b =
                                        BTreeMap::from([(mt, RulesInteger::from(i64::from(n)))]);
                                    if !bundles.contains(&b) {
                                        bundles.push(b);
                                    }
                                    continue;
                                }
                                let b = parse_mana_bundle(opt).ok_or("oracle:bad-mana-sym")?;
                                if !bundles.contains(&b) {
                                    bundles.push(b);
                                }
                            }
                            let uniform_single = bundles.first().and_then(|first| {
                                if first.len() != 1 {
                                    return None;
                                }
                                let qty = first.values().next()?.clone();
                                let mut colors = BTreeSet::new();
                                for b in &bundles {
                                    if b.len() != 1 || b.values().next() != Some(&qty) {
                                        return None;
                                    }
                                    colors.insert(*b.keys().next()?);
                                }
                                Some((colors, qty))
                            });
                            if let Some((colors, qty)) = uniform_single {
                                make_add(EffectManaYield::Choice {
                                    colors,
                                    amount: ValueExpr::Literal(qty),
                                })
                            } else if bundles.len() >= 2 {
                                make_add(EffectManaYield::ChoiceBundle { options: bundles })
                            } else {
                                return Err("oracle:bad-mana-sym");
                            }
                        }
                    }
                }
                ManaSpecAst::AnyColor {
                    amount,
                    single_color,
                } => {
                    let amt = lower_value_expr_in_context(
                        resolve_x(amount, bound_x),
                        &trial,
                        in_trigger,
                        vocabulary,
                    )?;
                    if *single_color {
                        make_add(EffectManaYield::Choice {
                            colors: wubrg(),
                            amount: amt,
                        })
                    } else {
                        Instruction::RepeatLoop {
                            count: amt,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![make_add(EffectManaYield::Choice {
                                    colors: wubrg(),
                                    amount: ValueExpr::literal(1),
                                })],
                            )),
                        }
                    }
                }
                ManaSpecAst::AnyLandCouldProduce { controller } => {
                    let players = match controller {
                        ControllerRelation::You => {
                            AffectedPlayers::One(ResolvingPlayer::Controller)
                        }
                        ControllerRelation::Opponent | ControllerRelation::Opponents => {
                            AffectedPlayers::EachOpponent
                        }
                        _ => return Err("oracle:unsupported-effect-mana"),
                    };
                    make_add(EffectManaYield::LandProducible {
                        controller: players,
                        amount: ValueExpr::literal(1),
                    })
                }
                ManaSpecAst::CommanderColorIdentity => {
                    make_add(EffectManaYield::CommanderIdentity {
                        amount: ValueExpr::literal(1),
                    })
                }
                ManaSpecAst::ChosenColor if has_source => {
                    make_add(EffectManaYield::SourceChosenColor {
                        amount: ValueExpr::literal(1),
                    })
                }
                ManaSpecAst::DifferentColors { amount } => {
                    let amt = lower_value_expr_in_context(
                        resolve_x(amount, bound_x),
                        &trial,
                        in_trigger,
                        vocabulary,
                    )?;
                    make_add(EffectManaYield::DistinctColors { amount: amt })
                }
                _ => return Err("oracle:unsupported-effect-mana"),
            };
            if let Some(players) = group_players {
                push_for_each_player(players, vec![emitted_op], &mut trial);
            } else {
                trial.instructions.push(emitted_op);
            }
            *builder = trial;
            Ok(())
        }
        InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject,
            duration,
        } => {
            let eff_subject =
                unwrap_in_zone_subject(subject).unwrap_or_else(|| (**subject).clone());
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = &eff_subject
                && filter.any_spell
            {
                let bt = lower_target_subject(&eff_subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::GainControlOfSpell);
                return Ok(());
            }
            let dur = lower_duration(duration)?;
            let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                spellwake_engine::characteristics::ControlRecipient::ResolvingController,
            )]);
            match &eff_subject {
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    if let Some((minimum, maximum)) =
                        builder.instructions.iter().rev().find_map(|op| match op {
                            Instruction::AffectPermanentTargets {
                                minimum, maximum, ..
                            } => Some((*minimum, *maximum)),
                            _ => None,
                        })
                    {
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum,
                                maximum,
                                action: PermanentTargetAction::Modify {
                                    parts,
                                    duration: dur,
                                },
                            });
                    } else {
                        builder.instructions.push(
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                parts,
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::CreatedTokens,
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::LastMoved,
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::Matching(obj),
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::TriggerSubject,
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::TriggerSubject,
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::Matching(ObjectFilter::Clause(
                                    ObjectClause::any().attached_to_self(),
                                )),
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !filter.any_spell
                        && !filter.is_ability =>
                {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    match count {
                        TargetCountAst::Exact(1) => {
                            builder.bind_target(bt)?;
                            builder.instructions.push(
                                super::super::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    parts,
                                    dur,
                                ),
                            );
                            Ok(())
                        }
                        TargetCountAst::Exact(n) if *n >= 2 => {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: *n,
                                    maximum: *n,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        TargetCountAst::UpTo(n) if *n > 0 => {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: *n,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        TargetCountAst::ChosenX | TargetCountAst::UpToX if !in_trigger => {
                            builder.bind_target(bt)?;
                            if *count == TargetCountAst::UpToX {
                                builder.up_to_x_target_count = true;
                            } else {
                                builder.x_target_count = true;
                            }
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: u16::MAX,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        TargetCountAst::AnyNumber => {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: u16::MAX,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        _ => Err("oracle:unsupported-target-subject"),
                    }
                }
                SubjectRefAst::TargetOneOf { options, count } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    match count {
                        TargetCountAst::Exact(1) => {
                            builder.bind_target(bt)?;
                            builder.instructions.push(
                                super::super::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    parts,
                                    dur,
                                ),
                            );
                            Ok(())
                        }
                        TargetCountAst::Exact(n) if *n >= 2 => {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: *n,
                                    maximum: *n,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        TargetCountAst::UpTo(n) if *n > 0 => {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: *n,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        TargetCountAst::ChosenX | TargetCountAst::UpToX if !in_trigger => {
                            builder.bind_target(bt)?;
                            if *count == TargetCountAst::UpToX {
                                builder.up_to_x_target_count = true;
                            } else {
                                builder.x_target_count = true;
                            }
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: u16::MAX,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        TargetCountAst::AnyNumber => {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 0,
                                    maximum: u16::MAX,
                                    action: PermanentTargetAction::Modify {
                                        parts,
                                        duration: dur,
                                    },
                                });
                            Ok(())
                        }
                        _ => Err("oracle:unsupported-target-subject"),
                    }
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::Matching(obj),
                            )),
                            parts,
                            dur,
                        ));
                    Ok(())
                }
                _ => {
                    let bt = lower_target_subject(&eff_subject, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            parts,
                            dur,
                        ));
                    Ok(())
                }
            }
        }
        InstructionAst::GainControl {
            controller: ctrl @ (PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent),
            subject,
            duration: DurationAst::Permanent,
        } if is_self_pronoun(subject, builder, self_pronoun_valid) => {
            let bt = if matches!(ctrl, PlayerRefAst::TargetOpponent) {
                BodyTarget::SingleOpponent
            } else {
                BodyTarget::SinglePlayer
            };
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(super::super::toolbox_emit::gain_control(
                    spellwake_engine::toolbox::ObjectQuery::Source,
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    ),
                    ResolvedDuration::NoStatedDuration,
                ));
            Ok(())
        }
        InstructionAst::GainControl {
            controller: ctrl @ (PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent),
            subject,
            duration,
        } if builder.target == BodyTarget::None
            && builder.instructions.is_empty()
            && matches!(
                subject.as_ref(),
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none()
            ) =>
        {
            let dur = lower_duration(duration)?;
            let player_bt = if matches!(ctrl, PlayerRefAst::TargetOpponent) {
                BodyTarget::SingleOpponent
            } else {
                BodyTarget::SinglePlayer
            };
            let SubjectRefAst::TargetObject { filter, .. } = subject.as_ref() else {
                return Err("oracle:unsupported-gain-control");
            };
            let perm_bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder
                .instructions
                .push(Instruction::FixedTargetClause(Box::new(
                    ExecutableBody::new(
                        player_bt,
                        vec![super::super::toolbox_emit::draw_target_player(
                            ValueExpr::literal(0),
                        )],
                    ),
                )));
            builder
                .instructions
                .push(Instruction::FixedTargetClause(Box::new(
                    ExecutableBody::new(perm_bt, vec![Instruction::RememberPermanentTarget]),
                )));
            builder
                .instructions
                .push(Instruction::GiveFixedTargetControl { duration: dur });
            Ok(())
        }
        InstructionAst::GainControl {
            controller,
            subject,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let group_players = try_group_affected_players(*controller);
            let rp = if group_players.is_some() {
                ResolvingPlayer::LoopPlayer
            } else {
                lower_resolving_player_in_context(*controller, builder, in_trigger)?
            };
            let eff_subject =
                unwrap_in_zone_subject(subject).unwrap_or_else(|| (**subject).clone());
            let eff_subj = match &eff_subject {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    EffectSubject::AbilitySource
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => EffectSubject::Matching(
                    ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                ),
                SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    EffectSubject::TriggerSubject
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
                    EffectSubject::TargetObject
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_created_tokens(&builder.instructions) =>
                {
                    EffectSubject::CreatedTokens
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    EffectSubject::LastMoved
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(obj) = builder.last_group_filter =>
                {
                    EffectSubject::Matching(obj)
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
                    EffectSubject::TargetObject
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    EffectSubject::TargetObject
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    EffectSubject::Matching(obj)
                }
                _ => return Err("oracle:unsupported-gain-control"),
            };
            let op = super::super::toolbox_emit::gain_control_of_subject(
                eff_subj,
                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                dur,
            );
            if let Some(players) = group_players {
                push_for_each_player(players, vec![op], builder);
            } else {
                builder.instructions.push(op);
            }
            Ok(())
        }
        InstructionAst::ChangeTargets {
            target,
            new_target: None,
            must_target: None,
            single_target_only,
        } => {
            if *single_target_only {
                if matches!(target.as_ref(), SubjectRefAst::Anaphora(_))
                    && is_stack_body_target(builder.target)
                {
                    builder.instructions.push(Instruction::ChangeOnlyTarget);
                    return Ok(());
                }
                let bt = if let SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } = target.as_ref()
                    && filter.flags.0 & AstFilterFlags::SINGLE_TARGET.0 != 0
                {
                    let mut inner = filter.as_ref().clone();
                    inner.flags.0 &= !AstFilterFlags::SINGLE_TARGET.0;
                    lower_target_subject(
                        &SubjectRefAst::TargetObject {
                            filter: Box::new(inner),
                            count: TargetCountAst::Exact(1),
                        },
                        vocabulary,
                    )?
                } else {
                    lower_target_subject(target, vocabulary)?
                };
                if !is_stack_body_target(bt) {
                    return Err("oracle:unsupported-change-targets");
                }
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::ChangeOnlyTarget);
                return Ok(());
            }
            if matches!(target.as_ref(), SubjectRefAst::Anaphora(_)) {
                if let Some(copy_op) = builder.instructions.iter_mut().rev().find(|op| {
                    matches!(
                        op,
                        Instruction::CopyStackObject { .. }
                            | Instruction::CopySpell
                            | Instruction::CopyAbility
                    )
                }) {
                    match copy_op {
                        Instruction::CopyStackObject {
                            may_choose_new_targets,
                            ..
                        } => {
                            *may_choose_new_targets = true;
                            return Ok(());
                        }
                        Instruction::CopySpell => {
                            *copy_op = Instruction::CopyStackObject {
                                target: StackCopyTarget::TargetSpell,
                                count: ValueExpr::literal(1),
                                may_choose_new_targets: true,
                            };
                            return Ok(());
                        }
                        Instruction::CopyAbility => {
                            *copy_op = Instruction::CopyStackObject {
                                target: StackCopyTarget::TargetAbility,
                                count: ValueExpr::literal(1),
                                may_choose_new_targets: true,
                            };
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                if is_stack_body_target(builder.target) {
                    builder.instructions.push(Instruction::ChooseNewTargets);
                    return Ok(());
                }
            }
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = target.as_ref()
                && filter.flags.0 & AstFilterFlags::SINGLE_TARGET.0 != 0
            {
                let mut inner = filter.as_ref().clone();
                inner.flags.0 &= !AstFilterFlags::SINGLE_TARGET.0;
                let stripped_subj = SubjectRefAst::TargetObject {
                    filter: Box::new(inner),
                    count: TargetCountAst::Exact(1),
                };
                let bt = lower_target_subject(&stripped_subj, vocabulary)?;
                if !is_stack_body_target(bt) {
                    return Err("oracle:unsupported-change-targets");
                }
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::ChangeOnlyTarget);
                return Ok(());
            }
            let bt = lower_target_subject(target, vocabulary)?;
            if !is_stack_body_target(bt) {
                return Err("oracle:unsupported-change-targets");
            }
            builder.bind_target(bt)?;
            builder.instructions.push(Instruction::ChooseNewTargets);
            Ok(())
        }
        InstructionAst::Exchange { kind, left, right } => match kind {
            ExchangeKind::Control => {
                let (a, b) = match (left.as_ref(), right.as_deref()) {
                    (l, Some(r)) => (l, r),
                    (SubjectRefAst::Compound(items), None) if items.len() == 2 => {
                        (&items[0], &items[1])
                    }
                    _ => return Err("oracle:unsupported-exchange"),
                };
                let had_perm_target = is_permanent_body_target(builder.target);
                let first = lower_fight_or_exchange_subject(
                    a,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                    true,
                    had_perm_target,
                )?;
                let second = lower_fight_or_exchange_subject(
                    b,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                    true,
                    had_perm_target,
                )?;
                builder
                    .instructions
                    .push(Instruction::Exchange(ExchangeSpec::Control {
                        first: Box::new(first),
                        second: Box::new(second),
                    }));
                Ok(())
            }
            ExchangeKind::LifeTotals => {
                let (a, b) = match (left.as_ref(), right.as_deref()) {
                    (l, Some(r)) => (l, r),
                    (SubjectRefAst::Compound(items), None) if items.len() == 2 => {
                        (&items[0], &items[1])
                    }
                    _ => return Err("oracle:unsupported-exchange"),
                };
                let first = extract_resolving_player_from_subject(a, builder, in_trigger)?;
                let second = extract_resolving_player_from_subject(b, builder, in_trigger)?;
                builder
                    .instructions
                    .push(Instruction::Exchange(ExchangeSpec::LifeTotals {
                        first,
                        second,
                    }));
                Ok(())
            }
            ExchangeKind::LifeWithStat(stat_kind) => {
                let stat = match stat_kind {
                    StatKind::Power => ObjectStatKind::Power,
                    StatKind::Toughness => ObjectStatKind::Toughness,
                    _ => return Err("oracle:unsupported-exchange"),
                };
                let Some(r) = right.as_deref() else {
                    return Err("oracle:unsupported-exchange");
                };
                let had_perm_target = matches!(
                    builder.target,
                    BodyTarget::SingleCreature
                        | BodyTarget::SinglePermanent
                        | BodyTarget::FilteredPermanent(_)
                );
                let is_player_subj = |s: &SubjectRefAst| {
                    matches!(
                        s,
                        SubjectRefAst::Player(_)
                            | SubjectRefAst::TargetPlayer {
                                count: TargetCountAst::Exact(1),
                                ..
                            }
                    )
                };
                let (player_subj, obj_subj) = if is_player_subj(left.as_ref()) {
                    (left.as_ref(), r)
                } else if is_player_subj(r) {
                    (r, left.as_ref())
                } else {
                    return Err("oracle:unsupported-exchange");
                };
                let player =
                    extract_resolving_player_from_subject(player_subj, builder, in_trigger)?;
                let object = lower_fight_or_exchange_subject(
                    obj_subj,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                    false,
                    had_perm_target,
                )?;
                builder
                    .instructions
                    .push(Instruction::Exchange(ExchangeSpec::LifeWithStat {
                        player,
                        object: Box::new(object),
                        stat,
                    }));
                Ok(())
            }
            ExchangeKind::Zones => {
                let Some(r) = right.as_deref() else {
                    return Err("oracle:unsupported-exchange-zones");
                };
                let (Some(from), Some(to)) = (
                    parse_exchange_zone_from_subject(left.as_ref()),
                    parse_exchange_zone_from_subject(r),
                ) else {
                    return Err("oracle:unsupported-exchange-zones");
                };
                if from == to {
                    return Err("oracle:unsupported-exchange-zones");
                }
                builder
                    .instructions
                    .push(Instruction::Exchange(ExchangeSpec::Zones {
                        player: ResolvingPlayer::Controller,
                        from,
                        to,
                    }));
                Ok(())
            }
        },
        InstructionAst::Discover { value } => {
            let amt = lower_value_expr_in_context(
                resolve_x(value, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder
                .instructions
                .push(Instruction::Discover { value: amt });
            Ok(())
        }
        InstructionAst::Attach {
            subject,
            target,
            any_number,
            all,
        } => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            if matches!(
                target.as_ref(),
                SubjectRefAst::Anaphora(raw) if raw.eq_ignore_ascii_case("unattach")
            ) {
                if *any_number || *all {
                    return Err("oracle:unsupported-attach");
                }
                let mut trial = builder.clone();
                let unattach_subj = match subject.as_ref() {
                    s if has_source && is_self_pronoun(s, &trial, self_pronoun_valid) => {
                        Some(MoveSubjectRef::AbilitySource)
                    }
                    SubjectRefAst::AttachedHost(_) if has_source => {
                        Some(MoveSubjectRef::AttachedHost)
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target_in_context(
                            filter, &trial, in_trigger, vocabulary,
                        )?;
                        let was_none = trial.target == BodyTarget::None;
                        trial.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            if !was_none {
                                return Err("oracle:unsupported-attach");
                            }
                            trial.up_to_one_target = true;
                        }
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&trial.instructions) =>
                    {
                        Some(MoveSubjectRef::LastMoved)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&trial.instructions) =>
                    {
                        Some(MoveSubjectRef::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some(MoveSubjectRef::TriggeringObject)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(MoveSubjectRef::TriggeringObject)
                    }
                    _ => None,
                };
                if let Some(subj) = unattach_subj {
                    trial
                        .instructions
                        .push(super::super::toolbox_emit::unattach_query(
                            super::super::toolbox_emit::move_subject_to_query(subj),
                        ));
                    *builder = trial;
                    return Ok(());
                }
                return Err("oracle:unsupported-attach");
            }
            if !*any_number && !*all {
                if is_self_pronoun(subject, builder, self_pronoun_valid) {
                    if in_trigger
                        && let SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } = target.as_ref()
                        && filter.zone.is_none()
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        if bt
                            == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
                        {
                            builder.bind_target(bt)?;
                            builder
                                .instructions
                                .push(Instruction::Attach(AttachAbility::Equip));
                            return Ok(());
                        }
                    }
                } else if matches!(subject.as_ref(), SubjectRefAst::TriggerSubject)
                    && in_trigger
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } = target.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    if bt == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()) {
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::AttachOnEntry {
                            entering_equipment: true,
                        });
                        return Ok(());
                    }
                }
            }
            let mut trial = builder.clone();
            if !*any_number
                && !*all
                && trial.target == BodyTarget::None
                && trial.instructions.is_empty()
                && let SubjectRefAst::TargetObject {
                    filter: att_f,
                    count: TargetCountAst::Exact(1),
                } = subject.as_ref()
                && att_f.zone.is_none()
            {
                let second_host_bt: Option<BodyTarget> = match target.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter: host_f,
                        count: TargetCountAst::Exact(1),
                    } if host_f.zone.is_none() => {
                        let bt = lower_battlefield_body_target_in_context(
                            host_f, &trial, in_trigger, vocabulary,
                        )?;
                        is_permanent_body_target(bt).then_some(bt)
                    }
                    SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                        Some(BodyTarget::SinglePlayer)
                    }
                    SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                        Some(BodyTarget::SingleOpponent)
                    }
                    _ => None,
                };
                if let Some(host_bt) = second_host_bt {
                    let att_bt = lower_battlefield_body_target_in_context(
                        att_f, &trial, in_trigger, vocabulary,
                    )?;
                    if is_permanent_body_target(att_bt) {
                        trial
                            .instructions
                            .push(Instruction::FixedTargetClause(Box::new(
                                ExecutableBody::new(
                                    att_bt,
                                    vec![Instruction::RememberPermanentTarget],
                                ),
                            )));
                        trial
                            .instructions
                            .push(Instruction::FixedTargetClause(Box::new(
                                ExecutableBody::new(
                                    host_bt,
                                    vec![Instruction::RememberPermanentTarget],
                                ),
                            )));
                        trial.instructions.push(Instruction::AttachFixedTargets);
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            let prior_target = trial.target;
            let resolve_fixed_att = |subj: &SubjectRefAst,
                                     b: &mut BodyBuilder|
             -> Result<Option<MoveSubjectRef>, &'static str> {
                match subj {
                    s if has_source && is_self_pronoun(s, b, self_pronoun_valid) => {
                        Ok(Some(MoveSubjectRef::AbilitySource))
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target_in_context(
                            filter, b, in_trigger, vocabulary,
                        )?;
                        let was_none = b.target == BodyTarget::None;
                        b.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            if !was_none {
                                return Err("oracle:unsupported-attach");
                            }
                            b.up_to_one_target = true;
                        }
                        Ok(Some(MoveSubjectRef::TargetObject))
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_permanent_body_target(prior_target)
                            || is_graveyard_body_target(prior_target) =>
                    {
                        Ok(Some(MoveSubjectRef::TargetObject))
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&b.instructions) =>
                    {
                        Ok(Some(MoveSubjectRef::CreatedTokens))
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&b.instructions) =>
                    {
                        Ok(Some(MoveSubjectRef::LastMoved))
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Ok(Some(MoveSubjectRef::TriggeringObject))
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Ok(Some(MoveSubjectRef::TriggeringObject))
                    }
                    SubjectRefAst::AttachedHost(_) if has_source => {
                        Ok(Some(MoveSubjectRef::AttachedHost))
                    }
                    _ => Ok(None),
                }
            };
            let resolve_fixed_host = |subj: &SubjectRefAst,
                                      b: &mut BodyBuilder|
             -> Result<Option<MoveSubjectRef>, &'static str> {
                let self_anaphora = match subj {
                    SubjectRefAst::SelfObject => true,
                    SubjectRefAst::Anaphora(p)
                        if self_pronoun_valid && prior_target == BodyTarget::None =>
                    {
                        matches!(
                            p.to_ascii_lowercase().as_str(),
                            "it" | "him"
                                | "her"
                                | "he"
                                | "she"
                                | "this creature"
                                | "this permanent"
                                | "this artifact"
                                | "this enchantment"
                                | "this land"
                        )
                    }
                    _ => false,
                };
                match subj {
                    _ if has_source && self_anaphora => Ok(Some(MoveSubjectRef::AbilitySource)),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target_in_context(
                            filter, b, in_trigger, vocabulary,
                        )?;
                        let was_none = b.target == BodyTarget::None;
                        b.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            if !was_none {
                                return Err("oracle:unsupported-attach");
                            }
                            b.up_to_one_target = true;
                        }
                        Ok(Some(MoveSubjectRef::TargetObject))
                    }
                    SubjectRefAst::Anaphora(_)
                        if prior_target != BodyTarget::None
                            && !is_stack_body_target(prior_target) =>
                    {
                        Ok(Some(MoveSubjectRef::TargetObject))
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&b.instructions) =>
                    {
                        Ok(Some(MoveSubjectRef::CreatedTokens))
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&b.instructions) =>
                    {
                        Ok(Some(MoveSubjectRef::LastMoved))
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Ok(Some(MoveSubjectRef::TriggeringObject))
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Ok(Some(MoveSubjectRef::TriggeringObject))
                    }
                    SubjectRefAst::AttachedHost(_) if has_source => {
                        Ok(Some(MoveSubjectRef::AttachedHost))
                    }
                    _ => Ok(None),
                }
            };
            if !*any_number && !*all {
                if let SubjectRefAst::Player(p) = target.as_ref() {
                    if try_group_affected_players(*p).is_some() {
                        return Err("oracle:unsupported-attach");
                    }
                    if let Some(attachment) = resolve_fixed_att(subject, &mut trial)? {
                        let rp = lower_resolving_player_in_context(*p, &mut trial, in_trigger)?;
                        trial
                            .instructions
                            .push(super::super::toolbox_emit::attach_to_player(
                                super::super::toolbox_emit::move_subject_to_query(attachment),
                                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                            ));
                        *builder = trial;
                        return Ok(());
                    }
                    return Err("oracle:unsupported-attach");
                }
                if let SubjectRefAst::Group(host_f) = target.as_ref() {
                    if host_f.zone.is_some() {
                        return Err("oracle:unsupported-attach");
                    }
                    if let Some(att_ref) = resolve_fixed_att(subject, &mut trial)? {
                        let mut host_clean = host_f.as_ref().clone();
                        let mut exclude_current_host = false;
                        if (host_clean.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0 {
                            host_clean.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
                            exclude_current_host = true;
                        }
                        let mut kept_preds = Vec::with_capacity(host_clean.predicates.len());
                        for pred in host_clean.predicates {
                            match pred {
                                spellwake_oracle_compiler::ast::FilterPredicateAst::Exclude(
                                    ref excl,
                                ) if matches!(
                                    excl.as_ref(),
                                    SubjectRefAst::AttachedHost(_) | SubjectRefAst::Anaphora(_)
                                ) =>
                                {
                                    exclude_current_host = true;
                                }
                                spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(
                                    ref s,
                                ) if matches!(
                                    s.trim().to_ascii_lowercase().as_str(),
                                    "other than that creature"
                                        | "other than that permanent"
                                        | "other than that card"
                                        | "other than enchanted creature"
                                        | "other than enchanted permanent"
                                        | "other than equipped creature"
                                ) =>
                                {
                                    exclude_current_host = true;
                                }
                                spellwake_oracle_compiler::ast::FilterPredicateAst::CouldAttachTo(
                                    ref subj,
                                ) if matches!(
                                    subj.as_ref(),
                                    SubjectRefAst::SelfObject
                                        | SubjectRefAst::Anaphora(_)
                                        | SubjectRefAst::TriggerSubject
                                ) => {}
                                other => kept_preds.push(other),
                            }
                        }
                        host_clean.predicates = kept_preds;
                        let obj = lower_object_filter_in_context(
                            &host_clean,
                            &trial,
                            in_trigger,
                            vocabulary,
                        )?;
                        bind_filter_player_target(&mut trial, &obj)?;
                        trial
                            .instructions
                            .push(Instruction::AttachChoosingPermanent {
                                chooser: ResolvingPlayer::Controller,
                                attachment: spellwake_engine::program::AttachSlot::Fixed(att_ref),
                                host: spellwake_engine::program::AttachSlot::ChooseMatching {
                                    filter: Box::new(obj),
                                    exclude_current_host,
                                },
                                optional: false,
                                all: false,
                            });
                        *builder = trial;
                        return Ok(());
                    }
                    return Err("oracle:unsupported-attach");
                }
            }
            if let SubjectRefAst::Group(att_f)
            | SubjectRefAst::All(att_f)
            | SubjectRefAst::Each(att_f) = subject.as_ref()
            {
                if att_f.zone.is_some() {
                    return Err("oracle:unsupported-attach");
                }
                if let Some(host_ref) = resolve_fixed_host(target, &mut trial)? {
                    let obj =
                        lower_object_filter_in_context(att_f, &trial, in_trigger, vocabulary)?;
                    bind_filter_player_target(&mut trial, &obj)?;
                    let (optional, choose_all) = if *all
                        || matches!(
                            subject.as_ref(),
                            SubjectRefAst::All(_) | SubjectRefAst::Each(_)
                        ) {
                        (false, true)
                    } else if *any_number {
                        (true, true)
                    } else {
                        (false, false)
                    };
                    trial
                        .instructions
                        .push(Instruction::AttachChoosingPermanent {
                            chooser: ResolvingPlayer::Controller,
                            attachment: spellwake_engine::program::AttachSlot::ChooseMatching {
                                filter: Box::new(obj),
                                exclude_current_host: false,
                            },
                            host: spellwake_engine::program::AttachSlot::Fixed(host_ref),
                            optional,
                            all: choose_all,
                        });
                    *builder = trial;
                    return Ok(());
                }
                return Err("oracle:unsupported-attach");
            }
            if !*any_number && !*all {
                let att_ref = resolve_fixed_att(subject, &mut trial)?;
                let host_ref = resolve_fixed_host(target, &mut trial)?;
                if let (Some(attachment), Some(host)) = (att_ref, host_ref) {
                    trial
                        .instructions
                        .push(super::super::toolbox_emit::attach_query(
                            super::super::toolbox_emit::move_subject_to_query(attachment),
                            super::super::toolbox_emit::move_subject_to_query(host),
                        ));
                    *builder = trial;
                    return Ok(());
                }
            }
            Err("oracle:unsupported-attach")
        }
        InstructionAst::LookTopLibrary {
            reveal,
            library,
            count,
        } => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            let lib_low = library.trim().to_ascii_lowercase();
            if (lib_low == "ref" || (lib_low == "exiled" && !*reveal))
                && has_prior_moved_object(&builder.instructions)
            {
                let dest = if *reveal {
                    spellwake_engine::toolbox::ZoneDest::RevealOnly
                } else {
                    spellwake_engine::toolbox::ZoneDest::LookOnly {
                        viewer: spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    }
                };
                builder
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            dest,
                        ),
                    ));
                return Ok(());
            }
            let group_players = match lib_low.as_str() {
                "each player" | "each player's" | "each player's library" | "eachplayer" => {
                    Some(AffectedPlayers::EachPlayer)
                }
                "each opponent"
                | "each opponent's"
                | "each opponent's library"
                | "eachopponent" => Some(AffectedPlayers::EachOpponent),
                "each other player"
                | "each other player's"
                | "each other player's library"
                | "eachotherplayer" => Some(AffectedPlayers::EachOtherPlayer),
                _ => None,
            };
            if let Some(players) = group_players {
                let cnt = lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?;
                push_for_each_player(
                    players,
                    vec![Instruction::LookTopChoose {
                        count: cnt,
                        library_owner: ResolvingPlayer::LoopPlayer,
                        filter: None,
                        reveal: *reveal,
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
                    }],
                    builder,
                );
                return Ok(());
            }
            let lib_owner_key = lib_low
                .strip_suffix(" library")
                .unwrap_or(lib_low.as_str())
                .trim();
            let owner_opt = match lib_owner_key {
                "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" | "library" => {
                    Some((None, ResolvingPlayer::Controller))
                }
                "target player" | "target player's" | "targetplayer" => Some((
                    Some(BodyTarget::SinglePlayer),
                    ResolvingPlayer::TargetPlayer,
                )),
                "target opponent" | "target opponent's" | "targetopponent" => Some((
                    Some(BodyTarget::SingleOpponent),
                    ResolvingPlayer::TargetPlayer,
                )),
                "that player" | "that player's" | "thatplayer" | "that opponent"
                | "that opponent's" | "thatopponent" | "that" | "their" => Some((
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?,
                )),
                "its controller"
                | "its controller's"
                | "that creature's controller"
                | "that permanent's controller" => Some((
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ItsController,
                        builder,
                        in_trigger,
                    )?,
                )),
                "its owner"
                | "its owner's"
                | "that card's owner"
                | "that creature's owner"
                | "that permanent's owner" => Some((
                    None,
                    lower_resolving_player_in_context(PlayerRefAst::ItsOwner, builder, in_trigger)?,
                )),
                "chosen player"
                | "chosen player's"
                | "the chosen player"
                | "the chosen player's" => Some((
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ChosenPlayer,
                        builder,
                        in_trigger,
                    )?,
                )),
                "defending player"
                | "defending player's"
                | "defendingplayer"
                | "the defending player"
                | "the defending player's" => Some((None, ResolvingPlayer::DefendingPlayer)),
                "enchanted player"
                | "enchanted player's"
                | "enchantedplayer"
                | "the enchanted player"
                | "the enchanted player's"
                    if has_source =>
                {
                    Some((None, ResolvingPlayer::EnchantedPlayer))
                }
                "active player"
                | "active player's"
                | "activeplayer"
                | "the active player"
                | "the active player's" => Some((None, ResolvingPlayer::ActivePlayer)),
                _ => None,
            };
            let (req_bt, library_owner) = owner_opt.ok_or("oracle:unsupported-instruction")?;
            if let Some(bt) = req_bt {
                builder.bind_target(bt)?;
            }
            let cnt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder.instructions.push(Instruction::LookTopChoose {
                count: cnt,
                library_owner,
                filter: None,
                reveal: *reveal,
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
            Ok(())
        }
        InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject,
            free,
            duration,
            count_limit,
            alt_cost: None,
        } if count_limit.is_none()
            || matches!(
                count_limit,
                Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber)
            )
            || matches!(
                count_limit,
                Some(spellwake_oracle_compiler::ast::CountLimitAst::UpTo(e))
                    if matches!(e.as_ref(), ValueExprAst::Const(1))
            ) =>
        {
            let group_players = try_group_affected_players(*player);
            let rp = if group_players.is_some() {
                ResolvingPlayer::LoopPlayer
            } else {
                lower_resolving_player_in_context(*player, builder, in_trigger)?
            };
            let dur_low = duration.trim().to_ascii_lowercase();
            let (impulse_dur, resolved_dur, any_color) =
                parse_grant_cast_duration(&dur_low, *player)
                    .ok_or("oracle:unsupported-grant-cast-duration")?;
            let dur_core = strip_any_color_suffix(&dur_low);
            let is_exile_top_anaphora = matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "them"
                        | "that card"
                        | "those cards"
                        | "the exiled card"
                        | "the exiled cards"
                        | "cards exiled this way"
                        | "a card exiled this way"
                )
            );
            if *player == PlayerRefAst::You
                && count_limit.is_none()
                && is_exile_top_anaphora
                && dur_core != "now"
            {
                match builder.instructions.last_mut() {
                    Some(Instruction::ExileTopPlay {
                        duration: slot_dur,
                        cast_only: slot_cast_only,
                        free: slot_free,
                        any_color: slot_any_color,
                        grant_play: slot_grant_play,
                        ..
                    }) if !*slot_grant_play => {
                        *slot_dur = impulse_dur;
                        *slot_cast_only = *cast_only;
                        *slot_free = *free;
                        *slot_any_color = any_color;
                        *slot_grant_play = true;
                        return Ok(());
                    }
                    Some(last @ Instruction::ZoneOp(_)) => {
                        if let Instruction::ZoneOp(spec) = last
                            && let spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Library(p_sel),
                                slice: spellwake_engine::toolbox::QuerySlice::Top(amount),
                                ..
                            } = &spec.source
                            && let spellwake_engine::toolbox::PlayerSelector::Resolving(lib_p) =
                                p_sel.as_ref()
                            && let spellwake_engine::toolbox::ZoneDest::Exile {
                                face_down,
                                link_group,
                                ..
                            } = &spec.destination
                            && (link_group.is_none() || **amount != ValueExpr::literal(1))
                        {
                            let face_down = *face_down;
                            *last = Instruction::ExileTopPlay {
                                count: (**amount).clone(),
                                library_owner: AffectedPlayers::One(*lib_p),
                                duration: impulse_dur,
                                cast_only: *cast_only,
                                free: *free,
                                any_color,
                                face_down,
                                grant_play: true,
                            };
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
            let play_dur = resolved_dur.unwrap_or(match impulse_dur {
                spellwake_engine::program::ImpulsePlayDuration::ThisTurn => {
                    ResolvedDuration::UntilEndOfTurn
                }
                spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn => {
                    ResolvedDuration::ThroughControllerNextTurn
                }
                spellwake_engine::program::ImpulsePlayDuration::UntilNextEndStep
                | spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextEndStep
                | spellwake_engine::program::ImpulsePlayDuration::UntilNextUpkeep => {
                    ResolvedDuration::UntilControllerNextTurn
                }
                spellwake_engine::program::ImpulsePlayDuration::WhileExiled
                | spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource => {
                    ResolvedDuration::NoStatedDuration
                }
            });
            let emitted_op = match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    count: TargetCountAst::Exact(1),
                    ..
                } if count_limit.is_none() => {
                    let bt = lower_target_subject(subject, vocabulary)?;
                    builder.bind_target(bt)?;
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::TriggerSubject if in_trigger && count_limit.is_none() => {
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::SelfObject if self_pronoun_valid && count_limit.is_none() => {
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::Anaphora(_)
                    if count_limit.is_none()
                        && builder.target != BodyTarget::None
                        && !matches!(
                            builder.target,
                            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                        ) =>
                {
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::Anaphora(_)
                    if count_limit.is_none()
                        && (last_instruction_moved_object(&builder.instructions)
                            || (has_prior_moved_object(&builder.instructions)
                                && !has_checked_revealed_top_strict(&builder.instructions))) =>
                {
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::Anaphora(raw)
                    if count_limit.is_none()
                        && in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::Anaphora(_) if count_limit.is_none() => {
                    super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::Revealed,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    )
                }
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    let resolved_dur =
                        resolved_dur.ok_or("oracle:unsupported-grant-cast-duration")?;
                    let mut inner = f.as_ref().clone();
                    let is_linked_exile = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                    let once = match count_limit {
                        Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber) => false,
                        Some(spellwake_oracle_compiler::ast::CountLimitAst::UpTo(_)) => true,
                        None => matches!(subject.as_ref(), SubjectRefAst::Group(_)),
                    };
                    let revealed_target_hand_match = !is_linked_exile
                        && once
                        && group_players.is_none()
                        && *player == PlayerRefAst::You
                        && f.zone.is_none()
                        && matches!(
                            builder.instructions.last(),
                            Some(Instruction::ZoneOp(spec))
                                if matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::RevealOnly)
                                    && matches!(
                                        &spec.source,
                                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                            zone: spellwake_engine::toolbox::QueryZone::Hand(ps),
                                            ..
                                        } if matches!(
                                            ps.as_ref(),
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TargetPlayer
                                            )
                                        )
                                    )
                        )
                        && strip_revealed_hand_ref_predicates(&mut inner);
                    let target_zone_choose_one: Option<(Option<BodyTarget>, SourceZoneFilter)> =
                        if revealed_target_hand_match {
                            Some((None, SourceZoneFilter::TargetPlayerHand))
                        } else if !is_linked_exile
                            && once
                            && group_players.is_none()
                            && *player == PlayerRefAst::You
                        {
                            match f.zone {
                                Some(ZoneFilter::TargetPlayerHand) => Some((
                                    Some(BodyTarget::SinglePlayer),
                                    SourceZoneFilter::TargetPlayerHand,
                                )),
                                Some(ZoneFilter::TargetOpponentHand) => Some((
                                    Some(BodyTarget::SingleOpponent),
                                    SourceZoneFilter::TargetPlayerHand,
                                )),
                                Some(ZoneFilter::ThatPlayerHand)
                                    if matches!(
                                        builder.target,
                                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                                    ) =>
                                {
                                    Some((None, SourceZoneFilter::TargetPlayerHand))
                                }
                                Some(ZoneFilter::TargetPlayerGraveyard) => Some((
                                    Some(BodyTarget::SinglePlayer),
                                    SourceZoneFilter::TargetPlayerGraveyard,
                                )),
                                Some(ZoneFilter::TargetOpponentGraveyard) => Some((
                                    Some(BodyTarget::SingleOpponent),
                                    SourceZoneFilter::TargetPlayerGraveyard,
                                )),
                                Some(ZoneFilter::ThatPlayerGraveyard)
                                    if matches!(
                                        builder.target,
                                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                                    ) =>
                                {
                                    Some((None, SourceZoneFilter::TargetPlayerGraveyard))
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                    if let Some((req_bt, src_zone)) = target_zone_choose_one {
                        if let Some(bt) = req_bt {
                            builder.bind_target(bt)?;
                        }
                        inner.zone = None;
                        let obj_filter = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(Instruction::ChooseObjects {
                            players: AffectedPlayers::One(ResolvingPlayer::Controller),
                            source_zone: src_zone,
                            filter: Box::new(obj_filter),
                            min: ValueExpr::literal(0),
                            max: ValueExpr::literal(1),
                            distinct_powers: false,
                            max_total_power: None,
                            unchosen_action: None,
                        });
                        builder.instructions.push(
                            super::super::toolbox_emit::grant_play_permission(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                resolved_dur,
                                *cast_only,
                                *free,
                                any_color,
                            ),
                        );
                        return Ok(());
                    }
                    let last_moved_match = !is_linked_exile
                        && dur_core != "now"
                        && strip_last_moved_for_grant_play(&mut inner, &builder.instructions);
                    let zone =
                        if is_linked_exile && matches!(f.zone, None | Some(ZoneFilter::Exile)) {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                            SourceZoneFilter::LinkedExile
                        } else if last_moved_match {
                            SourceZoneFilter::LastMoved
                        } else {
                            match f.zone {
                                Some(ZoneFilter::YourHand) => SourceZoneFilter::Hand,
                                Some(ZoneFilter::YourHandOrGraveyard) => {
                                    SourceZoneFilter::HandOrGraveyard
                                }
                                Some(ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands) => {
                                    SourceZoneFilter::OpponentHand
                                }
                                Some(ZoneFilter::YourGraveyard) => SourceZoneFilter::Graveyard,
                                Some(
                                    ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                                ) => SourceZoneFilter::OpponentGraveyard,
                                Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                                    SourceZoneFilter::AnyGraveyard
                                }
                                Some(ZoneFilter::YourLibrary) => SourceZoneFilter::YourLibrary,
                                Some(ZoneFilter::CommandZone) => SourceZoneFilter::CommandZone,
                                Some(ZoneFilter::Exile)
                                    if matches!(
                                        f.controller,
                                        Some(
                                            ControllerRelation::OwnerOpponent
                                                | ControllerRelation::Opponent
                                                | ControllerRelation::Opponents
                                        )
                                    ) =>
                                {
                                    inner.controller = None;
                                    SourceZoneFilter::OpponentExile
                                }
                                Some(ZoneFilter::Exile) => SourceZoneFilter::Exile,
                                None => SourceZoneFilter::Hand,
                                _ => return Err("oracle:unsupported-grant-cast-zone"),
                            }
                        };
                    inner.zone = None;
                    let flt = if is_unrestricted_filter(&inner) {
                        None
                    } else {
                        Some(lower_object_filter(&inner, vocabulary)?)
                    };
                    let use_last_moved_perm = last_moved_match
                        && dur_core != "now"
                        && resolved_dur != ResolvedDuration::UntilControllerNextTurn
                        && (!once
                            || (!*cast_only
                                && count_limit.is_none()
                                && last_moved_producer_is_single_or_bulk_play(
                                    &builder.instructions,
                                )));
                    if use_last_moved_perm {
                        let selector = match flt {
                            Some(f) => spellwake_engine::toolbox::ObjectQuery::filtered(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                f,
                            ),
                            None => spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        };
                        super::super::toolbox_emit::grant_play_permission(
                            selector,
                            spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                            resolved_dur,
                            *cast_only,
                            *free,
                            any_color,
                        )
                    } else {
                        super::super::toolbox_emit::apply_temporary_rule(
                            TemporaryRuleKind::ZoneCastPermission {
                                player: rp,
                                zone,
                                filter: flt,
                                cast_only: *cast_only,
                                free: *free,
                                any_color,
                                once,
                                sorcery_only: false,
                            },
                            resolved_dur,
                        )
                    }
                }
                _ => return Err("oracle:unsupported-grant-cast"),
            };
            if let Some(players) = group_players {
                push_for_each_player(players, vec![emitted_op], builder);
            } else {
                builder.instructions.push(emitted_op);
            }
            Ok(())
        }
        InstructionAst::RetainManaUntilEndOfTurn => {
            for mt in [
                ManaType::White,
                ManaType::Blue,
                ManaType::Black,
                ManaType::Red,
                ManaType::Green,
                ManaType::Colourless,
            ] {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::Modifier {
                                id: ids.static_id(),
                                rule: StaticModifierRule::RetainMana(mt),
                            },
                        )),
                        ResolvedDuration::UntilEndOfTurn,
                    ));
            }
            Ok(())
        }
        InstructionAst::GrantModifiedCastPermission(perm) => {
            if perm.face_down_2_2 || perm.additional_cost.is_some() {
                return Err("oracle:unsupported-modified-cast-permission");
            }
            let effective_dur = if perm.spend_mana_as_any_color {
                let d_low = perm.duration.trim().to_ascii_lowercase();
                if d_low.contains("any color") || d_low.contains("any type") {
                    perm.duration.clone()
                } else if d_low.is_empty()
                    || d_low == "this_turn"
                    || d_low == "until_end_of_turn"
                    || d_low == "now"
                {
                    "this turn any color".to_owned()
                } else if d_low == "until_end_of_next_turn" {
                    "end of your next turn any color".to_owned()
                } else if d_low == "while_exiled" || d_low == "permanent" {
                    "remains exiled any color".to_owned()
                } else if d_low == "until_next_exile" {
                    "until you exile another any color".to_owned()
                } else {
                    format!("{} any color", perm.duration)
                }
            } else {
                perm.duration.clone()
            };
            if perm.as_though_flash {
                let (SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)) =
                    perm.subject.as_ref()
                else {
                    return Err("oracle:unsupported-modified-cast-permission");
                };
                let mut inner_f = f.as_ref().clone();
                inner_f.zone = None;
                inner_f.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                let flash_filter = if is_unrestricted_filter(&inner_f) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner_f, vocabulary)?
                };
                let start_len = builder.instructions.len();
                lower_instruction(
                    &InstructionAst::GrantCastPermission {
                        player: perm.player,
                        cast_only: perm.cast_only,
                        subject: perm.subject.clone(),
                        free: perm.free,
                        duration: effective_dur,
                        count_limit: None,
                        alt_cost: None,
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let flash_dur = match builder.instructions.get(start_len) {
                    Some(Instruction::ApplyContinuous(spec))
                        if matches!(
                            spec.payload,
                            spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                                | spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                        ) =>
                    {
                        spec.duration
                    }
                    Some(Instruction::ForEachPlayer { body, .. }) => {
                        match body.instructions.first() {
                            Some(Instruction::ApplyContinuous(spec))
                                if matches!(
                                    spec.payload,
                                    spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                                        | spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                                ) =>
                            {
                                spec.duration
                            }
                            _ => return Err("oracle:unsupported-modified-cast-permission"),
                        }
                    }
                    _ => return Err("oracle:unsupported-modified-cast-permission"),
                };
                if let Some(players) = try_group_affected_players(perm.player) {
                    push_for_each_player(
                        players,
                        vec![super::super::toolbox_emit::apply_temporary_rule(
                            TemporaryRuleKind::FilteredFlash {
                                player: ResolvingPlayer::LoopPlayer,
                                filter: flash_filter,
                                next_only: false,
                            },
                            flash_dur,
                        )],
                        builder,
                    );
                } else {
                    let rp = lower_resolving_player_in_context(perm.player, builder, in_trigger)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::apply_temporary_rule(
                            TemporaryRuleKind::FilteredFlash {
                                player: rp,
                                filter: flash_filter,
                                next_only: false,
                            },
                            flash_dur,
                        ));
                }
                return Ok(());
            }
            lower_instruction(
                &InstructionAst::GrantCastPermission {
                    player: perm.player,
                    cast_only: perm.cast_only,
                    subject: perm.subject.clone(),
                    free: perm.free,
                    duration: effective_dur,
                    count_limit: None,
                    alt_cost: None,
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::AddQualifiedMana(qm) => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            if qm.player != PlayerRefAst::You {
                return Err("oracle:unsupported-qualified-mana");
            }
            if qm.kind == "mana_choice" && qm.choices.len() >= 2 {
                let all_fixed: Option<Vec<&str>> = qm
                    .choices
                    .iter()
                    .map(|spec| match spec {
                        ManaSpecAst::Fixed(raw) => Some(raw.as_str()),
                        _ => None,
                    })
                    .collect();
                if let Some(raws) = all_fixed {
                    let all_colored_single: Option<BTreeSet<ManaType>> = raws
                        .iter()
                        .map(|raw| {
                            parse_single_mana_type(raw).filter(|mt| *mt != ManaType::Colourless)
                        })
                        .collect();
                    if let Some(colors) = all_colored_single
                        && !colors.is_empty()
                    {
                        builder
                            .instructions
                            .push(Instruction::AddMana(EffectManaYield::Choice {
                                colors,
                                amount: ValueExpr::literal(1),
                            }));
                        return Ok(());
                    }
                    let mut options: Vec<BTreeMap<ManaType, RulesInteger>> = Vec::new();
                    for raw in raws {
                        let bundle =
                            parse_mana_bundle(raw).ok_or("oracle:unsupported-qualified-mana")?;
                        if !options.contains(&bundle) {
                            options.push(bundle);
                        }
                    }
                    if options.len() >= 2 {
                        builder.instructions.push(Instruction::AddMana(
                            EffectManaYield::ChoiceBundle { options },
                        ));
                        return Ok(());
                    }
                    if let [only] = options.as_slice() {
                        builder
                            .instructions
                            .push(Instruction::AddMana(EffectManaYield::Fixed(only.clone())));
                        return Ok(());
                    }
                    return Err("oracle:unsupported-qualified-mana");
                }
                if has_source
                    && qm
                        .choices
                        .iter()
                        .any(|spec| matches!(spec, ManaSpecAst::ChosenColor))
                {
                    let mut options: Vec<ChooseOptionBranch> = Vec::with_capacity(qm.choices.len());
                    for spec in &qm.choices {
                        match spec {
                            ManaSpecAst::Fixed(raw) => {
                                let bundle = parse_mana_bundle(raw)
                                    .ok_or("oracle:unsupported-qualified-mana")?;
                                options.push(ChooseOptionBranch {
                                    label: format!("Add {raw}"),
                                    body: ExecutableBody::new(
                                        BodyTarget::None,
                                        vec![Instruction::AddMana(EffectManaYield::Fixed(bundle))],
                                    ),
                                });
                            }
                            ManaSpecAst::ChosenColor => {
                                options.push(ChooseOptionBranch {
                                    label: "Add one mana of the chosen color".to_owned(),
                                    body: ExecutableBody::new(
                                        BodyTarget::None,
                                        vec![Instruction::AddMana(
                                            EffectManaYield::SourceChosenColor {
                                                amount: ValueExpr::literal(1),
                                            },
                                        )],
                                    ),
                                });
                            }
                            _ => return Err("oracle:unsupported-qualified-mana"),
                        }
                    }
                    builder.instructions.push(Instruction::ChooseOption {
                        chooser: ResolvingPlayer::Controller,
                        options,
                    });
                    return Ok(());
                }
                return Err("oracle:unsupported-qualified-mana");
            }
            if qm.kind == "any_type_source_could_produce"
                && let Some(src) = qm.source.as_deref()
                && let SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) =
                    src
            {
                let mut rest = f.as_ref().clone();
                let controller_rel = rest.controller.take();
                let is_plain_land = rest.any_types == AstCardTypeMask::LAND && {
                    rest.any_types = AstCardTypeMask::default();
                    is_unrestricted_filter(&rest)
                };
                if is_plain_land {
                    let controller = match controller_rel {
                        Some(ControllerRelation::You) => {
                            AffectedPlayers::One(ResolvingPlayer::Controller)
                        }
                        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                            AffectedPlayers::EachOpponent
                        }
                        None => AffectedPlayers::EachPlayer,
                        _ => return Err("oracle:unsupported-qualified-mana"),
                    };
                    let amount = match qm.amount.as_deref() {
                        Some(expr) => lower_value_expr_in_context(
                            resolve_x(expr, bound_x),
                            builder,
                            in_trigger,
                            vocabulary,
                        )?,
                        None => ValueExpr::literal(1),
                    };
                    builder.instructions.push(Instruction::AddMana(
                        EffectManaYield::LandProducible { controller, amount },
                    ));
                    return Ok(());
                }
            }
            Err("oracle:unsupported-qualified-mana")
        }
        InstructionAst::PayAnyAmountOfMana { player } => {
            if try_group_affected_players(*player).is_some() {
                return Err("oracle:unsupported-pay-any-mana");
            }
            let payer = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::OfferDynamicManaPayment {
                    payer,
                    fixed: ManaCost::free(),
                    extra_generic: ValueExpr::chosen_x(),
                    paid_destination: idx + 1,
                    declined_destination: idx + 1,
                });
            Ok(())
        }
        InstructionAst::PlayerLookTopLibrary {
            player,
            library,
            count,
            reveal,
        } => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            let lib_low = library.trim().to_ascii_lowercase();
            let mut trial = builder.clone();
            if *reveal
                && matches!(
                    lib_low.as_str(),
                    "hand"
                        | "your hand"
                        | "their hand"
                        | "that player's hand"
                        | "that opponent's hand"
                        | "target player's hand"
                        | "target opponent's hand"
                )
            {
                let cnt = lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    &trial,
                    in_trigger,
                    vocabulary,
                )?;
                if let Some(group_players) = try_group_affected_players(*player) {
                    const HAND_BIND: spellwake_engine::toolbox::BindingId =
                        spellwake_engine::toolbox::BindingId(915);
                    let seat_sel = spellwake_engine::toolbox::PlayerSelector::Bound(HAND_BIND);
                    let hand_owner = if lib_low == "your hand" {
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        )
                    } else {
                        seat_sel.clone()
                    };
                    let inner = super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Hand(Box::new(hand_owner)),
                                ObjectFilter::any(),
                            ),
                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                chooser: seat_sel,
                                min: Box::new(cnt.clone()),
                                max: Box::new(cnt),
                                constraint: None,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                hidden: true,
                            },
                            spellwake_engine::toolbox::ZoneDest::RevealOnly,
                        ),
                    );
                    trial.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                            group_players,
                        )),
                        bind_player: HAND_BIND,
                        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
                    });
                    *builder = trial;
                    return Ok(());
                }
                let actor = lower_resolving_player_in_context(*player, &mut trial, in_trigger)?;
                let hand_owner = match lib_low.as_str() {
                    "your hand" => ResolvingPlayer::Controller,
                    "target player's hand" => {
                        trial.bind_target(BodyTarget::SinglePlayer)?;
                        ResolvingPlayer::TargetPlayer
                    }
                    "target opponent's hand" => {
                        trial.bind_target(BodyTarget::SingleOpponent)?;
                        ResolvingPlayer::TargetPlayer
                    }
                    "their hand" | "hand" => actor,
                    _ => lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        &mut trial,
                        in_trigger,
                    )?,
                };
                trial.instructions.push(super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(hand_owner),
                            )),
                            ObjectFilter::any(),
                        ),
                        spellwake_engine::toolbox::ZoneSelection::Prompt {
                            chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(actor),
                            min: Box::new(cnt.clone()),
                            max: Box::new(cnt),
                            constraint: None,
                            category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                            hidden: true,
                        },
                        spellwake_engine::toolbox::ZoneDest::RevealOnly,
                    ),
                ));
                *builder = trial;
                return Ok(());
            }
            let lib_owner_key = lib_low
                .strip_suffix(" library")
                .unwrap_or(lib_low.as_str())
                .trim();
            if let Some(group_players) = try_group_affected_players(*player) {
                let (req_bt, library_owner) = match lib_owner_key {
                    "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" | "library" => {
                        (None, ResolvingPlayer::Controller)
                    }
                    "their" | "that player" | "that player's" | "thatplayer" | "that opponent"
                    | "that opponent's" => (None, ResolvingPlayer::LoopPlayer),
                    "target player" | "target player's" | "targetplayer" => (
                        Some(BodyTarget::SinglePlayer),
                        ResolvingPlayer::TargetPlayer,
                    ),
                    "target opponent" | "target opponent's" | "targetopponent" => (
                        Some(BodyTarget::SingleOpponent),
                        ResolvingPlayer::TargetPlayer,
                    ),
                    "defending player" | "defending player's" | "defendingplayer" => {
                        (None, ResolvingPlayer::DefendingPlayer)
                    }
                    "enchanted player" | "enchanted player's" | "enchantedplayer" if has_source => {
                        (None, ResolvingPlayer::EnchantedPlayer)
                    }
                    "active player" | "active player's" | "activeplayer" => {
                        (None, ResolvingPlayer::ActivePlayer)
                    }
                    _ => return Err("oracle:unsupported-player-look-top"),
                };
                if let Some(bt) = req_bt {
                    trial.bind_target(bt)?;
                }
                let cnt = lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    &trial,
                    in_trigger,
                    vocabulary,
                )?;
                if *reveal {
                    push_for_each_player(
                        group_players,
                        vec![Instruction::LookTopChoose {
                            count: cnt,
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
                        }],
                        &mut trial,
                    );
                } else {
                    const LOOK_BIND: spellwake_engine::toolbox::BindingId =
                        spellwake_engine::toolbox::BindingId(912);
                    let lib_sel = if library_owner == ResolvingPlayer::LoopPlayer {
                        spellwake_engine::toolbox::PlayerSelector::Bound(LOOK_BIND)
                    } else {
                        spellwake_engine::toolbox::PlayerSelector::Resolving(library_owner)
                    };
                    let inner = super::super::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                    lib_sel,
                                )),
                                filter: Box::new(ObjectFilter::any()),
                                slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(cnt)),
                            },
                            spellwake_engine::toolbox::ZoneDest::LookOnly {
                                viewer: spellwake_engine::toolbox::PlayerSelector::Bound(LOOK_BIND),
                            },
                        ),
                    );
                    trial.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                            group_players,
                        )),
                        bind_player: LOOK_BIND,
                        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
                    });
                }
                *builder = trial;
                return Ok(());
            }
            let actor = lower_resolving_player_in_context(*player, &mut trial, in_trigger)?;
            let (req_bt, library_owner) = match lib_owner_key {
                "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" | "library" => {
                    (None, ResolvingPlayer::Controller)
                }
                "their" if actor != ResolvingPlayer::Controller => (None, actor),
                "that player" | "that player's" | "thatplayer" | "that opponent"
                | "that opponent's" | "thatopponent" | "that" | "their" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        &mut trial,
                        in_trigger,
                    )?,
                ),
                "its controller"
                | "its controller's"
                | "that creature's controller"
                | "that permanent's controller" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ItsController,
                        &mut trial,
                        in_trigger,
                    )?,
                ),
                "its owner"
                | "its owner's"
                | "that card's owner"
                | "that creature's owner"
                | "that permanent's owner" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ItsOwner,
                        &mut trial,
                        in_trigger,
                    )?,
                ),
                "chosen player"
                | "chosen player's"
                | "the chosen player"
                | "the chosen player's" => (
                    None,
                    lower_resolving_player_in_context(
                        PlayerRefAst::ChosenPlayer,
                        &mut trial,
                        in_trigger,
                    )?,
                ),
                "target player" | "target player's" | "targetplayer" => (
                    Some(BodyTarget::SinglePlayer),
                    ResolvingPlayer::TargetPlayer,
                ),
                "target opponent" | "target opponent's" | "targetopponent" => (
                    Some(BodyTarget::SingleOpponent),
                    ResolvingPlayer::TargetPlayer,
                ),
                "defending player"
                | "defending player's"
                | "defendingplayer"
                | "the defending player"
                | "the defending player's" => (None, ResolvingPlayer::DefendingPlayer),
                "enchanted player"
                | "enchanted player's"
                | "enchantedplayer"
                | "the enchanted player"
                | "the enchanted player's"
                    if has_source =>
                {
                    (None, ResolvingPlayer::EnchantedPlayer)
                }
                "active player"
                | "active player's"
                | "activeplayer"
                | "the active player"
                | "the active player's" => (None, ResolvingPlayer::ActivePlayer),
                _ => return Err("oracle:unsupported-player-look-top"),
            };
            if let Some(bt) = req_bt {
                trial.bind_target(bt)?;
            }
            let cnt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                &trial,
                in_trigger,
                vocabulary,
            )?;
            if !*reveal && actor != ResolvingPlayer::Controller {
                trial.instructions.push(super::super::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(library_owner),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(cnt)),
                        },
                        spellwake_engine::toolbox::ZoneDest::LookOnly {
                            viewer: spellwake_engine::toolbox::PlayerSelector::Resolving(actor),
                        },
                    ),
                ));
            } else {
                trial.instructions.push(Instruction::LookTopChoose {
                    count: cnt,
                    library_owner,
                    filter: None,
                    reveal: *reveal,
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
            }
            *builder = trial;
            Ok(())
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

type CounterStackMatchSpec = (Option<Box<SpellFilter>>, bool, bool, bool);

fn lower_single_counter_matching_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<CounterStackMatchSpec>, &'static str> {
    if !f.any_spell && !f.is_ability && f.zone != Some(ZoneFilter::Stack) {
        return Ok(None);
    }
    let opponents_only = match f.controller {
        None => false,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => true,
        _ => return Ok(None),
    };
    let (include_spells, include_abilities) = if f.is_ability && f.any_spell {
        (true, true)
    } else if f.is_ability {
        (false, true)
    } else {
        (true, false)
    };
    let mut inner = f.clone();
    inner.any_spell = false;
    inner.is_ability = false;
    inner.controller = None;
    inner.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
    if inner.zone == Some(ZoneFilter::Stack) {
        inner.zone = None;
    }
    let filter = if is_unrestricted_filter(&inner) {
        None
    } else {
        let sf = lower_spell_filter(&inner, vocabulary)?;
        if sf.cast_zone != spellwake_engine::program::SpellCastZoneFilter::Any
            || sf.target_shape != spellwake_engine::program::SpellTargetShape::Any
            || sf.ordinal_this_turn.is_some()
            || sf.special != spellwake_engine::program::SpellSpecialFilter::None
            || sf.object.has_target_player()
            || sf.object.has_related_player()
        {
            return Ok(None);
        }
        Some(Box::new(sf))
    };
    Ok(Some((
        filter,
        include_spells,
        include_abilities,
        opponents_only,
    )))
}

fn try_lower_counter_matching_stack_objects(
    target: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<Vec<Instruction>>, &'static str> {
    match target {
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            if let Some((filter, include_spells, include_abilities, opponents_only)) =
                lower_single_counter_matching_filter(f, vocabulary)?
            {
                Ok(Some(vec![Instruction::CounterMatchingStackObjects {
                    filter,
                    include_spells,
                    include_abilities,
                    opponents_only,
                }]))
            } else {
                Ok(None)
            }
        }
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut specs = Vec::with_capacity(items.len());
            for item in items {
                let (SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)) =
                    item
                else {
                    return Ok(None);
                };
                let Some(spec) = lower_single_counter_matching_filter(f, vocabulary)? else {
                    return Ok(None);
                };
                specs.push(spec);
            }
            if specs.len() == 2
                && specs[0].0 == specs[1].0
                && specs[0].3 == specs[1].3
                && ((specs[0].1 && !specs[0].2 && !specs[1].1 && specs[1].2)
                    || (!specs[0].1 && specs[0].2 && specs[1].1 && !specs[1].2))
            {
                let (filter, _, _, opponents_only) = specs.remove(0);
                return Ok(Some(vec![Instruction::CounterMatchingStackObjects {
                    filter,
                    include_spells: true,
                    include_abilities: true,
                    opponents_only,
                }]));
            }
            Ok(Some(
                specs
                    .into_iter()
                    .map(
                        |(filter, include_spells, include_abilities, opponents_only)| {
                            Instruction::CounterMatchingStackObjects {
                                filter,
                                include_spells,
                                include_abilities,
                                opponents_only,
                            }
                        },
                    )
                    .collect(),
            ))
        }
        _ => Ok(None),
    }
}

fn is_grant_play_last_moved_producer(op: &Instruction, expected_exile_only: bool) -> bool {
    match op {
        Instruction::ExileTopForPlay { .. } => true,
        Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
            if crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                .is_some() =>
        {
            let spec = crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                .unwrap();
            !expected_exile_only
                || matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                )
        }
        Instruction::LookTopChooseExt { keep_dest, .. }
        | Instruction::RevealUntilMatchExt {
            match_dest: keep_dest,
            ..
        } => {
            !expected_exile_only
                || matches!(
                    keep_dest,
                    spellwake_engine::program::LookTopKeepDest::Exile { .. }
                        | spellwake_engine::program::LookTopKeepDest::ExileLinked { .. }
                )
        }
        Instruction::ChooseObjects { .. } => !expected_exile_only,
        Instruction::ForEachPlayer { body, .. } => body
            .instructions
            .iter()
            .rev()
            .any(|inner| is_grant_play_last_moved_producer(inner, expected_exile_only)),
        Instruction::Mill { .. } | Instruction::Discard { .. } => !expected_exile_only,
        _ => false,
    }
}

fn strip_last_moved_for_grant_play(filter: &mut ObjectFilterAst, ops: &[Instruction]) -> bool {
    if strip_last_moved_predicates(filter, ops) {
        return true;
    }
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
    let classify_exile_only =
        |pred: &spellwake_oracle_compiler::ast::FilterPredicateAst| -> Option<bool> {
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
                        | "among those cards"
                        | "milled_this_way"
                        | "cards_milled_this_way"
                        | "among_the_cards_milled_this_way"
                        | "among the cards milled this way"
                        | "among_cards_milled_this_way"
                        | "among cards milled this way"
                        | "from_among_the_cards_milled_this_way"
                        | "from among the cards milled this way"
                        | "from_among_cards_milled_this_way"
                        | "from among cards milled this way" => Some(false),
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
                        | "from_among_those_cards_exiled_this_way" => Some(true),
                        _ => None,
                    }
                }
                FilterPredicateAst::AffectedByPriorAction(PriorActionRefKind::ExiledThisWay) => {
                    Some(true)
                }
                FilterPredicateAst::StateQualification(raw)
                    if raw.trim().eq_ignore_ascii_case("exiled this way") =>
                {
                    Some(true)
                }
                _ => None,
            }
        };
    let mut matched = false;
    let mut require_exile = false;
    for pred in &filter.predicates {
        if let Some(exile_only) = classify_exile_only(pred) {
            matched = true;
            require_exile |= exile_only;
        }
    }
    if !matched {
        return false;
    }
    let mut found_producer = false;
    for op in ops.iter().rev() {
        if is_grant_play_last_moved_producer(op, require_exile) {
            found_producer = true;
            break;
        }
        match op {
            Instruction::DrawCards(_)
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::AdjustLife(_)
            | Instruction::AddMana(_)
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
            Instruction::ZoneOp(spec) if spec.shuffle_library.is_some() => continue,
            _ => return false,
        }
    }
    if !found_producer {
        return false;
    }
    filter
        .predicates
        .retain(|pred| classify_exile_only(pred).is_none());
    filter.zone = None;
    true
}

fn strip_revealed_hand_ref_predicates(filter: &mut ObjectFilterAst) -> bool {
    use spellwake_oracle_compiler::ast::{FilterPredicateAst, PriorActionRefKind};
    let is_ref = |pred: &FilterPredicateAst| -> bool {
        match pred {
            FilterPredicateAst::PriorActionZoneRef { kind } => matches!(
                kind.trim().to_ascii_lowercase().as_str(),
                "from_among_those"
                    | "from_among_those_cards"
                    | "among_them"
                    | "among them"
                    | "from_among_them"
                    | "from among them"
                    | "among_those"
                    | "among those"
                    | "among_those_cards"
                    | "among those cards"
                    | "revealed_this_way"
                    | "among_the_cards_revealed_this_way"
                    | "from_among_the_cards_revealed_this_way"
                    | "from_among_cards_revealed_this_way"
            ),
            FilterPredicateAst::AffectedByPriorAction(PriorActionRefKind::RevealedThisWay) => true,
            FilterPredicateAst::StateQualification(raw) => matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "revealed this way" | "from it" | "from them"
            ),
            _ => false,
        }
    };
    if !filter.predicates.iter().any(is_ref) {
        return false;
    }
    filter.predicates.retain(|p| !is_ref(p));
    true
}

fn last_moved_producer_is_single_or_bulk_play(ops: &[Instruction]) -> bool {
    for op in ops.iter().rev() {
        match op {
            Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
                if let Some(spec) = super::super::toolbox_emit::unwrap_zone_op(op)
                    && matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::Exile { .. }
                    ) =>
            {
                if let spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                    zone: spellwake_engine::toolbox::QueryZone::Library(_),
                    slice: spellwake_engine::toolbox::QuerySlice::Top(n),
                    ..
                } = &spec.source
                {
                    return **n == ValueExpr::literal(1);
                }
                return true;
            }
            Instruction::DrawCards(_)
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::BindResolutionX(_) => continue,
            Instruction::ApplyContinuous(spec)
                if matches!(
                    spec.payload,
                    spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                ) =>
            {
                continue;
            }
            Instruction::ZoneOp(spec) if spec.shuffle_library.is_some() => continue,
            _ => return false,
        }
    }
    false
}

fn lower_counter_unless_dynamic_mana(
    cost_atom: &CostAtomAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Result<(ManaCost, ValueExpr), &'static str> {
    match cost_atom {
        CostAtomAst::PayGenericManaExpr(expr) => {
            let extra_generic = lower_value_expr_in_context(
                resolve_x(expr, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            Ok((ManaCost::free(), extra_generic))
        }
        CostAtomAst::PayScaledMana { base, per } => {
            let base_trim = base.trim();
            let per_clean = per.trim().trim_end_matches('.').trim().to_ascii_lowercase();
            if per_clean == "twice" && base_trim.eq_ignore_ascii_case("{x}") {
                let x_val = match bound_x {
                    Some(expr) => {
                        lower_value_expr_in_context(expr, builder, in_trigger, vocabulary)?
                    }
                    None => ValueExpr::chosen_x(),
                };
                return Ok((ManaCost::free(), scale_value_expr(x_val, 2)));
            }
            let base_low = base_trim.to_ascii_lowercase();
            let (fixed, unit_n) = if let Some(pos) = base_low.find(" plus an additional ") {
                let fixed_raw = base_trim[..pos].trim();
                let unit_raw = base_trim[pos + " plus an additional ".len()..].trim();
                let fixed_cost =
                    parse_fixed_mana_cost(fixed_raw).ok_or("oracle:counter-unless-non-mana")?;
                let unit = parse_generic_mana_amount(unit_raw)
                    .filter(|&n| n > 0)
                    .ok_or("oracle:counter-unless-non-mana")?;
                (fixed_cost, unit)
            } else {
                let unit = parse_generic_mana_amount(base_trim)
                    .filter(|&n| n > 0)
                    .ok_or("oracle:counter-unless-non-mana")?;
                (ManaCost::free(), unit)
            };
            let per_expr = match per_clean.as_str() {
                "basic land type among lands you control" => ValueExpr::DomainCount,
                "creature in your party" => lower_value_expr_in_context(
                    &ValueExprAst::PartySize,
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                "card discarded this way" | "cards discarded this way" => {
                    ValueExpr::event(EventValueKind::LastActionCount)
                }
                _ => {
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                    if let Ok(parsed_flt) = spellwake_oracle_compiler::filter::parse_object_filter(
                        &per_clean,
                        &compiler_vocab,
                    ) && let Ok(expr) = lower_value_expr_in_context(
                        &ValueExprAst::Count(Box::new(parsed_flt)),
                        builder,
                        in_trigger,
                        vocabulary,
                    ) {
                        expr
                    } else if let Ok(parsed_expr) =
                        spellwake_oracle_compiler::filter::parse_value_expr(
                            &format!("the number of {per_clean}"),
                            &compiler_vocab,
                        )
                    {
                        lower_value_expr_in_context(&parsed_expr, builder, in_trigger, vocabulary)?
                    } else {
                        return Err("oracle:counter-unless-non-mana");
                    }
                }
            };
            let extra_generic = if unit_n == 1 {
                per_expr
            } else {
                scale_value_expr(per_expr, i64::from(unit_n))
            };
            Ok((fixed, extra_generic))
        }
        _ => Err("oracle:counter-unless-non-mana"),
    }
}

pub(super) fn strip_grant_kw_trailing_note(raw: &str) -> String {
    let trimmed = raw.trim();
    let low = trimmed.to_ascii_lowercase();
    for suffix in [
        ". this effect doesn't remove ~.",
        ". this effect doesn't remove ~",
        " this effect doesn't remove ~.",
        " this effect doesn't remove ~",
        ". this effect doesn't remove this aura.",
        ". this effect doesn't remove this aura",
        ". this effect doesn't remove this equipment.",
        ". this effect doesn't remove this equipment",
    ] {
        if let Some(prefix) = low.strip_suffix(suffix) {
            return trimmed[..prefix.len()].trim().to_owned();
        }
    }
    trimmed.to_owned()
}

fn strip_any_color_suffix(dur_low: &str) -> &str {
    let mut s = dur_low.trim();
    while let Some(rest) = s
        .strip_suffix(" any color")
        .or_else(|| s.strip_suffix(" any type"))
    {
        s = rest.trim();
    }
    s
}

fn parse_grant_cast_duration(
    dur_low: &str,
    player: PlayerRefAst,
) -> Option<(
    spellwake_engine::program::ImpulsePlayDuration,
    Option<ResolvedDuration>,
    bool,
)> {
    use spellwake_engine::program::ImpulsePlayDuration;
    let any_color = dur_low.contains("any color") || dur_low.contains("any type");
    let core = strip_any_color_suffix(dur_low);
    let is_you = player == PlayerRefAst::You;
    // Check "end of ... next end step" before generic "next end step" so both remain distinct.
    if core == "until_end_of_next_end_step"
        || (is_you && core.contains("end of your next end step"))
        || (!is_you
            && (core.contains("end of their next end step")
                || core.contains("end of that player's next end step")))
    {
        return Some((ImpulsePlayDuration::UntilEndOfNextEndStep, None, any_color));
    }
    if core == "until_next_end_step"
        || (is_you
            && (core == "until_your_next_end_step"
                || core == "until_your_next_step"
                || core.contains("your next end step")))
        || (!is_you
            && (core == "until_their_next_end_step"
                || core.contains("their next end step")
                || core.contains("that player's next end step")))
    {
        return Some((ImpulsePlayDuration::UntilNextEndStep, None, any_color));
    }
    if core == "until_next_upkeep"
        || (is_you && (core == "until_your_next_upkeep" || core.contains("your next upkeep")))
        || (!is_you
            && (core == "until_their_next_upkeep"
                || core.contains("their next upkeep")
                || core.contains("that player's next upkeep")))
    {
        return Some((ImpulsePlayDuration::UntilNextUpkeep, None, any_color));
    }
    // Under CR 502.4 no player receives priority during the untap step, so for card play/cast
    // permissions "until your next turn" and "until the beginning of your next upkeep" share the
    // exact same legal play windows as UntilNextUpkeep, while temporary cast rules use
    // ResolvedDuration::UntilControllerNextTurn.
    if is_you && matches!(core, "until_your_next_turn" | "until your next turn") {
        return Some((
            ImpulsePlayDuration::UntilNextUpkeep,
            Some(ResolvedDuration::UntilControllerNextTurn),
            any_color,
        ));
    }
    if is_you
        && (core == "until_end_of_next_turn"
            || core.contains("end of your next turn")
            || core.contains("untilendofnextturn")
            || core.contains("untilyournextturn"))
    {
        return Some((
            ImpulsePlayDuration::UntilEndOfNextTurn,
            Some(ResolvedDuration::ThroughControllerNextTurn),
            any_color,
        ));
    }
    if !is_you
        && (core == "until_end_of_players_next_turn"
            || core.contains("end of that player's next turn")
            || core.contains("end of their next turn"))
    {
        return Some((ImpulsePlayDuration::UntilEndOfNextTurn, None, any_color));
    }
    if core == "while_exiled"
        || core.contains("remains exiled")
        || core.contains("remain exiled")
        || core == "permanent"
    {
        return Some((
            ImpulsePlayDuration::WhileExiled,
            Some(ResolvedDuration::NoStatedDuration),
            any_color,
        ));
    }
    if core == "until_next_exile" || core.contains("until you exile another") {
        return Some((
            ImpulsePlayDuration::UntilNextFromSource,
            Some(ResolvedDuration::NoStatedDuration),
            any_color,
        ));
    }
    if core.is_empty()
        || core == "this_turn"
        || core == "until_end_of_turn"
        || core == "now"
        || core.contains("this turn")
        || core.contains("end of turn")
        || core.contains("untilendofturn")
    {
        return Some((
            ImpulsePlayDuration::ThisTurn,
            Some(ResolvedDuration::UntilEndOfTurn),
            any_color,
        ));
    }
    None
}

fn search_ast_filter_uses_chosen_x(filter: &ObjectFilterAst) -> bool {
    filter
        .stat_bounds
        .iter()
        .any(|b| value_expr_uses_chosen_x_search(&b.value))
}

fn value_expr_uses_chosen_x_search(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ChosenX => true,
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Max(a, b) => {
            value_expr_uses_chosen_x_search(a) || value_expr_uses_chosen_x_search(b)
        }
        _ => false,
    }
}

fn parse_toolbox_search_dest(
    raw: &str,
    chooser: &spellwake_engine::toolbox::PlayerSelector,
    is_group_loop: bool,
) -> Option<(spellwake_engine::toolbox::ZoneDest, bool)> {
    use spellwake_engine::toolbox::{
        GraveyardSemantics, LibraryOrderPos, ZoneController, ZoneDest,
    };
    let low = raw.trim().to_ascii_lowercase();
    let cleaned = low
        .replace("_after_shuffle", "")
        .replace(" after_shuffle", "")
        .replace("after_shuffle", "");
    let d = cleaned.trim();

    if matches!(
        d,
        "hand"
            | "your hand"
            | "to your hand"
            | "into your hand"
            | "put it into your hand"
            | "put them into your hand"
            | "their hand"
            | "to their hand"
            | "into their hand"
            | "put it into their hand"
            | "put them into their hand"
            | "that player's hand"
            | "to that player's hand"
            | "into that player's hand"
            | "put it into that player's hand"
            | "put them into that player's hand"
            | "its owner's hand"
            | "to its owner's hand"
            | "into its owner's hand"
            | "to hand"
            | "into hand"
            | "hands"
    ) {
        return Some((ZoneDest::Hand, false));
    }
    if matches!(
        d,
        "graveyard"
            | "your graveyard"
            | "to your graveyard"
            | "into your graveyard"
            | "put it into your graveyard"
            | "put them into your graveyard"
            | "their graveyard"
            | "to their graveyard"
            | "into their graveyard"
            | "put it into their graveyard"
            | "put them into their graveyard"
            | "that player's graveyard"
            | "into that player's graveyard"
            | "put it into that player's graveyard"
            | "put them into that player's graveyard"
            | "its owner's graveyard"
            | "into its owner's graveyard"
            | "to graveyard"
            | "into graveyard"
    ) {
        return Some((
            ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Move,
            },
            false,
        ));
    }
    if matches!(
        d,
        "exile"
            | "into exile"
            | "to exile"
            | "exile it"
            | "exile them"
            | "exile that card"
            | "exile those cards"
            | "exile face down"
            | "exile it face down"
            | "exile them face down"
            | "face down"
            | "into exile face down"
    ) {
        let face_down = d.contains("face down");
        return Some((
            ZoneDest::Exile {
                face_down,
                viewer: face_down.then(|| chooser.clone()),
                link_to_source: false,
                delayed_return: None,
                link_group: None,
            },
            false,
        ));
    }
    if let Some(before_from) = d
        .strip_suffix(" from the top")
        .or_else(|| d.strip_suffix(" from the top of your library"))
        .or_else(|| d.strip_suffix(" from the top of their library"))
        .or_else(|| d.strip_suffix(" from the top of that player's library"))
    {
        let ord = before_from.split_whitespace().last().unwrap_or("");
        let pos_opt: Option<i64> = match ord {
            "second" | "2nd" => Some(1),
            "third" | "3rd" => Some(2),
            "fourth" | "4th" => Some(3),
            "fifth" | "5th" => Some(4),
            "sixth" | "6th" => Some(5),
            "seventh" | "7th" => Some(6),
            _ => None,
        };
        if let Some(offset) = pos_opt {
            return Some((
                ZoneDest::Library {
                    position: LibraryOrderPos::NthFromTop(Box::new(ValueExpr::literal(offset))),
                },
                true,
            ));
        }
    }
    if matches!(d, "reveal" | "revealed") {
        return Some((ZoneDest::RevealOnly, false));
    }
    if matches!(
        d,
        "top"
            | "top_of_library"
            | "top of library"
            | "top of your library"
            | "on top of your library"
            | "top of their library"
            | "on top of their library"
            | "top of that player's library"
            | "on top of that player's library"
            | "top of its owner's library"
            | "on top of its owner's library"
    ) {
        return Some((
            ZoneDest::Library {
                position: LibraryOrderPos::Top,
            },
            true,
        ));
    }
    if matches!(
        d,
        "bottom"
            | "bottom_of_library"
            | "bottom of library"
            | "bottom of your library"
            | "on the bottom of your library"
            | "bottom of their library"
            | "on the bottom of their library"
            | "bottom of that player's library"
            | "on the bottom of that player's library"
            | "bottom of its owner's library"
    ) {
        return Some((
            ZoneDest::Library {
                position: LibraryOrderPos::Bottom,
            },
            true,
        ));
    }
    if d.contains("battlefield")
        && !d.contains("attached")
        && !d.contains("counter")
        && !d.contains("transformed")
    {
        let tapped = d.contains("tapped");
        let attacking = d.contains("attacking");
        let face_down = d.contains("face down");
        let under_owner = d.contains("owner's control")
            || d.contains("owners' control")
            || d.contains("their control")
            || d.contains("that player's control")
            || is_group_loop;
        let controller = if under_owner {
            ZoneController::Owner
        } else {
            ZoneController::Player(Box::new(chooser.clone()))
        };
        return Some((
            ZoneDest::Battlefield {
                controller,
                tapped,
                attacking,
                transformed: false,
                face_down,
                entering_counters: Vec::new(),
                attach_to: None,
            },
            false,
        ));
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn try_lower_search_library_toolbox<'a>(
    library: &str,
    count: &'a ValueExprAst,
    filter: &ObjectFilterAst,
    reveal: bool,
    to_zone: &str,
    shuffle: bool,
    up_to: bool,
    any_number: bool,
    exact: bool,
    extra_zones: &'a [ZoneFilter],
    secondary_filter: Option<&'a ObjectFilterAst>,
    secondary_is_or: bool,
    where_x: Option<&'a ValueExprAst>,
    rest_to_zone: Option<&'a str>,
    modifier: Option<&'a spellwake_oracle_compiler::ast::SearchModifierAst>,
    follow_up: &[InstructionAst],
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&'a ValueExprAst>,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{
        BindingId, ChoiceCategory, ObjectQuery, PlayerSelector, QueryZone, ZoneDest, ZoneOpSpec,
        ZoneSelection,
    };

    match modifier {
        None | Some(spellwake_oracle_compiler::ast::SearchModifierAst::Additional) => {}
        Some(spellwake_oracle_compiler::ast::SearchModifierAst::InsteadOfOne) => {
            return Err("oracle:unsupported-search-modifier");
        }
    }
    if rest_to_zone.is_some() {
        return Err("oracle:unsupported-search-rest");
    }
    if secondary_filter.is_some() && !secondary_is_or {
        return Err("oracle:unsupported-secondary-search-filter");
    }

    let mut eff_reveal = reveal;
    let mut eff_shuffle = shuffle || to_zone.to_ascii_lowercase().contains("after_shuffle");
    let mut eff_to_zone = to_zone.to_owned();
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
                face_down,
                into_pile: false,
            }) if subject_is_anaphora(subject) => {
                eff_to_zone = if *face_down {
                    "exile face down".to_owned()
                } else {
                    "exile".to_owned()
                };
                idx += 1;
            }
            _ => return Err("oracle:unsupported-search-follow-up"),
        }
        for ins in &follow_up[idx..] {
            if matches!(
                ins,
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You | PlayerRefAst::ThatPlayer
                }
            ) {
                eff_shuffle = true;
                continue;
            }
            if instruction_references_anaphora(ins) {
                return Err("oracle:unsupported-search-follow-up");
            }
            trailing.push(ins);
        }
    } else if !follow_up.is_empty() {
        if follow_up.iter().all(|ins| {
            matches!(
                ins,
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You | PlayerRefAst::ThatPlayer
                }
            )
        }) {
            eff_shuffle = true;
        } else {
            return Err("oracle:unsupported-search-follow-up");
        }
    }

    let eff_count_ast = match where_x {
        Some(we) if matches!(count, ValueExprAst::ChosenX) => {
            if search_ast_filter_uses_chosen_x(filter)
                || secondary_filter.is_some_and(search_ast_filter_uses_chosen_x)
            {
                return Err("oracle:unsupported-search-where-x");
            }
            resolve_x(we, bound_x)
        }
        Some(_) => return Err("oracle:unsupported-search-where-x"),
        None => resolve_x(count, bound_x),
    };

    let mut trial = builder.clone();
    let lib_low = library.trim().to_ascii_lowercase();
    let mut extra_gy = false;
    let mut extra_hand = false;
    let mut extra_exile = false;
    for ez in extra_zones {
        match ez {
            ZoneFilter::YourGraveyard
            | ZoneFilter::ThatPlayerGraveyard
            | ZoneFilter::TargetPlayerGraveyard
            | ZoneFilter::TargetOpponentGraveyard
            | ZoneFilter::ItsOwnerGraveyard
            | ZoneFilter::OpponentGraveyard => extra_gy = true,
            ZoneFilter::YourHand
            | ZoneFilter::ThatPlayerHand
            | ZoneFilter::TargetPlayerHand
            | ZoneFilter::TargetOpponentHand
            | ZoneFilter::OpponentHand => extra_hand = true,
            ZoneFilter::Exile => extra_exile = true,
            _ => return Err("oracle:unsupported-search-source"),
        }
    }

    enum SearchScope {
        Single {
            chooser: PlayerSelector,
            lib_owner: PlayerSelector,
        },
        Group(AffectedPlayers),
    }

    let mut owner_part = lib_low.as_str();
    let zone_suffixes = [
        " graveyard, hand, and/or library",
        " graveyard, hand and/or library",
        " graveyard, hand, and library",
        " graveyard, hand and library",
        " library, hand, and/or graveyard",
        " library, hand and/or graveyard",
        " library, hand, and graveyard",
        " library, hand and graveyard",
        " hand, graveyard, and/or library",
        " hand, graveyard and/or library",
        " hand, graveyard, and library",
        " hand, graveyard and library",
        " library, graveyard, and/or hand",
        " library, graveyard and/or hand",
        " library, graveyard, and hand",
        " library, graveyard and hand",
        " hand, library, and/or graveyard",
        " hand, library and/or graveyard",
        " hand, library, and graveyard",
        " hand, library and graveyard",
        " library and/or graveyard",
        " graveyard and/or library",
        " library and graveyard",
        " graveyard and library",
        " hand and/or library",
        " library and/or hand",
        " hand and library",
        " library and hand",
        " libraries",
        " library",
    ];
    for suf in zone_suffixes {
        if let Some(prefix) = owner_part.strip_suffix(suf) {
            if suf.contains("graveyard") {
                extra_gy = true;
            }
            if suf.contains("hand") {
                extra_hand = true;
            }
            owner_part = prefix.trim();
            break;
        }
    }

    let scope = match owner_part {
        "your" | "own" | "" | "library" => SearchScope::Single {
            chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
            lib_owner: PlayerSelector::Resolving(ResolvingPlayer::Controller),
        },
        "target player" | "target player's" => {
            trial.bind_target(BodyTarget::SinglePlayer)?;
            SearchScope::Single {
                chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                lib_owner: PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
            }
        }
        "target opponent" | "target opponent's" => {
            trial.bind_target(BodyTarget::SingleOpponent)?;
            SearchScope::Single {
                chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                lib_owner: PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
            }
        }
        "that player" | "that player's" | "their" => {
            let rp = lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                &mut trial,
                in_trigger,
            )?;
            let chooser = if extra_gy || extra_hand {
                PlayerSelector::Resolving(ResolvingPlayer::Controller)
            } else {
                PlayerSelector::Resolving(rp)
            };
            SearchScope::Single {
                chooser,
                lib_owner: PlayerSelector::Resolving(rp),
            }
        }
        "its controller"
        | "its controller's"
        | "that spell's controller"
        | "that spell's controller's"
        | "that creature's controller"
        | "that creature's controller's"
        | "that land's controller"
        | "that land's controller's"
        | "that artifact's controller"
        | "that artifact's controller's"
        | "that enchantment's controller"
        | "that enchantment's controller's"
        | "that permanent's controller"
        | "that permanent's controller's" => {
            let rp = lower_resolving_player_in_context(
                PlayerRefAst::ItsController,
                &mut trial,
                in_trigger,
            )?;
            SearchScope::Single {
                chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                lib_owner: PlayerSelector::Resolving(rp),
            }
        }
        "its owner"
        | "its owner's"
        | "that card's owner"
        | "that card's owner's"
        | "that creature's owner"
        | "that creature's owner's"
        | "that permanent's owner"
        | "that permanent's owner's" => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ItsOwner, &mut trial, in_trigger)?;
            SearchScope::Single {
                chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                lib_owner: PlayerSelector::Resolving(rp),
            }
        }
        "defending player" | "defending player's" => SearchScope::Single {
            chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
            lib_owner: PlayerSelector::Resolving(ResolvingPlayer::DefendingPlayer),
        },
        "each opponent" | "each opponent's" | "opponents'" | "your opponents'" => {
            SearchScope::Group(AffectedPlayers::EachOpponent)
        }
        "each player" | "each player's" => SearchScope::Group(AffectedPlayers::EachPlayer),
        "each other player" | "each other player's" => {
            SearchScope::Group(AffectedPlayers::EachOtherPlayer)
        }
        _ => return Err("oracle:unsupported-search-source"),
    };

    let mut clean_filter = filter.clone();
    clean_filter.zone = None;
    let is_unrestricted = is_unrestricted_filter(&clean_filter) && secondary_filter.is_none();
    let obj1 = if is_unrestricted_filter(&clean_filter) {
        ObjectFilter::any()
    } else {
        lower_object_filter_in_context(&clean_filter, &trial, in_trigger, vocabulary)?
    };
    bind_filter_player_target(&mut trial, &obj1)?;
    if obj1.uses_target_permanent() && !is_permanent_body_target(trial.target) {
        return Err("oracle:unsupported-search-source");
    }

    let obj2_opt = if let Some(sec) = secondary_filter {
        let mut clean_sec = sec.clone();
        clean_sec.zone = None;
        let obj2 = if is_unrestricted_filter(&clean_sec) {
            ObjectFilter::any()
        } else {
            lower_object_filter_in_context(&clean_sec, &trial, in_trigger, vocabulary)?
        };
        bind_filter_player_target(&mut trial, &obj2)?;
        if obj2.uses_target_permanent() && !is_permanent_body_target(trial.target) {
            return Err("oracle:unsupported-search-source");
        }
        Some(obj2)
    } else {
        None
    };

    let (min_expr, max_expr) = if any_number {
        (ValueExpr::literal(0), ValueExpr::literal(99))
    } else {
        let cnt = lower_value_expr_in_context(eff_count_ast, &trial, in_trigger, vocabulary)?;
        if up_to || (!exact && !is_unrestricted) {
            (ValueExpr::literal(0), cnt)
        } else {
            (cnt.clone(), cnt)
        }
    };

    let build_source_query = |owner_sel: &PlayerSelector, flt: ObjectFilter| -> ObjectQuery {
        let mut parts = vec![ObjectQuery::zone(
            QueryZone::Library(Box::new(owner_sel.clone())),
            flt,
        )];
        if extra_gy {
            parts.push(ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(owner_sel.clone())),
                flt,
            ));
        }
        if extra_hand {
            parts.push(ObjectQuery::zone(
                QueryZone::Hand(Box::new(owner_sel.clone())),
                flt,
            ));
        }
        if extra_exile {
            parts.push(ObjectQuery::zone(QueryZone::Exile, flt));
        }
        if parts.len() == 1 {
            parts.pop().unwrap_or(ObjectQuery::Empty)
        } else {
            ObjectQuery::Union(parts)
        }
    };

    let build_full_query = |owner_sel: &PlayerSelector| -> ObjectQuery {
        let q1 = build_source_query(owner_sel, obj1);
        if let Some(obj2) = obj2_opt {
            ObjectQuery::Union(vec![q1, build_source_query(owner_sel, obj2)])
        } else {
            q1
        }
    };

    match scope {
        SearchScope::Single { chooser, lib_owner } => {
            let (dest, needs_shuffle_before_place) =
                parse_toolbox_search_dest(&eff_to_zone, &chooser, false)
                    .ok_or("oracle:unsupported-search-dest")?;
            let source = build_full_query(&lib_owner);
            let selection = ZoneSelection::Prompt {
                chooser,
                min: Box::new(min_expr),
                max: Box::new(max_expr),
                constraint: None,
                category: ChoiceCategory::Search,
                hidden: !eff_reveal,
            };
            if eff_shuffle && needs_shuffle_before_place {
                const SEARCH_BIND: BindingId = BindingId(912);
                let mut select_spec = ZoneOpSpec::simple(
                    source,
                    selection,
                    if eff_reveal {
                        ZoneDest::RevealOnly
                    } else {
                        ZoneDest::None
                    },
                );
                select_spec.bind_moved = Some(SEARCH_BIND);
                select_spec.shuffle_library = Some(lib_owner);
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(select_spec));
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                        ObjectQuery::Bound(SEARCH_BIND),
                        dest,
                    )));
            } else {
                let mut spec = ZoneOpSpec::simple(source, selection, dest);
                if eff_shuffle {
                    spec.shuffle_library = Some(lib_owner);
                }
                trial
                    .instructions
                    .push(super::super::toolbox_emit::zone_op(spec));
            }
        }
        SearchScope::Group(players) => {
            const SEAT_BIND: BindingId = BindingId(913);
            let seat_sel = PlayerSelector::Bound(SEAT_BIND);
            let (dest, needs_shuffle_before_place) =
                parse_toolbox_search_dest(&eff_to_zone, &seat_sel, true)
                    .ok_or("oracle:unsupported-search-dest")?;
            let source = build_full_query(&seat_sel);
            let selection = ZoneSelection::Prompt {
                chooser: seat_sel.clone(),
                min: Box::new(min_expr),
                max: Box::new(max_expr),
                constraint: None,
                category: ChoiceCategory::Search,
                hidden: !eff_reveal,
            };
            let mut body_ops = Vec::new();
            if eff_shuffle && needs_shuffle_before_place {
                const SEARCH_BIND: BindingId = BindingId(914);
                let mut select_spec = ZoneOpSpec::simple(
                    source,
                    selection,
                    if eff_reveal {
                        ZoneDest::RevealOnly
                    } else {
                        ZoneDest::None
                    },
                );
                select_spec.bind_moved = Some(SEARCH_BIND);
                select_spec.shuffle_library = Some(seat_sel);
                body_ops.push(super::super::toolbox_emit::zone_op(select_spec));
                body_ops.push(super::super::toolbox_emit::zone_op(ZoneOpSpec::move_all(
                    ObjectQuery::Bound(SEARCH_BIND),
                    dest,
                )));
            } else {
                let mut spec = ZoneOpSpec::simple(source, selection, dest);
                if eff_shuffle {
                    spec.shuffle_library = Some(seat_sel);
                }
                body_ops.push(super::super::toolbox_emit::zone_op(spec));
            }
            trial.instructions.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(PlayerSelector::Group(players)),
                bind_player: SEAT_BIND,
                body: Box::new(ExecutableBody::new(BodyTarget::None, body_ops)),
            });
        }
    }

    for tr in trailing {
        lower_instruction(
            tr,
            &mut trial,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
    }

    *builder = trial;
    Ok(())
}
