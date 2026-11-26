use super::*;

pub(super) fn lower_comparative_controls_action(
    act_low: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Option<Result<ResolutionCondition, &'static str>> {
    if let Some(after) = act_low
        .strip_prefix("an opponent has at least ")
        .or_else(|| act_low.strip_prefix("any opponent has at least "))
        && let Some(cnt_raw) = after.strip_suffix(" more cards in hand than you")
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::OpponentMaxCount {
                zone: ValueZone::Hand(ResolvingPlayer::Controller),
                filter: Box::new(ObjectFilter::any()),
            },
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::Add(
                Box::new(ValueExpr::count_hand(ResolvingPlayer::Controller)),
                Box::new(ValueExpr::literal(n)),
            ),
        }));
    }
    if let Some(after) = act_low
        .strip_prefix("an opponent controls at least ")
        .or_else(|| act_low.strip_prefix("any opponent controls at least "))
        && let Some(before_than) = after.strip_suffix(" than you")
        && let Some((cnt_raw, flt_raw)) = before_than.split_once(" more ")
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        let flt_clean = flt_raw.trim();
        let obj = match flt_clean {
            "creatures" | "creature" => ObjectFilter::creature(),
            "lands" | "land" => ObjectFilter::land(),
            "artifacts" | "artifact" => ObjectFilter::artifact(),
            "enchantments" | "enchantment" => ObjectFilter::enchantment(),
            "planeswalkers" | "planeswalker" => ObjectFilter::planeswalker(),
            "permanents" | "permanent" => ObjectFilter::permanent(),
            "nonland permanents" | "nonland permanent" => ObjectFilter::nonland(),
            other => {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let ast_f = match spellwake_oracle_compiler::filter::parse_object_filter(
                    other,
                    &compiler_vocab,
                ) {
                    Ok(f) if f.zone.is_none() && f.controller.is_none() && f.named.is_none() => f,
                    _ => return None,
                };
                match lower_object_filter(&ast_f, vocabulary) {
                    Ok(o) => o,
                    Err(e) => return Some(Err(e)),
                }
            }
        };
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::OpponentMaxCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj),
            },
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::Add(
                Box::new(ValueExpr::count_player_battlefield(
                    ResolvingPlayer::Controller,
                    obj,
                )),
                Box::new(ValueExpr::literal(n)),
            ),
        }));
    }
    let (p1_raw, cmp, rest) = if let Some((lhs, rhs)) = act_low
        .split_once(" controls more ")
        .or_else(|| act_low.split_once(" control more "))
    {
        (lhs.trim(), Comparison::Greater, rhs.trim())
    } else if let Some((lhs, rhs)) = act_low
        .split_once(" controls fewer ")
        .or_else(|| act_low.split_once(" control fewer "))
    {
        (lhs.trim(), Comparison::Less, rhs.trim())
    } else {
        return None;
    };
    let (flt_raw, p2_raw) = rest.split_once(" than ")?;
    let flt_clean = flt_raw.trim();
    let p2_clean = p2_raw.trim();
    let obj = match flt_clean {
        "creatures" | "creature" => ObjectFilter::creature(),
        "lands" | "land" => ObjectFilter::land(),
        "artifacts" | "artifact" => ObjectFilter::artifact(),
        "enchantments" | "enchantment" => ObjectFilter::enchantment(),
        "planeswalkers" | "planeswalker" => ObjectFilter::planeswalker(),
        "permanents" | "permanent" => ObjectFilter::permanent(),
        "nonland permanents" | "nonland permanent" => ObjectFilter::nonland(),
        other => {
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let ast_f = match spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &compiler_vocab,
            ) {
                Ok(f) if f.zone.is_none() && f.controller.is_none() && f.named.is_none() => f,
                _ => return None,
            };
            match lower_object_filter(&ast_f, vocabulary) {
                Ok(o) => o,
                Err(e) => return Some(Err(e)),
            }
        }
    };
    let resolve_single = |raw: &str,
                          b: &mut BodyBuilder|
     -> Option<Result<ResolvingPlayer, &'static str>> {
        match raw {
            "you" => Some(Ok(ResolvingPlayer::Controller)),
            "attacking player" | "the attacking player" => Some(Ok(ResolvingPlayer::ActivePlayer)),
            "defending player" | "the defending player" => {
                Some(Ok(ResolvingPlayer::DefendingPlayer))
            }
            "enchanted player" | "the enchanted player" => {
                Some(Ok(ResolvingPlayer::EnchantedPlayer))
            }
            "that player" | "they" => Some(lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                b,
                in_trigger,
            )),
            "target player" => Some(lower_resolving_player_in_context(
                PlayerRefAst::TargetPlayer,
                b,
                in_trigger,
            )),
            "target opponent" => Some(lower_resolving_player_in_context(
                PlayerRefAst::TargetOpponent,
                b,
                in_trigger,
            )),
            _ => None,
        }
    };
    let rp1 = match resolve_single(p1_raw, builder)? {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    if p2_clean == "each other player" && cmp == Comparison::Greater {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerControlsMost {
                player: rp1,
                zone: ValueZone::Battlefield,
                filter: Box::new(obj),
                allow_tie: false,
            }),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }));
    }
    let left = ValueExpr::count_player_battlefield(rp1, obj);
    let right = if rp1 == ResolvingPlayer::Controller
        && matches!(
            p2_clean,
            "an opponent" | "each opponent" | "each other player"
        ) {
        let use_max = matches!(
            (cmp, p2_clean),
            (Comparison::Greater, "each opponent" | "each other player")
                | (Comparison::Less, "an opponent")
        );
        if use_max {
            ValueExpr::OpponentMaxCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj),
            }
        } else {
            ValueExpr::OpponentMinCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj),
            }
        }
    } else {
        let rp2 = match resolve_single(p2_clean, builder)? {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        ValueExpr::count_player_battlefield(rp2, obj)
    };
    Some(Ok(ResolutionCondition {
        left,
        comparison: cmp,
        right,
    }))
}

pub(super) fn lower_mana_spent_on_triggering_spell_action(
    act_low: &str,
    in_trigger: bool,
) -> Option<ResolutionCondition> {
    if !in_trigger {
        return None;
    }
    let cnt_raw = act_low
        .strip_suffix(" or more mana was spent to cast that spell")
        .or_else(|| act_low.strip_suffix(" or more mana was spent to cast it"))
        .or_else(|| {
            act_low.strip_prefix("at least ").and_then(|s| {
                s.strip_suffix(" mana was spent to cast that spell")
                    .or_else(|| s.strip_suffix(" mana was spent to cast it"))
            })
        })?;
    let n = parse_small_positive_count(cnt_raw)?;
    Some(ResolutionCondition {
        left: ValueExpr::event(EventValueKind::TriggeringSpellManaSpent),
        comparison: Comparison::GreaterOrEqual,
        right: ValueExpr::literal(n),
    })
}

pub(super) fn lower_graveyard_count_action(
    act_low: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<ResolutionCondition, &'static str>> {
    let body = act_low.strip_suffix(" in your graveyard")?.trim();
    let (cnt_raw, flt_raw) = if let Some(after) = body
        .strip_prefix("there are at least ")
        .or_else(|| body.strip_prefix("you have at least "))
    {
        after.trim().split_once(' ')?
    } else {
        let after = body
            .strip_prefix("there are ")
            .or_else(|| body.strip_prefix("you have "))?;
        after.trim().split_once(" or more ")?
    };
    let n = parse_small_positive_count(cnt_raw)?;
    let flt_clean = flt_raw.trim();
    if flt_clean.is_empty()
        || flt_clean.contains("chosen")
        || flt_clean.contains("share")
        || flt_clean.contains("same ")
        || flt_clean.contains("named ")
        || flt_clean.contains(" with ")
        || flt_clean.contains(" that ")
        || flt_clean.contains(" whose ")
    {
        return None;
    }
    let obj = if matches!(flt_clean, "card" | "cards") {
        ObjectFilter::any()
    } else {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let parsed =
            spellwake_oracle_compiler::filter::parse_object_filter(flt_clean, &compiler_vocab)
                .ok()?;
        if is_unrestricted_filter(&parsed)
            || parsed.zone.is_some()
            || !parsed.extra_zones.is_empty()
            || parsed.controller.is_some()
            || parsed.owner.is_some()
            || parsed.named.is_some()
            || !parsed.predicates.is_empty()
            || !parsed.flags.is_empty()
        {
            return None;
        }
        match lower_object_filter(&parsed, vocabulary) {
            Ok(f) => f,
            Err(e) => return Some(Err(e)),
        }
    };
    Some(Ok(ResolutionCondition {
        left: ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj),
        comparison: Comparison::GreaterOrEqual,
        right: ValueExpr::literal(n),
    }))
}

pub(super) fn lower_zone_action_this_way_action(
    act_low: &str,
    builder: &BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<ResolutionCondition, &'static str>> {
    let core = act_low.strip_suffix(" this way")?.trim();
    if core.contains("chosen")
        || core.contains("share")
        || core.contains("same ")
        || core.contains("named ")
        || core.contains(" whose ")
        || core.contains(" that ")
        || core.contains(" with ")
        || core.contains(" from ")
    {
        return None;
    }
    let (subject_ref, subj_phrase) = if let Some(after_you) = core.strip_prefix("you ") {
        let after_you = after_you.trim();
        if let Some(s) = after_you
            .strip_prefix("sacrificed ")
            .or_else(|| after_you.strip_prefix("sacrifice "))
            .or_else(|| after_you.strip_prefix("discarded "))
            .or_else(|| after_you.strip_prefix("discard "))
            .or_else(|| after_you.strip_prefix("milled "))
            .or_else(|| after_you.strip_prefix("mill "))
            .or_else(|| after_you.strip_prefix("destroyed "))
            .or_else(|| after_you.strip_prefix("destroy "))
        {
            (MatchObjectRef::LastMovedToGraveyard, s.trim())
        } else if let Some(s) = after_you
            .strip_prefix("exiled ")
            .or_else(|| after_you.strip_prefix("exile "))
        {
            (MatchObjectRef::LastMovedToExile, s.trim())
        } else if let Some(s) = after_you
            .strip_prefix("returned ")
            .or_else(|| after_you.strip_prefix("return "))
            .and_then(|r| r.strip_suffix(" to your hand"))
            .or_else(|| {
                after_you
                    .strip_prefix("put ")
                    .and_then(|r| r.strip_suffix(" into your hand"))
            })
        {
            (MatchObjectRef::LastMovedToHand, s.trim())
        } else if let Some(s) = after_you.strip_prefix("put ").and_then(|r| {
            r.strip_suffix(" into your graveyard")
                .or_else(|| r.strip_suffix(" into a graveyard"))
        }) {
            (MatchObjectRef::LastMovedToGraveyard, s.trim())
        } else {
            let s = after_you
                .strip_prefix("put ")
                .and_then(|r| r.strip_suffix(" onto the battlefield"))?;
            (MatchObjectRef::LastMovedToBattlefield, s.trim())
        }
    } else if let Some(after_actor) = core
        .strip_prefix("the player ")
        .or_else(|| core.strip_prefix("that player "))
        .or_else(|| core.strip_prefix("an opponent "))
        .or_else(|| core.strip_prefix("a player "))
        .or_else(|| core.strip_prefix("they "))
    {
        let after_actor = after_actor.trim();
        if let Some(s) = after_actor
            .strip_prefix("mills ")
            .or_else(|| after_actor.strip_prefix("mill "))
            .or_else(|| after_actor.strip_prefix("milled "))
            .or_else(|| after_actor.strip_prefix("discards "))
            .or_else(|| after_actor.strip_prefix("discard "))
            .or_else(|| after_actor.strip_prefix("discarded "))
            .or_else(|| after_actor.strip_prefix("sacrifices "))
            .or_else(|| after_actor.strip_prefix("sacrifice "))
            .or_else(|| after_actor.strip_prefix("sacrificed "))
        {
            (MatchObjectRef::LastMovedToGraveyard, s.trim())
        } else {
            let s = after_actor
                .strip_prefix("exiles ")
                .or_else(|| after_actor.strip_prefix("exile "))
                .or_else(|| after_actor.strip_prefix("exiled "))?;
            (MatchObjectRef::LastMovedToExile, s.trim())
        }
    } else if let Some(s) = core
        .strip_suffix(" was returned")
        .or_else(|| core.strip_suffix(" is returned"))
        .or_else(|| core.strip_suffix(" were returned"))
        .or_else(|| core.strip_suffix(" are returned"))
    {
        if matches!(
            builder.instructions.last(),
            Some(
                Instruction::ReturnToOwnersHand
                    | Instruction::MoveSubject { .. }
                    | Instruction::MoveGroup { .. }
                    | Instruction::SelectZoneCardsToZone { .. }
            )
        ) {
            (MatchObjectRef::LastAffected, s.trim())
        } else {
            return None;
        }
    } else {
        const PASSIVE_VERBS: &[(&str, MatchObjectRef)] = &[
            (
                " was returned to its owner's hand",
                MatchObjectRef::LastMovedToHand,
            ),
            (
                " is returned to its owner's hand",
                MatchObjectRef::LastMovedToHand,
            ),
            (
                " was returned to your hand",
                MatchObjectRef::LastMovedToHand,
            ),
            (" is returned to your hand", MatchObjectRef::LastMovedToHand),
            (
                " were returned to your hand",
                MatchObjectRef::LastMovedToHand,
            ),
            (
                " are returned to your hand",
                MatchObjectRef::LastMovedToHand,
            ),
            (" was put into your hand", MatchObjectRef::LastMovedToHand),
            (" is put into your hand", MatchObjectRef::LastMovedToHand),
            (" were put into your hand", MatchObjectRef::LastMovedToHand),
            (" are put into your hand", MatchObjectRef::LastMovedToHand),
            (
                " was put into your graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " is put into your graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " were put into your graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " are put into your graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " was put into a graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " is put into a graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " were put into a graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " are put into a graveyard",
                MatchObjectRef::LastMovedToGraveyard,
            ),
            (
                " was put onto the battlefield",
                MatchObjectRef::LastMovedToBattlefield,
            ),
            (
                " is put onto the battlefield",
                MatchObjectRef::LastMovedToBattlefield,
            ),
            (
                " were put onto the battlefield",
                MatchObjectRef::LastMovedToBattlefield,
            ),
            (
                " are put onto the battlefield",
                MatchObjectRef::LastMovedToBattlefield,
            ),
            (
                " entered the battlefield",
                MatchObjectRef::LastMovedToBattlefield,
            ),
            (
                " enters the battlefield",
                MatchObjectRef::LastMovedToBattlefield,
            ),
            (" was sacrificed", MatchObjectRef::LastMovedToGraveyard),
            (" is sacrificed", MatchObjectRef::LastMovedToGraveyard),
            (" were sacrificed", MatchObjectRef::LastMovedToGraveyard),
            (" are sacrificed", MatchObjectRef::LastMovedToGraveyard),
            (" was discarded", MatchObjectRef::LastMovedToGraveyard),
            (" is discarded", MatchObjectRef::LastMovedToGraveyard),
            (" were discarded", MatchObjectRef::LastMovedToGraveyard),
            (" are discarded", MatchObjectRef::LastMovedToGraveyard),
            (" was destroyed", MatchObjectRef::LastMovedToGraveyard),
            (" is destroyed", MatchObjectRef::LastMovedToGraveyard),
            (" were destroyed", MatchObjectRef::LastMovedToGraveyard),
            (" are destroyed", MatchObjectRef::LastMovedToGraveyard),
            (" was milled", MatchObjectRef::LastMovedToGraveyard),
            (" is milled", MatchObjectRef::LastMovedToGraveyard),
            (" were milled", MatchObjectRef::LastMovedToGraveyard),
            (" are milled", MatchObjectRef::LastMovedToGraveyard),
            (" was exiled", MatchObjectRef::LastMovedToExile),
            (" is exiled", MatchObjectRef::LastMovedToExile),
            (" were exiled", MatchObjectRef::LastMovedToExile),
            (" are exiled", MatchObjectRef::LastMovedToExile),
            (" entered", MatchObjectRef::LastMovedToBattlefield),
            (" enters", MatchObjectRef::LastMovedToBattlefield),
            (" died", MatchObjectRef::LastMovedToGraveyard),
            (" dies", MatchObjectRef::LastMovedToGraveyard),
        ];
        PASSIVE_VERBS
            .iter()
            .find_map(|&(suffix, mref)| core.strip_suffix(suffix).map(|s| (mref, s.trim())))?
    };

    // Fail closed if the immediately preceding instruction does not populate `last_moved_objects`.
    if !last_moved_subject_safe(builder, subject_ref) {
        return None;
    }

    let flt_raw = subj_phrase
        .strip_prefix("at least one ")
        .or_else(|| subj_phrase.strip_prefix("one or more "))
        .or_else(|| subj_phrase.strip_prefix("an "))
        .or_else(|| subj_phrase.strip_prefix("a "))?
        .trim();
    if flt_raw.is_empty() {
        return None;
    }

    let obj_filter = if matches!(flt_raw, "card" | "cards") {
        if subject_ref == MatchObjectRef::LastMovedToGraveyard {
            ObjectFilter::Clause(ObjectClause {
                token: Some(false),
                ..ObjectClause::any()
            })
        } else {
            ObjectFilter::any()
        }
    } else if matches!(flt_raw, "permanent" | "permanents") {
        ObjectFilter::permanent()
    } else {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let mut parsed =
            spellwake_oracle_compiler::filter::parse_object_filter(flt_raw, &compiler_vocab)
                .ok()?;
        if is_unrestricted_filter(&parsed)
            || parsed.zone.is_some()
            || !parsed.extra_zones.is_empty()
            || parsed.controller.is_some()
            || parsed.owner.is_some()
            || parsed.named.is_some()
            || !parsed.predicates.is_empty()
            || !parsed.flags.is_empty()
        {
            return None;
        }
        if parsed.any_card
            && parsed.token.is_none()
            && subject_ref == MatchObjectRef::LastMovedToGraveyard
        {
            parsed.token = Some(false);
        }
        match lower_object_filter(&parsed, vocabulary) {
            Ok(f) => f,
            Err(e) => return Some(Err(e)),
        }
    };

    Some(Ok(ResolutionCondition {
        left: ValueExpr::object_matches(subject_ref, obj_filter),
        comparison: Comparison::GreaterOrEqual,
        right: ValueExpr::literal(1),
    }))
}

pub(super) fn last_moved_subject_safe(builder: &BodyBuilder, subject_ref: MatchObjectRef) -> bool {
    match subject_ref {
        MatchObjectRef::LastMovedToGraveyard
            if matches!(
                builder.instructions.last(),
                Some(Instruction::DiscardOneAtRandom(_) | Instruction::SacrificeRandomUntilLand)
            ) =>
        {
            false
        }
        MatchObjectRef::LastMovedToHand | MatchObjectRef::LastMovedToBattlefield
            if matches!(
                builder.instructions.last(),
                Some(
                    Instruction::LookTopChoose { .. }
                        | Instruction::CreateTokens { .. }
                        | Instruction::CreateAttackingTokens { .. }
                        | Instruction::ReturnOwnGraveyardCreatureToBattlefield
                        | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                        | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                        | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
                        | Instruction::ReturnRandomGraveyardCards { .. }
                )
            ) =>
        {
            false
        }
        MatchObjectRef::LastMovedToExile
            if matches!(
                builder.instructions.last(),
                Some(
                    Instruction::ExileLibraryTop { .. }
                        | Instruction::ExileTopForPlay { .. }
                        | Instruction::ExileTopFaceDown
                        | Instruction::ExileTopForFreePlay
                )
            ) =>
        {
            false
        }
        _ => true,
    }
}

pub(super) fn body_turn_record_filter_supported(
    filter: ObjectFilter,
    allow_artifact_subtypes: bool,
) -> bool {
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

pub(super) fn parse_turn_record_filter_phrase(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
    allow_artifact_subtypes: bool,
) -> Option<ObjectFilter> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let parsed =
        spellwake_oracle_compiler::filter::parse_object_filter(trimmed, &compiler_vocab).ok()?;
    if parsed.zone.is_some()
        || !parsed.extra_zones.is_empty()
        || parsed.controller.is_some()
        || parsed.owner.is_some()
        || parsed.named.is_some()
        || !parsed.predicates.is_empty()
        || !parsed.flags.is_empty()
    {
        return None;
    }
    let obj = if is_unrestricted_filter(&parsed) {
        ObjectFilter::any()
    } else {
        lower_object_filter(&parsed, vocabulary).ok()?
    };
    body_turn_record_filter_supported(obj, allow_artifact_subtypes).then_some(obj)
}

pub(super) fn strip_turn_count_prefix(raw: &str) -> Option<(i64, &str)> {
    let t = raw.trim();
    if let Some(rest) = t
        .strip_prefix("at least one ")
        .or_else(|| t.strip_prefix("one or more "))
        .or_else(|| t.strip_prefix("an "))
        .or_else(|| t.strip_prefix("a "))
    {
        return Some((1, rest.trim()));
    }
    if let Some((cnt_part, rest)) = t.split_once(" or more ")
        && let Some(n) = parse_small_positive_count(cnt_part)
    {
        return Some((n, rest.trim()));
    }
    None
}

pub(super) fn lower_turn_history_player_action(
    player: PlayerRefAst,
    act_low: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Option<Result<ResolutionCondition, &'static str>> {
    // 1. "~ dealt damage to it / that creature / that permanent this turn"
    if matches!(
        act_low,
        "~ dealt damage to it this turn"
            | "~ dealt damage to that creature this turn"
            | "~ dealt damage to that permanent this turn"
            | "it was dealt damage by ~ this turn"
            | "that creature was dealt damage by ~ this turn"
            | "that permanent was dealt damage by ~ this turn"
    ) {
        let Some(mref) = resolve_match_object_ref(
            &SubjectRefAst::Anaphora("it".to_string()),
            builder,
            in_trigger,
            false,
        ) else {
            return Some(Err("oracle:unsupported-if-condition"));
        };
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                mref,
                ObjectFilter::any().with_flags(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }));
    }

    // 2. Opponent permanent left the battlefield this turn
    if matches!(
        act_low,
        "a permanent an opponent controlled left the battlefield this turn"
            | "a permanent left the battlefield under an opponent's control this turn"
            | "an opponent had a permanent leave the battlefield this turn"
    ) {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::PermanentLeft,
            )),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }));
    }

    // 3. Generic counters placed on creatures this turn (strictly reject specific counter kinds)
    if matches!(
        act_low,
        "a counter was put on a creature this turn"
            | "one or more counters were put on a creature this turn"
    ) {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CountersPlacedOnCreaturesThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }));
    }
    if let Some(rest) = act_low
        .strip_prefix("you put ")
        .or_else(|| act_low.strip_prefix("you've put "))
        .and_then(|s| {
            s.strip_suffix(" counters on a creature this turn")
                .or_else(|| s.strip_suffix(" counters on creatures this turn"))
        })
        && let Some(cnt_raw) = rest.strip_suffix(" or more")
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CountersPlacedOnCreaturesThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(n),
        }));
    }

    // 4. Filtered sacrifices -> SacrificedMatchingThisTurn
    let snake_sac_filter: Option<&str> = match act_low {
        "sacrificed_creature" => Some("creature"),
        "sacrificed_artifact" => Some("artifact"),
        "sacrificed_enchantment" => Some("enchantment"),
        "sacrificed_land" => Some("land"),
        "sacrificed_food" => Some("Food"),
        "sacrificed_treasure" => Some("Treasure"),
        "sacrificed_clue" => Some("Clue"),
        "sacrificed_blood" => Some("Blood"),
        _ => None,
    };
    if let Some(flt_text) = snake_sac_filter {
        let players = match player {
            PlayerRefAst::You => AffectedPlayers::One(ResolvingPlayer::Controller),
            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => AffectedPlayers::EachOpponent,
            PlayerRefAst::AnyPlayer => AffectedPlayers::EachPlayer,
            other => match lower_resolving_player_in_context(other, builder, in_trigger) {
                Ok(rp) => AffectedPlayers::One(rp),
                Err(e) => return Some(Err(e)),
            },
        };
        let obj = parse_turn_record_filter_phrase(flt_text, vocabulary, true)?;
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::SacrificedMatchingThisTurn {
                players,
                filter: Box::new(obj),
            }),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }));
    }
    if let Some(core) = act_low.strip_suffix(" this turn") {
        let sac_parts: Option<(AffectedPlayers, &str, bool)> = if let Some(rest) = core
            .strip_prefix("you sacrificed ")
            .or_else(|| core.strip_prefix("you've sacrificed "))
        {
            Some((
                AffectedPlayers::One(ResolvingPlayer::Controller),
                rest,
                false,
            ))
        } else if let Some(rest) = core.strip_prefix("an opponent sacrificed ") {
            Some((AffectedPlayers::EachOpponent, rest, true))
        } else if let Some(rest) = core.strip_prefix("a player sacrificed ") {
            Some((AffectedPlayers::EachPlayer, rest, true))
        } else if let Some(rest) = core
            .strip_prefix("that player sacrificed ")
            .or_else(|| core.strip_prefix("they sacrificed "))
        {
            let rp = match lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                builder,
                in_trigger,
            ) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some((AffectedPlayers::One(rp), rest, false))
        } else {
            None
        };
        if let Some((players, rest, is_multi_player)) = sac_parts
            && let Some((min_cnt, flt_raw)) = strip_turn_count_prefix(rest)
            && !(is_multi_player && min_cnt >= 2)
            && !matches!(flt_raw, "permanent" | "permanents")
            && let Some(obj) = parse_turn_record_filter_phrase(flt_raw, vocabulary, true)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SacrificedMatchingThisTurn {
                    players,
                    filter: Box::new(obj),
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(min_cnt),
            }));
        }

        // 5. Filtered graveyard arrivals / deaths -> GraveyardArrivalsMatchingThisTurn
        if !core.contains("from anywhere other than") {
            let gy_parts: Option<(&str, bool, Option<ResolvingPlayer>)> = if let Some(subj) =
                core.strip_suffix(" you controlled died")
            {
                Some((subj, true, Some(ResolvingPlayer::Controller)))
            } else if let Some(subj) = core.strip_suffix(" died") {
                Some((subj, true, None))
            } else if let Some(subj) = core
                .strip_suffix(" you controlled was put into a graveyard from the battlefield")
                .or_else(|| {
                    core.strip_suffix(
                        " you controlled were put into a graveyard from the battlefield",
                    )
                })
                .or_else(|| {
                    core.strip_suffix(
                        " you controlled was put into your graveyard from the battlefield",
                    )
                })
            {
                Some((subj, true, Some(ResolvingPlayer::Controller)))
            } else if let Some(subj) = core
                .strip_suffix(" was put into your graveyard from the battlefield")
                .or_else(|| core.strip_suffix(" were put into your graveyard from the battlefield"))
            {
                Some((subj, true, Some(ResolvingPlayer::Controller)))
            } else if let Some(subj) = core
                .strip_suffix(" was put into a graveyard from the battlefield")
                .or_else(|| core.strip_suffix(" were put into a graveyard from the battlefield"))
            {
                Some((subj, true, None))
            } else if let Some(subj) = core
                .strip_suffix(" was put into your graveyard from anywhere")
                .or_else(|| core.strip_suffix(" were put into your graveyard from anywhere"))
                .or_else(|| core.strip_suffix(" was put into your graveyard"))
                .or_else(|| core.strip_suffix(" were put into your graveyard"))
            {
                Some((subj, false, Some(ResolvingPlayer::Controller)))
            } else if let Some(subj) = core
                .strip_suffix(" was put into a graveyard from anywhere")
                .or_else(|| core.strip_suffix(" were put into a graveyard from anywhere"))
                .or_else(|| core.strip_suffix(" was put into a graveyard"))
                .or_else(|| core.strip_suffix(" were put into a graveyard"))
            {
                Some((subj, false, None))
            } else {
                None
            };
            if let Some((subj_raw, from_battlefield_only, controller)) = gy_parts
                && let Some((min_cnt, flt_raw)) = strip_turn_count_prefix(subj_raw)
                && !flt_raw.contains(" you ")
                && !flt_raw.contains(" opponent")
                && let Some(obj) = parse_turn_record_filter_phrase(flt_raw, vocabulary, true)
            {
                return Some(Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(obj),
                            from_battlefield_only,
                            controller,
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(min_cnt),
                }));
            }
        }

        // 6. Filtered spells cast -> SpellsCastMatchingThisTurn (allow_artifact_subtypes = false!)
        let cast_parts: Option<(AffectedPlayers, &str, bool)> = if let Some(rest) = core
            .strip_prefix("you cast ")
            .or_else(|| core.strip_prefix("you've cast "))
        {
            Some((
                AffectedPlayers::One(ResolvingPlayer::Controller),
                rest,
                false,
            ))
        } else if let Some(rest) = core
            .strip_prefix("an opponent cast ")
            .or_else(|| core.strip_prefix("an opponent has cast "))
        {
            Some((AffectedPlayers::EachOpponent, rest, true))
        } else if let Some(rest) = core
            .strip_prefix("a player cast ")
            .or_else(|| core.strip_prefix("a player has cast "))
        {
            Some((AffectedPlayers::EachPlayer, rest, true))
        } else {
            None
        };
        if let Some((players, rest, is_multi_player)) = cast_parts {
            let (spell_part, from_zone): (&str, Option<SourceZoneFilter>) =
                if let Some(p) = rest.strip_suffix(" from your graveyard") {
                    (p, Some(SourceZoneFilter::Graveyard))
                } else if let Some(p) = rest.strip_suffix(" from a graveyard") {
                    (p, Some(SourceZoneFilter::AnyGraveyard))
                } else if let Some(p) = rest.strip_suffix(" from exile") {
                    (p, Some(SourceZoneFilter::Exile))
                } else if let Some(p) = rest.strip_suffix(" from your hand") {
                    (p, Some(SourceZoneFilter::Hand))
                } else if rest.contains(" from ") {
                    return None;
                } else {
                    (rest, None)
                };
            if let Some((min_cnt, flt_with_spell)) = strip_turn_count_prefix(spell_part)
                && !(is_multi_player && min_cnt >= 2)
                && (flt_with_spell.ends_with(" spell") || flt_with_spell.ends_with(" spells"))
                && !flt_with_spell.starts_with("other ")
                && !flt_with_spell.starts_with("another ")
                && let Some(obj) =
                    parse_turn_record_filter_phrase(flt_with_spell, vocabulary, false)
            {
                return Some(Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                        players,
                        filter: Box::new(obj),
                        from_zone,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(min_cnt),
                }));
            }
        }

        // 7. Ordinal spell cast this turn ("it was the second [filter] spell you cast this turn")
        if let Some(after_ord) = core
            .strip_prefix("it was the ")
            .or_else(|| core.strip_prefix("it's the "))
            .or_else(|| core.strip_prefix("this is the "))
            .and_then(|s| {
                s.strip_suffix(" spell you cast")
                    .or_else(|| s.strip_suffix(" spell you've cast"))
            })
        {
            let (ord_word, flt_part) = match after_ord.split_once(' ') {
                Some((o, f)) => (o.trim(), Some(f.trim())),
                None => (after_ord.trim(), None),
            };
            let ord_n: Option<i64> = match ord_word {
                "first" | "1st" => Some(1),
                "second" | "2nd" => Some(2),
                "third" | "3rd" => Some(3),
                "fourth" | "4th" => Some(4),
                "fifth" | "5th" => Some(5),
                "sixth" | "6th" => Some(6),
                _ => None,
            };
            if let Some(n) = ord_n {
                match flt_part {
                    None => {
                        return Some(Ok(ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::ControllerSpellsCastThisTurn,
                            ),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(n),
                        }));
                    }
                    Some(flt_raw) => {
                        let spell_phrase = format!("{flt_raw} spell");
                        if let Some(obj) =
                            parse_turn_record_filter_phrase(&spell_phrase, vocabulary, false)
                        {
                            let count_cond = ResolutionCondition {
                                left: ValueExpr::PlayerStat(
                                    PlayerStatExpr::SpellsCastMatchingThisTurn {
                                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                        filter: Box::new(obj),
                                        from_zone: None,
                                    },
                                ),
                                comparison: Comparison::Equal,
                                right: ValueExpr::literal(n),
                            };
                            if in_trigger && obj != ObjectFilter::any() {
                                return Some(
                                    combine_resolution_conditions_and(vec![
                                        ResolutionCondition {
                                            left: ValueExpr::object_matches(
                                                MatchObjectRef::TriggerSubject,
                                                obj,
                                            ),
                                            comparison: Comparison::Equal,
                                            right: ValueExpr::literal(1),
                                        },
                                        count_cond,
                                    ])
                                    .ok_or("oracle:unsupported-if-condition"),
                                );
                            }
                            return Some(Ok(count_cond));
                        }
                    }
                }
            }
        }

        // 8. Opponent-controlled deaths ("a creature died under an opponent's control this turn")
        if let Some(subj_raw) = core
            .strip_suffix(" died under an opponent's control")
            .or_else(|| core.strip_suffix(" died under your opponents' control"))
            && let Some((1, flt_raw)) = strip_turn_count_prefix(subj_raw)
            && let Some(obj) = parse_turn_record_filter_phrase(flt_raw, vocabulary, true)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                    filter: Box::new(obj.opposing()),
                    from_battlefield_only: true,
                    controller: None,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }));
        }

        // 9. General turn-history life / draw / spell / permanent counts
        if matches!(
            core,
            "a player other than you lost life" | "another player lost life"
        ) {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }));
        }
        if core == "a player lost life" {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::MaxPlayerLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }));
        }
        if core == "your team gained life" {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }));
        }
        if core == "no permanents left the battlefield" {
            return Some(
                combine_resolution_conditions_and(vec![
                    ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    },
                    ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            spellwake_engine::program::OpponentTurnStat::PermanentLeft,
                        )),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition"),
            );
        }
        if let Some(cnt_raw) = core
            .strip_prefix("an opponent lost ")
            .and_then(|s| s.strip_suffix(" or more life"))
            && let Some(n) = parse_small_positive_count(cnt_raw)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(n),
            }));
        }
        if let Some(cnt_raw) = core
            .strip_prefix("you lost ")
            .or_else(|| core.strip_prefix("you've lost "))
            .and_then(|s| s.strip_suffix(" or more life"))
            && let Some(n) = parse_small_positive_count(cnt_raw)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(n),
            }));
        }
        if let Some(cnt_raw) = core
            .strip_prefix("you gained ")
            .or_else(|| core.strip_prefix("you've gained "))
            .and_then(|s| s.strip_suffix(" or more life"))
            && let Some(n) = parse_small_positive_count(cnt_raw)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(n),
            }));
        }
        if let Some(cnt_raw) = core
            .strip_prefix("you drew ")
            .or_else(|| core.strip_prefix("you've drawn "))
            .and_then(|s| s.strip_suffix(" or more cards"))
            && let Some(n) = parse_small_positive_count(cnt_raw)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(n),
            }));
        }
        if let Some(cnt_raw) = core
            .strip_prefix("you cast ")
            .or_else(|| core.strip_prefix("you've cast "))
            .and_then(|s| s.strip_suffix(" or more spells"))
            && let Some(n) = parse_small_positive_count(cnt_raw)
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(n),
            }));
        }
        if let Some(type_raw) = core.strip_prefix("~ or another ").and_then(|s| {
            s.strip_suffix(" entered the battlefield under your control")
                .or_else(|| s.strip_suffix(" entered under your control"))
        }) {
            let stat = match type_raw.trim() {
                "creature" => Some(PlayerStatExpr::CreatureEnteredUnderControlThisTurn),
                "artifact" => Some(PlayerStatExpr::EnteredTypeThisTurn(CardType::Artifact)),
                "enchantment" => Some(PlayerStatExpr::EnteredTypeThisTurn(CardType::Enchantment)),
                "land" => Some(PlayerStatExpr::EnteredTypeThisTurn(CardType::Land)),
                "planeswalker" => Some(PlayerStatExpr::EnteredTypeThisTurn(CardType::Planeswalker)),
                "permanent" => Some(PlayerStatExpr::EnteredPermanentThisTurn),
                _ => None,
            };
            if let Some(st) = stat {
                return Some(Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(st),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }));
            }
        }
    }

    // 10. Turn ownership, coin flips, action outcomes, chosen name, and board state checks
    match act_low {
        "it's your turn" | "it is your turn" => {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }));
        }
        "it's not your turn"
        | "it isn't your turn"
        | "it is not your turn"
        | "it's an opponent's turn"
        | "it is an opponent's turn" => {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }));
        }
        "it's not their turn"
        | "it isn't their turn"
        | "it's not that player's turn"
        | "it isn't that player's turn" => {
            let rp = match lower_resolving_player_in_context(
                PlayerRefAst::ThatPlayer,
                builder,
                in_trigger,
            ) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(rp)),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }));
        }
        "no counters were removed this way"
        | "no life is lost this way"
        | "no life was lost this way"
            if !builder.instructions.is_empty() =>
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }));
        }
        "you cast a spell this way" | "~ is destroyed this way"
            if !builder.instructions.is_empty() =>
        {
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }));
        }
        "you control that creature" | "you control it" | "you control that permanent" => {
            let subj = if act_low.contains("creature") {
                "that creature"
            } else {
                "it"
            };
            let Some(mref) = resolve_match_object_ref(
                &SubjectRefAst::Anaphora(subj.to_string()),
                builder,
                in_trigger,
                false,
            ) else {
                return Some(Err("oracle:unsupported-if-condition"));
            };
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::any().controlled()),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }));
        }
        "you don't control that creature"
        | "you don't control it"
        | "you don't control that permanent" => {
            let subj = if act_low.contains("creature") {
                "that creature"
            } else {
                "it"
            };
            let Some(mref) = resolve_match_object_ref(
                &SubjectRefAst::Anaphora(subj.to_string()),
                builder,
                in_trigger,
                false,
            ) else {
                return Some(Err("oracle:unsupported-if-condition"));
            };
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::any().controlled()),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }));
        }
        "that card has the chosen name"
        | "it has the chosen name"
        | "the revealed card has the chosen name" => {
            let subj = if act_low.contains("revealed") {
                "the revealed card"
            } else {
                "it"
            };
            let Some(mref) = resolve_match_object_ref(
                &SubjectRefAst::Anaphora(subj.to_string()),
                builder,
                in_trigger,
                false,
            ) else {
                return Some(Err("oracle:unsupported-if-condition"));
            };
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::any().with_flags(ObjectFilterFlags::MATCHES_CHOSEN_NAME),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }));
        }
        "you control a land of each basic land type and a creature of each color" => {
            return Some(
                combine_resolution_conditions_and(vec![
                    ResolutionCondition {
                        left: ValueExpr::DomainCount,
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(5),
                    },
                    ResolutionCondition {
                        left: ValueExpr::DistinctColors {
                            filter: Box::new(ObjectFilter::creature().controlled()),
                        },
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(5),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition"),
            );
        }
        "there are twenty or more counters on it or you have twenty or more cards in hand"
        | "there are 20 or more counters on it or you have 20 or more cards in hand" => {
            return Some(
                combine_resolution_conditions_or(vec![
                    ResolutionCondition {
                        left: ValueExpr::counters(
                            spellwake_engine::program::CounterSubjectRef::AbilitySource,
                            None,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(20),
                    },
                    ResolutionCondition {
                        left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(20),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition"),
            );
        }
        "it has the greatest power or is tied for greatest power among creatures on the battlefield" =>
        {
            let Some(vref) = resolve_value_object_ref_for_stat(
                &SubjectRefAst::Anaphora("it".to_string()),
                builder,
                in_trigger,
                false,
            ) else {
                return Some(Err("oracle:unsupported-if-condition"));
            };
            return Some(Ok(ResolutionCondition {
                left: ValueExpr::object_stat(vref, ObjectStatKind::Power),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::aggregate(
                    ObjectFilter::creature(),
                    ObjectStatKind::Power,
                    StatAggregate::Max,
                ),
            }));
        }
        "three or more cards left your graveyard this turn or if you've sacrificed a food this turn"
        | "three or more cards left your graveyard this turn or you've sacrificed a food this turn" =>
        {
            let Some(food_obj) = parse_turn_record_filter_phrase("Food", vocabulary, true) else {
                return Some(Err("oracle:unsupported-if-condition"));
            };
            return Some(
                combine_resolution_conditions_or(vec![
                    ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(3),
                    },
                    ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::SacrificedMatchingThisTurn {
                            players: AffectedPlayers::One(ResolvingPlayer::Controller),
                            filter: Box::new(food_obj),
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition"),
            );
        }
        "the number of permanents is even" => {
            let total = ValueExpr::count_battlefield(ObjectFilter::permanent());
            let rem = ValueExpr::Sub(
                Box::new(total.clone()),
                Box::new(ValueExpr::Scale {
                    expr: Box::new(total.half_down()),
                    factor: 2.into(),
                }),
            );
            return Some(Ok(ResolutionCondition {
                left: rem,
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }));
        }
        _ => {}
    }

    if let Some(cnt_raw) = act_low
        .strip_prefix("you won ")
        .or_else(|| act_low.strip_prefix("you win "))
        .and_then(|s| {
            s.strip_suffix(" or more flips this way")
                .or_else(|| s.strip_suffix(" flips this way"))
        })
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CoinFlipsWon),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(n),
        }));
    }
    if !builder.instructions.is_empty()
        && let Some(cnt_raw) = act_low
            .strip_prefix("you exiled ")
            .and_then(|s| s.strip_suffix(" or more cards this way"))
            .or_else(|| {
                act_low
                    .strip_suffix(" or more cards were exiled this way")
                    .or_else(|| act_low.strip_suffix(" or more cards are exiled this way"))
            })
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::LastActionCount),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(n),
        }));
    }
    if let Some(cnt_raw) = act_low.strip_suffix(" or more creatures are attacking")
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::count_battlefield(ObjectFilter::creature().attacking()),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(n),
        }));
    }
    if let Some(cnt_raw) = act_low.strip_prefix("exactly ").and_then(|s| {
        s.strip_suffix(" creature is attacking")
            .or_else(|| s.strip_suffix(" creatures are attacking"))
    }) && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::count_battlefield(ObjectFilter::creature().attacking()),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(n),
        }));
    }
    if let Some(rest) = act_low
        .strip_prefix("they have exactly ")
        .or_else(|| act_low.strip_prefix("that player has exactly "))
        .and_then(|s| s.strip_suffix(" cards in hand"))
        && let Some((a_raw, b_raw)) = rest.split_once(" or exactly ")
        && let (Some(a), Some(b)) = (
            parse_small_positive_count(a_raw),
            parse_small_positive_count(b_raw),
        )
    {
        let rp = match lower_resolving_player_in_context(
            PlayerRefAst::ThatPlayer,
            builder,
            in_trigger,
        ) {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        return Some(
            combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::count_hand(rp),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(a),
                },
                ResolutionCondition {
                    left: ValueExpr::count_hand(rp),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(b),
                },
            ])
            .ok_or("oracle:unsupported-if-condition"),
        );
    }
    if let Some((p_ref, cnt_raw)) = act_low
        .strip_prefix("target player has fewer than ")
        .and_then(|s| s.strip_suffix(" poison counters"))
        .map(|c| (PlayerRefAst::TargetPlayer, c))
        .or_else(|| {
            act_low
                .strip_prefix("that player has fewer than ")
                .and_then(|s| s.strip_suffix(" poison counters"))
                .map(|c| (PlayerRefAst::ThatPlayer, c))
        })
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        let rp = match lower_resolving_player_in_context(p_ref, builder, in_trigger) {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::counters(
                spellwake_engine::program::CounterSubjectRef::PlayerCounter(
                    rp,
                    CounterKind::Poison,
                ),
                Some(CounterKind::Poison),
            ),
            comparison: Comparison::Less,
            right: ValueExpr::literal(n),
        }));
    }
    if let Some(cnt_raw) = act_low
        .strip_suffix(" or more equipment are attached to ~")
        .or_else(|| {
            (!is_object_body_target(builder.target, false) && !in_trigger)
                .then(|| act_low.strip_suffix(" or more equipment are attached to it"))
                .flatten()
        })
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::AttachedCount(
                spellwake_engine::program::AttachedCountKind::EquipmentOnSource,
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(n),
        }));
    }
    if let Some(rest) = act_low
        .strip_prefix("there is exactly ")
        .or_else(|| act_low.strip_prefix("there are exactly "))
        .and_then(|s| s.strip_suffix(" on ~"))
        && let Some((cnt_raw, kind_tail)) = rest.split_once(' ')
        && let Some(n) = parse_small_positive_count(cnt_raw)
        && let Some(kind_raw) = kind_tail
            .strip_suffix(" counters")
            .or_else(|| kind_tail.strip_suffix(" counter"))
        && let Some(ck) = parse_counter_kind(kind_raw)
    {
        return Some(Ok(ResolutionCondition {
            left: ValueExpr::counters(
                spellwake_engine::program::CounterSubjectRef::AbilitySource,
                Some(ck),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(n),
        }));
    }

    None
}

pub(super) fn lower_marker_or_action_condition(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    _bound_x: Option<&ValueExprAst>,
) -> Result<ResolutionCondition, &'static str> {
    match cond {
        ConditionAst::Marker(GameMarker::CitysBlessing) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerHasCityBlessing),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Bargained) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::Bargained),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::DescendedThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DescendedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CommittedCrimeThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NoCreaturesOnBattlefield) => Ok(ResolutionCondition {
            left: ValueExpr::count_battlefield(ObjectFilter::creature()),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::CastFromGraveyard) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromGraveyard),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastFromExile) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromExile),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastFromOutsideHand) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromOutsideHand),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::GiftPromised) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::GiftPromised),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::EvidenceCollected) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::EvidenceCollected),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AdditionalCostPaid { .. }) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::AdditionalCostPaid),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ManaSpentLessThanManaValue) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::ManaSpentToCast),
            comparison: Comparison::Less,
            right: ValueExpr::ObjectStat {
                subject: ValueObjectRef::AbilitySource,
                stat: ObjectStatKind::ManaValue,
            },
        }),
        ConditionAst::Marker(GameMarker::CastDuringMainPhase) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastDuringOwnMain),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Initiative(PlayerRefAst::You)) => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::HasInitiative),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CompletedDungeon) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CompletedDungeons),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        // Guardrail: ONLY lower ControlsGreatestStat for single players.
        // AnyPlayer / EachPlayer / AnyOpponent / EachOpponent are lossy in Stage 1 and must remain fail-closed.
        ConditionAst::Marker(GameMarker::ControlsGreatestStat {
            player: PlayerRefAst::You,
            filter,
            stat,
        }) => {
            let is_plain_creature = *stat == StatKind::Power
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter.forbidden_types.is_empty()
                && filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.subtypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.controller.is_none()
                && filter.owner.is_none()
                && filter.zone.is_none()
                && filter.extra_zones.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.counter_kind.is_none()
                && filter.named.is_none()
                && filter.flags.is_empty()
                && filter.predicates.is_empty();
            if is_plain_creature {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestCreature),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestStat {
                    player: ResolvingPlayer::Controller,
                    filter: Box::new(obj),
                    stat: st,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::ControlsGreatestStat {
            player,
            filter,
            stat,
        }) if !matches!(
            player,
            PlayerRefAst::AnyPlayer
                | PlayerRefAst::EachPlayer
                | PlayerRefAst::EachOtherPlayer
                | PlayerRefAst::AnyOpponent
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachOtherOpponent
        ) =>
        {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestStat {
                    player: rp,
                    filter: Box::new(obj),
                    stat: st,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            action,
        } => {
            use spellwake_engine::program::OpponentTurnStat;
            let act_low = action.trim().trim_end_matches('.').to_ascii_lowercase();
            let stat = match act_low.as_str() {
                "lost_life" | "lose_life" | "lost life" | "an opponent lost life this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "discard"
                | "discarded"
                | "discarded_card"
                | "discarded a card"
                | "an opponent discarded a card this turn" => OpponentTurnStat::Discarded,
                "sacrifice"
                | "sacrificed"
                | "sacrificed_permanent"
                | "sacrificed a permanent"
                | "an opponent sacrificed a permanent this turn" => OpponentTurnStat::Sacrificed,
                "cast_spell" | "cast a spell" | "an opponent cast a spell this turn" => {
                    OpponentTurnStat::CastSpells
                }
                "drew_card" | "drew a card" | "an opponent drew a card this turn" => {
                    OpponentTurnStat::DrewCards
                }
                "attacked" | "attack" | "an opponent attacked this turn" => {
                    OpponentTurnStat::Attacked
                }
                "gained_life"
                | "gain_life"
                | "gained life"
                | "an opponent gained life this turn" => OpponentTurnStat::GainedLife,
                "played_land" | "played a land" | "an opponent played a land this turn" => {
                    OpponentTurnStat::PlayedLand
                }
                "creature_entered" | "a creature entered under an opponent's control this turn" => {
                    OpponentTurnStat::CreatureEntered
                }
                "permanent_left_battlefield" | "revolt" => OpponentTurnStat::PermanentLeft,
                other => {
                    if let Some(res) = lower_turn_history_player_action(
                        PlayerRefAst::AnyOpponent,
                        other,
                        builder,
                        vocabulary,
                        in_trigger,
                    ) {
                        return res;
                    }
                    if let Some(res) =
                        lower_comparative_controls_action(other, builder, vocabulary, in_trigger)
                    {
                        return res;
                    }
                    return Err("oracle:unsupported-if-condition");
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(stat)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => {
            let act_low = action.trim().trim_end_matches('.').to_ascii_lowercase();
            let (stat, min): (PlayerStatExpr, i64) = match act_low.as_str() {
                "lost_life" | "lose_life" | "you lost life this turn" => {
                    (PlayerStatExpr::ControllerLifeLostThisTurn, 1)
                }
                "this is the first or second time this ability has resolved this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
                        comparison: Comparison::LessOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "no permanents left the battlefield this turn"
                | "no permanent left the battlefield this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
                            )),
                            Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                                spellwake_engine::program::OpponentTurnStat::PermanentLeft,
                            ))),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "a creature died under an opponent's control this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                                filter: Box::new(ObjectFilter::creature().opposing()),
                                from_battlefield_only: true,
                                controller: None,
                            },
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "an enchantment was put into your graveyard from the battlefield this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                                filter: Box::new(ObjectFilter::Clause(ObjectClause {
                                    any_types: CardTypeMask::ENCHANTMENT,
                                    owner: ControllerFilter::OwnerYou,
                                    ..ObjectClause::any()
                                })),
                                from_battlefield_only: true,
                                controller: None,
                            },
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you didn't lose life this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "gained_life" | "gain_life" | "you gained life this turn" => {
                    (PlayerStatExpr::ControllerLifeGainedThisTurn, 1)
                }
                "attacked" | "attack" | "you attacked this turn" => {
                    (PlayerStatExpr::ControllerAttackedThisTurn, 1)
                }
                "discard" | "discarded" | "discarded_card" | "you discarded a card this turn" => {
                    (PlayerStatExpr::ControllerDiscardedThisTurn, 1)
                }
                "sacrifice"
                | "sacrificed"
                | "sacrificed_permanent"
                | "you sacrificed a permanent this turn" => {
                    (PlayerStatExpr::ControllerSacrificedThisTurn, 1)
                }
                "revolt"
                | "permanent_left_battlefield"
                | "a permanent you controlled left the battlefield this turn" => {
                    (PlayerStatExpr::PermanentLeftBattlefieldThisTurn, 1)
                }
                "descended" | "you descended this turn" => (PlayerStatExpr::DescendedThisTurn, 1),
                "crime" | "committed_crime" | "you committed a crime this turn" => {
                    (PlayerStatExpr::CommittedCrimeThisTurn, 1)
                }
                "cast_two_spells" | "you've cast two or more spells this turn" => {
                    (PlayerStatExpr::ControllerSpellsCastThisTurn, 2)
                }
                "drew_card" | "you drew a card this turn" => {
                    (PlayerStatExpr::CardsDrawnThisTurn, 1)
                }
                "drew_two_cards"
                | "drew_2plus"
                | "you've drawn two or more cards this turn"
                | "you've drawn more than one card this turn" => {
                    (PlayerStatExpr::CardsDrawnThisTurn, 2)
                }
                "you cast a spell this turn" | "you've cast a spell this turn" => {
                    (PlayerStatExpr::ControllerSpellsCastThisTurn, 1)
                }
                "an opponent lost life this turn" => (PlayerStatExpr::OpponentsLifeLostThisTurn, 1),
                "an opponent is poisoned" => (PlayerStatExpr::AnyOpponentPoisoned, 1),
                "an opponent has been dealt noncombat damage this turn"
                | "an opponent was dealt noncombat damage this turn" => (
                    PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: true,
                    },
                    1,
                ),
                "an opponent has been dealt damage this turn"
                | "an opponent was dealt damage this turn" => (
                    PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: false,
                    },
                    1,
                ),
                "a creature entered the battlefield under your control this turn"
                | "a creature entered under your control this turn" => {
                    (PlayerStatExpr::CreatureEnteredUnderControlThisTurn, 1)
                }
                "a creature died this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "no creatures died this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you played a land this turn"
                | "you've played a land this turn"
                | "played_land" => (PlayerStatExpr::PlayedLandThisTurn, 1),
                "you didn't play a land this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you have a card in hand" | "you have at least one card in hand" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "your library has no cards in it" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                            ResolvingPlayer::Controller,
                        )),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "there are five colors among permanents you control" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::DistinctColors {
                            filter: Box::new(ObjectFilter::permanent().controlled()),
                        },
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(5),
                    });
                }
                "~ attacked this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you cast a noncreature spell this turn"
                | "you've cast a noncreature spell this turn" => {
                    (PlayerStatExpr::CastNoncreatureSpellsThisTurn, 1)
                }
                "you cast a creature spell this turn"
                | "you've cast a creature spell this turn" => {
                    (PlayerStatExpr::CastSpellTypeThisTurn(CardType::Creature), 1)
                }
                "you cast an instant or sorcery spell this turn"
                | "you've cast an instant or sorcery spell this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                            )),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "a card left your graveyard this turn"
                | "one or more cards left your graveyard this turn" => {
                    (PlayerStatExpr::CardsLeftGraveyardThisTurn, 1)
                }
                "you created a token this turn" | "you've created a token this turn" => {
                    (PlayerStatExpr::CreatedTokensThisTurn, 1)
                }
                "you gained or lost life this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::ControllerLifeGainedThisTurn,
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::ControllerLifeLostThisTurn,
                            )),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you didn't attack this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you didn't cast a spell this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you sacrificed one or more permanents this turn" => {
                    (PlayerStatExpr::ControllerSacrificedThisTurn, 1)
                }
                "you're the defending player" => (PlayerStatExpr::ControllerIsDefending, 1),
                "~ attacked or blocked this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "a library has twenty or fewer cards in it"
                | "a library has 20 or fewer cards in it" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::MinLibrarySize),
                        comparison: Comparison::LessOrEqual,
                        right: ValueExpr::literal(20),
                    });
                }
                "you've cast both a creature spell and a noncreature spell this turn" => {
                    let c1 = ValueExpr::Conditional {
                        left: Box::new(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastSpellTypeThisTurn(CardType::Creature),
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(ValueExpr::literal(1)),
                        then_value: Box::new(ValueExpr::literal(1)),
                        else_value: Box::new(ValueExpr::literal(0)),
                    };
                    let c2 = ValueExpr::Conditional {
                        left: Box::new(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastNoncreatureSpellsThisTurn,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(ValueExpr::literal(1)),
                        then_value: Box::new(ValueExpr::literal(1)),
                        else_value: Box::new(ValueExpr::literal(0)),
                    };
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(Box::new(c1), Box::new(c2)),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(2),
                    });
                }
                "an opponent discarded a card this turn" => (
                    PlayerStatExpr::OpponentStatThisTurn(
                        spellwake_engine::program::OpponentTurnStat::Discarded,
                    ),
                    1,
                ),
                "it's your main phase" | "your main phase" => (PlayerStatExpr::IsOwnMainPhase, 1),
                "you've activated a loyalty ability this turn"
                | "you activated a loyalty ability this turn" => {
                    (PlayerStatExpr::LoyaltyActivatedThisTurn, 1)
                }
                "you've completed a dungeon" | "completed a dungeon" => {
                    (PlayerStatExpr::CompletedDungeons, 1)
                }
                "you have the initiative" | "have the initiative" => {
                    (PlayerStatExpr::HasInitiative, 1)
                }
                "the ring has tempted you" => (PlayerStatExpr::RingLevel, 1),
                "you won the flip" | "won the flip" | "you win the flip" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you lose the flip" | "you lost the flip" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you won the clash"
                | "won the clash"
                | "you win"
                | "you do"
                | "you did"
                | "they do"
                | "that player does"
                | "the player does"
                | "a player does"
                | "the spell is countered this way"
                | "that spell is countered this way"
                | "a spell is countered this way"
                | "that card is returned to its owner's hand this way"
                | "a card is put into your hand this way"
                | "a creature is destroyed this way"
                | "that permanent is destroyed this way" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you don't"
                | "they don't"
                | "that player doesn't"
                | "the player doesn't"
                | "no player does"
                | "you can't"
                | "they can't"
                | "that player can't"
                | "no cards were discarded this way"
                | "no card was discarded this way"
                | "no creatures were sacrificed this way"
                | "no permanent was sacrificed this way"
                | "you don't cast a spell this way"
                | "you don't draw a card this way"
                | "you don't copy a spell this way" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "a card is exiled with ~"
                | "a card is exiled with it"
                | "there is a card exiled with ~"
                | "one or more cards are exiled with ~" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Count {
                            zone: ValueZone::Exile(None),
                            filter: Box::new(
                                ObjectFilter::any().with_flags(ObjectFilterFlags::EXILED_WITH_SELF),
                            ),
                            controller_scope: None,
                        },
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you put a counter on a creature this turn"
                | "you've put a counter on a creature this turn"
                | "a counter was put on a creature you control this turn"
                | "you put one or more counters on a creature this turn" => {
                    (PlayerStatExpr::CountersPlacedOnCreaturesThisTurn, 1)
                }
                "~ is your ring-bearer" | "it is your ring-bearer" | "it's your ring-bearer" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::IsRingBearer),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "~ is in your graveyard or on the battlefield"
                | "~ is on the battlefield or in your graveyard" => {
                    use spellwake_engine::program::SourceZoneKind;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Graveyard,
                            ))),
                            Box::new(ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Battlefield,
                            ))),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "~ was dealt damage this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::DamageDealtThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "~ is still on the battlefield" | "it's still on the battlefield" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::InZone(
                            spellwake_engine::program::SourceZoneKind::Battlefield,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "it was cast from your graveyard" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::CastFromGraveyard),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you cast it from anywhere other than your hand"
                | "~ was cast from anywhere other than your hand" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::CastFromOutsideHand),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "~ was cast using web-slinging" | "that creature was cast for its warp cost" => {
                    let left = if in_trigger {
                        ValueExpr::SourceState(SourceStateExpr::PaidAlternative)
                    } else {
                        ValueExpr::event(EventValueKind::PaidAlternative)
                    };
                    return Ok(ResolutionCondition {
                        left,
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you had another land enter the battlefield under your control this turn"
                | "you had another land enter under your control this turn" => (
                    PlayerStatExpr::LandsEnteredThisTurn(ResolvingPlayer::Controller),
                    2,
                ),
                "a land entered the battlefield under your control this turn"
                | "a land entered under your control this turn" => (
                    PlayerStatExpr::LandsEnteredThisTurn(ResolvingPlayer::Controller),
                    1,
                ),
                "that player had another land enter the battlefield under their control this turn"
                | "that player had another land enter under their control this turn"
                | "they had another land enter the battlefield under their control this turn"
                | "they had another land enter under their control this turn" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    (PlayerStatExpr::LandsEnteredThisTurn(rp), 2)
                }
                "a land entered the battlefield under that player's control this turn"
                | "a land entered the battlefield under their control this turn"
                | "a land entered under that player's control this turn"
                | "a land entered under their control this turn" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    (PlayerStatExpr::LandsEnteredThisTurn(rp), 1)
                }
                "you've rolled a die this turn"
                | "you rolled a die this turn"
                | "you've rolled one or more dice this turn"
                | "you rolled one or more dice this turn" => {
                    (PlayerStatExpr::DiceRolledThisTurn, 1)
                }
                "you have less life than an opponent" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                        comparison: Comparison::Less,
                        right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                    });
                }
                "you have less life than each opponent" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                        comparison: Comparison::Less,
                        right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                    });
                }
                "you have more life than an opponent" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                        comparison: Comparison::Greater,
                        right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                    });
                }
                "you have more life than each opponent" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                        comparison: Comparison::Greater,
                        right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                    });
                }
                "ten or more lands are on the battlefield" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_battlefield(ObjectFilter::land()),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(10),
                    });
                }
                "an enchantment is on the battlefield" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_battlefield(ObjectFilter::enchantment()),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "no cards are in that graveyard" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerGraveyardSize(rp)),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "that creature has greater power or toughness than ~"
                | "it has greater power or toughness than ~" => {
                    let s_ref = if builder.target.guarantees_creature() {
                        ValueObjectRef::TargetObject
                    } else if in_trigger {
                        ValueObjectRef::TriggerSubject
                    } else {
                        return Err("oracle:unsupported-if-condition");
                    };
                    return combine_resolution_conditions_or(vec![
                        ResolutionCondition {
                            left: ValueExpr::object_stat(s_ref, ObjectStatKind::Power),
                            comparison: Comparison::Greater,
                            right: ValueExpr::object_stat(
                                ValueObjectRef::AbilitySource,
                                ObjectStatKind::Power,
                            ),
                        },
                        ResolutionCondition {
                            left: ValueExpr::object_stat(s_ref, ObjectStatKind::Toughness),
                            comparison: Comparison::Greater,
                            right: ValueExpr::object_stat(
                                ValueObjectRef::AbilitySource,
                                ObjectStatKind::Toughness,
                            ),
                        },
                    ])
                    .ok_or("oracle:unsupported-if-condition");
                }
                "its mana value is odd" | "its mana value is even" => {
                    let mv = if act_low.ends_with("odd") {
                        ManaValueFilter::Odd
                    } else {
                        ManaValueFilter::Even
                    };
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("it".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::Clause(ObjectClause {
                                mana_value: mv,
                                ..ObjectClause::any()
                            }),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    });
                }
                "that land was nonbasic" => {
                    if !is_object_body_target(builder.target, false) {
                        return Err("oracle:unsupported-if-condition");
                    }
                    let nonbasic_land = ObjectFilter::Clause(ObjectClause {
                        forbidden_supertypes: SupertypeMask::BASIC,
                        ..ObjectClause::land()
                    });
                    let target_cond = ResolutionCondition {
                        left: ValueExpr::object_matches(
                            MatchObjectRef::TargetObject,
                            nonbasic_land,
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    };
                    let target_just_moved = matches!(
                        builder.instructions.last(),
                        Some(
                            Instruction::DestroyTarget
                                | Instruction::DestroyTargetWithoutRegeneration
                                | Instruction::ExileTarget
                                | Instruction::ReturnToOwnersHand
                                | Instruction::MoveSubject { .. }
                        )
                    );
                    if target_just_moved {
                        let moved_cond = ResolutionCondition {
                            left: ValueExpr::object_matches(
                                MatchObjectRef::LastAffected,
                                nonbasic_land,
                            ),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        };
                        return combine_resolution_conditions_or(vec![target_cond, moved_cond])
                            .ok_or("oracle:unsupported-if-condition");
                    }
                    return Ok(target_cond);
                }
                "at least one creature is blocking ~" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_battlefield(
                            ObjectFilter::creature().combat(CombatFilter::BlockingSource),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you have exactly zero or seven cards in hand" => {
                    return combine_resolution_conditions_or(vec![
                        ResolutionCondition {
                            left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(0),
                        },
                        ResolutionCondition {
                            left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(7),
                        },
                    ])
                    .ok_or("oracle:unsupported-if-condition");
                }
                "you've cast a spell from a graveyard or activated an ability of a card in a graveyard this turn"
                | "you cast a spell from a graveyard or activated an ability of a card in a graveyard this turn" =>
                {
                    return combine_resolution_conditions_or(vec![
                        ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::SpellsCastMatchingThisTurn {
                                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                    filter: Box::new(ObjectFilter::any()),
                                    from_zone: Some(SourceZoneFilter::AnyGraveyard),
                                },
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(
                                    ResolvingPlayer::Controller,
                                ),
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                    ])
                    .ok_or("oracle:unsupported-if-condition");
                }
                "you activated an ability of a card in your graveyard this turn"
                | "you activated an ability of a card in a graveyard this turn"
                | "you've activated an ability of a card in your graveyard this turn"
                | "you've activated an ability of a card in a graveyard this turn" => (
                    PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(ResolvingPlayer::Controller),
                    1,
                ),
                "one or more cards were put into exile this turn"
                | "a card was put into exile this turn" => (PlayerStatExpr::CardsExiledThisTurn, 1),
                "you haven't activated an exhaust ability this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ExhaustActivatedThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "~ dealt damage to an opponent this turn" => (
                    PlayerStatExpr::DamageDealtThisTurn {
                        subject: ValueObjectRef::AbilitySource,
                        to_opponent_only: true,
                        combat_only: false,
                    },
                    1,
                ),
                "~ dealt combat damage to an opponent this turn" => (
                    PlayerStatExpr::DamageDealtThisTurn {
                        subject: ValueObjectRef::AbilitySource,
                        to_opponent_only: true,
                        combat_only: true,
                    },
                    1,
                ),
                "~ dealt combat damage to a player this turn" => (
                    PlayerStatExpr::DamageDealtThisTurn {
                        subject: ValueObjectRef::AbilitySource,
                        to_opponent_only: false,
                        combat_only: true,
                    },
                    1,
                ),
                "a land you controlled was put into a graveyard from the battlefield this turn" => {
                    (
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(ObjectFilter::land()),
                            from_battlefield_only: true,
                            controller: Some(ResolvingPlayer::Controller),
                        },
                        1,
                    )
                }
                "it didn't attack this turn" | "that creature didn't attack this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "that creature attacked this turn" | "it attacked this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    });
                }
                "it was dealt damage this turn" | "that creature was dealt damage this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any()
                                .with_flags(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    });
                }
                "that creature was dealt excess damage this turn"
                | "it was dealt excess damage this turn" => {
                    let vref = resolve_value_object_ref_for_stat(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    (
                        PlayerStatExpr::DamageReceivedThisTurn {
                            subject: vref,
                            excess_only: true,
                        },
                        1,
                    )
                }
                "it dealt combat damage to a player this turn"
                | "that creature dealt combat damage to a player this turn" => {
                    let vref = resolve_value_object_ref_for_stat(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    (
                        PlayerStatExpr::DamageDealtThisTurn {
                            subject: vref,
                            to_opponent_only: false,
                            combat_only: true,
                        },
                        1,
                    )
                }
                "it shares a card type with a card exiled with ~"
                | "it shares a card type with the exiled card" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("it".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::SharesCardTypeWithLinkedExile(mref)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "that artifact is put into a graveyard this way"
                | "that creature is put into a graveyard this way"
                | "a creature is put into a graveyard this way" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                other => {
                    if let Some(cnt_raw) = other
                        .strip_prefix("you've rolled ")
                        .or_else(|| other.strip_prefix("you rolled "))
                        .and_then(|s| s.strip_suffix(" or more dice this turn"))
                        && let Some(n) = parse_small_positive_count(cnt_raw)
                    {
                        (PlayerStatExpr::DiceRolledThisTurn, n)
                    } else if let Some(cnt_raw) = other
                        .strip_prefix(
                            "the total mana value of other spells you've cast this turn is ",
                        )
                        .or_else(|| {
                            other.strip_prefix(
                                "the total mana value of other spells you cast this turn is ",
                            )
                        })
                        .and_then(|s| s.strip_suffix(" or greater"))
                        && let Some(n) = parse_small_positive_count(cnt_raw)
                    {
                        (PlayerStatExpr::OtherSpellsCastManaValueThisTurn, n)
                    } else if other.starts_with("you gain control of ")
                        && other.ends_with(" this way")
                    {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    } else if let Some(rc) =
                        lower_mana_spent_on_triggering_spell_action(other, in_trigger)
                    {
                        return Ok(rc);
                    } else if let Some(res) = lower_graveyard_count_action(other, vocabulary) {
                        return res;
                    } else if let Some(res) =
                        lower_zone_action_this_way_action(other, builder, vocabulary)
                    {
                        return res;
                    } else if let Some(res) = lower_turn_history_player_action(
                        PlayerRefAst::You,
                        other,
                        builder,
                        vocabulary,
                        in_trigger,
                    ) {
                        return res;
                    } else if let Some(res) =
                        lower_comparative_controls_action(other, builder, vocabulary, in_trigger)
                    {
                        return res;
                    } else {
                        return Err("oracle:unsupported-if-condition");
                    }
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(stat),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(min),
            })
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::AnyPlayer,
            action,
        } if matches!(
            action
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str(),
            "discard" | "discarded" | "discarded_card"
        ) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::Add(
                    Box::new(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerDiscardedThisTurn,
                    )),
                    Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        spellwake_engine::program::OpponentTurnStat::Discarded,
                    ))),
                ),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::PlayerActionThisTurn { player, action } => {
            let act_low = action.trim().trim_end_matches('.').to_ascii_lowercase();
            match act_low.as_str() {
                "the player does"
                | "a player does"
                | "they do"
                | "that player does"
                | "that spell is countered this way"
                | "the spell is countered this way"
                | "a spell is countered this way" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "the player doesn't"
                | "no player does"
                | "they don't"
                | "that player doesn't"
                | "they can't"
                | "that player can't" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "that player had another land enter the battlefield under their control this turn"
                | "that player had another land enter under their control this turn"
                | "they had another land enter the battlefield under their control this turn"
                | "they had another land enter under their control this turn" => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(rp)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(2),
                    })
                }
                "a land entered the battlefield under that player's control this turn"
                | "a land entered the battlefield under their control this turn"
                | "a land entered under that player's control this turn"
                | "a land entered under their control this turn" => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(rp)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "they control a commander" | "that player controls a commander" => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::count_player_battlefield(
                            rp,
                            ObjectFilter::Clause(ObjectClause::any().commander()),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "they control two or more basic lands"
                | "that player controls two or more basic lands" => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::count_player_battlefield(
                            rp,
                            ObjectFilter::Clause(ObjectClause {
                                required_supertypes: SupertypeMask::BASIC,
                                ..ObjectClause::land()
                            }),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(2),
                    })
                }
                other => {
                    if let Some(res) = lower_turn_history_player_action(
                        *player, other, builder, vocabulary, in_trigger,
                    ) {
                        res
                    } else if let Some(res) =
                        lower_comparative_controls_action(other, builder, vocabulary, in_trigger)
                    {
                        res
                    } else {
                        Err("oracle:unsupported-if-condition")
                    }
                }
            }
        }
        ConditionAst::Marker(GameMarker::InZone { subject, zone })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("it") || is_self_anaphora(a)) =>
        {
            use spellwake_engine::program::SourceZoneKind;
            let sz = match zone {
                ZoneFilter::Battlefield => SourceZoneKind::Battlefield,
                ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard => SourceZoneKind::Graveyard,
                ZoneFilter::YourHand => SourceZoneKind::Hand,
                ZoneFilter::Exile => SourceZoneKind::Exile,
                ZoneFilter::Stack => SourceZoneKind::Stack,
                ZoneFilter::CommandZone => SourceZoneKind::Command,
                ZoneFilter::YourLibrary => SourceZoneKind::Library,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(sz)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::KeywordCostPaid(kw)) => {
            let left = match kw.trim().to_ascii_lowercase().as_str() {
                "kicker" | "kicked" | "multikicker" | "offspring" | "squad" | "entwine"
                | "buyback" => ValueExpr::event(EventValueKind::KickerCount(0)),
                "bargain" | "bargained" => ValueExpr::event(EventValueKind::Bargained),
                "cleave" | "cleaved" => ValueExpr::event(EventValueKind::Cleaved),
                "gift" | "gifted" => ValueExpr::event(EventValueKind::GiftPromised),
                "overload"
                | "surge"
                | "spectacle"
                | "prowl"
                | "dash"
                | "blitz"
                | "evoke"
                | "escape"
                | "escaped"
                | "madness"
                | "miracle"
                | "foretell"
                | "foretold"
                | "plot"
                | "plotted"
                | "emerge"
                | "freerunning"
                | "warp"
                | "warped"
                | "web-slinging"
                | "bestow"
                | "disturb"
                | "mutate"
                | "mutated"
                | "flashback"
                | "prototype"
                | "mayhem"
                | "sneak"
                | "harmonize"
                | "encore"
                | "unearth"
                | "ninjutsu"
                | "suspend"
                | "teamwork"
                | "sprint"
                | "spree"
                | "additional"
                | "optional additional"
                | "additional cast" => {
                    if in_trigger {
                        ValueExpr::SourceState(SourceStateExpr::PaidAlternative)
                    } else {
                        ValueExpr::event(EventValueKind::PaidAlternative)
                    }
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(
            GameMarker::Foretold | GameMarker::Escaped | GameMarker::CastUsingTeamwork,
        ) => {
            let left = if in_trigger {
                ValueExpr::SourceState(SourceStateExpr::PaidAlternative)
            } else {
                ValueExpr::event(EventValueKind::PaidAlternative)
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::DidNotPutToZone { .. }) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::ControlledSubject(subject)) => {
            let mref = resolve_match_object_ref(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::any().controlled()),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::InCommandZoneOrBattlefield) => {
            use spellwake_engine::program::SourceZoneKind;
            Ok(ResolutionCondition {
                left: ValueExpr::Add(
                    Box::new(ValueExpr::SourceState(SourceStateExpr::InZone(
                        SourceZoneKind::Command,
                    ))),
                    Box::new(ValueExpr::SourceState(SourceStateExpr::InZone(
                        SourceZoneKind::Battlefield,
                    ))),
                ),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::RemainsExiled) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                spellwake_engine::program::SourceZoneKind::Exile,
            )),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::IsSuspended) => combine_resolution_conditions_and(vec![
            ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(
                    spellwake_engine::program::SourceZoneKind::Exile,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
            ResolutionCondition {
                left: ValueExpr::counters(
                    spellwake_engine::program::CounterSubjectRef::AbilitySource,
                    Some(CounterKind::Time),
                ),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        ])
        .ok_or("oracle:unsupported-if-condition"),
        ConditionAst::Marker(GameMarker::NotDeclaredAttacker) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::GreaterPowerOrToughnessThan { subject, other }) => {
            let resolve_gt_ref = |subj: &SubjectRefAst| -> Option<ValueObjectRef> {
                resolve_value_object_ref_for_stat(subj, builder, in_trigger, true).or_else(|| {
                    if in_trigger
                        && builder.target == BodyTarget::None
                        && matches!(subj, SubjectRefAst::Group(_))
                    {
                        Some(ValueObjectRef::TriggerSubject)
                    } else {
                        None
                    }
                })
            };
            let s_ref = resolve_gt_ref(subject).ok_or("oracle:unsupported-if-condition")?;
            let o_ref = resolve_gt_ref(other).ok_or("oracle:unsupported-if-condition")?;
            combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::object_stat(s_ref, ObjectStatKind::Power),
                    comparison: Comparison::Greater,
                    right: ValueExpr::object_stat(o_ref, ObjectStatKind::Power),
                },
                ResolutionCondition {
                    left: ValueExpr::object_stat(s_ref, ObjectStatKind::Toughness),
                    comparison: Comparison::Greater,
                    right: ValueExpr::object_stat(o_ref, ObjectStatKind::Toughness),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(GameMarker::PairedWithAnotherCreature) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::Paired),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::OwnerHasCardInExile { player }) => {
            let left = match player {
                PlayerRefAst::You => ValueExpr::Count {
                    zone: ValueZone::Exile(Some(ResolvingPlayer::Controller)),
                    filter: Box::new(ObjectFilter::any()),
                    controller_scope: None,
                },
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Exile(None),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    ValueExpr::Count {
                        zone: ValueZone::Exile(Some(rp)),
                        filter: Box::new(ObjectFilter::any()),
                        controller_scope: None,
                    }
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::PlayerHasRadCounter {
            player: PlayerRefAst::AnyOpponent,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(CounterKind::Rad)),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::FirstCombatPhase) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CombatPhasesThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::RingBearerOtherThanSelf) => {
            combine_resolution_conditions_and(vec![
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::RingLevel),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::IsRingBearer),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(
            GameMarker::EnteredOrCastFromGraveyard | GameMarker::AnyEnteredOrCastFromGraveyard,
        ) => combine_resolution_conditions_or(vec![
            ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::EnteredFromGraveyard),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
            ResolutionCondition {
                left: ValueExpr::event(EventValueKind::CastFromGraveyard),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        ])
        .ok_or("oracle:unsupported-if-condition"),
        ConditionAst::Marker(GameMarker::DiedThisWay) => Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                spellwake_engine::program::MatchObjectRef::LastMovedToGraveyard,
                ObjectFilter::any(),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ZoneActionHappenedThisWay { filter, action }) => {
            let act_low = action.trim().to_ascii_lowercase();
            let subject = match act_low.as_str() {
                "dies" | "died" | "destroyed" | "put_into_graveyard" | "graveyard"
                | "discarded" | "milled" | "sacrificed" => {
                    spellwake_engine::program::MatchObjectRef::LastMovedToGraveyard
                }
                "exiled" | "exile" | "put_into_exile" => {
                    spellwake_engine::program::MatchObjectRef::LastMovedToExile
                }
                "returned_to_hand" | "put_into_hand" | "hand" => {
                    spellwake_engine::program::MatchObjectRef::LastMovedToHand
                }
                "entered" | "put_onto_battlefield" | "battlefield" => {
                    spellwake_engine::program::MatchObjectRef::LastMovedToBattlefield
                }
                _ => spellwake_engine::program::MatchObjectRef::LastAffected,
            };
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj_filter = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(subject, obj_filter),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::OnlyObjectInZone {
            subject,
            filter,
            zone: ZoneFilter::YourGraveyard,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            combine_resolution_conditions_and(vec![
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::InZone(
                        spellwake_engine::program::SourceZoneKind::Graveyard,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::object_matches(MatchObjectRef::AbilitySource, obj),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(GameMarker::CardPutIntoGraveyardThisTurn { filter })
            if is_unrestricted_filter(filter) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::CardsEnteredGraveyardThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CardPutIntoGraveyardThisTurn { filter }) => {
            let from_battlefield_only = filter.zone == Some(ZoneFilter::Battlefield);
            if filter.zone.is_some() && !from_battlefield_only {
                return Err("oracle:unsupported-if-condition");
            }
            let (controller, keep_opp_ctrl) = match filter.controller {
                None => (None, false),
                Some(ControllerRelation::You) => (Some(ResolvingPlayer::Controller), false),
                Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => (None, true),
                Some(_) => return Err("oracle:unsupported-if-condition"),
            };
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            if !keep_opp_ctrl {
                clean.controller = None;
            }
            let obj = lower_object_filter(&clean, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                    filter: Box::new(obj),
                    from_battlefield_only,
                    controller,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::SacrificedThisTurn { player, filter }) => {
            let players = match player {
                PlayerRefAst::You => AffectedPlayers::One(ResolvingPlayer::Controller),
                PlayerRefAst::AnyOpponent => AffectedPlayers::EachOpponent,
                PlayerRefAst::AnyPlayer => AffectedPlayers::EachPlayer,
                PlayerRefAst::EachOpponent | PlayerRefAst::EachPlayer => {
                    return Err("oracle:unsupported-if-condition");
                }
                other => AffectedPlayers::One(lower_resolving_player_in_context(
                    *other, builder, in_trigger,
                )?),
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SacrificedMatchingThisTurn {
                    players,
                    filter: Box::new(obj),
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::Monarch(
            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
        )) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentIsMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NoMonarch) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::NoMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::MaxSpeed) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(4),
        }),
        ConditionAst::Marker(GameMarker::FullParty) => {
            let roles = resolve_party_roles(vocabulary).ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::PartyCount(Box::new(roles)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(4),
            })
        }
        ConditionAst::Marker(GameMarker::YouAreAttacking) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsAttacking),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::YouAreDefending) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsDefending),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::RemainsTapped) => Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                MatchObjectRef::AbilitySource,
                ObjectFilter::any().tapped(),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackingAlone) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AttackingAlone),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedOrBlockedThisCombat) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisCombat),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedThisCombat { subject }) => {
            let mref = resolve_match_object_ref(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::any().attacking()),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AttackedThisTurn { subject }) => {
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            let mref = resolve_match_object_ref(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AttackedOrBlockedThisTurn { subject })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a)) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
            subject,
            or_entered_this_turn,
        }) => {
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                let left = if *or_entered_this_turn {
                    ValueExpr::Add(
                        Box::new(ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn)),
                        Box::new(ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn)),
                    )
                } else {
                    ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn)
                };
                return Ok(ResolutionCondition {
                    left,
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            let mref = resolve_match_object_ref(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            let attacked_expr = ValueExpr::object_matches(
                mref,
                ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN),
            );
            let left = if *or_entered_this_turn {
                ValueExpr::Add(
                    Box::new(attacked_expr),
                    Box::new(ValueExpr::object_matches(
                        mref,
                        ObjectFilter::any().with_flags(ObjectFilterFlags::ENTERED_THIS_TURN),
                    )),
                )
            } else {
                attacked_expr
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            })
        }
        ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subject)) => {
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            let mref = resolve_match_object_ref(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::any().with_flags(ObjectFilterFlags::ENTERED_THIS_TURN),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::DamageDealtToSubjectThisTurn {
            subject,
            min_amount,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a)) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::DamageDealtThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_amount)),
            })
        }
        ConditionAst::Marker(GameMarker::DamageDealtToSubjectThisTurn {
            subject,
            min_amount: 1,
        }) => {
            if matches!(
                subject.as_ref(),
                SubjectRefAst::Player(PlayerRefAst::AnyOpponent)
            ) {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: false,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            let mref = resolve_match_object_ref(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::any().with_flags(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::DamageDealtToSubjectThisTurn {
            subject,
            min_amount,
        }) if *min_amount > 1 => {
            let vref = resolve_value_object_ref_for_stat(subject, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::DamageReceivedThisTurn {
                    subject: vref,
                    excess_only: false,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_amount)),
            })
        }
        ConditionAst::Marker(GameMarker::DealtDamageBySelfThisTurn { combat_only }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                    subject: ValueObjectRef::AbilitySource,
                    to_opponent_only: false,
                    combat_only: *combat_only,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::OpponentDealtDamageThisTurn { noncombat_only }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentDealtDamageThisTurn {
                    noncombat_only: *noncombat_only,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NonlandPermanentLeftThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AnyNonlandPermanentLeftBattlefieldThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::SpellWarpedThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AnySpellWarpedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::counters(
                spellwake_engine::program::CounterSubjectRef::ControllerPoison,
                Some(CounterKind::Poison),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AnyOpponentPoisoned),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerIsPoisoned { player }) => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::counters(
                    spellwake_engine::program::CounterSubjectRef::PlayerCounter(
                        rp,
                        CounterKind::Poison,
                    ),
                    Some(CounterKind::Poison),
                ),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::PlayerHasRadCounter { player }) => {
            let rad = parse_counter_kind("rad").ok_or("oracle:unsupported-if-condition")?;
            match player {
                PlayerRefAst::AnyOpponent => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(rad)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::counters(
                            spellwake_engine::program::CounterSubjectRef::PlayerCounter(rp, rad),
                            Some(rad),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
            }
        }
        ConditionAst::Marker(GameMarker::AttackedWithMinCreaturesThisTurn {
            player: PlayerRefAst::You,
            min_count,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AttackingCreaturesThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_count)),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
            from_hand_only: true,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellsFromHandThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::DifferentManaValuesInGraveyard { op, value }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::DistinctManaValuesInGraveyard(ResolvingPlayer::Controller),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Marker(GameMarker::PlayerGainedOrLostLifeThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerLifeGainedThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerLifeLostThisTurn,
                )),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ControlsMoreThanOpponents {
            filter,
            than_each_other,
        }) => {
            let obj = lower_object_filter(filter, vocabulary)?;
            let rhs = if *than_each_other {
                ValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                }
            } else {
                ValueExpr::OpponentMinCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                comparison: Comparison::Greater,
                right: rhs,
            })
        }
        ConditionAst::Marker(GameMarker::ControlsFewerThanOpponent { filter }) => {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                comparison: Comparison::Less,
                right: ValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                },
            })
        }
        ConditionAst::Marker(GameMarker::HandMoreThanEachOpponent) => Ok(ResolutionCondition {
            left: ValueExpr::count_hand(ResolvingPlayer::Controller),
            comparison: Comparison::Greater,
            right: ValueExpr::OpponentMaxCount {
                zone: ValueZone::Hand(ResolvingPlayer::Controller),
                filter: Box::new(ObjectFilter::any()),
            },
        }),
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player: PlayerRefAst::You,
            filter: None,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player: PlayerRefAst::You,
            filter: Some(f),
        }) if f.subtypes.is_empty()
            && f.any_colors.is_empty()
            && f.forbidden_colors.is_empty()
            && f.color_arity.is_none()
            && f.stat_bounds.is_empty()
            && f.token.is_none()
            && f.flags.is_empty() =>
        {
            if f.any_types.is_empty() && f.forbidden_types == AstCardTypeMask::CREATURE {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CastNoncreatureSpellsThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if f.forbidden_types.is_empty() {
                if f.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                            )),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                let mut types = card_type_mask_to_set(f.any_types)?;
                if types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellTypeThisTurn(kind)),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
            }
            let obj = lower_object_filter(f, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    filter: Box::new(obj),
                    from_zone: None,
                }),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            })
        }
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player,
            filter: Some(f),
        }) => {
            // Keep AnyOpponent fail-closed: sum == 0 means NO opponent cast, not "an opponent didn't".
            let players = match player {
                PlayerRefAst::You => AffectedPlayers::One(ResolvingPlayer::Controller),
                PlayerRefAst::EachOpponent => AffectedPlayers::EachOpponent,
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => AffectedPlayers::EachPlayer,
                PlayerRefAst::AnyOpponent => return Err("oracle:unsupported-if-condition"),
                other => AffectedPlayers::One(lower_resolving_player_in_context(
                    *other, builder, in_trigger,
                )?),
            };
            let from_zone = match f.zone {
                None => None,
                Some(ZoneFilter::YourHand) => Some(SourceZoneFilter::Hand),
                Some(ZoneFilter::YourGraveyard) => Some(SourceZoneFilter::Graveyard),
                Some(ZoneFilter::YourHandOrGraveyard) => Some(SourceZoneFilter::HandOrGraveyard),
                Some(ZoneFilter::Battlefield) => Some(SourceZoneFilter::Battlefield),
                Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
                Some(ZoneFilter::OpponentGraveyard) => Some(SourceZoneFilter::OpponentGraveyard),
                Some(
                    ZoneFilter::AnyGraveyard
                    | ZoneFilter::AllGraveyards
                    | ZoneFilter::SingleGraveyard,
                ) => Some(SourceZoneFilter::AnyGraveyard),
                Some(ZoneFilter::YourLibrary) => Some(SourceZoneFilter::YourLibrary),
                Some(_) => return Err("oracle:unsupported-if-condition"),
            };
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            let obj = lower_object_filter(&clean, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players,
                    filter: Box::new(obj),
                    from_zone,
                }),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            })
        }
        ConditionAst::Marker(GameMarker::DidNotAttackWithCreatureThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotAttackWithCreatureThisTurn {
            player: PlayerRefAst::EachOpponent,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::Attacked,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotAttackWithCreatureThisTurn {
            player: PlayerRefAst::AnyPlayer,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerAttackedThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::Attacked,
                ))),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player: PlayerRefAst::EachOpponent,
            filter: None,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::CastSpells,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player: PlayerRefAst::AnyPlayer,
            filter: None,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerSpellsCastThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::CastSpells,
                ))),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::AnyOpponent,
            op: CompareOp::Ge,
            value: 1,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::CastSpells,
            )),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::EachOpponent,
            op: CompareOp::Eq,
            value: 0,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::CastSpells,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::AnyOpponent,
            op: CompareOp::Ge,
            value: 1,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::DrewCards,
            )),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::EachOpponent,
            op: CompareOp::Eq,
            value: 0,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::DrewCards,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CreatedTokensThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerHasMoreHandThan { player, than }) => {
            let rp_a = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let rp_b = lower_resolving_player_in_context(*than, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_hand(rp_a),
                comparison: Comparison::Greater,
                right: ValueExpr::count_hand(rp_b),
            })
        }
        ConditionAst::Marker(GameMarker::Day) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Night) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(2),
        }),
        ConditionAst::Marker(GameMarker::NeitherDayNorNight) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::ControlsSelf) => Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                MatchObjectRef::AbilitySource,
                ObjectFilter::permanent().controlled(),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::RemainsOnBattlefield) if in_trigger => {
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::TriggerSourceOnBattlefield),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn { op, value }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AbilityActivatedThisTurn),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Marker(GameMarker::AbilityResolvedNthTimeThisTurn(n)) if *n >= 1 => {
            // `ability_resolutions_by_source_this_turn` increments when an ability finishes
            // resolving, so during its Nth resolution this turn the stored count is N - 1.
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(i64::from(*n - 1)),
            })
        }
        ConditionAst::Marker(GameMarker::VoteResult { winning }) => {
            let options = builder
                .instructions
                .iter()
                .rev()
                .find_map(|op| match op {
                    Instruction::Vote { options, .. } => Some(options),
                    _ => None,
                })
                .ok_or("oracle:unsupported-if-condition")?;
            let w_low = winning.trim().to_ascii_lowercase();
            let includes_tie = w_low.contains("tied") || w_low.contains("tie");
            let needle = w_low
                .split(" gets more")
                .next()
                .unwrap_or(&w_low)
                .split(" get more")
                .next()
                .unwrap_or(&w_low)
                .split(" receives more")
                .next()
                .unwrap_or(&w_low)
                .split(" or the vote")
                .next()
                .unwrap_or(&w_low)
                .trim()
                .trim_matches('"')
                .trim();
            let idx = options
                .iter()
                .position(|opt| {
                    let o = opt.trim().to_ascii_lowercase();
                    o == needle || needle.contains(&o) || o.contains(needle)
                })
                .ok_or("oracle:unsupported-if-condition")?;
            let idx_u8 = u8::try_from(idx).map_err(|_| "oracle:unsupported-if-condition")?;
            if includes_tie {
                if options.len() == 2 {
                    let other = 1u8 - idx_u8;
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::VoteCount(idx_u8)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::event(EventValueKind::VoteCount(other)),
                    })
                } else {
                    Err("oracle:unsupported-if-condition")
                }
            } else {
                Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::WinningVoteOption),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(i64::from(idx_u8)),
                })
            }
        }
        ConditionAst::Marker(GameMarker::SameManaValueAsRevealed) => {
            let spell_ref = if is_object_body_target(builder.target, false) {
                ValueObjectRef::TargetObject
            } else if in_trigger {
                ValueObjectRef::TriggeringSpell
            } else {
                return Err("oracle:unsupported-if-condition");
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_stat(spell_ref, ObjectStatKind::ManaValue),
                comparison: Comparison::Equal,
                right: ValueExpr::object_stat(
                    ValueObjectRef::RevealedTop,
                    ObjectStatKind::ManaValue,
                ),
            })
        }
        ConditionAst::Marker(GameMarker::SameManaValueAsTriggerSpell)
            if in_trigger && is_object_body_target(builder.target, false) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::object_stat(
                    ValueObjectRef::TargetObject,
                    ObjectStatKind::ManaValue,
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::object_stat(
                    ValueObjectRef::TriggeringSpell,
                    ObjectStatKind::ManaValue,
                ),
            })
        }
        ConditionAst::Marker(GameMarker::SubjectWasCast { subject, was_cast })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a)
                    || (a.eq_ignore_ascii_case("it")
                        && !is_object_body_target(builder.target, false)
                        && !in_trigger)) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::WasCast {
                    by_controller: false,
                    from_hand: false,
                }),
                comparison: if *was_cast {
                    Comparison::GreaterOrEqual
                } else {
                    Comparison::Equal
                },
                right: ValueExpr::literal(if *was_cast { 1 } else { 0 }),
            })
        }
        ConditionAst::Marker(GameMarker::EnteredOrCastFromZone(zone)) => match zone {
            ZoneFilter::YourGraveyard => combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                        SourceZoneFilter::Graveyard,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::CastFromGraveyard),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition"),
            ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards | ZoneFilter::SingleGraveyard => {
                combine_resolution_conditions_or(vec![
                    ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                            SourceZoneFilter::AnyGraveyard,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::CastFromGraveyard),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition")
            }
            ZoneFilter::Exile => combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                        SourceZoneFilter::Exile,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::CastFromExile),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition"),
            ZoneFilter::YourHand => combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                        SourceZoneFilter::Hand,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::WasCast {
                        by_controller: false,
                        from_hand: true,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition"),
            ZoneFilter::YourLibrary => Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                    SourceZoneFilter::YourLibrary,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }),
            ZoneFilter::OpponentGraveyard => Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                    SourceZoneFilter::OpponentGraveyard,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }),
            ZoneFilter::Battlefield => Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::EnteredFromZone(
                    SourceZoneFilter::Battlefield,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }),
            _ => Err("oracle:unsupported-if-condition"),
        },
        ConditionAst::Marker(GameMarker::DamagePreventedThisWay) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::DamagePreventedThisWay),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::DealtDamageThisWay { combat_only }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::DealtDamageThisWay {
                    combat_only: *combat_only,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::SharesCardTypeWithExiled) => {
            let mref = resolve_match_object_ref(
                &SubjectRefAst::Anaphora("it".to_string()),
                builder,
                in_trigger,
                false,
            )
            .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::SharesCardTypeWithLinkedExile(mref)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::LostLifeLastTurn { player }) => {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLifeLostLastTurn(rp)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::TopCardOfZoneMatches { zone, filter }) => {
            let rp = match zone {
                ZoneFilter::YourLibrary => ResolvingPlayer::Controller,
                ZoneFilter::TargetPlayerLibrary => {
                    if builder.target == BodyTarget::None {
                        builder.bind_target(BodyTarget::SinglePlayer)?;
                    }
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) {
                        ResolvingPlayer::TargetPlayer
                    } else {
                        return Err("oracle:unsupported-if-condition");
                    }
                }
                ZoneFilter::TargetOpponentLibrary => {
                    if builder.target == BodyTarget::None {
                        builder.bind_target(BodyTarget::SingleOpponent)?;
                    }
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) {
                        ResolvingPlayer::TargetPlayer
                    } else {
                        return Err("oracle:unsupported-if-condition");
                    }
                }
                ZoneFilter::ThatPlayerLibrary => lower_resolving_player_in_context(
                    PlayerRefAst::ThatPlayer,
                    builder,
                    in_trigger,
                )?,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::LibraryTop(rp), obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CastFilteredSpellsThisTurn {
            player,
            filter,
            op,
            value,
        }) => {
            let players = match player {
                PlayerRefAst::You => AffectedPlayers::One(ResolvingPlayer::Controller),
                PlayerRefAst::AnyOpponent
                    if (*op == CompareOp::Ge && *value == 1)
                        || (*op == CompareOp::Gt && *value == 0) =>
                {
                    AffectedPlayers::EachOpponent
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Eq | CompareOp::Le) && *value == 0 =>
                {
                    AffectedPlayers::EachOpponent
                }
                PlayerRefAst::AnyPlayer
                    if (*op == CompareOp::Ge && *value == 1)
                        || (*op == CompareOp::Gt && *value == 0) =>
                {
                    AffectedPlayers::EachPlayer
                }
                PlayerRefAst::EachPlayer
                    if matches!(op, CompareOp::Eq | CompareOp::Le) && *value == 0 =>
                {
                    AffectedPlayers::EachPlayer
                }
                PlayerRefAst::AnyOpponent
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::AnyPlayer
                | PlayerRefAst::EachPlayer => return Err("oracle:unsupported-if-condition"),
                other => AffectedPlayers::One(lower_resolving_player_in_context(
                    *other, builder, in_trigger,
                )?),
            };
            let from_zone = match filter.zone {
                None => None,
                Some(ZoneFilter::YourHand) => Some(SourceZoneFilter::Hand),
                Some(ZoneFilter::YourGraveyard) => Some(SourceZoneFilter::Graveyard),
                Some(ZoneFilter::YourHandOrGraveyard) => Some(SourceZoneFilter::HandOrGraveyard),
                Some(ZoneFilter::Battlefield) => Some(SourceZoneFilter::Battlefield),
                Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
                Some(ZoneFilter::OpponentGraveyard) => Some(SourceZoneFilter::OpponentGraveyard),
                Some(
                    ZoneFilter::AnyGraveyard
                    | ZoneFilter::AllGraveyards
                    | ZoneFilter::SingleGraveyard,
                ) => Some(SourceZoneFilter::AnyGraveyard),
                Some(ZoneFilter::YourLibrary) => Some(SourceZoneFilter::YourLibrary),
                Some(_) => return Err("oracle:unsupported-if-condition"),
            };
            let mut clean_filter = filter.as_ref().clone();
            clean_filter.zone = None;
            let obj = lower_object_filter(&clean_filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players,
                    filter: Box::new(obj),
                    from_zone,
                }),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Marker(GameMarker::MostCommonPermanentColor { color, or_tied }) => {
            let target_color = match *color {
                c if c == AstColorMask::WHITE => Color::White,
                c if c == AstColorMask::BLUE => Color::Blue,
                c if c == AstColorMask::BLACK => Color::Black,
                c if c == AstColorMask::RED => Color::Red,
                c if c == AstColorMask::GREEN => Color::Green,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let target_count = ValueExpr::count_battlefield(
                ObjectFilter::permanent().with_color(ObjectColorFilter::has(target_color)),
            );
            let cmp = if *or_tied {
                Comparison::GreaterOrEqual
            } else {
                Comparison::Greater
            };
            let conjuncts = [
                Color::White,
                Color::Blue,
                Color::Black,
                Color::Red,
                Color::Green,
            ]
            .into_iter()
            .filter(|&c| c != target_color)
            .map(|other_c| ResolutionCondition {
                left: target_count.clone(),
                comparison: cmp,
                right: ValueExpr::count_battlefield(
                    ObjectFilter::permanent().with_color(ObjectColorFilter::has(other_c)),
                ),
            })
            .collect();
            combine_resolution_conditions_and(conjuncts).ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(GameMarker::PlayerHadCardsPutIntoGraveyardThisTurn {
            player,
            min_count,
        }) => match player {
            PlayerRefAst::You => Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::CardsEnteredGraveyardThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            }),
            PlayerRefAst::AnyOpponent if *min_count == 1 => Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                    filter: Box::new(ObjectFilter::Clause(ObjectClause {
                        controller: ControllerFilter::OwnerOpponent,
                        ..ObjectClause::any()
                    })),
                    from_battlefield_only: false,
                    controller: None,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            }),
            PlayerRefAst::AnyOpponent
            | PlayerRefAst::EachOpponent
            | PlayerRefAst::AnyPlayer
            | PlayerRefAst::EachPlayer => Err("oracle:unsupported-if-condition"),
            other => {
                let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(ObjectFilter::any()),
                            from_battlefield_only: false,
                            controller: Some(rp),
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                })
            }
        },
        ConditionAst::Marker(GameMarker::RingTemptedMinTimesThisGame(n)) => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::RingLevel),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*n)),
            })
        }
        ConditionAst::Marker(GameMarker::SubjectIsRingBearer(subj)) => {
            let subj = unwrap_battlefield_subject(subj);
            if matches!(subj, SubjectRefAst::SelfObject)
                || matches!(subj, SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                Ok(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::IsRingBearer),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })
            } else {
                Err("oracle:unsupported-if-condition")
            }
        }
        ConditionAst::Marker(GameMarker::TargetsCommander) => {
            if is_object_body_target(builder.target, true) {
                Ok(ResolutionCondition {
                    left: ValueExpr::object_matches(
                        MatchObjectRef::TargetObject,
                        ObjectFilter::Clause(ObjectClause::any().commander()),
                    ),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                })
            } else {
                Err("oracle:unsupported-if-condition")
            }
        }
        ConditionAst::Marker(GameMarker::PlayerHasExtremePlayerStat {
            player: PlayerRefAst::You,
            stat,
            greatest,
        }) => {
            let cmp = if *greatest {
                Comparison::Greater
            } else {
                Comparison::Less
            };
            match stat {
                PlayerStatKind::LifeTotal => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: cmp,
                    right: ValueExpr::PlayerStat(if *greatest {
                        PlayerStatExpr::OpponentMaxLife
                    } else {
                        PlayerStatExpr::OpponentMinLife
                    }),
                }),
                PlayerStatKind::CardsInHand => Ok(ResolutionCondition {
                    left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                    comparison: cmp,
                    right: if *greatest {
                        ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        }
                    } else {
                        ValueExpr::OpponentMinCount {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        }
                    },
                }),
                PlayerStatKind::CardsInGraveyard => Ok(ResolutionCondition {
                    left: ValueExpr::count_graveyard(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    ),
                    comparison: cmp,
                    right: if *greatest {
                        ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        }
                    } else {
                        ValueExpr::OpponentMinCount {
                            zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        }
                    },
                }),
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::Marker(GameMarker::IsNthSpellMatchingThisTurn { nth, filters }) => {
            if filters.is_empty() || *nth == 0 || (filters.len() > 1 && !in_trigger) {
                return Err("oracle:unsupported-if-condition");
            }
            let mut disjuncts = Vec::with_capacity(filters.len());
            for flt in filters {
                if (flt.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0 {
                    return Err("oracle:unsupported-if-condition");
                }
                let from_zone = match flt.zone {
                    None => None,
                    Some(ZoneFilter::YourHand) => Some(SourceZoneFilter::Hand),
                    Some(ZoneFilter::YourGraveyard) => Some(SourceZoneFilter::Graveyard),
                    Some(ZoneFilter::YourHandOrGraveyard) => {
                        Some(SourceZoneFilter::HandOrGraveyard)
                    }
                    Some(ZoneFilter::Battlefield) => Some(SourceZoneFilter::Battlefield),
                    Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
                    Some(ZoneFilter::OpponentGraveyard) => {
                        Some(SourceZoneFilter::OpponentGraveyard)
                    }
                    Some(
                        ZoneFilter::AnyGraveyard
                        | ZoneFilter::AllGraveyards
                        | ZoneFilter::SingleGraveyard,
                    ) => Some(SourceZoneFilter::AnyGraveyard),
                    Some(ZoneFilter::YourLibrary) => Some(SourceZoneFilter::YourLibrary),
                    Some(_) => return Err("oracle:unsupported-if-condition"),
                };
                let mut clean = flt.clone();
                clean.zone = None;
                let obj = lower_object_filter(&clean, vocabulary)?;
                let count_cond = ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        filter: Box::new(obj),
                        from_zone,
                    }),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(i64::from(*nth)),
                };
                let branch = if in_trigger && obj != ObjectFilter::any() {
                    combine_resolution_conditions_and(vec![
                        ResolutionCondition {
                            left: ValueExpr::object_matches(MatchObjectRef::TriggerSubject, obj),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        },
                        count_cond,
                    ])
                    .ok_or("oracle:unsupported-if-condition")?
                } else {
                    count_cond
                };
                disjuncts.push(branch);
            }
            combine_resolution_conditions_or(disjuncts).ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(GameMarker::DistinctCardTypesAmong { filter, min_count }) => {
            let unrestricted_types = filter.any_types.is_empty()
                && filter.forbidden_types.is_empty()
                && filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.subtypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.controller.is_none()
                && filter.owner.is_none()
                && filter.extra_zones.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.counter_kind.is_none()
                && filter.named.is_none()
                && filter.flags.is_empty()
                && filter.predicates.is_empty();
            if !unrestricted_types {
                return Err("oracle:unsupported-if-condition");
            }
            match filter.zone {
                Some(ZoneFilter::YourGraveyard) => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                }),
                Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard) => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::AllGraveyardsDistinctCardTypes),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    })
                }
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::EventOccurredThisTurn { event } => {
            let ev = event.trim().to_ascii_lowercase();
            match ev.as_str() {
                "you haven't cast a spell this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "you cast a spell this turn" | "you've cast a spell this turn" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "you lost life this turn" | "you've lost life this turn" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "you didn't activate a loyalty ability of a planeswalker this turn" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::LoyaltyActivatedThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "it isn't your main phase" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::IsOwnMainPhase),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "you control a permanent of each color" => Ok(ResolutionCondition {
                    left: ValueExpr::DistinctColors {
                        filter: Box::new(ObjectFilter::permanent().controlled()),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(5),
                }),
                "you have no land cards in hand" => Ok(ResolutionCondition {
                    left: ValueExpr::Count {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::land()),
                        controller_scope: None,
                    },
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "you have seven or more land cards in your hand" => Ok(ResolutionCondition {
                    left: ValueExpr::Count {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::land()),
                        controller_scope: None,
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(7),
                }),
                "you control a forest and a plains" => combine_resolution_conditions_and(vec![
                    ResolutionCondition {
                        left: ValueExpr::count_player_battlefield(
                            ResolvingPlayer::Controller,
                            ObjectFilter::Clause(ObjectClause {
                                subtypes: SubtypeFilter::Land(LandType::Forest),
                                ..ObjectClause::any()
                            }),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    ResolutionCondition {
                        left: ValueExpr::count_player_battlefield(
                            ResolvingPlayer::Controller,
                            ObjectFilter::Clause(ObjectClause {
                                subtypes: SubtypeFilter::Land(LandType::Plains),
                                ..ObjectClause::any()
                            }),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition"),
                "you control five or more enchantments" => Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::enchantment(),
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(5),
                }),
                "you control nine or more creatures" => Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::creature(),
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(9),
                }),
                "you control six or more shrines" => Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::Clause(ObjectClause {
                            subtypes: SubtypeFilter::Enchantment(EnchantmentSubtype::Shrine),
                            ..ObjectClause::any()
                        }),
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(6),
                }),
                "an instant card and a sorcery card are in your graveyard" => {
                    combine_resolution_conditions_and(vec![
                        ResolutionCondition {
                            left: ValueExpr::Count {
                                zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                                filter: Box::new(ObjectFilter::Clause(ObjectClause::instant())),
                                controller_scope: None,
                            },
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        ResolutionCondition {
                            left: ValueExpr::Count {
                                zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                                filter: Box::new(ObjectFilter::Clause(ObjectClause::sorcery())),
                                controller_scope: None,
                            },
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                    ])
                    .ok_or("oracle:unsupported-if-condition")
                }
                "that player controls three or fewer lands" => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::count_player_battlefield(rp, ObjectFilter::land()),
                        comparison: Comparison::LessOrEqual,
                        right: ValueExpr::literal(3),
                    })
                }
                "that creature is attacking" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(mref, ObjectFilter::any().attacking()),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })
                }
                "that creature remains tapped" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(mref, ObjectFilter::any().tapped()),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })
                }
                "that creature wasn't dealt damage this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any()
                                .with_flags(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "an opponent controls that creature" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(mref, ObjectFilter::any().opposing()),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })
                }
                "you don't control it" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("it".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(mref, ObjectFilter::any().controlled()),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "it's white" | "it's blue" | "it's black" | "it's red" | "it's green" => {
                    let color = match ev.as_str() {
                        "it's white" => Color::White,
                        "it's blue" => Color::Blue,
                        "it's black" => Color::Black,
                        "it's red" => Color::Red,
                        "it's green" => Color::Green,
                        _ => return Err("oracle:unsupported-if-condition"),
                    };
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("it".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::Clause(ObjectClause {
                                color: ObjectColorFilter::has(color),
                                ..ObjectClause::any()
                            }),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })
                }
                "~ is unblocked" => Ok(ResolutionCondition {
                    left: ValueExpr::object_matches(
                        MatchObjectRef::AbilitySource,
                        ObjectFilter::any().combat(CombatFilter::Unblocked),
                    ),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                }),
                "~ is in a graveyard" => Ok(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::InZone(
                        spellwake_engine::program::SourceZoneKind::Graveyard,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "~'s power is negative" => Ok(ResolutionCondition {
                    left: ValueExpr::object_stat(
                        ValueObjectRef::AbilitySource,
                        ObjectStatKind::Power,
                    ),
                    comparison: Comparison::Less,
                    right: ValueExpr::literal(0),
                }),
                "this is the fourth time this ability has resolved this turn" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(3),
                    })
                }
                "~'s sneak cost was paid" => Ok(ResolutionCondition {
                    left: if in_trigger {
                        ValueExpr::SourceState(SourceStateExpr::PaidAlternative)
                    } else {
                        ValueExpr::event(EventValueKind::PaidAlternative)
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "it comes up heads" | "the coin comes up heads" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "it comes up tails" | "the coin comes up tails" | "you lose a flip" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "both coins come up heads" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CoinFlipsWon),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                }),
                "both coins come up tails" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CoinFlipsWon),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "you rolled 6 or higher" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastDieRoll),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(6),
                }),
                "the result is 3 or less" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastDieRoll),
                    comparison: Comparison::LessOrEqual,
                    right: ValueExpr::literal(3),
                }),
                "you cast a spell this way"
                | "that spell is countered this way"
                | "that permanent is destroyed this way"
                | "~ is destroyed this way"
                | "it is destroyed this way"
                | "you gain control of a creature this way"
                | "you draw one or more cards this way"
                | "those permanents are exchanged this way"
                | "you lost life this way" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "~ deals damage to you this way" | "it deals damage to you this way"
                    if !builder.instructions.is_empty() =>
                {
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::DealtDamageThisWay {
                            combat_only: false,
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "no counters were removed this way" if !builder.instructions.is_empty() => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionCount),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "it wasn't the first land you played this turn"
                | "this wasn't the first land you played this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                }),
                "you don't cast it this way" | "you don't put it into your hand" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "two or more cards are exiled this way" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionCount),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                }),
                "~ has seven or more ember counters on it" => {
                    let ck =
                        parse_counter_kind("ember").ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::counters(
                            spellwake_engine::program::CounterSubjectRef::AbilitySource,
                            Some(ck),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(7),
                    })
                }
                "you've cast a spell from a graveyard or activated an ability of a card in a graveyard this turn"
                | "you cast a spell from a graveyard or activated an ability of a card in a graveyard this turn" => {
                    combine_resolution_conditions_or(vec![
                        ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::SpellsCastMatchingThisTurn {
                                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                    filter: Box::new(ObjectFilter::any()),
                                    from_zone: Some(SourceZoneFilter::AnyGraveyard),
                                },
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(
                                    ResolvingPlayer::Controller,
                                ),
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                    ])
                    .ok_or("oracle:unsupported-if-condition")
                }
                "you activated an ability of a card in your graveyard this turn"
                | "you activated an ability of a card in a graveyard this turn"
                | "you've activated an ability of a card in your graveyard this turn"
                | "you've activated an ability of a card in a graveyard this turn" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(
                                ResolvingPlayer::Controller,
                            ),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "one or more cards were put into exile this turn"
                | "a card was put into exile this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsExiledThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "you haven't activated an exhaust ability this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ExhaustActivatedThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "~ dealt damage to an opponent this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                        subject: ValueObjectRef::AbilitySource,
                        to_opponent_only: true,
                        combat_only: false,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "~ dealt combat damage to an opponent this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                        subject: ValueObjectRef::AbilitySource,
                        to_opponent_only: true,
                        combat_only: true,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "it was the second spell you cast this turn"
                | "it's the second spell you cast this turn"
                | "this is the second spell you cast this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(2),
                }),
                "you didn't cast a spell from exile this turn"
                | "you haven't cast a spell from exile this turn" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                        from_zone: Some(SourceZoneFilter::Exile),
                    }),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "a land you controlled was put into a graveyard from the battlefield this turn" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                                filter: Box::new(ObjectFilter::land()),
                                from_battlefield_only: true,
                                controller: Some(ResolvingPlayer::Controller),
                            },
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "it didn't attack this turn" | "that creature didn't attack this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                "that creature attacked this turn" | "it attacked this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })
                }
                "it was dealt damage this turn" | "that creature was dealt damage this turn" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(
                            mref,
                            ObjectFilter::any()
                                .with_flags(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })
                }
                "that creature was dealt excess damage this turn"
                | "it was dealt excess damage this turn" => {
                    let vref = resolve_value_object_ref_for_stat(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::DamageReceivedThisTurn {
                            subject: vref,
                            excess_only: true,
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "it dealt combat damage to a player this turn"
                | "that creature dealt combat damage to a player this turn" => {
                    let vref = resolve_value_object_ref_for_stat(
                        &SubjectRefAst::Anaphora("that creature".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                            subject: vref,
                            to_opponent_only: false,
                            combat_only: true,
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "it shares a card type with a card exiled with ~"
                | "it shares a card type with the exiled card" => {
                    let mref = resolve_match_object_ref(
                        &SubjectRefAst::Anaphora("it".to_string()),
                        builder,
                        in_trigger,
                        false,
                    )
                    .ok_or("oracle:unsupported-if-condition")?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::SharesCardTypeWithLinkedExile(mref)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "that artifact is put into a graveyard this way"
                | "that creature is put into a graveyard this way"
                | "a creature is put into a graveyard this way" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                other => {
                    if let Some((cnt_raw, rest)) = other.split_once(" or more ")
                        && let Some(min_n) = parse_small_positive_count(cnt_raw)
                        && let Some((flt_raw, counter_tail)) = rest
                            .split_once(" have an ")
                            .or_else(|| rest.split_once(" have a "))
                        && let Some(ck_raw) = counter_tail
                            .strip_suffix(" counter on them")
                            .or_else(|| counter_tail.strip_suffix(" counters on them"))
                        && let Some(ck) = parse_counter_kind(ck_raw.trim())
                        && ck.is_permanent()
                    {
                        let flt_clean = flt_raw.trim();
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        if !flt_clean.is_empty()
                            && !flt_clean.contains(" named ")
                            && !flt_clean.contains(" chosen ")
                            && let Ok(mut parsed) =
                                spellwake_oracle_compiler::filter::parse_object_filter(
                                    flt_clean,
                                    &compiler_vocab,
                                )
                            && parsed.zone.is_none()
                            && parsed.extra_zones.is_empty()
                            && parsed.counter_kind.is_none()
                            && parsed.named.is_none()
                            && parsed.predicates.is_empty()
                        {
                            parsed.counter_kind = Some(ck_raw.trim().to_string());
                            if let Ok(obj) = lower_object_filter(&parsed, vocabulary) {
                                return Ok(ResolutionCondition {
                                    left: ValueExpr::count_battlefield(obj),
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(min_n),
                                });
                            }
                        }
                    }
                    if let Some(after_ctrl) = other.strip_prefix("you control ")
                        && let Some((cnt_raw, flt_raw)) = after_ctrl.split_once(" or more ")
                        && let Some(min_n) = parse_small_positive_count(cnt_raw)
                    {
                        let flt_clean = flt_raw.trim().trim_end_matches('.');
                        if !flt_clean.is_empty()
                            && !flt_clean.contains(" and ")
                            && !flt_clean.contains(" or ")
                            && !flt_clean.contains(" named ")
                            && !flt_clean.contains(" chosen ")
                            && !flt_clean.contains(" same name ")
                        {
                            let compiler_vocab = cached_compiler_vocab(vocabulary);
                            if let Ok(parsed) =
                                spellwake_oracle_compiler::filter::parse_object_filter(
                                    flt_clean,
                                    &compiler_vocab,
                                )
                                && (matches!(flt_clean, "permanent" | "permanents")
                                    || !is_unrestricted_filter(&parsed))
                                && parsed.zone.is_none()
                                && parsed.extra_zones.is_empty()
                                && parsed.controller.is_none()
                                && parsed.owner.is_none()
                                && parsed.named.is_none()
                                && parsed.predicates.is_empty()
                                && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
                            {
                                return Ok(ResolutionCondition {
                                    left: ValueExpr::count_player_battlefield(
                                        ResolvingPlayer::Controller,
                                        obj,
                                    ),
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(min_n),
                                });
                            }
                        }
                    }
                    if let Some(flt_raw) = other
                        .strip_prefix("you control an ")
                        .or_else(|| other.strip_prefix("you control a "))
                    {
                        let flt_clean = flt_raw.trim().trim_end_matches('.');
                        if !flt_clean.is_empty()
                            && !flt_clean.contains(" and ")
                            && !flt_clean.contains(" or ")
                            && !flt_clean.contains(" named ")
                            && !flt_clean.contains(" chosen ")
                        {
                            let compiler_vocab = cached_compiler_vocab(vocabulary);
                            if let Ok(parsed) =
                                spellwake_oracle_compiler::filter::parse_object_filter(
                                    flt_clean,
                                    &compiler_vocab,
                                )
                                && (flt_clean == "permanent" || !is_unrestricted_filter(&parsed))
                                && parsed.zone.is_none()
                                && parsed.extra_zones.is_empty()
                                && parsed.controller.is_none()
                                && parsed.owner.is_none()
                                && parsed.named.is_none()
                                && parsed.predicates.is_empty()
                                && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
                            {
                                return Ok(ResolutionCondition {
                                    left: ValueExpr::count_player_battlefield(
                                        ResolvingPlayer::Controller,
                                        obj,
                                    ),
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(1),
                                });
                            }
                        }
                    }
                    if let Some(rc) = lower_mana_spent_on_triggering_spell_action(other, in_trigger)
                    {
                        Ok(rc)
                    } else if let Some(res) = lower_graveyard_count_action(other, vocabulary) {
                        res
                    } else if let Some(res) =
                        lower_zone_action_this_way_action(other, builder, vocabulary)
                    {
                        res
                    } else if let Some(res) = lower_turn_history_player_action(
                        PlayerRefAst::You,
                        other,
                        builder,
                        vocabulary,
                        in_trigger,
                    ) {
                        res
                    } else if let Some(res) =
                        lower_comparative_controls_action(other, builder, vocabulary, in_trigger)
                    {
                        res
                    } else {
                        Err("oracle:unsupported-if-condition")
                    }
                }
            }
        }
        ConditionAst::Marker(GameMarker::ControlsNamed {
            player,
            filter,
            name,
        }) => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !filter.extra_zones.is_empty()
                || filter.named.is_some()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let key = spellwake_engine::program::ExactNameKey::from_name(name)
                .ok_or("oracle:unsupported-if-condition")?;
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            let mut obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::permanent()
            } else {
                lower_object_filter(&clean, vocabulary)?
            };
            match &mut obj {
                ObjectFilter::Clause(c) => {
                    if c.exact_name.is_some() {
                        return Err("oracle:unsupported-if-condition");
                    }
                    c.exact_name = Some(key);
                }
                ObjectFilter::Either(a, b) => {
                    if a.exact_name.is_some() || b.exact_name.is_some() {
                        return Err("oracle:unsupported-if-condition");
                    }
                    a.exact_name = Some(key);
                    b.exact_name = Some(key);
                }
            }
            let left = match player {
                PlayerRefAst::You => {
                    ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj)
                }
                PlayerRefAst::AnyOpponent => ValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                },
                PlayerRefAst::EachOpponent => ValueExpr::OpponentMinCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                },
                PlayerRefAst::AnyPlayer => ValueExpr::count_battlefield(obj),
                PlayerRefAst::EachPlayer | PlayerRefAst::EachOtherPlayer => {
                    return Err("oracle:unsupported-if-condition");
                }
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    ValueExpr::count_player_battlefield(rp, obj)
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AllMatchingAre { filter, predicate }) => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !filter.extra_zones.is_empty()
                || !matches!(predicate.zone, None | Some(ZoneFilter::Battlefield))
                || !predicate.extra_zones.is_empty()
                || !predicate.any_types.is_empty()
                || predicate.named.is_some()
                || predicate.color_arity.is_some()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut base = filter.as_ref().clone();
            base.zone = None;
            let mut merged = base.clone();
            if let Some(t) = predicate.tapped {
                if merged.tapped.is_some() && merged.tapped != Some(t) {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.tapped = Some(t);
            }
            if let Some(tok) = predicate.token {
                if merged.token.is_some() && merged.token != Some(tok) {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.token = Some(tok);
            }
            if let Some(c) = predicate.combat {
                if merged.combat.is_some() && merged.combat != Some(c) {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.combat = Some(c);
            }
            if let Some(ctrl) = predicate.controller {
                if merged.controller.is_some() && merged.controller != Some(ctrl) {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.controller = Some(ctrl);
            }
            if let Some(own) = predicate.owner {
                if merged.owner.is_some() && merged.owner != Some(own) {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.owner = Some(own);
            }
            if !predicate.subtypes.is_empty() {
                if !merged.subtypes.is_empty() {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.subtypes = predicate.subtypes.clone();
            }
            if !predicate.any_colors.is_empty() {
                if !merged.any_colors.is_empty() || merged.color_arity.is_some() {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.any_colors = predicate.any_colors;
            }
            if let Some(ck) = &predicate.counter_kind {
                if merged.counter_kind.is_some() {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.counter_kind = Some(ck.clone());
            }
            merged.forbidden_types.0 |= predicate.forbidden_types.0;
            merged.required_supertypes.0 |= predicate.required_supertypes.0;
            merged.forbidden_supertypes.0 |= predicate.forbidden_supertypes.0;
            merged.forbidden_colors.0 |= predicate.forbidden_colors.0;
            merged.flags.0 |= predicate.flags.0;
            merged
                .forbidden_subtypes
                .extend(predicate.forbidden_subtypes.iter().cloned());
            merged
                .required_keywords
                .extend(predicate.required_keywords.iter().cloned());
            merged
                .forbidden_keywords
                .extend(predicate.forbidden_keywords.iter().cloned());
            merged
                .stat_bounds
                .extend(predicate.stat_bounds.iter().cloned());
            merged
                .predicates
                .extend(predicate.predicates.iter().cloned());
            let base_obj = lower_object_filter(&base, vocabulary)?;
            let merged_obj = lower_object_filter(&merged, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_battlefield(base_obj),
                comparison: Comparison::Equal,
                right: ValueExpr::count_battlefield(merged_obj),
            })
        }
        ConditionAst::Marker(GameMarker::AbilityCounteredThisWay {
            permanent_only: false,
        }) if !builder.instructions.is_empty() => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ZoneActionCountThisWay { min_count, .. })
            if !builder.instructions.is_empty() && *min_count >= 1 =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::event(if *min_count == 1 {
                    EventValueKind::LastActionSucceeded
                } else {
                    EventValueKind::LastActionCount
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        ConditionAst::Marker(GameMarker::DuringStep { player, step }) => {
            use spellwake_engine::program::PhaseCheck;
            use spellwake_engine::turn::Step as EngineStep;
            use spellwake_oracle_compiler::ast::TurnStepAst;
            let opt_rp = match player {
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => None,
                PlayerRefAst::AnyOpponent
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachOtherPlayer
                | PlayerRefAst::EachOtherOpponent => {
                    return Err("oracle:unsupported-if-condition");
                }
                other => Some(lower_resolving_player_in_context(
                    *other, builder, in_trigger,
                )?),
            };
            let engine_step = match step {
                TurnStepAst::MainPhase => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::IsInPhase {
                            phase: PhaseCheck::Main,
                            player: opt_rp,
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                TurnStepAst::UntapStep => EngineStep::Untap,
                TurnStepAst::Upkeep => EngineStep::Upkeep,
                TurnStepAst::DrawStep => EngineStep::Draw,
                TurnStepAst::PrecombatMain => EngineStep::PrecombatMain,
                TurnStepAst::PostcombatMain => EngineStep::PostcombatMain,
                TurnStepAst::BeginningOfCombat => EngineStep::BeginningCombat,
                TurnStepAst::EndOfCombat => EngineStep::EndCombat,
                TurnStepAst::EndStep => EngineStep::End,
                TurnStepAst::CleanupStep => EngineStep::Cleanup,
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::IsInStep {
                    step: engine_step,
                    player: opt_rp,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AttackingPlayerWithMostLife { or_tied }) if in_trigger => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHasMostLife {
                    player: ResolvingPlayer::DefendingPlayer,
                    allow_tie: *or_tied,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::PlayerHasExtremePlayerStat {
            player,
            stat,
            greatest: true,
        }) if !matches!(
            player,
            PlayerRefAst::You
                | PlayerRefAst::AnyPlayer
                | PlayerRefAst::EachPlayer
                | PlayerRefAst::EachOtherPlayer
                | PlayerRefAst::AnyOpponent
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachOtherOpponent
        ) =>
        {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            let expr = match stat {
                PlayerStatKind::LifeTotal => PlayerStatExpr::PlayerHasMostLife {
                    player: rp,
                    allow_tie: false,
                },
                PlayerStatKind::CardsInHand => PlayerStatExpr::PlayerControlsMost {
                    player: rp,
                    zone: ValueZone::Hand(rp),
                    filter: Box::new(ObjectFilter::any()),
                    allow_tie: false,
                },
                PlayerStatKind::CardsInGraveyard => PlayerStatExpr::PlayerControlsMost {
                    player: rp,
                    zone: ValueZone::Graveyard(rp),
                    filter: Box::new(ObjectFilter::any()),
                    allow_tie: false,
                },
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(expr),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::AnyPlayer,
            op: CompareOp::Ge,
            value: 1,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerSpellsCastThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::CastSpells,
                ))),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
            op: CompareOp::Eq,
            value: 0,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerSpellsCastThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::CastSpells,
                ))),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::AnyOpponent,
            op: CompareOp::Ge,
            value: 1,
            from_hand_only: true,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                players: AffectedPlayers::EachOpponent,
                filter: Box::new(ObjectFilter::any()),
                from_zone: Some(SourceZoneFilter::Hand),
            }),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player,
            op,
            value,
            from_hand_only,
        }) if !matches!(
            player,
            PlayerRefAst::You
                | PlayerRefAst::AnyPlayer
                | PlayerRefAst::EachPlayer
                | PlayerRefAst::EachOtherPlayer
                | PlayerRefAst::AnyOpponent
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachOtherOpponent
        ) =>
        {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players: AffectedPlayers::One(rp),
                    filter: Box::new(ObjectFilter::any()),
                    from_zone: from_hand_only.then_some(SourceZoneFilter::Hand),
                }),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Marker(GameMarker::AttackedWithMinCreaturesThisTurn {
            player: PlayerRefAst::AnyOpponent,
            min_count: 1,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                spellwake_engine::program::OpponentTurnStat::Attacked,
            )),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedWithMinCreaturesThisTurn {
            player: PlayerRefAst::AnyPlayer,
            min_count: 1,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerAttackedThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::Attacked,
                ))),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedWithMinCreaturesThisTurn {
            player,
            min_count,
        }) if *min_count >= 1
            && !matches!(
                player,
                PlayerRefAst::You
                    | PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOtherPlayer
                    | PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherOpponent
            ) =>
        {
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                    players: AffectedPlayers::One(rp),
                    filter: Box::new(ObjectFilter::creature()),
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        ConditionAst::Marker(GameMarker::PlayerGainedOrLostLifeThisTurn {
            player: PlayerRefAst::AnyOpponent,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::GainedLife,
                ))),
                Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::LostLife,
                ))),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerGainedOrLostLifeThisTurn {
            player: PlayerRefAst::AnyPlayer,
        }) => combine_resolution_conditions_or(vec![
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::GainedLife,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                    spellwake_engine::program::OpponentTurnStat::LostLife,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        ])
        .ok_or("oracle:unsupported-if-condition"),
        ConditionAst::Marker(GameMarker::SubjectRemainsExiled { subject, any_of }) => {
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::InZone(
                        spellwake_engine::program::SourceZoneKind::Exile,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            let is_single_linked = !*any_of
                && matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("that card")
                );
            if in_trigger && (*any_of || is_single_linked) {
                return Ok(ResolutionCondition {
                    left: ValueExpr::Count {
                        zone: ValueZone::Exile(None),
                        filter: Box::new(
                            ObjectFilter::any().with_flags(ObjectFilterFlags::EXILED_WITH_SELF),
                        ),
                        controller_scope: None,
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            Err("oracle:unsupported-if-condition")
        }
        _ => Err("oracle:unsupported-if-condition"),
    }
}
