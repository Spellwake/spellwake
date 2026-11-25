use super::*;

pub(super) fn lower_replacement(
    rep: &ReplacementEffectAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if let ReplacementEffectAst::Compound(items) = rep {
        if items.is_empty() {
            return Err("oracle:unsupported-replacement");
        }
        for item in items {
            lower_replacement(item, program, vocabulary, ids)?;
        }
        return Ok(());
    }
    if let ReplacementEffectAst::EscapesWithCounters {
        counter_kind,
        count,
    } = rep
        && counter_kind == "+1/+1"
        && let ValueExprAst::Const(n) = count.as_ref()
        && let Ok(u) = u16::try_from(*n)
        && let Some(ConditionalCastCost::Escape {
            ref mut entry_counters,
            ..
        }) = program.conditional_cost
    {
        *entry_counters = entry_counters
            .checked_add(u)
            .ok_or("oracle:escape-counter-ovf")?;
        return Ok(());
    }
    if let ReplacementEffectAst::EventInstead {
        subject_or_event,
        would,
        instead,
        ..
    } = rep
    {
        if let Some(res) = lower_event_instead_replacement(
            subject_or_event,
            would,
            instead,
            program,
            vocabulary,
            ids,
        ) {
            return res;
        }
        return Err("oracle:unsupported-replacement");
    }
    let ProgramKind::Permanent(def) = &mut program.kind else {
        return Err("oracle:replacement-on-spell");
    };
    match rep {
        ReplacementEffectAst::EntersTapped { unless: None } => {
            if def.entry_tap.replace(EntryTap::Always).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::EntersTapped { unless: Some(cond) } => {
            if let Ok(entry_tap) = lower_enters_tapped_unless(cond) {
                if def.entry_tap.replace(entry_tap).is_some() {
                    return Err("oracle:duplicate-entry-tap");
                }
                Ok(())
            } else {
                let mut tmp_builder = BodyBuilder::new();
                let rc =
                    lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)
                        .map_err(|_| "oracle:unsupported-entry-unless")?;
                if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
                    return Err("oracle:unsupported-entry-unless");
                }
                if program.entry_tap_unless.replace(Box::new(rc)).is_some() {
                    return Err("oracle:duplicate-entry-tap");
                }
                Ok(())
            }
        }
        ReplacementEffectAst::EntersTappedUnlessPayLife { life } if *life > 0 => {
            let amount = u8::try_from(*life).map_err(|_| "oracle:life-overflow")?;
            if def
                .entry_tap
                .replace(EntryTap::UnlessPayLife(amount))
                .is_some()
            {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::OthersEnterTapped { subject } => {
            let selector = lower_permanent_selector(subject, vocabulary)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ids.static_id(),
                    rule: StaticRestriction::OthersEnterTapped(Box::new(selector)),
                },
            )
        }
        ReplacementEffectAst::EntersTappedUnlessReveal { filter } => {
            if filter.subtypes.len() == 2
                && let (Some(a), Some(b)) = (
                    parse_basic_land_subtype(&filter.subtypes[0]),
                    parse_basic_land_subtype(&filter.subtypes[1]),
                )
            {
                if def
                    .entry_tap
                    .replace(EntryTap::UnlessRevealHandLandType(a, b))
                    .is_some()
                {
                    return Err("oracle:duplicate-entry-tap");
                }
                return Ok(());
            }
            Err("oracle:unsupported-entry-reveal")
        }
        ReplacementEffectAst::EntersWithCounters {
            counter_kind,
            count,
            per,
            x_expr,
            condition,
        } => {
            let ck = counter_kind.to_ascii_lowercase();
            let push_entry_counter = |program: &mut Program,
                                      kind: CounterKind,
                                      amount: ValueExpr,
                                      cond: Option<&ConditionAst>|
             -> Result<(), &'static str> {
                if let Some(c) = cond {
                    let mut tmp_builder = BodyBuilder::new();
                    let rc =
                        lower_resolution_condition(c, &mut tmp_builder, vocabulary, false, None)?;
                    if tmp_builder.target != BodyTarget::None {
                        return Err("oracle:unsupported-entry-counters");
                    }
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::ConditionalCounter {
                            condition: Box::new(rc),
                            kind,
                            amount,
                        });
                } else {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::DynamicCounter { kind, amount });
                }
                Ok(())
            };
            // Case 1: "enters with [count] [kind] counters for each [per]"
            if let Some(per_filter) = per
                && x_expr.is_none()
                && let ValueExprAst::Const(mult) = count.as_ref()
                && *mult > 0
            {
                let kind = parse_counter_kind(&ck).ok_or("oracle:unsupported-entry-counters")?;
                if !kind.is_permanent() {
                    return Err("oracle:unsupported-entry-counters");
                }
                let mut inner = per_filter.as_ref().clone();
                let is_exiled_with_self = (inner.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
                let zone = match inner.zone.take() {
                    None if is_exiled_with_self => {
                        spellwake_engine::program::ValueZone::Exile(None)
                    }
                    None => spellwake_engine::program::ValueZone::Battlefield,
                    Some(ZoneFilter::YourGraveyard) => {
                        spellwake_engine::program::ValueZone::Graveyard(ResolvingPlayer::Controller)
                    }
                    Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard) => {
                        spellwake_engine::program::ValueZone::AllGraveyards
                    }
                    Some(ZoneFilter::OpponentsGraveyards) => {
                        if inner.controller.is_some() {
                            return Err("oracle:unsupported-entry-counters");
                        }
                        inner.controller = Some(ControllerRelation::OwnerOpponent);
                        spellwake_engine::program::ValueZone::AllGraveyards
                    }
                    Some(ZoneFilter::YourHand) => {
                        spellwake_engine::program::ValueZone::Hand(ResolvingPlayer::Controller)
                    }
                    Some(ZoneFilter::Exile) => spellwake_engine::program::ValueZone::Exile(None),
                    Some(ZoneFilter::YourLibrary) => {
                        spellwake_engine::program::ValueZone::Library(ResolvingPlayer::Controller)
                    }
                    _ => return Err("oracle:unsupported-entry-counters"),
                };
                let obj_filter = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner, vocabulary)?
                };
                let base = ValueExpr::Count {
                    zone,
                    filter: Box::new(obj_filter),
                    controller_scope: None,
                };
                let amount = if *mult == 1 {
                    base
                } else {
                    ValueExpr::Scale {
                        expr: Box::new(base),
                        factor: i64::from(*mult).into(),
                    }
                };
                return push_entry_counter(program, kind, amount, condition.as_deref());
            }
            // Case 2: "enters with X / N [kind] counters, where X is [x_expr]" or "enters with a +1/+1 counter for each [x_expr]"
            if let Some(xe) = x_expr
                && per.is_none()
            {
                let factor = match count.as_ref() {
                    ValueExprAst::ChosenX => Some(1i64),
                    ValueExprAst::Const(n) if *n > 0 => Some(i64::from(*n)),
                    _ => None,
                };
                if let Some(factor) = factor {
                    let kind =
                        parse_counter_kind(&ck).ok_or("oracle:unsupported-entry-counters")?;
                    if !kind.is_permanent() {
                        return Err("oracle:unsupported-entry-counters");
                    }
                    let base = lower_value_expr(xe, vocabulary)?;
                    let amount = if factor == 1 {
                        base
                    } else {
                        ValueExpr::Scale {
                            expr: Box::new(base),
                            factor: factor.into(),
                        }
                    };
                    return push_entry_counter(program, kind, amount, condition.as_deref());
                }
            }
            if per.is_some() || x_expr.is_some() {
                return Err("oracle:unsupported-entry-counters");
            }
            match (ck.as_str(), count.as_ref(), condition.as_deref()) {
                ("+1/+1", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedPlusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("-1/-1", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedMinusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("+1/+1", ValueExprAst::ChosenX, None) => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::ChosenXPlusOne);
                    Ok(())
                }
                (
                    "+1/+1",
                    ValueExprAst::Const(n),
                    Some(ConditionAst::Morbid {
                        your_control_only: false,
                    }),
                ) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::MorbidPlusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("charge", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedCharge(i64::from(*n).into()));
                    Ok(())
                }
                ("loyalty", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedLoyalty(i64::from(*n).into()));
                    Ok(())
                }
                (other, ValueExprAst::Const(n), None) if *n > 0 => {
                    let kind =
                        parse_counter_kind(other).ok_or("oracle:unsupported-entry-counters")?;
                    if kind.is_player_counter()
                        || matches!(kind, CounterKind::PlusOne | CounterKind::MinusOne)
                    {
                        return Err("oracle:unsupported-entry-counters");
                    }
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedNamed {
                            kind,
                            amount: i64::from(*n).into(),
                        });
                    Ok(())
                }
                (other, expr, cond) => {
                    let kind =
                        parse_counter_kind(other).ok_or("oracle:unsupported-entry-counters")?;
                    if !kind.is_permanent() {
                        return Err("oracle:unsupported-entry-counters");
                    }
                    let amount = lower_value_expr(expr, vocabulary)?;
                    push_entry_counter(program, kind, amount, cond)
                }
            }
        }
        ReplacementEffectAst::OthersEnterWithCounters {
            subject,
            counter_kind,
            count,
            additional: _,
            per,
        } => {
            let kind = parse_counter_kind(counter_kind)
                .ok_or("oracle:unsupported-others-entry-counters")?;
            if !kind.is_permanent() {
                return Err("oracle:unsupported-others-entry-counters");
            }
            let selectors = lower_permanent_selectors(subject, vocabulary)?;
            if per.is_none()
                && let ValueExprAst::Const(n) = count.as_ref()
                && let Some(cnt) = u8::try_from(*n).ok().filter(|&v| v > 0)
            {
                for sel in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Modifier {
                            id: ids.static_id(),
                            rule: StaticModifierRule::OthersEnterWithCounters {
                                selector: Box::new(sel),
                                kind,
                                count: cnt,
                            },
                        },
                    )?;
                }
                return Ok(());
            }
            let amount = if let Some(per_expr) = per {
                let mult = match count.as_ref() {
                    ValueExprAst::Const(n) if *n > 0 => i64::from(*n),
                    _ => return Err("oracle:unsupported-others-entry-counters"),
                };
                let base = lower_value_expr(per_expr, vocabulary)?;
                if mult == 1 {
                    base
                } else {
                    ValueExpr::Scale {
                        expr: Box::new(base),
                        factor: mult.into(),
                    }
                }
            } else {
                lower_value_expr(count, vocabulary)?
            };
            if !amount.is_world_evaluable() {
                return Err("oracle:unsupported-others-entry-counters");
            }
            for sel in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::OthersEnterWithCountersExpr {
                            selector: Box::new(sel),
                            kind,
                            amount: Box::new(amount.clone()),
                        },
                    },
                )?;
            }
            Ok(())
        }
        ReplacementEffectAst::EntersTappedWithCounters {
            counter_kind,
            count,
        } => {
            if def.entry_tap.replace(EntryTap::Always).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            match (counter_kind.to_ascii_lowercase().as_str(), count.as_ref()) {
                ("+1/+1", ValueExprAst::Const(n)) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedPlusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("+1/+1", ValueExprAst::ChosenX) => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::ChosenXPlusOne);
                    Ok(())
                }
                ("charge", ValueExprAst::Const(n)) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedCharge(i64::from(*n).into()));
                    Ok(())
                }
                (other, ValueExprAst::Const(n)) if *n > 0 => {
                    let kind = parse_counter_kind(other)
                        .ok_or("oracle:unsupported-entry-tapped-counters")?;
                    if matches!(
                        kind,
                        CounterKind::PlusOne
                            | CounterKind::MinusOne
                            | CounterKind::Poison
                            | CounterKind::Experience
                            | CounterKind::Energy
                    ) {
                        return Err("oracle:unsupported-entry-tapped-counters");
                    }
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedNamed {
                            kind,
                            amount: i64::from(*n).into(),
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-entry-tapped-counters"),
            }
        }
        ReplacementEffectAst::AsEntersChoose { choice } => {
            let low = choice.trim().to_ascii_lowercase();
            let parse_except_color = |s: &str| -> Option<ManaType> {
                let c = s.strip_prefix("a color other than ")?.trim();
                match c {
                    "white" => Some(ManaType::White),
                    "blue" => Some(ManaType::Blue),
                    "black" => Some(ManaType::Black),
                    "red" => Some(ManaType::Red),
                    "green" => Some(ManaType::Green),
                    _ => None,
                }
            };
            if let Some(excluded) = parse_except_color(&low) {
                match def.entry_tap {
                    None => {
                        def.entry_tap = Some(EntryTap::ChooseColorExcept(excluded));
                        return Ok(());
                    }
                    Some(EntryTap::Always) => {
                        def.entry_tap = Some(EntryTap::AlwaysChooseColorExcept(excluded));
                        return Ok(());
                    }
                    _ => return Err("oracle:duplicate-entry-tap"),
                }
            }
            if matches!(
                low.as_str(),
                "an opponent to protect it" | "an opponent to protect ~"
            ) {
                return match def.entry_tap {
                    None => {
                        def.entry_tap = Some(EntryTap::SiegeProtector);
                        Ok(())
                    }
                    Some(EntryTap::SiegeProtector) => Ok(()),
                    Some(_) => Err("oracle:duplicate-entry-tap"),
                };
            }
            let entry_tap = match low.as_str() {
                "a creature type" | "creature type" => EntryTap::ChooseCreatureType,
                "a nonland card name" | "nonland card name" => EntryTap::ChooseNonlandCardName,
                "two players" => EntryTap::ChooseTwoPlayers,
                "a color" | "color" => EntryTap::ChooseColor,
                "an opponent" | "opponent" => EntryTap::ChooseOpponent,
                "opponent_controller" => EntryTap::ChosenOpponentControls,
                "a player" | "player" => EntryTap::ChoosePlayer,
                "a card type" | "card type" => EntryTap::ChooseCardType,
                "a basic land type" | "basic land type" => EntryTap::ChooseBasicLandType,
                "odd or even" => EntryTap::ChooseOddOrEven,
                "khans or dragons" => EntryTap::ChooseKhansOrDragons,
                _ => return Err("oracle:unsupported-as-enters-choose"),
            };
            if def.entry_tap.replace(entry_tap).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::EntersPrepared => {
            program.enters_prepared = true;
            Ok(())
        }
        ReplacementEffectAst::EntersAsCopy {
            optional: true,
            filter,
            exception,
        } => {
            if exception.as_deref().is_some_and(|e| {
                let l = e.to_ascii_lowercase();
                l.contains("tapped") || l.contains("until end of turn")
            }) {
                return Err("oracle:unsupported-enters-as-copy");
            }
            if exception.is_none()
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.flags.is_empty()
                && let Some(rule) = match (filter.zone, filter.controller, filter.any_types) {
                    (Some(ZoneFilter::AnyGraveyard), None, m) if m == AstCardTypeMask::CREATURE => {
                        Some(EntryCopyRule::GraveyardCreature)
                    }
                    (None | Some(ZoneFilter::Battlefield), None, m)
                        if m == AstCardTypeMask::CREATURE =>
                    {
                        Some(EntryCopyRule::BattlefieldCreature)
                    }
                    (None | Some(ZoneFilter::Battlefield), Some(ControllerRelation::You), m)
                        if m == AstCardTypeMask::CREATURE =>
                    {
                        Some(EntryCopyRule::ControlledCreature)
                    }
                    (None | Some(ZoneFilter::Battlefield), None, m)
                        if m == AstCardTypeMask::ARTIFACT =>
                    {
                        Some(EntryCopyRule::BattlefieldArtifact)
                    }
                    (None | Some(ZoneFilter::Battlefield), None, m)
                        if m == AstCardTypeMask::ENCHANTMENT =>
                    {
                        Some(EntryCopyRule::BattlefieldEnchantment)
                    }
                    _ => None,
                }
            {
                if program.entry_copy.replace(rule).is_some() {
                    return Err("oracle:duplicate-entry-copy");
                }
                return Ok(());
            }
            let zone = match filter.zone {
                None | Some(ZoneFilter::Battlefield) => EntryCopyZone::Battlefield,
                Some(ZoneFilter::YourGraveyard) => EntryCopyZone::OwnGraveyard,
                Some(ZoneFilter::AnyGraveyard) => EntryCopyZone::AnyGraveyard,
                _ => return Err("oracle:unsupported-enters-as-copy"),
            };
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj_filter = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)
                    .map_err(|_| "oracle:unsupported-enters-as-copy")?
            };
            let mut ext = EntryCopySpecExt {
                zone,
                filter: obj_filter,
                nonlegendary: false,
                extra_types: BTreeSet::new(),
                extra_creature_subtypes: BTreeSet::new(),
                extra_keywords: BTreeSet::new(),
                extra_counters: Vec::new(),
                conditional_counters: Vec::new(),
                extra_activated: Vec::new(),
                extra_triggered: Vec::new(),
                extra_static: Vec::new(),
                override_pt: None,
                extra_colors: BTreeSet::new(),
                keep_name: false,
            };
            if let Some(exc_raw) = exception.as_deref() {
                lower_entry_copy_exception(exc_raw, &mut ext, vocabulary, ids)?;
            }
            let rule = if ext.extra_counters.len() <= 1
                && ext.conditional_counters.is_empty()
                && ext.extra_activated.len() <= 1
                && ext.extra_triggered.len() <= 1
                && ext.extra_static.is_empty()
                && ext.override_pt.is_none()
                && ext.extra_colors.is_empty()
                && !ext.keep_name
            {
                EntryCopyRule::Matching(Box::new(spellwake_engine::program::EntryCopySpec {
                    zone: ext.zone,
                    filter: ext.filter,
                    nonlegendary: ext.nonlegendary,
                    extra_types: ext.extra_types,
                    extra_creature_subtypes: ext.extra_creature_subtypes,
                    extra_keywords: ext.extra_keywords,
                    extra_counters: ext.extra_counters.into_iter().next(),
                    extra_activated: ext.extra_activated.into_iter().next().map(Box::new),
                    extra_triggered: ext.extra_triggered.into_iter().next().map(Box::new),
                }))
            } else {
                EntryCopyRule::MatchingExt(Box::new(ext))
            };
            if program.entry_copy.replace(rule).is_some() {
                return Err("oracle:duplicate-entry-copy");
            }
            Ok(())
        }
        _ => Err("oracle:unsupported-replacement"),
    }
}

pub(super) fn lower_enters_tapped_unless(cond: &ConditionAst) -> Result<EntryTap, &'static str> {
    match cond {
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 2,
        } => {
            if filter.any_types == AstCardTypeMask::LAND
                && filter.required_supertypes == AstSupertypeMask::BASIC
            {
                return Ok(EntryTap::UnlessTwoBasicLands);
            }
            if filter.any_types == AstCardTypeMask::LAND
                && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0
                && filter.required_supertypes.is_empty()
            {
                return Ok(EntryTap::UnlessTwoOtherLands);
            }
            Err("oracle:unsupported-entry-unless")
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Le,
            value: 2,
        } if filter.any_types == AstCardTypeMask::LAND
            && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 =>
        {
            Ok(EntryTap::UnlessTwoOrFewerOtherLands)
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 2 => {
            if let (Some(a), Some(b)) = (
                parse_basic_land_subtype(&filter.subtypes[0]),
                parse_basic_land_subtype(&filter.subtypes[1]),
            ) {
                return Ok(EntryTap::UnlessLandType(a, b));
            }
            Err("oracle:unsupported-entry-unless")
        }
        ConditionAst::OpponentCountCompare {
            op: CompareOp::Ge,
            value: 2,
        } => Ok(EntryTap::UnlessTwoOpponents),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value: 8,
        } if filter.any_types == AstCardTypeMask::LAND => Ok(EntryTap::UnlessOpponentLandsEight),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::AnyPlayer,
            op: CompareOp::Le,
            value: 13,
        } => Ok(EntryTap::UnlessPlayerLifeThirteenOrLess),
        _ => Err("oracle:unsupported-entry-unless"),
    }
}

fn lower_event_instead_replacement(
    subject_or_event: &str,
    would: &str,
    instead: &str,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Result<(), &'static str>> {
    let subj_low = subject_or_event.trim().to_ascii_lowercase();
    let would_raw = would
        .trim()
        .trim_end_matches(',')
        .trim()
        .to_ascii_lowercase();
    let would_norm = would_raw
        .strip_prefix("would ")
        .unwrap_or(&would_raw)
        .trim();
    let instead_raw = instead
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    let instead_norm = instead_raw
        .strip_prefix("you may instead ")
        .or_else(|| instead_raw.strip_prefix("instead, "))
        .or_else(|| instead_raw.strip_prefix("instead "))
        .unwrap_or(&instead_raw)
        .trim_end_matches(" instead")
        .trim();

    let next_zone_rep_id = |prog: &Program| -> ZoneReplacementAbilityId {
        let next = prog
            .zone_replacements
            .iter()
            .map(|z| z.id.0)
            .max()
            .map_or(0, |m| m + 1);
        ZoneReplacementAbilityId(next)
    };

    let is_permanent = matches!(program.kind, ProgramKind::Permanent(_));

    // 0. Discard-by-opponent replacement on self (Loxodon Smiter, Obstinate Baloth, Wilt-Leaf Liege)
    if let Some(res) = try_lower_self_discard_by_opponent(
        &subj_low,
        would_norm,
        &instead_raw,
        instead_norm,
        program,
        &next_zone_rep_id,
    ) {
        return Some(res);
    }

    // 1. Special full-sentence damage prevention / replacement shapes emitted by clauses.rs
    let parse_min_life_floor = |after_less: &str| -> Option<u8> {
        let (floor_part, target_part) = after_less.split_once(" reduces it to ")?;
        let floor_raw = floor_part
            .trim()
            .strip_suffix(" this turn")
            .unwrap_or(floor_part.trim())
            .trim();
        let target_no_instead = target_part
            .trim()
            .trim_end_matches('.')
            .trim()
            .strip_suffix(" instead")
            .unwrap_or(target_part.trim().trim_end_matches('.').trim())
            .trim();
        let target_raw = target_no_instead
            .strip_suffix(" this turn")
            .unwrap_or(target_no_instead)
            .trim();
        let floor = floor_raw.parse::<u8>().ok()?;
        let target = target_raw.parse::<u8>().ok()?;
        (floor > 0 && floor == target).then_some(floor)
    };
    if is_permanent
        && would_norm == "reduce_life_below_floor"
        && let Some(after_less) = instead_raw
            .split_once("reduce your life total to less than ")
            .map(|(_, rest)| rest)
        && let Some(floor) = parse_min_life_floor(after_less)
    {
        let require_creature = instead_raw.contains("if you control a creature");
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: StaticDamageRule::MinimumLifeTotal {
                    floor,
                    require_creature,
                },
            },
        ));
    }
    if is_permanent
        && would_norm == "would_event"
        && matches!(
            subj_low.as_str(),
            "damage" | "if you control a creature, damage"
        )
        && let Some(after_less) = instead_norm.strip_prefix("reduce your life total to less than ")
        && let Some(floor) = parse_min_life_floor(after_less)
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: StaticDamageRule::MinimumLifeTotal {
                    floor,
                    require_creature: subj_low.starts_with("if you control a creature"),
                },
            },
        ));
    }
    if is_permanent
        && (would_norm == "special_replacement"
            || subj_low == "damage_source"
            || subj_low == "spell_damage")
    {
        let full_norm = instead_raw
            .replace("this creature", "~")
            .replace("this permanent", "~")
            .replace("this artifact", "~")
            .replace("this enchantment", "~");
        let full = full_norm.as_str();
        if full
            == "prevent all noncombat damage that would be dealt to you and other permanents you control"
        {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventNoncombat,
                },
            ));
        }
        if matches!(
            full,
            "prevent all combat damage that would be dealt to and dealt by ~"
                | "prevent all combat damage that would be dealt by and to ~"
                | "prevent all combat damage that would be dealt to ~ and combat damage that ~ would deal"
        ) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventSourceCombat,
                },
            ));
        }
        if matches!(
            full,
            "prevent all combat damage that would be dealt to ~"
                | "if combat damage would be dealt to ~, prevent that damage"
        ) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventIncomingCombatToSource,
                },
            ));
        }
        if full
            == "if combat damage would be dealt to ~, prevent that damage and exile that many cards from the top of your library"
        {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventIncomingCombatAndExile,
                },
            ));
        }
        if full
            == "prevent all combat damage that would be dealt to attacking artifact creatures you control"
        {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventCombatToAttackingArtifacts,
                },
            ));
        }
        if matches!(
            full,
            "prevent all combat damage that would be dealt to and dealt by enchanted creature"
                | "prevent all combat damage that would be dealt by and to enchanted creature"
        ) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventEnchantedCreature { combat_only: true },
                },
            ));
        }
        if full == "prevent all damage that would be dealt by enchanted creature" {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventEnchantedCreature { combat_only: false },
                },
            ));
        }
        if matches!(
            full,
            "if damage would be dealt to ~, prevent that damage. remove a +1/+1 counter from ~"
                | "if damage would be dealt to ~, prevent that damage and remove a +1/+1 counter from ~"
                | "if damage would be dealt to ~, prevent that damage and remove a +1/+1 counter from it"
                | "if ~ would be dealt damage, prevent that damage. remove a +1/+1 counter from ~"
                | "if ~ would be dealt damage, prevent that damage. remove a +1/+1 counter from it"
                | "if ~ would be dealt damage, prevent that damage and remove a +1/+1 counter from ~"
                | "if ~ would be dealt damage, prevent that damage and remove a +1/+1 counter from it"
        ) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventIncomingRemovePlusOne,
                },
            ));
        }
        if matches!(
            full,
            "if damage would be dealt to ~ while it has a +1/+1 counter on it, prevent that damage and remove a +1/+1 counter from it"
                | "if damage would be dealt to ~ while it has a +1/+1 counter on it, prevent that damage and remove a +1/+1 counter from ~"
                | "if damage would be dealt to ~ while it has a +1/+1 counter on it, prevent that damage. remove a +1/+1 counter from it"
                | "if damage would be dealt to ~ while it has a +1/+1 counter on it, prevent that damage. remove a +1/+1 counter from ~"
                | "if ~ would be dealt damage while it has a +1/+1 counter on it, prevent that damage and remove a +1/+1 counter from it"
                | "if ~ would be dealt damage while it has a +1/+1 counter on it, prevent that damage and remove a +1/+1 counter from ~"
        ) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventIncomingRemovePlusOneIfCounter,
                },
            ));
        }
        if matches!(
            full,
            "if damage would be dealt to ~, prevent that damage and remove that many +1/+1 counters from ~"
                | "if damage would be dealt to ~, prevent that damage and remove that many +1/+1 counters from it"
                | "if ~ would be dealt damage, prevent that damage and remove that many +1/+1 counters from ~"
                | "if ~ would be dealt damage, prevent that damage and remove that many +1/+1 counters from it"
        ) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventIncomingRemoveNPlusOne,
                },
            ));
        }
        if full == "if damage would be dealt to ~, prevent half that damage, rounded up" {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::PreventHalfIncoming,
                },
            ));
        }
        let simple_prev = match full {
            "prevent all combat damage that would be dealt to creatures you control" => Some((
                StaticDamageRecipient::ControlledCreatures,
                StaticDamageKind::CombatOnly,
            )),
            "prevent all noncombat damage that would be dealt to creatures you control" => Some((
                StaticDamageRecipient::ControlledCreatures,
                StaticDamageKind::NoncombatOnly,
            )),
            "prevent all damage that would be dealt to creatures you control" => Some((
                StaticDamageRecipient::ControlledCreatures,
                StaticDamageKind::Any,
            )),
            "prevent all combat damage that would be dealt to you" => Some((
                StaticDamageRecipient::Controller,
                StaticDamageKind::CombatOnly,
            )),
            "prevent all noncombat damage that would be dealt to you" => Some((
                StaticDamageRecipient::Controller,
                StaticDamageKind::NoncombatOnly,
            )),
            "prevent all damage that would be dealt to you" => {
                Some((StaticDamageRecipient::Controller, StaticDamageKind::Any))
            }
            _ => None,
        };
        if let Some((recipient, kind)) = simple_prev {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::ModifyDamage {
                        source: None,
                        source_controller: PlayerRelation::AnyParticipant,
                        recipient,
                        kind,
                        op: StaticDamageOp::PreventAll,
                    },
                },
            ));
        }
        if let Some(rule) = parse_prevent_all_damage_sentence(full, vocabulary) {
            let ab = StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule,
            };
            if ab.validate(vocabulary).is_ok() {
                return Some(push_static_ability(program, ab));
            }
        }
        if let Some((cond_opt, s_part, w_part, i_part)) = split_if_would_instead(full) {
            let i_clean = i_part.trim().trim_end_matches(" instead").trim();
            if let Some(rules) =
                parse_damage_rules_full(cond_opt, s_part, w_part, i_clean, vocabulary, ids)
            {
                for rule in rules {
                    if let Err(err) = push_static_ability(
                        program,
                        StaticContinuousAbility::DamageRule {
                            id: ids.static_id(),
                            rule,
                        },
                    ) {
                        return Some(Err(err));
                    }
                }
                return Some(Ok(()));
            }
            if cond_opt.is_none()
                && let Some(rule) = parse_redirect_damage_event(s_part, w_part, i_clean, vocabulary)
            {
                return Some(push_static_ability(
                    program,
                    StaticContinuousAbility::DamageRule {
                        id: ids.static_id(),
                        rule,
                    },
                ));
            }
            if cond_opt.is_none()
                && would_norm == "special_replacement"
                && s_part != "damage_source"
                && s_part != "spell_damage"
                && w_part != "special_replacement"
                && let Some(res) = lower_event_instead_replacement(
                    s_part, w_part, i_part, program, vocabulary, ids,
                )
            {
                return Some(res);
            }
        }
    }

    // 2. Zone Replacement Abilities (CR 614.6)
    let is_self_subj = matches!(
        subj_low.as_str(),
        "~" | "this card"
            | "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
            | "this aura"
            | "this equipment"
            | "this vehicle"
            | "this battle"
            | "this spell"
            | "this token"
    );
    if is_self_subj
        && matches!(
            would_norm,
            "be put into a graveyard from anywhere"
                | "be put into your graveyard from anywhere"
                | "be put into its owner's graveyard from anywhere"
        )
        && let Some((ZoneReplacementChoice::Mandatory, replace_with, extra)) =
            parse_zone_replace_dest_and_extra(
                &instead_raw,
                instead_norm,
                true,
                true,
                false,
                false,
                vocabulary,
                ids,
            )
    {
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::Any,
            objects: ZoneObjectKind::Either,
            subject: ZoneReplacementSubject::SourceSelf,
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice: ZoneReplacementChoice::Mandatory,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    if !is_permanent {
        return None;
    }
    if is_self_subj
        && matches!(
            would_norm,
            "die"
                | "be put into a graveyard from the battlefield"
                | "be put into your graveyard from the battlefield"
                | "be put into its owner's graveyard from the battlefield"
        )
        && let Some((choice, replace_with, extra)) = parse_zone_replace_dest_and_extra(
            &instead_raw,
            instead_norm,
            true,
            true,
            false,
            true,
            vocabulary,
            ids,
        )
    {
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                ZoneActionKind::StateBased,
                ZoneActionKind::EffectDestroy,
                ZoneActionKind::EffectSacrifice,
                ZoneActionKind::SacrificeCost,
            ])),
            objects: ZoneObjectKind::Either,
            subject: ZoneReplacementSubject::SourceSelf,
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    let is_damaged_by_source_die = (matches!(
        subj_low.as_str(),
        "a creature dealt damage by ~ this turn"
            | "a permanent dealt damage by ~ this turn"
            | "a creature dealt damage this turn by ~"
            | "a permanent dealt damage this turn by ~"
            | "a creature dealt damage by this creature this turn"
            | "a permanent dealt damage by this creature this turn"
            | "a creature dealt damage this turn by this creature"
            | "a permanent dealt damage this turn by this creature"
            | "a creature dealt damage by this permanent this turn"
            | "a permanent dealt damage by this permanent this turn"
            | "a creature dealt damage this turn by this permanent"
            | "a permanent dealt damage this turn by this permanent"
            | "a creature that was dealt damage by ~ this turn"
            | "a permanent that was dealt damage by ~ this turn"
            | "a creature that was dealt damage this turn by ~"
            | "a permanent that was dealt damage this turn by ~"
            | "a creature that was dealt damage by this creature this turn"
            | "a permanent that was dealt damage by this creature this turn"
            | "any creature dealt damage by ~ this turn"
            | "any permanent dealt damage by ~ this turn"
    ) && matches!(
        would_norm,
        "die" | "be put into a graveyard from the battlefield"
    )) || (matches!(
        subj_low.as_str(),
        "a creature dealt damage by ~"
            | "a permanent dealt damage by ~"
            | "a creature dealt damage by this creature"
            | "a permanent dealt damage by this creature"
            | "a creature dealt damage by this permanent"
            | "a permanent dealt damage by this permanent"
    ) && would_norm == "die this turn");
    if is_damaged_by_source_die
        && let Some((choice, PublicZoneDestination::Exile, extra)) =
            parse_zone_replace_dest_and_extra(
                &instead_raw,
                instead_norm,
                false,
                false,
                false,
                false,
                vocabulary,
                ids,
            )
    {
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                ZoneActionKind::StateBased,
                ZoneActionKind::EffectDestroy,
                ZoneActionKind::EffectSacrifice,
                ZoneActionKind::SacrificeCost,
            ])),
            objects: ZoneObjectKind::Either,
            subject: ZoneReplacementSubject::DamagedBySourceThisTurn,
            destination: PublicZoneDestination::Graveyard,
            replace_with: PublicZoneDestination::Exile,
            choice,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    let would_zone_norm = would_norm
        .strip_suffix(" this turn")
        .unwrap_or(would_norm)
        .trim();
    // Containment Priest / Hallowed Moonlight / Mistcaller: enters without being cast
    if let Some(not_cast_ctrl) = match would_zone_norm {
        "enter and it wasn't cast"
        | "enter the battlefield and it wasn't cast"
        | "enter and wasn't cast"
        | "enter the battlefield and wasn't cast"
        | "enter without being cast"
        | "enter the battlefield without being cast" => Some(None),
        "enter under an opponent's control and it wasn't cast"
        | "enter the battlefield under an opponent's control and it wasn't cast"
        | "enter under an opponent's control without being cast"
        | "enter the battlefield under an opponent's control without being cast" => {
            Some(Some(ControllerRelation::Opponent))
        }
        "enter under your control and it wasn't cast"
        | "enter the battlefield under your control and it wasn't cast"
        | "enter under your control without being cast"
        | "enter the battlefield under your control without being cast" => {
            Some(Some(ControllerRelation::You))
        }
        _ => None,
    } && let Some((choice, replace_with, extra)) = parse_zone_replace_dest_and_extra(
        &instead_raw,
        instead_norm,
        true,
        false,
        false,
        true,
        vocabulary,
        ids,
    ) && let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
        &subj_low,
        &cached_compiler_vocab(vocabulary),
    ) && !(ast_f.any_spell && ast_f.any_types.is_empty())
    {
        let ctrl_ok = match (ast_f.controller, not_cast_ctrl) {
            (None, Some(c)) => {
                ast_f.controller = Some(c);
                true
            }
            (Some(a), Some(b)) => a == b,
            (_, None) => true,
        };
        if ctrl_ok && let Ok(obj) = lower_object_filter(&ast_f, vocabulary) {
            let objects = match ast_f.token {
                Some(false) => ZoneObjectKind::Card,
                Some(true) => ZoneObjectKind::Token,
                None => ZoneObjectKind::Either,
            };
            let id = next_zone_rep_id(program);
            program.zone_replacements.push(ZoneReplacementAbility {
                id,
                actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                    ZoneActionKind::EffectEnterBattlefield,
                ])),
                objects,
                subject: ZoneReplacementSubject::MatchingNotCast {
                    filter: Box::new(obj),
                    owner_relation: None,
                },
                destination: PublicZoneDestination::Battlefield,
                replace_with,
                choice,
            });
            if let Some(ex) = extra {
                program.zone_replacement_extras.insert(id, ex);
            }
            return Some(Ok(()));
        }
    }
    if matches!(
        subj_low.as_str(),
        "a card or token" | "a card" | "any card" | "any card or token"
    ) && matches!(
        would_zone_norm,
        "be put into an opponent's graveyard from anywhere"
            | "be put into a graveyard from anywhere"
            | "be put into your graveyard from anywhere"
            | "be put into a player's graveyard from anywhere"
            | "be put into its owner's graveyard from anywhere"
    ) && let Some((ZoneReplacementChoice::Mandatory, replace_with, extra)) =
        parse_zone_replace_dest_and_extra(
            &instead_raw,
            instead_norm,
            true,
            false,
            false,
            false,
            vocabulary,
            ids,
        )
    {
        let id = next_zone_rep_id(program);
        let objects = if matches!(subj_low.as_str(), "a card" | "any card") {
            ZoneObjectKind::Card
        } else {
            ZoneObjectKind::Either
        };
        let subject = if would_zone_norm.contains("opponent's graveyard") {
            ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::any()),
                owner_relation: Some(PlayerRelation::Opponent),
            }
        } else if would_zone_norm.contains("your graveyard") {
            ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::any()),
                owner_relation: Some(PlayerRelation::Controller),
            }
        } else {
            ZoneReplacementSubject::AnyObject
        };
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::Any,
            objects,
            subject,
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice: ZoneReplacementChoice::Mandatory,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    if subj_low == "a card you didn't control"
        && would_zone_norm == "be put into an opponent's graveyard from anywhere"
        && let Some((ZoneReplacementChoice::Mandatory, replace_with, extra)) =
            parse_zone_replace_dest_and_extra(
                &instead_raw,
                instead_norm,
                true,
                false,
                false,
                false,
                vocabulary,
                ids,
            )
    {
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::Any,
            objects: ZoneObjectKind::Card,
            subject: ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::any().not_controlled()),
                owner_relation: Some(PlayerRelation::Opponent),
            },
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice: ZoneReplacementChoice::Mandatory,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    if subj_low == "a nontoken creature an opponent owns"
        && would_zone_norm
            == "die or a creature card not on the battlefield would be put into an opponent's graveyard"
        && let Some((ZoneReplacementChoice::Mandatory, replace_with, extra)) =
            parse_zone_replace_dest_and_extra(
                &instead_raw,
                instead_norm,
                true,
                false,
                false,
                false,
                vocabulary,
                ids,
            )
    {
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::Any,
            objects: ZoneObjectKind::Card,
            subject: ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::creature()),
                owner_relation: Some(PlayerRelation::Opponent),
            },
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice: ZoneReplacementChoice::Mandatory,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    if subj_low == "a black or red permanent, spell, or card not on the battlefield"
        && would_zone_norm == "be put into a graveyard"
        && let Some((ZoneReplacementChoice::Mandatory, replace_with, extra)) =
            parse_zone_replace_dest_and_extra(
                &instead_raw,
                instead_norm,
                true,
                false,
                false,
                false,
                vocabulary,
                ids,
            )
    {
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::Any,
            objects: ZoneObjectKind::Either,
            subject: ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::any().with_color(ObjectColorFilter::Mask {
                    any_of: ColorMask::BLACK.union(ColorMask::RED),
                    none_of: ColorMask::EMPTY,
                })),
                owner_relation: None,
            },
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice: ZoneReplacementChoice::Mandatory,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }
    let is_battlefield_to_gy = matches!(
        would_zone_norm,
        "die"
            | "be put into a graveyard from the battlefield"
            | "be put into an opponent's graveyard from the battlefield"
            | "be put into your graveyard from the battlefield"
            | "be put into its owner's graveyard from the battlefield"
    ) || (matches!(
        would_zone_norm,
        "be put into a graveyard"
            | "be put into an opponent's graveyard"
            | "be put into your graveyard"
            | "be put into its owner's graveyard"
    ) && (subj_low.contains("permanent")
        || subj_low.contains("creature"))
        && !subj_low.contains("card")
        && !subj_low.contains("spell"));
    if is_battlefield_to_gy
        && let Some((choice, replace_with, extra)) = parse_zone_replace_dest_and_extra(
            &instead_raw,
            instead_norm,
            true,
            true,
            false,
            true,
            vocabulary,
            ids,
        )
    {
        let owner_relation = if would_zone_norm.contains("opponent's graveyard") {
            Some(PlayerRelation::Opponent)
        } else if would_zone_norm.contains("your graveyard") {
            Some(PlayerRelation::Controller)
        } else {
            None
        };
        let creature_or_pw = || {
            ObjectFilter::Clause(
                ObjectClause::any()
                    .any_types(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER)),
            )
        };
        let matched: Option<(ObjectFilter, ZoneObjectKind, Option<PlayerRelation>)> = match subj_low
            .as_str()
        {
            "a creature an opponent controls"
            | "a creature your opponents control"
            | "a creature opponents control" => Some((
                ObjectFilter::creature().opposing(),
                ZoneObjectKind::Either,
                None,
            )),
            "a creature you don't control" => Some((
                ObjectFilter::creature().not_controlled(),
                ZoneObjectKind::Either,
                None,
            )),
            "a nontoken creature an opponent controls"
            | "a nontoken creature your opponents control"
            | "a nontoken creature opponents control" => Some((
                ObjectFilter::creature().opposing().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a nontoken creature you don't control" => Some((
                ObjectFilter::creature().not_controlled().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a creature" => Some((ObjectFilter::creature(), ZoneObjectKind::Either, None)),
            "a nontoken creature" => Some((
                ObjectFilter::creature().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a creature token" => Some((
                ObjectFilter::creature().token(),
                ZoneObjectKind::Token,
                None,
            )),
            "another creature" => Some((
                ObjectFilter::creature().other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            "another nontoken creature" => Some((
                ObjectFilter::creature()
                    .other_than_ability_source()
                    .nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a creature you control" => Some((
                ObjectFilter::creature().controlled(),
                ZoneObjectKind::Either,
                None,
            )),
            "a nontoken creature you control" => Some((
                ObjectFilter::creature().controlled().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "another creature you control" => Some((
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            "another nontoken creature you control" => Some((
                ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source()
                    .nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "another creature an opponent controls"
            | "another creature your opponents control"
            | "another creature opponents control" => Some((
                ObjectFilter::creature()
                    .opposing()
                    .other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            "another nontoken creature an opponent controls"
            | "another nontoken creature your opponents control"
            | "another nontoken creature opponents control" => Some((
                ObjectFilter::creature()
                    .opposing()
                    .other_than_ability_source()
                    .nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "another creature you don't control" => Some((
                ObjectFilter::creature()
                    .not_controlled()
                    .other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            "another nontoken creature you don't control" => Some((
                ObjectFilter::creature()
                    .not_controlled()
                    .other_than_ability_source()
                    .nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a creature an opponent owns" => Some((
                ObjectFilter::creature(),
                ZoneObjectKind::Either,
                Some(PlayerRelation::Opponent),
            )),
            "a nontoken creature an opponent owns" => Some((
                ObjectFilter::creature().nontoken(),
                ZoneObjectKind::Card,
                Some(PlayerRelation::Opponent),
            )),
            "a creature you own" => Some((
                ObjectFilter::creature(),
                ZoneObjectKind::Either,
                Some(PlayerRelation::Controller),
            )),
            "a nontoken creature you own" => Some((
                ObjectFilter::creature().nontoken(),
                ZoneObjectKind::Card,
                Some(PlayerRelation::Controller),
            )),
            "a creature or planeswalker" => Some((creature_or_pw(), ZoneObjectKind::Either, None)),
            "a creature or planeswalker an opponent controls"
            | "a creature or planeswalker your opponents control"
            | "a creature or planeswalker opponents control" => {
                Some((creature_or_pw().opposing(), ZoneObjectKind::Either, None))
            }
            "a creature or planeswalker you control" => {
                Some((creature_or_pw().controlled(), ZoneObjectKind::Either, None))
            }
            "a creature or planeswalker you don't control" => Some((
                creature_or_pw().not_controlled(),
                ZoneObjectKind::Either,
                None,
            )),
            "a permanent an opponent controls"
            | "a permanent your opponents control"
            | "a permanent opponents control" => Some((
                ObjectFilter::permanent().opposing(),
                ZoneObjectKind::Either,
                None,
            )),
            "a permanent you don't control" => Some((
                ObjectFilter::permanent().not_controlled(),
                ZoneObjectKind::Either,
                None,
            )),
            "a nontoken permanent an opponent controls"
            | "a nontoken permanent your opponents control"
            | "a nontoken permanent opponents control" => Some((
                ObjectFilter::permanent().opposing().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a nontoken permanent you don't control" => Some((
                ObjectFilter::permanent().not_controlled().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a permanent you control" => Some((
                ObjectFilter::permanent().controlled(),
                ZoneObjectKind::Either,
                None,
            )),
            "a nontoken permanent you control" => Some((
                ObjectFilter::permanent().controlled().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "a permanent" => Some((ObjectFilter::permanent(), ZoneObjectKind::Either, None)),
            "a nontoken permanent" => Some((
                ObjectFilter::permanent().nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "another permanent" => Some((
                ObjectFilter::permanent().other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            "another nontoken permanent" => Some((
                ObjectFilter::permanent()
                    .other_than_ability_source()
                    .nontoken(),
                ZoneObjectKind::Card,
                None,
            )),
            "another permanent you control" => Some((
                ObjectFilter::permanent()
                    .controlled()
                    .other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            "another permanent an opponent controls"
            | "another permanent your opponents control"
            | "another permanent opponents control" => Some((
                ObjectFilter::permanent()
                    .opposing()
                    .other_than_ability_source(),
                ZoneObjectKind::Either,
                None,
            )),
            _ => spellwake_oracle_compiler::filter::parse_object_filter(
                &subj_low,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()
            .filter(|ast_f| !(ast_f.any_spell && ast_f.any_types.is_empty()))
            .and_then(|ast_f| {
                let kind = match ast_f.token {
                    Some(false) => ZoneObjectKind::Card,
                    Some(true) => ZoneObjectKind::Token,
                    None => ZoneObjectKind::Either,
                };
                lower_object_filter(&ast_f, vocabulary)
                    .ok()
                    .map(|obj| (obj, kind, None))
            }),
        };
        if let Some((filter, objects, subj_owner)) = matched {
            let merged_owner = match (owner_relation, subj_owner) {
                (Some(a), Some(b)) if a != b => None,
                (Some(a), _) | (None, Some(a)) => Some(Some(a)),
                (None, None) => Some(None),
            };
            if let Some(final_owner) = merged_owner {
                let id = next_zone_rep_id(program);
                program.zone_replacements.push(ZoneReplacementAbility {
                    id,
                    actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                        ZoneActionKind::StateBased,
                        ZoneActionKind::EffectDestroy,
                        ZoneActionKind::EffectSacrifice,
                        ZoneActionKind::SacrificeCost,
                    ])),
                    objects,
                    subject: ZoneReplacementSubject::Matching {
                        filter: Box::new(filter),
                        owner_relation: final_owner,
                    },
                    destination: PublicZoneDestination::Graveyard,
                    replace_with,
                    choice,
                });
                if let Some(ex) = extra {
                    program.zone_replacement_extras.insert(id, ex);
                }
                return Some(Ok(()));
            }
        }
    }
    if matches!(
        would_zone_norm,
        "be put into a graveyard from anywhere"
            | "be put into an opponent's graveyard from anywhere"
            | "be put into your graveyard from anywhere"
            | "be put into a player's graveyard from anywhere"
            | "be put into its owner's graveyard from anywhere"
    ) && let Some((ZoneReplacementChoice::Mandatory, replace_with, extra)) =
        parse_zone_replace_dest_and_extra(
            &instead_raw,
            instead_norm,
            true,
            false,
            false,
            false,
            vocabulary,
            ids,
        )
        && let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
            &subj_low,
            &cached_compiler_vocab(vocabulary),
        )
        && !(ast_f.any_spell && ast_f.any_types.is_empty())
        && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
    {
        let objects = match ast_f.token {
            Some(false) => ZoneObjectKind::Card,
            Some(true) => ZoneObjectKind::Token,
            None if subj_low.contains("card") && !subj_low.contains("token") => {
                ZoneObjectKind::Card
            }
            None => ZoneObjectKind::Either,
        };
        let owner_relation = if would_zone_norm.contains("opponent's graveyard") {
            Some(PlayerRelation::Opponent)
        } else if would_zone_norm.contains("your graveyard") {
            Some(PlayerRelation::Controller)
        } else {
            None
        };
        let id = next_zone_rep_id(program);
        program.zone_replacements.push(ZoneReplacementAbility {
            id,
            actions: ZoneActionPredicate::Any,
            objects,
            subject: ZoneReplacementSubject::Matching {
                filter: Box::new(obj),
                owner_relation,
            },
            destination: PublicZoneDestination::Graveyard,
            replace_with,
            choice: ZoneReplacementChoice::Mandatory,
        });
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Some(Ok(()));
    }

    // 3. Static Draw Replacements (CR 614.1a)
    if subj_low == "you"
        && would_norm == "draw a card while your library has no cards in it"
        && instead_norm == "you win the game"
    {
        return Some(push_primary_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::EmptyDrawWins,
            },
        ));
    }
    if subj_low == "you"
        && would_norm == "draw a card while your library has no cards in it"
        && let Some(rest) = instead_norm.strip_prefix("put ")
        && let Some((qty_kind, tail)) = rest.split_once(" counter")
        && (tail.contains("on ~") || tail.contains("on it"))
    {
        let mut parts = qty_kind.split_whitespace();
        if let Some(num_tok) = parts.next()
            && let Some(amount) = match num_tok {
                "a" | "an" | "one" => Some(1u8),
                "two" => Some(2),
                "three" => Some(3),
                "four" => Some(4),
                "five" => Some(5),
                "six" => Some(6),
                "seven" => Some(7),
                _ => num_tok.parse::<u8>().ok(),
            }
            && amount > 0
            && let Some(kind) = parse_counter_kind(parts.collect::<Vec<_>>().join(" ").trim())
            && kind.is_permanent()
        {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DrawRule {
                    id: ids.static_id(),
                    rule: StaticDrawRule::EmptyDrawPutCounters { kind, amount },
                },
            ));
        }
    }
    let draw_mult_count = |s: &str| -> Option<u8> {
        if matches!(
            s,
            "draw twice that many cards" | "you draw twice that many cards"
        ) {
            return Some(2);
        }
        if matches!(
            s,
            "draw three times that many cards" | "you draw three times that many cards"
        ) {
            return Some(3);
        }
        let rest = s
            .strip_prefix("you draw ")
            .or_else(|| s.strip_prefix("draw "))?;
        let n_tok = rest.strip_suffix(" cards")?.trim();
        parse_small_num(n_tok).filter(|&n| n >= 2)
    };
    let draw_plus_count = |s: &str| -> Option<u8> {
        if matches!(s, "draw an additional card" | "you draw an additional card") {
            return Some(1);
        }
        if let Some(rest) = s
            .strip_prefix("you draw that many cards plus ")
            .or_else(|| s.strip_prefix("draw that many cards plus "))
        {
            if matches!(rest.trim(), "an additional card" | "one additional card") {
                return Some(1);
            }
            return parse_small_num(rest.trim()).filter(|&n| n > 0);
        }
        let rest = s
            .strip_prefix("you draw that many plus ")
            .or_else(|| s.strip_prefix("draw that many plus "))?
            .strip_suffix(" cards")
            .or_else(|| {
                s.strip_prefix("you draw that many plus ")
                    .or_else(|| s.strip_prefix("draw that many plus "))
                    .and_then(|r| r.strip_suffix(" card"))
            })?;
        parse_small_num(rest.trim()).filter(|&n| n > 0)
    };
    let is_except_first_draw_step = matches!(
        would_norm,
        "draw a card except the first one you draw in each of your draw steps"
            | "draw a card except the first one you draw in each of your draw steps,"
            | "draw a card, except the first one you draw in each of your draw steps"
            | "draw one or more cards except the first one you draw in each of your draw steps"
            | "draw one or more cards except the first one you draw in each of your draw steps,"
            | "draw one or more cards, except the first one you draw in each of your draw steps"
    );
    if subj_low == "you" && is_except_first_draw_step {
        if let Some(multiply) = draw_mult_count(instead_norm) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DrawRule {
                    id: ids.static_id(),
                    rule: StaticDrawRule::ModifyDraw {
                        add: 0,
                        multiply,
                        skip_first_draw_step: true,
                        require_empty_hand: false,
                    },
                },
            ));
        }
        if let Some(add) = draw_plus_count(instead_norm) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DrawRule {
                    id: ids.static_id(),
                    rule: StaticDrawRule::ModifyDraw {
                        add,
                        multiply: 1,
                        skip_first_draw_step: true,
                        require_empty_hand: false,
                    },
                },
            ));
        }
    }
    if subj_low == "you" && would_norm == "draw a card while you have no cards in hand" {
        if let Some(multiply) = draw_mult_count(instead_norm) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DrawRule {
                    id: ids.static_id(),
                    rule: StaticDrawRule::ModifyDraw {
                        add: 0,
                        multiply,
                        skip_first_draw_step: false,
                        require_empty_hand: true,
                    },
                },
            ));
        }
        if let Some(add) = draw_plus_count(instead_norm) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DrawRule {
                    id: ids.static_id(),
                    rule: StaticDrawRule::ModifyDraw {
                        add,
                        multiply: 1,
                        skip_first_draw_step: false,
                        require_empty_hand: true,
                    },
                },
            ));
        }
    }
    if subj_low == "you"
        && would_norm == "draw a card"
        && let Some(multiply) = draw_mult_count(instead_norm)
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 0,
                    multiply,
                    skip_first_draw_step: false,
                    require_empty_hand: false,
                },
            },
        ));
    }
    if subj_low == "you"
        && (would_norm == "draw one or more cards" || would_norm == "draw a card")
        && let Some(add) = draw_plus_count(instead_norm)
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add,
                    multiply: 1,
                    skip_first_draw_step: false,
                    require_empty_hand: false,
                },
            },
        ));
    }
    if subj_low == "you"
        && would_norm == "draw one or more cards"
        && let Some(multiply) = if matches!(
            instead_norm,
            "draw twice that many cards" | "you draw twice that many cards"
        ) {
            Some(2u8)
        } else if matches!(
            instead_norm,
            "draw three times that many cards" | "you draw three times that many cards"
        ) {
            Some(3u8)
        } else {
            None
        }
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 0,
                    multiply,
                    skip_first_draw_step: false,
                    require_empty_hand: false,
                },
            },
        ));
    }
    if subj_low == "you"
        && matches!(
            would_norm,
            "lose unspent mana" | "lose unspent mana as steps and phases end"
        )
        && let Some(target_mana) = instead_norm
            .strip_prefix("that mana becomes ")
            .map(|rest| {
                let s = rest.trim();
                let s = s.strip_suffix(" instead").unwrap_or(s).trim();
                s.strip_suffix(" mana").unwrap_or(s).trim()
            })
            .and_then(parse_single_mana_type)
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ConvertUnspentMana(target_mana),
            },
        ));
    }
    if subj_low == "you"
        && would_norm == "draw a card"
        && instead_norm.contains("choose land or nonland")
        && instead_norm.contains("reveal cards from the top of your library")
    {
        return Some(push_primary_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ReplaceWithLandKind,
            },
        ));
    }
    if subj_low == "an opponent"
        && would_norm == "draw two or more cards"
        && instead_norm == "you and that player each draw a card"
    {
        return Some(push_primary_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::SplitOpponentsMultipleDraw,
            },
        ));
    }

    // 4. Token Creation Modifiers (CR 614.1a)
    if (subj_low == "an effect"
        && matches!(
            would_norm,
            "create one or more tokens under your control"
                | "put one or more tokens onto the battlefield under your control"
        )
        && matches!(
            instead_norm,
            "it creates twice that many of those tokens"
                | "it puts twice that many of those tokens onto the battlefield"
        ))
        || (subj_low == "one or more tokens"
            && would_norm == "be created under your control"
            && instead_norm == "twice that many of those tokens are created")
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::DoubleCreatedTokens,
            },
        ));
    }
    if (subj_low == "an effect"
        && would_norm == "create one or more tokens under your control"
        && matches!(
            instead_norm,
            "it creates three times that many of those tokens"
                | "it creates triple that many of those tokens"
        ))
        || (subj_low == "one or more tokens"
            && would_norm == "be created under your control"
            && matches!(
                instead_norm,
                "three times that many of those tokens are created"
                    | "triple that many of those tokens are created"
            ))
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyTokens {
                    controller: PlayerRelation::Controller,
                    add: 0,
                    multiply: 3,
                },
            },
        ));
    }
    if (subj_low == "an effect" && would_norm == "create one or more tokens under your control")
        || (subj_low == "one or more tokens" && would_norm == "be created under your control")
    {
        let add_n = if subj_low == "an effect" {
            instead_norm
                .strip_prefix("it creates that many tokens plus ")
                .or_else(|| instead_norm.strip_prefix("it creates those tokens plus "))
                .or_else(|| {
                    instead_norm
                        .strip_prefix("it creates that many plus ")
                        .and_then(|s| s.strip_suffix(" of those tokens"))
                })
                .and_then(parse_small_num)
        } else {
            instead_norm
                .strip_prefix("that many tokens plus ")
                .or_else(|| instead_norm.strip_prefix("those tokens plus "))
                .and_then(|s| s.strip_suffix(" are created"))
                .and_then(parse_small_num)
        };
        if let Some(add) = add_n.filter(|&n| n > 0) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::ModifyTokens {
                        controller: PlayerRelation::Controller,
                        add,
                        multiply: 1,
                    },
                },
            ));
        }
    }
    let bonus_controller = if (subj_low == "one or more tokens"
        && would_norm == "be created under your control")
        || (subj_low == "an effect" && would_norm == "create one or more tokens under your control")
        || (subj_low == "you" && would_norm == "create one or more tokens")
    {
        Some(PlayerRelation::Controller)
    } else if (subj_low == "one or more tokens" && would_norm == "be created")
        || (subj_low == "an effect" && would_norm == "create one or more tokens")
    {
        Some(PlayerRelation::AnyParticipant)
    } else {
        None
    };
    if let Some(controller) = bonus_controller {
        let trimmed = instead_norm
            .strip_prefix("you ")
            .or_else(|| instead_norm.strip_prefix("it "))
            .unwrap_or(instead_norm);
        let trimmed = trimmed
            .strip_prefix("creates ")
            .or_else(|| trimmed.strip_prefix("create "))
            .unwrap_or(trimmed);
        let trimmed = trimmed.strip_suffix(" are created").unwrap_or(trimmed);
        if let Some(bonus_raw) = trimmed
            .strip_prefix("those tokens plus ")
            .or_else(|| trimmed.strip_prefix("that many tokens plus "))
        {
            let (per_created, token_phrase) =
                if let Some(rest) = bonus_raw.strip_prefix("that many ") {
                    let singular = rest
                        .strip_suffix(" tokens")
                        .map(|s| format!("a {s} token"))
                        .unwrap_or_else(|| format!("a {rest}"));
                    (true, singular)
                } else if let Some(rest) = bonus_raw
                    .strip_prefix("an additional ")
                    .or_else(|| bonus_raw.strip_prefix("additional "))
                {
                    (false, format!("a {rest}"))
                } else {
                    (false, bonus_raw.to_string())
                };
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            if let Ok(tok_ast) = spellwake_oracle_compiler::filter::parse_token_creation(
                &token_phrase,
                &compiler_vocab,
            ) && let TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Creature { count, .. } = &tok_ast
                && let ValueExprAst::Const(n) = count.as_ref()
                && let Ok(count) = u8::try_from(*n)
                && count > 0
                && let Ok((creation, false)) = lower_token_spec(
                    &tok_ast,
                    TokenRecipients::Controller,
                    vocabulary,
                    ids,
                    None,
                    None,
                )
                && !creation.enters_tapped
                && creation.base_size.is_none()
            {
                return Some(push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::BonusTokenOnCreate {
                            controller,
                            token: Box::new(creation.definition),
                            count,
                            per_created,
                        },
                    },
                ));
            }
        }
    }
    if (subj_low == "an effect"
        && would_norm == "create one or more tokens"
        && instead_norm == "it creates twice that many of those tokens")
        || (subj_low == "one or more tokens"
            && would_norm == "be created"
            && instead_norm == "twice that many of those tokens are created")
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyTokens {
                    controller: PlayerRelation::AnyParticipant,
                    add: 0,
                    multiply: 2,
                },
            },
        ));
    }

    let parse_token_artifact_subtype = |w: &str| -> Option<ArtifactSubtype> {
        match w.trim() {
            "treasure" => Some(ArtifactSubtype::Treasure),
            "food" => Some(ArtifactSubtype::Food),
            "clue" => Some(ArtifactSubtype::Clue),
            "blood" => Some(ArtifactSubtype::Blood),
            "map" => Some(ArtifactSubtype::Map),
            "powerstone" => Some(ArtifactSubtype::Powerstone),
            _ => None,
        }
    };
    if subj_low == "you"
        && let Some(sub_word) = would_norm
            .strip_prefix("create one or more ")
            .and_then(|s| s.strip_suffix(" tokens"))
        && let Some(subtype) = parse_token_artifact_subtype(sub_word)
    {
        let sub_trim = sub_word.trim();
        let active_instead = instead_norm.strip_prefix("you ").unwrap_or(instead_norm);
        let mod_pair = if active_instead
            == format!("create those tokens plus an additional {sub_trim} token")
            || active_instead == format!("create that many {sub_trim} tokens plus one")
        {
            Some((1u8, 1u8))
        } else if active_instead == format!("create twice that many {sub_trim} tokens")
            || active_instead == "create twice that many of those tokens"
        {
            Some((0u8, 2u8))
        } else {
            None
        };
        if let Some((add, multiply)) = mod_pair {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::ModifyTokenSubtype {
                        subtype,
                        controller: PlayerRelation::Controller,
                        add,
                        multiply,
                    },
                },
            ));
        }
    }
    if let Some(sub_word) = subj_low
        .strip_prefix("one or more ")
        .and_then(|s| s.strip_suffix(" tokens"))
        && would_norm == "be created under your control"
        && let Some(subtype) = parse_token_artifact_subtype(sub_word)
    {
        let sub_trim = sub_word.trim();
        let mod_pair = if instead_norm
            == format!("those tokens plus an additional {sub_trim} token are created")
            || instead_norm == format!("that many {sub_trim} tokens plus one are created")
        {
            Some((1u8, 1u8))
        } else if instead_norm == "twice that many of those tokens are created"
            || instead_norm == format!("twice that many {sub_trim} tokens are created")
        {
            Some((0u8, 2u8))
        } else {
            None
        };
        if let Some((add, multiply)) = mod_pair {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::ModifyTokenSubtype {
                        subtype,
                        controller: PlayerRelation::Controller,
                        add,
                        multiply,
                    },
                },
            ));
        }
    }
    if let Some(rule) = lower_filtered_or_steal_token_replacement(
        &subj_low,
        would_norm,
        instead_norm,
        vocabulary,
        ids,
    ) {
        let ab = StaticContinuousAbility::Modifier {
            id: ids.static_id(),
            rule,
        };
        if ab.validate(vocabulary).is_ok() {
            return Some(push_static_ability(program, ab));
        }
    }
    if (subj_low == "an opponent"
        && would_norm.starts_with("put one or more counters on ")
        && instead_norm.contains("half that many"))
        || (subj_low == "one or more counters"
            && would_norm.contains("opponent")
            && instead_norm.contains("half that many"))
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::HalveOpponentCounters,
            },
        ));
    }

    // 5. Counter Placement Modifiers (CR 614.1a)
    if (subj_low == "an effect"
        && matches!(
            would_norm,
            "put one or more counters on a permanent you control"
                | "place one or more counters on a permanent you control"
        )
        && matches!(
            instead_norm,
            "it puts twice that many of those counters on that permanent"
                | "it places twice that many of those counters on that permanent"
        ))
        || (subj_low == "one or more counters"
            && would_norm == "be put on a permanent you control"
            && matches!(
                instead_norm,
                "twice that many of those counters are put on that permanent"
                    | "put twice that many of those counters on that permanent"
            ))
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::DoublePlacedCounters,
            },
        ));
    }
    if subj_low == "one or more +1/+1 counters"
        && matches!(
            would_norm,
            "be put on ~" | "be put on this creature" | "be put on this permanent"
        )
        && matches!(
            instead_norm,
            "that many plus one +1/+1 counters are put on it"
                | "that many plus one +1/+1 counters are put on ~"
                | "put that many plus one +1/+1 counters on it"
                | "put that many plus one +1/+1 counters on ~"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::AdditionalSelfPlusOneCounter,
            },
        ));
    }
    if subj_low == "one or more +1/+1 counters"
        && let Some(selector) = match would_norm {
            "be put on a creature you control" => Some(PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            }),
            "be put on another creature you control" => Some(PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                controller: ControllerSelector::SourceController,
                exclude_source: true,
                ..PermanentSelector::default()
            }),
            "be put on a nontoken creature you control" => Some(PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                controller: ControllerSelector::SourceController,
                token: Some(false),
                ..PermanentSelector::default()
            }),
            "be put on a permanent you control" => Some(PermanentSelector {
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            }),
            "be put on another permanent you control" => Some(PermanentSelector {
                controller: ControllerSelector::SourceController,
                exclude_source: true,
                ..PermanentSelector::default()
            }),
            "be put on a creature or planeswalker you control" => Some(PermanentSelector {
                any_types: BTreeSet::from([CardType::Creature, CardType::Planeswalker]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            }),
            _ => None,
        }
    {
        let mod_rule = match instead_norm {
            "that many plus one +1/+1 counters are put on it"
            | "that many plus one +1/+1 counters are put on that creature"
            | "that many plus one +1/+1 counters are put on that permanent"
            | "put that many plus one +1/+1 counters on it"
            | "put that many plus one +1/+1 counters on that creature"
            | "put that many plus one +1/+1 counters on that permanent" => {
                Some(StaticModifierRule::ModifyCounters {
                    selector: Box::new(selector),
                    counter_kind: Some(CounterKind::PlusOne),
                    add: 1,
                    multiply: 1,
                })
            }
            "twice that many +1/+1 counters are put on that creature"
            | "twice that many +1/+1 counters are put on that permanent"
            | "twice that many +1/+1 counters are put on it"
            | "put twice that many +1/+1 counters on that creature"
            | "put twice that many +1/+1 counters on that permanent"
            | "put twice that many +1/+1 counters on it" => {
                Some(StaticModifierRule::ModifyCounters {
                    selector: Box::new(selector),
                    counter_kind: Some(CounterKind::PlusOne),
                    add: 0,
                    multiply: 2,
                })
            }
            _ => None,
        };
        if let Some(rule) = mod_rule {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule,
                },
            ));
        }
    }
    if subj_low == "one or more +1/+1 counters"
        && would_norm == "be put on a creature"
        && matches!(
            instead_norm,
            "twice that many +1/+1 counters are put on that creature"
                | "twice that many +1/+1 counters are put on it"
                | "put twice that many +1/+1 counters on that creature"
        )
    {
        let any_creatures = PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            controller: ControllerSelector::Any,
            ..PermanentSelector::default()
        };
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyCounters {
                    selector: Box::new(any_creatures),
                    counter_kind: Some(CounterKind::PlusOne),
                    add: 0,
                    multiply: 2,
                },
            },
        ));
    }
    if subj_low == "one or more counters"
        && matches!(
            would_norm,
            "be put on a permanent you control"
                | "be put on a permanent your team controls"
                | "be put on an artifact or creature you control"
        )
        && matches!(
            instead_norm,
            "that many plus one of each of those kinds of counters are put on that permanent"
                | "that many plus one of each of those kinds of counters are put on it"
                | "put that many plus one of each of those kinds of counters on that permanent"
        )
    {
        let selector = if would_norm.contains("artifact or creature") {
            PermanentSelector {
                any_types: BTreeSet::from([CardType::Artifact, CardType::Creature]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            }
        } else {
            PermanentSelector {
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            }
        };
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyCounters {
                    selector: Box::new(selector),
                    counter_kind: None,
                    add: 1,
                    multiply: 1,
                },
            },
        ));
    }

    // General Counter Placement Modifier fallback (CR 614.1a)
    let rejoined_counter = rejoin_counter_comma_split(would_norm, instead_norm);
    let (counter_would, counter_instead) = match &rejoined_counter {
        Some((w, i)) => (w.as_str(), *i),
        None => (would_norm, instead_norm),
    };
    if let Some(after_one) = subj_low.strip_prefix("one or more ")
        && let Some(kind_raw) = after_one
            .strip_suffix(" counters")
            .or_else(|| (after_one == "counters").then_some(""))
        && let Some(target_raw) = counter_would
            .strip_prefix("be put on ")
            .or_else(|| counter_would.strip_prefix("be placed on "))
        && !counter_instead.contains(" and ")
        && !counter_instead.contains(". ")
    {
        let counter_kind = if kind_raw.trim().is_empty() {
            None
        } else {
            parse_counter_kind(kind_raw.trim()).filter(|k| k.is_permanent())
        };
        let kind_ok = kind_raw.trim().is_empty() || counter_kind.is_some();
        let selectors = parse_counter_target_selectors(target_raw.trim(), vocabulary);
        if kind_ok && !selectors.is_empty() {
            let (add, multiply) = if counter_instead.contains("twice that many")
                || counter_instead.contains("double that many")
            {
                (0u8, 2u8)
            } else if counter_instead.contains("three times that many")
                || counter_instead.contains("triple that many")
            {
                (0u8, 3u8)
            } else if let Some((_, rhs)) = counter_instead.split_once("that many plus ")
                && let Some(n_tok) = rhs.split_whitespace().next()
                && let Some(n) = parse_small_num(n_tok)
                && n > 0
            {
                (n, 1u8)
            } else if counter_instead.contains("plus one additional")
                || counter_instead.contains("plus an additional")
            {
                (1u8, 1u8)
            } else {
                (0u8, 1u8)
            };
            if multiply > 1 || add > 0 {
                for selector in selectors {
                    if let Err(err) = push_static_ability(
                        program,
                        StaticContinuousAbility::Modifier {
                            id: ids.static_id(),
                            rule: StaticModifierRule::ModifyCounters {
                                selector: Box::new(selector),
                                counter_kind,
                                add,
                                multiply,
                            },
                        },
                    ) {
                        return Some(Err(err));
                    }
                }
                return Some(Ok(()));
            }
        }
    }

    // 6. Life Gain Modifiers (CR 614.1a)
    let parse_life_gain_mod = |s: &str| -> Option<(u8, u8)> {
        if s.contains(" and ") || s.contains(". ") {
            return None;
        }
        if s.contains("twice that much life") || s.contains("double that much life") {
            Some((0, 2))
        } else if s.contains("three times that much life") || s.contains("triple that much life") {
            Some((0, 3))
        } else if let Some((_, rhs)) = s
            .split_once("that much life plus ")
            .or_else(|| s.split_once("an additional "))
            && let Some(n_tok) = rhs.split_whitespace().next()
            && let Some(n) = parse_small_num(n_tok)
            && n > 0
        {
            Some((n, 1))
        } else {
            None
        }
    };
    if matches!(
        subj_low.as_str(),
        "you" | "a player" | "any player" | "an opponent"
    ) && let Some(cond_raw) = would_norm
        .strip_prefix("gain life while ")
        .or_else(|| would_norm.strip_prefix("gain life if "))
        && let Some((add, multiply)) = parse_life_gain_mod(instead_norm)
        && let Some(rc) = lower_world_resolution_condition(cond_raw, vocabulary)
    {
        let player = match subj_low.as_str() {
            "you" => PlayerRelation::Controller,
            "an opponent" => PlayerRelation::Opponent,
            _ => PlayerRelation::AnyParticipant,
        };
        let ability = StaticContinuousAbility::Modifier {
            id: ids.static_id(),
            rule: StaticModifierRule::ConditionalModifyLifeGain {
                condition: Box::new(rc),
                player,
                add,
                multiply,
            },
        };
        if ability.validate(vocabulary).is_ok() {
            return Some(push_static_ability(program, ability));
        }
    }
    if matches!(
        subj_low.as_str(),
        "you" | "a player" | "any player" | "an opponent"
    ) && would_norm == "gain life"
    {
        if matches!(
            instead_norm,
            "that player gains no life" | "they gain no life" | "you gain no life"
        ) {
            return match subj_low.as_str() {
                "a player" | "any player" => Some(push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PlayersCannotGainLife,
                    },
                )),
                "an opponent" => Some(push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OpponentsCannotGainLife,
                    },
                )),
                "you" => Some(push_static_ability(
                    program,
                    StaticContinuousAbility::EventReplacement {
                        id: ids.static_id(),
                        event: ReplaceableEvent::LifeGain {
                            player: AffectedPlayers::One(ResolvingPlayer::Controller),
                        },
                        condition: None,
                        action: StaticReplacementAction::Skip,
                    },
                )),
                _ => None,
            };
        }
        let player = match subj_low.as_str() {
            "you" => PlayerRelation::Controller,
            "an opponent" => PlayerRelation::Opponent,
            _ => PlayerRelation::AnyParticipant,
        };
        if let Some((add, multiply)) = parse_life_gain_mod(instead_norm) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::ModifyLifeGain {
                        player,
                        add,
                        multiply,
                        lose_instead: false,
                    },
                },
            ));
        }
    }
    if matches!(subj_low.as_str(), "an opponent" | "a player" | "any player")
        && would_norm == "gain life"
        && matches!(
            instead_norm,
            "that player loses that much life" | "they lose that much life"
        )
    {
        let player = if subj_low == "an opponent" {
            PlayerRelation::Opponent
        } else {
            PlayerRelation::AnyParticipant
        };
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyLifeGain {
                    player,
                    add: 0,
                    multiply: 1,
                    lose_instead: true,
                },
            },
        ));
    }

    // 7. Static Damage Replacement & Modification Rules (CR 614.1a, 616.1)
    if matches!(subj_low.as_str(), "damage" | "all damage" | "a source")
        && matches!(
            would_norm,
            "be dealt to you"
                | "be dealt to you or another permanent you control"
                | "deal damage to you"
                | "deal damage to you or another permanent you control"
        )
    {
        let include_other = would_norm.contains("another permanent you control");
        let rule_opt = if matches!(
            instead_norm,
            "it is dealt to ~"
                | "that damage is dealt to ~"
                | "deal that damage to ~"
                | "it deals that damage to ~"
        ) {
            Some(StaticDamageRule::RedirectPlayerDamageToSource {
                include_other_permanents: include_other,
            })
        } else if !include_other
            && matches!(
                instead_norm,
                "it is dealt to enchanted creature"
                    | "that damage is dealt to enchanted creature"
                    | "it is dealt to equipped creature"
                    | "that damage is dealt to equipped creature"
            )
        {
            Some(StaticDamageRule::RedirectPlayerDamageToEnchanted)
        } else {
            None
        };
        if let Some(rule) = rule_opt {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule,
                },
            ));
        }
    }
    if would_norm == "deal combat damage to a player"
        && matches!(
            instead_norm,
            "that player mills that many cards"
                | "that player puts that many cards from the top of their library into their graveyard"
        )
        && let Some(sub_phrase) = subj_low
            .strip_prefix("a ")
            .or_else(|| subj_low.strip_prefix("an "))
            .and_then(|s| s.strip_suffix(" you control"))
    {
        let title = sub_phrase
            .split('-')
            .map(|seg| {
                let mut ch = seg.chars();
                ch.next()
                    .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("-");
        if let Some(ct) = vocabulary.resolve_label(&title) {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::ReplaceSubtypeCombatWithMill(ct),
                },
            ));
        }
    }
    if would_norm == "deal_damage"
        && instead_norm == "double_damage"
        && let Some((source, source_controller)) = parse_damage_source_filter(&subj_low, vocabulary)
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: StaticDamageRule::ModifyDamage {
                    source,
                    source_controller,
                    recipient: StaticDamageRecipient::Any,
                    kind: StaticDamageKind::Any,
                    op: StaticDamageOp::Multiply(2),
                },
            },
        ));
    }
    if subj_low == "a source"
        && would_norm == "deal damage to an opponent or a permanent an opponent controls"
        && matches!(
            instead_norm,
            "it deals double that damage to that player or permanent"
                | "it deals twice that much damage to that player or permanent"
                | "that source deals double that damage to that player or permanent"
                | "that source deals twice that much damage to that player or permanent"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: StaticDamageRule::DoubleToOpponents,
            },
        ));
    }
    if subj_low == "a source"
        && would_norm == "deal damage to enchanted player"
        && matches!(
            instead_norm,
            "it deals double that damage to that player"
                | "it deals double that damage to enchanted player"
                | "it deals twice that much damage to that player"
                | "that source deals double that damage to that player"
                | "that source deals double that damage to enchanted player"
                | "that source deals twice that much damage to that player"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: StaticDamageRule::DoubleToEnchantedPlayer,
            },
        ));
    }
    if let Some(after_battle_opp) = instead_norm.strip_prefix("battle, or opponent, ")
        && matches!(
            would_norm,
            "deal damage to a creature"
                | "deal combat damage to a creature"
                | "deal noncombat damage to a creature"
        )
        && let Some(mut base_rule) =
            parse_modify_damage_event(&subj_low, would_norm, after_battle_opp.trim(), vocabulary)
        && let StaticDamageRule::ModifyDamage {
            ref mut recipient, ..
        } = base_rule
    {
        *recipient = StaticDamageRecipient::MatchingPermanent(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::BATTLE),
        ));
        let mut opp_rule = base_rule.clone();
        if let StaticDamageRule::ModifyDamage {
            ref mut recipient, ..
        } = opp_rule
        {
            *recipient = StaticDamageRecipient::Opponents;
        }
        if let Err(err) = push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: base_rule,
            },
        ) {
            return Some(Err(err));
        }
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule: opp_rule,
            },
        ));
    }
    if let Some(rules) =
        parse_damage_rules_full(None, &subj_low, would_norm, instead_norm, vocabulary, ids)
    {
        for rule in rules {
            if let Err(err) = push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule,
                },
            ) {
                return Some(Err(err));
            }
        }
        return Some(Ok(()));
    }
    if let Some(rule) = parse_redirect_damage_event(&subj_low, would_norm, instead_norm, vocabulary)
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule,
            },
        ));
    }

    // 8. Generalized Event Replacements (CR 614.1a)
    lower_event_replacement_ability(
        &subj_low,
        would_norm,
        &instead_raw,
        instead_norm,
        program,
        vocabulary,
        ids,
    )
}

fn parse_small_num(tok: &str) -> Option<u8> {
    match tok.trim() {
        "one" | "1" => Some(1),
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        "seven" | "7" => Some(7),
        "eight" | "8" => Some(8),
        "nine" | "9" => Some(9),
        "ten" | "10" => Some(10),
        other => other.parse::<u8>().ok(),
    }
}

fn lower_world_resolution_condition(
    cond_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ResolutionCondition> {
    let trimmed = cond_raw.trim();
    if trimmed == "your library has no cards in it" {
        return Some(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                ResolvingPlayer::Controller,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        });
    }
    let cond_ast = spellwake_oracle_compiler::filter::parse_condition(
        trimmed,
        &cached_compiler_vocab(vocabulary),
    )
    .ok()?;
    let mut tmp_builder = BodyBuilder::new();
    let rc =
        lower_resolution_condition(&cond_ast, &mut tmp_builder, vocabulary, false, None).ok()?;
    if tmp_builder.target != BodyTarget::None
        || !rc.is_world_evaluable()
        || rc.validate_world(vocabulary).is_err()
    {
        return None;
    }
    Some(rc)
}

fn resolve_creature_subtype_token(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CreatureTypeId> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut cap_next = true;
    for ch in trimmed.chars() {
        if ch == ' ' || ch == '-' {
            out.push(ch);
            cap_next = true;
        } else if cap_next {
            for up in ch.to_uppercase() {
                out.push(up);
            }
            cap_next = false;
        } else {
            out.push(ch);
        }
    }
    vocabulary.resolve_label(&out)
}

fn parse_creature_subtype_list(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<CreatureTypeId>> {
    if !raw.contains(", ") && !raw.contains(" or ") {
        return None;
    }
    let mut subtypes = Vec::new();
    for comma_part in raw.split(',') {
        let part = comma_part.trim();
        if part.is_empty() {
            return None;
        }
        for or_part in part.split(" or ") {
            let tok = or_part
                .trim()
                .strip_prefix("or ")
                .unwrap_or(or_part.trim())
                .trim();
            let tok = tok
                .strip_prefix("an ")
                .or_else(|| tok.strip_prefix("a "))
                .unwrap_or(tok)
                .trim();
            let ct = resolve_creature_subtype_token(tok, vocabulary)?;
            if !subtypes.contains(&ct) {
                subtypes.push(ct);
            }
        }
    }
    if subtypes.len() >= 2 {
        Some(subtypes)
    } else {
        None
    }
}

fn rejoin_counter_comma_split<'a>(
    would_norm: &str,
    instead_norm: &'a str,
) -> Option<(String, &'a str)> {
    if !would_norm.starts_with("be put on ") && !would_norm.starts_with("be placed on ") {
        return None;
    }
    let markers = [
        ", that many ",
        ", twice that many ",
        ", double that many ",
        ", three times that many ",
        ", triple that many ",
        ", put ",
        ", you put ",
        ", instead ",
    ];
    let split_idx = markers.iter().filter_map(|m| instead_norm.find(m)).min()?;
    let tail_target = instead_norm[..split_idx].trim();
    if tail_target.is_empty() || tail_target.contains(". ") {
        return None;
    }
    let real_instead = instead_norm[split_idx + 2..].trim();
    Some((format!("{would_norm}, {tail_target}"), real_instead))
}

fn parse_artifact_subtype_ci(raw: &str) -> Option<ArtifactSubtype> {
    let parse_one = |s: &str| match s {
        "attraction" => Some(ArtifactSubtype::Attraction),
        "blood" => Some(ArtifactSubtype::Blood),
        "bobblehead" => Some(ArtifactSubtype::Bobblehead),
        "book" => Some(ArtifactSubtype::Book),
        "clue" => Some(ArtifactSubtype::Clue),
        "contraption" => Some(ArtifactSubtype::Contraption),
        "equipment" => Some(ArtifactSubtype::Equipment),
        "food" => Some(ArtifactSubtype::Food),
        "fortification" => Some(ArtifactSubtype::Fortification),
        "gold" => Some(ArtifactSubtype::Gold),
        "incubator" => Some(ArtifactSubtype::Incubator),
        "junk" => Some(ArtifactSubtype::Junk),
        "lander" => Some(ArtifactSubtype::Lander),
        "map" => Some(ArtifactSubtype::Map),
        "mutagen" => Some(ArtifactSubtype::Mutagen),
        "powerstone" => Some(ArtifactSubtype::Powerstone),
        "shard" => Some(ArtifactSubtype::Shard),
        "spacecraft" => Some(ArtifactSubtype::Spacecraft),
        "treasure" => Some(ArtifactSubtype::Treasure),
        "vehicle" => Some(ArtifactSubtype::Vehicle),
        _ => None,
    };
    let trimmed = raw.trim();
    parse_one(trimmed).or_else(|| trimmed.strip_suffix('s').and_then(parse_one))
}

fn parse_land_subtype_ci(raw: &str) -> Option<LandType> {
    let parse_one = |s: &str| match s {
        "plains" => Some(LandType::Plains),
        "island" => Some(LandType::Island),
        "swamp" => Some(LandType::Swamp),
        "mountain" => Some(LandType::Mountain),
        "forest" => Some(LandType::Forest),
        "cave" => Some(LandType::Cave),
        "desert" => Some(LandType::Desert),
        "gate" => Some(LandType::Gate),
        "lair" => Some(LandType::Lair),
        "locus" => Some(LandType::Locus),
        "mine" => Some(LandType::Mine),
        "planet" => Some(LandType::Planet),
        "power-plant" => Some(LandType::PowerPlant),
        "sphere" => Some(LandType::Sphere),
        "tower" => Some(LandType::Tower),
        "town" => Some(LandType::Town),
        "urza's" => Some(LandType::Urzas),
        _ => None,
    };
    let trimmed = raw.trim();
    parse_one(trimmed).or_else(|| trimmed.strip_suffix('s').and_then(parse_one))
}

fn parse_counter_target_disjunction(
    core_noun: &str,
    controller: ControllerSelector,
    exclude_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<PermanentSelector>> {
    if !core_noun.contains(", ") && !core_noun.contains(" or ") && !core_noun.contains(" and/or ") {
        return None;
    }
    let (token, list_raw) = if let Some(rest) = core_noun.strip_prefix("nontoken ") {
        (Some(false), rest.trim())
    } else if let Some(rest) = core_noun.strip_prefix("token ") {
        (Some(true), rest.trim())
    } else {
        (None, core_noun)
    };
    let mut card_types = BTreeSet::new();
    let mut creature_subtypes = BTreeSet::new();
    let mut artifact_subtypes = BTreeSet::new();
    let mut land_subtypes = BTreeSet::new();
    let mut atom_count: usize = 0;

    for comma_part in list_raw.split(',') {
        let part = comma_part.trim();
        if part.is_empty() {
            return None;
        }
        for and_or_part in part.split(" and/or ") {
            for or_part in and_or_part.split(" or ") {
                let mut atom = or_part.trim();
                atom = atom
                    .strip_prefix("and/or ")
                    .or_else(|| atom.strip_prefix("or "))
                    .unwrap_or(atom)
                    .trim();
                atom = atom
                    .strip_prefix("an ")
                    .or_else(|| atom.strip_prefix("a "))
                    .unwrap_or(atom)
                    .trim();
                if atom.is_empty() {
                    return None;
                }
                match atom {
                    "creature" | "creatures" => {
                        card_types.insert(CardType::Creature);
                    }
                    "artifact" | "artifacts" => {
                        card_types.insert(CardType::Artifact);
                    }
                    "enchantment" | "enchantments" => {
                        card_types.insert(CardType::Enchantment);
                    }
                    "land" | "lands" => {
                        card_types.insert(CardType::Land);
                    }
                    "planeswalker" | "planeswalkers" => {
                        card_types.insert(CardType::Planeswalker);
                    }
                    "battle" | "battles" => {
                        card_types.insert(CardType::Battle);
                    }
                    other => {
                        if let Some(at) = parse_artifact_subtype_ci(other) {
                            artifact_subtypes.insert(at);
                        } else if let Some(lt) = parse_land_subtype_ci(other) {
                            land_subtypes.insert(lt);
                        } else {
                            let ct = resolve_creature_subtype_token(other, vocabulary)?;
                            creature_subtypes.insert(ct);
                        }
                    }
                }
                atom_count += 1;
            }
        }
    }
    if atom_count < 2 {
        return None;
    }
    // Avoid double-applying counter modifiers if creature subtypes could overlap artifact/land subtypes
    // without a Creature card-type selector absorbing creatures.
    if !creature_subtypes.is_empty()
        && (!artifact_subtypes.is_empty() || !land_subtypes.is_empty())
        && !card_types.contains(&CardType::Creature)
    {
        return None;
    }

    let mut selectors = Vec::new();
    if !card_types.is_empty() {
        let mut sel = PermanentSelector {
            token,
            controller,
            exclude_source,
            ..PermanentSelector::default()
        };
        if card_types.len() == 1 {
            sel.required_types = card_types.clone();
        } else {
            sel.any_types = card_types.clone();
        }
        selectors.push(sel);
    }
    if !creature_subtypes.is_empty() {
        let mut sel = PermanentSelector {
            token,
            forbidden_types: card_types.clone(),
            controller,
            exclude_source,
            ..PermanentSelector::default()
        };
        if let Some(&only) = creature_subtypes
            .iter()
            .next()
            .filter(|_| creature_subtypes.len() == 1)
        {
            sel.creature_subtype = Some(only);
        } else {
            sel.any_creature_subtypes = creature_subtypes;
        }
        selectors.push(sel);
    }
    if !card_types.contains(&CardType::Artifact) {
        for at in artifact_subtypes {
            selectors.push(PermanentSelector {
                token,
                artifact_subtype: Some(at),
                forbidden_types: card_types.clone(),
                controller,
                exclude_source,
                ..PermanentSelector::default()
            });
        }
    }
    if !card_types.contains(&CardType::Land) {
        for lt in land_subtypes {
            selectors.push(PermanentSelector {
                token,
                land_subtype: Some(lt),
                forbidden_types: card_types.clone(),
                controller,
                exclude_source,
                ..PermanentSelector::default()
            });
        }
    }
    if selectors.is_empty() || selectors.iter().any(|s| s.validate(vocabulary).is_err()) {
        return None;
    }
    Some(selectors)
}

fn parse_counter_target_selectors(
    target_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Vec<PermanentSelector> {
    let trimmed = target_raw.trim();
    if matches!(
        trimmed,
        "~" | "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
    ) {
        return vec![PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }];
    }
    if matches!(
        trimmed,
        "enchanted creature" | "equipped creature" | "enchanted permanent"
    ) {
        return vec![PermanentSelector {
            attached_to_source: true,
            ..PermanentSelector::default()
        }];
    }

    let (controller, without_ctrl) = if let Some(rest) = trimmed
        .strip_suffix(" you control")
        .or_else(|| trimmed.strip_suffix(" your team controls"))
    {
        (ControllerSelector::SourceController, rest.trim())
    } else if let Some(rest) = trimmed
        .strip_suffix(" an opponent controls")
        .or_else(|| trimmed.strip_suffix(" opponents control"))
        .or_else(|| trimmed.strip_suffix(" you don't control"))
    {
        (ControllerSelector::Opponents, rest.trim())
    } else {
        (ControllerSelector::Any, trimmed)
    };
    let (exclude_source, core_noun) = if let Some(rest) = without_ctrl.strip_prefix("another ") {
        (true, rest.trim())
    } else if let Some(rest) = without_ctrl
        .strip_prefix("an ")
        .or_else(|| without_ctrl.strip_prefix("a "))
    {
        (false, rest.trim())
    } else {
        (false, without_ctrl)
    };
    if let Some(selectors) =
        parse_counter_target_disjunction(core_noun, controller, exclude_source, vocabulary)
    {
        return selectors;
    }

    let target_norm = trimmed.replace(" your team controls", " you control");
    if target_norm.contains("of the chosen")
        || target_norm.contains("player")
        || target_norm.starts_with("you")
    {
        return Vec::new();
    }
    let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
        &target_norm,
        &cached_compiler_vocab(vocabulary),
    ) else {
        return Vec::new();
    };
    let Ok(sel) = lower_filter_to_permanent_selector(&ast_f, vocabulary) else {
        return Vec::new();
    };
    if sel.validate(vocabulary).is_ok() {
        vec![sel]
    } else {
        Vec::new()
    }
}

fn parse_static_damage_recipient(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    match raw.trim() {
        "a permanent or player"
        | "a permanent, player, or battle"
        | "a creature, planeswalker, or player"
        | "a creature, planeswalker, battle, or player"
        | "a creature or player"
        | "any target" => Some(StaticDamageRecipient::Any),
        "an opponent or a permanent an opponent controls"
        | "an opponent or a permanent they control" => {
            Some(StaticDamageRecipient::OpponentOrOpponentPermanent)
        }
        "an opponent" => Some(StaticDamageRecipient::Opponents),
        "you" => Some(StaticDamageRecipient::Controller),
        "you or a permanent you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledPermanent)
        }
        "you or a creature you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledCreatures)
        }
        "a creature you control" => Some(StaticDamageRecipient::ControlledCreatures),
        "a creature an opponent controls" => Some(StaticDamageRecipient::OpponentCreatures),
        "a creature" => Some(StaticDamageRecipient::Creatures),
        "a permanent you control" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::permanent().controlled(),
        )),
        "another permanent you control" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::permanent()
                .controlled()
                .other_than_ability_source(),
        )),
        "a permanent an opponent controls" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::permanent().opposing(),
        )),
        "a permanent" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::permanent(),
        )),
        "a planeswalker you control" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::planeswalker().controlled(),
        )),
        "a planeswalker an opponent controls" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::planeswalker().opposing(),
        )),
        "a planeswalker" => Some(StaticDamageRecipient::MatchingPermanent(
            ObjectFilter::planeswalker(),
        )),
        "a player" => Some(StaticDamageRecipient::Players),
        "~" | "this creature" | "this permanent" | "this artifact" | "this enchantment" => {
            Some(StaticDamageRecipient::SelfObject)
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent" => {
            Some(StaticDamageRecipient::AttachedHost)
        }
        other => {
            if other.contains("of the chosen") || other.contains(" card") {
                return None;
            }
            let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            if ast_f.zone.is_some() || ast_f.any_spell {
                return None;
            }
            let obj = lower_object_filter(&ast_f, vocabulary).ok()?;
            Some(StaticDamageRecipient::MatchingPermanent(obj))
        }
    }
}

fn parse_redirect_damage_event(
    subj_low: &str,
    would_norm: &str,
    instead_norm: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRule> {
    let (effective_would, dest_str) = if would_norm == "would_event" {
        let (w_part, d_part) = instead_norm.split_once(" is dealt to ")?;
        (w_part.trim(), d_part.trim())
    } else {
        let d_part = instead_norm
            .strip_prefix("it is dealt to ")
            .or_else(|| instead_norm.strip_prefix("it's dealt to "))
            .or_else(|| instead_norm.strip_prefix("that damage is dealt to "))
            .or_else(|| instead_norm.strip_prefix("all of that damage is dealt to "))
            .or_else(|| instead_norm.strip_prefix("that source deals that damage to "))
            .or_else(|| instead_norm.strip_prefix("that source deals that much damage to "))
            .or_else(|| instead_norm.strip_prefix("it deals that damage to "))
            .or_else(|| instead_norm.strip_prefix("it deals that much damage to "))
            .or_else(|| instead_norm.strip_prefix("deal that damage to "))
            .or_else(|| instead_norm.strip_prefix("is dealt to "))?;
        (would_norm, d_part.trim())
    };
    let is_passive_would = effective_would.starts_with("be dealt to ");
    let to = match dest_str {
        "~" | "this creature" | "this permanent" => DamageRedirectTarget::AbilitySource,
        "enchanted creature" | "equipped creature" | "enchanted permanent" => {
            DamageRedirectTarget::AttachedHost
        }
        "you" => DamageRedirectTarget::Controller,
        "that source's controller" => DamageRedirectTarget::DamageSourceController,
        "its controller" if !is_passive_would => DamageRedirectTarget::DamageSourceController,
        _ => return None,
    };

    let (recipient_raw, would_combat) =
        if let Some(rest) = effective_would.strip_prefix("be dealt to ") {
            (rest.trim(), false)
        } else if let Some(rest) = effective_would.strip_prefix("deal combat damage to ") {
            (rest.trim(), true)
        } else if let Some(rest) = effective_would.strip_prefix("deal noncombat damage to ") {
            (rest.trim(), false)
        } else {
            let rest = effective_would.strip_prefix("deal damage to ")?;
            (rest.trim(), false)
        };

    let (mut source, subj_combat): (Option<ObjectFilter>, bool) = match subj_low {
        "damage" | "all damage" | "a source" => (None, false),
        "combat damage" | "all combat damage" => (None, true),
        "noncombat damage" | "all noncombat damage" => (None, false),
        "a source you control" => (Some(ObjectFilter::any().controlled()), false),
        "a source an opponent controls" | "a source you don't control" => {
            (Some(ObjectFilter::any().opposing()), false)
        }
        "a white source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::White))),
            false,
        ),
        "a blue source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Blue))),
            false,
        ),
        "a black source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Black))),
            false,
        ),
        "a red source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red))),
            false,
        ),
        "a green source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Green))),
            false,
        ),
        "~" | "this creature" | "this permanent" => (Some(ObjectFilter::self_object()), false),
        "enchanted creature" | "equipped creature" => (
            Some(ObjectFilter::Clause(ObjectClause {
                source_relation: SourceRelation::AttachedToSelf,
                ..ObjectClause::any()
            })),
            false,
        ),
        other => {
            if other.contains("of the chosen") {
                return None;
            }
            let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            if ast_f.any_spell && ast_f.any_types.is_empty() {
                return None;
            }
            if is_unrestricted_filter(&ast_f) {
                (None, false)
            } else {
                (Some(lower_object_filter(&ast_f, vocabulary).ok()?), false)
            }
        }
    };

    let recipient_clean = if source.is_none()
        && let Some((recip_part, by_src)) = recipient_raw.split_once(" by ")
    {
        let by_trim = by_src.trim();
        if by_trim.contains("of the chosen") {
            return None;
        }
        let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
            by_trim,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if ast_f.any_spell && ast_f.any_types.is_empty() {
            return None;
        }
        if !is_unrestricted_filter(&ast_f) {
            source = Some(lower_object_filter(&ast_f, vocabulary).ok()?);
        }
        recip_part.trim()
    } else {
        recipient_raw
    };

    let from = if matches!(
        recipient_clean,
        "you or another permanent you control" | "you and other permanents you control"
    ) && to == DamageRedirectTarget::AbilitySource
    {
        StaticDamageRecipient::ControllerOrControlledPermanent
    } else if matches!(
        recipient_clean,
        "you or another creature you control" | "you and other creatures you control"
    ) && to == DamageRedirectTarget::AbilitySource
    {
        StaticDamageRecipient::ControllerOrControlledCreatures
    } else {
        parse_static_damage_recipient(recipient_clean, vocabulary)?
    };

    Some(StaticDamageRule::RedirectDamage {
        source,
        from,
        to,
        combat_only: subj_combat || would_combat,
    })
}

fn lower_event_replacement_ability(
    subj_low: &str,
    would_norm: &str,
    instead_raw: &str,
    instead_norm: &str,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Result<(), &'static str>> {
    let affected_player = match subj_low {
        "you" => Some(AffectedPlayers::One(ResolvingPlayer::Controller)),
        "an opponent" => Some(AffectedPlayers::EachOpponent),
        "a player" | "any player" => Some(AffectedPlayers::EachPlayer),
        _ => None,
    };

    // Extra turn skip (e.g. Gerrard's Hourglass Pendant, Ugin's Nexus, Stranglehold)
    if let Some(player) = affected_player
        && matches!(would_norm, "begin an extra turn" | "take an extra turn")
        && matches!(
            instead_norm,
            "that player skips that turn"
                | "they skip that turn"
                | "you skip that turn"
                | "skip that turn"
                | "that player skips that extra turn"
                | "they skip that extra turn"
                | "you skip that extra turn"
                | "skip that extra turn"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::EventReplacement {
                id: ids.static_id(),
                event: ReplaceableEvent::ExtraTurn { player },
                condition: None,
                action: StaticReplacementAction::Skip,
            },
        ));
    }

    // Explore doubler (e.g. Topography Tracker)
    if would_norm == "explore"
        && (instead_norm.contains("explores, then it explores again")
            || instead_norm.contains("explores twice")
            || instead_norm.contains("explore twice"))
        && !instead_norm.contains(" and ")
        && !instead_norm.contains(". ")
    {
        let filter_opt = match subj_low {
            "a creature you control" => Some(ObjectFilter::creature().controlled()),
            "a creature" => Some(ObjectFilter::creature()),
            "a creature an opponent controls" => Some(ObjectFilter::creature().opposing()),
            "~" | "this creature" => Some(ObjectFilter::self_object()),
            other => spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()
            .and_then(|ast_f| lower_object_filter(&ast_f, vocabulary).ok()),
        };
        if let Some(filter) = filter_opt {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::Explore { filter },
                    condition: None,
                    action: StaticReplacementAction::ModifyAmount {
                        add: Box::new(ValueExpr::literal(0)),
                        multiply: 2,
                    },
                },
            ));
        }
    }

    // Life loss skip or amount modifier (including turn/world conditions like Bloodletter of Aclazotz)
    if let Some(player) = affected_player
        && !instead_norm.contains(" and ")
        && !instead_norm.contains(". ")
    {
        let life_loss_cond: Option<Option<ResolutionCondition>> = if would_norm == "lose life" {
            Some(None)
        } else if would_norm == "lose life during your turn" {
            Some(Some(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }))
        } else if would_norm == "lose life during an opponent's turn" {
            Some(Some(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }))
        } else if let Some(cond_raw) = would_norm
            .strip_prefix("lose life while ")
            .or_else(|| would_norm.strip_prefix("lose life if "))
        {
            lower_world_resolution_condition(cond_raw, vocabulary).map(Some)
        } else {
            None
        };
        if let Some(cond_opt) = life_loss_cond {
            let action_opt = if instead_norm.contains("prevent")
                || instead_norm.contains("loses no life")
                || instead_norm.contains("lose no life")
                || instead_norm.contains("don't lose")
                || instead_norm.contains("doesn't lose")
            {
                Some(StaticReplacementAction::Skip)
            } else if instead_norm.contains("twice that much life")
                || instead_norm.contains("double that much life")
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(0)),
                    multiply: 2,
                })
            } else if instead_norm.contains("three times that much life")
                || instead_norm.contains("triple that much life")
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(0)),
                    multiply: 3,
                })
            } else if let Some((_, plus_str)) = instead_norm.split_once("that much life plus ")
                && let Some(n_tok) = plus_str.split_whitespace().next()
                && let Some(n) = parse_small_num(n_tok)
                && n > 0
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(i64::from(n))),
                    multiply: 1,
                })
            } else {
                None
            };
            if let Some(action) = action_opt {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::LifeLoss { player },
                    condition: cond_opt
                        .map(|rc| Box::new(StaticSourceCondition::Eval(Box::new(rc)))),
                    action,
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
            } else if let Some((optional, body)) =
                lower_inline_replacement_body(instead_raw, instead_norm, vocabulary, ids)
            {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::LifeLoss { player },
                    condition: cond_opt
                        .map(|rc| Box::new(StaticSourceCondition::Eval(Box::new(rc)))),
                    action: StaticReplacementAction::ReplaceWithBody {
                        optional,
                        body: Box::new(body),
                    },
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
            }
        }
    }

    // Life gain skip or body replacement (CR 614.1a)
    if let Some(player) = affected_player {
        let life_gain_cond: Option<Option<ResolutionCondition>> = if would_norm == "gain life" {
            Some(None)
        } else if would_norm == "gain life during your turn" {
            Some(Some(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }))
        } else if would_norm == "gain life during an opponent's turn" {
            Some(Some(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }))
        } else if let Some(cond_raw) = would_norm
            .strip_prefix("gain life while ")
            .or_else(|| would_norm.strip_prefix("gain life if "))
        {
            lower_world_resolution_condition(cond_raw, vocabulary).map(Some)
        } else {
            None
        };
        if let Some(cond_opt) = life_gain_cond {
            let condition = cond_opt.map(|rc| Box::new(StaticSourceCondition::Eval(Box::new(rc))));
            if instead_norm.contains("gains no life")
                || instead_norm.contains("gain no life")
                || instead_norm.contains("don't gain")
                || instead_norm.contains("doesn't gain")
            {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::LifeGain { player },
                    condition,
                    action: StaticReplacementAction::Skip,
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
            } else if let Some((optional, body)) =
                lower_inline_replacement_body(instead_raw, instead_norm, vocabulary, ids)
            {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::LifeGain { player },
                    condition,
                    action: StaticReplacementAction::ReplaceWithBody {
                        optional,
                        body: Box::new(body),
                    },
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
            }
        }
    }

    // Mill and scry amount modifiers (e.g. Bruvac the Grandiloquent)
    if let Some(player) = affected_player
        && !would_norm.contains(" while ")
        && !would_norm.contains(" during ")
        && !would_norm.contains(" if ")
        && !would_norm.contains(" unless ")
        && !instead_norm.contains(" and ")
        && !instead_norm.contains(". ")
    {
        let event_opt = if would_norm.starts_with("mill ") {
            Some(ReplaceableEvent::Mill { player })
        } else if would_norm == "scry" || would_norm.starts_with("scry ") {
            Some(ReplaceableEvent::Scry { player })
        } else {
            None
        };
        if let Some(event) = event_opt {
            let action_opt = if instead_norm.contains("twice that many")
                || instead_norm.contains("double that many")
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(0)),
                    multiply: 2,
                })
            } else if instead_norm.contains("three times that many")
                || instead_norm.contains("triple that many")
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(0)),
                    multiply: 3,
                })
            } else if let Some((_, rhs)) = instead_norm.split_once(" plus ")
                && let Some(n_tok) = rhs.split_whitespace().next()
                && let Some(n) = parse_small_num(n_tok)
                && n > 0
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(i64::from(n))),
                    multiply: 1,
                })
            } else if instead_norm.contains("additional card") {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(1)),
                    multiply: 1,
                })
            } else if let Some((_, rhs)) = instead_norm.split_once("additional ")
                && let Some(n_tok) = rhs.split_whitespace().next()
                && let Some(n) = parse_small_num(n_tok)
                && n > 0
                && rhs.contains("card")
            {
                Some(StaticReplacementAction::ModifyAmount {
                    add: Box::new(ValueExpr::literal(i64::from(n))),
                    multiply: 1,
                })
            } else {
                None
            };
            if let Some(action) = action_opt {
                return Some(push_static_ability(
                    program,
                    StaticContinuousAbility::EventReplacement {
                        id: ids.static_id(),
                        event,
                        condition: None,
                        action,
                    },
                ));
            } else if let Some((optional, body)) =
                lower_inline_replacement_body(instead_raw, instead_norm, vocabulary, ids)
            {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event,
                    condition: None,
                    action: StaticReplacementAction::ReplaceWithBody {
                        optional,
                        body: Box::new(body),
                    },
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
            }
        }
    }

    // Draw skip, conditional amount modifier, or body replacement (CR 614.1a)
    if let Some(player) = affected_player {
        let draw_spec: Option<(bool, Option<ResolutionCondition>)> = if would_norm == "draw a card"
        {
            Some((true, None))
        } else if would_norm == "draw one or more cards" {
            Some((false, None))
        } else if let Some(cond_raw) = would_norm
            .strip_prefix("draw a card while ")
            .or_else(|| would_norm.strip_prefix("draw a card if "))
        {
            lower_world_resolution_condition(cond_raw, vocabulary).map(|rc| (true, Some(rc)))
        } else if let Some(cond_raw) = would_norm
            .strip_prefix("draw one or more cards while ")
            .or_else(|| would_norm.strip_prefix("draw one or more cards if "))
        {
            lower_world_resolution_condition(cond_raw, vocabulary).map(|rc| (false, Some(rc)))
        } else {
            None
        };

        if let Some((is_single_draw, cond_opt)) = draw_spec {
            let condition = cond_opt.map(|rc| Box::new(StaticSourceCondition::Eval(Box::new(rc))));
            if matches!(
                instead_norm,
                "skip that draw"
                    | "you skip that draw"
                    | "that player skips that draw"
                    | "they skip that draw"
            ) {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::Draw { player },
                    condition,
                    action: StaticReplacementAction::Skip,
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
                return None;
            }
            if condition.is_some()
                && !instead_norm.contains(" and ")
                && !instead_norm.contains(". ")
            {
                let mod_action = if matches!(
                    instead_norm,
                    "draw twice that many cards"
                        | "you draw twice that many cards"
                        | "that player draws twice that many cards"
                        | "they draw twice that many cards"
                ) {
                    Some(StaticReplacementAction::ModifyAmount {
                        add: Box::new(ValueExpr::literal(0)),
                        multiply: 2,
                    })
                } else if matches!(
                    instead_norm,
                    "draw three times that many cards"
                        | "you draw three times that many cards"
                        | "that player draws three times that many cards"
                        | "they draw three times that many cards"
                ) {
                    Some(StaticReplacementAction::ModifyAmount {
                        add: Box::new(ValueExpr::literal(0)),
                        multiply: 3,
                    })
                } else if is_single_draw
                    && let Some(n) = instead_norm
                        .strip_prefix("you draw ")
                        .or_else(|| instead_norm.strip_prefix("that player draws "))
                        .or_else(|| instead_norm.strip_prefix("they draw "))
                        .or_else(|| instead_norm.strip_prefix("draw "))
                        .and_then(|r| r.strip_suffix(" cards"))
                        .and_then(parse_small_num)
                    && n >= 2
                {
                    Some(StaticReplacementAction::ModifyAmount {
                        add: Box::new(ValueExpr::literal(0)),
                        multiply: n,
                    })
                } else if let Some(n) = instead_norm
                    .strip_prefix("you draw that many cards plus ")
                    .or_else(|| {
                        instead_norm.strip_prefix("that player draws that many cards plus ")
                    })
                    .or_else(|| instead_norm.strip_prefix("they draw that many cards plus "))
                    .or_else(|| instead_norm.strip_prefix("draw that many cards plus "))
                    .and_then(parse_small_num)
                    && n > 0
                {
                    Some(StaticReplacementAction::ModifyAmount {
                        add: Box::new(ValueExpr::literal(i64::from(n))),
                        multiply: 1,
                    })
                } else {
                    None
                };
                if let Some(action) = mod_action {
                    let ability = StaticContinuousAbility::EventReplacement {
                        id: ids.static_id(),
                        event: ReplaceableEvent::Draw { player },
                        condition: condition.clone(),
                        action,
                    };
                    if ability.validate(vocabulary).is_ok() {
                        return Some(push_static_ability(program, ability));
                    }
                }
            }
            if is_single_draw
                && let Some((optional, body)) =
                    lower_inline_replacement_body(instead_raw, instead_norm, vocabulary, ids)
            {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::Draw { player },
                    condition,
                    action: StaticReplacementAction::ReplaceWithBody {
                        optional,
                        body: Box::new(body),
                    },
                };
                if ability.validate(vocabulary).is_ok() {
                    return Some(push_static_ability(program, ability));
                }
            }
        }
    }

    None
}

fn lower_inline_replacement_body(
    instead_raw: &str,
    instead_norm: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<(bool, ExecutableBody)> {
    let (optional, action_raw) = if let Some(rest) = instead_raw
        .strip_prefix("you may instead ")
        .or_else(|| instead_raw.strip_prefix("that player may instead "))
        .or_else(|| instead_raw.strip_prefix("they may instead "))
        .map(|s| s.trim_end_matches(" instead").trim())
    {
        (true, rest)
    } else if let Some(rest) = instead_norm
        .strip_prefix("you may ")
        .or_else(|| instead_norm.strip_prefix("that player may "))
        .or_else(|| instead_norm.strip_prefix("they may "))
    {
        (true, rest.trim())
    } else {
        (false, instead_norm)
    };
    let action_owned = action_raw
        .replace("this creature", "~")
        .replace("this permanent", "~")
        .replace("this artifact", "~")
        .replace("this enchantment", "~")
        .replace(" instead. ", ". ");
    let action_str = action_owned.trim();
    if action_str.is_empty()
        || action_str.contains("instead")
        || matches!(
            action_str,
            "skip that draw"
                | "you skip that draw"
                | "that player skips that draw"
                | "they skip that draw"
        )
    {
        return None;
    }
    let (ins, fails) = spellwake_oracle_compiler::clauses::parse_effect_body(
        action_str,
        &cached_compiler_vocab(vocabulary),
    );
    if !fails.is_empty() || ins.is_empty() {
        return None;
    }
    let mut body = lower_effect_body(&ins, vocabulary, ids, false, true).ok()?;
    if body.target != BodyTarget::None {
        return None;
    }
    for op in &mut body.instructions {
        match op {
            Instruction::ExileLibraryTop { player, amount } => {
                *op = Instruction::ExileTopPlay {
                    count: ValueExpr::literal(i64::from(*amount)),
                    library_owner: AffectedPlayers::One(*player),
                    duration: spellwake_engine::program::ImpulsePlayDuration::ThisTurn,
                    cast_only: false,
                    free: false,
                    any_color: false,
                    face_down: false,
                    grant_play: false,
                };
            }
            Instruction::ExileTopForPlay {
                until_next_from_source,
            } => {
                *op = Instruction::ExileTopPlay {
                    count: ValueExpr::literal(1),
                    library_owner: AffectedPlayers::One(ResolvingPlayer::Controller),
                    duration: if *until_next_from_source {
                        spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource
                    } else {
                        spellwake_engine::program::ImpulsePlayDuration::ThisTurn
                    },
                    cast_only: false,
                    free: false,
                    any_color: false,
                    face_down: false,
                    grant_play: true,
                };
            }
            _ => {}
        }
    }
    Some((optional, body))
}

fn parse_subtype_damage_source(
    other: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(Option<ObjectFilter>, PlayerRelation)> {
    let (ctrl_rel, without_ctrl) = if let Some(rest) = other.strip_suffix(" you control") {
        (PlayerRelation::Controller, rest.trim())
    } else if let Some(rest) = other
        .strip_suffix(" an opponent controls")
        .or_else(|| other.strip_suffix(" opponents control"))
        .or_else(|| other.strip_suffix(" you don't control"))
    {
        (PlayerRelation::Opponent, rest.trim())
    } else {
        (PlayerRelation::AnyParticipant, other.trim())
    };
    let core = without_ctrl
        .strip_prefix("an ")
        .or_else(|| without_ctrl.strip_prefix("a "))
        .unwrap_or(without_ctrl)
        .trim();
    if let Some(sub_raw) = core.strip_suffix(" source")
        && let Some(ct) = resolve_creature_subtype_token(sub_raw, vocabulary)
    {
        return Some((Some(ObjectFilter::any().with_subtype(ct)), ctrl_rel));
    }
    let subtypes = parse_creature_subtype_list(core, vocabulary)?;
    let base = ObjectClause::permanent();
    let filter = match subtypes.as_slice() {
        [a, b] => ObjectFilter::either(base.with_subtype(*a), base.with_subtype(*b)),
        [a, b, c] => ObjectFilter::Clause(base.among_subtypes([*a, *b, *c])),
        [a, b, c, d] => {
            ObjectFilter::either(base.among_subtypes([*a, *b, *c]), base.with_subtype(*d))
        }
        [a, b, c, d, e] => ObjectFilter::either(
            base.among_subtypes([*a, *b, *c]),
            base.among_subtypes([*d, *e, *d]),
        ),
        [a, b, c, d, e, f] => ObjectFilter::either(
            base.among_subtypes([*a, *b, *c]),
            base.among_subtypes([*d, *e, *f]),
        ),
        _ => return None,
    };
    Some((Some(filter), ctrl_rel))
}

fn parse_damage_source_filter(
    subj_low: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(Option<ObjectFilter>, PlayerRelation)> {
    let (other_than_self, core_subj) = if let Some(rest) = subj_low.strip_prefix("another ") {
        (true, format!("a {rest}"))
    } else {
        (false, subj_low.to_string())
    };
    let (mut source, source_controller): (Option<ObjectFilter>, PlayerRelation) = match core_subj
        .as_str()
    {
        "a source" | "any source" | "sources" => (None, PlayerRelation::AnyParticipant),
        "a source you control" | "any source you control" | "sources you control" => {
            (None, PlayerRelation::Controller)
        }
        "a source an opponent controls"
        | "a source you don't control"
        | "sources opponents control"
        | "sources an opponent controls" => (None, PlayerRelation::Opponent),
        "a white source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::White))),
            PlayerRelation::AnyParticipant,
        ),
        "a blue source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Blue))),
            PlayerRelation::AnyParticipant,
        ),
        "a black source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Black))),
            PlayerRelation::AnyParticipant,
        ),
        "a red source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red))),
            PlayerRelation::AnyParticipant,
        ),
        "a green source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Green))),
            PlayerRelation::AnyParticipant,
        ),
        "a white source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::White))),
            PlayerRelation::Controller,
        ),
        "a blue source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Blue))),
            PlayerRelation::Controller,
        ),
        "a black source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Black))),
            PlayerRelation::Controller,
        ),
        "a red source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red))),
            PlayerRelation::Controller,
        ),
        "a green source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Green))),
            PlayerRelation::Controller,
        ),
        "a red or artifact source you control" => (
            Some(ObjectFilter::either(
                ObjectClause::any().with_color(ObjectColorFilter::has(Color::Red)),
                ObjectClause::artifact(),
            )),
            PlayerRelation::Controller,
        ),
        "a noncreature source" => (
            Some(ObjectFilter::noncreature()),
            PlayerRelation::AnyParticipant,
        ),
        "a noncreature source you control" => (
            Some(ObjectFilter::noncreature()),
            PlayerRelation::Controller,
        ),
        "a creature you control" | "creatures you control" | "creature sources you control" => {
            (Some(ObjectFilter::creature()), PlayerRelation::Controller)
        }
        "a creature" | "creatures" | "creature sources" => (
            Some(ObjectFilter::creature()),
            PlayerRelation::AnyParticipant,
        ),
        "creatures you control with counters on them"
        | "a creature you control with a counter on it" => (
            Some(ObjectFilter::creature().with_flags(ObjectFilterFlags::WITH_ANY_COUNTER)),
            PlayerRelation::Controller,
        ),
        "a creature you control with a +1/+1 counter on it"
        | "creatures you control with +1/+1 counters on them" => (
            Some(ObjectFilter::creature().with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)),
            PlayerRelation::Controller,
        ),
        "a creature you control that entered this turn"
        | "a creature you control that entered the battlefield this turn" => (
            Some(ObjectFilter::creature().entered_this_turn()),
            PlayerRelation::Controller,
        ),
        "a source you control with an odd mana value" => (
            Some(ObjectFilter::any().with_mana_value(ManaValueFilter::Odd)),
            PlayerRelation::Controller,
        ),
        "a source you control with an even mana value" => (
            Some(ObjectFilter::any().with_mana_value(ManaValueFilter::Even)),
            PlayerRelation::Controller,
        ),
        "a permanent you control" | "permanents you control" => {
            (Some(ObjectFilter::permanent()), PlayerRelation::Controller)
        }
        "a creature or planeswalker you control" => (
            Some(ObjectFilter::any_types(
                CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
            )),
            PlayerRelation::Controller,
        ),
        "an artifact you control" | "a artifact you control" => {
            (Some(ObjectFilter::artifact()), PlayerRelation::Controller)
        }
        "an artifact" | "a artifact" => (
            Some(ObjectFilter::artifact()),
            PlayerRelation::AnyParticipant,
        ),
        "a red instant or sorcery spell you control" => (
            Some(ObjectFilter::instant_or_sorcery().with_color(ObjectColorFilter::has(Color::Red))),
            PlayerRelation::Controller,
        ),
        "an instant or sorcery spell you control"
        | "a instant or sorcery spell you control"
        | "an instant or sorcery source you control"
        | "a instant or sorcery source you control" => (
            Some(ObjectFilter::instant_or_sorcery()),
            PlayerRelation::Controller,
        ),
        "an instant or sorcery source" | "a instant or sorcery source" => (
            Some(ObjectFilter::instant_or_sorcery()),
            PlayerRelation::AnyParticipant,
        ),
        "a red instant or sorcery spell you control or a red planeswalker you control" => (
            Some(ObjectFilter::either(
                ObjectClause::any()
                    .any_types(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY))
                    .with_color(ObjectColorFilter::has(Color::Red)),
                ObjectClause::planeswalker().with_color(ObjectColorFilter::has(Color::Red)),
            )),
            PlayerRelation::Controller,
        ),
        "~" | "this creature" | "this permanent" | "this artifact" | "this enchantment" => (
            Some(ObjectFilter::self_object()),
            PlayerRelation::AnyParticipant,
        ),
        "enchanted creature" | "equipped creature" => (
            Some(ObjectFilter::Clause(ObjectClause {
                source_relation: SourceRelation::AttachedToSelf,
                ..ObjectClause::any()
            })),
            PlayerRelation::AnyParticipant,
        ),
        other => {
            if other.contains("of the chosen") {
                return None;
            }
            if let Some(res) = parse_subtype_damage_source(other, vocabulary) {
                res
            } else if let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            ) {
                if ast_f.any_spell && ast_f.any_types.is_empty() {
                    if (ast_f.forbidden_types.0 & AstCardTypeMask::INSTANT.0) != 0
                        && (ast_f.forbidden_types.0 & AstCardTypeMask::SORCERY.0) != 0
                    {
                        return None;
                    }
                    ast_f.any_types =
                        AstCardTypeMask(AstCardTypeMask::INSTANT.0 | AstCardTypeMask::SORCERY.0);
                }
                let ctrl_rel = match ast_f.controller.take() {
                    Some(ControllerRelation::You) => PlayerRelation::Controller,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        PlayerRelation::Opponent
                    }
                    None => PlayerRelation::AnyParticipant,
                    _ => return None,
                };
                if is_unrestricted_filter(&ast_f) {
                    (None, ctrl_rel)
                } else {
                    (
                        Some(lower_object_filter(&ast_f, vocabulary).ok()?),
                        ctrl_rel,
                    )
                }
            } else {
                return None;
            }
        }
    };
    if other_than_self {
        source = Some(
            source
                .unwrap_or_else(ObjectFilter::any)
                .other_than_ability_source(),
        );
    }
    Some((source, source_controller))
}

pub(super) fn parse_modify_damage_event(
    subj_low: &str,
    would_norm: &str,
    instead_norm: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRule> {
    let mut dummy_ids = LoweringIds::default();
    let mut rules = parse_damage_rules_full(
        None,
        subj_low,
        would_norm,
        instead_norm,
        vocabulary,
        &mut dummy_ids,
    )?;
    if rules.len() == 1 { rules.pop() } else { None }
}

fn validate_inline_followup_body(
    body: &ExecutableBody,
    vocabulary: &CreatureTypeVocabulary,
) -> bool {
    if body.target != BodyTarget::None || body.instructions.is_empty() {
        return false;
    }
    StaticContinuousAbility::EventReplacement {
        id: StaticAbilityId(0),
        event: ReplaceableEvent::Draw {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        condition: None,
        action: StaticReplacementAction::ReplaceWithBody {
            optional: false,
            body: Box::new(body.clone()),
        },
    }
    .validate(vocabulary)
    .is_ok()
}

fn parse_entry_copy_condition(
    cond_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ResolutionCondition> {
    let trimmed = cond_raw.trim().trim_end_matches(',').trim();
    let copy_filter_raw = trimmed
        .strip_prefix("it enters as a copy of ")
        .or_else(|| trimmed.strip_prefix("it enters the battlefield as a copy of "))
        .or_else(|| trimmed.strip_prefix("he enters as a copy of "))
        .or_else(|| trimmed.strip_prefix("she enters as a copy of "))
        .or_else(|| trimmed.strip_prefix("~ enters as a copy of "))
        .or_else(|| trimmed.strip_prefix("it's "))
        .or_else(|| trimmed.strip_prefix("it is "))
        .or_else(|| trimmed.strip_prefix("he's "))
        .or_else(|| trimmed.strip_prefix("she's "))
        .or_else(|| trimmed.strip_prefix("~ is "));
    if let Some(flt_raw) = copy_filter_raw {
        let flt_clean = flt_raw.trim();
        if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
            flt_clean,
            &cached_compiler_vocab(vocabulary),
        ) && ast_f.zone.is_none()
            && !ast_f.any_spell
            && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
        {
            let rc = ResolutionCondition {
                left: ValueExpr::ObjectMatches {
                    subject: MatchObjectRef::AbilitySource,
                    filter: Box::new(obj),
                },
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            };
            if rc.is_world_evaluable() && rc.validate_world(vocabulary).is_ok() {
                return Some(rc);
            }
        }
    }
    lower_world_resolution_condition(trimmed, vocabulary)
}

fn parse_counter_spec_phrase(raw: &str) -> Option<(CounterKind, u8)> {
    let mut s = raw.trim();
    s = s
        .strip_prefix("an additional ")
        .or_else(|| s.strip_prefix("additional "))
        .unwrap_or(s)
        .trim();
    let mut parts = s.splitn(2, ' ');
    let first = parts.next()?;
    let rest = parts.next().unwrap_or("").trim();
    let (count, kind_tail) = if matches!(first, "a" | "an" | "one") {
        (1u8, rest)
    } else if let Some(n) = parse_small_num(first).filter(|&n| n > 0) {
        let stripped = rest.strip_prefix("additional ").unwrap_or(rest).trim();
        (n, stripped)
    } else {
        (1u8, s)
    };
    let kind_raw = kind_tail
        .strip_suffix(" counters")
        .or_else(|| kind_tail.strip_suffix(" counter"))?
        .trim();
    let kind = parse_counter_kind(kind_raw).filter(|k| k.is_permanent())?;
    Some((kind, count))
}

fn parse_literal_pt_token(raw: &str) -> Option<(RulesInteger, RulesInteger)> {
    let (p_str, t_str) = raw.trim().split_once('/')?;
    if p_str.is_empty()
        || t_str.is_empty()
        || !p_str.bytes().all(|b| b.is_ascii_digit())
        || !t_str.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let p: i64 = p_str.parse().ok()?;
    let t: i64 = t_str.parse().ok()?;
    Some((p.into(), t.into()))
}

fn lower_entry_copy_exception(
    exc_raw: &str,
    ext: &mut EntryCopySpecExt,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let mut rest = exc_raw.trim().to_string();

    // 1. Extract all quoted abilities (`"..."`)
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    while let Some(q1) = rest.find('"')
        && let Some(q2_rel) = rest[q1 + 1..].find('"')
    {
        let quoted = rest[q1 + 1..q1 + 1 + q2_rel].trim().to_string();
        let before = rest[..q1].trim();
        let after = rest[q1 + 2 + q2_rel..].trim();
        let norm_q = quoted
            .replace("this creature", "~")
            .replace("This creature", "~")
            .replace("this permanent", "~")
            .replace("This permanent", "~")
            .replace("this artifact", "~")
            .replace("This artifact", "~")
            .replace("this enchantment", "~")
            .replace("This enchantment", "~");
        let synthetic = compile_card(
            &CardInput {
                oracle_id: "",
                name: "Copy",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Copy",
                    type_line: "Creature",
                    oracle_text: &norm_q,
                }],
            },
            &compiler_vocab,
        );
        if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
            return Err("oracle:unsupported-enters-as-copy");
        }
        let synth_face = &synthetic.faces[0];
        if let Ok(parts) = lower_granted_ability_face_parts(synth_face, vocabulary, ids) {
            for part in parts {
                match part {
                    StaticPart::GrantActivated(ab) => {
                        ext.extra_activated.push(*ab);
                    }
                    StaticPart::GrantTrigger(tr) => {
                        ext.extra_triggered.push(*tr);
                    }
                    StaticPart::Abilities(change) => {
                        let mut check = change.clone();
                        check.granted_keywords.clear();
                        if check == AbilityChange::default() {
                            ext.extra_keywords.extend(change.granted_keywords);
                        } else {
                            let st = StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector: PermanentSelector {
                                    controller: ControllerSelector::SourceItself,
                                    ..PermanentSelector::default()
                                },
                                parts: StaticParts(vec![StaticPart::Abilities(change)]),
                            };
                            st.validate(vocabulary)
                                .map_err(|_| "oracle:unsupported-enters-as-copy")?;
                            ext.extra_static.push(st);
                        }
                    }
                    other => {
                        let st = StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: PermanentSelector {
                                controller: ControllerSelector::SourceItself,
                                ..PermanentSelector::default()
                            },
                            parts: StaticParts(vec![other]),
                        };
                        st.validate(vocabulary)
                            .map_err(|_| "oracle:unsupported-enters-as-copy")?;
                        ext.extra_static.push(st);
                    }
                }
            }
        } else {
            if !synth_face.replacements.is_empty() || !synth_face.spell_effects.is_empty() {
                return Err("oracle:unsupported-enters-as-copy");
            }
            let mut scratch = Program::new(
                BTreeSet::from([CardType::Creature]),
                BTreeSet::new(),
                BTreeSet::new(),
                ProgramKind::Permanent(PermanentDefinition {
                    entry_tap: None,
                    attachment: None,
                    printed_pt: None,
                    casting_cost: None,
                }),
            );
            for (idx, kw) in synth_face.keywords.iter().enumerate() {
                lower_keyword(kw, idx, synth_face, &mut scratch, vocabulary, ids, false)
                    .map_err(|_| "oracle:unsupported-enters-as-copy")?;
            }
            for st in &synth_face.statics {
                lower_static(st, &mut scratch, vocabulary, ids, false)
                    .map_err(|_| "oracle:unsupported-enters-as-copy")?;
            }
            for ab in &synth_face.abilities {
                lower_ability(ab, &mut scratch, vocabulary, ids)
                    .map_err(|_| "oracle:unsupported-enters-as-copy")?;
            }
            if !scratch.activation_restrictions.is_empty()
                || !scratch.extra_activation_costs.is_empty()
                || !scratch.activation_hybrid_symbols.is_empty()
                || !scratch.activation_modes.is_empty()
                || !scratch.loyalty_costs.is_empty()
                || !scratch.static_conditions.is_empty()
                || !scratch.mana.is_empty()
                || !scratch.zone_replacements.is_empty()
                || scratch.entry_copy.is_some()
                || scratch.entry_tap_unless.is_some()
                || !scratch.cast_reductions.is_empty()
                || !scratch.entry_counter_abilities.is_empty()
                || scratch.cannot_be_countered
                || scratch.cannot_be_copied
                || scratch.conditional_flash.is_some()
                || scratch.attack_requirement.is_some()
                || scratch.combat_limits != Default::default()
                || scratch.conditional_cost.is_some()
                || !scratch.additional_cast_costs.is_empty()
                || !scratch.extra_cast_costs.is_empty()
            {
                return Err("oracle:unsupported-enters-as-copy");
            }
            let statics: Vec<StaticContinuousAbility> = scratch
                .continuous
                .into_iter()
                .chain(scratch.additional_battlefield_abilities)
                .collect();
            for st in &statics {
                if !st.allowed_as_additional() || st.validate(vocabulary).is_err() {
                    return Err("oracle:unsupported-enters-as-copy");
                }
            }
            if scratch.printed_keywords.is_empty()
                && scratch.activated.is_empty()
                && scratch.triggered.is_empty()
                && statics.is_empty()
            {
                return Err("oracle:unsupported-enters-as-copy");
            }
            ext.extra_keywords.extend(scratch.printed_keywords);
            ext.extra_activated.extend(scratch.activated);
            ext.extra_triggered.extend(scratch.triggered);
            ext.extra_static.extend(statics);
        }
        rest = format!("{before} {after}").trim().to_string();
    }

    let mut low = rest.to_ascii_lowercase();
    if low.contains("tapped") || low.contains("until end of turn") {
        return Err("oracle:unsupported-enters-as-copy");
    }
    low = low
        .replace("except ", " ")
        .replace("enters the battlefield with ", "enters with ");

    // 2. Keep name
    for phrase in [
        "its name is still ~",
        "his name is still ~",
        "her name is still ~",
        "its name is ~",
        "his name is ~",
        "her name is ~",
        "it's named ~",
        "it is named ~",
        "it keeps its name",
        "it retains its name",
    ] {
        if low.contains(phrase) {
            ext.keep_name = true;
            low = low.replace(phrase, " ");
        }
    }

    // 3. Nonlegendary
    if low.contains("isn't legendary") || low.contains("is not legendary") {
        ext.nonlegendary = true;
        for phrase in [
            "it isn't legendary",
            "he isn't legendary",
            "she isn't legendary",
            "it is not legendary",
            "he is not legendary",
            "she is not legendary",
            "isn't legendary",
            "is not legendary",
        ] {
            low = low.replace(phrase, " ");
        }
    }

    // 4a. Conditional counters: prefix-if (`if <cond>, [it] enters with <N> <kind> counter(s) on it`)
    loop {
        let mut matched_span: Option<(usize, usize, ResolutionCondition, CounterKind, u8)> = None;
        let bytes = low.as_bytes();
        let mut search_from = 0usize;
        while let Some(rel) = low[search_from..].find("if ") {
            let if_pos = search_from + rel;
            search_from = if_pos + 3;
            if if_pos > 0 && bytes[if_pos - 1].is_ascii_alphanumeric() {
                continue;
            }
            let after_if = &low[if_pos + 3..];
            let Some((cond_part, after_comma)) = after_if.split_once(", ") else {
                continue;
            };
            let after_pron = after_comma
                .strip_prefix("it ")
                .or_else(|| after_comma.strip_prefix("he "))
                .or_else(|| after_comma.strip_prefix("she "))
                .unwrap_or(after_comma);
            let Some(after_ew) = after_pron.strip_prefix("enters with ") else {
                continue;
            };
            let end_rel = [" on it", " on him", " on her", " on ~"]
                .into_iter()
                .filter_map(|m| after_ew.find(m).map(|idx| (idx, m.len())))
                .min_by_key(|(idx, _)| *idx);
            let Some((counter_end, suffix_len)) = end_rel else {
                continue;
            };
            let counter_phrase = &after_ew[..counter_end];
            if let Some((kind, count)) = parse_counter_spec_phrase(counter_phrase)
                && let Some(rc) = parse_entry_copy_condition(cond_part, vocabulary)
            {
                let consumed_len = 3
                    + cond_part.len()
                    + 2
                    + (after_comma.len() - after_pron.len())
                    + "enters with ".len()
                    + counter_end
                    + suffix_len;
                matched_span = Some((if_pos, if_pos + consumed_len, rc, kind, count));
                break;
            }
        }
        if let Some((start, end, rc, kind, count)) = matched_span {
            ext.conditional_counters.push((rc, kind, count));
            low = format!("{} {}", &low[..start], &low[end..]);
        } else {
            break;
        }
    }

    // 4b. Conditional counters: suffix-if (`[it] enters with <N> <kind> counter(s) on it if <cond>`)
    loop {
        let mut matched_span: Option<(usize, usize, ResolutionCondition, CounterKind, u8)> = None;
        let mut search_from = 0usize;
        while let Some(rel) = low[search_from..].find("enters with ") {
            let ew_pos = search_from + rel;
            search_from = ew_pos + "enters with ".len();
            let start = ["it ", "he ", "she "]
                .into_iter()
                .find_map(|p| {
                    (ew_pos >= p.len() && &low[ew_pos - p.len()..ew_pos] == p)
                        .then_some(ew_pos - p.len())
                })
                .unwrap_or(ew_pos);
            let after_ew = &low[ew_pos + "enters with ".len()..];
            let on_if = [" on it if ", " on him if ", " on her if ", " on ~ if "]
                .into_iter()
                .filter_map(|m| after_ew.find(m).map(|idx| (idx, m.len())))
                .min_by_key(|(idx, _)| *idx);
            let Some((counter_end, mid_len)) = on_if else {
                continue;
            };
            let counter_phrase = &after_ew[..counter_end];
            let after_mid = &after_ew[counter_end + mid_len..];
            let cond_end = [",", ".", ";", " and "]
                .into_iter()
                .filter_map(|m| after_mid.find(m))
                .min()
                .unwrap_or(after_mid.len());
            let cond_part = &after_mid[..cond_end];
            if let Some((kind, count)) = parse_counter_spec_phrase(counter_phrase)
                && let Some(rc) = parse_entry_copy_condition(cond_part, vocabulary)
            {
                let end = ew_pos + "enters with ".len() + counter_end + mid_len + cond_end;
                matched_span = Some((start, end, rc, kind, count));
                break;
            }
        }
        if let Some((start, end, rc, kind, count)) = matched_span {
            ext.conditional_counters.push((rc, kind, count));
            low = format!("{} {}", &low[..start], &low[end..]);
        } else {
            break;
        }
    }

    // 5. Unconditional extra counters
    while let Some(ew_pos) = low.find("enters with ") {
        let start = ["it ", "he ", "she "]
            .into_iter()
            .find_map(|p| {
                (ew_pos >= p.len() && &low[ew_pos - p.len()..ew_pos] == p)
                    .then_some(ew_pos - p.len())
            })
            .unwrap_or(ew_pos);
        let after_ew = &low[ew_pos + "enters with ".len()..];
        let on_ref = [" on it", " on him", " on her", " on ~"]
            .into_iter()
            .filter_map(|m| after_ew.find(m).map(|idx| (idx, m.len())))
            .min_by_key(|(idx, _)| *idx);
        let (counter_phrase, end) = match on_ref {
            Some((idx, len)) => (&after_ew[..idx], ew_pos + "enters with ".len() + idx + len),
            None => {
                let stop = [",", ".", ";", " and "]
                    .into_iter()
                    .filter_map(|m| after_ew.find(m))
                    .min()
                    .unwrap_or(after_ew.len());
                (&after_ew[..stop], ew_pos + "enters with ".len() + stop)
            }
        };
        let Some((kind, count)) = parse_counter_spec_phrase(counter_phrase) else {
            return Err("oracle:unsupported-enters-as-copy");
        };
        ext.extra_counters.push((kind, count));
        low = format!("{} {}", &low[..start], &low[end..]);
    }

    // 6. Extra types, subtypes, additive colors, and literal P/T inside "in addition to its other ..."
    loop {
        let addition_match = [
            "in addition to its other colors and types",
            "in addition to his other colors and types",
            "in addition to her other colors and types",
            "in addition to its other types and colors",
            "in addition to his other types and colors",
            "in addition to her other types and colors",
            "in addition to its other creature types",
            "in addition to his other creature types",
            "in addition to her other creature types",
            "in addition to its other types",
            "in addition to his other types",
            "in addition to her other types",
            "in addition to its other colors",
            "in addition to his other colors",
            "in addition to her other colors",
        ]
        .into_iter()
        .filter_map(|suffix| {
            low.find(suffix)
                .map(|pos| (pos, suffix.len(), suffix.contains("colors")))
        })
        .min_by_key(|(pos, _, _)| *pos);
        let Some((pos, suffix_len, allows_colors)) = addition_match else {
            break;
        };
        let prefix = &low[..pos];
        let copula = ["it's ", "he's ", "she's ", "it is ", "he is ", "she is "]
            .into_iter()
            .filter_map(|op| prefix.rfind(op).map(|idx| (idx, op.len())))
            .max_by_key(|(idx, _)| *idx);
        let (clause_start, body) = match copula {
            Some((idx, len)) => (idx, prefix[idx + len..].trim()),
            None => (0, prefix.trim()),
        };
        for tok in body.split_whitespace() {
            let w = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '/');
            if let Some((p, t)) = parse_literal_pt_token(w) {
                ext.override_pt = Some((p, t));
                continue;
            }
            match w {
                "" | "a" | "an" | "and" | "it" | "he" | "she" | "is" | "it's" | "he's"
                | "she's" => {}
                "artifact" => {
                    ext.extra_types.insert(CardType::Artifact);
                }
                "enchantment" => {
                    ext.extra_types.insert(CardType::Enchantment);
                }
                "creature" => {
                    ext.extra_types.insert(CardType::Creature);
                }
                "land" => {
                    ext.extra_types.insert(CardType::Land);
                }
                "planeswalker" => {
                    ext.extra_types.insert(CardType::Planeswalker);
                }
                "white" if allows_colors => {
                    ext.extra_colors.insert(Color::White);
                }
                "blue" if allows_colors => {
                    ext.extra_colors.insert(Color::Blue);
                }
                "black" if allows_colors => {
                    ext.extra_colors.insert(Color::Black);
                }
                "red" if allows_colors => {
                    ext.extra_colors.insert(Color::Red);
                }
                "green" if allows_colors => {
                    ext.extra_colors.insert(Color::Green);
                }
                _ => {
                    if let Some(ct) = resolve_creature_subtype_token(w, vocabulary) {
                        ext.extra_creature_subtypes.insert(ct);
                    } else {
                        return Err("oracle:unsupported-enters-as-copy");
                    }
                }
            }
        }
        let clause_end = pos + suffix_len;
        low = format!("{} {}", &low[..clause_start], &low[clause_end..]);
    }

    // 7. Standalone literal base power and toughness override
    for pt_prefix in [
        "its base power and toughness are ",
        "it has base power and toughness ",
        "its power and toughness are ",
    ] {
        if let Some(pos) = low.find(pt_prefix) {
            let after = &low[pos + pt_prefix.len()..];
            if let Some(tok) = after.split_whitespace().next() {
                let clean = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/');
                if let Some((p, t)) = parse_literal_pt_token(clean) {
                    ext.override_pt = Some((p, t));
                    let end = pos + pt_prefix.len() + tok.len();
                    low = format!("{} {}", &low[..pos], &low[end..]);
                }
            }
        }
    }

    // 8. Parameterized and combat-trigger keywords (Flesh Duplicate: vanishing 3, Dack's Duplicate: dethrone)
    if let Some(v_pos) = low.find("vanishing") {
        let after = low[v_pos + "vanishing".len()..].trim_start();
        let ws_len = low[v_pos + "vanishing".len()..].len() - after.len();
        let next_tok = after.split_whitespace().next().unwrap_or("");
        let clean_num = next_tok.trim_matches(|c: char| !c.is_ascii_digit());
        let (van_n, end) = if !clean_num.is_empty()
            && let Ok(n) = clean_num.parse::<u32>()
        {
            (Some(n), v_pos + "vanishing".len() + ws_len + next_tok.len())
        } else {
            (None, v_pos + "vanishing".len())
        };
        ext.extra_triggered
            .push(CounterKeyword::Vanishing(van_n).ability(ids.triggered()));
        ext.extra_triggered
            .push(CounterKeyword::VanishingLastCounter.ability(ids.triggered()));
        low = format!("{} {}", &low[..v_pos], &low[end..]);
    }
    if let Some(f_pos) = low.find("fading ") {
        let after = &low[f_pos + "fading ".len()..];
        let next_tok = after.split_whitespace().next().unwrap_or("");
        let clean_num = next_tok.trim_matches(|c: char| !c.is_ascii_digit());
        if !clean_num.is_empty()
            && let Ok(n) = clean_num.parse::<u32>()
        {
            ext.extra_triggered
                .push(CounterKeyword::Fading(n).ability(ids.triggered()));
            let end = f_pos + "fading ".len() + next_tok.len();
            low = format!("{} {}", &low[..f_pos], &low[end..]);
        }
    }
    for (kw_word, trig_builder) in [
        (
            "dethrone",
            (|ids: &mut LoweringIds| {
                TriggeredAbility::combat_keyword(ids.triggered(), CombatTriggerKeyword::Dethrone)
            }) as fn(&mut LoweringIds) -> TriggeredAbility,
        ),
        ("melee", |ids| {
            TriggeredAbility::combat_keyword(ids.triggered(), CombatTriggerKeyword::Melee)
        }),
        ("prowess", |ids| {
            TriggeredAbility::combat_keyword(ids.triggered(), CombatTriggerKeyword::Prowess)
        }),
        ("training", |ids| {
            TriggeredAbility::combat_keyword(ids.triggered(), CombatTriggerKeyword::Training)
        }),
        ("flanking", |ids| {
            TriggeredAbility::combat_keyword(ids.triggered(), CombatTriggerKeyword::Flanking)
        }),
        ("undying", |ids| {
            CounterKeyword::Undying.ability(ids.triggered())
        }),
        ("persist", |ids| {
            CounterKeyword::Persist.ability(ids.triggered())
        }),
    ] {
        let mut matched_kw = false;
        for phrase in [
            format!("it has {kw_word}"),
            format!("he has {kw_word}"),
            format!("she has {kw_word}"),
            format!("it gains {kw_word}"),
            format!("he gains {kw_word}"),
            format!("she gains {kw_word}"),
            format!("has {kw_word}"),
            format!("gains {kw_word}"),
            format!("and {kw_word}"),
            format!(", {kw_word}"),
        ] {
            if low.contains(&phrase) {
                matched_kw = true;
                low = low.replace(&phrase, " ");
            }
        }
        if matched_kw {
            ext.extra_triggered.push(trig_builder(ids));
        }
    }

    // 9. Static keyword abilities
    for kw_name in [
        "first strike",
        "double strike",
        "nonbasic landwalk",
        "legendary landwalk",
        "snow landwalk",
        "artifact landwalk",
        "haste",
        "flying",
        "trample",
        "vigilance",
        "lifelink",
        "deathtouch",
        "menace",
        "hexproof",
        "indestructible",
        "changeling",
        "reach",
        "flash",
        "defender",
        "shroud",
        "fear",
        "intimidate",
        "skulk",
        "shadow",
        "horsemanship",
        "phasing",
        "wither",
        "infect",
        "devoid",
        "decayed",
        "plainswalk",
        "islandwalk",
        "swampwalk",
        "mountainwalk",
        "forestwalk",
    ] {
        if let Some(ka) = lower_keyword_ability_name(kw_name) {
            let mut matched_kw = false;
            for phrase in [
                format!("it has {kw_name}"),
                format!("he has {kw_name}"),
                format!("she has {kw_name}"),
                format!("it gains {kw_name}"),
                format!("he gains {kw_name}"),
                format!("she gains {kw_name}"),
                format!("has {kw_name}"),
                format!("gains {kw_name}"),
            ] {
                if low.contains(&phrase) {
                    matched_kw = true;
                    low = low.replace(&phrase, " ");
                }
            }
            if !matched_kw
                && (!ext.extra_keywords.is_empty()
                    || !ext.extra_triggered.is_empty()
                    || !ext.extra_activated.is_empty())
            {
                for phrase in [format!("and {kw_name}"), format!(", {kw_name}")] {
                    if low.contains(&phrase) {
                        matched_kw = true;
                        low = low.replace(&phrase, " ");
                    }
                }
            }
            if matched_kw {
                ext.extra_keywords.insert(ka);
                if ka == KeywordAbility::Decayed {
                    ext.extra_triggered.push(TriggeredAbility::combat_keyword(
                        ids.triggered(),
                        CombatTriggerKeyword::Decayed,
                    ));
                }
            }
        }
    }

    let mut scrubbed = low;
    for phrase in [
        "it has",
        "he has",
        "she has",
        "it gains",
        "he gains",
        "she gains",
    ] {
        scrubbed = scrubbed.replace(phrase, " ");
    }
    let scrubbed = scrubbed.replace([',', '.', ';'], " ");
    let has_unrecognized = scrubbed
        .split_whitespace()
        .any(|tok| !matches!(tok, "it" | "he" | "she" | "and" | "except"));
    if has_unrecognized {
        return Err("oracle:unsupported-enters-as-copy");
    }
    Ok(())
}

fn try_lower_self_discard_by_opponent(
    subj_low: &str,
    would_norm: &str,
    instead_raw: &str,
    instead_norm: &str,
    program: &mut Program,
    next_zone_rep_id: &impl Fn(&Program) -> ZoneReplacementAbilityId,
) -> Option<Result<(), &'static str>> {
    let parse_discard_dest = |tail: &str| -> Option<PublicZoneDestination> {
        let t = tail
            .trim()
            .trim_end_matches('.')
            .trim()
            .strip_suffix(" instead of putting it into your graveyard")
            .or_else(|| {
                tail.trim()
                    .trim_end_matches('.')
                    .trim()
                    .strip_suffix(" instead of putting ~ into your graveyard")
            })
            .or_else(|| {
                tail.trim()
                    .trim_end_matches('.')
                    .trim()
                    .strip_suffix(" instead")
            })
            .unwrap_or(tail.trim().trim_end_matches('.').trim())
            .trim();
        match t {
            "put it onto the battlefield"
            | "put ~ onto the battlefield"
            | "put it onto the battlefield under your control"
            | "put ~ onto the battlefield under your control"
            | "put it onto the battlefield under its owner's control" => {
                Some(PublicZoneDestination::Battlefield)
            }
            "return it to your hand"
            | "return ~ to your hand"
            | "return it to its owner's hand"
            | "return ~ to its owner's hand"
            | "put it into your hand"
            | "put ~ into your hand"
            | "put it into its owner's hand" => Some(PublicZoneDestination::Hand),
            _ => None,
        }
    };

    let replace_with = if would_norm == "special_replacement" {
        let full_norm = instead_raw
            .replace("this card", "~")
            .replace("this creature", "~")
            .replace("this permanent", "~")
            .replace("this artifact", "~")
            .replace("this enchantment", "~");
        full_norm
            .strip_prefix("if a spell or ability an opponent controls causes you to discard ~, ")
            .or_else(|| {
                full_norm.strip_prefix(
                    "if a spell or ability an opponent controls causes you to discard it, ",
                )
            })
            .and_then(parse_discard_dest)
    } else if matches!(
        subj_low,
        "a spell or ability an opponent controls" | "an opponent's spell or ability"
    ) && matches!(
        would_norm,
        "cause you to discard ~"
            | "causes you to discard ~"
            | "cause you to discard this card"
            | "cause you to discard this creature"
            | "cause you to discard this permanent"
    ) {
        parse_discard_dest(instead_norm)
    } else {
        None
    };

    let replace_with = replace_with?;
    let id = next_zone_rep_id(program);
    program.zone_replacements.push(ZoneReplacementAbility {
        id,
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::SourceSelfDiscardByOpponent,
        destination: PublicZoneDestination::Graveyard,
        replace_with,
        choice: ZoneReplacementChoice::Mandatory,
    });
    Some(Ok(()))
}

#[allow(clippy::too_many_arguments)]
fn parse_zone_replace_dest_and_extra(
    instead_raw: &str,
    instead_norm: &str,
    allow_hand: bool,
    allow_library: bool,
    allow_battlefield: bool,
    allow_optional: bool,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<(
    ZoneReplacementChoice,
    PublicZoneDestination,
    Option<ZoneReplacementExtra>,
)> {
    let (choice, core) = if let Some(rest) = instead_raw
        .strip_prefix("you may instead ")
        .map(|s| s.trim_end_matches(" instead").trim())
    {
        if !allow_optional {
            return None;
        }
        (ZoneReplacementChoice::OptionalAffectedPlayer, rest)
    } else if let Some(rest) = instead_norm.strip_prefix("you may ") {
        if !allow_optional {
            return None;
        }
        (ZoneReplacementChoice::OptionalAffectedPlayer, rest.trim())
    } else {
        (ZoneReplacementChoice::Mandatory, instead_norm)
    };

    let parse_followup_tail =
        |tail_raw: &str, ids: &mut LoweringIds| -> Option<Option<Box<ExecutableBody>>> {
            let mut t = tail_raw.trim();
            t = t.strip_prefix("instead").unwrap_or(t).trim();
            t = t.trim_start_matches([',', '.']).trim();
            if t.is_empty() {
                return Some(None);
            }
            let body_raw = if let Some(rest) = t.strip_prefix("and ") {
                rest.trim()
            } else if let Some(rest) = t.strip_prefix("then ") {
                rest.trim()
            } else if let Some(rest) = t.strip_prefix("if you do, ") {
                rest.trim()
            } else if tail_raw.trim_start().starts_with('.')
                || tail_raw.trim_start().starts_with("instead.")
            {
                t
            } else {
                return None;
            };
            if body_raw.is_empty()
                || body_raw.starts_with("when you do")
                || body_raw.starts_with("whenever ")
            {
                return None;
            }
            let action_owned = body_raw
                .replace("this creature", "~")
                .replace("this permanent", "~")
                .replace("this artifact", "~")
                .replace("this enchantment", "~")
                .replace(" instead. ", ". ");
            let (ins, fails) = spellwake_oracle_compiler::clauses::parse_effect_body(
                action_owned.trim(),
                &cached_compiler_vocab(vocabulary),
            );
            if !fails.is_empty() || ins.is_empty() {
                return None;
            }
            let body = lower_effect_body(&ins, vocabulary, ids, false, true).ok()?;
            if !validate_inline_followup_body(&body, vocabulary) {
                return None;
            }
            Some(Some(Box::new(body)))
        };

    // 1. Exile (+ optional counters + optional inline followup)
    if let Some(after_ex) = core.strip_prefix("exile ") {
        let ref_match = [
            "that planeswalker",
            "that permanent",
            "that creature",
            "this enchantment",
            "this permanent",
            "this creature",
            "this artifact",
            "that token",
            "that spell",
            "that card",
            "this card",
            "it",
            "~",
        ]
        .into_iter()
        .find_map(|r| {
            after_ex.strip_prefix(r).filter(|rem| {
                rem.is_empty()
                    || rem.starts_with(' ')
                    || rem.starts_with('.')
                    || rem.starts_with(',')
            })
        });
        if let Some(mut rem) = ref_match {
            rem = rem.trim();
            rem = rem.strip_prefix("instead").unwrap_or(rem).trim();
            let mut exile_counters: Option<(CounterKind, u8)> = None;
            if let Some(after_with) = rem.strip_prefix("with ") {
                let on_pos = [" on it", " on ~", " on that card", " on that creature"]
                    .into_iter()
                    .filter_map(|m| after_with.find(m).map(|idx| (idx, m.len())))
                    .min_by_key(|(idx, _)| *idx)?;
                let (kind, count) = parse_counter_spec_phrase(&after_with[..on_pos.0])?;
                exile_counters = Some((kind, count));
                rem = &after_with[on_pos.0 + on_pos.1..];
            }
            let followup_body = parse_followup_tail(rem, ids)?;
            let extra = if exile_counters.is_some() || followup_body.is_some() {
                Some(ZoneReplacementExtra {
                    exile_counters,
                    followup_body,
                })
            } else {
                None
            };
            return Some((choice, PublicZoneDestination::Exile, extra));
        }
    }

    // 2. Hand
    if allow_hand {
        let hand_rem = [
            "return it to its owner's hand",
            "return ~ to its owner's hand",
            "return that card to its owner's hand",
            "return that creature to its owner's hand",
            "return that permanent to its owner's hand",
            "return that planeswalker to its owner's hand",
            "return it to your hand",
            "return ~ to your hand",
            "return that card to your hand",
            "return that creature to your hand",
            "return that permanent to your hand",
            "put it into its owner's hand",
            "put ~ into its owner's hand",
            "put that card into its owner's hand",
            "put it into your hand",
            "put ~ into your hand",
            "put that card into your hand",
        ]
        .into_iter()
        .find_map(|p| {
            core.strip_prefix(p).filter(|rem| {
                rem.is_empty()
                    || rem.starts_with(' ')
                    || rem.starts_with('.')
                    || rem.starts_with(',')
            })
        });
        if let Some(rem) = hand_rem {
            let followup_body = parse_followup_tail(rem, ids)?;
            let extra = followup_body.map(|fb| ZoneReplacementExtra {
                exile_counters: None,
                followup_body: Some(fb),
            });
            return Some((choice, PublicZoneDestination::Hand, extra));
        }
    }

    // 3. Library (mandatory only, matching zone_replacement.rs validation)
    if allow_library
        && choice == ZoneReplacementChoice::Mandatory
        && !core.contains(". ")
        && (matches!(
            core,
            "reveal ~ and shuffle it into its owner's library"
                | "reveal it and shuffle it into its owner's library"
                | "shuffle ~ into its owner's library"
                | "shuffle it into its owner's library"
                | "shuffle ~ into your library"
                | "shuffle it into your library"
                | "put ~ on the bottom of its owner's library"
                | "put it on the bottom of its owner's library"
                | "put ~ on top of its owner's library"
                | "put it on top of its owner's library"
        ) || (!core.contains(" and ")
            && core.contains("shuffle")
            && core.contains("into its owner's library")))
    {
        return Some((choice, PublicZoneDestination::Library, None));
    }

    // 4. Battlefield
    if allow_battlefield
        && matches!(
            core,
            "put it onto the battlefield"
                | "put ~ onto the battlefield"
                | "put it onto the battlefield under your control"
                | "put ~ onto the battlefield under your control"
        )
    {
        return Some((choice, PublicZoneDestination::Battlefield, None));
    }

    None
}

fn parse_token_filter_phrase(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let vocab = cached_compiler_vocab(vocabulary);
    let try_parse = |candidate: &str| -> Option<ObjectFilter> {
        let ast_f =
            spellwake_oracle_compiler::filter::parse_object_filter(candidate, &vocab).ok()?;
        if ast_f.is_empty() || ast_f.zone.is_some() || ast_f.any_spell {
            return None;
        }
        lower_object_filter(&ast_f, vocabulary).ok()
    };
    let article = if matches!(
        trimmed.as_bytes().first(),
        Some(b'a' | b'e' | b'i' | b'o' | b'u')
    ) {
        "an"
    } else {
        "a"
    };
    try_parse(&format!("{article} {trimmed}"))
        .or_else(|| try_parse(&format!("{article} {trimmed} permanent")))
}

fn lower_filtered_or_steal_token_replacement(
    subj_low: &str,
    would_norm: &str,
    instead_norm: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<StaticModifierRule> {
    // 1. StealCreatedTokens (Crafty Cutpurse and static/temporary variants)
    if would_norm == "would_event" {
        let tok_flt_opt: Option<Option<ObjectFilter>> =
            if matches!(subj_low, "each token" | "any token" | "tokens" | "a token") {
                Some(None)
            } else if let Some(mid) = subj_low
                .strip_prefix("each ")
                .or_else(|| subj_low.strip_prefix("any "))
                .or_else(|| subj_low.strip_prefix("a "))
                .and_then(|s| s.strip_suffix(" token"))
            {
                parse_token_filter_phrase(mid, vocabulary).map(Some)
            } else if let Some(mid) = subj_low.strip_suffix(" tokens") {
                parse_token_filter_phrase(mid, vocabulary).map(Some)
            } else {
                None
            };
        if let Some(tok_flt) = tok_flt_opt
            && let Some(after_created) = instead_norm.strip_prefix("be created ")
        {
            let affected = if after_created
                == "under an opponent's control this turn is created under your control"
                || after_created == "under an opponent's control is created under your control"
                || after_created
                    == "under an opponent's control this turn are created under your control"
                || after_created == "under an opponent's control are created under your control"
            {
                Some(PlayerRelation::Opponent)
            } else if after_created
                == "under a player's control this turn is created under your control"
                || after_created == "under a player's control is created under your control"
                || after_created
                    == "under another player's control this turn is created under your control"
                || after_created == "under another player's control is created under your control"
            {
                Some(PlayerRelation::AnyParticipant)
            } else {
                None
            };
            if let Some(affected) = affected {
                return Some(StaticModifierRule::StealCreatedTokens {
                    affected,
                    token_filter: tok_flt.map(Box::new),
                });
            }
        }
    }

    // Standard "If one or more [<flt>] tokens would be created under an opponent's control, those tokens are created under your control instead"
    if let Some(after_one) = subj_low.strip_prefix("one or more ")
        && let Some(flt_raw) = after_one
            .strip_suffix(" tokens")
            .or_else(|| (after_one == "tokens").then_some(""))
        && matches!(
            instead_norm,
            "those tokens are created under your control"
                | "they are created under your control"
                | "they're created under your control"
                | "you create those tokens"
        )
    {
        let affected = match would_norm {
            "be created under an opponent's control" | "be created under opponents' control" => {
                Some(PlayerRelation::Opponent)
            }
            "be created under a player's control" | "be created under another player's control" => {
                Some(PlayerRelation::AnyParticipant)
            }
            _ => None,
        };
        if let Some(affected) = affected {
            let token_filter = if flt_raw.trim().is_empty() {
                None
            } else {
                Some(Box::new(parse_token_filter_phrase(flt_raw, vocabulary)?))
            };
            return Some(StaticModifierRule::StealCreatedTokens {
                affected,
                token_filter,
            });
        }
    }
    if matches!(subj_low, "an opponent" | "a player")
        && let Some(after_create) = would_norm.strip_prefix("create one or more ")
        && let Some(flt_raw) = after_create
            .strip_suffix(" tokens")
            .or_else(|| (after_create == "tokens").then_some(""))
        && matches!(
            instead_norm,
            "you create those tokens" | "you create them" | "create those tokens"
        )
    {
        let affected = if subj_low == "an opponent" {
            PlayerRelation::Opponent
        } else {
            PlayerRelation::AnyParticipant
        };
        let token_filter = if flt_raw.trim().is_empty() {
            None
        } else {
            Some(Box::new(parse_token_filter_phrase(flt_raw, vocabulary)?))
        };
        return Some(StaticModifierRule::StealCreatedTokens {
            affected,
            token_filter,
        });
    }
    if subj_low == "an effect"
        && let Some(after_create) = would_norm.strip_prefix("create one or more ")
        && let Some((tok_part, ctrl_part)) = after_create.split_once(" under ")
        && let Some(flt_raw) = tok_part
            .trim()
            .strip_suffix(" tokens")
            .or_else(|| (tok_part.trim() == "tokens").then_some(""))
        && matches!(
            instead_norm,
            "it creates those tokens under your control"
                | "it creates them under your control"
                | "those tokens are created under your control"
        )
    {
        let affected = match ctrl_part.trim() {
            "an opponent's control" | "opponents' control" => Some(PlayerRelation::Opponent),
            "a player's control" | "another player's control" => {
                Some(PlayerRelation::AnyParticipant)
            }
            _ => None,
        };
        if let Some(affected) = affected {
            let token_filter = if flt_raw.trim().is_empty() {
                None
            } else {
                Some(Box::new(parse_token_filter_phrase(flt_raw, vocabulary)?))
            };
            return Some(StaticModifierRule::StealCreatedTokens {
                affected,
                token_filter,
            });
        }
    }

    // 2. Extract filtered token subject + controller relation
    let (filter_raw, controller): (&str, PlayerRelation) = if let Some(mid) = subj_low
        .strip_prefix("one or more ")
        .and_then(|s| s.strip_suffix(" tokens"))
        && !mid.trim().is_empty()
    {
        let ctrl = match would_norm {
            "be created under your control" => PlayerRelation::Controller,
            "be created under an opponent's control" | "be created under opponents' control" => {
                PlayerRelation::Opponent
            }
            "be created" | "be created under a player's control" => PlayerRelation::AnyParticipant,
            _ => return None,
        };
        (mid.trim(), ctrl)
    } else if matches!(subj_low, "you" | "an opponent" | "a player")
        && let Some(mid) = would_norm
            .strip_prefix("create one or more ")
            .and_then(|s| s.strip_suffix(" tokens"))
        && !mid.trim().is_empty()
    {
        let ctrl = match subj_low {
            "you" => PlayerRelation::Controller,
            "an opponent" => PlayerRelation::Opponent,
            _ => PlayerRelation::AnyParticipant,
        };
        (mid.trim(), ctrl)
    } else if subj_low == "an effect"
        && let Some(after_create) = would_norm.strip_prefix("create one or more ")
    {
        if let Some((tok_part, ctrl_part)) = after_create.split_once(" under ") {
            let mid = tok_part.trim().strip_suffix(" tokens")?.trim();
            if mid.is_empty() {
                return None;
            }
            let ctrl = match ctrl_part.trim() {
                "your control" => PlayerRelation::Controller,
                "an opponent's control" | "opponents' control" => PlayerRelation::Opponent,
                "a player's control" => PlayerRelation::AnyParticipant,
                _ => return None,
            };
            (mid, ctrl)
        } else {
            let mid = after_create.strip_suffix(" tokens")?.trim();
            if mid.is_empty() {
                return None;
            }
            (mid, PlayerRelation::AnyParticipant)
        }
    } else {
        return None;
    };

    let obj_filter = parse_token_filter_phrase(filter_raw, vocabulary)?;

    // 3. BonusTokenFiltered (Queen Allenal of Ruadach, filtered Chatterfang variants)
    {
        let trimmed = instead_norm
            .strip_prefix("you ")
            .or_else(|| instead_norm.strip_prefix("it "))
            .unwrap_or(instead_norm);
        let trimmed = trimmed
            .strip_prefix("creates ")
            .or_else(|| trimmed.strip_prefix("create "))
            .unwrap_or(trimmed);
        let trimmed = trimmed.strip_suffix(" are created").unwrap_or(trimmed);
        let bonus_raw_opt = trimmed
            .strip_prefix("those tokens plus ")
            .or_else(|| trimmed.strip_prefix("that many tokens plus "))
            .or_else(|| {
                trimmed
                    .strip_prefix("that many ")
                    .and_then(|r| r.split_once(" tokens plus ").map(|(_, b)| b))
            });
        if let Some(bonus_raw) = bonus_raw_opt {
            let bonus_t = bonus_raw.trim();
            if parse_small_num(bonus_t).is_none() {
                let (per_created, token_phrase) =
                    if let Some(rest) = bonus_t.strip_prefix("that many ") {
                        let singular = rest
                            .strip_suffix(" tokens")
                            .map(|s| format!("a {s} token"))
                            .unwrap_or_else(|| format!("a {rest}"));
                        (true, singular)
                    } else if let Some(rest) = bonus_t
                        .strip_prefix("an additional ")
                        .or_else(|| bonus_t.strip_prefix("additional "))
                    {
                        (false, format!("a {rest}"))
                    } else {
                        (false, bonus_t.to_string())
                    };
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                if let Ok(tok_ast) = spellwake_oracle_compiler::filter::parse_token_creation(
                    &token_phrase,
                    &compiler_vocab,
                ) && let TokenSpecAst::Predefined { count, .. }
                | TokenSpecAst::Creature { count, .. } = &tok_ast
                    && let ValueExprAst::Const(n) = count.as_ref()
                    && let Ok(count) = u8::try_from(*n)
                    && count > 0
                    && let Ok((creation, false)) = lower_token_spec(
                        &tok_ast,
                        TokenRecipients::Controller,
                        vocabulary,
                        ids,
                        None,
                        None,
                    )
                    && !creation.enters_tapped
                    && creation.base_size.is_none()
                {
                    return Some(StaticModifierRule::BonusTokenFiltered {
                        filter: Box::new(obj_filter),
                        controller,
                        token: Box::new(creation.definition),
                        count,
                        per_created,
                    });
                }
            }
        }
    }

    // 4. ModifyTokensFiltered (Ojer Taq, Deepest Foundation, Kaya, etc.)
    if instead_norm.contains(" and ") || instead_norm.contains(". ") {
        return None;
    }
    let (add, multiply) =
        if instead_norm.contains("twice that many") || instead_norm.contains("double that many") {
            (0u8, 2u8)
        } else if instead_norm.contains("three times that many")
            || instead_norm.contains("triple that many")
        {
            (0u8, 3u8)
        } else if instead_norm.contains("four times that many") {
            (0u8, 4u8)
        } else if let Some((_, rhs)) = instead_norm.split_once(" plus ")
            && let Some(n_tok) = rhs.split_whitespace().next()
            && let Some(n) = parse_small_num(n_tok)
            && n > 0
        {
            let after_n = rhs.strip_prefix(n_tok).unwrap_or("").trim();
            if matches!(
                after_n,
                "" | "are created" | "of those tokens" | "of those tokens are created"
            ) {
                (n, 1u8)
            } else {
                return None;
            }
        } else {
            return None;
        };

    Some(StaticModifierRule::ModifyTokensFiltered {
        filter: Box::new(obj_filter),
        controller,
        add,
        multiply,
    })
}

fn split_if_would_instead(full: &str) -> Option<(Option<&str>, &str, &str, &str)> {
    let (leading_cond, after_if) = if let Some(rest) = full.strip_prefix("as long as ")
        && let Some((cond, after_inner)) = rest.split_once(", if ")
    {
        (Some(cond.trim()), after_inner.trim())
    } else {
        let rest = full.strip_prefix("if ")?;
        if let Some((before_would, _)) = rest.split_once(" would ")
            && let Some((cond, _)) = before_would.split_once(", if ")
        {
            let offset = cond.len() + ", if ".len();
            (Some(cond.trim()), rest[offset..].trim())
        } else {
            (None, rest.trim())
        }
    };

    let (s_part, rest) = after_if.split_once(" would ")?;
    let replacement_starters = [
        "prevent ",
        "you may prevent ",
        "it deals ",
        "that source deals ",
        "double ",
        "triple ",
        "twice ",
        "three times ",
        "that much ",
        "half ",
        "put ",
        "that damage ",
        "it is dealt ",
        "it's dealt ",
        "deal ",
        "all of that damage ",
        "instead",
        "you ",
        "that player ",
        "they ",
        "remove ",
        "draw ",
        "exile ",
        "return ",
        "shuffle ",
        "those tokens ",
        "only ",
    ];

    let mut chosen_split: Option<(&str, &str)> = None;
    let mut search_idx = 0usize;
    while let Some(rel) = rest[search_idx..].find(", ") {
        let idx = search_idx + rel;
        let rhs = rest[idx + 2..].trim();
        if replacement_starters.iter().any(|st| rhs.starts_with(st)) {
            chosen_split = Some((rest[..idx].trim(), rhs));
            break;
        }
        search_idx = idx + 2;
    }
    let (w_part, i_part) =
        chosen_split.or_else(|| rest.split_once(", ").map(|(w, i)| (w.trim(), i.trim())))?;
    Some((leading_cond, s_part.trim(), w_part, i_part))
}

fn parse_prevent_all_damage_sentence(
    full: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRule> {
    let (kind, rest) = if let Some(r) = full.strip_prefix("prevent all combat damage that would ") {
        (StaticDamageKind::CombatOnly, r)
    } else if let Some(r) = full.strip_prefix("prevent all noncombat damage that would ") {
        (StaticDamageKind::NoncombatOnly, r)
    } else {
        let r = full.strip_prefix("prevent all damage that would ")?;
        (StaticDamageKind::Any, r)
    };
    let (source, source_controller, recipient) =
        if let Some(after_to) = rest.strip_prefix("be dealt to ") {
            if let Some((recip_raw, by_src)) = after_to.split_once(" by ") {
                let (src, ctrl) = parse_damage_source_filter(by_src.trim(), vocabulary)?;
                let recip = parse_static_damage_recipient(recip_raw.trim(), vocabulary)?;
                (src, ctrl, recip)
            } else {
                let recip = parse_static_damage_recipient(after_to.trim(), vocabulary)?;
                (None, PlayerRelation::AnyParticipant, recip)
            }
        } else {
            let after_by = rest.strip_prefix("be dealt by ")?;
            if let Some((by_src, recip_raw)) = after_by.split_once(" to ") {
                let (src, ctrl) = parse_damage_source_filter(by_src.trim(), vocabulary)?;
                let recip = parse_static_damage_recipient(recip_raw.trim(), vocabulary)?;
                (src, ctrl, recip)
            } else {
                let (src, ctrl) = parse_damage_source_filter(after_by.trim(), vocabulary)?;
                (src, ctrl, StaticDamageRecipient::Any)
            }
        };
    Some(StaticDamageRule::ModifyDamage {
        source,
        source_controller,
        recipient,
        kind,
        op: StaticDamageOp::PreventAll,
    })
}

fn parse_damage_static_condition(
    cond_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticSourceCondition> {
    let trimmed = cond_raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(cond_ast) = spellwake_oracle_compiler::filter::parse_condition(
        trimmed,
        &cached_compiler_vocab(vocabulary),
    ) && let Some(sc) = lower_static_source_condition(&cond_ast, vocabulary)
    {
        return Some(sc);
    }
    lower_world_resolution_condition(trimmed, vocabulary)
        .map(|rc| StaticSourceCondition::Eval(Box::new(rc)))
}

struct ParsedDamageClause {
    source: Option<ObjectFilter>,
    source_controller: PlayerRelation,
    kind: StaticDamageKind,
    recipients: Vec<StaticDamageRecipient>,
    threshold_cap: Option<u8>,
    at_most_prevent: Option<u8>,
}

fn parse_damage_recipients_maybe_compound(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<StaticDamageRecipient>> {
    let t = raw.trim();
    if let Some(single) = parse_static_damage_recipient(t, vocabulary) {
        return Some(vec![single]);
    }
    if let Some(other) = t.strip_prefix("you or ")
        && let Some(r_other) = parse_static_damage_recipient(other.trim(), vocabulary)
    {
        return Some(vec![StaticDamageRecipient::Controller, r_other]);
    }
    None
}

fn parse_damage_threshold_and_kind(
    raw: &str,
) -> Option<(StaticDamageKind, Option<u8>, Option<u8>, &str)> {
    if let Some(rest) = raw.strip_prefix("combat damage") {
        return Some((StaticDamageKind::CombatOnly, None, None, rest.trim()));
    }
    if let Some(rest) = raw.strip_prefix("noncombat damage") {
        return Some((StaticDamageKind::NoncombatOnly, None, None, rest.trim()));
    }
    if let Some(rest) = raw.strip_prefix("damage") {
        return Some((StaticDamageKind::Any, None, None, rest.trim()));
    }
    if let Some(rest) = raw.strip_prefix("more than ") {
        let mut parts = rest.splitn(2, ' ');
        let n_tok = parts.next()?;
        let after_n = parts.next()?.trim();
        let cap = parse_small_num(n_tok)?;
        if let Some(t) = after_n.strip_prefix("combat damage") {
            return Some((StaticDamageKind::CombatOnly, Some(cap), None, t.trim()));
        }
        if let Some(t) = after_n.strip_prefix("noncombat damage") {
            return Some((StaticDamageKind::NoncombatOnly, Some(cap), None, t.trim()));
        }
        let t = after_n.strip_prefix("damage")?;
        return Some((StaticDamageKind::Any, Some(cap), None, t.trim()));
    }
    let mut parts = raw.splitn(2, ' ');
    let n_tok = parts.next()?;
    let after_n = parts.next()?.trim();
    let n = parse_small_num(n_tok)?;
    if n == 0 {
        return None;
    }
    if let Some(after_or_less) = after_n.strip_prefix("or less ") {
        if let Some(t) = after_or_less.strip_prefix("combat damage") {
            return Some((StaticDamageKind::CombatOnly, None, Some(n), t.trim()));
        }
        if let Some(t) = after_or_less.strip_prefix("noncombat damage") {
            return Some((StaticDamageKind::NoncombatOnly, None, Some(n), t.trim()));
        }
        let t = after_or_less.strip_prefix("damage")?;
        return Some((StaticDamageKind::Any, None, Some(n), t.trim()));
    }
    let after_or_more = after_n.strip_prefix("or more ")?;
    if let Some(t) = after_or_more.strip_prefix("combat damage") {
        return Some((StaticDamageKind::CombatOnly, Some(n - 1), None, t.trim()));
    }
    if let Some(t) = after_or_more.strip_prefix("noncombat damage") {
        return Some((StaticDamageKind::NoncombatOnly, Some(n - 1), None, t.trim()));
    }
    let t = after_or_more.strip_prefix("damage")?;
    Some((StaticDamageKind::Any, Some(n - 1), None, t.trim()))
}

fn parse_damage_clause_core(
    subj_low: &str,
    would_core: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ParsedDamageClause> {
    // 1. Passive voice #1: "If [combat|noncombat] damage would be dealt [to <recip>] [by <src>]"
    let passive_kind = match subj_low {
        "damage" | "all damage" | "any damage" => Some(StaticDamageKind::Any),
        "combat damage" | "all combat damage" | "any combat damage" => {
            Some(StaticDamageKind::CombatOnly)
        }
        "noncombat damage" | "all noncombat damage" | "any noncombat damage" => {
            Some(StaticDamageKind::NoncombatOnly)
        }
        _ => None,
    };
    if let Some(kind) = passive_kind {
        if let Some(after_to) = would_core.strip_prefix("be dealt to ") {
            let (source, source_controller, recip_str) =
                if let Some((recip_part, by_src)) = after_to.split_once(" by ") {
                    let (src, ctrl) = parse_damage_source_filter(by_src.trim(), vocabulary)?;
                    (src, ctrl, recip_part.trim())
                } else {
                    (None, PlayerRelation::AnyParticipant, after_to.trim())
                };
            let recipients = parse_damage_recipients_maybe_compound(recip_str, vocabulary)?;
            return Some(ParsedDamageClause {
                source,
                source_controller,
                kind,
                recipients,
                threshold_cap: None,
                at_most_prevent: None,
            });
        }
        if let Some(after_by) = would_core.strip_prefix("be dealt by ") {
            let (src_str, recipients) =
                if let Some((by_src, recip_part)) = after_by.split_once(" to ") {
                    (
                        by_src.trim(),
                        parse_damage_recipients_maybe_compound(recip_part.trim(), vocabulary)?,
                    )
                } else {
                    (after_by.trim(), vec![StaticDamageRecipient::Any])
                };
            let (source, source_controller) = parse_damage_source_filter(src_str, vocabulary)?;
            return Some(ParsedDamageClause {
                source,
                source_controller,
                kind,
                recipients,
                threshold_cap: None,
                at_most_prevent: None,
            });
        }
        return None;
    }

    // 2. Passive voice #2: "If <recipient> would be dealt [threshold] [combat|noncombat] damage [by <src>]"
    if let Some(after_be_dealt) = would_core.strip_prefix("be dealt ") {
        let (kind, threshold_cap, at_most_prevent, tail) =
            parse_damage_threshold_and_kind(after_be_dealt)?;
        let (source, source_controller) = if tail.is_empty() {
            (None, PlayerRelation::AnyParticipant)
        } else {
            let by_src = tail.strip_prefix("by ")?;
            parse_damage_source_filter(by_src.trim(), vocabulary)?
        };
        let recipients = parse_damage_recipients_maybe_compound(subj_low, vocabulary)?;
        return Some(ParsedDamageClause {
            source,
            source_controller,
            kind,
            recipients,
            threshold_cap,
            at_most_prevent,
        });
    }

    // 3. Active voice: "If <source> would deal [threshold] [combat|noncombat] damage [to <recip>]"
    if let Some(after_deal) = would_core.strip_prefix("deal ") {
        let (source, source_controller) = parse_damage_source_filter(subj_low, vocabulary)?;
        let (kind, threshold_cap, at_most_prevent, tail) =
            parse_damage_threshold_and_kind(after_deal)?;
        let recipients = if tail.is_empty() {
            if threshold_cap.is_some() || at_most_prevent.is_some() {
                return None;
            }
            vec![StaticDamageRecipient::Any]
        } else {
            let recip_str = tail.strip_prefix("to ")?;
            parse_damage_recipients_maybe_compound(recip_str.trim(), vocabulary)?
        };
        return Some(ParsedDamageClause {
            source,
            source_controller,
            kind,
            recipients,
            threshold_cap,
            at_most_prevent,
        });
    }

    None
}

fn parse_prevent_followup(
    followup_raw: &str,
    recipient: StaticDamageRecipient,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<DamagePreventFollowup> {
    let f = followup_raw
        .trim()
        .trim_end_matches('.')
        .trim()
        .replace("this creature", "~")
        .replace("this permanent", "~")
        .replace("this artifact", "~")
        .replace("this enchantment", "~");
    let f = f.as_str();
    let f_no_you = f.strip_prefix("you ").unwrap_or(f).trim();

    let is_recip_ref = |r: &str| {
        matches!(
            r.trim(),
            "it" | "that creature" | "that permanent" | "that planeswalker"
        ) || (recipient == StaticDamageRecipient::SelfObject && r.trim() == "~")
    };

    // 1. PutCountersOnRecipient: "put that many <kind> counters on <recip>" or "put a <kind> counter on <recip> for each 1 damage prevented this way"
    if let Some(after_put) = f_no_you.strip_prefix("put ")
        && let Some((before_on, after_on)) = after_put.split_once(" on ")
    {
        let (target_ref, per_from_tail) = if let Some(t) = after_on
            .strip_suffix(" for each 1 damage prevented this way")
            .or_else(|| after_on.strip_suffix(" for each damage prevented this way"))
        {
            (t.trim(), true)
        } else {
            (after_on.trim(), false)
        };
        if is_recip_ref(target_ref) {
            if let Some(kind_raw) = before_on
                .strip_prefix("that many ")
                .and_then(|s| s.strip_suffix(" counters"))
                && let Some(kind) = parse_counter_kind(kind_raw).filter(|k| k.is_permanent())
            {
                return Some(DamagePreventFollowup::PutCountersOnRecipient {
                    kind,
                    per_prevented: true,
                });
            }
            if let Some((kind, 1)) = parse_counter_spec_phrase(before_on) {
                return Some(DamagePreventFollowup::PutCountersOnRecipient {
                    kind,
                    per_prevented: per_from_tail,
                });
            }
        } else if target_ref == "~" {
            if let Some(kind_raw) = before_on
                .strip_prefix("that many ")
                .and_then(|s| s.strip_suffix(" counters"))
                && let Some(kind) = parse_counter_kind(kind_raw).filter(|k| k.is_permanent())
            {
                let body = ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::AbilitySource),
                        kind,
                        amount: ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay),
                    }],
                );
                if validate_inline_followup_body(&body, vocabulary) {
                    return Some(DamagePreventFollowup::InlineBody(Box::new(body)));
                }
            }
            if per_from_tail && let Some((kind, 1)) = parse_counter_spec_phrase(before_on) {
                let body = ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::AbilitySource),
                        kind,
                        amount: ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay),
                    }],
                );
                if validate_inline_followup_body(&body, vocabulary) {
                    return Some(DamagePreventFollowup::InlineBody(Box::new(body)));
                }
            }
        }
    }
    if let Some(after_for) = f_no_you
        .strip_prefix("for each 1 damage prevented this way, put ")
        .or_else(|| f_no_you.strip_prefix("for each damage prevented this way, put "))
        && let Some((before_on, target_ref)) = after_for.split_once(" on ")
        && let Some((kind, 1)) = parse_counter_spec_phrase(before_on)
    {
        if is_recip_ref(target_ref) {
            return Some(DamagePreventFollowup::PutCountersOnRecipient {
                kind,
                per_prevented: true,
            });
        }
        if target_ref.trim() == "~" {
            let body = ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PutCountersOnSubject {
                    subject: Box::new(EffectSubject::AbilitySource),
                    kind,
                    amount: ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay),
                }],
            );
            if validate_inline_followup_body(&body, vocabulary) {
                return Some(DamagePreventFollowup::InlineBody(Box::new(body)));
            }
        }
    }

    // 2. RemoveCountersFromSource
    if let Some(after_rem) = f_no_you.strip_prefix("remove ")
        && let Some((before_from, after_from)) = after_rem.split_once(" from ")
    {
        let (src_ref, per_from_tail) = if let Some(t) = after_from
            .strip_suffix(" for each 1 damage prevented this way")
            .or_else(|| after_from.strip_suffix(" for each damage prevented this way"))
        {
            (t.trim(), true)
        } else {
            (after_from.trim(), false)
        };
        let is_src =
            src_ref == "~" || (recipient == StaticDamageRecipient::SelfObject && src_ref == "it");
        if is_src {
            if let Some(kind_raw) = before_from
                .strip_prefix("that many ")
                .and_then(|s| s.strip_suffix(" counters"))
                && let Some(kind) = parse_counter_kind(kind_raw).filter(|k| k.is_permanent())
            {
                return Some(DamagePreventFollowup::RemoveCountersFromSource {
                    kind,
                    per_prevented: true,
                });
            }
            if let Some((kind, 1)) = parse_counter_spec_phrase(before_from) {
                return Some(DamagePreventFollowup::RemoveCountersFromSource {
                    kind,
                    per_prevented: per_from_tail,
                });
            }
        }
    }
    if let Some(after_for) = f_no_you
        .strip_prefix("for each 1 damage prevented this way, remove ")
        .or_else(|| f_no_you.strip_prefix("for each damage prevented this way, remove "))
        && let Some((before_from, src_ref)) = after_for.split_once(" from ")
        && (src_ref.trim() == "~"
            || (recipient == StaticDamageRecipient::SelfObject && src_ref.trim() == "it"))
        && let Some((kind, 1)) = parse_counter_spec_phrase(before_from)
    {
        return Some(DamagePreventFollowup::RemoveCountersFromSource {
            kind,
            per_prevented: true,
        });
    }

    // 3. InlineBody fast-paths: gain that much life / draw that many cards / create that many tokens
    if matches!(
        f_no_you,
        "gain that much life"
            | "gain life equal to the damage prevented this way"
            | "gain 1 life for each 1 damage prevented this way"
            | "for each 1 damage prevented this way, you gain 1 life"
            | "for each 1 damage prevented this way, gain 1 life"
    ) {
        let body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay),
            }],
        );
        if validate_inline_followup_body(&body, vocabulary) {
            return Some(DamagePreventFollowup::InlineBody(Box::new(body)));
        }
    }
    if matches!(
        f_no_you,
        "draw that many cards"
            | "draw cards equal to the damage prevented this way"
            | "draw a card for each 1 damage prevented this way"
            | "for each 1 damage prevented this way, draw a card"
            | "for each 1 damage prevented this way, you draw a card"
    ) {
        let body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay),
            }],
        );
        if validate_inline_followup_body(&body, vocabulary) {
            return Some(DamagePreventFollowup::InlineBody(Box::new(body)));
        }
    }
    if let Some(after_create) = f_no_you.strip_prefix("create ") {
        let tok_phrase = if let Some(rest) = after_create.strip_prefix("that many ") {
            Some(
                rest.strip_suffix(" tokens")
                    .map(|s| format!("a {s} token"))
                    .unwrap_or_else(|| format!("a {rest}")),
            )
        } else {
            after_create
                .strip_suffix(" for each 1 damage prevented this way")
                .or_else(|| after_create.strip_suffix(" for each damage prevented this way"))
                .map(|s| s.trim().to_string())
        };
        if let Some(tp) = tok_phrase
            && let Ok(tok_ast) = spellwake_oracle_compiler::filter::parse_token_creation(
                &tp,
                &cached_compiler_vocab(vocabulary),
            )
            && let Ok((mut creation, false)) = lower_token_spec(
                &tok_ast,
                TokenRecipients::Controller,
                vocabulary,
                ids,
                None,
                None,
            )
            && creation.base_size.is_none()
        {
            creation.amount = ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay);
            let body = ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![creation],
                }],
            );
            if validate_inline_followup_body(&body, vocabulary) {
                return Some(DamagePreventFollowup::InlineBody(Box::new(body)));
            }
        }
    }

    // 4. General InlineBody fallback
    let (ins, fails) = spellwake_oracle_compiler::clauses::parse_effect_body(
        f,
        &cached_compiler_vocab(vocabulary),
    );
    if !fails.is_empty() || ins.is_empty() {
        return None;
    }
    let mut body = lower_effect_body(&ins, vocabulary, ids, false, true).ok()?;
    rewrite_body_prior_result(&mut body, EventValueKind::DamagePreventedThisWay);
    if validate_inline_followup_body(&body, vocabulary) {
        Some(DamagePreventFollowup::InlineBody(Box::new(body)))
    } else {
        None
    }
}

fn parse_static_damage_op_full(
    instead_norm: &str,
    recipient: StaticDamageRecipient,
    threshold_cap: Option<u8>,
    at_most_prevent: Option<u8>,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<StaticDamageOp> {
    let target_suffix_ok = |tail: &str| {
        matches!(
            tail,
            "" | "to that player"
                | "to that permanent or player"
                | "to that player or permanent"
                | "to that creature"
                | "to that permanent"
                | "to it"
                | "to you"
                | "to ~"
                | "to this creature"
                | "to this permanent"
                | "to equipped creature"
                | "to enchanted creature"
                | "to enchanted player"
                | "is dealt"
                | "is dealt to that player"
                | "is dealt to that permanent or player"
                | "is dealt to that player or permanent"
                | "is dealt to that creature"
                | "is dealt to that permanent"
                | "is dealt to it"
                | "is dealt to you"
                | "is dealt to ~"
                | "is dealt to this creature"
                | "is dealt to this permanent"
                | "is dealt to equipped creature"
                | "is dealt to enchanted creature"
                | "is dealt to enchanted player"
        )
    };
    let after_deals = instead_norm
        .strip_prefix("it deals ")
        .or_else(|| instead_norm.strip_prefix("that source deals "));

    // 1. Threshold / at-most variants
    if let Some(max_prevent) = at_most_prevent {
        return if matches!(
            instead_norm,
            "prevent that damage" | "prevent all of that damage" | "prevent all that damage"
        ) {
            Some(StaticDamageOp::PreventAtMost(max_prevent))
        } else {
            None
        };
    }
    if let Some(expected_cap) = threshold_cap {
        let rest = after_deals
            .or_else(|| instead_norm.strip_prefix("only "))
            .unwrap_or(instead_norm);
        let mut parts = rest.splitn(2, ' ');
        let k_tok = parts.next()?;
        let after_k = parts.next()?.trim();
        let k = parse_small_num(k_tok)?;
        if k != expected_cap {
            return None;
        }
        let tail = after_k
            .strip_prefix("combat damage")
            .or_else(|| after_k.strip_prefix("noncombat damage"))
            .or_else(|| after_k.strip_prefix("damage"))?
            .trim();
        return target_suffix_ok(tail).then_some(StaticDamageOp::CapAt(k));
    }

    // 2. PreventAndFollowup ("prevent ... damage. <followup>" or "prevent ... damage and <followup>")
    let prevent_followup_split: Option<(DamagePreventMode, &str)> = [
        (
            "prevent all of that combat damage and ",
            DamagePreventMode::All,
        ),
        (
            "prevent all of that combat damage. ",
            DamagePreventMode::All,
        ),
        (
            "prevent all of that noncombat damage and ",
            DamagePreventMode::All,
        ),
        (
            "prevent all of that noncombat damage. ",
            DamagePreventMode::All,
        ),
        ("prevent all of that damage and ", DamagePreventMode::All),
        ("prevent all of that damage. ", DamagePreventMode::All),
        ("prevent all that damage and ", DamagePreventMode::All),
        ("prevent all that damage. ", DamagePreventMode::All),
        ("prevent that combat damage and ", DamagePreventMode::All),
        ("prevent that combat damage. ", DamagePreventMode::All),
        ("prevent that noncombat damage and ", DamagePreventMode::All),
        ("prevent that noncombat damage. ", DamagePreventMode::All),
        ("prevent that damage and ", DamagePreventMode::All),
        ("prevent that damage. ", DamagePreventMode::All),
        (
            "prevent half that damage, rounded up, and ",
            DamagePreventMode::Half { round_up: true },
        ),
        (
            "prevent half that damage, rounded up. ",
            DamagePreventMode::Half { round_up: true },
        ),
        (
            "prevent half of that damage, rounded up, and ",
            DamagePreventMode::Half { round_up: true },
        ),
        (
            "prevent half of that damage, rounded up. ",
            DamagePreventMode::Half { round_up: true },
        ),
        (
            "prevent half that damage, rounded down, and ",
            DamagePreventMode::Half { round_up: false },
        ),
        (
            "prevent half that damage, rounded down. ",
            DamagePreventMode::Half { round_up: false },
        ),
        (
            "prevent half of that damage, rounded down, and ",
            DamagePreventMode::Half { round_up: false },
        ),
        (
            "prevent half of that damage, rounded down. ",
            DamagePreventMode::Half { round_up: false },
        ),
    ]
    .into_iter()
    .find_map(|(pref, mode)| instead_norm.strip_prefix(pref).map(|f| (mode, f)))
    .or_else(|| {
        let rest = instead_norm
            .strip_prefix("prevent up to x of that damage, where x is the number of ")
            .or_else(|| {
                instead_norm.strip_prefix("prevent x of that damage, where x is the number of ")
            })?;
        let (kind_raw, after_counters) = rest.split_once(" counters on ")?;
        let kind = parse_counter_kind(kind_raw.trim()).filter(|k| k.is_permanent())?;
        let followup = after_counters
            .strip_prefix("~, and ")
            .or_else(|| after_counters.strip_prefix("~. "))
            .or_else(|| after_counters.strip_prefix("it, and "))
            .or_else(|| after_counters.strip_prefix("it. "))
            .or_else(|| after_counters.strip_prefix("this creature, and "))
            .or_else(|| after_counters.strip_prefix("this creature. "))
            .or_else(|| after_counters.strip_prefix("this permanent, and "))
            .or_else(|| after_counters.strip_prefix("this permanent. "))?;
        Some((DamagePreventMode::UpToSourceCounters(kind), followup))
    })
    .or_else(|| {
        let rest = instead_norm.strip_prefix("prevent ")?;
        let (n_tok, tail) = rest.split_once(' ')?;
        let n = parse_small_num(n_tok).filter(|&v| v > 0)?;
        let f = tail
            .strip_prefix("of that damage and ")
            .or_else(|| tail.strip_prefix("of that damage. "))?;
        Some((DamagePreventMode::UpTo(n), f))
    });
    if let Some((mode, followup_str)) = prevent_followup_split
        && let Some(followup) = parse_prevent_followup(followup_str, recipient, vocabulary, ids)
    {
        return Some(StaticDamageOp::PreventAndFollowup { mode, followup });
    }

    // 3. Dynamic PreventUpToExpr (Battletide Alchemist: "[you may] prevent X of that damage, where X is <expr>")
    let prev_core = instead_norm
        .strip_prefix("you may ")
        .unwrap_or(instead_norm)
        .trim();
    if let Some(after_prev) = prev_core.strip_prefix("prevent ") {
        let dyn_expr_raw = if let Some((head, where_expr)) = after_prev.split_once(", where x is ")
            && matches!(
                head.trim(),
                "x of that damage" | "up to x of that damage" | "x damage"
            ) {
            Some(where_expr.trim())
        } else {
            after_prev
                .strip_prefix("damage equal to ")
                .or_else(|| after_prev.strip_prefix("an amount of that damage equal to "))
                .map(str::trim)
        };
        if let Some(expr_raw) = dyn_expr_raw
            && let Ok(ast_expr) = spellwake_oracle_compiler::filter::parse_value_expr(
                expr_raw,
                &cached_compiler_vocab(vocabulary),
            )
            && let Ok(val_expr) = lower_value_expr(&ast_expr, vocabulary)
            && val_expr.is_world_evaluable()
            && !matches!(val_expr, ValueExpr::Literal(ref v) if !v.is_positive())
        {
            return Some(StaticDamageOp::PreventUpToExpr(Box::new(val_expr)));
        }
    }

    // 4. Replace with -1/-1 counters
    if instead_norm == "put that many -1/-1 counters on it"
        || instead_norm == "put that many -1/-1 counters on that creature"
    {
        if !matches!(
            recipient,
            StaticDamageRecipient::OpponentCreatures
                | StaticDamageRecipient::ControlledCreatures
                | StaticDamageRecipient::Creatures
        ) {
            return None;
        }
        return Some(StaticDamageOp::ReplaceWithMinusOneCounters);
    }

    // 5. PreventHalf (active + passive)
    if matches!(
        instead_norm,
        "prevent half that damage, rounded up"
            | "prevent half of that damage, rounded up"
            | "it deals half that much damage, rounded down"
            | "that source deals half that much damage, rounded down"
            | "half that much damage, rounded down, is dealt"
            | "half of that damage, rounded down, is dealt"
    ) {
        return Some(StaticDamageOp::PreventHalf { round_up: true });
    }
    if matches!(
        instead_norm,
        "prevent half that damage, rounded down"
            | "prevent half of that damage, rounded down"
            | "it deals half that much damage, rounded up"
            | "that source deals half that much damage, rounded up"
            | "half that much damage, rounded up, is dealt"
            | "half of that damage, rounded up, is dealt"
    ) {
        return Some(StaticDamageOp::PreventHalf { round_up: false });
    }

    // 6. PreventAll / CapAt / PreventUpTo
    if matches!(
        instead_norm,
        "prevent that damage"
            | "prevent all of that damage"
            | "prevent all that damage"
            | "prevent that combat damage"
            | "prevent that noncombat damage"
            | "prevent it"
    ) {
        return Some(StaticDamageOp::PreventAll);
    }
    if let Some(n) = instead_norm
        .strip_prefix("prevent all but ")
        .and_then(|r| r.strip_suffix(" of that damage"))
        .and_then(parse_small_num)
        && n > 0
    {
        return Some(StaticDamageOp::CapAt(n));
    }
    if let Some(rest) = instead_norm.strip_prefix("prevent ")
        && let Some((n_tok, tail)) = rest.split_once(' ')
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && (tail == "of that damage"
            || tail
                .strip_prefix("damage that spell would deal")
                .or_else(|| tail.strip_prefix("damage that source would deal"))
                .or_else(|| tail.strip_prefix("damage it would deal"))
                .is_some_and(|t| target_suffix_ok(t.trim())))
    {
        return Some(StaticDamageOp::PreventUpTo(n));
    }

    // 7. Subtract / Multiply / Add / AddExpr (both active "it deals ..." and passive "... is dealt")
    let expr_head = after_deals.unwrap_or(instead_norm);
    if let Some(rest) = expr_head.strip_prefix("that much damage minus ")
        && let Some(n_tok) = rest.split_whitespace().next()
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && target_suffix_ok(rest.strip_prefix(n_tok).unwrap_or("").trim())
    {
        return Some(StaticDamageOp::Subtract(n));
    }
    if let Some(n_tok) = expr_head.split_whitespace().next()
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && let Some(after_less) = expr_head
            .strip_prefix(n_tok)
            .unwrap_or("")
            .trim()
            .strip_prefix("less damage")
        && target_suffix_ok(after_less.trim())
    {
        return Some(StaticDamageOp::Subtract(n));
    }
    if let Some(tail) = expr_head
        .strip_prefix("double that damage")
        .or_else(|| expr_head.strip_prefix("twice that much damage"))
        && target_suffix_ok(tail.trim())
    {
        return Some(StaticDamageOp::Multiply(2));
    }
    if let Some(tail) = expr_head
        .strip_prefix("triple that damage")
        .or_else(|| expr_head.strip_prefix("three times that much damage"))
        && target_suffix_ok(tail.trim())
    {
        return Some(StaticDamageOp::Multiply(3));
    }

    let rest = expr_head.strip_prefix("that much damage plus ")?;
    if let Some(n_tok) = rest.split_whitespace().next()
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && target_suffix_ok(rest.strip_prefix(n_tok).unwrap_or("").trim())
    {
        return Some(StaticDamageOp::Add(n));
    }
    fn strip_target_tail(s: &str) -> &str {
        let s = s.trim();
        let s = s
            .strip_suffix(" to that permanent or player")
            .or_else(|| s.strip_suffix(" to that player or permanent"))
            .or_else(|| s.strip_suffix(" to that player"))
            .or_else(|| s.strip_suffix(" to that creature"))
            .or_else(|| s.strip_suffix(" to that permanent"))
            .or_else(|| s.strip_suffix(" to it"))
            .or_else(|| s.strip_suffix(" to you"))
            .or_else(|| s.strip_suffix(" to ~"))
            .or_else(|| s.strip_suffix(" to this creature"))
            .or_else(|| s.strip_suffix(" to this permanent"))
            .or_else(|| s.strip_suffix(" to equipped creature"))
            .or_else(|| s.strip_suffix(" to enchanted creature"))
            .or_else(|| s.strip_suffix(" to enchanted player"))
            .unwrap_or(s)
            .trim();
        s.strip_suffix(" is dealt").unwrap_or(s).trim()
    }
    let expr_raw = if let Some((before_where, where_expr)) = rest.split_once(", where x is ") {
        let head = before_where.trim().trim_end_matches(" instead").trim();
        if strip_target_tail(head) == "x" {
            where_expr.trim()
        } else {
            return None;
        }
    } else {
        let tail = strip_target_tail(rest);
        tail.strip_prefix("an amount of damage equal to ")
            .or_else(|| tail.strip_prefix("an amount equal to "))
            .or_else(|| tail.strip_prefix("damage equal to "))
            .or_else(|| tail.strip_prefix("equal to "))
            .unwrap_or(tail)
            .trim()
    };
    let ast_expr = spellwake_oracle_compiler::filter::parse_value_expr(
        expr_raw,
        &cached_compiler_vocab(vocabulary),
    )
    .ok()?;
    let val_expr = lower_value_expr(&ast_expr, vocabulary).ok()?;
    if !val_expr.is_world_evaluable()
        || matches!(val_expr, ValueExpr::Literal(ref v) if !v.is_positive())
    {
        return None;
    }
    Some(StaticDamageOp::AddExpr(Box::new(val_expr)))
}

fn parse_damage_rules_full(
    leading_cond: Option<&str>,
    subj_low: &str,
    would_norm: &str,
    instead_norm: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Vec<StaticDamageRule>> {
    // 1. Peel optional leading condition embedded in `subj_low` ("<cond>, if <actual_subj>")
    let (cond_lead_str, eff_subj) = if leading_cond.is_some() {
        (leading_cond, subj_low)
    } else if let Some((c_part, s_part)) = subj_low.split_once(", if ") {
        (Some(c_part.trim()), s_part.trim())
    } else {
        (None, subj_low)
    };

    // 2. Peel optional trailing condition on `would_norm` ("... while <cond>" / "... if <cond>")
    let (eff_would, cond_trail_str) =
        if let Some((w_part, c_part)) = would_norm.split_once(" while ") {
            (w_part.trim(), Some(c_part.trim()))
        } else if let Some((w_part, c_part)) = would_norm.split_once(" if ") {
            (w_part.trim(), Some(c_part.trim()))
        } else {
            (would_norm, None)
        };

    let cond_lead = match cond_lead_str {
        Some(c) => Some(parse_damage_static_condition(c, vocabulary)?),
        None => None,
    };
    let cond_trail = match cond_trail_str {
        Some(c) => Some(parse_damage_static_condition(c, vocabulary)?),
        None => None,
    };
    let combined_cond = match (cond_lead, cond_trail) {
        (Some(a), Some(b)) => Some(StaticSourceCondition::And(Box::new(a), Box::new(b))),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    };

    let parsed = parse_damage_clause_core(eff_subj, eff_would, vocabulary)?;
    let mut rules = Vec::with_capacity(parsed.recipients.len());
    for recipient in parsed.recipients {
        let op = parse_static_damage_op_full(
            instead_norm,
            recipient,
            parsed.threshold_cap,
            parsed.at_most_prevent,
            vocabulary,
            ids,
        )?;
        let rule = match &combined_cond {
            Some(cond) => StaticDamageRule::ConditionalModifyDamage {
                condition: Box::new(cond.clone()),
                source: parsed.source,
                source_controller: parsed.source_controller,
                recipient,
                kind: parsed.kind,
                op,
            },
            None => StaticDamageRule::ModifyDamage {
                source: parsed.source,
                source_controller: parsed.source_controller,
                recipient,
                kind: parsed.kind,
                op,
            },
        };
        let probe = StaticContinuousAbility::DamageRule {
            id: StaticAbilityId(0),
            rule: rule.clone(),
        };
        if probe.validate(vocabulary).is_err() {
            return None;
        }
        rules.push(rule);
    }
    if rules.is_empty() { None } else { Some(rules) }
}
