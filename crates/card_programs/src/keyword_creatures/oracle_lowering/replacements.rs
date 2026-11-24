use super::*;

pub(super) fn lower_replacement(
    rep: &ReplacementEffectAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
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
        } => {
            let kind = parse_counter_kind(counter_kind)
                .ok_or("oracle:unsupported-others-entry-counters")?;
            if !kind.is_permanent() {
                return Err("oracle:unsupported-others-entry-counters");
            }
            let ValueExprAst::Const(n) = count.as_ref() else {
                return Err("oracle:unsupported-others-entry-counters");
            };
            let cnt = u8::try_from(*n)
                .ok()
                .filter(|&v| v > 0)
                .ok_or("oracle:unsupported-others-entry-counters")?;
            let selectors = lower_permanent_selectors(subject, vocabulary)?;
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
                    low = low
                        .replace("it isn't legendary", "")
                        .replace("isn't legendary", "")
                        .replace("is not legendary", "");
                }
                if low.contains("enters with an additional +1/+1 counter")
                    || low.contains("enters with a +1/+1 counter")
                {
                    spec.extra_counters = Some((CounterKind::PlusOne, 1));
                    low = low
                        .replace("it enters with an additional +1/+1 counter on it", "")
                        .replace("enters with an additional +1/+1 counter on it", "")
                        .replace("it enters with a +1/+1 counter on it", "")
                        .replace("enters with a +1/+1 counter on it", "");
                } else if low.contains("enters with a shield counter") {
                    spec.extra_counters = Some((CounterKind::Shield, 1));
                    low = low
                        .replace("it enters with a shield counter on it", "")
                        .replace("enters with a shield counter on it", "");
                }
                if let Some(pos) = low.find("in addition to its other types") {
                    let prefix = &low[..pos];
                    let body = prefix.rsplit("it's ").next().unwrap_or(prefix).trim();
                    for tok in body.split_whitespace() {
                        let w = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                        match w {
                            "" | "a" | "an" | "it" | "is" | "it's" => {}
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
                                let title = {
                                    let mut ch = w.chars();
                                    ch.next()
                                        .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                                        .unwrap_or_default()
                                };
                                if let Some(ct) = vocabulary.resolve_label(&title) {
                                    spec.extra_creature_subtypes.insert(ct);
                                } else {
                                    return Err("oracle:unsupported-enters-as-copy");
                                }
                            }
                        }
                    }
                    low = low.replace(&low[..pos + "in addition to its other types".len()], "");
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
                            format!("it gains {kw_name}"),
                            format!("has {kw_name}"),
                        ] {
                            if low.contains(&phrase) {
                                spec.extra_keywords.insert(ka);
                                low = low.replace(&phrase, "");
                            }
                        }
                    }
                }
                let cleaned = low
                    .replace("it has", "")
                    .replace("it gains", "")
                    .replace("it ", "")
                    .replace([',', '.', ';'], "")
                    .trim()
                    .to_string();
                if !cleaned.is_empty() {
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
    );
    if is_self_subj
        && matches!(
            would_norm,
            "be put into a graveyard from anywhere"
                | "be put into your graveyard from anywhere"
                | "be put into its owner's graveyard from anywhere"
        )
    {
        let dest = if matches!(instead_norm, "exile it" | "exile ~") {
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
        )
    {
        let dest = if matches!(instead_norm, "exile it" | "exile ~") {
            Some(PublicZoneDestination::Exile)
        } else if matches!(
            instead_norm,
            "shuffle ~ into its owner's library"
                | "shuffle it into its owner's library"
                | "reveal ~ and shuffle it into its owner's library"
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
    if matches!(
        subj_low.as_str(),
        "a creature dealt damage by ~ this turn" | "a permanent dealt damage by ~ this turn"
    ) && matches!(
        would_norm,
        "die" | "be put into a graveyard from the battlefield"
    ) && matches!(
        instead_norm,
        "exile it" | "exile that creature" | "exile that permanent"
    ) {
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
    if matches!(subj_low.as_str(), "a card or token" | "a card")
        && matches!(
            would_norm,
            "be put into an opponent's graveyard from anywhere"
                | "be put into a graveyard from anywhere"
        )
        && instead_norm == "exile it"
    {
        let id = next_zone_rep_id(program);
        let objects = if subj_low == "a card" {
            ZoneObjectKind::Card
        } else {
            ZoneObjectKind::Either
        };
        let subject = if would_norm.contains("opponent's graveyard") {
            ZoneReplacementSubject::Matching {
                filter: Box::new(ObjectFilter::any()),
                owner_relation: Some(PlayerRelation::Opponent),
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
    if matches!(
        would_norm,
        "die"
            | "be put into a graveyard from the battlefield"
            | "be put into an opponent's graveyard from the battlefield"
            | "be put into your graveyard from the battlefield"
    ) {
        let (choice, instead_core) = if let Some(rest) = instead_norm.strip_prefix("you may ") {
            (ZoneReplacementChoice::OptionalAffectedPlayer, rest.trim())
        } else {
            (ZoneReplacementChoice::Mandatory, instead_norm)
        };
        let replace_dest = if matches!(
            instead_core,
            "exile it" | "exile that creature" | "exile that permanent" | "exile that card"
        ) {
            Some(PublicZoneDestination::Exile)
        } else if choice == ZoneReplacementChoice::Mandatory
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
            let matched = match subj_low.as_str() {
                "a creature an opponent controls" => {
                    Some((ObjectFilter::creature().opposing(), ZoneObjectKind::Either))
                }
                "a nontoken creature an opponent controls" => Some((
                    ObjectFilter::creature().opposing().nontoken(),
                    ZoneObjectKind::Card,
                )),
                "a creature" => Some((ObjectFilter::creature(), ZoneObjectKind::Either)),
                "a nontoken creature" => {
                    Some((ObjectFilter::creature().nontoken(), ZoneObjectKind::Card))
                }
                "another creature" => Some((
                    ObjectFilter::creature().other_than_ability_source(),
                    ZoneObjectKind::Either,
                )),
                "another nontoken creature" => Some((
                    ObjectFilter::creature()
                        .other_than_ability_source()
                        .nontoken(),
                    ZoneObjectKind::Card,
                )),
                "a creature you control" => Some((
                    ObjectFilter::creature().controlled(),
                    ZoneObjectKind::Either,
                )),
                "a nontoken creature you control" => Some((
                    ObjectFilter::creature().controlled().nontoken(),
                    ZoneObjectKind::Card,
                )),
                "a permanent an opponent controls" => {
                    Some((ObjectFilter::permanent().opposing(), ZoneObjectKind::Either))
                }
                "a nontoken permanent an opponent controls" => Some((
                    ObjectFilter::permanent().opposing().nontoken(),
                    ZoneObjectKind::Card,
                )),
                "a permanent" => Some((ObjectFilter::permanent(), ZoneObjectKind::Either)),
                _ => spellwake_oracle_compiler::filter::parse_object_filter(
                    &subj_low,
                    &cached_compiler_vocab(vocabulary),
                )
                .ok()
                .and_then(|ast_f| {
                    let kind = match ast_f.token {
                        Some(false) => ZoneObjectKind::Card,
                        Some(true) => ZoneObjectKind::Token,
                        None => ZoneObjectKind::Either,
                    };
                    lower_object_filter(&ast_f, vocabulary)
                        .ok()
                        .map(|obj| (obj, kind))
                }),
            };
            if let Some((filter, objects)) = matched {
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
                        owner_relation,
                    },
                    destination: PublicZoneDestination::Graveyard,
                    replace_with,
                    choice,
                });
                return Some(Ok(()));
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
    ) && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
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
    if subj_low == "you"
        && matches!(
            would_norm,
            "draw a card except the first one you draw in each of your draw steps"
                | "draw a card except the first one you draw in each of your draw steps,"
        )
        && matches!(instead_norm, "draw two cards" | "you draw two cards")
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 0,
                    multiply: 2,
                    skip_first_draw_step: true,
                    require_empty_hand: false,
                },
            },
        ));
    }
    if subj_low == "you"
        && would_norm == "draw a card while you have no cards in hand"
        && matches!(instead_norm, "draw two cards" | "you draw two cards")
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 0,
                    multiply: 2,
                    skip_first_draw_step: false,
                    require_empty_hand: true,
                },
            },
        ));
    }
    if subj_low == "you"
        && would_norm == "draw a card"
        && matches!(instead_norm, "draw two cards" | "you draw two cards")
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 0,
                    multiply: 2,
                    skip_first_draw_step: false,
                    require_empty_hand: false,
                },
            },
        ));
    }
    if subj_low == "you"
        && (would_norm == "draw one or more cards" || would_norm == "draw a card")
        && matches!(
            instead_norm,
            "draw that many cards plus one"
                | "you draw that many cards plus one"
                | "draw an additional card"
                | "you draw an additional card"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 1,
                    multiply: 1,
                    skip_first_draw_step: false,
                    require_empty_hand: false,
                },
            },
        ));
    }
    if subj_low == "you"
        && would_norm == "draw one or more cards"
        && matches!(
            instead_norm,
            "draw twice that many cards" | "you draw twice that many cards"
        )
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::DrawRule {
                id: ids.static_id(),
                rule: StaticDrawRule::ModifyDraw {
                    add: 0,
                    multiply: 2,
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
    if (subj_low == "an effect"
        && would_norm == "create one or more tokens under your control"
        && matches!(
            instead_norm,
            "it creates that many tokens plus one"
                | "it creates that many plus one of those tokens"
                | "it creates those tokens plus one"
        ))
        || (subj_low == "one or more tokens"
            && would_norm == "be created under your control"
            && matches!(
                instead_norm,
                "that many tokens plus one are created" | "those tokens plus one are created"
            ))
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyTokens {
                    controller: PlayerRelation::Controller,
                    add: 1,
                    multiply: 1,
                },
            },
        ));
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

    if subj_low == "you"
        && would_norm.starts_with("create one or more ")
        && let Some(sub_word) = would_norm
            .strip_prefix("create one or more ")
            .and_then(|s| s.strip_suffix(" tokens"))
        && let Some(subtype) = match sub_word.trim() {
            "treasure" => Some(ArtifactSubtype::Treasure),
            "food" => Some(ArtifactSubtype::Food),
            "clue" => Some(ArtifactSubtype::Clue),
            "blood" => Some(ArtifactSubtype::Blood),
            "map" => Some(ArtifactSubtype::Map),
            "powerstone" => Some(ArtifactSubtype::Powerstone),
            _ => None,
        }
        && instead_norm.contains("plus an additional")
    {
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ModifyTokenSubtype {
                    subtype,
                    controller: PlayerRelation::Controller,
                    add: 1,
                    multiply: 1,
                },
            },
        ));
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
            "be put on a permanent you control" | "be put on an artifact or creature you control"
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
    if let Some(after_one) = subj_low.strip_prefix("one or more ")
        && let Some(kind_raw) = after_one
            .strip_suffix(" counters")
            .or_else(|| (after_one == "counters").then_some(""))
        && let Some(target_raw) = would_norm
            .strip_prefix("be put on ")
            .or_else(|| would_norm.strip_prefix("be placed on "))
    {
        let counter_kind = if kind_raw.trim().is_empty() {
            None
        } else {
            parse_counter_kind(kind_raw.trim()).filter(|k| k.is_permanent())
        };
        let kind_ok = kind_raw.trim().is_empty() || counter_kind.is_some();
        let selector_opt = if matches!(target_raw.trim(), "~" | "this creature" | "this permanent")
        {
            Some(PermanentSelector {
                controller: ControllerSelector::SourceItself,
                ..PermanentSelector::default()
            })
        } else {
            spellwake_oracle_compiler::filter::parse_object_filter(
                target_raw.trim(),
                &cached_compiler_vocab(vocabulary),
            )
            .ok()
            .and_then(|f| lower_filter_to_permanent_selector(&f, vocabulary).ok())
        };
        if kind_ok && let Some(selector) = selector_opt {
            let (add, multiply) = if instead_norm.contains("twice that many")
                || instead_norm.contains("double that many")
            {
                (0u8, 2u8)
            } else if instead_norm.contains("three times that many")
                || instead_norm.contains("triple that many")
            {
                (0u8, 3u8)
            } else if instead_norm.contains("that many plus one")
                || instead_norm.contains("plus one additional")
                || instead_norm.contains("plus an additional")
            {
                (1u8, 1u8)
            } else if instead_norm.contains("that many plus two") {
                (2u8, 1u8)
            } else {
                (0u8, 1u8)
            };
            if multiply > 1 || add > 0 {
                return Some(push_static_ability(
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
                ));
            }
        }
    }

    // 6. Life Gain Modifiers (CR 614.1a)
    if subj_low == "you"
        && let Some(cond_life) = would_norm.strip_prefix("gain life while you have ")
        && let Some(limit_str) = cond_life.strip_suffix(" or less life")
        && let Ok(limit) = limit_str.trim().parse::<i64>()
        && instead_norm == "you gain twice that much life"
    {
        let condition = ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
            comparison: Comparison::LessOrEqual,
            right: ValueExpr::literal(limit),
        };
        return Some(push_static_ability(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ConditionalModifyLifeGain {
                    condition: Box::new(condition),
                    player: PlayerRelation::Controller,
                    add: 0,
                    multiply: 2,
                },
            },
        ));
    }
    if matches!(
        subj_low.as_str(),
        "you" | "a player" | "any player" | "an opponent"
    ) && would_norm == "gain life"
    {
        let player = match subj_low.as_str() {
            "you" => PlayerRelation::Controller,
            "an opponent" => PlayerRelation::Opponent,
            _ => PlayerRelation::AnyParticipant,
        };
        let life_rule = if instead_norm.contains("twice that much life")
            || instead_norm.contains("double that much life")
        {
            Some(StaticModifierRule::ModifyLifeGain {
                player,
                add: 0,
                multiply: 2,
                lose_instead: false,
            })
        } else if instead_norm.contains("three times that much life") {
            Some(StaticModifierRule::ModifyLifeGain {
                player,
                add: 0,
                multiply: 3,
                lose_instead: false,
            })
        } else if let Some(plus_str) = instead_norm
            .split_once("that much life plus ")
            .or_else(|| instead_norm.split_once("an additional "))
            .map(|(_, r)| r.trim())
            && let Some(n) = match plus_str.split_whitespace().next().unwrap_or("") {
                "one" | "1" => Some(1u8),
                "two" | "2" => Some(2),
                "three" | "3" => Some(3),
                "four" | "4" => Some(4),
                "five" | "5" => Some(5),
                tok => tok.parse::<u8>().ok(),
            }
            && n > 0
        {
            Some(StaticModifierRule::ModifyLifeGain {
                player,
                add: n,
                multiply: 1,
                lose_instead: false,
            })
        } else {
            None
        };
        if let Some(rule) = life_rule {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule,
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
    if would_norm == "deal_damage" && instead_norm == "double_damage" {
        let rule = match subj_low.as_str() {
            "a source" => Some(StaticDamageRule::ModifyDamage {
                source: None,
                source_controller: PlayerRelation::AnyParticipant,
                recipient: StaticDamageRecipient::Any,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::Multiply(2),
            }),
            "a source you control" => Some(StaticDamageRule::ModifyDamage {
                source: None,
                source_controller: PlayerRelation::Controller,
                recipient: StaticDamageRecipient::Any,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::Multiply(2),
            }),
            _ => None,
        };
        if let Some(rule) = rule {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule,
                },
            ));
        }
    }
    if subj_low == "a source"
        && would_norm == "deal damage to an opponent or a permanent an opponent controls"
        && matches!(
            instead_norm,
            "it deals double that damage to that player or permanent"
                | "it deals twice that much damage to that player or permanent"
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

fn parse_static_damage_recipient(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    match raw.trim() {
        "a permanent or player"
        | "a creature, planeswalker, or player"
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
        "a player" => Some(StaticDamageRecipient::Players),
        "~" | "this creature" | "this permanent" => Some(StaticDamageRecipient::SelfObject),
        "enchanted creature" | "equipped creature" | "enchanted permanent" => {
            Some(StaticDamageRecipient::AttachedHost)
        }
        other => {
            let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
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
    let to = match dest_str {
        "~" | "this creature" | "this permanent" => DamageRedirectTarget::AbilitySource,
        "enchanted creature" | "equipped creature" | "enchanted permanent" => {
            DamageRedirectTarget::AttachedHost
        }
        "you" => DamageRedirectTarget::Controller,
        "that source's controller" | "its controller" => {
            DamageRedirectTarget::DamageSourceController
        }
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
            let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
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
        let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
            by_src.trim(),
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if !is_unrestricted_filter(&ast_f) {
            source = Some(lower_object_filter(&ast_f, vocabulary).ok()?);
        }
        recip_part.trim()
    } else {
        recipient_raw
    };

    let from = if recipient_clean == "you or another permanent you control"
        && to == DamageRedirectTarget::AbilitySource
    {
        StaticDamageRecipient::ControllerOrControlledPermanent
    } else if recipient_clean == "you or another creature you control"
        && to == DamageRedirectTarget::AbilitySource
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

    // Life loss skip or amount modifier
    if let Some(player) = affected_player
        && would_norm == "lose life"
        && !instead_norm.contains(" and ")
        && !instead_norm.contains(". ")
    {
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
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::LifeLoss { player },
                    condition: None,
                    action,
                },
            ));
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
            }
        }
    }

    // Draw skip or body replacement (CR 614.1a)
    if subj_low == "you" && would_norm == "draw a card" {
        if matches!(instead_norm, "skip that draw" | "you skip that draw") {
            return Some(push_static_ability(
                program,
                StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::Draw {
                        player: AffectedPlayers::One(ResolvingPlayer::Controller),
                    },
                    condition: None,
                    action: StaticReplacementAction::Skip,
                },
            ));
        }
        let (optional, action_str) = if let Some(rest) = instead_raw
            .strip_prefix("you may instead ")
            .map(|s| s.trim_end_matches(" instead").trim())
        {
            (true, rest)
        } else if let Some(rest) = instead_norm.strip_prefix("you may ") {
            (true, rest.trim())
        } else {
            (false, instead_norm)
        };
        if !action_str.is_empty()
            && !action_str.contains("instead")
            && !matches!(action_str, "skip that draw" | "you skip that draw")
        {
            let (ins, fails) = spellwake_oracle_compiler::clauses::parse_effect_body(
                action_str,
                &cached_compiler_vocab(vocabulary),
            );
            if fails.is_empty()
                && !ins.is_empty()
                && let Ok(body) = lower_effect_body(&ins, vocabulary, ids, false, true)
                && body.target == BodyTarget::None
            {
                let ability = StaticContinuousAbility::EventReplacement {
                    id: ids.static_id(),
                    event: ReplaceableEvent::Draw {
                        player: AffectedPlayers::One(ResolvingPlayer::Controller),
                    },
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

    None
}

pub(super) fn parse_modify_damage_event(
    subj_low: &str,
    would_norm: &str,
    instead_norm: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRule> {
    let (other_than_self, core_subj) = if let Some(rest) = subj_low.strip_prefix("another ") {
        (true, format!("a {rest}"))
    } else {
        (false, subj_low.to_string())
    };
    let (mut source, source_controller): (Option<ObjectFilter>, PlayerRelation) = match core_subj
        .as_str()
    {
        "a source" => (None, PlayerRelation::AnyParticipant),
        "a source you control" => (None, PlayerRelation::Controller),
        "a red source" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red))),
            PlayerRelation::AnyParticipant,
        ),
        "a red source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red))),
            PlayerRelation::Controller,
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
        "a green source you control" => (
            Some(ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Green))),
            PlayerRelation::Controller,
        ),
        "a creature you control" => (Some(ObjectFilter::creature()), PlayerRelation::Controller),
        "a creature" => (
            Some(ObjectFilter::creature()),
            PlayerRelation::AnyParticipant,
        ),
        "a permanent you control" => (Some(ObjectFilter::permanent()), PlayerRelation::Controller),
        "a creature or planeswalker you control" => (
            Some(ObjectFilter::any_types(
                CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
            )),
            PlayerRelation::Controller,
        ),
        "an artifact you control" | "a artifact you control" => {
            (Some(ObjectFilter::artifact()), PlayerRelation::Controller)
        }
        "a red instant or sorcery spell you control" => (
            Some(ObjectFilter::instant_or_sorcery().with_color(ObjectColorFilter::has(Color::Red))),
            PlayerRelation::Controller,
        ),
        "an instant or sorcery spell you control" | "a instant or sorcery spell you control" => (
            Some(ObjectFilter::instant_or_sorcery()),
            PlayerRelation::Controller,
        ),
        other => {
            if other == "a source an opponent controls" || other == "a source you don't control" {
                (None, PlayerRelation::Opponent)
            } else if matches!(other, "~" | "this creature" | "this permanent") {
                (
                    Some(ObjectFilter::self_object()),
                    PlayerRelation::AnyParticipant,
                )
            } else if matches!(other, "enchanted creature" | "equipped creature") {
                (
                    Some(ObjectFilter::Clause(ObjectClause {
                        source_relation: SourceRelation::AttachedToSelf,
                        ..ObjectClause::any()
                    })),
                    PlayerRelation::AnyParticipant,
                )
            } else if let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            ) {
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

    let mut threshold_cap: Option<u8> = None;
    let (kind, rest_target) = if let Some(rest) = would_norm.strip_prefix("deal combat damage to ")
    {
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
    };

    let recipient = parse_static_damage_recipient(rest_target, vocabulary)?;

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
        )
    };

    let op = if let Some(expected_cap) = threshold_cap {
        let rest = instead_norm.strip_prefix("it deals ")?;
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
    } else if let Some(rest) = instead_norm.strip_prefix("it deals that much damage minus ")
        && let Some(n_tok) = rest.split_whitespace().next()
        && let Some(n) = parse_small_num(n_tok)
        && n > 0
        && target_suffix_ok(rest.strip_prefix(n_tok).unwrap_or("").trim())
    {
        StaticDamageOp::Subtract(n)
    } else if let Some(rest) = instead_norm.strip_prefix("it deals ")
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
    } else if matches!(
        instead_norm,
        "it deals double that damage"
            | "it deals double that damage to that player"
            | "it deals double that damage to that permanent or player"
            | "it deals double that damage to that player or permanent"
            | "it deals double that damage to that creature"
            | "it deals twice that much damage"
            | "it deals twice that much damage to that player"
            | "it deals twice that much damage to that permanent or player"
            | "it deals twice that much damage to that player or permanent"
            | "it deals twice that much damage to that creature"
    ) {
        StaticDamageOp::Multiply(2)
    } else if matches!(
        instead_norm,
        "it deals triple that damage"
            | "it deals triple that damage to that player"
            | "it deals triple that damage to that permanent or player"
            | "it deals triple that damage to that player or permanent"
            | "it deals triple that damage to that creature"
            | "it deals three times that much damage"
            | "it deals three times that much damage to that player"
            | "it deals three times that much damage to that permanent or player"
            | "it deals three times that much damage to that player or permanent"
    ) {
        StaticDamageOp::Multiply(3)
    } else {
        let rest = instead_norm.strip_prefix("it deals that much damage plus ")?;
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
