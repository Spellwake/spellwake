//! Strictly-typed [`StaticAbility`] parser for `v2`.

use crate::ast::{ControllerRelation, TurnStepAst};
use crate::v2::condition::parse_condition;
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    ConditionExpr, FilterPredicate, ObjectFilter, PlayerRef, PlayerRestrictionKind,
    RuleRestrictionKind, StaticAbility, SubjectRef, ValueExpr, VarName,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Parse one strictly-typed [`StaticAbility`] sentence (fail-closed: zero leftover tokens).
pub fn parse_static_ability(ts: &mut TokenStream<'_>) -> PResult<StaticAbility> {
    // Optional leading `As long as <condition>, ` or `During your turn, `
    let mut condition = if ts.eat_phrase(&["as", "long", "as"]) {
        let c = parse_condition(ts)?;
        if !ts.eat_comma() {
            return Err(ts.err("expected `,` after `As long as <condition>`"));
        }
        Some(c)
    } else if ts.eat_phrase(&["during", "your", "turn"]) && ts.eat_comma() {
        Some(ConditionExpr::IsYourTurn)
    } else {
        None
    };

    // Strip trailing `as long as <condition>` if present!
    let end_trim = if matches!(
        ts.tokens.last().map(|t| &t.kind),
        Some(crate::v2::lexer::TokenKind::Period)
    ) {
        ts.tokens.len() - 1
    } else {
        ts.tokens.len()
    };

    // Strip trailing `, where X is <ValueExpr>` binder
    if let Some(where_idx) = find_where_x_is(ts.tokens, ts.pos, end_trim) {
        let expr_slice = &ts.tokens[where_idx + 4..end_trim];
        let mut expr_ts = ts.sub_stream(expr_slice, ts.stage);
        if let Ok(expr) = parse_value_expr(&mut expr_ts)
            && expr_ts.is_eof()
        {
            let body_slice = &ts.tokens[ts.pos..where_idx];
            let mut body_ts = ts.sub_stream(body_slice, ts.stage);
            let inner = parse_static_core(&mut body_ts, condition.take())?;
            body_ts.expect_eof()?;
            ts.pos = ts.tokens.len();
            return Ok(StaticAbility::BindVariable {
                var: VarName::X,
                expr,
                inner: Box::new(inner),
            });
        }
    }

    if condition.is_none()
        && let Some(ala_idx) = find_as_long_as(ts.tokens, ts.pos, end_trim)
        && ala_idx > ts.pos
    {
        let cond_slice = &ts.tokens[ala_idx + 3..end_trim];
        let mut cond_ts = ts.sub_stream(cond_slice, ts.stage);
        if let Ok(c) = parse_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let body_slice = &ts.tokens[ts.pos..ala_idx];
            let mut body_ts = ts.sub_stream(body_slice, ts.stage);
            let st = parse_static_core(&mut body_ts, Some(c))?;
            body_ts.expect_eof()?;
            ts.pos = ts.tokens.len();
            return Ok(st);
        }
    }

    let st = parse_static_core(ts, condition.take())?;
    let _ = ts.eat_period();
    ts.expect_eof()?;
    Ok(st)
}

fn find_where_x_is(
    tokens: &[crate::v2::lexer::Token<'_>],
    start: usize,
    end: usize,
) -> Option<usize> {
    use crate::v2::lexer::TokenKind;
    if end < start + 5 {
        return None;
    }
    for i in start..=end - 5 {
        if matches!(tokens[i].kind, TokenKind::Comma)
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("where"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x"))
            && matches!(tokens[i + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("is"))
        {
            return Some(i);
        }
    }
    None
}

fn find_as_long_as(
    tokens: &[crate::v2::lexer::Token<'_>],
    start: usize,
    end: usize,
) -> Option<usize> {
    use crate::v2::lexer::TokenKind;
    if end < start + 3 {
        return None;
    }
    for i in start..=end - 3 {
        if matches!(tokens[i].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("as"))
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("long"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("as"))
        {
            return Some(i);
        }
    }
    None
}

fn parse_static_core(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 0. `As an additional cost to cast this spell/~, <cost>`
    if (ts.eat_phrase(&[
        "as",
        "an",
        "additional",
        "cost",
        "to",
        "cast",
        "this",
        "spell",
    ]) || (ts.eat_phrase(&["as", "an", "additional", "cost", "to", "cast"]) && ts.eat_tilde()))
        && ts.eat_comma()
    {
        let costs = parse_cost_list(ts)?;
        return Ok(StaticAbility::AdditionalCost(costs));
    }

    // 0aa0. Class level header (`Level 2`, `Level 3`)
    if ts.eat_word("level")
        && let Some(lvl) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(StaticAbility::ClassLevelHeader(lvl));
    }

    // 0aa. `You control enchanted creature/permanent`
    if ts.eat_phrase(&["you", "control", "enchanted"])
        && (ts.eat_word("creature")
            || ts.eat_word("permanent")
            || ts.eat_word("artifact")
            || ts.eat_word("land"))
    {
        return Ok(StaticAbility::ControlEnchanted {
            controller: PlayerRef::You,
        });
    }

    // 0ab. `Players can't gain life / search libraries / draw more than one card each turn`
    if ts.eat_phrase(&["players", "can't"]) || ts.eat_phrase(&["opponents", "can't"]) {
        let player = PlayerRef::EachPlayer;
        if ts.eat_phrase(&["gain", "life"]) {
            return Ok(StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantGainLife,
                condition,
            });
        }
        if ts.eat_phrase(&["search", "libraries"]) {
            return Ok(StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantSearchLibraries,
                condition,
            });
        }
        if ts.eat_phrase(&["draw", "more", "than", "one", "card", "each", "turn"]) {
            return Ok(StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantDrawMoreThanOne,
                condition,
            });
        }
        if ts.eat_phrase(&["cast", "spells"]) {
            if let Some(filter) = opt(ts, parse_object_filter) {
                return Ok(StaticAbility::CantCastSpellsMatching {
                    player,
                    filter,
                    condition,
                });
            }
            return Ok(StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantCastSpells,
                condition,
            });
        }
    }

    // 0ac. `Prevent all [combat] damage that would be dealt [to <subj>] [by <subj>]`
    if ts.eat_phrase(&["prevent", "all"]) {
        let combat_only = ts.eat_word("combat");
        if ts.eat_word("damage") {
            let _ = ts.eat_phrase(&["that", "would", "be", "dealt"]);
            let mut source = None;
            let mut target = None;
            loop {
                if ts.eat_word("to") {
                    target = Some(parse_subject_ref(ts)?);
                    continue;
                }
                if ts.eat_word("by") {
                    source = Some(parse_subject_ref(ts)?);
                    continue;
                }
                break;
            }
            return Ok(StaticAbility::PreventDamageStatic {
                combat_only,
                source,
                target,
            });
        }
    }

    // 0ad. `This ability costs {M} less/more to activate [for each <expr>]`
    if ts.eat_phrase(&["this", "ability", "costs"]) {
        let delta = ts
            .eat_mana_cost()
            .ok_or_else(|| ts.err("expected mana delta in activation cost modifier"))?;
        let less = if ts.eat_phrase(&["less", "to", "activate"]) {
            true
        } else if ts.eat_phrase(&["more", "to", "activate"]) {
            false
        } else {
            return Err(ts.err("expected `less/more to activate`"));
        };
        let scale = opt(ts, parse_for_each_expr);
        return Ok(StaticAbility::ActivationCostModifier {
            delta,
            less,
            scale,
            condition,
        });
    }

    // 0a. `~ can be your commander` / `~ enters prepared` / `~'s power and toughness are each equal to <val>`
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() {
            if ts.eat_phrase(&["can", "be", "your", "commander"]) {
                return Ok(StaticAbility::CanBeCommander);
            }
            if ts.eat_phrase(&["enters", "prepared"]) {
                return Ok(StaticAbility::EntersPrepared);
            }
            let _ = ts.eat_word("'s");
            if ts.eat_phrase(&["power", "and", "toughness", "are", "each", "equal", "to"]) {
                let val = parse_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val.clone()),
                    toughness: Some(val),
                });
            }
            if ts.eat_phrase(&["power", "is", "equal", "to"]) {
                let val = parse_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val),
                    toughness: None,
                });
            }
            if ts.eat_phrase(&["toughness", "is", "equal", "to"]) {
                let val = parse_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: None,
                    toughness: Some(val),
                });
            }
        }
        ts.restore(cp);
    }

    // 0b. `Skip your <step>`
    if ts.eat_phrase(&["skip", "your"]) {
        let step = if ts.eat_phrase(&["draw", "step"]) {
            TurnStepAst::DrawStep
        } else if ts.eat_word("upkeep") {
            TurnStepAst::Upkeep
        } else if ts.eat_phrase(&["untap", "step"]) {
            TurnStepAst::UntapStep
        } else if ts.eat_phrase(&["end", "step"]) {
            TurnStepAst::EndStep
        } else {
            return Err(ts.err("expected step name after `Skip your`"));
        };
        return Ok(StaticAbility::SkipStep {
            player: PlayerRef::You,
            step,
        });
    }

    // 0b. Trigger-doubling static:
    //     `If <flt> entering causes a triggered ability of <perm_flt> to trigger, that ability triggers an additional time`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(event_filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["entering", "causes", "a", "triggered", "ability", "of"])
            && let Ok(permanent_filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["to", "trigger"])
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
        {
            return Ok(StaticAbility::AdditionalTriggers {
                event_filter,
                permanent_filter,
                extra_count: 1,
            });
        }
        ts.restore(cp);
    }

    // 0c. Player-headed static abilities (`You may look at the top card of ...`,
    //     `You may play lands and cast spells from ...`,
    //     `Each opponent can cast spells only any time they could cast a sorcery`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if player == PlayerRef::You && ts.eat_phrase(&["have", "no", "maximum", "hand", "size"])
            {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::NoMaxHandSize,
                    condition,
                });
            }
            // `You may <cost> rather than pay this spell's/~'s mana cost`
            {
                let alt_cp = ts.checkpoint();
                if player == PlayerRef::You
                    && ts.eat_word("may")
                    && let Ok(costs) = try_parse(ts, parse_cost_list)
                    && ts.eat_phrase(&["rather", "than", "pay"])
                    && (ts.eat_phrase(&["this", "spell's", "mana", "cost"])
                        || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"])))
                {
                    return Ok(StaticAbility::AlternativeCost(costs));
                }
                ts.restore(alt_cp);
            }
            if player == PlayerRef::You
                && ts.eat_phrase(&["may", "exert"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["as", "it", "attacks"])
            {
                return Ok(StaticAbility::MayExertOnAttack);
            }
            if player == PlayerRef::You
                && ts.eat_phrase(&["may", "choose", "not", "to", "untap"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["during", "your", "untap", "step"])
            {
                return Ok(StaticAbility::MayNotUntap {
                    subject: SubjectRef::SelfObject,
                });
            }
            if ts.eat_phrase(&["may", "look", "at", "the", "top", "card", "of"]) {
                let library = parse_zone_ref(ts)?;
                let _ = ts.eat_phrase(&["any", "time"]);
                return Ok(StaticAbility::LookAtTopOfLibrary { player, library });
            }
            if ts.eat_phrase(&["may", "play", "an", "additional", "land"]) {
                let _ = ts.eat_phrase(&["on", "each", "of", "your", "turns"])
                    || ts.eat_phrase(&["each", "turn"]);
                return Ok(StaticAbility::AdditionalLandPlayStatic { player, extra: 1 });
            }
            if ts.eat_phrase(&["may", "play", "lands", "and", "cast", "spells", "from"]) {
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(StaticAbility::PlayFromZone {
                    player,
                    zone,
                    top_only,
                    lands: true,
                    spells: true,
                });
            }
            if ts.eat_phrase(&["may", "play", "lands", "from"]) {
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(StaticAbility::PlayFromZone {
                    player,
                    zone,
                    top_only,
                    lands: true,
                    spells: false,
                });
            }
            if ts.eat_phrase(&["may", "play", "cards", "exiled", "with"]) && ts.eat_tilde() {
                return Ok(StaticAbility::PlayFromZone {
                    player,
                    zone: crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Exile,
                        owner: crate::v2::ir::ZoneOwner::Default,
                    },
                    top_only: false,
                    lands: true,
                    spells: true,
                });
            }
            if ts.eat_phrase(&[
                "can", "cast", "spells", "only", "any", "time", "they", "could", "cast", "a",
                "sorcery",
            ]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CastOnlyAsSorcery,
                    condition,
                });
            }
            if ts.eat_phrase(&["may", "cast"]) {
                // `You may cast ~ from your graveyard` / `You may cast ~ as though it had flash [if ...]`
                let c_cp = ts.checkpoint();
                if ts.eat_tilde() {
                    if ts.eat_word("from") {
                        let zone = parse_zone_ref(ts)?;
                        return Ok(StaticAbility::CastFromZone {
                            player,
                            subject: SubjectRef::SelfObject,
                            zone,
                            condition,
                        });
                    }
                    if ts.eat_phrase(&["as", "though", "it", "had", "flash"]) {
                        return Ok(StaticAbility::CastAsThoughFlash {
                            player,
                            filter: ObjectFilter {
                                any_spell: true,
                                ..Default::default()
                            },
                        });
                    }
                }
                ts.restore(c_cp);
                if let Ok(mut filter) = try_parse(ts, parse_object_filter) {
                    if ts.eat_phrase(&["as", "though", "they", "had", "flash"]) {
                        return Ok(StaticAbility::CastAsThoughFlash { player, filter });
                    }
                    if ts.eat_word("from") {
                        let top_only = ts.eat_phrase(&["the", "top", "of"]);
                        if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                            return Ok(StaticAbility::PlayFromZone {
                                player,
                                zone,
                                top_only,
                                lands: false,
                                spells: true,
                            });
                        }
                    } else if let Some(zone) = filter.zones.pop() {
                        return Ok(StaticAbility::PlayFromZone {
                            player,
                            zone,
                            top_only: false,
                            lands: false,
                            spells: true,
                        });
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 0d. `Damage can't be prevented`
    if ts.eat_phrase(&["damage", "can't", "be", "prevented"]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: None,
            rule: RuleRestrictionKind::DamageCantBePrevented,
            condition,
        });
    }

    // 1. `The next <flt> you cast this turn can't be countered / has <kws> / costs {N} less to cast`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["you", "cast", "this", "turn"])
                || (flt.controller == Some(ControllerRelation::You)
                    && ts.eat_phrase(&["this", "turn"])))
        {
            flt.controller = Some(ControllerRelation::You);
            if ts.eat_phrase(&["can't", "be", "countered"]) {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt,
                    effect: Box::new(StaticAbility::RuleRestriction {
                        subject: None,
                        rule: RuleRestrictionKind::CantBeCountered,
                        condition,
                    }),
                });
            }
            if ts.eat_word("has")
                && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
            {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt.clone(),
                    effect: Box::new(StaticAbility::GrantKeywords {
                        subject: SubjectRef::Group(Box::new(flt)),
                        keywords,
                        condition,
                    }),
                });
            }
            if ts.eat_word("costs")
                && let Some(delta) = ts.eat_mana_cost()
                && (ts.eat_phrase(&["less", "to", "cast"]) || ts.eat_word("less"))
            {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt.clone(),
                    effect: Box::new(StaticAbility::CostModifier {
                        subject: flt,
                        delta,
                        less: true,
                        scale: None,
                        min_mana: None,
                        condition,
                    }),
                });
            }
        }
        ts.restore(cp);
    }

    // 2. Static spell cost modifier:
    //    `[The first ]<filter> [<caster> cast(s)] [each turn] cost(s) {M} more/less to cast [for each <expr>]`
    if let Ok(cm) = try_parse(ts, |s| parse_cost_modifier(s, condition.clone())) {
        return Ok(cm);
    }

    // 3. Subject-headed static abilities (`<subj> gets/get +X/+Y [and has/have <kws>]`,
    //    `<subj> can't be the targets of <flt>`, `<subj> can't attack`, etc.)
    let subject = parse_subject_ref(ts)?;

    // `<subj> gets/get +X/+Y [, has/have <kws>] [and has/have <kws>] [and can't <rule>] [for each <expr>]`
    if ts.eat_word("gets") || ts.eat_word("get") {
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let mut grant_keywords = Vec::new();
        let mut extra_parts = Vec::new();
        {
            let kcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "have"])
                || ts.eat_phrase(&["and", "has"])
                || ts.eat_word("has")
                || ts.eat_word("have")
            {
                if let Some(quoted) = ts.next_quoted() {
                    let compiled = crate::v2::card::compile_face_v2(
                        crate::FaceInput {
                            name: "~",
                            type_line: "Permanent",
                            oracle_text: quoted,
                        },
                        ts.vocab,
                    );
                    if compiled.is_ok() {
                        extra_parts.push(StaticAbility::GrantAbility {
                            subject: subject.clone(),
                            ability: Box::new(compiled),
                            condition: condition.clone(),
                        });
                    } else {
                        ts.restore(kcp);
                    }
                } else if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                    grant_keywords = kws;
                } else {
                    ts.restore(kcp);
                }
            } else {
                ts.restore(kcp);
            }
        }
        loop {
            let rcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "can't"])
                && let Ok(rule) = try_parse(ts, parse_cant_rule)
            {
                extra_parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp);
            let rcp2 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "attacks", "each", "combat", "if", "able"])
                || ts.eat_phrase(&["and", "attack", "each", "combat", "if", "able"])
                || ts.eat_phrase(&["and", "attacks", "each", "turn", "if", "able"])
            {
                extra_parts.push(StaticAbility::MustAttack {
                    subject: subject.clone(),
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp2);
            let rcp3 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "is", "all", "creature", "types"])
                || ts.eat_phrase(&["and", "are", "all", "creature", "types"])
            {
                extra_parts.push(StaticAbility::AllCreatureTypes {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(rcp3);
            let rcp4 = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
                || ts.eat_phrase(&["and", "don't", "untap", "during"]))
                && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
                    || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
                    || ts.eat_phrase(&["your", "untap", "step"]))
            {
                extra_parts.push(StaticAbility::DoesntUntap {
                    subject: subject.clone(),
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp4);
            let rcp5 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&[
                "and",
                "its",
                "activated",
                "abilities",
                "can't",
                "be",
                "activated",
            ]) || ts.eat_phrase(&[
                "and",
                "their",
                "activated",
                "abilities",
                "can't",
                "be",
                "activated",
            ]) {
                extra_parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp5);
            break;
        }
        let scale = opt(ts, parse_for_each_expr).map(Box::new);
        let base = StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale,
            condition,
        };
        if extra_parts.is_empty() {
            return Ok(base);
        }
        let mut all = vec![base];
        all.extend(extra_parts);
        return Ok(StaticAbility::Compound(all));
    }

    // `<subj> isn't a <type>` / `<subj> aren't <types>` (e.g. Theros Gods: `~ isn't a creature`)
    if ts.eat_word("isn't")
        || ts.eat_phrase(&["is", "not"])
        || ts.eat_word("aren't")
        || ts.eat_phrase(&["are", "not"])
    {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let mut types = crate::ast::CardTypeMask::EMPTY;
        while let Some(w) = ts.peek_word() {
            if let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase()) {
                ts.next_word();
                types = types.union(ct);
                let _ = ts.eat_word("or") || ts.eat_word("and");
            } else {
                break;
            }
        }
        if !types.is_empty() {
            return Ok(StaticAbility::RemoveTypes {
                subject,
                types,
                condition,
            });
        }
    }

    // `<subj> can attack as though it/they didn't have defender`
    if ts.eat_phrase(&[
        "can", "attack", "as", "though", "it", "didn't", "have", "defender",
    ]) || ts.eat_phrase(&[
        "can", "attack", "as", "though", "they", "didn't", "have", "defender",
    ]) {
        return Ok(StaticAbility::CanAttackWithDefender { subject, condition });
    }

    // `<subj> are <Subtypes>` (e.g. `Nonbasic lands are Mountains.`)
    if ts.eat_word("are") || ts.eat_word("is") {
        let mut subtypes = Vec::new();
        while let Some(w) = ts.peek_word() {
            if let Some(sub) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase()) {
                ts.next_word();
                subtypes.push(sub.to_string());
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
            } else {
                break;
            }
        }
        if !subtypes.is_empty() {
            let keep_existing = ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
            return Ok(StaticAbility::SetSubtypes {
                subject,
                subtypes,
                keep_existing,
            });
        }
    }

    // `<subj> lose all abilities [and have base power and toughness <P>/<T>]`
    if ts.eat_phrase(&["lose", "all", "abilities"]) || ts.eat_phrase(&["loses", "all", "abilities"])
    {
        let mut parts = vec![StaticAbility::RemoveAllAbilities {
            subject: subject.clone(),
        }];
        if (ts.eat_phrase(&["and", "have", "base", "power", "and", "toughness"])
            || ts.eat_phrase(&["and", "has", "base", "power", "and", "toughness"]))
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            parts.push(StaticAbility::SetBasePT {
                subject,
                power,
                toughness,
            });
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> has/have all activated abilities of <flt>`
    // OR `<subj> has/have base power and toughness <P>/<T> [and are <Subtypes> in addition to their other types]`
    // OR `<subj> has/have "<quoted ability>"` OR `<subj> has/have <kws>`
    if ts.eat_word("has") || ts.eat_word("have") {
        if ts.eat_phrase(&["all", "activated", "abilities", "of"]) {
            let source = parse_object_filter(ts)?;
            return Ok(StaticAbility::CopyActivatedAbilities { subject, source });
        }
        if ts.eat_phrase(&["base", "power", "and", "toughness"])
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let mut parts = vec![StaticAbility::SetBasePT {
                subject: subject.clone(),
                power,
                toughness,
            }];
            if ts.eat_phrase(&["and", "are"]) || ts.eat_phrase(&["and", "is"]) {
                let mut subtypes = Vec::new();
                while let Some(w) = ts.peek_word() {
                    if let Some(sub) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase()) {
                        ts.next_word();
                        subtypes.push(sub.to_string());
                        let _ = ts.eat_comma();
                        let _ = ts.eat_word("and");
                    } else {
                        break;
                    }
                }
                if !subtypes.is_empty() {
                    let keep_existing = ts
                        .eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
                    parts.push(StaticAbility::SetSubtypes {
                        subject,
                        subtypes,
                        keep_existing,
                    });
                }
            }
            return Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                StaticAbility::Compound(parts)
            });
        }
        if let Some(quoted) = ts.next_quoted() {
            let compiled = crate::v2::card::compile_face_v2(
                crate::FaceInput {
                    name: "~",
                    type_line: "Permanent",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !compiled.is_ok() {
                return Err(compiled
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile static granted ability")));
            }
            return Ok(StaticAbility::GrantAbility {
                subject,
                ability: Box::new(compiled),
                condition,
            });
        }
        let (keywords, _) = parse_keyword_list(ts)?;
        if ts.eat_word("and")
            && let Some(quoted) = ts.next_quoted()
        {
            let compiled = crate::v2::card::compile_face_v2(
                crate::FaceInput {
                    name: "~",
                    type_line: "Permanent",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !compiled.is_ok() {
                return Err(compiled
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile static granted ability")));
            }
            return Ok(StaticAbility::Compound(vec![
                StaticAbility::GrantKeywords {
                    subject: subject.clone(),
                    keywords,
                    condition: condition.clone(),
                },
                StaticAbility::GrantAbility {
                    subject,
                    ability: Box::new(compiled),
                    condition,
                },
            ]));
        }
        return Ok(StaticAbility::GrantKeywords {
            subject,
            keywords,
            condition,
        });
    }

    // `<subj> doesn't untap during its controller's untap step`
    if (ts.eat_phrase(&["doesn't", "untap", "during"])
        || ts.eat_phrase(&["don't", "untap", "during"]))
        && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
            || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
            || ts.eat_phrase(&["your", "untap", "step"]))
    {
        return Ok(StaticAbility::DoesntUntap { subject, condition });
    }

    // `<subj> attacks each combat if able`
    if ts.eat_phrase(&["attacks", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attacks", "each", "turn", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "turn", "if", "able"])
    {
        return Ok(StaticAbility::MustAttack { subject, condition });
    }

    // `<subj> can block only <flt>`
    if ts.eat_phrase(&["can", "block", "only"]) {
        let flt = parse_object_filter(ts)?;
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject),
            rule: RuleRestrictionKind::CanBlockOnly(Box::new(flt)),
            condition,
        });
    }

    // `<subj> can block an additional creature each combat`
    if ts.eat_phrase(&[
        "can",
        "block",
        "an",
        "additional",
        "creature",
        "each",
        "combat",
    ]) {
        return Ok(StaticAbility::CanBlockAdditional {
            subject,
            count: 1,
            condition,
        });
    }

    // `<subj> enter(s) tapped` / `<subj> enter(s) with <N> [additional] <kind> counters on it/them`
    if ts.eat_phrase(&["enters", "tapped"]) || ts.eat_phrase(&["enter", "tapped"]) {
        return Ok(StaticAbility::EntersTappedStatic { subject, condition });
    }
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let additional = ts.eat_word("additional");
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
        {
            return Ok(StaticAbility::EntersWithCountersStatic {
                subject,
                kind,
                count,
                additional,
                condition,
            });
        }
    }

    // `<subj> can't ...` (supports compound `and can't ...`)
    if ts.eat_word("can't") {
        let first_rule = parse_cant_rule(ts)?;
        let mut parts = vec![StaticAbility::RuleRestriction {
            subject: Some(subject.clone()),
            rule: first_rule,
            condition: condition.clone(),
        }];
        while ts.eat_phrase(&["and", "can't"]) {
            let next_rule = parse_cant_rule(ts)?;
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: next_rule,
                condition: condition.clone(),
            });
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    Err(ts.err("expected static ability predicate"))
}

fn parse_cant_rule(ts: &mut TokenStream<'_>) -> PResult<RuleRestrictionKind> {
    if ts.eat_phrase(&["be", "the", "targets", "of"])
        || ts.eat_phrase(&["be", "the", "target", "of"])
    {
        let src = parse_object_filter(ts)?;
        return Ok(RuleRestrictionKind::CantBeTargetedBy(Box::new(src)));
    }
    if ts.eat_phrase(&["attack", "or", "block"]) {
        if ts.eat_word("unless") {
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackOrBlockUnless(Box::new(cond)));
        }
        return Ok(RuleRestrictionKind::CantAttackOrBlock);
    }
    if ts.eat_word("attack") {
        if ts.eat_phrase(&["unless", "defending", "player", "controls"]) {
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantAttackUnlessDefendingControls(
                Box::new(flt),
            ));
        }
        if ts.eat_word("unless") {
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackUnless(Box::new(cond)));
        }
        return Ok(RuleRestrictionKind::CantAttack);
    }
    if ts.eat_word("block") {
        if ts.eat_word("unless") {
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantBlockUnless(Box::new(cond)));
        }
        return Ok(RuleRestrictionKind::CantBlock);
    }
    if ts.eat_phrase(&["be", "blocked"]) {
        if ts.eat_phrase(&["by", "more", "than", "one", "creature"]) {
            return Ok(RuleRestrictionKind::CantBeBlockedByMoreThan(1));
        }
        if ts.eat_phrase(&["except", "by"]) {
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)));
        }
        if ts.eat_word("by") {
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)));
        }
        return Ok(RuleRestrictionKind::CantBeBlocked);
    }
    if ts.eat_phrase(&["be", "countered"]) {
        return Ok(RuleRestrictionKind::CantBeCountered);
    }
    Err(ts.err("expected restriction after `can't`"))
}

fn parse_cost_modifier(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let mut nth_spell: Option<u8> = None;
    if ts.eat_phrase(&["the", "first"]) {
        nth_spell = Some(1);
    } else if ts.eat_phrase(&["the", "second"]) {
        nth_spell = Some(2);
    }

    let mut flt: ObjectFilter = if ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]) {
        ObjectFilter {
            any_spell: true,
            ..Default::default()
        }
    } else {
        parse_object_filter(ts)?
    };

    // Preserve caster relation explicitly!
    if ts.eat_phrase(&["you", "cast"]) {
        flt.controller = Some(ControllerRelation::You);
    } else if ts.eat_phrase(&["your", "opponents", "cast"]) || ts.eat_phrase(&["opponents", "cast"])
    {
        flt.controller = Some(ControllerRelation::Opponents);
    } else if ts.eat_phrase(&["an", "opponent", "casts"]) {
        flt.controller = Some(ControllerRelation::Opponent);
    } else if ts.eat_phrase(&["players", "cast"]) {
        // Any player
    }
    let _ = ts.eat_phrase(&["each", "turn"]);

    if let Some(n) = nth_spell {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(n));
    }

    if !(ts.eat_word("cost") || ts.eat_word("costs")) {
        return Err(ts.err("expected `cost` or `costs` in cost modifier"));
    }

    let delta = ts
        .eat_mana_cost()
        .ok_or_else(|| ts.err("expected mana delta in cost modifier"))?;

    let less = if ts.eat_phrase(&["less", "to", "cast"]) {
        true
    } else if ts.eat_phrase(&["more", "to", "cast"]) {
        false
    } else {
        return Err(ts.err("expected `less to cast` or `more to cast`"));
    };

    let scale = opt(ts, parse_for_each_expr);
    let final_cond = if ts.eat_word("if") {
        let c = parse_condition(ts)?;
        Some(match condition {
            Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
            None => c,
        })
    } else {
        condition
    };

    Ok(StaticAbility::CostModifier {
        subject: flt,
        delta,
        less,
        scale,
        min_mana: None,
        condition: final_cond,
    })
}
