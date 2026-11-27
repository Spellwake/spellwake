use super::*;

fn instruction_supports_optional_wrapper(op: &Instruction) -> bool {
    match op {
        Instruction::ZoneOp(spec) => match &spec.destination {
            spellwake_engine::toolbox::ZoneDest::Delayed(action) => !matches!(
                action,
                spellwake_engine::toolbox::DelayedZoneAction::ReturnChosenXGraveyardCreatures
                    | spellwake_engine::toolbox::DelayedZoneAction::ReturnGraveyardToHand(_)
                    | spellwake_engine::toolbox::DelayedZoneAction::ReturnExiledTargetCohort { .. }
                    | spellwake_engine::toolbox::DelayedZoneAction::ExileObjects(_)
            ),
            _ => true,
        },
        Instruction::MutateObject(spec) => !matches!(
            spec.op,
            spellwake_engine::toolbox::ObjectMutation::ReturnEarthbentCard
                | spellwake_engine::toolbox::ObjectMutation::ResolveGrowth(_)
                | spellwake_engine::toolbox::ObjectMutation::ResolveCounterKeyword(_)
                | spellwake_engine::toolbox::ObjectMutation::SacrificeEvoked
                | spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                | spellwake_engine::toolbox::ObjectMutation::ExileDefeatedSiege
        ),
        Instruction::MutatePlayer(spec) => !matches!(
            spec.op,
            spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                | spellwake_engine::toolbox::PlayerMutation::ChooseRevealedHandCard { .. }
                | spellwake_engine::toolbox::PlayerMutation::ExtortDrain
                | spellwake_engine::toolbox::PlayerMutation::OfferSourceCounters { .. }
                | spellwake_engine::toolbox::PlayerMutation::SacrificeRandomUntilLand
                | spellwake_engine::toolbox::PlayerMutation::ExileOnePerPlayerUntilSourceLeaves
        ),
        Instruction::StackOp(spec) => !matches!(
            spec.op,
            spellwake_engine::toolbox::StackOpKind::ChooseFoundSpell
                | spellwake_engine::toolbox::StackOpKind::CastFoundSpell(_)
                | spellwake_engine::toolbox::StackOpKind::Encore
        ),
        Instruction::DealDamage(spec) => !matches!(
            spec.recipients,
            spellwake_engine::toolbox::DamageTargetQuery::FromGrantingPermanent
        ),
        Instruction::SearchLibrary { filter } => filter.supports_optional_wrapper(),
        Instruction::Discard {
            players: AffectedPlayers::One(_),
            ..
        }
        | Instruction::Sacrifice {
            players: AffectedPlayers::One(_),
            ..
        } => true,
        Instruction::BranchCondition { .. }
        | Instruction::Jump { .. }
        | Instruction::OfferDynamicManaPayment { .. }
        | Instruction::OptionalSequence { .. }
        | Instruction::Sacrifice { .. }
        | Instruction::Discard { .. } => false,
        _ => true,
    }
}

fn wrap_optional_instructions(
    ops: &mut Vec<Instruction>,
    start: usize,
    chooser: ResolvingPlayer,
) -> Result<(), &'static str> {
    let count = ops.len().saturating_sub(start);
    if count == 0 {
        return Err("oracle:empty-optional");
    }
    let Some(head) = ops.get(start) else {
        return Err("oracle:empty-optional");
    };
    if count == 1 {
        let already_optional = matches!(head, Instruction::OfferDynamicManaPayment { .. })
            || matches!(
                head,
                Instruction::MutatePlayer(spec)
                    if matches!(
                        spec.op,
                        spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                            | spellwake_engine::toolbox::PlayerMutation::ExtortDrain
                            | spellwake_engine::toolbox::PlayerMutation::OfferSourceCounters { .. }
                    )
            )
            || matches!(
                head,
                Instruction::OptionalSequence { chooser: existing, .. }
                    if *existing == chooser
            );
        if already_optional {
            return Ok(());
        }
        if instruction_supports_optional_wrapper(head) {
            prepend_instruction_at(
                ops,
                start,
                Instruction::OptionalSequence {
                    chooser,
                    instructions: 1,
                },
            );
            return Ok(());
        }
    } else if instruction_supports_optional_wrapper(head)
        || matches!(head, Instruction::OfferDynamicManaPayment { .. })
    {
        prepend_instruction_at(
            ops,
            start,
            Instruction::OptionalSequence {
                chooser,
                instructions: count,
            },
        );
        return Ok(());
    }
    let mut branch_ops = ops.split_off(start);
    if start > 0 {
        let shift = -isize::try_from(start).map_err(|_| "oracle:optional-jump-overflow")?;
        for op in &mut branch_ops {
            op.shift_jumps(shift, start);
        }
    }
    ops.push(Instruction::ChooseOption {
        chooser,
        options: vec![
            ChooseOptionBranch {
                label: "Take optional action".to_owned(),
                body: ExecutableBody::new(BodyTarget::None, branch_ops),
            },
            ChooseOptionBranch {
                label: "Decline".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Jump { destination: 1 }],
                ),
            },
        ],
    });
    Ok(())
}

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
                && !matches!(
                    then_body.as_slice(),
                    [InstructionAst::PayCost(CostAtomAst::PayMana(_))]
                )
            {
                return lower_instruction_seq(
                    &[InstructionAst::IfCondition {
                        condition: condition.clone(),
                        then_body: vec![InstructionAst::Optional(then_body.clone())],
                    }],
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
                        let mask = types.into_iter().fold(
                            spellwake_engine::program::CardTypeMask::EMPTY,
                            spellwake_engine::program::CardTypeMask::with,
                        );
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_hand_permanent_filtered(
                                ResolvingPlayer::Controller,
                                ObjectFilter::from_clause(
                                    spellwake_engine::program::ObjectClause::any().any_types(mask),
                                ),
                                1,
                                dest_low.contains("tapped"),
                            ),
                        );
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
            if let [InstructionAst::PayAnyAmountOfMana { player }] = inner.as_slice() {
                if try_group_affected_players(*player).is_some() {
                    return Err("oracle:unsupported-pay-any-mana");
                }
                let payer = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                let offer_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: ManaCost::free(),
                        extra_generic: ValueExpr::chosen_x(),
                        paid_destination: offer_idx + 2,
                        declined_destination: offer_idx + 1,
                    });
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(
                            ValueExpr::literal(0),
                        ),
                    )));
                return Ok(());
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
            wrap_optional_instructions(
                &mut builder.instructions,
                start,
                ResolvingPlayer::Controller,
            )?;
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
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::offer_any_player(
                            players,
                            cost_body,
                            ExecutableBody::new(BodyTarget::None, Vec::new()),
                            None,
                        ),
                    )));
                return Ok(());
            }
            if matches!(
                *player,
                PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherPlayer
            ) {
                if matches!(body.as_slice(), [InstructionAst::PayAnyAmountOfMana { .. }]) {
                    return Err("oracle:unsupported-pay-any-mana");
                }
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
                if sub
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::OfferDynamicManaPayment { .. }))
                {
                    return Err("oracle:unsupported-pay-any-mana");
                }
                wrap_optional_instructions(&mut sub.instructions, 0, ResolvingPlayer::LoopPlayer)?;
                builder.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                    body: Box::new(sub),
                });
                return Ok(());
            }
            if let [InstructionAst::PayAnyAmountOfMana { player: inner_p }] = body.as_slice() {
                let eff_player = if *inner_p == PlayerRefAst::You {
                    *player
                } else {
                    *inner_p
                };
                if try_group_affected_players(eff_player).is_some() {
                    return Err("oracle:unsupported-pay-any-mana");
                }
                let payer = lower_resolving_player_in_context(eff_player, builder, in_trigger)?;
                let offer_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: ManaCost::free(),
                        extra_generic: ValueExpr::chosen_x(),
                        paid_destination: offer_idx + 2,
                        declined_destination: offer_idx + 1,
                    });
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(
                            ValueExpr::literal(0),
                        ),
                    )));
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
            wrap_optional_instructions(&mut builder.instructions, start, chooser)?;
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
            let selector = match player {
                PlayerRefAst::You => spellwake_engine::toolbox::PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ),
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    )
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    )
                }
                PlayerRefAst::ThatPlayer
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    )
                }
                PlayerRefAst::EachOpponent => {
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent)
                }
                PlayerRefAst::EachPlayer => {
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer)
                }
                p => {
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    spellwake_engine::toolbox::PlayerSelector::Group(players)
                }
            };
            let op = if *gain {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(selector, expr)
            } else {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(selector, expr)
            };
            builder.instructions.push(op);
            Ok(())
        }
        InstructionAst::SetLifeTotal { player, value } => {
            let resolved_value = resolve_x(value, bound_x);
            if matches!(
                player,
                PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherPlayer
            ) && value_expr_ast_references_that_player(resolved_value)
            {
                let players = match *player {
                    PlayerRefAst::EachPlayer => AffectedPlayers::EachPlayer,
                    PlayerRefAst::EachOpponent => AffectedPlayers::EachOpponent,
                    _ => AffectedPlayers::EachOtherPlayer,
                };
                let rewritten = rewrite_loop_player_ast(
                    &InstructionAst::SetLifeTotal {
                        player: *player,
                        value: Box::new(resolved_value.clone()),
                    },
                    *player,
                );
                let sub = lower_loop_player_sub_body(
                    &[rewritten],
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                builder.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                    body: Box::new(sub),
                });
                return Ok(());
            }
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            let amount =
                lower_value_expr_in_context(resolved_value, builder, in_trigger, vocabulary)?;
            let selector = match players {
                AffectedPlayers::One(rp) => {
                    spellwake_engine::toolbox::PlayerSelector::Resolving(rp)
                }
                other => spellwake_engine::toolbox::PlayerSelector::Group(other),
            };
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::set_life(selector, amount),
            );
            Ok(())
        }
        InstructionAst::BecomeMonarch { player } => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::become_monarch(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                ),
            );
            Ok(())
        }
        InstructionAst::PayCost(cost) => {
            if let CostAtomAst::PayMana(raw) = cost {
                let idx = builder.instructions.len();
                if let Some(mc) = parse_fixed_mana_cost(raw) {
                    builder
                        .instructions
                        .push(Instruction::OfferDynamicManaPayment {
                            payer: ResolvingPlayer::Controller,
                            fixed: mc,
                            extra_generic: ValueExpr::Literal(0.into()),
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
                } else if let Some(op) =
                    lower_hybrid_mana_payment_instruction(ResolvingPlayer::Controller, raw)
                {
                    builder.instructions.push(op);
                    return Ok(());
                }
                return Err("oracle:unsupported-pay-cost");
            }
            if let Some(op) = lower_extra_pay_cost_instruction(
                ResolvingPlayer::Controller,
                cost,
                builder.instructions.len(),
                builder,
                in_trigger,
                vocabulary,
                bound_x,
            )? {
                builder.instructions.push(op);
                return Ok(());
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
                    Instruction::OfferDynamicManaPayment {
                        payer: rp,
                        fixed: mc,
                        extra_generic: ValueExpr::Literal(0.into()),
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
                } else if let Some(hyb_op) = lower_hybrid_mana_payment_instruction(rp, raw) {
                    hyb_op
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
            let base_idx = if group_players.is_some() {
                0
            } else {
                builder.instructions.len()
            };
            if let Some(op) = lower_extra_pay_cost_instruction(
                rp, cost, base_idx, builder, in_trigger, vocabulary, bound_x,
            )? {
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
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::exert_source());
            Ok(())
        }
        InstructionAst::CollectEvidence { value } => {
            let min = lower_value_expr_in_context(
                resolve_x(value, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::collect_evidence(
                        ResolvingPlayer::Controller,
                        min,
                    ),
                )));
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
                    s.x_target_count
                        || s.up_to_x_target_count
                        || s.up_to_one_target
                        || s.graveyard_target_bounds.is_some()
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
                builder.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ChooseOption {
                            chooser: ResolvingPlayer::LoopPlayer,
                            options,
                        }],
                    )),
                });
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
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::vote(options, true),
                )));
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
            if let Some(res) = try_lower_choose_objects_new_primitives(
                *chooser,
                cleaned,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            ) {
                return res;
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
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::vote(options, false),
                )));
            Ok(())
        }
        InstructionAst::RollDie { sides } => {
            let nz = std::num::NonZeroU16::new(*sides).ok_or("oracle:zero-die-sides")?;
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::roll_die(
                        ResolvingPlayer::Controller,
                        nz,
                        ValueExpr::literal(1),
                        spellwake_engine::program::DieRollMode::Sum,
                        Vec::new(),
                    ),
                )));
            Ok(())
        }
        InstructionAst::RollDiceChooseOne { count, sides } => {
            let nz = std::num::NonZeroU16::new(*sides).ok_or("oracle:zero-die-sides")?;
            if *count == 0 {
                return Err("oracle:zero-die-count");
            }
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::roll_die(
                        ResolvingPlayer::Controller,
                        nz,
                        ValueExpr::literal(i64::from(*count)),
                        spellwake_engine::program::DieRollMode::ChooseOne,
                        Vec::new(),
                    ),
                )));
            Ok(())
        }
        InstructionAst::FlipCoin => {
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::flip_coins(
                        ResolvingPlayer::Controller,
                        spellwake_engine::program::CoinFlipMode::Single,
                        None,
                        None,
                        None,
                    ),
                )));
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
                    "an opponent at random" | "opponent at random" | "random opponent" => {
                        builder.instructions.push(Instruction::ChooseValue {
                            chooser: ResolvingPlayer::Controller,
                            domain:
                                spellwake_engine::program::ResolutionChoiceDomain::RandomPlayer {
                                    opponents_only: true,
                                },
                        });
                        return Ok(());
                    }
                    "a player at random"
                    | "player at random"
                    | "another player at random"
                    | "random player" => {
                        builder.instructions.push(Instruction::ChooseValue {
                            chooser: ResolvingPlayer::Controller,
                            domain:
                                spellwake_engine::program::ResolutionChoiceDomain::RandomPlayer {
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
                && (cleaned_choice == "any target"
                    || cleaned_choice.starts_with("target ")
                    || cleaned_choice.starts_with("up to one target "))
            {
                let (is_up_to_one, target_text) =
                    if let Some(rest) = cleaned_choice.strip_prefix("up to one ") {
                        (true, rest)
                    } else {
                        (false, cleaned_choice)
                    };
                if target_text == "any target" {
                    builder.bind_target(BodyTarget::SingleAnyTarget)?;
                    if is_up_to_one {
                        builder.up_to_one_target = true;
                    }
                    return Ok(());
                }
                if target_text == "target player" {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    if is_up_to_one {
                        builder.up_to_one_target = true;
                    }
                    return Ok(());
                }
                if target_text == "target opponent" {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    if is_up_to_one {
                        builder.up_to_one_target = true;
                    }
                    return Ok(());
                }
                if let Ok(subj) = spellwake_oracle_compiler::filter::parse_subject_or_target(
                    target_text,
                    &cached_compiler_vocab(vocabulary),
                ) {
                    let bt = lower_target_subject(&subj, vocabulary)?;
                    builder.bind_target(bt)?;
                    if is_up_to_one {
                        builder.up_to_one_target = true;
                    }
                    return Ok(());
                }
            }
            if matches!(
                cleaned_choice,
                "one of them"
                    | "one of those"
                    | "one of those cards"
                    | "one of the cards"
                    | "a card from among them"
                    | "a card from among those"
                    | "one of the exiled cards"
            ) && (last_instruction_moved_object(&builder.instructions)
                || matches!(
                    builder.instructions.last(),
                    Some(
                        Instruction::ZoneOp(_)
                            | Instruction::ForEachPlayerQuery { .. }
                            | Instruction::ExileTopPlay { .. }
                            | Instruction::LookTopChooseExt { .. }
                            | Instruction::RevealUntilMatchExt { .. }
                            | Instruction::ChooseObjects { .. }
                    )
                )
                || matches!(
                    builder.instructions.last(),
                    Some(Instruction::MutatePlayer(spec))
                        if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Mill(_))
                ))
                && let Ok(chooser_rp) =
                    lower_resolving_player_in_context(chooser_ref, builder, in_trigger)
            {
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::choose_one_last_moved(
                            chooser_rp,
                        ),
                    )));
                return Ok(());
            }
            if matches!(
                cleaned_choice,
                "a number greater than 0 and a color"
                    | "a number greater than zero and a color"
                    | "a positive number and a color"
            ) && let Ok(chooser_rp) =
                lower_resolving_player_in_context(chooser_ref, builder, in_trigger)
            {
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: chooser_rp,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Number {
                        min: 1,
                        max: 20,
                    },
                });
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: chooser_rp,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Color,
                });
                return Ok(());
            }
            if let Ok(kind) = parse_choice_kind(cleaned_choice, true, vocabulary) {
                return emit_choice_for_player_ref(chooser_ref, kind, builder, ids, in_trigger);
            }
            if let Some(res) = try_lower_choose_objects_new_primitives(
                chooser_ref,
                cleaned_choice,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            ) {
                return res;
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
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::clash(ResolvingPlayer::Controller),
                )));
            Ok(())
        }
        InstructionAst::RingTemptsYou => {
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::ring_tempts_you(),
                )));
            Ok(())
        }
        InstructionAst::OpenAttraction => {
            builder.instructions.push(Instruction::SideDeckOp(Box::new(
                spellwake_engine::toolbox::SideDeckOpSpec {
                    owner: Some(spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    )),
                    kind: spellwake_engine::toolbox::SideDeckKind::Attraction,
                    op: spellwake_engine::toolbox::SideDeckOpKind::AdvanceTop,
                    bind_succeeded: None,
                },
            )));
            Ok(())
        }
        InstructionAst::Learn => {
            let reveal_lesson = ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SideDeckOp(Box::new(
                    spellwake_engine::toolbox::SideDeckOpSpec {
                        owner: Some(spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        )),
                        kind: spellwake_engine::toolbox::SideDeckKind::Custom(1),
                        op: spellwake_engine::toolbox::SideDeckOpKind::AdvanceTop,
                        bind_succeeded: None,
                    },
                ))],
            );
            let discard_to_draw = ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                            )),
                            ObjectFilter::any(),
                        ),
                        spellwake_engine::toolbox::ZoneSelection::Prompt {
                            chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            min: Box::new(ValueExpr::literal(1)),
                            max: Box::new(ValueExpr::literal(1)),
                            constraint: None,
                            category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                            hidden: true,
                        },
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                        },
                    ))),
                    Instruction::BranchCondition {
                        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                            Box::new(ResolutionCondition {
                                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(1),
                            }),
                        )),
                        destination: 3,
                    },
                    super::super::toolbox_emit::draw_controller(ValueExpr::literal(1)),
                ],
            );
            let do_nothing =
                ExecutableBody::new(BodyTarget::None, vec![Instruction::Jump { destination: 1 }]);
            builder.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options: vec![
                    ChooseOptionBranch {
                        label:
                            "Reveal a Lesson card from outside the game and put it into your hand"
                                .to_owned(),
                        body: reveal_lesson,
                    },
                    ChooseOptionBranch {
                        label: "Discard a card to draw a card".to_owned(),
                        body: discard_to_draw,
                    },
                    ChooseOptionBranch {
                        label: "Do nothing".to_owned(),
                        body: do_nothing,
                    },
                ],
            });
            Ok(())
        }
        InstructionAst::Populate { count } => {
            interactive::lower_populate(count, builder, vocabulary, in_trigger, bound_x)
        }
        InstructionAst::VentureIntoDungeon { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::venture_into_dungeon(
                            ResolvingPlayer::LoopPlayer,
                            false,
                        ),
                    ))],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::venture_into_dungeon(rp, false),
                )));
            Ok(())
        }
        InstructionAst::TakeInitiative { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::take_initiative(
                            ResolvingPlayer::LoopPlayer,
                        ),
                    ))],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::take_initiative(rp),
                )));
            Ok(())
        }
        InstructionAst::UnlockDoor { subject } => {
            let eff_subject =
                unwrap_in_zone_subject(subject).unwrap_or_else(|| (**subject).clone());
            if let SubjectRefAst::Group(f) = &eff_subject
                && matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && !f.any_spell
                && !f.is_ability
            {
                let mut inner = f.as_ref().clone();
                inner.zone = None;
                let source_zone = if inner.controller == Some(ControllerRelation::You) {
                    inner.controller = None;
                    SourceZoneFilter::Battlefield
                } else {
                    SourceZoneFilter::AnyBattlefield
                };
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                };
                if !obj.has_target_player() {
                    builder.instructions.push(Instruction::ChooseObjects {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        source_zone,
                        filter: Box::new(obj),
                        min: ValueExpr::literal(1),
                        max: ValueExpr::literal(1),
                        distinct_powers: false,
                        max_total_power: None,
                        unchosen_action: None,
                    });
                    builder
                        .instructions
                        .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::unlock_door(
                                spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                    MoveSubjectRef::LastMoved,
                                ),
                                None,
                                false,
                            ),
                        )));
                    return Ok(());
                }
            }
            let subj = lower_move_subject_ref(
                subject,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder
                .instructions
                .push(Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::unlock_door(
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(subj),
                        None,
                        false,
                    ),
                )));
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
            builder.instructions.push(Instruction::BranchCondition {
                condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                    Box::new(rc),
                )),
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
            if let Some(Instruction::BranchCondition { destination, .. }) =
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
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::offer_any_player(
                            players,
                            cost_body,
                            ExecutableBody::new(BodyTarget::None, Vec::new()),
                            Some(declined_body),
                        ),
                    )));
                return Ok(());
            }
            if let Some(CostAtomAst::PayMana(raw)) = unless_cost
                && let Some(mana) = parse_fixed_mana_cost(raw)
            {
                let payer = lower_resolving_player_in_context(player_ast, builder, in_trigger)?;
                let offer_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: mana,
                        extra_generic: ValueExpr::Literal(0.into()),
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
                if let Some(Instruction::OfferDynamicManaPayment {
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
                    builder.instructions.push(Instruction::BranchCondition {
                        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                            Box::new(ResolutionCondition {
                                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                                comparison: Comparison::Equal,
                                right: ValueExpr::literal(0),
                            }),
                        )),
                        destination: 0,
                    });
                    splice_probed_body(builder, probe)?;
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::BranchCondition { destination, .. }) =
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
                    .map(|s| {
                        let r = try_rewrite_for_each_player(s, *player)?;
                        if super::super::body::try_wrap_each_player_that_player(&r).is_some() {
                            None
                        } else {
                            Some(r)
                        }
                    })
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
                        builder.instructions.push(Instruction::ForEachPlayerQuery {
                            selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                                players,
                            )),
                            bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                            body: Box::new(sub),
                        });
                        return Ok(());
                    }
                } else if *player == PlayerRefAst::You {
                    return lower_instruction_seq(
                        body,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                } else if matches!(
                    *player,
                    PlayerRefAst::TargetOpponent
                        | PlayerRefAst::TargetPlayer
                        | PlayerRefAst::ThatPlayer
                ) {
                    let mut trial = builder.clone();
                    if *player == PlayerRefAst::TargetOpponent {
                        trial.bind_target(BodyTarget::SingleOpponent)?;
                    } else if *player == PlayerRefAst::TargetPlayer {
                        trial.bind_target(BodyTarget::SinglePlayer)?;
                    }
                    let rewritten_body: Vec<InstructionAst> = body
                        .iter()
                        .map(|ins| rewrite_loop_player_ast(ins, PlayerRefAst::You))
                        .collect();
                    if lower_instruction_seq(
                        &rewritten_body,
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
                } else if matches!(
                    *player,
                    PlayerRefAst::DefendingPlayer
                        | PlayerRefAst::ActivePlayer
                        | PlayerRefAst::EnchantedPlayer
                        | PlayerRefAst::ItsController
                        | PlayerRefAst::ItsOwner
                        | PlayerRefAst::ChosenPlayer
                ) {
                    let mut trial = builder.clone();
                    if let Ok(rp) =
                        lower_resolving_player_in_context(*player, &mut trial, in_trigger)
                    {
                        let rewritten_body: Vec<InstructionAst> = body
                            .iter()
                            .map(|ins| rewrite_loop_player_ast(ins, PlayerRefAst::You))
                            .collect();
                        if let Ok(sub) = lower_loop_player_sub_body(
                            &rewritten_body,
                            &mut trial,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        ) && !sub.instructions.is_empty()
                        {
                            trial.instructions.push(Instruction::ForEachPlayerQuery {
                                selector: Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                ),
                                bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                                body: Box::new(sub),
                            });
                            *builder = trial;
                            return Ok(());
                        }
                    }
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ForEachMatchingPlayer {
            player,
            condition_text: _,
            condition: Some(cond),
            body,
        } if !body.is_empty() => {
            let players = match *player {
                PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => AffectedPlayers::EachPlayer,
                PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
                    AffectedPlayers::EachOpponent
                }
                PlayerRefAst::EachOtherPlayer => AffectedPlayers::EachOtherPlayer,
                _ => return Err("oracle:unsupported-instruction"),
            };
            let rewritten_cond = rewrite_loop_player_condition(cond, *player);
            let rewritten_body: Vec<InstructionAst> = body
                .iter()
                .map(|ins| rewrite_loop_player_ast(ins, *player))
                .collect();
            let gated = vec![InstructionAst::IfCondition {
                condition: Box::new(rewritten_cond),
                then_body: rewritten_body,
            }];
            let sub = lower_loop_player_sub_body(
                &gated,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if sub.instructions.is_empty() {
                return Err("oracle:unsupported-instruction");
            }
            builder.instructions.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                body: Box::new(sub),
            });
            Ok(())
        }
        InstructionAst::ForEachObject { filter, body } => {
            if !body.is_empty() && !body.iter().any(instruction_references_anaphora) {
                let scaled_all: Option<Vec<InstructionAst>> = body
                    .iter()
                    .map(|s| try_scale_instruction_ext(s, ValueExprAst::Count(filter.clone())))
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
                    .map(|s| try_scale_instruction_ext(s, mult.clone()))
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
                builder.instructions.push(Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
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
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_from_zone(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            CostExileZone::Graveyard,
                            ValueExpr::literal(3),
                            ObjectFilter::any(),
                        ),
                    ],
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
        InstructionAst::LookAtHand { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Group(players),
                                )),
                                filter: Box::new(ObjectFilter::any()),
                                slice: spellwake_engine::toolbox::QuerySlice::All,
                            },
                            spellwake_engine::toolbox::ZoneDest::LookOnly {
                                viewer: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                            },
                        ),
                    ),
                );
                return Ok(());
            }
            if matches!(player, PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer) {
                const LOOK_HAND_PLAYER_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(922);
                let group = if *player == PlayerRefAst::AnyOpponent {
                    AffectedPlayers::EachOpponent
                } else {
                    AffectedPlayers::EachPlayer
                };
                builder
                    .instructions
                    .push(Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec {
                            chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            domain: spellwake_engine::toolbox::PromptDomain::Players {
                                selector: spellwake_engine::toolbox::PlayerSelector::Group(group),
                                min: 1,
                                max: 1,
                            },
                            category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                            bind_to: LOOK_HAND_PLAYER_BIND,
                            bind_unchosen: None,
                            tag_partition: None,
                            auto_resolve_trivial: true,
                        },
                    )));
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Bound(
                                        LOOK_HAND_PLAYER_BIND,
                                    ),
                                )),
                                filter: Box::new(ObjectFilter::any()),
                                slice: spellwake_engine::toolbox::QuerySlice::All,
                            },
                            spellwake_engine::toolbox::ZoneDest::LookOnly {
                                viewer: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                            },
                        ),
                    ),
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::All,
                        },
                        spellwake_engine::toolbox::ZoneDest::LookOnly {
                            viewer: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                        },
                    ),
                ),
            );
            Ok(())
        }
        InstructionAst::ExileWithCounters {
            subject,
            counter_kind,
            count,
            face_down,
        } => {
            const EXILE_COUNTERS_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(920);
            let kind = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?;
            if !kind.is_permanent() {
                return Err("oracle:unsupported-counter-kind");
            }
            let eff_subject =
                unwrap_in_zone_subject(subject).unwrap_or_else(|| (**subject).clone());
            let (source_q, selection) = lower_exile_with_counters_subject(
                &eff_subject,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            let amt_expr = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let mut exile_spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                source_q,
                selection,
                spellwake_engine::toolbox::ZoneDest::Exile {
                    face_down: *face_down,
                    viewer: (*face_down).then_some(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    ),
                    link_to_source: false,
                    delayed_return: None,
                    link_group: None,
                },
            );
            exile_spec.bind_moved = Some(EXILE_COUNTERS_BIND);
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(exile_spec));
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                    spellwake_engine::toolbox::ObjectQuery::Bound(EXILE_COUNTERS_BIND),
                    kind,
                    amt_expr,
                ),
            );
            Ok(())
        }
        InstructionAst::PutSticker { kind, target }
        | InstructionAst::PutStickerWithCost { kind, target, .. } => lower_put_sticker_instruction(
            kind,
            target,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
        InstructionAst::RevealUntil {
            player,
            filter,
            count,
        } => {
            const REVEAL_UNTIL_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(925);
            let p_sel = if let Some(players) = try_group_affected_players(*player) {
                spellwake_engine::toolbox::PlayerSelector::Group(players)
            } else {
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                spellwake_engine::toolbox::PlayerSelector::Resolving(rp)
            };
            let obj_filter =
                lower_object_filter_in_context(filter, builder, in_trigger, vocabulary)?;
            bind_filter_player_target(builder, &obj_filter)?;
            let cnt_expr = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let mut spec = spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                    zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(p_sel)),
                    filter: Box::new(ObjectFilter::any()),
                    slice: spellwake_engine::toolbox::QuerySlice::UntilMatch {
                        filter: Box::new(obj_filter),
                        count: Box::new(cnt_expr),
                        include_non_matches: true,
                    },
                },
                spellwake_engine::toolbox::ZoneDest::RevealOnly,
            );
            spec.bind_moved = Some(REVEAL_UNTIL_BIND);
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
            Ok(())
        }
        InstructionAst::Empower {
            planeswalker,
            count,
        } => lower_empower_instruction(
            planeswalker,
            count,
            builder,
            vocabulary,
            in_trigger,
            bound_x,
        ),
        InstructionAst::ChooseAndKeep {
            pick,
            keep_to,
            rest_to,
        } => lower_standalone_choose_and_keep(
            pick, keep_to, rest_to, builder, vocabulary, in_trigger, bound_x,
        ),
        InstructionAst::IfPreviousFailed(fail_body) => {
            if let Some(Instruction::ScheduleDelayedStepTrigger {
                body: delayed_body, ..
            }) = builder.instructions.last_mut()
                && let Some(offer_idx) = delayed_body.instructions.len().checked_sub(1)
                && matches!(
                    delayed_body.instructions.get(offer_idx),
                    Some(Instruction::OfferDynamicManaPayment {
                        paid_destination,
                        declined_destination,
                        ..
                    }) if *paid_destination == offer_idx + 1
                        && *declined_destination == offer_idx + 1
                )
            {
                let mut fail_builder = BodyBuilder::new();
                fail_builder.has_source = builder.has_source || in_trigger || self_pronoun_valid;
                lower_instruction_seq(
                    fail_body,
                    &mut fail_builder,
                    vocabulary,
                    ids,
                    true,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if fail_builder.target != BodyTarget::None || fail_builder.instructions.is_empty() {
                    return Err("oracle:unsupported-instruction");
                }
                let offset = offer_idx + 1;
                let delta =
                    isize::try_from(offset).map_err(|_| "oracle:unsupported-instruction")?;
                for mut op in fail_builder.instructions {
                    op.shift_jumps(delta, 0);
                    delayed_body.instructions.push(op);
                }
                let end_dest = delayed_body.instructions.len();
                if let Some(Instruction::OfferDynamicManaPayment {
                    paid_destination, ..
                }) = delayed_body.instructions.get_mut(offer_idx)
                {
                    *paid_destination = end_dest;
                }
                return Ok(());
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::RevealRandomFromHand { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                let p_sel = spellwake_engine::toolbox::PlayerSelector::Resolving(
                    ResolvingPlayer::LoopPlayer,
                );
                let op = crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                p_sel.clone(),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::All,
                        },
                        spellwake_engine::toolbox::ZoneSelection::Random {
                            count: Box::new(ValueExpr::literal(1)),
                            chooser: p_sel,
                        },
                        spellwake_engine::toolbox::ZoneDest::RevealOnly,
                    ),
                );
                push_for_each_player(players, vec![op], builder);
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let p_sel = spellwake_engine::toolbox::PlayerSelector::Resolving(rp);
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                p_sel.clone(),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::All,
                        },
                        spellwake_engine::toolbox::ZoneSelection::Random {
                            count: Box::new(ValueExpr::literal(1)),
                            chooser: p_sel,
                        },
                        spellwake_engine::toolbox::ZoneDest::RevealOnly,
                    ),
                ),
            );
            Ok(())
        }
        InstructionAst::ManifestByPlayer {
            player,
            count,
            dread,
            subject: None,
        } => {
            if *player == PlayerRefAst::You {
                return lower_instruction(
                    &InstructionAst::Manifest {
                        count: count.clone(),
                        dread: *dread,
                        subject: None,
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            if *player == PlayerRefAst::ItsOwner && builder.target == BodyTarget::None {
                return Err("oracle:unsupported-its-owner");
            }
            const MANIFEST_DREAD_REST_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(945);
            let build_ops = |rp: ResolvingPlayer,
                             b: &BodyBuilder|
             -> Result<Vec<Instruction>, &'static str> {
                if !*dread {
                    let amt = lower_value_expr_in_context(
                        resolve_x(count, bound_x),
                        b,
                        in_trigger,
                        vocabulary,
                    )?;
                    return Ok(vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::manifest_top(rp, amt),
                    ))]);
                }
                let n = match resolve_x(count, bound_x) {
                    ValueExprAst::Const(k) if *k >= 1 && *k <= 8 => {
                        usize::try_from(*k).unwrap_or(1)
                    }
                    _ => return Err("oracle:unsupported-manifest-dread-count"),
                };
                let p_sel = spellwake_engine::toolbox::PlayerSelector::Resolving(rp);
                let mut ops = Vec::with_capacity(n * 2);
                for _ in 0..n {
                    let mut pick = spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                p_sel.clone(),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(
                                ValueExpr::literal(2),
                            )),
                        },
                        spellwake_engine::toolbox::ZoneSelection::Prompt {
                            chooser: p_sel.clone(),
                            min: Box::new(ValueExpr::literal(1)),
                            max: Box::new(ValueExpr::literal(1)),
                            constraint: None,
                            category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                            hidden: true,
                        },
                        spellwake_engine::toolbox::ZoneDest::battlefield_face_down(
                            spellwake_engine::toolbox::ZoneController::Player(Box::new(
                                p_sel.clone(),
                            )),
                            false,
                        ),
                    );
                    pick.bind_unchosen = Some(MANIFEST_DREAD_REST_BIND);
                    ops.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(pick),
                    );
                    ops.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Bound(
                                    MANIFEST_DREAD_REST_BIND,
                                ),
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Move,
                                },
                            ),
                        ),
                    );
                }
                Ok(ops)
            };
            if let Some(players) = try_group_affected_players(*player) {
                let ops = build_ops(ResolvingPlayer::LoopPlayer, builder)?;
                push_for_each_player(players, ops, builder);
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let ops = build_ops(rp, builder)?;
            builder.instructions.extend(ops);
            Ok(())
        }
        InstructionAst::CloakByPlayer {
            player,
            count,
            subject,
        } => {
            if *player == PlayerRefAst::You {
                return lower_instruction(
                    &InstructionAst::Cloak {
                        count: count.clone(),
                        subject: subject.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            if *player == PlayerRefAst::ItsOwner && builder.target == BodyTarget::None {
                return Err("oracle:unsupported-its-owner");
            }
            let build_ops = |rp: ResolvingPlayer,
                             b: &BodyBuilder|
             -> Result<Vec<Instruction>, &'static str> {
                let p_sel = spellwake_engine::toolbox::PlayerSelector::Resolving(rp);
                let dest = spellwake_engine::toolbox::ZoneDest::battlefield_cloaked(
                    spellwake_engine::toolbox::ZoneController::Player(Box::new(p_sel.clone())),
                    false,
                );
                let amt = lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    b,
                    in_trigger,
                    vocabulary,
                )?;
                match subject.as_deref() {
                    None => {
                        let spec = spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                    p_sel,
                                )),
                                filter: Box::new(ObjectFilter::any()),
                                slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(amt)),
                            },
                            dest,
                        );
                        Ok(vec![
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec),
                        ])
                    }
                    Some(SubjectRefAst::Group(f))
                        if matches!(
                            f.zone,
                            Some(
                                ZoneFilter::YourHand
                                    | ZoneFilter::ThatPlayerHand
                                    | ZoneFilter::EachPlayerHand
                            )
                        ) && !f.any_spell
                            && !f.is_ability =>
                    {
                        let mut clean = f.as_ref().clone();
                        clean.zone = None;
                        if matches!(
                            clean.controller,
                            Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
                        ) {
                            clean.controller = None;
                        }
                        let obj = if is_unrestricted_filter(&clean) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&clean, b, in_trigger, vocabulary)?
                        };
                        let spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Hand(Box::new(p_sel.clone())),
                                obj,
                            ),
                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                chooser: p_sel,
                                min: Box::new(amt.clone()),
                                max: Box::new(amt),
                                constraint: None,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                hidden: true,
                            },
                            dest,
                        );
                        Ok(vec![
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec),
                        ])
                    }
                    Some(SubjectRefAst::Anaphora(raw)) => {
                        let low = raw.trim().to_ascii_lowercase();
                        if matches!(
                            low.as_str(),
                            "the top card of their library"
                                | "top card of their library"
                                | "the top card of that player's library"
                                | "top card of that player's library"
                                | "the top card of your library"
                                | "top card of your library"
                        ) {
                            let spec = spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                        p_sel,
                                    )),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(
                                        amt,
                                    )),
                                },
                                dest,
                            );
                            return Ok(vec![
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                    spec,
                                ),
                            ]);
                        }
                        if matches!(
                            low.as_str(),
                            "it" | "that card"
                                | "them"
                                | "those cards"
                                | "the exiled card"
                                | "that exiled card"
                                | "the revealed card"
                                | "that revealed card"
                        ) {
                            let q = if b.target != BodyTarget::None
                                && !matches!(
                                    b.target,
                                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                                ) {
                                spellwake_engine::toolbox::ObjectQuery::Targets
                            } else if last_instruction_moved_object(&b.instructions)
                                || has_prior_moved_object(&b.instructions)
                            {
                                spellwake_engine::toolbox::ObjectQuery::LastMoved
                            } else if in_trigger {
                                spellwake_engine::toolbox::ObjectQuery::TriggerSubject
                            } else {
                                return Err("oracle:unsupported-cloak-subject");
                            };
                            return Ok(vec![
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(q, dest),
                                ),
                            ]);
                        }
                        Err("oracle:unsupported-cloak-subject")
                    }
                    Some(SubjectRefAst::TriggerSubject) if in_trigger => Ok(vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                dest,
                            ),
                        ),
                    ]),
                    _ => Err("oracle:unsupported-cloak-subject"),
                }
            };
            if let Some(players) = try_group_affected_players(*player) {
                let ops = build_ops(ResolvingPlayer::LoopPlayer, builder)?;
                push_for_each_player(players, ops, builder);
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let ops = build_ops(rp, builder)?;
            builder.instructions.extend(ops);
            Ok(())
        }
        InstructionAst::SacrificeAllExcept {
            player,
            filter,
            keep,
        } => {
            if filter.is_ability
                || filter.any_spell
                || !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            {
                return Err("oracle:unsupported-instruction");
            }
            if !matches!(
                filter.controller,
                None | Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
            ) {
                return Err("oracle:unsupported-instruction");
            }
            if let Some(players) = try_group_affected_players(*player) {
                let rewritten = InstructionAst::SacrificeAllExcept {
                    player: PlayerRefAst::ThatPlayer,
                    filter: Box::new(rewrite_loop_player_filter(filter, *player)),
                    keep: Box::new(rewrite_loop_player_value_expr(keep, *player)),
                };
                let sub = lower_loop_player_sub_body(
                    &[rewritten],
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                builder.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                    body: Box::new(sub),
                });
                return Ok(());
            }
            const SAC_ALL_EXCEPT_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(946);
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let keep_expr = lower_value_expr_in_context(
                resolve_x(keep, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            clean.controller = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&clean, builder, in_trigger, vocabulary)?
            };
            let p_sel = spellwake_engine::toolbox::PlayerSelector::Resolving(rp);
            let mut keep_pick = spellwake_engine::toolbox::ZoneOpSpec::simple(
                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                    zone: spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                        p_sel.clone(),
                    )),
                    filter: Box::new(obj),
                    slice: spellwake_engine::toolbox::QuerySlice::All,
                },
                spellwake_engine::toolbox::ZoneSelection::Prompt {
                    chooser: p_sel,
                    min: Box::new(keep_expr.clone()),
                    max: Box::new(keep_expr),
                    constraint: None,
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    hidden: false,
                },
                spellwake_engine::toolbox::ZoneDest::None,
            );
            keep_pick.bind_unchosen = Some(SAC_ALL_EXCEPT_UNCHOSEN_BIND);
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(keep_pick));
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Bound(SAC_ALL_EXCEPT_UNCHOSEN_BIND),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                        },
                    ),
                ),
            );
            Ok(())
        }
        InstructionAst::ManualTabletopAction { .. } => {
            Err("oracle:tabletop-honor-excluded-from-constructed")
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

fn try_scale_instruction_ext(instr: &InstructionAst, mult: ValueExprAst) -> Option<InstructionAst> {
    if let Some(scaled) = try_scale_instruction(instr, mult.clone()) {
        return Some(scaled);
    }
    match instr {
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except,
            half: false,
            rounding: None,
            chooser,
        } if matches!(subject.as_ref(), SubjectRefAst::Group(_)) => {
            Some(InstructionAst::Sacrifice {
                player: *player,
                count: Some(Box::new(scale_ast_amount(
                    count.as_deref().unwrap_or(&ValueExprAst::Const(1)),
                    mult,
                ))),
                subject: subject.clone(),
                except: except.clone(),
                half: false,
                rounding: None,
                chooser: *chooser,
            })
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: None,
        } if parse_signed_pt_delta(dx).is_some() && parse_signed_pt_delta(dy).is_some() => {
            Some(InstructionAst::ModifyPT {
                subject: subject.clone(),
                dx: dx.clone(),
                dy: dy.clone(),
                duration: duration.clone(),
                scale: Some(Box::new(mult)),
            })
        }
        InstructionAst::AddMana {
            player,
            mana: ManaSpecAst::Fixed(raw),
        } => Some(InstructionAst::AddMana {
            player: *player,
            mana: ManaSpecAst::ScaledSymbol {
                symbol: raw.clone(),
                amount: Box::new(mult),
            },
        }),
        _ => None,
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

fn try_lower_choose_objects_new_primitives(
    chooser: PlayerRefAst,
    cleaned: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Option<Result<(), &'static str>> {
    let raw = cleaned.trim().trim_end_matches('.');
    // 0. Random card/permanent choice via universal ZoneOp(ZoneSelection::Random, ZoneDest::None).
    if let Some((prefix_raw, suffix_raw)) = raw.split_once(" at random") {
        let count_body = prefix_raw
            .strip_prefix("one ")
            .or_else(|| prefix_raw.strip_prefix("an "))
            .or_else(|| prefix_raw.strip_prefix("a "))
            .map(str::trim);
        if let Some(card_phrase) = count_body
            && !card_phrase.is_empty()
            && !card_phrase.contains("target")
        {
            let suffix = suffix_raw.trim();
            let mut trial = builder.clone();
            let linked_exile_suffix = (self_pronoun_valid || in_trigger)
                && matches!(
                    suffix,
                    "exiled with ~"
                        | "exiled with it"
                        | "exiled with this enchantment"
                        | "exiled with this artifact"
                        | "exiled with this creature"
                        | "exiled with this permanent"
                );
            let qz_opt: Option<spellwake_engine::toolbox::QueryZone> = if linked_exile_suffix {
                None
            } else if suffix.is_empty() {
                Some(spellwake_engine::toolbox::QueryZone::Battlefield)
            } else if matches!(suffix, "from your graveyard" | "in your graveyard") {
                Some(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                )))
            } else if matches!(
                suffix,
                "from an opponent's graveyard" | "in an opponent's graveyard"
            ) {
                Some(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent),
                )))
            } else if matches!(
                suffix,
                "from target opponent's graveyard" | "in target opponent's graveyard"
            ) {
                trial
                    .bind_target(BodyTarget::SingleOpponent)
                    .ok()
                    .map(|()| {
                        spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                        ))
                    })
            } else if matches!(
                suffix,
                "from target player's graveyard" | "in target player's graveyard"
            ) {
                trial.bind_target(BodyTarget::SinglePlayer).ok().map(|()| {
                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::TargetPlayer,
                        ),
                    ))
                })
            } else if matches!(suffix, "from a graveyard" | "in a graveyard") {
                Some(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer),
                )))
            } else if matches!(suffix, "from your hand" | "in your hand") {
                Some(spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                )))
            } else if matches!(
                suffix,
                "from target opponent's hand" | "in target opponent's hand"
            ) {
                trial
                    .bind_target(BodyTarget::SingleOpponent)
                    .ok()
                    .map(|()| {
                        spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                        ))
                    })
            } else if matches!(
                suffix,
                "from target player's hand" | "in target player's hand"
            ) {
                trial.bind_target(BodyTarget::SinglePlayer).ok().map(|()| {
                    spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::TargetPlayer,
                        ),
                    ))
                })
            } else if matches!(suffix, "from exile" | "in exile") {
                Some(spellwake_engine::toolbox::QueryZone::Exile)
            } else {
                None
            };
            if (linked_exile_suffix || qz_opt.is_some())
                && let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                    card_phrase,
                    &cached_compiler_vocab(vocabulary),
                )
                && parsed.zone.is_none()
                && !parsed.any_spell
                && !parsed.is_ability
            {
                let p_sel = if let Some(players) = try_group_affected_players(chooser) {
                    Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
                } else {
                    lower_resolving_player_in_context(chooser, &mut trial, in_trigger)
                        .ok()
                        .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
                };
                if let Some(p_sel) = p_sel
                    && let Ok(obj) = if is_unrestricted_filter(&parsed) {
                        Ok(ObjectFilter::any())
                    } else {
                        lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary)
                    }
                    && bind_filter_player_target(&mut trial, &obj).is_ok()
                {
                    const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                        spellwake_engine::toolbox::BindingId(923);
                    let source_query = if linked_exile_suffix {
                        if obj == ObjectFilter::any() {
                            spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                        } else {
                            spellwake_engine::toolbox::ObjectQuery::Filtered {
                                source: Box::new(
                                    spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                        group: None,
                                    },
                                ),
                                filter: Box::new(obj),
                            }
                        }
                    } else {
                        let qz = qz_opt?;
                        spellwake_engine::toolbox::ObjectQuery::zone(qz, obj)
                    };
                    let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                        source_query,
                        spellwake_engine::toolbox::ZoneSelection::Random {
                            count: Box::new(ValueExpr::literal(1)),
                            chooser: p_sel,
                        },
                        spellwake_engine::toolbox::ZoneDest::None,
                    );
                    spec.bind_moved = Some(CHOOSE_OBJ_BIND);
                    trial.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec),
                    );
                    *builder = trial;
                    return Some(Ok(()));
                }
            }
        }
    }
    if raw.is_empty()
        || raw.contains("at random")
        || raw.contains("with different")
        || raw.contains("total power")
        || raw.contains("total mana value")
        || raw.contains("greatest")
        || raw.contains("least")
        || raw.contains("most")
        || raw.contains("fewest")
        || raw.contains("lesser")
        || raw.contains("greater")
        || raw.contains("equal")
        || raw.contains("secretly")
    {
        return None;
    }

    // 1. Suspended card you own (Exile + ObjectFilterFlags::SUSPENDED).
    if chooser == PlayerRefAst::You
        && matches!(raw, "a suspended card you own" | "suspended card you own")
    {
        let clause = ObjectClause {
            owner: ControllerFilter::You,
            flags: ObjectFilterFlags::SUSPENDED,
            ..ObjectClause::any()
        };
        builder.instructions.push(Instruction::ChooseObjects {
            players: AffectedPlayers::One(ResolvingPlayer::Controller),
            source_zone: SourceZoneFilter::Exile,
            filter: Box::new(ObjectFilter::Clause(clause)),
            min: ValueExpr::literal(1),
            max: ValueExpr::literal(1),
            distinct_powers: false,
            max_total_power: None,
            unchosen_action: None,
        });
        return Some(Ok(()));
    }

    let (min_val, max_val, body_raw) = if let Some(rest) = raw.strip_prefix("up to one ") {
        (0i64, 1i64, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to two ") {
        (0, 2, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to three ") {
        (0, 3, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to four ") {
        (0, 4, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to five ") {
        (0, 5, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to six ") {
        (0, 6, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to seven ") {
        (0, 7, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to eight ") {
        (0, 8, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to nine ") {
        (0, 9, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("up to ten ") {
        (0, 10, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("any number of ") {
        (0, 100, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("one or more ") {
        (1, 100, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("exactly four ") {
        (4, 4, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("exactly three ") {
        (3, 3, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("exactly two ") {
        (2, 2, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("exactly one ") {
        (1, 1, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("ten ") {
        (10, 10, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("nine ") {
        (9, 9, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("eight ") {
        (8, 8, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("seven ") {
        (7, 7, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("six ") {
        (6, 6, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("five ") {
        (5, 5, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("four ") {
        (4, 4, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("three ") {
        (3, 3, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("two ") {
        (2, 2, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("one ") {
        (1, 1, rest.trim())
    } else if let Some(rest) = raw.strip_prefix("an ") {
        (1, 1, rest.trim())
    } else {
        let rest = raw.strip_prefix("a ")?;
        (1, 1, rest.trim())
    };
    if body_raw.is_empty() {
        return None;
    }

    // 2. Card from a revealed or looked-at hand (TargetPlayerHand or prior ZoneOp Hand Reveal/Look).
    if chooser == PlayerRefAst::You
        && !body_raw.contains("target")
        && let Some(card_phrase) = body_raw
            .strip_suffix(" from it")
            .or_else(|| body_raw.strip_suffix(" in it"))
            .or_else(|| body_raw.strip_suffix(" from that hand"))
    {
        let prior_revealed_hand: Option<Option<spellwake_engine::toolbox::PlayerSelector>> =
            builder.instructions.iter().rev().find_map(|op| match op {
                Instruction::ZoneOp(spec)
                    if matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::RevealOnly
                            | spellwake_engine::toolbox::ZoneDest::LookOnly { .. }
                    ) =>
                {
                    if let spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Hand(owner),
                        ..
                    } = &spec.source
                    {
                        Some(Some((**owner).clone()))
                    } else {
                        Some(None)
                    }
                }

                _ => None,
            });
        if let Some(revealed_hand_owner) = prior_revealed_hand
            && let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                card_phrase.trim(),
                &cached_compiler_vocab(vocabulary),
            )
            && parsed.zone.is_none()
            && let Ok(obj) = if is_unrestricted_filter(&parsed) {
                Ok(ObjectFilter::any())
            } else {
                lower_object_filter_in_context(&parsed, builder, in_trigger, vocabulary)
            }
            && !obj.has_target_player()
        {
            if matches!(
                builder.target,
                BodyTarget::SingleOpponent | BodyTarget::SinglePlayer
            ) {
                builder.instructions.push(Instruction::ChooseObjects {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    source_zone: SourceZoneFilter::TargetPlayerHand,
                    filter: Box::new(obj),
                    min: ValueExpr::literal(min_val),
                    max: ValueExpr::literal(max_val),
                    distinct_powers: false,
                    max_total_power: None,
                    unchosen_action: None,
                });
                return Some(Ok(()));
            }
            if let Some(hand_owner) = revealed_hand_owner {
                const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(923);
                const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(924);
                let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                    spellwake_engine::toolbox::ObjectQuery::zone(
                        spellwake_engine::toolbox::QueryZone::Hand(Box::new(hand_owner)),
                        obj,
                    ),
                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                        chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        min: Box::new(ValueExpr::literal(min_val)),
                        max: Box::new(ValueExpr::literal(max_val)),
                        constraint: None,
                        category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                        hidden: false,
                    },
                    spellwake_engine::toolbox::ZoneDest::None,
                );
                spec.bind_moved = Some(CHOOSE_OBJ_BIND);
                spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
                return Some(Ok(()));
            }
        }
    }

    // 3. Card from objects moved earlier in this resolution (SourceZoneFilter::LastMoved).
    let prior_moved = last_instruction_moved_object(&builder.instructions)
        || has_prior_moved_object(&builder.instructions)
        || builder.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ZoneOp(_)
                    | Instruction::ForEachPlayerQuery { .. }
                    | Instruction::ExileTopPlay { .. }
            ) || matches!(
                op,
                Instruction::MutatePlayer(spec)
                    if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Mill(_))
            )
        });
    if prior_moved
        && !body_raw.contains("target")
        && let Some(card_phrase) = body_raw
            .strip_suffix(" exiled this way")
            .or_else(|| body_raw.strip_suffix(" moved this way"))
            .or_else(|| body_raw.strip_suffix(" milled this way"))
            .or_else(|| body_raw.strip_suffix(" from among them"))
            .or_else(|| body_raw.strip_suffix(" from among those cards"))
            .or_else(|| body_raw.strip_suffix(" from among those"))
    {
        let card_phrase = card_phrase.trim();
        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            card_phrase,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if parsed.zone.is_some() {
            return None;
        }
        let mut trial = builder.clone();
        let players = lower_affected_players_in_context(chooser, &mut trial, in_trigger).ok()?;
        let obj = if is_unrestricted_filter(&parsed) {
            ObjectFilter::any()
        } else {
            lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary).ok()?
        };
        if obj.has_target_player() {
            return None;
        }
        trial.instructions.push(Instruction::ChooseObjects {
            players,
            source_zone: SourceZoneFilter::LastMoved,
            filter: Box::new(obj),
            min: ValueExpr::literal(min_val),
            max: ValueExpr::literal(max_val),
            distinct_powers: false,
            max_total_power: None,
            unchosen_action: None,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    // 4. Explicit "on the battlefield" non-target choice (e.g. Cytoshape).
    if !body_raw.contains("target")
        && !body_raw.contains(" from ")
        && let Some(bf_phrase) = body_raw.strip_suffix(" on the battlefield")
    {
        let bf_phrase = bf_phrase.trim();
        let mut parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            bf_phrase,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if !matches!(parsed.zone, None | Some(ZoneFilter::Battlefield)) {
            return None;
        }
        parsed.zone = None;
        let mut trial = builder.clone();
        let players = lower_affected_players_in_context(chooser, &mut trial, in_trigger).ok()?;
        let obj = if is_unrestricted_filter(&parsed) {
            ObjectFilter::any()
        } else {
            lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary).ok()?
        };
        if obj.has_target_player() {
            return None;
        }
        trial.instructions.push(Instruction::ChooseObjects {
            players,
            source_zone: SourceZoneFilter::AnyBattlefield,
            filter: Box::new(obj),
            min: ValueExpr::literal(min_val),
            max: ValueExpr::literal(max_val),
            distinct_powers: false,
            max_total_power: None,
            unchosen_action: None,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    // 5. Permanent you control other than this permanent (any chooser, e.g. Wormfang Crab).
    if !body_raw.contains("target")
        && !body_raw.contains(" in ")
        && !body_raw.contains(" from ")
        && (self_pronoun_valid || in_trigger)
        && let Some(before_other) = body_raw
            .strip_suffix(" you control other than this creature")
            .or_else(|| body_raw.strip_suffix(" you control other than this permanent"))
            .or_else(|| body_raw.strip_suffix(" you control other than ~"))
    {
        let mut parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            before_other.trim(),
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if parsed.zone.is_some() {
            return None;
        }
        parsed.flags = parsed.flags.union(AstFilterFlags::EXCLUDE_SELF);
        if chooser == PlayerRefAst::You {
            parsed.controller = Some(ControllerRelation::You);
            let mut trial = builder.clone();
            let players =
                lower_affected_players_in_context(chooser, &mut trial, in_trigger).ok()?;
            let obj =
                lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary).ok()?;
            if obj.has_target_player() {
                return None;
            }
            trial.instructions.push(Instruction::ChooseObjects {
                players,
                source_zone: SourceZoneFilter::AnyBattlefield,
                filter: Box::new(obj),
                min: ValueExpr::literal(min_val),
                max: ValueExpr::literal(max_val),
                distinct_powers: false,
                max_total_power: None,
                unchosen_action: None,
            });
            *builder = trial;
            return Some(Ok(()));
        }
        parsed.controller = None;
        let mut trial = builder.clone();
        let p_sel = if matches!(chooser, PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer) {
            trial.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                    opponents_only: chooser == PlayerRefAst::AnyOpponent,
                },
            });
            Some(spellwake_engine::toolbox::PlayerSelector::Resolving(
                ResolvingPlayer::ChosenPlayer,
            ))
        } else if let Some(players) = try_group_affected_players(chooser) {
            Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
        } else {
            lower_resolving_player_in_context(chooser, &mut trial, in_trigger)
                .ok()
                .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
        };
        if let Some(p_sel) = p_sel
            && let Ok(obj) = lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary)
            && !obj.has_target_player()
        {
            const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(923);
            const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(924);
            let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    )),
                    obj,
                ),
                spellwake_engine::toolbox::ZoneSelection::Prompt {
                    chooser: p_sel,
                    min: Box::new(ValueExpr::literal(min_val)),
                    max: Box::new(ValueExpr::literal(max_val)),
                    constraint: None,
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    hidden: false,
                },
                spellwake_engine::toolbox::ZoneDest::None,
            );
            spec.bind_moved = Some(CHOOSE_OBJ_BIND);
            spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
            trial
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
            *builder = trial;
            return Some(Ok(()));
        }
    }

    // 6. Non-target controlled battlefield object in ChooseOption (e.g. Season of Weaving, Urza's Sylex).
    if !body_raw.contains("target")
        && !body_raw.contains(" in ")
        && !body_raw.contains(" from ")
        && let Some(ctrl_phrase) = body_raw.strip_suffix(" you control")
    {
        let mut parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            ctrl_phrase.trim(),
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if parsed.zone.is_some() {
            return None;
        }
        parsed.controller = None;
        if chooser == PlayerRefAst::You {
            let obj = if is_unrestricted_filter(&parsed) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&parsed, builder, in_trigger, vocabulary).ok()?
            };
            if obj.has_target_player() {
                return None;
            }
            builder.instructions.push(Instruction::ChooseObjects {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                source_zone: SourceZoneFilter::Battlefield,
                filter: Box::new(obj),
                min: ValueExpr::literal(min_val),
                max: ValueExpr::literal(max_val),
                distinct_powers: false,
                max_total_power: None,
                unchosen_action: None,
            });
            return Some(Ok(()));
        }
        let mut trial = builder.clone();
        let p_sel = if matches!(chooser, PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer) {
            trial.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                    opponents_only: chooser == PlayerRefAst::AnyOpponent,
                },
            });
            Some(spellwake_engine::toolbox::PlayerSelector::Resolving(
                ResolvingPlayer::ChosenPlayer,
            ))
        } else if let Some(players) = try_group_affected_players(chooser) {
            Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
        } else {
            lower_resolving_player_in_context(chooser, &mut trial, in_trigger)
                .ok()
                .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
        };
        if let Some(p_sel) = p_sel
            && let Ok(obj) = if is_unrestricted_filter(&parsed) {
                Ok(ObjectFilter::any())
            } else {
                lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary)
            }
            && !obj.has_target_player()
        {
            const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(923);
            const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(924);
            let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    )),
                    obj,
                ),
                spellwake_engine::toolbox::ZoneSelection::Prompt {
                    chooser: p_sel,
                    min: Box::new(ValueExpr::literal(min_val)),
                    max: Box::new(ValueExpr::literal(max_val)),
                    constraint: None,
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    hidden: false,
                },
                spellwake_engine::toolbox::ZoneDest::None,
            );
            spec.bind_moved = Some(CHOOSE_OBJ_BIND);
            spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
            trial
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
            *builder = trial;
            return Some(Ok(()));
        }
    }

    // 6b. Non-You chooser choosing permanents "they control" (e.g. Urza's Sylex: "Each player chooses six lands they control").
    if chooser != PlayerRefAst::You
        && !body_raw.contains("target")
        && !body_raw.contains(" in ")
        && !body_raw.contains(" from ")
        && let Some(ctrl_phrase) = body_raw.strip_suffix(" they control")
    {
        let mut parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            ctrl_phrase.trim(),
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if parsed.zone.is_some() {
            return None;
        }
        parsed.controller = None;
        let mut trial = builder.clone();
        let players = lower_affected_players_in_context(chooser, &mut trial, in_trigger).ok()?;
        let obj = if is_unrestricted_filter(&parsed) {
            ObjectFilter::any()
        } else {
            lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary).ok()?
        };
        if obj.has_target_player() {
            return None;
        }
        trial.instructions.push(Instruction::ChooseObjects {
            players,
            source_zone: SourceZoneFilter::Battlefield,
            filter: Box::new(obj),
            min: ValueExpr::literal(min_val),
            max: ValueExpr::literal(max_val),
            distinct_powers: false,
            max_total_power: None,
            unchosen_action: None,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    // 7. Graveyard card choices using TargetPlayerGraveyard, OpponentGraveyard, AnyGraveyard, Graveyard.
    if !body_raw.contains(" from ") {
        let gy_match: Option<(&str, SourceZoneFilter, Option<BodyTarget>)> =
            if let Some(p) = body_raw.strip_suffix(" in target opponent's graveyard") {
                Some((
                    p,
                    SourceZoneFilter::TargetPlayerGraveyard,
                    Some(BodyTarget::SingleOpponent),
                ))
            } else if let Some(p) = body_raw.strip_suffix(" in target player's graveyard") {
                Some((
                    p,
                    SourceZoneFilter::TargetPlayerGraveyard,
                    Some(BodyTarget::SinglePlayer),
                ))
            } else if let Some(p) = body_raw.strip_suffix(" in an opponent's graveyard")
                && chooser == PlayerRefAst::You
            {
                Some((p, SourceZoneFilter::OpponentGraveyard, None))
            } else if let Some(p) = body_raw
                .strip_suffix(" in a graveyard")
                .or_else(|| body_raw.strip_suffix(" in any graveyard"))
            {
                Some((p, SourceZoneFilter::AnyGraveyard, None))
            } else if let Some(p) = body_raw.strip_suffix(" in your graveyard")
                && chooser == PlayerRefAst::You
            {
                Some((p, SourceZoneFilter::Graveyard, None))
            } else if let Some(p) = body_raw.strip_suffix(" in their graveyard") {
                Some((p, SourceZoneFilter::Graveyard, None))
            } else {
                None
            };
        if let Some((card_phrase, source_zone, target_opt)) = gy_match
            && !card_phrase.contains("target")
        {
            let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                card_phrase.trim(),
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            if parsed.zone.is_some() {
                return None;
            }
            let mut trial = builder.clone();
            if let Some(bt) = target_opt
                && trial.bind_target(bt).is_err()
            {
                return None;
            }
            let players =
                lower_affected_players_in_context(chooser, &mut trial, in_trigger).ok()?;
            let obj = if is_unrestricted_filter(&parsed) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary).ok()?
            };
            if obj.has_target_player() {
                return None;
            }
            trial.instructions.push(Instruction::ChooseObjects {
                players,
                source_zone,
                filter: Box::new(obj),
                min: ValueExpr::literal(min_val),
                max: ValueExpr::literal(max_val),
                distinct_powers: false,
                max_total_power: None,
                unchosen_action: None,
            });
            *builder = trial;
            return Some(Ok(()));
        }
    }

    // 8. Hand card choices via universal ZoneOp(ZoneSelection::Prompt, ZoneDest::None).
    {
        let hand_phrase = body_raw
            .strip_suffix(" in your hand")
            .or_else(|| body_raw.strip_suffix(" from your hand"))
            .or_else(|| body_raw.strip_suffix(" in their hand"))
            .or_else(|| body_raw.strip_suffix(" from their hand"));
        if let Some(card_phrase) = hand_phrase
            && !card_phrase.contains("target")
            && let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                card_phrase.trim(),
                &cached_compiler_vocab(vocabulary),
            )
            && parsed.zone.is_none()
        {
            let mut trial = builder.clone();
            let p_sel = if let Some(players) = try_group_affected_players(chooser) {
                Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
            } else {
                lower_resolving_player_in_context(chooser, &mut trial, in_trigger)
                    .ok()
                    .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
            };
            if let Some(p_sel) = p_sel
                && let Ok(obj) = if is_unrestricted_filter(&parsed) {
                    Ok(ObjectFilter::any())
                } else {
                    lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary)
                }
                && bind_filter_player_target(&mut trial, &obj).is_ok()
            {
                const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(923);
                const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(924);
                let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                    spellwake_engine::toolbox::ObjectQuery::zone(
                        spellwake_engine::toolbox::QueryZone::Hand(Box::new(p_sel.clone())),
                        obj,
                    ),
                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                        chooser: p_sel,
                        min: Box::new(ValueExpr::literal(min_val)),
                        max: Box::new(ValueExpr::literal(max_val)),
                        constraint: None,
                        category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                        hidden: true,
                    },
                    spellwake_engine::toolbox::ZoneDest::None,
                );
                spec.bind_moved = Some(CHOOSE_OBJ_BIND);
                spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
                trial
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
                *builder = trial;
                return Some(Ok(()));
            }
        }
    }

    // 9. Exile card choices via universal ZoneOp(ZoneSelection::Prompt, ZoneDest::None).
    {
        let mut required_counter_override: Option<CounterKind> = None;
        let mut linked_exile_phrase: Option<&str> = None;
        if self_pronoun_valid || in_trigger {
            for suf in [
                " exiled with ~",
                " exiled with it",
                " exiled with this enchantment",
                " exiled with this artifact",
                " exiled with this creature",
                " exiled with this permanent",
            ] {
                if let Some(p) = body_raw.strip_suffix(suf) {
                    linked_exile_phrase = Some(p.trim());
                    break;
                }
            }
        }
        let exile_match: Option<(String, spellwake_engine::toolbox::QueryZone)> =
            if linked_exile_phrase.is_some() {
                None
            } else if let Some(p) = body_raw.strip_suffix(" you own in exile") {
                Some((
                    p.trim().to_string(),
                    spellwake_engine::toolbox::QueryZone::ExileOwnedBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    )),
                ))
            } else if let Some(p) = body_raw
                .strip_suffix(" an opponent owns in exile")
                .or_else(|| body_raw.strip_suffix(" opponents own in exile"))
            {
                Some((
                    p.trim().to_string(),
                    spellwake_engine::toolbox::QueryZone::ExileOwnedBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Group(
                            AffectedPlayers::EachOpponent,
                        ),
                    )),
                ))
            } else if let Some(p) = body_raw.strip_suffix(" in exile") {
                Some((
                    p.trim().to_string(),
                    spellwake_engine::toolbox::QueryZone::Exile,
                ))
            } else if let Some(after_ex) = body_raw.strip_prefix("exiled ") {
                let (without_void, has_void) =
                    if let Some(r) = after_ex.strip_suffix(" with a void counter on it") {
                        (r.trim(), true)
                    } else if let Some(r) = after_ex.strip_suffix(" with a void counter") {
                        (r.trim(), true)
                    } else {
                        (after_ex.trim(), false)
                    };
                if has_void {
                    required_counter_override = Some(CounterKind::Void);
                }
                if let Some(p) = without_void
                    .strip_suffix(" an opponent owns")
                    .or_else(|| without_void.strip_suffix(" opponents own"))
                {
                    Some((
                        p.trim().to_string(),
                        spellwake_engine::toolbox::QueryZone::ExileOwnedBy(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Group(
                                AffectedPlayers::EachOpponent,
                            ),
                        )),
                    ))
                } else if let Some(p) = without_void.strip_suffix(" you own") {
                    Some((
                        p.trim().to_string(),
                        spellwake_engine::toolbox::QueryZone::ExileOwnedBy(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                        )),
                    ))
                } else if has_void {
                    Some((
                        without_void.to_string(),
                        spellwake_engine::toolbox::QueryZone::Exile,
                    ))
                } else {
                    None
                }
            } else {
                None
            };
        let phrase_opt: Option<&str> =
            linked_exile_phrase.or_else(|| exile_match.as_ref().map(|(s, _)| s.as_str()));
        if let Some(card_phrase) = phrase_opt
            && !card_phrase.contains("target")
            && let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                card_phrase.trim(),
                &cached_compiler_vocab(vocabulary),
            )
            && parsed.zone.is_none()
        {
            let mut trial = builder.clone();
            let p_sel = if let Some(players) = try_group_affected_players(chooser) {
                Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
            } else {
                lower_resolving_player_in_context(chooser, &mut trial, in_trigger)
                    .ok()
                    .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
            };
            if let Some(p_sel) = p_sel
                && let Ok(mut obj) = if is_unrestricted_filter(&parsed) {
                    Ok(ObjectFilter::any())
                } else {
                    lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary)
                }
                && bind_filter_player_target(&mut trial, &obj).is_ok()
            {
                if let Some(ck) = required_counter_override {
                    obj = match obj {
                        ObjectFilter::Clause(mut clause) => {
                            if clause.required_counter_kind.is_some() {
                                return None;
                            }
                            clause.required_counter_kind = Some(ck);
                            ObjectFilter::Clause(clause)
                        }
                        _ => return None,
                    };
                }
                const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(923);
                const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(924);
                let source_query = if linked_exile_phrase.is_some() {
                    if obj == ObjectFilter::any() {
                        spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                    } else {
                        spellwake_engine::toolbox::ObjectQuery::Filtered {
                            source: Box::new(spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                group: None,
                            }),
                            filter: Box::new(obj),
                        }
                    }
                } else if let Some((_, qz)) = exile_match {
                    spellwake_engine::toolbox::ObjectQuery::zone(qz, obj)
                } else {
                    return None;
                };
                let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                    source_query,
                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                        chooser: p_sel,
                        min: Box::new(ValueExpr::literal(min_val)),
                        max: Box::new(ValueExpr::literal(max_val)),
                        constraint: None,
                        category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                        hidden: false,
                    },
                    spellwake_engine::toolbox::ZoneDest::None,
                );
                spec.bind_moved = Some(CHOOSE_OBJ_BIND);
                spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
                trial
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
                *builder = trial;
                return Some(Ok(()));
            }
        }
    }

    // 9b. Graveyard card choices via universal ZoneOp(ZoneSelection::Prompt, ZoneDest::None).
    {
        let mut trial_gy = builder.clone();
        let gy_match: Option<(&str, spellwake_engine::toolbox::QueryZone)> = if let Some(p) =
            body_raw
                .strip_suffix(" in your graveyard")
                .or_else(|| body_raw.strip_suffix(" from your graveyard"))
        {
            Some((
                p,
                spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                )),
            ))
        } else if let Some(p) = body_raw
            .strip_suffix(" in their graveyard")
            .or_else(|| body_raw.strip_suffix(" from their graveyard"))
        {
            let gy_owner = if let Some(players) = try_group_affected_players(chooser) {
                Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
            } else {
                lower_resolving_player_in_context(chooser, &mut trial_gy, in_trigger)
                    .ok()
                    .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
            };
            gy_owner.map(|sel| {
                (
                    p,
                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(sel)),
                )
            })
        } else if let Some(p) = body_raw
            .strip_suffix(" in target opponent's graveyard")
            .or_else(|| body_raw.strip_suffix(" from target opponent's graveyard"))
        {
            trial_gy
                .bind_target(BodyTarget::SingleOpponent)
                .ok()
                .map(|()| {
                    (
                        p,
                        spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                        )),
                    )
                })
        } else if let Some(p) = body_raw
            .strip_suffix(" in target player's graveyard")
            .or_else(|| body_raw.strip_suffix(" from target player's graveyard"))
        {
            trial_gy
                .bind_target(BodyTarget::SinglePlayer)
                .ok()
                .map(|()| {
                    (
                        p,
                        spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                        )),
                    )
                })
        } else if let Some(p) = body_raw
            .strip_suffix(" in an opponent's graveyard")
            .or_else(|| body_raw.strip_suffix(" from an opponent's graveyard"))
        {
            Some((
                p,
                spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent),
                )),
            ))
        } else if let Some(p) = body_raw
            .strip_suffix(" in a graveyard")
            .or_else(|| body_raw.strip_suffix(" from a graveyard"))
        {
            Some((
                p,
                spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer),
                )),
            ))
        } else {
            None
        };
        if let Some((card_phrase, qz)) = gy_match
            && !card_phrase.contains("target")
            && let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                card_phrase.trim(),
                &cached_compiler_vocab(vocabulary),
            )
            && parsed.zone.is_none()
        {
            let p_sel = if let Some(players) = try_group_affected_players(chooser) {
                Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
            } else {
                lower_resolving_player_in_context(chooser, &mut trial_gy, in_trigger)
                    .ok()
                    .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
            };
            if let Some(p_sel) = p_sel
                && let Ok(obj) = if is_unrestricted_filter(&parsed) {
                    Ok(ObjectFilter::any())
                } else {
                    lower_object_filter_in_context(&parsed, &trial_gy, in_trigger, vocabulary)
                }
                && bind_filter_player_target(&mut trial_gy, &obj).is_ok()
            {
                const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(923);
                const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                    spellwake_engine::toolbox::BindingId(924);
                let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                    spellwake_engine::toolbox::ObjectQuery::zone(qz, obj),
                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                        chooser: p_sel,
                        min: Box::new(ValueExpr::literal(min_val)),
                        max: Box::new(ValueExpr::literal(max_val)),
                        constraint: None,
                        category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                        hidden: false,
                    },
                    spellwake_engine::toolbox::ZoneDest::None,
                );
                spec.bind_moved = Some(CHOOSE_OBJ_BIND);
                spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
                trial_gy
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
                *builder = trial_gy;
                return Some(Ok(()));
            }
        }
    }

    // 10. General non-target battlefield permanent choice (e.g. "a creature an opponent controls",
    // "a nonland permanent", "another creature you control") via universal ZoneOp.
    if !body_raw.contains("target")
        && !body_raw.contains(" in ")
        && !body_raw.contains(" from ")
        && let Ok(mut parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
            body_raw,
            &cached_compiler_vocab(vocabulary),
        )
        && matches!(parsed.zone, None | Some(ZoneFilter::Battlefield))
        && !parsed.any_spell
        && !parsed.is_ability
        && (!parsed.any_types.is_empty()
            || parsed.any_permanent
            || !parsed.subtypes.is_empty()
            || !parsed.forbidden_types.is_empty()
            || parsed.controller.is_some())
    {
        parsed.zone = None;
        let mut trial = builder.clone();
        let p_sel = if let Some(players) = try_group_affected_players(chooser) {
            Some(spellwake_engine::toolbox::PlayerSelector::Group(players))
        } else {
            lower_resolving_player_in_context(chooser, &mut trial, in_trigger)
                .ok()
                .map(spellwake_engine::toolbox::PlayerSelector::Resolving)
        };
        if let Some(p_sel) = p_sel
            && let Ok(obj) = if is_unrestricted_filter(&parsed) {
                Ok(ObjectFilter::any())
            } else {
                lower_object_filter_in_context(&parsed, &trial, in_trigger, vocabulary)
            }
            && bind_filter_player_target(&mut trial, &obj).is_ok()
        {
            const CHOOSE_OBJ_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(923);
            const CHOOSE_UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                spellwake_engine::toolbox::BindingId(924);
            let mut spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::Battlefield,
                    obj,
                ),
                spellwake_engine::toolbox::ZoneSelection::Prompt {
                    chooser: p_sel,
                    min: Box::new(ValueExpr::literal(min_val)),
                    max: Box::new(ValueExpr::literal(max_val)),
                    constraint: None,
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    hidden: false,
                },
                spellwake_engine::toolbox::ZoneDest::None,
            );
            spec.bind_moved = Some(CHOOSE_OBJ_BIND);
            spec.bind_unchosen = Some(CHOOSE_UNCHOSEN_BIND);
            trial
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
            *builder = trial;
            return Some(Ok(()));
        }
    }

    None
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
        InstructionAst::ExileWithCounters {
            subject,
            counter_kind,
            count,
            face_down,
        } if is_loop_object_anaphora(subject) && !value_expr_ast_references_anaphora(count) => {
            Some(InstructionAst::ExileWithCounters {
                subject: all_subj(),
                counter_kind: counter_kind.clone(),
                count: count.clone(),
                face_down: *face_down,
            })
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
        Instruction::DealDamage(_)
            | Instruction::AdjustLife(_)
            | Instruction::Discard {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::Sacrifice {
                players: AffectedPlayers::One(_),
                ..
            }
            | Instruction::ZoneOp(_)
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::MutateObject(_)
            | Instruction::ChooseObjects { .. }
    ) || matches!(
        op,
        Instruction::MutatePlayer(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::PlayerMutation::PayCounter { .. }
                    | spellwake_engine::toolbox::PlayerMutation::Mill(_)
                    | spellwake_engine::toolbox::PlayerMutation::Blight { .. }
                    | spellwake_engine::toolbox::PlayerMutation::CollectEvidence { .. }
            )
    ) || matches!(
        op,
        Instruction::StackOp(spec)
            if matches!(
                spec.target,
                spellwake_engine::toolbox::StackTargetSelector::Matching { .. }
            )
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
    Prompt(spellwake_engine::toolbox::PromptDomain),
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
    if let Some(rest) = cleaned_choice.strip_prefix("a color other than ") {
        let mut excluded = spellwake_engine::program::ColorMask::EMPTY;
        for tok in rest.replace(" or ", ",").replace(" and ", ",").split(',') {
            let color = match tok.trim() {
                "white" => Some(spellwake_engine::characteristics::Color::White),
                "blue" => Some(spellwake_engine::characteristics::Color::Blue),
                "black" => Some(spellwake_engine::characteristics::Color::Black),
                "red" => Some(spellwake_engine::characteristics::Color::Red),
                "green" => Some(spellwake_engine::characteristics::Color::Green),
                _ => None,
            };
            if let Some(c) = color {
                excluded = excluded.with(c);
            }
        }
        if !excluded.is_empty() {
            let mut allowed = spellwake_engine::program::ColorMask::EMPTY;
            for c in [
                spellwake_engine::characteristics::Color::White,
                spellwake_engine::characteristics::Color::Blue,
                spellwake_engine::characteristics::Color::Black,
                spellwake_engine::characteristics::Color::Red,
                spellwake_engine::characteristics::Color::Green,
            ] {
                if !excluded.contains(c) {
                    allowed = allowed.with(c);
                }
            }
            if !allowed.is_empty() {
                return Ok(ParsedChoiceKind::Prompt(
                    spellwake_engine::toolbox::PromptDomain::Color { allowed },
                ));
            }
        }
    }
    match cleaned_choice {
        "a color" | "a color of your choice" | "any color" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Color))
        }
        "a card type" | "a card type of your choice" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::CardType(Vec::new()),
        )),
        "a basic land type" | "a land type" | "a basic land type of your choice" => Ok(
            ParsedChoiceKind::Domain(ResolutionChoiceDomain::BasicLandType),
        ),
        "a planeswalker type" | "a planeswalker subtype" => Ok(ParsedChoiceKind::Prompt(
            spellwake_engine::toolbox::PromptDomain::Subtype {
                kind: spellwake_engine::toolbox::SubtypeChoiceKind::Planeswalker,
            },
        )),
        "an opponent" | "an opponent of your choice" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Player {
                opponents_only: true,
            }))
        }
        "a player" | "another player" | "a player of your choice" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Player {
                opponents_only: false,
            }))
        }
        "an opponent at random" | "opponent at random" | "random opponent" => Ok(
            ParsedChoiceKind::Domain(ResolutionChoiceDomain::RandomPlayer {
                opponents_only: true,
            }),
        ),
        "a player at random"
        | "player at random"
        | "another player at random"
        | "random player" => Ok(ParsedChoiceKind::Domain(
            ResolutionChoiceDomain::RandomPlayer {
                opponents_only: false,
            },
        )),
        "a number"
        | "a number of your choice"
        | "any number"
        | "0 or greater"
        | "a number 0 or greater"
        | "a number greater than or equal to 0" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Number {
                min: 0,
                max: 20,
            }))
        }
        "a positive number"
        | "1 or greater"
        | "a number 1 or greater"
        | "a number greater than 0"
        | "a number greater than zero" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Number {
                min: 1,
                max: 20,
            }))
        }
        "a card name" | "a card name of your choice" | "any card name" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::CardName {
                nonland_only: false,
            }))
        }
        "a nonland card name"
        | "a nonland card name of your choice"
        | "a noncreature card name"
        | "a creature card name"
        | "an artifact card name"
        | "an instant or sorcery card name" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::CardName {
                nonland_only: true,
            }))
        }
        "a creature type" | "a creature type of your choice" => {
            Ok(ParsedChoiceKind::CreatureType {
                excluding: BTreeSet::new(),
            })
        }
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

fn format_mana_payment_label(cost: &ManaCost, life: u16) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(g) = cost.generic.to_usize()
        && g > 0
    {
        parts.push(format!("{{{g}}}"));
    }
    for (mt, qty) in &cost.required {
        let sym = match mt {
            ManaType::White => "W",
            ManaType::Blue => "U",
            ManaType::Black => "B",
            ManaType::Red => "R",
            ManaType::Green => "G",
            ManaType::Colourless => "C",
        };
        if let Some(n) = qty.to_usize() {
            for _ in 0..n {
                parts.push(format!("{{{sym}}}"));
            }
        }
    }
    if life > 0 {
        parts.push(format!("{life} life"));
    }
    if parts.is_empty() {
        "Pay {0}".to_string()
    } else {
        format!("Pay {}", parts.join(""))
    }
}

fn lower_hybrid_mana_payment_instruction(payer: ResolvingPlayer, raw: &str) -> Option<Instruction> {
    let (mut base_cost, x_cnt, hybs) = parse_mana_cost_full(raw)?;
    if x_cnt != 0 || hybs.is_empty() || hybs.len() > 4 {
        return None;
    }
    base_cost.required.retain(|_, v| v.is_positive());
    let mut variants: Vec<(ManaCost, u16)> = vec![(base_cost, 0)];
    for sym in &hybs {
        let mut next: Vec<(ManaCost, u16)> = Vec::with_capacity(variants.len() * 2);
        for (c, life) in variants {
            let mut alt = c.clone();
            let mut alt_life = life;
            let first = alt.required.get_mut(&sym.first)?;
            if !first.is_positive() {
                return None;
            }
            *first = first.minus(&1.into());
            if !first.is_positive() {
                alt.required.remove(&sym.first);
            }
            match sym.alternative {
                spellwake_engine::mana::HybridAlternative::Mana(other) => {
                    let entry = alt.required.entry(other).or_insert_with(|| 0.into());
                    *entry = entry.plus(&1.into());
                }
                spellwake_engine::mana::HybridAlternative::TwoGeneric => {
                    alt.generic = alt.generic.plus(&2.into());
                }
                spellwake_engine::mana::HybridAlternative::TwoLife => {
                    alt_life = alt_life.saturating_add(2);
                }
            }
            alt.required.retain(|_, v| v.is_positive());
            next.push((c, life));
            next.push((alt, alt_life));
        }
        variants = next;
    }
    let mut dedup: Vec<(ManaCost, u16)> = Vec::new();
    for item in variants {
        if !dedup.contains(&item) {
            dedup.push(item);
        }
    }
    if dedup.len() < 2 {
        return None;
    }
    let options: Vec<ChooseOptionBranch> = dedup
        .into_iter()
        .map(|(cost, life)| {
            let label = format_mana_payment_label(&cost, life);
            let ops = if life == 0 {
                vec![Instruction::OfferDynamicManaPayment {
                    payer,
                    fixed: cost,
                    extra_generic: ValueExpr::Literal(0.into()),
                    paid_destination: 1,
                    declined_destination: 1,
                }]
            } else if cost == ManaCost::free() {
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(payer),
                        ValueExpr::literal(i64::from(life)),
                    ),
                ]
            } else {
                vec![
                    Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: cost,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: 1,
                        declined_destination: 2,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(payer),
                        ValueExpr::literal(i64::from(life)),
                    ),
                ]
            };
            ChooseOptionBranch {
                label,
                body: ExecutableBody::new(BodyTarget::None, ops),
            }
        })
        .collect();
    Some(Instruction::ChooseOption {
        chooser: payer,
        options,
    })
}

fn lower_extra_pay_cost_instruction(
    payer: ResolvingPlayer,
    cost: &CostAtomAst,
    base_idx: usize,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Result<Option<Instruction>, &'static str> {
    match cost {
        CostAtomAst::PayManaChoice(opts) if opts.len() >= 2 && opts.len() <= 8 => {
            let mut branches = Vec::with_capacity(opts.len());
            for opt_raw in opts {
                let Some(mc) = parse_fixed_mana_cost(opt_raw) else {
                    return Ok(None);
                };
                let label = format_mana_payment_label(&mc, 0);
                branches.push(ChooseOptionBranch {
                    label,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::OfferDynamicManaPayment {
                            payer,
                            fixed: mc,
                            extra_generic: ValueExpr::Literal(0.into()),
                            paid_destination: 1,
                            declined_destination: 1,
                        }],
                    ),
                });
            }
            Ok(Some(Instruction::ChooseOption {
                chooser: payer,
                options: branches,
            }))
        }
        CostAtomAst::PayGenericManaExpr(expr) => {
            let extra_generic = lower_value_expr_in_context(
                resolve_x(expr, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            Ok(Some(Instruction::OfferDynamicManaPayment {
                payer,
                fixed: ManaCost::free(),
                extra_generic,
                paid_destination: base_idx + 1,
                declined_destination: base_idx + 1,
            }))
        }
        CostAtomAst::PayEnergy(n) if *n > 0 => Ok(Some(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_player_counter(
                spellwake_engine::toolbox::PlayerSelector::Resolving(payer),
                CounterKind::Energy,
                ValueExpr::literal(i64::from(*n)),
            ),
        )),
        CostAtomAst::PayEnergyExpr(expr) => {
            let amount = lower_value_expr_in_context(
                resolve_x(expr, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            Ok(Some(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_player_counter(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(payer),
                    CounterKind::Energy,
                    amount,
                ),
            ))
        }
        _ => Ok(None),
    }
}

pub(in super::super) fn emit_choice_for_player_ref(
    chooser: PlayerRefAst,
    kind: ParsedChoiceKind,
    builder: &mut BodyBuilder,
    ids: &mut LoweringIds,
    in_trigger: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::program::ResolutionChoiceDomain;
    const PROMPT_CHOICE_BIND: spellwake_engine::toolbox::BindingId =
        spellwake_engine::toolbox::BindingId(928);
    const PROMPT_SEAT_BIND: spellwake_engine::toolbox::BindingId =
        spellwake_engine::toolbox::BindingId(929);
    match chooser {
        PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer => {
            let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
            match kind {
                ParsedChoiceKind::Domain(domain) => {
                    if let Some(options) = choice_domain_to_player_option_branches(&domain) {
                        builder.instructions.push(Instruction::ForEachPlayerQuery {
                            selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                                players,
                            )),
                            bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::ChooseOption {
                                    chooser: ResolvingPlayer::LoopPlayer,
                                    options,
                                }],
                            )),
                        });
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
                ParsedChoiceKind::CreatureType { .. } => {
                    builder.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                            players,
                        )),
                        bind_player: PROMPT_SEAT_BIND,
                        body: Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::PromptChoice(Box::new(
                                spellwake_engine::toolbox::PromptChoiceSpec {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Bound(
                                        PROMPT_SEAT_BIND,
                                    ),
                                    domain: spellwake_engine::toolbox::PromptDomain::Subtype {
                                        kind:
                                            spellwake_engine::toolbox::SubtypeChoiceKind::Creature,
                                    },
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    bind_to: PROMPT_CHOICE_BIND,
                                    bind_unchosen: None,
                                    tag_partition: None,
                                    auto_resolve_trivial: false,
                                },
                            ))],
                        )),
                    });
                    Ok(())
                }
                ParsedChoiceKind::Prompt(domain) => {
                    builder.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                            players,
                        )),
                        bind_player: PROMPT_SEAT_BIND,
                        body: Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::PromptChoice(Box::new(
                                spellwake_engine::toolbox::PromptChoiceSpec {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Bound(
                                        PROMPT_SEAT_BIND,
                                    ),
                                    domain,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    bind_to: PROMPT_CHOICE_BIND,
                                    bind_unchosen: None,
                                    tag_partition: None,
                                    auto_resolve_trivial: false,
                                },
                            ))],
                        )),
                    });
                    Ok(())
                }
            }
        }
        PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer => {
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: ResolutionChoiceDomain::Player {
                    opponents_only: chooser == PlayerRefAst::AnyOpponent,
                },
            });
            match kind {
                ParsedChoiceKind::Domain(domain) => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser: ResolvingPlayer::ChosenPlayer,
                        domain,
                    });
                    Ok(())
                }
                ParsedChoiceKind::CreatureType { .. } => {
                    builder
                        .instructions
                        .push(Instruction::PromptChoice(Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::ChosenPlayer,
                                ),
                                domain: spellwake_engine::toolbox::PromptDomain::Subtype {
                                    kind: spellwake_engine::toolbox::SubtypeChoiceKind::Creature,
                                },
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                bind_to: PROMPT_CHOICE_BIND,
                                bind_unchosen: None,
                                tag_partition: None,
                                auto_resolve_trivial: false,
                            },
                        )));
                    Ok(())
                }
                ParsedChoiceKind::Prompt(domain) => {
                    builder
                        .instructions
                        .push(Instruction::PromptChoice(Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::ChosenPlayer,
                                ),
                                domain,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                bind_to: PROMPT_CHOICE_BIND,
                                bind_unchosen: None,
                                tag_partition: None,
                                auto_resolve_trivial: false,
                            },
                        )));
                    Ok(())
                }
            }
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
                ParsedChoiceKind::CreatureType { .. } => {
                    builder
                        .instructions
                        .push(Instruction::PromptChoice(Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                domain: spellwake_engine::toolbox::PromptDomain::Subtype {
                                    kind: spellwake_engine::toolbox::SubtypeChoiceKind::Creature,
                                },
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                bind_to: PROMPT_CHOICE_BIND,
                                bind_unchosen: None,
                                tag_partition: None,
                                auto_resolve_trivial: false,
                            },
                        )));
                    Ok(())
                }
                ParsedChoiceKind::Prompt(domain) => {
                    builder
                        .instructions
                        .push(Instruction::PromptChoice(Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                domain,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                bind_to: PROMPT_CHOICE_BIND,
                                bind_unchosen: None,
                                tag_partition: None,
                                auto_resolve_trivial: false,
                            },
                        )));
                    Ok(())
                }
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

fn filter_ast_references_that_player(f: &ObjectFilterAst) -> bool {
    f.controller == Some(ControllerRelation::ThatPlayer)
        || f.owner == Some(ControllerRelation::ThatPlayer)
        || matches!(
            f.zone,
            Some(
                ZoneFilter::ThatPlayerHand
                    | ZoneFilter::ThatPlayerGraveyard
                    | ZoneFilter::ThatPlayerLibrary
            )
        )
}

fn value_expr_ast_references_that_player(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::Count(f)
        | ValueExprAst::AggregateStat { filter: f, .. }
        | ValueExprAst::ColorsAmong(f)
        | ValueExprAst::CardTypesAmong(f)
        | ValueExprAst::SubtypesAmong(f)
        | ValueExprAst::ExtremeStatAmong { filter: f, .. }
        | ValueExprAst::DistinctCountAmong { filter: f, .. }
        | ValueExprAst::MaxSharedSubtypeCount { filter: f } => filter_ast_references_that_player(f),
        ValueExprAst::PlayerStat { player, .. }
        | ValueExprAst::PlayerCounterCount { player, .. }
        | ValueExprAst::StartingLifeTotal(player)
        | ValueExprAst::LifeChangedThisTurn { player, .. } => *player == PlayerRefAst::ThatPlayer,
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Pow { base: a, exp: b }
        | ValueExprAst::Shortfall {
            threshold: a,
            actual: b,
        } => value_expr_ast_references_that_player(a) || value_expr_ast_references_that_player(b),
        ValueExprAst::DivConst { expr: inner, .. } => value_expr_ast_references_that_player(inner),
        _ => false,
    }
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
    builder.up_to_x_target_count |= probed.up_to_x_target_count;
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
        InstructionAst::RevealRandomFromHand { player } => InstructionAst::RevealRandomFromHand {
            player: map_p(*player),
        },
        InstructionAst::ManifestByPlayer {
            player,
            count,
            dread,
            subject,
        } => InstructionAst::ManifestByPlayer {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
            dread: *dread,
            subject: subject.as_deref().map(|s| Box::new(map_subj(s))),
        },
        InstructionAst::CloakByPlayer {
            player,
            count,
            subject,
        } => InstructionAst::CloakByPlayer {
            player: map_p(*player),
            count: Box::new(map_ve(count)),
            subject: subject.as_deref().map(|s| Box::new(map_subj(s))),
        },
        InstructionAst::SacrificeAllExcept {
            player,
            filter,
            keep,
        } => InstructionAst::SacrificeAllExcept {
            player: map_p(*player),
            filter: Box::new(map_flt(filter)),
            keep: Box::new(map_ve(keep)),
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
            Instruction::Discard { players, amount }
            | Instruction::Sacrifice {
                players, amount, ..
            } => {
                rewrite_chosen_to_loop_ap(players);
                rewrite_chosen_to_loop_ve(amount);
            }
            Instruction::AdjustLife(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.players,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                match &mut spec.op {
                    spellwake_engine::toolbox::LifeOp::Gain(amount)
                    | spellwake_engine::toolbox::LifeOp::Lose(amount)
                    | spellwake_engine::toolbox::LifeOp::Set(amount)
                    | spellwake_engine::toolbox::LifeOp::Pay(amount) => {
                        rewrite_chosen_to_loop_ve(amount);
                    }
                    spellwake_engine::toolbox::LifeOp::Drain {
                        amount, gain_to, ..
                    } => {
                        rewrite_chosen_to_loop_ve(amount);
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            gain_to,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::LifeOp::ExchangeTotals { other } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            other,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    _ => {}
                }
            }
            Instruction::DealDamage(spec) => {
                if let spellwake_engine::toolbox::DamageSourceQuery::Objects(q) = &mut spec.source {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                        q,
                        &rewrite_chosen_to_loop_rp,
                        &rewrite_chosen_to_loop_ap,
                        &rewrite_chosen_to_loop_ve,
                    );
                }
                match &mut spec.recipients {
                    spellwake_engine::toolbox::DamageTargetQuery::Objects(q) => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            q,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Players(p) => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            p,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Both { objects, players } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            objects,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            players,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(rel) => {
                        if let DamageRecipientsRef::Group(g) = rel.as_mut() {
                            rewrite_chosen_to_loop_dg(g);
                        }
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Divided { total, .. } => {
                        rewrite_chosen_to_loop_ve(total);
                    }
                    _ => {}
                }
                if let DamageAmountSpec::Expr(e) = &mut spec.amount {
                    rewrite_chosen_to_loop_ve(e);
                }
            }
            Instruction::MutatePlayer(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.players,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                match &mut spec.op {
                    spellwake_engine::toolbox::PlayerMutation::AddCounter { amount, .. }
                    | spellwake_engine::toolbox::PlayerMutation::PayCounter { amount, .. }
                    | spellwake_engine::toolbox::PlayerMutation::Scry(amount)
                    | spellwake_engine::toolbox::PlayerMutation::Surveil(amount)
                    | spellwake_engine::toolbox::PlayerMutation::Mill(amount)
                    | spellwake_engine::toolbox::PlayerMutation::ManifestTop { amount }
                    | spellwake_engine::toolbox::PlayerMutation::Incubate { count: amount }
                    | spellwake_engine::toolbox::PlayerMutation::Blight { amount }
                    | spellwake_engine::toolbox::PlayerMutation::CollectEvidence {
                        minimum: amount,
                    }
                    | spellwake_engine::toolbox::PlayerMutation::RevealTopTwoPiles {
                        count: amount,
                        ..
                    }
                    | spellwake_engine::toolbox::PlayerMutation::RollDie {
                        count: amount, ..
                    }
                    | spellwake_engine::toolbox::PlayerMutation::BindResolutionX(amount) => {
                        rewrite_chosen_to_loop_ve(amount);
                    }
                    spellwake_engine::toolbox::PlayerMutation::AddMana(yield_kind) => {
                        rewrite_chosen_to_loop_mana_yield(yield_kind);
                    }
                    spellwake_engine::toolbox::PlayerMutation::OfferAnyPlayer {
                        cost_body,
                        paid_body,
                        declined_body,
                    } => {
                        rewrite_chosen_to_loop_ops(&mut cost_body.instructions);
                        rewrite_chosen_to_loop_ops(&mut paid_body.instructions);
                        if let Some(dec) = declined_body {
                            rewrite_chosen_to_loop_ops(&mut dec.instructions);
                        }
                    }
                    spellwake_engine::toolbox::PlayerMutation::FlipCoins {
                        win_body,
                        lose_body,
                        ..
                    } => {
                        if let Some(wb) = win_body {
                            rewrite_chosen_to_loop_ops(&mut wb.instructions);
                        }
                        if let Some(lb) = lose_body {
                            rewrite_chosen_to_loop_ops(&mut lb.instructions);
                        }
                    }
                    _ => {}
                }
            }
            Instruction::ExileTopPlay {
                count,
                library_owner,
                ..
            } => {
                rewrite_chosen_to_loop_ve(count);
                rewrite_chosen_to_loop_ap(library_owner);
            }
            Instruction::ZoneOp(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_zone_op(
                    spec,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
            }
            Instruction::ForEachPlayerQuery { selector, body, .. } => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    selector,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                rewrite_chosen_to_loop_ops(&mut body.instructions);
            }
            Instruction::ForEach { query, body, .. } => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                    query,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                rewrite_chosen_to_loop_ops(&mut body.instructions);
            }
            Instruction::PromptChoice(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.chooser,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                match &mut spec.domain {
                    spellwake_engine::toolbox::PromptDomain::Objects {
                        query, min, max, ..
                    } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            query,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                        rewrite_chosen_to_loop_ve(min);
                        rewrite_chosen_to_loop_ve(max);
                    }
                    spellwake_engine::toolbox::PromptDomain::Players { selector, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            selector,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::PromptDomain::Number { min, max } => {
                        rewrite_chosen_to_loop_ve(min);
                        rewrite_chosen_to_loop_ve(max);
                    }
                    spellwake_engine::toolbox::PromptDomain::Partition { query, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            query,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    _ => {}
                }
            }
            Instruction::SetObjectTag { query, .. } => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                    query,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
            }
            Instruction::DrawCards(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.players,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                rewrite_chosen_to_loop_ve(&mut spec.amount);
            }
            Instruction::MutateObject(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                    &mut spec.source,
                    &rewrite_chosen_to_loop_rp,
                    &rewrite_chosen_to_loop_ap,
                    &rewrite_chosen_to_loop_ve,
                );
                match &mut spec.selection {
                    spellwake_engine::toolbox::ZoneSelection::Random { count, chooser } => {
                        rewrite_chosen_to_loop_ve(count);
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            chooser,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::ZoneSelection::Prompt {
                        chooser, min, max, ..
                    } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            chooser,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                        rewrite_chosen_to_loop_ve(min);
                        rewrite_chosen_to_loop_ve(max);
                    }
                    spellwake_engine::toolbox::ZoneSelection::All => {}
                }
                match &mut spec.op {
                    spellwake_engine::toolbox::ObjectMutation::Goad { by } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            by,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::ObjectMutation::Attach { host, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            host,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::ObjectMutation::AttachToPlayer { player } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            player,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::ObjectMutation::AddCounters { amount, .. }
                    | spellwake_engine::toolbox::ObjectMutation::RemoveCounters {
                        amount, ..
                    }
                    | spellwake_engine::toolbox::ObjectMutation::BecomeMonstrous { amount }
                    | spellwake_engine::toolbox::ObjectMutation::Earthbend { amount }
                    | spellwake_engine::toolbox::ObjectMutation::Connive { count: amount } => {
                        rewrite_chosen_to_loop_ve(amount);
                    }
                    spellwake_engine::toolbox::ObjectMutation::CreateTokenCopy(copy) => {
                        rewrite_chosen_to_loop_ve(&mut copy.count);
                        if let TokenRecipients::Player(rp) = &mut copy.recipients {
                            rewrite_chosen_to_loop_rp(rp);
                        }
                        if let spellwake_engine::token::AttackingDefenderScope::Player(rp)
                        | spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(
                            rp,
                        ) = &mut copy.defender_scope
                        {
                            rewrite_chosen_to_loop_rp(rp);
                        }
                    }
                    spellwake_engine::toolbox::ObjectMutation::MoveCounters {
                        to, amount, ..
                    } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            to,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                        if let Some(amt) = amount {
                            rewrite_chosen_to_loop_ve(amt);
                        }
                    }
                    spellwake_engine::toolbox::ObjectMutation::GainControl {
                        controller, ..
                    } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            controller,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    _ => {}
                }
            }
            Instruction::ApplyContinuous(spec) => {
                if let Some(sel) = &mut spec.selector {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                        sel,
                        &rewrite_chosen_to_loop_rp,
                        &rewrite_chosen_to_loop_ap,
                        &rewrite_chosen_to_loop_ve,
                    );
                }
                match &mut spec.payload {
                    spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(rule) => {
                        rewrite_chosen_to_loop_rule(rule);
                    }
                    spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                        power,
                        toughness,
                        ..
                    } => {
                        rewrite_chosen_to_loop_ve(power);
                        rewrite_chosen_to_loop_ve(toughness);
                    }
                    spellwake_engine::toolbox::ContinuousPayload::PlayPermission {
                        player, ..
                    }
                    | spellwake_engine::toolbox::ContinuousPayload::SkipStep { player, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            player,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                    }
                    spellwake_engine::toolbox::ContinuousPayload::SkipTurns { player, count } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            player,
                            &rewrite_chosen_to_loop_rp,
                            &rewrite_chosen_to_loop_ap,
                            &rewrite_chosen_to_loop_ve,
                        );
                        rewrite_chosen_to_loop_ve(count);
                    }
                    _ => {}
                }
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

            Instruction::OfferDynamicManaPayment {
                payer,
                extra_generic,
                ..
            } => {
                rewrite_chosen_to_loop_rp(payer);
                rewrite_chosen_to_loop_ve(extra_generic);
            }
            Instruction::OptionalSequence { chooser, .. }
            | Instruction::ChooseValue { chooser, .. } => {
                rewrite_chosen_to_loop_rp(chooser);
            }
            Instruction::ChooseOption { chooser, options } => {
                rewrite_chosen_to_loop_rp(chooser);
                for br in options {
                    rewrite_chosen_to_loop_ops(&mut br.body.instructions);
                }
            }
            Instruction::CreateTokens { creations } => {
                for c in creations {
                    rewrite_chosen_to_loop_ve(&mut c.amount);
                    if let TokenRecipients::Player(rp) = &mut c.recipients {
                        rewrite_chosen_to_loop_rp(rp);
                    }
                    if let Some(
                        spellwake_engine::token::AttackingDefenderScope::Player(rp)
                        | spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(
                            rp,
                        ),
                    ) = &mut c.attacking_defender
                    {
                        rewrite_chosen_to_loop_rp(rp);
                    }
                }
            }

            Instruction::BranchCondition { condition, .. } => {
                if let spellwake_engine::toolbox::ConditionExpr::Resolution(rc) = condition.as_mut()
                {
                    rewrite_chosen_to_loop_cond(rc);
                }
            }
            Instruction::RepeatLoop { count, body } => {
                rewrite_chosen_to_loop_ve(count);
                rewrite_chosen_to_loop_ops(&mut body.instructions);
            }

            Instruction::SearchLibrary { filter } => {
                rewrite_chosen_to_loop_rp(&mut filter.searcher);
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn lower_loop_player_sub_body(
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
            }
        ) || matches!(
            op,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::PlayerMutation::SelectExtremeLife { .. }
                )
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
    outer_builder.up_to_x_target_count |= sub.up_to_x_target_count;
    Ok(ExecutableBody::new(BodyTarget::None, sub.instructions))
}

fn zone_filter_to_query_zone_in_context(
    zone: Option<ZoneFilter>,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<(spellwake_engine::toolbox::QueryZone, bool), &'static str> {
    use spellwake_engine::toolbox::{PlayerSelector, QueryZone};
    match zone {
        None | Some(ZoneFilter::Battlefield) => Ok((QueryZone::Battlefield, false)),
        Some(ZoneFilter::YourHand) => Ok((
            QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            true,
        )),
        Some(ZoneFilter::ThatPlayerHand) => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok((
                QueryZone::Hand(Box::new(PlayerSelector::Resolving(rp))),
                true,
            ))
        }
        Some(ZoneFilter::YourGraveyard) => Ok((
            QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            false,
        )),
        Some(
            ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
        ) => Ok((
            QueryZone::Graveyard(Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer))),
            false,
        )),
        Some(ZoneFilter::OpponentGraveyard) => Ok((
            QueryZone::Graveyard(Box::new(PlayerSelector::Group(
                AffectedPlayers::EachOpponent,
            ))),
            false,
        )),
        Some(ZoneFilter::ThatPlayerGraveyard) => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok((
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(rp))),
                false,
            ))
        }
        Some(ZoneFilter::Exile) => Ok((QueryZone::Exile, false)),
        Some(ZoneFilter::YourLibrary) => Ok((
            QueryZone::Library(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            true,
        )),
        Some(ZoneFilter::ThatPlayerLibrary) => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok((
                QueryZone::Library(Box::new(PlayerSelector::Resolving(rp))),
                true,
            ))
        }
        _ => Err("oracle:unsupported-zone"),
    }
}

fn lower_exile_with_counters_subject(
    subject: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<
    (
        spellwake_engine::toolbox::ObjectQuery,
        spellwake_engine::toolbox::ZoneSelection,
    ),
    &'static str,
> {
    use spellwake_engine::toolbox::{ChoiceCategory, ObjectQuery, PlayerSelector, ZoneSelection};
    if is_self_pronoun(subject, builder, self_pronoun_valid) {
        return Ok((ObjectQuery::Source, ZoneSelection::All));
    }
    match subject {
        SubjectRefAst::TargetObject {
            filter,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } => {
            let bt = if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.is_ability
                && !filter.any_spell
            {
                let mut clean = filter.as_ref().clone();
                clean.zone = None;
                lower_battlefield_body_target_in_context(&clean, builder, in_trigger, vocabulary)?
            } else {
                lower_target_subject(
                    &SubjectRefAst::TargetObject {
                        filter: filter.clone(),
                        count: TargetCountAst::Exact(1),
                    },
                    vocabulary,
                )?
            };
            builder.bind_target(bt)?;
            if *count == TargetCountAst::UpTo(1) && is_permanent_body_target(bt) {
                builder.up_to_one_target = true;
            }
            Ok((ObjectQuery::Targets, ZoneSelection::All))
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } => {
            let bt = lower_target_subject(
                &SubjectRefAst::TargetOneOf {
                    options: options.clone(),
                    count: TargetCountAst::Exact(1),
                },
                vocabulary,
            )?;
            builder.bind_target(bt)?;
            if *count == TargetCountAst::UpTo(1) && is_permanent_body_target(bt) {
                builder.up_to_one_target = true;
            }
            Ok((ObjectQuery::Targets, ZoneSelection::All))
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            Ok((ObjectQuery::TriggerSubject, ZoneSelection::All))
        }
        SubjectRefAst::AttachedHost(_)
            if in_trigger || self_pronoun_valid || builder.has_source =>
        {
            Ok((ObjectQuery::AttachedHost, ZoneSelection::All))
        }
        SubjectRefAst::Anaphora(_) => {
            if is_permanent_body_target(builder.target)
                || is_graveyard_body_target(builder.target)
                || is_stack_body_target(builder.target)
            {
                Ok((ObjectQuery::Targets, ZoneSelection::All))
            } else if last_instruction_moved_object(&builder.instructions) {
                Ok((ObjectQuery::LastMoved, ZoneSelection::All))
            } else if last_instruction_created_tokens(&builder.instructions) {
                Ok((ObjectQuery::CreatedTokens, ZoneSelection::All))
            } else if in_trigger {
                Ok((ObjectQuery::TriggerSubject, ZoneSelection::All))
            } else if self_pronoun_valid || builder.has_source {
                Ok((ObjectQuery::Source, ZoneSelection::All))
            } else {
                Err("oracle:unsupported-exile-subject")
            }
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) if !f.is_ability && !f.any_spell => {
            let (qz, _) = zone_filter_to_query_zone_in_context(f.zone, builder, in_trigger)?;
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&clean, builder, in_trigger, vocabulary)?
            };
            bind_filter_player_target(builder, &obj)?;
            Ok((ObjectQuery::zone(qz, obj), ZoneSelection::All))
        }
        SubjectRefAst::Group(f) if !f.is_ability && !f.any_spell => {
            let (qz, hidden) = zone_filter_to_query_zone_in_context(f.zone, builder, in_trigger)?;
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&clean, builder, in_trigger, vocabulary)?
            };
            bind_filter_player_target(builder, &obj)?;
            Ok((
                ObjectQuery::zone(qz, obj),
                ZoneSelection::Prompt {
                    chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    min: Box::new(ValueExpr::literal(1)),
                    max: Box::new(ValueExpr::literal(1)),
                    constraint: None,
                    category: ChoiceCategory::Generic,
                    hidden,
                },
            ))
        }
        _ => Err("oracle:unsupported-exile-subject"),
    }
}

fn lower_put_sticker_instruction(
    kind: &str,
    target: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{
        BindingId, ChoiceCategory, ObjectQuery, PlayerSelector, PromptChoiceSpec, PromptDomain,
        QueryZone, TagKey, TagValue, ZoneDest, ZoneOpSpec, ZoneSelection,
    };
    const STICKER_PICK_BIND: BindingId = BindingId(921);
    let tag_key = TagKey::Label("sticker".to_owned());
    let kind_clean = kind.trim().to_ascii_lowercase();
    let tag_val = if kind_clean.is_empty() || kind_clean == "any" {
        TagValue::Flag(true)
    } else {
        TagValue::Text(kind_clean)
    };
    let eff = unwrap_in_zone_subject(target).unwrap_or_else(|| target.clone());
    let emit_tag_query = |builder: &mut BodyBuilder, q: ObjectQuery| {
        if q.uses_target() {
            let mut spec = ZoneOpSpec::move_all(q, ZoneDest::None);
            spec.tag_moved = Some((tag_key.clone(), tag_val.clone()));
            builder
                .instructions
                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
        } else {
            builder.instructions.push(Instruction::SetObjectTag {
                query: Box::new(q),
                key: tag_key.clone(),
                value: Some(tag_val.clone()),
            });
        }
    };
    if is_self_pronoun(&eff, builder, self_pronoun_valid) {
        emit_tag_query(builder, ObjectQuery::Source);
        return Ok(());
    }
    match &eff {
        SubjectRefAst::TargetObject {
            filter,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            && !filter.is_ability
            && !filter.any_spell =>
        {
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            let bt =
                lower_battlefield_body_target_in_context(&clean, builder, in_trigger, vocabulary)?;
            builder.bind_target(bt)?;
            if *count == TargetCountAst::UpTo(1) && is_permanent_body_target(bt) {
                builder.up_to_one_target = true;
            }
            emit_tag_query(builder, ObjectQuery::Targets);
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } => {
            let bt = lower_target_subject(
                &SubjectRefAst::TargetOneOf {
                    options: options.clone(),
                    count: TargetCountAst::Exact(1),
                },
                vocabulary,
            )?;
            if !is_permanent_body_target(bt) {
                return Err("oracle:unsupported-sticker-target");
            }
            builder.bind_target(bt)?;
            if *count == TargetCountAst::UpTo(1) {
                builder.up_to_one_target = true;
            }
            emit_tag_query(builder, ObjectQuery::Targets);
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            emit_tag_query(builder, ObjectQuery::TriggerSubject);
            Ok(())
        }
        SubjectRefAst::AttachedHost(_)
            if in_trigger || self_pronoun_valid || builder.has_source =>
        {
            emit_tag_query(builder, ObjectQuery::AttachedHost);
            Ok(())
        }
        SubjectRefAst::Anaphora(_) => {
            if is_permanent_body_target(builder.target) {
                emit_tag_query(builder, ObjectQuery::Targets);
                Ok(())
            } else if last_instruction_created_tokens(&builder.instructions) {
                emit_tag_query(builder, ObjectQuery::CreatedTokens);
                Ok(())
            } else if last_instruction_moved_object(&builder.instructions) {
                emit_tag_query(builder, ObjectQuery::LastMoved);
                Ok(())
            } else if in_trigger {
                emit_tag_query(builder, ObjectQuery::TriggerSubject);
                Ok(())
            } else if self_pronoun_valid || builder.has_source {
                emit_tag_query(builder, ObjectQuery::Source);
                Ok(())
            } else {
                Err("oracle:unsupported-sticker-target")
            }
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f)
            if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && !f.is_ability
                && !f.any_spell =>
        {
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&clean, builder, in_trigger, vocabulary)?
            };
            bind_filter_player_target(builder, &obj)?;
            emit_tag_query(builder, ObjectQuery::zone(QueryZone::Battlefield, obj));
            Ok(())
        }
        SubjectRefAst::Group(f)
            if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && !f.is_ability
                && !f.any_spell =>
        {
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter_in_context(&clean, builder, in_trigger, vocabulary)?
            };
            bind_filter_player_target(builder, &obj)?;
            let query = ObjectQuery::zone(QueryZone::Battlefield, obj);
            if query.uses_target() {
                let mut spec = ZoneOpSpec::simple(
                    query,
                    ZoneSelection::Prompt {
                        chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                        min: Box::new(ValueExpr::literal(1)),
                        max: Box::new(ValueExpr::literal(1)),
                        constraint: None,
                        category: ChoiceCategory::Generic,
                        hidden: false,
                    },
                    ZoneDest::None,
                );
                spec.tag_moved = Some((tag_key, tag_val));
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spec));
            } else {
                builder
                    .instructions
                    .push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                        chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                        domain: PromptDomain::Objects {
                            query,
                            min: Box::new(ValueExpr::literal(1)),
                            max: Box::new(ValueExpr::literal(1)),
                            constraint: None,
                            hidden: false,
                        },
                        category: ChoiceCategory::Generic,
                        bind_to: STICKER_PICK_BIND,
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial: true,
                    })));
                builder.instructions.push(Instruction::SetObjectTag {
                    query: Box::new(ObjectQuery::Bound(STICKER_PICK_BIND)),
                    key: tag_key,
                    value: Some(tag_val),
                });
            }
            Ok(())
        }
        _ => Err("oracle:unsupported-sticker-target"),
    }
}

fn lower_empower_instruction(
    planeswalker: &str,
    count: &ValueExprAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    if !planeswalker.trim().eq_ignore_ascii_case("jace") {
        return Err("oracle:unsupported-instruction");
    }
    let amt =
        lower_value_expr_in_context(resolve_x(count, bound_x), builder, in_trigger, vocabulary)?;
    let jace_sub =
        spellwake_engine::program::SingleSubtype::Planeswalker(PlaneswalkerSubtype::Jace);
    let jace_list = spellwake_engine::program::SubtypeList::from_slice(&[jace_sub])
        .ok_or("oracle:empower-subtype")?;
    let mut jace_clause = ObjectClause::any();
    jace_clause.subtypes = SubtypeFilter::AnyOfList(jace_list);
    jace_clause.token = Some(true);
    let jace_filter = ObjectFilter::Clause(jace_clause);

    let mut jace_program = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: None,
        }),
    );
    jace_program
        .planeswalker_subtypes
        .insert(PlaneswalkerSubtype::Jace);
    jace_program.effect_defined_colors = Some(BTreeSet::from([Color::Blue]));
    jace_program.printed_loyalty = Some(0.into());

    let surveil_id = ActivatedAbilityId(0);
    jace_program.loyalty_costs.insert(surveil_id, -1);
    jace_program.activated.push(ActivatedAbility {
        id: surveil_id,
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                    ResolvingPlayer::Controller,
                    ValueExpr::literal(1),
                ),
            ))],
        ),
    });

    let draw_id = ActivatedAbilityId(1);
    jace_program.loyalty_costs.insert(draw_id, -3);
    jace_program.activated.push(ActivatedAbility {
        id: draw_id,
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawCards(Box::new(
                spellwake_engine::toolbox::DrawCardsSpec::simple(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    ValueExpr::literal(1),
                ),
            ))],
        ),
    });

    let jace_name =
        EnglishCardName::new("Jace Token".to_string()).map_err(|_| "oracle:empower-token-name")?;
    let jace_def = TokenDefinition::new(jace_program, TokenName::Specified(jace_name), vocabulary)
        .map_err(|_| "oracle:empower-token-invalid")?;

    let skip_create = builder.instructions.len() + 2;
    builder.instructions.push(Instruction::BranchCondition {
        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Not(Box::new(
            spellwake_engine::toolbox::ConditionExpr::NonEmpty(Box::new(
                spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    )),
                    jace_filter,
                ),
            )),
        ))),
        destination: skip_create,
    });
    builder.instructions.push(Instruction::CreateTokens {
        creations: vec![TokenCreation::one(jace_def)],
    });
    builder
        .instructions
        .push(Instruction::MutateObject(Box::new(
            spellwake_engine::toolbox::MutateObjectSpec {
                source: spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    )),
                    jace_filter,
                ),
                selection: spellwake_engine::toolbox::ZoneSelection::Prompt {
                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    min: Box::new(ValueExpr::literal(1)),
                    max: Box::new(ValueExpr::literal(1)),
                    constraint: None,
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    hidden: false,
                },
                op: spellwake_engine::toolbox::ObjectMutation::AddCounters {
                    kind: CounterKind::Loyalty,
                    amount: amt,
                },
                bind_mutated: None,
                bind_succeeded: None,
            },
        )));
    Ok(())
}

fn instruction_produces_card_pool(op: &Instruction) -> bool {
    match op {
        Instruction::ZoneOp(_)
        | Instruction::ForEachPlayerQuery { .. }
        | Instruction::LookTopChooseExt { .. }
        | Instruction::RevealUntilMatchExt { .. }
        | Instruction::ExileTopPlay { .. }
        | Instruction::SearchLibrary { .. }
        | Instruction::ChooseObjects { .. } => true,
        Instruction::MutatePlayer(spec) => {
            matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Mill(_))
        }

        _ => false,
    }
}

fn lower_standalone_choose_and_keep(
    pick: &str,
    keep_to: &str,
    rest_to: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{
        BindingId, ChoiceCategory, GraveyardSemantics, LibraryOrderPos, ObjectQuery,
        PlayerSelector, ZoneController, ZoneDest, ZoneOpSpec, ZoneSelection,
    };
    let has_pool = last_instruction_moved_object(&builder.instructions)
        || builder
            .instructions
            .iter()
            .rev()
            .any(instruction_produces_card_pool);
    if !has_pool {
        return Err("oracle:unsupported-choose-and-keep");
    }
    let keep_low = keep_to.trim().to_ascii_lowercase();
    let keep_dest = if keep_low.contains("hand") {
        ZoneDest::Hand
    } else if keep_low.contains("battlefield") {
        if keep_low.contains("opponent's control") || keep_low.contains("their control") {
            return Err("oracle:unsupported-choose-and-keep");
        }
        let controller =
            if keep_low.contains("owner's control") || keep_low.contains("owners' control") {
                ZoneController::Owner
            } else {
                ZoneController::Player(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                )))
            };
        ZoneDest::Battlefield {
            controller,
            tapped: keep_low.contains("tapped"),
            attacking: keep_low.contains("attacking"),
            transformed: false,
            face_down: keep_low
                .contains("face down")
                .then_some(spellwake_engine::face_down::FaceDownKind::Manifest),
            entering_counters: Vec::new(),
            attach_to: None,
        }
    } else if keep_low.contains("graveyard") {
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Move,
        }
    } else if keep_low.contains("exile") {
        ZoneDest::exile()
    } else if keep_low.contains("top") {
        ZoneDest::Library {
            position: LibraryOrderPos::Top,
        }
    } else if keep_low.contains("bottom") {
        ZoneDest::Library {
            position: if keep_low.contains("random") {
                LibraryOrderPos::BottomRandom
            } else {
                LibraryOrderPos::Bottom
            },
        }
    } else {
        return Err("oracle:unsupported-choose-and-keep");
    };
    let rest_low = rest_to.trim().to_ascii_lowercase();
    let rest_shuffle = rest_low.contains("shuffle");
    let rest_dest: Option<ZoneDest> = if rest_low.contains("bottom") {
        Some(ZoneDest::Library {
            position: if rest_low.contains("random") {
                LibraryOrderPos::BottomRandom
            } else {
                LibraryOrderPos::Bottom
            },
        })
    } else if rest_low.contains("graveyard") {
        Some(ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Move,
        })
    } else if rest_low.contains("exile") {
        Some(ZoneDest::exile())
    } else if rest_low.contains("hand") {
        Some(ZoneDest::Hand)
    } else if rest_low.contains("top") {
        Some(ZoneDest::Library {
            position: LibraryOrderPos::Top,
        })
    } else if rest_low.is_empty() || rest_shuffle {
        None
    } else {
        return Err("oracle:unsupported-choose-and-keep");
    };

    let pick_low = pick.trim().trim_end_matches('.').to_ascii_lowercase();
    let (keep_all, min_expr, max_expr, filter_opt): (
        bool,
        ValueExpr,
        ValueExpr,
        Option<ObjectFilter>,
    ) = if matches!(
        pick_low.as_str(),
        "all"
            | "all of them"
            | "them"
            | "those"
            | "all cards"
            | "all of those cards"
            | "all cards revealed this way"
            | "all cards exiled this way"
            | "those cards"
    ) {
        (true, ValueExpr::literal(0), ValueExpr::literal(0), None)
    } else if matches!(
        pick_low.as_str(),
        "one"
            | "1"
            | "one card"
            | "one of them"
            | "one of those cards"
            | "a card"
            | "that card"
            | "a card from among them"
    ) {
        (false, ValueExpr::literal(1), ValueExpr::literal(1), None)
    } else if matches!(
        pick_low.as_str(),
        "up to one"
            | "up to 1"
            | "up to one of them"
            | "up to one card"
            | "up to one of those cards"
    ) {
        (false, ValueExpr::literal(0), ValueExpr::literal(1), None)
    } else if matches!(
        pick_low.as_str(),
        "two" | "2" | "two cards" | "two of them" | "two of those cards"
    ) {
        (false, ValueExpr::literal(2), ValueExpr::literal(2), None)
    } else if matches!(
        pick_low.as_str(),
        "up to two" | "up to two cards" | "up to two of them" | "up to two of those cards"
    ) {
        (false, ValueExpr::literal(0), ValueExpr::literal(2), None)
    } else if matches!(
        pick_low.as_str(),
        "three" | "3" | "three cards" | "three of them" | "three of those cards"
    ) {
        (false, ValueExpr::literal(3), ValueExpr::literal(3), None)
    } else if matches!(
        pick_low.as_str(),
        "up to three" | "up to three cards" | "up to three of them" | "up to three of those cards"
    ) {
        (false, ValueExpr::literal(0), ValueExpr::literal(3), None)
    } else if matches!(
        pick_low.as_str(),
        "four" | "4" | "four cards" | "four of them" | "four of those cards"
    ) {
        (false, ValueExpr::literal(4), ValueExpr::literal(4), None)
    } else if matches!(
        pick_low.as_str(),
        "up to four" | "up to four cards" | "up to four of them" | "up to four of those cards"
    ) {
        (false, ValueExpr::literal(0), ValueExpr::literal(4), None)
    } else if matches!(
        pick_low.as_str(),
        "five" | "5" | "five cards" | "five of them" | "five of those cards"
    ) {
        (false, ValueExpr::literal(5), ValueExpr::literal(5), None)
    } else if matches!(
        pick_low.as_str(),
        "up to five" | "up to five cards" | "up to five of them" | "up to five of those cards"
    ) {
        (false, ValueExpr::literal(0), ValueExpr::literal(5), None)
    } else if matches!(
        pick_low.as_str(),
        "up to x" | "up to x of them" | "up to x cards"
    ) && let Some(bx) = bound_x
    {
        let x_ve = lower_value_expr_in_context(bx, builder, in_trigger, vocabulary)
            .map_err(|_| "oracle:unsupported-choose-and-keep")?;
        (false, ValueExpr::literal(0), x_ve, None)
    } else if matches!(pick_low.as_str(), "x" | "x of them" | "x cards")
        && let Some(bx) = bound_x
    {
        let x_ve = lower_value_expr_in_context(bx, builder, in_trigger, vocabulary)
            .map_err(|_| "oracle:unsupported-choose-and-keep")?;
        (false, x_ve.clone(), x_ve, None)
    } else {
        let (all_flag, lo_ve, hi_ve, phrase) = if let Some(r) = pick_low
            .strip_prefix("all ")
            .or_else(|| pick_low.strip_prefix("each "))
            .or_else(|| pick_low.strip_prefix("those "))
        {
            (true, ValueExpr::literal(0), ValueExpr::literal(0), r)
        } else if let Some(r) = pick_low.strip_prefix("up to one ") {
            (false, ValueExpr::literal(0), ValueExpr::literal(1), r)
        } else if let Some(r) = pick_low.strip_prefix("up to two ") {
            (false, ValueExpr::literal(0), ValueExpr::literal(2), r)
        } else if let Some(r) = pick_low.strip_prefix("up to three ") {
            (false, ValueExpr::literal(0), ValueExpr::literal(3), r)
        } else if let Some(r) = pick_low.strip_prefix("up to four ") {
            (false, ValueExpr::literal(0), ValueExpr::literal(4), r)
        } else if let Some(r) = pick_low.strip_prefix("up to five ") {
            (false, ValueExpr::literal(0), ValueExpr::literal(5), r)
        } else if let Some(r) = pick_low.strip_prefix("up to x ")
            && let Some(bx) = bound_x
        {
            let x_ve = lower_value_expr_in_context(bx, builder, in_trigger, vocabulary)
                .map_err(|_| "oracle:unsupported-choose-and-keep")?;
            (false, ValueExpr::literal(0), x_ve, r)
        } else if let Some(r) = pick_low.strip_prefix("any number of ") {
            (false, ValueExpr::literal(0), ValueExpr::literal(100), r)
        } else if let Some(r) = pick_low.strip_prefix("two ") {
            (false, ValueExpr::literal(2), ValueExpr::literal(2), r)
        } else if let Some(r) = pick_low.strip_prefix("three ") {
            (false, ValueExpr::literal(3), ValueExpr::literal(3), r)
        } else if let Some(r) = pick_low.strip_prefix("four ") {
            (false, ValueExpr::literal(4), ValueExpr::literal(4), r)
        } else if let Some(r) = pick_low.strip_prefix("five ") {
            (false, ValueExpr::literal(5), ValueExpr::literal(5), r)
        } else if let Some(r) = pick_low.strip_prefix("x ")
            && let Some(bx) = bound_x
        {
            let x_ve = lower_value_expr_in_context(bx, builder, in_trigger, vocabulary)
                .map_err(|_| "oracle:unsupported-choose-and-keep")?;
            (false, x_ve.clone(), x_ve, r)
        } else if let Some(r) = pick_low
            .strip_prefix("one ")
            .or_else(|| pick_low.strip_prefix("a "))
            .or_else(|| pick_low.strip_prefix("an "))
        {
            (false, ValueExpr::literal(1), ValueExpr::literal(1), r)
        } else {
            return Err("oracle:unsupported-choose-and-keep");
        };
        let mut cleaned = phrase.trim();
        loop {
            let next = cleaned
                .trim_end_matches(" revealed this way")
                .trim_end_matches(" exiled this way")
                .trim_end_matches(" looked at this way")
                .trim_end_matches(" from among the revealed cards")
                .trim_end_matches(" from among those cards")
                .trim_end_matches(" from among those")
                .trim_end_matches(" from among them")
                .trim_end_matches(" of them")
                .trim();
            if next.len() == cleaned.len() {
                break;
            }
            cleaned = next;
        }
        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            cleaned,
            &cached_compiler_vocab(vocabulary),
        )
        .map_err(|_| "oracle:unsupported-choose-and-keep")?;
        if parsed.zone.is_some() {
            return Err("oracle:unsupported-choose-and-keep");
        }
        let obj = if is_unrestricted_filter(&parsed) {
            None
        } else {
            let f = lower_object_filter(&parsed, vocabulary)
                .map_err(|_| "oracle:unsupported-choose-and-keep")?;
            if f.has_target_player() || f.uses_target_permanent() {
                return Err("oracle:unsupported-choose-and-keep");
            }
            Some(f)
        };
        (all_flag, lo_ve, hi_ve, obj)
    };

    const POOL_BIND: BindingId = BindingId(927);
    const KEPT_BIND: BindingId = BindingId(925);
    const UNCHOSEN_BIND: BindingId = BindingId(926);

    let mut snap_spec = ZoneOpSpec::move_all(ObjectQuery::LastMoved, ZoneDest::None);
    snap_spec.bind_moved = Some(POOL_BIND);
    builder
        .instructions
        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(snap_spec));

    let keep_source = match filter_opt {
        Some(f) => ObjectQuery::BoundFiltered {
            binding: POOL_BIND,
            filter: Box::new(f),
        },
        None => ObjectQuery::Bound(POOL_BIND),
    };
    let keep_selection = if keep_all {
        ZoneSelection::All
    } else {
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
            min: Box::new(min_expr),
            max: Box::new(max_expr),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: false,
        }
    };
    let mut keep_spec = ZoneOpSpec::simple(keep_source, keep_selection, keep_dest);
    keep_spec.bind_moved = Some(KEPT_BIND);
    keep_spec.bind_unchosen = Some(UNCHOSEN_BIND);
    builder
        .instructions
        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(keep_spec));

    if let Some(rd) = rest_dest {
        let mut rest_spec = ZoneOpSpec::move_all(
            ObjectQuery::Difference(
                Box::new(ObjectQuery::Bound(POOL_BIND)),
                Box::new(ObjectQuery::Bound(KEPT_BIND)),
            ),
            rd,
        );
        if rest_shuffle {
            rest_spec.shuffle_library =
                Some(PlayerSelector::Resolving(ResolvingPlayer::Controller));
        }
        builder
            .instructions
            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(rest_spec));
    } else if rest_shuffle {
        builder
            .instructions
            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
    }
    Ok(())
}

fn is_safe_resolving_player_for_shuffle(
    p: ResolvingPlayer,
    builder: &BodyBuilder,
    in_trigger: bool,
) -> bool {
    let has_source = in_trigger || builder.has_source;
    match p {
        ResolvingPlayer::Controller
        | ResolvingPlayer::ActivePlayer
        | ResolvingPlayer::DefendingPlayer
        | ResolvingPlayer::RandomOpponent => true,
        ResolvingPlayer::EnchantedPlayer
        | ResolvingPlayer::AttachedHostController
        | ResolvingPlayer::SourceOwner
        | ResolvingPlayer::SourceChosenPlayer => has_source,
        ResolvingPlayer::TargetPlayer => matches!(
            builder.target,
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
        ),
        ResolvingPlayer::TargetController => matches!(
            builder.target,
            BodyTarget::SingleOpponent
                | BodyTarget::SinglePlayer
                | BodyTarget::SingleSpell(_)
                | BodyTarget::SingleStackObject(_)
                | BodyTarget::SingleStackEntry
                | BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::FilteredPermanent(_)
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SinglePlayerOrPlaneswalker
                | BodyTarget::SingleOpponentOrPlaneswalker
                | BodyTarget::SingleAnyTarget
        ),
        ResolvingPlayer::TargetOwner => matches!(
            builder.target,
            BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::FilteredPermanent(_)
                | BodyTarget::SingleOwnGraveyardCard
                | BodyTarget::FilteredOwnGraveyardCard(_)
                | BodyTarget::FilteredAnyGraveyardCard { .. }
                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                | BodyTarget::AnyGraveyardCard
                | BodyTarget::AnyGraveyardCreature
                | BodyTarget::SingleSpell(_)
        ),
        ResolvingPlayer::TargetSpellController => matches!(
            builder.target,
            BodyTarget::SingleSpell(_)
                | BodyTarget::SingleStackObject(_)
                | BodyTarget::SingleStackEntry
                | BodyTarget::ControlledAbility { .. }
                | BodyTarget::StackEntryTargetingControlledPermanent
        ),
        ResolvingPlayer::ChosenPlayer => builder.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ChooseValue {
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Player { .. },
                    ..
                }
            ) || matches!(
                op,
                Instruction::MutatePlayer(spec)
                    if matches!(
                        spec.op,
                        spellwake_engine::toolbox::PlayerMutation::SelectExtremeLife { .. }
                    )
            ) || matches!(
                op,
                Instruction::PromptChoice(spec)
                    if matches!(spec.domain, spellwake_engine::toolbox::PromptDomain::Players { .. })
            )
        }),
        ResolvingPlayer::UniqueRelatedPlayer
        | ResolvingPlayer::TriggerSubjectController
        | ResolvingPlayer::TriggerSubjectOwner => in_trigger,
        _ => false,
    }
}

pub(super) fn lower_shuffle_that_player_fallback(
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{ObjectQuery, PlayerSelector, ZoneOpSpec};
    // 1. Prior SearchLibrary(PlayerLibrary(players)) or multi-player loop/offer/choice/draw.
    for op in builder.instructions.iter().rev() {
        match op {
            Instruction::SearchLibrary { filter } => {
                let rp = filter.searcher;
                if is_safe_resolving_player_for_shuffle(rp, builder, in_trigger) {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            ZoneOpSpec::shuffle(PlayerSelector::Resolving(rp)),
                        ),
                    );
                    return Ok(());
                }
            }
            Instruction::DrawCards(spec) => match &spec.players {
                PlayerSelector::Resolving(rp) | PlayerSelector::Group(AffectedPlayers::One(rp))
                    if *rp != ResolvingPlayer::Controller
                        && is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(*rp),
                    );
                    return Ok(());
                }
                PlayerSelector::Group(players) if !matches!(players, AffectedPlayers::One(_)) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            ZoneOpSpec::shuffle(PlayerSelector::Group(*players)),
                        ),
                    );
                    return Ok(());
                }
                _ => {}
            },
            Instruction::ForEachPlayerQuery { selector, .. } => {
                if let PlayerSelector::Group(players) = selector.as_ref() {
                    let ok = match players {
                        AffectedPlayers::One(rp) => {
                            is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger)
                        }
                        _ => true,
                    };
                    if ok {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                ZoneOpSpec::shuffle(PlayerSelector::Group(*players)),
                            ),
                        );
                        return Ok(());
                    }
                }
            }
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::PlayerMutation::OfferAnyPlayer { .. }
                ) =>
            {
                if let PlayerSelector::Group(players) = &spec.players {
                    let ok = match players {
                        AffectedPlayers::One(rp) => {
                            is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger)
                        }
                        _ => true,
                    };
                    if ok {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                ZoneOpSpec::shuffle(PlayerSelector::Group(*players)),
                            ),
                        );
                        return Ok(());
                    }
                }
            }
            Instruction::AdjustLife(spec) => match &spec.players {
                PlayerSelector::Resolving(rp) | PlayerSelector::Group(AffectedPlayers::One(rp))
                    if *rp != ResolvingPlayer::Controller
                        && is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(*rp),
                    );
                    return Ok(());
                }
                PlayerSelector::Group(players) if !matches!(players, AffectedPlayers::One(_)) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            ZoneOpSpec::shuffle(PlayerSelector::Group(*players)),
                        ),
                    );
                    return Ok(());
                }
                _ => {}
            },
            Instruction::MutatePlayer(spec) => match &spec.players {
                PlayerSelector::Resolving(rp) | PlayerSelector::Group(AffectedPlayers::One(rp))
                    if *rp != ResolvingPlayer::Controller
                        && is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(*rp),
                    );
                    return Ok(());
                }
                PlayerSelector::Group(players) if !matches!(players, AffectedPlayers::One(_)) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            ZoneOpSpec::shuffle(PlayerSelector::Group(*players)),
                        ),
                    );
                    return Ok(());
                }
                _ => {}
            },
            Instruction::Discard { players, .. }
            | Instruction::Sacrifice { players, .. }
            | Instruction::ChooseObjects { players, .. } => match players {
                AffectedPlayers::One(rp)
                    if *rp != ResolvingPlayer::Controller
                        && is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(*rp),
                    );
                    return Ok(());
                }
                group if !matches!(group, AffectedPlayers::One(_)) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            ZoneOpSpec::shuffle(PlayerSelector::Group(*group)),
                        ),
                    );
                    return Ok(());
                }
                _ => {}
            },
            Instruction::OfferDynamicManaPayment { payer: player, .. }
            | Instruction::ChooseOption {
                chooser: player, ..
            } if *player != ResolvingPlayer::Controller
                && is_safe_resolving_player_for_shuffle(*player, builder, in_trigger) =>
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(
                        *player,
                    ),
                );
                return Ok(());
            }
            Instruction::PromptChoice(spec) => match &spec.chooser {
                PlayerSelector::Resolving(rp)
                    if *rp != ResolvingPlayer::Controller
                        && is_safe_resolving_player_for_shuffle(*rp, builder, in_trigger) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(*rp),
                    );
                    return Ok(());
                }
                _ => {}
            },
            _ => {}
        }
    }
    // 1b. Bound spell/permanent/graveyard/player target when prior instruction acted on it
    // (e.g. Counter target spell ... Then that player shuffles).
    let target_rp = match builder.target {
        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
            Some(ResolvingPlayer::TargetPlayer)
        }
        BodyTarget::SingleSpell(_)
        | BodyTarget::SingleStackObject(_)
        | BodyTarget::SingleStackEntry
        | BodyTarget::ControlledAbility { .. }
        | BodyTarget::StackEntryTargetingControlledPermanent => {
            Some(ResolvingPlayer::TargetSpellController)
        }
        BodyTarget::SinglePermanent
        | BodyTarget::SingleCreature
        | BodyTarget::SinglePlaneswalker
        | BodyTarget::FilteredPermanent(_) => Some(ResolvingPlayer::TargetController),
        BodyTarget::AnyGraveyardCard
        | BodyTarget::AnyGraveyardCreature
        | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
        | BodyTarget::FilteredAnyGraveyardCard { .. }
        | BodyTarget::FilteredExiledCard(_) => Some(ResolvingPlayer::TargetOwner),
        _ => None,
    };
    if let Some(rp) = target_rp
        && is_safe_resolving_player_for_shuffle(rp, builder, in_trigger)
    {
        builder.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(rp),
        );
        return Ok(());
    }
    // 2. Prior object movement or selection: shuffle the owners of those moved/selected objects.
    let has_prior_objects = last_instruction_moved_object(&builder.instructions)
        || builder.instructions.iter().rev().any(|op| {
            matches!(
                op,
                Instruction::ZoneOp(_)
                    | Instruction::ForEachPlayerQuery { .. }
                    | Instruction::SearchLibrary { .. }
                    | Instruction::LookTopChooseExt { .. }
                    | Instruction::RevealUntilMatchExt { .. }
                    | Instruction::ChooseObjects { .. }
                    | Instruction::MutateObject(_)
            )
        });
    if has_prior_objects {
        builder.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(ZoneOpSpec::shuffle(
                PlayerSelector::OwnersOf(Box::new(ObjectQuery::LastMoved)),
            )),
        );
        return Ok(());
    }
    Err("oracle:unbound-that-player")
}
