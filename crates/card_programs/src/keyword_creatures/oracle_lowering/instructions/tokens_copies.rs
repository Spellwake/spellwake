use super::*;

pub(super) fn lower_tokens_copies_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::CreateToken { controller, token }
        | InstructionAst::CreateTokenWithAbilities {
            controller, token, ..
        } => {
            let compiled_abilities = match ins {
                InstructionAst::CreateTokenWithAbilities {
                    compiled_abilities, ..
                } => Some(compiled_abilities.as_ref()),
                _ => None,
            };
            if compiled_abilities.is_some() && !matches!(token, TokenSpecAst::Creature { .. }) {
                return Err("oracle:unsupported-token-abilities");
            }
            if let TokenSpecAst::Copy {
                count,
                source,
                tapped,
                attacking,
                exception,
            } = token
            {
                let mut trial = builder.clone();
                let cnt = lower_value_expr_in_context(
                    resolve_x(count, bound_x),
                    &trial,
                    in_trigger,
                    vocabulary,
                )?;
                let eff_source =
                    unwrap_in_zone_subject(source).unwrap_or_else(|| (**source).clone());
                let src_ref = match &eff_source {
                    s if is_self_pronoun(s, &trial, self_pronoun_valid) => {
                        Some(MoveSubjectRef::AbilitySource)
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        || (*count == TargetCountAst::Exact(1)
                            && matches!(
                                filter.zone,
                                Some(
                                    ZoneFilter::YourGraveyard
                                        | ZoneFilter::OpponentGraveyard
                                        | ZoneFilter::AnyGraveyard
                                )
                            )) =>
                    {
                        let bt = if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?
                        } else {
                            lower_target_subject(&eff_source, vocabulary)?
                        };
                        trial.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&trial.instructions) =>
                    {
                        Some(MoveSubjectRef::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_permanent_body_target(trial.target)
                            || is_graveyard_body_target(trial.target) =>
                    {
                        Some(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&trial.instructions) =>
                    {
                        Some(MoveSubjectRef::LastMoved)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if (self_pronoun_valid || in_trigger)
                            && raw.trim().eq_ignore_ascii_case("them")
                            && trial.target == BodyTarget::None
                            && !last_instruction_created_tokens(&trial.instructions)
                            && !last_instruction_moved_object(&trial.instructions) =>
                    {
                        Some(MoveSubjectRef::AbilitySource)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some(MoveSubjectRef::TriggeringObject)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(MoveSubjectRef::TriggeringObject)
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        Some(MoveSubjectRef::AttachedHost)
                    }
                    SubjectRefAst::Group(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                            && matches!(f.zone, None | Some(ZoneFilter::Exile))
                            && (in_trigger || self_pronoun_valid) =>
                    {
                        let mut inner = f.as_ref().clone();
                        inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        inner.zone = None;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        if obj.has_target_player() {
                            None
                        } else {
                            trial.instructions.push(Instruction::ChooseObjects {
                                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                source_zone: SourceZoneFilter::LinkedExile,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                distinct_powers: false,
                                max_total_power: None,
                                unchosen_action: None,
                            });
                            Some(MoveSubjectRef::LastMoved)
                        }
                    }
                    SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                            && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0
                            && !f.any_spell
                            && !f.is_ability =>
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
                            lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                        };
                        if obj.has_target_player() {
                            None
                        } else {
                            trial.instructions.push(Instruction::ChooseObjects {
                                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                source_zone,
                                filter: Box::new(obj),
                                min: ValueExpr::literal(1),
                                max: ValueExpr::literal(1),
                                distinct_powers: false,
                                max_total_power: None,
                                unchosen_action: None,
                            });
                            Some(MoveSubjectRef::LastMoved)
                        }
                    }
                    _ => None,
                };
                let recipients_opt = match controller {
                    PlayerRefAst::You => Some(TokenRecipients::Controller),
                    PlayerRefAst::EachOpponent => Some(TokenRecipients::EachOpponent),
                    PlayerRefAst::EachPlayer => Some(TokenRecipients::EachPlayer),
                    PlayerRefAst::TargetPlayer if trial.target == BodyTarget::None => {
                        match trial.bind_target(BodyTarget::SinglePlayer) {
                            Ok(()) => Some(TokenRecipients::Player(ResolvingPlayer::TargetPlayer)),
                            Err(_) => None,
                        }
                    }
                    PlayerRefAst::TargetOpponent if trial.target == BodyTarget::None => {
                        match trial.bind_target(BodyTarget::SingleOpponent) {
                            Ok(()) => Some(TokenRecipients::Player(ResolvingPlayer::TargetPlayer)),
                            Err(_) => None,
                        }
                    }
                    other => lower_resolving_player_in_context(*other, &mut trial, in_trigger)
                        .ok()
                        .map(TokenRecipients::Player),
                };
                if let (Some(src_ref), Some(recipients)) = (src_ref, recipients_opt)
                    && let Some(parsed) =
                        parse_token_copy_exceptions(exception.as_deref(), vocabulary)
                {
                    let enters_tapped = *tapped || parsed.extra_tapped;
                    let enters_attacking = *attacking || parsed.extra_attacking;
                    let needs_legacy_copy = parsed.strip_mana_cost
                        || parsed.saw_colorless
                        || (!parsed.colors_are_additive && !parsed.parsed_colors.is_empty());
                    if needs_legacy_copy {
                        let legacy_pt = match parsed.override_pt {
                            None => Some(None),
                            Some((p, t)) => {
                                match (
                                    p.to_usize().and_then(|v| u8::try_from(v).ok()),
                                    t.to_usize().and_then(|v| u8::try_from(v).ok()),
                                ) {
                                    (Some(pu), Some(tu)) => Some(Some((pu, tu))),
                                    _ => None,
                                }
                            }
                        };
                        if let Some(override_pt) = legacy_pt {
                            let mut kws = parsed.grant_keywords;
                            if parsed.haste {
                                kws.insert(KeywordAbility::Haste);
                            }
                            let override_colors = if !parsed.parsed_colors.is_empty() {
                                Some(parsed.parsed_colors)
                            } else if parsed.saw_colorless {
                                Some(BTreeSet::new())
                            } else {
                                None
                            };
                            trial.instructions.push(Instruction::CreateTokenCopy {
                                source: src_ref,
                                count: cnt,
                                tapped: enters_tapped,
                                attacking: enters_attacking,
                                sacrifice_at_end_step: parsed.sacrifice_at_end_step,
                                exile_at_end_step: parsed.exile_at_end_step,
                                grant_keywords: kws,
                                nonlegendary: parsed.nonlegendary,
                                override_pt,
                                override_colors,
                                add_subtypes: parsed.add_subtypes,
                                add_card_types: parsed.add_card_types,
                                strip_mana_cost: parsed.strip_mana_cost,
                                recipients,
                            });
                            *builder = trial;
                            return Ok(());
                        }
                    } else {
                        let has_ext_fields = parsed.haste
                            || parsed.nonlegendary
                            || parsed.sacrifice_at_end_step
                            || parsed.exile_at_end_step
                            || !parsed.add_card_types.is_empty()
                            || !parsed.add_subtypes.is_empty()
                            || !parsed.grant_keywords.is_empty()
                            || parsed.override_pt.is_some()
                            || !parsed.parsed_colors.is_empty();
                        if has_ext_fields {
                            let mut extra_keywords = parsed.grant_keywords;
                            let haste =
                                parsed.haste || extra_keywords.remove(&KeywordAbility::Haste);
                            trial.instructions.push(Instruction::CreateTokenCopyExt {
                                source: src_ref,
                                count: cnt,
                                recipients,
                                enters_tapped,
                                enters_attacking,
                                haste,
                                sacrifice_at_end_step: parsed.sacrifice_at_end_step,
                                exile_at_end_step: parsed.exile_at_end_step,
                                nonlegendary: parsed.nonlegendary,
                                extra_types: parsed.add_card_types,
                                extra_subtypes: parsed.add_subtypes,
                                extra_keywords,
                                override_pt: parsed.override_pt,
                                extra_colors: parsed.parsed_colors,
                                defender_scope:
                                    spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                            });
                        } else {
                            trial.instructions.push(Instruction::CreateTokenCopy {
                                source: src_ref,
                                count: cnt,
                                tapped: enters_tapped,
                                attacking: enters_attacking,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                grant_keywords: BTreeSet::new(),
                                nonlegendary: false,
                                override_pt: None,
                                override_colors: None,
                                add_subtypes: BTreeSet::new(),
                                add_card_types: BTreeSet::new(),
                                strip_mana_cost: false,
                                recipients,
                            });
                        }
                        *builder = trial;
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
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
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
                            let after_group_removal = !in_trigger
                                && builder.instructions.iter().any(|op| {
                                    matches!(
                                        op,
                                        Instruction::ZoneOp(_)
                                            | Instruction::ForEachPlayerQuery { .. }
                                            | Instruction::Sacrifice { .. }
                                    )
                                });
                            if after_group_removal {
                                TokenRecipients::DestroyedPermanentControllers
                            } else if in_trigger {
                                TokenRecipients::Player(ResolvingPlayer::UniqueRelatedPlayer)
                            } else if (in_trigger || self_pronoun_valid || builder.has_source)
                                && !builder
                                    .instructions
                                    .iter()
                                    .any(has_prior_non_attached_object_op)
                            {
                                TokenRecipients::Player(ResolvingPlayer::AttachedHostController)
                            } else {
                                return Err("oracle:unbound-that-player");
                            }
                        }
                        BodyTarget::SingleSpell(_)
                        | BodyTarget::SingleStackObject(_)
                        | BodyTarget::SingleStackEntry
                        | BodyTarget::ControlledAbility { .. }
                        | BodyTarget::StackEntryTargetingControlledPermanent => {
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
                    BodyTarget::None
                        if builder.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::ZoneOp(_)
                                    | Instruction::ForEachPlayerQuery { .. }
                                    | Instruction::Sacrifice { .. }
                            )
                        }) =>
                    {
                        TokenRecipients::DestroyedPermanentControllers
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
            let precompiled_quoted = if compiled_abilities.is_none() {
                try_precompile_token_quoted_abilities(token, vocabulary)
            } else {
                None
            };
            let (eff_token, eff_compiled_abilities) = match &precompiled_quoted {
                Some((clean_tok, compiled_face)) => (clean_tok, Some(compiled_face)),
                None => (token, compiled_abilities),
            };
            let (mut creation, attacking) =
                if let TokenSpecAst::Predefined {
                    name,
                    count,
                    tapped,
                } = token
                    && let Some(def) = try_lower_extra_predefined_token_def(name, vocabulary, ids)?
                {
                    let amt = lower_value_expr_in_context(
                        resolve_x(count, bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )?;
                    (
                        TokenCreation {
                            enters_tapped: *tapped,
                            definition: def,
                            amount: amt,
                            base_size: None,
                            recipients,
                        },
                        false,
                    )
                } else if let TokenSpecAst::Creature {
                    count,
                    power,
                    toughness,
                    colors,
                    supertypes,
                    card_types,
                    subtypes,
                    keywords,
                    tapped,
                    attacking,
                    named,
                } = eff_token
                    && let Some(face) = eff_compiled_abilities
                    && (power.trim() == "*" || power.trim().is_empty())
                    && (toughness.trim() == "*" || toughness.trim().is_empty())
                    && face
                        .statics
                        .iter()
                        .any(|s| matches!(s, StaticAbilityAst::CdaPowerToughness { .. }))
                {
                    let normalized = TokenSpecAst::Creature {
                        count: count.clone(),
                        power: "0".to_owned(),
                        toughness: "0".to_owned(),
                        colors: *colors,
                        supertypes: *supertypes,
                        card_types: *card_types,
                        subtypes: subtypes.clone(),
                        keywords: keywords.clone(),
                        tapped: *tapped,
                        attacking: *attacking,
                        named: named.clone(),
                    };
                    let (creation, is_attacking) = lower_token_spec_with_abilities(
                        &normalized,
                        recipients,
                        vocabulary,
                        ids,
                        bound_x,
                        Some((builder, in_trigger)),
                        eff_compiled_abilities,
                    )?;
                    if !creation.definition.program().continuous.iter().any(|ab| {
                        matches!(ab, StaticContinuousAbility::CharacteristicDefining { .. })
                    }) {
                        return Err("oracle:token-pt");
                    }
                    (creation, is_attacking)
                } else if let TokenSpecAst::Creature {
                    count,
                    power,
                    toughness,
                    colors,
                    supertypes,
                    card_types,
                    subtypes,
                    keywords,
                    tapped,
                    attacking,
                    named,
                } = eff_token
                    && ((power.trim().eq_ignore_ascii_case("x")
                        && toughness.trim().parse::<i64>().is_ok())
                        || (toughness.trim().eq_ignore_ascii_case("x")
                            && power.trim().parse::<i64>().is_ok()))
                {
                    let x_expr = if let Some(bx) = bound_x {
                        lower_value_expr_in_context(bx, builder, in_trigger, vocabulary)?
                    } else {
                        ValueExpr::chosen_x()
                    };
                    let p_expr = if power.trim().eq_ignore_ascii_case("x") {
                        x_expr.clone()
                    } else {
                        ValueExpr::literal(
                            power.trim().parse::<i64>().map_err(|_| "oracle:token-pt")?,
                        )
                    };
                    let t_expr = if toughness.trim().eq_ignore_ascii_case("x") {
                        x_expr
                    } else {
                        ValueExpr::literal(
                            toughness
                                .trim()
                                .parse::<i64>()
                                .map_err(|_| "oracle:token-pt")?,
                        )
                    };
                    let normalized = TokenSpecAst::Creature {
                        count: count.clone(),
                        power: if power.trim().eq_ignore_ascii_case("x") {
                            "0".to_owned()
                        } else {
                            power.clone()
                        },
                        toughness: if toughness.trim().eq_ignore_ascii_case("x") {
                            "0".to_owned()
                        } else {
                            toughness.clone()
                        },
                        colors: *colors,
                        supertypes: *supertypes,
                        card_types: *card_types,
                        subtypes: subtypes.clone(),
                        keywords: keywords.clone(),
                        tapped: *tapped,
                        attacking: *attacking,
                        named: named.clone(),
                    };
                    let (mut creation, is_attacking) = lower_token_spec_with_abilities(
                        &normalized,
                        recipients,
                        vocabulary,
                        ids,
                        bound_x,
                        Some((builder, in_trigger)),
                        eff_compiled_abilities,
                    )?;
                    creation.base_size = Some(TokenSize::AtCreationPair {
                        power: Box::new(p_expr),
                        toughness: Box::new(t_expr),
                    });
                    (creation, is_attacking)
                } else if let Some((clean_tok, compiled_face)) = &precompiled_quoted
                    && let Ok(res) = lower_token_spec_with_abilities(
                        clean_tok,
                        recipients,
                        vocabulary,
                        ids,
                        bound_x,
                        Some((builder, in_trigger)),
                        Some(compiled_face),
                    )
                {
                    res
                } else {
                    lower_token_spec_with_abilities(
                        token,
                        recipients,
                        vocabulary,
                        ids,
                        bound_x,
                        Some((builder, in_trigger)),
                        compiled_abilities,
                    )?
                };
            if builder.target == BodyTarget::None
                && let ValueExpr::CounterCount { subject, kind } = &mut creation.amount
                && *subject == spellwake_engine::program::CounterSubjectRef::TargetObject
            {
                let matches_source_counter = match builder.instructions.last() {
                    Some(Instruction::PutNamedCounterOnSource { kind: src_kind, .. }) => {
                        kind.is_none() || kind.as_ref() == Some(src_kind)
                    }
                    Some(Instruction::PutCountersOnSource { .. }) => {
                        kind.is_none() || *kind == Some(CounterKind::PlusOne)
                    }
                    _ => false,
                };
                if matches_source_counter {
                    *subject = spellwake_engine::program::CounterSubjectRef::AbilitySource;
                }
            }
            if attacking {
                if creation.recipients != TokenRecipients::Controller {
                    return Err("oracle:unsupported-attacking-token");
                }
                builder
                    .instructions
                    .push(Instruction::CreateAttackingTokens {
                        creations: vec![creation],
                        defender_scope: spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                    });
            } else {
                builder.instructions.push(Instruction::CreateTokens {
                    creations: vec![creation],
                });
            }
            Ok(())
        }
        InstructionAst::Amass { subtype, count } => {
            let ValueExpr::Literal(n) = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?
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
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder.instructions.push(Instruction::ManifestTop {
                player: ResolvingPlayer::Controller,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Investigate { player, count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let def = predefined_token_def("clue", vocabulary, ids)?;
            match player {
                PlayerRefAst::You => {
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
                PlayerRefAst::EachPlayer => {
                    builder.instructions.push(Instruction::CreateTokens {
                        creations: vec![TokenCreation {
                            enters_tapped: false,
                            definition: def,
                            amount: amt,
                            base_size: None,
                            recipients: TokenRecipients::EachPlayer,
                        }],
                    });
                    Ok(())
                }
                PlayerRefAst::EachOpponent => {
                    builder.instructions.push(Instruction::CreateTokens {
                        creations: vec![TokenCreation {
                            enters_tapped: false,
                            definition: def,
                            amount: amt,
                            base_size: None,
                            recipients: TokenRecipients::EachOpponent,
                        }],
                    });
                    Ok(())
                }
                PlayerRefAst::EachOtherPlayer => {
                    push_for_each_player(
                        AffectedPlayers::EachOtherPlayer,
                        vec![Instruction::CreateTokens {
                            creations: vec![TokenCreation {
                                enters_tapped: false,
                                definition: def,
                                amount: amt,
                                base_size: None,
                                recipients: TokenRecipients::Player(ResolvingPlayer::LoopPlayer),
                            }],
                        }],
                        builder,
                    );
                    Ok(())
                }
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    builder.instructions.push(Instruction::CreateTokens {
                        creations: vec![TokenCreation {
                            enters_tapped: false,
                            definition: def,
                            amount: amt,
                            base_size: None,
                            recipients: TokenRecipients::Player(rp),
                        }],
                    });
                    Ok(())
                }
            }
        }
        InstructionAst::Copy {
            subject,
            exception: None,
            count,
        } => {
            let count_expr = match count {
                Some(c) => lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?,
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
        InstructionAst::MayChooseNewTargetsForCopy => {
            if set_last_copy_may_choose_new_targets(&mut builder.instructions) {
                Ok(())
            } else {
                Err("oracle:unsupported-choose-new-targets-for-copy")
            }
        }
        InstructionAst::Incubate { count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder
                .instructions
                .push(Instruction::Incubate { count: amt });
            Ok(())
        }
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
            if builder.target == BodyTarget::None
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
            let eff_receiver =
                unwrap_in_zone_subject(subject).unwrap_or_else(|| (**subject).clone());
            let eff_donor = unwrap_in_zone_subject(source).unwrap_or_else(|| (**source).clone());
            let receiver = match &eff_receiver {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    EffectSubject::AbilitySource
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
                SubjectRefAst::Anaphora(raw)
                    if last_instruction_created_tokens(&builder.instructions)
                        || (has_prior_token_creation(&builder.instructions)
                            && raw.to_ascii_lowercase().contains("token")) =>
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
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
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
                _ => return Err("oracle:unsupported-become-copy"),
            };
            let donor = match &eff_donor {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    MoveSubjectRef::AbilitySource
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if matches!(
                    filter.zone,
                    None | Some(
                        ZoneFilter::Battlefield
                            | ZoneFilter::YourGraveyard
                            | ZoneFilter::OpponentGraveyard
                            | ZoneFilter::AnyGraveyard
                    )
                ) =>
                {
                    let bt = if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?
                    } else {
                        lower_target_subject(&eff_donor, vocabulary)?
                    };
                    builder.bind_target(bt)?;
                    MoveSubjectRef::TargetObject
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    MoveSubjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    MoveSubjectRef::CreatedTokens
                }
                SubjectRefAst::Anaphora(_)
                    if is_permanent_body_target(builder.target)
                        || is_graveyard_body_target(builder.target) =>
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
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                    MoveSubjectRef::AttachedHost
                }
                SubjectRefAst::Group(f)
                    if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                        && matches!(f.zone, None | Some(ZoneFilter::Exile))
                        && (in_trigger || self_pronoun_valid) =>
                {
                    let mut inner = f.as_ref().clone();
                    inner.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?
                    };
                    if obj.has_target_player() {
                        return Err("oracle:unsupported-become-copy");
                    }
                    builder.instructions.push(Instruction::ChooseObjects {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        source_zone: SourceZoneFilter::LinkedExile,
                        filter: Box::new(obj),
                        min: ValueExpr::literal(1),
                        max: ValueExpr::literal(1),
                        distinct_powers: false,
                        max_total_power: None,
                        unchosen_action: None,
                    });
                    MoveSubjectRef::LastMoved
                }
                SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                        && (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) == 0
                        && !f.any_spell
                        && !f.is_ability =>
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
                    if obj.has_target_player() {
                        return Err("oracle:unsupported-become-copy");
                    }
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
                    MoveSubjectRef::LastMoved
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
        InstructionAst::CreateQualifiedToken(qt) => {
            if qt.blocking || qt.chosen_color || qt.chosen_type || qt.dynamic_pt_text.is_some() {
                return Err("oracle:unsupported-qualified-token");
            }
            let mut trial = builder.clone();
            let defender_scope = match qt.attacking_target.as_deref() {
                None => spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                Some(SubjectRefAst::Player(PlayerRefAst::ThatPlayerOrPlaneswalkerController)) => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayerOrPlaneswalkerController,
                        &mut trial,
                        in_trigger,
                    )?;
                    spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(rp)
                }
                Some(SubjectRefAst::Player(p)) => {
                    let rp = lower_resolving_player_in_context(*p, &mut trial, in_trigger)?;
                    spellwake_engine::token::AttackingDefenderScope::Player(rp)
                }
                Some(SubjectRefAst::Compound(items)) if items.len() == 2 => {
                    match (&items[0], &items[1]) {
                        (
                            SubjectRefAst::Player(p),
                            SubjectRefAst::Group(f)
                            | SubjectRefAst::All(f)
                            | SubjectRefAst::Each(f),
                        ) if f.any_types == AstCardTypeMask::PLANESWALKER
                            && f.zone.is_none()
                            && matches!(
                                f.controller,
                                None | Some(
                                    ControllerRelation::ThatPlayer
                                        | ControllerRelation::DefendingPlayer
                                        | ControllerRelation::Opponent
                                        | ControllerRelation::Opponents
                                )
                            ) =>
                        {
                            let rp = lower_resolving_player_in_context(*p, &mut trial, in_trigger)?;
                            spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(rp)
                        }
                        _ => return Err("oracle:unsupported-qualified-token"),
                    }
                }
                Some(SubjectRefAst::Anaphora(raw)) => {
                    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
                    match low.as_str() {
                        "that player or a planeswalker they control"
                        | "that opponent or a planeswalker they control"
                        | "that player or a planeswalker that player controls" => {
                            let rp = lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayer,
                                &mut trial,
                                in_trigger,
                            )?;
                            spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(rp)
                        }
                        "defending player or a planeswalker they control"
                        | "the defending player or a planeswalker they control" => {
                            spellwake_engine::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(
                                ResolvingPlayer::DefendingPlayer,
                            )
                        }
                        "that player" | "that opponent" => {
                            let rp = lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayer,
                                &mut trial,
                                in_trigger,
                            )?;
                            spellwake_engine::token::AttackingDefenderScope::Player(rp)
                        }
                        "defending player" | "the defending player" => {
                            spellwake_engine::token::AttackingDefenderScope::Player(
                                ResolvingPlayer::DefendingPlayer,
                            )
                        }
                        _ => return Err("oracle:unsupported-qualified-token"),
                    }
                }
                Some(_) => return Err("oracle:unsupported-qualified-token"),
            };
            let apply_defender_scope = |ops: &mut [Instruction]| -> bool {
                if defender_scope == spellwake_engine::token::AttackingDefenderScope::AnyLegal {
                    return true;
                }
                let Some(last) = ops.last_mut() else {
                    return false;
                };
                match last {
                    Instruction::CreateAttackingTokens {
                        defender_scope: ds, ..
                    } => {
                        *ds = defender_scope;
                        true
                    }
                    Instruction::CreateTokenCopyExt {
                        enters_attacking: true,
                        defender_scope: ds,
                        ..
                    } => {
                        *ds = defender_scope;
                        true
                    }
                    Instruction::CreateTokenCopy {
                        source,
                        count,
                        tapped,
                        attacking: true,
                        sacrifice_at_end_step,
                        exile_at_end_step,
                        grant_keywords,
                        nonlegendary,
                        override_pt,
                        override_colors: None,
                        add_subtypes,
                        add_card_types,
                        strip_mana_cost: false,
                        recipients,
                    } => {
                        let mut extra_keywords = std::mem::take(grant_keywords);
                        let haste = extra_keywords.remove(&KeywordAbility::Haste);
                        let ext_pt =
                            override_pt.map(|(p, t)| (i64::from(p).into(), i64::from(t).into()));
                        *last = Instruction::CreateTokenCopyExt {
                            source: *source,
                            count: count.clone(),
                            recipients: *recipients,
                            enters_tapped: *tapped,
                            enters_attacking: true,
                            haste,
                            sacrifice_at_end_step: *sacrifice_at_end_step,
                            exile_at_end_step: *exile_at_end_step,
                            nonlegendary: *nonlegendary,
                            extra_types: std::mem::take(add_card_types),
                            extra_subtypes: std::mem::take(add_subtypes),
                            extra_keywords,
                            override_pt: ext_pt,
                            extra_colors: BTreeSet::new(),
                            defender_scope,
                        };
                        true
                    }
                    _ => false,
                }
            };
            if qt.dynamic_power.is_none() && qt.dynamic_toughness.is_none() {
                let before_len = trial.instructions.len();
                lower_instruction(
                    &InstructionAst::CreateToken {
                        controller: qt.controller,
                        token: qt.token.clone(),
                    },
                    &mut trial,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if trial.instructions.len() <= before_len
                    || !apply_defender_scope(&mut trial.instructions)
                {
                    return Err("oracle:unsupported-qualified-token");
                }
                *builder = trial;
                return Ok(());
            }
            if let TokenSpecAst::Creature {
                count,
                power,
                toughness,
                colors,
                supertypes,
                card_types,
                subtypes,
                keywords,
                tapped,
                attacking,
                named,
            } = &qt.token
            {
                let token_size = match (&qt.dynamic_power, &qt.dynamic_toughness) {
                    (Some(dp), Some(dt)) if dp == dt => {
                        let size_expr = lower_value_expr_in_context(
                            resolve_x(dp, bound_x),
                            &trial,
                            in_trigger,
                            vocabulary,
                        )?;
                        TokenSize::AtCreation(size_expr)
                    }
                    (dp_opt, dt_opt) => {
                        let parse_fixed_pt = |raw: &str| -> Result<ValueExpr, &'static str> {
                            let n = raw
                                .trim()
                                .parse::<i64>()
                                .map_err(|_| "oracle:unsupported-qualified-token")?;
                            Ok(ValueExpr::literal(n))
                        };
                        let p_expr = match dp_opt {
                            Some(dp) => lower_value_expr_in_context(
                                resolve_x(dp, bound_x),
                                &trial,
                                in_trigger,
                                vocabulary,
                            )?,
                            None => parse_fixed_pt(power)?,
                        };
                        let t_expr = match dt_opt {
                            Some(dt) => lower_value_expr_in_context(
                                resolve_x(dt, bound_x),
                                &trial,
                                in_trigger,
                                vocabulary,
                            )?,
                            None => parse_fixed_pt(toughness)?,
                        };
                        TokenSize::AtCreationPair {
                            power: Box::new(p_expr),
                            toughness: Box::new(t_expr),
                        }
                    }
                };
                let norm_pt = |raw: &str| -> String {
                    let t = raw.trim();
                    if t.is_empty() || t == "*" || t.eq_ignore_ascii_case("x") {
                        "0".to_owned()
                    } else {
                        t.to_owned()
                    }
                };
                let normalized_token = TokenSpecAst::Creature {
                    count: count.clone(),
                    power: norm_pt(power),
                    toughness: norm_pt(toughness),
                    colors: *colors,
                    supertypes: *supertypes,
                    card_types: *card_types,
                    subtypes: subtypes.clone(),
                    keywords: keywords.clone(),
                    tapped: *tapped,
                    attacking: *attacking,
                    named: named.clone(),
                };
                let before_len = trial.instructions.len();
                lower_instruction(
                    &InstructionAst::CreateToken {
                        controller: qt.controller,
                        token: normalized_token,
                    },
                    &mut trial,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if trial.instructions.len() == before_len + 1
                    && apply_defender_scope(&mut trial.instructions)
                    && let Some(
                        Instruction::CreateTokens { creations }
                        | Instruction::CreateAttackingTokens { creations, .. },
                    ) = trial.instructions.last_mut()
                    && creations.len() == 1
                {
                    creations[0].base_size = Some(token_size);
                    *builder = trial;
                    return Ok(());
                }
            }
            Err("oracle:unsupported-qualified-token")
        }
        InstructionAst::Earthbend { count } => {
            let amount = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder.bind_target(BodyTarget::FilteredPermanent(
                ObjectFilter::land().controlled(),
            ))?;
            builder.instructions.push(Instruction::Earthbend { amount });
            Ok(())
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

fn try_lower_extra_predefined_token_def(
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<Option<TokenDefinition>, &'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "vibranium" => {
            let mut p = Program::artifact(None, vec![]);
            p.printed_keywords.insert(KeywordAbility::Indestructible);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())]))
                    .restricted(ManaRestriction::NotNonartifactSpell),
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Vibranium".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map(Some)
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "heartwood" => {
            let mut p = Program::artifact(None, vec![]);
            p.effect_defined_colors = Some(BTreeSet::from([Color::Red, Color::Green]));
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
                },
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Heartwood".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map(Some)
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "spellgorger weird" => {
            let weird = vocabulary
                .resolve_label("Weird")
                .ok_or("oracle:predef-token-invalid")?;
            let cost = ManaCost {
                generic: 2.into(),
                required: BTreeMap::from([(ManaType::Red, 1.into())]),
            };
            let mut p = Program::creature(Some(cost), 2.into(), 2.into(), BTreeSet::from([weird]));
            p.effect_defined_colors = Some(BTreeSet::from([Color::Red]));
            p.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::new(TriggerEvent::SpellCast {
                    caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                    filter: SpellFilter::from_object(ObjectFilter::Clause(ObjectClause {
                        forbidden_types: CardTypeMask::CREATURE,
                        ..ObjectClause::any()
                    })),
                    self_cast: false,
                    include_copy: false,
                    min_mana_value: None,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSource { amount: 1.into() }],
                ),
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Spellgorger Weird".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map(Some)
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "gingerbrute" => {
            let golem = vocabulary
                .resolve_label("Golem")
                .ok_or("oracle:predef-token-invalid")?;
            let mut p = Program::creature(
                Some(ManaCost::generic(1.into())),
                1.into(),
                1.into(),
                BTreeSet::from([golem]),
            );
            p.card_types.insert(CardType::Artifact);
            p.artifact_subtypes.insert(ArtifactSubtype::Food);
            p.printed_keywords.insert(KeywordAbility::Haste);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: ManaCost::generic(1.into()),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                            BlockingRestriction::OnlyBlockedByMatching(ObjectFilter::Clause(
                                ObjectClause {
                                    keyword: Some(KeywordAbility::Haste),
                                    ..ObjectClause::creature()
                                },
                            )),
                        )]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    }],
                ),
            });
            p.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: ManaCost::generic(2.into()),
                source_cost: SourceCost::TapSacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    }],
                ),
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Gingerbrute".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map(Some)
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "mutavault" => {
            let mut p = Program::land(
                BTreeSet::new(),
                vec![ManaAbility {
                    id: ids.mana(),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
                }],
            );
            p.effect_defined_colors = Some(BTreeSet::new());
            p.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: ManaCost::generic(1.into()),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![
                            ResolvedPartSpec::Type(TypeChangeSpec {
                                supertypes: TypeOperation::Keep,
                                types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                                subtypes: SubtypeOperation::Keep,
                                land_subtypes: SubtypeOperation::Keep,
                            }),
                            ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
                                BTreeSet::from([KeywordAbility::Changeling]),
                            )),
                            ResolvedPartSpec::SetPT(PowerToughness {
                                power: 2.into(),
                                toughness: 2.into(),
                            }),
                        ]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    }],
                ),
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Mutavault".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map(Some)
            .map_err(|_| "oracle:predef-token-invalid")
        }
        _ => Ok(None),
    }
}

fn set_last_copy_may_choose_new_targets(ops: &mut [Instruction]) -> bool {
    for op in ops.iter_mut().rev() {
        match op {
            Instruction::CopyStackObject {
                may_choose_new_targets,
                ..
            } => {
                *may_choose_new_targets = true;
                return true;
            }
            Instruction::CopySpell => {
                *op = Instruction::CopyStackObject {
                    target: StackCopyTarget::TargetSpell,
                    count: ValueExpr::literal(1),
                    may_choose_new_targets: true,
                };
                return true;
            }
            Instruction::CopyAbility => {
                *op = Instruction::CopyStackObject {
                    target: StackCopyTarget::TargetAbility,
                    count: ValueExpr::literal(1),
                    may_choose_new_targets: true,
                };
                return true;
            }
            Instruction::FlipCoins {
                win_body,
                lose_body,
                ..
            } => {
                if let Some(body) = lose_body
                    && set_last_copy_may_choose_new_targets(&mut body.instructions)
                {
                    return true;
                }
                if let Some(body) = win_body
                    && set_last_copy_may_choose_new_targets(&mut body.instructions)
                {
                    return true;
                }
            }
            Instruction::ScheduleDelayedEventTrigger { body, .. }
            | Instruction::ScheduleDelayedStepTrigger { body, .. }
            | Instruction::EmitReflexiveTrigger { body, .. }
            | Instruction::RepeatLoop { body, .. }
            | Instruction::FixedTargetClause(body) => {
                if set_last_copy_may_choose_new_targets(&mut body.instructions) {
                    return true;
                }
            }
            Instruction::ForEachPlayer { body, .. } => {
                if set_last_copy_may_choose_new_targets(&mut body.instructions) {
                    return true;
                }
            }
            Instruction::ChooseOption { options, .. }
            | Instruction::ChooseOptionForPlayers { options, .. } => {
                for opt in options.iter_mut().rev() {
                    if set_last_copy_may_choose_new_targets(&mut opt.body.instructions) {
                        return true;
                    }
                }
            }
            Instruction::OfferAnyPlayer {
                cost_body,
                paid_body,
                declined_body,
                ..
            } => {
                if let Some(body) = declined_body
                    && set_last_copy_may_choose_new_targets(&mut body.instructions)
                {
                    return true;
                }
                if set_last_copy_may_choose_new_targets(&mut paid_body.instructions) {
                    return true;
                }
                if set_last_copy_may_choose_new_targets(&mut cost_body.instructions) {
                    return true;
                }
            }
            Instruction::RollDie { bands, .. } => {
                for band in bands.iter_mut().rev() {
                    if set_last_copy_may_choose_new_targets(&mut band.body.instructions) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn strip_self_name_clause_for_copy(low: &str, keep_name: bool) -> String {
    if !keep_name {
        return low.to_owned();
    }
    let mut out = low.to_owned();
    for prefix in [
        "except its name is still ",
        "except his name is still ",
        "except her name is still ",
        "except its name is ",
        "except his name is ",
        "except her name is ",
        "its name is still ",
        "his name is still ",
        "her name is still ",
        "its name is ",
        "his name is ",
        "her name is ",
    ] {
        let Some(start) = out.find(prefix) else {
            continue;
        };
        let after = &out[start + prefix.len()..];
        if after.starts_with('~') {
            continue;
        }
        let end_rel = [
            " and it ",
            " and it's ",
            " and he ",
            " and he's ",
            " and she ",
            " and she's ",
            " and they ",
            ", it's ",
            ", it ",
            ", he's ",
            ", he ",
            ", she's ",
            ", she ",
            ", they ",
            ", and ",
            ".",
        ]
        .iter()
        .filter_map(|delim| after.find(delim))
        .min()
        .unwrap_or(after.len());
        let name_seg = after[..end_rel].trim();
        let forbidden = [
            "legendary",
            "copy",
            "ability",
            "abilities",
            "type",
            "types",
            "creature",
            "artifact",
            "enchantment",
            "land",
            "planeswalker",
            "token",
            "counter",
            "color",
            "except",
            "gains",
            "has",
            "becomes",
            "/",
        ];
        if !name_seg.is_empty()
            && name_seg.len() <= 48
            && !forbidden.iter().any(|w| name_seg.contains(w))
        {
            out.replace_range(start..start + prefix.len() + end_rel, " ");
            break;
        }
    }
    out
}

pub(super) fn parse_copy_exceptions(
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
                        && ab.granted_triggers.is_empty()
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
        || low.contains("except his name is")
        || low.contains("except her name is")
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
    let grants_this_ability = low.contains("this ability")
        && (low.contains(" has ")
            || low.contains(" gains ")
            || low.starts_with("has ")
            || low.starts_with("gains "));
    if low.contains("in addition to its other abilities")
        || low.contains("in addition to their other abilities")
        || low.contains("except it has this ability")
        || low.contains("it has this ability")
        || low.contains("has this ability")
        || low.contains("it gains this ability")
        || low.contains("gains this ability")
        || grants_this_ability
        || low.contains("except it keeps its abilities")
        || low.contains("except it keeps its other abilities")
        || low.contains("except it keeps all its abilities")
        || low.contains("keeps its abilities")
        || low.contains("keeps its other abilities")
        || low.contains("keep their abilities")
    {
        out.keep_abilities = true;
    }
    let mut remainder = strip_self_name_clause_for_copy(&low, out.keep_name);
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
        "except the token isn't legendary",
        "except the tokens aren't legendary",
        "except that token isn't legendary",
        "except those tokens aren't legendary",
        "except it isn't legendary",
        "except they aren't legendary",
        "except it's not legendary",
        "except they're not legendary",
        "the token isn't legendary",
        "the tokens aren't legendary",
        "that token isn't legendary",
        "those tokens aren't legendary",
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
        "except he has this ability",
        "except he gains this ability",
        "except she has this ability",
        "except she gains this ability",
        "it has this ability",
        "it gains this ability",
        "he has this ability",
        "he gains this ability",
        "she has this ability",
        "she gains this ability",
        "has this ability",
        "gains this ability",
        "and this ability",
        "this ability",
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
        "except it becomes an",
        "except it becomes a",
        "it becomes an",
        "it becomes a",
        "except he's an",
        "except he's a",
        "except he's",
        "except she's an",
        "except she's a",
        "except she's",
        "except it has",
        "except they have",
        "except he has",
        "except she has",
        "except it's an",
        "except it's a",
        "except it's",
        "except they're",
        "except it gains",
        "except they gain",
        "except he gains",
        "except she gains",
        "except",
        "it has",
        "they have",
        "he has",
        "she has",
        "it gains",
        "they gain",
        "he gains",
        "she gains",
        "it's an",
        "it's a",
        "it's",
        "he's an",
        "he's a",
        "he's",
        "she's an",
        "she's a",
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
        if let Some(kw) = lower_keyword_ability_name(tok) {
            out.extra_keywords.insert(kw);
            continue;
        }
        if let Some(ct) = resolve_protection_subtype(tok, vocabulary) {
            out.extra_creature_subtypes.insert(ct);
            continue;
        }
        return Err("oracle:unsupported-become-copy-exception");
    }
    Ok(out)
}

pub(super) fn static_type_change_to_spec_temp(stc: StaticTypeChange) -> Option<TypeChangeSpec> {
    let subtypes = match stc.subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Set(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Set(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return None,
    };
    let land_subtypes = match stc.land_subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Add(s),
        SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Set(s),
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return None,
    };
    Some(TypeChangeSpec {
        land_subtypes,
        supertypes: stc.supertypes,
        types: stc.types,
        subtypes,
    })
}

pub(super) fn parse_battlefield_dest(dest: &str) -> Option<BattlefieldDestFlags> {
    if !dest.contains("battlefield") {
        return None;
    }
    if dest.contains("face down")
        || dest.contains("face-down")
        || dest.contains("blocking")
        || dest.contains("as a copy")
        || dest.contains("as a ")
        || dest.contains("melded")
        || dest.contains(" with ")
    {
        return None;
    }
    let attach_to_self = if let Some((_, after_attach)) = dest.split_once("attached to ") {
        let target_clean = after_attach.trim().trim_end_matches('.');
        matches!(
            target_clean,
            "~" | "this creature" | "this permanent" | "this artifact" | "this enchantment" | "it"
        )
    } else if dest.contains("attached to") {
        return None;
    } else {
        false
    };
    if dest.contains("attached to") && !attach_to_self {
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
        attach_to_self,
    })
}

pub(super) struct ParsedTokenCopyExceptions {
    nonlegendary: bool,
    sacrifice_at_end_step: bool,
    exile_at_end_step: bool,
    strip_mana_cost: bool,
    haste: bool,
    extra_tapped: bool,
    extra_attacking: bool,
    colors_are_additive: bool,
    saw_colorless: bool,
    override_pt: Option<(RulesInteger, RulesInteger)>,
    parsed_colors: BTreeSet<Color>,
    grant_keywords: BTreeSet<KeywordAbility>,
    add_subtypes: BTreeSet<CreatureTypeId>,
    add_card_types: BTreeSet<CardType>,
}

pub(super) fn parse_token_copy_exceptions(
    exception: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ParsedTokenCopyExceptions> {
    let exc_low = exception.unwrap_or("").trim().to_ascii_lowercase();
    for forbidden in [
        "loses all other card types",
        "lose all other card types",
        "loses all abilities",
        "lose all abilities",
        "isn't a creature",
        "aren't creatures",
        "isn't an artifact",
        "aren't artifacts",
        "isn't an enchantment",
        "aren't enchantments",
        "name is",
        "named ",
        "attached to",
        "enters with",
        "enter with",
        "counter",
    ] {
        if exc_low.contains(forbidden) {
            return None;
        }
    }
    let nonlegendary = exc_low.contains("isn't legendary")
        || exc_low.contains("aren't legendary")
        || exc_low.contains("not legendary");
    let mut sacrifice_at_end_step = exc_low.contains("sacrifice") && exc_low.contains("end step");
    let mut exile_at_end_step = exc_low.contains("exile") && exc_low.contains("end step");
    let strip_mana_cost = exc_low.contains("has no mana cost")
        || exc_low.contains("have no mana cost")
        || exc_low.contains("no mana cost");
    let colors_are_additive = exc_low.contains("in addition to its other colors")
        || exc_low.contains("in addition to their other colors");
    let in_addition = colors_are_additive
        || exc_low.contains("in addition to its other types")
        || exc_low.contains("in addition to their other types")
        || exc_low.contains("in addition to its other creature types")
        || exc_low.contains("in addition to their other creature types")
        || exc_low.contains("in addition to its other artifact types")
        || exc_low.contains("in addition to their other artifact types")
        || exc_low.contains("in addition to its other land types")
        || exc_low.contains("in addition to their other land types")
        || exc_low.contains("is also ");
    let mut remainder = exc_low;
    strip_quoted_end_step_token_cleanup(
        &mut remainder,
        &mut sacrifice_at_end_step,
        &mut exile_at_end_step,
    );
    if remainder.contains('"') {
        return None;
    }
    let mut extra_tapped = false;
    let mut extra_attacking = false;
    for pat in [
        "that's tapped and attacking",
        "that are tapped and attacking",
        "it enters tapped and attacking",
        "they enter tapped and attacking",
        "enters tapped and attacking",
        "enter tapped and attacking",
        "tapped and attacking",
    ] {
        if remainder.contains(pat) {
            extra_tapped = true;
            extra_attacking = true;
            remainder = remainder.replace(pat, " ");
        }
    }
    for pat in [
        "it enters the battlefield tapped",
        "they enter the battlefield tapped",
        "enters the battlefield tapped",
        "enter the battlefield tapped",
        "it enters tapped",
        "they enter tapped",
        "enters tapped",
        "enter tapped",
        "that's tapped",
        "that are tapped",
    ] {
        if remainder.contains(pat) {
            extra_tapped = true;
            remainder = remainder.replace(pat, " ");
        }
    }
    for pat in ["that's attacking", "that are attacking"] {
        if remainder.contains(pat) {
            extra_attacking = true;
            remainder = remainder.replace(pat, " ");
        }
    }
    let mut grant_keywords: BTreeSet<KeywordAbility> = BTreeSet::new();
    for (phrase, kw) in [
        ("first strike", KeywordAbility::FirstStrike),
        ("double strike", KeywordAbility::DoubleStrike),
    ] {
        if remainder.contains(phrase) {
            grant_keywords.insert(kw);
            remainder = remainder.replace(phrase, " ");
        }
    }
    while let Some(idx) = remainder.find("protection from ") {
        let after = &remainder[idx + "protection from ".len()..];
        let end_rel = after
            .find([',', '.'])
            .or_else(|| after.find(" in addition"))
            .or_else(|| after.find(" and it "))
            .or_else(|| after.find(" and they "))
            .unwrap_or(after.len());
        let qual = after[..end_rel].trim();
        let parsed = parse_protection_keywords(qual, vocabulary)?;
        grant_keywords.extend(parsed);
        remainder.replace_range(idx..idx + "protection from ".len() + end_rel, " ");
    }
    while let Some(idx) = remainder.find("hexproof from ") {
        let after = &remainder[idx + "hexproof from ".len()..];
        let end_rel = after
            .find([',', '.'])
            .or_else(|| after.find(" in addition"))
            .or_else(|| after.find(" and it "))
            .or_else(|| after.find(" and they "))
            .unwrap_or(after.len());
        let qual = after[..end_rel].trim();
        let parsed = parse_hexproof_from_qualities(qual, vocabulary)?;
        for q in parsed {
            grant_keywords.insert(KeywordAbility::HexproofFrom(q));
        }
        remainder.replace_range(idx..idx + "hexproof from ".len() + end_rel, " ");
    }
    for pat in [
        "except the token isn't legendary",
        "except the tokens aren't legendary",
        "except that token isn't legendary",
        "except those tokens aren't legendary",
        "except it isn't legendary",
        "except they aren't legendary",
        "except it's not legendary",
        "except they're not legendary",
        "the token isn't legendary",
        "the tokens aren't legendary",
        "that token isn't legendary",
        "those tokens aren't legendary",
        "it isn't legendary",
        "they aren't legendary",
        "it's not legendary",
        "they're not legendary",
        "not legendary",
        "it has no mana cost",
        "they have no mana cost",
        "has no mana cost",
        "have no mana cost",
        "no mana cost",
        "sacrifice this token at the beginning of the next end step",
        "sacrifice this token at the beginning of the end step",
        "sacrifice this token at the beginning of your next end step",
        "sacrifice this token at the beginning of your end step",
        "sacrifice this permanent at the beginning of the next end step",
        "sacrifice this permanent at the beginning of the end step",
        "sacrifice this permanent at the beginning of your next end step",
        "sacrifice this permanent at the beginning of your end step",
        "sacrifice the token at the beginning of the next end step",
        "sacrifice the token at the beginning of the end step",
        "sacrifice those tokens at the beginning of the next end step",
        "sacrifice those tokens at the beginning of the end step",
        "sacrifice the token at the beginning of your next end step",
        "sacrifice the token at the beginning of your end step",
        "sacrifice those tokens at the beginning of your next end step",
        "sacrifice those tokens at the beginning of your end step",
        "sacrifice it at the beginning of the next end step",
        "sacrifice it at the beginning of the end step",
        "sacrifice them at the beginning of the next end step",
        "sacrifice them at the beginning of the end step",
        "sacrifice it at the beginning of your next end step",
        "sacrifice it at the beginning of your end step",
        "sacrifice them at the beginning of your next end step",
        "sacrifice them at the beginning of your end step",
        "at the beginning of the next end step, sacrifice this token",
        "at the beginning of the end step, sacrifice this token",
        "at the beginning of your next end step, sacrifice this token",
        "at the beginning of your end step, sacrifice this token",
        "at the beginning of the next end step, sacrifice this permanent",
        "at the beginning of the end step, sacrifice this permanent",
        "at the beginning of your next end step, sacrifice this permanent",
        "at the beginning of your end step, sacrifice this permanent",
        "at the beginning of the next end step, sacrifice the token",
        "at the beginning of the end step, sacrifice the token",
        "at the beginning of the next end step, sacrifice those tokens",
        "at the beginning of the end step, sacrifice those tokens",
        "at the beginning of the next end step, sacrifice it",
        "at the beginning of the end step, sacrifice it",
        "at the beginning of the next end step, sacrifice them",
        "at the beginning of the end step, sacrifice them",
        "at the beginning of your next end step, sacrifice the token",
        "at the beginning of your end step, sacrifice the token",
        "at the beginning of your next end step, sacrifice those tokens",
        "at the beginning of your end step, sacrifice those tokens",
        "at the beginning of your next end step, sacrifice it",
        "at the beginning of your end step, sacrifice it",
        "at the beginning of your next end step, sacrifice them",
        "at the beginning of your end step, sacrifice them",
        "exile this token at the beginning of the next end step",
        "exile this token at the beginning of the end step",
        "exile this token at the beginning of your next end step",
        "exile this token at the beginning of your end step",
        "exile this permanent at the beginning of the next end step",
        "exile this permanent at the beginning of the end step",
        "exile this permanent at the beginning of your next end step",
        "exile this permanent at the beginning of your end step",
        "exile the token at the beginning of the next end step",
        "exile the token at the beginning of the end step",
        "exile those tokens at the beginning of the next end step",
        "exile those tokens at the beginning of the end step",
        "exile the token at the beginning of your next end step",
        "exile the token at the beginning of your end step",
        "exile those tokens at the beginning of your next end step",
        "exile those tokens at the beginning of your end step",
        "exile it at the beginning of the next end step",
        "exile it at the beginning of the end step",
        "exile them at the beginning of the next end step",
        "exile them at the beginning of the end step",
        "exile it at the beginning of your next end step",
        "exile it at the beginning of your end step",
        "exile them at the beginning of your next end step",
        "exile them at the beginning of your end step",
        "at the beginning of the next end step, exile this token",
        "at the beginning of the end step, exile this token",
        "at the beginning of your next end step, exile this token",
        "at the beginning of your end step, exile this token",
        "at the beginning of the next end step, exile this permanent",
        "at the beginning of the end step, exile this permanent",
        "at the beginning of your next end step, exile this permanent",
        "at the beginning of your end step, exile this permanent",
        "at the beginning of the next end step, exile the token",
        "at the beginning of the end step, exile the token",
        "at the beginning of the next end step, exile those tokens",
        "at the beginning of the end step, exile those tokens",
        "at the beginning of the next end step, exile it",
        "at the beginning of the end step, exile it",
        "at the beginning of the next end step, exile them",
        "at the beginning of the end step, exile them",
        "at the beginning of your next end step, exile the token",
        "at the beginning of your end step, exile the token",
        "at the beginning of your next end step, exile those tokens",
        "at the beginning of your end step, exile those tokens",
        "at the beginning of your next end step, exile it",
        "at the beginning of your end step, exile it",
        "at the beginning of your next end step, exile them",
        "at the beginning of your end step, exile them",
        "in addition to its other colors and types",
        "in addition to their other colors and types",
        "in addition to its other creature types",
        "in addition to their other creature types",
        "in addition to its other artifact types",
        "in addition to their other artifact types",
        "in addition to its other land types",
        "in addition to their other land types",
        "in addition to its other colors",
        "in addition to their other colors",
        "in addition to its other types",
        "in addition to their other types",
        "with base power and toughness",
        "has base power and toughness",
        "have base power and toughness",
        "base power and toughness",
        "until end of turn",
        "that token gains",
        "those tokens gain",
        "the token gains",
        "the tokens gain",
        "that token has",
        "those tokens have",
        "the token has",
        "the tokens have",
        "except it has",
        "except they have",
        "except it gains",
        "except they gain",
        "except it becomes an",
        "except it becomes a",
        "it becomes an",
        "it becomes a",
        "except it's an",
        "except it's a",
        "except it's",
        "except they're",
        "except the token is",
        "except the tokens are",
        "except",
        "it has",
        "they have",
        "it gains",
        "they gain",
        "and has ",
        "and have ",
        "and gains ",
        "and gain ",
        ", has ",
        ", have ",
        ", gains ",
        ", gain ",
        "and it's an",
        "and it's a",
        "and it's",
        "and they're",
        "that's an",
        "that's a",
        "the token is",
        "the tokens are",
        "is also",
        "are also",
        "it's an",
        "it's a",
        "it's",
        "they're",
        "and",
        ",",
        ".",
    ] {
        remainder = remainder.replace(pat, " ");
    }
    let mut override_pt: Option<(RulesInteger, RulesInteger)> = None;
    let mut parsed_colors: BTreeSet<Color> = BTreeSet::new();
    let mut saw_colorless = false;
    let mut add_subtypes: BTreeSet<CreatureTypeId> = BTreeSet::new();
    let mut add_card_types: BTreeSet<CardType> = BTreeSet::new();
    let mut saw_creature_word = false;
    for raw_tok in remainder.split_whitespace() {
        let tok = raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '-');
        if tok.is_empty()
            || matches!(
                tok,
                "a" | "an"
                    | "the"
                    | "is"
                    | "are"
                    | "with"
                    | "token"
                    | "tokens"
                    | "that"
                    | "that's"
                    | "those"
            )
        {
            continue;
        }
        if matches!(tok, "creature" | "creatures") {
            saw_creature_word = true;
            continue;
        }
        if let Some((ps, ts)) = tok.split_once('/')
            && let (Ok(p), Ok(t)) = (ps.parse::<i64>(), ts.parse::<i64>())
            && override_pt.is_none()
        {
            override_pt = Some((p.into(), t.into()));
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
            "artifact" | "artifacts" if in_addition => {
                add_card_types.insert(CardType::Artifact);
                continue;
            }
            "enchantment" | "enchantments" if in_addition => {
                add_card_types.insert(CardType::Enchantment);
                continue;
            }
            "land" | "lands" if in_addition => {
                add_card_types.insert(CardType::Land);
                continue;
            }
            "planeswalker" | "planeswalkers" if in_addition => {
                add_card_types.insert(CardType::Planeswalker);
                continue;
            }
            _ => {}
        }
        if let Some(kw) = lower_keyword_ability_name(tok) {
            grant_keywords.insert(kw);
            continue;
        }
        if let Some(ct) = resolve_protection_subtype(tok, vocabulary) {
            add_subtypes.insert(ct);
            continue;
        }
        return None;
    }
    if saw_creature_word && (in_addition || override_pt.is_some()) {
        add_card_types.insert(CardType::Creature);
    }
    let haste = grant_keywords.contains(&KeywordAbility::Haste);
    Some(ParsedTokenCopyExceptions {
        nonlegendary,
        sacrifice_at_end_step,
        exile_at_end_step,
        strip_mana_cost,
        haste,
        extra_tapped,
        extra_attacking,
        colors_are_additive,
        saw_colorless,
        override_pt,
        parsed_colors,
        grant_keywords,
        add_subtypes,
        add_card_types,
    })
}

pub(super) fn strip_quoted_end_step_token_cleanup(
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
        let parse_cleanup_action = |tail: &str| -> Option<bool> {
            match tail.trim() {
                "sacrifice this permanent"
                | "sacrifice this token"
                | "sacrifice this creature"
                | "sacrifice this artifact"
                | "sacrifice this enchantment"
                | "sacrifice ~"
                | "sacrifice the token"
                | "sacrifice it"
                | "sacrifice them" => Some(true),
                "exile this permanent"
                | "exile this token"
                | "exile this creature"
                | "exile this artifact"
                | "exile this enchantment"
                | "exile ~"
                | "exile the token"
                | "exile it"
                | "exile them" => Some(false),
                _ => None,
            }
        };
        let step_phrases = [
            "at the beginning of the next end step",
            "at the beginning of your next end step",
            "at the beginning of the end step",
            "at the beginning of your end step",
            "at the beginning of each end step",
        ];
        let matched_action = step_phrases
            .iter()
            .find_map(|prefix| {
                inner
                    .strip_prefix(prefix)
                    .and_then(|rest| rest.strip_prefix(", ").or_else(|| rest.strip_prefix(' ')))
                    .and_then(parse_cleanup_action)
            })
            .or_else(|| {
                step_phrases.iter().find_map(|suffix| {
                    inner
                        .strip_suffix(suffix)
                        .and_then(|lead| parse_cleanup_action(lead.trim()))
                })
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

pub(super) fn parse_suppressed_keyword_list(
    kws_raw: &str,
) -> Result<BTreeSet<KeywordAbility>, &'static str> {
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

pub(super) fn parse_untap_up_to_phrase(raw: &str) -> Option<(CardType, u16)> {
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

pub(super) fn is_temp_menace_blocking_rule(
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

pub(super) fn build_emblem_definition(
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

fn normalize_nested_single_quotes_for_token(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut in_single = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = raw[i..].chars().next().unwrap_or('\0');
        let clen = ch.len_utf8();
        if ch == '\'' {
            let prev_is_word = i > 0 && bytes[i - 1].is_ascii_alphanumeric();
            let next_byte = bytes.get(i + 1).copied();
            if !in_single
                && !prev_is_word
                && next_byte.is_some_and(|b| b.is_ascii_alphabetic() || b == b'{' || b == b'~')
            {
                in_single = true;
                out.push('"');
                i += clen;
                continue;
            }
            if in_single
                && next_byte
                    .is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b',' | b'.' | b';'))
            {
                in_single = false;
                out.push('"');
                i += clen;
                continue;
            }
        }
        out.push(ch);
        i += clen;
    }
    out
}

fn fix_token_ability_capitalization(text: &str) -> String {
    // 1. Capitalize after em-dash + space (e.g. "Landfall <em-dash> Whenever ...").
    let mut after_dash = String::with_capacity(text.len());
    let mut cap_after_dash = false;
    let mut saw_dash = false;
    for ch in text.chars() {
        if cap_after_dash && ch.is_ascii_alphabetic() {
            after_dash.push(ch.to_ascii_uppercase());
            cap_after_dash = false;
            saw_dash = false;
        } else {
            after_dash.push(ch);
            if ch == '\u{2014}' {
                saw_dash = true;
            } else if saw_dash && ch == ' ' {
                cap_after_dash = true;
                saw_dash = false;
            } else {
                saw_dash = false;
            }
        }
    }
    // 2. Before the first colon in an activated ability cost, capitalize cost-atom verbs
    //    following ", " so oracle_compiler::clauses::parse_cost_list does not merge them
    //    into the preceding cost atom as lowercase English continuations.
    let Some((cost_part, effect_part)) = after_dash.split_once(':') else {
        return after_dash;
    };
    let mut fixed_cost = String::with_capacity(cost_part.len());
    let mut first_atom = true;
    for seg in cost_part.split(", ") {
        if !first_atom {
            fixed_cost.push_str(", ");
        }
        let seg_low = seg.trim_start().to_ascii_lowercase();
        let is_cost_verb = [
            "sacrifice ",
            "exile ",
            "discard ",
            "pay ",
            "tap ",
            "untap ",
            "return ",
            "remove ",
            "put ",
            "reveal ",
            "mill ",
            "forage",
            "collect ",
            "waterbend ",
            "behold ",
            "exert ",
            "flip ",
            "roll ",
            "choose ",
        ]
        .iter()
        .any(|verb| seg_low.starts_with(verb));
        if !first_atom && is_cost_verb {
            let mut chars = seg.chars();
            if let Some(first_ch) = chars.next() {
                fixed_cost.push(first_ch.to_ascii_uppercase());
                fixed_cost.push_str(chars.as_str());
            }
        } else {
            fixed_cost.push_str(seg);
        }
        first_atom = false;
    }
    format!("{fixed_cost}:{effect_part}")
}

fn try_precompile_token_quoted_abilities(
    token: &TokenSpecAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(TokenSpecAst, FaceAst)> {
    let TokenSpecAst::Creature {
        count,
        power,
        toughness,
        colors,
        supertypes,
        card_types,
        subtypes,
        keywords,
        tapped,
        attacking,
        named,
    } = token
    else {
        return None;
    };
    if !keywords.iter().any(|k| k.contains('"')) {
        return None;
    }
    let mut bare_keywords: Vec<String> = Vec::new();
    let mut quoted_texts: Vec<String> = Vec::new();
    for kw_raw in keywords {
        if !kw_raw.contains('"') {
            let trimmed = kw_raw.trim();
            if !trimmed.is_empty() {
                bare_keywords.push(trimmed.to_string());
            }
            continue;
        }
        let mut rest = kw_raw.as_str();
        let mut outside = String::new();
        while let Some(open) = rest.find('"') {
            outside.push_str(&rest[..open]);
            let after_open = &rest[open + 1..];
            let close = after_open.find('"')?;
            let cleaned = after_open[..close]
                .trim()
                .trim_end_matches([',', ';'])
                .trim();
            if !cleaned.is_empty() {
                quoted_texts.push(cleaned.to_string());
            }
            rest = &after_open[close + 1..];
        }
        outside.push_str(rest);
        let outside_trim = outside.trim().trim_matches([',', ';', '.']).trim();
        if !outside_trim.is_empty() {
            for seg in outside_trim.split(',') {
                for piece in seg.split(" and ") {
                    let clean = piece
                        .trim()
                        .trim_start_matches("with ")
                        .trim_start_matches("With ")
                        .trim_matches([',', ';', '.'])
                        .trim();
                    if !clean.is_empty() && !clean.eq_ignore_ascii_case("and") {
                        bare_keywords.push(clean.to_string());
                    }
                }
            }
        }
    }
    if quoted_texts.is_empty() {
        return None;
    }
    let tok_name = named.as_deref().unwrap_or("Token");
    let mut type_words: Vec<&str> = Vec::new();
    if (supertypes.0 & spellwake_oracle_compiler::ast::SupertypeMask::LEGENDARY.0) != 0 {
        type_words.push("Legendary");
    }
    for (mask, label) in [
        (AstCardTypeMask::ARTIFACT, "Artifact"),
        (AstCardTypeMask::BATTLE, "Battle"),
        (AstCardTypeMask::CREATURE, "Creature"),
        (AstCardTypeMask::ENCHANTMENT, "Enchantment"),
        (AstCardTypeMask::KINDRED, "Kindred"),
        (AstCardTypeMask::LAND, "Land"),
        (AstCardTypeMask::PLANESWALKER, "Planeswalker"),
    ] {
        if (card_types.0 & mask.0) != 0 {
            type_words.push(label);
        }
    }
    if subtypes.iter().any(|s| s.eq_ignore_ascii_case("aura")) {
        type_words.push("Aura");
    }
    if subtypes.iter().any(|s| s.eq_ignore_ascii_case("equipment")) {
        type_words.push("Equipment");
    }
    let tok_type_line = if type_words.is_empty() {
        "Creature".to_string()
    } else {
        type_words.join(" ")
    };
    let normalized_lines: Vec<String> = quoted_texts
        .iter()
        .map(|q| {
            let unnested = normalize_nested_single_quotes_for_token(q);
            let base = normalize_token_ability_text(&unnested, tok_name);
            fix_token_ability_capitalization(&base)
        })
        .collect();
    let oracle_text = normalized_lines.join("\n");
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let parsed = spellwake_oracle_compiler::compile_card(
        &spellwake_oracle_compiler::CardInput {
            oracle_id: "",
            name: tok_name,
            layout: "normal",
            faces: vec![spellwake_oracle_compiler::FaceInput {
                name: tok_name,
                type_line: &tok_type_line,
                oracle_text: &oracle_text,
            }],
        },
        &compiler_vocab,
    );
    if !parsed.strictly_compiled {
        return None;
    }
    let face = parsed.faces.into_iter().next()?;
    if !face.spell_effects.is_empty()
        || !face.failures.is_empty()
        || (face.keywords.is_empty()
            && face.statics.is_empty()
            && face.replacements.is_empty()
            && face.abilities.is_empty())
    {
        return None;
    }
    let cleaned_token = TokenSpecAst::Creature {
        count: count.clone(),
        power: power.clone(),
        toughness: toughness.clone(),
        colors: *colors,
        supertypes: *supertypes,
        card_types: *card_types,
        subtypes: subtypes.clone(),
        keywords: bare_keywords,
        tapped: *tapped,
        attacking: *attacking,
        named: named.clone(),
    };
    Some((cleaned_token, face))
}

#[cfg(test)]
use super::super::engine_trajectories as token_trajectories;

#[cfg(test)]
mod wave12_token_ability_tests {
    use super::*;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "wave12-tokens".into(),
            [0; 32],
            vec![
                "Human".into(),
                "Pest".into(),
                "Rat".into(),
                "Warlock".into(),
            ],
        )
        .unwrap()
    }

    fn compiled_token() -> (TokenSpecAst, FaceAst) {
        let vocab = vocabulary();
        let parsed = compile_card(
            &CardInput {
                oracle_id: "",
                name: "Token Maker",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Token Maker",
                    type_line: "Sorcery",
                    oracle_text: "Create two tapped 1/1 black and green Pest creature tokens with flying and \"When this creature dies, you gain 1 life.\"",
                }],
            },
            &cached_compiler_vocab(&vocab),
        );
        assert!(parsed.strictly_compiled);
        match &parsed.faces[0].spell_effects[0] {
            InstructionAst::CreateTokenWithAbilities {
                token,
                compiled_abilities,
                ..
            } => (token.clone(), (**compiled_abilities).clone()),
            other => panic!("expected compiled token: {other:?}"),
        }
    }

    #[test]
    fn wave12_compiled_token_keeps_local_trigger_ids_and_token_properties() {
        let vocab = vocabulary();
        let (token, face) = compiled_token();
        let mut ids = LoweringIds {
            next_triggered: 73,
            next_activated: 91,
            ..Default::default()
        };
        let (creation, attacking) = lower_token_spec_with_abilities(
            &token,
            TokenRecipients::EachOpponent,
            &vocab,
            &mut ids,
            None,
            None,
            Some(&face),
        )
        .unwrap();
        assert!(creation.enters_tapped);
        assert!(!attacking);
        assert_eq!(creation.amount, ValueExpr::literal(2));
        assert_eq!(creation.recipients, TokenRecipients::EachOpponent);
        let program = creation.definition.program();
        assert_eq!(program.triggered.len(), 1);
        assert!(program.printed_keywords.contains(&KeywordAbility::Flying));
        assert!(program.triggered[0].body.instructions.iter().any(|op|
            matches!(op, Instruction::AdjustControllerLife { adjustment: LifeAdjustment::Gain(amount) } if *amount == 1.into())));
        let (second, _) = lower_token_spec_with_abilities(
            &token,
            TokenRecipients::EachOpponent,
            &vocab,
            &mut ids,
            None,
            None,
            Some(&face),
        )
        .unwrap();
        assert_eq!(creation.definition, second.definition);
        assert_eq!(ids.next_triggered, 73);
        assert_eq!(ids.next_activated, 91);
    }

    #[test]
    fn wave12_compiled_token_rejects_spell_effects_in_nested_face() {
        let vocab = vocabulary();
        let (token, mut face) = compiled_token();
        face.spell_effects.push(InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token: token.clone(),
        });
        assert!(
            lower_token_spec_with_abilities(
                &token,
                TokenRecipients::Controller,
                &vocab,
                &mut LoweringIds::default(),
                None,
                None,
                Some(&face),
            )
            .is_err()
        );
    }

    #[test]
    fn wave12_compiled_pest_death_trigger_gains_life_only_when_it_resolves() {
        use super::token_trajectories::{P0, P1, object, pass_pair, spell, submit, world};
        use spellwake_engine::{
            game::GameCommand, mana::ManaPayment, object_identity::ObjectZone, targets::TargetRef,
        };
        let vocab = vocabulary();
        let (mut token, face) = compiled_token();
        if let TokenSpecAst::Creature { count, .. } = &mut token {
            **count = ValueExprAst::Const(1);
        }
        let (creation, _) = lower_token_spec_with_abilities(
            &token,
            TokenRecipients::Controller,
            &vocab,
            &mut LoweringIds::default(),
            None,
            None,
            Some(&face),
        )
        .unwrap();
        let game = world(
            std::sync::Arc::new(vocab),
            vec![
                spell(
                    1,
                    BodyTarget::None,
                    vec![Instruction::CreateTokens {
                        creations: vec![creation],
                    }],
                ),
                spell(
                    2,
                    BodyTarget::SingleCreature,
                    vec![Instruction::DestroyTarget],
                ),
            ],
            vec![],
        );
        let cast = submit(
            &game,
            P0,
            GameCommand::CastUntargeted {
                spell: object(&game, 1),
                payment: Some(ManaPayment::default()),
            },
        );
        let created = pass_pair(cast.state());
        let token_object = created
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .find(|card| card.zone() == ObjectZone::Battlefield)
            .unwrap()
            .id();
        assert_eq!(created.state().execution().life_total(P0), Some(&20.into()));
        let destroy = submit(
            created.state(),
            P0,
            GameCommand::CastPaid {
                spell: object(created.state(), 2),
                target: TargetRef::Object(token_object),
                payment: ManaPayment::default(),
            },
        );
        let died = pass_pair(destroy.state());
        assert_eq!(died.state().execution().life_total(P0), Some(&20.into()));
        let resolved = pass_pair(died.state());
        assert_eq!(
            resolved.state().execution().life_total(P0),
            Some(&21.into())
        );
        assert_eq!(
            resolved.state().execution().life_total(P1),
            Some(&20.into())
        );
    }
    #[test]
    fn wave12_named_token_cards_lower_complete() {
        let vocab = vocabulary();
        for (name, cost, text) in [
            (
                "Rat Out",
                "{B}",
                "Target creature gets -1/-1 until end of turn. Create a 1/1 black Rat creature token with \"This token can't block.\"",
            ),
            (
                "Pest Summoning",
                "{1}{B/G}{B/G}",
                "Create two 1/1 black and green Pest creature tokens with \"When this token dies, you gain 1 life.\"",
            ),
            (
                "Hunt for Specimens",
                "{1}{B}",
                "Create a 1/1 black and green Pest creature token with \"When this creature dies, you gain 1 life.\"\nLearn.",
            ),
        ] {
            let card = InventoryCard {
                scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
                oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
                name: name.into(),
                layout: "normal".into(),
                faces: vec![spellwake_content::CatalogFace {
                    name: name.into(),
                    type_line: Some(
                        if name == "Rat Out" {
                            "Instant"
                        } else if name == "Pest Summoning" {
                            "Sorcery \u{2014} Lesson"
                        } else {
                            "Sorcery"
                        }
                        .into(),
                    ),
                    oracle_text: Some(text.into()),
                    mana_cost: Some(cost.into()),
                    power: None,
                    toughness: None,
                }],
                legalities: BTreeMap::new(),
                eligible_formats: Vec::new(),
                required_by_base_formats: true,
                program_evidence: "test",
            };
            let result = compile_with_reason(&card, &vocab);
            if name == "Hunt for Specimens" {
                // Learn was unsupported before the E14 token AST migration.
                assert_eq!(result.unwrap_err(), "oracle:unsupported-instruction");
            } else {
                result.unwrap_or_else(|error| panic!("{name}: {error}"));
            }
        }
    }
}
