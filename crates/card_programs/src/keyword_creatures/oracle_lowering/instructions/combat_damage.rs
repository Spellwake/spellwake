use super::*;

pub(super) fn lower_combat_damage_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            let target_is_explicit_target = matches!(
                target.as_ref(),
                SubjectRefAst::AnyTarget
                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                    | SubjectRefAst::TargetPlayer { .. }
                    | SubjectRefAst::TargetObject { .. }
                    | SubjectRefAst::Player(
                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                    )
            );
            let target_matches_permanent_body_target = matches!(
                target.as_ref(),
                SubjectRefAst::TargetObject { .. } | SubjectRefAst::AnyTarget
            ) || matches!(
                target.as_ref(),
                SubjectRefAst::Anaphora(a) if !a.eq_ignore_ascii_case("itself")
            );
            let target_is_returned_gy_card = matches!(
                builder.target,
                BodyTarget::SingleOwnGraveyardCard
                    | BodyTarget::AnyGraveyardCard
                    | BodyTarget::AnyGraveyardCreature
                    | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                    | BodyTarget::FilteredOwnGraveyardCard(_)
                    | BodyTarget::FilteredAnyGraveyardCard { .. }
            ) && builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ReturnOwnGraveyardCreatureToBattlefield
                        | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                        | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { .. }
                        | Instruction::MoveSubject {
                            subject: MoveSubjectRef::TargetObject,
                            destination: SubjectDestination::Battlefield { .. },
                        }
                )
            });
            let target_can_be_damage_source =
                is_permanent_body_target(builder.target) || target_is_returned_gy_card;
            let is_explicit_self_noun = |raw: &str| {
                let low = raw.trim().to_ascii_lowercase();
                low == "this spell"
                    || ((self_pronoun_valid || in_trigger)
                        && matches!(
                            low.as_str(),
                            "this creature"
                                | "this permanent"
                                | "this artifact"
                                | "this enchantment"
                                | "this land"
                                | "this planeswalker"
                        ))
            };
            let is_self_damage_source = is_self_pronoun(source, builder, self_pronoun_valid)
                || match source.as_ref() {
                    SubjectRefAst::Anaphora(raw) if is_explicit_self_noun(raw) => true,
                    SubjectRefAst::Anaphora(raw)
                        if is_self_anaphora_label(raw)
                            && (self_pronoun_valid
                                || (!in_trigger
                                    && !last_instruction_created_tokens(
                                        &builder.instructions,
                                    )
                                    && !last_instruction_moved_object(&builder.instructions)
                                    && (builder.target != BodyTarget::None
                                        || target_is_explicit_target)))
                            && (!target_can_be_damage_source
                                || target_matches_permanent_body_target) =>
                    {
                        true
                    }
                    _ => false,
                };
            if !is_self_damage_source {
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
                        let bt = lower_battlefield_body_target_in_context(
                            filter, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        Some(DamageSourceRef::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_permanent_body_target(builder.target)
                            && builder.instructions.iter().any(|op| {
                                matches!(
                                    op,
                                    Instruction::AffectPermanentTargets {
                                        minimum,
                                        maximum,
                                        ..
                                    } if (*minimum, *maximum) != (1, 1)
                                )
                            }) =>
                    {
                        Some(DamageSourceRef::AllPermanentTargets)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if target_can_be_damage_source
                            && (!is_self_anaphora_label(raw)
                                || !target_matches_permanent_body_target) =>
                    {
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_moved_object(&builder.instructions) =>
                    {
                        Some(DamageSourceRef::LastMoved)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(DamageSourceRef::TriggeringObject)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some(DamageSourceRef::TriggeringObject)
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        Some(DamageSourceRef::AttachedHost)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj =
                            lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
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
                        DamageAmountSpec::Expr(rewrite_milled_card_mana_value(
                            lower_value_expr_in_context(
                                resolve_x(amount, bound_x),
                                builder,
                                in_trigger,
                                vocabulary,
                            )?,
                            builder,
                        ))
                    };
                    let rec_ref = match target.as_ref() {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::UpTo(1),
                        } if filter.zone.is_none() => {
                            let bt = lower_battlefield_body_target_in_context(
                                filter, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            builder.up_to_one_target = true;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } => {
                            let bt = lower_target_one_of_damage_body_target(
                                options, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            if *count == TargetCountAst::UpTo(1) {
                                if is_permanent_body_target(bt) {
                                    builder.up_to_one_target = true;
                                } else if src == DamageSourceRef::AbilitySource
                                    && let DamageAmountSpec::Expr(expr) = amt_spec
                                {
                                    builder.instructions.push(
                                        Instruction::DealDamageToAnnouncedTargets {
                                            amount: expr,
                                            min_targets: 0,
                                            max_targets: 1,
                                        },
                                    );
                                    return Ok(());
                                } else {
                                    return Err("oracle:unsupported-damage-target");
                                }
                            }
                            Some(DamageRecipientsRef::Target)
                        }
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
                                DamageSourceRef::LastMoved => Some(DamageRecipientsRef::LastMoved),
                                DamageSourceRef::CreatedTokens => {
                                    Some(DamageRecipientsRef::CreatedTokens)
                                }
                                _ => None,
                            }
                        }
                        SubjectRefAst::Anaphora(raw)
                            if is_player_anaphora_label(raw)
                                && matches!(builder.target, BodyTarget::SingleSpell(_)) =>
                        {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                            )))
                        }
                        SubjectRefAst::Anaphora(raw)
                            if is_player_anaphora_label(raw)
                                && is_permanent_body_target(builder.target) =>
                        {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Anaphora(_)
                            if last_instruction_created_tokens(&builder.instructions)
                                && !matches!(src, DamageSourceRef::CreatedTokens) =>
                        {
                            Some(DamageRecipientsRef::CreatedTokens)
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target != BodyTarget::None
                                && !is_stack_body_target(builder.target)
                                && !is_graveyard_body_target(builder.target) =>
                        {
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(builder.target)
                                && last_instruction_moved_object(&builder.instructions)
                                && !matches!(src, DamageSourceRef::LastMoved) =>
                        {
                            Some(DamageRecipientsRef::LastMoved)
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
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                            Some(DamageRecipientsRef::AttachedHost)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController,
                        ) if builder.target != BodyTarget::None => {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                            if matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            ) =>
                        {
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                            if matches!(builder.target, BodyTarget::SingleSpell(_)) =>
                        {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                            )))
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
                        SubjectRefAst::Player(PlayerRefAst::EachOtherPlayer) => {
                            Some(DamageRecipientsRef::EachOtherPlayer)
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachOtherOpponent) => {
                            Some(DamageRecipientsRef::EachOtherOpponent)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetOpponentAtRandom) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::RandomOpponent),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::ActivePlayer) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::ActivePlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::DefendingPlayer) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::DefendingPlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EnchantedPlayer)
                            if in_trigger || self_pronoun_valid || builder.has_source =>
                        {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::EnchantedPlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                            if !in_trigger && is_permanent_body_target(builder.target) =>
                        {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ThatPlayer
                            | PlayerRefAst::ItsController
                            | PlayerRefAst::ThatObjectController,
                        ) if in_trigger => {
                            Some(DamageRecipientsRef::Group(DamageGroup::RelatedPlayer))
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f)
                            if f.zone.is_none() =>
                        {
                            let group = lower_damage_group(f, vocabulary)?;
                            if let DamageGroup::Filtered(obj) = group {
                                bind_filter_player_target(builder, &obj)?;
                            }
                            Some(DamageRecipientsRef::Group(group))
                        }
                        SubjectRefAst::Compound(items) if items.len() == 2 => {
                            let is_other_creatures = |s: &SubjectRefAst| {
                                matches!(
                                    s,
                                    SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                        if f.zone.is_none()
                                            && f.any_types == AstCardTypeMask::CREATURE
                                            && f.flags == AstFilterFlags::EXCLUDE_SELF
                                            && filter_only_has(
                                                f,
                                                FilterAllow {
                                                    types: true,
                                                    flags: true,
                                                    ..FilterAllow::NONE
                                                },
                                            )
                                )
                            };
                            let is_all_creatures = |s: &SubjectRefAst| {
                                matches!(
                                    s,
                                    SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                        if f.zone.is_none()
                                            && f.any_types == AstCardTypeMask::CREATURE
                                            && filter_only_has(
                                                f,
                                                FilterAllow {
                                                    types: true,
                                                    ..FilterAllow::NONE
                                                },
                                            )
                                )
                            };
                            let is_all_pws = |s: &SubjectRefAst| {
                                matches!(
                                    s,
                                    SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                        if f.zone.is_none()
                                            && f.any_types == AstCardTypeMask::PLANESWALKER
                                            && filter_only_has(
                                                f,
                                                FilterAllow {
                                                    types: true,
                                                    ..FilterAllow::NONE
                                                },
                                            )
                                )
                            };
                            let is_each_player = |s: &SubjectRefAst| {
                                matches!(s, SubjectRefAst::Player(PlayerRefAst::EachPlayer))
                            };
                            if (is_other_creatures(&items[0]) && is_each_player(&items[1]))
                                || (is_each_player(&items[0]) && is_other_creatures(&items[1]))
                            {
                                Some(DamageRecipientsRef::OtherCreaturesAndEachPlayer)
                            } else if (is_all_creatures(&items[0]) && is_each_player(&items[1]))
                                || (is_each_player(&items[0]) && is_all_creatures(&items[1]))
                            {
                                Some(DamageRecipientsRef::Group(DamageGroup::CreaturesAndPlayers))
                            } else if (is_all_creatures(&items[0]) && is_all_pws(&items[1]))
                                || (is_all_pws(&items[0]) && is_all_creatures(&items[1]))
                            {
                                Some(DamageRecipientsRef::Group(
                                    DamageGroup::CreaturesAndPlaneswalkers,
                                ))
                            } else {
                                None
                            }
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
            let amt_expr = rewrite_milled_card_mana_value(
                lower_value_expr_in_context(effective_amount, builder, in_trigger, vocabulary)?,
                builder,
            );
            match target.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() && !filter.is_ability && !filter.any_spell => {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if filter.zone.is_none()
                        && !filter.is_ability
                        && !filter.any_spell
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
                        _ => return Err("oracle:unsupported-damage-target"),
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
                            action: PermanentTargetAction::Damage(amt_expr),
                        });
                    Ok(())
                }
                SubjectRefAst::TargetPlayer {
                    opponent_only,
                    count,
                } if matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                    || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                    || matches!(count, TargetCountAst::AnyNumber)
                    || (matches!(count, TargetCountAst::ChosenX) && !in_trigger) =>
                {
                    let (min_t, max_t, is_x) = match *count {
                        TargetCountAst::Exact(n) => (n, n, false),
                        TargetCountAst::UpTo(max) => (0, max, false),
                        TargetCountAst::AnyNumber => (0, u16::MAX, false),
                        TargetCountAst::ChosenX => (0, u16::MAX, true),
                        _ => return Err("oracle:unsupported-damage-target"),
                    };
                    builder.bind_target(if *opponent_only {
                        BodyTarget::SingleOpponent
                    } else {
                        BodyTarget::SinglePlayer
                    })?;
                    if is_x {
                        builder.x_target_count = true;
                    }
                    builder
                        .instructions
                        .push(Instruction::DealDamageToAnnouncedTargets {
                            amount: amt_expr,
                            min_targets: min_t,
                            max_targets: max_t,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetOneOf { options, count } => {
                    let bt = lower_target_one_of_damage_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    match *count {
                        TargetCountAst::Exact(1) => {
                            push_damage_target(&mut builder.instructions, amt_expr);
                            Ok(())
                        }
                        TargetCountAst::Exact(n) if n >= 2 => {
                            if is_permanent_body_target(bt) {
                                builder
                                    .instructions
                                    .push(Instruction::AffectPermanentTargets {
                                        minimum: n,
                                        maximum: n,
                                        action: PermanentTargetAction::Damage(amt_expr),
                                    });
                            } else {
                                builder.instructions.push(
                                    Instruction::DealDamageToAnnouncedTargets {
                                        amount: amt_expr,
                                        min_targets: n,
                                        max_targets: n,
                                    },
                                );
                            }
                            Ok(())
                        }
                        TargetCountAst::UpTo(max) if max > 0 => {
                            if is_permanent_body_target(bt) {
                                builder
                                    .instructions
                                    .push(Instruction::AffectPermanentTargets {
                                        minimum: 0,
                                        maximum: max,
                                        action: PermanentTargetAction::Damage(amt_expr),
                                    });
                            } else {
                                builder.instructions.push(
                                    Instruction::DealDamageToAnnouncedTargets {
                                        amount: amt_expr,
                                        min_targets: 0,
                                        max_targets: max,
                                    },
                                );
                            }
                            Ok(())
                        }
                        TargetCountAst::AnyNumber => {
                            if is_permanent_body_target(bt) {
                                builder
                                    .instructions
                                    .push(Instruction::AffectPermanentTargets {
                                        minimum: 0,
                                        maximum: u16::MAX,
                                        action: PermanentTargetAction::Damage(amt_expr),
                                    });
                            } else {
                                builder.instructions.push(
                                    Instruction::DealDamageToAnnouncedTargets {
                                        amount: amt_expr,
                                        min_targets: 0,
                                        max_targets: u16::MAX,
                                    },
                                );
                            }
                            Ok(())
                        }
                        TargetCountAst::ChosenX if !in_trigger => {
                            builder.x_target_count = true;
                            if is_permanent_body_target(bt) {
                                builder
                                    .instructions
                                    .push(Instruction::AffectPermanentTargets {
                                        minimum: 0,
                                        maximum: u16::MAX,
                                        action: PermanentTargetAction::Damage(amt_expr),
                                    });
                            } else {
                                builder.instructions.push(
                                    Instruction::DealDamageToAnnouncedTargets {
                                        amount: amt_expr,
                                        min_targets: 0,
                                        max_targets: u16::MAX,
                                    },
                                );
                            }
                            Ok(())
                        }
                        _ => Err("oracle:unsupported-damage-target"),
                    }
                }
                SubjectRefAst::AnyTarget
                | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                | SubjectRefAst::TargetPlayer { .. }
                | SubjectRefAst::TargetObject { .. } => {
                    let bt = lower_target_subject(target, vocabulary)?;
                    builder.bind_target(bt)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if is_player_anaphora_label(raw)
                        && matches!(builder.target, BodyTarget::SingleSpell(_)) =>
                {
                    let group = DamageGroup::AffectedPlayers(AffectedPlayers::One(
                        ResolvingPlayer::TargetSpellController,
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
                SubjectRefAst::Anaphora(raw)
                    if is_player_anaphora_label(raw)
                        && is_permanent_body_target(builder.target) =>
                {
                    let group = DamageGroup::AffectedPlayers(AffectedPlayers::One(
                        ResolvingPlayer::TargetController,
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
                SubjectRefAst::Anaphora(_)
                    if builder.target != BodyTarget::None
                        && !is_stack_body_target(builder.target)
                        && !is_graveyard_body_target(builder.target) =>
                {
                    if let Some((minimum, maximum)) =
                        builder.instructions.iter().rev().find_map(|op| {
                            if let Instruction::AffectPermanentTargets {
                                minimum, maximum, ..
                            } = op
                            {
                                Some((*minimum, *maximum))
                            } else {
                                None
                            }
                        })
                    {
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum,
                                maximum,
                                action: PermanentTargetAction::Damage(amt_expr),
                            });
                    } else {
                        push_damage_target(&mut builder.instructions, amt_expr);
                    }
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
                                | SubjectRefAst::TargetPlayer { .. }
                                | SubjectRefAst::TargetObject { .. }
                                | SubjectRefAst::TargetOneOf { .. }
                                | SubjectRefAst::Player(
                                    PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                                )
                        )
                    }) {
                        let is_dmg_target_item = |it: &SubjectRefAst| {
                            matches!(
                                it,
                                SubjectRefAst::AnyTarget
                                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                                    | SubjectRefAst::TargetPlayer { .. }
                                    | SubjectRefAst::TargetObject { .. }
                                    | SubjectRefAst::TargetOneOf { .. }
                                    | SubjectRefAst::Player(
                                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                                    )
                            )
                        };
                        let target_count = items.iter().filter(|it| is_dmg_target_item(it)).count();
                        if target_count > 1 {
                            if builder.target != BodyTarget::None {
                                return Err("oracle:unsupported-damage-target");
                            }
                            let mut trial = builder.clone();
                            for it in items {
                                let mut sub = BodyBuilder::with_trigger_hint(trial.trigger_hint);
                                sub.has_source =
                                    trial.has_source || in_trigger || self_pronoun_valid;
                                sub.last_group_filter = trial.last_group_filter;
                                lower_instruction(
                                    &InstructionAst::DealDamage {
                                        source: source.clone(),
                                        target: Box::new(it.clone()),
                                        amount: amount.clone(),
                                    },
                                    &mut sub,
                                    vocabulary,
                                    ids,
                                    in_trigger,
                                    self_pronoun_valid,
                                    bound_x,
                                )?;
                                if sub.target != BodyTarget::None {
                                    let body = sub.finish()?;
                                    if body.permanent_target_bounds().is_some()
                                        || !body
                                            .instructions
                                            .iter()
                                            .any(Instruction::uses_mode_target)
                                    {
                                        return Err("oracle:unsupported-damage-target");
                                    }
                                    trial
                                        .instructions
                                        .push(Instruction::FixedTargetClause(Box::new(body)));
                                } else {
                                    splice_probed_body(&mut trial, sub)?;
                                }
                            }
                            *builder = trial;
                            return Ok(());
                        }
                        if target_count != 1 {
                            return Err("oracle:unsupported-damage-target");
                        }
                        let mut target_items = Vec::new();
                        let mut non_target_items = Vec::new();
                        for it in items {
                            if is_dmg_target_item(it) {
                                target_items.push(it);
                            } else {
                                non_target_items.push(it);
                            }
                        }
                        for it in target_items.into_iter().chain(non_target_items) {
                            let mut adj_it = it.clone();
                            if matches!(
                                builder.target,
                                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                            ) {
                                let tgt_ctrl = if builder.target == BodyTarget::SingleOpponent {
                                    ControllerRelation::TargetOpponent
                                } else {
                                    ControllerRelation::TargetPlayer
                                };
                                match &mut adj_it {
                                    SubjectRefAst::Each(f)
                                    | SubjectRefAst::All(f)
                                    | SubjectRefAst::Group(f)
                                        if f.controller == Some(ControllerRelation::ThatPlayer) =>
                                    {
                                        f.controller = Some(tgt_ctrl);
                                    }
                                    _ => {}
                                }
                            }
                            lower_instruction(
                                &InstructionAst::DealDamage {
                                    source: source.clone(),
                                    target: Box::new(adj_it),
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
                        let mut simple_fallback_needed = false;
                        for item in items {
                            if let SubjectRefAst::Player(p) = item {
                                let ap =
                                    lower_affected_players_in_context(*p, builder, in_trigger)?;
                                if player_group.is_some() {
                                    simple_fallback_needed = true;
                                    break;
                                }
                                player_group = Some(ap);
                            }
                        }
                        if !simple_fallback_needed {
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
                                                    adj.controller =
                                                        Some(ControllerRelation::Opponent);
                                                }
                                                Some(AffectedPlayers::EachPlayer) => {
                                                    adj.controller = None;
                                                }
                                                Some(AffectedPlayers::EachOtherPlayer) => {
                                                    adj.controller =
                                                        Some(ControllerRelation::NotYou);
                                                }
                                                Some(AffectedPlayers::One(
                                                    ResolvingPlayer::Controller,
                                                )) => {
                                                    adj.controller = Some(ControllerRelation::You);
                                                }
                                                Some(AffectedPlayers::One(
                                                    ResolvingPlayer::TargetPlayer,
                                                )) => {
                                                    adj.controller = Some(
                                                        if builder.target
                                                            == BodyTarget::SingleOpponent
                                                        {
                                                            ControllerRelation::TargetOpponent
                                                        } else {
                                                            ControllerRelation::TargetPlayer
                                                        },
                                                    );
                                                }
                                                Some(AffectedPlayers::One(
                                                    ResolvingPlayer::DefendingPlayer,
                                                )) => {
                                                    adj.controller =
                                                        Some(ControllerRelation::DefendingPlayer);
                                                }
                                                _ => {}
                                            }
                                        }
                                        let obj = lower_object_filter_in_context(
                                            &adj, builder, in_trigger, vocabulary,
                                        )?;
                                        bind_filter_player_target(builder, &obj)?;
                                        perm_filters.push(obj);
                                    }
                                    SubjectRefAst::Player(_) => {}
                                    _ => {
                                        simple_fallback_needed = true;
                                        break;
                                    }
                                }
                            }
                            if !simple_fallback_needed {
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
                                        let Ok([first, second]) =
                                            <[ObjectClause; 2]>::try_from(clauses)
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
                            } else {
                                for it in items {
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
                            }
                        } else {
                            for it in items {
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
                SubjectRefAst::Player(PlayerRefAst::EachOtherOpponent) => {
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
                SubjectRefAst::Player(p)
                    if matches!(
                        p,
                        PlayerRefAst::ActivePlayer
                            | PlayerRefAst::EachOtherPlayer
                            | PlayerRefAst::DefendingPlayer
                            | PlayerRefAst::EnchantedPlayer
                            | PlayerRefAst::ItsController
                            | PlayerRefAst::ItsOwner
                            | PlayerRefAst::ThatObjectController
                            | PlayerRefAst::TargetOpponentAtRandom
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
        InstructionAst::Regenerate { subject } => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::RegenerateSource);
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
                    builder.instructions.push(Instruction::RegenerateTarget);
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if filter.zone.is_none()
                        && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    let (minimum, maximum) = match *count {
                        TargetCountAst::Exact(n) => (n, n),
                        TargetCountAst::UpTo(1) => {
                            builder.up_to_one_target = true;
                            (0, 1)
                        }
                        TargetCountAst::UpTo(max) => (0, max),
                        TargetCountAst::AnyNumber => (0, u16::MAX),
                        TargetCountAst::ChosenX => {
                            builder.x_target_count = true;
                            (0, u16::MAX)
                        }
                        _ => return Err("oracle:unsupported-regenerate"),
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum,
                            maximum,
                            action: PermanentTargetAction::Regenerate,
                        });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if has_source => {
                    builder.instructions.push(Instruction::RegenerateSubject {
                        subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))),
                    });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                    let mut cleaned = f.as_ref().clone();
                    let mut other_than_target = false;
                    let mut kept_preds = Vec::with_capacity(cleaned.predicates.len());
                    for pred in cleaned.predicates {
                        match pred {
                            spellwake_oracle_compiler::ast::FilterPredicateAst::Exclude(
                                ref excl,
                            ) if is_permanent_body_target(builder.target)
                                && matches!(excl.as_ref(), SubjectRefAst::Anaphora(_)) =>
                            {
                                other_than_target = true;
                            }
                            spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(
                                ref s,
                            ) if is_permanent_body_target(builder.target)
                                && matches!(
                                    s.trim().to_ascii_lowercase().as_str(),
                                    "other than that creature"
                                        | "other than that permanent"
                                        | "other than that card"
                                ) =>
                            {
                                other_than_target = true;
                            }
                            other => kept_preds.push(other),
                        }
                    }
                    cleaned.predicates = kept_preds;
                    if !has_source
                        && is_permanent_body_target(builder.target)
                        && (cleaned.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0
                    {
                        cleaned.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
                        other_than_target = true;
                    }
                    let obj =
                        lower_object_filter_in_context(&cleaned, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    let subj = if other_than_target {
                        EffectSubject::MatchingOtherThanTarget(obj)
                    } else {
                        EffectSubject::Matching(obj)
                    };
                    builder.instructions.push(Instruction::RegenerateSubject {
                        subject: Box::new(subj),
                    });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::RegenerateSubject {
                        subject: Box::new(EffectSubject::TriggerSubject),
                    });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw) => {
                    if let Some((minimum, maximum)) =
                        builder.instructions.iter().rev().find_map(|op| {
                            if let Instruction::AffectPermanentTargets {
                                minimum, maximum, ..
                            } = op
                            {
                                Some((*minimum, *maximum))
                            } else {
                                None
                            }
                        })
                    {
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum,
                                maximum,
                                action: PermanentTargetAction::Regenerate,
                            });
                        Ok(())
                    } else if is_permanent_body_target(builder.target) {
                        builder.instructions.push(Instruction::RegenerateTarget);
                        Ok(())
                    } else if last_instruction_created_tokens(&builder.instructions) {
                        builder.instructions.push(Instruction::RegenerateSubject {
                            subject: Box::new(EffectSubject::CreatedTokens),
                        });
                        Ok(())
                    } else if last_instruction_moved_object(&builder.instructions) {
                        builder.instructions.push(Instruction::RegenerateSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                        });
                        Ok(())
                    } else if let Some(group) = builder.last_group_filter {
                        builder.instructions.push(Instruction::RegenerateSubject {
                            subject: Box::new(EffectSubject::Matching(group)),
                        });
                        Ok(())
                    } else if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) {
                        builder.instructions.push(Instruction::RegenerateSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                        });
                        Ok(())
                    } else {
                        Err("oracle:unsupported-regenerate")
                    }
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    let mut trial = builder.clone();
                    for item in items {
                        lower_instruction(
                            &InstructionAst::Regenerate {
                                subject: Box::new(item.clone()),
                            },
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
                _ => Err("oracle:unsupported-regenerate"),
            }
        }
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
            if !builder.up_to_one_target
                && first == MoveSubjectRef::AbilitySource
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
        InstructionAst::PreventAllDamageFrom {
            combat_only,
            source,
            recipient,
            duration: DurationAst::UntilEndOfTurn,
        } => interactive::lower_prevent_all_damage_from(
            *combat_only,
            source,
            recipient.as_deref(),
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
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
            combat_only,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } => interactive::lower_prevent_all_damage(
            *combat_only,
            subj,
            builder,
            vocabulary,
            self_pronoun_valid,
        ),
        InstructionAst::PreventDamage {
            amount,
            target,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let amt_expr = lower_value_expr_in_context(
                resolve_x(amount, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let is_target_subj = matches!(
                target.as_ref(),
                SubjectRefAst::AnyTarget
                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                    | SubjectRefAst::TargetPlayer { .. }
                    | SubjectRefAst::TargetObject { .. }
                    | SubjectRefAst::Player(
                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                    )
            );
            if is_target_subj {
                let bt = match target.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() && !filter.is_ability && !filter.any_spell => {
                        lower_battlefield_body_target_in_context(
                            filter, builder, in_trigger, vocabulary,
                        )?
                    }
                    SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => BodyTarget::SinglePlayer,
                    SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                        BodyTarget::SingleOpponent
                    }
                    _ => lower_target_subject(target, vocabulary)?,
                };
                builder.bind_target(bt)?;
                if dur == ResolvedDuration::UntilEndOfTurn
                    && let ValueExpr::Literal(n) = &amt_expr
                    && n.is_positive()
                {
                    builder
                        .instructions
                        .push(Instruction::CreatePreventionShield { amount: n.clone() });
                    return Ok(());
                }
                let recipient = if is_permanent_body_target(bt) {
                    StaticDamageRecipient::TargetObject
                } else if matches!(
                    bt,
                    BodyTarget::SingleAnyTarget
                        | BodyTarget::SingleCreatureOrPlayer
                        | BodyTarget::SinglePlayerOrPlaneswalker
                        | BodyTarget::SingleOpponentOrPlaneswalker
                        | BodyTarget::SinglePlayer
                        | BodyTarget::SingleOpponent
                ) {
                    StaticDamageRecipient::TargetAny
                } else {
                    return Err("oracle:unsupported-prevent-damage");
                };
                let rule = match amt_expr {
                    ValueExpr::Literal(n) if n.is_positive() => {
                        TemporaryRuleKind::PreventNextDamage {
                            amount: n,
                            source: None,
                            recipient,
                            combat_only: false,
                        }
                    }
                    expr => TemporaryRuleKind::PreventNextDamageExpr {
                        amount: Box::new(expr),
                        source: None,
                        recipient,
                        combat_only: false,
                    },
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(rule),
                    duration: dur,
                });
                return Ok(());
            }
            let has_source = in_trigger || self_pronoun_valid;
            let recipient = if matches!(target.as_ref(), SubjectRefAst::Anaphora(_)) {
                if is_permanent_body_target(builder.target) {
                    StaticDamageRecipient::TargetObject
                } else if matches!(
                    builder.target,
                    BodyTarget::SingleAnyTarget
                        | BodyTarget::SingleCreatureOrPlayer
                        | BodyTarget::SinglePlayerOrPlaneswalker
                        | BodyTarget::SingleOpponentOrPlaneswalker
                        | BodyTarget::SinglePlayer
                        | BodyTarget::SingleOpponent
                ) {
                    StaticDamageRecipient::TargetAny
                } else {
                    return Err("oracle:unsupported-prevent-damage");
                }
            } else {
                lower_redirect_from_subject(target, has_source, vocabulary)
                    .ok_or("oracle:unsupported-prevent-damage")?
            };
            let rule = match amt_expr {
                ValueExpr::Literal(n) if n.is_positive() => TemporaryRuleKind::PreventNextDamage {
                    amount: n,
                    source: None,
                    recipient,
                    combat_only: false,
                },
                expr => TemporaryRuleKind::PreventNextDamageExpr {
                    amount: Box::new(expr),
                    source: None,
                    recipient,
                    combat_only: false,
                },
            };
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(rule),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::PreventNextSourceDamage {
            source_desc,
            target,
        } => {
            let has_source = in_trigger || self_pronoun_valid;
            let src_low = source_desc.trim().to_ascii_lowercase();
            let mut trial = builder.clone();
            let (source, source_chosen_color, source_chosen_type, source_is_target) =
                parse_next_time_source_with_target(
                    &src_low, &mut trial, has_source, in_trigger, vocabulary,
                )
                .ok_or("oracle:unsupported-prevent-damage")??;
            let recipient = lower_prevent_recipient_subject(
                target, &mut trial, has_source, in_trigger, vocabulary,
            )?;
            if source_is_target
                && matches!(
                    recipient,
                    StaticDamageRecipient::TargetObject | StaticDamageRecipient::TargetAny
                )
            {
                return Err("oracle:unsupported-prevent-damage");
            }
            trial.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::PreventNextTimeDamage {
                    source,
                    source_chosen_color,
                    source_chosen_type,
                    source_is_target,
                    half_round_down: false,
                    recipient,
                    combat_only: false,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            *builder = trial;
            Ok(())
        }
        InstructionAst::AdditionalCombat(spec) => {
            let low = spec.to_ascii_lowercase();
            if builder.target != BodyTarget::None {
                return Err("oracle:unsupported-additional-combat");
            }
            if low.contains("additional combat phase")
                && low.contains("untap")
                && low.contains("creatures you control")
            {
                builder.instructions.push(Instruction::AdditionalCombat {
                    untap: CombatUntapGroup::ControlledCreatures,
                });
                Ok(())
            } else if in_trigger
                && low.contains("additional combat phase")
                && low.contains("untap")
                && (low.contains("attacking creature") || low.contains("creatures that attacked"))
                && !low.contains("at the beginning of")
                && !low.contains("beginning of that combat")
            {
                builder
                    .instructions
                    .push(Instruction::AdditionalCombatAfterUntappingAttackers);
                Ok(())
            } else if low.contains("additional combat phase")
                && !low.contains("untap")
                && !low.contains("at the beginning of")
                && !low.contains("beginning of that combat")
                && !low.contains("beginning of the next")
                && !low.contains("only once")
                && !low.contains("if it's the first")
                && !low.contains("unless")
                && !low.contains("sacrifice")
                && !low.contains("exile")
                && !low.contains("create")
                && !low.contains("until end of turn")
            {
                builder.instructions.push(Instruction::AdditionalCombat {
                    untap: CombatUntapGroup::None,
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
            let eff_subject = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } => inner.as_ref(),
                other => other,
            };
            if matches!(
                low.trim().trim_end_matches('.'),
                "turn_face_up" | "turn face up" | "turns face up"
            ) {
                let face_up_subj = match eff_subject {
                    s if (self_pronoun_valid || in_trigger)
                        && is_self_pronoun(s, builder, self_pronoun_valid) =>
                    {
                        EffectSubject::AbilitySource
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))
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
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        EffectSubject::CreatedTokens
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        EffectSubject::LastMoved
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger
                            && builder.target == BodyTarget::None
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        EffectSubject::TriggerSubject
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        EffectSubject::Matching(group_obj)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        EffectSubject::Matching(obj)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                builder.instructions.push(Instruction::TurnFaceUpSubject {
                    subject: Box::new(face_up_subj),
                });
                return Ok(());
            }
            if let Some((lhs_raw, rhs_raw)) =
                low.split_once(" and creatures you control can't be the targets of ")
                && let Some(counter_raw) =
                    lhs_raw.strip_prefix("spells you control can't be countered by ")
            {
                let counter_clean = counter_raw
                    .trim()
                    .trim_end_matches(',')
                    .trim_end_matches(" until end of turn")
                    .trim_end_matches(" this turn")
                    .trim();
                let target_clean = rhs_raw
                    .trim()
                    .trim_end_matches('.')
                    .trim_end_matches(" until end of turn")
                    .trim_end_matches(" this turn")
                    .trim();
                if let Some((counter_colors, true)) = parse_cant_be_target_colors(counter_clean)
                    && let Some((target_colors, target_spells_only)) =
                        parse_cant_be_target_colors(target_clean)
                {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::CantBeCounteredByColors {
                            colors: counter_colors,
                            filter: None,
                            controller_only: true,
                            next_only: false,
                        }),
                        duration: dur,
                    });
                    let target_part = if target_spells_only {
                        ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(target_colors)
                    } else {
                        ResolvedPartSpec::CannotBeTargetedByColors(target_colors)
                    };
                    builder.instructions.push(Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::Matching(
                            ObjectFilter::creature().controlled(),
                        )),
                        parts: ResolvedPartsSpec(vec![target_part]),
                        duration: dur,
                    });
                    return Ok(());
                }
            }
            if let Some((is_your_spells, by_raw)) = low
                .strip_prefix("spells you control can't be countered by ")
                .map(|r| (true, r))
                .or_else(|| {
                    low.strip_prefix("can't be countered by ")
                        .map(|r| (false, r))
                })
            {
                let (by_body, eff_dur) = strip_trailing_temp_duration(by_raw, dur)
                    .unwrap_or((by_raw.trim().trim_end_matches('.').to_owned(), dur));
                if let Some((colors, true)) = parse_cant_be_target_colors(&by_body) {
                    let (filter, controller_only) = if is_your_spells {
                        (None, true)
                    } else {
                        match eff_subject {
                            SubjectRefAst::All(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::Group(f)
                                if f.zone.is_none() =>
                            {
                                let mut inner = f.as_ref().clone();
                                let ctrl_only = match inner.controller {
                                    Some(ControllerRelation::You) => {
                                        inner.controller = None;
                                        true
                                    }
                                    None => false,
                                    _ => return Err("oracle:unsupported-combat-mod"),
                                };
                                inner.any_spell = false;
                                let flt = if is_unrestricted_filter(&inner) {
                                    None
                                } else {
                                    Some(lower_object_filter(&inner, vocabulary)?)
                                };
                                (flt, ctrl_only)
                            }
                            _ => return Err("oracle:unsupported-combat-mod"),
                        }
                    };
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::CantBeCounteredByColors {
                            colors,
                            filter,
                            controller_only,
                            next_only: false,
                        }),
                        duration: eff_dur,
                    });
                    return Ok(());
                }
            }
            if low == "it becomes day" || low == "becomes_day" {
                builder
                    .instructions
                    .push(Instruction::SetDayNight(DayNight::Day));
                return Ok(());
            }
            if low == "it becomes night" || low == "becomes_night" {
                builder
                    .instructions
                    .push(Instruction::SetDayNight(DayNight::Night));
                return Ok(());
            }
            if let Some(same_tail) = low.strip_prefix("same_is_true_for:") {
                let parsed_kws = parse_same_is_true_for_keywords(same_tail)?;
                let len = builder.instructions.len();
                if len < 2 {
                    return Err("oracle:unsupported-combat-mod");
                }
                let prev_branch = builder.instructions[len - 2].clone();
                let prev_grant = builder.instructions[len - 1].clone();
                let Instruction::BranchIfFalse {
                    condition:
                        ResolutionCondition {
                            left:
                                ValueExpr::Count {
                                    zone: prev_zone,
                                    filter: prev_filter,
                                    controller_scope,
                                },
                            comparison: Comparison::GreaterOrEqual,
                            right,
                        },
                    destination,
                } = prev_branch
                else {
                    return Err("oracle:unsupported-combat-mod");
                };
                if destination != len || right != ValueExpr::literal(1) {
                    return Err("oracle:unsupported-combat-mod");
                }
                let ObjectFilter::Clause(prev_clause) = *prev_filter else {
                    return Err("oracle:unsupported-combat-mod");
                };
                let Some(prev_kw) = prev_clause.keyword else {
                    return Err("oracle:unsupported-combat-mod");
                };
                enum SameGrantShape {
                    BoostSource,
                    ModifyGroupUntilEndOfTurn(Box<EffectSubject>),
                    ChangeAbilitySource(ResolvedDuration),
                    ChangeGroup(Box<EffectSubject>, ResolvedDuration),
                    CreateContinuousEffect(ResolvedDuration),
                    PutCountersOnSubject(Box<EffectSubject>, ValueExpr),
                    PutNamedCounterOnTarget(ValueExpr),
                    PutNamedCounterOnSource(u16),
                }
                let kw_to_counter = |kw: KeywordAbility| -> Option<CounterKind> {
                    match kw {
                        KeywordAbility::Flying => Some(CounterKind::Flying),
                        KeywordAbility::FirstStrike => Some(CounterKind::FirstStrike),
                        KeywordAbility::DoubleStrike => Some(CounterKind::DoubleStrike),
                        KeywordAbility::Deathtouch => Some(CounterKind::Deathtouch),
                        KeywordAbility::Haste => Some(CounterKind::Haste),
                        KeywordAbility::Hexproof => Some(CounterKind::Hexproof),
                        KeywordAbility::Indestructible => Some(CounterKind::Indestructible),
                        KeywordAbility::Lifelink => Some(CounterKind::Lifelink),
                        KeywordAbility::Menace => Some(CounterKind::Menace),
                        KeywordAbility::Reach => Some(CounterKind::Reach),
                        KeywordAbility::Shadow => Some(CounterKind::Shadow),
                        KeywordAbility::Trample => Some(CounterKind::Trample),
                        KeywordAbility::Vigilance => Some(CounterKind::Vigilance),
                        _ => None,
                    }
                };
                let prev_kw_counter = kw_to_counter(prev_kw);
                let is_single_kw_ab = |parts: &ResolvedPartsSpec| -> bool {
                    matches!(
                        parts.0.as_slice(),
                        [ResolvedPartSpec::Abilities(ab)]
                            if ab.removal.is_none()
                                && ab.suppressed_keywords.is_empty()
                                && ab.granted_exalted == 0
                                && !ab.grants_unblockable
                                && ab.granted_triggers.is_empty()
                                && ab.granted_toxic.is_none()
                                && ab.granted_combat_keywords.is_empty()
                                && ab.granted_keywords.len() == 1
                                && ab.granted_keywords.contains(&prev_kw)
                    )
                };
                let shape = match prev_grant {
                    Instruction::BoostSource {
                        power_toughness: None,
                        keywords,
                    } if keywords.len() == 1 && keywords.contains(&prev_kw) => {
                        SameGrantShape::BoostSource
                    }
                    Instruction::ModifyGroupUntilEndOfTurn {
                        group,
                        power_toughness: None,
                        keywords,
                    } if keywords.len() == 1 && keywords.contains(&prev_kw) => {
                        SameGrantShape::ModifyGroupUntilEndOfTurn(group)
                    }
                    Instruction::ChangeAbilitySource { parts, duration }
                        if is_single_kw_ab(&parts) =>
                    {
                        SameGrantShape::ChangeAbilitySource(duration)
                    }
                    Instruction::ChangeGroup {
                        group,
                        parts,
                        duration,
                    } if is_single_kw_ab(&parts) => SameGrantShape::ChangeGroup(group, duration),
                    Instruction::CreateContinuousEffect { parts, duration }
                        if is_single_kw_ab(&parts) =>
                    {
                        SameGrantShape::CreateContinuousEffect(duration)
                    }
                    Instruction::PutCountersOnSubject {
                        subject,
                        kind,
                        amount,
                    } if prev_kw_counter == Some(kind) => {
                        SameGrantShape::PutCountersOnSubject(subject, amount)
                    }
                    Instruction::PutNamedCounterOnTarget { kind, amount }
                        if prev_kw_counter == Some(kind) =>
                    {
                        SameGrantShape::PutNamedCounterOnTarget(amount)
                    }
                    Instruction::PutNamedCounterOnSource { kind, amount }
                        if prev_kw_counter == Some(kind) =>
                    {
                        SameGrantShape::PutNamedCounterOnSource(amount)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                let is_counter_shape = matches!(
                    shape,
                    SameGrantShape::PutCountersOnSubject(..)
                        | SameGrantShape::PutNamedCounterOnTarget(..)
                        | SameGrantShape::PutNamedCounterOnSource(..)
                );
                if is_counter_shape && parsed_kws.iter().any(|kw| kw_to_counter(*kw).is_none()) {
                    return Err("oracle:unsupported-combat-mod");
                }
                for kw in parsed_kws {
                    let mut clause = prev_clause;
                    clause.keyword = Some(kw);
                    let cond = ResolutionCondition {
                        left: ValueExpr::Count {
                            zone: prev_zone,
                            filter: Box::new(ObjectFilter::Clause(clause)),
                            controller_scope,
                        },
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    };
                    let next_dest = builder.instructions.len() + 2;
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: cond,
                        destination: next_dest,
                    });
                    let grant_op = match &shape {
                        SameGrantShape::BoostSource => Instruction::BoostSource {
                            power_toughness: None,
                            keywords: BTreeSet::from([kw]),
                        },
                        SameGrantShape::ModifyGroupUntilEndOfTurn(group) => {
                            Instruction::ModifyGroupUntilEndOfTurn {
                                group: group.clone(),
                                power_toughness: None,
                                keywords: BTreeSet::from([kw]),
                            }
                        }
                        SameGrantShape::ChangeAbilitySource(d) => {
                            let mut ab = AbilityChange::default();
                            ab.granted_keywords.insert(kw);
                            Instruction::ChangeAbilitySource {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]),
                                duration: *d,
                            }
                        }
                        SameGrantShape::ChangeGroup(group, d) => {
                            let mut ab = AbilityChange::default();
                            ab.granted_keywords.insert(kw);
                            Instruction::ChangeGroup {
                                group: group.clone(),
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]),
                                duration: *d,
                            }
                        }
                        SameGrantShape::CreateContinuousEffect(d) => {
                            let mut ab = AbilityChange::default();
                            ab.granted_keywords.insert(kw);
                            Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]),
                                duration: *d,
                            }
                        }
                        SameGrantShape::PutCountersOnSubject(subj, amt) => {
                            let ck = kw_to_counter(kw).ok_or("oracle:unsupported-combat-mod")?;
                            Instruction::PutCountersOnSubject {
                                subject: subj.clone(),
                                kind: ck,
                                amount: amt.clone(),
                            }
                        }
                        SameGrantShape::PutNamedCounterOnTarget(amt) => {
                            let ck = kw_to_counter(kw).ok_or("oracle:unsupported-combat-mod")?;
                            Instruction::PutNamedCounterOnTarget {
                                kind: ck,
                                amount: amt.clone(),
                            }
                        }
                        SameGrantShape::PutNamedCounterOnSource(amt) => {
                            let ck = kw_to_counter(kw).ok_or("oracle:unsupported-combat-mod")?;
                            Instruction::PutNamedCounterOnSource {
                                kind: ck,
                                amount: *amt,
                            }
                        }
                    };
                    builder.instructions.push(grant_op);
                }
                return Ok(());
            }
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
            if low == "next_spell_cant_be_countered"
                || matches!(low.as_str(), "can't be countered" | "cant_be_countered")
            {
                let flt = match subject.as_ref() {
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        Some(lower_object_filter(f, vocabulary)?)
                    }
                    _ => None,
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantBeCountered {
                        filter: flt,
                        controller_only: true,
                        next_only: low == "next_spell_cant_be_countered",
                    }),
                    duration: dur,
                });
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "cant_play_lands" | "can't play lands" | "cantplaylands"
            ) {
                let players = match subject.as_ref() {
                    SubjectRefAst::Player(p) => {
                        lower_affected_players_in_context(*p, builder, in_trigger)?
                    }
                    SubjectRefAst::TargetPlayer {
                        opponent_only,
                        count: TargetCountAst::Exact(1),
                    } => {
                        builder.bind_target(if *opponent_only {
                            BodyTarget::SingleOpponent
                        } else {
                            BodyTarget::SinglePlayer
                        })?;
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::CantPlayLands { players }),
                    duration: dur,
                });
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "your life total can't change"
                    | "life total can't change"
                    | "life_total_cant_change"
            ) && matches!(
                subject.as_ref(),
                SubjectRefAst::Player(PlayerRefAst::You) | SubjectRefAst::SelfObject
            ) {
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerLifeTotalCantChange,
                        },
                    ))),
                    duration: dur,
                });
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "players can't search libraries"
                    | "your opponents can't search libraries"
                    | "opponents can't search libraries"
                    | "can't search libraries"
                    | "cant_search_libraries"
            ) {
                let rel = if low.contains("opponent") {
                    Some(PlayerRelation::Opponent)
                } else if low.starts_with("players ") {
                    Some(PlayerRelation::AnyParticipant)
                } else {
                    match subject.as_ref() {
                        SubjectRefAst::Player(PlayerRefAst::You) => {
                            Some(PlayerRelation::Controller)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent,
                        ) => Some(PlayerRelation::Opponent),
                        SubjectRefAst::Player(
                            PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer,
                        ) => Some(PlayerRelation::AnyParticipant),
                        _ => None,
                    }
                };
                if let Some(rel) = rel {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::PlayersCantSearchLibraries(rel),
                            },
                        ))),
                        duration: dur,
                    });
                    return Ok(());
                }
            }
            if let Some((rule_body, eff_dur)) = strip_trailing_temp_duration(&low, dur) {
                if let Some(nonmana_only) = parse_player_cant_activate_rule(&rule_body)
                    && matches!(
                        subject.as_ref(),
                        SubjectRefAst::Player(_)
                            | SubjectRefAst::TargetPlayer {
                                count: TargetCountAst::Exact(1),
                                ..
                            }
                    )
                {
                    let players = match subject.as_ref() {
                        SubjectRefAst::Player(p) => {
                            lower_affected_players_in_context(*p, builder, in_trigger)?
                        }
                        SubjectRefAst::TargetPlayer {
                            opponent_only,
                            count: TargetCountAst::Exact(1),
                        } => {
                            builder.bind_target(if *opponent_only {
                                BodyTarget::SingleOpponent
                            } else {
                                BodyTarget::SinglePlayer
                            })?;
                            AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                        }
                        _ => return Err("oracle:unsupported-combat-mod"),
                    };
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::CantActivateAbilities {
                            players,
                            nonmana_only,
                        }),
                        duration: eff_dur,
                    });
                    return Ok(());
                }
                if let Some(lock_parts) = parse_temp_permanent_lock_parts(&rule_body) {
                    if eff_dur.requires_ability_source() && !(in_trigger || self_pronoun_valid) {
                        return Err("oracle:unsupported-combat-mod");
                    }
                    let use_skip_next_untap = lock_parts.len() == 1
                        && matches!(
                            lock_parts[0],
                            ResolvedPartSpec::Abilities(ref ab)
                                if ab.doesnt_untap && ab.suppressed_activations.is_none()
                        )
                        && matches!(
                            eff_dur,
                            ResolvedDuration::NoStatedDuration | ResolvedDuration::UntilEndOfTurn
                        );
                    if !use_skip_next_untap {
                        return emit_resolved_parts_for_subject(
                            subject,
                            ResolvedPartsSpec(lock_parts),
                            eff_dur,
                            builder,
                            vocabulary,
                            in_trigger,
                            self_pronoun_valid,
                        );
                    }
                }
                if let Some((flt_raw, nonmana_only)) = rule_body
                    .strip_prefix("activated abilities of ")
                    .and_then(|r| {
                        r.strip_suffix(" can't be activated unless they're mana abilities")
                            .map(|f| (f, true))
                            .or_else(|| r.strip_suffix(" can't be activated").map(|f| (f, false)))
                    })
                    && let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                        flt_raw.trim(),
                        &cached_compiler_vocab(vocabulary),
                    )
                    && let Ok(sel) =
                        lower_permanent_selector(&SubjectRefAst::Group(Box::new(ast_f)), vocabulary)
                {
                    let rule = if nonmana_only {
                        StaticRestriction::PreventMatchingNonManaActivations(Box::new(sel))
                    } else {
                        StaticRestriction::PreventMatchingActivations(Box::new(sel))
                    };
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule,
                            },
                        ))),
                        duration: eff_dur,
                    });
                    return Ok(());
                }
            }
            if matches!(
                low.as_str(),
                "all creature types"
                    | "gains all creature types"
                    | "gain all creature types"
                    | "has all creature types"
                    | "have all creature types"
                    | "is every creature type"
                    | "are every creature type"
            ) {
                let mut all_subtypes = BTreeSet::new();
                for ord in 0..vocabulary.len() {
                    if let Ok(ordinal) = u16::try_from(ord)
                        && let Some(ct) = vocabulary.resolve_member(vocabulary.id(), ordinal)
                    {
                        all_subtypes.insert(ct);
                    }
                }
                let type_part = ResolvedPartSpec::Type(TypeChangeSpec {
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                        all_subtypes,
                    )),
                    land_subtypes: SubtypeOperation::Keep,
                });
                return emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![type_part]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            if let Some(type_part) = parse_type_removal_part(&low, vocabulary) {
                return emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![type_part]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            if low.contains(
                "reselect which player or permanent target attacking creature is attacking",
            ) || low == "reselect_attack_defender"
            {
                builder.bind_target(BodyTarget::FilteredPermanent(
                    ObjectFilter::creature().attacking(),
                ))?;
                builder
                    .instructions
                    .push(Instruction::ReselectAttackDefender);
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
            if low == "activate_loyalty_twice"
                || low
                    == "you may activate loyalty abilities of planeswalkers you control twice each turn rather than only once"
            {
                let is_controlled_pws = match subject.as_ref() {
                    SubjectRefAst::Player(PlayerRefAst::You) => true,
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                        f.zone.is_none()
                            && f.any_types == AstCardTypeMask::PLANESWALKER
                            && f.controller == Some(ControllerRelation::You)
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    types: true,
                                    controller: true,
                                    ..FilterAllow::NONE
                                },
                            )
                    }
                    _ => false,
                };
                if is_controlled_pws {
                    if dur == ResolvedDuration::UntilEndOfTurn {
                        builder
                            .instructions
                            .push(Instruction::AdditionalLoyaltyActivation);
                    } else {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::Modifier {
                                    id: ids.static_id(),
                                    rule: StaticModifierRule::TwiceLoyaltyPerTurn,
                                },
                            ))),
                            duration: dur,
                        });
                    }
                    return Ok(());
                }
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
                    SubjectRefAst::TargetPlayer {
                        opponent_only,
                        count: TargetCountAst::Exact(1),
                    } => {
                        builder.bind_target(if *opponent_only {
                            BodyTarget::SingleOpponent
                        } else {
                            BodyTarget::SinglePlayer
                        })?;
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
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
            if matches!(
                dur,
                ResolvedDuration::NoStatedDuration | ResolvedDuration::UntilEndOfTurn
            ) && (low.contains("doesn't untap during")
                || low.contains("don't untap during")
                || low == "doesnt_untap"
                || low == "doesntuntap")
            {
                match eff_subject {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::SkipNextUntap {
                            subject: Box::new(EffectSubject::AbilitySource),
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n > 0 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n > 0 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 1,
                                maximum: 1,
                                action: PermanentTargetAction::SkipNextUntap,
                            });
                        return Ok(());
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
                                action: PermanentTargetAction::SkipNextUntap,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
            } else if low == "must_attack"
                || low == "attacks_if_able"
                || low == "attacks this turn if able"
                || low == "attack this turn if able"
                || low == "attacks this combat if able"
                || low == "attack this combat if able"
                || low == "attacks each combat if able"
                || low == "attack each combat if able"
                || low == "attacks each turn if able"
                || low == "attack each turn if able"
                || low == "attacks if able"
                || low == "attack if able"
                || low == "must attack"
                || low == "must attack this turn"
                || low == "must attack this turn if able"
                || low == "must attack this combat"
                || low == "must attack this combat if able"
                || low == "must attack each combat if able"
                || low == "must attack if able"
                || low.strip_prefix("must_attack:").is_some_and(|t| {
                    matches!(
                        t.trim(),
                        "" | "this turn"
                            | "this combat"
                            | "each combat"
                            | "each turn"
                            | "if able"
                            | "this turn if able"
                            | "this combat if able"
                            | "each combat if able"
                    )
                })
                || low.strip_prefix("attacks_if_able:").is_some_and(|t| {
                    matches!(
                        t.trim(),
                        "" | "this turn"
                            | "this combat"
                            | "each combat"
                            | "each turn"
                            | "if able"
                            | "this turn if able"
                            | "this combat if able"
                            | "each combat if able"
                    )
                })
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
            } else if low == "becomes_blocked"
                || low == "becomes blocked"
                || low == "become blocked"
            {
                Some(CombatActionKind::BecomeBlocked)
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
                match eff_subject {
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TargetObject),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TargetObject),
                            kind: action_kind,
                        });
                        return Ok(());
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
                                action: PermanentTargetAction::RequireCombatAction(action_kind),
                            });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *max > 0 =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::TargetObject),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::CreatedTokens),
                            kind: action_kind,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::RequireCombatAction {
                            subject: Box::new(EffectSubject::LastMoved),
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
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.last_group_filter = Some(obj);
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
                    builder.last_group_filter = Some(ObjectFilter::creature().opposing());
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
            for (infix, implied_dur) in [
                (" this turn if able", ResolvedDuration::UntilEndOfTurn),
                (" this combat if able", ResolvedDuration::UntilEndOfCombat),
            ] {
                if let Some(prefix) = rule_for_br.strip_suffix(infix) {
                    rule_for_br = format!("{} if able", prefix.trim());
                    if br_dur == ResolvedDuration::NoStatedDuration
                        || (br_dur == ResolvedDuration::UntilEndOfTurn
                            && implied_dur != ResolvedDuration::UntilEndOfTurn)
                    {
                        br_dur = implied_dur;
                    }
                    break;
                }
            }
            if let Some(brs) = lower_combat_mod_blocking_restrictions(&rule_for_br, vocabulary)
                .filter(|v| v.len() >= 2)
                && brs.iter().all(|br| {
                    !matches!(
                        br,
                        BlockingRestriction::CannotBlockSource
                            | BlockingRestriction::MustBlockSource
                            | BlockingRestriction::CannotBeBlockedBySource
                    ) || self_pronoun_valid
                })
            {
                let parts = ResolvedPartsSpec(
                    brs.into_iter()
                        .map(ResolvedPartSpec::BlockingRestriction)
                        .collect(),
                );
                return emit_resolved_parts_for_subject(
                    eff_subject,
                    parts,
                    br_dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .map_err(|_| "oracle:unsupported-combat-mod");
            }
            if let Some(br) = lower_combat_mod_blocking_restriction(&rule_for_br, vocabulary)
                .or_else(|| lower_rule_blocking_restriction(&rule_for_br, None, vocabulary))
                .or_else(|| lower_extra_combat_mod_blocking_restriction(&rule_for_br))
                && (!matches!(
                    br,
                    BlockingRestriction::CannotBlockSource
                        | BlockingRestriction::MustBlockSource
                        | BlockingRestriction::CannotBeBlockedBySource
                ) || self_pronoun_valid)
            {
                match eff_subject {
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: br_dur,
                            });
                        return Ok(());
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
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *max > 0 =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
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
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::CreatedTokens),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::LastMoved),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: br_dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
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
            if let Some(src_raw) = rule_for_br
                .strip_prefix("cantbetargetof:")
                .or_else(|| rule_for_br.strip_prefix("can't be the target of "))
                .or_else(|| rule_for_br.strip_prefix("can't be the targets of "))
            {
                if let Some((mask, spells_only)) = parse_cant_be_target_colors(src_raw) {
                    let part = if spells_only {
                        ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(mask)
                    } else {
                        ResolvedPartSpec::CannotBeTargetedByColors(mask)
                    };
                    return emit_resolved_parts_for_subject(
                        eff_subject,
                        ResolvedPartsSpec(vec![part]),
                        br_dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                    .map_err(|_| "oracle:unsupported-combat-mod");
                }
                if let Some(kws) = parse_cant_be_target_keywords(src_raw, vocabulary) {
                    return emit_resolved_parts_for_subject(
                        subject,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                            AbilityChange::grant_keywords(kws),
                        )]),
                        br_dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                    .map_err(|_| "oracle:unsupported-combat-mod");
                }
            }
            if let Some(attacker_raw) = rule_for_br
                .strip_prefix("must_block:")
                .or_else(|| rule_for_br.strip_prefix("must block "))
                .or_else(|| {
                    rule_for_br
                        .strip_prefix("blocks ")
                        .and_then(|r| r.strip_suffix(" if able"))
                })
                .or_else(|| {
                    rule_for_br
                        .strip_prefix("block ")
                        .and_then(|r| r.strip_suffix(" if able"))
                })
            {
                let mut trial = builder.clone();
                if let Some(attacker_ref) = parse_must_block_subject(
                    attacker_raw,
                    &mut trial,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                ) && emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::MustBlockSubject(attacker_ref)]),
                    br_dur,
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
            Err("oracle:unsupported-combat-mod")
        }
        InstructionAst::RedirectDamage {
            amount,
            from,
            to,
            source_choice,
            duration,
        } => {
            let has_source = in_trigger || self_pronoun_valid;
            let dur = match duration {
                Some(d) => lower_duration(d)?,
                None => ResolvedDuration::UntilEndOfTurn,
            };
            let mut trial = builder.clone();
            let (source, source_is_target) = match source_choice.as_deref() {
                None => (None, false),
                Some(sc) => {
                    let sc_low = sc.trim().to_ascii_lowercase();
                    let (src, chosen_color, chosen_type, is_tgt) =
                        parse_next_time_source_with_target(
                            &sc_low, &mut trial, has_source, in_trigger, vocabulary,
                        )
                        .ok_or("oracle:unsupported-redirect-damage")??;
                    if chosen_color || chosen_type {
                        return Err("oracle:unsupported-redirect-damage");
                    }
                    (src, is_tgt)
                }
            };
            let from_rec = lower_redirect_from_subject_with_target(
                from, &mut trial, has_source, in_trigger, vocabulary,
            )?;
            let to_dst = lower_redirect_to_subject(to, &mut trial, has_source, vocabulary)?;
            let target_slots = usize::from(source_is_target)
                + usize::from(matches!(
                    from_rec,
                    StaticDamageRecipient::TargetObject | StaticDamageRecipient::TargetAny
                ))
                + usize::from(matches!(
                    to_dst,
                    DamageRedirectTarget::TargetObject | DamageRedirectTarget::TargetAny
                ));
            if target_slots > 1 {
                return Err("oracle:unsupported-redirect-damage");
            }
            let resolved_amt = resolve_x(amount, bound_x);
            let rule = if matches!(resolved_amt, ValueExprAst::PriorResult) {
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from: from_rec,
                    to: to_dst,
                    combat_only: false,
                    amount: None,
                    one_shot: false,
                }
            } else {
                let expr =
                    lower_value_expr_in_context(resolved_amt, &trial, in_trigger, vocabulary)?;
                match expr {
                    ValueExpr::Literal(n) if n.is_positive() => {
                        TemporaryRuleKind::RedirectNextDamage {
                            source,
                            source_is_target,
                            from: from_rec,
                            to: to_dst,
                            combat_only: false,
                            amount: Some(n),
                            one_shot: false,
                        }
                    }
                    dyn_expr => TemporaryRuleKind::RedirectNextDamageExpr {
                        source,
                        source_is_target,
                        from: from_rec,
                        to: to_dst,
                        combat_only: false,
                        amount: Box::new(dyn_expr),
                        one_shot: false,
                    },
                }
            };
            trial.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(rule),
                duration: dur,
            });
            *builder = trial;
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
        InstructionAst::DealDamageDivided {
            source,
            amount,
            targets_desc,
            divider: None,
        } => {
            let amt = lower_value_expr_in_context(
                resolve_x(amount, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
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
        InstructionAst::Detain { subject } => {
            let lock_parts =
                parse_temp_permanent_lock_parts("detain").ok_or("oracle:unsupported-detain")?;
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(lock_parts),
                ResolvedDuration::UntilControllerNextTurn,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|_| "oracle:unsupported-detain")
        }
        InstructionAst::SetGoaded {
            subject,
            remove: false,
        } => lower_instruction(
            &InstructionAst::CombatRuleMod {
                subject: subject.clone(),
                rule: "goad".to_string(),
                duration: DurationAst::UntilYourNextTurn,
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::ForcedBlockByFilter {
            blockers,
            target,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let target_ref = lower_move_subject_ref(
                target,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            emit_resolved_parts_for_subject(
                blockers,
                ResolvedPartsSpec(vec![ResolvedPartSpec::MustBlockSubject(target_ref)]),
                dur,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|_| "oracle:unsupported-forced-block")
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

pub(super) fn parse_next_time_damage_source(
    src_raw: &str,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(Option<ObjectFilter>, bool, bool)> {
    let mut s = src_raw.trim().to_ascii_lowercase();
    if matches!(s.as_str(), "~" | "this creature" | "this permanent") {
        if !has_source {
            return None;
        }
        return Some((
            Some(ObjectFilter::Clause(ObjectClause {
                source_relation: SourceRelation::SelfObject,
                ..ObjectClause::any()
            })),
            false,
            false,
        ));
    }
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
    let mut source_chosen_type = false;
    for pat in [
        "of the chosen creature type",
        "of the last chosen creature type",
        "that's the chosen creature type",
        "of a creature type of your choice",
        "of the chosen type",
        "of the last chosen type",
        "that's the chosen type",
    ] {
        if s.contains(pat) {
            source_chosen_type = true;
            s = s.replace(pat, " ");
        }
    }
    if (source_chosen_color || source_chosen_type) && !has_source {
        return None;
    }
    for pat in ["of your choice", "you choose"] {
        s = s.replace(pat, " ");
    }
    let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut required_keyword: Option<KeywordAbility> = None;
    let body = if let Some((before_with, kw_part)) = normalized.rsplit_once(" with ")
        && let Some(kw) = lower_keyword_ability_name(kw_part.trim())
    {
        required_keyword = Some(kw);
        before_with.trim().to_owned()
    } else {
        normalized
    };
    let tokens: Vec<&str> = body
        .split_whitespace()
        .filter(|t| !matches!(*t, "a" | "an" | "any" | "the" | "source" | "sources" | "or"))
        .collect();
    if tokens.is_empty() {
        return match required_keyword {
            None => Some((None, source_chosen_color, source_chosen_type)),
            Some(kw) => Some((
                Some(ObjectFilter::Clause(ObjectClause {
                    keyword: Some(kw),
                    ..ObjectClause::any()
                })),
                source_chosen_color,
                source_chosen_type,
            )),
        };
    }
    let mut any_colors = ColorMask::default();
    let mut forbidden_colors = ColorMask::default();
    let mut any_types = CardTypeMask::default();
    let mut subtype: Option<CreatureTypeId> = None;
    let mut saw_spell = false;
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
            "spell" | "spells" => saw_spell = true,
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
    if saw_spell
        && !(any_types == CardTypeMask::INSTANT
            || any_types == CardTypeMask::SORCERY
            || any_types == CardTypeMask::INSTANT.union(CardTypeMask::SORCERY))
    {
        token_ok = false;
    }
    if !token_ok {
        let candidate = body.trim();
        if !candidate.is_empty()
            && let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                candidate,
                &cached_compiler_vocab(vocabulary),
            )
        {
            ast_f.zone = None;
            if let Ok(mut obj) = lower_object_filter(&ast_f, vocabulary)
                && !obj.has_target_player()
            {
                if let Some(kw) = required_keyword {
                    match &mut obj {
                        ObjectFilter::Clause(c) if c.keyword.is_none() => c.keyword = Some(kw),
                        _ => return None,
                    }
                }
                return Some((Some(obj), source_chosen_color, source_chosen_type));
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
    if let Some(kw) = required_keyword {
        clause.keyword = Some(kw);
    }
    Some((
        Some(ObjectFilter::Clause(clause)),
        source_chosen_color,
        source_chosen_type,
    ))
}

pub(super) fn lower_fight_or_exchange_subject(
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
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
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
            if *count == TargetCountAst::UpTo(1) {
                builder.up_to_one_target = true;
            }
            Ok(MoveSubjectRef::TargetObject)
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } => {
            let bt = lower_target_one_of_permanent_body_target(
                options, builder, in_trigger, vocabulary,
            )?;
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
            if *count == TargetCountAst::UpTo(1) {
                builder.up_to_one_target = true;
            }
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

pub(super) fn extract_resolving_player_from_subject(
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

pub(super) fn parse_exchange_zone_from_subject(subj: &SubjectRefAst) -> Option<ExchangeZone> {
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

pub(super) type ParsedNextTimeSource = (Option<ObjectFilter>, bool, bool, bool);

pub(super) fn parse_next_time_source_with_target(
    src_raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<ParsedNextTimeSource, &'static str>> {
    let trimmed = src_raw.trim().to_ascii_lowercase();
    if let Some(filter_raw) = trimmed.strip_prefix("target ") {
        let filter_clean = filter_raw.trim().trim_end_matches('.');
        if matches!(filter_clean, "source" | "permanent") {
            return Some(
                builder
                    .bind_target(BodyTarget::SinglePermanent)
                    .map(|()| (None, false, false, true)),
            );
        }
        let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
            filter_clean,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        ast_f.zone = None;
        let bt = lower_battlefield_body_target_in_context(&ast_f, builder, in_trigger, vocabulary)
            .ok()?;
        if !is_permanent_body_target(bt) {
            return None;
        }
        return Some(builder.bind_target(bt).map(|()| (None, false, false, true)));
    }
    if matches!(
        trimmed.as_str(),
        "that creature" | "that permanent" | "that source" | "it"
    ) && is_permanent_body_target(builder.target)
    {
        return Some(Ok((None, false, false, true)));
    }
    parse_next_time_damage_source(&trimmed, has_source, vocabulary)
        .map(|(src, chosen_color, chosen_type)| Ok((src, chosen_color, chosen_type, false)))
}

pub(super) fn parse_damage_recipient_phrase_with_target(
    raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<StaticDamageRecipient, &'static str>> {
    let cleaned = raw
        .trim()
        .trim_end_matches('.')
        .strip_prefix("to ")
        .unwrap_or_else(|| raw.trim().trim_end_matches('.'))
        .trim();
    match cleaned {
        "any target" | "target creature, planeswalker, or player" | "target creature or player" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SingleAnyTarget)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target player or planeswalker" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SinglePlayerOrPlaneswalker)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target opponent or planeswalker" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SingleOpponentOrPlaneswalker)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target player" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SinglePlayer)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target opponent" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SingleOpponent)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "that creature" | "that permanent" | "it" if is_permanent_body_target(builder.target) => {
            return Some(Ok(StaticDamageRecipient::TargetObject));
        }
        "that player"
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer
                    | BodyTarget::SingleOpponent
                    | BodyTarget::SingleAnyTarget
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker
            ) =>
        {
            return Some(Ok(StaticDamageRecipient::TargetAny));
        }
        other if other.starts_with("target ") => {
            let filter_raw = other.strip_prefix("target ")?.trim();
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                filter_raw,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let bt =
                lower_battlefield_body_target_in_context(&ast_f, builder, in_trigger, vocabulary)
                    .ok()?;
            if !is_permanent_body_target(bt) {
                return None;
            }
            return Some(
                builder
                    .bind_target(bt)
                    .map(|()| StaticDamageRecipient::TargetObject),
            );
        }
        _ => {}
    }
    parse_damage_recipient_phrase(cleaned, has_source, vocabulary).map(Ok)
}

pub(super) fn parse_damage_recipient_phrase(
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
        | "one or more players" => Some(StaticDamageRecipient::Any),
        "an opponent" => Some(StaticDamageRecipient::Opponents),
        "an opponent or a permanent an opponent controls"
        | "an opponent or a permanent they control" => {
            Some(StaticDamageRecipient::OpponentOrOpponentPermanent)
        }
        "you" => Some(StaticDamageRecipient::Controller),
        "you or a permanent you control"
        | "you and/or permanents you control"
        | "you and permanents you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledPermanent)
        }
        "you or a creature you control"
        | "you or creatures you control"
        | "you and/or creatures you control"
        | "you and creatures you control" => {
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

pub(super) fn parse_damage_redirect_target(
    raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
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
        "any target" | "target creature, planeswalker, or player" | "target creature or player" => {
            Some(
                builder
                    .bind_target(BodyTarget::SingleAnyTarget)
                    .map(|()| DamageRedirectTarget::TargetAny),
            )
        }
        "target player or planeswalker" => Some(
            builder
                .bind_target(BodyTarget::SinglePlayerOrPlaneswalker)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "target opponent or planeswalker" => Some(
            builder
                .bind_target(BodyTarget::SingleOpponentOrPlaneswalker)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "target player" => Some(
            builder
                .bind_target(BodyTarget::SinglePlayer)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "target opponent" => Some(
            builder
                .bind_target(BodyTarget::SingleOpponent)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
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
            let bt =
                lower_battlefield_body_target_in_context(&ast_f, builder, in_trigger, vocabulary)
                    .ok()?;
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

pub(super) fn lower_redirect_from_subject_with_target(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticDamageRecipient, &'static str> {
    match subj {
        SubjectRefAst::AnyTarget => {
            builder.bind_target(BodyTarget::SingleAnyTarget)?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::TargetPlayerOrPlaneswalker { opponent_only } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SingleOpponentOrPlaneswalker
            } else {
                BodyTarget::SinglePlayerOrPlaneswalker
            })?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SingleOpponent
            } else {
                BodyTarget::SinglePlayer
            })?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt =
                lower_battlefield_body_target_in_context(filter, builder, in_trigger, vocabulary)?;
            if !is_permanent_body_target(bt) {
                return Err("oracle:unsupported-redirect-damage");
            }
            builder.bind_target(bt)?;
            Ok(StaticDamageRecipient::TargetObject)
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            Ok(StaticDamageRecipient::TargetObject)
        }
        other => lower_redirect_from_subject(other, has_source, vocabulary)
            .ok_or("oracle:unsupported-redirect-damage"),
    }
}

pub(super) fn lower_prevent_recipient_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticDamageRecipient, &'static str> {
    lower_redirect_from_subject_with_target(subj, builder, has_source, in_trigger, vocabulary)
        .map_err(|_| "oracle:unsupported-prevent-damage")
}

pub(super) fn lower_redirect_from_subject(
    subj: &SubjectRefAst,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    match subj {
        SubjectRefAst::Player(PlayerRefAst::You) => Some(StaticDamageRecipient::Controller),
        SubjectRefAst::Player(PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent) => {
            Some(StaticDamageRecipient::Opponents)
        }
        SubjectRefAst::Player(PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer) => {
            Some(StaticDamageRecipient::Players)
        }
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

pub(super) fn lower_redirect_to_subject(
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
        SubjectRefAst::AnyTarget => {
            builder.bind_target(BodyTarget::SingleAnyTarget)?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::TargetPlayerOrPlaneswalker { opponent_only } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SingleOpponentOrPlaneswalker
            } else {
                BodyTarget::SinglePlayerOrPlaneswalker
            })?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SingleOpponent
            } else {
                BodyTarget::SinglePlayer
            })?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(DamageRedirectTarget::TargetAny)
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

pub(super) fn extract_redirect_destination_phrase(rep_norm: &str) -> Option<&str> {
    let cleaned = rep_norm
        .strip_prefix("instead, ")
        .or_else(|| rep_norm.strip_prefix("instead "))
        .unwrap_or(rep_norm)
        .trim();
    for prefix in [
        "that creature deals that combat damage to ",
        "that source deals that combat damage to ",
        "that permanent deals that combat damage to ",
        "~ deals that combat damage to ",
        "it deals that combat damage to ",
        "that creature deals that damage to ",
        "that source deals that damage to ",
        "that spell deals that damage to ",
        "that permanent deals that damage to ",
        "~ deals that damage to ",
        "it deals that damage to ",
        "that combat damage is dealt to ",
        "all combat damage is dealt to ",
        "all damage is dealt to ",
        "that damage is dealt to ",
        "damage is dealt to ",
        "it's dealt to ",
        "it is dealt to ",
        "you may have that combat damage dealt to ",
        "you may have that combat damage be dealt to ",
        "you may have that damage dealt to ",
        "you may have that damage be dealt to ",
        "you may have it dealt to ",
        "deal that combat damage to ",
        "deal that damage to ",
        "is dealt to ",
        "are dealt to ",
        "be dealt to ",
        "dealt to ",
    ] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }
    None
}

pub(super) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn split_dealt_from_and_source(phrase: &str) -> Option<(&str, Option<&str>)> {
    let p = phrase.trim();
    if let Some(rest) = p
        .strip_prefix("be dealt by ")
        .or_else(|| p.strip_prefix("dealt by "))
        .or_else(|| p.strip_prefix("be dealt from "))
        .or_else(|| p.strip_prefix("dealt from "))
        && let Some((s_part, f_part)) = rest.split_once(" to ")
    {
        return Some((f_part.trim(), Some(s_part.trim())));
    }
    let after_dealt = p
        .strip_prefix("be dealt to ")
        .or_else(|| p.strip_prefix("dealt to "))?;
    if let Some((f_part, s_part)) = after_dealt
        .split_once(" by ")
        .or_else(|| after_dealt.split_once(" from "))
    {
        Some((f_part.trim(), Some(s_part.trim())))
    } else {
        Some((after_dealt.trim(), None))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_lower_redirect_next_damage(
    subj_low: &str,
    ev_low: &str,
    rep_low: &str,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    has_source: bool,
    bound_x: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<(), &'static str>> {
    let rep_no_dot = rep_low.trim_end_matches('.').trim();
    let rep_no_instead = rep_no_dot
        .strip_suffix(" instead")
        .unwrap_or(rep_no_dot)
        .trim();
    let rep_norm_owned = collapse_ws(&rep_no_instead.replace(" this turn", ""));
    let rep_norm = rep_norm_owned.trim();
    let to_raw = extract_redirect_destination_phrase(rep_norm)?;

    let check_single_target_slot = |source_is_target: bool,
                                    from: StaticDamageRecipient,
                                    to: DamageRedirectTarget,
                                    allow_same_source_and_to: bool|
     -> bool {
        let from_uses = matches!(
            from,
            StaticDamageRecipient::TargetObject | StaticDamageRecipient::TargetAny
        );
        let to_uses = matches!(
            to,
            DamageRedirectTarget::TargetObject | DamageRedirectTarget::TargetAny
        );
        if allow_same_source_and_to
            && source_is_target
            && !from_uses
            && to == DamageRedirectTarget::TargetObject
        {
            return true;
        }
        usize::from(source_is_target) + usize::from(from_uses) + usize::from(to_uses) <= 1
    };

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
        let mut trial = builder.clone();
        if to_raw == "itself" {
            let (source, chosen_color, chosen_type, source_is_target) =
                match parse_next_time_source_with_target(
                    src_raw, &mut trial, has_source, in_trigger, vocabulary,
                )? {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            if chosen_color || chosen_type || !source_is_target {
                return None;
            }
            let from = if after_dmg.is_empty() {
                StaticDamageRecipient::Any
            } else {
                match parse_damage_recipient_phrase_with_target(
                    after_dmg, &mut trial, has_source, in_trigger, vocabulary,
                )? {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                }
            };
            if !check_single_target_slot(true, from, DamageRedirectTarget::TargetObject, true) {
                return None;
            }
            trial.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target: true,
                    from,
                    to: DamageRedirectTarget::TargetObject,
                    combat_only,
                    amount: None,
                    one_shot: true,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            *builder = trial;
            return Some(Ok(()));
        }
        let (source, chosen_color, chosen_type, source_is_target) =
            match parse_next_time_source_with_target(
                src_raw, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
        if chosen_color || chosen_type {
            return None;
        }
        let from = if after_dmg.is_empty() {
            StaticDamageRecipient::Any
        } else {
            match parse_damage_recipient_phrase_with_target(
                after_dmg, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            }
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        trial.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                source_is_target,
                from,
                to,
                combat_only,
                amount: None,
                one_shot: true,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    let subj_no_dur = collapse_ws(
        &subj_low
            .trim()
            .replace(" this turn", "")
            .replace(" this combat", ""),
    );
    let subj_clean = subj_no_dur
        .trim()
        .strip_suffix(" that")
        .or_else(|| subj_no_dur.trim().strip_suffix(" which"))
        .unwrap_or(subj_no_dur.trim())
        .trim();
    let ev_no_would = ev_low
        .trim()
        .trim_end_matches('.')
        .strip_prefix("would ")
        .unwrap_or(ev_low.trim().trim_end_matches('.'))
        .trim();
    let ev_norm_owned = collapse_ws(
        &ev_no_would
            .replace(" this turn", "")
            .replace(" this combat", ""),
    );
    let ev_norm = ev_norm_owned.trim();

    if let Some((after_next, fallback_ev)) = subj_clean
        .strip_prefix("the next ")
        .map(|rest| (rest, ev_norm))
        .or_else(|| ev_norm.strip_prefix("the next ").map(|rest| (rest, "")))
        && let Some((amt_tok, kind_rest)) = after_next.split_once(' ')
        && kind_rest.contains("damage")
    {
        let is_x = amt_tok == "x";
        let lit_n = if is_x {
            None
        } else {
            parse_small_number_word(amt_tok).filter(|&n| n > 0)
        };
        if !is_x && lit_n.is_none() {
            return None;
        }
        let combat_only = kind_rest.contains("combat damage");
        let kind_norm = collapse_ws(&kind_rest.replace(" this turn", ""));
        let fb_norm = collapse_ws(&fallback_ev.replace(" this turn", ""));
        let mut trial = builder.clone();
        let (from_part, source, source_is_target) =
            if let Some((before_would, dst_part)) = kind_norm.split_once(" would deal to ") {
                let src_raw = before_would
                    .split_once(" that ")
                    .map_or("", |(_, s)| s.trim());
                let (src, chosen_color, chosen_type, is_tgt) = if src_raw.is_empty() {
                    (None, false, false, false)
                } else {
                    match parse_next_time_source_with_target(
                        src_raw, &mut trial, has_source, in_trigger, vocabulary,
                    )? {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    }
                };
                if chosen_color || chosen_type {
                    return None;
                }
                (dst_part.trim().to_owned(), src, is_tgt)
            } else if let Some((before_would, dst_part)) = fb_norm.split_once(" would deal to ") {
                let src_raw = before_would
                    .split_once(" that ")
                    .map_or(before_would.trim(), |(_, s)| s.trim());
                let (src, chosen_color, chosen_type, is_tgt) = if src_raw.is_empty() {
                    (None, false, false, false)
                } else {
                    match parse_next_time_source_with_target(
                        src_raw, &mut trial, has_source, in_trigger, vocabulary,
                    )? {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    }
                };
                if chosen_color || chosen_type {
                    return None;
                }
                (dst_part.trim().to_owned(), src, is_tgt)
            } else if let Some((f_part, s_part_opt)) = split_dealt_from_and_source(&fb_norm)
                .or_else(|| {
                    kind_norm
                        .find("be dealt ")
                        .or_else(|| kind_norm.find("dealt "))
                        .and_then(|idx| split_dealt_from_and_source(&kind_norm[idx..]))
                })
            {
                if let Some(s_part) = s_part_opt {
                    let (src, chosen_color, chosen_type, is_tgt) =
                        match parse_next_time_source_with_target(
                            s_part, &mut trial, has_source, in_trigger, vocabulary,
                        )? {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        };
                    if chosen_color || chosen_type {
                        return None;
                    }
                    (f_part.to_owned(), src, is_tgt)
                } else {
                    (f_part.to_owned(), None, false)
                }
            } else {
                return None;
            };
        let from = match parse_damage_recipient_phrase_with_target(
            &from_part, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        let rule = if let Some(n) = lit_n {
            TemporaryRuleKind::RedirectNextDamage {
                source,
                source_is_target,
                from,
                to,
                combat_only,
                amount: Some(i64::from(n).into()),
                one_shot: false,
            }
        } else {
            let x_expr = match bound_x {
                Some(expr) => {
                    match lower_value_expr_in_context(expr, &trial, in_trigger, vocabulary) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    }
                }
                None => ValueExpr::chosen_x(),
            };
            TemporaryRuleKind::RedirectNextDamageExpr {
                source,
                source_is_target,
                from,
                to,
                combat_only,
                amount: Box::new(x_expr),
                one_shot: false,
            }
        };
        trial.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(rule),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    if matches!(
        subj_clean,
        "damage"
            | "all damage"
            | "any damage"
            | "the damage"
            | "combat damage"
            | "all combat damage"
            | "any combat damage"
            | "the combat damage"
    ) && let Some((from_part, s_part_opt)) = split_dealt_from_and_source(ev_norm)
    {
        let combat_only = subj_clean.contains("combat");
        let mut trial = builder.clone();
        let (source, source_is_target) = if let Some(s_part) = s_part_opt {
            let (src, chosen_color, chosen_type, is_tgt) = match parse_next_time_source_with_target(
                s_part, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if chosen_color || chosen_type {
                return None;
            }
            (src, is_tgt)
        } else {
            (None, false)
        };
        let from = match parse_damage_recipient_phrase_with_target(
            from_part, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        trial.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                source_is_target,
                from,
                to,
                combat_only,
                amount: None,
                one_shot: false,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    if let Some((combat_only, from_raw)) = ev_norm
        .strip_prefix("deal combat damage to ")
        .map(|r| (true, r))
        .or_else(|| ev_norm.strip_prefix("deal damage to ").map(|r| (false, r)))
    {
        let mut trial = builder.clone();
        let (source, chosen_color, chosen_type, source_is_target) =
            match parse_next_time_source_with_target(
                subj_clean, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
        if chosen_color || chosen_type {
            return None;
        }
        let from = match parse_damage_recipient_phrase_with_target(
            from_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        trial.instructions.push(Instruction::CreateTemporaryRule {
            rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                source,
                source_is_target,
                from,
                to,
                combat_only,
                amount: None,
                one_shot: false,
            }),
            duration: ResolvedDuration::UntilEndOfTurn,
        });
        *builder = trial;
        return Some(Ok(()));
    }

    None
}

pub(super) fn lower_target_one_of_damage_body_target(
    options: &[SubjectRefAst],
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    let mut has_any_player = false;
    let mut has_opponent = false;
    let mut has_creature = false;
    let mut has_planeswalker = false;
    let mut perm_clauses: Vec<ObjectClause> = Vec::new();
    let mut has_non_perm_special = false;
    for opt in options {
        match opt {
            SubjectRefAst::TargetPlayer {
                opponent_only: false,
                ..
            }
            | SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                has_any_player = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::TargetPlayer {
                opponent_only: true,
                ..
            }
            | SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                has_opponent = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::TargetPlayerOrPlaneswalker { opponent_only } => {
                if *opponent_only {
                    has_opponent = true;
                } else {
                    has_any_player = true;
                }
                has_planeswalker = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::AnyTarget => {
                return Ok(BodyTarget::SingleAnyTarget);
            }
            SubjectRefAst::TargetObject { filter, .. }
                if filter.zone.is_none() && !filter.is_ability && !filter.any_spell =>
            {
                let obj = lower_object_filter_in_context(filter, builder, in_trigger, vocabulary)?;
                if obj == ObjectFilter::creature() {
                    has_creature = true;
                } else if obj == ObjectFilter::planeswalker() {
                    has_planeswalker = true;
                }
                match obj {
                    ObjectFilter::Clause(c) => perm_clauses.push(c),
                    ObjectFilter::Either(a, b) => {
                        perm_clauses.push(a);
                        perm_clauses.push(b);
                    }
                }
            }
            _ => return Err("oracle:unsupported-damage-target"),
        }
    }
    if has_non_perm_special {
        let non_special_perm_count = perm_clauses
            .iter()
            .filter(|c| {
                ObjectFilter::Clause(**c) != ObjectFilter::creature()
                    && ObjectFilter::Clause(**c) != ObjectFilter::planeswalker()
            })
            .count();
        if non_special_perm_count > 0 {
            return Err("oracle:unsupported-damage-target");
        }
        return match (has_any_player, has_opponent, has_creature, has_planeswalker) {
            (true, false, true, true) => Ok(BodyTarget::SingleAnyTarget),
            (true, false, true, false) => Ok(BodyTarget::SingleCreatureOrPlayer),
            (true, false, false, true) => Ok(BodyTarget::SinglePlayerOrPlaneswalker),
            (false, true, false, true) => Ok(BodyTarget::SingleOpponentOrPlaneswalker),
            (true, false, false, false) => Ok(BodyTarget::SinglePlayer),
            (false, true, false, false) => Ok(BodyTarget::SingleOpponent),
            _ => Err("oracle:unsupported-damage-target"),
        };
    }
    match perm_clauses.len() {
        1 => {
            let f = ObjectFilter::Clause(perm_clauses[0]);
            if f == ObjectFilter::creature() {
                Ok(BodyTarget::SingleCreature)
            } else if f == ObjectFilter::planeswalker() {
                Ok(BodyTarget::SinglePlaneswalker)
            } else {
                Ok(BodyTarget::FilteredPermanent(f))
            }
        }
        2 => Ok(BodyTarget::FilteredPermanent(ObjectFilter::Either(
            perm_clauses[0],
            perm_clauses[1],
        ))),
        _ => Err("oracle:unsupported-damage-target"),
    }
}

pub(super) fn lower_extra_combat_mod_blocking_restriction(
    rule_body: &str,
) -> Option<BlockingRestriction> {
    let low = rule_body.trim().trim_end_matches('.').trim();
    match low {
        "cant_be_regenerated"
        | "cantberegenerated"
        | "can't be regenerated"
        | "cannot be regenerated" => Some(BlockingRestriction::CannotBeRegenerated),
        "cant_attack_alone" | "cantattackalone" | "can't attack alone" | "cannot attack alone" => {
            Some(BlockingRestriction::CannotAttackAlone)
        }
        "cant_block_alone" | "cantblockalone" | "can't block alone" | "cannot block alone" => {
            Some(BlockingRestriction::CannotBlockAlone)
        }
        "assigns_damage_by_toughness"
        | "assigns combat damage equal to its toughness rather than its power"
        | "assign combat damage equal to their toughness rather than their power" => {
            Some(BlockingRestriction::AssignDamageByToughness)
        }
        "assigns_no_combat_damage"
        | "assignsnocombatdamage"
        | "assigns no combat damage"
        | "assign no combat damage" => Some(BlockingRestriction::AssignNoCombatDamage),
        "must_block_all_attackers"
        | "must block all attackers"
        | "blocks each attacking creature if able"
        | "blocks all creatures able to block" => Some(BlockingRestriction::MustBlockAllAttackers),
        _ => {
            if let Some(n_raw) = low
                .strip_prefix("cant_be_blocked_by_more_than:")
                .or_else(|| low.strip_prefix("can't be blocked by more than "))
                .or_else(|| low.strip_prefix("cannot be blocked by more than "))
            {
                let num_tok = n_raw.split_whitespace().next().unwrap_or("");
                let n = parse_small_number_word(num_tok).and_then(|v| u8::try_from(v).ok())?;
                if n > 0 {
                    return Some(BlockingRestriction::CannotBeBlockedByMoreThan(n));
                }
            }
            None
        }
    }
}

pub(super) fn parse_must_block_subject(
    raw: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Option<MoveSubjectRef> {
    let mut clean = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    for suffix in [
        " this combat if able",
        " this turn if able",
        " if able",
        " this combat",
        " this turn",
    ] {
        if let Some(s) = clean.strip_suffix(suffix) {
            clean = s.trim().to_owned();
        }
    }
    match clean.as_str() {
        "~" | "this creature" | "this permanent" if self_pronoun_valid || in_trigger => {
            Some(MoveSubjectRef::AbilitySource)
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent"
            if self_pronoun_valid || in_trigger =>
        {
            Some(MoveSubjectRef::AttachedHost)
        }
        "it" | "them" | "that creature" | "those creatures" | "that permanent" | "that token"
        | "those tokens" => {
            if last_instruction_created_tokens(&builder.instructions) {
                Some(MoveSubjectRef::CreatedTokens)
            } else if is_permanent_body_target(builder.target) {
                Some(MoveSubjectRef::TargetObject)
            } else if last_instruction_moved_object(&builder.instructions) {
                Some(MoveSubjectRef::LastMoved)
            } else if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(&clean)) {
                Some(MoveSubjectRef::TriggeringObject)
            } else if self_pronoun_valid {
                Some(MoveSubjectRef::AbilitySource)
            } else {
                None
            }
        }
        _ => {
            let parsed = spellwake_oracle_compiler::filter::parse_subject_or_target(
                &clean,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            lower_move_subject_ref(&parsed, builder, vocabulary, in_trigger, self_pronoun_valid)
                .ok()
        }
    }
}

pub(super) fn rewrite_loop_player_filter(
    f: &ObjectFilterAst,
    loop_player: PlayerRefAst,
) -> ObjectFilterAst {
    let mut out = f.clone();
    if loop_player == PlayerRefAst::You {
        if out.controller == Some(ControllerRelation::You) {
            out.controller = Some(ControllerRelation::ThatPlayer);
        }
        if out.owner == Some(ControllerRelation::You) {
            out.owner = Some(ControllerRelation::ThatPlayer);
        }
        out.zone = match out.zone {
            Some(ZoneFilter::YourGraveyard) => Some(ZoneFilter::ThatPlayerGraveyard),
            Some(ZoneFilter::YourHand) => Some(ZoneFilter::ThatPlayerHand),
            Some(ZoneFilter::YourLibrary) => Some(ZoneFilter::ThatPlayerLibrary),
            other => other,
        };
    } else if matches!(
        loop_player,
        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent
    ) && matches!(
        out.controller,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
    ) {
        out.controller = Some(ControllerRelation::ThatPlayer);
    }
    for sb in &mut out.stat_bounds {
        *sb.value = rewrite_loop_player_value_expr(&sb.value, loop_player);
    }
    out
}

pub(super) fn rewrite_loop_player_subject(
    s: &SubjectRefAst,
    loop_player: PlayerRefAst,
) -> SubjectRefAst {
    match s {
        SubjectRefAst::Player(p) if *p == loop_player => {
            SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
        }
        SubjectRefAst::All(f) => {
            SubjectRefAst::All(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        SubjectRefAst::Each(f) => {
            SubjectRefAst::Each(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        SubjectRefAst::Group(f) => {
            SubjectRefAst::Group(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        SubjectRefAst::TargetObject { filter, count } => SubjectRefAst::TargetObject {
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            count: *count,
        },
        SubjectRefAst::InZone { subject, zone } => {
            let mapped_zone = if loop_player == PlayerRefAst::You {
                match zone {
                    ZoneFilter::YourGraveyard => ZoneFilter::ThatPlayerGraveyard,
                    ZoneFilter::YourHand => ZoneFilter::ThatPlayerHand,
                    ZoneFilter::YourLibrary => ZoneFilter::ThatPlayerLibrary,
                    other => *other,
                }
            } else {
                *zone
            };
            SubjectRefAst::InZone {
                subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
                zone: mapped_zone,
            }
        }
        SubjectRefAst::Compound(items) => SubjectRefAst::Compound(
            items
                .iter()
                .map(|it| rewrite_loop_player_subject(it, loop_player))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod wave12_source_prevention_tests {
    use super::*;
    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test-vocab".into(),
            [0; 32],
            vec![
                "Cleric".into(),
                "Human".into(),
                "Knight".into(),
                "Phantom".into(),
                "Soldier".into(),
                "Spirit".into(),
                "Wizard".into(),
            ],
        )
        .unwrap()
    }
    #[test]
    fn pinned_source_prevention_cards_and_spell_source_residual() {
        let vocabulary = super::super::super::engine_trajectories::vocabulary();
        for (name, type_line, cost, power, toughness, text) in [
            (
                "Deftblade Elite",
                "Creature \u{2014} Human Soldier",
                "{W}",
                Some("1"),
                Some("1"),
                "Provoke (Whenever this creature attacks, you may have target creature defending player controls untap and block it if able.)\n{1}{W}: Prevent all combat damage that would be dealt to and dealt by this creature this turn.",
            ),
            (
                "Ethereal Haze",
                "Instant \u{2014} Arcane",
                "{W}",
                None,
                None,
                "Prevent all damage that would be dealt by creatures this turn.",
            ),
            (
                "Falling Timber",
                "Instant",
                "{2}{G}",
                None,
                None,
                "Kicker\u{2014}Sacrifice a land. (You may sacrifice a land in addition to any other costs as you cast this spell.)\nPrevent all combat damage target creature would deal this turn. If this spell was kicked, prevent all combat damage another target creature would deal this turn.",
            ),
            (
                "Fend Off",
                "Instant",
                "{1}{W}",
                None,
                None,
                "Prevent all combat damage that would be dealt by target creature this turn.\nCycling {2} ({2}, Discard this card: Draw a card.)",
            ),
            (
                "Gossamer Chains",
                "Enchantment",
                "{W}{W}",
                None,
                None,
                "Return this enchantment to its owner's hand: Prevent all combat damage that would be dealt by target unblocked creature this turn.",
            ),
            (
                "Horn of Deafening",
                "Artifact",
                "{4}",
                None,
                None,
                "{2}, {T}: Prevent all combat damage that would be dealt by target creature this turn.",
            ),
            (
                "Ignoble Soldier",
                "Creature \u{2014} Human Soldier",
                "{2}{W}",
                Some("3"),
                Some("1"),
                "Whenever this creature becomes blocked, prevent all combat damage that would be dealt by it this turn.",
            ),
            (
                "Kor Haven",
                "Legendary Land",
                "",
                None,
                None,
                "{T}: Add {C}.\n{1}{W}, {T}: Prevent all combat damage that would be dealt by target attacking creature this turn.",
            ),
            (
                "Lady Evangela",
                "Legendary Creature \u{2014} Human Cleric",
                "{W}{U}{B}",
                Some("1"),
                Some("2"),
                "{W}{B}, {T}: Prevent all combat damage that would be dealt by target creature this turn.",
            ),
            (
                "Maze of Ith",
                "Land",
                "",
                None,
                None,
                "{T}: Untap target attacking creature. Prevent all combat damage that would be dealt to and dealt by that creature this turn.",
            ),
            (
                "Resistance Fighter",
                "Creature \u{2014} Human Soldier",
                "{W}",
                Some("1"),
                Some("1"),
                "Sacrifice this creature: Prevent all combat damage target creature would deal this turn.",
            ),
            (
                "Restrain",
                "Instant",
                "{2}{W}",
                None,
                None,
                "Prevent all combat damage that would be dealt by target attacking creature this turn.\nDraw a card.",
            ),
            (
                "Safeguard",
                "Enchantment",
                "{3}{W}{W}",
                None,
                None,
                "{2}{W}: Prevent all combat damage that would be dealt by target creature this turn.",
            ),
            (
                "Shieldmage Elder",
                "Creature \u{2014} Human Cleric Wizard",
                "{5}{W}",
                Some("2"),
                Some("3"),
                "Tap two untapped Clerics you control: Prevent all damage target creature would deal this turn.\nTap two untapped Wizards you control: Prevent all damage target spell would deal this turn.",
            ),
            (
                "Urborg Phantom",
                "Creature \u{2014} Spirit Minion",
                "{2}{B}",
                Some("3"),
                Some("1"),
                "This creature can't block.\n{U}: Prevent all combat damage that would be dealt to and dealt by this creature this turn.",
            ),
            (
                "Warning",
                "Instant",
                "{W}",
                None,
                None,
                "Prevent all combat damage that would be dealt by target attacking creature this turn.",
            ),
        ] {
            let card = InventoryCard {
                scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
                oracle_id: None,
                name: name.into(),
                layout: "normal".into(),
                faces: vec![spellwake_content::CatalogFace {
                    name: name.into(),
                    type_line: Some(type_line.into()),
                    oracle_text: Some(text.into()),
                    mana_cost: Some(cost.into()),
                    power: power.map(str::to_owned),
                    toughness: toughness.map(str::to_owned),
                }],
                legalities: BTreeMap::new(),
                eligible_formats: vec![],
                required_by_base_formats: true,
                program_evidence: "test",
            };
            // Shieldmage Elder is now qualified: CR 400.7c spell-to-permanent and spell-copy
            // identity propagation is implemented in DamageState::propagate_permanent_spell_identity.
            compile_with_reason(&card, &vocabulary)
                .unwrap_or_else(|reason| panic!("{name}: {reason}"));
        }
    }
    #[test]
    fn source_prevention_rejects_duration_and_anaphora_ambiguity_and_lowers_qualified_specs() {
        let vocab = vocabulary();
        let filter = spellwake_oracle_compiler::filter::parse_object_filter(
            "creatures",
            &cached_compiler_vocab(&vocab),
        )
        .unwrap();
        let target = SubjectRefAst::TargetObject {
            filter: Box::new(filter.clone()),
            count: TargetCountAst::Exact(1),
        };
        // Variant 1 (ongoing duration) and variant 3 (raw untyped anaphora instead of
        // ChosenByPlayer filter predicate) must remain fail-closed.
        for variant in [1usize, 3] {
            let source = match variant {
                1 => SubjectRefAst::Anaphora(
                    "that creature for as long as this remains on the battlefield".into(),
                ),
                3 => SubjectRefAst::Anaphora("a source of your choice".into()),
                _ => unreachable!(),
            };
            let ins = InstructionAst::PreventAllDamageFrom {
                combat_only: true,
                source: Box::new(source),
                recipient: None,
                duration: DurationAst::UntilEndOfTurn,
            };
            assert!(
                lower_combat_damage_instruction(
                    &ins,
                    &mut BodyBuilder::new(),
                    &vocab,
                    &mut LoweringIds::default(),
                    false,
                    false,
                    None
                )
                .is_err(),
                "variant {variant} must stay fail-closed"
            );
        }
        // Variant 0 (explicit Controller recipient) and variant 2 (targeted spell source)
        // now lower to exact qualified `Instruction::PreventDamageSpec`.
        for variant in [0usize, 2] {
            let source = if variant == 2 {
                let mut f = filter.clone();
                f.any_spell = true;
                SubjectRefAst::TargetObject {
                    filter: Box::new(f),
                    count: TargetCountAst::Exact(1),
                }
            } else {
                target.clone()
            };
            let ins = InstructionAst::PreventAllDamageFrom {
                combat_only: true,
                source: Box::new(source),
                recipient: if variant == 0 {
                    Some(Box::new(SubjectRefAst::Player(PlayerRefAst::You)))
                } else {
                    None
                },
                duration: DurationAst::UntilEndOfTurn,
            };
            let mut builder = BodyBuilder::new();
            lower_combat_damage_instruction(
                &ins,
                &mut builder,
                &vocab,
                &mut LoweringIds::default(),
                false,
                false,
                None,
            )
            .unwrap_or_else(|e| panic!("variant {variant} should lower: {e}"));
            assert!(
                matches!(
                    builder.instructions.as_slice(),
                    [Instruction::PreventDamageSpec(_)]
                ),
                "variant {variant} should lower to PreventDamageSpec"
            );
        }
    }
}
