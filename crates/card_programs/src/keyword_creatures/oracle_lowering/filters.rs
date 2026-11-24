use super::*;

pub(super) fn lower_permanent_selectors(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<PermanentSelector>, &'static str> {
    match subject {
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.extend(lower_permanent_selectors(item, vocabulary)?);
            }
            Ok(out)
        }
        other => Ok(vec![lower_permanent_selector(other, vocabulary)?]),
    }
}

pub(super) fn lower_permanent_selector(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    match subject {
        SubjectRefAst::SelfObject => Ok(PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }),
        SubjectRefAst::AttachedHost(_) => Ok(PermanentSelector {
            attached_to_source: true,
            ..PermanentSelector::default()
        }),
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            lower_filter_to_permanent_selector(f, vocabulary)
        }
        _ => Err("oracle:unsupported-permanent-selector"),
    }
}

pub(super) fn lower_filter_to_permanent_selector(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    if !matches!(f.zone, None | Some(ZoneFilter::Battlefield))
        || f.required_keywords.len() > 1
        || f.any_spell
        || f.is_ability
    {
        return Err("oracle:unsupported-selector-predicate");
    }
    let mut sel = PermanentSelector::default();
    if let Some(named) = &f.named {
        use spellwake_oracle_compiler::ast::NameFilterAst;
        sel.name = match named {
            NameFilterAst::SelfName => {
                spellwake_engine::characteristics::NameFilter::SharesWithSource
            }
            NameFilterAst::Exact(n) => spellwake_engine::characteristics::NameFilter::Named(
                EnglishCardName::new(n.clone())
                    .map_err(|_| "oracle:unsupported-selector-predicate")?,
            ),
            NameFilterAst::SameAsReferenced => {
                return Err("oracle:unsupported-selector-predicate");
            }
        };
    }
    if let Some(ck_raw) = f.counter_kind.as_deref() {
        match ck_raw.trim().to_ascii_lowercase().as_str() {
            "+1/+1" => sel.plus_one_counter = true,
            "-1/-1" => sel.minus_one_counter = true,
            "any" | "a" => sel.any_counter = true,
            _ => return Err("oracle:unsupported-selector-predicate"),
        }
    }
    for sb in &f.stat_bounds {
        let ValueExprAst::Const(v) = sb.value.as_ref() else {
            return Err("oracle:unsupported-selector-predicate");
        };
        let v_i64 = i64::from(*v);
        match sb.stat {
            StatKind::Power => match sb.op {
                CompareOp::Ge => sel.min_power = Some(v_i64),
                CompareOp::Gt => sel.min_power = Some(v_i64 + 1),
                CompareOp::Le => sel.max_power = Some(v_i64),
                CompareOp::Lt => sel.max_power = Some(v_i64 - 1),
                CompareOp::Eq => {
                    sel.min_power = Some(v_i64);
                    sel.max_power = Some(v_i64);
                }
            },
            StatKind::Toughness => match sb.op {
                CompareOp::Ge => sel.min_toughness = Some(v_i64),
                CompareOp::Gt => sel.min_toughness = Some(v_i64 + 1),
                CompareOp::Le => sel.max_toughness = Some(v_i64),
                CompareOp::Lt => sel.max_toughness = Some(v_i64 - 1),
                CompareOp::Eq => {
                    sel.min_toughness = Some(v_i64);
                    sel.max_toughness = Some(v_i64);
                }
            },
            StatKind::ManaValue => {
                let v_u32 =
                    u32::try_from(*v).map_err(|_| "oracle:unsupported-selector-predicate")?;
                match sb.op {
                    CompareOp::Ge => sel.min_mana_value = Some(v_u32),
                    CompareOp::Gt => sel.min_mana_value = Some(v_u32.saturating_add(1)),
                    CompareOp::Le => sel.max_mana_value = Some(v_u32),
                    CompareOp::Lt => {
                        sel.max_mana_value = Some(
                            v_u32
                                .checked_sub(1)
                                .ok_or("oracle:unsupported-selector-predicate")?,
                        );
                    }
                    CompareOp::Eq => {
                        sel.min_mana_value = Some(v_u32);
                        sel.max_mana_value = Some(v_u32);
                    }
                }
            }
            _ => return Err("oracle:unsupported-selector-predicate"),
        }
    }
    if f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        sel.exclude_source = true;
    }
    if f.flags.0 & AstFilterFlags::WITH_ANY_COUNTER.0 != 0 {
        sel.any_counter = true;
    }
    if f.flags.0 & AstFilterFlags::EQUIPPED.0 != 0 {
        sel.equipped_by_at_least = 1;
    }
    if f.flags.0 & AstFilterFlags::ENCHANTED.0 != 0 {
        sel.requires_enchanted = true;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0 != 0 {
        sel.matches_source_entry_type = true;
    }
    let unsupported_flags = f.flags.0
        & !(AstFilterFlags::EXCLUDE_SELF.0
            | AstFilterFlags::WITH_ANY_COUNTER.0
            | AstFilterFlags::EQUIPPED.0
            | AstFilterFlags::ENCHANTED.0
            | AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0);
    if unsupported_flags != 0 {
        return Err("oracle:unsupported-selector-flag");
    }

    if let Some(kw_raw) = f.required_keywords.first() {
        let kw = lower_keyword_ability_name(kw_raw)
            .or_else(|| {
                let low = kw_raw.trim().to_ascii_lowercase();
                if let Some(rest) = low.strip_prefix("protection from ") {
                    let quals = parse_protection_keywords(rest, vocabulary)?;
                    if quals.len() == 1 {
                        return Some(quals[0]);
                    }
                }
                if let Some(rest) = low.strip_prefix("hexproof from ") {
                    let quals = parse_hexproof_from_qualities(rest, vocabulary)?;
                    if quals.len() == 1 {
                        return Some(KeywordAbility::HexproofFrom(quals[0]));
                    }
                }
                None
            })
            .ok_or("oracle:unsupported-selector-predicate")?;
        if kw == KeywordAbility::Flying {
            sel.requires_flying = true;
        } else {
            sel.required_keyword = Some(kw);
        }
    }
    if f.forbidden_keywords.len() == 1 {
        let kw = lower_keyword_ability_name(&f.forbidden_keywords[0])
            .ok_or("oracle:unsupported-selector-predicate")?;
        sel.forbidden_keyword = Some(kw);
    } else if f.forbidden_keywords.len() > 1 {
        for kw_raw in &f.forbidden_keywords {
            let kw = lower_keyword_ability_name(kw_raw)
                .ok_or("oracle:unsupported-selector-predicate")?;
            sel.forbidden_keywords.insert(kw);
        }
    }
    sel.token = f.token;
    match f.tapped {
        None => {}
        Some(false) => sel.requires_untapped = true,
        Some(true) => sel.requires_tapped = true,
    }
    match f.combat {
        None => {}
        Some(CombatState::Attacking) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Attacking);
        }
        Some(CombatState::Blocking) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Blocking);
        }
        Some(CombatState::AttackingOrBlocking) => {
            sel.combat_role =
                Some(spellwake_engine::characteristics::CombatSelector::AttackingOrBlocking);
        }
        _ => return Err("oracle:selector-combat-unsupported"),
    }
    let mut any_types_set = card_type_mask_to_set(f.any_types)?;
    if any_types_set.len() == 1 {
        if let Some(t) = any_types_set.pop_first() {
            sel.required_types.insert(t);
        }
    } else if any_types_set.len() > 1 {
        sel.any_types = any_types_set;
    }
    sel.forbidden_types = card_type_mask_to_set(f.forbidden_types)?;
    sel.required_supertypes = supertype_mask_to_set(f.required_supertypes)?;
    sel.forbidden_supertypes = supertype_mask_to_set(f.forbidden_supertypes)?;

    // Subtypes
    if f.forbidden_subtypes.len() > 1 {
        return Err("oracle:selector-multi-subtypes");
    }
    if f.subtypes.len() > 1 {
        for st in &f.subtypes {
            let ct = vocabulary
                .resolve_label(st)
                .ok_or("oracle:selector-multi-subtypes")?;
            sel.any_creature_subtypes.insert(ct);
        }
    } else if let Some(st) = f.subtypes.first() {
        if let Some(ct) = vocabulary.resolve_label(st) {
            sel.creature_subtype = Some(ct);
        } else if let Some(lt) = parse_land_subtype(st) {
            sel.land_subtype = Some(lt);
        } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
            sel.artifact_subtype = Some(at);
        } else {
            return Err("oracle:selector-unknown-subtype");
        }
    }
    if let Some(fst) = f.forbidden_subtypes.first() {
        if fst.eq_ignore_ascii_case("aura") {
            sel.exclude_auras = true;
        } else if let Some(ct) = vocabulary.resolve_label(fst) {
            sel.forbidden_creature_subtype = Some(ct);
        } else {
            return Err("oracle:selector-unknown-forbidden-subtype");
        }
    }

    // Color
    sel.color = lower_color_filter(f.any_colors, f.forbidden_colors, f.color_arity)?;

    // Controller
    sel.controller = match f.controller {
        None => ControllerSelector::Any,
        Some(ControllerRelation::You) => ControllerSelector::SourceController,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            ControllerSelector::Opponents
        }
        _ => return Err("oracle:selector-unsupported-controller"),
    };
    Ok(sel)
}

pub(super) fn ast_color_mask_to_bits(mask: AstColorMask) -> u8 {
    let mut bits = 0u8;
    if mask.0 & AstColorMask::WHITE.0 != 0 {
        bits |= Color::White.bit();
    }
    if mask.0 & AstColorMask::BLUE.0 != 0 {
        bits |= Color::Blue.bit();
    }
    if mask.0 & AstColorMask::BLACK.0 != 0 {
        bits |= Color::Black.bit();
    }
    if mask.0 & AstColorMask::RED.0 != 0 {
        bits |= Color::Red.bit();
    }
    if mask.0 & AstColorMask::GREEN.0 != 0 {
        bits |= Color::Green.bit();
    }
    bits
}

pub(super) fn lower_color_filter(
    any: AstColorMask,
    forbidden: AstColorMask,
    arity: Option<ColorArity>,
) -> Result<ColorFilter, &'static str> {
    if let Some(a) = arity {
        if !any.is_empty() || !forbidden.is_empty() {
            return Err("oracle:mixed-color-filter");
        }
        return match a {
            ColorArity::Colorless => Ok(ColorFilter::Colorless),
            ColorArity::Monocolored => Ok(ColorFilter::Monocolored),
            ColorArity::Multicolored => Ok(ColorFilter::Multicolored),
            ColorArity::Colored => Ok(ColorFilter::Colored),
        };
    }
    if !any.is_empty() && !forbidden.is_empty() {
        return Err("oracle:mixed-color-filter");
    }
    if !any.is_empty() {
        if let Some(c) = single_color_from_mask(any) {
            return Ok(ColorFilter::Has(c));
        }
        return Ok(ColorFilter::AnyOf(ast_color_mask_to_bits(any)));
    }
    if !forbidden.is_empty() {
        if let Some(c) = single_color_from_mask(forbidden) {
            return Ok(ColorFilter::Lacks(c));
        }
        return Ok(ColorFilter::LacksAll(ast_color_mask_to_bits(forbidden)));
    }
    Ok(ColorFilter::Any)
}

pub(super) fn single_color_from_mask(mask: AstColorMask) -> Option<Color> {
    match mask {
        AstColorMask::WHITE => Some(Color::White),
        AstColorMask::BLUE => Some(Color::Blue),
        AstColorMask::BLACK => Some(Color::Black),
        AstColorMask::RED => Some(Color::Red),
        AstColorMask::GREEN => Some(Color::Green),
        _ => None,
    }
}

pub(super) fn apply_selector_condition(
    sel: &mut PermanentSelector,
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    use spellwake_engine::characteristics::StaticSourceCondition;
    match cond {
        ConditionAst::ControlsCommander => {
            sel.while_controlling_own_commander = true;
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && (filter.any_types.is_empty() || filter.any_types == AstCardTypeMask::CREATURE)
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    subtypes: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            )
            && matches!(filter.controller, None | Some(ControllerRelation::You)) =>
        {
            let ct = vocabulary
                .resolve_label(&filter.subtypes[0])
                .ok_or("oracle:cond-unknown-subtype")?;
            sel.source_condition = Some(StaticSourceCondition::ControllerControlsSubtype(ct));
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            )
            && matches!(filter.controller, None | Some(ControllerRelation::You)) =>
        {
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(t) = types.pop_first()
            {
                sel.controller_type_threshold =
                    Some((t, NonZeroU16::new(*value).ok_or("oracle:zero")?));
                return Ok(());
            }
            if let Some(sc) = lower_static_source_condition(cond, vocabulary) {
                sel.source_condition = Some(sc);
                return Ok(());
            }
            Err("oracle:unsupported-static-cond")
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } if action == "two or more equipment are attached to it" => {
            sel.equipped_by_at_least = 2;
            Ok(())
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => {
            sel.source_condition = Some(StaticSourceCondition::ControllerLifeAtMost(
                u16::try_from(*value).map_err(|_| "oracle:life-overflow")?,
            ));
            Ok(())
        }
        ConditionAst::DevotionCompare {
            colors,
            op: CompareOp::Lt,
            value,
        } => {
            let color = match colors.to_ascii_lowercase().as_str() {
                "white" => Some(Color::White),
                "blue" => Some(Color::Blue),
                "black" => Some(Color::Black),
                "red" => Some(Color::Red),
                "green" => Some(Color::Green),
                _ => None,
            };
            if let Some(color) = color
                && let Ok(v) = u8::try_from(*value)
            {
                sel.devotion_below = Some((color, v));
                return Ok(());
            }
            if let Some(sc) = lower_static_source_condition(cond, vocabulary) {
                sel.source_condition = Some(sc);
                return Ok(());
            }
            Err("oracle:multi-color-devotion")
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && lower_keyword_ability_name(keyword).is_some() =>
        {
            if let Some(kw) = lower_keyword_ability_name(keyword) {
                sel.source_condition = Some(StaticSourceCondition::HasKeyword(kw));
            }
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && (filter.any_types.is_empty() || filter.any_types == AstCardTypeMask::CREATURE)
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    subtypes: true,
                    ..FilterAllow::NONE
                },
            ) =>
        {
            let ct = vocabulary
                .resolve_label(&filter.subtypes[0])
                .ok_or("oracle:cond-unknown-subtype")?;
            sel.source_condition = Some(StaticSourceCondition::OpponentControlsSubtype(ct));
            Ok(())
        }
        ConditionAst::IsYourTurn => {
            sel.source_condition = Some(StaticSourceCondition::ControllersTurn);
            Ok(())
        }
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            sel.source_condition = Some(StaticSourceCondition::OpponentsTurn);
            Ok(())
        }
        ConditionAst::Delirium => {
            sel.source_condition = Some(StaticSourceCondition::Delirium);
            Ok(())
        }
        ConditionAst::Metalcraft => {
            sel.source_condition = Some(StaticSourceCondition::Metalcraft);
            Ok(())
        }
        ConditionAst::Threshold => {
            sel.source_condition = Some(StaticSourceCondition::Threshold);
            Ok(())
        }
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            sel.source_condition = Some(StaticSourceCondition::ControllerHasCityBlessing);
            Ok(())
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => {
            sel.source_condition = Some(StaticSourceCondition::ControllerLifeAtLeast(
                u16::try_from(*value).map_err(|_| "oracle:life-overflow")?,
            ));
            Ok(())
        }
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op,
            value,
        } if !matches!(op, CompareOp::Eq if *value > 0)
            && !matches!(op, CompareOp::Lt if *value == 0) =>
        {
            let sc = match op {
                CompareOp::Le => StaticSourceCondition::HandCountAtMost(*value),
                CompareOp::Lt => StaticSourceCondition::HandCountAtMost(*value - 1),
                CompareOp::Eq => StaticSourceCondition::HandCountAtMost(0),
                CompareOp::Ge => StaticSourceCondition::HandCountAtLeast(*value),
                CompareOp::Gt => StaticSourceCondition::HandCountAtLeast(value.saturating_add(1)),
            };
            sel.source_condition = Some(sc);
            Ok(())
        }
        ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => {
            sel.source_condition = Some(StaticSourceCondition::GraveyardCountAtLeast {
                types: CardTypeMask::EMPTY,
                minimum: *value,
            });
            Ok(())
        }
        ConditionAst::ZoneObjectCount {
            filter,
            op: CompareOp::Ge,
            value,
        } if filter.zone == Some(ZoneFilter::YourGraveyard)
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    zone: true,
                    ..FilterAllow::NONE
                },
            ) =>
        {
            let types = lower_card_type_mask(filter.any_types)?;
            sel.source_condition = Some(StaticSourceCondition::GraveyardCountAtLeast {
                types,
                minimum: *value,
            });
            Ok(())
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("tapped") =>
        {
            sel.source_condition = Some(StaticSourceCondition::SourceIsTapped);
            Ok(())
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("untapped") =>
        {
            sel.source_condition = Some(StaticSourceCondition::SourceIsUntapped);
            Ok(())
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && counter_kind == "+1/+1"
            && *value >= 1 =>
        {
            sel.source_condition = Some(StaticSourceCondition::SourceCounterAtLeast {
                kind: CounterKind::PlusOne,
                minimum: *value,
            });
            Ok(())
        }
        ConditionAst::Marker(GameMarker::MaxSpeed)
            if sel.controller == ControllerSelector::SourceItself =>
        {
            sel.controller = ControllerSelector::SelfAtMaxSpeed;
            Ok(())
        }
        ConditionAst::Marker(GameMarker::PairedWithAnotherCreature)
            if sel.controller == ControllerSelector::SourceItself =>
        {
            sel.controller = ControllerSelector::SourcePair;
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value: 0,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0
            && sel.controller == ControllerSelector::SourceItself =>
        {
            sel.sole_controlled_creature = true;
            Ok(())
        }
        other => {
            if let Some(sc) = lower_static_source_condition(other, vocabulary) {
                sel.source_condition = Some(sc);
                Ok(())
            } else {
                Err("oracle:unsupported-static-cond")
            }
        }
    }
}

pub(super) fn lower_static_source_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticSourceCondition> {
    use spellwake_engine::characteristics::StaticSourceCondition;
    match cond {
        ConditionAst::Not(inner) => {
            if matches!(inner.as_ref(), ConditionAst::IsYourTurn) {
                return Some(StaticSourceCondition::OpponentsTurn);
            }
            Some(StaticSourceCondition::Not(Box::new(
                lower_static_source_condition(inner, vocabulary)?,
            )))
        }
        ConditionAst::And(a, b) => Some(StaticSourceCondition::And(
            Box::new(lower_static_source_condition(a, vocabulary)?),
            Box::new(lower_static_source_condition(b, vocabulary)?),
        )),
        ConditionAst::Or(a, b) => Some(StaticSourceCondition::Or(
            Box::new(lower_static_source_condition(a, vocabulary)?),
            Box::new(lower_static_source_condition(b, vocabulary)?),
        )),
        other => {
            if let Some(specialized) = try_specialized_static_source_condition(other, vocabulary) {
                return Some(specialized);
            }
            let mut tmp_builder = BodyBuilder::new();
            let rc = lower_resolution_condition(other, &mut tmp_builder, vocabulary, false, None)
                .ok()?;
            if tmp_builder.target == BodyTarget::None && rc.is_world_evaluable() {
                Some(StaticSourceCondition::Eval(Box::new(rc)))
            } else {
                None
            }
        }
    }
}

fn try_specialized_static_source_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticSourceCondition> {
    use spellwake_engine::characteristics::StaticSourceCondition;
    match cond {
        ConditionAst::GainedLifeThisTurn => {
            Some(StaticSourceCondition::ControllerGainedLifeThisTurn)
        }
        ConditionAst::OpponentLostLifeThisTurn => {
            Some(StaticSourceCondition::OpponentLostLifeThisTurn)
        }
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        } => Some(StaticSourceCondition::ControllerLostLifeThisTurn),
        ConditionAst::AttackedThisTurn => Some(StaticSourceCondition::ControllerAttackedThisTurn),
        ConditionAst::EnteredUnderYourControlThisTurn {
            filter,
            min_count: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            Some(StaticSourceCondition::CreatureEnteredUnderControlThisTurn)
        }
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count }
            if filter.any_types.is_empty()
                && filter.forbidden_types == AstCardTypeMask::LAND
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.flags.is_empty() =>
        {
            Some(StaticSourceCondition::NonlandPermanentsEnteredThisTurnAtLeast(*min_count))
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.is_empty() => Some(StaticSourceCondition::ControllerCastSpellsThisTurn(1)),
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => {
            let low = action.trim().to_ascii_lowercase();
            match low.as_str() {
                "gained_life" | "gain_life" | "you gained life this turn" => {
                    Some(StaticSourceCondition::ControllerGainedLifeThisTurn)
                }
                "lost_life" | "lose_life" | "you lost life this turn" => {
                    Some(StaticSourceCondition::ControllerLostLifeThisTurn)
                }
                "attacked" | "attack" | "you attacked this turn" => {
                    Some(StaticSourceCondition::ControllerAttackedThisTurn)
                }
                "discard" | "discarded" | "you discarded a card this turn" => {
                    Some(StaticSourceCondition::ControllerDiscardedThisTurn)
                }
                "sacrifice" | "sacrificed" | "you sacrificed a permanent this turn" => {
                    Some(StaticSourceCondition::ControllerSacrificedThisTurn)
                }
                "revolt"
                | "permanent_left_battlefield"
                | "a permanent you controlled left the battlefield this turn" => {
                    Some(StaticSourceCondition::PermanentLeftBattlefieldThisTurn)
                }
                "descended" | "you descended this turn" => {
                    Some(StaticSourceCondition::DescendedThisTurn)
                }
                "crime" | "committed_crime" | "you committed a crime this turn" => {
                    Some(StaticSourceCondition::CommittedCrimeThisTurn)
                }
                "cast_two_spells" | "you've cast two or more spells this turn" => {
                    Some(StaticSourceCondition::ControllerCastSpellsThisTurn(2))
                }
                "drew_two_cards" | "you've drawn two or more cards this turn" => {
                    Some(StaticSourceCondition::ControllerDrewCardsThisTurn(2))
                }
                "~ entered this turn"
                | "it entered this turn"
                | "~ entered the battlefield this turn"
                | "it entered the battlefield this turn" => {
                    Some(StaticSourceCondition::SourceEnteredThisTurn)
                }
                "two or more nonland permanents entered the battlefield under your control this turn" => {
                    Some(StaticSourceCondition::NonlandPermanentsEnteredThisTurnAtLeast(2))
                }
                "a library has twenty or fewer cards in it" => {
                    Some(StaticSourceCondition::AnyLibraryAtMost(20))
                }
                "a player has no cards in hand" => {
                    Some(StaticSourceCondition::AnyPlayerHandCountAtMost(0))
                }
                "white is the most common color among all permanents or is tied for most common" => {
                    Some(StaticSourceCondition::MostCommonPermanentColor {
                        color: Color::White,
                        or_tied: true,
                    })
                }
                "blue is the most common color among all permanents or is tied for most common" => {
                    Some(StaticSourceCondition::MostCommonPermanentColor {
                        color: Color::Blue,
                        or_tied: true,
                    })
                }
                "black is the most common color among all permanents or is tied for most common" => {
                    Some(StaticSourceCondition::MostCommonPermanentColor {
                        color: Color::Black,
                        or_tied: true,
                    })
                }
                "red is the most common color among all permanents or is tied for most common" => {
                    Some(StaticSourceCondition::MostCommonPermanentColor {
                        color: Color::Red,
                        or_tied: true,
                    })
                }
                "green is the most common color among all permanents or is tied for most common" => {
                    Some(StaticSourceCondition::MostCommonPermanentColor {
                        color: Color::Green,
                        or_tied: true,
                    })
                }
                _ => None,
            }
        }
        ConditionAst::IsYourTurn => Some(StaticSourceCondition::ControllersTurn),
        ConditionAst::Delirium => Some(StaticSourceCondition::Delirium),
        ConditionAst::Metalcraft => Some(StaticSourceCondition::Metalcraft),
        ConditionAst::Threshold => Some(StaticSourceCondition::Threshold),
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            Some(StaticSourceCondition::ControllerHasCityBlessing)
        }
        ConditionAst::Marker(GameMarker::PairedWithAnotherCreature) => {
            Some(StaticSourceCondition::SourcePaired)
        }
        ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subj))
            if matches!(subj.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Some(StaticSourceCondition::SourceEnteredThisTurn)
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => Some(StaticSourceCondition::ControllerLifeAtMost(
            u16::try_from(*value).ok()?,
        )),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => Some(StaticSourceCondition::ControllerLifeAtLeast(
            u16::try_from(*value).ok()?,
        )),
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op,
            value,
        } => match op {
            CompareOp::Le => Some(StaticSourceCondition::HandCountAtMost(*value)),
            CompareOp::Lt if *value > 0 => Some(StaticSourceCondition::HandCountAtMost(*value - 1)),
            CompareOp::Eq if *value == 0 => Some(StaticSourceCondition::HandCountAtMost(0)),
            CompareOp::Ge => Some(StaticSourceCondition::HandCountAtLeast(*value)),
            CompareOp::Gt => Some(StaticSourceCondition::HandCountAtLeast(
                value.saturating_add(1),
            )),
            _ => None,
        },
        ConditionAst::HandCount {
            player: PlayerRefAst::AnyPlayer,
            op,
            value,
        } => match op {
            CompareOp::Le => Some(StaticSourceCondition::AnyPlayerHandCountAtMost(*value)),
            CompareOp::Lt if *value > 0 => {
                Some(StaticSourceCondition::AnyPlayerHandCountAtMost(*value - 1))
            }
            CompareOp::Eq if *value == 0 => {
                Some(StaticSourceCondition::AnyPlayerHandCountAtMost(0))
            }
            _ => None,
        },
        ConditionAst::Compare { lhs, op, rhs }
            if matches!(
                lhs.as_ref(),
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyPlayer,
                    stat: PlayerStatKind::CardsInLibrary,
                }
            ) =>
        {
            if let ValueExprAst::Const(v) = rhs.as_ref()
                && *v >= 0
                && let Ok(n) = u16::try_from(*v)
            {
                match op {
                    CompareOp::Le => Some(StaticSourceCondition::AnyLibraryAtMost(n)),
                    CompareOp::Lt if n > 0 => Some(StaticSourceCondition::AnyLibraryAtMost(n - 1)),
                    CompareOp::Eq if n == 0 => Some(StaticSourceCondition::AnyLibraryAtMost(0)),
                    _ => None,
                }
            } else {
                None
            }
        }
        ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => Some(StaticSourceCondition::GraveyardCountAtLeast {
            types: CardTypeMask::EMPTY,
            minimum: *value,
        }),
        ConditionAst::ZoneObjectCount {
            filter,
            op: CompareOp::Ge,
            value,
        } if filter.zone == Some(ZoneFilter::YourGraveyard)
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    zone: true,
                    ..FilterAllow::NONE
                },
            ) =>
        {
            let types = lower_card_type_mask(filter.any_types).ok()?;
            Some(StaticSourceCondition::GraveyardCountAtLeast {
                types,
                minimum: *value,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary.resolve_label(&filter.subtypes[0])?;
            Some(StaticSourceCondition::ControllerControlsSubtype(ct))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary.resolve_label(&filter.subtypes[0])?;
            Some(StaticSourceCondition::OpponentControlsSubtype(ct))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.subtypes.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let mut types = card_type_mask_to_set(filter.any_types).ok()?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                return Some(StaticSourceCondition::ControlsCardType {
                    kind,
                    minimum: *value,
                });
            }
            if let Ok(selector) = lower_filter_to_permanent_selector(filter, vocabulary) {
                return Some(StaticSourceCondition::ControlsMatching {
                    selector: Box::new(selector),
                    minimum: *value,
                });
            }
            None
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let selector = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            Some(StaticSourceCondition::ControlsMatching {
                selector: Box::new(selector),
                minimum: *value,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value: 0,
        } => {
            let selector = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            Some(StaticSourceCondition::Not(Box::new(
                StaticSourceCondition::ControlsMatching {
                    selector: Box::new(selector),
                    minimum: 1,
                },
            )))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let selector = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            Some(StaticSourceCondition::OpponentControlsMatching {
                selector: Box::new(selector),
                minimum: *value,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Eq,
            value: 0,
        } => {
            let selector = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            Some(StaticSourceCondition::Not(Box::new(
                StaticSourceCondition::OpponentControlsMatching {
                    selector: Box::new(selector),
                    minimum: 1,
                },
            )))
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Some(StaticSourceCondition::HasKeyword(
                lower_keyword_ability_name(keyword)?,
            ))
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            match status.to_ascii_lowercase().as_str() {
                "tapped" => Some(StaticSourceCondition::SourceIsTapped),
                "untapped" => Some(StaticSourceCondition::SourceIsUntapped),
                "solved" => Some(StaticSourceCondition::SourceIsSolved),
                "prepared" => Some(StaticSourceCondition::SourceIsPrepared),
                _ => None,
            }
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) && *value >= 1 => {
            let kind = parse_counter_kind(counter_kind)?;
            Some(StaticSourceCondition::SourceCounterAtLeast {
                kind,
                minimum: *value,
            })
        }
        _ => None,
    }
}

pub(super) fn parse_devotion_color_mask(colors: &str) -> Option<ColorMask> {
    let mut mask = ColorMask::EMPTY;
    let low = colors.to_ascii_lowercase();
    if low.contains("white") || colors.contains('W') {
        mask = mask.union(ColorMask::WHITE);
    }
    if low.contains("blue") || colors.contains('U') {
        mask = mask.union(ColorMask::BLUE);
    }
    if low.contains("black") || colors.contains('B') {
        mask = mask.union(ColorMask::BLACK);
    }
    if low.contains("red") || colors.contains('R') {
        mask = mask.union(ColorMask::RED);
    }
    if low.contains("green") || colors.contains('G') {
        mask = mask.union(ColorMask::GREEN);
    }
    (!mask.is_empty()).then_some(mask)
}

pub(super) fn resolve_party_roles(
    vocabulary: &CreatureTypeVocabulary,
) -> Option<[CreatureTypeId; 4]> {
    Some([
        vocabulary.resolve_label("Cleric")?,
        vocabulary.resolve_label("Rogue")?,
        vocabulary.resolve_label("Warrior")?,
        vocabulary.resolve_label("Wizard")?,
    ])
}

pub(super) fn lower_card_type_mask(mask: AstCardTypeMask) -> Result<CardTypeMask, &'static str> {
    let mut out = CardTypeMask::EMPTY;
    if mask.0 & AstCardTypeMask::BATTLE.0 != 0 {
        out = out.union(CardTypeMask::BATTLE);
    }
    if mask.0 & AstCardTypeMask::PLANESWALKER.0 != 0 {
        out = out.union(CardTypeMask::PLANESWALKER);
    }
    if mask.0 & AstCardTypeMask::LAND.0 != 0 {
        out = out.union(CardTypeMask::LAND);
    }
    if mask.0 & AstCardTypeMask::ARTIFACT.0 != 0 {
        out = out.union(CardTypeMask::ARTIFACT);
    }
    if mask.0 & AstCardTypeMask::CREATURE.0 != 0 {
        out = out.union(CardTypeMask::CREATURE);
    }
    if mask.0 & AstCardTypeMask::ENCHANTMENT.0 != 0 {
        out = out.union(CardTypeMask::ENCHANTMENT);
    }
    if mask.0 & AstCardTypeMask::KINDRED.0 != 0 {
        out = out.union(CardTypeMask::KINDRED);
    }
    if mask.0 & AstCardTypeMask::INSTANT.0 != 0 {
        out = out.union(CardTypeMask::INSTANT);
    }
    if mask.0 & AstCardTypeMask::SORCERY.0 != 0 {
        out = out.union(CardTypeMask::SORCERY);
    }
    Ok(out)
}

pub(super) fn card_type_mask_to_set(
    mask: AstCardTypeMask,
) -> Result<BTreeSet<CardType>, &'static str> {
    let mut set = BTreeSet::new();
    let pairs = [
        (AstCardTypeMask::BATTLE, CardType::Battle),
        (AstCardTypeMask::PLANESWALKER, CardType::Planeswalker),
        (AstCardTypeMask::LAND, CardType::Land),
        (AstCardTypeMask::ARTIFACT, CardType::Artifact),
        (AstCardTypeMask::CREATURE, CardType::Creature),
        (AstCardTypeMask::ENCHANTMENT, CardType::Enchantment),
        (AstCardTypeMask::KINDRED, CardType::Kindred),
        (AstCardTypeMask::INSTANT, CardType::Instant),
        (AstCardTypeMask::SORCERY, CardType::Sorcery),
    ];
    for (bit, kind) in pairs {
        if mask.0 & bit.0 != 0 {
            set.insert(kind);
        }
    }
    Ok(set)
}

pub(super) fn supertype_mask_to_set(
    mask: AstSupertypeMask,
) -> Result<BTreeSet<Supertype>, &'static str> {
    let mut set = BTreeSet::new();
    if mask.0 & AstSupertypeMask::BASIC.0 != 0 {
        set.insert(Supertype::Basic);
    }
    if mask.0 & AstSupertypeMask::LEGENDARY.0 != 0 {
        set.insert(Supertype::Legendary);
    }
    if mask.0 & AstSupertypeMask::SNOW.0 != 0 {
        set.insert(Supertype::Snow);
    }
    if mask.0 & AstSupertypeMask::WORLD.0 != 0 {
        set.insert(Supertype::World);
    }
    if mask.0 & AstSupertypeMask::HISTORIC.0 != 0 {
        return Err("oracle:historic-in-supertype-set");
    }
    Ok(set)
}

pub(super) fn lower_color_mask(mask: AstColorMask) -> ColorMask {
    let mut out = ColorMask::EMPTY;
    if mask.0 & AstColorMask::WHITE.0 != 0 {
        out = out.union(ColorMask::WHITE);
    }
    if mask.0 & AstColorMask::BLUE.0 != 0 {
        out = out.union(ColorMask::BLUE);
    }
    if mask.0 & AstColorMask::BLACK.0 != 0 {
        out = out.union(ColorMask::BLACK);
    }
    if mask.0 & AstColorMask::RED.0 != 0 {
        out = out.union(ColorMask::RED);
    }
    if mask.0 & AstColorMask::GREEN.0 != 0 {
        out = out.union(ColorMask::GREEN);
    }
    out
}

pub(super) fn color_mask_to_set(mask: AstColorMask) -> BTreeSet<Color> {
    let mut out = BTreeSet::new();
    if mask.0 & AstColorMask::WHITE.0 != 0 {
        out.insert(Color::White);
    }
    if mask.0 & AstColorMask::BLUE.0 != 0 {
        out.insert(Color::Blue);
    }
    if mask.0 & AstColorMask::BLACK.0 != 0 {
        out.insert(Color::Black);
    }
    if mask.0 & AstColorMask::RED.0 != 0 {
        out.insert(Color::Red);
    }
    if mask.0 & AstColorMask::GREEN.0 != 0 {
        out.insert(Color::Green);
    }
    out
}

pub(super) fn lower_object_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    if f.is_ability || f.counter_kind.is_some() || f.named.is_some() {
        return Err("oracle:unsupported-object-filter");
    }
    let mut clause = ObjectClause::any();
    clause.any_types = lower_card_type_mask(f.any_types)?;
    clause.forbidden_types = lower_card_type_mask(f.forbidden_types)?;

    // Supertypes (Historic maps to ObjectFilterFlags::HISTORIC)
    let mut req_sup = SupertypeMask::EMPTY;
    if f.required_supertypes.0 & AstSupertypeMask::BASIC.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::BASIC);
    }
    if f.required_supertypes.0 & AstSupertypeMask::LEGENDARY.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::LEGENDARY);
    }
    if f.required_supertypes.0 & AstSupertypeMask::SNOW.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::SNOW);
    }
    if f.required_supertypes.0 & AstSupertypeMask::WORLD.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::WORLD);
    }
    if f.required_supertypes.0 & AstSupertypeMask::HISTORIC.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::HISTORIC);
    }
    clause.required_supertypes = req_sup;

    let mut forb_sup = SupertypeMask::EMPTY;
    if f.forbidden_supertypes.0 & AstSupertypeMask::BASIC.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::BASIC);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::LEGENDARY.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::LEGENDARY);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::SNOW.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::SNOW);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::WORLD.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::WORLD);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::HISTORIC.0 != 0 {
        return Err("oracle:forbidden-historic");
    }
    clause.forbidden_supertypes = forb_sup;

    // Colors
    clause.color = if let Some(arity) = f.color_arity {
        if !f.any_colors.is_empty() || !f.forbidden_colors.is_empty() {
            return Err("oracle:mixed-color-arity");
        }
        match arity {
            ColorArity::Colorless => ObjectColorFilter::Colorless,
            ColorArity::Colored => ObjectColorFilter::Colored,
            ColorArity::Multicolored => ObjectColorFilter::Multicolored,
            ColorArity::Monocolored => ObjectColorFilter::Monocolored,
        }
    } else if !f.any_colors.is_empty() || !f.forbidden_colors.is_empty() {
        ObjectColorFilter::Mask {
            any_of: lower_color_mask(f.any_colors),
            none_of: lower_color_mask(f.forbidden_colors),
        }
    } else {
        ObjectColorFilter::Any
    };

    // Controller
    clause.controller = match f.controller {
        None => ControllerFilter::Any,
        Some(ControllerRelation::You | ControllerRelation::YourTeam) => ControllerFilter::You,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            ControllerFilter::Opponent
        }
        Some(ControllerRelation::NotYou) => ControllerFilter::NotYou,
        Some(ControllerRelation::OwnerYou) => ControllerFilter::OwnerYou,
        Some(ControllerRelation::OwnerOpponent) => ControllerFilter::OwnerOpponent,
        Some(ControllerRelation::ThatPlayer | ControllerRelation::DefendingPlayer) => {
            ControllerFilter::RelatedPlayer
        }
        Some(ControllerRelation::TargetPlayer) => ControllerFilter::TargetPlayer,
        Some(ControllerRelation::TargetOpponent) => ControllerFilter::TargetOpponent,
        _ => return Err("oracle:unsupported-controller-filter"),
    };

    // Combat
    clause.combat = match f.combat {
        None => CombatFilter::Any,
        Some(CombatState::Attacking) => CombatFilter::Attacking,
        Some(CombatState::Blocking) => CombatFilter::Blocking,
        Some(CombatState::AttackingOrBlocking) => CombatFilter::AttackingOrBlocking,
        Some(CombatState::Blocked) => CombatFilter::Blocked,
        Some(CombatState::Unblocked) => CombatFilter::Unblocked,
        Some(CombatState::BlockingSelf) => CombatFilter::BlockingSource,
        Some(CombatState::BlockedBySelf) => CombatFilter::BlockedBySource,
        Some(CombatState::BlockingOrBlockedBySelf) => CombatFilter::BlockingOrBlockedBySource,
        Some(CombatState::NotAttacking) => CombatFilter::NotAttacking,
    };

    clause.tapped = f.tapped;
    clause.token = f.token;

    // Keywords
    if f.required_keywords.len() > 1 || f.forbidden_keywords.len() > 1 {
        return Err("oracle:multi-keyword-filter");
    }
    if let Some(k) = f.required_keywords.first() {
        clause.keyword = Some(lower_keyword_ability_name(k).ok_or("oracle:filter-unknown-kw")?);
    }
    if let Some(k) = f.forbidden_keywords.first() {
        clause.forbidden_keyword =
            Some(lower_keyword_ability_name(k).ok_or("oracle:filter-unknown-forb-kw")?);
    }

    // Stat bounds
    let lower_cmp = |op: CompareOp| -> spellwake_engine::program::Comparison {
        match op {
            CompareOp::Lt => spellwake_engine::program::Comparison::Less,
            CompareOp::Le => spellwake_engine::program::Comparison::LessOrEqual,
            CompareOp::Eq => spellwake_engine::program::Comparison::Equal,
            CompareOp::Ge => spellwake_engine::program::Comparison::GreaterOrEqual,
            CompareOp::Gt => spellwake_engine::program::Comparison::Greater,
        }
    };
    let lower_anchor =
        |val: &ValueExprAst| -> Option<spellwake_engine::program::DynamicStatAnchor> {
            use spellwake_engine::program::DynamicStatAnchor as A;
            match val {
                ValueExprAst::ChosenX => Some(A::ChosenX),
                ValueExprAst::ObjectStat { subject, stat } => {
                    let is_self = matches!(subject.as_ref(), SubjectRefAst::SelfObject);
                    let is_trig = matches!(subject.as_ref(), SubjectRefAst::Anaphora(_));
                    match (is_self, is_trig, stat) {
                        (true, _, StatKind::Power) => Some(A::SourcePower),
                        (true, _, StatKind::Toughness) => Some(A::SourceToughness),
                        (true, _, StatKind::ManaValue) => Some(A::SourceManaValue),
                        (_, true, StatKind::Power) => Some(A::TriggerSubjectPower),
                        (_, true, StatKind::ManaValue) => Some(A::TriggerSubjectManaValue),
                        _ => None,
                    }
                }
                ValueExprAst::CounterCount {
                    subject,
                    counter_kind,
                } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                    let ck = parse_counter_kind(counter_kind)?;
                    if ck.is_permanent() {
                        Some(A::SourceCounters(ck))
                    } else {
                        None
                    }
                }
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat,
                } => match stat {
                    PlayerStatKind::LifeTotal => Some(A::ControllerLife),
                    PlayerStatKind::CardsInHand => Some(A::ControllerHandSize),
                    PlayerStatKind::CardsInGraveyard => Some(A::ControllerGraveyardCount),
                    _ => None,
                },
                ValueExprAst::Count(inner) => {
                    if inner.zone == Some(ZoneFilter::YourHand)
                        && filter_only_has(
                            inner,
                            FilterAllow {
                                zone: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Some(A::ControllerHandSize);
                    }
                    if inner.zone == Some(ZoneFilter::YourGraveyard)
                        && filter_only_has(
                            inner,
                            FilterAllow {
                                zone: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Some(A::ControllerGraveyardCount);
                    }
                    if inner.zone.is_none()
                        && inner.controller == Some(ControllerRelation::You)
                        && filter_only_has(
                            inner,
                            FilterAllow {
                                types: true,
                                subtypes: true,
                                controller: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        if inner.subtypes.len() == 1
                            && (inner.any_types.is_empty()
                                || inner.any_types == AstCardTypeMask::CREATURE)
                            && let Some(ct) = vocabulary.resolve_label(&inner.subtypes[0])
                        {
                            return Some(A::ControllerSubtypeCount(ct));
                        }
                        if inner.subtypes.is_empty() {
                            let types = card_type_mask_to_set(inner.any_types).ok()?;
                            if types.len() == 1
                                && let Some(&ct) = types.iter().next()
                            {
                                return Some(A::ControllerControlledCount(ct));
                            }
                        }
                    }
                    None
                }
                _ => None,
            }
        };
    for sb in &f.stat_bounds {
        match (sb.stat, sb.op, sb.value.as_ref()) {
            (StatKind::Power, CompareOp::Ge, ValueExprAst::Const(n)) => {
                clause.power =
                    PowerFilter::AtLeast(i16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Gt, ValueExprAst::Const(n)) => {
                clause.power =
                    PowerFilter::AtLeast(i16::try_from(*n + 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Le, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.power =
                    PowerFilter::AtMost(u16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Lt, ValueExprAst::Const(n)) if *n > 0 => {
                clause.power =
                    PowerFilter::AtMost(u16::try_from(*n - 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Eq, ValueExprAst::Const(n)) => {
                clause.power =
                    PowerFilter::Equal(i16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Le, ValueExprAst::ChosenX) => {
                clause.power = PowerFilter::AtMostX;
            }
            (StatKind::Power, CompareOp::Eq, ValueExprAst::ChosenX) => {
                clause.power = PowerFilter::EqualX;
            }
            (StatKind::Power, CompareOp::Ge, ValueExprAst::ChosenX) => {
                clause.power = PowerFilter::AtLeastX;
            }
            (StatKind::Power, CompareOp::Le, ValueExprAst::Count(inner))
                if inner.zone.is_none()
                    && inner.any_types == AstCardTypeMask::CREATURE
                    && inner.controller == Some(ControllerRelation::You)
                    && filter_only_has(
                        inner,
                        FilterAllow {
                            types: true,
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    ) =>
            {
                clause.power = PowerFilter::AtMostControlledCreatureCount;
            }
            (StatKind::Power, op, val) if lower_anchor(val).is_some() => {
                if let Some(anchor) = lower_anchor(val) {
                    clause.power = PowerFilter::CompareAnchor {
                        op: lower_cmp(op),
                        anchor,
                    };
                }
            }
            (StatKind::Toughness, CompareOp::Ge, ValueExprAst::Const(n)) => {
                clause.toughness =
                    ToughnessFilter::AtLeast(i16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Toughness, CompareOp::Gt, ValueExprAst::Const(n)) => {
                clause.toughness =
                    ToughnessFilter::AtLeast(i16::try_from(*n + 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Toughness, CompareOp::Le, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.toughness =
                    ToughnessFilter::AtMost(u16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Toughness, CompareOp::Lt, ValueExprAst::Const(n)) if *n > 0 => {
                clause.toughness =
                    ToughnessFilter::AtMost(u16::try_from(*n - 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Toughness, CompareOp::Eq, ValueExprAst::Const(n)) => {
                clause.toughness =
                    ToughnessFilter::Equal(i16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Toughness, CompareOp::Le, ValueExprAst::ChosenX) => {
                clause.toughness = ToughnessFilter::AtMostX;
            }
            (StatKind::Toughness, CompareOp::Eq, ValueExprAst::ChosenX) => {
                clause.toughness = ToughnessFilter::EqualX;
            }
            (StatKind::Toughness, CompareOp::Ge, ValueExprAst::ChosenX) => {
                clause.toughness = ToughnessFilter::AtLeastX;
            }
            (StatKind::Toughness, op, val) if lower_anchor(val).is_some() => {
                if let Some(anchor) = lower_anchor(val) {
                    clause.toughness = ToughnessFilter::CompareAnchor {
                        op: lower_cmp(op),
                        anchor,
                    };
                }
            }
            (StatKind::ManaValue, CompareOp::Le, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::AtMost(u32::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Lt, ValueExprAst::Const(n)) if *n > 0 => {
                clause.mana_value =
                    ManaValueFilter::AtMost(u32::try_from(*n - 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Ge, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::AtLeast(u32::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Gt, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::AtLeast(u32::try_from(*n + 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Eq, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::Equal(u32::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Le, ValueExprAst::ChosenX) => {
                clause.mana_value = ManaValueFilter::AtMostX;
            }
            (StatKind::ManaValue, CompareOp::Eq, ValueExprAst::ChosenX) => {
                clause.mana_value = ManaValueFilter::EqualX;
            }
            (StatKind::ManaValue, CompareOp::Ge, ValueExprAst::ChosenX) => {
                clause.mana_value = ManaValueFilter::AtLeastX;
            }
            (StatKind::ManaValue, op, val) if lower_anchor(val).is_some() => {
                if let Some(anchor) = lower_anchor(val) {
                    clause.mana_value = ManaValueFilter::CompareAnchor {
                        op: lower_cmp(op),
                        anchor,
                    };
                }
            }
            _ => return Err("oracle:unsupported-stat-bound"),
        }
    }

    // Flags
    if f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        clause.source_relation = SourceRelation::OtherThanAbilitySource;
    }
    if f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENTERED_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::ATTACKED_THIS_TURN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::ATTACKED_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::DIED_THIS_TURN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::DIED_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::MODIFIED.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::MODIFIED);
    }
    if f.flags.0 & AstFilterFlags::EQUIPPED.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::EQUIPPED);
    }
    if f.flags.0 & AstFilterFlags::ENCHANTED.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENCHANTED);
    }
    if f.flags.0 & AstFilterFlags::WITH_ANY_COUNTER.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
    }
    if f.flags.0 & AstFilterFlags::WITHOUT_COUNTERS.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
    }
    if f.flags.0 & AstFilterFlags::FACE_DOWN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_DOWN);
    }
    if f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
    }
    if f.flags.0 & AstFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR);
    }
    if f.flags.0 & AstFilterFlags::ATTACHED_TO_SELF.0 != 0 {
        clause.source_relation = SourceRelation::AttachedToSelf;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_NAMED_AGENDA.0 != 0 {
        clause.source_relation = SourceRelation::MatchesNamedAgendaSlot;
    }
    let handled_flags = AstFilterFlags::EXCLUDE_SELF.0
        | AstFilterFlags::ENTERED_THIS_TURN.0
        | AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0
        | AstFilterFlags::ATTACKED_THIS_TURN.0
        | AstFilterFlags::DIED_THIS_TURN.0
        | AstFilterFlags::MODIFIED.0
        | AstFilterFlags::EQUIPPED.0
        | AstFilterFlags::ENCHANTED.0
        | AstFilterFlags::WITH_ANY_COUNTER.0
        | AstFilterFlags::WITHOUT_COUNTERS.0
        | AstFilterFlags::FACE_DOWN.0
        | AstFilterFlags::EXILED_WITH_SELF.0
        | AstFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN.0
        | AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0
        | AstFilterFlags::ATTACHED_TO_SELF.0
        | AstFilterFlags::MATCHES_NAMED_AGENDA.0;
    if (f.flags.0 & !handled_flags) != 0 {
        return Err("oracle:unsupported-filter-flag");
    }

    // Forbidden subtypes
    let mut forb_creature_types = Vec::new();
    for fst in &f.forbidden_subtypes {
        if fst.eq_ignore_ascii_case("aura") {
            clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
        } else if fst.eq_ignore_ascii_case("saga") {
            clause.flags = clause.flags.union(ObjectFilterFlags::NON_SAGA);
        } else if let Some(ct) = vocabulary.resolve_label(fst) {
            forb_creature_types.push(ct);
        } else if let Some(lt) = parse_land_subtype(fst) {
            if clause.subtypes != SubtypeFilter::Any {
                return Err("oracle:multi-subtype-filter");
            }
            clause.subtypes = SubtypeFilter::NotLand(lt);
        } else {
            return Err("oracle:unsupported-forbidden-subtype");
        }
    }
    if !forb_creature_types.is_empty() {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:multi-subtype-filter");
        }
        clause.subtypes = match forb_creature_types.as_slice() {
            [a] => SubtypeFilter::Lacks(*a),
            [a, b] => SubtypeFilter::NoneOf([*a, *b, *b]),
            [a, b, c] => SubtypeFilter::NoneOf([*a, *b, *c]),
            _ => return Err("oracle:multi-subtype-filter"),
        };
    }

    // Positive subtypes
    let resolve_single_subtype = |st: &str| -> Option<SubtypeFilter> {
        if st.eq_ignore_ascii_case("aura") {
            Some(SubtypeFilter::Aura)
        } else if let Some(ct) = vocabulary.resolve_label(st) {
            Some(SubtypeFilter::Has(ct))
        } else if let Some(lt) = parse_land_subtype(st) {
            Some(SubtypeFilter::Land(lt))
        } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
            Some(SubtypeFilter::Artifact(at))
        } else if let Some(et) = EnchantmentSubtype::from_wire_label(st) {
            Some(SubtypeFilter::Enchantment(et))
        } else {
            match st.trim().to_ascii_lowercase().as_str() {
                "arcane" => Some(SubtypeFilter::Spell(SpellSubtype::Arcane)),
                "lesson" => Some(SubtypeFilter::Spell(SpellSubtype::Lesson)),
                "adventure" => Some(SubtypeFilter::Spell(SpellSubtype::Adventure)),
                "trap" => Some(SubtypeFilter::Spell(SpellSubtype::Trap)),
                "omen" => Some(SubtypeFilter::Spell(SpellSubtype::Omen)),
                _ => None,
            }
        }
    };
    if f.subtypes.len() == 1 {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:mixed-subtype-filter");
        }
        clause.subtypes =
            resolve_single_subtype(&f.subtypes[0]).ok_or("oracle:unsupported-subtype-filter")?;
    } else if f.subtypes.len() == 2 && clause.subtypes == SubtypeFilter::Any {
        if let (Some(sa), Some(sb)) = (
            resolve_single_subtype(&f.subtypes[0]),
            resolve_single_subtype(&f.subtypes[1]),
        ) {
            let mut c1 = clause;
            let mut c2 = clause;
            c1.subtypes = sa;
            c2.subtypes = sb;
            return Ok(ObjectFilter::either(c1, c2));
        }
        return Err("oracle:unsupported-multi-subtype");
    } else if f.subtypes.len() == 3 && clause.subtypes == SubtypeFilter::Any {
        if let (Some(a), Some(b), Some(c)) = (
            vocabulary.resolve_label(&f.subtypes[0]),
            vocabulary.resolve_label(&f.subtypes[1]),
            vocabulary.resolve_label(&f.subtypes[2]),
        ) {
            clause.subtypes = SubtypeFilter::AnyOf([a, b, c]);
        } else {
            return Err("oracle:unsupported-multi-subtype");
        }
    } else if f.subtypes.len() > 3 {
        return Err("oracle:too-many-subtypes");
    }

    Ok(ObjectFilter::Clause(clause))
}

pub(super) fn lower_spell_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SpellFilter, &'static str> {
    let cast_zone = match f.zone {
        None | Some(ZoneFilter::Stack) => spellwake_engine::program::SpellCastZoneFilter::Any,
        Some(ZoneFilter::YourLibrary) => spellwake_engine::program::SpellCastZoneFilter::OwnLibrary,
        Some(
            ZoneFilter::YourGraveyard
            | ZoneFilter::OpponentGraveyard
            | ZoneFilter::OpponentsGraveyards
            | ZoneFilter::AnyGraveyard
            | ZoneFilter::AllGraveyards
            | ZoneFilter::ThatPlayerGraveyard
            | ZoneFilter::TargetPlayerGraveyard,
        ) => spellwake_engine::program::SpellCastZoneFilter::Graveyard,
        Some(ZoneFilter::Exile) => spellwake_engine::program::SpellCastZoneFilter::Exile,
        Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => {
            spellwake_engine::program::SpellCastZoneFilter::Hand
        }
        _ => return Err("oracle:spell-filter-zone"),
    };
    let single_target = (f.flags.0 & AstFilterFlags::SINGLE_TARGET.0) != 0;
    let obj = if single_target || f.zone.is_some() {
        let mut stripped = f.clone();
        stripped.flags.0 &= !AstFilterFlags::SINGLE_TARGET.0;
        stripped.zone = None;
        lower_object_filter(&stripped, vocabulary)?
    } else {
        lower_object_filter(f, vocabulary)?
    };
    let mut sf = SpellFilter::from_object(obj);
    sf.cast_zone = cast_zone;
    if single_target {
        sf.target_shape = spellwake_engine::program::SpellTargetShape::SingleTarget;
    }
    Ok(sf)
}

pub(super) fn lower_spell_subject_filter(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(bool, SpellFilter), &'static str> {
    let f = match subject {
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => f,
        _ => return Err("oracle:unsupported-spell-subject"),
    };
    let any_player = f.controller.is_none();
    let mut stripped = f.as_ref().clone();
    if stripped.controller == Some(ControllerRelation::You) {
        stripped.controller = None;
    } else if stripped.controller.is_some() {
        return Err("oracle:spell-discount-controller");
    }
    let sf = lower_spell_filter(&stripped, vocabulary)?;
    Ok((any_player, sf))
}

pub(super) fn bind_value_expr_targets(
    expr: &ValueExprAst,
    builder: &mut BodyBuilder,
) -> Result<(), &'static str> {
    match expr {
        ValueExprAst::Count(f) => {
            if matches!(f.zone, Some(ZoneFilter::TargetPlayerGraveyard)) {
                builder.bind_target(BodyTarget::SinglePlayer)?;
            }
            match f.controller {
                Some(ControllerRelation::TargetPlayer) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                }
                Some(ControllerRelation::TargetOpponent) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                }
                _ => {}
            }
            Ok(())
        }
        ValueExprAst::PlayerStat { player, .. } => match player {
            PlayerRefAst::TargetPlayer => builder.bind_target(BodyTarget::SinglePlayer),
            PlayerRefAst::TargetOpponent => builder.bind_target(BodyTarget::SingleOpponent),
            _ => Ok(()),
        },
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            bind_value_expr_targets(a, builder)?;
            bind_value_expr_targets(b, builder)
        }
        ValueExprAst::DivConst { expr: inner, .. } => bind_value_expr_targets(inner, builder),
        _ => Ok(()),
    }
}

pub(super) fn lower_value_expr(
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, &'static str> {
    match expr {
        ValueExprAst::Const(n) => Ok(ValueExpr::literal(i64::from(*n))),
        ValueExprAst::ChosenX => Ok(ValueExpr::chosen_x()),
        ValueExprAst::Count(f) => {
            if let Some(zone) = f.zone {
                let mut inner = f.as_ref().clone();
                inner.zone = None;
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner, vocabulary)?
                };
                return match zone {
                    ZoneFilter::YourGraveyard => {
                        Ok(ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj))
                    }
                    ZoneFilter::TargetPlayerGraveyard => Ok(ValueExpr::count_graveyard(
                        ResolvingPlayer::TargetPlayer,
                        obj,
                    )),
                    ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards => Ok(ValueExpr::Count {
                        zone: ValueZone::AllGraveyards,
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::ThatPlayerGraveyard => Ok(ValueExpr::count_graveyard(
                        ResolvingPlayer::UniqueRelatedPlayer,
                        obj,
                    )),
                    ZoneFilter::Exile => Ok(ValueExpr::Count {
                        zone: ValueZone::Exile(None),
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::YourHand => Ok(ValueExpr::Count {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::YourLibrary => Ok(ValueExpr::Count {
                        zone: ValueZone::Library(ResolvingPlayer::Controller),
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::Battlefield => Ok(ValueExpr::count_battlefield(obj)),
                    _ => Err("oracle:unsupported-count-zone"),
                };
            }
            let controller_scope = match f.controller {
                Some(ControllerRelation::ThatPlayer) => Some(ResolvingPlayer::UniqueRelatedPlayer),
                Some(ControllerRelation::TargetPlayer | ControllerRelation::TargetOpponent) => {
                    Some(ResolvingPlayer::TargetPlayer)
                }
                Some(ControllerRelation::DefendingPlayer) => Some(ResolvingPlayer::DefendingPlayer),
                Some(ControllerRelation::ItsController) => Some(ResolvingPlayer::TargetController),
                _ => None,
            };
            if let Some(scope) = controller_scope {
                let mut inner = f.clone();
                inner.controller = None;
                let obj = lower_object_filter(&inner, vocabulary)?;
                return Ok(ValueExpr::Count {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                    controller_scope: Some(scope),
                });
            }
            let obj = lower_object_filter(f, vocabulary)?;
            Ok(ValueExpr::count_battlefield(obj))
        }
        ValueExprAst::PlayerStat { player, stat } => {
            let role = match player {
                PlayerRefAst::You => ResolvingPlayer::Controller,
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent => {
                    ResolvingPlayer::TargetPlayer
                }
                PlayerRefAst::ThatPlayer => ResolvingPlayer::UniqueRelatedPlayer,
                PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController => {
                    ResolvingPlayer::TargetController
                }
                PlayerRefAst::ItsOwner => ResolvingPlayer::TargetOwner,
                _ => return Err("oracle:unsupported-player-stat"),
            };
            match stat {
                PlayerStatKind::LifeTotal => Ok(if role == ResolvingPlayer::Controller {
                    ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                } else {
                    ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(role))
                }),
                PlayerStatKind::CardsInHand => Ok(
                    if role == ResolvingPlayer::Controller || role == ResolvingPlayer::TargetPlayer
                    {
                        ValueExpr::count_hand(role)
                    } else {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(role))
                    },
                ),
                PlayerStatKind::CardsInGraveyard => Ok(
                    if role == ResolvingPlayer::Controller || role == ResolvingPlayer::TargetPlayer
                    {
                        ValueExpr::count_graveyard(role, ObjectFilter::any())
                    } else {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerGraveyardSize(role))
                    },
                ),
                PlayerStatKind::CardsInLibrary => Ok(ValueExpr::PlayerStat(
                    PlayerStatExpr::PlayerLibrarySize(role),
                )),
            }
        }
        ValueExprAst::CardTypesInGraveyard {
            all_graveyards: false,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::OwnGraveyardDistinctCardTypes,
        )),
        ValueExprAst::CardTypesInGraveyard {
            all_graveyards: true,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::AllGraveyardsDistinctCardTypes,
        )),
        ValueExprAst::PartySize => {
            let roles = resolve_party_roles(vocabulary).ok_or("oracle:unsupported-value-expr")?;
            Ok(ValueExpr::PartyCount(Box::new(roles)))
        }
        ValueExprAst::ColorsAmong(filter) => {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ValueExpr::DistinctColors {
                filter: Box::new(obj),
            })
        }
        ValueExprAst::StartingLifeTotal(PlayerRefAst::You) => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::ControllerStartingLife,
        )),
        ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::You,
            gained: true,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::ControllerLifeGainedThisTurn,
        )),
        ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::You,
            gained: false,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::ControllerLifeLostThisTurn,
        )),
        ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::EachOpponent,
            gained: false,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::OpponentsLifeLostThisTurn,
        )),
        ValueExprAst::TimesKicked => Ok(ValueExpr::EventValue(EventValueKind::KickerCount(0))),
        ValueExprAst::ColorsOfManaSpent => {
            Ok(ValueExpr::EventValue(EventValueKind::ColorsSpentToCast))
        }
        ValueExprAst::ManaSpentToCast { mana_filter: None } => {
            Ok(ValueExpr::EventValue(EventValueKind::ManaSpentToCast))
        }
        ValueExprAst::ManaSpentToCast {
            mana_filter: Some(f),
        } => {
            let low = f.trim().to_ascii_lowercase();
            if low.contains("snow") || low == "{s}" {
                Ok(ValueExpr::EventValue(EventValueKind::SnowManaSpentToCast))
            } else if low.contains("treasure") {
                Ok(ValueExpr::EventValue(
                    EventValueKind::TreasureManaSpentToCast,
                ))
            } else if low.contains("creature") {
                Ok(ValueExpr::EventValue(
                    EventValueKind::CreatureManaSpentToCast,
                ))
            } else if let Some(mt) = parse_single_mana_type(&low) {
                Ok(ValueExpr::EventValue(EventValueKind::ManaTypeSpentToCast(
                    mt,
                )))
            } else {
                Err("oracle:unsupported-value-expr")
            }
        }
        ValueExprAst::ObjectStat { subject, stat } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => ValueObjectRef::AbilitySource,
                SubjectRefAst::TriggerSubject => ValueObjectRef::DyingCreature,
                SubjectRefAst::AttachedHost(_) => ValueObjectRef::AttachedHost,
                SubjectRefAst::Anaphora(_) => ValueObjectRef::TargetObject,
                _ => return Err("oracle:unsupported-stat-subject"),
            };
            Ok(ValueExpr::ObjectStat {
                subject: subj,
                stat: st,
            })
        }
        ValueExprAst::AggregateStat { op, stat, filter } => {
            let agg = match op {
                AggregateOp::Max => StatAggregate::Max,
                AggregateOp::Sum => StatAggregate::Sum,
                AggregateOp::Min => return Err("oracle:min-aggregate-unsupported"),
            };
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ValueExpr::aggregate(obj, st, agg))
        }
        ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            counter_kind,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?;
            let subject = match ck {
                CounterKind::Experience => {
                    spellwake_engine::program::CounterSubjectRef::ControllerExperience
                }
                CounterKind::Energy => {
                    spellwake_engine::program::CounterSubjectRef::ControllerEnergy
                }
                CounterKind::Poison => {
                    spellwake_engine::program::CounterSubjectRef::ControllerPoison
                }
                _ => return Err("oracle:unsupported-counter-kind"),
            };
            Ok(ValueExpr::CounterCount {
                subject,
                kind: None,
            })
        }
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if matches!(
            subject.as_ref(),
            SubjectRefAst::SelfObject
                | SubjectRefAst::TriggerSubject
                | SubjectRefAst::AttachedHost(_)
                | SubjectRefAst::Anaphora(_)
        ) =>
        {
            let kind = if counter_kind.eq_ignore_ascii_case("any") || counter_kind.trim().is_empty()
            {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?)
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => {
                    spellwake_engine::program::CounterSubjectRef::AbilitySource
                }
                SubjectRefAst::TriggerSubject => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSubject
                }
                SubjectRefAst::AttachedHost(_) => {
                    spellwake_engine::program::CounterSubjectRef::AttachedHost
                }
                SubjectRefAst::Anaphora(_) => {
                    spellwake_engine::program::CounterSubjectRef::TargetObject
                }
                _ => unreachable!(),
            };
            Ok(ValueExpr::CounterCount {
                subject: subj,
                kind,
            })
        }
        ValueExprAst::Add(a, b) => Ok(ValueExpr::Add(
            Box::new(lower_value_expr(a, vocabulary)?),
            Box::new(lower_value_expr(b, vocabulary)?),
        )),
        ValueExprAst::Sub(a, b) => Ok(ValueExpr::Sub(
            Box::new(lower_value_expr(a, vocabulary)?),
            Box::new(lower_value_expr(b, vocabulary)?),
        )),
        ValueExprAst::DomainCount => Ok(ValueExpr::DomainCount),
        ValueExprAst::OpponentsCount => Ok(ValueExpr::OpponentsCount),
        ValueExprAst::Devotion { colors } => {
            let mut mask = ColorMask::EMPTY;
            let low = colors.to_ascii_lowercase();
            if low.contains("white") || colors.contains('W') {
                mask = mask.union(ColorMask::WHITE);
            }
            if low.contains("blue") || colors.contains('U') {
                mask = mask.union(ColorMask::BLUE);
            }
            if low.contains("black") || colors.contains('B') {
                mask = mask.union(ColorMask::BLACK);
            }
            if low.contains("red") || colors.contains('R') {
                mask = mask.union(ColorMask::RED);
            }
            if low.contains("green") || colors.contains('G') {
                mask = mask.union(ColorMask::GREEN);
            }
            if mask.is_empty() {
                return Err("oracle:unsupported-value-expr");
            }
            Ok(ValueExpr::Devotion(mask))
        }
        ValueExprAst::Mul(a, b) => match (a.as_ref(), b.as_ref()) {
            (ValueExprAst::Const(k), inner) | (inner, ValueExprAst::Const(k)) => {
                Ok(ValueExpr::Scale {
                    expr: Box::new(lower_value_expr(inner, vocabulary)?),
                    factor: i64::from(*k).into(),
                })
            }
            _ => Err("oracle:non-const-mul"),
        },
        ValueExprAst::DivConst {
            expr,
            denom,
            rounding,
        } if *denom > 0 => {
            let d = NonZeroU16::new(u16::try_from(*denom).map_err(|_| "oracle:div-ovf")?)
                .ok_or("oracle:div-zero")?;
            Ok(ValueExpr::DivConst {
                expr: Box::new(lower_value_expr(expr, vocabulary)?),
                divisor: d,
                round_up: *rounding == Rounding::Up,
            })
        }
        ValueExprAst::PriorResult => Ok(ValueExpr::EventValue(EventValueKind::LastActionCount)),
        ValueExprAst::RollDie { .. } => Ok(ValueExpr::EventValue(EventValueKind::LastDieRoll)),
        _ => Err("oracle:unsupported-value-expr"),
    }
}

pub(super) fn parse_look_top_pick_filter(
    pick_low: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let stripped = pick_low
        .trim()
        .trim_start_matches("up to one ")
        .trim_start_matches("a ")
        .trim_start_matches("an ")
        .trim_start_matches("one ")
        .trim_end_matches(" cards from among them")
        .trim_end_matches(" card from among them")
        .trim_end_matches(" from among them")
        .trim_end_matches(" cards")
        .trim_end_matches(" card")
        .trim();
    match stripped {
        "creature" => Some(ObjectFilter::creature()),
        "land" => Some(ObjectFilter::land()),
        "artifact" => Some(ObjectFilter::artifact()),
        "enchantment" => Some(ObjectFilter::enchantment()),
        "planeswalker" => Some(ObjectFilter::planeswalker()),
        "instant" => Some(ObjectFilter::instant()),
        "sorcery" => Some(ObjectFilter::sorcery()),
        "permanent" => Some(ObjectFilter::permanent()),
        "nonland" | "nonland permanent" => Some(ObjectFilter::nonland()),
        "noncreature" => Some(ObjectFilter::from_clause(
            ObjectClause::any().forbid_types(CardTypeMask::CREATURE),
        )),
        "instant or sorcery" | "sorcery or instant" => Some(ObjectFilter::instant_or_sorcery()),
        "creature or land" | "land or creature" => Some(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::LAND),
        )),
        "creature or planeswalker" | "planeswalker or creature" => Some(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
        )),
        "artifact or enchantment" | "enchantment or artifact" => Some(ObjectFilter::any_types(
            CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT),
        )),
        "artifact or creature" | "creature or artifact" => Some(ObjectFilter::any_types(
            CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE),
        )),
        "artifact or land" | "land or artifact" => Some(ObjectFilter::any_types(
            CardTypeMask::ARTIFACT.union(CardTypeMask::LAND),
        )),
        "creature or enchantment" | "enchantment or creature" => Some(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::ENCHANTMENT),
        )),
        "basic land" => {
            let mut clause = ObjectClause::land();
            clause.required_supertypes = spellwake_engine::program::SupertypeMask::BASIC;
            Some(ObjectFilter::from_clause(clause))
        }
        "legendary" | "legendary permanent" => Some(ObjectFilter::any().legendary()),
        "legendary creature" => Some(ObjectFilter::creature().legendary()),
        other => {
            let cap = {
                let mut chars = other.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            };
            if let Some(ct) = vocabulary.resolve_label(&cap) {
                return Some(ObjectFilter::any().with_subtype(ct));
            }
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            if let Ok(mut parsed) =
                spellwake_oracle_compiler::filter::parse_object_filter(other, &compiler_vocab)
            {
                parsed.zone = None;
                if let Ok(obj) = lower_object_filter(&parsed, vocabulary) {
                    return Some(obj);
                }
            }
            None
        }
    }
}

pub(super) fn parse_counter_kind(raw: &str) -> Option<CounterKind> {
    let lower = raw.trim().to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "{e}" | "e" | "energy" => "energy",
        other => other,
    };
    CounterKind::from_oracle_label(normalized)
}

#[derive(Clone, Copy)]
pub(super) struct FilterAllow {
    pub(super) types: bool,
    pub(super) forbidden_types: bool,
    pub(super) req_supertypes: bool,
    pub(super) subtypes: bool,
    pub(super) colors: bool,
    pub(super) controller: bool,
    pub(super) zone: bool,
    pub(super) combat: bool,
    pub(super) token: bool,
    pub(super) req_keywords: bool,
    pub(super) stat_bounds: bool,
    pub(super) counter_kind: bool,
    pub(super) flags: bool,
}

impl FilterAllow {
    pub(super) const NONE: Self = Self {
        types: false,
        forbidden_types: false,
        req_supertypes: false,
        subtypes: false,
        colors: false,
        controller: false,
        zone: false,
        combat: false,
        token: false,
        req_keywords: false,
        stat_bounds: false,
        counter_kind: false,
        flags: false,
    };
}

/// Exhaustive check that `f` carries no predicates outside `allow`.
/// Full struct destructuring ensures any new field on `ObjectFilterAst`
/// must be audited here at compile time rather than silently dropped.
pub(super) fn filter_only_has(f: &ObjectFilterAst, allow: FilterAllow) -> bool {
    let ObjectFilterAst {
        any_types,
        forbidden_types,
        required_supertypes,
        forbidden_supertypes,
        subtypes,
        forbidden_subtypes,
        any_colors,
        forbidden_colors,
        color_arity,
        controller,
        zone,
        combat,
        tapped,
        token,
        required_keywords,
        forbidden_keywords,
        stat_bounds,
        counter_kind,
        flags,
        any_permanent: _,
        any_card: _,
        any_spell: _,
        is_ability,
        named,
    } = f;
    (allow.types || any_types.is_empty())
        && (allow.forbidden_types || forbidden_types.is_empty())
        && (allow.req_supertypes || required_supertypes.is_empty())
        && forbidden_supertypes.is_empty()
        && (allow.subtypes || subtypes.is_empty())
        && forbidden_subtypes.is_empty()
        && (allow.colors || any_colors.is_empty())
        && forbidden_colors.is_empty()
        && color_arity.is_none()
        && (allow.controller || controller.is_none())
        && (allow.zone || zone.is_none())
        && (allow.combat || combat.is_none())
        && tapped.is_none()
        && (allow.token || token.is_none())
        && (allow.req_keywords || required_keywords.is_empty())
        && forbidden_keywords.is_empty()
        && (allow.stat_bounds || stat_bounds.is_empty())
        && (allow.counter_kind || counter_kind.is_none())
        && (allow.flags || flags.is_empty())
        && !*is_ability
        && named.is_none()
}

pub(super) fn is_unrestricted_filter(f: &ObjectFilterAst) -> bool {
    filter_only_has(f, FilterAllow::NONE) && !f.any_permanent
}
