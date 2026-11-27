use super::*;
use spellwake_oracle_compiler::ast::{FilterPredicateAst, PriorActionRefKind};

fn resolve_outlaw_creature_types(
    vocabulary: &CreatureTypeVocabulary,
) -> Option<[CreatureTypeId; 5]> {
    Some([
        vocabulary.resolve_label("Assassin")?,
        vocabulary.resolve_label("Mercenary")?,
        vocabulary.resolve_label("Pirate")?,
        vocabulary.resolve_label("Rogue")?,
        vocabulary.resolve_label("Warlock")?,
    ])
}

fn try_union_any_types_clauses(a: ObjectClause, b: ObjectClause) -> Option<ObjectClause> {
    if a.any_types.is_empty()
        || b.any_types.is_empty()
        || !a.required_types.is_empty()
        || !b.required_types.is_empty()
        || !a.forbidden_types.is_empty()
        || !b.forbidden_types.is_empty()
    {
        return None;
    }
    let mut a_norm = a;
    let mut b_norm = b;
    a_norm.any_types = CardTypeMask::EMPTY;
    b_norm.any_types = CardTypeMask::EMPTY;
    if a_norm == b_norm {
        a_norm.any_types = a.any_types.union(b.any_types);
        Some(a_norm)
    } else {
        None
    }
}

fn try_union_color_clauses(a: ObjectClause, b: ObjectClause) -> Option<ObjectClause> {
    let (
        ObjectColorFilter::Mask {
            any_of: a_any,
            none_of: a_none,
        },
        ObjectColorFilter::Mask {
            any_of: b_any,
            none_of: b_none,
        },
    ) = (a.color, b.color)
    else {
        return None;
    };
    if a_any.is_empty() || b_any.is_empty() || a_none != b_none {
        return None;
    }
    let mut a_norm = a;
    let mut b_norm = b;
    a_norm.color = ObjectColorFilter::Any;
    b_norm.color = ObjectColorFilter::Any;
    if a_norm == b_norm {
        a_norm.color = ObjectColorFilter::Mask {
            any_of: a_any.union(b_any),
            none_of: a_none,
        };
        Some(a_norm)
    } else {
        None
    }
}

/// Pick one canonical implied bit from `req` for `ObjectClause::any_types` so helpers such as
/// `guarantees_creature()`, `guarantees_damageable()`, and `allows_instant_or_sorcery()` remain
/// accurate alongside `ObjectClause::required_types`.
fn canonical_any_type_for_required(req: CardTypeMask) -> CardTypeMask {
    if req.contains(CardType::Creature) {
        CardTypeMask::CREATURE
    } else if req.contains(CardType::Planeswalker) {
        CardTypeMask::PLANESWALKER
    } else if req.contains(CardType::Battle) {
        CardTypeMask::BATTLE
    } else if req.contains(CardType::Instant) {
        CardTypeMask::INSTANT
    } else if req.contains(CardType::Sorcery) {
        CardTypeMask::SORCERY
    } else if req.contains(CardType::Land) {
        CardTypeMask::LAND
    } else if req.contains(CardType::Artifact) {
        CardTypeMask::ARTIFACT
    } else if req.contains(CardType::Enchantment) {
        CardTypeMask::ENCHANTMENT
    } else {
        req
    }
}

fn is_simple_attachment_filter(att_f: &ObjectFilterAst) -> bool {
    att_f.zone.is_none()
        && att_f.extra_zones.is_empty()
        && att_f.predicates.is_empty()
        && att_f.controller.is_none()
        && att_f.owner.is_none()
        && att_f.any_colors.is_empty()
        && att_f.forbidden_colors.is_empty()
        && att_f.color_arity.is_none()
        && att_f.forbidden_types.is_empty()
        && att_f.required_supertypes.is_empty()
        && att_f.forbidden_supertypes.is_empty()
        && att_f.forbidden_subtypes.is_empty()
        && att_f.combat.is_none()
        && att_f.tapped.is_none()
        && att_f.token.is_none()
        && att_f.required_keywords.is_empty()
        && att_f.forbidden_keywords.is_empty()
        && att_f.stat_bounds.is_empty()
        && att_f.counter_kind.is_none()
        && att_f.named.is_none()
        && att_f.flags.0 == 0
}

fn prior_action_moved_count_zone(f: &ObjectFilterAst) -> Option<ValueZone> {
    let mut inferred: Option<ValueZone> = None;
    for pred in &f.predicates {
        let next = match pred {
            FilterPredicateAst::AffectedByPriorAction(kind) => match kind {
                PriorActionRefKind::ExiledThisWay => Some(ValueZone::Exile(None)),
                PriorActionRefKind::DiscardedThisWay
                | PriorActionRefKind::DestroyedThisWay
                | PriorActionRefKind::SacrificedThisWay
                | PriorActionRefKind::PutIntoGraveyardThisWay => Some(ValueZone::AllGraveyards),
                PriorActionRefKind::ReturnedToHandThisWay => Some(ValueZone::AllHands),
                _ => None,
            },
            FilterPredicateAst::PriorActionZoneRef { kind } => match kind.as_str() {
                "milled_this_way" => Some(ValueZone::AllGraveyards),
                _ => None,
            },
            FilterPredicateAst::StateQualification(raw) => {
                match raw.trim().to_ascii_lowercase().as_str() {
                    "exiled this way"
                    | "that was exiled this way"
                    | "that were exiled this way" => Some(ValueZone::Exile(None)),
                    "discarded this way"
                    | "you discarded this way"
                    | "destroyed this way"
                    | "that were destroyed this way"
                    | "put into a graveyard this way"
                    | "that was put into a graveyard this way"
                    | "that were put into a graveyard this way"
                    | "put into graveyards this way"
                    | "you put into your graveyard this way"
                    | "milled this way"
                    | "they milled this way" => Some(ValueZone::AllGraveyards),
                    "returned this way" => Some(ValueZone::AllHands),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(z) = next {
            if inferred.is_some_and(|prev| prev != z) {
                return None;
            }
            inferred = Some(z);
        }
    }
    inferred
}

fn is_unrestricted_last_moved_count_filter(inner: &ObjectFilterAst) -> bool {
    let single_moved_pred = match inner.predicates.as_slice() {
        [FilterPredicateAst::AffectedByPriorAction(kind)] => matches!(
            kind,
            PriorActionRefKind::ExiledThisWay
                | PriorActionRefKind::DiscardedThisWay
                | PriorActionRefKind::DestroyedThisWay
                | PriorActionRefKind::SacrificedThisWay
                | PriorActionRefKind::PutIntoGraveyardThisWay
                | PriorActionRefKind::ReturnedToHandThisWay
        ),
        [FilterPredicateAst::PriorActionZoneRef { kind }] => {
            matches!(
                kind.as_str(),
                "milled_this_way" | "put_onto_battlefield_this_way"
            )
        }
        _ => false,
    };
    if !single_moved_pred {
        return false;
    }
    let mut stripped = inner.clone();
    stripped.predicates.clear();
    stripped.zone = None;
    filter_only_has(&stripped, FilterAllow::NONE)
}

pub(super) fn lower_permanent_selectors(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<PermanentSelector>, &'static str> {
    match subject {
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => lower_permanent_selectors(inner, vocabulary),
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
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => lower_permanent_selector(inner, vocabulary),
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

fn apply_selector_required_keyword(
    sel: &mut PermanentSelector,
    kw_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    let low = kw_raw.trim().to_ascii_lowercase();
    if low == "toxic" {
        sel.requires_toxic = true;
        return Ok(());
    }
    let kw = lower_keyword_ability_name(kw_raw)
        .or_else(|| {
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
    } else if sel.required_keyword.is_none() || sel.required_keyword == Some(kw) {
        sel.required_keyword = Some(kw);
    } else if sel.extra_required_keyword.is_none() || sel.extra_required_keyword == Some(kw) {
        sel.extra_required_keyword = Some(kw);
    } else {
        return Err("oracle:unsupported-selector-predicate");
    }
    Ok(())
}

fn apply_selector_counter_kind(
    sel: &mut PermanentSelector,
    ck_raw: &str,
) -> Result<(), &'static str> {
    let ck = ck_raw.trim().to_ascii_lowercase();
    let ck_clean = ck
        .trim_end_matches(" counters")
        .trim_end_matches(" counter")
        .trim();
    match ck_clean {
        "+1/+1" => sel.plus_one_counter = true,
        "-1/-1" => sel.minus_one_counter = true,
        "" | "any" | "a" | "one or more" => sel.any_counter = true,
        other => {
            let kind = parse_counter_kind(other)
                .filter(|k| k.is_permanent())
                .ok_or("oracle:unsupported-selector-predicate")?;
            if sel.required_counter_kind.is_some_and(|prev| prev != kind) {
                return Err("oracle:unsupported-selector-predicate");
            }
            sel.required_counter_kind = Some(kind);
        }
    }
    Ok(())
}

fn extract_state_qual_counter_kind(norm: &str) -> Option<&str> {
    let body = norm
        .strip_prefix("with ")
        .or_else(|| norm.strip_prefix("that have "))
        .or_else(|| norm.strip_prefix("that has "))?;
    let body = body
        .strip_suffix(" on it")
        .or_else(|| body.strip_suffix(" on them"))?;
    let body = body
        .strip_suffix(" counters")
        .or_else(|| body.strip_suffix(" counter"))?;
    let kind = body
        .strip_prefix("one or more ")
        .or_else(|| body.strip_prefix("a "))
        .or_else(|| body.strip_prefix("an "))
        .unwrap_or(body)
        .trim();
    Some(kind)
}

fn resolve_selector_subtype(
    st: &str,
    any_types: AstCardTypeMask,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::SelectorSubtype> {
    use spellwake_engine::characteristics::SelectorSubtype;
    if any_types == AstCardTypeMask::PLANESWALKER
        && let Some(pw) = parse_planeswalker_subtype(st)
    {
        return Some(SelectorSubtype::Planeswalker(pw));
    }
    if st.eq_ignore_ascii_case("aura") {
        return Some(SelectorSubtype::Aura);
    }
    if let Some(ct) = vocabulary.resolve_label(st) {
        return Some(SelectorSubtype::Creature(ct));
    }
    if let Some(lt) = parse_land_subtype(st) {
        return Some(SelectorSubtype::Land(lt));
    }
    if let Some(at) = ArtifactSubtype::from_wire_label(st) {
        return Some(SelectorSubtype::Artifact(at));
    }
    if let Some(et) = EnchantmentSubtype::from_wire_label(st) {
        return Some(SelectorSubtype::Enchantment(et));
    }
    parse_planeswalker_subtype(st).map(SelectorSubtype::Planeswalker)
}

pub(super) fn lower_filter_to_permanent_selector(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    lower_filter_to_permanent_selector_with_host_context(f, false, vocabulary)
}

pub(super) fn lower_filter_to_permanent_selector_with_host_context(
    f: &ObjectFilterAst,
    subject_is_attached_host: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    use spellwake_engine::characteristics::{OwnerSelector, SelectorSubtype};
    if !matches!(f.zone, None | Some(ZoneFilter::Battlefield))
        || !f.extra_zones.is_empty()
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
                EnglishCardName::new(n.trim().to_string())
                    .map_err(|_| "oracle:unsupported-selector-predicate")?,
            ),
            NameFilterAst::SameAs(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    spellwake_engine::characteristics::NameFilter::SharesWithSource
                }
                SubjectRefAst::Anaphora(raw)
                    if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                {
                    spellwake_engine::characteristics::NameFilter::SharesWithSource
                }
                _ => return Err("oracle:unsupported-selector-predicate"),
            },
            NameFilterAst::SameAsReferenced => {
                return Err("oracle:unsupported-selector-predicate");
            }
        };
    }
    if let Some(ck_raw) = f.counter_kind.as_deref() {
        apply_selector_counter_kind(&mut sel, ck_raw)?;
    }
    for sb in &f.stat_bounds {
        match (sb.stat, sb.op, sb.value.as_ref()) {
            (
                StatKind::Toughness,
                CompareOp::Gt,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                },
            )
            | (
                StatKind::Power,
                CompareOp::Lt,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Toughness,
                },
            ) if matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "its" | "his" | "her" | "their"
                    )
            ) =>
            {
                sel.toughness_greater_than_power = true;
                continue;
            }
            _ => {}
        }
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
    if f.flags.0 & AstFilterFlags::WITHOUT_COUNTERS.0 != 0 {
        if sel.any_counter
            || sel.plus_one_counter
            || sel.minus_one_counter
            || sel.required_counter_kind.is_some()
        {
            return Err("oracle:unsupported-selector-flag");
        }
        sel.without_counters = true;
    }
    if f.flags.0 & AstFilterFlags::EQUIPPED.0 != 0 {
        sel.equipped_by_at_least = 1;
    }
    if f.flags.0 & AstFilterFlags::ENCHANTED.0 != 0 {
        sel.requires_enchanted = true;
    }
    if f.flags.0 & AstFilterFlags::MODIFIED.0 != 0 {
        sel.requires_modified = true;
    }
    let has_is_commander = (f.flags.0 & AstFilterFlags::IS_COMMANDER.0) != 0;
    let has_not_commander = (f.flags.0 & AstFilterFlags::NOT_COMMANDER.0) != 0;
    if has_is_commander && has_not_commander {
        return Err("oracle:unsupported-selector-flag");
    }
    if has_is_commander {
        sel.commander = Some(true);
    } else if has_not_commander {
        sel.commander = Some(false);
    }
    let has_face_down = (f.flags.0 & AstFilterFlags::FACE_DOWN.0) != 0;
    let has_face_up = (f.flags.0 & AstFilterFlags::FACE_UP.0) != 0;
    if has_face_down && has_face_up {
        return Err("oracle:unsupported-selector-flag");
    }
    if has_face_down {
        sel.face_down = Some(true);
    } else if has_face_up {
        sel.face_down = Some(false);
    }
    if f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0 != 0 {
        sel.entered_this_turn = true;
    }
    let has_attacked = (f.flags.0 & AstFilterFlags::ATTACKED_THIS_TURN.0) != 0;
    let has_did_not_attack = (f.flags.0 & AstFilterFlags::DID_NOT_ATTACK_THIS_TURN.0) != 0;
    if has_attacked && has_did_not_attack {
        return Err("oracle:unsupported-selector-flag");
    }
    if has_attacked {
        sel.attacked_this_turn = Some(true);
    } else if has_did_not_attack {
        sel.attacked_this_turn = Some(false);
    }
    if f.flags.0 & AstFilterFlags::BLOCKED_THIS_TURN.0 != 0 {
        sel.blocked_this_turn = Some(true);
    }
    if f.flags.0 & AstFilterFlags::PAIRED_WITH_SELF.0 != 0 {
        sel.paired_with_source = true;
    }
    if f.flags.0 & AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0 != 0 {
        sel.dealt_damage_this_turn = true;
    }
    if f.flags.0 & AstFilterFlags::ATTACHED_TO_SELF.0 != 0 {
        if subject_is_attached_host {
            sel.attached_to_source_host = true;
        } else {
            sel.attached_to_source_permanent = true;
        }
    }
    if f.flags.0
        & (AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0 | AstFilterFlags::MATCHES_CHOSEN_TYPE.0)
        != 0
    {
        sel.matches_source_entry_type = true;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_COLOR.0 != 0 {
        sel.matches_source_entry_color = true;
    }
    let keywords_match_any = (f.flags.0 & AstFilterFlags::KEYWORDS_MATCH_ANY.0) != 0;
    let unsupported_flags = f.flags.0
        & !(AstFilterFlags::EXCLUDE_SELF.0
            | AstFilterFlags::WITH_ANY_COUNTER.0
            | AstFilterFlags::WITHOUT_COUNTERS.0
            | AstFilterFlags::EQUIPPED.0
            | AstFilterFlags::ENCHANTED.0
            | AstFilterFlags::MODIFIED.0
            | AstFilterFlags::IS_COMMANDER.0
            | AstFilterFlags::NOT_COMMANDER.0
            | AstFilterFlags::FACE_DOWN.0
            | AstFilterFlags::FACE_UP.0
            | AstFilterFlags::ENTERED_THIS_TURN.0
            | AstFilterFlags::ATTACKED_THIS_TURN.0
            | AstFilterFlags::DID_NOT_ATTACK_THIS_TURN.0
            | AstFilterFlags::BLOCKED_THIS_TURN.0
            | AstFilterFlags::PAIRED_WITH_SELF.0
            | AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0
            | AstFilterFlags::ATTACHED_TO_SELF.0
            | AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0
            | AstFilterFlags::MATCHES_CHOSEN_TYPE.0
            | AstFilterFlags::MATCHES_CHOSEN_COLOR.0
            | AstFilterFlags::KEYWORDS_MATCH_ANY.0
            | AstFilterFlags::LKI_PAST_TENSE.0);
    if unsupported_flags != 0 {
        return Err("oracle:unsupported-selector-flag");
    }

    if keywords_match_any && f.required_keywords.len() > 1 {
        for kw_raw in &f.required_keywords {
            let kw = lower_keyword_ability_name(kw_raw)
                .ok_or("oracle:unsupported-selector-predicate")?;
            sel.any_required_keywords.insert(kw);
        }
    } else {
        for kw_raw in &f.required_keywords {
            apply_selector_required_keyword(&mut sel, kw_raw, vocabulary)?;
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
        Some(CombatState::Blocked) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Blocked);
        }
        Some(CombatState::Unblocked) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Unblocked);
        }
        Some(CombatState::BlockingSelf) => {
            sel.combat_role =
                Some(spellwake_engine::characteristics::CombatSelector::BlockingSource);
        }
        Some(CombatState::BlockedBySelf) => {
            sel.combat_role =
                Some(spellwake_engine::characteristics::CombatSelector::BlockedBySource);
        }
        Some(CombatState::BlockingOrBlockedBySelf) => {
            sel.combat_role =
                Some(spellwake_engine::characteristics::CombatSelector::BlockingOrBlockedBySource);
        }
        Some(CombatState::NotAttacking) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::NotAttacking);
        }
    }
    let types_match_all = f
        .predicates
        .iter()
        .any(|p| matches!(p, FilterPredicateAst::TypesMatchAll));
    let mut any_types_set = card_type_mask_to_set(f.any_types)?;
    if types_match_all {
        sel.required_types.extend(any_types_set);
    } else if any_types_set.len() == 1 {
        if let Some(t) = any_types_set.pop_first() {
            sel.required_types.insert(t);
        }
    } else if any_types_set.len() > 1 {
        sel.any_types = any_types_set;
    }
    sel.forbidden_types = card_type_mask_to_set(f.forbidden_types)?;
    if !sel.required_types.is_disjoint(&sel.forbidden_types) {
        return Err("oracle:unsupported-selector-predicate");
    }
    let req_historic = (f.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0;
    let forb_historic = (f.forbidden_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0;
    if req_historic && forb_historic {
        return Err("oracle:unsupported-selector-predicate");
    }
    if req_historic {
        sel.historic = Some(true);
    } else if forb_historic {
        sel.historic = Some(false);
    }
    sel.required_supertypes = supertype_mask_to_set(AstSupertypeMask(
        f.required_supertypes.0 & !AstSupertypeMask::HISTORIC.0,
    ))?;
    sel.forbidden_supertypes = supertype_mask_to_set(AstSupertypeMask(
        f.forbidden_supertypes.0 & !AstSupertypeMask::HISTORIC.0,
    ))?;

    // Subtypes
    if f.subtypes.len() > 1 {
        let match_any = f
            .predicates
            .iter()
            .any(|p| matches!(p, FilterPredicateAst::SubtypesMatchAny));
        if match_any {
            if let Some(lands) = f
                .subtypes
                .iter()
                .map(|st| parse_land_subtype(st))
                .collect::<Option<BTreeSet<LandType>>>()
            {
                sel.any_land_subtypes = lands;
            } else if let Some(creatures) = f
                .subtypes
                .iter()
                .map(|st| vocabulary.resolve_label(st))
                .collect::<Option<BTreeSet<CreatureTypeId>>>()
            {
                sel.any_creature_subtypes = creatures;
            } else {
                for st in &f.subtypes {
                    if st.eq_ignore_ascii_case("outlaw") {
                        let outlaws = resolve_outlaw_creature_types(vocabulary)
                            .ok_or("oracle:selector-multi-subtypes")?;
                        for ct in outlaws {
                            sel.any_subtypes.insert(SelectorSubtype::Creature(ct));
                        }
                    } else {
                        let sub = resolve_selector_subtype(st, f.any_types, vocabulary)
                            .ok_or("oracle:selector-multi-subtypes")?;
                        sel.any_subtypes.insert(sub);
                    }
                }
            }
        } else {
            // Stage 1 currently drops `"and"` between plural subtype lists (`Mounts and Vehicles`,
            // `Skeletons and Zombies`) without emitting `SubtypesMatchAny`, making them
            // indistinguishable in the AST from conjunctive compound subtypes (`Eldrazi Spawn`).
            // Fail closed until Stage 1 explicitly distinguishes disjunction from conjunction.
            return Err("oracle:selector-multi-subtypes");
        }
    } else if let Some(st) = f.subtypes.first() {
        if st.eq_ignore_ascii_case("outlaw") {
            let outlaws = resolve_outlaw_creature_types(vocabulary)
                .ok_or("oracle:selector-unknown-subtype")?;
            sel.any_creature_subtypes.extend(outlaws);
        } else {
            let sub = resolve_selector_subtype(st, f.any_types, vocabulary)
                .ok_or("oracle:selector-unknown-subtype")?;
            match sub {
                SelectorSubtype::Creature(ct) => sel.creature_subtype = Some(ct),
                SelectorSubtype::Land(lt) => sel.land_subtype = Some(lt),
                SelectorSubtype::Artifact(at) => sel.artifact_subtype = Some(at),
                SelectorSubtype::Planeswalker(pw) => sel.planeswalker_subtype = Some(pw),
                SelectorSubtype::Enchantment(_) | SelectorSubtype::Aura => {
                    sel.required_subtypes.insert(sub);
                }
            }
        }
    }
    if f.forbidden_subtypes.len() == 1 {
        let fst = &f.forbidden_subtypes[0];
        if fst.eq_ignore_ascii_case("aura") {
            sel.exclude_auras = true;
        } else if fst.eq_ignore_ascii_case("outlaw") {
            let outlaws = resolve_outlaw_creature_types(vocabulary)
                .ok_or("oracle:selector-unknown-forbidden-subtype")?;
            for ct in outlaws {
                sel.forbidden_subtypes.insert(SelectorSubtype::Creature(ct));
            }
        } else if let Some(ct) = vocabulary.resolve_label(fst) {
            sel.forbidden_creature_subtype = Some(ct);
        } else if let Some(sub) = resolve_selector_subtype(fst, f.any_types, vocabulary) {
            sel.forbidden_subtypes.insert(sub);
        } else {
            return Err("oracle:selector-unknown-forbidden-subtype");
        }
    } else if f.forbidden_subtypes.len() > 1 {
        for fst in &f.forbidden_subtypes {
            if fst.eq_ignore_ascii_case("outlaw") {
                let outlaws = resolve_outlaw_creature_types(vocabulary)
                    .ok_or("oracle:selector-unknown-forbidden-subtype")?;
                for ct in outlaws {
                    sel.forbidden_subtypes.insert(SelectorSubtype::Creature(ct));
                }
            } else {
                let sub = resolve_selector_subtype(fst, f.any_types, vocabulary)
                    .ok_or("oracle:selector-unknown-forbidden-subtype")?;
                sel.forbidden_subtypes.insert(sub);
            }
        }
    }

    // Color
    sel.color = lower_color_filter(f.any_colors, f.forbidden_colors, f.color_arity)?;

    // Controller & owner
    sel.controller = match f.controller {
        None => ControllerSelector::Any,
        Some(ControllerRelation::You | ControllerRelation::YourTeam) => {
            ControllerSelector::SourceController
        }
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            ControllerSelector::Opponents
        }
        Some(ControllerRelation::OwnerYou) => ControllerSelector::OwnerIsSourceController,
        Some(ControllerRelation::OwnerOpponent) => ControllerSelector::OwnerIsOpponent,
        _ => return Err("oracle:selector-unsupported-controller"),
    };
    sel.owner = match f.owner {
        None => None,
        Some(ControllerRelation::You | ControllerRelation::OwnerYou) => {
            Some(OwnerSelector::SourceController)
        }
        Some(
            ControllerRelation::Opponent
            | ControllerRelation::Opponents
            | ControllerRelation::OwnerOpponent,
        ) => Some(OwnerSelector::Opponents),
        _ => return Err("oracle:selector-unsupported-controller"),
    };
    let set_sel_combat = |sel: &mut PermanentSelector,
                          role: spellwake_engine::characteristics::CombatSelector|
     -> Result<(), &'static str> {
        if sel.combat_role.is_some_and(|prev| prev != role) {
            return Err("oracle:unsupported-selector-predicate");
        }
        sel.combat_role = Some(role);
        Ok(())
    };
    for pred in &f.predicates {
        match pred {
            FilterPredicateAst::SubtypesMatchAny | FilterPredicateAst::TypesMatchAll => {}
            FilterPredicateAst::HasBasicLandType => {
                if sel.land_subtype.is_some() || !sel.any_land_subtypes.is_empty() {
                    return Err("oracle:unsupported-selector-predicate");
                }
                sel.any_land_subtypes.extend([
                    LandType::Plains,
                    LandType::Island,
                    LandType::Swamp,
                    LandType::Mountain,
                    LandType::Forest,
                ]);
            }
            FilterPredicateAst::ColorCountCompare { op, expr } => {
                let target_color = match (op, expr.as_ref()) {
                    (CompareOp::Eq | CompareOp::Le, ValueExprAst::Const(0)) => {
                        ColorFilter::Colorless
                    }
                    (CompareOp::Eq, ValueExprAst::Const(1)) => ColorFilter::Monocolored,
                    (CompareOp::Ge, ValueExprAst::Const(1))
                    | (CompareOp::Gt, ValueExprAst::Const(0)) => ColorFilter::Colored,
                    (CompareOp::Ge, ValueExprAst::Const(2))
                    | (CompareOp::Gt, ValueExprAst::Const(1)) => ColorFilter::Multicolored,
                    _ => return Err("oracle:unsupported-selector-predicate"),
                };
                if sel.color != ColorFilter::Any && sel.color != target_color {
                    return Err("oracle:unsupported-selector-predicate");
                }
                sel.color = target_color;
            }
            FilterPredicateAst::HasAbilityKind { kind, negated } => {
                let norm_kind = kind.trim().to_ascii_lowercase();
                if norm_kind == "toxic" && !negated {
                    sel.requires_toxic = true;
                } else if let Some(kw) = lower_keyword_ability_name(&norm_kind) {
                    if *negated {
                        sel.forbidden_keywords.insert(kw);
                    } else {
                        apply_selector_required_keyword(&mut sel, &norm_kind, vocabulary)?;
                    }
                } else {
                    return Err("oracle:unsupported-selector-predicate");
                }
            }
            FilterPredicateAst::AttachedBy(att_f) => {
                let simple_attachment = is_simple_attachment_filter(att_f);
                if simple_attachment
                    && att_f.subtypes.len() == 1
                    && att_f.subtypes[0].eq_ignore_ascii_case("aura")
                    && (att_f.any_types.is_empty()
                        || att_f.any_types == AstCardTypeMask::ENCHANTMENT)
                {
                    sel.requires_enchanted = true;
                } else if simple_attachment
                    && att_f.subtypes.len() == 1
                    && att_f.subtypes[0].eq_ignore_ascii_case("equipment")
                    && (att_f.any_types.is_empty() || att_f.any_types == AstCardTypeMask::ARTIFACT)
                {
                    sel.equipped_by_at_least = sel.equipped_by_at_least.max(1);
                } else {
                    return Err("oracle:unsupported-selector-predicate");
                }
            }
            FilterPredicateAst::AttachedTo(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    if subject_is_attached_host {
                        sel.attached_to_source_host = true;
                    } else {
                        sel.attached_to_source_permanent = true;
                    }
                }
                SubjectRefAst::AttachedHost(_) => {
                    sel.attached_to_source_host = true;
                }
                _ => return Err("oracle:unsupported-selector-predicate"),
            },
            FilterPredicateAst::Blocking(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    set_sel_combat(
                        &mut sel,
                        spellwake_engine::characteristics::CombatSelector::BlockingSource,
                    )?;
                }
                SubjectRefAst::Anaphora(raw)
                    if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                {
                    set_sel_combat(
                        &mut sel,
                        spellwake_engine::characteristics::CombatSelector::BlockingSource,
                    )?;
                }
                _ => return Err("oracle:unsupported-selector-predicate"),
            },
            FilterPredicateAst::BlockedBy {
                blocker,
                this_turn: false,
            } => match peel_battlefield_subject(blocker) {
                SubjectRefAst::SelfObject => {
                    set_sel_combat(
                        &mut sel,
                        spellwake_engine::characteristics::CombatSelector::BlockedBySource,
                    )?;
                }
                SubjectRefAst::Anaphora(raw)
                    if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                {
                    set_sel_combat(
                        &mut sel,
                        spellwake_engine::characteristics::CombatSelector::BlockedBySource,
                    )?;
                }
                _ => return Err("oracle:unsupported-selector-predicate"),
            },
            FilterPredicateAst::BlockedOrBlockedBy {
                other,
                this_turn: false,
            } if filter_represents_self_object(other) => {
                set_sel_combat(
                    &mut sel,
                    spellwake_engine::characteristics::CombatSelector::BlockingOrBlockedBySource,
                )?;
            }
            FilterPredicateAst::DifferentNameFrom(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    if !matches!(
                        sel.name,
                        spellwake_engine::characteristics::NameFilter::Any
                            | spellwake_engine::characteristics::NameFilter::DifferentFromSource
                    ) {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    sel.name = spellwake_engine::characteristics::NameFilter::DifferentFromSource;
                }
                SubjectRefAst::Anaphora(raw)
                    if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                {
                    if !matches!(
                        sel.name,
                        spellwake_engine::characteristics::NameFilter::Any
                            | spellwake_engine::characteristics::NameFilter::DifferentFromSource
                    ) {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    sel.name = spellwake_engine::characteristics::NameFilter::DifferentFromSource;
                }
                _ => return Err("oracle:unsupported-selector-predicate"),
            },
            FilterPredicateAst::SharesPropertyWith {
                property,
                reference,
                negated,
            } if matches!(
                property.trim().to_ascii_lowercase().as_str(),
                "name" | "a name"
            ) =>
            {
                let is_self = match peel_battlefield_subject(reference) {
                    SubjectRefAst::SelfObject => true,
                    SubjectRefAst::Anaphora(raw) => {
                        is_self_object_anaphora(&raw.trim().to_ascii_lowercase())
                    }
                    _ => false,
                };
                if !is_self {
                    return Err("oracle:unsupported-selector-predicate");
                }
                let want = if *negated {
                    spellwake_engine::characteristics::NameFilter::DifferentFromSource
                } else {
                    spellwake_engine::characteristics::NameFilter::SharesWithSource
                };
                if sel.name != spellwake_engine::characteristics::NameFilter::Any
                    && sel.name != want
                {
                    return Err("oracle:unsupported-selector-predicate");
                }
                sel.name = want;
            }
            FilterPredicateAst::ManaValueIn(vals) => {
                if sel.min_mana_value.is_some() || sel.max_mana_value.is_some() {
                    return Err("oracle:unsupported-selector-predicate");
                }
                let [v] = vals.as_slice() else {
                    return Err("oracle:unsupported-selector-predicate");
                };
                let v_u32 =
                    u32::try_from(*v).map_err(|_| "oracle:unsupported-selector-predicate")?;
                sel.min_mana_value = Some(v_u32);
                sel.max_mana_value = Some(v_u32);
            }
            FilterPredicateAst::StateQualification(raw) => {
                use spellwake_engine::characteristics::{CombatSelector, NameFilter};
                let norm = raw.trim().to_ascii_lowercase();
                match norm.as_str() {
                    "that's attacking" | "that is attacking" => {
                        set_sel_combat(&mut sel, CombatSelector::Attacking)?;
                    }
                    "that's blocking" | "that is blocking" => {
                        set_sel_combat(&mut sel, CombatSelector::Blocking)?;
                    }
                    "that's attacking or blocking" | "that is attacking or blocking" => {
                        set_sel_combat(&mut sel, CombatSelector::AttackingOrBlocking)?;
                    }
                    "that's blocked" | "that is blocked" => {
                        set_sel_combat(&mut sel, CombatSelector::Blocked)?;
                    }
                    "that's unblocked" | "that is unblocked" => {
                        set_sel_combat(&mut sel, CombatSelector::Unblocked)?;
                    }
                    "that's blocking it" | "blocking it" => {
                        set_sel_combat(&mut sel, CombatSelector::BlockingSource)?;
                    }
                    "that's tapped" | "that is tapped" | "that are tapped" => {
                        if sel.requires_untapped {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.requires_tapped = true;
                    }
                    "that's untapped" | "that is untapped" | "that are untapped" => {
                        if sel.requires_tapped {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.requires_untapped = true;
                    }
                    "that's equipped" | "that is equipped" | "that are equipped" => {
                        sel.equipped_by_at_least = sel.equipped_by_at_least.max(1);
                    }
                    "that's enchanted" | "that is enchanted" | "that are enchanted" => {
                        sel.requires_enchanted = true;
                    }
                    "that's modified" | "that is modified" | "that are modified" => {
                        sel.requires_modified = true;
                    }
                    "that attacked this turn" | "you attacked with this turn" => {
                        if sel.attacked_this_turn == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.attacked_this_turn = Some(true);
                    }
                    "that didn't attack this turn" => {
                        if sel.attacked_this_turn == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.attacked_this_turn = Some(false);
                    }
                    "that blocked this turn" | "blocked this turn" => {
                        if sel.blocked_this_turn == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.blocked_this_turn = Some(true);
                    }
                    "that didn't block this turn" => {
                        if sel.blocked_this_turn == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.blocked_this_turn = Some(false);
                    }
                    "paired with ~"
                    | "that's paired with ~"
                    | "that is paired with ~"
                    | "paired with it"
                    | "that's paired with it" => {
                        sel.paired_with_source = true;
                    }
                    "that entered this turn" | "that entered the battlefield this turn" => {
                        sel.entered_this_turn = true;
                    }
                    "that was dealt damage this turn"
                    | "that were dealt damage this turn"
                    | "that has been dealt damage this turn"
                    | "that have been dealt damage this turn" => {
                        sel.dealt_damage_this_turn = true;
                    }
                    "that's historic" | "historic" => {
                        if sel.historic == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.historic = Some(true);
                    }
                    "that's not historic" | "not historic" => {
                        if sel.historic == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.historic = Some(false);
                    }
                    "that's a commander" | "commander" => {
                        if sel.commander == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.commander = Some(true);
                    }
                    "that isn't a commander" | "that aren't commanders" | "noncommander" => {
                        if sel.commander == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.commander = Some(false);
                    }
                    "face down" | "face-down" | "that's face down" | "that is face down"
                    | "that are face down" => {
                        if sel.face_down == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.face_down = Some(true);
                    }
                    "face up" | "face-up" | "that's face up" | "that is face up"
                    | "that are face up" => {
                        if sel.face_down == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.face_down = Some(false);
                    }
                    "that's toxic" | "with toxic" => {
                        sel.requires_toxic = true;
                    }
                    "that's one or more colors" | "that are one or more colors" => {
                        if sel.color != ColorFilter::Any && sel.color != ColorFilter::Colored {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.color = ColorFilter::Colored;
                    }
                    "with a +1/+1 counter on it"
                    | "that has a +1/+1 counter on it"
                    | "that have a +1/+1 counter on them"
                    | "with a +1/+1 counter on them"
                    | "with +1/+1 counters on them"
                    | "with one or more +1/+1 counters on it" => {
                        sel.plus_one_counter = true;
                    }
                    "with a -1/-1 counter on it"
                    | "that have -1/-1 counters on them"
                    | "that has a -1/-1 counter on it"
                    | "with -1/-1 counters on them"
                    | "with a -1/-1 counter on them" => {
                        sel.minus_one_counter = true;
                    }
                    "with a counter on it"
                    | "with counters on it"
                    | "with a counter on them"
                    | "with counters on them"
                    | "with one or more counters on it"
                    | "with one or more counters on them"
                    | "that has a counter on it"
                    | "that have a counter on them" => {
                        sel.any_counter = true;
                    }
                    "without counters on it"
                    | "with no counters on it"
                    | "that has no counters on it"
                    | "that have no counters on them"
                    | "without counters on them"
                    | "with no counters on them"
                    | "that doesn't have a counter on it"
                    | "that don't have counters on them" => {
                        sel.without_counters = true;
                    }
                    "with toughness greater than its power"
                    | "with toughness greater than their power"
                    | "whose toughness is greater than its power"
                    | "toughness_greater_than_power" => {
                        sel.toughness_greater_than_power = true;
                    }
                    "with the same name as ~"
                    | "that has the same name as ~"
                    | "that have the same name as ~" => {
                        if !matches!(sel.name, NameFilter::Any | NameFilter::SharesWithSource) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.name = NameFilter::SharesWithSource;
                    }
                    "that don't have the same name as ~"
                    | "that doesn't have the same name as ~"
                    | "with a different name than ~" => {
                        if !matches!(sel.name, NameFilter::Any | NameFilter::DifferentFromSource) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.name = NameFilter::DifferentFromSource;
                    }
                    "you own" => {
                        if sel
                            .owner
                            .is_some_and(|o| o != OwnerSelector::SourceController)
                            || matches!(
                                sel.controller,
                                ControllerSelector::OwnerIsSourceController
                                    | ControllerSelector::OwnerIsOpponent
                            )
                        {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.owner = Some(OwnerSelector::SourceController);
                    }
                    "you own and control" => {
                        if !matches!(
                            sel.controller,
                            ControllerSelector::Any | ControllerSelector::SourceController
                        ) || sel
                            .owner
                            .is_some_and(|o| o != OwnerSelector::SourceController)
                        {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.controller = ControllerSelector::SourceController;
                        sel.owner = Some(OwnerSelector::SourceController);
                    }
                    "kicked" => {
                        sel.kicked = true;
                    }
                    "renowned" => {
                        sel.renowned = true;
                    }
                    "saddled" => {
                        sel.saddled = true;
                    }
                    "monstrous" => {
                        sel.monstrous = true;
                    }
                    "suspected" => {
                        sel.suspected = true;
                    }
                    "solved" => {
                        sel.solved = true;
                    }
                    "exerted" => {
                        sel.exerted = true;
                    }
                    other => {
                        if let Some(pt_raw) = other.strip_prefix("pt:")
                            && let Some((p_s, t_s)) = pt_raw.split_once('/')
                            && let (Ok(p), Ok(t)) =
                                (p_s.trim().parse::<i64>(), t_s.trim().parse::<i64>())
                        {
                            if sel.min_power.is_some()
                                || sel.max_power.is_some()
                                || sel.min_toughness.is_some()
                                || sel.max_toughness.is_some()
                            {
                                return Err("oracle:unsupported-selector-predicate");
                            }
                            sel.min_power = Some(p);
                            sel.max_power = Some(p);
                            sel.min_toughness = Some(t);
                            sel.max_toughness = Some(t);
                        } else if let Some(ck_raw) = extract_state_qual_counter_kind(other) {
                            apply_selector_counter_kind(&mut sel, ck_raw)?;
                        } else if let Some(rest) = other.strip_prefix("with ") {
                            apply_selector_required_keyword(&mut sel, rest, vocabulary)?;
                        } else if let Some(rest) = other.strip_prefix("without ")
                            && let Some(kw) = lower_keyword_ability_name(rest)
                        {
                            sel.forbidden_keywords.insert(kw);
                        } else {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                    }
                }
            }
            FilterPredicateAst::Exclude(subj)
                if matches!(peel_battlefield_subject(subj), SubjectRefAst::SelfObject)
                    || matches!(
                        peel_battlefield_subject(subj),
                        SubjectRefAst::Anaphora(raw)
                            if is_self_object_anaphora(&raw.trim().to_ascii_lowercase())
                    ) =>
            {
                sel.exclude_source = true;
            }
            FilterPredicateAst::Exclude(subj)
                if matches!(subj.as_ref(), SubjectRefAst::Group(_)) =>
            {
                use spellwake_engine::characteristics::NameFilter;
                use spellwake_oracle_compiler::ast::NameFilterAst;
                let SubjectRefAst::Group(excl) = subj.as_ref() else {
                    unreachable!();
                };
                let preds_only_subtype_any = excl
                    .predicates
                    .iter()
                    .all(|p| matches!(p, FilterPredicateAst::SubtypesMatchAny));
                let excl_simple_shell = excl.zone.is_none()
                    && excl.extra_zones.is_empty()
                    && excl.controller.is_none()
                    && excl.owner.is_none()
                    && excl.combat.is_none()
                    && excl.stat_bounds.is_empty()
                    && excl.counter_kind.is_none()
                    && excl.forbidden_types.0 == 0
                    && excl.forbidden_supertypes.0 == 0
                    && excl.forbidden_subtypes.is_empty()
                    && excl.forbidden_colors.0 == 0
                    && excl.forbidden_keywords.is_empty()
                    && !excl.any_spell
                    && !excl.is_ability
                    && preds_only_subtype_any;
                if !excl_simple_shell || excl.tapped == Some(false) || excl.token == Some(false) {
                    return Err("oracle:unsupported-selector-predicate");
                }
                let dim_types = excl.any_types.0 != 0;
                let dim_supers = excl.required_supertypes.0 != 0;
                let dim_subs = !excl.subtypes.is_empty();
                let dim_tapped = excl.tapped == Some(true);
                let dim_token = excl.token == Some(true);
                let dim_kws = !excl.required_keywords.is_empty();
                let dim_colors = excl.any_colors.0 != 0;
                let dim_arity = excl.color_arity.is_some();
                let dim_named = excl.named.is_some();
                let dim_flags = excl.flags.0 != 0;
                let active_dims = usize::from(dim_types)
                    + usize::from(dim_supers)
                    + usize::from(dim_subs)
                    + usize::from(dim_tapped)
                    + usize::from(dim_token)
                    + usize::from(dim_kws)
                    + usize::from(dim_colors)
                    + usize::from(dim_arity)
                    + usize::from(dim_named)
                    + usize::from(dim_flags);
                if active_dims != 1 {
                    return Err("oracle:unsupported-selector-predicate");
                }
                if dim_subs {
                    let is_disjunctive = excl.subtypes.len() == 1
                        || excl
                            .predicates
                            .contains(&FilterPredicateAst::SubtypesMatchAny);
                    if !is_disjunctive {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    for fst in &excl.subtypes {
                        if fst.eq_ignore_ascii_case("aura") {
                            sel.exclude_auras = true;
                        } else if fst.eq_ignore_ascii_case("outlaw") {
                            let outlaws = resolve_outlaw_creature_types(vocabulary)
                                .ok_or("oracle:unsupported-selector-predicate")?;
                            for ct in outlaws {
                                sel.forbidden_subtypes.insert(SelectorSubtype::Creature(ct));
                            }
                        } else if let Some(sub) =
                            resolve_selector_subtype(fst, f.any_types, vocabulary)
                        {
                            sel.forbidden_subtypes.insert(sub);
                        } else {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                    }
                } else if dim_types {
                    for ct in card_type_mask_to_set(excl.any_types)? {
                        sel.forbidden_types.insert(ct);
                    }
                } else if dim_supers {
                    if (excl.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0 {
                        if excl.required_supertypes.0 != AstSupertypeMask::HISTORIC.0
                            || sel.historic == Some(true)
                        {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.historic = Some(false);
                    } else {
                        if excl.required_supertypes.0.count_ones() != 1 {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        for st in supertype_mask_to_set(excl.required_supertypes)? {
                            sel.forbidden_supertypes.insert(st);
                        }
                    }
                } else if dim_colors {
                    let bits = ast_color_mask_to_bits(excl.any_colors);
                    sel.color = match sel.color {
                        ColorFilter::Any => {
                            lower_color_filter(AstColorMask::EMPTY, excl.any_colors, None)
                                .map_err(|_| "oracle:unsupported-selector-predicate")?
                        }
                        ColorFilter::Lacks(c) => ColorFilter::LacksAll(c.bit() | bits),
                        ColorFilter::LacksAll(prev) => ColorFilter::LacksAll(prev | bits),
                        _ => return Err("oracle:unsupported-selector-predicate"),
                    };
                } else if dim_arity {
                    let negated_color = match excl.color_arity {
                        Some(ColorArity::Colorless) => ColorFilter::Colored,
                        Some(ColorArity::Colored) => ColorFilter::Colorless,
                        _ => return Err("oracle:unsupported-selector-predicate"),
                    };
                    if sel.color != ColorFilter::Any && sel.color != negated_color {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    sel.color = negated_color;
                } else if dim_named {
                    if !matches!(excl.named, Some(NameFilterAst::SelfName))
                        || !matches!(sel.name, NameFilter::Any | NameFilter::DifferentFromSource)
                    {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    sel.name = NameFilter::DifferentFromSource;
                } else if dim_flags {
                    if excl.flags == AstFilterFlags::IS_COMMANDER {
                        if sel.commander == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.commander = Some(false);
                    } else if excl.flags == AstFilterFlags::FACE_DOWN {
                        if sel.face_down == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.face_down = Some(false);
                    } else if excl.flags == AstFilterFlags::FACE_UP {
                        if sel.face_down == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.face_down = Some(true);
                    } else if excl.flags == AstFilterFlags::ATTACKED_THIS_TURN {
                        if sel.attacked_this_turn == Some(true) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.attacked_this_turn = Some(false);
                    } else if excl.flags == AstFilterFlags::DID_NOT_ATTACK_THIS_TURN {
                        if sel.attacked_this_turn == Some(false) {
                            return Err("oracle:unsupported-selector-predicate");
                        }
                        sel.attacked_this_turn = Some(true);
                    } else {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                } else if dim_token {
                    if sel.token == Some(true) {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    sel.token = Some(false);
                } else if dim_tapped {
                    if sel.requires_tapped {
                        return Err("oracle:unsupported-selector-predicate");
                    }
                    sel.requires_untapped = true;
                } else if dim_kws {
                    for kw_raw in &excl.required_keywords {
                        let kw = lower_keyword_ability_name(kw_raw)
                            .ok_or("oracle:unsupported-selector-predicate")?;
                        sel.forbidden_keywords.insert(kw);
                    }
                }
            }
            _ => return Err("oracle:unsupported-selector-predicate"),
        }
    }
    if sel.without_counters
        && (sel.any_counter
            || sel.plus_one_counter
            || sel.minus_one_counter
            || sel.minimum_plus_one_counters > 0
            || sel.required_counter_kind.is_some())
    {
        return Err("oracle:unsupported-selector-predicate");
    }
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
            if let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0]) {
                sel.source_condition = Some(StaticSourceCondition::ControllerControlsSubtype(ct));
                return Ok(());
            }
            if let Some(sc) = lower_static_source_condition(cond, vocabulary) {
                sel.source_condition = Some(sc);
                return Ok(());
            }
            Err("oracle:cond-unknown-subtype")
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
            if let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0]) {
                sel.source_condition = Some(StaticSourceCondition::OpponentControlsSubtype(ct));
                return Ok(());
            }
            if let Some(sc) = lower_static_source_condition(cond, vocabulary) {
                sel.source_condition = Some(sc);
                return Ok(());
            }
            Err("oracle:cond-unknown-subtype")
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
            push_selector_source_condition(
                sel,
                StaticSourceCondition::GraveyardCountAtLeast {
                    types,
                    minimum: *value,
                },
            );
            Ok(())
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || (is_pronoun_anaphora(subject)
                    && sel.controller == ControllerSelector::SourceItself) =>
        {
            let cond_opt = match status.trim().to_ascii_lowercase().as_str() {
                "tapped" => Some(StaticSourceCondition::SourceIsTapped),
                "untapped" => Some(StaticSourceCondition::SourceIsUntapped),
                "equipped" => Some(StaticSourceCondition::SourceIsEquipped),
                "enchanted" => Some(StaticSourceCondition::SourceIsEnchanted),
                "solved" => Some(StaticSourceCondition::SourceIsSolved),
                "prepared" => Some(StaticSourceCondition::SourceIsPrepared),
                "attacking" if sel.controller == ControllerSelector::SourceItself => {
                    apply_selector_combat_role(
                        sel,
                        spellwake_engine::characteristics::CombatSelector::Attacking,
                    )?;
                    return Ok(());
                }
                "blocking" if sel.controller == ControllerSelector::SourceItself => {
                    apply_selector_combat_role(
                        sel,
                        spellwake_engine::characteristics::CombatSelector::Blocking,
                    )?;
                    return Ok(());
                }
                _ => None,
            };
            if let Some(sc) = cond_opt {
                push_selector_source_condition(sel, sc);
                return Ok(());
            }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && let Some(sc) = lower_static_source_condition(cond, vocabulary)
            {
                push_selector_source_condition(sel, sc);
                Ok(())
            } else {
                Err("oracle:unsupported-static-cond")
            }
        }
        ConditionAst::HasStatus { subject, status }
            if (matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_))
                || is_pronoun_anaphora(subject))
                && sel.attached_to_source =>
        {
            apply_selector_status(sel, status)
        }
        ConditionAst::HasStatus { subject, status }
            if is_pronoun_anaphora(subject)
                && sel.controller != ControllerSelector::SourceItself
                && !sel.attached_to_source =>
        {
            apply_selector_status(sel, status)
        }
        ConditionAst::Not(inner)
            if matches!(
                inner.as_ref(),
                ConditionAst::HasStatus { subject, .. }
                    if is_pronoun_anaphora(subject)
                        && sel.controller != ControllerSelector::SourceItself
            ) =>
        {
            if let ConditionAst::HasStatus { status, .. } = inner.as_ref() {
                apply_selector_negated_status(sel, status)
            } else {
                Err("oracle:unsupported-static-cond")
            }
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) =>
        {
            if let Ok(host_sel) = lower_filter_to_permanent_selector(filter, vocabulary) {
                push_selector_source_condition(
                    sel,
                    StaticSourceCondition::AttachedHostMatches(Box::new(host_sel)),
                );
                Ok(())
            } else if let Some(sc) = lower_selector_source_condition(sel, cond, vocabulary) {
                push_selector_source_condition(sel, sc);
                Ok(())
            } else {
                Err("oracle:unsupported-static-cond")
            }
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: op @ (CompareOp::Ge | CompareOp::Gt),
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let min_val = if *op == CompareOp::Gt {
                value.saturating_add(1)
            } else {
                *value
            };
            if min_val == 0 {
                return Err("oracle:unsupported-static-cond");
            }
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let sc = if matches!(ck_low.as_str(), "" | "any" | "a" | "one or more") {
                StaticSourceCondition::SourceAnyCounterAtLeast(min_val)
            } else if let Some(kind) = parse_counter_kind(&ck_low).filter(|k| k.is_permanent()) {
                StaticSourceCondition::SourceCounterAtLeast {
                    kind,
                    minimum: min_val,
                }
            } else {
                return Err("oracle:unsupported-static-cond");
            };
            sel.source_condition = Some(match sel.source_condition.take() {
                Some(prev) => StaticSourceCondition::And(Box::new(prev), Box::new(sc)),
                None => sc,
            });
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value,
        } if *value > 0 => {
            let f_sel = lower_filter_to_permanent_selector(filter, vocabulary)
                .map_err(|_| "oracle:unsupported-static-cond")?;
            let sc = StaticSourceCondition::ControlsExactCount {
                selector: Box::new(f_sel),
                count: *value,
            };
            sel.source_condition = Some(match sel.source_condition.take() {
                Some(prev) => StaticSourceCondition::And(Box::new(prev), Box::new(sc)),
                None => sc,
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
        ConditionAst::MatchesFilter { subject, filter }
            if is_pronoun_anaphora(subject)
                && sel.controller != ControllerSelector::SourceItself
                && !sel.attached_to_source =>
        {
            fold_selector_condition_filter(sel, filter, vocabulary)
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: op @ (CompareOp::Ge | CompareOp::Gt),
            value,
        } if is_pronoun_anaphora(subject)
            && sel.controller != ControllerSelector::SourceItself
            && !sel.attached_to_source =>
        {
            let min_val = if *op == CompareOp::Gt {
                value.saturating_add(1)
            } else {
                *value
            };
            if min_val == 0 {
                return Err("oracle:unsupported-static-cond");
            }
            match counter_kind.trim().to_ascii_lowercase().as_str() {
                "+1/+1" => {
                    if min_val == 1 {
                        sel.plus_one_counter = true;
                    } else {
                        sel.minimum_plus_one_counters = sel.minimum_plus_one_counters.max(min_val);
                    }
                    Ok(())
                }
                "-1/-1" if min_val == 1 => {
                    sel.minus_one_counter = true;
                    Ok(())
                }
                "" | "any" | "a" | "one or more" if min_val == 1 => {
                    sel.any_counter = true;
                    Ok(())
                }
                other if min_val == 1 => apply_selector_counter_kind(sel, other)
                    .map_err(|_| "oracle:unsupported-static-cond"),
                _ => Err("oracle:unsupported-static-cond"),
            }
        }
        ConditionAst::HasKeyword { subject, keyword }
            if is_pronoun_anaphora(subject)
                && sel.controller != ControllerSelector::SourceItself
                && !sel.attached_to_source =>
        {
            apply_selector_required_keyword(sel, keyword, vocabulary)
                .map_err(|_| "oracle:unsupported-static-cond")
        }
        other => {
            if let Some(sc) = lower_selector_source_condition(sel, other, vocabulary) {
                sel.source_condition = Some(match sel.source_condition.take() {
                    Some(prev) => StaticSourceCondition::And(Box::new(prev), Box::new(sc)),
                    None => sc,
                });
                Ok(())
            } else {
                Err("oracle:unsupported-static-cond")
            }
        }
    }
}

fn push_selector_source_condition(sel: &mut PermanentSelector, sc: StaticSourceCondition) {
    sel.source_condition = Some(match sel.source_condition.take() {
        Some(prev) => StaticSourceCondition::And(Box::new(prev), Box::new(sc)),
        None => sc,
    });
}

fn apply_selector_combat_role(
    sel: &mut PermanentSelector,
    role: spellwake_engine::characteristics::CombatSelector,
) -> Result<(), &'static str> {
    if sel.combat_role.is_some_and(|prev| prev != role) {
        return Err("oracle:unsupported-static-cond");
    }
    sel.combat_role = Some(role);
    Ok(())
}

fn apply_selector_status(sel: &mut PermanentSelector, status: &str) -> Result<(), &'static str> {
    use spellwake_engine::characteristics::CombatSelector;
    match status.trim().to_ascii_lowercase().as_str() {
        "tapped" => {
            if sel.requires_untapped {
                return Err("oracle:unsupported-static-cond");
            }
            sel.requires_tapped = true;
        }
        "untapped" => {
            if sel.requires_tapped {
                return Err("oracle:unsupported-static-cond");
            }
            sel.requires_untapped = true;
        }
        "equipped" => {
            sel.equipped_by_at_least = sel.equipped_by_at_least.max(1);
        }
        "enchanted" => {
            sel.requires_enchanted = true;
        }
        "attacking" => apply_selector_combat_role(sel, CombatSelector::Attacking)?,
        "blocking" => apply_selector_combat_role(sel, CombatSelector::Blocking)?,
        "attacking or blocking" => {
            apply_selector_combat_role(sel, CombatSelector::AttackingOrBlocking)?;
        }
        "blocked" => apply_selector_combat_role(sel, CombatSelector::Blocked)?,
        "unblocked" => apply_selector_combat_role(sel, CombatSelector::Unblocked)?,
        "token" => {
            if sel.token == Some(false) {
                return Err("oracle:unsupported-static-cond");
            }
            sel.token = Some(true);
        }
        "nontoken" => {
            if sel.token == Some(true) {
                return Err("oracle:unsupported-static-cond");
            }
            sel.token = Some(false);
        }
        _ => return Err("oracle:unsupported-static-cond"),
    }
    Ok(())
}

fn apply_selector_negated_status(
    sel: &mut PermanentSelector,
    status: &str,
) -> Result<(), &'static str> {
    use spellwake_engine::characteristics::CombatSelector;
    match status.trim().to_ascii_lowercase().as_str() {
        "tapped" => apply_selector_status(sel, "untapped"),
        "untapped" => apply_selector_status(sel, "tapped"),
        "attacking" => apply_selector_combat_role(sel, CombatSelector::NotAttacking),
        "token" => apply_selector_status(sel, "nontoken"),
        "nontoken" => apply_selector_status(sel, "token"),
        _ => Err("oracle:unsupported-static-cond"),
    }
}

/// Fold a per-permanent `MatchesFilter` on `Anaphora("it")` directly into a `PermanentSelector`
/// to preserve CR 611.3a dynamic matching, failing closed on contradictions, non-local predicates,
/// or Layer 4/6/7 cyclic keyword or P/T predicates.
fn fold_selector_condition_filter(
    sel: &mut PermanentSelector,
    filter: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    use spellwake_engine::characteristics::NameFilter;
    let extra = lower_filter_to_permanent_selector(filter, vocabulary)
        .map_err(|_| "oracle:unsupported-static-cond")?;
    if extra.controller != ControllerSelector::Any
        || extra.attached_to_source
        || extra.sole_controlled_creature
        || extra.while_controlling_own_commander
        || extra.source_has_imprinted_creature
        || extra.controlled_creatures_and_typed_tokens.is_some()
        || extra.controller_type_threshold.is_some()
        || extra.source_condition.is_some()
        || extra.devotion_below.is_some()
        || extra.station_at.is_some()
        // Fail closed on Layer 6 keyword predicates and Layer 7 P/T bounds inside MatchesFilter
        // to avoid Layer 4/6/7 cyclic dependencies across arbitrary static parts.
        || extra.requires_flying
        || extra.required_keyword.is_some()
        || extra.extra_required_keyword.is_some()
        || extra.requires_toxic
        || extra.forbidden_keyword.is_some()
        || !extra.forbidden_keywords.is_empty()
        || extra.min_power.is_some()
        || extra.max_power.is_some()
        || extra.min_toughness.is_some()
        || extra.max_toughness.is_some()
    {
        return Err("oracle:unsupported-static-cond");
    }

    let mut merged = sel.clone();
    if let Some(role) = extra.combat_role {
        apply_selector_combat_role(&mut merged, role)?;
    }
    if extra.requires_tapped {
        if merged.requires_untapped {
            return Err("oracle:unsupported-static-cond");
        }
        merged.requires_tapped = true;
    }
    if extra.requires_untapped {
        if merged.requires_tapped {
            return Err("oracle:unsupported-static-cond");
        }
        merged.requires_untapped = true;
    }
    if let Some(tok) = extra.token {
        if merged.token.is_some_and(|prev| prev != tok) {
            return Err("oracle:unsupported-static-cond");
        }
        merged.token = Some(tok);
    }
    merged.required_supertypes.extend(extra.required_supertypes);
    merged
        .forbidden_supertypes
        .extend(extra.forbidden_supertypes);
    merged.required_types.extend(extra.required_types);
    merged.forbidden_types.extend(extra.forbidden_types);
    if !extra.any_types.is_empty() {
        if merged.any_types.is_empty() {
            merged.any_types = extra.any_types;
        } else if merged.any_types != extra.any_types {
            return Err("oracle:unsupported-static-cond");
        }
    }
    if !merged.required_types.is_disjoint(&merged.forbidden_types)
        || !merged
            .required_supertypes
            .is_disjoint(&merged.forbidden_supertypes)
        || (!merged.any_types.is_empty() && merged.any_types.is_subset(&merged.forbidden_types))
    {
        return Err("oracle:unsupported-static-cond");
    }
    if let Some(ct) = extra.creature_subtype {
        if merged.creature_subtype.is_some_and(|prev| prev != ct) {
            return Err("oracle:unsupported-static-cond");
        }
        merged.creature_subtype = Some(ct);
    }
    if !extra.any_creature_subtypes.is_empty() {
        if merged.any_creature_subtypes.is_empty() {
            merged.any_creature_subtypes = extra.any_creature_subtypes;
        } else if merged.any_creature_subtypes != extra.any_creature_subtypes {
            return Err("oracle:unsupported-static-cond");
        }
    }
    if let Some(fct) = extra.forbidden_creature_subtype {
        if merged
            .forbidden_creature_subtype
            .is_some_and(|prev| prev != fct)
        {
            return Err("oracle:unsupported-static-cond");
        }
        merged.forbidden_creature_subtype = Some(fct);
    }
    if merged.creature_subtype.is_some()
        && merged.creature_subtype == merged.forbidden_creature_subtype
    {
        return Err("oracle:unsupported-static-cond");
    }
    if let Some(lt) = extra.land_subtype {
        if merged.land_subtype.is_some_and(|prev| prev != lt) {
            return Err("oracle:unsupported-static-cond");
        }
        merged.land_subtype = Some(lt);
    }
    if let Some(at) = extra.artifact_subtype {
        if merged.artifact_subtype.is_some_and(|prev| prev != at) {
            return Err("oracle:unsupported-static-cond");
        }
        merged.artifact_subtype = Some(at);
    }
    if let Some(pw) = extra.planeswalker_subtype {
        if merged.planeswalker_subtype.is_some_and(|prev| prev != pw) {
            return Err("oracle:unsupported-static-cond");
        }
        merged.planeswalker_subtype = Some(pw);
    }
    if extra.color != ColorFilter::Any {
        if merged.color != ColorFilter::Any && merged.color != extra.color {
            return Err("oracle:unsupported-static-cond");
        }
        merged.color = extra.color;
    }
    if extra.name != NameFilter::Any {
        if merged.name != NameFilter::Any && merged.name != extra.name {
            return Err("oracle:unsupported-static-cond");
        }
        merged.name = extra.name;
    }
    merged.plus_one_counter |= extra.plus_one_counter;
    merged.minus_one_counter |= extra.minus_one_counter;
    merged.minimum_plus_one_counters = merged
        .minimum_plus_one_counters
        .max(extra.minimum_plus_one_counters);
    merged.any_counter |= extra.any_counter;
    if let Some(ck) = extra.required_counter_kind {
        if merged.required_counter_kind.is_some_and(|prev| prev != ck) {
            return Err("oracle:unsupported-static-cond");
        }
        merged.required_counter_kind = Some(ck);
    }
    merged.exclude_auras |= extra.exclude_auras;
    merged.matches_source_entry_type |= extra.matches_source_entry_type;
    merged.equipped_by_at_least = merged.equipped_by_at_least.max(extra.equipped_by_at_least);
    merged.requires_enchanted |= extra.requires_enchanted;
    merged.exclude_source |= extra.exclude_source;
    if let Some(min_mv) = extra.min_mana_value {
        merged.min_mana_value = Some(
            merged
                .min_mana_value
                .map_or(min_mv, |prev| prev.max(min_mv)),
        );
    }
    if let Some(max_mv) = extra.max_mana_value {
        merged.max_mana_value = Some(
            merged
                .max_mana_value
                .map_or(max_mv, |prev| prev.min(max_mv)),
        );
    }
    if let (Some(lo), Some(hi)) = (merged.min_mana_value, merged.max_mana_value)
        && lo > hi
    {
        return Err("oracle:unsupported-static-cond");
    }
    merged
        .validate(vocabulary)
        .map_err(|_| "oracle:unsupported-static-cond")?;
    *sel = merged;
    Ok(())
}

fn is_pronoun_anaphora(subject: &SubjectRefAst) -> bool {
    matches!(
        subject,
        SubjectRefAst::Anaphora(raw)
            if matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "it" | "he" | "she" | "him" | "her"
            )
    )
}

fn lower_compare_op(op: CompareOp) -> Comparison {
    match op {
        CompareOp::Lt => Comparison::Less,
        CompareOp::Le => Comparison::LessOrEqual,
        CompareOp::Eq => Comparison::Equal,
        CompareOp::Ge => Comparison::GreaterOrEqual,
        CompareOp::Gt => Comparison::Greater,
    }
}

fn lower_selector_value_expr(
    sel: &PermanentSelector,
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ValueExpr> {
    match expr {
        ValueExprAst::ObjectStat { subject, stat } if is_pronoun_anaphora(subject) => {
            let obj_ref = if sel.controller == ControllerSelector::SourceItself {
                ValueObjectRef::AbilitySource
            } else if sel.attached_to_source {
                ValueObjectRef::AttachedHost
            } else {
                return None;
            };
            Some(ValueExpr::object_stat(obj_ref, lower_stat_kind(*stat)))
        }
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if is_pronoun_anaphora(subject) => {
            let subj_ref = if sel.controller == ControllerSelector::SourceItself {
                spellwake_engine::program::CounterSubjectRef::AbilitySource
            } else if sel.attached_to_source {
                spellwake_engine::program::CounterSubjectRef::AttachedHost
            } else {
                return None;
            };
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let kind = if matches!(ck_low.as_str(), "" | "any" | "a" | "one or more") {
                None
            } else {
                Some(parse_counter_kind(&ck_low).filter(|k| k.is_permanent())?)
            };
            Some(ValueExpr::counters(subj_ref, kind))
        }
        other => {
            let lowered = lower_value_expr(other, vocabulary).ok()?;
            lowered.is_world_evaluable().then_some(lowered)
        }
    }
}

fn lower_selector_source_condition(
    sel: &PermanentSelector,
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticSourceCondition> {
    match cond {
        ConditionAst::Not(inner) => {
            if matches!(inner.as_ref(), ConditionAst::IsYourTurn) {
                return Some(StaticSourceCondition::OpponentsTurn);
            }
            Some(StaticSourceCondition::Not(Box::new(
                lower_selector_source_condition(sel, inner, vocabulary)?,
            )))
        }
        ConditionAst::And(a, b) => Some(StaticSourceCondition::And(
            Box::new(lower_selector_source_condition(sel, a, vocabulary)?),
            Box::new(lower_selector_source_condition(sel, b, vocabulary)?),
        )),
        ConditionAst::Or(a, b) => Some(StaticSourceCondition::Or(
            Box::new(lower_selector_source_condition(sel, a, vocabulary)?),
            Box::new(lower_selector_source_condition(sel, b, vocabulary)?),
        )),
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if is_pronoun_anaphora(subject) => {
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let kind = if matches!(ck_low.as_str(), "" | "any" | "a" | "one or more") {
                None
            } else {
                Some(parse_counter_kind(&ck_low).filter(|k| k.is_permanent())?)
            };
            if sel.controller == ControllerSelector::SourceItself {
                if matches!(op, CompareOp::Ge | CompareOp::Gt) {
                    let min_val = if *op == CompareOp::Gt {
                        value.saturating_add(1)
                    } else {
                        *value
                    };
                    if min_val >= 1 {
                        return Some(match kind {
                            None => StaticSourceCondition::SourceAnyCounterAtLeast(min_val),
                            Some(k) => StaticSourceCondition::SourceCounterAtLeast {
                                kind: k,
                                minimum: min_val,
                            },
                        });
                    }
                }
                Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::counters(
                        spellwake_engine::program::CounterSubjectRef::AbilitySource,
                        kind,
                    ),
                    comparison: lower_compare_op(*op),
                    right: ValueExpr::literal(i64::from(*value)),
                })))
            } else if sel.attached_to_source {
                Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::counters(
                        spellwake_engine::program::CounterSubjectRef::AttachedHost,
                        kind,
                    ),
                    comparison: lower_compare_op(*op),
                    right: ValueExpr::literal(i64::from(*value)),
                })))
            } else {
                None
            }
        }
        ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subj))
            if is_pronoun_anaphora(subj) && sel.controller == ControllerSelector::SourceItself =>
        {
            Some(StaticSourceCondition::SourceEnteredThisTurn)
        }
        ConditionAst::Marker(GameMarker::AttackedThisTurn { subject })
            if is_pronoun_anaphora(subject)
                && sel.controller == ControllerSelector::SourceItself =>
        {
            Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1_i64),
            })))
        }
        ConditionAst::MatchesFilter { subject, filter }
            if filter.controller.is_none()
                && filter.owner.is_none()
                && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && filter.extra_zones.is_empty()
                && !filter.any_spell =>
        {
            let mref = if sel.controller == ControllerSelector::SourceItself
                && (matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                    || is_pronoun_anaphora(subject))
            {
                MatchObjectRef::AbilitySource
            } else if sel.attached_to_source
                && (matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_))
                    || is_pronoun_anaphora(subject))
            {
                MatchObjectRef::AttachedHost
            } else {
                return None;
            };
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            let rc = ResolutionCondition {
                left: ValueExpr::object_matches(mref, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1_i64),
            };
            rc.is_world_evaluable()
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        ConditionAst::StatCompare {
            subject,
            stat,
            op,
            value,
        } if is_pronoun_anaphora(subject) => {
            let obj_ref = if sel.controller == ControllerSelector::SourceItself {
                ValueObjectRef::AbilitySource
            } else if sel.attached_to_source {
                ValueObjectRef::AttachedHost
            } else {
                return None;
            };
            Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::object_stat(obj_ref, lower_stat_kind(*stat)),
                comparison: lower_compare_op(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })))
        }
        ConditionAst::Compare { lhs, op, rhs }
            if matches!(
                lhs.as_ref(),
                ValueExprAst::ObjectStat { subject, .. }
                    | ValueExprAst::CounterCount { subject, .. }
                    if is_pronoun_anaphora(subject)
            ) || matches!(
                rhs.as_ref(),
                ValueExprAst::ObjectStat { subject, .. }
                    | ValueExprAst::CounterCount { subject, .. }
                    if is_pronoun_anaphora(subject)
            ) =>
        {
            let left = lower_selector_value_expr(sel, lhs, vocabulary)?;
            let right = lower_selector_value_expr(sel, rhs, vocabulary)?;
            let rc = ResolutionCondition {
                left,
                comparison: lower_compare_op(*op),
                right,
            };
            rc.is_world_evaluable()
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        other => lower_static_source_condition(other, vocabulary),
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
        ConditionAst::Marker(GameMarker::AdditionalCostPaid { description: None }) => {
            Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::AdditionalCostPaid),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1_i64),
            })))
        }
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
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyPlayer,
            min_amount,
        } if *min_amount >= 1 => Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::MaxPlayerLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }))),
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
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.any_spell
            && !filter.any_permanent
            && !filter.any_card
            && (filter.flags.0 & !AstFilterFlags::LKI_PAST_TENSE.0)
                == AstFilterFlags::EXCLUDE_SELF.0
            && filter_only_has(
                filter,
                FilterAllow {
                    flags: true,
                    ..FilterAllow::NONE
                },
            ) =>
        {
            Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OtherSpellsCastThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1_i64),
            })))
        }
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
                _ => {
                    if let Some((creatures_only, kw_raw)) = low
                        .strip_prefix("a card exiled with ~ has ")
                        .or_else(|| low.strip_prefix("a card exiled with it has "))
                        .or_else(|| low.strip_prefix("as long as a card exiled with ~ has "))
                        .or_else(|| low.strip_prefix("as long as a card exiled with it has "))
                        .map(|kw| (false, kw))
                        .or_else(|| {
                            low.strip_prefix("a creature card exiled with ~ has ")
                                .or_else(|| low.strip_prefix("a creature card exiled with it has "))
                                .or_else(|| {
                                    low.strip_prefix(
                                        "as long as a creature card exiled with ~ has ",
                                    )
                                })
                                .or_else(|| {
                                    low.strip_prefix(
                                        "as long as a creature card exiled with it has ",
                                    )
                                })
                                .map(|kw| (true, kw))
                        })
                        && let Some(kw) =
                            lower_keyword_ability_name(kw_raw.trim().trim_end_matches('.'))
                    {
                        let mut clause = ObjectClause::any();
                        if creatures_only {
                            clause.any_types = CardTypeMask::CREATURE;
                        }
                        clause.keyword = Some(kw);
                        let rc = ResolutionCondition {
                            left: ValueExpr::Count {
                                zone: ValueZone::LinkedExile,
                                filter: Box::new(ObjectFilter::Clause(clause)),
                                controller_scope: None,
                            },
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1_i64),
                        };
                        if rc.is_world_evaluable() && rc.validate_world(vocabulary).is_ok() {
                            return Some(StaticSourceCondition::Eval(Box::new(rc)));
                        }
                    }
                    if let Some(rest) = low
                        .strip_prefix("there is exactly ")
                        .or_else(|| low.strip_prefix("there are exactly "))
                        && let Some(before_on) = rest.strip_suffix(" on ~")
                        && let Some(before_cnt) = before_on
                            .strip_suffix(" counters")
                            .or_else(|| before_on.strip_suffix(" counter"))
                        && let Some((n_tok, kind_raw)) = before_cnt.split_once(' ')
                    {
                        let count = match n_tok {
                            "zero" => Some(0_i64),
                            "one" => Some(1_i64),
                            "two" => Some(2_i64),
                            "three" => Some(3_i64),
                            "four" => Some(4_i64),
                            "five" => Some(5_i64),
                            _ => n_tok.parse::<i64>().ok().filter(|&n| n >= 0),
                        };
                        if let Some(n) = count
                            && let Some(kind) =
                                parse_counter_kind(kind_raw.trim()).filter(|k| k.is_permanent())
                        {
                            return Some(StaticSourceCondition::Eval(Box::new(
                                ResolutionCondition {
                                    left: ValueExpr::counters(
                                        spellwake_engine::program::CounterSubjectRef::AbilitySource,
                                        Some(kind),
                                    ),
                                    comparison: Comparison::Equal,
                                    right: ValueExpr::literal(n),
                                },
                            )));
                        }
                    }
                    None
                }
            }
        }
        ConditionAst::IsYourTurn | ConditionAst::IsPlayersTurn(PlayerRefAst::You) => {
            Some(StaticSourceCondition::ControllersTurn)
        }
        ConditionAst::IsPlayersTurn(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent) => {
            Some(StaticSourceCondition::OpponentsTurn)
        }
        ConditionAst::Marker(GameMarker::TopCardOfZoneMatches {
            zone: ZoneFilter::YourLibrary,
            filter,
        }) => {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            let rc = ResolutionCondition {
                left: ValueExpr::object_matches(
                    MatchObjectRef::LibraryTop(ResolvingPlayer::Controller),
                    obj,
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1_i64),
            };
            rc.is_world_evaluable()
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        ConditionAst::Marker(GameMarker::MostCommonPermanentColor { color, or_tied }) => {
            let color = single_color_from_mask(*color)?;
            Some(StaticSourceCondition::MostCommonPermanentColor {
                color,
                or_tied: *or_tied,
            })
        }
        ConditionAst::Marker(GameMarker::CastFilteredSpellsThisTurn {
            player,
            filter,
            op,
            value,
        }) => lower_cast_filtered_spells_static_cond(*player, filter, *op, *value, vocabulary),
        ConditionAst::ZoneChangeThisTurn {
            filter,
            from_zone,
            to_zone,
            controller,
            min_count,
        } => lower_zone_change_this_turn_static_cond(
            filter,
            *from_zone,
            *to_zone,
            *controller,
            *min_count,
            vocabulary,
        ),
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
        ConditionAst::Marker(GameMarker::AttackedThisTurn { subject })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1_i64),
            })))
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            match status.trim().to_ascii_lowercase().as_str() {
                "tapped" => Some(StaticSourceCondition::SourceIsTapped),
                "untapped" => Some(StaticSourceCondition::SourceIsUntapped),
                "equipped" => Some(StaticSourceCondition::SourceIsEquipped),
                "enchanted" => Some(StaticSourceCondition::SourceIsEnchanted),
                "modified" => Some(StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::object_matches(
                        MatchObjectRef::AbilitySource,
                        ObjectFilter::any().with_flags(ObjectFilterFlags::MODIFIED),
                    ),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1_i64),
                }))),
                "attached:a creature" => Some(StaticSourceCondition::AttachedHostMatches(
                    Box::new(PermanentSelector {
                        required_types: BTreeSet::from([CardType::Creature]),
                        ..PermanentSelector::default()
                    }),
                )),
                _ => None,
            }
        }
        ConditionAst::HasKeyword { subject, keyword }
            if !matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let kw = lower_keyword_ability_name(keyword.trim())?;
            let obj_opt = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f)
                    if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                        && matches!(f.zone, None | Some(ZoneFilter::Exile)) =>
                {
                    let mut cleaned = f.as_ref().clone();
                    cleaned.zone = None;
                    cleaned.flags =
                        AstFilterFlags(cleaned.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
                    let mut clause = if is_unrestricted_filter(&cleaned) {
                        ObjectClause::any()
                    } else {
                        match lower_object_filter(&cleaned, vocabulary).ok()? {
                            ObjectFilter::Clause(c) => c,
                            ObjectFilter::Either(_, _) => return None,
                        }
                    };
                    if clause.keyword.is_some() && clause.keyword != Some(kw) {
                        return None;
                    }
                    clause.keyword = Some(kw);
                    Some(ObjectFilter::Clause(clause))
                }
                SubjectRefAst::Anaphora(raw) => {
                    let norm = raw.trim().to_ascii_lowercase();
                    let creatures_only = match norm.as_str() {
                        "a card exiled with ~" | "a card exiled with it" => Some(false),
                        "a creature card exiled with ~" | "a creature card exiled with it" => {
                            Some(true)
                        }
                        _ => None,
                    }?;
                    let mut clause = ObjectClause::any();
                    if creatures_only {
                        clause.any_types = CardTypeMask::CREATURE;
                    }
                    clause.keyword = Some(kw);
                    Some(ObjectFilter::Clause(clause))
                }
                _ => None,
            };
            let obj = obj_opt?;
            let rc = ResolutionCondition {
                left: ValueExpr::Count {
                    zone: ValueZone::LinkedExile,
                    filter: Box::new(obj),
                    controller_scope: None,
                },
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1_i64),
            };
            (rc.is_world_evaluable() && rc.validate_world(vocabulary).is_ok())
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        ConditionAst::ZoneObjectCount { filter, op, value }
            if (filter.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                && matches!(filter.zone, None | Some(ZoneFilter::Exile)) =>
        {
            let mut cleaned = filter.as_ref().clone();
            cleaned.zone = None;
            cleaned.flags = AstFilterFlags(cleaned.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&cleaned, vocabulary).ok()?
            };
            let comparison = match op {
                CompareOp::Ge => Comparison::GreaterOrEqual,
                CompareOp::Gt => Comparison::Greater,
                CompareOp::Le => Comparison::LessOrEqual,
                CompareOp::Lt => Comparison::Less,
                CompareOp::Eq => Comparison::Equal,
            };
            let rc = ResolutionCondition {
                left: ValueExpr::Count {
                    zone: ValueZone::LinkedExile,
                    filter: Box::new(obj),
                    controller_scope: None,
                },
                comparison,
                right: ValueExpr::literal(i64::from(*value)),
            };
            (rc.is_world_evaluable() && rc.validate_world(vocabulary).is_ok())
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) =>
        {
            if let Ok(host_sel) = lower_filter_to_permanent_selector(filter, vocabulary) {
                Some(StaticSourceCondition::AttachedHostMatches(Box::new(
                    host_sel,
                )))
            } else if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && filter.extra_zones.is_empty()
                && !filter.any_spell
            {
                let obj = lower_object_filter(filter, vocabulary).ok()?;
                let rc = ResolutionCondition {
                    left: ValueExpr::object_matches(MatchObjectRef::AttachedHost, obj),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1_i64),
                };
                rc.is_world_evaluable()
                    .then(|| StaticSourceCondition::Eval(Box::new(rc)))
            } else {
                None
            }
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && filter.extra_zones.is_empty()
                && !filter.any_spell =>
        {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            let rc = ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::AbilitySource, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1_i64),
            };
            rc.is_world_evaluable()
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a)
                    if matches!(
                        a.trim().to_ascii_lowercase().as_str(),
                        "the top card of your library" | "top card of your library"
                    )
            ) && filter.zone.is_none()
                && filter.extra_zones.is_empty()
                && !filter.any_spell =>
        {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            let rc = ResolutionCondition {
                left: ValueExpr::object_matches(
                    MatchObjectRef::LibraryTop(ResolvingPlayer::Controller),
                    obj,
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1_i64),
            };
            rc.is_world_evaluable()
                .then(|| StaticSourceCondition::Eval(Box::new(rc)))
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: op @ (CompareOp::Ge | CompareOp::Gt),
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let min_val = if *op == CompareOp::Gt {
                value.saturating_add(1)
            } else {
                *value
            };
            if min_val == 0 {
                return None;
            }
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            if matches!(ck_low.as_str(), "" | "any" | "a" | "one or more") {
                Some(StaticSourceCondition::SourceAnyCounterAtLeast(min_val))
            } else {
                let kind = parse_counter_kind(&ck_low).filter(|k| k.is_permanent())?;
                Some(StaticSourceCondition::SourceCounterAtLeast {
                    kind,
                    minimum: min_val,
                })
            }
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value,
        } if *value > 0 => {
            let f_sel = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            Some(StaticSourceCondition::ControlsExactCount {
                selector: Box::new(f_sel),
                count: *value,
            })
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
        ConditionAst::ZoneObjectCount { filter, op, value } => {
            if *op == CompareOp::Ge
                && filter.zone == Some(ZoneFilter::YourGraveyard)
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                let types = lower_card_type_mask(filter.any_types).ok()?;
                return Some(StaticSourceCondition::GraveyardCountAtLeast {
                    types,
                    minimum: *value,
                });
            }
            lower_zone_object_count_static_eval(filter, *op, *value, vocabulary)
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
            if let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0]) {
                Some(StaticSourceCondition::ControllerControlsSubtype(ct))
            } else if let Ok(selector) = lower_filter_to_permanent_selector(filter, vocabulary) {
                Some(StaticSourceCondition::ControlsMatching {
                    selector: Box::new(selector),
                    minimum: 1,
                })
            } else {
                None
            }
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
            if let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0]) {
                Some(StaticSourceCondition::OpponentControlsSubtype(ct))
            } else if let Ok(selector) = lower_filter_to_permanent_selector(filter, vocabulary) {
                Some(StaticSourceCondition::OpponentControlsMatching {
                    selector: Box::new(selector),
                    minimum: 1,
                })
            } else {
                None
            }
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

fn lower_zone_object_count_static_eval(
    filter: &ObjectFilterAst,
    op: CompareOp,
    value: u16,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticSourceCondition> {
    if !filter.extra_zones.is_empty() {
        return None;
    }
    if let Some(
        opp_zone @ (ZoneFilter::OpponentGraveyard
        | ZoneFilter::OpponentHand
        | ZoneFilter::OpponentLibrary),
    ) = filter.zone
    {
        if filter.controller.is_some() || filter.owner.is_some() {
            return None;
        }
        let mut inner = filter.clone();
        inner.zone = None;
        let obj = if is_unrestricted_filter(&inner) {
            ObjectFilter::any()
        } else {
            lower_object_filter(&inner, vocabulary).ok()?
        };
        let value_zone = match opp_zone {
            ZoneFilter::OpponentGraveyard => ValueZone::Graveyard(ResolvingPlayer::Controller),
            ZoneFilter::OpponentHand => ValueZone::Hand(ResolvingPlayer::Controller),
            ZoneFilter::OpponentLibrary => ValueZone::Library(ResolvingPlayer::Controller),
            _ => return None,
        };
        let left = match op {
            CompareOp::Ge | CompareOp::Gt => ValueExpr::OpponentMaxCount {
                zone: value_zone,
                filter: Box::new(obj),
            },
            CompareOp::Le | CompareOp::Lt => ValueExpr::OpponentMinCount {
                zone: value_zone,
                filter: Box::new(obj),
            },
            CompareOp::Eq if value == 0 => ValueExpr::OpponentMinCount {
                zone: value_zone,
                filter: Box::new(obj),
            },
            CompareOp::Eq => return None,
        };
        let rc = ResolutionCondition {
            left,
            comparison: lower_compare_op(op),
            right: ValueExpr::literal(i64::from(value)),
        };
        return rc
            .is_world_evaluable()
            .then(|| StaticSourceCondition::Eval(Box::new(rc)));
    }
    let left = lower_value_expr(&ValueExprAst::Count(Box::new(filter.clone())), vocabulary).ok()?;
    let rc = ResolutionCondition {
        left,
        comparison: lower_compare_op(op),
        right: ValueExpr::literal(i64::from(value)),
    };
    rc.is_world_evaluable()
        .then(|| StaticSourceCondition::Eval(Box::new(rc)))
}

fn lower_cast_filtered_spells_static_cond(
    player: PlayerRefAst,
    filter: &ObjectFilterAst,
    op: CompareOp,
    value: u16,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticSourceCondition> {
    if filter.any_permanent
        || filter.is_ability
        || filter.named.is_some()
        || !filter.extra_zones.is_empty()
        || filter.owner.is_some()
        || !filter.predicates.is_empty()
    {
        return None;
    }
    let clean_flags =
        filter.flags.0 & !(AstFilterFlags::ENTERED_THIS_TURN.0 | AstFilterFlags::LKI_PAST_TENSE.0);
    if clean_flags != 0 || (filter.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0 {
        return None;
    }
    let (players, effective_op, effective_val) = match player {
        PlayerRefAst::You => {
            if filter.controller.is_some_and(|c| {
                !matches!(c, ControllerRelation::You | ControllerRelation::YourTeam)
            }) {
                return None;
            }
            (AffectedPlayers::One(ResolvingPlayer::Controller), op, value)
        }
        PlayerRefAst::ActivePlayer => {
            if filter
                .controller
                .is_some_and(|c| c != ControllerRelation::ActivePlayer)
            {
                return None;
            }
            (
                AffectedPlayers::One(ResolvingPlayer::ActivePlayer),
                op,
                value,
            )
        }
        PlayerRefAst::EnchantedPlayer => {
            if filter.controller.is_some() {
                return None;
            }
            (
                AffectedPlayers::One(ResolvingPlayer::EnchantedPlayer),
                op,
                value,
            )
        }
        PlayerRefAst::AnyOpponent
            if (op == CompareOp::Ge && value == 1) || (op == CompareOp::Gt && value == 0) =>
        {
            if filter.controller.is_some_and(|c| {
                !matches!(
                    c,
                    ControllerRelation::Opponent | ControllerRelation::Opponents
                )
            }) {
                return None;
            }
            (AffectedPlayers::EachOpponent, CompareOp::Ge, 1)
        }
        PlayerRefAst::EachOpponent
            if (op == CompareOp::Eq && value == 0)
                || (op == CompareOp::Le && value == 0)
                || (op == CompareOp::Lt && value == 1) =>
        {
            if filter.controller.is_some_and(|c| {
                !matches!(
                    c,
                    ControllerRelation::Opponent | ControllerRelation::Opponents
                )
            }) {
                return None;
            }
            (AffectedPlayers::EachOpponent, CompareOp::Eq, 0)
        }
        PlayerRefAst::AnyPlayer
            if (op == CompareOp::Ge && value == 1) || (op == CompareOp::Gt && value == 0) =>
        {
            if filter.controller.is_some() {
                return None;
            }
            (AffectedPlayers::EachPlayer, CompareOp::Ge, 1)
        }
        PlayerRefAst::EachPlayer
            if (op == CompareOp::Eq && value == 0)
                || (op == CompareOp::Le && value == 0)
                || (op == CompareOp::Lt && value == 1) =>
        {
            if filter.controller.is_some() {
                return None;
            }
            (AffectedPlayers::EachPlayer, CompareOp::Eq, 0)
        }
        _ => return None,
    };
    let from_zone = match filter.zone {
        None | Some(ZoneFilter::Stack) => None,
        Some(
            ZoneFilter::YourHand
            | ZoneFilter::ThatPlayerHand
            | ZoneFilter::TargetPlayerHand
            | ZoneFilter::TargetOpponentHand
            | ZoneFilter::OpponentHand
            | ZoneFilter::OpponentsHands
            | ZoneFilter::EachPlayerHand
            | ZoneFilter::AnyHand,
        ) => Some(SourceZoneFilter::Hand),
        Some(
            ZoneFilter::YourGraveyard
            | ZoneFilter::ThatPlayerGraveyard
            | ZoneFilter::TargetPlayerGraveyard
            | ZoneFilter::TargetOpponentGraveyard
            | ZoneFilter::OpponentGraveyard
            | ZoneFilter::OpponentsGraveyards
            | ZoneFilter::AnyGraveyard
            | ZoneFilter::AllGraveyards,
        ) => Some(SourceZoneFilter::Graveyard),
        Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
        _ => return None,
    };
    let mut cleaned = filter.clone();
    cleaned.controller = None;
    cleaned.zone = None;
    cleaned.flags = AstFilterFlags::EMPTY;
    let obj = if is_unrestricted_filter(&cleaned) {
        ObjectFilter::any()
    } else {
        let lowered = lower_object_filter(&cleaned, vocabulary).ok()?;
        if !turn_record_filter_supported(lowered, false) {
            return None;
        }
        lowered
    };
    let rc = ResolutionCondition {
        left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
            players,
            filter: Box::new(obj),
            from_zone,
        }),
        comparison: lower_compare_op(effective_op),
        right: ValueExpr::literal(i64::from(effective_val)),
    };
    rc.is_world_evaluable()
        .then(|| StaticSourceCondition::Eval(Box::new(rc)))
}

fn lower_zone_change_this_turn_static_cond(
    filter: &ObjectFilterAst,
    from_zone: Option<ZoneFilter>,
    to_zone: Option<ZoneFilter>,
    controller: Option<PlayerRefAst>,
    min_count: u16,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticSourceCondition> {
    use spellwake_engine::program::OpponentTurnStat;
    if min_count == 0
        || !filter.extra_zones.is_empty()
        || filter.owner.is_some()
        || filter.is_ability
        || filter.named.is_some()
        || !filter.predicates.is_empty()
        || filter.any_spell
    {
        return None;
    }

    let ge_eval = |stat: PlayerStatExpr, min: u16| -> Option<StaticSourceCondition> {
        let rc = ResolutionCondition {
            left: ValueExpr::PlayerStat(stat),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(min)),
        };
        rc.is_world_evaluable()
            .then(|| StaticSourceCondition::Eval(Box::new(rc)))
    };

    // 1. Graveyard arrivals / dies this turn
    if let Some(
        gy_zone @ (ZoneFilter::AnyGraveyard
        | ZoneFilter::AllGraveyards
        | ZoneFilter::SingleGraveyard
        | ZoneFilter::YourGraveyard),
    ) = to_zone
    {
        let from_battlefield_only = match from_zone {
            None => false,
            Some(ZoneFilter::Battlefield) => true,
            Some(_) => return None,
        };
        if gy_zone == ZoneFilter::SingleGraveyard && !from_battlefield_only {
            return None;
        }
        let extra_flags = filter.flags.0
            & !(AstFilterFlags::DIED_THIS_TURN.0
                | AstFilterFlags::ENTERED_THIS_TURN.0
                | AstFilterFlags::LKI_PAST_TENSE.0);
        if extra_flags != 0 || (filter.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0 {
            return None;
        }
        let mut cleaned = filter.clone();
        cleaned.zone = None;
        cleaned.flags = AstFilterFlags::EMPTY;
        let (ctrl_role, allow_clause_ctrl) = match (controller, gy_zone, filter.controller) {
            (None, ZoneFilter::YourGraveyard, None) => {
                cleaned.controller = Some(ControllerRelation::OwnerYou);
                (None, true)
            }
            (None, ZoneFilter::YourGraveyard, Some(ControllerRelation::You))
            | (
                Some(PlayerRefAst::You),
                ZoneFilter::YourGraveyard,
                None | Some(ControllerRelation::You),
            ) => {
                cleaned.controller = Some(ControllerRelation::OwnerYou);
                (Some(ResolvingPlayer::Controller), true)
            }
            (
                Some(PlayerRefAst::You),
                ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
                None | Some(ControllerRelation::You),
            )
            | (
                None,
                ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
                Some(ControllerRelation::You),
            ) => {
                cleaned.controller = None;
                (Some(ResolvingPlayer::Controller), false)
            }
            (
                None,
                ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
                None,
            ) => (None, false),
            (
                Some(PlayerRefAst::AnyOpponent),
                ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
                None | Some(ControllerRelation::Opponent | ControllerRelation::Opponents),
            )
            | (
                None,
                ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard,
                Some(ControllerRelation::Opponent | ControllerRelation::Opponents),
            ) if from_battlefield_only && min_count == 1 => {
                cleaned.controller = Some(ControllerRelation::Opponents);
                (None, true)
            }
            _ => return None,
        };
        if from_battlefield_only
            && !allow_clause_ctrl
            && cleaned.any_types == AstCardTypeMask::CREATURE
            && filter_only_has(
                &cleaned,
                FilterAllow {
                    types: true,
                    ..FilterAllow::NONE
                },
            )
        {
            return match ctrl_role {
                None => ge_eval(PlayerStatExpr::CreaturesDiedThisTurn, min_count),
                Some(ResolvingPlayer::Controller) => {
                    ge_eval(PlayerStatExpr::ControlledCreatureDiedThisTurn, min_count)
                }
                _ => None,
            };
        }
        let obj = if !allow_clause_ctrl && is_unrestricted_filter(&cleaned) {
            ObjectFilter::any()
        } else {
            let lowered = lower_object_filter(&cleaned, vocabulary).ok()?;
            let mut check_filter = lowered;
            if allow_clause_ctrl {
                let (mut a, b) = check_filter.clauses();
                a.controller = ControllerFilter::Any;
                check_filter = match b {
                    Some(mut c2) => {
                        c2.controller = ControllerFilter::Any;
                        ObjectFilter::either(a, c2)
                    }
                    None => ObjectFilter::Clause(a),
                };
            }
            if !turn_record_filter_supported(check_filter, true) {
                return None;
            }
            lowered
        };
        return ge_eval(
            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                filter: Box::new(obj),
                from_battlefield_only,
                controller: ctrl_role,
            },
            min_count,
        );
    }

    // 2. Entered battlefield this turn
    if from_zone.is_none() && to_zone == Some(ZoneFilter::Battlefield) {
        let extra_flags = filter.flags.0
            & !(AstFilterFlags::ENTERED_THIS_TURN.0 | AstFilterFlags::LKI_PAST_TENSE.0);
        if extra_flags != 0 || filter.zone.is_some() {
            return None;
        }
        let eff_ctrl = match (controller, filter.controller) {
            (Some(PlayerRefAst::You), None | Some(ControllerRelation::You))
            | (None, Some(ControllerRelation::You)) => Some(PlayerRefAst::You),
            (
                Some(PlayerRefAst::AnyOpponent),
                None | Some(ControllerRelation::Opponent | ControllerRelation::Opponents),
            )
            | (None, Some(ControllerRelation::Opponent | ControllerRelation::Opponents)) => {
                Some(PlayerRefAst::AnyOpponent)
            }
            _ => None,
        }?;
        match eff_ctrl {
            PlayerRefAst::You => {
                if filter.any_types == AstCardTypeMask::CREATURE
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            types: true,
                            controller: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    return if min_count == 1 {
                        Some(StaticSourceCondition::CreatureEnteredUnderControlThisTurn)
                    } else {
                        ge_eval(
                            PlayerStatExpr::CreatureEnteredUnderControlThisTurn,
                            min_count,
                        )
                    };
                }
                if filter.any_types == AstCardTypeMask::LAND
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            types: true,
                            controller: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    return ge_eval(
                        PlayerStatExpr::LandsEnteredThisTurn(ResolvingPlayer::Controller),
                        min_count,
                    );
                }
                if filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        controller: true,
                        flags: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    if filter.any_types.is_empty() {
                        return ge_eval(PlayerStatExpr::EnteredPermanentThisTurn, min_count);
                    }
                    let mut types = card_type_mask_to_set(filter.any_types).ok()?;
                    if types.len() == 1
                        && let Some(kind) = types.pop_first()
                    {
                        return ge_eval(PlayerStatExpr::EnteredTypeThisTurn(kind), min_count);
                    }
                }
                if filter.any_types.is_empty()
                    && filter.forbidden_types == AstCardTypeMask::LAND
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            forbidden_types: true,
                            controller: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    return Some(
                        StaticSourceCondition::NonlandPermanentsEnteredThisTurnAtLeast(min_count),
                    );
                }
                return None;
            }
            PlayerRefAst::AnyOpponent if min_count == 1 => {
                if filter.any_types == AstCardTypeMask::CREATURE
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            types: true,
                            controller: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    return ge_eval(
                        PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::CreatureEntered),
                        1,
                    );
                }
                return None;
            }
            _ => return None,
        }
    }

    // 3. Left battlefield this turn
    if from_zone == Some(ZoneFilter::Battlefield) && to_zone.is_none() {
        let extra_flags = filter.flags.0 & !AstFilterFlags::LKI_PAST_TENSE.0;
        if extra_flags != 0 || filter.zone.is_some() {
            return None;
        }
        let eff_ctrl = match (controller, filter.controller) {
            (Some(PlayerRefAst::You), None | Some(ControllerRelation::You))
            | (None, Some(ControllerRelation::You)) => Some(PlayerRefAst::You),
            (
                Some(PlayerRefAst::AnyOpponent),
                None | Some(ControllerRelation::Opponent | ControllerRelation::Opponents),
            )
            | (None, Some(ControllerRelation::Opponent | ControllerRelation::Opponents)) => {
                Some(PlayerRefAst::AnyOpponent)
            }
            _ => None,
        }?;
        match eff_ctrl {
            PlayerRefAst::You => {
                if filter_only_has(
                    filter,
                    FilterAllow {
                        controller: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    return if min_count == 1 {
                        Some(StaticSourceCondition::PermanentLeftBattlefieldThisTurn)
                    } else {
                        ge_eval(PlayerStatExpr::PermanentLeftBattlefieldThisTurn, min_count)
                    };
                }
                if filter.any_types.is_empty()
                    && filter.forbidden_types == AstCardTypeMask::LAND
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            forbidden_types: true,
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    return ge_eval(
                        PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn,
                        min_count,
                    );
                }
                return None;
            }
            PlayerRefAst::AnyOpponent if min_count == 1 => {
                if filter_only_has(
                    filter,
                    FilterAllow {
                        controller: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    return ge_eval(
                        PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::PermanentLeft),
                        1,
                    );
                }
                return None;
            }
            _ => return None,
        }
    }

    // 4. Left your graveyard this turn
    if from_zone == Some(ZoneFilter::YourGraveyard)
        && to_zone.is_none()
        && matches!(controller, None | Some(PlayerRefAst::You))
        && is_unrestricted_filter(filter)
    {
        return ge_eval(PlayerStatExpr::CardsLeftGraveyardThisTurn, min_count);
    }

    // 5. Put into exile this turn
    if from_zone.is_none()
        && to_zone == Some(ZoneFilter::Exile)
        && controller.is_none()
        && is_unrestricted_filter(filter)
    {
        return ge_eval(PlayerStatExpr::CardsExiledThisTurn, min_count);
    }

    None
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

fn set_clause_source_relation(
    slot: &mut SourceRelation,
    rel: SourceRelation,
) -> Result<(), &'static str> {
    match (*slot, rel) {
        (SourceRelation::Any, r) => {
            *slot = r;
            Ok(())
        }
        (a, b) if a == b => Ok(()),
        (SourceRelation::OtherThanAbilitySource, SourceRelation::SharingNameWithAbilitySource)
        | (SourceRelation::SharingNameWithAbilitySource, SourceRelation::OtherThanAbilitySource)
        | (
            SourceRelation::OtherThanAbilitySourceSharingName,
            SourceRelation::OtherThanAbilitySource | SourceRelation::SharingNameWithAbilitySource,
        )
        | (
            SourceRelation::OtherThanAbilitySource | SourceRelation::SharingNameWithAbilitySource,
            SourceRelation::OtherThanAbilitySourceSharingName,
        ) => {
            *slot = SourceRelation::OtherThanAbilitySourceSharingName;
            Ok(())
        }
        _ => Err("oracle:unsupported-object-filter"),
    }
}

fn set_clause_combat_filter(
    slot: &mut CombatFilter,
    combat: CombatFilter,
) -> Result<(), &'static str> {
    if *slot != CombatFilter::Any && *slot != combat {
        return Err("oracle:unsupported-filter-predicate");
    }
    *slot = combat;
    Ok(())
}

fn filter_represents_self_object(f: &ObjectFilterAst) -> bool {
    use spellwake_oracle_compiler::ast::NameFilterAst;
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
        owner,
        zone,
        extra_zones,
        combat,
        tapped,
        token,
        required_keywords,
        forbidden_keywords,
        stat_bounds,
        counter_kind,
        flags,
        predicates,
        any_permanent,
        any_card,
        any_spell,
        is_ability,
        named,
    } = f;
    matches!(named, Some(NameFilterAst::SelfName))
        && any_types.is_empty()
        && forbidden_types.is_empty()
        && required_supertypes.is_empty()
        && forbidden_supertypes.is_empty()
        && subtypes.is_empty()
        && forbidden_subtypes.is_empty()
        && any_colors.is_empty()
        && forbidden_colors.is_empty()
        && color_arity.is_none()
        && controller.is_none()
        && owner.is_none()
        && zone.is_none()
        && extra_zones.is_empty()
        && combat.is_none()
        && tapped.is_none()
        && token.is_none()
        && required_keywords.is_empty()
        && forbidden_keywords.is_empty()
        && stat_bounds.is_empty()
        && counter_kind.is_none()
        && (flags.0 & !AstFilterFlags::LKI_PAST_TENSE.0) == 0
        && predicates.is_empty()
        && !*any_permanent
        && !*any_card
        && !*any_spell
        && !*is_ability
}

fn intersect_object_clauses(
    base: ObjectClause,
    extra: ObjectClause,
) -> Result<ObjectClause, &'static str> {
    let mut required_types = base.required_types.union(extra.required_types);
    let forbidden_types = base.forbidden_types.union(extra.forbidden_types);
    let any_types = match (base.any_types, extra.any_types) {
        (a, b) if a.is_empty() => b,
        (a, b) if b.is_empty() => a,
        (a, b) if a == b => a,
        (a, b) if a.bits().count_ones() == 1 => {
            required_types = required_types.union(a);
            b
        }
        (a, b) if b.bits().count_ones() == 1 => {
            required_types = required_types.union(b);
            a
        }
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    if (required_types.bits() & forbidden_types.bits()) != 0 {
        return Err("oracle:unsupported-filter-predicate");
    }
    let required_supertypes = base.required_supertypes.union(extra.required_supertypes);
    let forbidden_supertypes = base.forbidden_supertypes.union(extra.forbidden_supertypes);
    let subtypes = if base.subtypes == SubtypeFilter::Any {
        extra.subtypes
    } else if extra.subtypes == SubtypeFilter::Any || base.subtypes == extra.subtypes {
        base.subtypes
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let controller = if base.controller == ControllerFilter::Any {
        extra.controller
    } else if extra.controller == ControllerFilter::Any || base.controller == extra.controller {
        base.controller
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let owner = if base.owner == ControllerFilter::Any {
        extra.owner
    } else if extra.owner == ControllerFilter::Any || base.owner == extra.owner {
        base.owner
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let mut source_relation = base.source_relation;
    if extra.source_relation != SourceRelation::Any {
        set_clause_source_relation(&mut source_relation, extra.source_relation)
            .map_err(|_| "oracle:unsupported-filter-predicate")?;
    }
    let color = match (base.color, extra.color) {
        (ObjectColorFilter::Any, c) | (c, ObjectColorFilter::Any) => c,
        (a, b) if a == b => a,
        (
            ObjectColorFilter::Mask {
                any_of: a_any,
                none_of: a_none,
            },
            ObjectColorFilter::Mask {
                any_of: b_any,
                none_of: b_none,
            },
        ) if a_any.is_empty() || b_any.is_empty() || a_any == b_any => {
            let any_of = if a_any.is_empty() { b_any } else { a_any };
            let none_of = a_none.union(b_none);
            if !any_of.is_empty()
                && !Color::ALL
                    .iter()
                    .any(|&c| any_of.contains(c) && !none_of.contains(c))
            {
                return Err("oracle:unsupported-filter-predicate");
            }
            ObjectColorFilter::Mask { any_of, none_of }
        }
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    let combat = if base.combat == CombatFilter::Any {
        extra.combat
    } else if extra.combat == CombatFilter::Any || base.combat == extra.combat {
        base.combat
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let power = if base.power == PowerFilter::Any {
        extra.power
    } else if extra.power == PowerFilter::Any || base.power == extra.power {
        base.power
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let toughness = if base.toughness == ToughnessFilter::Any {
        extra.toughness
    } else if extra.toughness == ToughnessFilter::Any || base.toughness == extra.toughness {
        base.toughness
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let mana_value = if base.mana_value == ManaValueFilter::Any {
        extra.mana_value
    } else if extra.mana_value == ManaValueFilter::Any || base.mana_value == extra.mana_value {
        base.mana_value
    } else {
        return Err("oracle:unsupported-filter-predicate");
    };
    let keyword = match (base.keyword, extra.keyword) {
        (None, k) | (k, None) => k,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    let forbidden_keyword = match (base.forbidden_keyword, extra.forbidden_keyword) {
        (None, k) | (k, None) => k,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    if keyword.is_some() && keyword == forbidden_keyword {
        return Err("oracle:unsupported-filter-predicate");
    }
    let tapped = match (base.tapped, extra.tapped) {
        (None, t) | (t, None) => t,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    let token = match (base.token, extra.token) {
        (None, t) | (t, None) => t,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    let flags = base.flags.union(extra.flags);
    if flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
        && (flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER)
            || flags.contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
            || flags.contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER))
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    if flags.contains(ObjectFilterFlags::HISTORIC)
        && flags.contains(ObjectFilterFlags::NON_HISTORIC)
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    if flags.contains(ObjectFilterFlags::FACE_DOWN) && flags.contains(ObjectFilterFlags::FACE_UP) {
        return Err("oracle:unsupported-filter-predicate");
    }
    if (flags.contains(ObjectFilterFlags::EVEN_MANA_VALUE) || mana_value == ManaValueFilter::Even)
        && (flags.contains(ObjectFilterFlags::ODD_MANA_VALUE) || mana_value == ManaValueFilter::Odd)
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    if flags.contains(ObjectFilterFlags::HAS_ACTIVATED_ABILITY)
        && flags.contains(ObjectFilterFlags::NO_ACTIVATED_ABILITIES)
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    if (flags.contains(ObjectFilterFlags::SAME_NAME_AS_SOURCE)
        || matches!(
            source_relation,
            SourceRelation::SharingNameWithAbilitySource
                | SourceRelation::OtherThanAbilitySourceSharingName
        ))
        && flags.contains(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE)
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    let required_counter_kind = match (base.required_counter_kind, extra.required_counter_kind) {
        (None, k) | (k, None) => k,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    let forbidden_counter_kind = match (base.forbidden_counter_kind, extra.forbidden_counter_kind) {
        (None, k) | (k, None) => k,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    if required_counter_kind.is_some()
        && (required_counter_kind == forbidden_counter_kind
            || flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS))
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    if forbidden_counter_kind == Some(CounterKind::PlusOne)
        && flags.contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    if forbidden_counter_kind == Some(CounterKind::MinusOne)
        && flags.contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
    {
        return Err("oracle:unsupported-filter-predicate");
    }
    let exact_name = match (base.exact_name, extra.exact_name) {
        (None, n) | (n, None) => n,
        (Some(a), Some(b)) if a == b => Some(a),
        _ => return Err("oracle:unsupported-filter-predicate"),
    };
    Ok(ObjectClause {
        any_types,
        required_types,
        forbidden_types,
        required_supertypes,
        forbidden_supertypes,
        subtypes,
        controller,
        owner,
        source_relation,
        color,
        combat,
        power,
        toughness,
        mana_value,
        keyword,
        forbidden_keyword,
        tapped,
        token,
        flags,
        required_counter_kind,
        forbidden_counter_kind,
        exact_name,
    })
}

pub(super) fn lower_object_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    if f.is_ability || !f.extra_zones.is_empty() {
        return Err("oracle:unsupported-object-filter");
    }
    let mut clause = ObjectClause::any();
    if let Some(named) = &f.named {
        use spellwake_engine::program::ExactNameKey;
        use spellwake_oracle_compiler::ast::NameFilterAst;
        match named {
            NameFilterAst::SelfName => {
                set_clause_source_relation(
                    &mut clause.source_relation,
                    SourceRelation::SharingNameWithAbilitySource,
                )?;
            }
            NameFilterAst::Exact(name) => {
                let key = ExactNameKey::from_name(name.trim())
                    .ok_or("oracle:unsupported-object-filter")?;
                clause.exact_name = Some(key);
            }
            NameFilterAst::SameAs(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_SOURCE);
                }
                SubjectRefAst::Anaphora(raw)
                    if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                {
                    clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_SOURCE);
                }
                SubjectRefAst::TargetObject {
                    count: TargetCountAst::Exact(1),
                    ..
                } => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_TARGET);
                }
                SubjectRefAst::Anaphora(raw)
                    if raw.trim().to_ascii_lowercase().contains("target") =>
                {
                    clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_TARGET);
                }
                _ => return Err("oracle:unsupported-object-filter"),
            },
            NameFilterAst::SameAsReferenced => {
                return Err("oracle:unsupported-object-filter");
            }
        }
    }
    let types_match_all = f
        .predicates
        .iter()
        .any(|p| matches!(p, FilterPredicateAst::TypesMatchAll));
    if types_match_all {
        let req_types = lower_card_type_mask(f.any_types)?;
        clause.required_types = req_types;
        clause.any_types = canonical_any_type_for_required(req_types);
    } else {
        clause.any_types = lower_card_type_mask(f.any_types)?;
    }
    clause.forbidden_types = lower_card_type_mask(f.forbidden_types)?;
    if (clause.required_types.bits() & clause.forbidden_types.bits()) != 0 {
        return Err("oracle:unsupported-object-filter");
    }

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
        if clause.flags.contains(ObjectFilterFlags::HISTORIC) {
            return Err("oracle:forbidden-historic");
        }
        clause.flags = clause.flags.union(ObjectFilterFlags::NON_HISTORIC);
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

    // Controller and owner lowered independently
    let mut owner_from_controller = ControllerFilter::Any;
    clause.controller = match f.controller {
        None => ControllerFilter::Any,
        Some(ctrl) => match ctrl {
            ControllerRelation::You | ControllerRelation::YourTeam => ControllerFilter::You,
            ControllerRelation::Opponent | ControllerRelation::Opponents => {
                ControllerFilter::Opponent
            }
            ControllerRelation::NotYou => ControllerFilter::NotYou,
            ControllerRelation::OwnerYou => ControllerFilter::OwnerYou,
            ControllerRelation::OwnerOpponent => ControllerFilter::OwnerOpponent,
            ControllerRelation::OwnerThatPlayer if f.owner.is_none() => {
                owner_from_controller = ControllerFilter::RelatedPlayer;
                ControllerFilter::Any
            }
            ControllerRelation::ThatPlayer => ControllerFilter::RelatedPlayer,
            ControllerRelation::DefendingPlayer => ControllerFilter::DefendingPlayer,
            ControllerRelation::ActivePlayer => ControllerFilter::ActivePlayer,
            ControllerRelation::TargetPlayer => ControllerFilter::TargetPlayer,
            ControllerRelation::TargetOpponent => ControllerFilter::TargetOpponent,
            _ => return Err("oracle:unsupported-controller-filter"),
        },
    };
    clause.owner = match f.owner {
        None => owner_from_controller,
        Some(owner) => match owner {
            ControllerRelation::You
            | ControllerRelation::YourTeam
            | ControllerRelation::OwnerYou => ControllerFilter::You,
            ControllerRelation::Opponent
            | ControllerRelation::Opponents
            | ControllerRelation::OwnerOpponent => ControllerFilter::Opponent,
            ControllerRelation::NotYou => ControllerFilter::NotYou,
            ControllerRelation::ThatPlayer | ControllerRelation::OwnerThatPlayer => {
                ControllerFilter::RelatedPlayer
            }
            ControllerRelation::DefendingPlayer => ControllerFilter::DefendingPlayer,
            ControllerRelation::ActivePlayer => ControllerFilter::ActivePlayer,
            ControllerRelation::TargetPlayer => ControllerFilter::TargetPlayer,
            ControllerRelation::TargetOpponent => ControllerFilter::TargetOpponent,
            _ => return Err("oracle:unsupported-controller-filter"),
        },
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
    let keywords_match_any = (f.flags.0 & AstFilterFlags::KEYWORDS_MATCH_ANY.0) != 0;
    if f.forbidden_keywords.len() > 1 {
        return Err("oracle:multi-keyword-filter");
    }
    let mut keyword_pair: Option<(KeywordAbility, KeywordAbility)> = None;
    if f.required_keywords.len() == 2 && keywords_match_any {
        let kw_a = lower_keyword_ability_name(&f.required_keywords[0])
            .ok_or("oracle:filter-unknown-kw")?;
        let kw_b = lower_keyword_ability_name(&f.required_keywords[1])
            .ok_or("oracle:filter-unknown-kw")?;
        if kw_a == kw_b {
            clause.keyword = Some(kw_a);
        } else {
            keyword_pair = Some((kw_a, kw_b));
        }
    } else if f.required_keywords.len() > 1 {
        return Err("oracle:multi-keyword-filter");
    } else if let Some(k) = f.required_keywords.first() {
        if k.trim().eq_ignore_ascii_case("toxic") {
            clause.flags = clause.flags.union(ObjectFilterFlags::HAS_TOXIC);
        } else {
            clause.keyword = Some(lower_keyword_ability_name(k).ok_or("oracle:filter-unknown-kw")?);
        }
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
                ValueExprAst::CardsExiledThisWay => Some(A::LastMovedCount),
                ValueExprAst::ObjectStat { subject, stat } => {
                    match (peel_battlefield_subject(subject), stat) {
                        (SubjectRefAst::SelfObject, StatKind::Power) => Some(A::SourcePower),
                        (SubjectRefAst::SelfObject, StatKind::Toughness) => {
                            Some(A::SourceToughness)
                        }
                        (SubjectRefAst::SelfObject, StatKind::ManaValue) => {
                            Some(A::SourceManaValue)
                        }
                        (SubjectRefAst::Anaphora(raw), stat_kind)
                            if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                        {
                            match stat_kind {
                                StatKind::Power => Some(A::SourcePower),
                                StatKind::Toughness => Some(A::SourceToughness),
                                StatKind::ManaValue => Some(A::SourceManaValue),
                                StatKind::Loyalty => None,
                            }
                        }
                        (SubjectRefAst::TargetObject { .. }, StatKind::Power) => {
                            Some(A::TargetPower)
                        }
                        (SubjectRefAst::TargetObject { .. }, StatKind::Toughness) => {
                            Some(A::TargetToughness)
                        }
                        (SubjectRefAst::TargetObject { .. }, StatKind::ManaValue) => {
                            Some(A::TargetManaValue)
                        }
                        (SubjectRefAst::Anaphora(raw), stat_kind) => {
                            let low = raw.trim().to_ascii_lowercase();
                            if low.contains("target") {
                                match stat_kind {
                                    StatKind::Power => Some(A::TargetPower),
                                    StatKind::Toughness => Some(A::TargetToughness),
                                    StatKind::ManaValue => Some(A::TargetManaValue),
                                    StatKind::Loyalty => None,
                                }
                            } else if low.contains("sacrificed") {
                                match stat_kind {
                                    StatKind::Power => Some(A::LastSacrificedPower),
                                    StatKind::Toughness => Some(A::LastSacrificedToughness),
                                    StatKind::ManaValue => Some(A::LastSacrificedManaValue),
                                    StatKind::Loyalty => None,
                                }
                            } else if low.contains("discarded")
                                || low.contains("exiled")
                                || low.contains("revealed")
                                || low.contains("milled")
                                || low.contains("tapped")
                                || low.contains("ring-bearer")
                                || low.contains("chosen")
                                || low.contains("returned")
                                || low.contains("exploited")
                                || matches!(low.as_str(), "its" | "his" | "her" | "their")
                            {
                                None
                            } else {
                                match stat_kind {
                                    StatKind::Power => Some(A::TriggerSubjectPower),
                                    StatKind::Toughness => Some(A::TriggerSubjectToughness),
                                    StatKind::ManaValue => Some(A::TriggerSubjectManaValue),
                                    StatKind::Loyalty => None,
                                }
                            }
                        }
                        (SubjectRefAst::TriggerSubject, StatKind::Power) => {
                            Some(A::TriggerSubjectPower)
                        }
                        (SubjectRefAst::TriggerSubject, StatKind::Toughness) => {
                            Some(A::TriggerSubjectToughness)
                        }
                        (SubjectRefAst::TriggerSubject, StatKind::ManaValue) => {
                            Some(A::TriggerSubjectManaValue)
                        }
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
                    if is_unrestricted_last_moved_count_filter(inner) {
                        return Some(A::LastMovedCount);
                    }
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
            (
                StatKind::Power,
                CompareOp::Eq,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Toughness,
                },
            )
            | (
                StatKind::Toughness,
                CompareOp::Eq,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                },
            ) if matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "its" | "his" | "her" | "their"
                    )
            ) =>
            {
                clause.flags = clause.flags.union(ObjectFilterFlags::SAME_TOTAL_PT);
            }
            (
                StatKind::Toughness,
                CompareOp::Gt,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                },
            )
            | (
                StatKind::Power,
                CompareOp::Lt,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Toughness,
                },
            ) if matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "its" | "his" | "her" | "their"
                    )
            ) =>
            {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::TOUGHNESS_GREATER_THAN_POWER);
            }
            (
                StatKind::Power,
                CompareOp::Gt,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Toughness,
                },
            )
            | (
                StatKind::Toughness,
                CompareOp::Lt,
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                },
            ) if matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "its" | "his" | "her" | "their"
                    )
            ) =>
            {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::POWER_GREATER_THAN_TOUGHNESS);
            }
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
        set_clause_source_relation(
            &mut clause.source_relation,
            SourceRelation::OtherThanAbilitySource,
        )?;
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
    if f.flags.0 & AstFilterFlags::BLOCKED_THIS_TURN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::BLOCKED_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::PAIRED_WITH_SELF.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::PAIRED_WITH_SELF);
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
    if f.flags.0 & AstFilterFlags::IS_COMMANDER.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::COMMANDER_ONLY);
    }
    if f.flags.0 & AstFilterFlags::NOT_COMMANDER.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::NOT_COMMANDER);
    }
    let has_with_any_flag = (f.flags.0 & AstFilterFlags::WITH_ANY_COUNTER.0) != 0;
    let has_without_flag = (f.flags.0 & AstFilterFlags::WITHOUT_COUNTERS.0) != 0;
    if let Some(ck_raw) = f.counter_kind.as_deref() {
        if has_with_any_flag && has_without_flag {
            return Err("oracle:unsupported-object-filter");
        }
        let ck = ck_raw.trim().to_ascii_lowercase();
        let ck_trimmed = ck
            .trim_end_matches(" counters")
            .trim_end_matches(" counter")
            .trim();
        let ck_clean = ck_trimmed
            .strip_prefix("one or more ")
            .or_else(|| ck_trimmed.strip_prefix("any "))
            .or_else(|| ck_trimmed.strip_prefix("a "))
            .or_else(|| ck_trimmed.strip_prefix("an "))
            .unwrap_or(ck_trimmed)
            .trim();
        if has_without_flag {
            match ck_clean {
                "" | "any" | "a" | "one or more" | "none" | "no" => {
                    clause = clause.with_flags(ObjectFilterFlags::WITHOUT_COUNTERS);
                }
                other => {
                    let kind = parse_counter_kind(other)
                        .filter(|k| k.is_permanent())
                        .ok_or("oracle:unsupported-object-filter")?;
                    clause.forbidden_counter_kind = Some(kind);
                }
            }
        } else {
            match ck_clean {
                "+1/+1" => {
                    clause = clause.with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER);
                }
                "-1/-1" => {
                    clause = clause.with_flags(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER);
                }
                "" | "any" | "a" | "one or more" => {
                    clause = clause.with_flags(ObjectFilterFlags::WITH_ANY_COUNTER);
                }
                "none" | "no" => {
                    clause = clause.with_flags(ObjectFilterFlags::WITHOUT_COUNTERS);
                }
                other => {
                    let kind = parse_counter_kind(other)
                        .filter(|k| k.is_permanent())
                        .ok_or("oracle:unsupported-object-filter")?;
                    clause.required_counter_kind = Some(kind);
                }
            }
        }
    } else {
        if has_with_any_flag {
            clause = clause.with_flags(ObjectFilterFlags::WITH_ANY_COUNTER);
        }
        if has_without_flag {
            clause = clause.with_flags(ObjectFilterFlags::WITHOUT_COUNTERS);
        }
    }
    let has_face_down = (f.flags.0 & AstFilterFlags::FACE_DOWN.0) != 0;
    let has_face_up = (f.flags.0 & AstFilterFlags::FACE_UP.0) != 0;
    if has_face_down && has_face_up {
        return Err("oracle:unsupported-filter-flag");
    }
    if has_face_down {
        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_DOWN);
    }
    if has_face_up {
        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_UP);
    }
    if f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
    }
    if f.flags.0 & AstFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
    }
    if f.flags.0
        & (AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0 | AstFilterFlags::MATCHES_CHOSEN_COLOR.0)
        != 0
    {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR);
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_TYPE.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE);
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_CARD_TYPE.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::MATCHES_CHOSEN_CARD_TYPE);
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_NAME.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::MATCHES_CHOSEN_NAME);
    }
    if f.flags.0 & AstFilterFlags::DID_NOT_ATTACK_THIS_TURN.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::ATTACHED_TO_SELF.0 != 0 {
        set_clause_source_relation(&mut clause.source_relation, SourceRelation::AttachedToSelf)?;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_NAMED_AGENDA.0 != 0 {
        set_clause_source_relation(
            &mut clause.source_relation,
            SourceRelation::MatchesNamedAgendaSlot,
        )?;
    }
    let handled_flags = AstFilterFlags::EXCLUDE_SELF.0
        | AstFilterFlags::ENTERED_THIS_TURN.0
        | AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0
        | AstFilterFlags::ATTACKED_THIS_TURN.0
        | AstFilterFlags::DID_NOT_ATTACK_THIS_TURN.0
        | AstFilterFlags::BLOCKED_THIS_TURN.0
        | AstFilterFlags::PAIRED_WITH_SELF.0
        | AstFilterFlags::DIED_THIS_TURN.0
        | AstFilterFlags::MODIFIED.0
        | AstFilterFlags::EQUIPPED.0
        | AstFilterFlags::ENCHANTED.0
        | AstFilterFlags::IS_COMMANDER.0
        | AstFilterFlags::NOT_COMMANDER.0
        | AstFilterFlags::WITH_ANY_COUNTER.0
        | AstFilterFlags::WITHOUT_COUNTERS.0
        | AstFilterFlags::FACE_DOWN.0
        | AstFilterFlags::FACE_UP.0
        | AstFilterFlags::EXILED_WITH_SELF.0
        | AstFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN.0
        | AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0
        | AstFilterFlags::MATCHES_CHOSEN_COLOR.0
        | AstFilterFlags::MATCHES_CHOSEN_TYPE.0
        | AstFilterFlags::MATCHES_CHOSEN_CARD_TYPE.0
        | AstFilterFlags::MATCHES_CHOSEN_NAME.0
        | AstFilterFlags::ATTACHED_TO_SELF.0
        | AstFilterFlags::MATCHES_NAMED_AGENDA.0
        | AstFilterFlags::KEYWORDS_MATCH_ANY.0
        | AstFilterFlags::LKI_PAST_TENSE.0;
    if (f.flags.0 & !handled_flags) != 0 {
        return Err("oracle:unsupported-filter-flag");
    }
    let is_graveyard_zone = |zone: ZoneFilter| {
        matches!(
            zone,
            ZoneFilter::YourGraveyard
                | ZoneFilter::OpponentGraveyard
                | ZoneFilter::OpponentsGraveyards
                | ZoneFilter::TargetOpponentGraveyard
                | ZoneFilter::DefendingPlayerGraveyard
                | ZoneFilter::SingleGraveyard
                | ZoneFilter::AnyGraveyard
                | ZoneFilter::AllGraveyards
                | ZoneFilter::ThatPlayerGraveyard
                | ZoneFilter::TargetPlayerGraveyard
                | ZoneFilter::ItsOwnerGraveyard
        )
    };
    let in_graveyard_context = match f.zone {
        Some(z) => is_graveyard_zone(z),
        None => f.any_card && !f.any_spell,
    };
    let zone_compatible_with_graveyard = f.zone.is_none_or(is_graveyard_zone);
    let mut mana_value_pair: Option<(u32, u32)> = None;
    let mut any_of_pair: Option<(ObjectClause, ObjectClause)> = None;
    let mut subtypes_match_any = false;
    let mut extra_forbidden_subtypes: Vec<&str> = Vec::new();
    for pred in &f.predicates {
        match pred {
            FilterPredicateAst::HasAbilityKind { kind, negated } => {
                let norm_kind = kind.trim().to_ascii_lowercase();
                if matches!(norm_kind.as_str(), "adventure" | "an adventure") && !negated {
                    clause.flags = clause.flags.union(ObjectFilterFlags::HAS_ADVENTURE);
                } else if norm_kind == "toxic" && !negated {
                    clause.flags = clause.flags.union(ObjectFilterFlags::HAS_TOXIC);
                } else if matches!(
                    norm_kind.as_str(),
                    "activated"
                        | "activated_ability"
                        | "activated ability"
                        | "an activated ability"
                        | "activated abilities"
                ) {
                    if *negated {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::HAS_ACTIVATED_ABILITY)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::NO_ACTIVATED_ABILITIES);
                    } else {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::NO_ACTIVATED_ABILITIES)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_ACTIVATED_ABILITY);
                    }
                } else if let Some(kw) = lower_keyword_ability_name(&norm_kind) {
                    if *negated {
                        if clause.forbidden_keyword.is_some_and(|prev| prev != kw)
                            || clause.keyword == Some(kw)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.forbidden_keyword = Some(kw);
                    } else {
                        if clause.keyword.is_some_and(|prev| prev != kw)
                            || clause.forbidden_keyword == Some(kw)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.keyword = Some(kw);
                    }
                } else {
                    return Err("oracle:unsupported-filter-predicate");
                }
            }
            FilterPredicateAst::NotOwnedByController => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::NOT_OWNED_BY_CONTROLLER);
            }
            FilterPredicateAst::SameTotalPowerAndToughness => {
                clause.flags = clause.flags.union(ObjectFilterFlags::SAME_TOTAL_PT);
            }
            FilterPredicateAst::ColorCountCompare { op, expr } => {
                let target_color = match (op, expr.as_ref()) {
                    (CompareOp::Eq | CompareOp::Le, ValueExprAst::Const(0)) => {
                        ObjectColorFilter::Colorless
                    }
                    (CompareOp::Eq, ValueExprAst::Const(1)) => ObjectColorFilter::Monocolored,
                    (CompareOp::Ge, ValueExprAst::Const(1))
                    | (CompareOp::Gt, ValueExprAst::Const(0)) => ObjectColorFilter::Colored,
                    (CompareOp::Ge, ValueExprAst::Const(2))
                    | (CompareOp::Gt, ValueExprAst::Const(1)) => ObjectColorFilter::Multicolored,
                    _ => return Err("oracle:unsupported-filter-predicate"),
                };
                if clause.color != ObjectColorFilter::Any && clause.color != target_color {
                    return Err("oracle:unsupported-filter-predicate");
                }
                clause.color = target_color;
            }
            FilterPredicateAst::AttachedBy(att_f) => {
                let simple_attachment = is_simple_attachment_filter(att_f);
                if simple_attachment
                    && att_f.subtypes.len() == 1
                    && att_f.subtypes[0].eq_ignore_ascii_case("aura")
                    && (att_f.any_types.is_empty()
                        || att_f.any_types == AstCardTypeMask::ENCHANTMENT)
                {
                    clause.flags = clause.flags.union(ObjectFilterFlags::ENCHANTED);
                } else if simple_attachment
                    && att_f.subtypes.len() == 1
                    && att_f.subtypes[0].eq_ignore_ascii_case("equipment")
                    && (att_f.any_types.is_empty() || att_f.any_types == AstCardTypeMask::ARTIFACT)
                {
                    clause.flags = clause.flags.union(ObjectFilterFlags::EQUIPPED);
                } else {
                    return Err("oracle:unsupported-filter-predicate");
                }
            }
            FilterPredicateAst::WithSameControllerAs(subj) => {
                match peel_battlefield_subject(subj) {
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => {
                        if !matches!(
                            clause.controller,
                            ControllerFilter::Any | ControllerFilter::FirstTargetController
                        ) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.controller = ControllerFilter::FirstTargetController;
                    }
                    SubjectRefAst::Anaphora(raw)
                        if raw.trim().to_ascii_lowercase().contains("target") =>
                    {
                        if !matches!(
                            clause.controller,
                            ControllerFilter::Any | ControllerFilter::FirstTargetController
                        ) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.controller = ControllerFilter::FirstTargetController;
                    }
                    _ => return Err("oracle:unsupported-filter-predicate"),
                }
            }
            FilterPredicateAst::WithoutCounterKind(ck_raw) => {
                let ck_low = ck_raw.trim().to_ascii_lowercase();
                let ck_clean = ck_low
                    .trim_end_matches(" counters")
                    .trim_end_matches(" counter")
                    .trim();
                let ck = ck_clean
                    .strip_prefix("any ")
                    .or_else(|| ck_clean.strip_prefix("a "))
                    .or_else(|| ck_clean.strip_prefix("an "))
                    .unwrap_or(ck_clean)
                    .trim();
                if matches!(ck, "" | "any" | "a" | "one or more") {
                    if clause.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER)
                        || clause
                            .flags
                            .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
                        || clause
                            .flags
                            .contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
                        || clause.required_counter_kind.is_some()
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                } else {
                    let kind = parse_counter_kind(ck)
                        .filter(|k| k.is_permanent())
                        .ok_or("oracle:unsupported-filter-predicate")?;
                    if clause.required_counter_kind == Some(kind)
                        || (kind == CounterKind::PlusOne
                            && clause
                                .flags
                                .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER))
                        || (kind == CounterKind::MinusOne
                            && clause
                                .flags
                                .contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER))
                        || clause
                            .forbidden_counter_kind
                            .is_some_and(|prev| prev != kind)
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.forbidden_counter_kind = Some(kind);
                }
            }
            FilterPredicateAst::SharesPropertyWith {
                property,
                reference,
                negated: false,
            } => {
                let prop_raw = property.trim().to_ascii_lowercase();
                if matches!(prop_raw.as_str(), "mana_value" | "mana value") {
                    let anchor = lower_anchor(&ValueExprAst::ObjectStat {
                        subject: reference.clone(),
                        stat: StatKind::ManaValue,
                    })
                    .ok_or("oracle:unsupported-filter-predicate")?;
                    if clause.mana_value != ManaValueFilter::Any || mana_value_pair.is_some() {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.mana_value = ManaValueFilter::CompareAnchor {
                        op: Comparison::Equal,
                        anchor,
                    };
                } else if prop_raw == "power" {
                    let anchor = lower_anchor(&ValueExprAst::ObjectStat {
                        subject: reference.clone(),
                        stat: StatKind::Power,
                    })
                    .ok_or("oracle:unsupported-filter-predicate")?;
                    if clause.power != PowerFilter::Any {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.power = PowerFilter::CompareAnchor {
                        op: Comparison::Equal,
                        anchor,
                    };
                } else if prop_raw == "toughness" {
                    let anchor = lower_anchor(&ValueExprAst::ObjectStat {
                        subject: reference.clone(),
                        stat: StatKind::Toughness,
                    })
                    .ok_or("oracle:unsupported-filter-predicate")?;
                    if clause.toughness != ToughnessFilter::Any {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.toughness = ToughnessFilter::CompareAnchor {
                        op: Comparison::Equal,
                        anchor,
                    };
                } else {
                    let prop = match prop_raw.as_str() {
                        "color" | "a color" => "color",
                        "creature_type" | "creature type" | "a creature type" => "creature_type",
                        "card_type" | "card type" | "a card type" => "card_type",
                        "name" | "a name" => "name",
                        _ => return Err("oracle:unsupported-filter-predicate"),
                    };
                    let flag = match peel_battlefield_subject(reference) {
                        SubjectRefAst::SelfObject => match prop {
                            "color" => ObjectFilterFlags::SHARES_COLOR_WITH_SOURCE,
                            "creature_type" => ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_SOURCE,
                            "card_type" => ObjectFilterFlags::SHARES_CARD_TYPE_WITH_SOURCE,
                            "name" => ObjectFilterFlags::SAME_NAME_AS_SOURCE,
                            _ => return Err("oracle:unsupported-filter-predicate"),
                        },
                        SubjectRefAst::Anaphora(raw)
                            if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                        {
                            match prop {
                                "color" => ObjectFilterFlags::SHARES_COLOR_WITH_SOURCE,
                                "creature_type" => {
                                    ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_SOURCE
                                }
                                "card_type" => ObjectFilterFlags::SHARES_CARD_TYPE_WITH_SOURCE,
                                "name" => ObjectFilterFlags::SAME_NAME_AS_SOURCE,
                                _ => return Err("oracle:unsupported-filter-predicate"),
                            }
                        }
                        SubjectRefAst::TargetObject {
                            count: TargetCountAst::Exact(1),
                            ..
                        } => match prop {
                            "color" => ObjectFilterFlags::SHARES_COLOR_WITH_TARGET,
                            "creature_type" => ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_TARGET,
                            "card_type" => ObjectFilterFlags::SHARES_CARD_TYPE_WITH_TARGET,
                            "name" => ObjectFilterFlags::SAME_NAME_AS_TARGET,
                            _ => return Err("oracle:unsupported-filter-predicate"),
                        },
                        SubjectRefAst::Anaphora(raw) => {
                            let low = raw.trim().to_ascii_lowercase();
                            if low.contains("target") {
                                match prop {
                                    "color" => ObjectFilterFlags::SHARES_COLOR_WITH_TARGET,
                                    "creature_type" => {
                                        ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_TARGET
                                    }
                                    "card_type" => ObjectFilterFlags::SHARES_CARD_TYPE_WITH_TARGET,
                                    "name" => ObjectFilterFlags::SAME_NAME_AS_TARGET,
                                    _ => return Err("oracle:unsupported-filter-predicate"),
                                }
                            } else if (low.contains("sacrificed creature")
                                || low.contains("sacrificed permanent")
                                || low.contains("sacrificed card"))
                                && prop == "color"
                            {
                                ObjectFilterFlags::SHARES_COLOR_WITH_LAST_SACRIFICED
                            } else {
                                return Err("oracle:unsupported-filter-predicate");
                            }
                        }
                        _ => return Err("oracle:unsupported-filter-predicate"),
                    };
                    clause.flags = clause.flags.union(flag);
                }
            }
            FilterPredicateAst::SharesPropertyWith {
                property,
                reference,
                negated: true,
            } if matches!(
                property.trim().to_ascii_lowercase().as_str(),
                "name" | "a name"
            ) =>
            {
                let is_self = match peel_battlefield_subject(reference) {
                    SubjectRefAst::SelfObject => true,
                    SubjectRefAst::Anaphora(raw) => {
                        is_self_object_anaphora(&raw.trim().to_ascii_lowercase())
                    }
                    _ => false,
                };
                if !is_self
                    || clause
                        .flags
                        .contains(ObjectFilterFlags::SAME_NAME_AS_SOURCE)
                    || matches!(
                        clause.source_relation,
                        SourceRelation::SharingNameWithAbilitySource
                            | SourceRelation::OtherThanAbilitySourceSharingName
                    )
                {
                    return Err("oracle:unsupported-filter-predicate");
                }
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE);
            }
            FilterPredicateAst::DifferentNameFrom(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    if clause
                        .flags
                        .contains(ObjectFilterFlags::SAME_NAME_AS_SOURCE)
                        || matches!(
                            clause.source_relation,
                            SourceRelation::SharingNameWithAbilitySource
                                | SourceRelation::OtherThanAbilitySourceSharingName
                        )
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE);
                }
                SubjectRefAst::Anaphora(raw)
                    if is_self_object_anaphora(&raw.trim().to_ascii_lowercase()) =>
                {
                    if clause
                        .flags
                        .contains(ObjectFilterFlags::SAME_NAME_AS_SOURCE)
                        || matches!(
                            clause.source_relation,
                            SourceRelation::SharingNameWithAbilitySource
                                | SourceRelation::OtherThanAbilitySourceSharingName
                        )
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE);
                }
                SubjectRefAst::Group(g) | SubjectRefAst::All(g) | SubjectRefAst::Each(g)
                    if g.any_permanent
                        && g.controller == Some(ControllerRelation::You)
                        && filter_only_has(
                            g,
                            FilterAllow {
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                        && (g.flags.0
                            & !(AstFilterFlags::EXCLUDE_SELF.0
                                | AstFilterFlags::LKI_PAST_TENSE.0))
                            == 0 =>
                {
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::UNIQUE_NAME_AMONG_CONTROLLED);
                }
                _ => return Err("oracle:unsupported-filter-predicate"),
            },
            FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay
                | PriorActionRefKind::DiscardedThisWay
                | PriorActionRefKind::DestroyedThisWay
                | PriorActionRefKind::SacrificedThisWay
                | PriorActionRefKind::PutIntoGraveyardThisWay
                | PriorActionRefKind::ReturnedToHandThisWay
                | PriorActionRefKind::RevealedThisWay,
            ) => {
                clause.flags = clause.flags.union(ObjectFilterFlags::IN_LAST_MOVED);
            }
            FilterPredicateAst::PriorActionZoneRef { kind }
                if matches!(
                    kind.as_str(),
                    "milled_this_way"
                        | "put_onto_battlefield_this_way"
                        | "exiled_this_way"
                        | "discarded_this_way"
                        | "destroyed_this_way"
                        | "sacrificed_this_way"
                        | "returned_this_way"
                        | "put_into_graveyard_this_way"
                        | "returned_to_hand_this_way"
                        | "revealed_this_way"
                ) =>
            {
                clause.flags = clause.flags.union(ObjectFilterFlags::IN_LAST_MOVED);
            }
            FilterPredicateAst::SubtypesMatchAny => {
                subtypes_match_any = true;
            }
            FilterPredicateAst::TypesMatchAll => {}
            FilterPredicateAst::HasBasicLandType => {
                if clause.subtypes == SubtypeFilter::Any {
                    use spellwake_engine::program::{SingleSubtype, SubtypeList};
                    let list = SubtypeList::from_slice(&[
                        SingleSubtype::Land(LandType::Plains),
                        SingleSubtype::Land(LandType::Island),
                        SingleSubtype::Land(LandType::Swamp),
                        SingleSubtype::Land(LandType::Mountain),
                        SingleSubtype::Land(LandType::Forest),
                    ])
                    .ok_or("oracle:unsupported-filter-predicate")?;
                    clause.subtypes = SubtypeFilter::AnyOfList(list);
                } else {
                    clause.flags = clause.flags.union(ObjectFilterFlags::HAS_BASIC_LAND_TYPE);
                }
            }
            FilterPredicateAst::HasManaSymbolInCost(sym)
                if sym.trim().to_ascii_lowercase().contains("{x}")
                    || sym.trim().eq_ignore_ascii_case("x") =>
            {
                clause.flags = clause.flags.union(ObjectFilterFlags::HAS_X_IN_COST);
            }
            FilterPredicateAst::HasAbilities {
                has_abilities: false,
            } => {
                clause.flags = clause.flags.union(ObjectFilterFlags::NO_ABILITIES);
            }
            FilterPredicateAst::StateQualification(raw)
                if raw.trim().to_ascii_lowercase().starts_with("pt:") =>
            {
                let norm = raw.trim().to_ascii_lowercase();
                let Some((p_s, t_s)) = norm
                    .strip_prefix("pt:")
                    .and_then(|rest| rest.split_once('/'))
                else {
                    return Err("oracle:unsupported-filter-predicate");
                };
                let (Ok(p), Ok(t)) = (p_s.trim().parse::<i16>(), t_s.trim().parse::<i16>()) else {
                    return Err("oracle:unsupported-filter-predicate");
                };
                if clause.power != PowerFilter::Any || clause.toughness != ToughnessFilter::Any {
                    return Err("oracle:unsupported-filter-predicate");
                }
                clause.power = PowerFilter::Equal(p);
                clause.toughness = ToughnessFilter::Equal(t);
            }
            FilterPredicateAst::PutIntoZoneThisTurn => {
                if !in_graveyard_context {
                    return Err("oracle:unsupported-filter-predicate");
                }
                clause.flags = clause.flags.union(ObjectFilterFlags::PUT_INTO_GY_THIS_TURN);
            }
            FilterPredicateAst::EnteredZoneFromThisTurn { from_zone } => {
                if !in_graveyard_context {
                    return Err("oracle:unsupported-filter-predicate");
                }
                match from_zone {
                    ZoneFilter::YourLibrary | ZoneFilter::AnyLibrary => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::PUT_INTO_GY_FROM_LIBRARY_THIS_TURN);
                    }
                    ZoneFilter::Battlefield => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::DIED_THIS_TURN);
                    }
                    _ => return Err("oracle:unsupported-filter-predicate"),
                }
            }
            FilterPredicateAst::DealtDamageBy(subj)
                if matches!(subj.as_ref(), SubjectRefAst::SelfObject) =>
            {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
            }
            FilterPredicateAst::AnyOf(filters) => {
                if any_of_pair.is_some() || mana_value_pair.is_some() || filters.len() < 2 {
                    return Err("oracle:unsupported-filter-predicate");
                }
                let mut clauses = Vec::with_capacity(filters.len());
                for sub_ast in filters {
                    let ObjectFilter::Clause(sub_clause) =
                        lower_object_filter(sub_ast, vocabulary)?
                    else {
                        return Err("oracle:unsupported-filter-predicate");
                    };
                    let mut merged = false;
                    for existing in &mut clauses {
                        if let Some(u) = try_union_any_types_clauses(*existing, sub_clause)
                            .or_else(|| try_union_color_clauses(*existing, sub_clause))
                        {
                            *existing = u;
                            merged = true;
                            break;
                        }
                    }
                    if !merged {
                        clauses.push(sub_clause);
                    }
                }
                match clauses.as_slice() {
                    [single] => {
                        clause = intersect_object_clauses(clause, *single)?;
                    }
                    [c1, c2] => {
                        any_of_pair = Some((*c1, *c2));
                    }
                    _ => return Err("oracle:unsupported-filter-predicate"),
                }
            }
            FilterPredicateAst::Exclude(subj)
                if matches!(peel_battlefield_subject(subj), SubjectRefAst::SelfObject)
                    || matches!(
                        peel_battlefield_subject(subj),
                        SubjectRefAst::Anaphora(raw)
                            if is_self_object_anaphora(&raw.trim().to_ascii_lowercase())
                    ) =>
            {
                set_clause_source_relation(
                    &mut clause.source_relation,
                    SourceRelation::OtherThanAbilitySource,
                )?;
            }
            FilterPredicateAst::Exclude(subj)
                if matches!(subj.as_ref(), SubjectRefAst::Group(_)) =>
            {
                use spellwake_oracle_compiler::ast::NameFilterAst;
                let SubjectRefAst::Group(excl) = subj.as_ref() else {
                    unreachable!();
                };
                // Pure single-dimension negation: fold into forbidden_types / forbidden_subtypes /
                // forbidden_supertypes / color / tapped / token / forbidden_keyword / flags without
                // distorting multi-attribute conjunctions (De Morgan safety).
                let preds_only_subtype_any = excl
                    .predicates
                    .iter()
                    .all(|p| matches!(p, FilterPredicateAst::SubtypesMatchAny));
                let excl_simple_shell = excl.zone.is_none()
                    && excl.extra_zones.is_empty()
                    && excl.controller.is_none()
                    && excl.owner.is_none()
                    && excl.combat.is_none()
                    && excl.stat_bounds.is_empty()
                    && excl.counter_kind.is_none()
                    && excl.forbidden_types.0 == 0
                    && excl.forbidden_supertypes.0 == 0
                    && excl.forbidden_subtypes.is_empty()
                    && excl.forbidden_colors.0 == 0
                    && excl.forbidden_keywords.is_empty()
                    && !excl.any_spell
                    && !excl.is_ability
                    && preds_only_subtype_any;
                if !excl_simple_shell {
                    return Err("oracle:unsupported-filter-predicate");
                }
                let dim_types = excl.any_types.0 != 0;
                let dim_supers = excl.required_supertypes.0 != 0;
                let dim_subs = !excl.subtypes.is_empty();
                let dim_colors = excl.any_colors.0 != 0;
                let dim_arity = excl.color_arity.is_some();
                let dim_named = excl.named.is_some();
                let dim_flags = excl.flags.0 != 0;
                let dim_tapped = excl.tapped == Some(true);
                let dim_token = excl.token == Some(true);
                let dim_kws = !excl.required_keywords.is_empty();
                if excl.tapped == Some(false) || excl.token == Some(false) {
                    return Err("oracle:unsupported-filter-predicate");
                }
                let active_dims = usize::from(dim_types)
                    + usize::from(dim_supers)
                    + usize::from(dim_subs)
                    + usize::from(dim_colors)
                    + usize::from(dim_arity)
                    + usize::from(dim_named)
                    + usize::from(dim_flags)
                    + usize::from(dim_tapped)
                    + usize::from(dim_token)
                    + usize::from(dim_kws);
                if active_dims != 1 {
                    return Err("oracle:unsupported-filter-predicate");
                }
                if dim_subs {
                    let is_disjunctive = excl.subtypes.len() == 1
                        || excl
                            .predicates
                            .contains(&FilterPredicateAst::SubtypesMatchAny);
                    if !is_disjunctive {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    extra_forbidden_subtypes.extend(excl.subtypes.iter().map(String::as_str));
                } else if dim_types {
                    let ft = lower_card_type_mask(excl.any_types)?;
                    clause.forbidden_types = clause.forbidden_types.union(ft);
                } else if dim_supers {
                    if (excl.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0 {
                        if excl.required_supertypes.0 != AstSupertypeMask::HISTORIC.0
                            || clause.flags.contains(ObjectFilterFlags::HISTORIC)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::NON_HISTORIC);
                    } else {
                        if excl.required_supertypes.0.count_ones() != 1 {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        for st in supertype_mask_to_set(excl.required_supertypes)? {
                            clause.forbidden_supertypes = clause
                                .forbidden_supertypes
                                .union(SupertypeMask::from_supertype(st));
                        }
                    }
                } else if dim_colors {
                    let cm = lower_color_mask(excl.any_colors);
                    match clause.color {
                        ObjectColorFilter::Any => {
                            clause.color = ObjectColorFilter::Mask {
                                any_of: ColorMask::EMPTY,
                                none_of: cm,
                            };
                        }
                        ObjectColorFilter::Mask { any_of, none_of } => {
                            clause.color = ObjectColorFilter::Mask {
                                any_of,
                                none_of: none_of.union(cm),
                            };
                        }
                        _ => return Err("oracle:unsupported-filter-predicate"),
                    }
                } else if dim_arity {
                    let negated_color = match excl.color_arity {
                        Some(ColorArity::Colorless) => ObjectColorFilter::Colored,
                        Some(ColorArity::Colored) => ObjectColorFilter::Colorless,
                        _ => return Err("oracle:unsupported-filter-predicate"),
                    };
                    if clause.color != ObjectColorFilter::Any && clause.color != negated_color {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.color = negated_color;
                } else if dim_named {
                    if !matches!(excl.named, Some(NameFilterAst::SelfName))
                        || clause
                            .flags
                            .contains(ObjectFilterFlags::SAME_NAME_AS_SOURCE)
                        || matches!(
                            clause.source_relation,
                            SourceRelation::SharingNameWithAbilitySource
                                | SourceRelation::OtherThanAbilitySourceSharingName
                        )
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE);
                } else if dim_flags {
                    if excl.flags == AstFilterFlags::IS_COMMANDER {
                        if clause.flags.contains(ObjectFilterFlags::COMMANDER_ONLY) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::NOT_COMMANDER);
                    } else if excl.flags == AstFilterFlags::FACE_DOWN {
                        if clause.flags.contains(ObjectFilterFlags::FACE_DOWN) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_UP);
                    } else if excl.flags == AstFilterFlags::FACE_UP {
                        if clause.flags.contains(ObjectFilterFlags::FACE_UP) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_DOWN);
                    } else if excl.flags == AstFilterFlags::ATTACKED_THIS_TURN {
                        if clause.flags.contains(ObjectFilterFlags::ATTACKED_THIS_TURN) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN);
                    } else if excl.flags == AstFilterFlags::DID_NOT_ATTACK_THIS_TURN {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::ATTACKED_THIS_TURN);
                    } else {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                } else if dim_token {
                    if clause.token.is_some_and(|v| v) {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.token = Some(false);
                } else if dim_tapped {
                    if clause.tapped.is_some_and(|v| v) {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.tapped = Some(false);
                } else if dim_kws {
                    let [kw_raw] = excl.required_keywords.as_slice() else {
                        return Err("oracle:unsupported-filter-predicate");
                    };
                    let kw = lower_keyword_ability_name(kw_raw)
                        .ok_or("oracle:unsupported-filter-predicate")?;
                    if clause.forbidden_keyword.is_some_and(|prev| prev != kw) {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.forbidden_keyword = Some(kw);
                }
            }
            FilterPredicateAst::AttachedTo(subj) => match peel_battlefield_subject(subj) {
                SubjectRefAst::SelfObject => {
                    set_clause_source_relation(
                        &mut clause.source_relation,
                        SourceRelation::AttachedToSelf,
                    )?;
                }
                SubjectRefAst::TargetObject {
                    count: TargetCountAst::Exact(1),
                    ..
                } => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::ATTACHED_TO_TARGET);
                }
                SubjectRefAst::Anaphora(raw)
                    if raw.trim().to_ascii_lowercase().contains("target") =>
                {
                    clause.flags = clause.flags.union(ObjectFilterFlags::ATTACHED_TO_TARGET);
                }
                _ => return Err("oracle:unsupported-filter-predicate"),
            },
            FilterPredicateAst::Blocking(subj)
                if matches!(peel_battlefield_subject(subj), SubjectRefAst::SelfObject)
                    || matches!(
                        peel_battlefield_subject(subj),
                        SubjectRefAst::Anaphora(raw)
                            if is_self_object_anaphora(&raw.trim().to_ascii_lowercase())
                    ) =>
            {
                set_clause_combat_filter(&mut clause.combat, CombatFilter::BlockingSource)?;
            }
            FilterPredicateAst::Blocking(subj)
                if matches!(
                    peel_battlefield_subject(subj),
                    SubjectRefAst::AttachedHost(_)
                ) =>
            {
                set_clause_combat_filter(&mut clause.combat, CombatFilter::BlockingAttachedHost)?;
            }
            FilterPredicateAst::BlockedBy {
                blocker,
                this_turn: false,
            } if matches!(peel_battlefield_subject(blocker), SubjectRefAst::SelfObject)
                || matches!(
                    peel_battlefield_subject(blocker),
                    SubjectRefAst::Anaphora(raw)
                        if is_self_object_anaphora(&raw.trim().to_ascii_lowercase())
                ) =>
            {
                set_clause_combat_filter(&mut clause.combat, CombatFilter::BlockedBySource)?;
            }
            FilterPredicateAst::BlockedOrBlockedBy {
                other,
                this_turn: false,
            } if filter_represents_self_object(other) => {
                set_clause_combat_filter(
                    &mut clause.combat,
                    CombatFilter::BlockingOrBlockedBySource,
                )?;
            }
            FilterPredicateAst::AttackingTarget {
                target,
                this_turn: false,
            } => {
                let mapped = match target.as_ref() {
                    SubjectRefAst::Player(PlayerRefAst::You) => {
                        Some(CombatFilter::AttackingController)
                    }
                    SubjectRefAst::Player(
                        PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                    ) => Some(CombatFilter::AttackingOpponent),
                    SubjectRefAst::Player(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer) => {
                        Some(CombatFilter::AttackingPlayer)
                    }
                    SubjectRefAst::Compound(parts) if parts.len() == 2 => {
                        let is_pw = |s: &SubjectRefAst, expected_ctrl: ControllerRelation| match s {
                            SubjectRefAst::Group(f) | SubjectRefAst::All(f) => {
                                f.any_types == AstCardTypeMask::PLANESWALKER
                                    && f.controller == Some(expected_ctrl)
                                    && f.forbidden_types.is_empty()
                                    && f.predicates.is_empty()
                            }
                            _ => false,
                        };
                        match (&parts[0], &parts[1]) {
                            (SubjectRefAst::Player(PlayerRefAst::You), second)
                                if is_pw(second, ControllerRelation::You) =>
                            {
                                Some(CombatFilter::AttackingControllerOrPlaneswalker)
                            }
                            (
                                SubjectRefAst::Player(
                                    PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                                ),
                                second,
                            ) if is_pw(second, ControllerRelation::Opponents)
                                || is_pw(second, ControllerRelation::Opponent) =>
                            {
                                Some(CombatFilter::AttackingOpponentOrPlaneswalker)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(cf) = mapped {
                    set_clause_combat_filter(&mut clause.combat, cf)?;
                } else {
                    return Err("oracle:unsupported-filter-predicate");
                }
            }
            FilterPredicateAst::ManaValueParity { even } => {
                if clause.mana_value == ManaValueFilter::Any && mana_value_pair.is_none() {
                    if (*even && clause.flags.contains(ObjectFilterFlags::ODD_MANA_VALUE))
                        || (!*even && clause.flags.contains(ObjectFilterFlags::EVEN_MANA_VALUE))
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.mana_value = if *even {
                        ManaValueFilter::Even
                    } else {
                        ManaValueFilter::Odd
                    };
                } else if *even {
                    if clause.mana_value == ManaValueFilter::Odd
                        || clause.flags.contains(ObjectFilterFlags::ODD_MANA_VALUE)
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.flags = clause.flags.union(ObjectFilterFlags::EVEN_MANA_VALUE);
                } else {
                    if clause.mana_value == ManaValueFilter::Even
                        || clause.flags.contains(ObjectFilterFlags::EVEN_MANA_VALUE)
                    {
                        return Err("oracle:unsupported-filter-predicate");
                    }
                    clause.flags = clause.flags.union(ObjectFilterFlags::ODD_MANA_VALUE);
                }
            }
            FilterPredicateAst::ManaValueIn(vals) => {
                if clause.mana_value != ManaValueFilter::Any || mana_value_pair.is_some() {
                    return Err("oracle:unsupported-filter-predicate");
                }
                match vals.as_slice() {
                    [v] => {
                        let v_u32 =
                            u32::try_from(*v).map_err(|_| "oracle:unsupported-filter-predicate")?;
                        clause.mana_value = ManaValueFilter::Equal(v_u32);
                    }
                    [a, b] => {
                        if any_of_pair.is_some() {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        let a_u32 =
                            u32::try_from(*a).map_err(|_| "oracle:unsupported-filter-predicate")?;
                        let b_u32 =
                            u32::try_from(*b).map_err(|_| "oracle:unsupported-filter-predicate")?;
                        if a_u32 == b_u32 {
                            clause.mana_value = ManaValueFilter::Equal(a_u32);
                        } else {
                            mana_value_pair = Some((a_u32, b_u32));
                        }
                    }
                    many if many.len() >= 3 => {
                        let mut parsed = Vec::with_capacity(many.len());
                        for v in many {
                            parsed.push(
                                u32::try_from(*v)
                                    .map_err(|_| "oracle:unsupported-filter-predicate")?,
                            );
                        }
                        parsed.sort_unstable();
                        parsed.dedup();
                        if parsed.first() == Some(&0)
                            && parsed
                                .iter()
                                .enumerate()
                                .all(|(idx, &val)| val as usize == idx)
                        {
                            let max_mv =
                                *parsed.last().ok_or("oracle:unsupported-filter-predicate")?;
                            clause.mana_value = ManaValueFilter::AtMost(max_mv);
                        } else {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                    }
                    _ => return Err("oracle:unsupported-filter-predicate"),
                }
            }
            FilterPredicateAst::StatRelationTo { stat, cmp, expr } => {
                let anchor = lower_anchor(expr).ok_or("oracle:unsupported-filter-predicate")?;
                let op = lower_cmp(*cmp);
                match stat {
                    StatKind::Power => {
                        if clause.power != PowerFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.power = PowerFilter::CompareAnchor { op, anchor };
                    }
                    StatKind::Toughness => {
                        if clause.toughness != ToughnessFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.toughness = ToughnessFilter::CompareAnchor { op, anchor };
                    }
                    StatKind::ManaValue => {
                        if clause.mana_value != ManaValueFilter::Any || mana_value_pair.is_some() {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.mana_value = ManaValueFilter::CompareAnchor { op, anchor };
                    }
                    StatKind::Loyalty => return Err("oracle:unsupported-filter-predicate"),
                }
            }
            FilterPredicateAst::StateQualification(raw) => {
                use spellwake_engine::program::DynamicStatAnchor;
                let norm = raw.trim().to_ascii_lowercase();
                match norm.as_str() {
                    "that was dealt damage this turn"
                    | "that were dealt damage this turn"
                    | "that has been dealt damage this turn"
                    | "that have been dealt damage this turn" => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
                    }
                    "that's tapped" | "that is tapped" | "that are tapped" => {
                        if clause.tapped == Some(false) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.tapped = Some(true);
                    }
                    "that's untapped" | "that is untapped" | "that are untapped" => {
                        if clause.tapped == Some(true) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.tapped = Some(false);
                    }
                    "that's equipped" | "that is equipped" | "that are equipped" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::EQUIPPED);
                    }
                    "that's enchanted" | "that is enchanted" | "that are enchanted" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::ENCHANTED);
                    }
                    "that's modified" | "that is modified" | "that are modified" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::MODIFIED);
                    }
                    "that's attacking" | "that is attacking" => {
                        set_clause_combat_filter(&mut clause.combat, CombatFilter::Attacking)?;
                    }
                    "that's blocking" | "that is blocking" => {
                        set_clause_combat_filter(&mut clause.combat, CombatFilter::Blocking)?;
                    }
                    "that's attacking or blocking" | "that is attacking or blocking" => {
                        set_clause_combat_filter(
                            &mut clause.combat,
                            CombatFilter::AttackingOrBlocking,
                        )?;
                    }
                    "that's blocked" | "that is blocked" => {
                        set_clause_combat_filter(&mut clause.combat, CombatFilter::Blocked)?;
                    }
                    "that's unblocked" | "that is unblocked" => {
                        set_clause_combat_filter(&mut clause.combat, CombatFilter::Unblocked)?;
                    }
                    "that's blocking it" | "blocking it" => {
                        set_clause_combat_filter(&mut clause.combat, CombatFilter::BlockingSource)?;
                    }
                    "that attacked this turn" | "you attacked with this turn" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::ATTACKED_THIS_TURN);
                    }
                    "that entered this turn" | "that entered the battlefield this turn" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::ENTERED_THIS_TURN);
                    }
                    "with a +1/+1 counter on it" => {
                        if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER);
                    }
                    "with a -1/-1 counter on it"
                    | "that have -1/-1 counters on them"
                    | "that has a -1/-1 counter on it"
                    | "with -1/-1 counters on them"
                    | "with a -1/-1 counter on them" => {
                        if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER);
                    }
                    "with a counter on it"
                    | "with counters on it"
                    | "with a counter on them"
                    | "with counters on them"
                    | "with one or more counters on it"
                    | "with one or more counters on them"
                    | "that has a counter on it"
                    | "that have a counter on them" => {
                        if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
                    }
                    "without counters on it" | "with no counters on it" => {
                        if clause.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER)
                            || clause
                                .flags
                                .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
                            || clause
                                .flags
                                .contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                    }
                    "that were put there from the battlefield this turn"
                    | "that was put there from the battlefield this turn"
                    | "that were put into your graveyard from the battlefield this turn"
                    | "that were put into graveyards from the battlefield this turn"
                    | "that was put into your graveyard from the battlefield this turn"
                    | "put into a graveyard from the battlefield this turn"
                    | "that was put into a graveyard from the battlefield this turn"
                    | "that were put into a graveyard from the battlefield this turn"
                    | "put into your graveyard from the battlefield this turn"
                    | "put into graveyards from the battlefield this turn" => {
                        if !zone_compatible_with_graveyard {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::DIED_THIS_TURN);
                    }
                    "put into your graveyard this turn"
                    | "that were put into your graveyard this turn"
                    | "that was put into your graveyard this turn"
                    | "that were put into a graveyard this turn"
                    | "put into a graveyard this turn"
                    | "that was put into a graveyard this turn"
                    | "that were put into graveyards this turn"
                    | "put into graveyards this turn" => {
                        if !zone_compatible_with_graveyard {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::PUT_INTO_GY_THIS_TURN);
                    }
                    "that were put into your graveyard from your library this turn"
                    | "put into your graveyard from your library this turn"
                    | "that was put into your graveyard from your library this turn" => {
                        if !zone_compatible_with_graveyard {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::PUT_INTO_GY_FROM_LIBRARY_THIS_TURN);
                    }
                    "that was put there from your library this turn"
                    | "that were put there from libraries this turn" => {
                        if !in_graveyard_context {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::PUT_INTO_GY_FROM_LIBRARY_THIS_TURN);
                    }
                    "from among cards in graveyards that were put there from libraries this turn" =>
                    {
                        if !zone_compatible_with_graveyard {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::PUT_INTO_GY_FROM_LIBRARY_THIS_TURN);
                    }
                    "that were put there from anywhere this turn"
                    | "that was put there this turn"
                    | "that were put there this turn" => {
                        if !in_graveyard_context {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::PUT_INTO_GY_THIS_TURN);
                    }
                    "adventure" => {
                        if clause.subtypes != SubtypeFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.subtypes = SubtypeFilter::Spell(SpellSubtype::Adventure);
                    }
                    "that has an adventure" | "that have an adventure" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_ADVENTURE);
                    }
                    "that didn't attack this turn" => {
                        if clause.flags.contains(ObjectFilterFlags::ATTACKED_THIS_TURN) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN);
                    }
                    "that blocked this turn" | "blocked this turn" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::BLOCKED_THIS_TURN);
                    }
                    "paired with ~"
                    | "that's paired with ~"
                    | "that is paired with ~"
                    | "paired with it"
                    | "that's paired with it" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::PAIRED_WITH_SELF);
                    }
                    "blocking enchanted creature"
                    | "that's blocking enchanted creature"
                    | "that is blocking enchanted creature"
                    | "blocking equipped creature"
                    | "that's blocking equipped creature"
                    | "that is blocking equipped creature"
                    | "blocking attached creature" => {
                        set_clause_combat_filter(
                            &mut clause.combat,
                            CombatFilter::BlockingAttachedHost,
                        )?;
                    }
                    "kicked" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::KICKED);
                    }
                    "renowned" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::RENOWNED);
                    }
                    "saddled" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::SADDLED);
                    }
                    "monstrous" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::MONSTROUS);
                    }
                    "suspected" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::SUSPECTED);
                    }
                    "goaded" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::GOADED);
                    }
                    "solved" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::SOLVED);
                    }
                    "prepared" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::PREPARED);
                    }
                    "exerted" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::EXERTED);
                    }
                    "that's not historic" | "not historic" => {
                        if clause.flags.contains(ObjectFilterFlags::HISTORIC) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::NON_HISTORIC);
                    }
                    "with power less than ~'s"
                    | "with power less than ~'s power"
                    | "with lesser power than ~" => {
                        if clause.power != PowerFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.power = PowerFilter::CompareAnchor {
                            op: Comparison::Less,
                            anchor: DynamicStatAnchor::SourcePower,
                        };
                    }
                    "with lesser mana value than the creature that died"
                    | "with mana value less than the creature that died"
                    | "with mana value less than the creature that died's" => {
                        if clause.mana_value != ManaValueFilter::Any || mana_value_pair.is_some() {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.mana_value = ManaValueFilter::CompareAnchor {
                            op: Comparison::Less,
                            anchor: DynamicStatAnchor::TriggerSubjectManaValue,
                        };
                    }
                    "you own" => {
                        if clause.owner != ControllerFilter::Any
                            || matches!(
                                clause.controller,
                                ControllerFilter::OwnerYou | ControllerFilter::OwnerOpponent
                            )
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.owner = ControllerFilter::You;
                    }
                    "exiled with ~" | "that was exiled with ~" | "that were exiled with ~" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
                    }
                    "suspended" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::SUSPENDED);
                    }
                    "with an activated ability"
                    | "that has an activated ability"
                    | "that have activated abilities" => {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::NO_ACTIVATED_ABILITIES)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_ACTIVATED_ABILITY);
                    }
                    "without activated abilities"
                    | "with no activated abilities"
                    | "that doesn't have an activated ability"
                    | "that don't have activated abilities" => {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::HAS_ACTIVATED_ABILITY)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::NO_ACTIVATED_ABILITIES);
                    }
                    "with power equal to its toughness"
                    | "whose power is equal to its toughness" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::SAME_TOTAL_PT);
                    }
                    "that shares a color with ~" | "that share a color with ~" => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::SHARES_COLOR_WITH_SOURCE);
                    }
                    "that shares a creature type with ~" | "that share a creature type with ~" => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_SOURCE);
                    }
                    "that shares a card type with ~" | "that share a card type with ~" => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::SHARES_CARD_TYPE_WITH_SOURCE);
                    }
                    "that don't have the same name as ~"
                    | "that doesn't have the same name as ~"
                    | "with a different name than ~" => {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::SAME_NAME_AS_SOURCE)
                            || matches!(
                                clause.source_relation,
                                SourceRelation::SharingNameWithAbilitySource
                                    | SourceRelation::OtherThanAbilitySourceSharingName
                            )
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE);
                    }
                    "you own and control" => {
                        if !matches!(
                            clause.controller,
                            ControllerFilter::Any | ControllerFilter::You
                        ) || !matches!(
                            clause.owner,
                            ControllerFilter::Any | ControllerFilter::You
                        ) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.controller = ControllerFilter::You;
                        clause.owner = ControllerFilter::You;
                    }
                    "you neither own nor control" => {
                        if !matches!(
                            clause.controller,
                            ControllerFilter::Any | ControllerFilter::NotYou
                        ) || !matches!(
                            clause.owner,
                            ControllerFilter::Any | ControllerFilter::NotYou
                        ) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.controller = ControllerFilter::NotYou;
                        clause.owner = ControllerFilter::NotYou;
                    }
                    "they own" => {
                        if !matches!(
                            clause.owner,
                            ControllerFilter::Any | ControllerFilter::RelatedPlayer
                        ) || matches!(
                            clause.controller,
                            ControllerFilter::OwnerYou | ControllerFilter::OwnerOpponent
                        ) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.owner = ControllerFilter::RelatedPlayer;
                    }
                    "they own that you control" => {
                        if !matches!(
                            clause.owner,
                            ControllerFilter::Any | ControllerFilter::RelatedPlayer
                        ) || !matches!(
                            clause.controller,
                            ControllerFilter::Any | ControllerFilter::You
                        ) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.owner = ControllerFilter::RelatedPlayer;
                        clause.controller = ControllerFilter::You;
                    }
                    "exiled this way"
                    | "that was exiled this way"
                    | "that were exiled this way"
                    | "discarded this way"
                    | "you discarded this way"
                    | "destroyed this way"
                    | "that were destroyed this way"
                    | "put into a graveyard this way"
                    | "that was put into a graveyard this way"
                    | "that were put into a graveyard this way"
                    | "put into graveyards this way"
                    | "you put into your graveyard this way"
                    | "milled this way"
                    | "they milled this way"
                    | "returned this way" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::IN_LAST_MOVED);
                    }
                    "with power 3 or greater" => {
                        if clause.power != PowerFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.power = PowerFilter::AtLeast(3);
                    }
                    "with power less than ~'s toughness" => {
                        if clause.power != PowerFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.power = PowerFilter::CompareAnchor {
                            op: Comparison::Less,
                            anchor: DynamicStatAnchor::SourceToughness,
                        };
                    }
                    "with toughness less than ~'s" | "with toughness less than ~'s toughness" => {
                        if clause.toughness != ToughnessFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.toughness = ToughnessFilter::CompareAnchor {
                            op: Comparison::Less,
                            anchor: DynamicStatAnchor::SourceToughness,
                        };
                    }
                    "with lesser mana value than ~"
                    | "with mana value less than ~'s"
                    | "with mana value less than ~'s mana value" => {
                        if clause.mana_value != ManaValueFilter::Any || mana_value_pair.is_some() {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.mana_value = ManaValueFilter::CompareAnchor {
                            op: Comparison::Less,
                            anchor: DynamicStatAnchor::SourceManaValue,
                        };
                    }
                    "has_adventure" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_ADVENTURE);
                    }
                    "same_total_pt" | "whose power and toughness are equal" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::SAME_TOTAL_PT);
                    }
                    "power_greater_than_toughness"
                    | "with power greater than its toughness"
                    | "with power greater than their toughness"
                    | "whose power is greater than its toughness" => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::POWER_GREATER_THAN_TOUGHNESS);
                    }
                    "toughness_greater_than_power"
                    | "with toughness greater than its power"
                    | "with toughness greater than their power"
                    | "whose toughness is greater than its power" => {
                        clause.flags = clause
                            .flags
                            .union(ObjectFilterFlags::TOUGHNESS_GREATER_THAN_POWER);
                    }
                    "that's one or more colors" | "that are one or more colors" => {
                        if clause.color != ObjectColorFilter::Any
                            && clause.color != ObjectColorFilter::Colored
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.color = ObjectColorFilter::Colored;
                    }
                    "with mana value 1 or 2" => {
                        if clause.mana_value != ManaValueFilter::Any
                            || mana_value_pair.is_some()
                            || any_of_pair.is_some()
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        mana_value_pair = Some((1, 2));
                    }
                    "that's a cleric, rogue, warrior, and/or wizard" => {
                        if clause.subtypes != SubtypeFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        use spellwake_engine::program::{SingleSubtype, SubtypeList};
                        let roles = resolve_party_roles(vocabulary)
                            .ok_or("oracle:unsupported-filter-predicate")?;
                        let atoms = [
                            SingleSubtype::Creature(roles[0]),
                            SingleSubtype::Creature(roles[1]),
                            SingleSubtype::Creature(roles[2]),
                            SingleSubtype::Creature(roles[3]),
                        ];
                        let list = SubtypeList::from_slice(&atoms)
                            .ok_or("oracle:unsupported-filter-predicate")?;
                        clause.subtypes = SubtypeFilter::AnyOfList(list);
                    }
                    "that are instant cards, sorcery cards, and/or have an adventure"
                    | "that are instant cards, are sorcery cards, and/or have an adventure" => {
                        if any_of_pair.is_some() || mana_value_pair.is_some() {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        any_of_pair = Some((
                            ObjectClause::any()
                                .any_types(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY)),
                            ObjectClause::any().with_flags(ObjectFilterFlags::HAS_ADVENTURE),
                        ));
                    }
                    "that are oozes or are named ~" => {
                        if any_of_pair.is_some() || mana_value_pair.is_some() {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        let ooze = vocabulary
                            .resolve_label("Ooze")
                            .ok_or("oracle:unsupported-filter-predicate")?;
                        any_of_pair = Some((
                            ObjectClause::any().with_subtype(ooze),
                            ObjectClause::any().with_flags(ObjectFilterFlags::SAME_NAME_AS_SOURCE),
                        ));
                    }
                    "with no abilities" | "that has no abilities" | "that have no abilities" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::NO_ABILITIES);
                    }
                    "face down" | "face-down" | "that's face down" | "that is face down"
                    | "that are face down" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_DOWN);
                    }
                    "face up" | "face-up" | "that's face up" | "that is face up"
                    | "that are face up" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_UP);
                    }
                    "that's historic" | "historic" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::HISTORIC);
                    }
                    "that's a commander" | "commander" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::COMMANDER_ONLY);
                    }
                    "that isn't a commander" | "that aren't commanders" | "noncommander" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::NOT_COMMANDER);
                    }
                    "with even mana value" | "with an even mana value" => {
                        if clause.mana_value == ManaValueFilter::Odd
                            || clause.flags.contains(ObjectFilterFlags::ODD_MANA_VALUE)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::EVEN_MANA_VALUE);
                    }
                    "with odd mana value" | "with an odd mana value" => {
                        if clause.mana_value == ManaValueFilter::Even
                            || clause.flags.contains(ObjectFilterFlags::EVEN_MANA_VALUE)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::ODD_MANA_VALUE);
                    }
                    "with a basic land type"
                    | "that has a basic land type"
                    | "that have a basic land type" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_BASIC_LAND_TYPE);
                    }
                    "that's toxic" | "with toxic" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_TOXIC);
                    }
                    "that has a +1/+1 counter on it"
                    | "that have a +1/+1 counter on them"
                    | "with a +1/+1 counter on them"
                    | "with +1/+1 counters on them"
                    | "with one or more +1/+1 counters on it" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER);
                    }
                    "that has no counters on it"
                    | "that have no counters on them"
                    | "without counters on them"
                    | "with no counters on them"
                    | "that doesn't have a counter on it"
                    | "that don't have counters on them" => {
                        clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                    }
                    "with the same name as ~"
                    | "that has the same name as ~"
                    | "that have the same name as ~" => {
                        if clause
                            .flags
                            .contains(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE)
                        {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_SOURCE);
                    }
                    other
                        if other.starts_with("with power or toughness ")
                            && other.ends_with(" or less") =>
                    {
                        let num_s = other
                            .trim_start_matches("with power or toughness ")
                            .trim_end_matches(" or less")
                            .trim();
                        let max_pt = num_s
                            .parse::<u16>()
                            .map_err(|_| "oracle:unsupported-filter-predicate")?;
                        if clause.power != PowerFilter::Any {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        clause.power = PowerFilter::PowerOrToughnessAtMost(max_pt);
                    }
                    other => {
                        if let Some(ck_raw) = extract_state_qual_counter_kind(other) {
                            let ck = ck_raw.trim().to_ascii_lowercase();
                            match ck.as_str() {
                                "+1/+1" => {
                                    if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
                                        || clause.forbidden_counter_kind
                                            == Some(CounterKind::PlusOne)
                                    {
                                        return Err("oracle:unsupported-filter-predicate");
                                    }
                                    clause.flags = clause
                                        .flags
                                        .union(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER);
                                }
                                "-1/-1" => {
                                    if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
                                        || clause.forbidden_counter_kind
                                            == Some(CounterKind::MinusOne)
                                    {
                                        return Err("oracle:unsupported-filter-predicate");
                                    }
                                    clause.flags = clause
                                        .flags
                                        .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER);
                                }
                                "" | "any" | "a" | "one or more" => {
                                    if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS) {
                                        return Err("oracle:unsupported-filter-predicate");
                                    }
                                    clause.flags =
                                        clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
                                }
                                kind_str => {
                                    let kind = parse_counter_kind(kind_str)
                                        .filter(|k| k.is_permanent())
                                        .ok_or("oracle:unsupported-filter-predicate")?;
                                    if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
                                        || clause.forbidden_counter_kind == Some(kind)
                                        || clause
                                            .required_counter_kind
                                            .is_some_and(|prev| prev != kind)
                                    {
                                        return Err("oracle:unsupported-filter-predicate");
                                    }
                                    clause.required_counter_kind = Some(kind);
                                }
                            }
                        } else if let Some(body) = other
                            .strip_prefix("without ")
                            .or_else(|| other.strip_prefix("that doesn't have "))
                            .or_else(|| other.strip_prefix("that don't have "))
                            .and_then(|s| {
                                s.strip_suffix(" on it")
                                    .or_else(|| s.strip_suffix(" on them"))
                            })
                            .and_then(|s| {
                                s.strip_suffix(" counters")
                                    .or_else(|| s.strip_suffix(" counter"))
                            })
                        {
                            let ck = body
                                .strip_prefix("any ")
                                .or_else(|| body.strip_prefix("a "))
                                .or_else(|| body.strip_prefix("an "))
                                .unwrap_or(body)
                                .trim();
                            if matches!(ck, "" | "any" | "a" | "one or more") {
                                if clause.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER)
                                    || clause
                                        .flags
                                        .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
                                    || clause
                                        .flags
                                        .contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
                                    || clause.required_counter_kind.is_some()
                                {
                                    return Err("oracle:unsupported-filter-predicate");
                                }
                                clause.flags =
                                    clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                            } else {
                                let kind = parse_counter_kind(ck)
                                    .filter(|k| k.is_permanent())
                                    .ok_or("oracle:unsupported-filter-predicate")?;
                                if clause.required_counter_kind == Some(kind)
                                    || (kind == CounterKind::PlusOne
                                        && clause
                                            .flags
                                            .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER))
                                    || (kind == CounterKind::MinusOne
                                        && clause
                                            .flags
                                            .contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER))
                                    || clause
                                        .forbidden_counter_kind
                                        .is_some_and(|prev| prev != kind)
                                {
                                    return Err("oracle:unsupported-filter-predicate");
                                }
                                clause.forbidden_counter_kind = Some(kind);
                            }
                        } else if let Some(rest) = other.strip_prefix("with ")
                            && let Some(kw) = lower_keyword_ability_name(rest)
                            && clause.keyword.is_none_or(|prev| prev == kw)
                            && clause.forbidden_keyword != Some(kw)
                        {
                            clause.keyword = Some(kw);
                        } else if let Some(rest) = other.strip_prefix("without ")
                            && let Some(kw) = lower_keyword_ability_name(rest)
                            && clause.forbidden_keyword.is_none_or(|prev| prev == kw)
                            && clause.keyword != Some(kw)
                        {
                            clause.forbidden_keyword = Some(kw);
                        } else {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                    }
                }
            }
            _ => return Err("oracle:unsupported-filter-predicate"),
        }
    }

    let resolve_single_subtype_atom =
        |st: &str| -> Option<spellwake_engine::program::SingleSubtype> {
            use spellwake_engine::program::SingleSubtype;
            if f.any_types == AstCardTypeMask::PLANESWALKER
                && let Some(pw) = parse_planeswalker_subtype(st)
            {
                Some(SingleSubtype::Planeswalker(pw))
            } else if st.eq_ignore_ascii_case("aura") {
                Some(SingleSubtype::Aura)
            } else if let Some(ct) = vocabulary.resolve_label(st) {
                Some(SingleSubtype::Creature(ct))
            } else if let Some(lt) = parse_land_subtype(st) {
                Some(SingleSubtype::Land(lt))
            } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
                Some(SingleSubtype::Artifact(at))
            } else if let Some(et) = EnchantmentSubtype::from_wire_label(st) {
                Some(SingleSubtype::Enchantment(et))
            } else {
                match st.trim().to_ascii_lowercase().as_str() {
                    "arcane" => Some(SingleSubtype::Spell(SpellSubtype::Arcane)),
                    "lesson" => Some(SingleSubtype::Spell(SpellSubtype::Lesson)),
                    "adventure" => Some(SingleSubtype::Spell(SpellSubtype::Adventure)),
                    "trap" => Some(SingleSubtype::Spell(SpellSubtype::Trap)),
                    "omen" => Some(SingleSubtype::Spell(SpellSubtype::Omen)),
                    _ => parse_planeswalker_subtype(st).map(SingleSubtype::Planeswalker),
                }
            }
        };

    // Forbidden subtypes
    let mut forb_creature_types = Vec::new();
    let mut forb_land_types = Vec::new();
    let mut forb_other_atoms = Vec::new();
    for fst in f
        .forbidden_subtypes
        .iter()
        .map(String::as_str)
        .chain(extra_forbidden_subtypes)
    {
        if fst.eq_ignore_ascii_case("aura") {
            clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
        } else if fst.eq_ignore_ascii_case("saga") {
            clause.flags = clause.flags.union(ObjectFilterFlags::NON_SAGA);
        } else if fst.eq_ignore_ascii_case("outlaw") {
            let outlaws = resolve_outlaw_creature_types(vocabulary)
                .ok_or("oracle:unsupported-forbidden-subtype")?;
            forb_creature_types.extend(outlaws);
        } else if let Some(ct) = vocabulary.resolve_label(fst) {
            forb_creature_types.push(ct);
        } else if let Some(lt) = parse_land_subtype(fst) {
            forb_land_types.push(lt);
        } else if let Some(atom) = resolve_single_subtype_atom(fst) {
            forb_other_atoms.push(atom);
        } else {
            return Err("oracle:unsupported-forbidden-subtype");
        }
    }
    let has_mixed_forbidden = (!forb_other_atoms.is_empty())
        || (!forb_land_types.is_empty() && !forb_creature_types.is_empty())
        || forb_creature_types.len() > 3;
    if has_mixed_forbidden {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:multi-subtype-filter");
        }
        let mut all_forb = Vec::with_capacity(
            forb_creature_types.len() + forb_land_types.len() + forb_other_atoms.len(),
        );
        all_forb.extend(
            forb_creature_types
                .into_iter()
                .map(spellwake_engine::program::SingleSubtype::Creature),
        );
        all_forb.extend(
            forb_land_types
                .into_iter()
                .map(spellwake_engine::program::SingleSubtype::Land),
        );
        all_forb.extend(forb_other_atoms);
        let list = spellwake_engine::program::SubtypeList::from_slice(&all_forb)
            .ok_or("oracle:too-many-subtypes")?;
        clause.subtypes = SubtypeFilter::NoneOfList(list);
    } else if !forb_land_types.is_empty() {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:multi-subtype-filter");
        }
        clause.subtypes = match forb_land_types.as_slice() {
            [lt] => SubtypeFilter::NotLand(*lt),
            many => {
                let mask = many.iter().copied().fold(0u32, |acc, lt| acc | lt.bit());
                SubtypeFilter::NotLandMask(mask)
            }
        };
    } else if !forb_creature_types.is_empty() {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:multi-subtype-filter");
        }
        clause.subtypes = match forb_creature_types.as_slice() {
            [a] => SubtypeFilter::Lacks(*a),
            [a, b] => SubtypeFilter::NoneOf([*a, *b, *b]),
            [a, b, c] => SubtypeFilter::NoneOf([*a, *b, *c]),
            _ => unreachable!(),
        };
    }

    // Positive subtypes
    let resolve_single_subtype = |st: &str| -> Option<SubtypeFilter> {
        use spellwake_engine::program::SingleSubtype;
        resolve_single_subtype_atom(st).map(|atom| match atom {
            SingleSubtype::Creature(ct) => SubtypeFilter::Has(ct),
            SingleSubtype::Land(lt) => SubtypeFilter::Land(lt),
            SingleSubtype::Artifact(at) => SubtypeFilter::Artifact(at),
            SingleSubtype::Enchantment(et) => SubtypeFilter::Enchantment(et),
            SingleSubtype::Planeswalker(pw) => SubtypeFilter::Planeswalker(pw),
            SingleSubtype::Spell(sp) => SubtypeFilter::Spell(sp),
            SingleSubtype::Aura => SubtypeFilter::Aura,
        })
    };
    if f.subtypes.len() == 1 {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:mixed-subtype-filter");
        }
        if f.subtypes[0].eq_ignore_ascii_case("outlaw") {
            let outlaws = resolve_outlaw_creature_types(vocabulary)
                .ok_or("oracle:unsupported-subtype-filter")?;
            let atoms: Vec<_> = outlaws
                .into_iter()
                .map(spellwake_engine::program::SingleSubtype::Creature)
                .collect();
            let list = spellwake_engine::program::SubtypeList::from_slice(&atoms)
                .ok_or("oracle:too-many-subtypes")?;
            clause.subtypes = SubtypeFilter::AnyOfList(list);
        } else {
            clause.subtypes = resolve_single_subtype(&f.subtypes[0])
                .ok_or("oracle:unsupported-subtype-filter")?;
        }
    } else if f.subtypes.len() >= 2 {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:mixed-subtype-filter");
        }
        if !subtypes_match_any {
            // Stage 1 currently drops `"and"` between plural subtype lists (`Mounts and Vehicles`,
            // `Auras and Equipment`) without emitting `SubtypesMatchAny`, making them
            // indistinguishable in the AST from conjunctive compound subtypes (`Eldrazi Spawn`,
            // `Spider Hero`) or two-object costs (`a Swamp and a Forest`). Fail closed until
            // Stage 1 explicitly distinguishes disjunction from conjunction.
            return Err("oracle:unsupported-multi-subtype");
        } else if let Some(land_mask) = f.subtypes.iter().try_fold(0u32, |acc, st| {
            parse_land_subtype(st).map(|lt| acc | lt.bit())
        }) {
            clause.subtypes = SubtypeFilter::AnyLandMask(land_mask);
        } else if f.subtypes.len() == 3
            && let (Some(a), Some(b), Some(c)) = (
                vocabulary.resolve_label(&f.subtypes[0]),
                vocabulary.resolve_label(&f.subtypes[1]),
                vocabulary.resolve_label(&f.subtypes[2]),
            )
        {
            clause.subtypes = SubtypeFilter::AnyOf([a, b, c]);
        } else {
            let mut atoms = Vec::with_capacity(f.subtypes.len());
            for st in &f.subtypes {
                if st.eq_ignore_ascii_case("outlaw") {
                    let outlaws = resolve_outlaw_creature_types(vocabulary)
                        .ok_or("oracle:unsupported-multi-subtype")?;
                    atoms.extend(
                        outlaws
                            .into_iter()
                            .map(spellwake_engine::program::SingleSubtype::Creature),
                    );
                } else {
                    atoms.push(
                        resolve_single_subtype_atom(st)
                            .ok_or("oracle:unsupported-multi-subtype")?,
                    );
                }
            }
            let list = spellwake_engine::program::SubtypeList::from_slice(&atoms)
                .ok_or("oracle:too-many-subtypes")?;
            clause.subtypes = SubtypeFilter::AnyOfList(list);
        }
    }

    if let Some((kw_a, kw_b)) = keyword_pair {
        if clause.keyword.is_some() || mana_value_pair.is_some() || any_of_pair.is_some() {
            return Err("oracle:multi-keyword-filter");
        }
        let mut c1 = clause;
        let mut c2 = clause;
        c1.keyword = Some(kw_a);
        c2.keyword = Some(kw_b);
        return Ok(ObjectFilter::either(c1, c2));
    }

    if let Some((mv_a, mv_b)) = mana_value_pair {
        if any_of_pair.is_some() {
            return Err("oracle:unsupported-filter-predicate");
        }
        let mut c1 = clause;
        let mut c2 = clause;
        c1.mana_value = ManaValueFilter::Equal(mv_a);
        c2.mana_value = ManaValueFilter::Equal(mv_b);
        return Ok(ObjectFilter::either(c1, c2));
    }

    if let Some((b1, b2)) = any_of_pair {
        let c1 = intersect_object_clauses(clause, b1)?;
        let c2 = intersect_object_clauses(clause, b2)?;
        return Ok(ObjectFilter::either(c1, c2));
    }

    Ok(ObjectFilter::Clause(clause))
}

pub(super) fn lower_spell_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SpellFilter, &'static str> {
    use spellwake_engine::program::SpellCastZoneFilter;
    let mut cast_zone = match f.zone {
        None | Some(ZoneFilter::Stack) => SpellCastZoneFilter::Any,
        Some(ZoneFilter::YourLibrary) => SpellCastZoneFilter::OwnLibrary,
        Some(
            ZoneFilter::YourGraveyard
            | ZoneFilter::OpponentGraveyard
            | ZoneFilter::OpponentsGraveyards
            | ZoneFilter::AnyGraveyard
            | ZoneFilter::AllGraveyards
            | ZoneFilter::ThatPlayerGraveyard
            | ZoneFilter::TargetPlayerGraveyard,
        ) => SpellCastZoneFilter::Graveyard,
        Some(ZoneFilter::Exile) if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0 => {
            SpellCastZoneFilter::LinkedExile
        }
        Some(ZoneFilter::Exile) => SpellCastZoneFilter::Exile,
        Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => SpellCastZoneFilter::Hand,
        _ => return Err("oracle:spell-filter-zone"),
    };
    let mut ordinal_this_turn: Option<NonZeroU32> = None;
    let mut stripped_spell_pred = false;
    let mut remaining_predicates = Vec::with_capacity(f.predicates.len());
    for pred in &f.predicates {
        match pred {
            FilterPredicateAst::NthSpellCastThisTurn(n) => {
                let ord =
                    NonZeroU32::new(u32::from(*n)).ok_or("oracle:unsupported-filter-predicate")?;
                if ordinal_this_turn.is_some_and(|prev| prev != ord) {
                    return Err("oracle:unsupported-filter-predicate");
                }
                ordinal_this_turn = Some(ord);
                stripped_spell_pred = true;
            }
            FilterPredicateAst::StateQualification(raw) => {
                let norm = raw.trim().to_ascii_lowercase();
                match norm.as_str() {
                    "cast from a graveyard"
                    | "cast from your graveyard"
                    | "cast from any graveyard"
                    | "cast_from_graveyard" => {
                        if !matches!(
                            cast_zone,
                            SpellCastZoneFilter::Any | SpellCastZoneFilter::Graveyard
                        ) {
                            return Err("oracle:spell-filter-zone");
                        }
                        cast_zone = SpellCastZoneFilter::Graveyard;
                        stripped_spell_pred = true;
                    }
                    "cast from exile" => {
                        if !matches!(
                            cast_zone,
                            SpellCastZoneFilter::Any | SpellCastZoneFilter::Exile
                        ) {
                            return Err("oracle:spell-filter-zone");
                        }
                        cast_zone = SpellCastZoneFilter::Exile;
                        stripped_spell_pred = true;
                    }
                    "exiled with ~"
                    | "cast from among cards exiled with ~"
                    | "cast from among the cards exiled with ~" => {
                        if !matches!(
                            cast_zone,
                            SpellCastZoneFilter::Any
                                | SpellCastZoneFilter::Exile
                                | SpellCastZoneFilter::LinkedExile
                        ) {
                            return Err("oracle:spell-filter-zone");
                        }
                        cast_zone = SpellCastZoneFilter::LinkedExile;
                        stripped_spell_pred = true;
                    }
                    "cast from your library" => {
                        if !matches!(
                            cast_zone,
                            SpellCastZoneFilter::Any | SpellCastZoneFilter::OwnLibrary
                        ) {
                            return Err("oracle:spell-filter-zone");
                        }
                        cast_zone = SpellCastZoneFilter::OwnLibrary;
                        stripped_spell_pred = true;
                    }
                    "cast from your hand" | "cast from a hand" => {
                        if !matches!(
                            cast_zone,
                            SpellCastZoneFilter::Any | SpellCastZoneFilter::Hand
                        ) {
                            return Err("oracle:spell-filter-zone");
                        }
                        cast_zone = SpellCastZoneFilter::Hand;
                        stripped_spell_pred = true;
                    }
                    "that wasn't cast from its owner's hand"
                    | "cast from outside hand"
                    | "cast from anywhere other than your hand" => {
                        if !matches!(
                            cast_zone,
                            SpellCastZoneFilter::Any | SpellCastZoneFilter::NotOwnersHand
                        ) {
                            return Err("oracle:spell-filter-zone");
                        }
                        cast_zone = SpellCastZoneFilter::NotOwnersHand;
                        stripped_spell_pred = true;
                    }
                    "that's the second spell cast this turn" => {
                        let ord =
                            NonZeroU32::new(2).ok_or("oracle:unsupported-filter-predicate")?;
                        if ordinal_this_turn.is_some_and(|prev| prev != ord) {
                            return Err("oracle:unsupported-filter-predicate");
                        }
                        ordinal_this_turn = Some(ord);
                        stripped_spell_pred = true;
                    }
                    _ => remaining_predicates.push(pred.clone()),
                }
            }
            _ => remaining_predicates.push(pred.clone()),
        }
    }
    let single_target = (f.flags.0 & AstFilterFlags::SINGLE_TARGET.0) != 0;
    let chosen_entry_type = (f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_TYPE.0) != 0;
    if chosen_entry_type {
        let mut stripped = f.clone();
        stripped.flags.0 &=
            !(AstFilterFlags::SINGLE_TARGET.0 | AstFilterFlags::MATCHES_CHOSEN_TYPE.0);
        stripped.zone = None;
        if stripped_spell_pred {
            stripped.predicates = remaining_predicates.clone();
        }
        if (stripped.any_types.is_empty() || stripped.any_types == AstCardTypeMask::CREATURE)
            && filter_only_has(
                &stripped,
                FilterAllow {
                    types: true,
                    ..FilterAllow::NONE
                },
            )
        {
            let mut sf = SpellFilter::any().of_chosen_entry_type();
            sf.cast_zone = cast_zone;
            sf.ordinal_this_turn = ordinal_this_turn;
            if single_target {
                sf.target_shape = spellwake_engine::program::SpellTargetShape::SingleTarget;
            }
            return Ok(sf);
        }
    }
    let obj = if single_target
        || f.zone.is_some()
        || stripped_spell_pred
        || cast_zone == SpellCastZoneFilter::LinkedExile
    {
        let mut stripped = f.clone();
        stripped.flags.0 &= !AstFilterFlags::SINGLE_TARGET.0;
        if cast_zone == SpellCastZoneFilter::LinkedExile {
            stripped.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
        }
        stripped.zone = None;
        if stripped_spell_pred {
            stripped.predicates = remaining_predicates;
        }
        lower_object_filter(&stripped, vocabulary)?
    } else {
        lower_object_filter(f, vocabulary)?
    };
    let mut sf = SpellFilter::from_object(obj);
    sf.cast_zone = cast_zone;
    sf.ordinal_this_turn = ordinal_this_turn;
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

fn peel_battlefield_subject(mut subject: &SubjectRefAst) -> &SubjectRefAst {
    while let SubjectRefAst::InZone {
        subject: inner,
        zone: ZoneFilter::Battlefield,
    } = subject
    {
        subject = inner.as_ref();
    }
    subject
}

fn try_bind_target_object_filter(filter: &ObjectFilterAst) -> Option<BodyTarget> {
    if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
        || !filter.extra_zones.is_empty()
        || filter.owner.is_some()
        || filter.any_spell
        || filter.is_ability
        || filter.named.is_some()
        || !filter.subtypes.is_empty()
        || !filter
            .predicates
            .iter()
            .all(|p| matches!(p, FilterPredicateAst::TypesMatchAll))
        || !filter.stat_bounds.is_empty()
    {
        return None;
    }
    let mut non_aura_only = false;
    if !filter.forbidden_subtypes.is_empty() {
        if filter.forbidden_subtypes.len() == 1
            && filter.forbidden_subtypes[0].eq_ignore_ascii_case("aura")
        {
            non_aura_only = true;
        } else {
            return None;
        }
    }
    if filter_only_has(
        filter,
        FilterAllow {
            types: true,
            zone: true,
            ..FilterAllow::NONE
        },
    ) {
        if filter.any_types == AstCardTypeMask::CREATURE {
            return Some(BodyTarget::SingleCreature);
        }
        if filter.any_types == AstCardTypeMask::PLANESWALKER {
            return Some(BodyTarget::SinglePlaneswalker);
        }
        if filter.any_types.is_empty() && filter.any_permanent {
            return Some(BodyTarget::SinglePermanent);
        }
    }
    if (filter.flags.0 & !(AstFilterFlags::LKI_PAST_TENSE.0 | AstFilterFlags::EXCLUDE_SELF.0)) != 0
        || !filter.required_supertypes.is_empty()
        || !filter.forbidden_supertypes.is_empty()
        || filter.required_keywords.len() > 1
        || filter.forbidden_keywords.len() > 1
        || filter.counter_kind.is_some()
    {
        return None;
    }
    let mut clause = ObjectClause::any();
    if non_aura_only {
        clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
    }
    if (filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0 {
        clause.source_relation = SourceRelation::OtherThanAbilitySource;
    }
    if filter
        .predicates
        .iter()
        .any(|p| matches!(p, FilterPredicateAst::TypesMatchAll))
    {
        let req_types = lower_card_type_mask(filter.any_types).ok()?;
        clause.required_types = req_types;
        clause.any_types = canonical_any_type_for_required(req_types);
    } else {
        clause.any_types = lower_card_type_mask(filter.any_types).ok()?;
    }
    clause.forbidden_types = lower_card_type_mask(filter.forbidden_types).ok()?;
    if (clause.required_types.bits() & clause.forbidden_types.bits()) != 0 {
        return None;
    }
    clause.color = if let Some(arity) = filter.color_arity {
        if !filter.any_colors.is_empty() || !filter.forbidden_colors.is_empty() {
            return None;
        }
        match arity {
            ColorArity::Colorless => ObjectColorFilter::Colorless,
            ColorArity::Colored => ObjectColorFilter::Colored,
            ColorArity::Multicolored => ObjectColorFilter::Multicolored,
            ColorArity::Monocolored => ObjectColorFilter::Monocolored,
        }
    } else if !filter.any_colors.is_empty() || !filter.forbidden_colors.is_empty() {
        ObjectColorFilter::Mask {
            any_of: lower_color_mask(filter.any_colors),
            none_of: lower_color_mask(filter.forbidden_colors),
        }
    } else {
        ObjectColorFilter::Any
    };
    clause.controller = match filter.controller {
        None => ControllerFilter::Any,
        Some(ControllerRelation::You | ControllerRelation::YourTeam) => ControllerFilter::You,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            ControllerFilter::Opponent
        }
        Some(ControllerRelation::NotYou) => ControllerFilter::NotYou,
        _ => return None,
    };
    clause.combat = match filter.combat {
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
    clause.tapped = filter.tapped;
    clause.token = filter.token;
    if let Some(k) = filter.required_keywords.first() {
        clause.keyword = Some(lower_keyword_ability_name(k)?);
    }
    if let Some(k) = filter.forbidden_keywords.first() {
        clause.forbidden_keyword = Some(lower_keyword_ability_name(k)?);
    }
    if (clause.keyword.is_some() && clause.keyword == clause.forbidden_keyword)
        || (clause == ObjectClause::any() && !filter.any_permanent)
    {
        return None;
    }
    Some(BodyTarget::FilteredPermanent(ObjectFilter::Clause(clause)))
}

fn target_filter_matches_body_target(
    filter: &ObjectFilterAst,
    target: BodyTarget,
    vocabulary: &CreatureTypeVocabulary,
) -> bool {
    match target {
        BodyTarget::SingleCreature => {
            matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.any_spell
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
        }
        BodyTarget::SinglePlaneswalker => {
            matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.any_spell
                && filter.any_types == AstCardTypeMask::PLANESWALKER
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
        }
        BodyTarget::SinglePermanent => {
            matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.any_spell
                && filter.any_permanent
                && filter_only_has(
                    filter,
                    FilterAllow {
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
        }
        BodyTarget::FilteredPermanent(expected) => {
            matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.any_spell
                && lower_object_filter(filter, vocabulary).ok() == Some(expected)
        }
        BodyTarget::SingleSpell(expected) => {
            filter.any_spell && lower_spell_filter(filter, vocabulary).ok() == Some(expected)
        }
        _ => false,
    }
}

fn bind_condition_value_expr_targets(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
) -> Result<(), &'static str> {
    match cond {
        ConditionAst::Not(inner) => bind_condition_value_expr_targets(inner, builder),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            bind_condition_value_expr_targets(a, builder)?;
            bind_condition_value_expr_targets(b, builder)
        }
        ConditionAst::Compare { lhs, rhs, .. } | ConditionAst::ExprCompare { lhs, rhs, .. } => {
            bind_value_expr_targets(lhs, builder)?;
            bind_value_expr_targets(rhs, builder)
        }
        ConditionAst::LifeCompare { player, .. }
        | ConditionAst::HandCount { player, .. }
        | ConditionAst::GraveyardCount { player, .. }
        | ConditionAst::ControlsCount { player, .. } => match player {
            PlayerRefAst::TargetPlayer => builder.bind_target(BodyTarget::SinglePlayer),
            PlayerRefAst::TargetOpponent => builder.bind_target(BodyTarget::SingleOpponent),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

pub(super) fn bind_value_expr_targets(
    expr: &ValueExprAst,
    builder: &mut BodyBuilder,
) -> Result<(), &'static str> {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = peel_battlefield_subject(subject)
                && let Some(bt) = try_bind_target_object_filter(filter)
            {
                builder.bind_target(bt)?;
            }
            Ok(())
        }
        ValueExprAst::Count(f) => {
            match f.zone {
                Some(
                    ZoneFilter::TargetPlayerGraveyard
                    | ZoneFilter::TargetPlayerHand
                    | ZoneFilter::TargetPlayerLibrary,
                ) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                }
                Some(
                    ZoneFilter::TargetOpponentGraveyard
                    | ZoneFilter::TargetOpponentHand
                    | ZoneFilter::TargetOpponentLibrary,
                ) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                }
                _ => {}
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
        ValueExprAst::AggregateStat { filter, .. }
        | ValueExprAst::ExtremeStatAmong { filter, .. }
        | ValueExprAst::ColorsAmong(filter) => {
            match filter.controller {
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
        ValueExprAst::DistinctCountAmong { filter, .. } => {
            match filter.zone {
                Some(ZoneFilter::TargetPlayerGraveyard) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                }
                Some(ZoneFilter::TargetOpponentGraveyard) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                }
                _ => {}
            }
            match filter.controller {
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
        ValueExprAst::PlayerStat { player, .. }
        | ValueExprAst::PlayerCounterCount { player, .. } => match player {
            PlayerRefAst::TargetPlayer => builder.bind_target(BodyTarget::SinglePlayer),
            PlayerRefAst::TargetOpponent => builder.bind_target(BodyTarget::SingleOpponent),
            _ => Ok(()),
        },
        ValueExprAst::EventCountThisTurn { event, player } => {
            match player {
                Some(PlayerRefAst::TargetPlayer) => {
                    return builder.bind_target(BodyTarget::SinglePlayer);
                }
                Some(PlayerRefAst::TargetOpponent) => {
                    return builder.bind_target(BodyTarget::SingleOpponent);
                }
                _ => {}
            }
            let low = event.trim().to_ascii_lowercase();
            if low.contains("target opponent") {
                builder.bind_target(BodyTarget::SingleOpponent)
            } else if low.contains("target player") {
                builder.bind_target(BodyTarget::SinglePlayer)
            } else {
                Ok(())
            }
        }
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Max(a, b)
        | ValueExprAst::Min(a, b)
        | ValueExprAst::AbsDiff(a, b)
        | ValueExprAst::Pow { base: a, exp: b }
        | ValueExprAst::Shortfall {
            threshold: a,
            actual: b,
        } => {
            bind_value_expr_targets(a, builder)?;
            bind_value_expr_targets(b, builder)
        }
        ValueExprAst::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            bind_condition_value_expr_targets(condition, builder)?;
            bind_value_expr_targets(then_expr, builder)?;
            bind_value_expr_targets(else_expr, builder)
        }
        ValueExprAst::DivConst { expr: inner, .. } => bind_value_expr_targets(inner, builder),
        _ => Ok(()),
    }
}

pub(super) fn lower_value_expr(
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, &'static str> {
    lower_value_expr_inner(expr, vocabulary, None)
}

// Context-aware entrypoint for body.rs, instructions.rs, and helpers.rs callers with BodyBuilder and in_trigger.
pub(super) fn lower_value_expr_in_context(
    expr: &ValueExprAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, &'static str> {
    lower_value_expr_inner(expr, vocabulary, Some((builder, in_trigger)))
}

fn lower_stat_kind(stat: StatKind) -> ObjectStatKind {
    match stat {
        StatKind::Power => ObjectStatKind::Power,
        StatKind::Toughness => ObjectStatKind::Toughness,
        StatKind::ManaValue => ObjectStatKind::ManaValue,
        StatKind::Loyalty => ObjectStatKind::Loyalty,
    }
}

fn is_self_object_anaphora(low: &str) -> bool {
    matches!(
        low,
        "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
            | "this card"
            | "this battle"
            | "this vehicle"
    )
}

fn body_has_object_or_spell_target(target: BodyTarget) -> bool {
    !matches!(
        target,
        BodyTarget::None | BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
    )
}

fn body_target_guarantees_creature(target: BodyTarget) -> bool {
    match target {
        BodyTarget::SingleCreature => true,
        BodyTarget::FilteredPermanent(filter) => {
            let is_creature_clause = |c: ObjectClause| {
                c.any_types == CardTypeMask::CREATURE
                    || (c.required_types.bits() & CardTypeMask::CREATURE.bits()) != 0
            };
            let (a, b) = filter.clauses();
            is_creature_clause(a) && b.is_none_or(is_creature_clause)
        }
        _ => false,
    }
}

fn find_value_expr_cost_object_ref(expr: &ValueExpr) -> Option<ValueObjectRef> {
    match expr {
        ValueExpr::ObjectStat { subject, .. }
            if matches!(
                subject,
                ValueObjectRef::SacrificedCreature
                    | ValueObjectRef::LastSacrificedCreature
                    | ValueObjectRef::SacrificedPermanent
                    | ValueObjectRef::ExiledCostCreature
                    | ValueObjectRef::RevealedCostCard
                    | ValueObjectRef::RevealedTop
                    | ValueObjectRef::LastAmassedArmy
            ) =>
        {
            Some(*subject)
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            find_value_expr_cost_object_ref(inner)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            find_value_expr_cost_object_ref(a).or_else(|| find_value_expr_cost_object_ref(b))
        }
        _ => None,
    }
}

fn find_prior_cost_object_ref(instructions: &[Instruction]) -> Option<ValueObjectRef> {
    instructions.iter().rev().find_map(|op| match op {
        Instruction::DrawCards(spec) => find_value_expr_cost_object_ref(&spec.amount),
        Instruction::MutateObject(spec) => match &spec.op {
            spellwake_engine::toolbox::ObjectMutation::AddCounters { amount, .. }
            | spellwake_engine::toolbox::ObjectMutation::RemoveCounters { amount, .. }
            | spellwake_engine::toolbox::ObjectMutation::BecomeMonstrous { amount }
            | spellwake_engine::toolbox::ObjectMutation::Earthbend { amount }
            | spellwake_engine::toolbox::ObjectMutation::Connive { count: amount } => {
                find_value_expr_cost_object_ref(amount)
            }
            spellwake_engine::toolbox::ObjectMutation::CreateTokenCopy(copy) => {
                find_value_expr_cost_object_ref(&copy.count)
            }
            spellwake_engine::toolbox::ObjectMutation::MoveCounters {
                amount: Some(amount),
                ..
            } => find_value_expr_cost_object_ref(amount),
            _ => None,
        },
        Instruction::ApplyContinuous(spec) => match &spec.payload {
            spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                power, toughness, ..
            } => find_value_expr_cost_object_ref(power)
                .or_else(|| find_value_expr_cost_object_ref(toughness)),
            spellwake_engine::toolbox::ContinuousPayload::SkipTurns { count, .. } => {
                find_value_expr_cost_object_ref(count)
            }
            _ => None,
        },
        Instruction::AdjustLife(spec) => match &spec.op {
            spellwake_engine::toolbox::LifeOp::Gain(amount)
            | spellwake_engine::toolbox::LifeOp::Lose(amount)
            | spellwake_engine::toolbox::LifeOp::Set(amount)
            | spellwake_engine::toolbox::LifeOp::Pay(amount)
            | spellwake_engine::toolbox::LifeOp::Drain { amount, .. } => {
                find_value_expr_cost_object_ref(amount)
            }
            _ => None,
        },
        Instruction::DealDamage(spec) => match &spec.amount {
            spellwake_engine::program::DamageAmountSpec::Expr(amount) => {
                find_value_expr_cost_object_ref(amount)
            }
            _ => None,
        }
        .or_else(|| match &spec.recipients {
            spellwake_engine::toolbox::DamageTargetQuery::Divided { total, .. } => {
                find_value_expr_cost_object_ref(total)
            }
            _ => None,
        }),
        Instruction::MutatePlayer(spec) => match &spec.op {
            spellwake_engine::toolbox::PlayerMutation::AddCounter { amount, .. }
            | spellwake_engine::toolbox::PlayerMutation::PayCounter { amount, .. }
            | spellwake_engine::toolbox::PlayerMutation::Mill(amount)
            | spellwake_engine::toolbox::PlayerMutation::Scry(amount)
            | spellwake_engine::toolbox::PlayerMutation::Surveil(amount)
            | spellwake_engine::toolbox::PlayerMutation::ManifestTop { amount }
            | spellwake_engine::toolbox::PlayerMutation::Incubate { count: amount }
            | spellwake_engine::toolbox::PlayerMutation::Blight { amount }
            | spellwake_engine::toolbox::PlayerMutation::CollectEvidence { minimum: amount }
            | spellwake_engine::toolbox::PlayerMutation::BindResolutionX(amount)
            | spellwake_engine::toolbox::PlayerMutation::RollDie { count: amount, .. }
            | spellwake_engine::toolbox::PlayerMutation::RevealTopTwoPiles {
                count: amount, ..
            } => find_value_expr_cost_object_ref(amount),
            _ => None,
        },
        Instruction::CreateTokens { creations } => creations
            .iter()
            .find_map(|c| find_value_expr_cost_object_ref(&c.amount)),
        _ => None,
    })
}

fn has_prior_reveal_top(context: Option<(&BodyBuilder, bool)>) -> bool {
    context.is_some_and(|(b, _)| {
        b.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ZoneOp(spec)
                    if matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::RevealOnly
                            | spellwake_engine::toolbox::ZoneDest::LookOnly { .. }
                    ) && matches!(
                        &spec.source,
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Library(_),
                            slice: spellwake_engine::toolbox::QuerySlice::Top(_),
                            ..
                        }
                    )
            ) || matches!(
                op,
                Instruction::MutatePlayer(spec)
                    if matches!(
                        spec.op,
                        spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                    )
            )
        })
    })
}

fn has_prior_reveal_other(context: Option<(&BodyBuilder, bool)>) -> bool {
    context.is_some_and(|(b, _)| {
        b.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ZoneOp(spec)
                    if matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::RevealOnly)
                        || (matches!(
                            &spec.source,
                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                zone: spellwake_engine::toolbox::QueryZone::Library(_),
                                ..
                            }
                        ) && !matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::LookOnly { .. }
                        ))
            )
        })
    })
}

fn has_prior_exile_non_target(context: Option<(&BodyBuilder, bool)>) -> bool {
    context.is_some_and(|(b, _)| {
        b.instructions.iter().any(|op| {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op).is_some_and(
                |spec| {
                    matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::Exile { .. }
                    ) && !matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                },
            )
        })
    })
}

fn has_prior_single_sacrifice(context: Option<(&BodyBuilder, bool)>) -> bool {
    context.is_some_and(|(b, _)| {
        b.instructions.iter().any(|op| {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op).is_some_and(
                |spec| {
                    if let Some((players, amount, _, _)) = spec.as_group_sacrifice() {
                        matches!(players, AffectedPlayers::One(_))
                            && *amount == ValueExpr::literal(1)
                    } else {
                        matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                            }
                        )
                    }
                },
            )
        })
    })
}

fn resolve_that_player_for_value_expr(
    context: Option<(&BodyBuilder, bool)>,
    allow_spell_controller: bool,
) -> Result<ResolvingPlayer, &'static str> {
    let Some((builder, in_trigger)) = context else {
        return Ok(ResolvingPlayer::UniqueRelatedPlayer);
    };
    match builder.target {
        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => Ok(ResolvingPlayer::TargetPlayer),
        BodyTarget::SinglePermanent
        | BodyTarget::SingleCreature
        | BodyTarget::SinglePlaneswalker
        | BodyTarget::FilteredPermanent(_)
        | BodyTarget::SingleAnyTarget
        | BodyTarget::SingleCreatureOrPlayer
        | BodyTarget::SinglePlayerOrPlaneswalker
        | BodyTarget::SingleOpponentOrPlaneswalker => Ok(ResolvingPlayer::TargetController),
        BodyTarget::SingleSpell(_)
        | BodyTarget::SingleStackObject(_)
        | BodyTarget::SingleStackEntry
        | BodyTarget::ControlledAbility { .. }
        | BodyTarget::StackEntryTargetingControlledPermanent
            if allow_spell_controller =>
        {
            Ok(ResolvingPlayer::TargetSpellController)
        }
        BodyTarget::SingleOwnGraveyardCard | BodyTarget::FilteredOwnGraveyardCard(_) => {
            Ok(ResolvingPlayer::Controller)
        }
        BodyTarget::FilteredAnyGraveyardCard { .. }
        | BodyTarget::FilteredExiledCard(_)
        | BodyTarget::AnyGraveyardCard
        | BodyTarget::AnyGraveyardCreature
        | BodyTarget::AnyGraveyardCreatureOrPlaneswalker => Ok(ResolvingPlayer::TargetOwner),
        _ if builder.instructions.iter().any(|op| {
            matches!(
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
        }) =>
        {
            Ok(ResolvingPlayer::ChosenPlayer)
        }
        _ => {
            if let Some(prior_p) = builder.instructions.iter().rev().find_map(|op| match op {
                Instruction::OptionalSequence { chooser, .. }
                    if *chooser != ResolvingPlayer::Controller
                        && (*chooser != ResolvingPlayer::UniqueRelatedPlayer || in_trigger) =>
                {
                    Some(*chooser)
                }
                Instruction::ForEachPlayerQuery { selector, .. } => match selector.as_ref() {
                    spellwake_engine::toolbox::PlayerSelector::Resolving(p)
                    | spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::One(p))
                        if *p != ResolvingPlayer::Controller
                            && (*p != ResolvingPlayer::UniqueRelatedPlayer || in_trigger) =>
                    {
                        Some(*p)
                    }
                    _ => None,
                },
                Instruction::FixedTargetClause(body) => match body.target {
                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                        Some(ResolvingPlayer::TargetPlayer)
                    }
                    BodyTarget::SinglePermanent
                    | BodyTarget::SingleCreature
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::FilteredPermanent(_)
                    | BodyTarget::SingleAnyTarget
                    | BodyTarget::SingleCreatureOrPlayer
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker => {
                        Some(ResolvingPlayer::TargetController)
                    }
                    BodyTarget::SingleSpell(_)
                    | BodyTarget::SingleStackObject(_)
                    | BodyTarget::SingleStackEntry
                    | BodyTarget::ControlledAbility { .. }
                    | BodyTarget::StackEntryTargetingControlledPermanent
                        if allow_spell_controller =>
                    {
                        Some(ResolvingPlayer::TargetSpellController)
                    }
                    BodyTarget::FilteredAnyGraveyardCard { .. }
                    | BodyTarget::AnyGraveyardCard
                    | BodyTarget::AnyGraveyardCreature
                    | BodyTarget::AnyGraveyardCreatureOrPlaneswalker => {
                        Some(ResolvingPlayer::TargetOwner)
                    }
                    _ => None,
                },
                _ => None,
            }) {
                return Ok(prior_p);
            }
            if in_trigger {
                Ok(ResolvingPlayer::UniqueRelatedPlayer)
            } else {
                Err("oracle:unbound-that-player")
            }
        }
    }
}

fn is_supported_player_counter_kind(kind: CounterKind) -> bool {
    kind.is_player_counter() || kind.oracle_label() == "ticket"
}

fn resolve_player_ref_for_value_expr(
    player: PlayerRefAst,
    context: Option<(&BodyBuilder, bool)>,
    allow_spell_controller: bool,
) -> Result<ResolvingPlayer, &'static str> {
    match player {
        PlayerRefAst::You => Ok(ResolvingPlayer::Controller),
        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent => {
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::ThatPlayer => {
            resolve_that_player_for_value_expr(context, allow_spell_controller)
        }
        PlayerRefAst::DefendingPlayer => Ok(ResolvingPlayer::DefendingPlayer),
        PlayerRefAst::ActivePlayer => Ok(ResolvingPlayer::ActivePlayer),
        PlayerRefAst::EnchantedPlayer => Ok(ResolvingPlayer::EnchantedPlayer),
        PlayerRefAst::ChosenPlayer => Ok(ResolvingPlayer::ChosenPlayer),
        PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController => match context {
            Some((builder, true)) if !body_has_object_or_spell_target(builder.target) => {
                Ok(ResolvingPlayer::TriggerSubjectController)
            }
            Some((builder, false)) if !body_has_object_or_spell_target(builder.target) => {
                Err("oracle:unsupported-player-stat")
            }
            _ => Ok(ResolvingPlayer::TargetController),
        },
        PlayerRefAst::ItsOwner => match context {
            Some((builder, true)) if !body_has_object_or_spell_target(builder.target) => {
                Ok(ResolvingPlayer::TriggerSubjectOwner)
            }
            Some((builder, false)) if !body_has_object_or_spell_target(builder.target) => {
                Err("oracle:unsupported-player-stat")
            }
            _ => Ok(ResolvingPlayer::TargetOwner),
        },
        _ => Err("oracle:unsupported-player-stat"),
    }
}

fn parse_value_expr_player_ref(raw: &str) -> Option<PlayerRefAst> {
    match raw.trim() {
        "you" => Some(PlayerRefAst::You),
        "target player" => Some(PlayerRefAst::TargetPlayer),
        "target opponent" => Some(PlayerRefAst::TargetOpponent),
        "that player" | "they" | "he or she" => Some(PlayerRefAst::ThatPlayer),
        "defending player" | "the defending player" => Some(PlayerRefAst::DefendingPlayer),
        "active player" | "the active player" => Some(PlayerRefAst::ActivePlayer),
        "enchanted player" => Some(PlayerRefAst::EnchantedPlayer),
        "chosen player" | "the chosen player" => Some(PlayerRefAst::ChosenPlayer),
        "its controller"
        | "that creature's controller"
        | "that permanent's controller"
        | "that spell's controller" => Some(PlayerRefAst::ItsController),
        "its owner" | "that card's owner" | "that creature's owner" => Some(PlayerRefAst::ItsOwner),
        _ => None,
    }
}

fn resolve_affected_players_ref(
    player: PlayerRefAst,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<AffectedPlayers, &'static str> {
    match player {
        PlayerRefAst::EachPlayer => Ok(AffectedPlayers::EachPlayer),
        PlayerRefAst::EachOpponent => Ok(AffectedPlayers::EachOpponent),
        PlayerRefAst::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        other => resolve_player_ref_for_value_expr(other, context, false).map(AffectedPlayers::One),
    }
}

fn parse_value_expr_affected_players(
    raw: &str,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<AffectedPlayers, &'static str> {
    let trimmed = raw
        .trim()
        .trim_end_matches(" have")
        .trim_end_matches(" has")
        .trim_end_matches("'ve")
        .trim();
    match trimmed {
        "" | "players" | "all players" | "each player" | "a player" => {
            Ok(AffectedPlayers::EachPlayer)
        }
        "your opponents" | "opponents" | "an opponent" | "each opponent" => {
            Ok(AffectedPlayers::EachOpponent)
        }
        "other players" | "each other player" => Ok(AffectedPlayers::EachOtherPlayer),
        other => {
            let pref = parse_value_expr_player_ref(other).ok_or("oracle:unsupported-value-expr")?;
            let rp = resolve_player_ref_for_value_expr(pref, context, false)?;
            Ok(AffectedPlayers::One(rp))
        }
    }
}

fn lower_player_counter_value_expr(
    player: PlayerRefAst,
    counter_kind: &str,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<ValueExpr, &'static str> {
    let ck = parse_counter_kind(counter_kind)
        .filter(|k| is_supported_player_counter_kind(*k))
        .ok_or("oracle:unsupported-counter-kind")?;
    if player == PlayerRefAst::You {
        return Ok(match ck {
            CounterKind::Experience => ValueExpr::CounterCount {
                subject: spellwake_engine::program::CounterSubjectRef::ControllerExperience,
                kind: None,
            },
            CounterKind::Energy => ValueExpr::CounterCount {
                subject: spellwake_engine::program::CounterSubjectRef::ControllerEnergy,
                kind: None,
            },
            CounterKind::Poison => ValueExpr::CounterCount {
                subject: spellwake_engine::program::CounterSubjectRef::ControllerPoison,
                kind: None,
            },
            other => ValueExpr::CounterCount {
                subject: spellwake_engine::program::CounterSubjectRef::PlayerCounter(
                    ResolvingPlayer::Controller,
                    other,
                ),
                kind: Some(other),
            },
        });
    }
    let role = resolve_player_ref_for_value_expr(player, context, false)
        .map_err(|_| "oracle:unsupported-counter-subject")?;
    Ok(ValueExpr::CounterCount {
        subject: spellwake_engine::program::CounterSubjectRef::PlayerCounter(role, ck),
        kind: Some(ck),
    })
}

fn resolve_damage_subject_ref(
    raw: &str,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<ValueObjectRef, &'static str> {
    let low = raw.trim().to_ascii_lowercase();
    if low == "~" || is_self_object_anaphora(&low) || low == "this spell" {
        return Ok(ValueObjectRef::AbilitySource);
    }
    if matches!(
        low.as_str(),
        "equipped creature" | "enchanted creature" | "enchanted permanent" | "fortified land"
    ) {
        return Ok(ValueObjectRef::AttachedHost);
    }
    if matches!(
        low.as_str(),
        "it" | "that creature"
            | "that permanent"
            | "the creature"
            | "the permanent"
            | "that spell"
            | "that planeswalker"
            | "that battle"
    ) {
        return match context {
            Some((builder, in_trigger)) => {
                if body_has_object_or_spell_target(builder.target) {
                    Ok(ValueObjectRef::TargetObject)
                } else if in_trigger {
                    Ok(ValueObjectRef::TriggerSubject)
                } else {
                    Err("oracle:unsupported-value-expr")
                }
            }
            None => Ok(ValueObjectRef::TargetObject),
        };
    }
    if matches!(
        low.as_str(),
        "target creature" | "target permanent" | "target planeswalker" | "target battle"
    ) && context.is_none_or(|(b, _)| body_has_object_or_spell_target(b.target))
    {
        return Ok(ValueObjectRef::TargetObject);
    }
    Err("oracle:unsupported-value-expr")
}

fn turn_record_filter_supported(filter: ObjectFilter, allow_artifact_subtypes: bool) -> bool {
    let empty_flags = ObjectClause::any().flags;
    let clause_ok = |c: ObjectClause| {
        let subtype_ok = match c.subtypes {
            SubtypeFilter::Any
            | SubtypeFilter::Has(_)
            | SubtypeFilter::Lacks(_)
            | SubtypeFilter::AnyOf(_)
            | SubtypeFilter::NoneOf(_) => true,
            SubtypeFilter::Artifact(_) => allow_artifact_subtypes,
            _ => false,
        };
        subtype_ok
            && c.controller == ControllerFilter::Any
            && c.owner == ControllerFilter::Any
            && c.combat == CombatFilter::Any
            && c.tapped.is_none()
            && c.token.is_none()
            && c.keyword.is_none()
            && c.forbidden_keyword.is_none()
            && c.power == PowerFilter::Any
            && c.toughness == ToughnessFilter::Any
            && c.mana_value == ManaValueFilter::Any
            && c.flags == empty_flags
            && c.source_relation == SourceRelation::Any
            && c.exact_name.is_none()
            && c.required_counter_kind.is_none()
            && c.forbidden_counter_kind.is_none()
    };
    let (a, b) = filter.clauses();
    clause_ok(a) && b.is_none_or(clause_ok)
}

fn parse_turn_event_filter(
    raw: &str,
    allow_artifact_subtypes: bool,
    is_spell_context: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let trimmed = raw.trim();
    match trimmed {
        "" | "card" | "cards" | "spell" | "spells" => return Some(ObjectFilter::any()),
        "permanent" | "permanents" => return Some(ObjectFilter::permanent()),
        "instant and sorcery" | "sorcery and instant" => {
            return Some(ObjectFilter::instant_or_sorcery());
        }
        _ => {}
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let parsed = spellwake_oracle_compiler::filter::parse_object_filter(trimmed, &compiler_vocab)
        .or_else(|_| {
            let suffix = if is_spell_context { " spell" } else { " card" };
            spellwake_oracle_compiler::filter::parse_object_filter(
                &format!("{trimmed}{suffix}"),
                &compiler_vocab,
            )
        })
        .ok()?;
    if parsed.zone.is_some()
        || parsed.controller.is_some()
        || parsed.owner.is_some()
        || !parsed.flags.is_empty()
        || !parsed.predicates.is_empty()
        || (parsed.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0
    {
        return None;
    }
    let obj = lower_object_filter(&parsed, vocabulary).ok()?;
    turn_record_filter_supported(obj, allow_artifact_subtypes).then_some(obj)
}

fn try_lower_spells_cast_count(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<Option<ValueExpr>, &'static str> {
    if !f.any_spell || f.any_permanent || f.any_card || f.is_ability || f.named.is_some() {
        return Ok(None);
    }
    let has_entered = (f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0) != 0;
    let clean_flags =
        f.flags.0 & !(AstFilterFlags::ENTERED_THIS_TURN.0 | AstFilterFlags::LKI_PAST_TENSE.0);
    if clean_flags != 0 {
        return Ok(None);
    }
    let mut implied_ctrl: Option<ControllerRelation> = None;
    let cast_this_turn = match f.predicates.as_slice() {
        [] => has_entered,
        [FilterPredicateAst::StateQualification(q)] => {
            match q.trim().to_ascii_lowercase().as_str() {
                "you cast this turn" => {
                    if f.controller.is_some_and(|c| c != ControllerRelation::You) {
                        return Ok(None);
                    }
                    implied_ctrl = Some(ControllerRelation::You);
                    true
                }
                "that was cast this turn" | "cast this turn" => true,
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    if !cast_this_turn {
        return Ok(None);
    }
    let from_zone = match f.zone {
        None => None,
        Some(
            ZoneFilter::YourHand
            | ZoneFilter::ThatPlayerHand
            | ZoneFilter::TargetPlayerHand
            | ZoneFilter::TargetOpponentHand
            | ZoneFilter::OpponentHand
            | ZoneFilter::OpponentsHands
            | ZoneFilter::EachPlayerHand
            | ZoneFilter::AnyHand,
        ) => Some(SourceZoneFilter::Hand),
        Some(
            ZoneFilter::YourGraveyard
            | ZoneFilter::ThatPlayerGraveyard
            | ZoneFilter::TargetPlayerGraveyard
            | ZoneFilter::TargetOpponentGraveyard
            | ZoneFilter::OpponentGraveyard
            | ZoneFilter::OpponentsGraveyards
            | ZoneFilter::AnyGraveyard
            | ZoneFilter::AllGraveyards,
        ) => Some(SourceZoneFilter::Graveyard),
        Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
        _ => return Ok(None),
    };
    let players = match f.controller.or(implied_ctrl) {
        Some(ControllerRelation::You | ControllerRelation::YourTeam) => {
            AffectedPlayers::One(ResolvingPlayer::Controller)
        }
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            AffectedPlayers::EachOpponent
        }
        Some(ControllerRelation::NotYou) => AffectedPlayers::EachOtherPlayer,
        Some(ControllerRelation::TargetPlayer | ControllerRelation::TargetOpponent) => {
            AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
        }
        Some(ControllerRelation::ThatPlayer) => {
            AffectedPlayers::One(resolve_that_player_for_value_expr(context, true)?)
        }
        Some(ControllerRelation::DefendingPlayer) => {
            AffectedPlayers::One(ResolvingPlayer::DefendingPlayer)
        }
        Some(ControllerRelation::ActivePlayer) => {
            AffectedPlayers::One(ResolvingPlayer::ActivePlayer)
        }
        None => {
            if matches!(
                f.zone,
                Some(ZoneFilter::YourHand | ZoneFilter::YourGraveyard)
            ) {
                AffectedPlayers::One(ResolvingPlayer::Controller)
            } else {
                AffectedPlayers::EachPlayer
            }
        }
        _ => return Ok(None),
    };
    let mut cleaned = f.clone();
    cleaned.controller = None;
    cleaned.zone = None;
    cleaned.flags = AstFilterFlags::EMPTY;
    cleaned.predicates.clear();
    if (cleaned.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0 {
        return Ok(None);
    }
    let filter = if is_unrestricted_filter(&cleaned) {
        ObjectFilter::any()
    } else {
        let obj = lower_object_filter(&cleaned, vocabulary)?;
        if !turn_record_filter_supported(obj, false) {
            return Ok(None);
        }
        obj
    };
    Ok(Some(ValueExpr::PlayerStat(
        PlayerStatExpr::SpellsCastMatchingThisTurn {
            players,
            filter: Box::new(filter),
            from_zone,
        },
    )))
}

fn try_lower_graveyard_arrivals_count(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<Option<ValueExpr>, &'static str> {
    if f.any_spell || f.is_ability || f.named.is_some() || !f.predicates.is_empty() {
        return Ok(None);
    }
    let has_died = (f.flags.0 & AstFilterFlags::DIED_THIS_TURN.0) != 0;
    let has_entered = (f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0) != 0;
    let extra_flags = f.flags.0
        & !(AstFilterFlags::DIED_THIS_TURN.0
            | AstFilterFlags::ENTERED_THIS_TURN.0
            | AstFilterFlags::LKI_PAST_TENSE.0);
    if extra_flags != 0 {
        return Ok(None);
    }
    let zone_owner = match f.zone {
        None => {
            if !has_died {
                return Ok(None);
            }
            None
        }
        Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard) if has_died || has_entered => {
            None
        }
        Some(ZoneFilter::YourGraveyard) if has_died || has_entered => {
            Some(ResolvingPlayer::Controller)
        }
        Some(ZoneFilter::TargetPlayerGraveyard | ZoneFilter::TargetOpponentGraveyard)
            if has_died || has_entered =>
        {
            Some(ResolvingPlayer::TargetPlayer)
        }
        Some(ZoneFilter::DefendingPlayerGraveyard) if has_died || has_entered => {
            Some(ResolvingPlayer::DefendingPlayer)
        }
        Some(ZoneFilter::ThatPlayerGraveyard) if has_died || has_entered => {
            Some(resolve_that_player_for_value_expr(context, true)?)
        }
        _ => return Ok(None),
    };
    let explicit_ctrl = match f.controller {
        None => None,
        Some(ControllerRelation::You | ControllerRelation::YourTeam) => {
            Some(ResolvingPlayer::Controller)
        }
        Some(ControllerRelation::ThatPlayer) => {
            Some(resolve_that_player_for_value_expr(context, true)?)
        }
        Some(ControllerRelation::TargetPlayer | ControllerRelation::TargetOpponent) => {
            Some(ResolvingPlayer::TargetPlayer)
        }
        Some(ControllerRelation::DefendingPlayer) => Some(ResolvingPlayer::DefendingPlayer),
        Some(ControllerRelation::ActivePlayer) => Some(ResolvingPlayer::ActivePlayer),
        _ => return Ok(None),
    };
    let controller = match (explicit_ctrl, zone_owner) {
        (Some(a), Some(b)) if a != b => return Ok(None),
        (Some(a), _) => Some(a),
        (None, b) => b,
    };
    let mut cleaned = f.clone();
    cleaned.zone = None;
    cleaned.controller = None;
    cleaned.flags = AstFilterFlags::EMPTY;
    if (cleaned.required_supertypes.0 & AstSupertypeMask::HISTORIC.0) != 0 {
        return Ok(None);
    }
    let filter = if is_unrestricted_filter(&cleaned) {
        ObjectFilter::any()
    } else {
        let obj = lower_object_filter(&cleaned, vocabulary)?;
        if !turn_record_filter_supported(obj, true) {
            return Ok(None);
        }
        obj
    };
    Ok(Some(ValueExpr::PlayerStat(
        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
            filter: Box::new(filter),
            from_battlefield_only: has_died,
            controller,
        },
    )))
}

fn try_lower_event_count_this_turn_ext(
    norm: &str,
    player: Option<PlayerRefAst>,
    vocabulary: &CreatureTypeVocabulary,
    context: Option<(&BodyBuilder, bool)>,
) -> Option<Result<ValueExpr, &'static str>> {
    // 1. Damage prevented this way
    if matches!(
        norm,
        "the amount of damage prevented this way"
            | "the damage prevented this way"
            | "damage prevented this way"
            | "1 damage prevented this way"
    ) && player.is_none()
    {
        return Some(Ok(ValueExpr::event(EventValueKind::DamagePreventedThisWay)));
    }

    // 2. Cards exiled this turn
    if matches!(
        norm,
        "the number of cards exiled this turn"
            | "cards exiled this turn"
            | "card exiled this turn"
            | "cards_exiled_this_turn"
    ) && player.is_none()
    {
        return Some(Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::CardsExiledThisTurn,
        )));
    }

    // 3. Spells cast this turn (shorthand token from Stage 1)
    if norm == "spells_cast_this_turn" {
        return Some(match player {
            None | Some(PlayerRefAst::You) => Ok(ValueExpr::PlayerStat(
                PlayerStatExpr::ControllerSpellsCastThisTurn,
            )),
            Some(p) => resolve_affected_players_ref(p, context).map(|players| {
                ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players,
                    filter: Box::new(ObjectFilter::any()),
                    from_zone: None,
                })
            }),
        });
    }

    // 4. Life lost last turn
    if norm == "life_lost_last_turn"
        && let Some(p) = player
    {
        return Some(
            resolve_player_ref_for_value_expr(p, context, false)
                .map(|rp| ValueExpr::PlayerStat(PlayerStatExpr::PlayerLifeLostLastTurn(rp))),
        );
    }
    if let Some(after_life) = norm
        .strip_prefix("the amount of life ")
        .or_else(|| norm.strip_prefix("amount of life "))
        .or_else(|| norm.strip_prefix("the life "))
        .or_else(|| norm.strip_prefix("life "))
        && let Some(who_raw) = after_life
            .strip_suffix(" lost last turn")
            .or_else(|| after_life.strip_suffix(" has lost last turn"))
            .or_else(|| after_life.strip_suffix(" have lost last turn"))
    {
        let pref = player.or_else(|| parse_value_expr_player_ref(who_raw));
        if let Some(p) = pref {
            return Some(
                resolve_player_ref_for_value_expr(p, context, false)
                    .map(|rp| ValueExpr::PlayerStat(PlayerStatExpr::PlayerLifeLostLastTurn(rp))),
            );
        }
    }

    // 5. Player counters: "the number of [poison/energy/experience/rad/ticket] counters [player] has"
    //    and "the amount of {e} [player] has/have"
    if let Some(after_energy) = norm
        .strip_prefix("the amount of {e} ")
        .or_else(|| norm.strip_prefix("amount of {e} "))
        .or_else(|| norm.strip_prefix("the amount of energy "))
        && let Some(who_raw) = after_energy
            .strip_suffix(" has")
            .or_else(|| after_energy.strip_suffix(" have"))
    {
        let pref = player.or_else(|| parse_value_expr_player_ref(who_raw));
        if let Some(p) = pref {
            return Some(lower_player_counter_value_expr(p, "energy", context));
        }
    }
    let after_num = norm
        .strip_prefix("the number of ")
        .or_else(|| norm.strip_prefix("number of "))
        .unwrap_or(norm);
    if let Some((ck_raw, who_part)) = after_num
        .split_once(" counters ")
        .or_else(|| after_num.split_once(" counter "))
        && let Some(who_raw) = who_part
            .strip_suffix(" has")
            .or_else(|| who_part.strip_suffix(" have"))
    {
        let pref = player.or_else(|| parse_value_expr_player_ref(who_raw));
        if let Some(p) = pref
            && parse_counter_kind(ck_raw).is_some_and(is_supported_player_counter_kind)
        {
            return Some(lower_player_counter_value_expr(p, ck_raw, context));
        }
    }

    // 6. Excess damage / damage received / damage dealt this turn
    if player.is_none()
        && let Some(body) = norm.strip_suffix(" this turn")
    {
        let body = body
            .strip_prefix("the amount of ")
            .or_else(|| body.strip_prefix("amount of "))
            .or_else(|| body.strip_prefix("the "))
            .unwrap_or(body);
        if let Some(subj_raw) = body.strip_prefix("excess damage dealt to ") {
            return Some(
                resolve_damage_subject_ref(subj_raw, context).map(|subject| {
                    ValueExpr::PlayerStat(PlayerStatExpr::DamageReceivedThisTurn {
                        subject,
                        excess_only: true,
                    })
                }),
            );
        }
        if let Some(subj_raw) = body
            .strip_prefix("damage already dealt to ")
            .or_else(|| body.strip_prefix("damage dealt to "))
        {
            return Some(
                resolve_damage_subject_ref(subj_raw, context).map(|subject| {
                    ValueExpr::PlayerStat(PlayerStatExpr::DamageReceivedThisTurn {
                        subject,
                        excess_only: false,
                    })
                }),
            );
        }
        let (combat_only, after_dmg) = if let Some(rest) = body.strip_prefix("combat damage ") {
            (true, Some(rest))
        } else if let Some(rest) = body.strip_prefix("damage ") {
            (false, Some(rest))
        } else {
            (false, None)
        };
        if let Some(rest) = after_dmg {
            let (subj_raw, to_opponent_only) = if let Some(s) = rest
                .strip_suffix(" dealt to your opponents")
                .or_else(|| rest.strip_suffix(" dealt to opponents"))
                .or_else(|| rest.strip_suffix(" dealt to an opponent"))
                .or_else(|| rest.strip_prefix("dealt to your opponents by "))
                .or_else(|| rest.strip_prefix("dealt to opponents by "))
                .or_else(|| rest.strip_prefix("dealt to an opponent by "))
            {
                (Some(s), true)
            } else if let Some(s) = rest
                .strip_suffix(" dealt")
                .or_else(|| rest.strip_prefix("dealt by "))
            {
                (Some(s), false)
            } else {
                (None, false)
            };
            if let Some(s) = subj_raw {
                return Some(resolve_damage_subject_ref(s, context).map(|subject| {
                    ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                        subject,
                        to_opponent_only,
                        combat_only,
                    })
                }));
            }
        }
    }

    // 7. Spells cast matching this turn
    if let Some(body) = norm.strip_suffix(" this turn") {
        let rest = body
            .strip_prefix("the number of ")
            .or_else(|| body.strip_prefix("number of "))
            .unwrap_or(body);
        let spell_split = if let Some(after) = rest
            .strip_prefix("spells ")
            .or_else(|| rest.strip_prefix("spell "))
        {
            Some(("", after))
        } else {
            rest.split_once(" spells ")
                .or_else(|| rest.split_once(" spell "))
        };
        if let Some((flt_raw, tail_raw)) = spell_split {
            let (tail_no_zone, from_zone, implied_you) =
                if let Some(t) = tail_raw.strip_suffix(" from your hand") {
                    (t.trim(), Some(SourceZoneFilter::Hand), true)
                } else if let Some(t) = tail_raw
                    .strip_suffix(" from their hand")
                    .or_else(|| tail_raw.strip_suffix(" from a hand"))
                    .or_else(|| tail_raw.strip_suffix(" from hand"))
                {
                    (t.trim(), Some(SourceZoneFilter::Hand), false)
                } else if let Some(t) = tail_raw.strip_suffix(" from your graveyard") {
                    (t.trim(), Some(SourceZoneFilter::Graveyard), true)
                } else if let Some(t) = tail_raw
                    .strip_suffix(" from a graveyard")
                    .or_else(|| tail_raw.strip_suffix(" from graveyards"))
                    .or_else(|| tail_raw.strip_suffix(" from their graveyard"))
                {
                    (t.trim(), Some(SourceZoneFilter::Graveyard), false)
                } else if let Some(t) = tail_raw.strip_suffix(" from exile") {
                    (t.trim(), Some(SourceZoneFilter::Exile), false)
                } else {
                    (tail_raw.trim(), None, false)
                };
            if let Some(who_raw) = tail_no_zone
                .strip_suffix(" cast")
                .or_else(|| (tail_no_zone == "cast").then_some(""))
            {
                let who_clean = who_raw.trim();
                let players_res = if let Some(p) = player {
                    resolve_affected_players_ref(p, context)
                } else if who_clean.is_empty() && implied_you {
                    Ok(AffectedPlayers::One(ResolvingPlayer::Controller))
                } else {
                    parse_value_expr_affected_players(who_clean, context)
                };
                if let Ok(players) = players_res
                    && let Some(filter) = parse_turn_event_filter(flt_raw, false, true, vocabulary)
                {
                    return Some(Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::SpellsCastMatchingThisTurn {
                            players,
                            filter: Box::new(filter),
                            from_zone,
                        },
                    )));
                }
            }
        }
    }

    // 8. Graveyard arrivals matching this turn
    if let Some(body) = norm.strip_suffix(" this turn") {
        let rest = body
            .strip_prefix("the number of ")
            .or_else(|| body.strip_prefix("number of "))
            .unwrap_or(body);
        if let Some((flt_part, after_put)) = rest
            .split_once(" that were put into ")
            .or_else(|| rest.split_once(" that was put into "))
            .or_else(|| rest.split_once(" put into "))
        {
            let (gy_part, from_battlefield_only) =
                if let Some(g) = after_put.strip_suffix(" from the battlefield") {
                    (g.trim(), true)
                } else if let Some(g) = after_put.strip_suffix(" from anywhere") {
                    (g.trim(), false)
                } else if after_put.contains(" from ") {
                    // Fail closed on restricted non-battlefield origins such as "from your library".
                    return Some(Err("oracle:unsupported-value-expr"));
                } else {
                    (after_put.trim(), false)
                };
            let controller_res: Option<Result<Option<ResolvingPlayer>, &'static str>> =
                match gy_part {
                    "graveyards" | "a graveyard" | "any graveyard" => Some(Ok(None)),
                    "your graveyard" => Some(Ok(Some(ResolvingPlayer::Controller))),
                    "their graveyard" | "that player's graveyard" => {
                        Some(resolve_that_player_for_value_expr(context, true).map(Some))
                    }
                    "target player's graveyard" | "target opponent's graveyard" => {
                        Some(Ok(Some(ResolvingPlayer::TargetPlayer)))
                    }
                    _ => None,
                };
            if let Some(ctrl_res) = controller_res
                && let Some(filter) = parse_turn_event_filter(flt_part, true, false, vocabulary)
            {
                return Some(ctrl_res.map(|controller| {
                    ValueExpr::PlayerStat(PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                        filter: Box::new(filter),
                        from_battlefield_only,
                        controller,
                    })
                }));
            }
        }
    }

    // 9. Sacrificed matching this turn
    if matches!(
        norm,
        "the number of permanents sacrificed this turn" | "permanent sacrificed this turn"
    ) && player.is_none()
    {
        return Some(Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::SacrificedMatchingThisTurn {
                players: AffectedPlayers::EachPlayer,
                filter: Box::new(ObjectFilter::permanent()),
            },
        )));
    }
    if let Some(body) = norm.strip_suffix(" sacrificed this turn") {
        let rest = body
            .strip_prefix("the number of ")
            .or_else(|| body.strip_prefix("number of "))
            .unwrap_or(body)
            .trim();
        let (flt_raw, who_raw) = if let Some(r) = rest
            .strip_suffix(" that were")
            .or_else(|| rest.strip_suffix(" that was"))
        {
            (r.trim(), "")
        } else {
            let mut found = (rest, "");
            for suffix in [
                " your opponents have",
                " opponents have",
                " an opponent has",
                " each opponent has",
                " each other player has",
                " other players have",
                " target opponent has",
                " target player has",
                " the defending player has",
                " defending player has",
                " the active player has",
                " active player has",
                " that player has",
                " all players have",
                " each player has",
                " players have",
                " a player has",
                " they have",
                " you have",
                " you've",
                " they've",
                " your opponents",
                " target opponent",
                " target player",
                " each opponent",
                " each other player",
                " other players",
                " the defending player",
                " defending player",
                " the active player",
                " active player",
                " that player",
                " all players",
                " each player",
                " an opponent",
                " opponents",
                " a player",
                " players",
                " they",
                " you",
            ] {
                if let Some(prefix) = rest.strip_suffix(suffix) {
                    found = (prefix.trim(), suffix.trim());
                    break;
                }
            }
            found
        };
        let players_res = if who_raw.is_empty()
            && let Some(p) = player
        {
            resolve_affected_players_ref(p, context)
        } else {
            parse_value_expr_affected_players(who_raw, context)
        };
        if let Ok(players) = players_res
            && let Some(filter) = parse_turn_event_filter(flt_raw, true, false, vocabulary)
        {
            return Some(Ok(ValueExpr::PlayerStat(
                PlayerStatExpr::SacrificedMatchingThisTurn {
                    players,
                    filter: Box::new(filter),
                },
            )));
        }
    }

    None
}

fn lower_value_expr_inner(
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
    context: Option<(&BodyBuilder, bool)>,
) -> Result<ValueExpr, &'static str> {
    match expr {
        ValueExprAst::Const(n) => Ok(ValueExpr::literal(i64::from(*n))),
        ValueExprAst::ChosenX => Ok(ValueExpr::chosen_x()),
        ValueExprAst::Count(f) => {
            // Party count ("for each creature in your party")
            if let [FilterPredicateAst::StateQualification(q)] = f.predicates.as_slice()
                && q.trim().eq_ignore_ascii_case("in_your_party")
                && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::CREATURE)
                && matches!(f.controller, None | Some(ControllerRelation::You))
                && f.zone.is_none()
                && f.extra_zones.is_empty()
                && f.owner.is_none()
                && f.subtypes.is_empty()
                && f.forbidden_types.is_empty()
                && f.forbidden_subtypes.is_empty()
                && f.required_supertypes.is_empty()
                && f.forbidden_supertypes.is_empty()
                && f.any_colors.is_empty()
                && f.forbidden_colors.is_empty()
                && f.color_arity.is_none()
                && f.combat.is_none()
                && f.tapped.is_none()
                && f.token.is_none()
                && f.required_keywords.is_empty()
                && f.forbidden_keywords.is_empty()
                && f.stat_bounds.is_empty()
                && f.counter_kind.is_none()
                && f.named.is_none()
                && f.flags.is_empty()
            {
                let roles =
                    resolve_party_roles(vocabulary).ok_or("oracle:unsupported-value-expr")?;
                return Ok(ValueExpr::PartyCount(Box::new(roles)));
            }
            // Exact prior-action count using LastActionMatchingCount (LKI snapshot of affected objects)
            if let Some((builder, _)) = context
                && f.zone.is_none()
                && f.extra_zones.is_empty()
                && let Some(pos) = f
                    .predicates
                    .iter()
                    .position(|p| matches!(p, FilterPredicateAst::AffectedByPriorAction(_)))
                && let Some(last_ins) = builder.instructions.last()
            {
                use spellwake_oracle_compiler::ast::PriorActionRefKind;
                let FilterPredicateAst::AffectedByPriorAction(kind) = &f.predicates[pos] else {
                    unreachable!();
                };
                let action_matches = matches!(
                    (kind, last_ins),
                    (PriorActionRefKind::TappedThisWay, Instruction::MutateObject(spec))
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::ObjectMutation::SetTapped(true)
                        )
                ) || crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(last_ins)
                    .is_some_and(|spec| match kind {
                        PriorActionRefKind::DestroyedThisWay => matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. }
                            }
                        ),
                        PriorActionRefKind::ExiledThisWay => matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::Exile { .. }
                        ),
                        PriorActionRefKind::SacrificedThisWay => matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                            }
                        ),
                        _ => false,
                    });
                if action_matches {
                    let mut clean = f.as_ref().clone();
                    clean.predicates.remove(pos);
                    if is_unrestricted_filter(&clean) {
                        return Ok(ValueExpr::EventValue(EventValueKind::LastActionCount));
                    }
                    let obj = lower_object_filter(&clean, vocabulary)?;
                    if !obj.has_target_player() {
                        return Ok(ValueExpr::LastActionMatchingCount(Box::new(obj)));
                    }
                }
            }
            if f.any_spell {
                if f.zone.is_none()
                    && !f.any_permanent
                    && !f.any_card
                    && matches!(f.controller, None | Some(ControllerRelation::You))
                    && (f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0) != 0
                {
                    let clean_flags = f.flags.0
                        & !(AstFilterFlags::ENTERED_THIS_TURN.0 | AstFilterFlags::LKI_PAST_TENSE.0);
                    if clean_flags == AstFilterFlags::EXCLUDE_SELF.0
                        && filter_only_has(
                            f,
                            FilterAllow {
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Ok(ValueExpr::PlayerStat(
                            PlayerStatExpr::OtherSpellsCastThisTurn,
                        ));
                    }
                    if clean_flags == 0
                        && filter_only_has(
                            f,
                            FilterAllow {
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Ok(ValueExpr::PlayerStat(
                            PlayerStatExpr::ControllerSpellsCastThisTurn,
                        ));
                    }
                    if clean_flags == 0
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        let types = card_type_mask_to_set(f.any_types)?;
                        if types.len() == 1
                            && let Some(&ct) = types.iter().next()
                        {
                            return Ok(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(ct),
                            ));
                        }
                        if f.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                            return Ok(ValueExpr::Add(
                                Box::new(ValueExpr::PlayerStat(
                                    PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                                )),
                                Box::new(ValueExpr::PlayerStat(
                                    PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                                )),
                            ));
                        }
                    }
                    if clean_flags == 0
                        && f.forbidden_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            f,
                            FilterAllow {
                                forbidden_types: true,
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Ok(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastNoncreatureSpellsThisTurn,
                        ));
                    }
                }
                if let Some(expr) = try_lower_spells_cast_count(f, vocabulary, context)? {
                    return Ok(expr);
                }
                return Err("oracle:unsupported-spell-count");
            }
            if f.zone == Some(ZoneFilter::Exile)
                && !f.any_permanent
                && !f.any_spell
                && (f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0) != 0
                && (f.flags.0
                    & !(AstFilterFlags::ENTERED_THIS_TURN.0 | AstFilterFlags::LKI_PAST_TENSE.0))
                    == 0
                && filter_only_has(
                    f,
                    FilterAllow {
                        zone: true,
                        flags: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                return Ok(ValueExpr::PlayerStat(PlayerStatExpr::CardsExiledThisTurn));
            }
            if let Some(expr) = try_lower_graveyard_arrivals_count(f, vocabulary, context)? {
                return Ok(expr);
            }
            if let Some(zone) = f.zone {
                let mut inner = f.as_ref().clone();
                inner.zone = None;
                if zone == ZoneFilter::OpponentsGraveyards {
                    if inner.controller.is_some() || inner.owner.is_some() {
                        return Err("oracle:unsupported-count-zone");
                    }
                    let mut with_zone = f.as_ref().clone();
                    with_zone.controller = Some(ControllerRelation::OwnerOpponent);
                    let opp_obj = lower_object_filter(&with_zone, vocabulary)?;
                    return Ok(ValueExpr::Count {
                        zone: ValueZone::AllGraveyards,
                        filter: Box::new(opp_obj),
                        controller_scope: None,
                    });
                }
                let obj = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(f, vocabulary)?
                };
                return match zone {
                    ZoneFilter::YourGraveyard => {
                        Ok(ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj))
                    }
                    ZoneFilter::TargetPlayerGraveyard | ZoneFilter::TargetOpponentGraveyard => Ok(
                        ValueExpr::count_graveyard(ResolvingPlayer::TargetPlayer, obj),
                    ),
                    ZoneFilter::DefendingPlayerGraveyard => Ok(ValueExpr::count_graveyard(
                        ResolvingPlayer::DefendingPlayer,
                        obj,
                    )),
                    ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards => Ok(ValueExpr::Count {
                        zone: ValueZone::AllGraveyards,
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::ThatPlayerGraveyard => {
                        let rp = resolve_that_player_for_value_expr(context, true)?;
                        Ok(ValueExpr::count_graveyard(rp, obj))
                    }
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
                    ZoneFilter::AnyHand | ZoneFilter::EachPlayerHand => Ok(ValueExpr::Count {
                        zone: ValueZone::AllHands,
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::OpponentsHands => Ok(ValueExpr::Count {
                        zone: ValueZone::OpponentsHands,
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::TargetPlayerHand | ZoneFilter::TargetOpponentHand => {
                        Ok(ValueExpr::Count {
                            zone: ValueZone::Hand(ResolvingPlayer::TargetPlayer),
                            filter: Box::new(obj),
                            controller_scope: None,
                        })
                    }
                    ZoneFilter::ThatPlayerHand => {
                        let rp = resolve_that_player_for_value_expr(context, true)?;
                        Ok(ValueExpr::Count {
                            zone: ValueZone::Hand(rp),
                            filter: Box::new(obj),
                            controller_scope: None,
                        })
                    }
                    ZoneFilter::YourLibrary => Ok(ValueExpr::Count {
                        zone: ValueZone::Library(ResolvingPlayer::Controller),
                        filter: Box::new(obj),
                        controller_scope: None,
                    }),
                    ZoneFilter::TargetPlayerLibrary | ZoneFilter::TargetOpponentLibrary => {
                        Ok(ValueExpr::Count {
                            zone: ValueZone::Library(ResolvingPlayer::TargetPlayer),
                            filter: Box::new(obj),
                            controller_scope: None,
                        })
                    }
                    ZoneFilter::ThatPlayerLibrary => {
                        let rp = resolve_that_player_for_value_expr(context, true)?;
                        Ok(ValueExpr::Count {
                            zone: ValueZone::Library(rp),
                            filter: Box::new(obj),
                            controller_scope: None,
                        })
                    }
                    ZoneFilter::YourHandOrGraveyard => {
                        let hand = ValueExpr::Count {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(obj),
                            controller_scope: None,
                        };
                        let gy = ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj);
                        Ok(ValueExpr::Add(Box::new(hand), Box::new(gy)))
                    }
                    ZoneFilter::YourLibraryOrGraveyard => {
                        let lib = ValueExpr::Count {
                            zone: ValueZone::Library(ResolvingPlayer::Controller),
                            filter: Box::new(obj),
                            controller_scope: None,
                        };
                        let gy = ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj);
                        Ok(ValueExpr::Add(Box::new(lib), Box::new(gy)))
                    }
                    ZoneFilter::YourGraveyardHandOrLibrary => {
                        let gy = ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj);
                        let hand = ValueExpr::Count {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(obj),
                            controller_scope: None,
                        };
                        let lib = ValueExpr::Count {
                            zone: ValueZone::Library(ResolvingPlayer::Controller),
                            filter: Box::new(obj),
                            controller_scope: None,
                        };
                        Ok(ValueExpr::Add(
                            Box::new(ValueExpr::Add(Box::new(gy), Box::new(hand))),
                            Box::new(lib),
                        ))
                    }
                    ZoneFilter::Battlefield => Ok(ValueExpr::count_battlefield(obj)),
                    _ => Err("oracle:unsupported-count-zone"),
                };
            }
            if let Some(moved_zone) = prior_action_moved_count_zone(f) {
                if f.controller.is_some()
                    || !context.is_some_and(|(b, in_trig)| in_trig || !b.instructions.is_empty())
                {
                    return Err("oracle:unsupported-filter-predicate");
                }
                let obj = lower_object_filter(f, vocabulary)?;
                return Ok(ValueExpr::Count {
                    zone: moved_zone,
                    filter: Box::new(obj),
                    controller_scope: None,
                });
            }
            let controller_scope = match f.controller {
                Some(ControllerRelation::ThatPlayer) => {
                    Some(resolve_that_player_for_value_expr(context, true)?)
                }
                Some(ControllerRelation::TargetPlayer | ControllerRelation::TargetOpponent) => {
                    if (f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0 {
                        return Err("oracle:unsupported-target-player-count");
                    }
                    Some(ResolvingPlayer::TargetPlayer)
                }
                Some(ControllerRelation::DefendingPlayer) => Some(ResolvingPlayer::DefendingPlayer),
                Some(ControllerRelation::ActivePlayer) => Some(ResolvingPlayer::ActivePlayer),
                Some(ControllerRelation::ItsController) => {
                    let rp = match context {
                        Some((builder, true))
                            if !body_has_object_or_spell_target(builder.target) =>
                        {
                            ResolvingPlayer::TriggerSubjectController
                        }
                        Some((builder, false))
                            if !body_has_object_or_spell_target(builder.target) =>
                        {
                            return Err("oracle:unbound-target-controller");
                        }
                        _ => ResolvingPlayer::TargetController,
                    };
                    Some(rp)
                }
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
            let allow_spell_controller = matches!(
                stat,
                PlayerStatKind::CardsInHand | PlayerStatKind::CardsInGraveyard
            );
            let role = match player {
                PlayerRefAst::You => ResolvingPlayer::Controller,
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent => {
                    ResolvingPlayer::TargetPlayer
                }
                PlayerRefAst::ThatPlayer => {
                    resolve_that_player_for_value_expr(context, allow_spell_controller)?
                }
                PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController => match context {
                    Some((builder, true)) if !body_has_object_or_spell_target(builder.target) => {
                        ResolvingPlayer::TriggerSubjectController
                    }
                    Some((builder, false)) if !body_has_object_or_spell_target(builder.target) => {
                        return Err("oracle:unsupported-player-stat");
                    }
                    _ => ResolvingPlayer::TargetController,
                },
                PlayerRefAst::ItsOwner => match context {
                    Some((builder, true)) if !body_has_object_or_spell_target(builder.target) => {
                        ResolvingPlayer::TriggerSubjectOwner
                    }
                    Some((builder, false)) if !body_has_object_or_spell_target(builder.target) => {
                        return Err("oracle:unsupported-player-stat");
                    }
                    _ => ResolvingPlayer::TargetOwner,
                },
                _ => return Err("oracle:unsupported-player-stat"),
            };
            let use_count_zone = matches!(
                role,
                ResolvingPlayer::Controller
                    | ResolvingPlayer::TargetPlayer
                    | ResolvingPlayer::TargetSpellController
            );
            match stat {
                PlayerStatKind::LifeTotal => Ok(if role == ResolvingPlayer::Controller {
                    ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                } else {
                    ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(role))
                }),
                PlayerStatKind::CardsInHand => Ok(if use_count_zone {
                    ValueExpr::count_hand(role)
                } else {
                    ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(role))
                }),
                PlayerStatKind::CardsInGraveyard => Ok(if use_count_zone {
                    ValueExpr::count_graveyard(role, ObjectFilter::any())
                } else {
                    ValueExpr::PlayerStat(PlayerStatExpr::PlayerGraveyardSize(role))
                }),
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
            let st = lower_stat_kind(*stat);
            let subj = match peel_battlefield_subject(subject) {
                SubjectRefAst::SelfObject => ValueObjectRef::AbilitySource,
                SubjectRefAst::TriggerSubject => ValueObjectRef::DyingCreature,
                SubjectRefAst::AttachedHost(_) => ValueObjectRef::AttachedHost,
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } => {
                    let Some((builder, _)) = context else {
                        return Err("oracle:unsupported-stat-subject");
                    };
                    if target_filter_matches_body_target(filter, builder.target, vocabulary) {
                        ValueObjectRef::TargetObject
                    } else {
                        return Err("oracle:unsupported-stat-subject");
                    }
                }
                SubjectRefAst::Anaphora(raw) => {
                    let low = raw.trim().to_ascii_lowercase();
                    let prior_reveal_top = has_prior_reveal_top(context);
                    let prior_reveal_other = has_prior_reveal_other(context);
                    let prior_exile_target = context.is_some_and(|(b, _)| {
                        b.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::ZoneOp(spec)
                                    if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                                        && matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Exile { .. })
                            )
                        })
                    });
                    let prior_exile_non_target = has_prior_exile_non_target(context);
                    let prior_single_sacrifice = has_prior_single_sacrifice(context);
                    let prior_mill = context.is_some_and(|(b, _)| {
                        b.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::MutatePlayer(spec)
                                    if matches!(
                                        spec.op,
                                        spellwake_engine::toolbox::PlayerMutation::Mill(_)
                                    )
                            )
                        })
                    });
                    let prior_discard = context.is_some_and(|(b, _)| {
                        b.instructions.iter().any(|op| {
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(
                                op,
                            )
                            .is_some_and(|spec| {
                                matches!(
                                    spec.destination,
                                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                                        semantics:
                                            spellwake_engine::toolbox::GraveyardSemantics::Discard
                                    }
                                )
                            })
                        })
                    });
                    let no_target = context.is_none_or(|(b, _)| b.target == BodyTarget::None);
                    let has_obj_target =
                        context.is_some_and(|(b, _)| body_has_object_or_spell_target(b.target));

                    if is_self_object_anaphora(&low) {
                        ValueObjectRef::AbilitySource
                    } else if low.contains("amassed army") || low.contains("army you amassed") {
                        ValueObjectRef::LastAmassedArmy
                    } else if low.contains("creature that died") || low.contains("dying creature") {
                        ValueObjectRef::DyingCreature
                    } else if low.contains("revealed card") || low.contains("revealed creature") {
                        if prior_reveal_top {
                            ValueObjectRef::RevealedTop
                        } else if prior_reveal_other {
                            return Err("oracle:unsupported-stat-subject");
                        } else {
                            ValueObjectRef::RevealedCostCard
                        }
                    } else if low.contains("exiled card") || low.contains("exiled creature") {
                        if !prior_exile_non_target && prior_exile_target && has_obj_target {
                            ValueObjectRef::TargetObject
                        } else if !prior_exile_non_target && !prior_exile_target && no_target {
                            ValueObjectRef::ExiledCostCreature
                        } else {
                            return Err("oracle:unsupported-stat-subject");
                        }
                    } else if low.contains("sacrificed creature")
                        || low.contains("sacrificed permanent")
                    {
                        if prior_single_sacrifice {
                            ValueObjectRef::LastSacrificedCreature
                        } else if low.contains("sacrificed permanent")
                            || st == ObjectStatKind::ManaValue
                        {
                            ValueObjectRef::SacrificedPermanent
                        } else {
                            ValueObjectRef::SacrificedCreature
                        }
                    } else if low.contains("sacrificed artifact")
                        || low.contains("sacrificed enchantment")
                        || low.contains("sacrificed land")
                        || low.contains("sacrificed planeswalker")
                    {
                        if !prior_single_sacrifice && st == ObjectStatKind::ManaValue {
                            ValueObjectRef::SacrificedPermanent
                        } else {
                            return Err("oracle:unsupported-stat-subject");
                        }
                    } else if low.contains("milled card") || low.contains("milled creature") {
                        if st == ObjectStatKind::ManaValue {
                            if context.is_some() {
                                if prior_mill {
                                    return Ok(ValueExpr::EventValue(
                                        EventValueKind::MilledCardsManaValueSum,
                                    ));
                                }
                                return Err("oracle:unsupported-stat-subject");
                            }
                            ValueObjectRef::TargetObject
                        } else {
                            return Err("oracle:unsupported-stat-subject");
                        }
                    } else if low.contains("discarded")
                        || low.contains("ring-bearer")
                        || low.contains("chosen card")
                        || low.contains("returned card")
                        || low.contains("they exiled")
                        || low.contains("you exiled")
                    {
                        return Err("oracle:unsupported-stat-subject");
                    } else if let Some((builder, in_trigger)) = context {
                        let is_pronoun = matches!(low.as_str(), "it" | "its");
                        if prior_reveal_top
                            && !has_obj_target
                            && (is_pronoun || matches!(low.as_str(), "that card" | "the card"))
                        {
                            ValueObjectRef::RevealedTop
                        } else if prior_single_sacrifice
                            && !has_obj_target
                            && (is_pronoun
                                || matches!(low.as_str(), "that creature" | "that permanent"))
                        {
                            ValueObjectRef::LastSacrificedCreature
                        } else if (prior_exile_non_target || prior_discard)
                            && !has_obj_target
                            && (is_pronoun
                                || matches!(
                                    low.as_str(),
                                    "that card" | "that creature" | "that permanent"
                                ))
                        {
                            return Err("oracle:unsupported-stat-subject");
                        } else if prior_mill
                            && !has_obj_target
                            && st == ObjectStatKind::ManaValue
                            && (is_pronoun || low == "that card")
                        {
                            return Ok(ValueExpr::EventValue(
                                EventValueKind::MilledCardsManaValueSum,
                            ));
                        } else if in_trigger
                            && matches!(low.as_str(), "that creature" | "the creature")
                            && !body_target_guarantees_creature(builder.target)
                        {
                            ValueObjectRef::TriggerSubject
                        } else if has_obj_target {
                            ValueObjectRef::TargetObject
                        } else if in_trigger {
                            ValueObjectRef::TriggerSubject
                        } else if is_pronoun
                            && let Some(prior_ref) =
                                find_prior_cost_object_ref(&builder.instructions)
                        {
                            prior_ref
                        } else {
                            return Err("oracle:unsupported-stat-subject");
                        }
                    } else if matches!(
                        low.as_str(),
                        "it" | "its"
                            | "his"
                            | "her"
                            | "their"
                            | "that creature"
                            | "that permanent"
                            | "that card"
                            | "that spell"
                            | "that artifact"
                            | "that enchantment"
                            | "that land"
                            | "that planeswalker"
                            | "that battle"
                            | "that vehicle"
                            | "that token"
                            | "the creature"
                            | "the permanent"
                            | "the card"
                            | "the spell"
                            | "that blocked creature"
                            | "the chosen creature"
                            | "the targeted creature"
                    ) {
                        ValueObjectRef::TargetObject
                    } else {
                        return Err("oracle:unsupported-stat-subject");
                    }
                }
                _ => return Err("oracle:unsupported-stat-subject"),
            };
            Ok(ValueExpr::ObjectStat {
                subject: subj,
                stat: st,
            })
        }
        ValueExprAst::AggregateStat { op, stat, filter } => {
            if filter.any_spell {
                let other_spells_this_turn = *op == AggregateOp::Sum
                    && *stat == StatKind::ManaValue
                    && matches!(filter.controller, None | Some(ControllerRelation::You))
                    && !filter.any_permanent
                    && !filter.any_card
                    && filter.flags.0
                        == (AstFilterFlags::EXCLUDE_SELF.0 | AstFilterFlags::ENTERED_THIS_TURN.0)
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            controller: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    );
                if other_spells_this_turn {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::OtherSpellsCastManaValueThisTurn,
                    ));
                }
                return Err("oracle:unsupported-aggregate-spell");
            }
            let agg = match op {
                AggregateOp::Max => StatAggregate::Max,
                AggregateOp::Min => StatAggregate::Min,
                AggregateOp::Sum => StatAggregate::Sum,
            };
            let st = lower_stat_kind(*stat);
            let obj = lower_object_filter(filter, vocabulary)?;
            let moved_onto_battlefield = filter.predicates.iter().any(|p| {
                matches!(
                    p,
                    FilterPredicateAst::PriorActionZoneRef { kind }
                        if kind == "put_onto_battlefield_this_way"
                )
            });
            if obj.has_flag(ObjectFilterFlags::IN_LAST_MOVED) && !moved_onto_battlefield {
                return Err("oracle:unsupported-filter-predicate");
            }
            Ok(ValueExpr::aggregate(obj, st, agg))
        }
        ValueExprAst::PlayerCounterCount {
            player,
            counter_kind,
        } => lower_player_counter_value_expr(*player, counter_kind, context),
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } => {
            let peeled = peel_battlefield_subject(subject);
            let kind = if counter_kind.eq_ignore_ascii_case("any") || counter_kind.trim().is_empty()
            {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?)
            };
            let subj = match peeled {
                SubjectRefAst::SelfObject => {
                    spellwake_engine::program::CounterSubjectRef::AbilitySource
                }
                SubjectRefAst::TriggerSubject => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSubject
                }
                SubjectRefAst::AttachedHost(_) => {
                    spellwake_engine::program::CounterSubjectRef::AttachedHost
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } => {
                    let Some((builder, _)) = context else {
                        return Err("oracle:unsupported-counter-subject");
                    };
                    if target_filter_matches_body_target(filter, builder.target, vocabulary) {
                        spellwake_engine::program::CounterSubjectRef::TargetObject
                    } else {
                        return Err("oracle:unsupported-counter-subject");
                    }
                }
                SubjectRefAst::Anaphora(raw) => {
                    let low = raw.trim().to_ascii_lowercase();
                    if is_self_object_anaphora(&low) {
                        spellwake_engine::program::CounterSubjectRef::AbilitySource
                    } else if let Some((builder, in_trigger)) = context {
                        let has_obj_target = body_has_object_or_spell_target(builder.target);
                        let has_prior_source_counter_op = builder.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::MutateObject(spec)
                                    if matches!(
                                        spec.op,
                                        spellwake_engine::toolbox::ObjectMutation::AddCounters { .. }
                                    ) && (matches!(
                                        spec.source,
                                        spellwake_engine::toolbox::ObjectQuery::Source
                                    ) || matches!(
                                        &spec.source,
                                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(s)
                                            if matches!(s.as_ref(), EffectSubject::AbilitySource)
                                    ))
                            )
                        });
                        if !has_obj_target
                            && has_prior_source_counter_op
                            && matches!(low.as_str(), "it" | "its")
                        {
                            spellwake_engine::program::CounterSubjectRef::AbilitySource
                        } else if has_obj_target {
                            spellwake_engine::program::CounterSubjectRef::TargetObject
                        } else if in_trigger {
                            spellwake_engine::program::CounterSubjectRef::TriggerSubject
                        } else {
                            return Err("oracle:unsupported-counter-subject");
                        }
                    } else {
                        spellwake_engine::program::CounterSubjectRef::TargetObject
                    }
                }
                _ => return Err("oracle:unsupported-counter-subject"),
            };
            Ok(ValueExpr::CounterCount {
                subject: subj,
                kind,
            })
        }
        ValueExprAst::Add(a, b) => Ok(ValueExpr::Add(
            Box::new(lower_value_expr_inner(a, vocabulary, context)?),
            Box::new(lower_value_expr_inner(b, vocabulary, context)?),
        )),
        ValueExprAst::Sub(a, b) => Ok(ValueExpr::Sub(
            Box::new(lower_value_expr_inner(a, vocabulary, context)?),
            Box::new(lower_value_expr_inner(b, vocabulary, context)?),
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
                    expr: Box::new(lower_value_expr_inner(inner, vocabulary, context)?),
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
                expr: Box::new(lower_value_expr_inner(expr, vocabulary, context)?),
                divisor: d,
                round_up: *rounding == Rounding::Up,
            })
        }
        ValueExprAst::PriorResult => Ok(ValueExpr::EventValue(EventValueKind::LastActionCount)),
        ValueExprAst::RollDie { .. } => Ok(ValueExpr::EventValue(EventValueKind::LastDieRoll)),
        ValueExprAst::ExtremeStatAmong {
            stat,
            greatest,
            filter,
            or_toughness: false,
        } => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) || filter.any_spell {
                return Err("oracle:unsupported-value-expr");
            }
            let obj = lower_object_filter(filter, vocabulary)?;
            let agg = if *greatest {
                StatAggregate::Max
            } else {
                StatAggregate::Min
            };
            Ok(ValueExpr::aggregate(obj, lower_stat_kind(*stat), agg))
        }
        ValueExprAst::DistinctCountAmong { category, filter } => {
            match category.trim().to_ascii_lowercase().as_str() {
                "different_powers" | "different powers" => {
                    if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        || filter.any_types != AstCardTypeMask::CREATURE
                        || filter.any_spell
                    {
                        return Err("oracle:unsupported-value-expr");
                    }
                    let obj = lower_object_filter(filter, vocabulary)?;
                    Ok(ValueExpr::aggregate(
                        obj,
                        ObjectStatKind::Power,
                        StatAggregate::CountDistinct,
                    ))
                }
                "colors" => {
                    if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        || filter.any_spell
                    {
                        return Err("oracle:unsupported-value-expr");
                    }
                    let obj = lower_object_filter(filter, vocabulary)?;
                    Ok(ValueExpr::DistinctColors {
                        filter: Box::new(obj),
                    })
                }
                "basic_land_types" | "basic land types" => {
                    if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !filter.any_spell
                        && filter.any_types == AstCardTypeMask::LAND
                        && filter.controller == Some(ControllerRelation::You)
                        && filter_only_has(
                            filter,
                            FilterAllow {
                                types: true,
                                controller: true,
                                zone: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        Ok(ValueExpr::DomainCount)
                    } else {
                        Err("oracle:unsupported-value-expr")
                    }
                }
                "different_mana_values" | "different mana values" => {
                    if filter.any_permanent
                        || filter.any_spell
                        || !filter_only_has(
                            filter,
                            FilterAllow {
                                zone: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Err("oracle:unsupported-value-expr");
                    }
                    let rp = match filter.zone {
                        Some(ZoneFilter::YourGraveyard) => ResolvingPlayer::Controller,
                        Some(
                            ZoneFilter::TargetPlayerGraveyard | ZoneFilter::TargetOpponentGraveyard,
                        ) => ResolvingPlayer::TargetPlayer,
                        Some(ZoneFilter::DefendingPlayerGraveyard) => {
                            ResolvingPlayer::DefendingPlayer
                        }
                        Some(ZoneFilter::ThatPlayerGraveyard) => {
                            resolve_that_player_for_value_expr(context, true)?
                        }
                        _ => return Err("oracle:unsupported-value-expr"),
                    };
                    Ok(ValueExpr::DistinctManaValuesInGraveyard(rp))
                }
                "card_types" | "card types" => {
                    if filter.any_permanent
                        || filter.any_spell
                        || !filter_only_has(
                            filter,
                            FilterAllow {
                                zone: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return Err("oracle:unsupported-value-expr");
                    }
                    match filter.zone {
                        Some(ZoneFilter::YourGraveyard) => Ok(ValueExpr::PlayerStat(
                            PlayerStatExpr::OwnGraveyardDistinctCardTypes,
                        )),
                        Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard) => Ok(
                            ValueExpr::PlayerStat(PlayerStatExpr::AllGraveyardsDistinctCardTypes),
                        ),
                        _ => Err("oracle:unsupported-value-expr"),
                    }
                }
                _ => Err("oracle:unsupported-value-expr"),
            }
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            let t = lower_value_expr_inner(threshold, vocabulary, context)?;
            let a = lower_value_expr_inner(actual, vocabulary, context)?;
            Ok(ValueExpr::conditional(
                t.clone(),
                Comparison::Greater,
                a.clone(),
                ValueExpr::Sub(Box::new(t), Box::new(a)),
                ValueExpr::literal(0_i64),
            ))
        }
        ValueExprAst::AbsDiff(lhs, rhs) => {
            let a = lower_value_expr_inner(lhs, vocabulary, context)?;
            let b = lower_value_expr_inner(rhs, vocabulary, context)?;
            Ok(ValueExpr::conditional(
                a.clone(),
                Comparison::GreaterOrEqual,
                b.clone(),
                ValueExpr::Sub(Box::new(a.clone()), Box::new(b.clone())),
                ValueExpr::Sub(Box::new(b), Box::new(a)),
            ))
        }
        ValueExprAst::Pow { base, exp } => match (base.as_ref(), exp.as_ref()) {
            (ValueExprAst::Const(b), ValueExprAst::Const(e)) => {
                if !(0..=16).contains(e) {
                    return Err("oracle:unsupported-value-expr");
                }
                let exp_u32 = u32::try_from(*e).map_err(|_| "oracle:unsupported-value-expr")?;
                let val = i64::from(*b)
                    .checked_pow(exp_u32)
                    .ok_or("oracle:unsupported-value-expr")?;
                Ok(ValueExpr::literal(val))
            }
            _ => Err("oracle:unsupported-value-expr"),
        },
        ValueExprAst::ColorsOfManaSpentOn(subj) => {
            let is_self_or_that_spell = match subj.as_ref() {
                SubjectRefAst::SelfObject => true,
                SubjectRefAst::Anaphora(s) => {
                    matches!(s.trim().to_ascii_lowercase().as_str(), "that spell" | "it")
                }
                _ => false,
            };
            if is_self_or_that_spell {
                Ok(ValueExpr::event(EventValueKind::ColorsSpentToCast))
            } else {
                Err("oracle:unsupported-value-expr")
            }
        }
        ValueExprAst::DamagePreventedThisWay => {
            Ok(ValueExpr::event(EventValueKind::DamagePreventedThisWay))
        }
        ValueExprAst::DamageDealtToSubjectThisTurn(subj) => {
            let obj_ref = match subj.as_ref() {
                SubjectRefAst::SelfObject => ValueObjectRef::AbilitySource,
                SubjectRefAst::AttachedHost(_) => ValueObjectRef::AttachedHost,
                SubjectRefAst::TriggerSubject => ValueObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(raw) => resolve_damage_subject_ref(raw, context)?,
                _ => return Err("oracle:unsupported-value-expr"),
            };
            Ok(ValueExpr::PlayerStat(
                PlayerStatExpr::DamageReceivedThisTurn {
                    subject: obj_ref,
                    excess_only: false,
                },
            ))
        }
        ValueExprAst::EventCountThisTurn { event, player } => {
            let norm = event.trim().to_ascii_lowercase();
            match norm.as_str() {
                "the amount of {e} you have"
                    if matches!(player, None | Some(PlayerRefAst::You)) =>
                {
                    Ok(ValueExpr::counters(
                        spellwake_engine::program::CounterSubjectRef::ControllerEnergy,
                        None,
                    ))
                }
                "the number of permanents you've sacrificed this turn"
                    if matches!(player, None | Some(PlayerRefAst::You)) =>
                {
                    Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerSacrificedThisTurn,
                    ))
                }
                "the number of cards you've discarded this turn"
                | "card you've cycled or discarded this turn"
                    if matches!(player, None | Some(PlayerRefAst::You)) =>
                {
                    Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerDiscardedThisTurn,
                    ))
                }
                "the total amount of life your opponents have lost this turn"
                    if player.is_none() =>
                {
                    Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::OpponentsLifeLostThisTurn,
                    ))
                }
                "the number of permanents destroyed this way" if player.is_none() => {
                    Ok(ValueExpr::event(EventValueKind::DestroyedPermanents))
                }
                "the result" | "that result" if player.is_none() => {
                    Ok(ValueExpr::event(EventValueKind::LastDieRoll))
                }
                "the sacrificed creature's power" if player.is_none() => {
                    let subject = if has_prior_single_sacrifice(context) {
                        ValueObjectRef::LastSacrificedCreature
                    } else {
                        ValueObjectRef::SacrificedCreature
                    };
                    Ok(ValueExpr::object_stat(subject, ObjectStatKind::Power))
                }
                "the exiled creature card's power" | "the exiled card's power"
                    if player.is_none() =>
                {
                    if has_prior_exile_non_target(context) {
                        return Err("oracle:unsupported-value-expr");
                    }
                    Ok(ValueExpr::object_stat(
                        ValueObjectRef::ExiledCostCreature,
                        ObjectStatKind::Power,
                    ))
                }
                "the exiled card's mana value" | "the exiled creature card's mana value"
                    if player.is_none() =>
                {
                    if has_prior_exile_non_target(context) {
                        return Err("oracle:unsupported-value-expr");
                    }
                    Ok(ValueExpr::object_stat(
                        ValueObjectRef::ExiledCostCreature,
                        ObjectStatKind::ManaValue,
                    ))
                }
                "the revealed card's power" | "the revealed creature card's power"
                    if player.is_none() =>
                {
                    let subject = if has_prior_reveal_top(context) {
                        ValueObjectRef::RevealedTop
                    } else if has_prior_reveal_other(context) {
                        return Err("oracle:unsupported-value-expr");
                    } else {
                        ValueObjectRef::RevealedCostCard
                    };
                    Ok(ValueExpr::object_stat(subject, ObjectStatKind::Power))
                }
                "the revealed card's mana value" | "the revealed creature card's mana value"
                    if player.is_none() =>
                {
                    let subject = if has_prior_reveal_top(context) {
                        ValueObjectRef::RevealedTop
                    } else if has_prior_reveal_other(context) {
                        return Err("oracle:unsupported-value-expr");
                    } else {
                        ValueObjectRef::RevealedCostCard
                    };
                    Ok(ValueExpr::object_stat(subject, ObjectStatKind::ManaValue))
                }
                "half x" if player.is_none() => Ok(ValueExpr::chosen_x().half_down()),
                "half that many" if player.is_none() => {
                    Ok(ValueExpr::event(EventValueKind::LastActionCount).half_down())
                }
                "x minus one" if player.is_none() => Ok(ValueExpr::Sub(
                    Box::new(ValueExpr::chosen_x()),
                    Box::new(ValueExpr::literal(1_i64)),
                )),
                "x plus 2" if player.is_none() => Ok(ValueExpr::Add(
                    Box::new(ValueExpr::chosen_x()),
                    Box::new(ValueExpr::literal(2_i64)),
                )),
                "the number of creatures that died this turn" if player.is_none() => {
                    Ok(ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn))
                }
                "the number of cards put into your graveyard this turn"
                | "the number of cards put into your graveyard from anywhere this turn"
                    if matches!(player, None | Some(PlayerRefAst::You)) =>
                {
                    Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::CardsEnteredGraveyardThisTurn,
                    ))
                }
                "creature you attacked with this turn"
                    if matches!(player, None | Some(PlayerRefAst::You)) =>
                {
                    Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::AttackingCreaturesThisTurn,
                    ))
                }
                "half the highest life total among your opponents, rounded up"
                    if player.is_none() =>
                {
                    let two = NonZeroU16::new(2).ok_or("oracle:div-zero")?;
                    Ok(ValueExpr::DivConst {
                        expr: Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)),
                        divisor: two,
                        round_up: true,
                    })
                }
                "aura and equipment attached to ~"
                | "aura or equipment attached to ~"
                | "auras and equipment attached to ~"
                | "auras or equipment attached to ~"
                    if player.is_none() =>
                {
                    Ok(ValueExpr::AttachedCount(
                        spellwake_engine::program::AttachedCountKind::AurasAndEquipmentOnSource,
                    ))
                }
                "other creature you control" if player.is_none() => {
                    Ok(ValueExpr::count_battlefield(ObjectFilter::from_clause(
                        ObjectClause::creature()
                            .controlled()
                            .other_than_ability_source(),
                    )))
                }
                "other red creature you control" if player.is_none() => {
                    Ok(ValueExpr::count_battlefield(ObjectFilter::from_clause(
                        ObjectClause::creature()
                            .controlled()
                            .other_than_ability_source()
                            .with_color(ObjectColorFilter::has(Color::Red)),
                    )))
                }
                "other green creature you control" if player.is_none() => {
                    Ok(ValueExpr::count_battlefield(ObjectFilter::from_clause(
                        ObjectClause::creature()
                            .controlled()
                            .other_than_ability_source()
                            .with_color(ObjectColorFilter::has(Color::Green)),
                    )))
                }
                other => try_lower_event_count_this_turn_ext(other, *player, vocabulary, context)
                    .unwrap_or(Err("oracle:unsupported-value-expr")),
            }
        }
        ValueExprAst::CounterCountAmong {
            counter_kind,
            filter,
        } => {
            if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.any_spell
                && let Ok(ObjectFilter::Clause(clause)) = lower_object_filter(filter, vocabulary)
                && clause.source_relation == SourceRelation::SelfObject
            {
                let kind =
                    if counter_kind.eq_ignore_ascii_case("any") || counter_kind.trim().is_empty() {
                        None
                    } else {
                        let ck = parse_counter_kind(counter_kind)
                            .ok_or("oracle:unsupported-counter-kind")?;
                        if !ck.is_permanent() {
                            return Err("oracle:unsupported-counter-kind");
                        }
                        Some(ck)
                    };
                Ok(ValueExpr::counters(
                    spellwake_engine::program::CounterSubjectRef::AbilitySource,
                    kind,
                ))
            } else {
                Err("oracle:unsupported-value-expr")
            }
        }
        ValueExprAst::Max(a, b) => {
            let la = lower_value_expr_inner(a, vocabulary, context)?;
            let lb = lower_value_expr_inner(b, vocabulary, context)?;
            Ok(ValueExpr::conditional(
                la.clone(),
                Comparison::GreaterOrEqual,
                lb.clone(),
                la,
                lb,
            ))
        }
        ValueExprAst::Min(a, b) => {
            let la = lower_value_expr_inner(a, vocabulary, context)?;
            let lb = lower_value_expr_inner(b, vocabulary, context)?;
            Ok(ValueExpr::conditional(
                la.clone(),
                Comparison::LessOrEqual,
                lb.clone(),
                la,
                lb,
            ))
        }
        ValueExprAst::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let rc = if let Some((builder, in_trigger)) = context {
                let mut probe = builder.clone();
                let rc =
                    lower_resolution_condition(condition, &mut probe, vocabulary, in_trigger, None)
                        .map_err(|_| "oracle:unsupported-value-expr")?;
                if probe.target != builder.target {
                    return Err("oracle:unsupported-value-expr");
                }
                rc
            } else {
                let mut probe = BodyBuilder::new();
                let rc = lower_resolution_condition(condition, &mut probe, vocabulary, false, None)
                    .map_err(|_| "oracle:unsupported-value-expr")?;
                if probe.target != BodyTarget::None {
                    return Err("oracle:unsupported-value-expr");
                }
                rc
            };
            let then_val = lower_value_expr_inner(then_expr, vocabulary, context)?;
            let else_val = lower_value_expr_inner(else_expr, vocabulary, context)?;
            Ok(ValueExpr::conditional(
                rc.left,
                rc.comparison,
                rc.right,
                then_val,
                else_val,
            ))
        }
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

fn parse_planeswalker_subtype(label: &str) -> Option<PlaneswalkerSubtype> {
    let trimmed = label.trim();
    PlaneswalkerSubtype::from_label(trimmed).or_else(|| {
        PlaneswalkerSubtype::ALL
            .iter()
            .copied()
            .find(|pw| pw.label().eq_ignore_ascii_case(trimmed))
    })
}

pub(super) fn parse_counter_kind(raw: &str) -> Option<CounterKind> {
    let lower = raw.trim().to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "{e}" | "e" | "energy" => "energy",
        "{tk}" | "tk" | "ticket" | "tickets" => "ticket",
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
    pub(super) color_arity: bool,
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
        color_arity: false,
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
        owner,
        zone,
        extra_zones,
        combat,
        tapped,
        token,
        required_keywords,
        forbidden_keywords,
        stat_bounds,
        counter_kind,
        flags,
        predicates,
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
        && (allow.color_arity || color_arity.is_none())
        && (allow.controller || controller.is_none())
        && owner.is_none()
        && (allow.zone || zone.is_none())
        && extra_zones.is_empty()
        && (allow.combat || combat.is_none())
        && tapped.is_none()
        && (allow.token || token.is_none())
        && (allow.req_keywords || required_keywords.is_empty())
        && forbidden_keywords.is_empty()
        && (allow.stat_bounds || stat_bounds.is_empty())
        && (allow.counter_kind || counter_kind.is_none())
        && (allow.flags || (flags.0 & !AstFilterFlags::LKI_PAST_TENSE.0) == 0)
        && predicates.is_empty()
        && !*is_ability
        && named.is_none()
}

pub(super) fn is_unrestricted_filter(f: &ObjectFilterAst) -> bool {
    filter_only_has(f, FilterAllow::NONE) && !f.any_permanent
}

pub(super) fn chooser_matches_actor(chooser: PlayerRefAst, actor: PlayerRefAst) -> bool {
    match actor {
        PlayerRefAst::You => chooser == PlayerRefAst::You,
        PlayerRefAst::EachOpponent
        | PlayerRefAst::AnyOpponent
        | PlayerRefAst::TargetOpponent
        | PlayerRefAst::EachPlayer
        | PlayerRefAst::AnyPlayer
        | PlayerRefAst::TargetPlayer
        | PlayerRefAst::EachOtherPlayer
        | PlayerRefAst::ThatPlayer
        | PlayerRefAst::DefendingPlayer
        | PlayerRefAst::ActivePlayer => chooser == actor || chooser == PlayerRefAst::ThatPlayer,
        _ => chooser == actor,
    }
}

pub(super) fn strip_matching_chooser(f: &ObjectFilterAst, actor: PlayerRefAst) -> ObjectFilterAst {
    if !f.predicates.iter().any(|p| {
        matches!(
            p,
            FilterPredicateAst::ChosenByPlayer(c) if chooser_matches_actor(*c, actor)
        )
    }) {
        return f.clone();
    }
    let mut out = f.clone();
    out.predicates.retain(|p| {
        !matches!(
            p,
            FilterPredicateAst::ChosenByPlayer(c) if chooser_matches_actor(*c, actor)
        )
    });
    out
}

#[cfg(test)]
mod wave12_adventure_filter_tests {
    use super::*;

    #[test]
    fn wave12_typed_adventure_predicate_preserves_qualification() {
        let vocabulary =
            CreatureTypeVocabulary::new("wave12-adventure".into(), [0; 32], vec!["Human".into()])
                .expect("test vocabulary");
        let mut filter = ObjectFilterAst::default();
        filter.predicates.push(FilterPredicateAst::HasAbilityKind {
            kind: "adventure".into(),
            negated: false,
        });
        assert!(
            lower_object_filter(&filter, &vocabulary)
                .expect("adventure filter")
                .has_flag(ObjectFilterFlags::HAS_ADVENTURE)
        );
        filter.predicates = vec![FilterPredicateAst::HasAbilityKind {
            kind: "adventure".into(),
            negated: true,
        }];
        assert!(lower_object_filter(&filter, &vocabulary).is_err());
        filter.predicates = vec![FilterPredicateAst::HasAbilityKind {
            kind: "unrepresented".into(),
            negated: false,
        }];
        assert!(lower_object_filter(&filter, &vocabulary).is_err());
    }

    #[test]
    fn phase12_types_match_all_and_honest_predicates() {
        let vocabulary = CreatureTypeVocabulary::new(
            "phase12-filters".into(),
            [0; 32],
            vec![
                "Cleric".into(),
                "Human".into(),
                "Ooze".into(),
                "Rogue".into(),
                "Warrior".into(),
                "Wizard".into(),
            ],
        )
        .expect("test vocabulary");

        // Conjunctive multi-type filters (artifact creature / enchantment creature)
        let mut art_creature = ObjectFilterAst {
            any_types: AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE),
            ..Default::default()
        };
        art_creature
            .predicates
            .push(FilterPredicateAst::TypesMatchAll);
        let lowered = lower_object_filter(&art_creature, &vocabulary)
            .expect("artifact creature filter should lower");
        let (clause, second) = lowered.clauses();
        assert!(second.is_none());
        assert_eq!(
            clause.required_types,
            CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)
        );
        assert_eq!(clause.any_types, CardTypeMask::CREATURE);
        assert!(lowered.guarantees_creature());

        let sel = lower_filter_to_permanent_selector(&art_creature, &vocabulary)
            .expect("artifact creature selector should lower");
        assert!(sel.required_types.contains(&CardType::Artifact));
        assert!(sel.required_types.contains(&CardType::Creature));
        assert!(sel.any_types.is_empty());

        // SharesPropertyWith mana_value against source
        let mut same_mv = ObjectFilterAst {
            any_types: AstCardTypeMask::CREATURE,
            ..Default::default()
        };
        same_mv
            .predicates
            .push(FilterPredicateAst::SharesPropertyWith {
                property: "mana_value".into(),
                reference: Box::new(SubjectRefAst::SelfObject),
                negated: false,
            });
        let same_mv_lowered =
            lower_object_filter(&same_mv, &vocabulary).expect("same mana value should lower");
        assert_eq!(
            same_mv_lowered.clauses().0.mana_value,
            ManaValueFilter::CompareAnchor {
                op: Comparison::Equal,
                anchor: spellwake_engine::program::DynamicStatAnchor::SourceManaValue,
            }
        );

        // Spell cast-zone state qualifications
        let mut gy_spell = ObjectFilterAst {
            any_spell: true,
            ..Default::default()
        };
        gy_spell
            .predicates
            .push(FilterPredicateAst::StateQualification(
                "cast_from_graveyard".into(),
            ));
        let sf = lower_spell_filter(&gy_spell, &vocabulary).expect("cast_from_graveyard lowers");
        assert_eq!(
            sf.cast_zone,
            spellwake_engine::program::SpellCastZoneFilter::Graveyard
        );

        let mut outside_hand = ObjectFilterAst {
            any_spell: true,
            ..Default::default()
        };
        outside_hand
            .predicates
            .push(FilterPredicateAst::StateQualification(
                "cast from outside hand".into(),
            ));
        let sf_out =
            lower_spell_filter(&outside_hand, &vocabulary).expect("cast from outside hand lowers");
        assert_eq!(
            sf_out.cast_zone,
            spellwake_engine::program::SpellCastZoneFilter::NotOwnersHand
        );

        // Fail closed on genuine engine gaps (AttachedTo arbitrary host filter, dealt_damage_this_way)
        let mut aura_on_land = ObjectFilterAst {
            subtypes: vec!["Aura".into()],
            ..Default::default()
        };
        aura_on_land
            .predicates
            .push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::Group(Box::new(ObjectFilterAst {
                    any_types: AstCardTypeMask::LAND,
                    ..Default::default()
                })),
            )));
        assert!(lower_object_filter(&aura_on_land, &vocabulary).is_err());

        let mut dealt_this_way = ObjectFilterAst {
            any_types: AstCardTypeMask::CREATURE,
            ..Default::default()
        };
        dealt_this_way
            .predicates
            .push(FilterPredicateAst::PriorActionZoneRef {
                kind: "dealt_damage_this_way".into(),
            });
        assert!(lower_object_filter(&dealt_this_way, &vocabulary).is_err());
    }
}
