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
                let zone = match inner.zone.take() {
                    None => spellwake_engine::program::ValueZone::Battlefield,
                    Some(ZoneFilter::YourGraveyard) => {
                        spellwake_engine::program::ValueZone::Graveyard(ResolvingPlayer::Controller)
                    }
                    Some(ZoneFilter::AllGraveyards) => {
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
                None | Some(ZoneFilter::Battlefield) => {
                    spellwake_engine::program::EntryCopyZone::Battlefield
                }
                Some(ZoneFilter::YourGraveyard) => {
                    spellwake_engine::program::EntryCopyZone::OwnGraveyard
                }
                Some(ZoneFilter::AnyGraveyard) => {
                    spellwake_engine::program::EntryCopyZone::AnyGraveyard
                }
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
            let mut spec = spellwake_engine::program::EntryCopySpec {
                zone,
                filter: obj_filter,
                nonlegendary: false,
                extra_types: BTreeSet::new(),
                extra_creature_subtypes: BTreeSet::new(),
                extra_keywords: BTreeSet::new(),
                extra_counters: None,
                extra_activated: None,
                extra_triggered: None,
            };
            if let Some(exc_raw) = exception.as_deref() {
                let mut rest = exc_raw.trim().to_string();
                // Extract any quoted ability first (e.g. `except it has "{2}{U}{U}: Return ~ to its owner's hand."`)
                if let Some(q1) = rest.find('"')
                    && let Some(q2_rel) = rest[q1 + 1..].find('"')
                {
                    let quoted = rest[q1 + 1..q1 + 1 + q2_rel].trim().to_string();
                    let before = rest[..q1].trim();
                    let after = rest[q1 + 2 + q2_rel..].trim();
                    let norm_q = quoted
                        .replace("this creature", "~")
                        .replace("This creature", "~")
                        .replace("this permanent", "~")
                        .replace("This permanent", "~");
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
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
                    match lower_granted_ability_face(&synthetic.faces[0], vocabulary, ids)
                        .map_err(|_| "oracle:unsupported-enters-as-copy")?
                    {
                        StaticPart::GrantActivated(ab) => {
                            spec.extra_activated = Some(ab);
                        }
                        StaticPart::GrantTrigger(tr) => {
                            spec.extra_triggered = Some(tr);
                        }
                        _ => return Err("oracle:unsupported-enters-as-copy"),
                    }
                    rest = format!("{before} {after}").trim().to_string();
                }
                let mut low = rest.to_ascii_lowercase();
                for marker in ["except ", "and "] {
                    low = low.replace(marker, " ");
                }
                if low.contains("isn't legendary") || low.contains("is not legendary") {
                    spec.nonlegendary = true;
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
                        low = low.replace(phrase, "");
                    }
                }
                if low.contains("enters with an additional +1/+1 counter")
                    || low.contains("enters with a +1/+1 counter")
                {
                    spec.extra_counters = Some((CounterKind::PlusOne, 1));
                    for phrase in [
                        "it enters with an additional +1/+1 counter on it",
                        "he enters with an additional +1/+1 counter on him",
                        "she enters with an additional +1/+1 counter on her",
                        "he enters with an additional +1/+1 counter on it",
                        "she enters with an additional +1/+1 counter on it",
                        "enters with an additional +1/+1 counter on it",
                        "enters with an additional +1/+1 counter on him",
                        "enters with an additional +1/+1 counter on her",
                        "it enters with a +1/+1 counter on it",
                        "he enters with a +1/+1 counter on him",
                        "she enters with a +1/+1 counter on her",
                        "he enters with a +1/+1 counter on it",
                        "she enters with a +1/+1 counter on it",
                        "enters with a +1/+1 counter on it",
                        "enters with a +1/+1 counter on him",
                        "enters with a +1/+1 counter on her",
                    ] {
                        low = low.replace(phrase, "");
                    }
                } else if low.contains("enters with a shield counter") {
                    spec.extra_counters = Some((CounterKind::Shield, 1));
                    for phrase in [
                        "it enters with a shield counter on it",
                        "he enters with a shield counter on him",
                        "she enters with a shield counter on her",
                        "he enters with a shield counter on it",
                        "she enters with a shield counter on it",
                        "enters with a shield counter on it",
                        "enters with a shield counter on him",
                        "enters with a shield counter on her",
                    ] {
                        low = low.replace(phrase, "");
                    }
                }
                let addition_match = [
                    "in addition to its other creature types",
                    "in addition to his other creature types",
                    "in addition to her other creature types",
                    "in addition to its other types",
                    "in addition to his other types",
                    "in addition to her other types",
                ]
                .into_iter()
                .filter_map(|suffix| low.find(suffix).map(|pos| (pos, suffix.len())))
                .min_by_key(|(pos, _)| *pos);
                if let Some((pos, suffix_len)) = addition_match {
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
                        let w = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                        match w {
                            "" | "a" | "an" | "it" | "he" | "she" | "is" | "it's" | "he's"
                            | "she's" => {}
                            "artifact" => {
                                spec.extra_types.insert(CardType::Artifact);
                            }
                            "enchantment" => {
                                spec.extra_types.insert(CardType::Enchantment);
                            }
                            "creature" => {
                                spec.extra_types.insert(CardType::Creature);
                            }
                            "land" => {
                                spec.extra_types.insert(CardType::Land);
                            }
                            _ => {
                                if let Some(ct) = resolve_creature_subtype_token(w, vocabulary) {
                                    spec.extra_creature_subtypes.insert(ct);
                                } else {
                                    return Err("oracle:unsupported-enters-as-copy");
                                }
                            }
                        }
                    }
                    let clause_end = pos + suffix_len;
                    low = format!("{} {}", &low[..clause_start], &low[clause_end..]);
                }
                for kw_name in [
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
                    "first strike",
                    "double strike",
                    "reach",
                    "flash",
                ] {
                    if let Some(ka) = lower_keyword_ability_name(kw_name) {
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
                                spec.extra_keywords.insert(ka);
                                low = low.replace(&phrase, "");
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
                    .any(|tok| !matches!(tok, "it" | "he" | "she"));
                if has_unrecognized {
                    return Err("oracle:unsupported-enters-as-copy");
                }
            }
            if program
                .entry_copy
                .replace(EntryCopyRule::Matching(Box::new(spec)))
                .is_some()
            {
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

    // 1. Special full-sentence damage prevention / replacement shapes emitted by clauses.rs
    if is_permanent && would_norm == "reduce_life_below_floor" {
        let floor = instead_raw
            .split("less than ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|tok| tok.parse::<u8>().ok())
            .filter(|&n| n > 0);
        if let Some(floor) = floor {
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
    }
    if is_permanent
        && would_norm == "would_event"
        && matches!(
            subj_low.as_str(),
            "damage" | "if you control a creature, damage"
        )
        && let Some(after_less) = instead_norm.strip_prefix("reduce your life total to less than ")
        && let Some((floor_raw, target_raw)) = after_less.split_once(" reduces it to ")
        && let (Ok(floor), Ok(target)) = (
            floor_raw.trim().parse::<u8>(),
            target_raw.trim().parse::<u8>(),
        )
        && floor > 0
        && floor == target
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
    if is_permanent && (would_norm == "special_replacement" || subj_low == "damage_source") {
        let full = instead_raw.as_str();
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
        if let Some(after_if) = full.strip_prefix("if ")
            && let Some((s_part, rest)) = after_if.split_once(" would ")
            && let Some((w_part, i_part)) = rest.split_once(", ")
            && let Some(rule) = parse_modify_damage_event(
                s_part.trim(),
                w_part.trim(),
                i_part.trim().trim_end_matches(" instead").trim(),
                vocabulary,
            )
        {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule,
                },
            ));
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
    {
        let dest = if matches!(instead_norm, "exile it" | "exile ~" | "exile this card") {
            Some(PublicZoneDestination::Exile)
        } else if matches!(
            instead_norm,
            "reveal ~ and shuffle it into its owner's library"
                | "reveal it and shuffle it into its owner's library"
                | "shuffle ~ into its owner's library"
                | "shuffle it into its owner's library"
                | "put ~ on the bottom of its owner's library"
                | "put it on the bottom of its owner's library"
                | "put ~ on top of its owner's library"
                | "put it on top of its owner's library"
        ) {
            Some(PublicZoneDestination::Library)
        } else {
            None
        };
        if let Some(replace_with) = dest {
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
            return Some(Ok(()));
        }
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
    {
        let dest = if matches!(
            instead_norm,
            "exile it"
                | "exile ~"
                | "exile this creature"
                | "exile this permanent"
                | "exile this artifact"
                | "exile this enchantment"
                | "exile this card"
        ) {
            Some(PublicZoneDestination::Exile)
        } else if matches!(
            instead_norm,
            "shuffle ~ into its owner's library"
                | "shuffle it into its owner's library"
                | "shuffle ~ into your library"
                | "shuffle it into your library"
                | "reveal ~ and shuffle it into its owner's library"
                | "put it on top of its owner's library"
                | "put ~ on top of its owner's library"
                | "put it on the bottom of its owner's library"
                | "put ~ on the bottom of its owner's library"
        ) {
            Some(PublicZoneDestination::Library)
        } else {
            None
        };
        if let Some(replace_with) = dest {
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
                choice: ZoneReplacementChoice::Mandatory,
            });
            return Some(Ok(()));
        }
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
        && matches!(
            instead_norm,
            "exile it" | "exile that creature" | "exile that permanent" | "exile that card"
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
            choice: ZoneReplacementChoice::Mandatory,
        });
        return Some(Ok(()));
    }
    if matches!(
        subj_low.as_str(),
        "a card or token" | "a card" | "any card" | "any card or token"
    ) && matches!(
        would_norm,
        "be put into an opponent's graveyard from anywhere"
            | "be put into a graveyard from anywhere"
            | "be put into your graveyard from anywhere"
            | "be put into a player's graveyard from anywhere"
            | "be put into its owner's graveyard from anywhere"
    ) && matches!(
        instead_norm,
        "exile it" | "exile that card" | "exile that permanent"
    ) {
        let id = next_zone_rep_id(program);
        let objects = if matches!(subj_low.as_str(), "a card" | "any card") {
            ZoneObjectKind::Card
        } else {
            ZoneObjectKind::Either
        };
        let subject = if would_norm.contains("opponent's graveyard") {
            ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::any()),
                owner_relation: Some(PlayerRelation::Opponent),
            }
        } else if would_norm.contains("your graveyard") {
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
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::Mandatory,
        });
        return Some(Ok(()));
    }
    if subj_low == "a card you didn't control"
        && would_norm == "be put into an opponent's graveyard from anywhere"
        && matches!(instead_norm, "exile it" | "exile that card")
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
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::Mandatory,
        });
        return Some(Ok(()));
    }
    if subj_low == "a nontoken creature an opponent owns"
        && would_norm
            == "die or a creature card not on the battlefield would be put into an opponent's graveyard"
        && matches!(instead_norm, "exile that card" | "exile it")
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
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::Mandatory,
        });
        return Some(Ok(()));
    }
    if subj_low == "a black or red permanent, spell, or card not on the battlefield"
        && would_norm == "be put into a graveyard"
        && matches!(instead_norm, "exile it" | "exile that card")
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
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::Mandatory,
        });
        return Some(Ok(()));
    }
    let is_battlefield_to_gy = matches!(
        would_norm,
        "die"
            | "be put into a graveyard from the battlefield"
            | "be put into an opponent's graveyard from the battlefield"
            | "be put into your graveyard from the battlefield"
            | "be put into its owner's graveyard from the battlefield"
    ) || (matches!(
        would_norm,
        "be put into a graveyard"
            | "be put into an opponent's graveyard"
            | "be put into your graveyard"
            | "be put into its owner's graveyard"
    ) && (subj_low.contains("permanent")
        || subj_low.contains("creature"))
        && !subj_low.contains("card")
        && !subj_low.contains("spell"));
    if is_battlefield_to_gy {
        let (choice, instead_core) = if let Some(rest) = instead_norm.strip_prefix("you may ") {
            (ZoneReplacementChoice::OptionalAffectedPlayer, rest.trim())
        } else {
            (ZoneReplacementChoice::Mandatory, instead_norm)
        };
        let replace_dest = if matches!(
            instead_core,
            "exile it"
                | "exile that creature"
                | "exile that permanent"
                | "exile that card"
                | "exile that planeswalker"
                | "exile that token"
        ) {
            Some(PublicZoneDestination::Exile)
        } else if choice == ZoneReplacementChoice::Mandatory
            && !instead_core.contains(" and ")
            && !instead_core.contains(". ")
            && instead_core.contains("shuffle")
            && instead_core.contains("into its owner's library")
        {
            Some(PublicZoneDestination::Library)
        } else {
            None
        };
        if let Some(replace_with) = replace_dest {
            let owner_relation = if would_norm.contains("opponent's graveyard") {
                Some(PlayerRelation::Opponent)
            } else if would_norm.contains("your graveyard") {
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
            let matched: Option<(ObjectFilter, ZoneObjectKind, Option<PlayerRelation>)> =
                match subj_low.as_str() {
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
                    "a creature or planeswalker" => {
                        Some((creature_or_pw(), ZoneObjectKind::Either, None))
                    }
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
                    "a permanent" => {
                        Some((ObjectFilter::permanent(), ZoneObjectKind::Either, None))
                    }
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
                    return Some(Ok(()));
                }
            }
        }
    }
    if matches!(
        would_norm,
        "be put into a graveyard from anywhere"
            | "be put into an opponent's graveyard from anywhere"
            | "be put into your graveyard from anywhere"
    ) && matches!(
        instead_norm,
        "exile it" | "exile that card" | "exile that permanent" | "exile that creature"
    ) && let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
        &subj_low,
        &cached_compiler_vocab(vocabulary),
    ) && !(ast_f.any_spell && ast_f.any_types.is_empty())
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
        let owner_relation = if would_norm.contains("opponent's graveyard") {
            Some(PlayerRelation::Opponent)
        } else if would_norm.contains("your graveyard") {
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
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::Mandatory,
        });
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
        let rest = s
            .strip_prefix("you draw that many cards plus ")
            .or_else(|| s.strip_prefix("draw that many cards plus "))?;
        parse_small_num(rest.trim()).filter(|&n| n > 0)
    };
    if subj_low == "you"
        && matches!(
            would_norm,
            "draw a card except the first one you draw in each of your draw steps"
                | "draw a card except the first one you draw in each of your draw steps,"
        )
        && let Some(multiply) = draw_mult_count(instead_norm)
    {
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
    if subj_low == "you"
        && would_norm == "draw a card while you have no cards in hand"
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
                    require_empty_hand: true,
                },
            },
        ));
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
        && matches!(
            instead_norm,
            "that mana becomes colorless" | "that mana becomes colorless instead"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::RetainMana(ManaType::Colourless),
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
                && let Ok((creation, false)) =
                    lower_token_spec(&tok_ast, TokenRecipients::Controller, vocabulary, ids, None)
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
    if let Some(rule) = parse_modify_damage_event(&subj_low, would_norm, instead_norm, vocabulary) {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule,
            },
        ));
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
    let cond_ast = spellwake_oracle_compiler::filter::parse_condition(
        cond_raw.trim(),
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
        "~" | "this creature" | "this permanent" => Some(StaticDamageRecipient::SelfObject),
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
            if cond_raw.trim() == "your library has no cards in it" {
                None
            } else {
                lower_world_resolution_condition(cond_raw, vocabulary).map(|rc| (true, Some(rc)))
            }
        } else if let Some(cond_raw) = would_norm
            .strip_prefix("draw one or more cards while ")
            .or_else(|| would_norm.strip_prefix("draw one or more cards if "))
        {
            if cond_raw.trim() == "your library has no cards in it" {
                None
            } else {
                lower_world_resolution_condition(cond_raw, vocabulary).map(|rc| (false, Some(rc)))
            }
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
    let (optional, action_str) = if let Some(rest) = instead_raw
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
    let body = lower_effect_body(&ins, vocabulary, ids, false, true).ok()?;
    if body.target != BodyTarget::None {
        return None;
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
        "~" | "this creature" | "this permanent" => (
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
                    return None;
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
    let (source, source_controller) = parse_damage_source_filter(subj_low, vocabulary)?;

    let mut threshold_cap: Option<u8> = None;
    let mut at_most_prevent: Option<u8> = None;
    let (kind, recipient) = if would_norm == "deal combat damage" {
        (StaticDamageKind::CombatOnly, StaticDamageRecipient::Any)
    } else if would_norm == "deal noncombat damage" {
        (StaticDamageKind::NoncombatOnly, StaticDamageRecipient::Any)
    } else if would_norm == "deal damage" {
        (StaticDamageKind::Any, StaticDamageRecipient::Any)
    } else {
        let (kind, rest_target) =
            if let Some(rest) = would_norm.strip_prefix("deal combat damage to ") {
                (StaticDamageKind::CombatOnly, rest)
            } else if let Some(rest) = would_norm.strip_prefix("deal noncombat damage to ") {
                (StaticDamageKind::NoncombatOnly, rest)
            } else if let Some(rest) = would_norm.strip_prefix("deal damage to ") {
                (StaticDamageKind::Any, rest)
            } else if let Some(rest) = would_norm.strip_prefix("deal more than ") {
                let mut parts = rest.splitn(2, ' ');
                let n_tok = parts.next()?;
                let after_n = parts.next()?.trim();
                let cap = parse_small_num(n_tok)?;
                threshold_cap = Some(cap);
                if let Some(t) = after_n.strip_prefix("combat damage to ") {
                    (StaticDamageKind::CombatOnly, t)
                } else if let Some(t) = after_n.strip_prefix("noncombat damage to ") {
                    (StaticDamageKind::NoncombatOnly, t)
                } else {
                    let t = after_n.strip_prefix("damage to ")?;
                    (StaticDamageKind::Any, t)
                }
            } else {
                let rest = would_norm.strip_prefix("deal ")?;
                let mut parts = rest.splitn(2, ' ');
                let n_tok = parts.next()?;
                let after_n = parts.next()?.trim();
                let n = parse_small_num(n_tok)?;
                if let Some(after_or_less) = after_n.strip_prefix("or less ") {
                    if n == 0 {
                        return None;
                    }
                    at_most_prevent = Some(n);
                    if let Some(t) = after_or_less.strip_prefix("combat damage to ") {
                        (StaticDamageKind::CombatOnly, t)
                    } else if let Some(t) = after_or_less.strip_prefix("noncombat damage to ") {
                        (StaticDamageKind::NoncombatOnly, t)
                    } else {
                        let t = after_or_less.strip_prefix("damage to ")?;
                        (StaticDamageKind::Any, t)
                    }
                } else {
                    let after_or_more = after_n.strip_prefix("or more ")?;
                    if n == 0 {
                        return None;
                    }
                    threshold_cap = Some(n - 1);
                    if let Some(t) = after_or_more.strip_prefix("combat damage to ") {
                        (StaticDamageKind::CombatOnly, t)
                    } else if let Some(t) = after_or_more.strip_prefix("noncombat damage to ") {
                        (StaticDamageKind::NoncombatOnly, t)
                    } else {
                        let t = after_or_more.strip_prefix("damage to ")?;
                        (StaticDamageKind::Any, t)
                    }
                }
            };
        (
            kind,
            parse_static_damage_recipient(rest_target, vocabulary)?,
        )
    };

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
        )
    };
    let after_deals = instead_norm
        .strip_prefix("it deals ")
        .or_else(|| instead_norm.strip_prefix("that source deals "));

    let op = if let Some(max_prevent) = at_most_prevent {
        if matches!(
            instead_norm,
            "prevent that damage" | "prevent all of that damage"
        ) {
            StaticDamageOp::PreventAtMost(max_prevent)
        } else {
            return None;
        }
    } else if let Some(expected_cap) = threshold_cap {
        let rest = after_deals?;
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
        if !target_suffix_ok(tail) {
            return None;
        }
        StaticDamageOp::CapAt(k)
    } else if instead_norm == "put that many -1/-1 counters on it"
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
        StaticDamageOp::ReplaceWithMinusOneCounters
    } else if matches!(
        instead_norm,
        "prevent half that damage, rounded up"
            | "prevent half of that damage, rounded up"
            | "it deals half that much damage, rounded down"
            | "that source deals half that much damage, rounded down"
    ) {
        StaticDamageOp::PreventHalf { round_up: true }
    } else if matches!(
        instead_norm,
        "prevent half that damage, rounded down"
            | "prevent half of that damage, rounded down"
            | "it deals half that much damage, rounded up"
            | "that source deals half that much damage, rounded up"
    ) {
        StaticDamageOp::PreventHalf { round_up: false }
    } else if matches!(
        instead_norm,
        "prevent that damage" | "prevent all of that damage"
    ) {
        StaticDamageOp::PreventAll
    } else if let Some(n) = instead_norm
        .strip_prefix("prevent ")
        .and_then(|r| r.strip_suffix(" of that damage"))
        .and_then(parse_small_num)
        && n > 0
    {
        StaticDamageOp::PreventUpTo(n)
    } else if let Some(rest) = after_deals.and_then(|s| s.strip_prefix("that much damage minus "))
        && let Some(n_tok) = rest.split_whitespace().next()
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && target_suffix_ok(rest.strip_prefix(n_tok).unwrap_or("").trim())
    {
        StaticDamageOp::Subtract(n)
    } else if let Some(rest) = after_deals
        && let Some(n_tok) = rest.split_whitespace().next()
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && let Some(after_less) = rest
            .strip_prefix(n_tok)
            .unwrap_or("")
            .trim()
            .strip_prefix("less damage")
        && target_suffix_ok(after_less.trim())
    {
        StaticDamageOp::Subtract(n)
    } else if let Some(tail) = after_deals.and_then(|s| {
        s.strip_prefix("double that damage")
            .or_else(|| s.strip_prefix("twice that much damage"))
    }) && target_suffix_ok(tail.trim())
    {
        StaticDamageOp::Multiply(2)
    } else if let Some(tail) = after_deals.and_then(|s| {
        s.strip_prefix("triple that damage")
            .or_else(|| s.strip_prefix("three times that much damage"))
    }) && target_suffix_ok(tail.trim())
    {
        StaticDamageOp::Multiply(3)
    } else {
        let rest = after_deals?.strip_prefix("that much damage plus ")?;
        if let Some(n_tok) = rest.split_whitespace().next()
            && let Some(n) = parse_small_num(n_tok)
            && n > 0
            && target_suffix_ok(rest.strip_prefix(n_tok).unwrap_or("").trim())
        {
            StaticDamageOp::Add(n)
        } else {
            fn strip_target_tail(s: &str) -> &str {
                let s = s.trim();
                s.strip_suffix(" to that permanent or player")
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
                    .trim()
            }
            let expr_raw =
                if let Some((before_where, where_expr)) = rest.split_once(", where x is ") {
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
            StaticDamageOp::AddExpr(Box::new(val_expr))
        }
    };

    Some(StaticDamageRule::ModifyDamage {
        source,
        source_controller,
        recipient,
        kind,
        op,
    })
}
