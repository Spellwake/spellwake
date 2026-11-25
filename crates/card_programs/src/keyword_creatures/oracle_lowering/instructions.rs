use super::*;
use spellwake_engine::program::{ExchangeSpec, ExchangeZone, StackCopyTarget};
use spellwake_oracle_compiler::ast::ExchangeKind;

pub(super) fn lower_target_subject(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    match subject {
        SubjectRefAst::AnyTarget => Ok(BodyTarget::SingleAnyTarget),
        SubjectRefAst::TargetPlayerOrPlaneswalker {
            opponent_only: false,
        } => Ok(BodyTarget::SinglePlayerOrPlaneswalker),
        SubjectRefAst::TargetPlayerOrPlaneswalker {
            opponent_only: true,
        } => Ok(BodyTarget::SingleOpponentOrPlaneswalker),
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => Ok(if *opponent_only {
            BodyTarget::SingleOpponent
        } else {
            BodyTarget::SinglePlayer
        }),
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } => {
            if let Some(zone) = filter.zone {
                let mut inner = filter.as_ref().clone();
                inner.zone = None;
                return match zone {
                    ZoneFilter::YourGraveyard => {
                        if is_unrestricted_filter(&inner) {
                            Ok(BodyTarget::SingleOwnGraveyardCard)
                        } else {
                            Ok(BodyTarget::FilteredOwnGraveyardCard(lower_object_filter(
                                &inner, vocabulary,
                            )?))
                        }
                    }
                    ZoneFilter::AnyGraveyard => {
                        if is_unrestricted_filter(&inner) {
                            Ok(BodyTarget::AnyGraveyardCard)
                        } else if inner.any_types == AstCardTypeMask::CREATURE
                            && filter_only_has(
                                &inner,
                                FilterAllow {
                                    types: true,
                                    ..FilterAllow::NONE
                                },
                            )
                        {
                            Ok(BodyTarget::AnyGraveyardCreature)
                        } else if inner.any_types
                            == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
                            && filter_only_has(
                                &inner,
                                FilterAllow {
                                    types: true,
                                    ..FilterAllow::NONE
                                },
                            )
                        {
                            Ok(BodyTarget::AnyGraveyardCreatureOrPlaneswalker)
                        } else {
                            Ok(BodyTarget::FilteredAnyGraveyardCard {
                                filter: lower_object_filter(&inner, vocabulary)?,
                                opponents_only: false,
                            })
                        }
                    }
                    ZoneFilter::OpponentGraveyard => {
                        let filter = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        Ok(BodyTarget::FilteredAnyGraveyardCard {
                            filter,
                            opponents_only: true,
                        })
                    }
                    ZoneFilter::Stack => {
                        if inner.is_ability {
                            inner.is_ability = false;
                            inner.any_spell = false;
                            if is_unrestricted_filter(&inner) {
                                Ok(BodyTarget::SingleStackEntry)
                            } else {
                                Ok(BodyTarget::SingleStackObject(lower_spell_filter(
                                    &inner, vocabulary,
                                )?))
                            }
                        } else {
                            Ok(BodyTarget::SingleSpell(lower_spell_filter(
                                &inner, vocabulary,
                            )?))
                        }
                    }
                    ZoneFilter::Battlefield => lower_battlefield_body_target(&inner, vocabulary),
                    _ => Err("oracle:unsupported-target-zone"),
                };
            }
            if filter.is_ability {
                let mut inner = filter.as_ref().clone();
                inner.is_ability = false;
                inner.any_spell = false;
                return if is_unrestricted_filter(&inner) {
                    Ok(BodyTarget::SingleStackEntry)
                } else {
                    Ok(BodyTarget::SingleStackObject(lower_spell_filter(
                        &inner, vocabulary,
                    )?))
                };
            }
            if filter.any_spell {
                let mut inner = filter.as_ref().clone();
                inner.any_spell = false;
                return Ok(BodyTarget::SingleSpell(lower_spell_filter(
                    &inner, vocabulary,
                )?));
            }
            lower_battlefield_body_target(filter, vocabulary)
        }
        _ => Err("oracle:unsupported-target-subject"),
    }
}

pub(super) fn lower_battlefield_body_target(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    if f.any_permanent
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
    {
        return Ok(BodyTarget::SinglePermanent);
    }
    let obj = lower_object_filter(f, vocabulary)?;
    if obj == ObjectFilter::creature() {
        Ok(BodyTarget::SingleCreature)
    } else if obj == ObjectFilter::planeswalker() {
        Ok(BodyTarget::SinglePlaneswalker)
    } else {
        Ok(BodyTarget::FilteredPermanent(obj))
    }
}

pub(super) fn lower_resolving_player(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
) -> Result<ResolvingPlayer, &'static str> {
    lower_resolving_player_in_context(p, builder, true)
}

pub(super) fn lower_resolving_player_in_context(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<ResolvingPlayer, &'static str> {
    match p {
        PlayerRefAst::You => Ok(ResolvingPlayer::Controller),
        PlayerRefAst::TargetPlayer => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::TargetOpponent => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::ThatPlayer => match builder.target {
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                Ok(ResolvingPlayer::TargetPlayer)
            }
            BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetSpellController),
            BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_) => Ok(ResolvingPlayer::TargetController),
            BodyTarget::SingleOwnGraveyardCard | BodyTarget::FilteredOwnGraveyardCard(_) => {
                Ok(ResolvingPlayer::Controller)
            }
            BodyTarget::FilteredAnyGraveyardCard { .. }
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker => Ok(ResolvingPlayer::TargetOwner),
            _ if builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ChooseValue {
                        domain: spellwake_engine::program::ResolutionChoiceDomain::Player { .. },
                        ..
                    }
                )
            }) =>
            {
                Ok(ResolvingPlayer::ChosenPlayer)
            }
            _ if in_trigger => Ok(ResolvingPlayer::UniqueRelatedPlayer),
            _ => Err("oracle:unbound-that-player"),
        },
        PlayerRefAst::ItsController => match builder.target {
            BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetSpellController),
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            _ => Err("oracle:unsupported-its-controller"),
        },
        PlayerRefAst::ItsOwner => match builder.target {
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SingleOwnGraveyardCard
            | BodyTarget::FilteredOwnGraveyardCard(_)
            | BodyTarget::FilteredAnyGraveyardCard { .. }
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetOwner),
            _ => Err("oracle:unsupported-its-owner"),
        },
        PlayerRefAst::ActivePlayer => Ok(ResolvingPlayer::ActivePlayer),
        PlayerRefAst::EnchantedPlayer => Ok(ResolvingPlayer::EnchantedPlayer),
        PlayerRefAst::DefendingPlayer => Ok(ResolvingPlayer::DefendingPlayer),
        PlayerRefAst::ThatObjectController => match builder.target {
            BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetSpellController),
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            _ if in_trigger => Ok(ResolvingPlayer::UniqueRelatedPlayer),
            _ => Err("oracle:unbound-that-player"),
        },
        _ => Err("oracle:unsupported-resolving-player"),
    }
}

// Retained for cross-module signature stability; instructions.rs calls lower_affected_players_in_context.
#[allow(dead_code)]
pub(super) fn lower_affected_players(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
) -> Result<AffectedPlayers, &'static str> {
    lower_affected_players_in_context(p, builder, true)
}

fn lower_affected_players_in_context(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<AffectedPlayers, &'static str> {
    match p {
        PlayerRefAst::EachPlayer => Ok(AffectedPlayers::EachPlayer),
        PlayerRefAst::EachOpponent => Ok(AffectedPlayers::EachOpponent),
        PlayerRefAst::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        other => Ok(AffectedPlayers::One(lower_resolving_player_in_context(
            other, builder, in_trigger,
        )?)),
    }
}

pub(super) fn lower_duration(d: &DurationAst) -> Result<ResolvedDuration, &'static str> {
    match d {
        DurationAst::UntilEndOfCombat => Ok(ResolvedDuration::UntilEndOfCombat),
        DurationAst::UntilEndOfTurn => Ok(ResolvedDuration::UntilEndOfTurn),
        DurationAst::UntilYourNextTurn => Ok(ResolvedDuration::UntilControllerNextTurn),
        DurationAst::UntilEndOfNextTurn => Ok(ResolvedDuration::ThroughControllerNextTurn),
        DurationAst::Permanent | DurationAst::Instant => Ok(ResolvedDuration::NoStatedDuration),
        DurationAst::UntilSourceLeaves => Ok(ResolvedDuration::WhileControllingAbilitySource),
        DurationAst::ForAsLongAs(_) | DurationAst::UntilEndOfPlayersNextTurn(_) => {
            Err("oracle:unsupported-duration")
        }
    }
}

fn bind_instruction_value_expr_targets(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    let bind = |expr: &ValueExprAst, builder: &mut BodyBuilder| {
        bind_value_expr_targets(resolve_x(expr, bound_x), builder)
    };
    match ins {
        InstructionAst::Draw { count, .. }
        | InstructionAst::Mill { count, .. }
        | InstructionAst::Scry { count, .. }
        | InstructionAst::Surveil { count, .. }
        | InstructionAst::GainPlayerCounters { count, .. }
        | InstructionAst::LookTopLibrary { count, .. }
        | InstructionAst::PutCounters { count, .. } => bind(count, builder),
        InstructionAst::RemoveCounters { count: Some(c), .. }
        | InstructionAst::Discard { count: Some(c), .. }
        | InstructionAst::Sacrifice { count: Some(c), .. }
        | InstructionAst::MoveCounted { count: Some(c), .. }
        | InstructionAst::Copy { count: Some(c), .. } => bind(c, builder),
        InstructionAst::ChangeLife { delta, .. } => bind(delta, builder),
        InstructionAst::DealDamage { amount, .. } => bind(amount, builder),
        InstructionAst::ModifyPT { scale: Some(s), .. } => bind(s, builder),
        _ => Ok(()),
    }
}

pub(super) fn lower_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    bind_instruction_value_expr_targets(ins, builder, bound_x)?;
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
        InstructionAst::Draw {
            player,
            count,
            up_to: true,
        } if matches!(resolve_x(count, bound_x), ValueExprAst::Const(1)) => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::OptionalEffect { chooser: rp });
            match rp {
                ResolvingPlayer::Controller => {
                    builder.instructions.push(Instruction::DrawController {
                        amount: ValueExpr::literal(1),
                    });
                }
                ResolvingPlayer::TargetPlayer => {
                    builder.instructions.push(Instruction::DrawTargetPlayer {
                        amount: ValueExpr::literal(1),
                    });
                }
                _ => {
                    builder.instructions.push(Instruction::DrawPlayer {
                        players: AffectedPlayers::One(rp),
                        amount: ValueExpr::literal(1),
                    });
                }
            }
            Ok(())
        }
        InstructionAst::Draw {
            player,
            count,
            up_to: false,
        } => {
            let amount = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            match player {
                PlayerRefAst::You => {
                    builder
                        .instructions
                        .push(Instruction::DrawController { amount });
                    Ok(())
                }
                PlayerRefAst::EachPlayer => {
                    builder
                        .instructions
                        .push(Instruction::DrawEachPlayer { amount });
                    Ok(())
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::DrawTargetPlayer { amount });
                    Ok(())
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::DrawTargetPlayer { amount });
                    Ok(())
                }
                PlayerRefAst::EachOpponent => {
                    builder.instructions.push(Instruction::DrawPlayer {
                        players: AffectedPlayers::EachOpponent,
                        amount,
                    });
                    Ok(())
                }
                PlayerRefAst::EachOtherPlayer => {
                    builder.instructions.push(Instruction::DrawPlayer {
                        players: AffectedPlayers::EachOtherPlayer,
                        amount,
                    });
                    Ok(())
                }
                PlayerRefAst::ThatPlayer => {
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) {
                        builder
                            .instructions
                            .push(Instruction::DrawTargetPlayer { amount });
                        Ok(())
                    } else if let Ok(rp) =
                        lower_resolving_player_in_context(*player, builder, in_trigger)
                        && (rp != ResolvingPlayer::UniqueRelatedPlayer || in_trigger)
                    {
                        builder.instructions.push(Instruction::DrawPlayer {
                            players: AffectedPlayers::One(rp),
                            amount,
                        });
                        Ok(())
                    } else if in_trigger && amount == ValueExpr::literal(1) {
                        builder.instructions.push(Instruction::DrawRelatedPlayer);
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
                | PlayerRefAst::EnchantedPlayer => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    builder.instructions.push(Instruction::DrawPlayer {
                        players: AffectedPlayers::One(rp),
                        amount,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-draw-player"),
            }
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            if !is_self_pronoun(source, builder, self_pronoun_valid) {
                // Non-self creature deals damage equal to its power:
                // 1) "Target creature deals damage equal to its power to ~" -> TargetCreatureToAbilitySource
                // 2) "Target creature deals damage equal to its power to itself" -> TargetCreature
                let is_its_power = matches!(
                    amount.as_ref(),
                    ValueExprAst::ObjectStat {
                        subject,
                        stat: StatKind::Power,
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                );
                let bt_is_creature = |bt: BodyTarget| {
                    matches!(bt, BodyTarget::SingleCreature)
                        || matches!(
                            bt,
                            BodyTarget::FilteredPermanent(ObjectFilter::Clause(c))
                                if c.any_types == CardTypeMask::CREATURE
                        )
                };
                if is_its_power {
                    match source.as_ref() {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } if filter.zone.is_none() => {
                            let bt = lower_battlefield_body_target(filter, vocabulary)?;
                            if bt_is_creature(bt) {
                                if is_self_pronoun(target, builder, self_pronoun_valid) {
                                    builder.bind_target(bt)?;
                                    builder.instructions.push(Instruction::DamageFromPower(
                                        PowerDamageSource::TargetCreatureToAbilitySource,
                                    ));
                                    return Ok(());
                                }
                                if matches!(target.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself"))
                                {
                                    builder.bind_target(bt)?;
                                    builder.instructions.push(Instruction::DamageFromPower(
                                        PowerDamageSource::TargetCreature,
                                    ));
                                    return Ok(());
                                }
                            }
                        }
                        SubjectRefAst::Anaphora(_) if bt_is_creature(builder.target) => {
                            if is_self_pronoun(target, builder, self_pronoun_valid) {
                                builder.instructions.push(Instruction::DamageFromPower(
                                    PowerDamageSource::TargetCreatureToAbilitySource,
                                ));
                                return Ok(());
                            }
                            if matches!(target.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself"))
                            {
                                builder.instructions.push(Instruction::DamageFromPower(
                                    PowerDamageSource::TargetCreature,
                                ));
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }
                let src_ref = match source.as_ref() {
                    SubjectRefAst::SelfObject => Some(DamageSourceRef::AbilitySource),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(DamageSourceRef::TriggeringObject)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some(DamageSourceRef::TriggeringObject)
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        Some(DamageSourceRef::AttachedHost)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        Some(DamageSourceRef::Matching(obj))
                    }
                    _ => None,
                };
                if let Some(src) = src_ref {
                    let is_its_toughness = matches!(
                        amount.as_ref(),
                        ValueExprAst::ObjectStat {
                            subject,
                            stat: StatKind::Toughness,
                        } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                    );
                    let amt_spec = if is_its_power {
                        DamageAmountSpec::SourcePower
                    } else if is_its_toughness {
                        DamageAmountSpec::SourceToughness
                    } else {
                        DamageAmountSpec::Expr(lower_value_expr(
                            resolve_x(amount, bound_x),
                            vocabulary,
                        )?)
                    };
                    let rec_ref = match target.as_ref() {
                        SubjectRefAst::AnyTarget
                        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                        | SubjectRefAst::TargetPlayer { .. }
                        | SubjectRefAst::TargetObject { .. } => {
                            let bt = lower_target_subject(target, vocabulary)?;
                            builder.bind_target(bt)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                            builder.bind_target(BodyTarget::SinglePlayer)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                            builder.bind_target(BodyTarget::SingleOpponent)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself") => {
                            match &src {
                                DamageSourceRef::TargetObject => Some(DamageRecipientsRef::Target),
                                DamageSourceRef::TriggeringObject => {
                                    Some(DamageRecipientsRef::TriggeringObject)
                                }
                                DamageSourceRef::AttachedHost => {
                                    Some(DamageRecipientsRef::AttachedHost)
                                }
                                _ => None,
                            }
                        }
                        SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                            Some(DamageRecipientsRef::Target)
                        }
                        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                            Some(DamageRecipientsRef::AbilitySource)
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some(DamageRecipientsRef::TriggeringObject)
                        }
                        SubjectRefAst::Anaphora(raw)
                            if in_trigger
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                        {
                            Some(DamageRecipientsRef::TriggeringObject)
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                            Some(DamageRecipientsRef::AttachedHost)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController,
                        ) if builder.target != BodyTarget::None => {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Player(PlayerRefAst::You) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::Controller))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachOpponent) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::Opponents))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachPlayer) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::Players))
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ThatPlayer
                            | PlayerRefAst::DefendingPlayer
                            | PlayerRefAst::EnchantedPlayer
                            | PlayerRefAst::ItsController
                            | PlayerRefAst::ThatObjectController,
                        ) => Some(DamageRecipientsRef::Group(DamageGroup::RelatedPlayer)),
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                            let group = lower_damage_group(f, vocabulary)?;
                            if let DamageGroup::Filtered(obj) = group {
                                bind_filter_player_target(builder, &obj)?;
                            }
                            Some(DamageRecipientsRef::Group(group))
                        }
                        _ => None,
                    };
                    if let Some(rec) = rec_ref {
                        builder.instructions.push(Instruction::DealDamageFrom {
                            source: Box::new(src),
                            recipients: Box::new(rec),
                            amount: Box::new(amt_spec),
                        });
                        return Ok(());
                    }
                }
                return Err("oracle:non-self-damage-source");
            }
            let raw_amount = resolve_x(amount, bound_x);
            let rewrite_self_anaphora = builder.target == BodyTarget::None
                && (self_pronoun_valid || in_trigger)
                && (!(in_trigger && self_pronoun_valid)
                    || matches!(target.as_ref(), SubjectRefAst::TargetObject { .. }));
            let rewritten_amount;
            let effective_amount =
                if rewrite_self_anaphora && value_expr_ast_has_self_anaphora(raw_amount) {
                    rewritten_amount = rewrite_value_expr_ast_self_anaphora(raw_amount);
                    &rewritten_amount
                } else {
                    raw_amount
                };
            let amt_expr = lower_value_expr(effective_amount, vocabulary)?;
            match target.as_ref() {
                SubjectRefAst::AnyTarget
                | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                | SubjectRefAst::TargetPlayer { .. }
                | SubjectRefAst::TargetObject { .. } => {
                    let bt = lower_target_subject(target, vocabulary)?;
                    builder.bind_target(bt)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachOpponent) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::Opponents,
                            amount: n,
                        });
                    } else {
                        builder
                            .instructions
                            .push(Instruction::DamageOpponentsAmount(amt_expr));
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachPlayer) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::Players,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::Players,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::You) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::Controller,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::Controller,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::AbilitySource,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::AbilitySource,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    if rp == ResolvingPlayer::TargetPlayer
                        && matches!(
                            builder.target,
                            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                        )
                    {
                        builder
                            .instructions
                            .push(Instruction::DamageAmount(amt_expr));
                    } else if rp == ResolvingPlayer::UniqueRelatedPlayer
                        && builder.target == BodyTarget::None
                    {
                        builder
                            .instructions
                            .push(Instruction::DamageRelatedPlayerAmount(amt_expr));
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::AffectedPlayers(AffectedPlayers::One(rp)),
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    let group = DamageGroup::Filtered(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ));
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Each(f) | SubjectRefAst::All(f) | SubjectRefAst::Group(f) => {
                    if matches!(
                        f.controller,
                        Some(ControllerRelation::TargetOpponent | ControllerRelation::TargetPlayer)
                    ) && f.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                controller: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        let bt = if f.controller == Some(ControllerRelation::TargetOpponent) {
                            BodyTarget::SingleOpponent
                        } else {
                            BodyTarget::SinglePlayer
                        };
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::DamageTargetControllersCreatures {
                                amount: amt_expr,
                                include_target: false,
                                require_damaged_creatures_to_attack: false,
                            });
                        return Ok(());
                    }
                    let group = lower_damage_group(f, vocabulary)?;
                    if let DamageGroup::Filtered(obj) = group {
                        bind_filter_player_target(builder, &obj)?;
                    }
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Compound(items) if items.len() >= 2 => {
                    let is_all_creatures = |s: &SubjectRefAst| {
                        matches!(
                            s,
                            SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::CREATURE
                                    && f.forbidden_types.is_empty()
                                    && f.controller.is_none()
                                    && f.subtypes.is_empty()
                                    && f.forbidden_subtypes.is_empty()
                                    && f.any_colors.is_empty()
                                    && f.forbidden_colors.is_empty()
                                    && f.required_keywords.is_empty()
                                    && f.forbidden_keywords.is_empty()
                                    && f.flags.is_empty()
                        )
                    };
                    let is_all_pws = |s: &SubjectRefAst| {
                        matches!(
                            s,
                            SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::PLANESWALKER
                                    && f.forbidden_types.is_empty()
                                    && f.controller.is_none()
                                    && f.flags.is_empty()
                        )
                    };
                    let is_each_player = |s: &SubjectRefAst| {
                        matches!(s, SubjectRefAst::Player(PlayerRefAst::EachPlayer))
                    };
                    let group = if items.len() == 2
                        && ((is_all_creatures(&items[0]) && is_each_player(&items[1]))
                            || (is_each_player(&items[0]) && is_all_creatures(&items[1])))
                    {
                        DamageGroup::CreaturesAndPlayers
                    } else if items.len() == 2
                        && ((is_all_creatures(&items[0]) && is_all_pws(&items[1]))
                            || (is_all_pws(&items[0]) && is_all_creatures(&items[1])))
                    {
                        DamageGroup::CreaturesAndPlaneswalkers
                    } else if items.iter().any(|it| {
                        matches!(
                            it,
                            SubjectRefAst::AnyTarget
                                | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                                | SubjectRefAst::TargetPlayer {
                                    count: TargetCountAst::Exact(1),
                                    ..
                                }
                                | SubjectRefAst::TargetObject {
                                    count: TargetCountAst::Exact(1),
                                    ..
                                }
                        )
                    }) {
                        let target_count = items
                            .iter()
                            .filter(|it| {
                                matches!(
                                    it,
                                    SubjectRefAst::AnyTarget
                                        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                                        | SubjectRefAst::TargetPlayer { .. }
                                        | SubjectRefAst::TargetObject { .. }
                                )
                            })
                            .count();
                        if target_count != 1 {
                            return Err("oracle:unsupported-damage-target");
                        }
                        let mut target_items = Vec::new();
                        let mut non_target_items = Vec::new();
                        for it in items {
                            if matches!(
                                it,
                                SubjectRefAst::AnyTarget
                                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                                    | SubjectRefAst::TargetPlayer { .. }
                                    | SubjectRefAst::TargetObject { .. }
                            ) {
                                target_items.push(it);
                            } else {
                                non_target_items.push(it);
                            }
                        }
                        for it in target_items.into_iter().chain(non_target_items) {
                            lower_instruction(
                                &InstructionAst::DealDamage {
                                    source: source.clone(),
                                    target: Box::new(it.clone()),
                                    amount: amount.clone(),
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
                    } else {
                        let mut player_group: Option<AffectedPlayers> = None;
                        for item in items {
                            if let SubjectRefAst::Player(p) = item {
                                let ap =
                                    lower_affected_players_in_context(*p, builder, in_trigger)?;
                                if player_group.is_some() {
                                    return Err("oracle:unsupported-damage-target");
                                }
                                player_group = Some(ap);
                            }
                        }
                        let mut perm_filters = Vec::new();
                        for item in items {
                            match item {
                                SubjectRefAst::Each(f)
                                | SubjectRefAst::All(f)
                                | SubjectRefAst::Group(f)
                                    if f.zone.is_none() =>
                                {
                                    let mut adj = f.clone();
                                    if adj.controller == Some(ControllerRelation::ThatPlayer) {
                                        match player_group {
                                            Some(AffectedPlayers::EachOpponent) => {
                                                adj.controller = Some(ControllerRelation::Opponent);
                                            }
                                            Some(AffectedPlayers::EachPlayer) => {
                                                adj.controller = None;
                                            }
                                            Some(AffectedPlayers::EachOtherPlayer) => {
                                                adj.controller = Some(ControllerRelation::NotYou);
                                            }
                                            Some(AffectedPlayers::One(
                                                ResolvingPlayer::Controller,
                                            )) => {
                                                adj.controller = Some(ControllerRelation::You);
                                            }
                                            _ => {}
                                        }
                                    }
                                    let obj = lower_object_filter(&adj, vocabulary)?;
                                    bind_filter_player_target(builder, &obj)?;
                                    perm_filters.push(obj);
                                }
                                SubjectRefAst::Player(_) => {}
                                _ => return Err("oracle:unsupported-damage-target"),
                            }
                        }
                        if perm_filters.is_empty() {
                            if let Some(players) = player_group {
                                DamageGroup::AffectedPlayers(players)
                            } else {
                                return Err("oracle:unsupported-damage-target");
                            }
                        } else {
                            let combined_filter = if perm_filters.len() == 1 {
                                perm_filters[0]
                            } else {
                                let mut clauses = Vec::new();
                                for f in perm_filters {
                                    match f {
                                        ObjectFilter::Clause(c) => clauses.push(c),
                                        ObjectFilter::Either(a, b) => {
                                            clauses.push(a);
                                            clauses.push(b);
                                        }
                                    }
                                }
                                let Ok([first, second]) = <[ObjectClause; 2]>::try_from(clauses)
                                else {
                                    return Err("oracle:unsupported-damage-target");
                                };
                                ObjectFilter::Either(first, second)
                            };
                            match player_group {
                                Some(players) => DamageGroup::FilteredAndPlayers {
                                    filter: combined_filter,
                                    players,
                                },
                                None => DamageGroup::Filtered(combined_filter),
                            }
                        }
                    };
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(p)
                    if matches!(
                        p,
                        PlayerRefAst::ActivePlayer
                            | PlayerRefAst::EachOtherPlayer
                            | PlayerRefAst::EachOtherOpponent
                            | PlayerRefAst::DefendingPlayer
                            | PlayerRefAst::EnchantedPlayer
                            | PlayerRefAst::ItsController
                            | PlayerRefAst::ItsOwner
                            | PlayerRefAst::ThatObjectController
                    ) =>
                {
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    let group = DamageGroup::AffectedPlayers(players);
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::DealDamageFrom {
                        source: Box::new(DamageSourceRef::AbilitySource),
                        recipients: Box::new(DamageRecipientsRef::TriggeringObject),
                        amount: Box::new(DamageAmountSpec::Expr(amt_expr)),
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if builder.target == BodyTarget::None
                        && in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    builder.instructions.push(Instruction::DealDamageFrom {
                        source: Box::new(DamageSourceRef::AbilitySource),
                        recipients: Box::new(DamageRecipientsRef::TriggeringObject),
                        amount: Box::new(DamageAmountSpec::Expr(amt_expr)),
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && builder.last_group_filter.is_some() =>
                {
                    let obj = builder
                        .last_group_filter
                        .ok_or("oracle:unsupported-damage-target")?;
                    let group = DamageGroup::Filtered(obj);
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(
                    PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController,
                ) if builder.target != BodyTarget::None => {
                    builder.instructions.push(Instruction::DealDamageFrom {
                        source: Box::new(DamageSourceRef::AbilitySource),
                        recipients: Box::new(DamageRecipientsRef::TargetController),
                        amount: Box::new(DamageAmountSpec::Expr(amt_expr)),
                    });
                    Ok(())
                }
                SubjectRefAst::Player(
                    PlayerRefAst::DefendingPlayer
                    | PlayerRefAst::EnchantedPlayer
                    | PlayerRefAst::ItsController
                    | PlayerRefAst::ThatObjectController,
                ) if builder.target == BodyTarget::None => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::RelatedPlayer,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::RelatedPlayer,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-damage-target"),
            }
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => {
            let expr = lower_value_expr(resolve_x(delta, bound_x), vocabulary)?;
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
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(value, bound_x), vocabulary)?
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
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(obj)));
                Ok(())
            }
            SubjectRefAst::Group(f) if f.zone.is_none() => {
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
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
            SubjectRefAst::Anaphora(raw) if raw.trim().eq_ignore_ascii_case("your hand") => {
                builder.instructions.push(Instruction::ExileGroupFromZone {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    zone: SourceZoneFilter::Hand,
                    filter: Box::new(ObjectFilter::any()),
                });
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
                let obj = lower_object_filter(f, vocabulary)?;
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
                            ZoneFilter::OpponentGraveyard
                                | ZoneFilter::OpponentsGraveyards
                                | ZoneFilter::AnyGraveyard
                                | ZoneFilter::AllGraveyards
                                | ZoneFilter::ThatPlayerGraveyard
                                | ZoneFilter::TargetPlayerGraveyard
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
                for it in items {
                    lower_instruction(
                        &InstructionAst::Exile {
                            actor: None,
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
                        "their hand" | "your hand"
                    ) =>
                {
                    builder.instructions.push(Instruction::ExileGroupFromZone {
                        players,
                        zone: SourceZoneFilter::Hand,
                        filter: Box::new(ObjectFilter::any()),
                    });
                    Ok(())
                }
                SubjectRefAst::Group(f)
                    if matches!(
                        f.zone,
                        Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand)
                    ) =>
                {
                    let mut inner = f.as_ref().clone();
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
                        )
                    ) =>
                {
                    let mut inner = f.as_ref().clone();
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
                    let mut inner = f.as_ref().clone();
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
                        Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => {
                            SourceZoneFilter::Hand
                        }
                        Some(
                            ZoneFilter::YourGraveyard
                            | ZoneFilter::ThatPlayerGraveyard
                            | ZoneFilter::TargetPlayerGraveyard,
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
                    let mut inner = f.as_ref().clone();
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
            let bt = lower_target_subject(subject, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::ExileTargetUntilSourceLeaves);
            Ok(())
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
                let counter = parse_return_with_counter(with_counter.as_deref())?;
                (
                    SubjectDestination::Battlefield {
                        tapped: enters_tapped,
                        attacking: bf_f.attacking,
                        transformed: *transformed || bf_f.transformed,
                        under_owner_control: bf_f.under_owner_control,
                        with_counter: counter,
                        attach_to_self: false,
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
                let max_expr = lower_value_expr(resolve_x(cnt_ast, bound_x), vocabulary)?;
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: SubjectDestination::Library(pl),
                        });
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
                        if pl == LibraryPlacement::Bottom
                            && f.zone.is_none()
                            && from_zone.is_none()
                            && builder.target == BodyTarget::None =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder
                            .instructions
                            .push(Instruction::PutGroupOnLibraryBottom(Box::new(obj)));
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
                        builder.instructions.push(Instruction::ReturnToOwnersHand);
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
                let attach_to_self = attached_to.as_deref().is_some_and(|a| {
                    (in_trigger || self_pronoun_valid)
                        && (is_self_pronoun(a, builder, self_pronoun_valid)
                            || matches!(a, SubjectRefAst::Anaphora(_)))
                });
                if attached_to.is_some() && !attach_to_self {
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
                                    && filter.any_types == AstCardTypeMask::CREATURE
                                    && filter.forbidden_types.is_empty()
                                    && filter.subtypes.is_empty()
                                    && filter.stat_bounds.is_empty()
                                    && filter.any_colors.is_empty()
                                    && filter.forbidden_colors.is_empty()
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
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::ChosenX,
                    } if !has_extra_bf_mods
                        && !enters_tapped
                        && (filter.zone == Some(ZoneFilter::YourGraveyard)
                            || from_zone.as_deref().is_some_and(|z| {
                                z.to_ascii_lowercase().contains("your graveyard")
                            }))
                        && filter.any_types == AstCardTypeMask::CREATURE
                        && filter.forbidden_types.is_empty()
                        && filter.subtypes.is_empty()
                        && filter.any_colors.is_empty()
                        && filter.forbidden_colors.is_empty()
                        && filter.stat_bounds.is_empty()
                        && filter.flags.is_empty() =>
                    {
                        builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                            ObjectFilter::creature(),
                        ))?;
                        builder
                            .instructions
                            .push(Instruction::ReturnChosenXGraveyardCreatures);
                        Ok(())
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
                            Some(ZoneFilter::AnyGraveyard) => {
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
        InstructionAst::MoveZone { subject, to } => {
            let dest = to.trim().to_ascii_lowercase();
            if let Some(bf_flags) = parse_battlefield_dest(&dest) {
                let enters_tapped = bf_flags.tapped;
                let attacking = bf_flags.attacking;
                let to_bf = !bf_flags.under_owner_control;
                let bf_dest = SubjectDestination::Battlefield {
                    tapped: enters_tapped,
                    attacking,
                    transformed: bf_flags.transformed,
                    under_owner_control: bf_flags.under_owner_control,
                    with_counter: None,
                    attach_to_self: false,
                };
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if !attacking
                        && !bf_flags.transformed
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
                    SubjectRefAst::Group(f) => {
                        if !attacking
                            && !bf_flags.transformed
                            && !bf_flags.under_owner_control
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
                        builder
                            .instructions
                            .push(Instruction::ShuffleAbilitySourceIntoLibrary);
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
                            Some(ZoneFilter::AnyGraveyard) => {
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
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: SubjectDestination::Library(pl),
                        });
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
                        if pl == LibraryPlacement::Bottom
                            && f.zone.is_none()
                            && builder.target == BodyTarget::None =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder
                            .instructions
                            .push(Instruction::PutGroupOnLibraryBottom(Box::new(obj)));
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
                            Some(ZoneFilter::AnyGraveyard) => {
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
                    SubjectRefAst::Anaphora(_) if in_trigger => {
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
        ) =>
        {
            let f = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) if f.zone.is_none() => f,
                _ => return Err("oracle:unsupported-sacrifice"),
            };
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let ptf = lower_permanent_type_filter(f, vocabulary)?;
            let mut count_f = f.as_ref().clone();
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
                        PlayerRefAst::ThatPlayer,
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
            builder.instructions.push(Instruction::Sacrifice {
                players: AffectedPlayers::One(rp),
                amount: half_expr,
                filter: ptf,
            });
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
                let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
                if count.is_none()
                    && matches!(player, PlayerRefAst::You)
                    && let Some(sel) = lower_sacrifice_selection(f, vocabulary)
                {
                    builder
                        .instructions
                        .push(Instruction::SacrificeMatching(sel));
                    return Ok(());
                }
                let amount = match count {
                    Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary)?,
                    None => ValueExpr::literal(1),
                };
                let ptf = lower_permanent_type_filter(f, vocabulary)?;
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
                    Some(expr) => lower_value_expr(expr, vocabulary)?,
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
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let obj_filter = lower_object_filter(unless_filter, vocabulary)?;
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            let opt1 = ChooseOptionBranch {
                label: "Discard matching card".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DiscardFiltered {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        amount: ValueExpr::literal(1),
                        filter: Box::new(obj_filter),
                    }],
                ),
            };
            let opt2 = ChooseOptionBranch {
                label: "Discard cards".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Discard {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        amount: amt,
                    }],
                ),
            };
            match players {
                AffectedPlayers::One(rp) => {
                    builder.instructions.push(Instruction::ChooseOption {
                        chooser: rp,
                        options: vec![opt1, opt2],
                    });
                }
                multi => {
                    builder
                        .instructions
                        .push(Instruction::ChooseOptionForPlayers {
                            players: multi,
                            options: vec![opt1, opt2],
                        });
                }
            }
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
        } if !is_unrestricted_filter(f) => {
            let amt = match count {
                Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary)?,
                None => ValueExpr::literal(1),
            };
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            let obj_filter = lower_object_filter(f, vocabulary)?;
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
        } if filter.as_deref().is_none_or(is_unrestricted_filter) => {
            let amt = match count {
                Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary)?,
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
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Discard {
                players: AffectedPlayers::One(rp),
                amount: ValueExpr::count_hand(rp),
            });
            Ok(())
        }
        InstructionAst::Mill { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if matches!(player, PlayerRefAst::EachPlayer) {
                let ValueExpr::Literal(n) = amt else {
                    return Err("oracle:dynamic-mill-each");
                };
                let n_u16 = u16::try_from(n.to_usize().ok_or("oracle:mill-ovf")?)
                    .map_err(|_| "oracle:mill-ovf")?;
                builder
                    .instructions
                    .push(Instruction::MillEachPlayer(n_u16));
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Mill {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Scry { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Scry {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Surveil { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
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
        } => lower_search_library_instruction(
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
            false,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::ShuffleLibrary {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::ShuffleOwnLibrary);
            Ok(())
        }
        InstructionAst::ShuffleLibrary { player } => {
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
                builder.instructions.push(Instruction::TapAbilitySource);
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder.instructions.push(Instruction::TapEnchantedCreature);
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
                        action: PermanentTargetAction::Tap,
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
                        action: PermanentTargetAction::Tap,
                    });
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
                builder
                    .instructions
                    .push(Instruction::TapOrUntapControlled {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        tap: true,
                        amount: ValueExpr::literal(1),
                        filter: Box::new(obj),
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None && builder.last_group_filter.is_some() =>
            {
                let obj = builder
                    .last_group_filter
                    .ok_or("oracle:unsupported-tap-perm")?;
                builder.instructions.push(Instruction::SetGroupTapped {
                    filter: Box::new(obj),
                    tapped: true,
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
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder.instructions.push(Instruction::SetGroupTapped {
                    filter: Box::new(obj),
                    tapped: true,
                });
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
        InstructionAst::UntapPermanent { count: Some(_), .. }
        | InstructionAst::UntapPermanent { up_to: true, .. } => {
            Err("oracle:unsupported-untap-perm")
        }
        InstructionAst::UntapPermanent { subject, .. } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder.instructions.push(Instruction::UntapAbilitySource);
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder.instructions.push(Instruction::SetGroupTapped {
                    filter: Box::new(ObjectFilter::Clause(ObjectClause::any().attached_to_self())),
                    tapped: false,
                });
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
                builder
                    .instructions
                    .push(Instruction::TapOrUntapControlled {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        tap: false,
                        amount: ValueExpr::literal(1),
                        filter: Box::new(obj),
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None && builder.last_group_filter.is_some() =>
            {
                let obj = builder
                    .last_group_filter
                    .ok_or("oracle:unsupported-untap-perm")?;
                builder.instructions.push(Instruction::SetGroupTapped {
                    filter: Box::new(obj),
                    tapped: false,
                });
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
                    .push(Instruction::UntapUpTo { kind, maximum });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f)
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
                builder.last_group_filter = Some(ObjectFilter::land().controlled());
                builder.instructions.push(Instruction::UntapControlledLands);
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f)
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
                if builder.target != BodyTarget::None {
                    builder.instructions.push(Instruction::SetGroupTapped {
                        filter: Box::new(filter),
                        tapped: false,
                    });
                } else {
                    builder
                        .instructions
                        .push(Instruction::UntapControlledCreatures);
                }
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.last_group_filter = Some(obj);
                builder.instructions.push(Instruction::SetGroupTapped {
                    filter: Box::new(obj),
                    tapped: false,
                });
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
            _ => Err("oracle:unsupported-toggle-tap"),
        },
        InstructionAst::SkipNextUntap { subject } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                builder.instructions.push(Instruction::ExertAbilitySource);
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
                    builder.instructions.push(Instruction::SkipNextUntap {
                        subject: Box::new(EffectSubject::TargetObject),
                    });
                    return Ok(());
                }
                if in_trigger {
                    builder.instructions.push(Instruction::SkipNextUntap {
                        subject: Box::new(EffectSubject::TriggerSubject),
                    });
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
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.instructions.push(Instruction::SkipNextUntap {
                        subject: Box::new(EffectSubject::Matching(obj)),
                    });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder.instructions.push(Instruction::SkipNextUntap {
                        subject: Box::new(EffectSubject::Matching(
                            ObjectFilter::any().attached_to_self(),
                        )),
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-skip-untap"),
            }
        }
        InstructionAst::Regenerate { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder.instructions.push(Instruction::RegenerateSource);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::RegenerateTarget);
                Ok(())
            }
            _ => Err("oracle:unsupported-regenerate"),
        },
        InstructionAst::Transform { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::TransformSource);
            Ok(())
        }
        InstructionAst::BecomePrepared { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::BecomePreparedSource);
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
        InstructionAst::Fight { a, b } => {
            let had_perm_target = matches!(
                builder.target,
                BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::FilteredPermanent(_)
            );
            let first = lower_fight_or_exchange_subject(
                a,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                false,
                had_perm_target,
            )?;
            let second = lower_fight_or_exchange_subject(
                b,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                false,
                had_perm_target,
            )?;
            if first == MoveSubjectRef::AbilitySource
                && second == MoveSubjectRef::TargetObject
                && builder.target.guarantees_creature()
            {
                builder
                    .instructions
                    .push(Instruction::FightSourceWithTarget);
            } else {
                builder.instructions.push(Instruction::Fight {
                    first: Box::new(first),
                    second: Box::new(second),
                });
            }
            Ok(())
        }
        InstructionAst::Counter {
            target,
            unless_pay,
            unless_action: None,
        } => {
            let bt = lower_target_subject(target, vocabulary)?;
            builder.bind_target(bt)?;
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
                                Some(expr) => lower_value_expr(expr, vocabulary)?,
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
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: Some(scale_expr),
        } => {
            let dur = lower_duration(duration)?;
            let dp = parse_signed_pt_delta(dx).ok_or("oracle:non-numeric-pt-delta")?;
            let dt = parse_signed_pt_delta(dy).ok_or("oracle:non-numeric-pt-delta")?;
            let amt = lower_value_expr(resolve_x(scale_expr, bound_x), vocabulary)?;
            if dur == ResolvedDuration::UntilEndOfTurn {
                let group = match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        Some(EffectSubject::AbilitySource)
                    }
                    SubjectRefAst::AttachedHost(_) => Some(EffectSubject::Matching(
                        ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                    )),
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        Some(EffectSubject::Matching(obj))
                    }
                    _ => None,
                };
                if let Some(group) = group {
                    builder.instructions.push(Instruction::ModifyGroupByAmount {
                        group: Box::new(group),
                        scale: PowerToughness {
                            power: dp.into(),
                            toughness: dt.into(),
                        },
                        amount: amt,
                        keywords: BTreeSet::new(),
                    });
                    return Ok(());
                }
            }
            let pe = scale_value_expr(amt.clone(), dp);
            let te = scale_value_expr(amt, dt);
            lower_modify_pt_part(
                subject,
                ResolvedPartSpec::ModifyPTExpr {
                    power: Box::new(pe),
                    toughness: Box::new(te),
                },
                dur,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: None,
        } => {
            let dur = lower_duration(duration)?;
            let dp = parse_signed_pt_delta(dx);
            let dt = parse_signed_pt_delta(dy);
            match (dp, dt) {
                (Some(p), Some(t)) => {
                    let pt = PowerToughness {
                        power: p.into(),
                        toughness: t.into(),
                    };
                    lower_modify_pt_target(
                        subject,
                        pt,
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                }
                _ => {
                    let pe = parse_dynamic_pt_expr(dx, bound_x, vocabulary)
                        .ok_or("oracle:unsupported-dynamic-modify-pt")?;
                    let te = parse_dynamic_pt_expr(dy, bound_x, vocabulary)
                        .ok_or("oracle:unsupported-dynamic-modify-pt")?;
                    lower_modify_pt_part(
                        subject,
                        ResolvedPartSpec::ModifyPTExpr {
                            power: Box::new(pe),
                            toughness: Box::new(te),
                        },
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                }
            }
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            if dur == ResolvedDuration::UntilEndOfTurn
                && keywords.len() == 1
                && keywords[0].trim().eq_ignore_ascii_case("flashback")
            {
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, Some(ZoneFilter::YourGraveyard) | None)
                        && (filter.any_types == AstCardTypeMask::INSTANT
                            || filter.any_types
                                == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)) =>
                    {
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = lower_object_filter(&inner, vocabulary)?;
                        builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                        builder.instructions.push(Instruction::GrantFlashback(
                            spellwake_engine::program::FlashbackSubject::Target,
                        ));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(f.zone, Some(ZoneFilter::YourGraveyard))
                            && (f.any_types == AstCardTypeMask::INSTANT
                                || f.any_types
                                    == AstCardTypeMask::INSTANT
                                        .union(AstCardTypeMask::SORCERY)) =>
                    {
                        builder.instructions.push(Instruction::GrantFlashback(
                            spellwake_engine::program::FlashbackSubject::ControllerGraveyard,
                        ));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let cleaned_keywords: Vec<String> = keywords
                .iter()
                .map(|k| strip_grant_kw_trailing_note(k))
                .filter(|k| !k.is_empty())
                .collect();
            if cleaned_keywords.is_empty() {
                return Err("oracle:unsupported-grant-kw");
            }
            let is_chosen_color_prot = |k: &str| {
                let low = k.trim().trim_end_matches('.').to_ascii_lowercase();
                matches!(
                    low.as_str(),
                    "protection from the color of your choice"
                        | "protection from the chosen color"
                        | "protection from a color of your choice"
                        | "protection from one color of your choice"
                )
            };
            let has_chosen_color_prot = dur == ResolvedDuration::UntilEndOfTurn
                && cleaned_keywords.iter().any(|k| is_chosen_color_prot(k));
            let remaining_keywords: Vec<String> = if has_chosen_color_prot {
                cleaned_keywords
                    .iter()
                    .filter(|k| !is_chosen_color_prot(k))
                    .cloned()
                    .collect()
            } else {
                cleaned_keywords
            };
            if has_chosen_color_prot {
                let handled_prot = match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::TargetPermanent,
                            ));
                        true
                    }
                    SubjectRefAst::Anaphora(_)
                        if matches!(
                            builder.target,
                            BodyTarget::SingleCreature
                                | BodyTarget::SinglePermanent
                                | BodyTarget::FilteredPermanent(_)
                        ) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::TargetPermanent,
                            ));
                        true
                    }
                    SubjectRefAst::Compound(parts)
                        if parts.len() == 2
                            && matches!(&parts[0], SubjectRefAst::Player(PlayerRefAst::You))
                            && matches!(
                                &parts[1],
                                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                                    if f.zone.is_none()
                                        && f.controller == Some(ControllerRelation::You)
                                        && (f.any_permanent || f.any_types.is_empty())
                                        && f.forbidden_types.is_empty()
                                        && f.subtypes.is_empty()
                                        && f.any_colors.is_empty()
                                        && f.forbidden_colors.is_empty()
                                        && f.stat_bounds.is_empty()
                            ) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::ControllerAndPermanents,
                            ));
                        true
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::AbilitySource,
                            ));
                        true
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::Matching(Box::new(ObjectFilter::Clause(
                                    ObjectClause::any().attached_to_self(),
                                ))),
                            ));
                        true
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        if let Ok(obj) = lower_object_filter(f, vocabulary) {
                            bind_filter_player_target(builder, &obj)?;
                            builder
                                .instructions
                                .push(Instruction::GrantProtectionChosenColor(
                                    ProtectionRecipients::Matching(Box::new(obj)),
                                ));
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if handled_prot {
                    if remaining_keywords.is_empty() {
                        return Ok(());
                    }
                } else {
                    return Err("oracle:unsupported-granted-keyword");
                }
            }
            let mut expanded_keywords: Vec<String> = Vec::with_capacity(remaining_keywords.len());
            for kw in &remaining_keywords {
                let low = kw.trim().trim_end_matches('.').to_ascii_lowercase();
                if let Some(rest) = low.strip_prefix("protection from ")
                    && rest.contains(',')
                {
                    let mut parts = Vec::new();
                    let mut ok = true;
                    for comma_seg in rest.split(',') {
                        let seg = comma_seg.trim();
                        let seg = seg.strip_prefix("and ").unwrap_or(seg).trim();
                        if seg.is_empty() {
                            ok = false;
                            break;
                        }
                        for sub in seg.split(" and ") {
                            let item = sub.trim();
                            let item = item.strip_prefix("from ").unwrap_or(item).trim();
                            if item.is_empty() {
                                ok = false;
                                break;
                            }
                            parts.push(format!("protection from {item}"));
                        }
                        if !ok {
                            break;
                        }
                    }
                    if ok && parts.len() >= 2 {
                        expanded_keywords.extend(parts);
                        continue;
                    }
                }
                expanded_keywords.push(kw.clone());
            }
            let ab_change = lower_ability_change(&expanded_keywords, vocabulary)?;
            let pure_kws = (ab_change.removal.is_none()
                && ab_change.suppressed_keywords.is_empty()
                && ab_change.granted_exalted == 0
                && !ab_change.grants_unblockable
                && ab_change.granted_trigger.is_none()
                && ab_change.granted_toxic.is_none()
                && ab_change.granted_combat_keywords.is_empty()
                && !ab_change.granted_keywords.is_empty())
            .then(|| ab_change.granted_keywords.clone());
            match subject.as_ref() {
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid)
                    && dur == ResolvedDuration::UntilEndOfTurn =>
                {
                    if let Some(kws) = pure_kws {
                        if let Some(Instruction::BoostSource {
                            keywords: prev_kws, ..
                        }) = builder.instructions.last_mut()
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        if let Some(Instruction::ModifyGroupUntilEndOfTurn {
                            group,
                            keywords: prev_kws,
                            ..
                        }) = builder.instructions.last_mut()
                            && matches!(group.as_ref(), EffectSubject::AbilitySource)
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        if in_trigger {
                            builder
                                .instructions
                                .push(Instruction::ModifyGroupUntilEndOfTurn {
                                    group: Box::new(EffectSubject::AbilitySource),
                                    power_toughness: None,
                                    keywords: kws,
                                });
                        } else {
                            builder.instructions.push(Instruction::BoostSource {
                                power_toughness: None,
                                keywords: kws,
                            });
                        }
                        return Ok(());
                    }
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
                    Ok(())
                }
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    if let Some(Instruction::CreateContinuousEffect { parts, duration: d }) =
                        builder.instructions.last_mut()
                        && *d == dur
                        && !parts
                            .0
                            .iter()
                            .any(|p| matches!(p, ResolvedPartSpec::Abilities(_)))
                    {
                        parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                        return Ok(());
                    }
                    if let Some(kws) = pure_kws
                        && let Some(Instruction::BoostTargetPower {
                            keywords: prev_kws, ..
                        }) = builder.instructions.last_mut()
                        && dur == ResolvedDuration::UntilEndOfTurn
                    {
                        prev_kws.extend(kws);
                        return Ok(());
                    }
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
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
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
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
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
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
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    for it in items {
                        lower_instruction(
                            &InstructionAst::GrantKeywords {
                                subject: Box::new(it.clone()),
                                keywords: keywords.clone(),
                                duration: duration.clone(),
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
                    count: TargetCountAst::AnyNumber,
                } if filter.zone.is_none() => {
                    let obj = lower_object_filter(filter, vocabulary)?;
                    builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
                    builder
                        .instructions
                        .push(Instruction::ModifyAnyNumberOfPermanents {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::You)
                    if pure_kws.as_ref().is_some_and(|kws| {
                        kws.len() == 1 && kws.contains(&KeywordAbility::Hexproof)
                    }) =>
                {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::ControllerHasHexproof,
                            },
                        ))),
                        duration: dur,
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_created_tokens(&builder.instructions) =>
                {
                    let group = Box::new(EffectSubject::CreatedTokens);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    let group = Box::new(EffectSubject::LastMoved);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group: Box::new(EffectSubject::Matching(group_obj)),
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(group_obj)),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    let group = Box::new(EffectSubject::TriggerSubject);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    let group = Box::new(EffectSubject::TriggerSubject);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    let group = Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    )));
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    let group = Box::new(EffectSubject::Matching(obj));
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && !obj.has_target_player()
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-kw"),
            }
        }
        InstructionAst::SetDynamicPT {
            subject,
            power,
            toughness,
            duration,
        } => {
            if let (Some(p_ast), Some(t_ast)) = (power.as_deref(), toughness.as_deref()) {
                let p_expr = lower_value_expr(resolve_x(p_ast, bound_x), vocabulary)?;
                let t_expr = lower_value_expr(resolve_x(t_ast, bound_x), vocabulary)?;
                let dur = lower_duration(duration)?;
                return lower_modify_pt_part(
                    subject,
                    ResolvedPartSpec::SetPTExpr {
                        power: Box::new(p_expr),
                        toughness: Box::new(t_expr),
                    },
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::SwitchPowerToughness { subject, duration } => {
            let dur = lower_duration(duration)?;
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::SelfObject => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                        duration: dur,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-switch-pt"),
            }
        }
        InstructionAst::RemoveAllAbilities { subject, duration } => {
            let dur = lower_duration(duration)?;
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.zone.is_none()
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                            AbilityChange::remove(AbilityRemoval::All),
                        )]),
                        duration: dur,
                    });
                return Ok(());
            }
            Err("oracle:unsupported-remove-abilities")
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => {
            let kind_low_full = counter_kind.trim().to_ascii_lowercase();
            if let Some((lhs_low, rhs_low)) = kind_low_full.split_once(" and ")
                && (lhs_low.ends_with(" counter")
                    || lhs_low.ends_with(" counters")
                    || parse_counter_kind(lhs_low.trim()).is_some())
            {
                let k1 = lhs_low
                    .strip_suffix(" counters")
                    .or_else(|| lhs_low.strip_suffix(" counter"))
                    .unwrap_or(lhs_low)
                    .trim();
                let (count2, k2_raw) = if let Some(r) = rhs_low
                    .strip_prefix("a ")
                    .or_else(|| rhs_low.strip_prefix("an "))
                    .or_else(|| rhs_low.strip_prefix("one "))
                {
                    (ValueExprAst::Const(1), r.trim())
                } else if let Some(r) = rhs_low.strip_prefix("two ") {
                    (ValueExprAst::Const(2), r.trim())
                } else if let Some(r) = rhs_low.strip_prefix("three ") {
                    (ValueExprAst::Const(3), r.trim())
                } else if let Some((tok, rest)) = rhs_low.split_once(' ')
                    && let Some(n) = parse_small_number_word(tok)
                    && n > 0
                {
                    (ValueExprAst::Const(i32::from(n)), rest.trim())
                } else {
                    (ValueExprAst::Const(1), rhs_low.trim())
                };
                let k2 = k2_raw
                    .strip_suffix(" counters")
                    .or_else(|| k2_raw.strip_suffix(" counter"))
                    .unwrap_or(k2_raw)
                    .trim();
                let second_subject = match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    SubjectRefAst::TargetPlayer {
                        count: TargetCountAst::Exact(1),
                        ..
                    }
                    | SubjectRefAst::Player(
                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent,
                    ) => Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                    _ => subject.clone(),
                };
                lower_instruction(
                    &InstructionAst::PutCounters {
                        subject: subject.clone(),
                        counter_kind: k1.to_string(),
                        count: count.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                lower_instruction(
                    &InstructionAst::PutCounters {
                        subject: second_subject,
                        counter_kind: k2.to_string(),
                        count: Box::new(count2),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                return Ok(());
            }
            let mut kind_clean = kind_low_full.as_str();
            while let Some(rest) = kind_clean
                .strip_prefix("a number of ")
                .or_else(|| kind_clean.strip_prefix("number of "))
                .or_else(|| kind_clean.strip_prefix("or more "))
                .or_else(|| kind_clean.strip_prefix("that many "))
                .or_else(|| kind_clean.strip_prefix("an additional "))
                .or_else(|| kind_clean.strip_prefix("additional "))
                .or_else(|| kind_clean.strip_prefix("one additional "))
                .or_else(|| kind_clean.strip_prefix("another "))
            {
                kind_clean = rest.trim_start();
            }
            let ck = parse_counter_kind(kind_clean).ok_or("oracle:put-counter-kind")?;
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if ck.is_player_counter() {
                if let SubjectRefAst::Player(p) = subject.as_ref() {
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    builder.instructions.push(Instruction::AddPlayerCounter {
                        players,
                        kind: ck,
                        amount: amt,
                    });
                    return Ok(());
                }
                if let SubjectRefAst::TargetPlayer {
                    opponent_only,
                    count: TargetCountAst::Exact(1),
                } = subject.as_ref()
                {
                    builder.bind_target(if *opponent_only {
                        BodyTarget::SingleOpponent
                    } else {
                        BodyTarget::SinglePlayer
                    })?;
                    builder.instructions.push(Instruction::AddPlayerCounter {
                        players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        kind: ck,
                        amount: amt,
                    });
                    return Ok(());
                }
                return Err("oracle:put-counter-kind");
            }
            let const_n = match &amt {
                ValueExpr::Literal(n) => n.to_usize().filter(|&v| v > 0),
                _ => None,
            };
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    if let (ValueExpr::Literal(n), Some(n_usize)) = (&amt, const_n) {
                        if ck == CounterKind::PlusOne {
                            builder
                                .instructions
                                .push(Instruction::PutCountersOnSource { amount: n.clone() });
                            return Ok(());
                        }
                        if ck == CounterKind::Charge && n_usize == 1 {
                            builder
                                .instructions
                                .push(Instruction::PutChargeCounterOnSource);
                            return Ok(());
                        }
                        if ck != CounterKind::MinusOne
                            && let Ok(u) = u16::try_from(n_usize)
                        {
                            builder
                                .instructions
                                .push(Instruction::PutNamedCounterOnSource {
                                    kind: ck,
                                    amount: u,
                                });
                            return Ok(());
                        }
                    }
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::AbilitySource),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    if ck == CounterKind::Indestructible && const_n == Some(1) {
                        builder
                            .instructions
                            .push(Instruction::PutIndestructibleCounter);
                    } else if ck == CounterKind::Loyalty && const_n == Some(1) {
                        builder.instructions.push(Instruction::PutLoyaltyCounter);
                    } else if let (ValueExpr::Literal(n), Some(_)) = (&amt, const_n)
                        && matches!(ck, CounterKind::PlusOne | CounterKind::MinusOne)
                    {
                        builder
                            .instructions
                            .push(Instruction::PutPowerToughnessCounters {
                                counters: if ck == CounterKind::PlusOne {
                                    PowerToughnessCounters {
                                        plus_one: n.clone(),
                                        minus_one: 0.into(),
                                    }
                                } else {
                                    PowerToughnessCounters {
                                        plus_one: 0.into(),
                                        minus_one: n.clone(),
                                    }
                                },
                            });
                    } else {
                        builder
                            .instructions
                            .push(Instruction::PutNamedCounterOnTarget {
                                kind: ck,
                                amount: amt,
                            });
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if filter.zone.is_none() && *n >= 2 => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if filter.zone.is_none() && *max > 0 => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action,
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
                            action: PermanentTargetAction::PutCounters {
                                kind: ck,
                                amount: amt,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    if ck == CounterKind::Indestructible && const_n == Some(1) {
                        builder
                            .instructions
                            .push(Instruction::PutIndestructibleCounter);
                    } else if ck == CounterKind::Loyalty && const_n == Some(1) {
                        builder.instructions.push(Instruction::PutLoyaltyCounter);
                    } else {
                        builder
                            .instructions
                            .push(Instruction::PutNamedCounterOnTarget {
                                kind: ck,
                                amount: amt,
                            });
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject => {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::CreatedTokens),
                            kind: ck,
                            amount: amt,
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
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    if ck == CounterKind::PlusOne
                        && const_n == Some(1)
                        && f.controller == Some(ControllerRelation::You)
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                subtypes: true,
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        if f.subtypes.len() == 1
                            && f.flags.is_empty()
                            && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::CREATURE)
                            && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
                        {
                            builder
                                .instructions
                                .push(Instruction::PutCountersOnControlledSubtype(ct));
                            return Ok(());
                        }
                        if f.any_types == AstCardTypeMask::CREATURE && f.subtypes.is_empty() {
                            if f.flags == AstFilterFlags::EXCLUDE_SELF && self_pronoun_valid {
                                builder
                                    .instructions
                                    .push(Instruction::PutCountersOnOtherCreatures);
                                return Ok(());
                            }
                            if f.flags.is_empty() {
                                builder
                                    .instructions
                                    .push(Instruction::PutCountersOnControlledCreatures);
                                return Ok(());
                            }
                        }
                    }
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::ChosenX,
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder.x_target_count = true;
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::Matching(group_obj)),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    for it in items {
                        lower_instruction(
                            &InstructionAst::PutCounters {
                                subject: Box::new(it.clone()),
                                counter_kind: counter_kind.clone(),
                                count: count.clone(),
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
                _ => Err("oracle:unsupported-put-counters"),
            }
        }
        InstructionAst::Proliferate => {
            builder.instructions.push(Instruction::Proliferate);
            Ok(())
        }
        InstructionAst::CreateToken { controller, token } => {
            if *controller == PlayerRefAst::You
                && let TokenSpecAst::Copy {
                    count,
                    source,
                    tapped,
                    attacking,
                    exception,
                } = token
            {
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                let src_ref = match source.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        Some(MoveSubjectRef::AbilitySource)
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none()
                        || matches!(
                            filter.zone,
                            Some(
                                ZoneFilter::YourGraveyard
                                    | ZoneFilter::OpponentGraveyard
                                    | ZoneFilter::AnyGraveyard
                            )
                        ) =>
                    {
                        let bt = if filter.zone.is_none() {
                            lower_battlefield_body_target(filter, vocabulary)?
                        } else {
                            lower_target_subject(source, vocabulary)?
                        };
                        builder.bind_target(bt)?;
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        Some(MoveSubjectRef::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_permanent_body_target(builder.target)
                            || is_graveyard_body_target(builder.target) =>
                    {
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        Some(MoveSubjectRef::LastMoved)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some(MoveSubjectRef::TriggeringObject)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(MoveSubjectRef::TriggeringObject)
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        Some(MoveSubjectRef::AttachedHost)
                    }
                    _ => None,
                };
                if let Some(src_ref) = src_ref {
                    let exc_low = exception
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase();
                    let nonlegendary = exc_low.contains("isn't legendary")
                        || exc_low.contains("aren't legendary")
                        || exc_low.contains("not legendary");
                    let grant_haste = exc_low.contains("haste");
                    let mut sacrifice_at_end_step =
                        exc_low.contains("sacrifice") && exc_low.contains("end step");
                    let mut exile_at_end_step =
                        exc_low.contains("exile") && exc_low.contains("end step");
                    let strip_mana_cost = exc_low.contains("has no mana cost")
                        || exc_low.contains("have no mana cost")
                        || exc_low.contains("no mana cost");
                    let mut remainder = exc_low.clone();
                    strip_quoted_end_step_token_cleanup(
                        &mut remainder,
                        &mut sacrifice_at_end_step,
                        &mut exile_at_end_step,
                    );
                    for pat in [
                        "except it isn't legendary",
                        "except they aren't legendary",
                        "except it's not legendary",
                        "except they're not legendary",
                        "it isn't legendary",
                        "they aren't legendary",
                        "it's not legendary",
                        "they're not legendary",
                        "not legendary",
                        "except it has haste",
                        "except they have haste",
                        "except it gains haste",
                        "except they gain haste",
                        "it has haste",
                        "they have haste",
                        "it gains haste",
                        "they gain haste",
                        "with haste",
                        "it has no mana cost",
                        "they have no mana cost",
                        "has no mana cost",
                        "have no mana cost",
                        "no mana cost",
                        "sacrifice it at the beginning of the next end step",
                        "sacrifice them at the beginning of the next end step",
                        "sacrifice it at the beginning of your next end step",
                        "sacrifice them at the beginning of your next end step",
                        "exile it at the beginning of the next end step",
                        "exile them at the beginning of the next end step",
                        "exile it at the beginning of your next end step",
                        "exile them at the beginning of your next end step",
                        "in addition to its other types",
                        "in addition to their other types",
                        "with base power and toughness",
                        "except it's a",
                        "except it's an",
                        "except it's",
                        "except they're",
                        "except",
                        "it's a",
                        "it's an",
                        "it's",
                        "they're",
                        "and",
                        ",",
                        ".",
                    ] {
                        remainder = remainder.replace(pat, " ");
                    }
                    let mut override_pt: Option<(u8, u8)> = None;
                    let mut parsed_colors: BTreeSet<Color> = BTreeSet::new();
                    let mut saw_colorless = false;
                    let mut add_subtype: Option<CreatureTypeId> = None;
                    let mut parse_ok = true;
                    for raw_tok in remainder.split_whitespace() {
                        let tok = raw_tok
                            .trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '-');
                        if tok.is_empty()
                            || matches!(
                                tok,
                                "a" | "an"
                                    | "the"
                                    | "is"
                                    | "are"
                                    | "with"
                                    | "creature"
                                    | "creatures"
                                    | "token"
                                    | "tokens"
                                    | "that"
                                    | "that's"
                            )
                        {
                            continue;
                        }
                        if let Some((ps, ts)) = tok.split_once('/')
                            && let (Ok(p), Ok(t)) = (ps.parse::<u8>(), ts.parse::<u8>())
                            && override_pt.is_none()
                        {
                            override_pt = Some((p, t));
                            continue;
                        }
                        match tok {
                            "white" => {
                                parsed_colors.insert(Color::White);
                                continue;
                            }
                            "blue" => {
                                parsed_colors.insert(Color::Blue);
                                continue;
                            }
                            "black" => {
                                parsed_colors.insert(Color::Black);
                                continue;
                            }
                            "red" => {
                                parsed_colors.insert(Color::Red);
                                continue;
                            }
                            "green" => {
                                parsed_colors.insert(Color::Green);
                                continue;
                            }
                            "colorless" => {
                                saw_colorless = true;
                                continue;
                            }
                            _ => {}
                        }
                        if add_subtype.is_none()
                            && let Some(ct) = resolve_protection_subtype(tok, vocabulary)
                        {
                            add_subtype = Some(ct);
                            continue;
                        }
                        parse_ok = false;
                        break;
                    }
                    let override_colors = if !parsed_colors.is_empty() {
                        Some(parsed_colors)
                    } else if saw_colorless {
                        Some(BTreeSet::new())
                    } else {
                        None
                    };
                    if parse_ok {
                        builder.instructions.push(Instruction::CreateTokenCopy {
                            source: src_ref,
                            count: cnt,
                            tapped: *tapped,
                            attacking: *attacking,
                            sacrifice_at_end_step,
                            exile_at_end_step,
                            grant_haste,
                            nonlegendary,
                            override_pt,
                            override_colors,
                            add_subtype,
                            strip_mana_cost,
                        });
                        return Ok(());
                    }
                }
            }
            if *controller == PlayerRefAst::You
                && let TokenSpecAst::Role {
                    role_name,
                    attached_to,
                } = token
            {
                let host_ref = match attached_to.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        Some((MoveSubjectRef::AbilitySource, false))
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        Some((MoveSubjectRef::TargetObject, false))
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        Some((MoveSubjectRef::TargetObject, true))
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        Some((MoveSubjectRef::TargetObject, false))
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some((MoveSubjectRef::TriggeringObject, false))
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some((MoveSubjectRef::TriggeringObject, false))
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        Some((MoveSubjectRef::AttachedHost, false))
                    }
                    _ => None,
                };
                if let Some((host, up_to_one)) = host_ref {
                    let key = format!("{} role", role_name.trim().to_ascii_lowercase());
                    let def = predefined_token_def(&key, vocabulary, ids)?;
                    builder
                        .instructions
                        .push(Instruction::CreateAuraTokenAttached {
                            definition: Box::new(def),
                            host,
                            up_to_one,
                        });
                    return Ok(());
                }
            }
            let recipients = match controller {
                PlayerRefAst::You => TokenRecipients::Controller,
                PlayerRefAst::EachPlayer => TokenRecipients::EachPlayer,
                PlayerRefAst::EachOpponent => TokenRecipients::EachOpponent,
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    TokenRecipients::Player(ResolvingPlayer::TargetPlayer)
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    TokenRecipients::Player(ResolvingPlayer::TargetPlayer)
                }
                PlayerRefAst::ThatPlayer
                | PlayerRefAst::DefendingPlayer
                | PlayerRefAst::EnchantedPlayer => TokenRecipients::Player(
                    lower_resolving_player_in_context(*controller, builder, in_trigger)?,
                ),
                PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController => {
                    match &builder.target {
                        BodyTarget::None => {
                            let after_group_destroy = !in_trigger
                                && builder.instructions.iter().any(|op| {
                                    matches!(
                                        op,
                                        Instruction::DestroyGroup(_)
                                            | Instruction::DestroyGroupWithoutRegeneration(_)
                                            | Instruction::MoveGroup { .. }
                                    )
                                });
                            if after_group_destroy {
                                TokenRecipients::DestroyedPermanentControllers
                            } else if in_trigger {
                                TokenRecipients::Player(ResolvingPlayer::UniqueRelatedPlayer)
                            } else {
                                return Err("oracle:unbound-that-player");
                            }
                        }
                        BodyTarget::SingleSpell(_) => {
                            TokenRecipients::Player(ResolvingPlayer::TargetSpellController)
                        }
                        BodyTarget::SingleCreature
                        | BodyTarget::SinglePermanent
                        | BodyTarget::SinglePlaneswalker
                        | BodyTarget::SingleAnyTarget
                        | BodyTarget::SingleCreatureOrPlayer
                        | BodyTarget::SinglePlayerOrPlaneswalker
                        | BodyTarget::SingleOpponentOrPlaneswalker
                        | BodyTarget::FilteredPermanent(_) => {
                            TokenRecipients::Player(ResolvingPlayer::TargetController)
                        }
                        _ => return Err("oracle:unsupported-token-controller"),
                    }
                }
                PlayerRefAst::ItsOwner => match &builder.target {
                    BodyTarget::None if in_trigger => {
                        TokenRecipients::Player(ResolvingPlayer::UniqueRelatedPlayer)
                    }
                    BodyTarget::None => return Err("oracle:unbound-that-player"),
                    BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::SingleAnyTarget
                    | BodyTarget::SingleCreatureOrPlayer
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker
                    | BodyTarget::FilteredPermanent(_)
                    | BodyTarget::SingleOwnGraveyardCard
                    | BodyTarget::AnyGraveyardCard
                    | BodyTarget::AnyGraveyardCreature
                    | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                    | BodyTarget::FilteredOwnGraveyardCard(_)
                    | BodyTarget::FilteredAnyGraveyardCard { .. }
                    | BodyTarget::SingleSpell(_) => {
                        TokenRecipients::Player(ResolvingPlayer::TargetOwner)
                    }
                    _ => return Err("oracle:unsupported-token-controller"),
                },
                _ => return Err("oracle:unsupported-token-controller"),
            };
            let (creation, attacking) =
                lower_token_spec(token, recipients, vocabulary, ids, bound_x)?;
            if attacking {
                if !creation.enters_tapped || creation.recipients != TokenRecipients::Controller {
                    return Err("oracle:unsupported-attacking-token");
                }
                builder
                    .instructions
                    .push(Instruction::CreateAttackingTokens {
                        creations: vec![creation],
                    });
            } else {
                builder.instructions.push(Instruction::CreateTokens {
                    creations: vec![creation],
                });
            }
            Ok(())
        }
        InstructionAst::AddMana {
            player: PlayerRefAst::You,
            mana,
        } => {
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
            match mana {
                ManaSpecAst::Fixed(raw) => {
                    let bundle = parse_mana_bundle(raw).ok_or("oracle:bad-mana-bundle")?;
                    builder
                        .instructions
                        .push(Instruction::AddMana(EffectManaYield::Fixed(bundle)));
                }
                ManaSpecAst::ScaledSymbol { symbol, amount } => {
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    if let Some(kind) = parse_single_mana_type(symbol) {
                        builder
                            .instructions
                            .push(Instruction::AddMana(EffectManaYield::Counted {
                                kind,
                                amount: amt,
                            }));
                    } else if let Some(bundle) = parse_mana_bundle(symbol) {
                        builder.instructions.push(Instruction::RepeatLoop {
                            count: amt,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::AddMana(EffectManaYield::Fixed(bundle))],
                            )),
                        });
                    } else {
                        return Err("oracle:bad-mana-sym");
                    }
                }
                ManaSpecAst::CombinationOf { amount, symbols } if symbols.len() >= 2 => {
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    if symbols.len() == 2 {
                        let first =
                            parse_single_mana_type(&symbols[0]).ok_or("oracle:bad-mana-sym")?;
                        let second =
                            parse_single_mana_type(&symbols[1]).ok_or("oracle:bad-mana-sym")?;
                        if first == second {
                            return Err("oracle:unsupported-effect-mana");
                        }
                        builder
                            .instructions
                            .push(Instruction::AddMana(EffectManaYield::Split {
                                first,
                                second,
                                amount: amt,
                            }));
                    } else {
                        let mut colors = BTreeSet::new();
                        for s in symbols {
                            colors.insert(parse_single_mana_type(s).ok_or("oracle:bad-mana-sym")?);
                        }
                        builder.instructions.push(Instruction::RepeatLoop {
                            count: amt,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::AddMana(EffectManaYield::Choice {
                                    colors,
                                    amount: ValueExpr::literal(1),
                                })],
                            )),
                        });
                    }
                }
                ManaSpecAst::Choice(opts) if opts.len() >= 2 => {
                    let all_single: Option<BTreeSet<ManaType>> =
                        opts.iter().map(|opt| parse_single_mana_type(opt)).collect();
                    if let Some(colors) = all_single
                        && !colors.is_empty()
                    {
                        builder
                            .instructions
                            .push(Instruction::AddMana(EffectManaYield::Choice {
                                colors,
                                amount: ValueExpr::literal(1),
                            }));
                    } else {
                        let bundles: Vec<BTreeMap<ManaType, RulesInteger>> = opts
                            .iter()
                            .map(|opt| parse_mana_bundle(opt).ok_or("oracle:bad-mana-sym"))
                            .collect::<Result<_, _>>()?;
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
                            builder.instructions.push(Instruction::AddMana(
                                EffectManaYield::Choice {
                                    colors,
                                    amount: ValueExpr::Literal(qty),
                                },
                            ));
                        } else {
                            builder.instructions.push(Instruction::AddMana(
                                EffectManaYield::ChoiceBundle { options: bundles },
                            ));
                        }
                    }
                }
                ManaSpecAst::AnyColor {
                    amount,
                    single_color,
                } => {
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    if *single_color {
                        builder
                            .instructions
                            .push(Instruction::AddMana(EffectManaYield::Choice {
                                colors: wubrg(),
                                amount: amt,
                            }));
                    } else {
                        builder.instructions.push(Instruction::RepeatLoop {
                            count: amt,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::AddMana(EffectManaYield::Choice {
                                    colors: wubrg(),
                                    amount: ValueExpr::literal(1),
                                })],
                            )),
                        });
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
                    builder.instructions.push(Instruction::AddMana(
                        EffectManaYield::LandProducible {
                            controller: players,
                            amount: ValueExpr::literal(1),
                        },
                    ));
                }
                ManaSpecAst::CommanderColorIdentity => {
                    builder.instructions.push(Instruction::AddMana(
                        EffectManaYield::CommanderIdentity {
                            amount: ValueExpr::literal(1),
                        },
                    ));
                }
                ManaSpecAst::ChosenColor => {
                    builder.instructions.push(Instruction::AddMana(
                        EffectManaYield::SourceChosenColor {
                            amount: ValueExpr::literal(1),
                        },
                    ));
                }
                ManaSpecAst::DifferentColors { amount } => {
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    builder.instructions.push(Instruction::AddMana(
                        EffectManaYield::DistinctColors { amount: amt },
                    ));
                }
                _ => return Err("oracle:unsupported-effect-mana"),
            }
            Ok(())
        }
        InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject,
            duration,
        } => {
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.any_spell
            {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::GainControlOfSpell);
                return Ok(());
            }
            let dur = lower_duration(duration)?;
            let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                spellwake_engine::characteristics::ControlRecipient::ResolvingController,
            )]);
            match subject.as_ref() {
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder.instructions.push(Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))),
                        parts,
                        duration: dur,
                    });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.instructions.push(Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::Matching(obj)),
                        parts,
                        duration: dur,
                    });
                    Ok(())
                }
                _ => {
                    let bt = lower_target_subject(subject, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts,
                            duration: dur,
                        });
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
                .push(Instruction::GiveControlOfAbilitySource);
            Ok(())
        }
        InstructionAst::GainControl {
            controller: ctrl @ (PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent),
            subject,
            duration,
        } if !in_trigger
            && builder.target == BodyTarget::None
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
                        vec![Instruction::DrawPlayer {
                            players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                            amount: ValueExpr::literal(0),
                        }],
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
            let rp = lower_resolving_player_in_context(*controller, builder, in_trigger)?;
            let eff_subj = match subject.as_ref() {
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
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    EffectSubject::TargetObject
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    EffectSubject::Matching(obj)
                }
                _ => return Err("oracle:unsupported-gain-control"),
            };
            builder
                .instructions
                .push(Instruction::GainControlOfPermanent {
                    subject: Box::new(eff_subj),
                    controller: rp,
                    duration: dur,
                });
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: true,
            target: None,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::Combat,
                ));
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: None,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::All,
                ));
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: true,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } if is_self_pronoun(subj, builder, self_pronoun_valid) => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::CombatToOrFromAbilitySource,
                ));
            Ok(())
        }
        InstructionAst::RemoveCounters {
            subject,
            counter_kind,
            all,
            count,
        } => {
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let kind = if ck_low.is_empty() || ck_low == "any" {
                None
            } else {
                Some(parse_counter_kind(&ck_low).ok_or("oracle:unsupported-counter-kind")?)
            };
            let target_self = is_self_pronoun(subject, builder, self_pronoun_valid);
            let amt = if *all {
                if !target_self {
                    return Err("oracle:unsupported-remove-counters");
                }
                ValueExpr::CounterCount {
                    subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                    kind,
                }
            } else if let Some(c) = count {
                lower_value_expr(resolve_x(c, bound_x), vocabulary)?
            } else {
                ValueExpr::literal(1)
            };
            if target_self {
                builder
                    .instructions
                    .push(Instruction::RemoveCountersFromPermanent {
                        target_self: true,
                        kind,
                        amount: amt,
                    });
                Ok(())
            } else if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.zone.is_none()
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::RemoveCountersFromPermanent {
                        target_self: false,
                        kind,
                        amount: amt,
                    });
                Ok(())
            } else {
                Err("oracle:unsupported-remove-counters")
            }
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } if is_self_pronoun(subj, builder, self_pronoun_valid) => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::ToAbilitySource,
                ));
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } => {
            let bt = lower_target_subject(subj, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::ToTarget,
                ));
            Ok(())
        }
        InstructionAst::PreventDamage {
            amount,
            target,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-prevent-damage");
            };
            let bt = lower_target_subject(target, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreatePreventionShield { amount: n });
            Ok(())
        }
        InstructionAst::BecomeMonarch {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::BecomeMonarch(None));
            Ok(())
        }
        InstructionAst::Bolster { count } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-bolster");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:bolster-ovf")?)
                .map_err(|_| "oracle:bolster-ovf")?;
            builder.instructions.push(Instruction::Bolster(u));
            Ok(())
        }
        InstructionAst::Adapt { count } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-adapt");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:adapt-ovf")?)
                .map_err(|_| "oracle:adapt-ovf")?;
            builder.instructions.push(Instruction::Adapt(u));
            Ok(())
        }
        InstructionAst::Amass { subtype, count } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-amass");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:amass-ovf")?)
                .map_err(|_| "oracle:amass-ovf")?;
            let army = vocabulary
                .resolve_label("Army")
                .ok_or("oracle:missing-army")?;
            let st_label = if subtype.is_empty() {
                "Zombie"
            } else {
                subtype.as_str()
            };
            let sub =
                resolve_protection_subtype(st_label, vocabulary).ok_or("oracle:amass-subtype")?;
            builder
                .instructions
                .push(Instruction::CreateArmyIfAbsent { army, subtype: sub });
            builder.instructions.push(Instruction::Amass {
                army,
                subtype: sub,
                amount: u,
            });
            Ok(())
        }
        InstructionAst::Explore { subject } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                builder.instructions.push(Instruction::ExploreSource);
                return Ok(());
            }
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::ExploreSubject(MoveSubjectRef::TargetObject));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::ExploreSubject(MoveSubjectRef::TargetObject));
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::ExploreSubject(
                        MoveSubjectRef::TriggeringObject,
                    ));
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    builder
                        .instructions
                        .push(Instruction::ExploreSubject(MoveSubjectRef::AttachedHost));
                    Ok(())
                }
                _ => Err("oracle:unsupported-explore"),
            }
        }
        InstructionAst::Manifest {
            count,
            dread,
            subject: None,
        } => {
            if *dread {
                let n = match resolve_x(count, bound_x) {
                    ValueExprAst::Const(k) if *k >= 1 && *k <= 8 => {
                        usize::try_from(*k).unwrap_or(1)
                    }
                    _ => return Err("oracle:unsupported-manifest-dread-count"),
                };
                for _ in 0..n {
                    builder.instructions.push(Instruction::ManifestDread);
                }
                return Ok(());
            }
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder.instructions.push(Instruction::ManifestTop {
                player: ResolvingPlayer::Controller,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::ExtraTurn {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::TakeExtraTurn);
            Ok(())
        }
        InstructionAst::AdditionalCombat(spec) => {
            let low = spec.to_ascii_lowercase();
            if low.contains("additional combat phase")
                && low.contains("untap")
                && low.contains("creatures you control")
            {
                builder.instructions.push(Instruction::AdditionalCombat {
                    untap: CombatUntapGroup::ControlledCreatures,
                });
                Ok(())
            } else {
                Err("oracle:unsupported-additional-combat")
            }
        }
        InstructionAst::CombatRuleMod {
            subject,
            rule,
            duration,
        } => {
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && !items.is_empty()
            {
                for it in items {
                    lower_instruction(
                        &InstructionAst::CombatRuleMod {
                            subject: Box::new(it.clone()),
                            rule: rule.clone(),
                            duration: duration.clone(),
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
            let dur = lower_duration(duration)?;
            let low = rule.to_ascii_lowercase();
            if low.contains("you choose which creatures block") {
                builder.instructions.push(Instruction::SetBlockDeclarations(
                    spellwake_engine::program::BlockDeclarationRule::ControllerChooses,
                ));
                return Ok(());
            }
            if low.contains("each opponent can't block with more than one creature") {
                builder.instructions.push(Instruction::SetBlockDeclarations(
                    spellwake_engine::program::BlockDeclarationRule::EachOpponentOneBlocker,
                ));
                return Ok(());
            }
            if low == "no_max_hand_size"
                && matches!(subject.as_ref(), SubjectRefAst::Player(PlayerRefAst::You))
            {
                if dur == ResolvedDuration::NoStatedDuration {
                    builder
                        .instructions
                        .push(Instruction::GrantNoMaxHandSizeRestOfGame);
                } else {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::DrawRule {
                                id: ids.static_id(),
                                rule: spellwake_engine::characteristics::StaticDrawRule::NoMaximumHandSize,
                            },
                        ))),
                        duration: dur,
                    });
                }
                return Ok(());
            }
            if low == "next_spell_cant_be_countered" {
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantBeCountered {
                        filter: None,
                        controller_only: true,
                    }),
                    duration: dur,
                });
                return Ok(());
            }
            if let Some(kws_raw) = low.strip_prefix("cant_have:") {
                let suppressed = parse_suppressed_keyword_list(kws_raw)?;
                let ab_change = AbilityChange {
                    suppressed_keywords: suppressed,
                    ..AbilityChange::default()
                };
                return emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            if let Some(flt_raw) = low.strip_prefix("cant_cast:") {
                let flt_clean = flt_raw.trim().trim_end_matches('.');
                let flt = if flt_clean.is_empty()
                    || matches!(flt_clean, "spells" | "a spell" | "any spells")
                {
                    None
                } else {
                    let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                        flt_clean,
                        &cached_compiler_vocab(vocabulary),
                    )
                    .map_err(|_| "oracle:unsupported-combat-mod")?;
                    Some(lower_object_filter(&ast_f, vocabulary)?)
                };
                let players = match subject.as_ref() {
                    SubjectRefAst::Player(p) => {
                        lower_affected_players_in_context(*p, builder, in_trigger)?
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantCastSpells {
                        players,
                        filter: flt,
                    }),
                    duration: dur,
                });
                return Ok(());
            }
            if low.contains("doesn't untap during")
                || low.contains("don't untap during")
                || low == "doesnt_untap"
                || low == "doesntuntap"
            {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::AbilitySource),
                        });
                        return Ok(());
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
                                action: PermanentTargetAction::SkipNextUntap,
                            });
                        return Ok(());
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
                                action: PermanentTargetAction::SkipNextUntap,
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
                                action: PermanentTargetAction::SkipNextUntap,
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
                                action: PermanentTargetAction::SkipNextUntap,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::CreatedTokens),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        if let Some(Instruction::AffectPermanentTargets { action, .. }) =
                            builder.instructions.last_mut()
                            && *action == PermanentTargetAction::Tap
                        {
                            *action = PermanentTargetAction::TapAndSkipNextUntap;
                            return Ok(());
                        }
                        let has_multi_target = builder.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::AffectPermanentTargets {
                                    minimum,
                                    maximum,
                                    ..
                                } if (*minimum, *maximum) != (1, 1)
                            )
                        });
                        if has_multi_target {
                            builder.instructions.push(Instruction::SkipNextUntap {
                                subject: Box::new(EffectSubject::TargetObject),
                            });
                        } else {
                            builder
                                .instructions
                                .push(Instruction::AffectPermanentTargets {
                                    minimum: 1,
                                    maximum: 1,
                                    action: PermanentTargetAction::SkipNextUntap,
                                });
                        }
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::LastMoved),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::TriggerSubject),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::TriggerSubject),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::Matching(group_obj)),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::Matching(obj)),
                        });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let combat_action = if low.contains("goad") {
                Some(CombatActionKind::Goad)
            } else if low.contains("untap") && low.contains("remove") && low.contains("combat") {
                Some(CombatActionKind::UntapAndRemoveFromCombat)
            } else if low.contains("remove") && low.contains("from combat") {
                Some(CombatActionKind::RemoveFromCombat)
            } else if low.contains("attacks this turn if able")
                || low.contains("attack this turn if able")
                || (low.contains("attacks ") && !low.contains("block") && low.ends_with("if able"))
                || (low.contains("attack ") && !low.contains("block") && low.ends_with("if able"))
                || low == "must_attack"
                || low.starts_with("must_attack:")
                || low.starts_with("attacks_if_able:")
            {
                Some(CombatActionKind::MustAttack)
            } else if low == "must block"
                || low == "must block this turn"
                || low == "must block this combat"
                || low == "must block if able"
                || low == "must block this turn if able"
                || low == "must block this combat if able"
                || low == "blocks this turn if able"
                || low == "block this turn if able"
                || low == "blocks this combat if able"
                || low == "block this combat if able"
                || low == "blocks it this combat if able"
                || low == "blocks it this turn if able"
            {
                Some(CombatActionKind::MustBlock)
            } else if (low.contains("all creatures able to block") && low.contains("do so"))
                || low == "must_be_blocked_if_able"
                || low == "allableblock"
                || (low.contains("must be blocked")
                    && !low.contains("by two")
                    && !low.contains("by three"))
            {
                Some(CombatActionKind::MustBeBlocked)
            } else if low.contains("prevent all combat damage that would be dealt to and dealt by")
                || low.contains("prevent all combat damage that would be dealt by and to")
                || low.contains("prevent all combat damage that would be dealt to and by")
                || low.contains("prevent all combat damage that would be dealt by and dealt to")
                || low == "prevent_combat_damage_by_and_to"
            {
                Some(CombatActionKind::PreventCombatDamageByAndTo)
            } else {
                None
            };
            if let Some(action_kind) = combat_action {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::AbilitySource),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TargetObject),
                            kind: action_kind,
                        });
                        return Ok(());
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
                                action: PermanentTargetAction::RequireCombatAction(action_kind),
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
                                action: PermanentTargetAction::RequireCombatAction(action_kind),
                            });
                        return Ok(());
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
                                action: PermanentTargetAction::RequireCombatAction(action_kind),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TargetObject),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && builder.last_group_filter.is_some() =>
                    {
                        let obj = builder
                            .last_group_filter
                            .ok_or("oracle:unsupported-combat-mod")?;
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if low.contains("goad") {
                if matches!(
                    subject.as_ref(),
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(
                            f.controller,
                            Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                        ) && f.any_types == AstCardTypeMask::CREATURE
                            && f.forbidden_types.is_empty()
                            && f.subtypes.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.flags.is_empty()
                ) {
                    builder
                        .instructions
                        .push(Instruction::GoadOtherPlayersCreatures);
                    return Ok(());
                }
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::GoadTarget);
                return Ok(());
            }
            if low.contains("attacks this turn if able") || low == "must_attack" {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::RequireTargetAttack);
                return Ok(());
            }
            if (low.contains("must block this turn if able")
                || low.contains("blocks this turn if able"))
                && matches!(
                    subject.as_ref(),
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(
                            f.controller,
                            Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                        ) && f.any_types == AstCardTypeMask::CREATURE
                )
            {
                builder
                    .instructions
                    .push(Instruction::RequireOpponentsCreaturesBlockThisTurn);
                return Ok(());
            }
            if low.contains("all creatures able to block")
                && low.contains("do so")
                && let Ok(bt) = lower_target_subject(subject, vocabulary)
            {
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::RequireBlocksAgainstTargetThisTurn);
                return Ok(());
            }
            let mut rule_for_br = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            let mut br_dur = dur;
            for (suffix, implied_dur) in [
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
                if let Some(stripped) = rule_for_br.strip_suffix(suffix) {
                    rule_for_br = stripped.trim().to_owned();
                    if br_dur == ResolvedDuration::NoStatedDuration
                        || (br_dur == ResolvedDuration::UntilEndOfTurn
                            && implied_dur != ResolvedDuration::UntilEndOfTurn)
                    {
                        br_dur = implied_dur;
                    }
                    break;
                }
            }
            if let Some(br) = lower_combat_mod_blocking_restriction(&rule_for_br, vocabulary)
                && (!matches!(
                    br,
                    BlockingRestriction::CannotBlockSource
                        | BlockingRestriction::MustBlockSource
                        | BlockingRestriction::CannotBeBlockedBySource
                ) || self_pronoun_valid)
            {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: br_dur,
                            });
                        return Ok(());
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
                                action: PermanentTargetAction::Modify {
                                    parts: ResolvedPartsSpec(vec![
                                        ResolvedPartSpec::BlockingRestriction(br),
                                    ]),
                                    duration: br_dur,
                                },
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
                                action: PermanentTargetAction::Modify {
                                    parts: ResolvedPartsSpec(vec![
                                        ResolvedPartSpec::BlockingRestriction(br),
                                    ]),
                                    duration: br_dur,
                                },
                            });
                        return Ok(());
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
                                action: PermanentTargetAction::Modify {
                                    parts: ResolvedPartsSpec(vec![
                                        ResolvedPartSpec::BlockingRestriction(br),
                                    ]),
                                    duration: br_dur,
                                },
                            });
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) => {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(obj)),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: br_dur,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if filter.zone.is_none() => {
                        let obj = lower_object_filter(filter, vocabulary)?;
                        builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
                        builder
                            .instructions
                            .push(Instruction::ModifyAnyNumberOfPermanents {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: br_dur,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && builder.last_group_filter.is_some() =>
                    {
                        let obj = builder
                            .last_group_filter
                            .ok_or("oracle:unsupported-combat-mod")?;
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(obj)),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Err("oracle:unsupported-combat-mod")
        }
        InstructionAst::Investigate {
            player: PlayerRefAst::You,
            count,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let def = predefined_token_def("clue", vocabulary, ids)?;
            builder.instructions.push(Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    enters_tapped: false,
                    definition: def,
                    amount: amt,
                    base_size: None,
                    recipients: TokenRecipients::Controller,
                }],
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
            let src_zone = match f.zone {
                Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => SourceZoneFilter::Hand,
                Some(ZoneFilter::YourGraveyard) => SourceZoneFilter::Graveyard,
                Some(ZoneFilter::OpponentGraveyard) => SourceZoneFilter::OpponentGraveyard,
                Some(ZoneFilter::AnyGraveyard) => SourceZoneFilter::AnyGraveyard,
                Some(ZoneFilter::Exile) => SourceZoneFilter::Exile,
                Some(ZoneFilter::Battlefield) | None => SourceZoneFilter::Battlefield,
                _ => return Err("oracle:unsupported-move-counted-zone"),
            };
            let mut inner = f.as_ref().clone();
            inner.zone = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            let player = match actor {
                Some(p) => lower_resolving_player_in_context(*p, builder, in_trigger)?,
                None => ResolvingPlayer::Controller,
            };
            let (min, max) = if *any_number {
                (
                    ValueExpr::literal(0),
                    ValueExpr::literal(i64::from(u16::MAX)),
                )
            } else if let Some(cnt_ast) = count.as_deref() {
                let c = lower_value_expr(resolve_x(cnt_ast, bound_x), vocabulary)?;
                let min_v = if *up_to {
                    ValueExpr::literal(0)
                } else {
                    c.clone()
                };
                (min_v, c)
            } else {
                return Err("oracle:unsupported-move-counted-count");
            };
            builder
                .instructions
                .push(Instruction::SelectZoneCardsToZone {
                    player,
                    source_zone: src_zone,
                    filter: Box::new(obj),
                    min,
                    max,
                    destination: subj_dest,
                    reveal: false,
                });
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
                if let Some(Instruction::CopyStackObject {
                    may_choose_new_targets,
                    ..
                }) = builder.instructions.last_mut()
                {
                    *may_choose_new_targets = true;
                    return Ok(());
                }
                if let Some(last @ Instruction::CopySpell) = builder.instructions.last_mut() {
                    *last = Instruction::CopyStackObject {
                        target: StackCopyTarget::TargetSpell,
                        count: ValueExpr::literal(1),
                        may_choose_new_targets: true,
                    };
                    return Ok(());
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
        InstructionAst::Copy {
            subject,
            exception: None,
            count,
        } => {
            let count_expr = match count {
                Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary)?,
                None => ValueExpr::literal(1),
            };
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.any_spell
                    || filter.is_ability
                    || filter.zone == Some(ZoneFilter::Stack) =>
                {
                    let bt = lower_target_subject(subject, vocabulary)?;
                    if !is_stack_body_target(bt) {
                        return Err("oracle:unsupported-copy");
                    }
                    builder.bind_target(bt)?;
                    let copy_target = if filter.any_spell && filter.is_ability {
                        StackCopyTarget::TargetSpellOrAbility
                    } else if filter.is_ability {
                        StackCopyTarget::TargetAbility
                    } else {
                        StackCopyTarget::TargetSpell
                    };
                    builder.instructions.push(Instruction::CopyStackObject {
                        target: copy_target,
                        count: count_expr,
                        may_choose_new_targets: false,
                    });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::CopyStackObject {
                        target: StackCopyTarget::TriggeringSpell,
                        count: count_expr,
                        may_choose_new_targets: false,
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw) => {
                    let low_raw = raw.trim().to_ascii_lowercase();
                    let copy_target = if matches!(builder.target, BodyTarget::SingleSpell(_)) {
                        StackCopyTarget::TargetSpell
                    } else if is_stack_body_target(builder.target) {
                        if low_raw.contains("ability") && !low_raw.contains("spell") {
                            StackCopyTarget::TargetAbility
                        } else if low_raw.contains("spell") && !low_raw.contains("ability") {
                            StackCopyTarget::TargetSpell
                        } else {
                            StackCopyTarget::TargetSpellOrAbility
                        }
                    } else if in_trigger {
                        if low_raw.contains("ability") {
                            StackCopyTarget::TriggeringAbility
                        } else {
                            StackCopyTarget::TriggeringSpell
                        }
                    } else if !self_pronoun_valid && matches!(low_raw.as_str(), "it" | "this spell")
                    {
                        StackCopyTarget::ResolvingSpell
                    } else {
                        return Err("oracle:unsupported-copy");
                    };
                    builder.instructions.push(Instruction::CopyStackObject {
                        target: copy_target,
                        count: count_expr,
                        may_choose_new_targets: false,
                    });
                    Ok(())
                }
                SubjectRefAst::SelfObject => {
                    let copy_target = if !in_trigger && !self_pronoun_valid {
                        StackCopyTarget::ResolvingSpell
                    } else if in_trigger {
                        StackCopyTarget::TriggeringSpell
                    } else {
                        return Err("oracle:unsupported-copy");
                    };
                    builder.instructions.push(Instruction::CopyStackObject {
                        target: copy_target,
                        count: count_expr,
                        may_choose_new_targets: false,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-copy"),
            }
        }
        InstructionAst::MayChooseNewTargetsForCopy => match builder.instructions.last_mut() {
            Some(Instruction::CopyStackObject {
                may_choose_new_targets,
                ..
            }) => {
                *may_choose_new_targets = true;
                Ok(())
            }
            Some(last @ Instruction::CopySpell) => {
                *last = Instruction::CopyStackObject {
                    target: StackCopyTarget::TargetSpell,
                    count: ValueExpr::literal(1),
                    may_choose_new_targets: true,
                };
                Ok(())
            }
            Some(last @ Instruction::CopyAbility) => {
                *last = Instruction::CopyStackObject {
                    target: StackCopyTarget::TargetAbility,
                    count: ValueExpr::literal(1),
                    may_choose_new_targets: true,
                };
                Ok(())
            }
            _ => Err("oracle:unsupported-choose-new-targets-for-copy"),
        },
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
        InstructionAst::RedirectDamage {
            amount,
            from,
            to,
            source_choice: None,
            duration: None,
        } => {
            let has_source = in_trigger || self_pronoun_valid;
            let amt_opt = match resolve_x(amount, bound_x) {
                ValueExprAst::PriorResult => None,
                ValueExprAst::Const(n) if *n > 0 => Some(i64::from(*n).into()),
                _ => return Err("oracle:unsupported-redirect-damage"),
            };
            let from_rec = lower_redirect_from_subject(from, has_source, vocabulary)
                .ok_or("oracle:unsupported-redirect-damage")?;
            let to_dst = lower_redirect_to_subject(to, builder, has_source, vocabulary)?;
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                    source: None,
                    from: from_rec,
                    to: to_dst,
                    combat_only: false,
                    amount: amt_opt,
                    one_shot: false,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            Ok(())
        }
        InstructionAst::RemoveFromCombat { subject } => {
            let bt = lower_target_subject(subject, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::RemoveTargetFromCombat);
            Ok(())
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:player-counter-kind")?;
            if !ck.is_player_counter() {
                return Err("oracle:player-counter-kind");
            }
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if matches!(player, PlayerRefAst::You)
                && ck == CounterKind::Experience
                && amt == ValueExpr::literal(1)
            {
                builder.instructions.push(Instruction::AddExperienceCounter);
                return Ok(());
            }
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::AddPlayerCounter {
                players,
                kind: ck,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::EndTurn => {
            builder.instructions.push(Instruction::EndControllerTurn);
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
                        Some(expr) => lower_value_expr(expr, vocabulary)?,
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
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            if let CostAtomAst::PayMana(raw) = cost {
                let idx = builder.instructions.len();
                if let Some(mc) = parse_fixed_mana_cost(raw) {
                    builder.instructions.push(Instruction::OfferManaPayment {
                        payer: rp,
                        cost: mc,
                        paid_destination: idx + 1,
                        declined_destination: idx + 1,
                    });
                    return Ok(());
                } else if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
                    && x_cnt > 0
                {
                    let x_val = match bound_x {
                        Some(expr) => lower_value_expr(expr, vocabulary)?,
                        None => ValueExpr::chosen_x(),
                    };
                    let extra_generic = scale_value_expr(x_val, i64::from(x_cnt));
                    builder
                        .instructions
                        .push(Instruction::OfferDynamicManaPayment {
                            payer: rp,
                            fixed,
                            extra_generic,
                            paid_destination: idx + 1,
                            declined_destination: idx + 1,
                        });
                    return Ok(());
                }
                return Err("oracle:unsupported-pay-cost");
            }
            let ops = lower_resolution_pay_cost_ops(rp, cost, vocabulary, bound_x)?;
            builder.instructions.extend(ops);
            Ok(())
        }
        InstructionAst::Exert { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::ExertAbilitySource);
            Ok(())
        }
        InstructionAst::CollectEvidence { value } => {
            let min = lower_value_expr(resolve_x(value, bound_x), vocabulary)?;
            builder.instructions.push(Instruction::CollectEvidence {
                player: ResolvingPlayer::Controller,
                minimum: min,
            });
            Ok(())
        }
        InstructionAst::Blight { player, count } => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder.instructions.push(Instruction::Blight {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Choice(branches) if branches.len() >= 2 => {
            let mut options = Vec::with_capacity(branches.len());
            for (idx, branch) in branches.iter().enumerate() {
                let mut sub = BodyBuilder::new();
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
            Err("oracle:unsupported-instruction")
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
            if chooser_ref == PlayerRefAst::You && cleaned_choice.starts_with("target ") {
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
            let kind = parse_choice_kind(cleaned_choice, true, vocabulary)?;
            emit_choice_for_player_ref(chooser_ref, kind, builder, ids, in_trigger)
        }
        InstructionAst::Incubate { count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder
                .instructions
                .push(Instruction::Incubate { count: amt });
            Ok(())
        }
        InstructionAst::Connive { subject, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let subj = lower_move_subject_ref(
                subject,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder.instructions.push(Instruction::Connive {
                subject: subj,
                count: amt,
            });
            Ok(())
        }
        InstructionAst::Discover { value } => {
            let amt = lower_value_expr(resolve_x(value, bound_x), vocabulary)?;
            builder
                .instructions
                .push(Instruction::Discover { value: amt });
            Ok(())
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
        InstructionAst::VentureIntoDungeon { player } => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::VentureIntoDungeon {
                player: rp,
                undercity: false,
            });
            Ok(())
        }
        InstructionAst::TakeInitiative { player } => {
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
        InstructionAst::Support { count } => {
            let ValueExprAst::Const(n) = resolve_x(count, bound_x) else {
                return Err("oracle:non-const-support");
            };
            let max = u16::try_from(*n).map_err(|_| "oracle:non-const-support")?;
            if max == 0 {
                return Err("oracle:non-const-support");
            }
            if in_trigger && self_pronoun_valid {
                builder.bind_target(BodyTarget::SingleCreature)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: max,
                        action: PermanentTargetAction::PutPlusOneCounterOnOthers,
                    });
            } else if self_pronoun_valid {
                builder.bind_target(BodyTarget::FilteredPermanent(ObjectFilter::Clause(
                    ObjectClause {
                        source_relation: SourceRelation::OtherThanAbilitySource,
                        ..ObjectClause::creature()
                    },
                )))?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: max,
                        action: PermanentTargetAction::PutPlusOneCounter,
                    });
            } else {
                builder.bind_target(BodyTarget::SingleCreature)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: max,
                        action: PermanentTargetAction::PutPlusOneCounter,
                    });
            }
            Ok(())
        }
        InstructionAst::Endure { count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let ValueExpr::Literal(lit_amt) = &amt else {
                return Err("oracle:non-literal-endure");
            };
            let spirit = vocabulary
                .resolve_label("Spirit")
                .ok_or("oracle:token-unknown-subtype")?;
            let mut spirit_prog =
                Program::creature(None, 0.into(), 0.into(), BTreeSet::from([spirit]));
            spirit_prog.effect_defined_colors = Some(BTreeSet::from([Color::White]));
            let spirit_def = TokenDefinition::new(
                spirit_prog,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Creature(spirit)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:token-def-invalid")?;
            let counter_branch = ChooseOptionBranch {
                label: "Put +1/+1 counters on this creature".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSource {
                        amount: lit_amt.clone(),
                    }],
                ),
            };
            let token_branch = ChooseOptionBranch {
                label: "Create a white Spirit creature token".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateTokens {
                        creations: vec![TokenCreation {
                            enters_tapped: false,
                            definition: spirit_def,
                            amount: ValueExpr::literal(1),
                            base_size: Some(TokenSize::AtCreation(amt)),
                            recipients: TokenRecipients::Controller,
                        }],
                    }],
                ),
            };
            builder.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options: vec![counter_branch, token_branch],
            });
            Ok(())
        }
        InstructionAst::ExileTopLibrary {
            actor: None | Some(PlayerRefAst::You),
            whose,
            count,
            face_down,
        } => {
            let w = whose.trim().to_ascii_lowercase();
            let (req_target, library_owner) = match w.as_str() {
                "your" | "own" | "" => (None, AffectedPlayers::One(ResolvingPlayer::Controller)),
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
                "defending player" | "defending player's" => {
                    (None, AffectedPlayers::One(ResolvingPlayer::DefendingPlayer))
                }
                "enchanted player" | "enchanted player's" => {
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
            let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if !*face_down
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
            let amt = lower_value_expr(resolve_x(draw, bound_x), vocabulary)?;
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
            builder
                .instructions
                .push(Instruction::ShuffleAbilitySourceIntoLibrary);
            Ok(())
        }
        InstructionAst::ShuffleIntoLibrary { subject } => match subject.as_ref() {
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
        InstructionAst::AbandonSourceScheme => Err("oracle:scheme-excluded-from-constructed"),
        InstructionAst::DoubleStatOrCounters {
            subject,
            kind,
            duration,
        } => {
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && !items.is_empty()
            {
                for it in items {
                    lower_instruction(
                        &InstructionAst::DoubleStatOrCounters {
                            subject: Box::new(it.clone()),
                            kind: kind.clone(),
                            duration: duration.clone(),
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
            let low = kind.trim().to_ascii_lowercase();
            if matches!(low.as_str(), "life" | "life total" | "life_total") {
                let rp = extract_resolving_player_from_subject(subject, builder, in_trigger)
                    .map_err(|_| "oracle:unsupported-double-stat")?;
                if rp == ResolvingPlayer::Controller {
                    builder.instructions.push(Instruction::GainControllerLife {
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    });
                } else {
                    builder.instructions.push(Instruction::AdjustPlayersLife {
                        players: AffectedPlayers::One(rp),
                        gain: true,
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp)),
                    });
                }
                return Ok(());
            }
            let counter_double_kind: Option<Option<CounterKind>> = if matches!(
                low.as_str(),
                "counters" | "all_counters" | "each kind of counter" | "each counter"
            ) {
                Some(None)
            } else if let Some(rest) = low.strip_prefix("counters:") {
                let r = rest.trim();
                let r = r
                    .strip_suffix(" counters")
                    .or_else(|| r.strip_suffix(" counter"))
                    .unwrap_or(r)
                    .trim();
                if matches!(r, "" | "all" | "each" | "each kind" | "each kind of") {
                    Some(None)
                } else {
                    parse_counter_kind(r)
                        .filter(|ck| !ck.is_player_counter())
                        .map(Some)
                }
            } else if let Some(ck_raw) = low
                .strip_suffix(" counters")
                .or_else(|| low.strip_suffix(" counter"))
            {
                parse_counter_kind(ck_raw.trim())
                    .filter(|ck| !ck.is_player_counter())
                    .map(Some)
            } else {
                None
            };
            if let Some(ck_filter) = counter_double_kind {
                if ck_filter.is_none()
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } = subject.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::DoubleCountersOnTargets);
                    return Ok(());
                }
                if ck_filter.is_none()
                    && builder.instructions.is_empty()
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } = subject.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::DoubleCountersOnTargets);
                    return Ok(());
                }
                if ck_filter.is_none()
                    && builder.instructions.is_empty()
                    && !in_trigger
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::ChosenX,
                    } = subject.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder.x_target_count = true;
                    builder
                        .instructions
                        .push(Instruction::DoubleCountersOnTargets);
                    return Ok(());
                }
                let eff_subj = match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        EffectSubject::AbilitySource
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        EffectSubject::CreatedTokens
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
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        EffectSubject::LastMoved
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                            && builder.target == BodyTarget::None =>
                    {
                        EffectSubject::TriggerSubject
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        EffectSubject::Matching(group_obj)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder.last_group_filter = Some(obj);
                        EffectSubject::Matching(obj)
                    }
                    _ => return Err("oracle:unsupported-double-stat"),
                };
                builder
                    .instructions
                    .push(Instruction::DoubleCountersOnSubject {
                        subject: Box::new(eff_subj),
                        kind: ck_filter,
                    });
                return Ok(());
            }
            let (power, toughness) = match low.as_str() {
                "power" => (true, false),
                "toughness" => (false, true),
                "power and toughness" | "power_and_toughness" => (true, true),
                _ => return Err("oracle:unsupported-double-stat"),
            };
            let dur = lower_duration(duration)?;
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: true,
                            power,
                            toughness,
                            duration: dur,
                        });
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
                        .push(Instruction::DoublePowerToughness {
                            target_self: false,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::CreatedTokens),
                            power,
                            toughness,
                            duration: dur,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: false,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            power,
                            toughness,
                            duration: dur,
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
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::Matching(group_obj)),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    builder.last_group_filter = Some(obj);
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-double-stat"),
            }
        }
        InstructionAst::Attach {
            subject,
            target,
            any_number: false,
            all: false,
        } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                if in_trigger
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } = target.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    if bt == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()) {
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
            let att_ref = match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    Some(MoveSubjectRef::AbilitySource)
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    Some(MoveSubjectRef::TargetObject)
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    Some(MoveSubjectRef::TargetObject)
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    Some(MoveSubjectRef::TriggeringObject)
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    Some(MoveSubjectRef::TriggeringObject)
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    Some(MoveSubjectRef::AttachedHost)
                }
                _ => None,
            };
            let host_ref = match target.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    Some(MoveSubjectRef::AbilitySource)
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    Some(MoveSubjectRef::TargetObject)
                }
                SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    Some(MoveSubjectRef::TargetObject)
                }
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    Some(MoveSubjectRef::TargetObject)
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    Some(MoveSubjectRef::TargetObject)
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    Some(MoveSubjectRef::TriggeringObject)
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    Some(MoveSubjectRef::TriggeringObject)
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    Some(MoveSubjectRef::AttachedHost)
                }
                _ => None,
            };
            if let (Some(attachment), Some(host)) = (att_ref, host_ref) {
                builder
                    .instructions
                    .push(Instruction::AttachPermanent { attachment, host });
                return Ok(());
            }
            Err("oracle:unsupported-attach")
        }
        InstructionAst::DealDamageDivided {
            source,
            amount,
            targets_desc,
            divider: None,
        } => {
            let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
            let desc_low = targets_desc.trim().to_ascii_lowercase();
            if is_self_pronoun(source, builder, self_pronoun_valid)
                && let ValueExpr::Literal(n) = &amt
                && let Some(total) = n.to_usize().and_then(|v| u16::try_from(v).ok())
                && total > 0
            {
                let bt = if desc_low.contains("creatures you don't control")
                    || desc_low.contains("creatures an opponent controls")
                {
                    Some(BodyTarget::FilteredPermanent(
                        ObjectFilter::creature().opposing(),
                    ))
                } else if desc_low.contains("creatures and/or planeswalkers")
                    || desc_low.contains("creatures or planeswalkers")
                {
                    Some(BodyTarget::FilteredPermanent(ObjectFilter::Clause(
                        ObjectClause {
                            any_types: CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
                            ..ObjectClause::any()
                        },
                    )))
                } else if desc_low.contains("attacking creatures") {
                    Some(BodyTarget::FilteredPermanent(
                        ObjectFilter::creature().attacking(),
                    ))
                } else if desc_low.contains("blocking creatures") {
                    Some(BodyTarget::FilteredPermanent(ObjectFilter::Clause(
                        ObjectClause {
                            combat: CombatFilter::Blocking,
                            ..ObjectClause::creature()
                        },
                    )))
                } else if desc_low.contains("players and/or planeswalkers")
                    || desc_low.contains("opponents and/or planeswalkers")
                {
                    Some(BodyTarget::SinglePlayerOrPlaneswalker)
                } else if desc_low.contains("creatures and/or players")
                    || desc_low.contains("any target")
                    || desc_low.ends_with(" targets")
                    || desc_low == "targets"
                {
                    Some(BodyTarget::SingleAnyTarget)
                } else if desc_low.contains("creature") {
                    Some(BodyTarget::SingleCreature)
                } else {
                    None
                };
                if let Some(bt) = bt {
                    builder.bind_target(bt)?;
                    if in_trigger && total == 3 && bt == BodyTarget::SingleAnyTarget {
                        builder
                            .instructions
                            .push(Instruction::DivideThreeDamageAmongTargets);
                    } else {
                        builder
                            .instructions
                            .push(Instruction::DivideDamageAmongTargets { total });
                    }
                    return Ok(());
                }
            }
            Err("oracle:unsupported-divided-damage")
        }
        InstructionAst::DelayedTrigger { step, scope, body } => {
            let own_step_only = match scope {
                DelayedStepScope::Next => false,
                DelayedStepScope::YourNext => true,
                DelayedStepScope::ThatPlayersNext
                | DelayedStepScope::Each
                | DelayedStepScope::EachPlayers
                | DelayedStepScope::EachOpponents
                | DelayedStepScope::EachOfYour => {
                    return Err("oracle:unsupported-delayed-scope");
                }
            };
            let eng_step = match step {
                TurnStepAst::Upkeep => Step::Upkeep,
                TurnStepAst::DrawStep => Step::Draw,
                TurnStepAst::PrecombatMain | TurnStepAst::MainPhase => Step::PrecombatMain,
                TurnStepAst::PostcombatMain => Step::PostcombatMain,
                TurnStepAst::BeginningOfCombat => Step::BeginningCombat,
                TurnStepAst::EndOfCombat => Step::EndCombat,
                TurnStepAst::EndStep => Step::End,
                TurnStepAst::UntapStep | TurnStepAst::CleanupStep => {
                    return Err("oracle:unsupported-delayed-step");
                }
            };
            let delayed_body = lower_effect_body(body, vocabulary, ids, true, self_pronoun_valid)?;
            builder
                .instructions
                .push(Instruction::ScheduleDelayedStepTrigger {
                    step: eng_step,
                    own_step_only,
                    body: Box::new(delayed_body),
                });
            Ok(())
        }
        InstructionAst::ReflexiveTrigger(body) => {
            let reflexive_body =
                lower_effect_body(body, vocabulary, ids, true, self_pronoun_valid)?;
            builder
                .instructions
                .push(Instruction::EmitReflexiveTrigger {
                    body: Box::new(reflexive_body),
                });
            Ok(())
        }
        InstructionAst::RemoveKeywords {
            subject,
            keywords,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let suppressed = lower_keyword_list(keywords)?;
            if suppressed.is_empty() {
                return Err("oracle:empty-remove-keywords");
            }
            let ab_change = AbilityChange {
                suppressed_keywords: suppressed,
                ..AbilityChange::default()
            };
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-remove-keywords"),
            }
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_cost: Some(CostAtomAst::PayMana(raw)),
            unless_action: None,
            unless_condition: None,
        } => {
            let player_ast = unless_player.unwrap_or(PlayerRefAst::You);
            let mut probed_body: Option<BodyBuilder> = None;
            if builder.target == BodyTarget::None {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
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
        InstructionAst::TemporaryStatic {
            static_ability,
            duration,
        } => {
            match static_ability.as_ref() {
                StaticAbilityAst::Compound(items) if !items.is_empty() => {
                    for item in items {
                        lower_instruction(
                            &InstructionAst::TemporaryStatic {
                                static_ability: Box::new(item.clone()),
                                duration: duration.clone(),
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
                StaticAbilityAst::ContinuousModifyPT {
                    subject,
                    dx,
                    dy,
                    grant_keywords,
                    grant_ability_text,
                    scale,
                    condition: None,
                    paired_rule,
                } => {
                    let mut seq = vec![InstructionAst::ModifyPT {
                        subject: subject.clone(),
                        dx: dx.clone(),
                        dy: dy.clone(),
                        duration: duration.clone(),
                        scale: scale.clone(),
                    }];
                    if !grant_keywords.is_empty() {
                        seq.push(InstructionAst::GrantKeywords {
                            subject: subject.clone(),
                            keywords: grant_keywords.clone(),
                            duration: duration.clone(),
                        });
                    }
                    if let Some(raw_text) = grant_ability_text {
                        let cleaned = raw_text.trim().trim_matches('"').trim();
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let synthetic = compile_card(
                            &CardInput {
                                oracle_id: "",
                                name: "Synthetic",
                                layout: "normal",
                                faces: vec![FaceInput {
                                    name: "Synthetic",
                                    type_line: "Creature",
                                    oracle_text: cleaned,
                                }],
                            },
                            &compiler_vocab,
                        );
                        let compiled = if synthetic.strictly_compiled && synthetic.faces.len() == 1
                        {
                            Some(Box::new(synthetic.faces[0].clone()))
                        } else {
                            None
                        };
                        seq.push(InstructionAst::GrantAbilityText {
                            subject: subject.clone(),
                            ability_text: raw_text.clone(),
                            compiled,
                            duration: duration.clone(),
                        });
                    }
                    if let Some(paired) = paired_rule {
                        seq.push(InstructionAst::TemporaryStatic {
                            static_ability: paired.clone(),
                            duration: duration.clone(),
                        });
                    }
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
                StaticAbilityAst::GrantKeywords {
                    subject,
                    keywords,
                    condition: None,
                    paired_rule,
                } => {
                    let mut seq = vec![InstructionAst::GrantKeywords {
                        subject: subject.clone(),
                        keywords: keywords.clone(),
                        duration: duration.clone(),
                    }];
                    if let Some(paired) = paired_rule {
                        seq.push(InstructionAst::TemporaryStatic {
                            static_ability: paired.clone(),
                            duration: duration.clone(),
                        });
                    }
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
                StaticAbilityAst::ContinuousLayerEffect {
                    subject,
                    set_types,
                    additive_types,
                    base_pt,
                    set_colors,
                    grant_keywords,
                    lose_keywords,
                    lose_all_abilities,
                    condition: None,
                } => {
                    let dur = lower_duration(duration)?;
                    let parts = lower_continuous_layer_effect_parts(
                        set_types.as_deref(),
                        *additive_types,
                        base_pt.as_ref(),
                        set_colors.as_deref(),
                        grant_keywords,
                        lose_keywords,
                        *lose_all_abilities,
                        vocabulary,
                        bound_x,
                    )?;
                    return emit_resolved_parts_for_subject(
                        subject,
                        ResolvedPartsSpec(parts),
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    );
                }
                StaticAbilityAst::SetOrAddType {
                    subject,
                    type_or_subtype,
                    additive,
                    condition: None,
                } => {
                    return lower_animate_or_set_pt(
                        subject,
                        "",
                        "",
                        Some(type_or_subtype),
                        &[],
                        duration,
                        *additive,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only: true,
                    noncombat_only: false,
                    ..
                } => {
                    let dur = lower_duration(duration)?;
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::PreventAllCombatDamage),
                        duration: dur,
                    });
                    return Ok(());
                }
                StaticAbilityAst::PlayOrCastPermission {
                    permission,
                    filter: _,
                    cast_only: false,
                    condition: None,
                } if {
                    let p = permission.trim().trim_end_matches('.').to_ascii_lowercase();
                    p.contains("additional land") && p.contains("this turn")
                } =>
                {
                    let dur = lower_duration(duration)?;
                    let p = permission.trim().to_ascii_lowercase();
                    let count = if p.contains("two additional") {
                        2
                    } else if p.contains("three additional") {
                        3
                    } else {
                        1
                    };
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::AdditionalLandPlay {
                            player: ResolvingPlayer::Controller,
                            count,
                        }),
                        duration: dur,
                    });
                    return Ok(());
                }
                StaticAbilityAst::GrantAbilityText {
                    subject,
                    ability_text,
                    compiled,
                    condition: None,
                } => {
                    return lower_instruction(
                        &InstructionAst::GrantAbilityText {
                            subject: subject.clone(),
                            ability_text: ability_text.clone(),
                            compiled: compiled.clone(),
                            duration: duration.clone(),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                StaticAbilityAst::CostModifier {
                    subject,
                    delta,
                    less: false,
                    next_only: false,
                    per: None,
                    scale_expr: None,
                    condition: None,
                } => {
                    let dur = lower_duration(duration)?;
                    let filter_opt = match subject.as_ref() {
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f) => Some((
                            match f.controller {
                                Some(
                                    ControllerRelation::Opponent | ControllerRelation::Opponents,
                                ) => AffectedPlayers::EachOpponent,
                                Some(ControllerRelation::You) => {
                                    AffectedPlayers::One(ResolvingPlayer::Controller)
                                }
                                _ => AffectedPlayers::EachPlayer,
                            },
                            lower_object_filter(f, vocabulary)?,
                        )),
                        _ => None,
                    };
                    if let Some((players, obj)) = filter_opt
                        && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                        && hybrids.is_empty()
                        && mc.required.is_empty()
                        && let Some(generic) =
                            mc.generic.to_usize().and_then(|v| u16::try_from(v).ok())
                        && generic > 0
                    {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::SpellCostIncrease {
                                players,
                                filter: obj,
                                generic,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                }
                StaticAbilityAst::CostModifier {
                    subject,
                    delta,
                    less: true,
                    next_only,
                    per,
                    scale_expr,
                    condition: None,
                } => {
                    let dur = lower_duration(duration)?;
                    let filter_opt = match subject.as_ref() {
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f) => Some(lower_object_filter(f, vocabulary)?),
                        _ => None,
                    };
                    let next_only = *next_only;
                    if let Some(obj) = filter_opt {
                        if per.is_none()
                            && scale_expr.is_none()
                            && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                            && hybrids.is_empty()
                            && mc.required.is_empty()
                            && let Some(generic) =
                                mc.generic.to_usize().and_then(|v| u16::try_from(v).ok())
                            && generic > 0
                        {
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReduction {
                                    filter: obj,
                                    generic,
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                        if let Some(scale) = scale_expr
                            && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                            && hybrids.is_empty()
                            && mc.required.is_empty()
                            && let Some(unit) =
                                mc.generic.to_usize().and_then(|v| i64::try_from(v).ok())
                            && unit > 0
                        {
                            let base_val = lower_value_expr(resolve_x(scale, bound_x), vocabulary)?;
                            let amount = scale_value_expr(base_val, unit);
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                    filter: obj,
                                    amount: Box::new(amount),
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                        if let Some(per_filter) = per
                            && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                            && hybrids.is_empty()
                            && mc.required.is_empty()
                            && let Some(unit) =
                                mc.generic.to_usize().and_then(|v| i64::try_from(v).ok())
                            && unit > 0
                            && let Ok(base_val) = (match per_filter.zone {
                                None | Some(ZoneFilter::Battlefield) => {
                                    let mut pf = per_filter.as_ref().clone();
                                    pf.zone = None;
                                    lower_object_filter(&pf, vocabulary)
                                        .map(ValueExpr::count_battlefield)
                                }
                                Some(ZoneFilter::YourGraveyard) => {
                                    let mut pf = per_filter.as_ref().clone();
                                    pf.zone = None;
                                    lower_object_filter(&pf, vocabulary).map(|of| {
                                        ValueExpr::count_graveyard(ResolvingPlayer::Controller, of)
                                    })
                                }
                                _ => Err("oracle:unsupported-per-zone"),
                            })
                        {
                            let amount = scale_value_expr(base_val, unit);
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                    filter: obj,
                                    amount: Box::new(amount),
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                        if per.is_none()
                            && scale_expr.is_none()
                            && let Some((fixed, x_cnt)) = parse_mana_cost_with_x(delta)
                            && x_cnt > 0
                            && fixed.required.is_empty()
                        {
                            let x_val = match bound_x {
                                Some(expr) => lower_value_expr(expr, vocabulary)?,
                                None => ValueExpr::chosen_x(),
                            };
                            let mut amount = scale_value_expr(x_val, i64::from(x_cnt));
                            if fixed.generic.is_positive() {
                                amount = ValueExpr::Add(
                                    Box::new(amount),
                                    Box::new(ValueExpr::Literal(fixed.generic)),
                                );
                            }
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                    filter: obj,
                                    amount: Box::new(amount),
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                    }
                }
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject,
                    rule,
                    filter,
                    condition: None,
                } => {
                    let dur = lower_duration(duration)?;
                    let rule_low = rule.trim().to_ascii_lowercase();
                    if rule_low.contains("can't be countered") || rule_low == "cant_be_countered" {
                        let flt = filter
                            .as_deref()
                            .map(|f| lower_object_filter(f, vocabulary))
                            .transpose()?;
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantBeCountered {
                                filter: flt,
                                controller_only: true,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if rule_low.contains("can't gain life") || rule_low == "cant_gain_life" {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            None => AffectedPlayers::EachPlayer,
                            _ => AffectedPlayers::EachPlayer,
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::PlayerCantGainLife { players }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if rule_low.contains("damage can't be prevented")
                        || rule_low == "damage_cant_be_prevented"
                    {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::DamageCantBePrevented),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if rule_low.contains("can't cast") || rule_low == "cant_cast_spells" {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            None => AffectedPlayers::EachOpponent,
                            _ => AffectedPlayers::EachPlayer,
                        };
                        let flt = filter
                            .as_deref()
                            .map(|f| lower_object_filter(f, vocabulary))
                            .transpose()?;
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantCastSpells {
                                players,
                                filter: flt,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if let Some(subj) = subject.as_deref()
                        && let Some(type_part) = parse_type_removal_part(&rule_low, vocabulary)
                    {
                        return emit_resolved_parts_for_subject(
                            subj,
                            ResolvedPartsSpec(vec![type_part]),
                            dur,
                            builder,
                            vocabulary,
                            in_trigger,
                            self_pronoun_valid,
                        );
                    }
                    if is_temp_menace_blocking_rule(rule, &rule_low, filter.as_deref()) {
                        let mut ab = AbilityChange::default();
                        ab.granted_keywords.insert(KeywordAbility::Menace);
                        let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]);
                        match subject.as_deref() {
                            None if self_pronoun_valid => {
                                builder.instructions.push(Instruction::ChangeAbilitySource {
                                    parts,
                                    duration: dur,
                                });
                                return Ok(());
                            }
                            Some(subj) => {
                                return emit_resolved_parts_for_subject(
                                    subj,
                                    parts,
                                    dur,
                                    builder,
                                    vocabulary,
                                    in_trigger,
                                    self_pronoun_valid,
                                );
                            }
                            _ => {}
                        }
                    }
                }
                StaticAbilityAst::PlayOrCastPermission {
                    permission,
                    filter,
                    cast_only,
                    condition: None,
                } => {
                    let p_low = permission.trim().trim_end_matches('.').to_ascii_lowercase();
                    if matches!(
                        p_low.as_str(),
                        "flash" | "as though they had flash" | "cast_as_though_flash"
                    ) {
                        if matches!(duration, DurationAst::UntilEndOfTurn)
                            && is_unrestricted_filter(filter)
                        {
                            builder
                                .instructions
                                .push(Instruction::GrantControllerFlashUntilCleanup);
                            return Ok(());
                        }
                        let dur = lower_duration(duration)?;
                        let obj = lower_object_filter(filter, vocabulary)?;
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::FilteredFlash {
                                player: ResolvingPlayer::Controller,
                                filter: obj,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    let zone_opt = if p_low.contains("from your graveyard")
                        || p_low == "play_from_graveyard"
                        || p_low == "cast_from_graveyard"
                    {
                        Some(SourceZoneFilter::Graveyard)
                    } else if p_low.contains("from a graveyard")
                        || p_low.contains("from any graveyard")
                        || p_low.contains("from graveyards")
                    {
                        Some(SourceZoneFilter::AnyGraveyard)
                    } else if p_low.contains("from an opponent's graveyard")
                        || p_low.contains("from opponents' graveyards")
                    {
                        Some(SourceZoneFilter::OpponentGraveyard)
                    } else if p_low.contains("from exile") || p_low == "play_from_exile" {
                        Some(SourceZoneFilter::Exile)
                    } else if p_low.contains("from your hand") {
                        Some(SourceZoneFilter::Hand)
                    } else {
                        None
                    };
                    if let Some(zone) = zone_opt {
                        let dur = lower_duration(duration)?;
                        let flt = if is_unrestricted_filter(filter) {
                            None
                        } else {
                            Some(lower_object_filter(filter, vocabulary)?)
                        };
                        let free = p_low.contains("without paying its mana cost")
                            || p_low.contains("without paying their mana costs");
                        let any_color = p_low.contains("mana of any color")
                            || p_low.contains("mana of any type");
                        let once = p_low.contains("once")
                            || p_low.contains("a spell")
                            || p_low.contains("one ");
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::ZoneCastPermission {
                                player: ResolvingPlayer::Controller,
                                zone,
                                filter: flt,
                                cast_only: *cast_only,
                                free,
                                any_color,
                                once,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                }
                _ => {}
            }
            let dur = lower_duration(duration)?;
            if let StaticAbilityAst::RuleOrCombatRestriction {
                subject,
                rule,
                filter,
                condition: None,
            } = static_ability.as_ref()
                && let Some(br) =
                    lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary)
            {
                let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]);
                match subject.as_deref() {
                    None if self_pronoun_valid => {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts,
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(subj) => {
                        let mut trial = builder.clone();
                        if emit_resolved_parts_for_subject(
                            subj,
                            parts,
                            dur,
                            &mut trial,
                            vocabulary,
                            in_trigger,
                            self_pronoun_valid,
                        )
                        .is_ok()
                        {
                            *builder = trial;
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
            if let StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(subj),
                rule,
                filter: None,
                condition: None,
            } = static_ability.as_ref()
            {
                let mut trial = builder.clone();
                if lower_instruction(
                    &InstructionAst::CombatRuleMod {
                        subject: subj.clone(),
                        rule: rule.clone(),
                        duration: duration.clone(),
                    },
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
            {
                use spellwake_engine::characteristics::{
                    CastPermissionRule, LibraryTopPermission, SpellCostModifier, SpellDiscount,
                    StaticContinuousAbility, StaticDamageRecipient, StaticDamageRule,
                    StaticDrawRule, StaticModifierRule, StaticRestriction, StaticUntapRule,
                };
                let mut scratch = Program::new(
                    BTreeSet::from([CardType::Enchantment]),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    ProgramKind::Permanent(PermanentDefinition {
                        entry_tap: None,
                        attachment: None,
                        printed_pt: None,
                        casting_cost: Some(ManaCost::free()),
                    }),
                );
                let mut scratch_ids = LoweringIds::default();
                if lower_static(
                    static_ability,
                    &mut scratch,
                    vocabulary,
                    &mut scratch_ids,
                    false,
                )
                .is_ok()
                    && scratch.triggered.is_empty()
                    && scratch.activated.is_empty()
                    && scratch.mana.is_empty()
                    && scratch.zone_replacements.is_empty()
                    && scratch.attack_requirement.is_none()
                    && !scratch.cannot_be_countered
                    && !scratch.opening_hand_battlefield
                {
                    let abilities: Vec<StaticContinuousAbility> = scratch
                        .continuous
                        .into_iter()
                        .chain(scratch.additional_battlefield_abilities)
                        .collect();
                    let mut lowered_ops: Vec<Instruction> = Vec::with_capacity(abilities.len());
                    let mut all_temp_ok = !abilities.is_empty();
                    for ab in abilities {
                        match &ab {
                            StaticContinuousAbility::Battlefield {
                                selector, parts, ..
                            } => {
                                if let Some(op) = try_lower_temp_battlefield_static(
                                    selector,
                                    parts,
                                    dur,
                                    self_pronoun_valid,
                                ) {
                                    lowered_ops.push(op);
                                } else {
                                    let bf_ok = (in_trigger || self_pronoun_valid)
                                        || (!selector.attached_to_source
                                            && !selector.exclude_source
                                            && !matches!(
                                                selector.controller,
                                                ControllerSelector::SourceItself
                                                    | ControllerSelector::SelfAtMaxSpeed
                                                    | ControllerSelector::SourcePair
                                            ));
                                    if bf_ok {
                                        lowered_ops.push(Instruction::CreateTemporaryRule {
                                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(
                                                Box::new(ab),
                                            )),
                                            duration: dur,
                                        });
                                    } else {
                                        all_temp_ok = false;
                                        break;
                                    }
                                }
                            }
                            StaticContinuousAbility::DamageRule {
                                rule:
                                    StaticDamageRule::RedirectDamage {
                                        source,
                                        from,
                                        to,
                                        combat_only,
                                    },
                                ..
                            } => {
                                let ok = ((in_trigger || self_pronoun_valid)
                                    || (!matches!(
                                        from,
                                        StaticDamageRecipient::SelfObject
                                            | StaticDamageRecipient::AttachedHost
                                    ) && !matches!(
                                        to,
                                        DamageRedirectTarget::AbilitySource
                                            | DamageRedirectTarget::AttachedHost
                                    )))
                                    && (*to != DamageRedirectTarget::TargetObject
                                        || is_permanent_body_target(builder.target));
                                if ok {
                                    lowered_ops.push(Instruction::CreateTemporaryRule {
                                        rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                                            source: *source,
                                            from: *from,
                                            to: *to,
                                            combat_only: *combat_only,
                                            amount: None,
                                            one_shot: false,
                                        }),
                                        duration: dur,
                                    });
                                } else {
                                    all_temp_ok = false;
                                    break;
                                }
                            }
                            StaticContinuousAbility::CharacteristicDefining { .. }
                            | StaticContinuousAbility::CommandZoneActive { .. } => {
                                all_temp_ok = false;
                                break;
                            }
                            other => {
                                let ok = match other {
                                    StaticContinuousAbility::DamageRule { rule, .. } => {
                                        match rule {
                                            StaticDamageRule::PreventNoncombat
                                            | StaticDamageRule::PreventHalfIncoming
                                            | StaticDamageRule::DoubleToOpponents
                                            | StaticDamageRule::ReplaceSubtypeCombatWithMill(_)
                                            | StaticDamageRule::LethalUsesPower
                                            | StaticDamageRule::DamageCannotBePrevented => true,
                                            StaticDamageRule::ModifyDamage {
                                                recipient, ..
                                            } => {
                                                (in_trigger || self_pronoun_valid)
                                                    || !matches!(
                                                        recipient,
                                                        StaticDamageRecipient::SelfObject
                                                            | StaticDamageRecipient::AttachedHost
                                                    )
                                            }
                                            _ => false,
                                        }
                                    }
                                    StaticContinuousAbility::DrawRule { rule, .. } => {
                                        !matches!(rule, StaticDrawRule::EmptyDrawPutCounters { .. })
                                    }
                                    StaticContinuousAbility::UntapRule { rule, .. } => matches!(
                                        rule,
                                        StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(_)
                                    ),
                                    StaticContinuousAbility::Restriction { rule, .. } => !matches!(
                                        rule,
                                        StaticRestriction::EnchantedPlayerCastLimit
                                            | StaticRestriction::FlashSacrificeAtCleanup
                                    ),
                                    StaticContinuousAbility::Modifier { rule, .. } => matches!(
                                        rule,
                                        StaticModifierRule::DoubleCreatedTokens
                                            | StaticModifierRule::DoublePlacedCounters
                                            | StaticModifierRule::OthersEnterWithCounters { .. }
                                            | StaticModifierRule::ModifyCounters { .. }
                                            | StaticModifierRule::ModifyTokens { .. }
                                            | StaticModifierRule::ModifyTokenSubtype { .. }
                                            | StaticModifierRule::HalveOpponentCounters
                                            | StaticModifierRule::ModifyLifeGain { .. }
                                            | StaticModifierRule::ConditionalModifyLifeGain { .. }
                                            | StaticModifierRule::IgnoreLegendRule
                                            | StaticModifierRule::IgnoreLegendRuleForController
                                            | StaticModifierRule::IgnoreLegendForSubtype(_)
                                            | StaticModifierRule::RetainMana(_)
                                            | StaticModifierRule::AdditionalCreatureDeathTrigger
                                    ),
                                    StaticContinuousAbility::CostModifier { modifier, .. } => {
                                        matches!(
                                            modifier,
                                            SpellCostModifier::Discount {
                                                amount: SpellDiscount::Fixed(_)
                                                    | SpellDiscount::PerExperienceCounter(_)
                                                    | SpellDiscount::ControlledCreaturesWithPlusOneCounters,
                                                ..
                                            } | SpellCostModifier::IncreaseExceptType { .. }
                                                | SpellCostModifier::Increase { .. }
                                                | SpellCostModifier::FreeSubtypeCreatureSpells(_)
                                                | SpellCostModifier::NonartifactSpellsHaveImprovise
                                                | SpellCostModifier::HighManaValueProtected { .. }
                                        )
                                    }
                                    StaticContinuousAbility::CastPermission {
                                        permission, ..
                                    } => !matches!(
                                        permission,
                                        CastPermissionRule::PlayLibraryTop(
                                            LibraryTopPermission::CreatureOfSourceEntryType
                                        )
                                    ),
                                    StaticContinuousAbility::EventReplacement { .. } => true,
                                    StaticContinuousAbility::Battlefield { .. }
                                    | StaticContinuousAbility::CharacteristicDefining { .. }
                                    | StaticContinuousAbility::CommandZoneActive { .. } => false,
                                };
                                if ok {
                                    lowered_ops.push(Instruction::CreateTemporaryRule {
                                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(
                                            Box::new(ab),
                                        )),
                                        duration: dur,
                                    });
                                } else {
                                    all_temp_ok = false;
                                    break;
                                }
                            }
                        }
                    }
                    if all_temp_ok {
                        builder.instructions.extend(lowered_ops);
                        return Ok(());
                    }
                }
            }
            let conditional_unwrapped: Option<(&ConditionAst, StaticAbilityAst)> =
                match static_ability.as_ref() {
                    StaticAbilityAst::ContinuousModifyPT {
                        subject,
                        dx,
                        dy,
                        grant_keywords,
                        grant_ability_text,
                        scale,
                        condition: Some(cond),
                        paired_rule,
                    } => Some((
                        cond,
                        StaticAbilityAst::ContinuousModifyPT {
                            subject: subject.clone(),
                            dx: dx.clone(),
                            dy: dy.clone(),
                            grant_keywords: grant_keywords.clone(),
                            grant_ability_text: grant_ability_text.clone(),
                            scale: scale.clone(),
                            condition: None,
                            paired_rule: paired_rule.clone(),
                        },
                    )),
                    StaticAbilityAst::GrantKeywords {
                        subject,
                        keywords,
                        condition: Some(cond),
                        paired_rule,
                    } => Some((
                        cond,
                        StaticAbilityAst::GrantKeywords {
                            subject: subject.clone(),
                            keywords: keywords.clone(),
                            condition: None,
                            paired_rule: paired_rule.clone(),
                        },
                    )),
                    StaticAbilityAst::CostModifier {
                        subject,
                        delta,
                        less,
                        next_only,
                        per,
                        scale_expr,
                        condition: Some(cond),
                    } => Some((
                        cond,
                        StaticAbilityAst::CostModifier {
                            subject: subject.clone(),
                            delta: delta.clone(),
                            less: *less,
                            next_only: *next_only,
                            per: per.clone(),
                            scale_expr: scale_expr.clone(),
                            condition: None,
                        },
                    )),
                    _ => None,
                };
            if let Some((cond, inner_static)) = conditional_unwrapped {
                let mut trial = builder.clone();
                if let Ok(rc) =
                    lower_resolution_condition(cond, &mut trial, vocabulary, in_trigger, bound_x)
                    && rc.is_world_evaluable()
                {
                    let branch_idx = trial.instructions.len();
                    trial.instructions.push(Instruction::BranchIfFalse {
                        condition: rc,
                        destination: 0,
                    });
                    if lower_instruction(
                        &InstructionAst::TemporaryStatic {
                            static_ability: Box::new(inner_static),
                            duration: duration.clone(),
                        },
                        &mut trial,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .is_ok()
                        && !trial.instructions[branch_idx + 1..].iter().any(|op| {
                            matches!(
                                op,
                                Instruction::AffectPermanentTargets {
                                    minimum,
                                    maximum,
                                    ..
                                } if (*minimum, *maximum) != (1, 1)
                            ) || matches!(
                                op,
                                Instruction::ModifyAnyNumberOfPermanents { .. }
                                    | Instruction::DoubleCountersOnTargets
                            )
                        })
                    {
                        let end_dest = trial.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            trial.instructions.get_mut(branch_idx)
                        {
                            *destination = end_dest;
                        }
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            Err("oracle:unsupported-temp-static")
        }
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
            let player_ast = unless_player
                .or(inferred_actor)
                .unwrap_or(PlayerRefAst::You);
            let mut probed_body: Option<BodyBuilder> = None;
            if builder.target == BodyTarget::None {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
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
                let mana_action = match act_body.as_slice() {
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
                    InstructionAst::Optional(act_body.clone())
                } else {
                    InstructionAst::OptionalPlayer {
                        player: player_ast,
                        body: act_body.clone(),
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
                if let Ok(count_expr) =
                    lower_value_expr(&ValueExprAst::Count(filter.clone()), vocabulary)
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
                if let Ok(count_expr) = lower_value_expr(&mult, vocabulary)
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
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if matches!(
            restriction.as_str(),
            "DamageCantBePreventedThisTurn" | "DamageCantBePrevented"
        ) =>
        {
            builder
                .instructions
                .push(Instruction::DisallowDamagePreventionThisTurn);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("proliferate again") => {
            builder.instructions.push(Instruction::Proliferate);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("choose new targets for it")
            || restriction.eq_ignore_ascii_case("choose a new target for it")
            || restriction.eq_ignore_ascii_case("you may choose new targets for it")
            || restriction.eq_ignore_ascii_case("you may choose new targets for the copy") =>
        {
            builder.instructions.push(Instruction::ChooseNewTargets);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("players can't gain life this turn")
            || restriction.eq_ignore_ascii_case("players can't gain life") =>
        {
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::PlayerCantGainLife {
                    players: AffectedPlayers::EachPlayer,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("NoMaxHandSizeRestOfGame") => {
            builder
                .instructions
                .push(Instruction::GrantNoMaxHandSizeRestOfGame);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("FlashbackEqualManaCost") => {
            let has_flashback_grant = builder.instructions.iter().any(|op| {
                matches!(op, Instruction::GrantFlashback(_))
                    || matches!(
                        op,
                        Instruction::CreateTemporaryRule { rule, .. }
                            if matches!(
                                rule.as_ref(),
                                TemporaryRuleKind::ZoneCastPermission {
                                    zone: SourceZoneFilter::Graveyard,
                                    free: false,
                                    ..
                                }
                            )
                    )
            });
            if has_flashback_grant {
                Ok(())
            } else {
                Err("oracle:unsupported-instruction")
            }
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("XCantBeZero") => {
            if let Some(x_ast) = bound_x {
                let x_val = lower_value_expr(x_ast, vocabulary)?;
                if matches!(&x_val, ValueExpr::Literal(n) if n.is_positive()) {
                    return Ok(());
                }
                let has_whole_body_target_op = builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::AffectPermanentTargets { .. }
                            | Instruction::DoubleCountersOnTargets
                            | Instruction::ModifyAnyNumberOfPermanents { .. }
                    )
                });
                if !builder.instructions.is_empty() && !has_whole_body_target_op {
                    let end_dest = builder.instructions.len() + 1;
                    prepend_instruction_at(
                        &mut builder.instructions,
                        0,
                        Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: x_val,
                                comparison: Comparison::Greater,
                                right: ValueExpr::literal(0),
                            },
                            destination: end_dest,
                        },
                    );
                    return Ok(());
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("it's still a land")
            || restriction.eq_ignore_ascii_case("they're still lands") =>
        {
            if let Some(op) = builder.instructions.last_mut() {
                let parts = match op {
                    Instruction::ChangeAbilitySource { parts, .. }
                    | Instruction::CreateContinuousEffect { parts, .. }
                    | Instruction::ChangeGroup { parts, .. } => Some(parts),
                    Instruction::AffectPermanentTargets {
                        action: PermanentTargetAction::Modify { parts, .. },
                        ..
                    } => Some(parts),
                    _ => None,
                };
                if let Some(p) = parts {
                    let mut touched = false;
                    for part in &mut p.0 {
                        if let ResolvedPartSpec::Type(tc)
                        | ResolvedPartSpec::TypeWithSubtypes { base: tc, .. } = part
                        {
                            match &mut tc.types {
                                TypeOperation::Add(_) => touched = true,
                                TypeOperation::Set(types) => {
                                    let existing = std::mem::take(types);
                                    tc.types = TypeOperation::Add(existing);
                                    touched = true;
                                }
                                TypeOperation::Keep | TypeOperation::Remove(_) => {}
                            }
                        }
                    }
                    if touched {
                        return Ok(());
                    }
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::GrantAbilityText {
            subject,
            ability_text,
            compiled: Some(compiled_face),
            duration,
        } => {
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && !items.is_empty()
            {
                for it in items {
                    lower_instruction(
                        &InstructionAst::GrantAbilityText {
                            subject: Box::new(it.clone()),
                            ability_text: ability_text.clone(),
                            compiled: Some(compiled_face.clone()),
                            duration: duration.clone(),
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
            let dur = lower_duration(duration)?;
            let parts = lower_granted_ability_specs(compiled_face, vocabulary, ids)?;
            if parts.is_empty() {
                return Err("oracle:unsupported-grant-ability-text");
            }
            match subject.as_ref() {
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    for part in parts {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![part]),
                                duration: dur,
                            });
                    }
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
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(parts),
                                duration: dur,
                            },
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
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(parts),
                                duration: dur,
                            },
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
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(parts),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::AnyNumber,
                } if filter.zone.is_none() => {
                    let obj = lower_object_filter(filter, vocabulary)?;
                    builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
                    builder
                        .instructions
                        .push(Instruction::ModifyAnyNumberOfPermanents {
                            parts: ResolvedPartsSpec(parts),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    for part in parts {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![part]),
                                duration: dur,
                            });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_created_tokens(&builder.instructions) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::CreatedTokens),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::LastMoved),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(group_obj)),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if builder.target == BodyTarget::None
                        && in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(obj)),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-ability-text"),
            }
        }
        InstructionAst::AdditionalLandPlay { count, duration } if *count > 0 => {
            let dur = lower_duration(duration)?;
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::AdditionalLandPlay {
                    player: ResolvingPlayer::Controller,
                    count: *count,
                }),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::OneShotReplacement {
            subject,
            event,
            replace_with,
            instead_body,
        } => {
            let ev_low = event.trim().to_ascii_lowercase();
            let rep_low = replace_with.trim().to_ascii_lowercase();
            let subj_low = subject.trim().to_ascii_lowercase();
            let body_exiles = instead_body.as_ref().is_some_and(|b| {
                b.iter()
                    .any(|op| matches!(op, InstructionAst::Exile { .. }))
            });
            let is_exile = rep_low.contains("exile") || body_exiles;
            if is_exile
                && (ev_low.contains("counter")
                    || ev_low.contains("graveyard")
                    || subj_low.contains("spell"))
                && let Some(last) = builder.instructions.last_mut()
                && *last == Instruction::CounterSpell
            {
                *last = Instruction::CounterSpellToExile;
                return Ok(());
            }
            if is_exile
                && (ev_low.contains("die") || ev_low.contains("graveyard"))
                && (subj_low.contains("dealt damage this way")
                    || ev_low.contains("dealt damage this way"))
            {
                if let Some(last) = builder.instructions.last_mut() {
                    match last.clone() {
                        Instruction::Damage { amount } => {
                            *last = Instruction::DamageAmountThenExileIfDies(ValueExpr::Literal(
                                amount,
                            ));
                            return Ok(());
                        }
                        Instruction::DamageAmount(expr) => {
                            *last = Instruction::DamageAmountThenExileIfDies(expr);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                let has_damage_op = builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::Damage { .. }
                            | Instruction::DamageAmount(_)
                            | Instruction::DamageGroup { .. }
                            | Instruction::DamageGroupAmount { .. }
                            | Instruction::DamageOpponentsAmount(_)
                            | Instruction::DamageRelatedPlayerAmount(_)
                            | Instruction::DamageOrderedTargets { .. }
                            | Instruction::DamageFromPower(_)
                            | Instruction::DamageFromGrantingPermanent { .. }
                            | Instruction::DamageDamagedPlayersCreatures
                            | Instruction::DamageTargetControllersCreatures { .. }
                            | Instruction::DamageCapturedBlocker(_)
                            | Instruction::DamageCreatureAtSpellMastery { .. }
                            | Instruction::DealDamageFrom { .. }
                            | Instruction::DivideDamageAmongTargets { .. }
                            | Instruction::CreatureDealsPowerDamageToOther { .. }
                            | Instruction::Fight { .. }
                            | Instruction::FightSourceWithTarget
                            | Instruction::FightFixedTargets
                            | Instruction::FightPairedCreatures(_)
                    )
                });
                if has_damage_op {
                    let any_types = if subj_low.contains("creature or planeswalker") {
                        Some(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER))
                    } else if subj_low.contains("planeswalker") {
                        Some(CardTypeMask::PLANESWALKER)
                    } else if subj_low.contains("creature") {
                        Some(CardTypeMask::CREATURE)
                    } else if subj_low.contains("permanent") {
                        Some(CardTypeMask::PERMANENT)
                    } else {
                        None
                    };
                    if let Some(any_types) = any_types {
                        let dmg_flag = if self_pronoun_valid || in_trigger {
                            ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN
                        } else {
                            ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN
                        };
                        let clause = ObjectClause {
                            any_types,
                            flags: dmg_flag,
                            ..ObjectClause::any()
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::ExileIfDies {
                                subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                    clause,
                                ))),
                            }),
                            duration: ResolvedDuration::UntilEndOfTurn,
                        });
                        return Ok(());
                    }
                }
            }
            if ev_low == "next_time_this_turn" {
                let is_prevent_rep = matches!(
                    rep_low.as_str(),
                    "prevent that damage"
                        | "prevent all of that damage"
                        | "prevent all damage that would be dealt this way"
                );
                if is_prevent_rep
                    && let Some((src_raw, dst_raw)) = subj_low.split_once(" would deal ")
                    && dst_raw.contains("damage")
                {
                    let combat_only = dst_raw.contains("combat damage");
                    let after_dmg = dst_raw
                        .split_once("combat damage")
                        .or_else(|| dst_raw.split_once("damage"))
                        .map(|(_, r)| r.trim())
                        .unwrap_or("");
                    let recipient = match after_dmg {
                        "to you or a permanent you control"
                        | "to you and/or permanents you control" => {
                            Some(StaticDamageRecipient::ControllerOrControlledPermanent)
                        }
                        "to you or a creature you control" | "to you or creatures you control" => {
                            Some(StaticDamageRecipient::ControllerOrControlledCreatures)
                        }
                        "to creatures you control" | "to a creature you control" => {
                            Some(StaticDamageRecipient::ControlledCreatures)
                        }
                        "to you" => Some(StaticDamageRecipient::Controller),
                        "to ~" | "to this creature" | "to this permanent" if self_pronoun_valid => {
                            Some(StaticDamageRecipient::SelfObject)
                        }
                        "to enchanted creature"
                        | "to equipped creature"
                        | "to enchanted permanent"
                            if self_pronoun_valid =>
                        {
                            Some(StaticDamageRecipient::AttachedHost)
                        }
                        _ => None,
                    };
                    if let Some(recipient) = recipient
                        && let Some((source, source_chosen_color)) =
                            parse_next_time_damage_source(src_raw, vocabulary)
                    {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::PreventNextTimeDamage {
                                source,
                                source_chosen_color,
                                recipient,
                                combat_only,
                            }),
                            duration: ResolvedDuration::UntilEndOfTurn,
                        });
                        return Ok(());
                    }
                }
            }
            if ev_low.contains("combat damage") && rep_low.contains("prevent") {
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PreventAllCombatDamage),
                    duration: ResolvedDuration::UntilEndOfTurn,
                });
                return Ok(());
            }
            if is_exile
                && (ev_low.contains("leave the battlefield")
                    || ev_low.contains("leaves the battlefield"))
            {
                let eff_subj = if matches!(
                    subj_low.as_str(),
                    "it" | "them"
                        | "that creature"
                        | "those creatures"
                        | "that permanent"
                        | "those permanents"
                        | "that token"
                        | "those tokens"
                ) {
                    if last_instruction_created_tokens(&builder.instructions) {
                        Some(EffectSubject::CreatedTokens)
                    } else if is_permanent_body_target(builder.target) {
                        Some(EffectSubject::TargetObject)
                    } else if last_instruction_moved_object(&builder.instructions) {
                        Some(EffectSubject::LastMoved)
                    } else if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else if in_trigger {
                        Some(EffectSubject::TriggerSubject)
                    } else {
                        builder.last_group_filter.map(EffectSubject::Matching)
                    }
                } else if matches!(subj_low.as_str(), "~" | "this creature" | "this permanent") {
                    self_pronoun_valid.then_some(EffectSubject::AbilitySource)
                } else if subj_low.contains("a creature") || subj_low.contains("any creature") {
                    Some(EffectSubject::Matching(ObjectFilter::creature()))
                } else {
                    None
                };
                if let Some(eff_subj) = eff_subj {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::ExileIfLeavesBattlefield {
                            subject: Box::new(eff_subj),
                        }),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    });
                    return Ok(());
                }
            }
            if is_exile
                && ev_low.contains("graveyard")
                && (subj_low.contains("spell")
                    || ev_low.contains("spell")
                    || subj_low.contains("card cast this way"))
            {
                let players = if subj_low.contains("you cast")
                    || subj_low.contains("your")
                    || subj_low.contains("cast this way")
                {
                    AffectedPlayers::One(ResolvingPlayer::Controller)
                } else if subj_low.contains("opponent") {
                    AffectedPlayers::EachOpponent
                } else {
                    AffectedPlayers::EachPlayer
                };
                let filter = if subj_low.contains("instant or sorcery")
                    || subj_low.contains("instant and sorcery")
                {
                    Some(ObjectFilter::Clause(ObjectClause::any().any_types(
                        CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
                    )))
                } else if subj_low.contains("creature") {
                    Some(ObjectFilter::creature())
                } else {
                    None
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::ExileSpellOnResolve { filter, players }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                });
                return Ok(());
            }
            if rep_low.starts_with("prevent the next ") || ev_low.starts_with("the next ") {
                let text = if rep_low.starts_with("prevent the next ") {
                    &rep_low
                } else {
                    &ev_low
                };
                if let Some(after_next) = text
                    .strip_prefix("prevent the next ")
                    .or_else(|| text.strip_prefix("the next "))
                {
                    let mut parts = after_next.split_whitespace();
                    let amt_tok = parts.next().unwrap_or("");
                    let combat_only = after_next.contains("combat damage");
                    let recipient = if after_next.contains("to you or a permanent you control")
                        || after_next.contains("to you and/or permanents you control")
                    {
                        Some(StaticDamageRecipient::ControllerOrControlledPermanent)
                    } else if after_next.contains("to you or a creature you control") {
                        Some(StaticDamageRecipient::ControllerOrControlledCreatures)
                    } else if after_next.contains("to creatures you control") {
                        Some(StaticDamageRecipient::ControlledCreatures)
                    } else if after_next.contains("to you") {
                        Some(StaticDamageRecipient::Controller)
                    } else if after_next.contains("to ~") && self_pronoun_valid {
                        Some(StaticDamageRecipient::SelfObject)
                    } else {
                        None
                    };
                    if let Some(recipient) = recipient {
                        if amt_tok == "x" {
                            let x_expr = match bound_x {
                                Some(expr) => lower_value_expr(expr, vocabulary)?,
                                None => ValueExpr::chosen_x(),
                            };
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::PreventNextDamageExpr {
                                    amount: Box::new(x_expr),
                                    source: None,
                                    recipient,
                                    combat_only,
                                }),
                                duration: ResolvedDuration::UntilEndOfTurn,
                            });
                            return Ok(());
                        } else if let Ok(n) = amt_tok.parse::<i64>()
                            && n > 0
                        {
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::PreventNextDamage {
                                    amount: n.into(),
                                    source: None,
                                    recipient,
                                    combat_only,
                                }),
                                duration: ResolvedDuration::UntilEndOfTurn,
                            });
                            return Ok(());
                        }
                    }
                }
            }
            if (ev_low.contains("die") || ev_low.contains("graveyard")) && is_exile {
                let eff_subj = if matches!(
                    subj_low.as_str(),
                    "it" | "them"
                        | "that creature"
                        | "those creatures"
                        | "that permanent"
                        | "those permanents"
                        | "that token"
                        | "those tokens"
                ) {
                    if last_instruction_created_tokens(&builder.instructions) {
                        Some(EffectSubject::CreatedTokens)
                    } else if is_permanent_body_target(builder.target) {
                        Some(EffectSubject::TargetObject)
                    } else if last_instruction_moved_object(&builder.instructions) {
                        Some(EffectSubject::LastMoved)
                    } else if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else if in_trigger {
                        Some(EffectSubject::TriggerSubject)
                    } else {
                        builder.last_group_filter.map(EffectSubject::Matching)
                    }
                } else if matches!(subj_low.as_str(), "~" | "this creature" | "this permanent") {
                    self_pronoun_valid.then_some(EffectSubject::AbilitySource)
                } else if subj_low.contains("creature you control") {
                    Some(EffectSubject::Matching(
                        ObjectFilter::creature().controlled(),
                    ))
                } else if subj_low.contains("creature an opponent controls") {
                    Some(EffectSubject::Matching(ObjectFilter::creature().opposing()))
                } else if subj_low.contains("nontoken creature") {
                    Some(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause {
                            token: Some(false),
                            ..ObjectClause::creature()
                        },
                    )))
                } else if subj_low.contains("creature or planeswalker") {
                    Some(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any()
                            .any_types(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER)),
                    )))
                } else if subj_low.contains("planeswalker") {
                    Some(EffectSubject::Matching(ObjectFilter::planeswalker()))
                } else if subj_low.is_empty()
                    || subj_low.contains("a creature")
                    || subj_low.contains("any creature")
                {
                    Some(EffectSubject::Matching(ObjectFilter::creature()))
                } else {
                    None
                };
                if let Some(eff_subj) = eff_subj {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::ExileIfDies {
                            subject: Box::new(eff_subj),
                        }),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    });
                    return Ok(());
                }
            }
            if let Some(res) = try_lower_redirect_next_damage(
                &subj_low,
                &ev_low,
                &rep_low,
                builder,
                in_trigger || self_pronoun_valid,
                vocabulary,
            ) {
                return res;
            }
            let ev_norm = ev_low
                .trim_end_matches('.')
                .strip_suffix(" this turn")
                .unwrap_or_else(|| ev_low.trim_end_matches('.'))
                .trim();
            let rep_no_dot = rep_low.trim_end_matches('.').trim();
            let rep_no_instead = rep_no_dot
                .strip_suffix(" instead")
                .unwrap_or(rep_no_dot)
                .trim();
            let rep_norm = rep_no_instead
                .strip_suffix(" this turn")
                .unwrap_or(rep_no_instead)
                .trim();
            if let Some(rule) = parse_modify_damage_event(&subj_low, ev_norm, rep_norm, vocabulary)
            {
                let recipient_ok = (in_trigger || self_pronoun_valid)
                    || !matches!(
                        rule,
                        StaticDamageRule::ModifyDamage {
                            recipient: StaticDamageRecipient::SelfObject
                                | StaticDamageRecipient::AttachedHost,
                            ..
                        }
                    );
                if recipient_ok {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::DamageRule {
                                id: ids.static_id(),
                                rule,
                            },
                        ))),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    });
                    return Ok(());
                }
            }
            Err("oracle:unsupported-oneshot-replacement")
        }
        InstructionAst::DelayedEventTrigger { trigger, body } => {
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            let stripped = strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
            let self_pron = trigger_subject_is_self(trigger);
            let mut exec_body =
                lower_trigger_effect_body(&stripped, vocabulary, ids, &spec, self_pron)?;
            if let Some(cond) = guard {
                prepend_instruction_at(
                    &mut exec_body.instructions,
                    0,
                    Instruction::RequireCondition(cond),
                );
            }
            apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, self_pron);
            let once = !trigger.events.is_empty()
                && trigger.events.iter().all(|ev| {
                    matches!(
                        ev,
                        TriggerEventAst::ZoneChange { subject, .. }
                            if matches!(
                                subject.as_ref(),
                                SubjectRefAst::SelfObject
                                    | SubjectRefAst::Anaphora(_)
                                    | SubjectRefAst::TargetObject { .. }
                            )
                    )
                });
            builder
                .instructions
                .push(Instruction::ScheduleDelayedEventTrigger {
                    trigger: Box::new(spec),
                    body: Box::new(exec_body),
                    expires: ResolvedDuration::UntilEndOfTurn,
                    once,
                });
            Ok(())
        }
        InstructionAst::AnimateOrSetBasePT {
            subject,
            power,
            toughness,
            description,
            keywords,
            duration,
        } => lower_animate_or_set_pt(
            subject,
            power,
            toughness,
            description.as_deref(),
            keywords,
            duration,
            false,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::BecomeCopy {
            subject,
            source,
            duration,
            exception: None,
        } if is_self_pronoun(subject, builder, self_pronoun_valid)
            && matches!(
                source.as_ref(),
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none()
            ) =>
        {
            let dur = lower_duration(duration)?;
            let SubjectRefAst::TargetObject { filter, .. } = source.as_ref() else {
                return Err("oracle:unsupported-become-copy");
            };
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder.instructions.push(Instruction::CopyTargetToSource {
                duration: dur,
                power_toughness: None,
            });
            Ok(())
        }
        InstructionAst::BecomeCopy {
            subject,
            source,
            duration,
            exception: None,
        } if is_self_pronoun(subject, builder, self_pronoun_valid)
            && matches!(source.as_ref(), SubjectRefAst::Anaphora(_))
            && matches!(
                builder.target,
                BodyTarget::SinglePermanent
                    | BodyTarget::SingleCreature
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::FilteredPermanent(_)
            ) =>
        {
            let dur = lower_duration(duration)?;
            builder.instructions.push(Instruction::CopyTargetToSource {
                duration: dur,
                power_toughness: None,
            });
            Ok(())
        }
        InstructionAst::BecomeCopy {
            subject,
            source,
            duration,
            exception,
        } => {
            let dur = lower_duration(duration)?;
            let exc = parse_copy_exceptions(exception.as_deref(), vocabulary, ids)?;
            if !in_trigger
                && builder.target == BodyTarget::None
                && builder.instructions.is_empty()
                && let (
                    SubjectRefAst::TargetObject {
                        filter: recv_f,
                        count: TargetCountAst::Exact(1),
                    },
                    SubjectRefAst::TargetObject {
                        filter: donor_f,
                        count: TargetCountAst::Exact(1),
                    },
                ) = (subject.as_ref(), source.as_ref())
                && recv_f.zone.is_none()
                && donor_f.zone.is_none()
            {
                let recv_bt = lower_battlefield_body_target(recv_f, vocabulary)?;
                let donor_bt = lower_battlefield_body_target(donor_f, vocabulary)?;
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(recv_bt, vec![Instruction::RememberPermanentTarget]),
                    )));
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(donor_bt, vec![Instruction::RememberPermanentTarget]),
                    )));
                builder
                    .instructions
                    .push(Instruction::BecomeCopyFixedTargets {
                        duration: dur,
                        exceptions: Box::new(exc),
                    });
                return Ok(());
            }
            let receiver = match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    EffectSubject::AbilitySource
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    EffectSubject::TargetObject
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
                SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    EffectSubject::TriggerSubject
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => EffectSubject::Matching(
                    ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                ),
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    EffectSubject::Matching(group_obj)
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    builder.last_group_filter = Some(obj);
                    EffectSubject::Matching(obj)
                }
                _ => return Err("oracle:unsupported-become-copy"),
            };
            let donor = match source.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    MoveSubjectRef::AbilitySource
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    MoveSubjectRef::TargetObject
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
                    MoveSubjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    MoveSubjectRef::LastMoved
                }
                SubjectRefAst::TriggerSubject if in_trigger => MoveSubjectRef::TriggeringObject,
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    MoveSubjectRef::TriggeringObject
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    MoveSubjectRef::AttachedHost
                }
                _ => return Err("oracle:unsupported-become-copy"),
            };
            builder.instructions.push(Instruction::BecomeCopy {
                receiver: Box::new(receiver),
                donor,
                duration: dur,
                exceptions: Box::new(exc),
            });
            Ok(())
        }
        InstructionAst::BecomeChosenCharacteristic {
            subject,
            kind,
            duration,
        } if kind.trim().to_ascii_lowercase().contains("creature type") => {
            let dur = lower_duration(duration)?;
            let slot = ids.creature_type_slot();
            builder.instructions.push(Instruction::ChooseCreatureType {
                into: slot,
                excluding: BTreeSet::new(),
            });
            let type_part = ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(slot)),
            });
            let spec = ResolvedPartsSpec(vec![type_part]);
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: spec,
                        duration: dur,
                    });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: spec,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.instructions.push(Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::Matching(obj)),
                        parts: spec,
                        duration: dur,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-become-chosen"),
            }
        }
        InstructionAst::Modal { choice, modes } if modes.len() >= 2 => {
            if modes.iter().any(|m| m.granted.is_some()) {
                return Err("oracle:unsupported-modal-granted-face");
            }
            let (min_raw, max_raw) = parse_modal_choice(choice)?;
            let mut mode_bodies = Vec::with_capacity(modes.len());
            for m in modes {
                let mut sub = BodyBuilder::new();
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
        InstructionAst::LookTopLibrary {
            reveal,
            library,
            count,
        } => {
            let lib_low = library.trim().to_ascii_lowercase();
            let owner_opt = match lib_low.as_str() {
                "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" => {
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
                "that player" | "that player's" | "thatplayer" | "their" | "their library" => {
                    Some((
                        None,
                        lower_resolving_player_in_context(
                            PlayerRefAst::ThatPlayer,
                            builder,
                            in_trigger,
                        )?,
                    ))
                }
                "defending player" | "defending player's" | "defendingplayer" => {
                    Some((None, ResolvingPlayer::DefendingPlayer))
                }
                "enchanted player" | "enchanted player's" | "enchantedplayer" => {
                    Some((None, ResolvingPlayer::EnchantedPlayer))
                }
                "active player" | "active player's" | "activeplayer" => {
                    Some((None, ResolvingPlayer::ActivePlayer))
                }
                _ => None,
            };
            let (req_bt, library_owner) = owner_opt.ok_or("oracle:unsupported-instruction")?;
            if let Some(bt) = req_bt {
                builder.bind_target(bt)?;
            }
            let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
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
            count_limit: None,
            alt_cost: None,
        } => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let dur_low = duration.trim().to_ascii_lowercase();
            let (impulse_dur, resolved_dur) = if dur_low == "until_end_of_next_turn"
                || dur_low.contains("end of your next turn")
                || dur_low.contains("untilendofnextturn")
                || dur_low.contains("untilyournextturn")
            {
                (
                    spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn,
                    ResolvedDuration::ThroughControllerNextTurn,
                )
            } else if dur_low == "while_exiled"
                || dur_low.contains("remains exiled")
                || dur_low.contains("remain exiled")
                || dur_low == "permanent"
            {
                (
                    spellwake_engine::program::ImpulsePlayDuration::WhileExiled,
                    ResolvedDuration::NoStatedDuration,
                )
            } else if dur_low == "until_next_exile" || dur_low.contains("until you exile another") {
                (
                    spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource,
                    ResolvedDuration::NoStatedDuration,
                )
            } else if dur_low.is_empty()
                || dur_low == "this_turn"
                || dur_low == "until_end_of_turn"
                || dur_low == "now"
                || dur_low.contains("this turn")
                || dur_low.contains("end of turn")
                || dur_low.contains("untilendofturn")
            {
                (
                    spellwake_engine::program::ImpulsePlayDuration::ThisTurn,
                    ResolvedDuration::UntilEndOfTurn,
                )
            } else {
                return Err("oracle:unsupported-grant-cast-duration");
            };
            let any_color = dur_low.contains("any color") || dur_low.contains("any type");
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    count: TargetCountAst::Exact(1),
                    ..
                } => {
                    let bt = lower_target_subject(subject, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::GrantCardPlayPermission {
                            player: rp,
                            subject: MoveSubjectRef::TargetObject,
                            duration: impulse_dur,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
                        });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::GrantCardPlayPermission {
                            player: rp,
                            subject: MoveSubjectRef::TriggeringObject,
                            duration: impulse_dur,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
                        });
                    Ok(())
                }
                SubjectRefAst::SelfObject if self_pronoun_valid => {
                    builder
                        .instructions
                        .push(Instruction::GrantCardPlayPermission {
                            player: rp,
                            subject: MoveSubjectRef::AbilitySource,
                            duration: impulse_dur,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
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
                        .push(Instruction::GrantCardPlayPermission {
                            player: rp,
                            subject: MoveSubjectRef::TargetObject,
                            duration: impulse_dur,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    builder
                        .instructions
                        .push(Instruction::GrantCardPlayPermission {
                            player: rp,
                            subject: MoveSubjectRef::TriggeringObject,
                            duration: impulse_dur,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) => {
                    builder
                        .instructions
                        .push(Instruction::GrantCardPlayPermission {
                            player: rp,
                            subject: MoveSubjectRef::RevealedCard,
                            duration: impulse_dur,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
                        });
                    Ok(())
                }
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    let mut inner = f.as_ref().clone();
                    let is_linked_exile = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                    let zone =
                        if is_linked_exile && matches!(f.zone, None | Some(ZoneFilter::Exile)) {
                            inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                            SourceZoneFilter::LinkedExile
                        } else {
                            match f.zone {
                                Some(ZoneFilter::YourHand) => SourceZoneFilter::Hand,
                                Some(ZoneFilter::YourGraveyard) => SourceZoneFilter::Graveyard,
                                Some(
                                    ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards,
                                ) => SourceZoneFilter::OpponentGraveyard,
                                Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                                    SourceZoneFilter::AnyGraveyard
                                }
                                Some(ZoneFilter::YourLibrary) => SourceZoneFilter::YourLibrary,
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
                    let once = matches!(subject.as_ref(), SubjectRefAst::Group(_));
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::ZoneCastPermission {
                            player: rp,
                            zone,
                            filter: flt,
                            cast_only: *cast_only,
                            free: *free,
                            any_color,
                            once,
                        }),
                        duration: resolved_dur,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-cast"),
            }
        }
        InstructionAst::WinGame { player } => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::WinGame(rp));
            Ok(())
        }
        InstructionAst::LoseGame { player } => {
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::LoseGame(players));
            Ok(())
        }
        InstructionAst::DistributeCounters {
            counter_kind,
            count,
            targets,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:distribute-counter-kind")?;
            if ck.is_player_counter() {
                return Err("oracle:distribute-counter-kind");
            }
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let ValueExpr::Literal(n) = &amt else {
                return Err("oracle:distribute-non-const");
            };
            let Some(total) = n
                .to_usize()
                .and_then(|v| u16::try_from(v).ok())
                .filter(|&v| v > 0)
            else {
                return Err("oracle:distribute-non-const");
            };
            let filter = match targets.as_ref() {
                SubjectRefAst::TargetObject { filter, .. } if filter.zone.is_none() => filter,
                _ => return Err("oracle:distribute-unsupported-target"),
            };
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::DistributeCountersAmongTargets { kind: ck, total });
            Ok(())
        }
        InstructionAst::CreateEmblem(text) => {
            let def = build_emblem_definition(text, None, vocabulary)?;
            builder.instructions.push(Instruction::CreateEmblem(def));
            Ok(())
        }
        InstructionAst::CreateEmblemFor {
            recipient,
            text,
            compiled,
        } => {
            let def = build_emblem_definition(text, compiled.as_deref(), vocabulary)?;
            match recipient.as_ref() {
                SubjectRefAst::Player(PlayerRefAst::You) => {
                    builder.instructions.push(Instruction::CreateEmblem(def));
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachOpponent) => {
                    builder
                        .instructions
                        .push(Instruction::CreateOpponentEmblems(def));
                    Ok(())
                }
                _ => Err("oracle:unsupported-emblem-recipient"),
            }
        }
        InstructionAst::EntersTappedRule {
            subject,
            tapped: true,
            attacking: None,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let sel = lower_subject_to_permanent_selector(subject, vocabulary)?;
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OthersEnterTapped(Box::new(sel)),
                    },
                ))),
                duration: dur,
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
        InstructionAst::ForAsLongAs { condition, body } => {
            let is_remains_exiled = matches!(
                condition.as_ref(),
                ConditionAst::Marker(spellwake_oracle_compiler::ast::GameMarker::RemainsExiled)
            ) || matches!(
                condition.as_ref(),
                ConditionAst::GameStateFlag(flag)
                    if matches!(
                        flag.trim().trim_end_matches('.').to_ascii_lowercase().as_str(),
                        "it remains exiled"
                            | "those cards remain exiled"
                            | "they remain exiled"
                    )
            );
            if is_remains_exiled
                && let [
                    InstructionAst::GrantCastPermission {
                        player,
                        cast_only,
                        subject,
                        free,
                        duration: _,
                        count_limit,
                        alt_cost,
                    },
                ] = body.as_slice()
            {
                return lower_instruction(
                    &InstructionAst::GrantCastPermission {
                        player: *player,
                        cast_only: *cast_only,
                        subject: subject.clone(),
                        free: *free,
                        duration: "while_exiled".to_string(),
                        count_limit: count_limit.clone(),
                        alt_cost: alt_cost.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            let is_controls_self = matches!(
                condition.as_ref(),
                ConditionAst::Marker(spellwake_oracle_compiler::ast::GameMarker::ControlsSelf)
            ) || matches!(
                condition.as_ref(),
                ConditionAst::GameStateFlag(flag)
                    if flag.trim().trim_end_matches('.').eq_ignore_ascii_case("you control ~")
            );
            if is_controls_self
                && (self_pronoun_valid || in_trigger)
                && let [
                    InstructionAst::GainControl {
                        controller,
                        subject,
                        duration: _,
                    },
                ] = body.as_slice()
            {
                return lower_instruction(
                    &InstructionAst::GainControl {
                        controller: *controller,
                        subject: subject.clone(),
                        duration: DurationAst::UntilSourceLeaves,
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            Err("oracle:unsupported-for-as-long-as")
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
        InstructionAst::RevealCards { subject } => match subject.as_ref() {
            SubjectRefAst::Player(p) => {
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
        InstructionAst::Monstrosity { count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder
                .instructions
                .push(Instruction::Monstrosity { amount: amt });
            Ok(())
        }
        InstructionAst::ManualTabletopAction { .. } => {
            Err("oracle:tabletop-honor-excluded-from-constructed")
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

fn resolve_select_zone(
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
    let zone = filter.zone.take();
    match zone {
        None => Ok((ResolvingPlayer::Controller, default_zone)),
        Some(ZoneFilter::Battlefield) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::Battlefield))
        }
        Some(ZoneFilter::YourHand) => Ok((ResolvingPlayer::Controller, SourceZoneFilter::Hand)),
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

fn has_checked_revealed_top(ops: &[Instruction]) -> bool {
    ops.iter().rev().any(|op| {
        matches!(
            op,
            Instruction::LookTopChoose {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
                | Instruction::ExileLibraryTop { .. }
                | Instruction::RevealUntilMatch { .. }
        )
    })
}

/// True only when a prior instruction satisfies the engine's `definitely_checked_top`
/// invariant required to validate `MoveSubjectRef::RevealedCard`.
fn has_checked_revealed_top_strict(ops: &[Instruction]) -> bool {
    ops.iter().rev().any(|op| {
        matches!(
            op,
            Instruction::LookTopChoose {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
                | Instruction::ExileLibraryTop { .. }
        )
    })
}

fn parse_small_number_word(raw: &str) -> Option<u16> {
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

fn parse_vote_options(raw: &str) -> Result<Vec<String>, &'static str> {
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

fn lower_move_subject_ref(
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

fn parse_next_time_damage_source(
    src_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(Option<ObjectFilter>, bool)> {
    let mut s = src_raw.trim().to_ascii_lowercase();
    let mut source_chosen_color = false;
    for pat in [
        "of the chosen color",
        "of the last chosen color",
        "that's the chosen color",
    ] {
        if s.contains(pat) {
            source_chosen_color = true;
            s = s.replace(pat, " ");
        }
    }
    for pat in ["of your choice", "you choose"] {
        s = s.replace(pat, " ");
    }
    let tokens: Vec<&str> = s
        .split_whitespace()
        .filter(|t| !matches!(*t, "a" | "an" | "any" | "the" | "source" | "sources" | "or"))
        .collect();
    if tokens.is_empty() {
        return Some((None, source_chosen_color));
    }
    let mut any_colors = ColorMask::default();
    let mut forbidden_colors = ColorMask::default();
    let mut any_types = CardTypeMask::default();
    let mut subtype: Option<CreatureTypeId> = None;
    let mut token_ok = true;
    for tok in tokens {
        match tok {
            "white" => any_colors = any_colors.union(ColorMask::WHITE),
            "blue" => any_colors = any_colors.union(ColorMask::BLUE),
            "black" => any_colors = any_colors.union(ColorMask::BLACK),
            "red" => any_colors = any_colors.union(ColorMask::RED),
            "green" => any_colors = any_colors.union(ColorMask::GREEN),
            "colorless" => forbidden_colors = ColorMask::ALL,
            "artifact" | "artifacts" => any_types = any_types.union(CardTypeMask::ARTIFACT),
            "creature" | "creatures" => any_types = any_types.union(CardTypeMask::CREATURE),
            "enchantment" | "enchantments" => {
                any_types = any_types.union(CardTypeMask::ENCHANTMENT);
            }
            "land" | "lands" => any_types = any_types.union(CardTypeMask::LAND),
            "instant" | "instants" => any_types = any_types.union(CardTypeMask::INSTANT),
            "sorcery" | "sorceries" => any_types = any_types.union(CardTypeMask::SORCERY),
            "planeswalker" | "planeswalkers" => {
                any_types = any_types.union(CardTypeMask::PLANESWALKER);
            }
            _ => {
                if subtype.is_none()
                    && let Some(ct) = resolve_protection_subtype(tok, vocabulary)
                {
                    subtype = Some(ct);
                } else {
                    token_ok = false;
                    break;
                }
            }
        }
    }
    if !token_ok {
        let candidate = s.trim();
        if !candidate.is_empty()
            && let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                candidate,
                &cached_compiler_vocab(vocabulary),
            )
        {
            ast_f.zone = None;
            if let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
                && !obj.has_target_player()
            {
                return Some((Some(obj), source_chosen_color));
            }
        }
        return None;
    }
    let mut clause = ObjectClause::any();
    if !any_colors.is_empty() {
        clause.color = ObjectColorFilter::any_of(any_colors);
    } else if !forbidden_colors.is_empty() {
        clause.color = ObjectColorFilter::Colorless;
    }
    if !any_types.is_empty() {
        clause.any_types = any_types;
    }
    if let Some(ct) = subtype {
        clause.subtypes = SubtypeFilter::Has(ct);
    }
    Some((Some(ObjectFilter::Clause(clause)), source_chosen_color))
}

fn bind_filter_player_target(
    builder: &mut BodyBuilder,
    filter: &ObjectFilter,
) -> Result<(), &'static str> {
    let mut needs_player = false;
    let mut needs_opponent = false;
    let mut inspect = |clause: ObjectClause| match clause.controller {
        ControllerFilter::TargetPlayer => needs_player = true,
        ControllerFilter::TargetOpponent => needs_opponent = true,
        _ => {}
    };
    match *filter {
        ObjectFilter::Clause(c) => inspect(c),
        ObjectFilter::Either(a, b) => {
            inspect(a);
            inspect(b);
        }
    }
    if needs_opponent {
        if builder.target != BodyTarget::SingleOpponent {
            builder.bind_target(BodyTarget::SingleOpponent)?;
        }
    } else if needs_player
        && !matches!(
            builder.target,
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
        )
    {
        builder.bind_target(BodyTarget::SinglePlayer)?;
    }
    Ok(())
}

fn is_permanent_body_target(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::SingleCreature
            | BodyTarget::SinglePermanent
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
    )
}

fn is_stack_body_target(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::SingleSpell(_)
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::SingleStackEntry
            | BodyTarget::ControlledAbility { .. }
            | BodyTarget::StackEntryTargetingControlledPermanent
    )
}

fn last_instruction_created_tokens(ops: &[Instruction]) -> bool {
    matches!(
        ops.last(),
        Some(
            Instruction::CreateTokens { .. }
                | Instruction::CreateTokenCopy { .. }
                | Instruction::CreateAttackingTokens { .. }
        )
    )
}

fn last_instruction_moved_object(ops: &[Instruction]) -> bool {
    matches!(
        ops.last(),
        Some(
            Instruction::MoveSubject { .. }
                | Instruction::SelectZoneCardsToZone { .. }
                | Instruction::MoveGroup { .. }
                | Instruction::MoveGraveyardGroup { .. }
                | Instruction::PutHandPermanentFiltered { .. }
                | Instruction::PutHandPermanentsOntoBattlefield { .. }
                | Instruction::ReturnExiledTargetCohort { .. }
                | Instruction::ReturnCardsDestroyedThisWay
                | Instruction::ReturnCapturedDyingCreature
                | Instruction::ReturnTargetGraveyardCreatureToHand
                | Instruction::ReturnToOwnersHand
                | Instruction::RevealUntilMatch { .. }
                | Instruction::LookTopChoose { .. }
                | Instruction::ExileGroupFromZone { .. }
                | Instruction::ExileFromZone { .. }
                | Instruction::ExileChosenPermanents { .. }
                | Instruction::ExileTarget
                | Instruction::ExileGraveyardTarget
                | Instruction::ManifestDread
                | Instruction::Incubate { .. }
                | Instruction::SearchLibrary { .. }
        )
    )
}

fn lower_fight_or_exchange_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
    allow_planeswalker: bool,
    had_perm_target: bool,
) -> Result<MoveSubjectRef, &'static str> {
    let has_source = in_trigger || self_pronoun_valid;
    if has_source && is_self_pronoun(subj, builder, self_pronoun_valid) {
        return Ok(MoveSubjectRef::AbilitySource);
    }
    match subj {
        SubjectRefAst::SelfObject if has_source => Ok(MoveSubjectRef::AbilitySource),
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            let valid_bt = matches!(
                bt,
                BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::FilteredPermanent(_)
            ) || (allow_planeswalker && bt == BodyTarget::SinglePlaneswalker);
            if !valid_bt {
                return Err("oracle:unsupported-fight");
            }
            builder.bind_target(bt)?;
            Ok(MoveSubjectRef::TargetObject)
        }
        SubjectRefAst::TriggerSubject if in_trigger => Ok(MoveSubjectRef::TriggeringObject),
        SubjectRefAst::AttachedHost(_) if has_source => Ok(MoveSubjectRef::AttachedHost),
        SubjectRefAst::Anaphora(_) => {
            if had_perm_target {
                Ok(MoveSubjectRef::TargetObject)
            } else if last_instruction_created_tokens(&builder.instructions) {
                Ok(MoveSubjectRef::CreatedTokens)
            } else if in_trigger && !self_pronoun_valid {
                Ok(MoveSubjectRef::TriggeringObject)
            } else {
                Ok(MoveSubjectRef::LastMoved)
            }
        }
        _ => Err("oracle:unsupported-fight"),
    }
}

fn extract_resolving_player_from_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<ResolvingPlayer, &'static str> {
    match subj {
        SubjectRefAst::Player(p) => lower_resolving_player_in_context(*p, builder, in_trigger),
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SingleOpponent
            } else {
                BodyTarget::SinglePlayer
            })?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        _ => Err("oracle:unsupported-exchange"),
    }
}

fn parse_exchange_zone_from_subject(subj: &SubjectRefAst) -> Option<ExchangeZone> {
    match subj {
        SubjectRefAst::Anaphora(raw) => {
            let low = raw.trim().to_ascii_lowercase();
            if low.contains("hand") {
                Some(ExchangeZone::Hand)
            } else if low.contains("graveyard") {
                Some(ExchangeZone::Graveyard)
            } else if low.contains("library") {
                Some(ExchangeZone::Library)
            } else if low.contains("exile") {
                Some(ExchangeZone::Exile)
            } else {
                None
            }
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => match f.zone {
            Some(ZoneFilter::YourHand) => Some(ExchangeZone::Hand),
            Some(ZoneFilter::YourGraveyard) => Some(ExchangeZone::Graveyard),
            Some(ZoneFilter::YourLibrary) => Some(ExchangeZone::Library),
            Some(ZoneFilter::Exile) => Some(ExchangeZone::Exile),
            _ => None,
        },
        _ => None,
    }
}

fn parse_damage_recipient_phrase(
    raw: &str,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    let cleaned = raw
        .trim()
        .trim_end_matches('.')
        .strip_prefix("to ")
        .unwrap_or_else(|| raw.trim().trim_end_matches('.'))
        .trim();
    match cleaned {
        "a permanent or player"
        | "a creature, planeswalker, or player"
        | "a creature or player"
        | "any target" => Some(StaticDamageRecipient::Any),
        "an opponent" => Some(StaticDamageRecipient::Opponents),
        "an opponent or a permanent an opponent controls"
        | "an opponent or a permanent they control" => {
            Some(StaticDamageRecipient::OpponentOrOpponentPermanent)
        }
        "you" => Some(StaticDamageRecipient::Controller),
        "you or a permanent you control" | "you and/or permanents you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledPermanent)
        }
        "you or a creature you control" | "you or creatures you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledCreatures)
        }
        "a creature you control" | "creatures you control" | "any creature you control" => {
            Some(StaticDamageRecipient::ControlledCreatures)
        }
        "a creature an opponent controls" | "creatures an opponent controls" => {
            Some(StaticDamageRecipient::OpponentCreatures)
        }
        "a creature" | "any creature" | "creatures" => Some(StaticDamageRecipient::Creatures),
        "a player" | "any player" => Some(StaticDamageRecipient::Players),
        "~" | "this creature" | "this permanent" if has_source => {
            Some(StaticDamageRecipient::SelfObject)
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent" if has_source => {
            Some(StaticDamageRecipient::AttachedHost)
        }
        other => {
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let obj = lower_object_filter(&ast_f, vocabulary).ok()?;
            if obj.has_target_player() {
                return None;
            }
            Some(StaticDamageRecipient::MatchingPermanent(obj))
        }
    }
}

fn parse_damage_redirect_target(
    raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<DamageRedirectTarget, &'static str>> {
    let cleaned = raw.trim().trim_end_matches('.').trim();
    match cleaned {
        "~" | "this creature" | "this permanent" if has_source => {
            Some(Ok(DamageRedirectTarget::AbilitySource))
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent" if has_source => {
            Some(Ok(DamageRedirectTarget::AttachedHost))
        }
        "you" => Some(Ok(DamageRedirectTarget::Controller)),
        "its controller"
        | "that creature's controller"
        | "that source's controller"
        | "that permanent's controller" => Some(Ok(DamageRedirectTarget::DamageSourceController)),
        "that creature" | "that permanent" | "it" if is_permanent_body_target(builder.target) => {
            Some(Ok(DamageRedirectTarget::TargetObject))
        }
        other => {
            let filter_raw = other.strip_prefix("target ")?;
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                filter_raw,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let bt = lower_battlefield_body_target(&ast_f, vocabulary).ok()?;
            if !is_permanent_body_target(bt) {
                return None;
            }
            Some(
                builder
                    .bind_target(bt)
                    .map(|()| DamageRedirectTarget::TargetObject),
            )
        }
    }
}

fn lower_redirect_from_subject(
    subj: &SubjectRefAst,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    match subj {
        SubjectRefAst::Player(PlayerRefAst::You) => Some(StaticDamageRecipient::Controller),
        SubjectRefAst::SelfObject if has_source => Some(StaticDamageRecipient::SelfObject),
        SubjectRefAst::AttachedHost(_) if has_source => Some(StaticDamageRecipient::AttachedHost),
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            if f.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    f,
                    FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                match f.controller {
                    None => return Some(StaticDamageRecipient::Creatures),
                    Some(ControllerRelation::You) => {
                        return Some(StaticDamageRecipient::ControlledCreatures);
                    }
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        return Some(StaticDamageRecipient::OpponentCreatures);
                    }
                    _ => {}
                }
            }
            let obj = lower_object_filter(f, vocabulary).ok()?;
            if obj.has_target_player() {
                return None;
            }
            Some(StaticDamageRecipient::MatchingPermanent(obj))
        }
        SubjectRefAst::Compound(items) if items.len() == 2 => {
            if matches!(items[0], SubjectRefAst::Player(PlayerRefAst::You))
                && let SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) =
                    &items[1]
                && f.zone.is_none()
                && f.controller == Some(ControllerRelation::You)
            {
                if f.any_types == AstCardTypeMask::CREATURE {
                    return Some(StaticDamageRecipient::ControllerOrControlledCreatures);
                }
                if f.any_permanent || f.any_types.is_empty() {
                    return Some(StaticDamageRecipient::ControllerOrControlledPermanent);
                }
            }
            None
        }
        _ => None,
    }
}

fn lower_redirect_to_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<DamageRedirectTarget, &'static str> {
    match subj {
        SubjectRefAst::SelfObject if has_source => Ok(DamageRedirectTarget::AbilitySource),
        SubjectRefAst::AttachedHost(_) if has_source => Ok(DamageRedirectTarget::AttachedHost),
        SubjectRefAst::Player(PlayerRefAst::You) => Ok(DamageRedirectTarget::Controller),
        SubjectRefAst::Player(PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController) => {
            Ok(DamageRedirectTarget::DamageSourceController)
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            if !is_permanent_body_target(bt) {
                return Err("oracle:unsupported-redirect-damage");
            }
            builder.bind_target(bt)?;
            Ok(DamageRedirectTarget::TargetObject)
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            Ok(DamageRedirectTarget::TargetObject)
        }
        _ => Err("oracle:unsupported-redirect-damage"),
    }
}

fn extract_redirect_destination_phrase(rep_norm: &str) -> Option<&str> {
    for prefix in [
        "that creature deals that damage to ",
        "that source deals that damage to ",
        "it deals that damage to ",
        "that damage is dealt to ",
        "it's dealt to ",
        "it is dealt to ",
        "you may have that damage dealt to ",
        "you may have that damage be dealt to ",
        "you may have it dealt to ",
        "deal that damage to ",
        "is dealt to ",
        "are dealt to ",
        "be dealt to ",
        "dealt to ",
    ] {
        if let Some(rest) = rep_norm.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }
    None
}

fn try_lower_redirect_next_damage(
    subj_low: &str,
    ev_low: &str,
    rep_low: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<(), &'static str>> {
    let rep_no_dot = rep_low.trim_end_matches('.').trim();
    let rep_no_instead = rep_no_dot
        .strip_suffix(" instead")
        .unwrap_or(rep_no_dot)
        .trim();
    let rep_norm = rep_no_instead
        .strip_suffix(" this turn")
        .unwrap_or(rep_no_instead)
        .trim();
    let to_raw = extract_redirect_destination_phrase(rep_norm)?;

    if ev_low == "next_time_this_turn" {
        let (src_raw, dst_raw) = subj_low.split_once(" would deal ")?;
        if !dst_raw.contains("damage") {
            return None;
        }
        let combat_only = dst_raw.contains("combat damage");
        let after_dmg = dst_raw
            .split_once("combat damage")
            .or_else(|| dst_raw.split_once("damage"))
            .map_or("", |(_, r)| r.trim());
        let from = parse_damage_recipient_phrase(after_dmg, has_source, vocabulary)?;
        if to_raw == "itself" {
            let filter_raw = src_raw.trim().strip_prefix("target ")?;
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                filter_raw,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let bt = lower_battlefield_body_target(&ast_f, vocabulary).ok()?;
            if !is_permanent_body_target(bt) {
                return None;
            }
            let src_obj = lower_object_filter(&ast_f, vocabulary).ok()?;
            if src_obj.has_target_player() {
                return None;
            }
            if let Err(e) = builder.bind_target(bt) {
                return Some(Err(e));
            }
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                    source: Some(src_obj),
                    from,
                    to: DamageRedirectTarget::TargetObject,
                    combat_only,
                    amount: None,
                    one_shot: true,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            return Some(Ok(()));
        }
        let (source, chosen_color) = parse_next_time_damage_source(src_raw, vocabulary)?;
        if chosen_color {
            return None;
        }
        let to = match parse_damage_redirect_target(to_raw, builder, has_source, vocabulary)? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        builder.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                from,
                to,
                combat_only,
                amount: None,
                one_shot: true,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        return Some(Ok(()));
    }

    let ev_norm = ev_low
        .trim_end_matches('.')
        .strip_suffix(" this turn")
        .unwrap_or_else(|| ev_low.trim_end_matches('.'))
        .trim();

    if let Some((after_next, fallback_ev)) = subj_low
        .strip_prefix("the next ")
        .map(|rest| (rest, ev_norm))
        .or_else(|| ev_norm.strip_prefix("the next ").map(|rest| (rest, "")))
        && let Some((amt_tok, kind_rest)) = after_next.split_once(' ')
        && let Some(n) = parse_small_number_word(amt_tok)
        && n > 0
        && kind_rest.contains("damage")
    {
        let combat_only = kind_rest.contains("combat damage");
        let after_dealt = fallback_ev
            .strip_prefix("be dealt to ")
            .or_else(|| fallback_ev.strip_prefix("dealt to "))
            .or_else(|| kind_rest.split_once("be dealt to ").map(|(_, r)| r))
            .or_else(|| kind_rest.split_once("dealt to ").map(|(_, r)| r))?
            .trim();
        let after_dealt = after_dealt
            .strip_suffix(" this turn")
            .unwrap_or(after_dealt)
            .trim();
        let (from_part, source) = if let Some((f_part, s_part)) = after_dealt
            .split_once(" by ")
            .or_else(|| after_dealt.split_once(" from "))
        {
            let (src, chosen_color) = parse_next_time_damage_source(s_part, vocabulary)?;
            if chosen_color {
                return None;
            }
            (f_part.trim(), src)
        } else {
            (after_dealt, None)
        };
        let from = parse_damage_recipient_phrase(from_part, has_source, vocabulary)?;
        let to = match parse_damage_redirect_target(to_raw, builder, has_source, vocabulary)? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        builder.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                from,
                to,
                combat_only,
                amount: Some(i64::from(n).into()),
                one_shot: false,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        return Some(Ok(()));
    }

    if matches!(
        subj_low,
        "damage" | "all damage" | "combat damage" | "all combat damage"
    ) && let Some(after_dealt) = ev_norm.strip_prefix("be dealt to ")
    {
        let combat_only = subj_low.contains("combat");
        let (from_part, source) = if let Some((f_part, s_part)) = after_dealt.split_once(" by ") {
            let (src, chosen_color) = parse_next_time_damage_source(s_part, vocabulary)?;
            if chosen_color {
                return None;
            }
            (f_part.trim(), src)
        } else {
            (after_dealt.trim(), None)
        };
        let from = parse_damage_recipient_phrase(from_part, has_source, vocabulary)?;
        let to = match parse_damage_redirect_target(to_raw, builder, has_source, vocabulary)? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        builder.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                from,
                to,
                combat_only,
                amount: None,
                one_shot: false,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        return Some(Ok(()));
    }

    if let Some((combat_only, from_raw)) = ev_norm
        .strip_prefix("deal combat damage to ")
        .map(|r| (true, r))
        .or_else(|| ev_norm.strip_prefix("deal damage to ").map(|r| (false, r)))
    {
        let from = parse_damage_recipient_phrase(from_raw, has_source, vocabulary)?;
        let (source, chosen_color) = parse_next_time_damage_source(subj_low, vocabulary)?;
        if chosen_color {
            return None;
        }
        let to = match parse_damage_redirect_target(to_raw, builder, has_source, vocabulary)? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        builder.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                from,
                to,
                combat_only,
                amount: None,
                one_shot: false,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        return Some(Ok(()));
    }

    None
}

fn parse_copy_exceptions(
    exception: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<CopyExceptions, &'static str> {
    let mut out = CopyExceptions::default();
    let Some(raw) = exception.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(out);
    };
    let mut raw_without_quotes = String::with_capacity(raw.len());
    let mut rest_scan = raw;
    while let Some(q_start) = rest_scan.find('"') {
        raw_without_quotes.push_str(&rest_scan[..q_start]);
        let after_open = &rest_scan[q_start + 1..];
        let Some(q_end) = after_open.find('"') else {
            return Err("oracle:unsupported-become-copy-exception");
        };
        let quoted = after_open[..q_end].trim();
        if quoted.is_empty() {
            return Err("oracle:unsupported-become-copy-exception");
        }
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let synthetic = compile_card(
            &CardInput {
                oracle_id: "",
                name: "Synthetic",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Synthetic",
                    type_line: "Creature",
                    oracle_text: quoted,
                }],
            },
            &compiler_vocab,
        );
        if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
            return Err("oracle:unsupported-become-copy-exception");
        }
        let granted_parts = lower_granted_ability_face_parts(&synthetic.faces[0], vocabulary, ids)?;
        if granted_parts.is_empty() {
            return Err("oracle:unsupported-become-copy-exception");
        }
        for part in granted_parts {
            match part {
                StaticPart::GrantActivated(act) => out.extra_activated.push(*act),
                StaticPart::GrantTrigger(tr) => out.extra_triggered.push(*tr),
                StaticPart::Abilities(ab)
                    if ab.removal.is_none()
                        && ab.suppressed_keywords.is_empty()
                        && ab.granted_exalted == 0
                        && !ab.grants_unblockable
                        && ab.granted_trigger.is_none()
                        && ab.granted_toxic.is_none()
                        && ab.granted_combat_keywords.is_empty()
                        && !ab.granted_keywords.is_empty() =>
                {
                    out.extra_keywords.extend(ab.granted_keywords);
                }
                _ => return Err("oracle:unsupported-become-copy-exception"),
            }
        }
        raw_without_quotes.push(' ');
        rest_scan = &after_open[q_end + 1..];
    }
    raw_without_quotes.push_str(rest_scan);
    let low = raw_without_quotes.to_ascii_lowercase();
    if low.contains("isn't legendary")
        || low.contains("aren't legendary")
        || low.contains("not legendary")
    {
        out.nonlegendary = true;
    }
    if low.contains("its name is still")
        || low.contains("their names are still")
        || low.contains("except its name is")
        || low.contains("its name is ~")
        || low.contains("his name is ~")
        || low.contains("her name is ~")
        || low.contains("his name is still")
        || low.contains("her name is still")
        || low.contains("keeps its name")
    {
        out.keep_name = true;
    }
    if low.contains("in addition to its other types")
        || low.contains("in addition to their other types")
        || low.contains("still a ")
        || low.contains("still an ")
        || low.contains("are still ")
    {
        out.keep_types = true;
    }
    if low.contains("in addition to its other abilities")
        || low.contains("in addition to their other abilities")
        || low.contains("except it has this ability")
        || low.contains("it has this ability")
        || low.contains("has this ability")
        || low.contains("it gains this ability")
        || low.contains("gains this ability")
        || low.contains("except it keeps its abilities")
        || low.contains("except it keeps its other abilities")
        || low.contains("except it keeps all its abilities")
        || low.contains("keeps its abilities")
        || low.contains("keeps its other abilities")
        || low.contains("keep their abilities")
    {
        out.keep_abilities = true;
    }
    let mut remainder = low.clone();
    for (phrase, kw) in [
        ("first strike", KeywordAbility::FirstStrike),
        ("double strike", KeywordAbility::DoubleStrike),
        ("flying", KeywordAbility::Flying),
        ("haste", KeywordAbility::Haste),
        ("trample", KeywordAbility::Trample),
        ("vigilance", KeywordAbility::Vigilance),
        ("lifelink", KeywordAbility::Lifelink),
        ("deathtouch", KeywordAbility::Deathtouch),
        ("hexproof", KeywordAbility::Hexproof),
        ("shroud", KeywordAbility::Shroud),
        ("indestructible", KeywordAbility::Indestructible),
        ("menace", KeywordAbility::Menace),
        ("reach", KeywordAbility::Reach),
        ("defender", KeywordAbility::Defender),
        ("flash", KeywordAbility::Flash),
        ("fear", KeywordAbility::Fear),
        ("intimidate", KeywordAbility::Intimidate),
        ("skulk", KeywordAbility::Skulk),
        ("shadow", KeywordAbility::Shadow),
        ("horsemanship", KeywordAbility::Horsemanship),
        ("wither", KeywordAbility::Wither),
        ("infect", KeywordAbility::Infect),
        ("changeling", KeywordAbility::Changeling),
        ("decayed", KeywordAbility::Decayed),
        ("phasing", KeywordAbility::Phasing),
        ("plainswalk", KeywordAbility::Plainswalk),
        ("islandwalk", KeywordAbility::Islandwalk),
        ("swampwalk", KeywordAbility::Swampwalk),
        ("mountainwalk", KeywordAbility::Mountainwalk),
        ("forestwalk", KeywordAbility::Forestwalk),
    ] {
        if remainder.contains(phrase) {
            out.extra_keywords.insert(kw);
            remainder = remainder.replace(phrase, " ");
        }
    }
    for pat in [
        "except it isn't legendary",
        "except they aren't legendary",
        "except it's not legendary",
        "except they're not legendary",
        "it isn't legendary",
        "they aren't legendary",
        "it's not legendary",
        "they're not legendary",
        "not legendary",
        "except its name is still ~",
        "its name is still ~",
        "except its name is ~",
        "its name is ~",
        "except his name is still ~",
        "his name is still ~",
        "except his name is ~",
        "his name is ~",
        "except her name is still ~",
        "her name is still ~",
        "except her name is ~",
        "her name is ~",
        "except their names are still ~",
        "their names are still ~",
        "it keeps its name",
        "keeps its name",
        "in addition to its other types",
        "in addition to their other types",
        "in addition to its other abilities",
        "in addition to their other abilities",
        "except it has this ability",
        "except it gains this ability",
        "it has this ability",
        "it gains this ability",
        "has this ability",
        "gains this ability",
        "except it keeps its other abilities",
        "except it keeps all its abilities",
        "except it keeps its abilities",
        "it keeps its other abilities",
        "keeps its other abilities",
        "keeps its abilities",
        "they keep their abilities",
        "keep their abilities",
        "except it's still a land",
        "it's still a land",
        "it's still an artifact",
        "it's still an enchantment",
        "it's still a creature",
        "except it has base power and toughness",
        "except they have base power and toughness",
        "it has base power and toughness",
        "they have base power and toughness",
        "with base power and toughness",
        "base power and toughness",
        "except it becomes a",
        "except it becomes an",
        "it becomes a",
        "it becomes an",
        "except he's a",
        "except he's an",
        "except he's",
        "except she's a",
        "except she's an",
        "except she's",
        "except it has",
        "except they have",
        "except it's a",
        "except it's an",
        "except it's",
        "except they're",
        "except it gains",
        "except they gain",
        "except",
        "it has",
        "they have",
        "it gains",
        "they gain",
        "it's a",
        "it's an",
        "it's",
        "he's a",
        "he's an",
        "he's",
        "she's a",
        "she's an",
        "she's",
        "they're",
        "has",
        "gains",
        "and",
        ",",
        ".",
    ] {
        remainder = remainder.replace(pat, " ");
    }
    for raw_tok in remainder.split_whitespace() {
        let tok = raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '-');
        if tok.is_empty()
            || matches!(
                tok,
                "a" | "an" | "is" | "are" | "with" | "still" | "also" | "that" | "that's"
            )
        {
            continue;
        }
        if let Some((ps, ts)) = tok.split_once('/')
            && let (Ok(p), Ok(t)) = (ps.parse::<i64>(), ts.parse::<i64>())
            && out.override_pt.is_none()
        {
            out.override_pt = Some(PowerToughness {
                power: p.into(),
                toughness: t.into(),
            });
            continue;
        }
        match tok {
            "artifact" | "artifacts" => {
                out.extra_types.insert(CardType::Artifact);
                continue;
            }
            "enchantment" | "enchantments" => {
                out.extra_types.insert(CardType::Enchantment);
                continue;
            }
            "creature" | "creatures" => {
                out.extra_types.insert(CardType::Creature);
                continue;
            }
            "land" | "lands" => {
                out.extra_types.insert(CardType::Land);
                continue;
            }
            "planeswalker" | "planeswalkers" => {
                out.extra_types.insert(CardType::Planeswalker);
                continue;
            }
            "instant" | "instants" => {
                out.extra_types.insert(CardType::Instant);
                continue;
            }
            "sorcery" | "sorceries" => {
                out.extra_types.insert(CardType::Sorcery);
                continue;
            }
            "battle" | "battles" => {
                out.extra_types.insert(CardType::Battle);
                continue;
            }
            "treasure" => {
                out.extra_artifact_subtypes
                    .insert(ArtifactSubtype::Treasure);
                continue;
            }
            "food" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Food);
                continue;
            }
            "clue" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Clue);
                continue;
            }
            "equipment" => {
                out.extra_artifact_subtypes
                    .insert(ArtifactSubtype::Equipment);
                continue;
            }
            "blood" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Blood);
                continue;
            }
            "gold" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Gold);
                continue;
            }
            "junk" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Junk);
                continue;
            }
            "map" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Map);
                continue;
            }
            "powerstone" => {
                out.extra_artifact_subtypes
                    .insert(ArtifactSubtype::Powerstone);
                continue;
            }
            "vehicle" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Vehicle);
                continue;
            }
            "shard" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Shard);
                continue;
            }
            "lander" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Lander);
                continue;
            }
            "mutagen" => {
                out.extra_artifact_subtypes.insert(ArtifactSubtype::Mutagen);
                continue;
            }
            "incubator" => {
                out.extra_artifact_subtypes
                    .insert(ArtifactSubtype::Incubator);
                continue;
            }
            "fortification" => {
                out.extra_artifact_subtypes
                    .insert(ArtifactSubtype::Fortification);
                continue;
            }
            "spacecraft" => {
                out.extra_artifact_subtypes
                    .insert(ArtifactSubtype::Spacecraft);
                continue;
            }
            _ => {}
        }
        if let Some(ct) = resolve_protection_subtype(tok, vocabulary) {
            out.extra_creature_subtypes.insert(ct);
            continue;
        }
        return Err("oracle:unsupported-become-copy-exception");
    }
    Ok(out)
}

fn try_lower_temp_battlefield_static(
    selector: &PermanentSelector,
    parts: &StaticParts,
    duration: ResolvedDuration,
    self_pronoun_valid: bool,
) -> Option<Instruction> {
    use spellwake_engine::characteristics::{CombatSelector, ControlRecipient, NameFilter};
    if parts.0.is_empty() {
        return None;
    }
    let mut resolved = Vec::with_capacity(parts.0.len());
    for part in &parts.0 {
        let rp = match part {
            StaticPart::ControlSourceController => {
                ResolvedPartSpec::Control(ControlRecipient::ResolvingController)
            }
            StaticPart::Abilities(ab) => ResolvedPartSpec::Abilities(ab.clone()),
            StaticPart::BlockingRestriction(br) => {
                ResolvedPartSpec::BlockingRestriction(br.clone())
            }
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(p),
                toughness: NumericExpression::Literal(t),
            }) => ResolvedPartSpec::ModifyPT(PowerToughness {
                power: p.clone(),
                toughness: t.clone(),
            }),
            StaticPart::SetPT(NumericPair {
                power: NumericExpression::Literal(p),
                toughness: NumericExpression::Literal(t),
            }) => ResolvedPartSpec::SetPT(PowerToughness {
                power: p.clone(),
                toughness: t.clone(),
            }),
            StaticPart::SwitchPT => ResolvedPartSpec::SwitchPT,
            StaticPart::GrantTrigger(tr) => ResolvedPartSpec::GrantTrigger(tr.clone()),
            StaticPart::GrantActivated(act) => ResolvedPartSpec::GrantActivated(act.clone()),
            StaticPart::GrantActivatedRestricted {
                ability,
                restriction,
            } => ResolvedPartSpec::GrantActivatedRestricted {
                ability: ability.clone(),
                restriction: restriction.clone(),
            },
            StaticPart::GrantAnyColorMana => ResolvedPartSpec::GrantAnyColorMana,
            StaticPart::GrantFixedColorMana(mt) => ResolvedPartSpec::GrantFixedColorMana(*mt),
            StaticPart::Color(op) => match op {
                ColorOperation::Set(StaticColorOperand::Literal(c)) => {
                    ResolvedPartSpec::Color(ColorOperation::Set(c.clone()))
                }
                ColorOperation::Add(StaticColorOperand::Literal(c)) => {
                    ResolvedPartSpec::Color(ColorOperation::Add(c.clone()))
                }
                _ => return None,
            },
            _ => return None,
        };
        resolved.push(rp);
    }
    let PermanentSelector {
        combat_role,
        requires_untapped,
        requires_tapped,
        sole_controlled_creature,
        token,
        required_supertypes,
        forbidden_supertypes,
        required_types,
        any_types,
        forbidden_types,
        creature_subtype,
        any_creature_subtypes,
        forbidden_creature_subtype,
        land_subtype,
        color,
        name,
        controller,
        attached_to_source,
        while_controlling_own_commander,
        plus_one_counter,
        minus_one_counter,
        minimum_plus_one_counters,
        any_counter,
        source_has_imprinted_creature,
        controlled_creatures_and_typed_tokens,
        exclude_auras,
        artifact_subtype,
        matches_source_entry_type,
        controller_type_threshold,
        source_condition,
        requires_flying,
        required_keyword,
        forbidden_keyword,
        forbidden_keywords,
        equipped_by_at_least,
        requires_enchanted,
        min_power,
        max_power,
        min_toughness,
        max_toughness,
        min_mana_value,
        max_mana_value,
        devotion_below,
        station_at,
        exclude_source,
    } = selector;
    if *sole_controlled_creature
        || *while_controlling_own_commander
        || *source_has_imprinted_creature
        || controlled_creatures_and_typed_tokens.is_some()
        || *matches_source_entry_type
        || controller_type_threshold.is_some()
        || source_condition.is_some()
        || devotion_below.is_some()
        || station_at.is_some()
        || *name != NameFilter::Any
        || !any_creature_subtypes.is_empty()
        || *minimum_plus_one_counters > 0
        || *equipped_by_at_least > 0
        || *requires_enchanted
        || !forbidden_keywords.is_empty()
        || (*requires_flying && required_keyword.is_some())
        || (*requires_tapped && *requires_untapped)
        || *plus_one_counter
        || *minus_one_counter
        || *any_counter
        || min_power.is_some()
        || max_power.is_some()
        || min_toughness.is_some()
        || max_toughness.is_some()
        || min_mana_value.is_some()
        || max_mana_value.is_some()
    {
        return None;
    }
    if *controller == ControllerSelector::SourceItself {
        if !self_pronoun_valid
            || combat_role.is_some()
            || *requires_untapped
            || *requires_tapped
            || token.is_some()
            || !required_supertypes.is_empty()
            || !forbidden_supertypes.is_empty()
            || !required_types.is_empty()
            || !any_types.is_empty()
            || !forbidden_types.is_empty()
            || creature_subtype.is_some()
            || forbidden_creature_subtype.is_some()
            || land_subtype.is_some()
            || *color != ColorFilter::Any
            || *attached_to_source
            || *exclude_auras
            || artifact_subtype.is_some()
            || *requires_flying
            || required_keyword.is_some()
            || forbidden_keyword.is_some()
            || *exclude_source
        {
            return None;
        }
        return Some(Instruction::ChangeAbilitySource {
            parts: ResolvedPartsSpec(resolved),
            duration,
        });
    }
    let mut clause = ObjectClause::any();
    clause.controller = match controller {
        ControllerSelector::Any => ControllerFilter::Any,
        ControllerSelector::SourceController => ControllerFilter::You,
        ControllerSelector::Opponents => ControllerFilter::Opponent,
        _ => return None,
    };
    if *attached_to_source {
        if !self_pronoun_valid || *exclude_source {
            return None;
        }
        clause = clause.attached_to_self();
    } else if *exclude_source {
        if !self_pronoun_valid {
            return None;
        }
        clause.source_relation = SourceRelation::OtherThanAbilitySource;
    }
    for t in required_types {
        clause = clause.required_type(*t);
    }
    for t in any_types {
        clause = clause.any_types(CardTypeMask::from_card_type(*t));
    }
    for t in forbidden_types {
        clause = clause.forbid_types(CardTypeMask::from_card_type(*t));
    }
    for st in required_supertypes {
        clause.required_supertypes = clause
            .required_supertypes
            .union(SupertypeMask::from_supertype(*st));
    }
    for st in forbidden_supertypes {
        clause.forbidden_supertypes = clause
            .forbidden_supertypes
            .union(SupertypeMask::from_supertype(*st));
    }
    let mut sub_count = 0u8;
    if let Some(ct) = creature_subtype {
        clause.subtypes = SubtypeFilter::Has(*ct);
        sub_count += 1;
    }
    if let Some(ct) = forbidden_creature_subtype {
        clause.subtypes = SubtypeFilter::Lacks(*ct);
        sub_count += 1;
    }
    if let Some(lt) = land_subtype {
        clause.subtypes = SubtypeFilter::Land(*lt);
        sub_count += 1;
    }
    if let Some(at) = artifact_subtype {
        clause.subtypes = SubtypeFilter::Artifact(*at);
        sub_count += 1;
    }
    if *exclude_auras {
        return None;
    }
    if sub_count > 1 {
        return None;
    }
    clause.color = match color {
        ColorFilter::Any => ObjectColorFilter::Any,
        ColorFilter::Colorless => ObjectColorFilter::Colorless,
        ColorFilter::Colored => ObjectColorFilter::Colored,
        ColorFilter::Monocolored => ObjectColorFilter::Monocolored,
        ColorFilter::Multicolored => ObjectColorFilter::Multicolored,
        ColorFilter::Has(c) => ObjectColorFilter::has(*c),
        ColorFilter::Lacks(c) => ObjectColorFilter::lacks(*c),
        _ => return None,
    };
    clause.combat = match combat_role {
        None => CombatFilter::Any,
        Some(CombatSelector::Attacking) => CombatFilter::Attacking,
        Some(CombatSelector::Blocking) => CombatFilter::Blocking,
        Some(CombatSelector::AttackingOrBlocking) => CombatFilter::AttackingOrBlocking,
        Some(CombatSelector::OutsideCombat) => return None,
    };
    if *requires_tapped {
        clause.tapped = Some(true);
    } else if *requires_untapped {
        clause.tapped = Some(false);
    }
    clause.token = *token;
    if *requires_flying {
        clause.keyword = Some(KeywordAbility::Flying);
    } else if let Some(kw) = required_keyword {
        clause.keyword = Some(*kw);
    }
    clause.forbidden_keyword = *forbidden_keyword;
    Some(Instruction::ChangeGroup {
        group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(clause))),
        parts: ResolvedPartsSpec(resolved),
        duration,
    })
}

fn parse_lib_placement(dest: &str, allow_your_library: bool) -> Option<LibraryPlacement> {
    if dest.contains("after_shuffle") || dest.contains("random") {
        return None;
    }
    if dest == "owner_choice_top_or_bottom_of_library"
        || dest.contains("their choice of the top or bottom")
        || dest.contains("owner's choice of the top or bottom")
        || dest.contains("that player's choice of the top or bottom")
    {
        return Some(LibraryPlacement::ChoiceTopOrBottom {
            chooser: ResolvingPlayer::TargetOwner,
        });
    }
    if dest.contains("your choice of the top or bottom")
        || dest == "the top or bottom of your library"
        || dest == "top or bottom of your library"
    {
        return Some(LibraryPlacement::ChoiceTopOrBottom {
            chooser: ResolvingPlayer::Controller,
        });
    }
    if dest.contains("choice") || dest.contains("top or bottom") || dest.contains("bottom or top") {
        return None;
    }
    match dest {
        "top_of_library" => return Some(LibraryPlacement::Top),
        "bottom_of_library" => return Some(LibraryPlacement::Bottom),
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

fn is_single_card_subject(subject: &SubjectRefAst) -> bool {
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

fn parse_lib_placement_for_subject(
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

fn is_valid_put_target_in_library_target(bt: &BodyTarget) -> bool {
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
struct BattlefieldDestFlags {
    tapped: bool,
    attacking: bool,
    transformed: bool,
    under_owner_control: bool,
}

fn parse_battlefield_dest(dest: &str) -> Option<BattlefieldDestFlags> {
    if !dest.contains("battlefield") {
        return None;
    }
    if dest.contains("face down")
        || dest.contains("face-down")
        || dest.contains("blocking")
        || dest.contains("as a copy")
        || dest.contains("as a ")
        || dest.contains("melded")
        || dest.contains("attached to")
        || dest.contains(" with ")
    {
        return None;
    }
    let under_owner_control = dest.contains("owner's control") || dest.contains("owners' control");
    if dest.contains("under ") && !under_owner_control && !dest.contains("under your control") {
        return None;
    }
    let tapped = dest.contains("tapped") && !dest.contains("untapped");
    let attacking = dest.contains("attacking");
    let transformed = dest.contains("transformed") || dest.contains("converted");
    Some(BattlefieldDestFlags {
        tapped,
        attacking,
        transformed,
        under_owner_control,
    })
}

fn is_battlefield_owner_control_dest(dest: &str) -> bool {
    parse_battlefield_dest(dest).is_some_and(|flags| flags.under_owner_control)
}

fn parse_card_type_word(word: &str) -> Option<CardType> {
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

fn parse_card_type_choice_list(cleaned: &str) -> Option<Vec<CardType>> {
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

enum ParsedChoiceKind {
    Domain(spellwake_engine::program::ResolutionChoiceDomain),
    CreatureType { excluding: BTreeSet<CreatureTypeId> },
}

fn parse_choice_kind(
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
        "an opponent" | "an opponent at random" => {
            Ok(ParsedChoiceKind::Domain(ResolutionChoiceDomain::Player {
                opponents_only: true,
            }))
        }
        "a player" | "another player" | "a player at random" => {
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

fn choice_domain_to_player_option_branches(
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

fn emit_choice_for_player_ref(
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
            let options = choice_domain_to_player_option_branches(&domain)
                .ok_or("oracle:unsupported-choose-option")?;
            let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::ChooseOptionForPlayers { players, options });
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

fn classify_graveyard_group(
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

fn is_self_anaphora_label(raw: &str) -> bool {
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

fn is_self_anaphora_ref(subject: &SubjectRefAst) -> bool {
    matches!(subject, SubjectRefAst::Anaphora(p) if is_self_anaphora_label(p))
}

fn value_expr_ast_has_self_anaphora(expr: &ValueExprAst) -> bool {
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

fn rewrite_value_expr_ast_self_anaphora(expr: &ValueExprAst) -> ValueExprAst {
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

fn infer_instruction_actor(ins: &InstructionAst) -> Option<PlayerRefAst> {
    match ins {
        InstructionAst::Discard { player, .. }
        | InstructionAst::DiscardHand { player }
        | InstructionAst::DiscardUnless { player, .. }
        | InstructionAst::Sacrifice { player, .. }
        | InstructionAst::SacrificeAnyNumber { player, .. }
        | InstructionAst::PlayerPayCost { player, .. }
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
        | InstructionAst::RevealRandomFromHand { player }
        | InstructionAst::LookAtHand { player }
        | InstructionAst::AddMana { player, .. }
        | InstructionAst::Investigate { player, .. }
        | InstructionAst::Blight { player, .. }
        | InstructionAst::OptionalPlayer { player, .. } => Some(*player),
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

fn splice_probed_body(builder: &mut BodyBuilder, probed: BodyBuilder) -> Result<(), &'static str> {
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

fn is_graveyard_body_target(target: BodyTarget) -> bool {
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

fn is_hand_return_dest(dest: &str) -> bool {
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

fn parse_return_with_counter(
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

fn resolve_return_select_zone(
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

fn fold_except_into_filter(
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

fn apply_except_to_subject(
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
fn lower_search_library_instruction<'a>(
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

    let resolved_where_x = where_x.map(|e| resolve_x(e, bound_x));
    let mut search = lower_library_search_ext(
        library,
        resolve_x(count, bound_x),
        filter,
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

fn strip_grant_kw_trailing_note(raw: &str) -> String {
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

fn strip_quoted_end_step_token_cleanup(
    remainder: &mut String,
    sacrifice_at_end_step: &mut bool,
    exile_at_end_step: &mut bool,
) {
    while let Some(q_start) = remainder.find('"') {
        let after_open = &remainder[q_start + 1..];
        let Some(rel_end) = after_open.find('"') else {
            break;
        };
        let inner = after_open[..rel_end].trim().trim_end_matches('.');
        let matched_action = [
            "at the beginning of the next end step, ",
            "at the beginning of your next end step, ",
            "at the beginning of the end step, ",
            "at the beginning of your end step, ",
        ]
        .iter()
        .find_map(|prefix| inner.strip_prefix(prefix))
        .and_then(|tail| match tail.trim() {
            "sacrifice this permanent"
            | "sacrifice this token"
            | "sacrifice this creature"
            | "sacrifice it"
            | "sacrifice them" => Some(true),
            "exile this permanent"
            | "exile this token"
            | "exile this creature"
            | "exile it"
            | "exile them" => Some(false),
            _ => None,
        });
        let Some(is_sac) = matched_action else {
            break;
        };
        if is_sac {
            *sacrifice_at_end_step = true;
        } else {
            *exile_at_end_step = true;
        }
        let q_end = q_start + 1 + rel_end + 1;
        let prefix = remainder[..q_start].trim_end();
        let cut_start = [
            "except it has",
            "except it gains",
            "except they have",
            "except they gain",
            "it has",
            "it gains",
            "they have",
            "they gain",
            "with",
        ]
        .iter()
        .find_map(|lead| prefix.strip_suffix(lead).map(|p| p.len()))
        .unwrap_or(q_start);
        remainder.replace_range(cut_start..q_end, " ");
    }
}

fn parse_suppressed_keyword_list(kws_raw: &str) -> Result<BTreeSet<KeywordAbility>, &'static str> {
    let cleaned = kws_raw
        .trim()
        .trim_end_matches('.')
        .replace(", or ", ",")
        .replace(", and ", ",")
        .replace(" or ", ",")
        .replace(" and ", ",");
    let mut suppressed = BTreeSet::new();
    for seg in cleaned.split(',') {
        let tok = seg.trim();
        if tok.is_empty() {
            continue;
        }
        let kw = lower_keyword_ability_name(tok).ok_or("oracle:unsupported-combat-mod")?;
        suppressed.insert(kw);
    }
    if suppressed.is_empty() {
        return Err("oracle:unsupported-combat-mod");
    }
    Ok(suppressed)
}

fn parse_untap_up_to_phrase(raw: &str) -> Option<(CardType, u16)> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let after = low.strip_prefix("up to ")?;
    let (num_tok, kind_tok) = after.split_once(' ')?;
    let n = parse_small_number_word(num_tok.trim())?;
    if n == 0 {
        return None;
    }
    let kind = parse_card_type_word(kind_tok.trim())?;
    if !kind.is_permanent() {
        return None;
    }
    Some((kind, n))
}

fn emit_resolved_parts_for_subject(
    subject: &SubjectRefAst,
    parts: ResolvedPartsSpec,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            for it in items {
                emit_resolved_parts_for_subject(
                    it,
                    parts.clone(),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )?;
            }
            Ok(())
        }
        s if (self_pronoun_valid || in_trigger)
            && is_self_pronoun(s, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts,
                duration: dur,
            });
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
                .push(Instruction::CreateContinuousEffect {
                    parts,
                    duration: dur,
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
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
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
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
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
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if filter.zone.is_none() => {
            let obj = lower_object_filter(filter, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                ))),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None
                && last_instruction_created_tokens(&builder.instructions) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::CreatedTokens),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None
                && last_instruction_moved_object(&builder.instructions) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::LastMoved),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None
                && let Some(group_obj) = builder.last_group_filter =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(group_obj)),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if builder.target == BodyTarget::None
                && in_trigger
                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            let obj = lower_object_filter(f, vocabulary)?;
            bind_filter_player_target(builder, &obj)?;
            builder.last_group_filter = Some(obj);
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(obj)),
                parts,
                duration: dur,
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-temp-static"),
    }
}

fn parse_type_removal_part(
    desc: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ResolvedPartSpec> {
    let low = desc.trim().trim_end_matches('.').to_ascii_lowercase();
    let rest = [
        "isn't a ",
        "isn't an ",
        "isn't ",
        "aren't ",
        "is no longer a ",
        "is no longer an ",
        "is no longer ",
        "are no longer ",
        "no longer a ",
        "no longer an ",
        "no longer ",
    ]
    .iter()
    .find_map(|prefix| low.strip_prefix(prefix))?
    .trim();
    if rest.is_empty() {
        return None;
    }
    let mut supertypes = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut land_subtypes = BTreeSet::new();
    let mut creature_subtypes = BTreeSet::new();
    for seg in rest
        .replace(", or ", ",")
        .replace(", and ", ",")
        .replace(" or ", ",")
        .replace(" and ", ",")
        .split(',')
    {
        let tok = seg
            .trim()
            .strip_prefix("a ")
            .or_else(|| seg.trim().strip_prefix("an "))
            .unwrap_or(seg.trim())
            .trim();
        if tok.is_empty() {
            continue;
        }
        match tok {
            "snow" => {
                supertypes.insert(Supertype::Snow);
            }
            "legendary" => {
                supertypes.insert(Supertype::Legendary);
            }
            "basic" => {
                supertypes.insert(Supertype::Basic);
            }
            "plains" => {
                land_subtypes.insert(LandType::Plains);
            }
            "island" | "islands" => {
                land_subtypes.insert(LandType::Island);
            }
            "swamp" | "swamps" => {
                land_subtypes.insert(LandType::Swamp);
            }
            "mountain" | "mountains" => {
                land_subtypes.insert(LandType::Mountain);
            }
            "forest" | "forests" => {
                land_subtypes.insert(LandType::Forest);
            }
            other => {
                if let Some(ct) = parse_card_type_word(other) {
                    types.insert(ct);
                } else {
                    let sub = resolve_protection_subtype(other, vocabulary)?;
                    creature_subtypes.insert(sub);
                }
            }
        }
    }
    if supertypes.is_empty()
        && types.is_empty()
        && land_subtypes.is_empty()
        && creature_subtypes.is_empty()
    {
        return None;
    }
    Some(ResolvedPartSpec::Type(TypeChangeSpec {
        land_subtypes: if land_subtypes.is_empty() {
            SubtypeOperation::Keep
        } else {
            SubtypeOperation::Remove(land_subtypes)
        },
        supertypes: if supertypes.is_empty() {
            TypeOperation::Keep
        } else {
            TypeOperation::Remove(supertypes)
        },
        types: if types.is_empty() {
            TypeOperation::Keep
        } else {
            TypeOperation::Remove(types)
        },
        subtypes: if creature_subtypes.is_empty() {
            SubtypeOperation::Keep
        } else {
            SubtypeOperation::Remove(creature_subtypes)
        },
    }))
}

fn convert_static_type_change_to_spec(
    stc: StaticTypeChange,
) -> Result<TypeChangeSpec, &'static str> {
    let land_subtypes = match stc.land_subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Add(s),
        SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Set(s),
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return Err("oracle:unsupported-layer-type"),
    };
    let subtypes = match stc.subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Set(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Set(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return Err("oracle:unsupported-layer-type"),
    };
    Ok(TypeChangeSpec {
        land_subtypes,
        supertypes: stc.supertypes,
        types: stc.types,
        subtypes,
    })
}

// Mirrors the StaticAbilityAst::ContinuousLayerEffect payload fields for temporary lowering.
#[allow(clippy::too_many_arguments)]
fn lower_continuous_layer_effect_parts(
    set_types: Option<&str>,
    additive_types: bool,
    base_pt: Option<&(String, String)>,
    set_colors: Option<&str>,
    grant_keywords: &[String],
    lose_keywords: &[String],
    lose_all_abilities: bool,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Result<Vec<ResolvedPartSpec>, &'static str> {
    let mut parts = Vec::new();
    let mut has_color_part = false;
    if let Some(desc) = set_types {
        if let Some(removal_part) = parse_type_removal_part(desc, vocabulary) {
            parts.push(removal_part);
        } else {
            let static_part = parse_static_type_part(desc, additive_types, vocabulary)?;
            match static_part {
                StaticPart::Type(stc) => {
                    parts.push(ResolvedPartSpec::Type(convert_static_type_change_to_spec(
                        stc,
                    )?));
                }
                StaticPart::TypeWithSubtypes {
                    base,
                    artifact_subtypes,
                    enchantment_subtypes,
                } => {
                    parts.push(ResolvedPartSpec::TypeWithSubtypes {
                        base: convert_static_type_change_to_spec(base)?,
                        artifact_subtypes,
                        enchantment_subtypes,
                    });
                }
                StaticPart::Color(ColorOperation::Set(StaticColorOperand::Literal(c))) => {
                    has_color_part = true;
                    parts.push(ResolvedPartSpec::Color(ColorOperation::Set(c)));
                }
                StaticPart::Color(ColorOperation::Add(StaticColorOperand::Literal(c))) => {
                    has_color_part = true;
                    parts.push(ResolvedPartSpec::Color(ColorOperation::Add(c)));
                }
                _ => return Err("oracle:unsupported-layer-type"),
            }
        }
    }
    if let Some(color_str) = set_colors {
        if has_color_part {
            return Err("oracle:unsupported-layer-color");
        }
        let low_cs = color_str.trim().trim_end_matches('.').to_ascii_lowercase();
        let additive_colors = low_cs.contains("in addition to its other colors")
            || low_cs.contains("in addition to their other colors");
        let core = low_cs
            .replace("in addition to its other colors", " ")
            .replace("in addition to their other colors", " ");
        let mut colors = BTreeSet::new();
        let mut saw_colorless = false;
        for raw_tok in core
            .replace(" and ", " ")
            .replace(',', " ")
            .split_whitespace()
        {
            match raw_tok {
                "white" => {
                    colors.insert(Color::White);
                }
                "blue" => {
                    colors.insert(Color::Blue);
                }
                "black" => {
                    colors.insert(Color::Black);
                }
                "red" => {
                    colors.insert(Color::Red);
                }
                "green" => {
                    colors.insert(Color::Green);
                }
                "colorless" => {
                    saw_colorless = true;
                }
                "a" | "an" | "is" | "are" | "becomes" | "become" => {}
                _ => return Err("oracle:unsupported-layer-color"),
            }
        }
        if colors.is_empty() && !saw_colorless {
            return Err("oracle:unsupported-layer-color");
        }
        parts.push(ResolvedPartSpec::Color(if additive_colors {
            ColorOperation::Add(colors)
        } else {
            ColorOperation::Set(colors)
        }));
    }
    if lose_all_abilities || !grant_keywords.is_empty() || !lose_keywords.is_empty() {
        let mut change = if !grant_keywords.is_empty() {
            lower_ability_change(grant_keywords, vocabulary)?
        } else {
            AbilityChange::default()
        };
        if lose_all_abilities {
            change.removal = Some(AbilityRemoval::All);
        }
        if !lose_keywords.is_empty() {
            change
                .suppressed_keywords
                .extend(lower_keyword_list(lose_keywords)?);
        }
        parts.push(ResolvedPartSpec::Abilities(change));
    }
    if let Some((p_raw, t_raw)) = base_pt {
        let p_t = p_raw.trim();
        let t_t = t_raw.trim();
        if p_t.eq_ignore_ascii_case("keep") || t_t.eq_ignore_ascii_case("keep") {
            return Err("oracle:unsupported-layer-pt");
        }
        match (p_t.parse::<i64>(), t_t.parse::<i64>()) {
            (Ok(p), Ok(t)) => {
                parts.push(ResolvedPartSpec::SetPT(PowerToughness {
                    power: p.into(),
                    toughness: t.into(),
                }));
            }
            _ => {
                let parse_side = |s: &str| -> Result<ValueExpr, &'static str> {
                    if let Ok(n) = s.parse::<i64>() {
                        return Ok(ValueExpr::literal(n));
                    }
                    if s.eq_ignore_ascii_case("x") {
                        return if let Some(bx) = bound_x {
                            lower_value_expr(bx, vocabulary)
                        } else {
                            Ok(ValueExpr::chosen_x())
                        };
                    }
                    let ast_e = spellwake_oracle_compiler::filter::parse_value_expr(
                        s,
                        &cached_compiler_vocab(vocabulary),
                    )
                    .map_err(|_| "oracle:unsupported-layer-pt")?;
                    lower_value_expr(resolve_x(&ast_e, bound_x), vocabulary)
                };
                parts.push(ResolvedPartSpec::SetPTExpr {
                    power: Box::new(parse_side(p_t)?),
                    toughness: Box::new(parse_side(t_t)?),
                });
            }
        }
    }
    if parts.is_empty() {
        return Err("oracle:unsupported-layer-effect");
    }
    Ok(parts)
}

fn is_temp_menace_blocking_rule(
    rule: &str,
    rule_low: &str,
    filter: Option<&ObjectFilterAst>,
) -> bool {
    if matches!(
        rule_low,
        "minblockers:two or more creatures"
            | "minblockers:2 or more creatures"
            | "cant_be_blocked_except_by:two or more creatures"
            | "cantbeblockedexceptby:two or more creatures"
    ) {
        return true;
    }
    if rule == "CantBeBlockedExceptBy"
        && let Some(flt) = filter
    {
        return flt.predicates.iter().any(|p| {
            matches!(
                p,
                spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(s)
                    if {
                        let sl = s.trim().to_ascii_lowercase();
                        sl == "two or more creatures"
                            || sl == "2 or more creatures"
                            || sl == "two or more"
                    }
            )
        });
    }
    false
}

fn build_emblem_definition(
    text: &str,
    compiled: Option<&FaceAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<EmblemDefinition, &'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("oracle:empty-emblem");
    }
    let compiled_owned;
    let emblem_face: &FaceAst = if let Some(pre) = compiled {
        pre
    } else {
        compiled_owned = spellwake_oracle_compiler::compile_face(
            FaceInput {
                name: "Emblem",
                type_line: "Enchantment",
                oracle_text: trimmed,
            },
            &cached_compiler_vocab(vocabulary),
        );
        &compiled_owned
    };
    if !emblem_face.failures.is_empty()
        || !emblem_face.keywords.is_empty()
        || !emblem_face.replacements.is_empty()
        || !emblem_face.spell_effects.is_empty()
        || !emblem_face.level_bands.is_empty()
        || !emblem_face.class_levels.is_empty()
    {
        return Err("oracle:unsupported-emblem-text");
    }
    let mut scratch = Program::enchantment(None);
    let mut emblem_ids = LoweringIds::default();
    for st in &emblem_face.statics {
        lower_static(st, &mut scratch, vocabulary, &mut emblem_ids, false)?;
    }
    for ab in &emblem_face.abilities {
        if !matches!(ab, AbilityAst::Triggered { .. }) {
            return Err("oracle:unsupported-emblem-ability");
        }
        lower_ability(ab, &mut scratch, vocabulary, &mut emblem_ids)?;
    }
    if !scratch.activated.is_empty()
        || !scratch.mana.is_empty()
        || !scratch.zone_replacements.is_empty()
        || scratch.attack_requirement.is_some()
        || scratch.cannot_be_countered
        || scratch.opening_hand_battlefield
    {
        return Err("oracle:unsupported-emblem-text");
    }
    let continuous: Vec<StaticContinuousAbility> = scratch
        .continuous
        .into_iter()
        .chain(scratch.additional_battlefield_abilities)
        .collect();
    let triggered = scratch.triggered;
    if continuous.is_empty() && triggered.is_empty() {
        return Err("oracle:empty-emblem");
    }
    Ok(EmblemDefinition::Authored {
        label: trimmed.to_string(),
        continuous,
        triggered,
    })
}

fn lower_subject_to_permanent_selector(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    let f = match subject {
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            f
        }
        _ => return Err("oracle:unsupported-enters-tapped-rule"),
    };
    if !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.named.is_some()
        || !f.predicates.is_empty()
        || f.any_card
        || f.any_spell
        || f.is_ability
    {
        return Err("oracle:unsupported-enters-tapped-rule");
    }
    let allowed_flags = AstFilterFlags::EXCLUDE_SELF;
    if (f.flags.0 & !allowed_flags.0) != 0 {
        return Err("oracle:unsupported-enters-tapped-rule");
    }
    let mut sel = PermanentSelector {
        controller: match f.controller {
            None => ControllerSelector::Any,
            Some(ControllerRelation::You) => ControllerSelector::SourceController,
            Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                ControllerSelector::Opponents
            }
            _ => return Err("oracle:unsupported-enters-tapped-rule"),
        },
        token: f.token,
        exclude_source: (f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0,
        ..PermanentSelector::default()
    };
    if !f.required_supertypes.is_empty() {
        sel.required_supertypes = supertype_mask_to_set(f.required_supertypes)?;
    }
    if !f.forbidden_supertypes.is_empty() {
        sel.forbidden_supertypes = supertype_mask_to_set(f.forbidden_supertypes)?;
    }
    if !f.any_types.is_empty() {
        let types = card_type_mask_to_set(f.any_types)?;
        if types.len() == 1 {
            sel.required_types = types;
        } else {
            sel.any_types = types;
        }
    }
    if !f.forbidden_types.is_empty() {
        sel.forbidden_types = card_type_mask_to_set(f.forbidden_types)?;
    }
    if f.subtypes.len() == 1 {
        let st = &f.subtypes[0];
        if let Some(ct) = resolve_protection_subtype(st, vocabulary) {
            sel.creature_subtype = Some(ct);
        } else {
            return Err("oracle:unsupported-enters-tapped-rule");
        }
    } else if f.subtypes.len() > 1 {
        for st in &f.subtypes {
            let ct = resolve_protection_subtype(st, vocabulary)
                .ok_or("oracle:unsupported-enters-tapped-rule")?;
            sel.any_creature_subtypes.insert(ct);
        }
    }
    if f.forbidden_subtypes.len() == 1 {
        let ct = resolve_protection_subtype(&f.forbidden_subtypes[0], vocabulary)
            .ok_or("oracle:unsupported-enters-tapped-rule")?;
        sel.forbidden_creature_subtype = Some(ct);
    } else if f.forbidden_subtypes.len() > 1 {
        return Err("oracle:unsupported-enters-tapped-rule");
    }
    Ok(sel)
}

fn collect_combinations(
    n: usize,
    size: usize,
    start: usize,
    cur: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
    cap: usize,
) -> bool {
    if out.len() > cap {
        return false;
    }
    if cur.len() == size {
        out.push(cur.clone());
        return out.len() <= cap;
    }
    let remaining_needed = size - cur.len();
    for i in start..=n.saturating_sub(remaining_needed) {
        cur.push(i);
        if !collect_combinations(n, size, i + 1, cur, out, cap) {
            return false;
        }
        cur.pop();
    }
    true
}

fn str_mentions_y_var(s: &str) -> bool {
    let t = s.trim();
    t.eq_ignore_ascii_case("y") || t.eq_ignore_ascii_case("+y") || t.eq_ignore_ascii_case("-y")
}

fn body_mentions_y_var(body: &[InstructionAst]) -> bool {
    body.iter().any(|ins| match ins {
        InstructionAst::ModifyPT { dx, dy, .. } => str_mentions_y_var(dx) || str_mentions_y_var(dy),
        InstructionAst::AnimateOrSetBasePT {
            power, toughness, ..
        } => str_mentions_y_var(power) || str_mentions_y_var(toughness),
        InstructionAst::TemporaryStatic { static_ability, .. } => match static_ability.as_ref() {
            StaticAbilityAst::ContinuousModifyPT { dx, dy, .. } => {
                str_mentions_y_var(dx) || str_mentions_y_var(dy)
            }
            StaticAbilityAst::ContinuousLayerEffect {
                base_pt: Some((p, t)),
                ..
            } => str_mentions_y_var(p) || str_mentions_y_var(t),
            _ => false,
        },
        InstructionAst::Optional(inner)
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::ForEachPlayer { body: inner, .. }
        | InstructionAst::ForEachSubjectPlayer { body: inner, .. }
        | InstructionAst::ForAsLongAs { body: inner, .. }
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Otherwise(inner) => body_mentions_y_var(inner),
        _ => false,
    })
}
