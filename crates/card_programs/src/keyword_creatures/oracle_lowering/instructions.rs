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
            _ => Ok(ResolvingPlayer::UniqueRelatedPlayer),
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
            _ => Ok(ResolvingPlayer::UniqueRelatedPlayer),
        },
        _ => Err("oracle:unsupported-resolving-player"),
    }
}

pub(super) fn lower_affected_players(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
) -> Result<AffectedPlayers, &'static str> {
    match p {
        PlayerRefAst::EachPlayer => Ok(AffectedPlayers::EachPlayer),
        PlayerRefAst::EachOpponent => Ok(AffectedPlayers::EachOpponent),
        PlayerRefAst::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        other => Ok(AffectedPlayers::One(lower_resolving_player(
            other, builder,
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
        DurationAst::ForAsLongAs(_) => Err("oracle:unsupported-duration"),
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
                },
            ] = inner.as_slice()
            {
                let search = lower_library_search(
                    library,
                    count,
                    filter,
                    *reveal,
                    to_zone,
                    vocabulary,
                    true,
                    *up_to,
                    *any_number,
                )?;
                if search.source == SearchSource::TargetOpponentLibrary {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                } else if search.searcher == ResolvingPlayer::TargetPlayer {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                }
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
                builder.instructions.push(Instruction::SearchLibrary {
                    filter: Box::new(search),
                });
                return Ok(());
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
                },
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You,
                },
            ] = inner.as_slice()
            {
                let search = lower_library_search(
                    library,
                    count,
                    filter,
                    *reveal,
                    to_zone,
                    vocabulary,
                    true,
                    *up_to,
                    *any_number,
                )?;
                if search.source == SearchSource::TargetOpponentLibrary {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                } else if search.searcher == ResolvingPlayer::TargetPlayer {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                }
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
                builder.instructions.push(Instruction::SearchLibrary {
                    filter: Box::new(search),
                });
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
            let chooser = lower_resolving_player(*player, builder)?;
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
        InstructionAst::Draw { player, count } => {
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
                    } else if let Ok(rp) = lower_resolving_player(*player, builder)
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
                    let rp = lower_resolving_player(*player, builder)?;
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
                    SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
                        SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
            let amt_expr = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
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
                    builder
                        .instructions
                        .push(Instruction::DamageRelatedPlayerAmount(amt_expr));
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
                    } else {
                        let mut player_group: Option<AffectedPlayers> = None;
                        for item in items {
                            if let SubjectRefAst::Player(p) = item {
                                let ap = lower_affected_players(*p, builder)?;
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
                            | PlayerRefAst::ThatPlayer
                    ) =>
                {
                    let players = lower_affected_players(*p, builder)?;
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
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None && in_trigger && !self_pronoun_valid =>
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
                    let players = lower_affected_players(*p, builder)?;
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
        InstructionAst::Destroy { subject } => match subject.as_ref() {
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
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None && in_trigger && !self_pronoun_valid =>
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
        } => match subject.as_ref() {
            SubjectRefAst::Anaphora(_)
                if builder.target == BodyTarget::None
                    && builder.instructions.iter().rev().any(|op| {
                        matches!(
                            op,
                            Instruction::LookTopChoose {
                                reveal_only: true,
                                ..
                            } | Instruction::MayRevealLibraryTop
                        )
                    }) =>
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
                count: TargetCountAst::Exact(n),
            } if filter.zone.is_some() && *n >= 2 => {
                let bt = lower_target_subject(
                    &SubjectRefAst::TargetObject {
                        filter: filter.clone(),
                        count: TargetCountAst::Exact(1),
                    },
                    vocabulary,
                )?;
                if !matches!(
                    bt,
                    BodyTarget::AnyGraveyardCard
                        | BodyTarget::AnyGraveyardCreature
                        | BodyTarget::FilteredOwnGraveyardCard(_)
                        | BodyTarget::FilteredAnyGraveyardCard { .. }
                ) {
                    return Err("oracle:unsupported-exile-gy-target");
                }
                builder.bind_target(bt)?;
                builder.graveyard_target_bounds = Some((usize::from(*n), usize::from(*n)));
                builder.instructions.push(Instruction::ExileGraveyardTarget);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(max),
            } if filter.zone.is_some() && *max > 0 => {
                let bt = lower_target_subject(
                    &SubjectRefAst::TargetObject {
                        filter: filter.clone(),
                        count: TargetCountAst::Exact(1),
                    },
                    vocabulary,
                )?;
                if !matches!(
                    bt,
                    BodyTarget::AnyGraveyardCard
                        | BodyTarget::AnyGraveyardCreature
                        | BodyTarget::FilteredOwnGraveyardCard(_)
                        | BodyTarget::FilteredAnyGraveyardCard { .. }
                ) {
                    return Err("oracle:unsupported-exile-gy-target");
                }
                builder.bind_target(bt)?;
                builder.graveyard_target_bounds = Some((0, usize::from(*max)));
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
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder.instructions.push(Instruction::MoveSubject {
                    subject: MoveSubjectRef::TriggeringObject,
                    destination: SubjectDestination::Exile {
                        delayed_return: None,
                    },
                });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                let obj = lower_object_filter(f, vocabulary)?;
                bind_filter_player_target(builder, &obj)?;
                builder.instructions.push(Instruction::MoveGroup {
                    group: Box::new(obj),
                    destination: PublicZoneDestination::Exile,
                });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_some() => {
                let (players, zone) = match f.zone {
                    Some(ZoneFilter::YourHand) => (
                        AffectedPlayers::One(ResolvingPlayer::Controller),
                        SourceZoneFilter::Hand,
                    ),
                    Some(ZoneFilter::ThatPlayerHand) => (
                        AffectedPlayers::One(lower_resolving_player(
                            PlayerRefAst::ThatPlayer,
                            builder,
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
                        AffectedPlayers::One(lower_resolving_player(
                            PlayerRefAst::ThatPlayer,
                            builder,
                        )?),
                        SourceZoneFilter::Graveyard,
                    ),
                    Some(ZoneFilter::TargetPlayerGraveyard) => (
                        AffectedPlayers::One(lower_resolving_player(
                            PlayerRefAst::TargetPlayer,
                            builder,
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
                    _ => return Err("oracle:unsupported-exile"),
                };
                let mut inner = f.as_ref().clone();
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
                if f.zone.is_none() && f.controller == Some(ControllerRelation::You) =>
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
            SubjectRefAst::Compound(items) if !items.is_empty() => {
                for it in items {
                    lower_instruction(
                        &InstructionAst::Exile {
                            actor: None,
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
            _ => Err("oracle:unsupported-exile"),
        },
        InstructionAst::Exile {
            actor: Some(actor_player),
            subject,
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
            let players = lower_affected_players(*actor_player, builder)?;
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
        } if *transformed || with_counter.is_some() || attached_to.is_some() => {
            let dest_low = to.trim().to_ascii_lowercase();
            let to_bf = is_battlefield_controller_dest(&dest_low)
                || is_battlefield_owner_control_dest(&dest_low)
                || dest_low.contains("battlefield");
            if !to_bf {
                return Err("oracle:unsupported-return-dest");
            }
            let under_owner_control = is_battlefield_owner_control_dest(&dest_low)
                || dest_low.contains("owner's control");
            let enters_tapped = *tapped || dest_low.contains("tapped");
            let attacking = dest_low.contains("attacking");
            let counter = match with_counter.as_deref() {
                None => None,
                Some(raw) => {
                    let trimmed = raw.trim();
                    let low_raw = trimmed.to_ascii_lowercase();
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
                    let ck =
                        parse_counter_kind(kind_str).ok_or("oracle:unsupported-counter-kind")?;
                    Some((ck, qty))
                }
            };
            let attach_to_self = attached_to.as_deref().is_some_and(|a| {
                is_self_pronoun(a, builder, self_pronoun_valid)
                    || matches!(a, SubjectRefAst::Anaphora(_))
            });
            if attached_to.is_some() && !attach_to_self {
                return Err("oracle:unsupported-return-attach");
            }
            let destination = SubjectDestination::Battlefield {
                tapped: enters_tapped,
                attacking,
                transformed: *transformed,
                under_owner_control,
                with_counter: counter,
                attach_to_self,
            };
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::AbilitySource,
                        destination,
                    });
                    Ok(())
                }
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
                        } else if fl.contains("exile") {
                            patched.zone = Some(ZoneFilter::Exile);
                        }
                    }
                    let bt = lower_target_subject(
                        &SubjectRefAst::TargetObject {
                            filter: Box::new(patched),
                            count: TargetCountAst::Exact(1),
                        },
                        vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::TargetObject,
                        destination,
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) => {
                    let subj_ref = if builder.target != BodyTarget::None {
                        MoveSubjectRef::TargetObject
                    } else if in_trigger {
                        MoveSubjectRef::TriggeringObject
                    } else if self_pronoun_valid {
                        MoveSubjectRef::AbilitySource
                    } else {
                        return Err("oracle:unsupported-return-bf");
                    };
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: subj_ref,
                        destination,
                    });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::TriggeringObject,
                        destination,
                    });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                    builder.instructions.push(Instruction::MoveSubject {
                        subject: MoveSubjectRef::AttachedHost,
                        destination,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-return-bf"),
            }
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed: false,
            with_counter: None,
            attached_to: None,
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
            );
            let to_bf = is_battlefield_controller_dest(&dest);
            let to_bf_owner = is_battlefield_owner_control_dest(&dest);
            let enters_tapped = *tapped || dest.contains("tapped");
            let allow_your_lib = matches!(
                subject.as_ref(),
                SubjectRefAst::TargetObject { filter, .. }
                    if matches!(filter.zone, Some(ZoneFilter::YourGraveyard | ZoneFilter::YourHand))
            ) || from_zone
                .as_deref()
                .is_some_and(|z| z.to_ascii_lowercase().contains("your graveyard"));
            if !enters_tapped && let Some(pl) = parse_lib_placement(&dest, allow_your_lib) {
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
                        let (sel_player, src_zone) =
                            resolve_select_zone(f.zone, SourceZoneFilter::Battlefield, builder)?;
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
                        if has_checked_revealed_top(&builder.instructions) =>
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
            if to_hand {
                match subject.as_ref() {
                    s if !enters_tapped && is_self_pronoun(s, builder, self_pronoun_valid) => {
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
                    } if !enters_tapped => {
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
                    } if !enters_tapped && *n >= 2 => {
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
                    } if !enters_tapped && *max > 0 => {
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
                    } if !enters_tapped => {
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
                        if !enters_tapped && builder.target != BodyTarget::None =>
                    {
                        builder.instructions.push(Instruction::ReturnToOwnersHand);
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_) if !enters_tapped && in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::TriggeringObject,
                            destination: SubjectDestination::Hand,
                        });
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if !enters_tapped
                            && (f.zone == Some(ZoneFilter::YourGraveyard)
                                || from_zone.as_deref().is_some_and(|z| {
                                    z.to_ascii_lowercase().contains("your graveyard")
                                })) =>
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
                    SubjectRefAst::Group(f)
                        if !enters_tapped && f.zone.is_none() && from_zone.is_none() =>
                    {
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
                            && !enters_tapped
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
                            && !enters_tapped
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
                    SubjectRefAst::AttachedHost(_) if !enters_tapped => {
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )),
                            destination: PublicZoneDestination::Hand,
                        });
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone.is_none() && from_zone.is_none() && !enters_tapped =>
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
                        if !enters_tapped
                            && classify_graveyard_group(f.zone, from_zone.as_deref()).is_some() =>
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
                    SubjectRefAst::Compound(items) if !enters_tapped && !items.is_empty() => {
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
            } else if to_bf || to_bf_owner {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
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
                                if !enters_tapped
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
                                } else {
                                    builder.instructions.push(
                                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                                            tapped: enters_tapped,
                                        },
                                    );
                                }
                                Ok(())
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::OpponentGraveyard)
                                if to_bf || to_bf_owner =>
                            {
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
                                if to_bf_owner && !to_bf {
                                    builder.instructions.push(Instruction::MoveSubject {
                                        subject: MoveSubjectRef::TargetObject,
                                        destination: SubjectDestination::Battlefield {
                                            tapped: enters_tapped,
                                            attacking: dest.contains("attacking"),
                                            transformed: false,
                                            under_owner_control: true,
                                            with_counter: None,
                                            attach_to_self: false,
                                        },
                                    });
                                } else if !enters_tapped
                                    && bt == BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                                {
                                    builder.instructions.push(
                                        Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker,
                                    );
                                } else {
                                    builder.instructions.push(
                                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                                            tapped: enters_tapped,
                                        },
                                    );
                                }
                                Ok(())
                            }
                            _ => Err("oracle:unsupported-reanimate-zone"),
                        }
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::ChosenX,
                    } if to_bf
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
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if classify_graveyard_group(f.zone, from_zone.as_deref())
                            .is_some_and(|all_gy| to_bf || !all_gy) =>
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
                            destination: SubjectDestination::Battlefield {
                                tapped: enters_tapped,
                                attacking: dest.contains("attacking"),
                                transformed: false,
                                under_owner_control: to_bf_owner,
                                with_counter: None,
                                attach_to_self: false,
                            },
                        });
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_) => {
                        let subj_ref = if builder.target != BodyTarget::None {
                            MoveSubjectRef::TargetObject
                        } else if in_trigger {
                            MoveSubjectRef::TriggeringObject
                        } else if self_pronoun_valid {
                            MoveSubjectRef::AbilitySource
                        } else {
                            return Err("oracle:unsupported-return-bf");
                        };
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: subj_ref,
                            destination: SubjectDestination::Battlefield {
                                tapped: enters_tapped,
                                attacking: dest.contains("attacking"),
                                transformed: false,
                                under_owner_control: to_bf_owner,
                                with_counter: None,
                                attach_to_self: false,
                            },
                        });
                        Ok(())
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: SubjectDestination::Battlefield {
                                tapped: enters_tapped,
                                attacking: dest.contains("attacking"),
                                transformed: false,
                                under_owner_control: to_bf_owner,
                                with_counter: None,
                                attach_to_self: false,
                            },
                        });
                        Ok(())
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        builder.instructions.push(Instruction::MoveSubject {
                            subject: MoveSubjectRef::AttachedHost,
                            destination: SubjectDestination::Battlefield {
                                tapped: enters_tapped,
                                attacking: dest.contains("attacking"),
                                transformed: false,
                                under_owner_control: to_bf_owner,
                                with_counter: None,
                                attach_to_self: false,
                            },
                        });
                        Ok(())
                    }
                    SubjectRefAst::Group(f) => {
                        let src_zone = match f.zone {
                            Some(ZoneFilter::YourHand) => SourceZoneFilter::Hand,
                            Some(ZoneFilter::YourGraveyard) => SourceZoneFilter::Graveyard,
                            Some(ZoneFilter::OpponentGraveyard) => {
                                SourceZoneFilter::OpponentGraveyard
                            }
                            Some(ZoneFilter::AnyGraveyard) => SourceZoneFilter::AnyGraveyard,
                            Some(ZoneFilter::Exile) => SourceZoneFilter::Exile,
                            None if from_zone
                                .as_deref()
                                .is_some_and(|z| z.to_ascii_lowercase().contains("graveyard")) =>
                            {
                                SourceZoneFilter::Graveyard
                            }
                            None if from_zone
                                .as_deref()
                                .is_some_and(|z| z.to_ascii_lowercase().contains("exile")) =>
                            {
                                SourceZoneFilter::Exile
                            }
                            _ => return Err("oracle:unsupported-return-bf"),
                        };
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
                                source_zone: src_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                destination: SubjectDestination::Battlefield {
                                    tapped: enters_tapped,
                                    attacking: dest.contains("attacking"),
                                    transformed: false,
                                    under_owner_control: to_bf_owner,
                                    with_counter: None,
                                    attach_to_self: false,
                                },
                                reveal: false,
                            });
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
            let to_bf = is_battlefield_controller_dest(&dest)
                || (dest.contains("battlefield") && !is_battlefield_owner_control_dest(&dest));
            let to_bf_owner = is_battlefield_owner_control_dest(&dest);
            if to_bf || to_bf_owner {
                let enters_tapped = dest.contains("tapped");
                let attacking = dest.contains("attacking");
                let bf_dest = SubjectDestination::Battlefield {
                    tapped: enters_tapped,
                    attacking,
                    transformed: false,
                    under_owner_control: to_bf_owner,
                    with_counter: None,
                    attach_to_self: false,
                };
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if !attacking
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
                        let subj_ref = if builder.target != BodyTarget::None {
                            MoveSubjectRef::TargetObject
                        } else if in_trigger {
                            MoveSubjectRef::TriggeringObject
                        } else if self_pronoun_valid {
                            MoveSubjectRef::AbilitySource
                        } else {
                            MoveSubjectRef::RevealedCard
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
                    SubjectRefAst::Group(f)
                        if !attacking && matches!(f.zone, Some(ZoneFilter::YourHand)) =>
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
                                builder
                                    .instructions
                                    .push(Instruction::PutHandPermanentFiltered {
                                        player: ResolvingPlayer::Controller,
                                        filter: Box::new(obj),
                                        maximum: 1,
                                        tapped: enters_tapped,
                                    });
                                return Ok(());
                            }
                        }
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if classify_graveyard_group(f.zone, None)
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
                        let (sel_player, src_zone) =
                            resolve_select_zone(f.zone, SourceZoneFilter::Hand, builder)?;
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
                            && matches!(filter.controller, Some(ControllerRelation::You)))
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
            let single_card_subject = matches!(
                subject.as_ref(),
                SubjectRefAst::SelfObject
                    | SubjectRefAst::TriggerSubject
                    | SubjectRefAst::AttachedHost(_)
                    | SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    }
            ) || matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "that card" | "that creature" | "that permanent" | "the card"
                )
            );
            let lib_placement = parse_lib_placement(&dest, allow_your_lib).or_else(|| {
                if single_card_subject && dest.contains("random") {
                    let stripped = dest
                        .replace(" in a random order", "")
                        .replace(" in random order", "");
                    parse_lib_placement(stripped.trim(), allow_your_lib)
                } else {
                    None
                }
            });
            if let Some(pl) = lib_placement {
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
                                action: PermanentTargetAction::MoveTo(SubjectDestination::Library(
                                    pl,
                                )),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Group(f) => {
                        let (sel_player, src_zone) =
                            resolve_select_zone(f.zone, SourceZoneFilter::Battlefield, builder)?;
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
                        if has_checked_revealed_top(&builder.instructions) =>
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
                    | "to its owner's hand"
                    | "to their owner's hand"
                    | "to their owners' hands"
                    | "into your hand"
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
                    | "into your graveyard"
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
                        count: TargetCountAst::ChosenX,
                    } if filter.zone.is_none()
                        && matches!(
                            pd,
                            PublicZoneDestination::Hand
                                | PublicZoneDestination::Exile
                                | PublicZoneDestination::Graveyard
                        ) =>
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.x_target_count = true;
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
                        if matches!(
                            builder.target,
                            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                        ) && let Some(sd) = subj_dest_from_pd(pd) =>
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
                        let (sel_player, src_zone) =
                            resolve_select_zone(f.zone, SourceZoneFilter::Battlefield, builder)?;
                        let mut inner = f.as_ref().clone();
                        inner.zone = None;
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
                        if has_checked_revealed_top(&builder.instructions)
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
                } else if in_trigger && !self_pronoun_valid {
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
                let players = lower_affected_players(*player, builder)?;
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
            let players = lower_affected_players(*player, builder)?;
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
            player: PlayerRefAst::You,
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
            let players = lower_affected_players(*player, builder)?;
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
                let rp = lower_resolving_player(*player, builder)?;
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
            let players = lower_affected_players(*player, builder)?;
            builder.instructions.push(Instruction::Discard {
                players,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::DiscardHand { player } => {
            let rp = lower_resolving_player(*player, builder)?;
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
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::Mill {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Scry { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::Scry {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Surveil { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let rp = lower_resolving_player(*player, builder)?;
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
        } => {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                to_zone,
                vocabulary,
                false,
                *up_to,
                *any_number,
            )?;
            if search.source == SearchSource::TargetOpponentLibrary {
                builder.bind_target(BodyTarget::SingleOpponent)?;
            } else if search.searcher == ResolvingPlayer::TargetPlayer {
                builder.bind_target(BodyTarget::SinglePlayer)?;
            }
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
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
            let rp = lower_resolving_player(*player, builder)?;
            builder
                .instructions
                .push(Instruction::ShufflePlayerLibrary(rp));
            Ok(())
        }
        InstructionAst::RevealHand { player } => {
            let rp = lower_resolving_player(*player, builder)?;
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
        InstructionAst::TapPermanent { subject } => match subject.as_ref() {
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
            _ => Err("oracle:unsupported-tap-perm"),
        },
        InstructionAst::UntapPermanent { subject } => match subject.as_ref() {
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
                builder.last_group_filter = Some(ObjectFilter::creature().controlled());
                builder
                    .instructions
                    .push(Instruction::UntapControlledCreatures);
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
            let is_chosen_color_prot = |k: &str| {
                let low = k.trim().to_ascii_lowercase();
                low == "protection from the color of your choice"
                    || low == "protection from the chosen color"
                    || low.starts_with("protection from the color of your choice.")
                    || low.starts_with("protection from the chosen color.")
            };
            let has_chosen_color_prot = dur == ResolvedDuration::UntilEndOfTurn
                && keywords.iter().any(|k| is_chosen_color_prot(k));
            let remaining_keywords: Vec<String> = if has_chosen_color_prot {
                keywords
                    .iter()
                    .filter(|k| !is_chosen_color_prot(k))
                    .cloned()
                    .collect()
            } else {
                keywords.clone()
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
            let ab_change = lower_ability_change(&remaining_keywords, vocabulary)?;
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
                s if is_self_pronoun(s, builder, self_pronoun_valid)
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
                SubjectRefAst::Anaphora(_)
                    if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
                && (lhs_low.ends_with(" counter") || lhs_low.ends_with(" counters"))
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
                    let players = lower_affected_players(*p, builder)?;
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
                SubjectRefAst::Anaphora(_)
                    if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
                    let sacrifice_at_end_step =
                        exc_low.contains("sacrifice") && exc_low.contains("end step");
                    let exile_at_end_step =
                        exc_low.contains("exile") && exc_low.contains("end step");
                    let strip_mana_cost = exc_low.contains("has no mana cost")
                        || exc_low.contains("have no mana cost")
                        || exc_low.contains("no mana cost");
                    let mut remainder = exc_low.clone();
                    for pat in [
                        "except it isn't legendary",
                        "except they aren't legendary",
                        "it isn't legendary",
                        "they aren't legendary",
                        "not legendary",
                        "except it has haste",
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
                    SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
                | PlayerRefAst::EnchantedPlayer => {
                    if *controller == PlayerRefAst::DefendingPlayer {
                        TokenRecipients::Player(ResolvingPlayer::DefendingPlayer)
                    } else if *controller == PlayerRefAst::EnchantedPlayer {
                        TokenRecipients::Player(ResolvingPlayer::EnchantedPlayer)
                    } else if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) {
                        TokenRecipients::Player(ResolvingPlayer::TargetPlayer)
                    } else if matches!(
                        builder.target,
                        BodyTarget::SingleCreature
                            | BodyTarget::SinglePermanent
                            | BodyTarget::SinglePlaneswalker
                            | BodyTarget::SingleAnyTarget
                            | BodyTarget::SingleCreatureOrPlayer
                            | BodyTarget::SinglePlayerOrPlaneswalker
                            | BodyTarget::SingleOpponentOrPlaneswalker
                            | BodyTarget::FilteredPermanent(_)
                    ) {
                        TokenRecipients::Player(ResolvingPlayer::TargetController)
                    } else if matches!(builder.target, BodyTarget::SingleSpell(_)) {
                        TokenRecipients::Player(ResolvingPlayer::TargetSpellController)
                    } else {
                        TokenRecipients::Player(ResolvingPlayer::UniqueRelatedPlayer)
                    }
                }
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
                            } else {
                                TokenRecipients::Player(ResolvingPlayer::UniqueRelatedPlayer)
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
                    BodyTarget::None => {
                        TokenRecipients::Player(ResolvingPlayer::UniqueRelatedPlayer)
                    }
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
                if !creation.enters_tapped
                    || creation.recipients != TokenRecipients::Controller
                    || builder.target != BodyTarget::None
                {
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
            let rp = lower_resolving_player(*controller, builder)?;
            let eff_subj = match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    EffectSubject::AbilitySource
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => EffectSubject::Matching(
                    ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                ),
                SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                SubjectRefAst::Anaphora(_)
                    if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
            let sub = vocabulary
                .resolve_label(st_label)
                .ok_or("oracle:amass-subtype")?;
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
        InstructionAst::Manifest { count, dread } => {
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
            if low.contains("doesn't untap during")
                || low.contains("don't untap during")
                || low == "doesnt_untap"
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
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 1,
                                maximum: 1,
                                action: PermanentTargetAction::SkipNextUntap,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && !self_pronoun_valid =>
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
            if let Some(br) = lower_combat_mod_blocking_restriction(rule, vocabulary)
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
                            duration: dur,
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
                                duration: dur,
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
                                    duration: dur,
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
                                    duration: dur,
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
                                    duration: dur,
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
                            duration: dur,
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
                            duration: dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
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
                                duration: dur,
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
                                duration: dur,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && !self_pronoun_valid =>
                    {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
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
                            duration: dur,
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
                Some(p) => lower_resolving_player(*p, builder)?,
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
        InstructionAst::ChangeTargets { target } => {
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
                let first = extract_resolving_player_from_subject(a, builder)?;
                let second = extract_resolving_player_from_subject(b, builder)?;
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
                let player = extract_resolving_player_from_subject(player_subj, builder)?;
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
        InstructionAst::RedirectDamage { amount, from, to } => {
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
            let players = lower_affected_players(*player, builder)?;
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
            let rp = lower_resolving_player(*player, builder)?;
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
            let rp = lower_resolving_player(*player, builder)?;
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
                let players = lower_affected_players(*chooser, builder)?;
                builder
                    .instructions
                    .push(Instruction::ChooseOptionForPlayers { players, options });
            } else {
                let rp = lower_resolving_player(*chooser, builder)?;
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
            if let Some(rest) = d_low
                .strip_prefix("a number between ")
                .or_else(|| d_low.strip_prefix("number between "))
                && let Some((a_s, b_s)) = rest.split_once(" and ")
                && let (Some(a), Some(b)) = (
                    parse_small_number_word(a_s.trim()),
                    parse_small_number_word(b_s.trim()),
                )
                && a <= b
            {
                let rp = lower_resolving_player(*chooser, builder)?;
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: rp,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Number {
                        min: a,
                        max: b,
                    },
                });
                return Ok(());
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
            use spellwake_engine::program::ResolutionChoiceDomain;
            let trimmed = raw.trim();
            let (chooser, choice_text) = if let Some((prefix, rest)) = trimmed.split_once(':') {
                let p_low = prefix.trim().to_ascii_lowercase();
                let rp = match p_low.as_str() {
                    "you" => ResolvingPlayer::Controller,
                    "targetplayer" | "target player" => {
                        builder.bind_target(BodyTarget::SinglePlayer)?;
                        ResolvingPlayer::TargetPlayer
                    }
                    "targetopponent" | "target opponent" => {
                        builder.bind_target(BodyTarget::SingleOpponent)?;
                        ResolvingPlayer::TargetPlayer
                    }
                    "thatplayer" | "that player" | "an opponent" => {
                        ResolvingPlayer::UniqueRelatedPlayer
                    }
                    "activeplayer" | "active player" => ResolvingPlayer::ActivePlayer,
                    "defendingplayer" | "defending player" => ResolvingPlayer::DefendingPlayer,
                    "enchantedplayer" | "enchanted player" => ResolvingPlayer::EnchantedPlayer,
                    "itscontroller" | "its controller" | "thatobjectcontroller" => {
                        ResolvingPlayer::TargetController
                    }
                    _ => return Err("oracle:unsupported-choose-option"),
                };
                (rp, rest.trim().to_ascii_lowercase())
            } else {
                (ResolvingPlayer::Controller, trimmed.to_ascii_lowercase())
            };
            let cleaned_choice = choice_text
                .strip_prefix("choose ")
                .unwrap_or(choice_text.as_str())
                .trim()
                .trim_end_matches('.');
            match cleaned_choice {
                "a color" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::Color,
                    });
                    Ok(())
                }
                "a card type" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::CardType(Vec::new()),
                    });
                    Ok(())
                }
                "a basic land type" | "a land type" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::BasicLandType,
                    });
                    Ok(())
                }
                "an opponent" | "an opponent at random" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::Player {
                            opponents_only: true,
                        },
                    });
                    Ok(())
                }
                "a player" | "another player" | "a player at random" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::Player {
                            opponents_only: false,
                        },
                    });
                    Ok(())
                }
                "a number" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::Number { min: 0, max: 20 },
                    });
                    Ok(())
                }
                "a card name" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::CardName {
                            nonland_only: false,
                        },
                    });
                    Ok(())
                }
                "a nonland card name" => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::CardName { nonland_only: true },
                    });
                    Ok(())
                }
                "a creature type" if chooser == ResolvingPlayer::Controller => {
                    let slot = ids.creature_type_slot();
                    builder.instructions.push(Instruction::ChooseCreatureType {
                        into: slot,
                        excluding: BTreeSet::new(),
                    });
                    Ok(())
                }
                other
                    if chooser == ResolvingPlayer::Controller
                        && other.starts_with("a creature type other than ") =>
                {
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
                    let slot = ids.creature_type_slot();
                    builder.instructions.push(Instruction::ChooseCreatureType {
                        into: slot,
                        excluding,
                    });
                    Ok(())
                }
                other if other.contains(" or ") => {
                    let mut labels = Vec::new();
                    for seg in other.split(", or ").flat_map(|s| s.split(" or ")) {
                        for sub in seg.split(',') {
                            let t = sub.trim().trim_start_matches("or ").trim();
                            if !t.is_empty() {
                                labels.push(t.to_string());
                            }
                        }
                    }
                    if labels.len() < 2 || labels.len() > 20 {
                        return Err("oracle:unsupported-choose-option");
                    }
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser,
                        domain: ResolutionChoiceDomain::LabeledOption(labels),
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-choose-option"),
            }
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
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::VentureIntoDungeon {
                player: rp,
                undercity: false,
            });
            Ok(())
        }
        InstructionAst::TakeInitiative { player } => {
            let rp = lower_resolving_player(*player, builder)?;
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
                "that player" | "that player's" | "defending player" | "defending player's"
                | "enchanted player" | "enchanted player's" | "its controller"
                | "its controller's" => (
                    None,
                    AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
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
                Some(ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard)
            ) =>
            {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::ShuffleTargetIntoLibrary);
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
            SubjectRefAst::TriggerSubject if in_trigger => {
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::TriggerSubject),
                        zone: None,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_)
                if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                let zone = match f.zone {
                    None => None,
                    Some(ZoneFilter::YourGraveyard) => Some(SourceZoneFilter::Graveyard),
                    Some(ZoneFilter::OpponentGraveyard) => {
                        Some(SourceZoneFilter::OpponentGraveyard)
                    }
                    Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                        Some(SourceZoneFilter::AnyGraveyard)
                    }
                    Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
                    Some(ZoneFilter::YourHand) => Some(SourceZoneFilter::Hand),
                    _ => return Err("oracle:unsupported-shuffle-into-library"),
                };
                let mut inner = f.as_ref().clone();
                inner.zone = None;
                let obj = lower_object_filter(&inner, vocabulary)?;
                builder
                    .instructions
                    .push(Instruction::ShuffleSubjectIntoLibrary {
                        subject: Box::new(EffectSubject::Matching(obj)),
                        zone,
                    });
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
            let low = kind.trim().to_ascii_lowercase();
            let counter_double_kind: Option<Option<CounterKind>> =
                if low == "counters" || low == "each kind of counter" || low == "each counter" {
                    Some(None)
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
                    SubjectRefAst::Anaphora(_)
                        if in_trigger
                            && !self_pronoun_valid
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
                SubjectRefAst::Anaphora(_)
                    if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
        InstructionAst::Attach { subject, target } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                if let SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } = target.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    if in_trigger
                        && bt
                            == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
                    {
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::Attach(AttachAbility::Equip));
                        return Ok(());
                    }
                    if !in_trigger && bt == BodyTarget::SingleCreature {
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
                SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
                SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
            if builder.target == BodyTarget::None
                && matches!(
                    player_ast,
                    PlayerRefAst::ThatPlayer
                        | PlayerRefAst::ItsController
                        | PlayerRefAst::ThatObjectController
                        | PlayerRefAst::ItsOwner
                )
            {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                let mut probe_ids = LoweringIds::default();
                if lower_instruction_seq(
                    body,
                    &mut probe,
                    vocabulary,
                    &mut probe_ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.target != BodyTarget::None
                {
                    builder.bind_target(probe.target)?;
                }
            }
            let payer = lower_resolving_player(player_ast, builder)?;
            let offer_idx = builder.instructions.len();
            let offer_op = try_build_offer_mana(payer, raw, 0, offer_idx + 1, bound_x, vocabulary)
                .ok_or("oracle:unless-pay-x")?;
            builder.instructions.push(offer_op);
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
                            Some(SubjectRefAst::Player(p)) => lower_affected_players(*p, builder)?,
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
                            Some(SubjectRefAst::Player(p)) => lower_affected_players(*p, builder)?,
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
                match subject.as_deref() {
                    None => {
                        if self_pronoun_valid {
                            builder.instructions.push(Instruction::ChangeAbilitySource {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                            return Ok(());
                        }
                    }
                    Some(s) if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(SubjectRefAst::TargetObject {
                        filter: target_f,
                        count: TargetCountAst::Exact(1),
                    }) if target_f.zone.is_none() => {
                        let bt = lower_battlefield_body_target(target_f, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) if f.zone.is_none() => {
                        let obj = lower_object_filter(f, vocabulary)?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(obj)),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(SubjectRefAst::TargetObject {
                        filter: target_f,
                        count: TargetCountAst::Exact(n),
                    }) if target_f.zone.is_none() && *n >= 2 => {
                        let bt = lower_battlefield_body_target(target_f, vocabulary)?;
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
                                    duration: dur,
                                },
                            });
                        return Ok(());
                    }
                    Some(SubjectRefAst::TargetObject {
                        filter: target_f,
                        count: TargetCountAst::UpTo(max),
                    }) if target_f.zone.is_none() && *max > 0 => {
                        let bt = lower_battlefield_body_target(target_f, vocabulary)?;
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
                                    duration: dur,
                                },
                            });
                        return Ok(());
                    }
                    Some(SubjectRefAst::TriggerSubject) if in_trigger => {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(SubjectRefAst::Anaphora(_)) if builder.target != BodyTarget::None => {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    Some(SubjectRefAst::TargetObject {
                        filter: target_f,
                        count: TargetCountAst::AnyNumber,
                    }) if target_f.zone.is_none() => {
                        let obj = lower_object_filter(target_f, vocabulary)?;
                        builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
                        builder
                            .instructions
                            .push(Instruction::ModifyAnyNumberOfPermanents {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    Some(SubjectRefAst::Anaphora(_))
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && !self_pronoun_valid =>
                    {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(SubjectRefAst::Anaphora(_))
                        if builder.target == BodyTarget::None
                            && builder.last_group_filter.is_some() =>
                    {
                        let obj = builder
                            .last_group_filter
                            .ok_or("oracle:unsupported-temp-static")?;
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(obj)),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    _ => {}
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
            Err("oracle:unsupported-temp-static")
        }
        InstructionAst::SacrificeUnlessPay { subject, cost } => {
            let sac_ins = InstructionAst::Sacrifice {
                player: PlayerRefAst::You,
                count: None,
                subject: subject.clone(),
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
            let player_ast = unless_player.unwrap_or(PlayerRefAst::You);
            if builder.target == BodyTarget::None
                && matches!(
                    player_ast,
                    PlayerRefAst::ThatPlayer
                        | PlayerRefAst::ItsController
                        | PlayerRefAst::ThatObjectController
                        | PlayerRefAst::ItsOwner
                )
            {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                let mut probe_ids = LoweringIds::default();
                if lower_instruction_seq(
                    body,
                    &mut probe,
                    vocabulary,
                    &mut probe_ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.target != BodyTarget::None
                {
                    builder.bind_target(probe.target)?;
                }
            }
            if let Some(CostAtomAst::PayMana(raw)) = unless_cost
                && let Some(mana) = parse_fixed_mana_cost(raw)
            {
                let payer = lower_resolving_player(player_ast, builder)?;
                let offer_idx = builder.instructions.len();
                builder.instructions.push(Instruction::OfferManaPayment {
                    payer,
                    cost: mana,
                    paid_destination: 0,
                    declined_destination: offer_idx + 1,
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
                if let Some(Instruction::OfferManaPayment {
                    paid_destination, ..
                }) = builder.instructions.get_mut(offer_idx)
                {
                    *paid_destination = end_dest;
                }
                return Ok(());
            }
            if let Some(cost) = unless_cost {
                let opt = if player_ast == PlayerRefAst::You {
                    InstructionAst::Optional(vec![InstructionAst::PayCost(cost.clone())])
                } else {
                    InstructionAst::OptionalPlayer {
                        player: player_ast,
                        body: vec![InstructionAst::PayCost(cost.clone())],
                    }
                };
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
            if let Some(act_body) = unless_action {
                let opt = if player_ast == PlayerRefAst::You {
                    InstructionAst::Optional(act_body.clone())
                } else {
                    InstructionAst::OptionalPlayer {
                        player: player_ast,
                        body: act_body.clone(),
                    }
                };
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
            ability_text: _,
            compiled: Some(compiled_face),
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let parts = lower_granted_ability_specs(compiled_face, vocabulary, ids)?;
            if parts.is_empty() {
                return Err("oracle:unsupported-grant-ability-text");
            }
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
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
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
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
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
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
                && let Some(last) = builder.instructions.last_mut()
            {
                match last.clone() {
                    Instruction::Damage { amount } => {
                        *last =
                            Instruction::DamageAmountThenExileIfDies(ValueExpr::Literal(amount));
                        return Ok(());
                    }
                    Instruction::DamageAmount(expr) => {
                        *last = Instruction::DamageAmountThenExileIfDies(expr);
                        return Ok(());
                    }
                    _ => {}
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
            if is_exile && ev_low.contains("leave the battlefield") {
                let eff_subj = if subj_low == "it"
                    || subj_low == "that creature"
                    || subj_low == "that permanent"
                {
                    if builder.target != BodyTarget::None {
                        Some(EffectSubject::TargetObject)
                    } else if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else if in_trigger {
                        Some(EffectSubject::TriggerSubject)
                    } else {
                        None
                    }
                } else if subj_low == "~" {
                    if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else {
                        None
                    }
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
                && (subj_low.contains("spell") || ev_low.contains("spell"))
            {
                let players = if subj_low.contains("you cast") || subj_low.contains("your") {
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
                let eff_subj = if subj_low.is_empty()
                    || subj_low.contains("a creature")
                    || subj_low.contains("any creature")
                {
                    Some(EffectSubject::Matching(ObjectFilter::creature()))
                } else if subj_low == "it"
                    || subj_low == "that creature"
                    || subj_low == "that permanent"
                {
                    if builder.target != BodyTarget::None {
                        Some(EffectSubject::TargetObject)
                    } else if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else if in_trigger {
                        Some(EffectSubject::TriggerSubject)
                    } else {
                        None
                    }
                } else if subj_low.contains("creature you control") {
                    Some(EffectSubject::Matching(
                        ObjectFilter::creature().controlled(),
                    ))
                } else if subj_low.contains("creature an opponent controls") {
                    Some(EffectSubject::Matching(ObjectFilter::creature().opposing()))
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
            let exc = parse_copy_exceptions(exception.as_deref(), vocabulary)?;
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
                SubjectRefAst::Anaphora(_)
                    if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
                SubjectRefAst::TriggerSubject if in_trigger => MoveSubjectRef::TriggeringObject,
                SubjectRefAst::Anaphora(_)
                    if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
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
            let bounds = parse_modal_choice(choice)?;
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
            match bounds {
                (1, 1) => {
                    let branches = mode_bodies
                        .into_iter()
                        .enumerate()
                        .map(|(idx, body)| ChooseOptionBranch {
                            label: format!("Mode {}", idx + 1),
                            body,
                        })
                        .collect();
                    builder.instructions.push(Instruction::ChooseOption {
                        chooser: ResolvingPlayer::Controller,
                        options: branches,
                    });
                    Ok(())
                }
                (0, 1) => {
                    let mut branches: Vec<ChooseOptionBranch> = mode_bodies
                        .into_iter()
                        .enumerate()
                        .map(|(idx, body)| ChooseOptionBranch {
                            label: format!("Mode {}", idx + 1),
                            body,
                        })
                        .collect();
                    branches.push(ChooseOptionBranch {
                        label: "None".to_owned(),
                        body: ExecutableBody::new(BodyTarget::None, Vec::new()),
                    });
                    builder.instructions.push(Instruction::ChooseOption {
                        chooser: ResolvingPlayer::Controller,
                        options: branches,
                    });
                    Ok(())
                }
                (1, max) if mode_bodies.len() == 2 && max >= 2 => {
                    let mut both_ins = mode_bodies[0].instructions.clone();
                    both_ins.extend(mode_bodies[1].instructions.clone());
                    let branches = vec![
                        ChooseOptionBranch {
                            label: "Mode 1".to_owned(),
                            body: mode_bodies[0].clone(),
                        },
                        ChooseOptionBranch {
                            label: "Mode 2".to_owned(),
                            body: mode_bodies[1].clone(),
                        },
                        ChooseOptionBranch {
                            label: "Both modes".to_owned(),
                            body: ExecutableBody::new(BodyTarget::None, both_ins),
                        },
                    ];
                    builder.instructions.push(Instruction::ChooseOption {
                        chooser: ResolvingPlayer::Controller,
                        options: branches,
                    });
                    Ok(())
                }
                (2, 2) if mode_bodies.len() == 3 => {
                    let mut branches = Vec::with_capacity(3);
                    for (a, b) in [(0usize, 1usize), (0, 2), (1, 2)] {
                        let mut combined = mode_bodies[a].instructions.clone();
                        combined.extend(mode_bodies[b].instructions.clone());
                        branches.push(ChooseOptionBranch {
                            label: format!("Modes {} and {}", a + 1, b + 1),
                            body: ExecutableBody::new(BodyTarget::None, combined),
                        });
                    }
                    builder.instructions.push(Instruction::ChooseOption {
                        chooser: ResolvingPlayer::Controller,
                        options: branches,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-modal-choice"),
            }
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
                "their" | "their library"
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    Some((None, ResolvingPlayer::TargetPlayer))
                }
                "that player" | "that player's" | "thatplayer" | "their" | "their library" => {
                    Some((None, ResolvingPlayer::UniqueRelatedPlayer))
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
        } => {
            let rp = lower_resolving_player(*player, builder)?;
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
                SubjectRefAst::Anaphora(_) if in_trigger && !self_pronoun_valid => {
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
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::WinGame(rp));
            Ok(())
        }
        InstructionAst::LoseGame { player } => {
            let players = lower_affected_players(*player, builder)?;
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
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err("oracle:empty-emblem");
            }
            let emblem_face = spellwake_oracle_compiler::compile_face(
                FaceInput {
                    name: "Emblem",
                    type_line: "Enchantment",
                    oracle_text: trimmed,
                },
                &cached_compiler_vocab(vocabulary),
            );
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
            builder
                .instructions
                .push(Instruction::CreateEmblem(EmblemDefinition::Authored {
                    label: trimmed.to_string(),
                    continuous,
                    triggered,
                }));
            Ok(())
        }
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
    zone: Option<ZoneFilter>,
    default_zone: SourceZoneFilter,
    builder: &mut BodyBuilder,
) -> Result<(ResolvingPlayer, SourceZoneFilter), &'static str> {
    match zone {
        None => Ok((ResolvingPlayer::Controller, default_zone)),
        Some(ZoneFilter::Battlefield) => {
            Ok((ResolvingPlayer::Controller, SourceZoneFilter::Battlefield))
        }
        Some(ZoneFilter::YourHand) => Ok((ResolvingPlayer::Controller, SourceZoneFilter::Hand)),
        Some(ZoneFilter::ThatPlayerHand) => {
            Ok((ResolvingPlayer::UniqueRelatedPlayer, SourceZoneFilter::Hand))
        }
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
            ResolvingPlayer::UniqueRelatedPlayer,
            SourceZoneFilter::Graveyard,
        )),
        Some(ZoneFilter::TargetPlayerGraveyard) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok((ResolvingPlayer::TargetPlayer, SourceZoneFilter::Graveyard))
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
) -> Result<ResolvingPlayer, &'static str> {
    match subj {
        SubjectRefAst::Player(p) => lower_resolving_player(*p, builder),
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
) -> Result<CopyExceptions, &'static str> {
    let mut out = CopyExceptions::default();
    let Some(raw) = exception.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(out);
    };
    let low = raw.to_ascii_lowercase();
    if low.contains("isn't legendary")
        || low.contains("aren't legendary")
        || low.contains("not legendary")
    {
        out.nonlegendary = true;
    }
    if low.contains("its name is still")
        || low.contains("except its name is")
        || low.contains("keeps its name")
    {
        out.keep_name = true;
    }
    if low.contains("in addition to its other types")
        || low.contains("in addition to their other types")
        || low.contains("still a ")
        || low.contains("still an ")
    {
        out.keep_types = true;
    }
    if low.contains("in addition to its other abilities")
        || low.contains("except it has this ability")
        || low.contains("except it keeps its abilities")
        || low.contains("keeps its abilities")
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
        ("indestructible", KeywordAbility::Indestructible),
        ("menace", KeywordAbility::Menace),
        ("reach", KeywordAbility::Reach),
        ("defender", KeywordAbility::Defender),
        ("flash", KeywordAbility::Flash),
    ] {
        if remainder.contains(phrase) {
            out.extra_keywords.insert(kw);
            remainder = remainder.replace(phrase, " ");
        }
    }
    for pat in [
        "except it isn't legendary",
        "except they aren't legendary",
        "it isn't legendary",
        "they aren't legendary",
        "not legendary",
        "its name is still ~",
        "except its name is ~",
        "it keeps its name",
        "keeps its name",
        "in addition to its other types",
        "in addition to their other types",
        "in addition to its other abilities",
        "except it has this ability",
        "except it keeps its abilities",
        "keeps its abilities",
        "except it's still a land",
        "it's still a land",
        "it's still an artifact",
        "it's still an enchantment",
        "it's still a creature",
        "with base power and toughness",
        "base power and toughness",
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
        "they're",
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

fn is_battlefield_controller_dest(dest: &str) -> bool {
    matches!(
        dest,
        "battlefield"
            | "battlefield tapped"
            | "battlefield under your control"
            | "battlefield tapped under your control"
            | "the battlefield"
            | "the battlefield tapped"
            | "the battlefield tapped and attacking"
            | "the battlefield under your control"
            | "the battlefield tapped under your control"
            | "the battlefield tapped and attacking under your control"
    )
}

fn is_battlefield_owner_control_dest(dest: &str) -> bool {
    matches!(
        dest,
        "battlefield under its owner's control"
            | "battlefield tapped under its owner's control"
            | "battlefield under their owner's control"
            | "battlefield tapped under their owner's control"
            | "battlefield under their owners' control"
            | "battlefield tapped under their owners' control"
            | "the battlefield under its owner's control"
            | "the battlefield tapped under its owner's control"
            | "the battlefield under their owner's control"
            | "the battlefield tapped under their owner's control"
            | "the battlefield under their owners' control"
            | "the battlefield tapped under their owners' control"
    )
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
