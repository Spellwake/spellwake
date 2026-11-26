use super::*;

pub(super) fn lower_control_misc_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::BindX { x_expr, body } => lower_instruction_seq(
            body,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            Some(x_expr),
        ),
        InstructionAst::Optional(inner) => {
            if let [
                InstructionAst::IfCondition {
                    condition,
                    then_body,
                },
            ] = inner.as_slice()
            {
                return lower_instruction(
                    &InstructionAst::IfCondition {
                        condition: condition.clone(),
                        then_body: vec![InstructionAst::Optional(then_body.clone())],
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            if let [
                InstructionAst::Unless {
                    body,
                    unless_player,
                    unless_cost,
                    unless_action,
                    unless_condition,
                },
            ] = inner.as_slice()
            {
                return lower_instruction(
                    &InstructionAst::Unless {
                        body: vec![InstructionAst::Optional(body.clone())],
                        unless_player: *unless_player,
                        unless_cost: unless_cost.clone(),
                        unless_action: unless_action.clone(),
                        unless_condition: unless_condition.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            if let [InstructionAst::MoveZone { subject, to }] = inner.as_slice() {
                let dest_low = to.trim().to_ascii_lowercase();
                if dest_low.contains("battlefield")
                    && !dest_low.contains("attacking")
                    && !dest_low.contains("opponent")
                    && let SubjectRefAst::Group(f) = subject.as_ref()
                    && f.zone == Some(ZoneFilter::YourHand)
                    && f.forbidden_types.is_empty()
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
                    && f.flags.0 == 0
                {
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
                        builder
                            .instructions
                            .push(Instruction::PutHandPermanentsOntoBattlefield {
                                player: ResolvingPlayer::Controller,
                                types,
                                maximum: 1,
                                tapped: dest_low.contains("tapped"),
                            });
                        return Ok(());
                    }
                }
            }
            if let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: true,
                    up_to,
                    any_number,
                    extra_zones,
                    secondary_filter,
                    secondary_is_or,
                    where_x,
                    rest_to_zone,
                    modifier,
                    follow_up,
                    exact: false,
                },
            ] = inner.as_slice()
            {
                return lower_search_library_instruction(
                    library,
                    count,
                    filter,
                    *reveal,
                    to_zone,
                    *up_to,
                    *any_number,
                    extra_zones,
                    secondary_filter.as_deref(),
                    *secondary_is_or,
                    where_x.as_deref(),
                    rest_to_zone.as_deref(),
                    modifier.as_ref(),
                    follow_up,
                    true,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            if let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: false,
                    up_to,
                    any_number,
                    extra_zones,
                    secondary_filter,
                    secondary_is_or,
                    where_x,
                    rest_to_zone,
                    modifier,
                    follow_up,
                    exact: false,
                },
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You,
                },
            ] = inner.as_slice()
            {
                return lower_search_library_instruction(
                    library,
                    count,
                    filter,
                    *reveal,
                    to_zone,
                    *up_to,
                    *any_number,
                    extra_zones,
                    secondary_filter.as_deref(),
                    *secondary_is_or,
                    where_x.as_deref(),
                    rest_to_zone.as_deref(),
                    modifier.as_ref(),
                    follow_up,
                    true,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            let start = builder.instructions.len();
            lower_instruction_seq(
                inner,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let count = builder.instructions.len() - start;
            if count == 0 {
                return Err("oracle:empty-optional");
            }
            if count == 1 {
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
                        instructions: count,
                    },
                );
            }
            Ok(())
        }
        InstructionAst::OptionalPlayer { player, body } => {
            if matches!(*player, PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent) {
                let players = if *player == PlayerRefAst::AnyOpponent {
                    AffectedPlayers::EachOpponent
                } else {
                    AffectedPlayers::EachPlayer
                };
                let rewritten_body: Vec<InstructionAst> = body
                    .iter()
                    .map(|ins| rewrite_loop_player_ast(ins, *player))
                    .collect();
                let cost_body = lower_loop_player_sub_body(
                    &rewritten_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                builder.instructions.push(Instruction::OfferAnyPlayer {
                    players,
                    cost_body,
                    paid_body: ExecutableBody::new(BodyTarget::None, Vec::new()),
                    declined_body: None,
                });
                return Ok(());
            }
            if matches!(
                *player,
                PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherPlayer
            ) {
                let players = match *player {
                    PlayerRefAst::EachPlayer => AffectedPlayers::EachPlayer,
                    PlayerRefAst::EachOpponent => AffectedPlayers::EachOpponent,
                    _ => AffectedPlayers::EachOtherPlayer,
                };
                let rewritten_body: Vec<InstructionAst> = body
                    .iter()
                    .map(|ins| rewrite_loop_player_ast(ins, *player))
                    .collect();
                let mut sub = lower_loop_player_sub_body(
                    &rewritten_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let count = sub.instructions.len();
                if count == 0 {
                    return Err("oracle:empty-optional");
                }
                if count == 1 {
                    prepend_instruction_at(
                        &mut sub.instructions,
                        0,
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::LoopPlayer,
                        },
                    );
                } else {
                    prepend_instruction_at(
                        &mut sub.instructions,
                        0,
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::LoopPlayer,
                            instructions: count,
                        },
                    );
                }
                builder
                    .instructions
                    .push(Instruction::ForEachPlayer { players, body: sub });
                return Ok(());
            }
            let chooser = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let start = builder.instructions.len();
            lower_instruction_seq(
                body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let count = builder.instructions.len() - start;
            if count == 0 {
                return Err("oracle:empty-optional");
            }
            if count == 1 {
                prepend_instruction_at(
                    &mut builder.instructions,
                    start,
                    Instruction::OptionalEffect { chooser },
                );
            } else {
                prepend_instruction_at(
                    &mut builder.instructions,
                    start,
                    Instruction::OptionalSequence {
                        chooser,
                        instructions: count,
                    },
                );
            }
            Ok(())
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => {
            let expr = rewrite_milled_card_mana_value(
                lower_value_expr_in_context(
                    resolve_x(delta, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
                builder,
            );
            match (player, *gain, expr) {
                (PlayerRefAst::You, true, ValueExpr::Literal(n)) => {
                    builder
                        .instructions
                        .push(Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(n),
                        });
                    Ok(())
                }
                (PlayerRefAst::You, true, expr) => {
                    builder
                        .instructions
                        .push(Instruction::GainControllerLife { amount: expr });
                    Ok(())
                }
                (PlayerRefAst::You, false, ValueExpr::Literal(n)) => {
                    builder
                        .instructions
                        .push(Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Lose(n),
                        });
                    Ok(())
                }
                (PlayerRefAst::TargetPlayer, g, ValueExpr::Literal(n)) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: if g {
                            LifeAdjustment::Gain(n)
                        } else {
                            LifeAdjustment::Lose(n)
                        },
                    });
                    Ok(())
                }
                (PlayerRefAst::TargetOpponent, g, ValueExpr::Literal(n)) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: if g {
                            LifeAdjustment::Gain(n)
                        } else {
                            LifeAdjustment::Lose(n)
                        },
                    });
                    Ok(())
                }
                (PlayerRefAst::ThatPlayer, g, ValueExpr::Literal(n))
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: if g {
                            LifeAdjustment::Gain(n)
                        } else {
                            LifeAdjustment::Lose(n)
                        },
                    });
                    Ok(())
                }
                (PlayerRefAst::EachOpponent, g, ValueExpr::Literal(n)) => {
                    builder
                        .instructions
                        .push(Instruction::AdjustEachOpponentLife {
                            adjustment: if g {
                                LifeAdjustment::Gain(n)
                            } else {
                                LifeAdjustment::Lose(n)
                            },
                            controller_adjustment: None,
                        });
                    Ok(())
                }
                (PlayerRefAst::EachPlayer, g, ValueExpr::Literal(n)) => {
                    let adj = if g {
                        LifeAdjustment::Gain(n)
                    } else {
                        LifeAdjustment::Lose(n)
                    };
                    builder
                        .instructions
                        .push(Instruction::AdjustEachOpponentLife {
                            adjustment: adj.clone(),
                            controller_adjustment: Some(adj),
                        });
                    Ok(())
                }
                (p, g, dynamic_expr) => {
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    builder.instructions.push(Instruction::AdjustPlayersLife {
                        players,
                        gain: g,
                        amount: dynamic_expr,
                    });
                    Ok(())
                }
            }
        }
        InstructionAst::SetLifeTotal { player, value } => {
            let ValueExpr::Literal(n) = lower_value_expr_in_context(
                resolve_x(value, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?
            else {
                return Err("oracle:dynamic-set-life");
            };
            match player {
                PlayerRefAst::You => {
                    builder
                        .instructions
                        .push(Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Set(n),
                        });
                    Ok(())
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Set(n),
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-set-life"),
            }
        }
        InstructionAst::BecomeMonarch {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::BecomeMonarch(None));
            Ok(())
        }
        InstructionAst::PayCost(cost) => {
            if let CostAtomAst::PayMana(raw) = cost {
                let idx = builder.instructions.len();
                if let Some(mc) = parse_fixed_mana_cost(raw) {
                    builder.instructions.push(Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::Controller,
                        cost: mc,
                        paid_destination: idx + 1,
                        declined_destination: idx + 1,
                    });
                    return Ok(());
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
                            paid_destination: idx + 1,
                            declined_destination: idx + 1,
                        });
                    return Ok(());
                }
                return Err("oracle:unsupported-pay-cost");
            }
            let ops = lower_resolution_pay_cost_ops(
                ResolvingPlayer::Controller,
                cost,
                vocabulary,
                bound_x,
            )?;
            builder.instructions.extend(ops);
            Ok(())
        }
        InstructionAst::PlayerPayCost { player, cost } => {
            let group_players = try_group_affected_players(*player);
            let rp = if group_players.is_some() {
                ResolvingPlayer::LoopPlayer
            } else {
                lower_resolving_player_in_context(*player, builder, in_trigger)?
            };
            if let CostAtomAst::PayMana(raw) = cost {
                let base_idx = if group_players.is_some() {
                    0
                } else {
                    builder.instructions.len()
                };
                let op = if let Some(mc) = parse_fixed_mana_cost(raw) {
                    Instruction::OfferManaPayment {
                        payer: rp,
                        cost: mc,
                        paid_destination: base_idx + 1,
                        declined_destination: base_idx + 1,
                    }
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
                    Instruction::OfferDynamicManaPayment {
                        payer: rp,
                        fixed,
                        extra_generic,
                        paid_destination: base_idx + 1,
                        declined_destination: base_idx + 1,
                    }
                } else {
                    return Err("oracle:unsupported-pay-cost");
                };
                if let Some(players) = group_players {
                    push_for_each_player(players, vec![op], builder);
                } else {
                    builder.instructions.push(op);
                }
                return Ok(());
            }
            let ops = lower_resolution_pay_cost_ops(rp, cost, vocabulary, bound_x)?;
            if let Some(players) = group_players {
                push_for_each_player(players, ops, builder);
            } else {
                builder.instructions.extend(ops);
            }
            Ok(())
        }
        InstructionAst::Exert { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::ExertAbilitySource);
            Ok(())
        }
        InstructionAst::CollectEvidence { value } => {
            let min = lower_value_expr_in_context(
                resolve_x(value, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder.instructions.push(Instruction::CollectEvidence {
                player: ResolvingPlayer::Controller,
                minimum: min,
            });
            Ok(())
        }
        InstructionAst::Choice(branches) if branches.len() >= 2 => {
            let mut lowered_subs: Vec<BodyBuilder> = Vec::with_capacity(branches.len());
            for branch in branches {
                let mut sub = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                sub.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                sub.last_group_filter = builder.last_group_filter;
                lower_instruction_seq(
                    branch,
                    &mut sub,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                lowered_subs.push(sub);
            }
            let first_target = lowered_subs[0].target;
            let all_same_target = lowered_subs.iter().all(|s| s.target == first_target);
            if !all_same_target {
                return Err("oracle:choice-branch-target");
            }
            if first_target != BodyTarget::None {
                if lowered_subs.iter().any(|s| {
                    s.x_target_count || s.up_to_one_target || s.graveyard_target_bounds.is_some()
                }) {
                    return Err("oracle:choice-branch-target");
                }
                builder.bind_target(first_target)?;
            }
            let mut options = Vec::with_capacity(lowered_subs.len());
            for (idx, mut sub) in lowered_subs.into_iter().enumerate() {
                sub.target = BodyTarget::None;
                options.push(ChooseOptionBranch {
                    label: format!("Option {}", idx + 1),
                    body: sub.finish()?,
                });
            }
            builder.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options,
            });
            Ok(())
        }
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::VillainousChoice {
            chooser,
            options: raw_options,
        }) if raw_options.len() >= 2 => {
            let mut options = Vec::with_capacity(raw_options.len());
            for (idx, branch) in raw_options.iter().enumerate() {
                let mut sub = BodyBuilder::new();
                sub.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                lower_instruction_seq(
                    branch,
                    &mut sub,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if sub.target != BodyTarget::None {
                    return Err("oracle:choice-branch-target");
                }
                options.push(ChooseOptionBranch {
                    label: format!("Option {}", idx + 1),
                    body: sub.finish()?,
                });
            }
            if matches!(
                chooser,
                PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOtherPlayer
            ) {
                let players = lower_affected_players_in_context(*chooser, builder, in_trigger)?;
                builder
                    .instructions
                    .push(Instruction::ChooseOptionForPlayers { players, options });
            } else {
                let rp = lower_resolving_player_in_context(*chooser, builder, in_trigger)?;
                builder.instructions.push(Instruction::ChooseOption {
                    chooser: rp,
                    options,
                });
            }
            Ok(())
        }
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::SecretVote { prompt }) => {
            let options = parse_vote_options(prompt)?;
            builder.instructions.push(Instruction::Vote {
                options,
                secret: true,
            });
            Ok(())
        }
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::ChooseFromGroup {
            chooser,
            description,
        }) => {
            let d_low = description
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let cleaned = d_low
                .strip_prefix("choose ")
                .unwrap_or(d_low.as_str())
                .trim();
            if let Ok(kind) = parse_choice_kind(cleaned, false, vocabulary) {
                return emit_choice_for_player_ref(*chooser, kind, builder, ids, in_trigger);
            }
            interactive::try_lower_interactive_choose_from_group(
                *chooser, cleaned, builder, vocabulary, ids, in_trigger,
            )
        }
        InstructionAst::ChooseFromGroup {
            count,
            random,
            group,
        } => interactive::lower_choose_from_group(
            PlayerRefAst::You,
            count,
            *random,
            group,
            builder,
            vocabulary,
            in_trigger,
            bound_x,
        ),
        InstructionAst::PlayerChooseFromGroup {
            chooser,
            count,
            random,
            group,
        } => interactive::lower_choose_from_group(
            *chooser, count, *random, group, builder, vocabulary, in_trigger, bound_x,
        ),
        InstructionAst::PlayerChooseOption { chooser, options } => {
            interactive::lower_player_choose_option(
                *chooser, options, builder, vocabulary, ids, in_trigger,
            )
        }
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::SecretNumberChoice {
            min_value,
        }) => {
            let min = u16::try_from((*min_value).max(0)).unwrap_or(0);
            let max = min.saturating_add(20).min(100);
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: spellwake_engine::program::ResolutionChoiceDomain::Number { min, max },
            });
            Ok(())
        }
        InstructionAst::Vote { options: raw_opts } => {
            let options = parse_vote_options(raw_opts)?;
            builder.instructions.push(Instruction::Vote {
                options,
                secret: false,
            });
            Ok(())
        }
        InstructionAst::RollDie { sides } => {
            let nz = std::num::NonZeroU16::new(*sides).ok_or("oracle:zero-die-sides")?;
            builder.instructions.push(Instruction::RollDie {
                player: ResolvingPlayer::Controller,
                sides: nz,
                count: ValueExpr::literal(1),
                mode: spellwake_engine::program::DieRollMode::Sum,
                bands: Vec::new(),
            });
            Ok(())
        }
        InstructionAst::RollDiceChooseOne { count, sides } => {
            let nz = std::num::NonZeroU16::new(*sides).ok_or("oracle:zero-die-sides")?;
            if *count == 0 {
                return Err("oracle:zero-die-count");
            }
            builder.instructions.push(Instruction::RollDie {
                player: ResolvingPlayer::Controller,
                sides: nz,
                count: ValueExpr::literal(i64::from(*count)),
                mode: spellwake_engine::program::DieRollMode::ChooseOne,
                bands: Vec::new(),
            });
            Ok(())
        }
        InstructionAst::FlipCoin => {
            builder.instructions.push(Instruction::FlipCoins {
                player: ResolvingPlayer::Controller,
                mode: spellwake_engine::program::CoinFlipMode::Single,
                win_body: None,
                lose_body: None,
            });
            Ok(())
        }
        InstructionAst::ChooseOption(raw) => {
            let parse_chooser = |prefix: &str| -> Result<PlayerRefAst, &'static str> {
                match prefix.trim().to_ascii_lowercase().as_str() {
                    "you" => Ok(PlayerRefAst::You),
                    "targetplayer" | "target player" => Ok(PlayerRefAst::TargetPlayer),
                    "targetopponent" | "target opponent" => Ok(PlayerRefAst::TargetOpponent),
                    "thatplayer" | "that player" => Ok(PlayerRefAst::ThatPlayer),
                    "activeplayer" | "active player" => Ok(PlayerRefAst::ActivePlayer),
                    "defendingplayer" | "defending player" => Ok(PlayerRefAst::DefendingPlayer),
                    "enchantedplayer" | "enchanted player" => Ok(PlayerRefAst::EnchantedPlayer),
                    "itscontroller" | "its controller" | "thatobjectcontroller" => {
                        Ok(PlayerRefAst::ThatObjectController)
                    }
                    "eachplayer" | "each player" => Ok(PlayerRefAst::EachPlayer),
                    "eachopponent" | "each opponent" => Ok(PlayerRefAst::EachOpponent),
                    "eachotherplayer" | "each other player" => Ok(PlayerRefAst::EachOtherPlayer),
                    "an opponent" | "any opponent" | "anyopponent" => Ok(PlayerRefAst::AnyOpponent),
                    "a player" | "any player" | "anyplayer" => Ok(PlayerRefAst::AnyPlayer),
                    _ => Err("oracle:unsupported-choose-option"),
                }
            };
            let trimmed = raw.trim();
            if let Some(who_raw) = trimmed
                .strip_prefix("choose_player:")
                .or_else(|| trimmed.strip_prefix("CHOOSE_PLAYER:"))
            {
                let who_low = who_raw.trim().trim_end_matches('.').to_ascii_lowercase();
                match who_low.as_str() {
                    "target player" => {
                        builder.bind_target(BodyTarget::SinglePlayer)?;
                        return Ok(());
                    }
                    "target opponent" => {
                        builder.bind_target(BodyTarget::SingleOpponent)?;
                        return Ok(());
                    }
                    "an opponent" | "any opponent" => {
                        builder.instructions.push(Instruction::ChooseValue {
                            chooser: ResolvingPlayer::Controller,
                            domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                                opponents_only: true,
                            },
                        });
                        return Ok(());
                    }
                    "a player" | "any player" | "another player" => {
                        builder.instructions.push(Instruction::ChooseValue {
                            chooser: ResolvingPlayer::Controller,
                            domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                                opponents_only: false,
                            },
                        });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let (chooser_ref, choice_text) = if let Some((prefix, rest)) = trimmed.split_once(':') {
                (parse_chooser(prefix)?, rest.trim().to_ascii_lowercase())
            } else {
                let low_full = trimmed.to_ascii_lowercase();
                if let Some((prefix, rest)) = low_full
                    .split_once(" chooses ")
                    .or_else(|| low_full.split_once(" choose "))
                    && let Ok(pr) = parse_chooser(prefix)
                {
                    (pr, rest.trim().to_owned())
                } else {
                    (PlayerRefAst::You, low_full)
                }
            };
            let cleaned_choice = choice_text
                .strip_prefix("choose ")
                .unwrap_or(choice_text.as_str())
                .trim()
                .trim_end_matches('.');
            if chooser_ref == PlayerRefAst::You
                && (cleaned_choice == "any target" || cleaned_choice.starts_with("target "))
            {
                if cleaned_choice == "any target" {
                    builder.bind_target(BodyTarget::SingleAnyTarget)?;
                    return Ok(());
                }
                if cleaned_choice == "target player" {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    return Ok(());
                }
                if cleaned_choice == "target opponent" {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    return Ok(());
                }
                if let Ok(subj) = spellwake_oracle_compiler::filter::parse_subject_or_target(
                    cleaned_choice,
                    &cached_compiler_vocab(vocabulary),
                ) {
                    let bt = lower_target_subject(&subj, vocabulary)?;
                    builder.bind_target(bt)?;
                    return Ok(());
                }
            }
            if let Ok(kind) = parse_choice_kind(cleaned_choice, true, vocabulary) {
                return emit_choice_for_player_ref(chooser_ref, kind, builder, ids, in_trigger);
            }
            interactive::try_lower_choose_option_ext(
                chooser_ref,
                cleaned_choice,
                builder,
                vocabulary,
                ids,
                in_trigger,
            )
        }
        InstructionAst::Clash { .. } => {
            builder.instructions.push(Instruction::Clash {
                chooser: ResolvingPlayer::Controller,
            });
            Ok(())
        }
        InstructionAst::RingTemptsYou => {
            builder.instructions.push(Instruction::RingTemptsYou);
            Ok(())
        }
        InstructionAst::Populate { count } => {
            interactive::lower_populate(count, builder, vocabulary, in_trigger, bound_x)
        }
        InstructionAst::VentureIntoDungeon { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::VentureIntoDungeon {
                        player: ResolvingPlayer::LoopPlayer,
                        undercity: false,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::VentureIntoDungeon {
                player: rp,
                undercity: false,
            });
            Ok(())
        }
        InstructionAst::TakeInitiative { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::TakeInitiative {
                        player: ResolvingPlayer::LoopPlayer,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::TakeInitiative { player: rp });
            Ok(())
        }
        InstructionAst::UnlockDoor { subject } => {
            let subj = lower_move_subject_ref(
                subject,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder.instructions.push(Instruction::UnlockDoor {
                subject: subj,
                door: None,
                allow_lock: false,
            });
            Ok(())
        }
        InstructionAst::AbandonSourceScheme => Err("oracle:scheme-excluded-from-constructed"),
        InstructionAst::Unless {
            body,
            unless_player,
            unless_cost: Some(CostAtomAst::PayMana(raw)),
            unless_action: None,
            unless_condition: None,
        } => {
            let raw_player_ast = unless_player.unwrap_or(PlayerRefAst::You);
            let mut player_ast = raw_player_ast;
            if builder.target == BodyTarget::None {
                match raw_player_ast {
                    PlayerRefAst::TargetPlayer => {
                        builder.bind_target(BodyTarget::SinglePlayer)?;
                        player_ast = PlayerRefAst::ThatPlayer;
                    }
                    PlayerRefAst::TargetOpponent => {
                        builder.bind_target(BodyTarget::SingleOpponent)?;
                        player_ast = PlayerRefAst::ThatPlayer;
                    }
                    _ => {}
                }
            }
            let mut probed_body: Option<BodyBuilder> = None;
            if builder.target == BodyTarget::None {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                probe.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                probe.last_group_filter = builder.last_group_filter;
                if lower_instruction_seq(
                    body,
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.target != BodyTarget::None
                {
                    builder.bind_target(probe.target)?;
                    probed_body = Some(probe);
                }
            }
            let payer = lower_resolving_player_in_context(player_ast, builder, in_trigger)?;
            let offer_idx = builder.instructions.len();
            let offer_op = try_build_offer_mana(payer, raw, 0, offer_idx + 1, bound_x, vocabulary)
                .ok_or("oracle:unless-pay-x")?;
            builder.instructions.push(offer_op);
            if let Some(probe) = probed_body {
                splice_probed_body(builder, probe)?;
            } else {
                lower_instruction_seq(
                    body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
            }
            let end_dest = builder.instructions.len();
            set_offer_paid(builder.instructions.get_mut(offer_idx), end_dest);
            Ok(())
        }
        InstructionAst::Unless {
            body,
            unless_player: _,
            unless_cost: None,
            unless_action: None,
            unless_condition: Some(cond),
        } => {
            let mut rc =
                lower_resolution_condition(cond, builder, vocabulary, in_trigger, bound_x)?;
            rc.comparison = negate_comparison(rc.comparison);
            let branch_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc,
                destination: 0,
            });
            lower_instruction_seq(
                body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let end_dest = builder.instructions.len();
            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                builder.instructions.get_mut(branch_idx)
            {
                *destination = end_dest;
            }
            Ok(())
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_cost,
            unless_action,
            unless_condition,
        } => {
            if let Some(cond) = unless_condition
                && unless_cost.is_none()
                && unless_action.is_none()
            {
                return lower_instruction_seq(
                    &[InstructionAst::IfCondition {
                        condition: Box::new(ConditionAst::Not(cond.clone())),
                        then_body: body.clone(),
                    }],
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            let inferred_actor = unless_action
                .as_deref()
                .and_then(|acts| acts.first())
                .and_then(infer_instruction_actor);
            let raw_player_ast = unless_player
                .or(inferred_actor)
                .unwrap_or(PlayerRefAst::You);
            let mut player_ast = raw_player_ast;
            let mut bound_unless_target = false;
            if builder.target == BodyTarget::None {
                match raw_player_ast {
                    PlayerRefAst::TargetPlayer => {
                        builder.bind_target(BodyTarget::SinglePlayer)?;
                        player_ast = PlayerRefAst::ThatPlayer;
                        bound_unless_target = true;
                    }
                    PlayerRefAst::TargetOpponent => {
                        builder.bind_target(BodyTarget::SingleOpponent)?;
                        player_ast = PlayerRefAst::ThatPlayer;
                        bound_unless_target = true;
                    }
                    _ => {}
                }
            }
            let rewritten_unless_action: Option<Vec<InstructionAst>> = if bound_unless_target {
                unless_action.as_ref().map(|acts| {
                    acts.iter()
                        .map(|ins| rewrite_loop_player_ast(ins, raw_player_ast))
                        .collect()
                })
            } else {
                None
            };
            let unless_action = rewritten_unless_action
                .as_deref()
                .or(unless_action.as_deref());
            let mut probed_body: Option<BodyBuilder> = None;
            if builder.target == BodyTarget::None {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                probe.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                probe.last_group_filter = builder.last_group_filter;
                if lower_instruction_seq(
                    body,
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.target != BodyTarget::None
                {
                    builder.bind_target(probe.target)?;
                    probed_body = Some(probe);
                }
            }
            if matches!(
                player_ast,
                PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent
            ) && (unless_cost.is_some() || unless_action.is_some())
            {
                let players = if player_ast == PlayerRefAst::AnyOpponent {
                    AffectedPlayers::EachOpponent
                } else {
                    AffectedPlayers::EachPlayer
                };
                let cost_ast: Vec<InstructionAst> = if let Some(cost) = unless_cost {
                    vec![InstructionAst::PlayerPayCost {
                        player: PlayerRefAst::ThatPlayer,
                        cost: cost.clone(),
                    }]
                } else if let Some(act_body) = unless_action {
                    act_body
                        .iter()
                        .map(|ins| rewrite_loop_player_ast(ins, player_ast))
                        .collect()
                } else {
                    Vec::new()
                };
                let cost_body = lower_loop_player_sub_body(
                    &cost_ast,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let declined_body = if let Some(probe) = probed_body {
                    ExecutableBody::new(BodyTarget::None, probe.instructions)
                } else {
                    let mut dec = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                    dec.target = builder.target;
                    dec.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                    dec.last_group_filter = builder.last_group_filter;
                    lower_instruction_seq(
                        body,
                        &mut dec,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    if dec.target != builder.target {
                        if builder.target == BodyTarget::None {
                            builder.bind_target(dec.target)?;
                        } else {
                            return Err("oracle:multiple-targets");
                        }
                    }
                    ExecutableBody::new(BodyTarget::None, dec.instructions)
                };
                builder.instructions.push(Instruction::OfferAnyPlayer {
                    players,
                    cost_body,
                    paid_body: ExecutableBody::new(BodyTarget::None, Vec::new()),
                    declined_body: Some(declined_body),
                });
                return Ok(());
            }
            if let Some(CostAtomAst::PayMana(raw)) = unless_cost
                && let Some(mana) = parse_fixed_mana_cost(raw)
            {
                let payer = lower_resolving_player_in_context(player_ast, builder, in_trigger)?;
                let offer_idx = builder.instructions.len();
                builder.instructions.push(Instruction::OfferManaPayment {
                    payer,
                    cost: mana,
                    paid_destination: 0,
                    declined_destination: offer_idx + 1,
                });
                if let Some(probe) = probed_body {
                    splice_probed_body(builder, probe)?;
                } else {
                    lower_instruction_seq(
                        body,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                let end_dest = builder.instructions.len();
                if let Some(Instruction::OfferManaPayment {
                    paid_destination, ..
                }) = builder.instructions.get_mut(offer_idx)
                {
                    *paid_destination = end_dest;
                }
                return Ok(());
            }
            let opt_choice = if let Some(cost) = unless_cost {
                Some(if player_ast == PlayerRefAst::You {
                    InstructionAst::Optional(vec![InstructionAst::PayCost(cost.clone())])
                } else {
                    InstructionAst::OptionalPlayer {
                        player: player_ast,
                        body: vec![InstructionAst::PlayerPayCost {
                            player: player_ast,
                            cost: cost.clone(),
                        }],
                    }
                })
            } else if let Some(act_body) = unless_action {
                let mana_action = match act_body {
                    [InstructionAst::PayCost(CostAtomAst::PayMana(raw))] => {
                        Some((player_ast, raw.as_str()))
                    }
                    [
                        InstructionAst::PlayerPayCost {
                            player,
                            cost: CostAtomAst::PayMana(raw),
                        },
                    ] => Some((*player, raw.as_str())),
                    _ => None,
                };
                if let Some((mana_player, raw)) = mana_action {
                    let payer =
                        lower_resolving_player_in_context(mana_player, builder, in_trigger)?;
                    let offer_idx = builder.instructions.len();
                    let offer_op =
                        try_build_offer_mana(payer, raw, 0, offer_idx + 1, bound_x, vocabulary)
                            .ok_or("oracle:unless-pay-x")?;
                    builder.instructions.push(offer_op);
                    if let Some(probe) = probed_body {
                        splice_probed_body(builder, probe)?;
                    } else {
                        lower_instruction_seq(
                            body,
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    let end_dest = builder.instructions.len();
                    set_offer_paid(builder.instructions.get_mut(offer_idx), end_dest);
                    return Ok(());
                }
                Some(if player_ast == PlayerRefAst::You {
                    InstructionAst::Optional(act_body.to_vec())
                } else {
                    InstructionAst::OptionalPlayer {
                        player: player_ast,
                        body: act_body.to_vec(),
                    }
                })
            } else {
                None
            };
            if let Some(opt) = opt_choice {
                if let Some(probe) = probed_body {
                    lower_instruction_seq(
                        &[opt],
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    let branch_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(0),
                        },
                        destination: 0,
                    });
                    splice_probed_body(builder, probe)?;
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(branch_idx)
                    {
                        *destination = end_dest;
                    }
                    return Ok(());
                }
                return lower_instruction_seq(
                    &[opt, InstructionAst::IfPreviousFailed(body.clone())],
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ForEachPlayer { player, body } => {
            if !body.is_empty() {
                if *player == PlayerRefAst::EachOpponent {
                    let scaled_all: Option<Vec<InstructionAst>> = body
                        .iter()
                        .map(|s| try_scale_controller_instruction(s, ValueExprAst::OpponentsCount))
                        .collect();
                    if let Some(seq) = scaled_all {
                        return lower_instruction_seq(
                            &seq,
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        );
                    }
                }
                let rewritten_all: Option<Vec<InstructionAst>> = body
                    .iter()
                    .map(|s| try_rewrite_for_each_player(s, *player))
                    .collect();
                if let Some(seq) = rewritten_all {
                    return lower_instruction_seq(
                        &seq,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                if *player == PlayerRefAst::EachOpponent
                    && body.iter().all(instruction_acts_only_on_controller)
                    && let Ok(sub) =
                        lower_effect_body(body, vocabulary, ids, in_trigger, self_pronoun_valid)
                    && sub.target == BodyTarget::None
                    && !sub.instructions.is_empty()
                {
                    builder.instructions.push(Instruction::RepeatLoop {
                        count: ValueExpr::OpponentsCount,
                        body: Box::new(sub),
                    });
                    return Ok(());
                }
                let loop_players = match *player {
                    PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => {
                        Some(AffectedPlayers::EachPlayer)
                    }
                    PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
                        Some(AffectedPlayers::EachOpponent)
                    }
                    PlayerRefAst::EachOtherPlayer => Some(AffectedPlayers::EachOtherPlayer),
                    _ => None,
                };
                if let Some(players) = loop_players {
                    let rewritten_body: Vec<InstructionAst> = body
                        .iter()
                        .map(|ins| rewrite_loop_player_ast(ins, *player))
                        .collect();
                    if let Ok(sub) = lower_loop_player_sub_body(
                        &rewritten_body,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    ) && !sub.instructions.is_empty()
                    {
                        builder
                            .instructions
                            .push(Instruction::ForEachPlayer { players, body: sub });
                        return Ok(());
                    }
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ForEachObject { filter, body } => {
            if !body.is_empty() && !body.iter().any(instruction_references_anaphora) {
                let scaled_all: Option<Vec<InstructionAst>> = body
                    .iter()
                    .map(|s| try_scale_instruction(s, ValueExprAst::Count(filter.clone())))
                    .collect();
                if let Some(seq) = scaled_all {
                    return lower_instruction_seq(
                        &seq,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                if let Ok(count_expr) = lower_value_expr_in_context(
                    &ValueExprAst::Count(filter.clone()),
                    builder,
                    in_trigger,
                    vocabulary,
                ) && let Ok(sub) =
                    lower_effect_body(body, vocabulary, ids, in_trigger, self_pronoun_valid)
                    && sub.target == BodyTarget::None
                    && !sub.instructions.is_empty()
                {
                    builder.instructions.push(Instruction::RepeatLoop {
                        count: count_expr,
                        body: Box::new(sub),
                    });
                    return Ok(());
                }
            }
            if let [single] = body.as_slice()
                && let Some(desugared) = try_desugar_for_each_object_single(single, filter.as_ref())
            {
                let mut trial = builder.clone();
                if lower_instruction(
                    &desugared,
                    &mut trial,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                {
                    *builder = trial;
                    return Ok(());
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ForEachCount { count, body } => {
            if !body.is_empty() {
                let mult = resolve_x(count, bound_x).clone();
                let scaled_all: Option<Vec<InstructionAst>> = body
                    .iter()
                    .map(|s| try_scale_instruction(s, mult.clone()))
                    .collect();
                if let Some(seq) = scaled_all {
                    return lower_instruction_seq(
                        &seq,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                if let Ok(count_expr) =
                    lower_value_expr_in_context(&mult, builder, in_trigger, vocabulary)
                    && let Ok(sub) =
                        lower_effect_body(body, vocabulary, ids, in_trigger, self_pronoun_valid)
                    && sub.target == BodyTarget::None
                    && !sub.instructions.is_empty()
                {
                    builder.instructions.push(Instruction::RepeatLoop {
                        count: count_expr,
                        body: Box::new(sub),
                    });
                    return Ok(());
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::Modal { choice, modes } if modes.len() >= 2 => {
            if modes.iter().any(|m| m.granted.is_some()) {
                return Err("oracle:unsupported-modal-granted-face");
            }
            let (min_raw, max_raw) = parse_modal_choice(choice)?;
            let mut mode_bodies = Vec::with_capacity(modes.len());
            for m in modes {
                let mut sub = BodyBuilder::new();
                sub.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                lower_instruction_seq(
                    &m.body,
                    &mut sub,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if sub.target != BodyTarget::None {
                    return Err("oracle:nested-modal-targeted");
                }
                mode_bodies.push(sub.finish()?);
            }
            let n = mode_bodies.len();
            let min_u = usize::from(min_raw);
            let max_u = usize::from(max_raw).min(n);
            if min_u > max_u || max_u == 0 || n > 16 {
                return Err("oracle:unsupported-modal-choice");
            }
            let mut subset_indices: Vec<Vec<usize>> = Vec::new();
            if max_u == 1 {
                for idx in 0..n {
                    subset_indices.push(vec![idx]);
                }
            } else {
                let lo = min_u.max(1);
                let mut cur = Vec::with_capacity(max_u);
                for size in lo..=max_u {
                    if !collect_combinations(n, size, 0, &mut cur, &mut subset_indices, 16) {
                        return Err("oracle:unsupported-modal-choice");
                    }
                }
            }
            let mut branches = Vec::with_capacity(subset_indices.len());
            for idxs in subset_indices {
                let mut combined = Vec::new();
                for &idx in &idxs {
                    let offset = combined.len();
                    let delta =
                        isize::try_from(offset).map_err(|_| "oracle:unsupported-modal-choice")?;
                    for mut op in mode_bodies[idx].instructions.clone() {
                        if offset > 0 {
                            op.shift_jumps(delta, 0);
                        }
                        combined.push(op);
                    }
                }
                let label = if idxs.len() == 1 {
                    format!("Mode {}", idxs[0] + 1)
                } else if idxs.len() == n && n == 2 {
                    "Both modes".to_owned()
                } else {
                    let nums: Vec<String> = idxs.iter().map(|i| (i + 1).to_string()).collect();
                    format!("Modes {}", nums.join(" and "))
                };
                branches.push(ChooseOptionBranch {
                    label,
                    body: ExecutableBody::new(BodyTarget::None, combined),
                });
            }
            if branches.is_empty() {
                return Err("oracle:unsupported-modal-choice");
            }
            if branches.len() == 1 {
                if min_u == 0 {
                    return Err("oracle:unsupported-modal-choice");
                }
                let Some(only_branch) = branches.into_iter().next() else {
                    return Err("oracle:unsupported-modal-choice");
                };
                let base = builder.instructions.len();
                let delta = isize::try_from(base).map_err(|_| "oracle:unsupported-modal-choice")?;
                for mut op in only_branch.body.instructions {
                    if base > 0 {
                        op.shift_jumps(delta, 0);
                    }
                    builder.instructions.push(op);
                }
                return Ok(());
            }
            if min_u == 0 {
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
            }
            builder.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options: branches,
            });
            Ok(())
        }
        InstructionAst::BindXY {
            x_expr,
            y_expr: _,
            body,
        } => {
            if body_mentions_y_var(body) {
                return Err("oracle:unsupported-bind-xy");
            }
            lower_instruction_seq(
                body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                Some(x_expr.as_ref()),
            )
        }
        InstructionAst::ForEachSubjectPlayer { players, body } => {
            if let SubjectRefAst::Player(p) = players.as_ref() {
                lower_instruction(
                    &InstructionAst::ForEachPlayer {
                        player: *p,
                        body: body.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
            } else {
                Err("oracle:unsupported-for-each-subject-player")
            }
        }
        InstructionAst::Forage => {
            let exile_branch = ChooseOptionBranch {
                label: "Exile three cards from your graveyard".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ExileFromZone {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        zone: CostExileZone::Graveyard,
                        amount: ValueExpr::literal(3),
                        filter: Box::new(ObjectFilter::any()),
                    }],
                ),
            };
            let food_filter = PermanentTypeFilter {
                any_types: BTreeSet::new(),
                required_types: BTreeSet::new(),
                forbidden_types: BTreeSet::new(),
                object_filter: Some(ObjectFilter::Clause(ObjectClause {
                    subtypes: SubtypeFilter::Artifact(ArtifactSubtype::Food),
                    ..ObjectClause::any()
                })),
            };
            let sac_branch = ChooseOptionBranch {
                label: "Sacrifice a Food".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Sacrifice {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        amount: ValueExpr::literal(1),
                        filter: food_filter,
                    }],
                ),
            };
            builder.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options: vec![exile_branch, sac_branch],
            });
            Ok(())
        }
        InstructionAst::ChoosePlayerTarget { player } => match *player {
            PlayerRefAst::TargetPlayer => {
                builder.bind_target(BodyTarget::SinglePlayer)?;
                Ok(())
            }
            PlayerRefAst::TargetOpponent => {
                builder.bind_target(BodyTarget::SingleOpponent)?;
                Ok(())
            }
            PlayerRefAst::AnyOpponent => {
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: ResolvingPlayer::Controller,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                        opponents_only: true,
                    },
                });
                Ok(())
            }
            PlayerRefAst::AnyPlayer => {
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: ResolvingPlayer::Controller,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                        opponents_only: false,
                    },
                });
                Ok(())
            }
            _ => Err("oracle:unsupported-choose-player-target"),
        },
        InstructionAst::ManualTabletopAction { .. } => {
            Err("oracle:tabletop-honor-excluded-from-constructed")
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LastMovedProducerKind {
    Any,
    Mill,
    Exile,
    Discard,
    Graveyard,
    Sacrifice,
    Destroy,
}

pub(super) fn parse_generic_cost_reduction_delta(raw: &str) -> Option<u16> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(trimmed)
        .trim();
    let n = inner.parse::<u16>().ok()?;
    (n > 0).then_some(n)
}

pub(super) fn trigger_subject_has_unbound_target_or_anaphora(
    subj: &SubjectRefAst,
    allow_self: bool,
) -> bool {
    match subj {
        SubjectRefAst::SelfObject => !allow_self,
        SubjectRefAst::Anaphora(_)
        | SubjectRefAst::AnyTarget
        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::TargetObject { .. }
        | SubjectRefAst::TargetOneOf { .. }
        | SubjectRefAst::DistinctTarget { .. } => true,
        SubjectRefAst::Player(PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent) => true,
        SubjectRefAst::InZone { subject, .. }
        | SubjectRefAst::ControllerOf(subject)
        | SubjectRefAst::OwnerOf(subject) => {
            trigger_subject_has_unbound_target_or_anaphora(subject, allow_self)
        }
        SubjectRefAst::Compound(items) => items
            .iter()
            .any(|it| trigger_subject_has_unbound_target_or_anaphora(it, allow_self)),
        _ => false,
    }
}

pub(super) fn trigger_spec_has_unbound_target_or_anaphora(
    trigger: &TriggerSpecAst,
    allow_self: bool,
) -> bool {
    let check = |s: &SubjectRefAst| trigger_subject_has_unbound_target_or_anaphora(s, allow_self);
    trigger.events.iter().any(|ev| match ev {
        TriggerEventAst::ZoneChange { subject, .. }
        | TriggerEventAst::AttacksUnblocked { subject }
        | TriggerEventAst::Cycled { subject }
        | TriggerEventAst::TapStateChanged { subject, .. }
        | TriggerEventAst::BecomesTargeted { subject, .. }
        | TriggerEventAst::TurnedFaceUp { subject }
        | TriggerEventAst::Transforms { subject }
        | TriggerEventAst::CountersChanged { subject, .. }
        | TriggerEventAst::PermanentSacrificed { subject, .. }
        | TriggerEventAst::Mutates { subject }
        | TriggerEventAst::UnlockDoor { subject }
        | TriggerEventAst::BecomesMonstrous { subject }
        | TriggerEventAst::BecomesRenowned { subject }
        | TriggerEventAst::Exploits { subject }
        | TriggerEventAst::BecomesAttached { subject, .. }
        | TriggerEventAst::Exerted { subject }
        | TriggerEventAst::Crewed { subject }
        | TriggerEventAst::Explores { subject }
        | TriggerEventAst::Connives { subject }
        | TriggerEventAst::PhaseChanged { subject, .. }
        | TriggerEventAst::Exiled { subject } => check(subject),
        TriggerEventAst::DamageDealt {
            source, recipient, ..
        } => source.as_deref().is_some_and(check) || recipient.as_deref().is_some_and(check),
        TriggerEventAst::Attacks { subject, .. } => subject.as_deref().is_some_and(check),
        TriggerEventAst::AttacksWithOthers { subject, .. } => check(subject),
        TriggerEventAst::Blocks { subject, attacker } => {
            check(subject) || attacker.as_deref().is_some_and(check)
        }
        TriggerEventAst::BecomesBlocked { subject, blocker } => {
            check(subject) || blocker.as_deref().is_some_and(check)
        }
        TriggerEventAst::BlocksOrBecomesBlockedBy { subject, .. } => check(subject),
        TriggerEventAst::SpellCast { targets, .. } => targets.as_deref().is_some_and(check),
        _ => false,
    })
}

pub(super) fn value_expr_ast_references_anaphora(e: &ValueExprAst) -> bool {
    match e {
        ValueExprAst::ObjectStat { subject, .. }
        | ValueExprAst::CounterCount { subject, .. }
        | ValueExprAst::ColorsOfManaSpentOn(subject)
        | ValueExprAst::ColorPipsIn { subject, .. }
        | ValueExprAst::CountKeywordsIn { subject, .. }
        | ValueExprAst::TimesCastFromCommandZone(subject)
        | ValueExprAst::DamageDealtToSubjectThisTurn(subject)
        | ValueExprAst::TotalToxicValue(subject)
        | ValueExprAst::StickerLetterCount { subject, .. }
        | ValueExprAst::NotedTypesCount(subject) => {
            matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
        }
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Pow { base: a, exp: b }
        | ValueExprAst::Shortfall {
            threshold: a,
            actual: b,
        }
        | ValueExprAst::AbsDiff(a, b)
        | ValueExprAst::Max(a, b)
        | ValueExprAst::Min(a, b) => {
            value_expr_ast_references_anaphora(a) || value_expr_ast_references_anaphora(b)
        }
        ValueExprAst::DivConst { expr, .. }
        | ValueExprAst::AggregateOverPlayers { metric: expr, .. } => {
            value_expr_ast_references_anaphora(expr)
        }
        ValueExprAst::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            value_expr_ast_references_anaphora(then_expr)
                || value_expr_ast_references_anaphora(else_expr)
        }
        _ => false,
    }
}

pub(super) fn is_loop_object_anaphora(s: &SubjectRefAst) -> bool {
    matches!(
        s,
        SubjectRefAst::Anaphora(raw)
            if matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "it" | "them"
                    | "that permanent"
                    | "that creature"
                    | "that artifact"
                    | "that enchantment"
                    | "that land"
                    | "that planeswalker"
                    | "that card"
                    | "that token"
                    | "those permanents"
                    | "those creatures"
                    | "those cards"
            )
    )
}

pub(super) fn try_desugar_for_each_object_single(
    ins: &InstructionAst,
    filter: &ObjectFilterAst,
) -> Option<InstructionAst> {
    let all_subj = || Box::new(SubjectRefAst::All(Box::new(filter.clone())));
    match ins {
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } if is_loop_object_anaphora(subject) && !value_expr_ast_references_anaphora(count) => {
            Some(InstructionAst::PutCounters {
                subject: all_subj(),
                counter_kind: counter_kind.clone(),
                count: count.clone(),
            })
        }
        InstructionAst::RemoveCounters {
            subject,
            counter_kind,
            all,
            count,
        } if is_loop_object_anaphora(subject)
            && !count
                .as_deref()
                .is_some_and(value_expr_ast_references_anaphora) =>
        {
            Some(InstructionAst::RemoveCounters {
                subject: all_subj(),
                counter_kind: counter_kind.clone(),
                all: *all,
                count: count.clone(),
            })
        }
        InstructionAst::TapPermanent {
            subject,
            any_number: false,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::TapPermanent {
            subject: all_subj(),
            any_number: false,
        }),
        InstructionAst::UntapPermanent {
            subject,
            count: None,
            up_to: false,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::UntapPermanent {
            subject: all_subj(),
            count: None,
            up_to: false,
        }),
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale,
        } if is_loop_object_anaphora(subject)
            && !scale
                .as_deref()
                .is_some_and(value_expr_ast_references_anaphora) =>
        {
            Some(InstructionAst::ModifyPT {
                subject: all_subj(),
                dx: dx.clone(),
                dy: dy.clone(),
                duration: duration.clone(),
                scale: scale.clone(),
            })
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::GrantKeywords {
            subject: all_subj(),
            keywords: keywords.clone(),
            duration: duration.clone(),
        }),
        InstructionAst::RemoveKeywords {
            subject,
            keywords,
            duration,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::RemoveKeywords {
            subject: all_subj(),
            keywords: keywords.clone(),
            duration: duration.clone(),
        }),
        InstructionAst::RemoveAllAbilities { subject, duration }
            if is_loop_object_anaphora(subject) =>
        {
            Some(InstructionAst::RemoveAllAbilities {
                subject: all_subj(),
                duration: duration.clone(),
            })
        }
        InstructionAst::CombatRuleMod {
            subject,
            rule,
            duration,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::CombatRuleMod {
            subject: all_subj(),
            rule: rule.clone(),
            duration: duration.clone(),
        }),
        InstructionAst::Destroy {
            subject,
            random: false,
            except: None,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::Destroy {
            subject: all_subj(),
            random: false,
            except: None,
        }),
        InstructionAst::Exile {
            actor,
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::Exile {
            actor: *actor,
            subject: all_subj(),
            random: false,
            face_down: false,
            into_pile: false,
        }),
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
            count: None,
            up_to: false,
            any_number: false,
            actor,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::ReturnZone {
            subject: all_subj(),
            from_zone: from_zone.clone(),
            to: to.clone(),
            tapped: *tapped,
            transformed: *transformed,
            with_counter: with_counter.clone(),
            attached_to: None,
            random: false,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            actor: *actor,
        }),
        InstructionAst::MoveZone { subject, to } if is_loop_object_anaphora(subject) => {
            Some(InstructionAst::MoveZone {
                subject: all_subj(),
                to: to.clone(),
            })
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } if is_loop_object_anaphora(target)
            && !matches!(source.as_ref(), SubjectRefAst::Anaphora(_))
            && !value_expr_ast_references_anaphora(amount) =>
        {
            Some(InstructionAst::DealDamage {
                source: source.clone(),
                target: all_subj(),
                amount: amount.clone(),
            })
        }
        InstructionAst::DoubleStatOrCounters {
            subject,
            kind,
            duration,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::DoubleStatOrCounters {
            subject: all_subj(),
            kind: kind.clone(),
            duration: duration.clone(),
        }),
        InstructionAst::ShuffleIntoLibrary { subject } if is_loop_object_anaphora(subject) => {
            Some(InstructionAst::ShuffleIntoLibrary {
                subject: all_subj(),
            })
        }
        InstructionAst::Sacrifice {
            player,
            count: None,
            subject,
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        } if is_loop_object_anaphora(subject) => Some(InstructionAst::Sacrifice {
            player: *player,
            count: None,
            subject: all_subj(),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }),
        _ => None,
    }
}

pub(super) fn is_allowed_inside_optional_effect(op: &Instruction) -> bool {
    matches!(
        op,
        Instruction::DamageGroup { .. }
            | Instruction::DamageGroupAmount { .. }
            | Instruction::DamageOpponentsAmount(_)
            | Instruction::DealDamageFrom { .. }
            | Instruction::PayLife { .. }
            | Instruction::PayEnergy { .. }
            | Instruction::PayControllerLife(_)
            | Instruction::PayControllerEnergy(_)
            | Instruction::AdjustPlayersLife { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::Discard {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::DiscardFiltered {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::DiscardUpToExt {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::DiscardOneAtRandom(_)
            | Instruction::Sacrifice {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::SacrificeSubject { .. }
            | Instruction::SacrificeMatching(_)
            | Instruction::ExileFromZone {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::ExileGroupFromZone { .. }
            | Instruction::Mill { .. }
            | Instruction::MoveSubject { .. }
            | Instruction::SelectZoneCardsToZone { .. }
            | Instruction::ChooseObjects { .. }
            | Instruction::DestroyGroup(_)
            | Instruction::MoveGroup { .. }
            | Instruction::Blight { .. }
            | Instruction::CollectEvidence { .. }
    )
}

pub(super) fn is_battlefield_owner_control_dest(dest: &str) -> bool {
    parse_battlefield_dest(dest).is_some_and(|flags| flags.under_owner_control)
}

pub(super) fn parse_card_type_word(word: &str) -> Option<CardType> {
    match word.trim() {
        "artifact" | "artifacts" => Some(CardType::Artifact),
        "battle" | "battles" => Some(CardType::Battle),
        "creature" | "creatures" => Some(CardType::Creature),
        "enchantment" | "enchantments" => Some(CardType::Enchantment),
        "instant" | "instants" => Some(CardType::Instant),
        "land" | "lands" => Some(CardType::Land),
        "planeswalker" | "planeswalkers" => Some(CardType::Planeswalker),
        "sorcery" | "sorceries" => Some(CardType::Sorcery),
        _ => None,
    }
}

pub(super) fn parse_card_type_choice_list(cleaned: &str) -> Option<Vec<CardType>> {
    if !cleaned.contains(" or ") {
        return None;
    }
    let mut types = Vec::new();
    for seg in cleaned.split(", or ").flat_map(|s| s.split(" or ")) {
        for sub in seg.split(',') {
            let t = sub.trim().trim_start_matches("or ").trim();
            if t.is_empty() {
                continue;
            }
            let ct = parse_card_type_word(t)?;
            if !types.contains(&ct) {
                types.push(ct);
            }
        }
    }
    (types.len() >= 2).then_some(types)
}

pub(in super::super) enum ParsedChoiceKind {
    Domain(spellwake_engine::program::ResolutionChoiceDomain),
    CreatureType { excluding: BTreeSet<CreatureTypeId> },
}

pub(in super::super) fn parse_choice_kind(
    cleaned_choice: &str,
    allow_arbitrary_or_labels: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ParsedChoiceKind, &'static str> {
    use spellwake_engine::program::ResolutionChoiceDomain;
    if let Some(rest) = cleaned_choice
        .strip_prefix("a number between ")
        .or_else(|| cleaned_choice.strip_prefix("number between "))
        && let Some((a_s, b_s)) = rest.split_once(" and ")
        && let (Some(a), Some(b)) = (
            parse_small_number_word(a_s.trim()),
            parse_small_number_word(b_s.trim()),
        )
        && a <= b
        && b - a <= 100
    {
        return Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Number {
            min: a,
            max: b,
        }));
    }
    match cleaned_choice {
        "a color" => Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Color)),
        "a card type" => Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::CardType(
            Vec::new(),
        ))),
        "a basic land type" | "a land type" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::BasicLandType,
        )),
        "an opponent" => Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Player {
            opponents_only: true,
        })),
        "a player" | "another player" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Player {
                opponents_only: false,
            }))
        }
        "a number" => Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Number {
            min: 0,
            max: 20,
        })),
        "a card name" => Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::CardName {
            nonland_only: false,
        })),
        "a nonland card name" => Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::CardName {
            nonland_only: true,
        })),
        "a creature type" => Ok(ParsedChoiceKind::CreatureType {
            excluding: BTreeSet::new(),
        }),
        "odd or even" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::LabeledOption(vec!["odd".to_owned(), "even".to_owned()]),
        )),
        "left or right" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::LabeledOption(vec!["left".to_owned(), "right".to_owned()]),
        )),
        "friend or foe" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::LabeledOption(vec!["friend".to_owned(), "foe".to_owned()]),
        )),
        "heads or tails" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::LabeledOption(vec!["heads".to_owned(), "tails".to_owned()]),
        )),
        "khans or dragons" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::LabeledOption(vec!["khans".to_owned(), "dragons".to_owned()]),
        )),
        "mirran or phyrexian" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::LabeledOption(vec![
                "mirran".to_owned(),
                "phyrexian".to_owned(),
            ]),
        )),
        other if other.starts_with("a creature type other than ") => {
            let excl_raw = other
                .trim_start_matches("a creature type other than ")
                .trim();
            let mut excluding = BTreeSet::new();
            for tok in excl_raw
                .replace(" or ", ",")
                .replace(" and ", ",")
                .split(',')
            {
                let t = tok.trim();
                if t.is_empty() {
                    continue;
                }
                let ct = vocabulary
                    .resolve_label(t)
                    .ok_or("oracle:unsupported-choose-option")?;
                excluding.insert(ct);
            }
            Ok(ParsedChoiceKind::CreatureType { excluding })
        }
        other => {
            if let Some(types) = parse_card_type_choice_list(other) {
                return Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::CardType(
                    types,
                )));
            }
            if allow_arbitrary_or_labels && other.contains(" or ") {
                let mut labels = Vec::new();
                for seg in other.split(", or ").flat_map(|s| s.split(" or ")) {
                    for sub in seg.split(',') {
                        let t = sub.trim().trim_start_matches("or ").trim();
                        if !t.is_empty() {
                            labels.push(t.to_string());
                        }
                    }
                }
                if labels.len() >= 2 && labels.len() <= 20 {
                    return Ok(ParsedChoiceKind::Domain(
                        ResolutionChoiceDomain::LabeledOption(labels),
                    ));
                }
            }
            Err("oracle:unsupported-choose-option")
        }
    }
}

pub(super) fn choice_domain_to_player_option_branches(
    domain: &spellwake_engine::program::ResolutionChoiceDomain,
) -> Option<Vec<ChooseOptionBranch>> {
    use spellwake_engine::program::ResolutionChoiceDomain;
    let labels: Vec<String> = match domain {
        ResolutionChoiceDomain::Color => ["White", "Blue", "Black", "Red", "Green"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ResolutionChoiceDomain::BasicLandType => {
            ["Plains", "Island", "Swamp", "Mountain", "Forest"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        }
        ResolutionChoiceDomain::CardType(types) => {
            if types.is_empty() {
                [
                    "Artifact",
                    "Creature",
                    "Enchantment",
                    "Instant",
                    "Land",
                    "Planeswalker",
                    "Sorcery",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            } else {
                types.iter().map(|ct| format!("{ct:?}")).collect()
            }
        }
        ResolutionChoiceDomain::LabeledOption(items) if !items.is_empty() && items.len() <= 20 => {
            items.clone()
        }
        ResolutionChoiceDomain::Number { min, max } if min <= max && max - min <= 20 => {
            (*min..=*max).map(|n| n.to_string()).collect()
        }
        _ => return None,
    };
    Some(
        labels
            .into_iter()
            .map(|label| ChooseOptionBranch {
                label,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Jump { destination: 1 }],
                ),
            })
            .collect(),
    )
}

pub(in super::super) fn emit_choice_for_player_ref(
    chooser: PlayerRefAst,
    kind: ParsedChoiceKind,
    builder: &mut BodyBuilder,
    ids: &mut LoweringIds,
    in_trigger: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::program::ResolutionChoiceDomain;
    match chooser {
        PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer => {
            let ParsedChoiceKind::Domain(domain) = kind else {
                return Err("oracle:unsupported-choose-option");
            };
            let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
            if let Some(options) = choice_domain_to_player_option_branches(&domain) {
                builder
                    .instructions
                    .push(Instruction::ChooseOptionForPlayers { players, options });
            } else {
                push_for_each_player(
                    players,
                    vec![Instruction::ChooseValue {
                        chooser: ResolvingPlayer::LoopPlayer,
                        domain,
                    }],
                    builder,
                );
            }
            Ok(())
        }
        PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer => {
            let ParsedChoiceKind::Domain(domain) = kind else {
                return Err("oracle:unsupported-choose-option");
            };
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: ResolutionChoiceDomain::Player {
                    opponents_only: chooser == PlayerRefAst::AnyOpponent,
                },
            });
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::ChosenPlayer,
                domain,
            });
            Ok(())
        }
        other => {
            let rp = lower_resolving_player_in_context(other, builder, in_trigger)?;
            match kind {
                ParsedChoiceKind::Domain(domain) => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser: rp,
                        domain,
                    });
                    Ok(())
                }
                ParsedChoiceKind::CreatureType { excluding }
                    if rp == ResolvingPlayer::Controller =>
                {
                    let slot = ids.creature_type_slot();
                    builder.instructions.push(Instruction::ChooseCreatureType {
                        into: slot,
                        excluding,
                    });
                    Ok(())
                }
                ParsedChoiceKind::CreatureType { .. } => Err("oracle:unsupported-choose-option"),
            }
        }
    }
}

pub(super) fn classify_graveyard_group(
    filter_zone: Option<ZoneFilter>,
    from_zone: Option<&str>,
) -> Option<bool> {
    match filter_zone {
        Some(ZoneFilter::YourGraveyard) => Some(false),
        Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => Some(true),
        Some(_) => None,
        None => {
            let fz = from_zone?.trim().to_ascii_lowercase();
            if !fz.contains("graveyard") {
                return None;
            }
            if fz.contains("opponent's graveyard")
                || fz.contains("target player's graveyard")
                || fz.contains("that player's graveyard")
                || fz.contains("defending player's graveyard")
            {
                None
            } else if fz.contains("your graveyard") {
                Some(false)
            } else if fz.contains("all graveyards") || fz.contains("graveyards") {
                Some(true)
            } else {
                None
            }
        }
    }
}

pub(super) fn is_self_anaphora_label(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "it" | "its"
            | "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
    )
}

pub(super) fn is_self_anaphora_ref(subject: &SubjectRefAst) -> bool {
    matches!(subject, SubjectRefAst::Anaphora(p) if is_self_anaphora_label(p))
}

pub(super) fn value_expr_ast_has_self_anaphora(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            is_self_anaphora_ref(subject)
        }
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            value_expr_ast_has_self_anaphora(a) || value_expr_ast_has_self_anaphora(b)
        }
        ValueExprAst::DivConst { expr: inner, .. } => value_expr_ast_has_self_anaphora(inner),
        _ => false,
    }
}

pub(super) fn rewrite_value_expr_ast_self_anaphora(expr: &ValueExprAst) -> ValueExprAst {
    match expr {
        ValueExprAst::ObjectStat { subject, stat } if is_self_anaphora_ref(subject) => {
            ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::SelfObject),
                stat: *stat,
            }
        }
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if is_self_anaphora_ref(subject) => ValueExprAst::CounterCount {
            subject: Box::new(SubjectRefAst::SelfObject),
            counter_kind: counter_kind.clone(),
        },
        ValueExprAst::Add(a, b) => ValueExprAst::Add(
            Box::new(rewrite_value_expr_ast_self_anaphora(a)),
            Box::new(rewrite_value_expr_ast_self_anaphora(b)),
        ),
        ValueExprAst::Sub(a, b) => ValueExprAst::Sub(
            Box::new(rewrite_value_expr_ast_self_anaphora(a)),
            Box::new(rewrite_value_expr_ast_self_anaphora(b)),
        ),
        ValueExprAst::Mul(a, b) => ValueExprAst::Mul(
            Box::new(rewrite_value_expr_ast_self_anaphora(a)),
            Box::new(rewrite_value_expr_ast_self_anaphora(b)),
        ),
        ValueExprAst::DivConst {
            expr: inner,
            denom,
            rounding,
        } => ValueExprAst::DivConst {
            expr: Box::new(rewrite_value_expr_ast_self_anaphora(inner)),
            denom: *denom,
            rounding: *rounding,
        },
        _ => expr.clone(),
    }
}

pub(super) fn infer_instruction_actor(ins: &InstructionAst) -> Option<PlayerRefAst> {
    match ins {
        InstructionAst::Discard { player, .. }
        | InstructionAst::DiscardHand { player }
        | InstructionAst::DiscardUnless { player, .. }
        | InstructionAst::DiscardUpTo { player, .. }
        | InstructionAst::Sacrifice { player, .. }
        | InstructionAst::SacrificeAnyNumber { player, .. }
        | InstructionAst::PlayerPayCost { player, .. }
        | InstructionAst::PayAnyAmountOfMana { player }
        | InstructionAst::SkipNextTurn { player }
        | InstructionAst::SkipNextTurns { player, .. }
        | InstructionAst::PlayerLookTopLibrary { player, .. }
        | InstructionAst::TemporaryPlayerRestriction { player, .. }
        | InstructionAst::ChangeLife { player, .. }
        | InstructionAst::SetLifeTotal { player, .. }
        | InstructionAst::Draw { player, .. }
        | InstructionAst::Mill { player, .. }
        | InstructionAst::Scry { player, .. }
        | InstructionAst::Surveil { player, .. }
        | InstructionAst::GainPlayerCounters { player, .. }
        | InstructionAst::ShuffleLibrary { player }
        | InstructionAst::ShuffleGraveyardIntoLibrary { player }
        | InstructionAst::RevealHand { player }
        | InstructionAst::RevealByPlayer { player, .. }
        | InstructionAst::RevealRandomFromHand { player }
        | InstructionAst::LookAtHand { player }
        | InstructionAst::AddMana { player, .. }
        | InstructionAst::Investigate { player, .. }
        | InstructionAst::Blight { player, .. }
        | InstructionAst::OptionalPlayer { player, .. } => Some(*player),
        InstructionAst::AddQualifiedMana(qm) => Some(qm.player),
        InstructionAst::CreateToken { controller, .. }
        | InstructionAst::GainControl { controller, .. } => Some(*controller),
        InstructionAst::Exile {
            actor: Some(player),
            ..
        }
        | InstructionAst::ExileTopLibrary {
            actor: Some(player),
            ..
        }
        | InstructionAst::MoveCounted {
            actor: Some(player),
            ..
        } => Some(*player),
        InstructionAst::Optional(inner) => inner.first().and_then(infer_instruction_actor),
        InstructionAst::MoveZone { subject, .. } | InstructionAst::ReturnZone { subject, .. } => {
            match subject.as_ref() {
                SubjectRefAst::Group(f)
                    if matches!(
                        f.zone,
                        Some(ZoneFilter::ThatPlayerHand | ZoneFilter::ThatPlayerGraveyard)
                    ) =>
                {
                    Some(PlayerRefAst::ThatPlayer)
                }
                _ => None,
            }
        }
        InstructionAst::ExileGraveyard { whose } => {
            match whose.trim().to_ascii_lowercase().as_str() {
                "that player" | "that player's" | "their" | "their graveyard" => {
                    Some(PlayerRefAst::ThatPlayer)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn splice_probed_body(
    builder: &mut BodyBuilder,
    probed: BodyBuilder,
) -> Result<(), &'static str> {
    if builder.graveyard_target_bounds.is_none() {
        builder.graveyard_target_bounds = probed.graveyard_target_bounds;
    }
    builder.x_target_count |= probed.x_target_count;
    if probed.last_group_filter.is_some() {
        builder.last_group_filter = probed.last_group_filter;
    }
    let start = builder.instructions.len();
    let shift = isize::try_from(start).map_err(|_| "oracle:jump-shift-ovf")?;
    for mut op in probed.instructions {
        op.shift_jumps(shift, 0);
        builder.instructions.push(op);
    }
    Ok(())
}

pub(super) fn is_graveyard_body_target(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::SingleOwnGraveyardCard
            | BodyTarget::FilteredOwnGraveyardCard(_)
            | BodyTarget::FilteredAnyGraveyardCard { .. }
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::RecentlyDiscardedOrFromLibrary
    )
}

pub(super) fn is_hand_return_dest(dest: &str) -> bool {
    matches!(
        dest,
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
    )
}

pub(super) fn parse_return_with_counter(
    with_counter: Option<&str>,
) -> Result<Option<(CounterKind, u8)>, &'static str> {
    let Some(raw) = with_counter else {
        return Ok(None);
    };
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
    let ck = parse_counter_kind(kind_str).ok_or("oracle:unsupported-counter-kind")?;
    Ok(Some((ck, qty)))
}

pub(super) fn rewrite_loop_player_value_expr(
    e: &ValueExprAst,
    loop_player: PlayerRefAst,
) -> ValueExprAst {
    let map_p = |p: PlayerRefAst| -> PlayerRefAst {
        if p == loop_player {
            PlayerRefAst::ThatPlayer
        } else {
            p
        }
    };
    match e {
        ValueExprAst::Count(f) => {
            ValueExprAst::Count(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        ValueExprAst::ObjectStat { subject, stat } => ValueExprAst::ObjectStat {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            stat: *stat,
        },
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } => ValueExprAst::CounterCount {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            counter_kind: counter_kind.clone(),
        },
        ValueExprAst::PlayerStat { player, stat } => ValueExprAst::PlayerStat {
            player: map_p(*player),
            stat: *stat,
        },
        ValueExprAst::AggregateStat { op, stat, filter } => ValueExprAst::AggregateStat {
            op: *op,
            stat: *stat,
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
        },
        ValueExprAst::Add(a, b) => ValueExprAst::Add(
            Box::new(rewrite_loop_player_value_expr(a, loop_player)),
            Box::new(rewrite_loop_player_value_expr(b, loop_player)),
        ),
        ValueExprAst::Sub(a, b) => ValueExprAst::Sub(
            Box::new(rewrite_loop_player_value_expr(a, loop_player)),
            Box::new(rewrite_loop_player_value_expr(b, loop_player)),
        ),
        ValueExprAst::Mul(a, b) => ValueExprAst::Mul(
            Box::new(rewrite_loop_player_value_expr(a, loop_player)),
            Box::new(rewrite_loop_player_value_expr(b, loop_player)),
        ),
        ValueExprAst::DivConst {
            expr,
            denom,
            rounding,
        } => ValueExprAst::DivConst {
            expr: Box::new(rewrite_loop_player_value_expr(expr, loop_player)),
            denom: *denom,
            rounding: *rounding,
        },
        ValueExprAst::ColorsAmong(f) => {
            ValueExprAst::ColorsAmong(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        ValueExprAst::CardTypesAmong(f) => {
            ValueExprAst::CardTypesAmong(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        ValueExprAst::SubtypesAmong(f) => {
            ValueExprAst::SubtypesAmong(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        ValueExprAst::PlayerCounterCount {
            player,
            counter_kind,
        } => ValueExprAst::PlayerCounterCount {
            player: map_p(*player),
            counter_kind: counter_kind.clone(),
        },
        ValueExprAst::StartingLifeTotal(p) => ValueExprAst::StartingLifeTotal(map_p(*p)),
        ValueExprAst::LifeChangedThisTurn { player, gained } => ValueExprAst::LifeChangedThisTurn {
            player: map_p(*player),
            gained: *gained,
        },
        ValueExprAst::ExtremeStatAmong {
            stat,
            greatest,
            filter,
            or_toughness,
        } => ValueExprAst::ExtremeStatAmong {
            stat: *stat,
            greatest: *greatest,
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            or_toughness: *or_toughness,
        },
        ValueExprAst::DistinctCountAmong { category, filter } => ValueExprAst::DistinctCountAmong {
            category: category.clone(),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
        },
        ValueExprAst::CounterCountAmong {
            counter_kind,
            filter,
        } => ValueExprAst::CounterCountAmong {
            counter_kind: counter_kind.clone(),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
        },
        ValueExprAst::EventCountThisTurn { event, player } => ValueExprAst::EventCountThisTurn {
            event: event.clone(),
            player: player.map(map_p),
        },
        ValueExprAst::Conditional {
            condition,
            then_expr,
            else_expr,
        } => ValueExprAst::Conditional {
            condition: Box::new(rewrite_loop_player_condition(condition, loop_player)),
            then_expr: Box::new(rewrite_loop_player_value_expr(then_expr, loop_player)),
            else_expr: Box::new(rewrite_loop_player_value_expr(else_expr, loop_player)),
        },
        ValueExprAst::DomainCountFor(p) => ValueExprAst::DomainCountFor(map_p(*p)),
        ValueExprAst::CountDrawnThisWay(p) => ValueExprAst::CountDrawnThisWay(map_p(*p)),
        ValueExprAst::LifePaidThisWay(p) => ValueExprAst::LifePaidThisWay(map_p(*p)),
        ValueExprAst::UnspentManaAmount(p) => ValueExprAst::UnspentManaAmount(map_p(*p)),
        other => other.clone(),
    }
}

pub(super) fn rewrite_loop_player_condition(
    c: &ConditionAst,
    loop_player: PlayerRefAst,
) -> ConditionAst {
    let map_p = |p: PlayerRefAst| -> PlayerRefAst {
        if p == loop_player {
            PlayerRefAst::ThatPlayer
        } else {
            p
        }
    };
    match c {
        ConditionAst::Compare { lhs, op, rhs } => ConditionAst::Compare {
            lhs: Box::new(rewrite_loop_player_value_expr(lhs, loop_player)),
            op: *op,
            rhs: Box::new(rewrite_loop_player_value_expr(rhs, loop_player)),
        },
        ConditionAst::ExprCompare { lhs, op, rhs } => ConditionAst::ExprCompare {
            lhs: Box::new(rewrite_loop_player_value_expr(lhs, loop_player)),
            op: *op,
            rhs: Box::new(rewrite_loop_player_value_expr(rhs, loop_player)),
        },
        ConditionAst::ControlsCount {
            player,
            filter,
            op,
            value,
        } => ConditionAst::ControlsCount {
            player: map_p(*player),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            op: *op,
            value: *value,
        },
        ConditionAst::ZoneObjectCount { filter, op, value } => ConditionAst::ZoneObjectCount {
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            op: *op,
            value: *value,
        },
        ConditionAst::LifeCompare { player, op, value } => ConditionAst::LifeCompare {
            player: map_p(*player),
            op: *op,
            value: *value,
        },
        ConditionAst::HandCount { player, op, value } => ConditionAst::HandCount {
            player: map_p(*player),
            op: *op,
            value: *value,
        },
        ConditionAst::GraveyardCount { player, op, value } => ConditionAst::GraveyardCount {
            player: map_p(*player),
            op: *op,
            value: *value,
        },
        ConditionAst::PlayerCounterCompare {
            player,
            counter_kind,
            op,
            value,
        } => ConditionAst::PlayerCounterCompare {
            player: map_p(*player),
            counter_kind: counter_kind.clone(),
            op: *op,
            value: *value,
        },
        ConditionAst::CastSpellThisTurn { player, filter } => ConditionAst::CastSpellThisTurn {
            player: map_p(*player),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
        },
        ConditionAst::PlayerActionThisTurn { player, action } => {
            ConditionAst::PlayerActionThisTurn {
                player: map_p(*player),
                action: action.clone(),
            }
        }
        ConditionAst::LostLifeThisTurn { player, min_amount } => ConditionAst::LostLifeThisTurn {
            player: map_p(*player),
            min_amount: *min_amount,
        },
        ConditionAst::IsPlayersTurn(p) => ConditionAst::IsPlayersTurn(map_p(*p)),
        ConditionAst::ZoneChangeThisTurn {
            filter,
            from_zone,
            to_zone,
            controller,
            min_count,
        } => ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            from_zone: *from_zone,
            to_zone: *to_zone,
            controller: controller.map(map_p),
            min_count: *min_count,
        },
        ConditionAst::TargetsSubject { subject, target } => ConditionAst::TargetsSubject {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            target: Box::new(rewrite_loop_player_subject(target, loop_player)),
        },
        ConditionAst::AttachedTo { subject, host } => ConditionAst::AttachedTo {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            host: Box::new(rewrite_loop_player_subject(host, loop_player)),
        },
        ConditionAst::SameNameAs { subject, other } => ConditionAst::SameNameAs {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            other: Box::new(rewrite_loop_player_subject(other, loop_player)),
        },
        ConditionAst::BlockedByFilter {
            subject,
            blocker_filter,
            min_count,
        } => ConditionAst::BlockedByFilter {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            blocker_filter: Box::new(rewrite_loop_player_filter(blocker_filter, loop_player)),
            min_count: *min_count,
        },
        ConditionAst::PlayerControlsMost {
            player,
            filter,
            allow_tie,
        } => ConditionAst::PlayerControlsMost {
            player: map_p(*player),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            allow_tie: *allow_tie,
        },
        ConditionAst::ControlsMoreThan {
            player,
            filter,
            than_player,
        } => ConditionAst::ControlsMoreThan {
            player: map_p(*player),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            than_player: map_p(*than_player),
        },
        ConditionAst::AttackedWithFilteredThisTurn {
            player,
            filter,
            min_count,
        } => ConditionAst::AttackedWithFilteredThisTurn {
            player: map_p(*player),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            min_count: *min_count,
        },
        ConditionAst::SubjectControlledBy { subject, player } => {
            ConditionAst::SubjectControlledBy {
                subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
                player: map_p(*player),
            }
        }
        ConditionAst::SubjectInZone { subject, zone } => ConditionAst::SubjectInZone {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            zone: *zone,
        },
        ConditionAst::TopCardMatches { zone, filter } => ConditionAst::TopCardMatches {
            zone: *zone,
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
        },
        ConditionAst::SharesPropertyWith {
            subject,
            other,
            property,
        } => ConditionAst::SharesPropertyWith {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            other: Box::new(rewrite_loop_player_subject(other, loop_player)),
            property: property.clone(),
        },
        ConditionAst::PlayerCoinFlipOutcome { player, outcome } => {
            ConditionAst::PlayerCoinFlipOutcome {
                player: map_p(*player),
                outcome: outcome.clone(),
            }
        }
        ConditionAst::DieRollCompare {
            player,
            op,
            value,
            any_of_group,
            exact_times,
        } => ConditionAst::DieRollCompare {
            player: map_p(*player),
            op: *op,
            value: *value,
            any_of_group: *any_of_group,
            exact_times: *exact_times,
        },
        ConditionAst::CardWithNameInZone {
            name_filter,
            filter,
            zone,
            op,
            count,
        } => ConditionAst::CardWithNameInZone {
            name_filter: name_filter.clone(),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            zone: *zone,
            op: *op,
            count: *count,
        },
        ConditionAst::IsPhaseOrStep {
            player,
            phase,
            first_only,
        } => ConditionAst::IsPhaseOrStep {
            player: player.map(map_p),
            phase: phase.clone(),
            first_only: *first_only,
        },
        ConditionAst::IsStartingPlayer(p) => ConditionAst::IsStartingPlayer(map_p(*p)),
        ConditionAst::LifeComparePlayers { player, op, other } => {
            ConditionAst::LifeComparePlayers {
                player: map_p(*player),
                op: *op,
                other: map_p(*other),
            }
        }
        ConditionAst::DamageDealtThisTurn {
            source,
            target_filter,
            combat_only,
            min_damage,
        } => ConditionAst::DamageDealtThisTurn {
            source: Box::new(rewrite_loop_player_subject(source, loop_player)),
            target_filter: target_filter
                .as_deref()
                .map(|f| Box::new(rewrite_loop_player_filter(f, loop_player))),
            combat_only: *combat_only,
            min_damage: *min_damage,
        },
        ConditionAst::PriorActionOccurred {
            action,
            subject_filter,
            min_count,
        } => ConditionAst::PriorActionOccurred {
            action: action.clone(),
            subject_filter: subject_filter
                .as_deref()
                .map(|f| Box::new(rewrite_loop_player_filter(f, loop_player))),
            min_count: *min_count,
        },
        ConditionAst::WasCastFromZone { subject, zone } => ConditionAst::WasCastFromZone {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            zone: *zone,
        },
        ConditionAst::PaidAltCost { subject, cost_kind } => ConditionAst::PaidAltCost {
            subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
            cost_kind: cost_kind.clone(),
        },
        ConditionAst::AllColors(subj) => {
            ConditionAst::AllColors(Box::new(rewrite_loop_player_subject(subj, loop_player)))
        }
        ConditionAst::ControlsEachColor(p) => ConditionAst::ControlsEachColor(map_p(*p)),
        ConditionAst::GainedLifeAtLeastThisTurn { player, amount } => {
            ConditionAst::GainedLifeAtLeastThisTurn {
                player: map_p(*player),
                amount: *amount,
            }
        }
        ConditionAst::Not(inner) => {
            ConditionAst::Not(Box::new(rewrite_loop_player_condition(inner, loop_player)))
        }
        ConditionAst::And(a, b) => ConditionAst::And(
            Box::new(rewrite_loop_player_condition(a, loop_player)),
            Box::new(rewrite_loop_player_condition(b, loop_player)),
        ),
        ConditionAst::Or(a, b) => ConditionAst::Or(
            Box::new(rewrite_loop_player_condition(a, loop_player)),
            Box::new(rewrite_loop_player_condition(b, loop_player)),
        ),
        other => other.clone(),
    }
}

pub(super) fn rewrite_loop_player_cost_atom(
    cost: &CostAtomAst,
    loop_player: PlayerRefAst,
) -> CostAtomAst {
    match cost {
        CostAtomAst::PayLife(v) => {
            CostAtomAst::PayLife(Box::new(rewrite_loop_player_value_expr(v, loop_player)))
        }
        CostAtomAst::Sacrifice {
            count,
            filter,
            up_to,
            min_one,
        } => CostAtomAst::Sacrifice {
            count: Box::new(rewrite_loop_player_value_expr(count, loop_player)),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            up_to: *up_to,
            min_one: *min_one,
        },
        CostAtomAst::SacrificeAll(f) => {
            CostAtomAst::SacrificeAll(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        CostAtomAst::Discard {
            count,
            filter,
            random,
            up_to,
            any_number,
        } => CostAtomAst::Discard {
            count: Box::new(rewrite_loop_player_value_expr(count, loop_player)),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            random: *random,
            up_to: *up_to,
            any_number: *any_number,
        },
        CostAtomAst::Exile {
            count,
            filter,
            face_down,
            face_up,
            single_graveyard,
            any_number,
            min_one,
            total_mv,
        } => CostAtomAst::Exile {
            count: Box::new(rewrite_loop_player_value_expr(count, loop_player)),
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            face_down: *face_down,
            face_up: *face_up,
            single_graveyard: *single_graveyard,
            any_number: *any_number,
            min_one: *min_one,
            total_mv: total_mv.as_ref().map(|(op, e)| {
                (
                    *op,
                    Box::new(rewrite_loop_player_value_expr(e, loop_player)),
                )
            }),
        },
        CostAtomAst::Mill(v) => {
            CostAtomAst::Mill(Box::new(rewrite_loop_player_value_expr(v, loop_player)))
        }
        CostAtomAst::Blight(v) => {
            CostAtomAst::Blight(Box::new(rewrite_loop_player_value_expr(v, loop_player)))
        }
        CostAtomAst::CollectEvidence(v) => {
            CostAtomAst::CollectEvidence(Box::new(rewrite_loop_player_value_expr(v, loop_player)))
        }
        CostAtomAst::Or(a, b) => CostAtomAst::Or(
            Box::new(rewrite_loop_player_cost_atom(a, loop_player)),
            Box::new(rewrite_loop_player_cost_atom(b, loop_player)),
        ),
        other => other.clone(),
    }
}

pub(super) fn rewrite_loop_player_library_str(raw: &str, loop_player: PlayerRefAst) -> String {
    let low = raw.trim().to_ascii_lowercase();
    if loop_player == PlayerRefAst::You
        && matches!(
            low.as_str(),
            "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary"
        )
    {
        return "that player's".to_string();
    }
    raw.to_string()
}

pub(super) fn rewrite_loop_player_ast(
    ins: &InstructionAst,
    loop_player: PlayerRefAst,
) -> InstructionAst {
    let map_p = |p: PlayerRefAst| -> PlayerRefAst {
        if p == loop_player {
            PlayerRefAst::ThatPlayer
        } else {
            p
        }
    };
    let map_subj = |s: &SubjectRefAst| rewrite_loop_player_subject(s, loop_player);
    let map_ve = |e: &ValueExprAst| rewrite_loop_player_value_expr(e, loop_player);
    let map_flt = |f: &ObjectFilterAst| rewrite_loop_player_filter(f, loop_player);
    let map_seq = |seq: &[InstructionAst]| -> Vec<InstructionAst> {
        seq.iter()
            .map(|s| rewrite_loop_player_ast(s, loop_player))
            .collect()
    };
    match ins {
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } => InstructionAst::Draw {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
            up_to: *up_to,
        },
        InstructionAst::Mill { player, count } => InstructionAst::Mill {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::Scry { player, count } => InstructionAst::Scry {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::Surveil { player, count } => InstructionAst::Surveil {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => InstructionAst::ChangeLife {
            player: map_p(*player),
            delta: Box::new(map_ve(delta)),
            gain: *gain,
        },
        InstructionAst::SetLifeTotal { player, value } => InstructionAst::SetLifeTotal {
            player: map_p(*player),
            value: Box::new(map_ve(value)),
        },
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } => InstructionAst::Discard {
            player: map_p(*player),
            count: count.as_deref().map(|c| Box::new(map_ve(c))),
            filter: filter.as_deref().map(|f| Box::new(map_flt(f))),
            anaphora: anaphora.clone(),
            random: *random,
        },
        InstructionAst::DiscardHand { player } => InstructionAst::DiscardHand {
            player: map_p(*player),
        },
        InstructionAst::DiscardUnless {
            player,
            count,
            unless_filter,
        } => InstructionAst::DiscardUnless {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
            unless_filter: Box::new(map_flt(unless_filter)),
        },
        InstructionAst::DiscardUpTo {
            player,
            max_count,
            filter,
            any_number,
            all,
            random,
        } => InstructionAst::DiscardUpTo {
            player: map_p(*player),
            max_count: Box::new(map_ve(max_count)),
            filter: filter.as_deref().map(|f| Box::new(map_flt(f))),
            any_number: *any_number,
            all: *all,
            random: *random,
        },
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except,
            half,
            rounding,
            chooser,
        } => InstructionAst::Sacrifice {
            player: map_p(*player),
            count: count.as_deref().map(|c| Box::new(map_ve(c))),
            subject: Box::new(map_subj(subject)),
            except: except.as_deref().map(|e| Box::new(map_subj(e))),
            half: *half,
            rounding: *rounding,
            chooser: chooser.map(map_p),
        },
        InstructionAst::Exile {
            actor,
            subject,
            random,
            face_down,
            into_pile,
        } => InstructionAst::Exile {
            actor: actor.map(map_p),
            subject: Box::new(map_subj(subject)),
            random: *random,
            face_down: *face_down,
            into_pile: *into_pile,
        },
        InstructionAst::ExileTopLibrary {
            actor,
            whose,
            count,
            face_down,
        } => InstructionAst::ExileTopLibrary {
            actor: actor.map(map_p),
            whose: rewrite_loop_player_library_str(whose, loop_player),
            count: Box::new(map_ve(count)),
            face_down: *face_down,
        },
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
        } => InstructionAst::ReturnZone {
            subject: Box::new(map_subj(subject)),
            from_zone: from_zone.clone(),
            to: to.clone(),
            tapped: *tapped,
            transformed: *transformed,
            with_counter: with_counter.clone(),
            attached_to: attached_to.as_deref().map(|a| Box::new(map_subj(a))),
            random: *random,
            except: except.as_deref().map(|e| Box::new(map_subj(e))),
            count: count.as_deref().map(|c| Box::new(map_ve(c))),
            up_to: *up_to,
            any_number: *any_number,
            actor: actor.map(map_p),
        },
        InstructionAst::MoveZone { subject, to } => InstructionAst::MoveZone {
            subject: Box::new(map_subj(subject)),
            to: to.clone(),
        },
        InstructionAst::MoveCounted {
            actor,
            count,
            up_to,
            any_number,
            subject,
            to,
        } => InstructionAst::MoveCounted {
            actor: actor.map(map_p),
            count: count.as_deref().map(|c| Box::new(map_ve(c))),
            up_to: *up_to,
            any_number: *any_number,
            subject: Box::new(map_subj(subject)),
            to: to.clone(),
        },
        InstructionAst::AddMana { player, mana } => InstructionAst::AddMana {
            player: map_p(*player),
            mana: mana.clone(),
        },
        InstructionAst::GainControl {
            controller,
            subject,
            duration,
        } => InstructionAst::GainControl {
            controller: map_p(*controller),
            subject: Box::new(map_subj(subject)),
            duration: duration.clone(),
        },
        InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject,
            free,
            duration,
            count_limit,
            alt_cost,
        } => InstructionAst::GrantCastPermission {
            player: map_p(*player),
            cast_only: *cast_only,
            subject: Box::new(map_subj(subject)),
            free: *free,
            duration: duration.clone(),
            count_limit: count_limit.clone(),
            alt_cost: alt_cost.clone(),
        },
        InstructionAst::GrantModifiedCastPermission(perm) => {
            let mut cloned = perm.as_ref().clone();
            cloned.player = map_p(cloned.player);
            cloned.subject = Box::new(map_subj(&cloned.subject));
            InstructionAst::GrantModifiedCastPermission(Box::new(cloned))
        }
        InstructionAst::Blight { player, count } => InstructionAst::Blight {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::LookTopLibrary {
            reveal,
            library,
            count,
        } => InstructionAst::LookTopLibrary {
            reveal: *reveal,
            library: rewrite_loop_player_library_str(library, loop_player),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::SearchLibrary {
            library,
            count,
            up_to,
            any_number,
            exact,
            filter,
            reveal,
            to_zone,
            shuffle,
            extra_zones,
            secondary_filter,
            secondary_is_or,
            where_x,
            rest_to_zone,
            modifier,
            follow_up,
        } => InstructionAst::SearchLibrary {
            library: rewrite_loop_player_library_str(library, loop_player),
            count: Box::new(map_ve(count)),
            up_to: *up_to,
            any_number: *any_number,
            exact: *exact,
            filter: Box::new(map_flt(filter)),
            reveal: *reveal,
            to_zone: to_zone.clone(),
            shuffle: *shuffle,
            extra_zones: extra_zones.clone(),
            secondary_filter: secondary_filter.as_deref().map(|f| Box::new(map_flt(f))),
            secondary_is_or: *secondary_is_or,
            where_x: where_x.as_deref().map(|e| Box::new(map_ve(e))),
            rest_to_zone: rest_to_zone.clone(),
            modifier: *modifier,
            follow_up: map_seq(follow_up),
        },
        InstructionAst::PlayerPayCost { player, cost } => InstructionAst::PlayerPayCost {
            player: map_p(*player),
            cost: rewrite_loop_player_cost_atom(cost, loop_player),
        },
        InstructionAst::PayCost(cost) => InstructionAst::PlayerPayCost {
            player: PlayerRefAst::ThatPlayer,
            cost: rewrite_loop_player_cost_atom(cost, loop_player),
        },
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => InstructionAst::GainPlayerCounters {
            player: map_p(*player),
            counter_kind: counter_kind.clone(),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::Investigate { player, count } => InstructionAst::Investigate {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
        },
        InstructionAst::VentureIntoDungeon { player } => InstructionAst::VentureIntoDungeon {
            player: map_p(*player),
        },
        InstructionAst::TakeInitiative { player } => InstructionAst::TakeInitiative {
            player: map_p(*player),
        },
        InstructionAst::WinGame { player } => InstructionAst::WinGame {
            player: map_p(*player),
        },
        InstructionAst::LoseGame { player } => InstructionAst::LoseGame {
            player: map_p(*player),
        },
        InstructionAst::RevealHand { player } => InstructionAst::RevealHand {
            player: map_p(*player),
        },
        InstructionAst::RevealByPlayer { player, subject } => InstructionAst::RevealByPlayer {
            player: map_p(*player),
            subject: Box::new(map_subj(subject)),
        },
        InstructionAst::RevealCards { subject } => InstructionAst::RevealCards {
            subject: Box::new(map_subj(subject)),
        },
        InstructionAst::SkipNextTurn { player } => InstructionAst::SkipNextTurn {
            player: map_p(*player),
        },
        InstructionAst::SkipNextTurns { player, count } => InstructionAst::SkipNextTurns {
            player: map_p(*player),
            count: *count,
        },
        InstructionAst::PlayerLookTopLibrary {
            player,
            library,
            count,
            reveal,
        } => InstructionAst::PlayerLookTopLibrary {
            player: map_p(*player),
            library: rewrite_loop_player_library_str(library, loop_player),
            count: Box::new(map_ve(count)),
            reveal: *reveal,
        },
        InstructionAst::PayAnyAmountOfMana { player } => InstructionAst::PayAnyAmountOfMana {
            player: map_p(*player),
        },
        InstructionAst::ChoosePlayerTarget { player } => InstructionAst::ChoosePlayerTarget {
            player: map_p(*player),
        },
        InstructionAst::AddQualifiedMana(qm) => {
            let mut cloned = qm.as_ref().clone();
            cloned.player = map_p(cloned.player);
            if let Some(amt) = cloned.amount.as_deref() {
                cloned.amount = Some(Box::new(map_ve(amt)));
            }
            if let Some(src) = cloned.source.as_deref() {
                cloned.source = Some(Box::new(map_subj(src)));
            }
            InstructionAst::AddQualifiedMana(Box::new(cloned))
        }
        InstructionAst::Earthbend { count } => InstructionAst::Earthbend {
            count: Box::new(map_ve(count)),
        },
        InstructionAst::Regenerate { subject } => InstructionAst::Regenerate {
            subject: Box::new(map_subj(subject)),
        },
        InstructionAst::Attach {
            subject,
            target,
            any_number,
            all,
        } => InstructionAst::Attach {
            subject: Box::new(map_subj(subject)),
            target: Box::new(map_subj(target)),
            any_number: *any_number,
            all: *all,
        },
        InstructionAst::ScopedDuration { duration, body } => InstructionAst::ScopedDuration {
            duration: duration.clone(),
            body: map_seq(body),
        },
        InstructionAst::ScopedDurationText {
            duration_text,
            body,
        } => InstructionAst::ScopedDurationText {
            duration_text: duration_text.clone(),
            body: map_seq(body),
        },
        InstructionAst::TemporaryPlayerRestriction {
            player,
            subject,
            rule,
            duration,
        } => InstructionAst::TemporaryPlayerRestriction {
            player: map_p(*player),
            subject: subject.as_deref().map(|s| Box::new(map_subj(s))),
            rule: rule.clone(),
            duration: duration.clone(),
        },
        InstructionAst::ShuffleLibrary { player } => InstructionAst::ShuffleLibrary {
            player: map_p(*player),
        },
        InstructionAst::ShuffleGraveyardIntoLibrary { player } => {
            InstructionAst::ShuffleGraveyardIntoLibrary {
                player: map_p(*player),
            }
        }
        InstructionAst::CreateToken { controller, token } => InstructionAst::CreateToken {
            controller: map_p(*controller),
            token: token.clone(),
        },
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => InstructionAst::DealDamage {
            source: Box::new(map_subj(source)),
            target: Box::new(map_subj(target)),
            amount: Box::new(map_ve(amount)),
        },
        InstructionAst::Optional(inner) => InstructionAst::OptionalPlayer {
            player: PlayerRefAst::ThatPlayer,
            body: map_seq(inner),
        },
        InstructionAst::OptionalPlayer { player, body } => InstructionAst::OptionalPlayer {
            player: map_p(*player),
            body: map_seq(body),
        },
        InstructionAst::BindX { x_expr, body } => InstructionAst::BindX {
            x_expr: Box::new(map_ve(x_expr)),
            body: map_seq(body),
        },
        InstructionAst::Unless {
            body,
            unless_player,
            unless_cost,
            unless_action,
            unless_condition,
        } => InstructionAst::Unless {
            body: map_seq(body),
            unless_player: unless_player.map(map_p),
            unless_cost: unless_cost
                .as_ref()
                .map(|c| rewrite_loop_player_cost_atom(c, loop_player)),
            unless_action: unless_action.as_deref().map(map_seq),
            unless_condition: unless_condition
                .as_deref()
                .map(|c| Box::new(rewrite_loop_player_condition(c, loop_player))),
        },
        InstructionAst::Choice(branches) => {
            InstructionAst::Choice(branches.iter().map(|b| map_seq(b)).collect())
        }
        InstructionAst::IfCondition {
            condition,
            then_body,
        } => InstructionAst::IfCondition {
            condition: Box::new(rewrite_loop_player_condition(condition, loop_player)),
            then_body: map_seq(then_body),
        },
        InstructionAst::IfPreviousSucceeded(inner) => {
            InstructionAst::IfPreviousSucceeded(map_seq(inner))
        }
        InstructionAst::IfPreviousFailed(inner) => InstructionAst::IfPreviousFailed(map_seq(inner)),
        InstructionAst::Otherwise(inner) => InstructionAst::Otherwise(map_seq(inner)),
        other => other.clone(),
    }
}

pub(super) fn rewrite_chosen_to_loop_rp(rp: &mut ResolvingPlayer) {
    if *rp == ResolvingPlayer::ChosenPlayer {
        *rp = ResolvingPlayer::LoopPlayer;
    }
}

pub(super) fn rewrite_chosen_to_loop_ap(ap: &mut AffectedPlayers) {
    if let AffectedPlayers::One(rp) = ap {
        rewrite_chosen_to_loop_rp(rp);
    }
}

pub(super) fn rewrite_chosen_to_loop_mana_yield(yield_kind: &mut EffectManaYield) {
    match yield_kind {
        EffectManaYield::RetainedUntilCleanup(inner) => rewrite_chosen_to_loop_mana_yield(inner),
        EffectManaYield::RetainedUntilEndOfCombat { amount, .. }
        | EffectManaYield::Counted { amount, .. }
        | EffectManaYield::RestrictedCounted { amount, .. }
        | EffectManaYield::Split { amount, .. }
        | EffectManaYield::RestrictedChoice { amount, .. }
        | EffectManaYield::Choice { amount, .. }
        | EffectManaYield::CommanderIdentity { amount }
        | EffectManaYield::SourceChosenColor { amount }
        | EffectManaYield::DistinctColors { amount } => rewrite_chosen_to_loop_ve(amount),
        EffectManaYield::LandProducible { controller, amount } => {
            rewrite_chosen_to_loop_ap(controller);
            rewrite_chosen_to_loop_ve(amount);
        }
        EffectManaYield::Fixed(_)
        | EffectManaYield::CombatDamageUntilCleanup(_)
        | EffectManaYield::RestrictedFixed { .. }
        | EffectManaYield::ChoiceBundle { .. } => {}
    }
}

pub(super) fn rewrite_chosen_to_loop_pl(pl: &mut LibraryPlacement) {
    if let LibraryPlacement::ChoiceTopOrBottom { chooser } = pl {
        rewrite_chosen_to_loop_rp(chooser);
    }
}

pub(super) fn rewrite_chosen_to_loop_dest(dest: &mut SubjectDestination) {
    if let SubjectDestination::Library(pl) = dest {
        rewrite_chosen_to_loop_pl(pl);
    }
}

pub(super) fn rewrite_chosen_to_loop_dg(dg: &mut DamageGroup) {
    match dg {
        DamageGroup::AffectedPlayers(ap) | DamageGroup::FilteredAndPlayers { players: ap, .. } => {
            rewrite_chosen_to_loop_ap(ap);
        }
        _ => {}
    }
}

pub(super) fn rewrite_chosen_to_loop_ve(ve: &mut ValueExpr) {
    match ve {
        ValueExpr::Count {
            zone,
            controller_scope,
            ..
        } => {
            match zone {
                ValueZone::Graveyard(rp) | ValueZone::Hand(rp) | ValueZone::Library(rp) => {
                    rewrite_chosen_to_loop_rp(rp);
                }
                ValueZone::Exile(Some(rp)) => rewrite_chosen_to_loop_rp(rp),
                _ => {}
            }
            if let Some(rp) = controller_scope {
                rewrite_chosen_to_loop_rp(rp);
            }
        }
        ValueExpr::OpponentMaxCount { zone, .. } | ValueExpr::OpponentMinCount { zone, .. } => {
            match zone {
                ValueZone::Graveyard(rp) | ValueZone::Hand(rp) | ValueZone::Library(rp) => {
                    rewrite_chosen_to_loop_rp(rp);
                }
                ValueZone::Exile(Some(rp)) => rewrite_chosen_to_loop_rp(rp),
                _ => {}
            }
        }
        ValueExpr::DistinctManaValuesInGraveyard(rp) => rewrite_chosen_to_loop_rp(rp),
        ValueExpr::CounterCount {
            subject: spellwake_engine::program::CounterSubjectRef::PlayerCounter(rp, _),
            ..
        } => rewrite_chosen_to_loop_rp(rp),
        ValueExpr::PlayerStat(ps) => match ps {
            PlayerStatExpr::PlayerIsActivePlayer(rp)
            | PlayerStatExpr::PlayerLife(rp)
            | PlayerStatExpr::PlayerHandSize(rp)
            | PlayerStatExpr::PlayerLibrarySize(rp)
            | PlayerStatExpr::PlayerGraveyardSize(rp)
            | PlayerStatExpr::LandsEnteredThisTurn(rp)
            | PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(rp)
            | PlayerStatExpr::PlayerLifeLostLastTurn(rp)
            | PlayerStatExpr::ControlsGreatestStat { player: rp, .. } => {
                rewrite_chosen_to_loop_rp(rp);
            }
            PlayerStatExpr::SpellsCastMatchingThisTurn { players, .. }
            | PlayerStatExpr::SacrificedMatchingThisTurn { players, .. } => {
                rewrite_chosen_to_loop_ap(players);
            }
            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                controller: Some(rp),
                ..
            } => rewrite_chosen_to_loop_rp(rp),
            _ => {}
        },
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            rewrite_chosen_to_loop_ve(expr);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_chosen_to_loop_ve(a);
            rewrite_chosen_to_loop_ve(b);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_chosen_to_loop_ve(left);
            rewrite_chosen_to_loop_ve(right);
            rewrite_chosen_to_loop_ve(then_value);
            rewrite_chosen_to_loop_ve(else_value);
        }
        _ => {}
    }
}

pub(super) fn rewrite_chosen_to_loop_cond(cond: &mut ResolutionCondition) {
    rewrite_chosen_to_loop_ve(&mut cond.left);
    rewrite_chosen_to_loop_ve(&mut cond.right);
}

pub(super) fn rewrite_chosen_to_loop_rule(rule: &mut TemporaryRuleKind) {
    match rule {
        TemporaryRuleKind::PlayerCantGainLife { players }
        | TemporaryRuleKind::CantCastSpells { players, .. }
        | TemporaryRuleKind::CantActivateAbilities { players, .. }
        | TemporaryRuleKind::CantPlayLands { players }
        | TemporaryRuleKind::PlayerGrantKeyword { players, .. }
        | TemporaryRuleKind::SpellCostIncrease { players, .. } => {
            rewrite_chosen_to_loop_ap(players);
        }
        TemporaryRuleKind::AdditionalLandPlay { player, .. }
        | TemporaryRuleKind::FilteredFlash { player, .. }
        | TemporaryRuleKind::ZoneCastPermission { player, .. } => {
            rewrite_chosen_to_loop_rp(player);
        }
        TemporaryRuleKind::SpellCostReductionExpr { amount, .. } => {
            rewrite_chosen_to_loop_ve(amount);
        }
        _ => {}
    }
}

pub(super) fn rewrite_chosen_to_loop_ops(ops: &mut [Instruction]) {
    for op in ops {
        match op {
            Instruction::DrawPlayer { players, amount }
            | Instruction::AdjustPlayersLife {
                players, amount, ..
            }
            | Instruction::Discard { players, amount }
            | Instruction::DiscardFiltered {
                players, amount, ..
            }
            | Instruction::DiscardUpToExt {
                players,
                max_count: amount,
                ..
            }
            | Instruction::Sacrifice {
                players, amount, ..
            }
            | Instruction::ExileChosenPermanents {
                players, amount, ..
            }
            | Instruction::ReturnControlledPermanents {
                players, amount, ..
            }
            | Instruction::ExileFromZone {
                players, amount, ..
            }
            | Instruction::TapOrUntapControlled {
                players, amount, ..
            }
            | Instruction::AddPlayerCounter {
                players, amount, ..
            } => {
                rewrite_chosen_to_loop_ap(players);
                rewrite_chosen_to_loop_ve(amount);
            }
            Instruction::AddMana(yield_kind) => {
                rewrite_chosen_to_loop_mana_yield(yield_kind);
            }
            Instruction::AddManaForPlayer { player, yield_kind } => {
                rewrite_chosen_to_loop_rp(player);
                rewrite_chosen_to_loop_mana_yield(yield_kind);
            }
            Instruction::ExileTopPlay {
                count,
                library_owner,
                ..
            } => {
                rewrite_chosen_to_loop_ve(count);
                rewrite_chosen_to_loop_ap(library_owner);
            }
            Instruction::ExilePlayerGraveyards(players)
            | Instruction::LoseGame(players)
            | Instruction::ExileGroupFromZone { players, .. } => {
                rewrite_chosen_to_loop_ap(players);
            }
            Instruction::MoveGraveyardGroupOwned {
                players,
                destination,
                ..
            } => {
                rewrite_chosen_to_loop_ap(players);
                rewrite_chosen_to_loop_dest(destination);
            }
            Instruction::ChooseOptionForPlayers { players, options } => {
                rewrite_chosen_to_loop_ap(players);
                for br in options {
                    rewrite_chosen_to_loop_ops(&mut br.body.instructions);
                }
            }
            Instruction::Scry { player, amount }
            | Instruction::Surveil { player, amount }
            | Instruction::Mill { player, amount }
            | Instruction::PayLife { player, amount }
            | Instruction::PayEnergy { player, amount }
            | Instruction::ManifestTop { player, amount }
            | Instruction::Blight { player, amount }
            | Instruction::ExileLibraryTopLinked { player, amount, .. } => {
                rewrite_chosen_to_loop_rp(player);
                rewrite_chosen_to_loop_ve(amount);
            }
            Instruction::CollectEvidence { player, minimum } => {
                rewrite_chosen_to_loop_rp(player);
                rewrite_chosen_to_loop_ve(minimum);
            }
            Instruction::UntapOrTapSelect { chooser, count, .. }
            | Instruction::DestroyChosen { chooser, count, .. } => {
                rewrite_chosen_to_loop_rp(chooser);
                rewrite_chosen_to_loop_ve(count);
            }
            Instruction::SelectZoneCardsToZone {
                player,
                min,
                max,
                destination,
                ..
            } => {
                rewrite_chosen_to_loop_rp(player);
                rewrite_chosen_to_loop_ve(min);
                rewrite_chosen_to_loop_ve(max);
                rewrite_chosen_to_loop_dest(destination);
            }
            Instruction::ChooseObjects {
                players,
                min,
                max,
                max_total_power,
                ..
            } => {
                if let AffectedPlayers::One(rp) = players {
                    rewrite_chosen_to_loop_rp(rp);
                }
                rewrite_chosen_to_loop_ve(min);
                rewrite_chosen_to_loop_ve(max);
                if let Some(limit) = max_total_power {
                    rewrite_chosen_to_loop_ve(limit);
                }
            }
            Instruction::LookTopChoose {
                count,
                library_owner,
                ..
            } => {
                rewrite_chosen_to_loop_ve(count);
                rewrite_chosen_to_loop_rp(library_owner);
            }
            Instruction::LookTopChooseExt {
                library_owner,
                chooser,
                count,
                ..
            }
            | Instruction::RevealUntilMatchExt {
                library_owner,
                chooser,
                count,
                ..
            } => {
                rewrite_chosen_to_loop_rp(library_owner);
                rewrite_chosen_to_loop_rp(chooser);
                rewrite_chosen_to_loop_ve(count);
            }
            Instruction::RevealTopTwoPiles { count, .. } => {
                rewrite_chosen_to_loop_ve(count);
            }
            Instruction::OfferManaPayment { payer, .. } => {
                rewrite_chosen_to_loop_rp(payer);
            }
            Instruction::OfferDynamicManaPayment {
                payer,
                extra_generic,
                ..
            } => {
                rewrite_chosen_to_loop_rp(payer);
                rewrite_chosen_to_loop_ve(extra_generic);
            }
            Instruction::OptionalEffect { chooser }
            | Instruction::OptionalSequence { chooser, .. }
            | Instruction::Clash { chooser }
            | Instruction::ChooseValue { chooser, .. }
            | Instruction::AttachChoosingPermanent { chooser, .. } => {
                rewrite_chosen_to_loop_rp(chooser);
            }
            Instruction::ChooseOption { chooser, options } => {
                rewrite_chosen_to_loop_rp(chooser);
                for br in options {
                    rewrite_chosen_to_loop_ops(&mut br.body.instructions);
                }
            }
            Instruction::ShufflePlayerLibrary(rp)
            | Instruction::RevealPlayerHand(rp)
            | Instruction::DiscardOneAtRandom(rp)
            | Instruction::WinGame(rp)
            | Instruction::SkipNextTurns { player: rp, .. }
            | Instruction::SkipNextStep { player: rp, .. }
            | Instruction::AttachPermanentToPlayer { player: rp, .. }
            | Instruction::ExileLibraryTop { player: rp, .. }
            | Instruction::ExileCardsFromGraveyard { player: rp, .. }
            | Instruction::PutHandPermanentFiltered { player: rp, .. }
            | Instruction::PutHandPermanentsOntoBattlefield { player: rp, .. }
            | Instruction::GrantCardPlayPermission { player: rp, .. }
            | Instruction::VentureIntoDungeon { player: rp, .. }
            | Instruction::TakeInitiative { player: rp }
            | Instruction::GainControlOfPermanent { controller: rp, .. } => {
                rewrite_chosen_to_loop_rp(rp);
            }
            Instruction::CreateTokens { creations } => {
                for c in creations {
                    rewrite_chosen_to_loop_ve(&mut c.amount);
                    if let TokenRecipients::Player(rp) = &mut c.recipients {
                        rewrite_chosen_to_loop_rp(rp);
                    }
                }
            }
            Instruction::CreateAttackingTokens {
                creations,
                defender_scope,
            } => {
                for c in creations {
                    rewrite_chosen_to_loop_ve(&mut c.amount);
                    if let TokenRecipients::Player(rp) = &mut c.recipients {
                        rewrite_chosen_to_loop_rp(rp);
                    }
                }
                if let spellwake_engine::token::AttackingDefenderScope::Player(rp)
                | spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(
                    rp,
                ) = defender_scope
                {
                    rewrite_chosen_to_loop_rp(rp);
                }
            }
            Instruction::CreateTokenCopy {
                count, recipients, ..
            } => {
                rewrite_chosen_to_loop_ve(count);
                if let TokenRecipients::Player(rp) = recipients {
                    rewrite_chosen_to_loop_rp(rp);
                }
            }
            Instruction::CreateTokenCopyExt {
                count,
                recipients,
                defender_scope,
                ..
            } => {
                rewrite_chosen_to_loop_ve(count);
                if let TokenRecipients::Player(rp) = recipients {
                    rewrite_chosen_to_loop_rp(rp);
                }
                if let spellwake_engine::token::AttackingDefenderScope::Player(rp)
                | spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(
                    rp,
                ) = defender_scope
                {
                    rewrite_chosen_to_loop_rp(rp);
                }
            }
            Instruction::DamageGroup { group, .. } => {
                rewrite_chosen_to_loop_dg(group);
            }
            Instruction::DamageGroupAmount { group, amount } => {
                rewrite_chosen_to_loop_dg(group);
                rewrite_chosen_to_loop_ve(amount);
            }
            Instruction::DealDamageFrom {
                recipients, amount, ..
            } => {
                if let DamageRecipientsRef::Group(g) = recipients.as_mut() {
                    rewrite_chosen_to_loop_dg(g);
                }
                if let DamageAmountSpec::Expr(e) = amount.as_mut() {
                    rewrite_chosen_to_loop_ve(e);
                }
            }
            Instruction::MoveSubject { destination, .. } => {
                rewrite_chosen_to_loop_dest(destination);
            }
            Instruction::PutTargetInLibrary(pl)
            | Instruction::PutResolvingSpellInLibrary(pl)
            | Instruction::MoveGroupToLibrary { placement: pl, .. } => {
                rewrite_chosen_to_loop_pl(pl);
            }
            Instruction::CreateTemporaryRule { rule, .. } => {
                rewrite_chosen_to_loop_rule(rule);
            }
            Instruction::BranchIfFalse { condition, .. } => {
                rewrite_chosen_to_loop_cond(condition);
            }
            Instruction::RepeatLoop { count, body } => {
                rewrite_chosen_to_loop_ve(count);
                rewrite_chosen_to_loop_ops(&mut body.instructions);
            }
            Instruction::ForEachPlayer { players, body } => {
                rewrite_chosen_to_loop_ap(players);
                rewrite_chosen_to_loop_ops(&mut body.instructions);
            }
            Instruction::OfferAnyPlayer {
                players,
                cost_body,
                paid_body,
                declined_body,
            } => {
                rewrite_chosen_to_loop_ap(players);
                rewrite_chosen_to_loop_ops(&mut cost_body.instructions);
                rewrite_chosen_to_loop_ops(&mut paid_body.instructions);
                if let Some(dec) = declined_body {
                    rewrite_chosen_to_loop_ops(&mut dec.instructions);
                }
            }
            Instruction::Exchange(spec) => match spec {
                ExchangeSpec::LifeTotals { first, second } => {
                    rewrite_chosen_to_loop_rp(first);
                    rewrite_chosen_to_loop_rp(second);
                }
                ExchangeSpec::LifeWithStat { player, .. } | ExchangeSpec::Zones { player, .. } => {
                    rewrite_chosen_to_loop_rp(player);
                }
                ExchangeSpec::Control { .. } => {}
            },
            Instruction::RollDie { player, count, .. } => {
                rewrite_chosen_to_loop_rp(player);
                rewrite_chosen_to_loop_ve(count);
            }
            Instruction::FlipCoins {
                player,
                win_body,
                lose_body,
                ..
            } => {
                rewrite_chosen_to_loop_rp(player);
                if let Some(wb) = win_body {
                    rewrite_chosen_to_loop_ops(&mut wb.instructions);
                }
                if let Some(lb) = lose_body {
                    rewrite_chosen_to_loop_ops(&mut lb.instructions);
                }
            }
            Instruction::DrawController { amount }
            | Instruction::DrawTargetPlayer { amount }
            | Instruction::DrawEachPlayer { amount }
            | Instruction::DrawControllerAndTarget { amount }
            | Instruction::GainControllerLife { amount }
            | Instruction::DamageAmount(amount)
            | Instruction::DamageOpponentsAmount(amount)
            | Instruction::DamageRelatedPlayerAmount(amount)
            | Instruction::DrainOpponents(amount)
            | Instruction::DrainTargetAndGain(amount)
            | Instruction::OpponentsLoseAndControllerGains(amount)
            | Instruction::PutCountersOnSubject { amount, .. }
            | Instruction::PutNamedCounterOnTarget { amount, .. }
            | Instruction::RemoveCountersFromPermanent { amount, .. }
            | Instruction::DealDamageToAnnouncedTargets { amount, .. }
            | Instruction::Earthbend { amount }
            | Instruction::BindResolutionX(amount) => {
                rewrite_chosen_to_loop_ve(amount);
            }
            Instruction::RemoveCountersFromSubject {
                amount: Some(amt), ..
            } => {
                rewrite_chosen_to_loop_ve(amt);
            }
            Instruction::SearchLibrary { filter } => {
                rewrite_chosen_to_loop_rp(&mut filter.searcher);
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_loop_player_sub_body(
    body: &[InstructionAst],
    outer_builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<ExecutableBody, &'static str> {
    if body.is_empty() {
        return Err("oracle:empty-loop-body");
    }
    let mut sub = BodyBuilder::with_trigger_hint(outer_builder.trigger_hint);
    sub.has_source = outer_builder.has_source || in_trigger || self_pronoun_valid;
    sub.last_group_filter = outer_builder.last_group_filter;
    sub.instructions.push(Instruction::ChooseValue {
        chooser: ResolvingPlayer::Controller,
        domain: ResolutionChoiceDomain::Player {
            opponents_only: false,
        },
    });
    lower_instruction_seq(
        body,
        &mut sub,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    )?;
    if sub.instructions.len() <= 1 {
        return Err("oracle:empty-loop-body");
    }
    sub.instructions.remove(0);
    for op in &mut sub.instructions {
        op.shift_jumps(-1, 1);
    }
    if sub.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::ChooseValue {
                domain: ResolutionChoiceDomain::Player { .. },
                ..
            } | Instruction::SelectExtremeLifePlayer { .. }
        )
    }) {
        return Err("oracle:loop-body-chooses-player");
    }
    rewrite_chosen_to_loop_ops(&mut sub.instructions);
    if sub.target != BodyTarget::None {
        if outer_builder.target == BodyTarget::None {
            outer_builder.bind_target(sub.target)?;
        } else if outer_builder.target != sub.target {
            return Err("oracle:multiple-targets");
        }
        sub.target = BodyTarget::None;
    }
    if outer_builder.graveyard_target_bounds.is_none() {
        outer_builder.graveyard_target_bounds = sub.graveyard_target_bounds;
    }
    outer_builder.x_target_count |= sub.x_target_count;
    Ok(ExecutableBody::new(BodyTarget::None, sub.instructions))
}
