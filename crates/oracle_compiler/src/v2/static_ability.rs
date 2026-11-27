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
    // Optional leading `As long as <condition>, ` or `If <condition>, ` or `During your turn, `
    let mut condition = if ts.eat_phrase(&["as", "long", "as"]) {
        let c = parse_condition(ts)?;
        if !ts.eat_comma() {
            return Err(ts.err("expected `,` after `As long as <condition>`"));
        }
        Some(c)
    } else if ts.eat_phrase(&["during", "your", "turn"]) && ts.eat_comma() {
        Some(ConditionExpr::IsYourTurn)
    } else if ts.eat_phrase(&["during", "turns", "other", "than", "yours"]) && ts.eat_comma() {
        Some(ConditionExpr::Not(Box::new(ConditionExpr::IsYourTurn)))
    } else {
        let icp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(c) = try_parse(ts, parse_condition)
            && ts.eat_comma()
        {
            Some(c)
        } else {
            ts.restore(icp);
            None
        }
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

    // Strip trailing `during your turn` (`Kutzil`, `Grand Abolisher`)
    if condition.is_none()
        && end_trim >= ts.pos + 3
        && matches!(ts.tokens[end_trim - 3].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("during"))
        && matches!(ts.tokens[end_trim - 2].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("your"))
        && matches!(ts.tokens[end_trim - 1].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("turn"))
    {
        let body_slice = &ts.tokens[ts.pos..end_trim - 3];
        let mut body_ts = ts.sub_stream(body_slice, ts.stage);
        if let Ok(st) = parse_static_core(&mut body_ts, Some(ConditionExpr::IsYourTurn))
            && body_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(st);
        }
    }

    // Strip trailing `if <cond>` on static abilities (`Mindbender Spores`, `Veldt`)
    if condition.is_none()
        && let Some(if_idx) = (ts.pos + 1..end_trim)
            .find(|&idx| matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("if")))
    {
        let cond_slice = &ts.tokens[if_idx + 1..end_trim];
        let mut cond_ts = ts.sub_stream(cond_slice, ts.stage);
        if let Ok(c) = parse_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let body_slice = &ts.tokens[ts.pos..if_idx];
            let mut body_ts = ts.sub_stream(body_slice, ts.stage);
            if let Ok(st) = parse_static_core(&mut body_ts, Some(c))
                && body_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(st);
            }
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
    // 0. `As an additional cost to cast this spell/~, [you may] <cost>`
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
        let _ = ts.eat_phrase(&["you", "may"]);
        let costs = parse_cost_list(ts)?;
        return Ok(StaticAbility::AdditionalCost(costs));
    }

    // 0aa00. Bare Level Up base P/T row (`3/3`, `6/6`)
    if let Some((p, t)) = ts.next_base_pt() {
        return Ok(StaticAbility::LevelBasePT(p, t));
    }

    // 0aa0. Class level header (`Level 2`) or Level Up band (`LEVEL 1-2`, `LEVEL 3+`)
    if ts.eat_word("level")
        && let Some(lvl) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        if matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Plus)
        ) {
            ts.next_tok();
            return Ok(StaticAbility::LevelBand {
                min: lvl,
                max: None,
            });
        }
        if matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Minus)
        ) {
            ts.next_tok();
            if let Some(hi) = ts.next_number(false).and_then(|n| u8::try_from(n).ok()) {
                return Ok(StaticAbility::LevelBand {
                    min: lvl,
                    max: Some(hi),
                });
            }
        }
        return Ok(StaticAbility::ClassLevelHeader(lvl));
    }

    // 0aa1. Deckbuilding / Conspiracy / Reveal-top / Max-hand / Once-per-turn statics
    if ts.eat_phrase(&[
        "play", "with", "the", "top", "card", "of", "your", "library", "revealed",
    ]) {
        return Ok(StaticAbility::PlayWithTopRevealed {
            player: PlayerRef::You,
        });
    }
    if ts.eat_phrase(&[
        "a", "deck", "can", "have", "any", "number", "of", "cards", "named",
    ]) && ts.eat_tilde()
    {
        return Ok(StaticAbility::AnyNumberInDeck);
    }
    if ts.eat_word("draft") && ts.eat_tilde() && ts.eat_phrase(&["face", "up"]) {
        return Ok(StaticAbility::DraftFaceUp);
    }
    if ts.eat_phrase(&["your", "maximum", "hand", "size", "is", "equal", "to"]) {
        let value = parse_value_expr(ts)?;
        return Ok(StaticAbility::MaxHandSize {
            player: PlayerRef::You,
            value,
        });
    }
    if ts.eat_phrase(&["your", "maximum", "hand", "size", "is", "increased", "by"])
        && let Some(n) = ts.next_number(false)
    {
        return Ok(StaticAbility::MaxHandSizeDelta {
            player: PlayerRef::You,
            delta: i16::try_from(n).unwrap_or(1),
        });
    }
    if ts.eat_phrase(&["your", "maximum", "hand", "size", "is", "reduced", "by"])
        && let Some(n) = ts.next_number(false)
    {
        return Ok(StaticAbility::MaxHandSizeDelta {
            player: PlayerRef::You,
            delta: -i16::try_from(n).unwrap_or(1),
        });
    }
    if ts.eat_phrase(&["your", "maximum", "hand", "size", "is"]) {
        let value = parse_value_expr(ts)?;
        return Ok(StaticAbility::MaxHandSize {
            player: PlayerRef::You,
            value,
        });
    }
    if ts.eat_phrase(&["players", "have", "no", "maximum", "hand", "size"])
        || ts.eat_phrase(&["each", "player", "has", "no", "maximum", "hand", "size"])
    {
        return Ok(StaticAbility::PlayerRestriction {
            player: PlayerRef::EachPlayer,
            restriction: PlayerRestrictionKind::NoMaxHandSize,
            condition,
        });
    }
    // `No more than <N> creature(s) can attack [you/~] each combat` (`Silent Arbiter`, `Crawlspace`, `The Eternal Wanderer`)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["no", "more", "than"])
            && let Some(n) = ts.next_number(false)
            && (ts.eat_word("creature") || ts.eat_word("creatures"))
            && ts.eat_phrase(&["can", "attack"])
        {
            let target = if ts.peek_phrase(&["each", "combat"]) {
                None
            } else {
                opt(ts, parse_subject_ref)
            };
            if ts.eat_phrase(&["each", "combat"]) {
                return Ok(StaticAbility::MaxAttackersPerCombat {
                    max: u8::try_from(n.max(0)).unwrap_or(1),
                    target,
                });
            }
        }
        ts.restore(ncp);
    }
    // `Untap ~ during each other player's untap step` (`Urban Burgeoning`, `Seedborn Muse`)
    {
        let ucp = ts.checkpoint();
        if ts.eat_word("untap")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["during", "each", "other", "player's", "untap", "step"])
        {
            return Ok(StaticAbility::UntapDuringEachOtherUntap { subject });
        }
        ts.restore(ucp);
    }
    // `If a creature you control attacks, ~ also attacks if able` (`Ekundu Cyclops`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(trigger_filter) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("attacks") || ts.eat_word("attack"))
            && ts.eat_comma()
            && ts.eat_tilde()
            && ts.eat_phrase(&["also", "attacks", "if", "able"])
        {
            return Ok(StaticAbility::AlsoAttacksIfAble { trigger_filter });
        }
        ts.restore(ecp);
    }
    // `You don't lose the game for having 0 or less life` (`Lich`)
    if ts.eat_phrase(&[
        "you", "don't", "lose", "the", "game", "for", "having", "0", "or", "less", "life",
    ]) {
        return Ok(StaticAbility::DontLoseForZeroLife);
    }
    // `You don't lose unspent <color> mana as steps and phases end` (`Fangorn`)
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "don't", "lose", "unspent"])
            && let Some(w) = ts.peek_word()
            && let Some(_c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&["mana", "as", "steps", "and", "phases", "end"]) {
                return Ok(StaticAbility::CostReductionRule);
            }
        }
        ts.restore(fcp);
    }
    // `Cast ~ only if <cond>` (`Talara's Battalion`)
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("cast")
            && ts.eat_tilde()
            && ts.eat_phrase(&["only", "if"])
            && let Ok(cond) = try_parse(ts, parse_condition)
        {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::SelfObject),
                rule: RuleRestrictionKind::CantBeCast,
                condition: Some(ConditionExpr::Not(Box::new(cond))),
            });
        }
        ts.restore(ccp);
    }
    // `<flt> entering don't cause abilities to trigger` (`Doorkeeper Thrull`, `Torpor Orb`)
    {
        let dcp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["entering", "don't", "cause", "abilities", "to", "trigger"])
        {
            return Ok(StaticAbility::EntersDontTrigger { filter });
        }
        ts.restore(dcp);
    }
    // `Double all damage that <flt> would deal` (`Raphael`, `Gratuitous Violence`)
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["double", "all", "damage", "that"])
            && let Ok(source) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["would", "deal"])
        {
            return Ok(StaticAbility::DoubleDamageStatic { source });
        }
        ts.restore(dcp);
    }
    // `Equip abilities you activate [that target <subj>] cost {M} less/more to activate` (`Strong Back`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["equip", "abilities", "you", "activate"]) {
            if ts.eat_phrase(&["that", "target"]) {
                let _ = opt(ts, parse_subject_ref);
            }
            if (ts.eat_word("cost") || ts.eat_word("costs"))
                && let Some(delta) = ts.eat_mana_cost()
            {
                let less = if ts.eat_phrase(&["less", "to", "activate"]) {
                    Some(true)
                } else if ts.eat_phrase(&["more", "to", "activate"]) {
                    Some(false)
                } else {
                    None
                };
                if let Some(less) = less {
                    return Ok(StaticAbility::ActivationCostModifier {
                        delta,
                        less,
                        scale: None,
                        condition,
                    });
                }
            }
        }
        ts.restore(ecp);
    }
    if ts.eat_phrase(&["all", "creatures", "able", "to", "block"])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
        && ts.eat_phrase(&["do", "so"])
    {
        return Ok(StaticAbility::AllMustBlock { subject });
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "the", "first", "spell", "each", "player", "casts", "during", "each", "of", "their",
            "turns", "may", "be", "cast", "without", "paying", "its", "mana", "cost",
        ]) {
            return Ok(StaticAbility::FirstSpellFree {
                player: PlayerRef::EachPlayer,
            });
        }
        ts.restore(cp);
    }
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["once", "during", "each", "of", "your", "turns"])
            || ts.eat_phrase(&["once", "each", "turn"]))
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may", "cast"])
            && let Ok(mut filter) = try_parse(ts, parse_object_filter)
        {
            let zone = if ts.eat_word("from") {
                opt(ts, parse_zone_ref)
            } else {
                filter.zones.pop()
            };
            let free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
            let alt_cost = if !free && ts.eat_word("by") {
                opt(ts, parse_cost_list)
            } else {
                None
            };
            let _ = ts.eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"]);
            return Ok(StaticAbility::OncePerTurnCast {
                player: PlayerRef::You,
                filter,
                zone,
                free,
                alt_cost,
            });
        }
        ts.restore(cp);
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

    // 0ab. `Players can't gain life / search libraries / draw more than one card each turn / cast more than one spell each turn / untap more than N permanents`
    if ts.eat_phrase(&[
        "the",
        "same",
        "is",
        "true",
        "for",
        "creature",
        "spells",
        "you",
        "control",
        "and",
        "creature",
        "cards",
        "you",
        "own",
        "that",
        "aren't",
        "on",
        "the",
        "battlefield",
    ]) || ts.eat_phrase(&[
        "the",
        "same",
        "is",
        "true",
        "for",
        "creature",
        "spells",
        "you",
        "control",
        "and",
        "creature",
        "cards",
        "you",
        "own",
        "that",
        "are",
        "not",
        "on",
        "the",
        "battlefield",
    ]) {
        return Ok(StaticAbility::AppliesInAllZones);
    }
    if ts.eat_phrase(&["this", "effect", "doesn't", "remove"]) && ts.eat_tilde() {
        return Ok(StaticAbility::RetainAttachment);
    }
    if ts.eat_phrase(&[
        "this", "effect", "reduces", "only", "the", "amount", "of", "colored", "mana", "you", "pay",
    ]) || ts.eat_phrase(&[
        "this", "effect", "can't", "reduce", "the", "mana", "in", "that", "cost", "to", "less",
        "than", "one", "mana",
    ]) {
        return Ok(StaticAbility::CostReductionRule);
    }
    if ts.eat_phrase(&["players", "can't"])
        || ts.eat_phrase(&["your", "opponents", "can't"])
        || ts.eat_phrase(&["each", "opponent", "can't"])
        || ts.eat_phrase(&["opponents", "can't"])
        || ts.eat_phrase(&["each", "player", "can't"])
    {
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
        if ts.eat_phrase(&["cast", "more", "than", "one", "spell", "each", "turn"]) {
            return Ok(StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
                condition,
            });
        }
        if ts.eat_phrase(&["untap", "more", "than"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["permanents", "during", "their", "untap", "steps"])
        {
            return Ok(StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantUntapMoreThan(u8::try_from(n).unwrap_or(2)),
                condition,
            });
        }
        if ts.eat_phrase(&["cast", "spells"]) {
            if ts.eat_phrase(&["with", "the", "chosen", "name"]) {
                let mut filter = ObjectFilter {
                    any_spell: true,
                    ..Default::default()
                };
                filter.predicates.push(FilterPredicate::OfChosenName);
                return Ok(StaticAbility::CantCastSpellsMatching {
                    player,
                    filter,
                    condition,
                });
            }
            if ts.eat_phrase(&["with", "the", "same", "name", "as"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                let mut filter = ObjectFilter {
                    any_spell: true,
                    ..Default::default()
                };
                filter.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: crate::v2::ir::SharedProperty::Name,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                return Ok(StaticAbility::CantCastSpellsMatching {
                    player,
                    filter,
                    condition,
                });
            }
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

    // 0ac. `Prevent all [combat|noncombat] damage that would be dealt [to <subj>] [by <subj>]`
    if ts.eat_phrase(&["prevent", "all"]) {
        let combat_only = ts.eat_word("combat");
        let _ = ts.eat_word("noncombat");
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
            if ts.eat_phrase(&[
                "saddles", "mounts", "and", "crews", "vehicles", "as", "though", "its", "power",
                "were",
            ]) && let Some(n) = ts.next_number(false)
                && ts.eat_word("greater")
            {
                return Ok(StaticAbility::CrewPowerBonus(n));
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
                let toughness = if ts.eat_phrase(&["and", "its", "toughness", "is", "equal", "to"])
                {
                    if ts.eat_phrase(&["that", "number", "plus"])
                        && let Some(n) = ts.next_number(false)
                    {
                        Some(ValueExpr::Add(
                            Box::new(val.clone()),
                            Box::new(ValueExpr::Const(n)),
                        ))
                    } else {
                        Some(parse_value_expr(ts)?)
                    }
                } else {
                    None
                };
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val),
                    toughness,
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
    //     OR `If a triggered ability of <flt> triggers, that ability triggers an additional time`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["if", "a", "triggered", "ability", "of"])
            && let Ok(source_filter) = try_parse(ts, parse_object_filter)
            && ts.eat_word("triggers")
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
        {
            return Ok(StaticAbility::AdditionalTriggerOnAbility {
                source_filter,
                extra_count: 1,
            });
        }
        ts.restore(cp);
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
            if player == PlayerRef::You && ts.eat_phrase(&["can't", "lose", "the", "game"]) {
                let first = StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantLoseGame,
                    condition: condition.clone(),
                };
                if ts.eat_phrase(&["and", "your", "opponents", "can't", "win", "the", "game"]) {
                    return Ok(StaticAbility::Compound(vec![
                        first,
                        StaticAbility::PlayerRestriction {
                            player: PlayerRef::EachOpponent,
                            restriction: PlayerRestrictionKind::CantWinGame,
                            condition,
                        },
                    ]));
                }
                return Ok(first);
            }
            if player == PlayerRef::You && ts.eat_phrase(&["have", "no", "maximum", "hand", "size"])
            {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::NoMaxHandSize,
                    condition,
                });
            }
            // `You may <cost> rather than pay this spell's/~'s mana cost` OR `... rather than pay the mana cost for <flt>`
            {
                let alt_cp = ts.checkpoint();
                if player == PlayerRef::You
                    && ts.eat_word("may")
                    && let Ok(costs) = try_parse(ts, parse_cost_list)
                    && ts.eat_phrase(&["rather", "than", "pay"])
                {
                    if ts.eat_phrase(&["this", "spell's", "mana", "cost"])
                        || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"]))
                    {
                        return Ok(StaticAbility::AlternativeCost(costs));
                    }
                    if ts.eat_phrase(&["the", "mana", "cost", "for"])
                        && let Ok(filter) = try_parse(ts, parse_object_filter)
                    {
                        return Ok(StaticAbility::AlternativeCostForFilter {
                            cost: costs,
                            filter,
                        });
                    }
                }
                ts.restore(alt_cp);
            }
            if player == PlayerRef::You
                && ts.eat_phrase(&["may", "have"])
                && ts.eat_tilde()
                && ts.eat_phrase(&[
                    "assign", "its", "combat", "damage", "as", "though", "it", "weren't", "blocked",
                ])
            {
                return Ok(StaticAbility::AssignDamageAsUnblocked {
                    subject: SubjectRef::SelfObject,
                });
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
                let first = StaticAbility::LookAtTopOfLibrary { player, library };
                let ccp = ts.checkpoint();
                if ts.eat_comma() && ts.eat_word("and") {
                    if let Ok(second) = try_parse(ts, |t| parse_static_core(t, condition.clone())) {
                        return Ok(StaticAbility::Compound(vec![first, second]));
                    }
                    ts.restore(ccp);
                }
                return Ok(first);
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
            {
                let plcp = ts.checkpoint();
                if ts.eat_phrase(&["may", "play", "lands", "and", "cast"])
                    && let Ok(_flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_word("from")
                {
                    let top_only = ts.eat_phrase(&["the", "top", "of"]);
                    if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                        return Ok(StaticAbility::PlayFromZone {
                            player,
                            zone,
                            top_only,
                            lands: true,
                            spells: true,
                        });
                    }
                }
                ts.restore(plcp);
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
            if ts.eat_phrase(&["may", "play", "cards", "from"]) {
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
                    if ts.eat_phrase(&["without", "paying", "its", "mana", "cost"]) {
                        return Ok(StaticAbility::CastWithoutPaying {
                            player,
                            subject: SubjectRef::SelfObject,
                            condition,
                        });
                    }
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
                        if ts.eat_phrase(&["if", "you", "pay"])
                            && let Some(mc) = ts.eat_mana_cost()
                            && ts.eat_phrase(&["more", "to", "cast", "it"])
                        {
                            return Ok(StaticAbility::CastAsThoughFlashWithExtraCost {
                                player,
                                filter: ObjectFilter {
                                    any_spell: true,
                                    ..Default::default()
                                },
                                extra_cost: crate::v2::ir::Cost::Mana(mc),
                            });
                        }
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
                    if ts.eat_phrase(&["without", "paying", "their", "mana", "costs"])
                        || ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                    {
                        return Ok(StaticAbility::CastWithoutPaying {
                            player,
                            subject: SubjectRef::Group(Box::new(filter)),
                            condition,
                        });
                    }
                    if ts.eat_phrase(&["from", "among", "cards", "exiled", "with"])
                        && ts.eat_tilde()
                    {
                        return Ok(StaticAbility::PlayFromZone {
                            player,
                            zone: crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Exile,
                                owner: crate::v2::ir::ZoneOwner::Default,
                            },
                            top_only: false,
                            lands: false,
                            spells: true,
                        });
                    }
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
            let rcp6 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "is", "goaded"]) || ts.eat_phrase(&["and", "are", "goaded"]) {
                extra_parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::AreGoaded,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp6);
            let rcp7 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "is", "a"])
                || ts.eat_phrase(&["and", "is", "an"])
                || ts.eat_phrase(&["and", "are"])
            {
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
                        .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                    extra_parts.push(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes,
                        keep_existing,
                    });
                    continue;
                }
            }
            ts.restore(rcp7);
            let rcp8 = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "has"]) || ts.eat_phrase(&["and", "have"]))
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
                if compiled.is_ok() {
                    extra_parts.push(StaticAbility::GrantAbility {
                        subject: subject.clone(),
                        ability: Box::new(compiled),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(rcp8);
            let rcp9 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "loses", "all", "abilities"])
                || ts.eat_phrase(&["and", "lose", "all", "abilities"])
            {
                extra_parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(rcp9);
            let rcp9b = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "loses"]) || ts.eat_phrase(&["and", "lose"]))
                && let Ok(_kws) = try_parse(ts, parse_keyword_list)
            {
                extra_parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(rcp9b);
            let rcp10 = ts.checkpoint();
            let _ = ts.eat_comma();
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
                if compiled.is_ok() {
                    extra_parts.push(StaticAbility::GrantAbility {
                        subject: subject.clone(),
                        ability: Box::new(compiled),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(rcp10);
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

    // `<subj> assigns combat damage equal to its toughness rather than its power`
    if ts.eat_phrase(&[
        "assigns",
        "combat",
        "damage",
        "equal",
        "to",
        "its",
        "toughness",
        "rather",
        "than",
        "its",
        "power",
    ]) || ts.eat_phrase(&[
        "assign",
        "combat",
        "damage",
        "equal",
        "to",
        "their",
        "toughness",
        "rather",
        "than",
        "their",
        "power",
    ]) {
        return Ok(StaticAbility::AssignDamageByToughness { subject, condition });
    }

    // `<subj> can attack as though it/they didn't have defender`
    if ts.eat_phrase(&[
        "can", "attack", "as", "though", "it", "didn't", "have", "defender",
    ]) || ts.eat_phrase(&[
        "can", "attack", "as", "though", "they", "didn't", "have", "defender",
    ]) {
        return Ok(StaticAbility::CanAttackWithDefender { subject, condition });
    }

    // `<subj> is/are the chosen type in addition to its/their other types` (`Metallic Mimic`)
    {
        let mcp = ts.checkpoint();
        if (ts.eat_word("is") || ts.eat_word("are"))
            && ts.eat_phrase(&["the", "chosen", "type", "in", "addition", "to"])
            && (ts.eat_phrase(&["its", "other", "types"])
                || ts.eat_phrase(&["their", "other", "types"]))
        {
            return Ok(StaticAbility::AdditionalTypesChosen { subject });
        }
        ts.restore(mcp);
    }

    // `<subj> is a [color] [subtypes] creature [with base power and toughness P/T] [in addition to its other types]` (`Ensoul Artifact`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["is", "a"]) || ts.eat_phrase(&["is", "an"]) || ts.eat_word("are") {
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if crate::vocab::lookup_color(&low).is_some() || low == "and" {
                    ts.next_word();
                } else if crate::vocab::lookup_card_type(&low).is_some() {
                    ts.next_word();
                    saw_type = true;
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else {
                    break;
                }
            }
            if saw_type
                && ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                && let Some((power, toughness)) = ts.next_base_pt()
            {
                let _ = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                let mut parts = vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness,
                }];
                if !subtypes.is_empty() {
                    parts.push(StaticAbility::SetSubtypes {
                        subject,
                        subtypes,
                        keep_existing: true,
                    });
                }
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        ts.restore(ecp);
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

    // `<subj> lose all abilities [and have base power and toughness <P>/<T> | and is a <color> <subtype> creature with base power and toughness <P>/<T> | and doesn't untap ...]`
    if ts.eat_phrase(&["lose", "all", "abilities"]) || ts.eat_phrase(&["loses", "all", "abilities"])
    {
        let mut parts = vec![StaticAbility::RemoveAllAbilities {
            subject: subject.clone(),
        }];
        loop {
            let lcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "have", "base", "power", "and", "toughness"])
                || ts.eat_phrase(&["and", "has", "base", "power", "and", "toughness"]))
                && let Some((power, toughness)) = ts.next_base_pt()
            {
                parts.push(StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness,
                });
                continue;
            }
            ts.restore(lcp);
            let lcp2 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "is", "a"])
                || ts.eat_phrase(&["and", "is", "an"])
                || ts.eat_phrase(&["and", "are"])
            {
                let mut subtypes = Vec::new();
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if crate::vocab::lookup_color(&low).is_some()
                        || crate::vocab::lookup_card_type(&low).is_some()
                        || low == "and"
                    {
                        ts.next_word();
                    } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                        ts.next_word();
                        subtypes.push(sub.to_string());
                    } else {
                        break;
                    }
                }
                if !subtypes.is_empty() {
                    parts.push(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes,
                        keep_existing: false,
                    });
                }
                if ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                    && let Some((power, toughness)) = ts.next_base_pt()
                {
                    parts.push(StaticAbility::SetBasePT {
                        subject: subject.clone(),
                        power,
                        toughness,
                    });
                }
                if parts.len() > 1 {
                    continue;
                }
            }
            ts.restore(lcp2);
            let lcp3 = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
                || ts.eat_phrase(&["and", "don't", "untap", "during"]))
                && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
                    || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
                    || ts.eat_phrase(&["your", "untap", "step"]))
            {
                parts.push(StaticAbility::DoesntUntap {
                    subject: subject.clone(),
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(lcp3);
            break;
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
            let first = StaticAbility::GrantAbility {
                subject: subject.clone(),
                ability: Box::new(compiled),
                condition: condition.clone(),
            };
            if ts.eat_word("and")
                && let Some(q2) = ts.next_quoted()
            {
                let c2 = crate::v2::card::compile_face_v2(
                    crate::FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: q2,
                    },
                    ts.vocab,
                );
                if c2.is_ok() {
                    return Ok(StaticAbility::Compound(vec![
                        first,
                        StaticAbility::GrantAbility {
                            subject,
                            ability: Box::new(c2),
                            condition,
                        },
                    ]));
                }
            }
            return Ok(first);
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

    // `<subj> attacks each combat if able [unless <cond>]`
    if ts.eat_phrase(&["attacks", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attacks", "each", "turn", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "turn", "if", "able"])
    {
        let final_cond = if ts.eat_word("unless") {
            let u = ConditionExpr::Not(Box::new(parse_condition(ts)?));
            Some(match condition {
                Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(u)),
                None => u,
            })
        } else {
            condition
        };
        return Ok(StaticAbility::MustAttack {
            subject,
            condition: final_cond,
        });
    }

    // `<subj> can be blocked as though they/it didn't have <kw>`
    if (ts.eat_phrase(&[
        "can", "be", "blocked", "as", "though", "they", "didn't", "have",
    ]) || ts.eat_phrase(&[
        "can", "be", "blocked", "as", "though", "it", "didn't", "have",
    ])) && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
    {
        return Ok(StaticAbility::IgnoreKeywordForBlocking {
            subject,
            keyword: kw,
        });
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

    // `<subj> can block any number of creatures`
    if ts.eat_phrase(&["can", "block", "any", "number", "of", "creatures"]) {
        return Ok(StaticAbility::CanBlockAnyNumber { subject });
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

    // `<subj> enter(s) tapped / untapped / under the control of an opponent`
    if ts.eat_phrase(&["enters", "untapped"]) || ts.eat_phrase(&["enter", "untapped"]) {
        return Ok(StaticAbility::EntersUntappedStatic { subject, condition });
    }
    if ts.eat_phrase(&[
        "enters", "under", "the", "control", "of", "an", "opponent", "of", "your", "choice",
    ]) {
        return Ok(StaticAbility::EntersUnderOpponentControl);
    }
    if ts.eat_phrase(&["enters", "tapped"]) || ts.eat_phrase(&["enter", "tapped"]) {
        return Ok(StaticAbility::EntersTappedStatic { subject, condition });
    }
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else if ts.eat_word("a") || ts.eat_word("an") {
            Some(ValueExpr::Const(1))
        } else {
            Some(try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1)))
        };
        let additional = ts.eat_word("additional");
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
        {
            let count = if let Some(c) = count_opt {
                if let Some(scale) = opt(ts, parse_for_each_expr) {
                    ValueExpr::Mul(Box::new(c), Box::new(scale))
                } else {
                    c
                }
            } else if ts.eat_phrase(&["equal", "to"]) {
                parse_value_expr(ts)?
            } else {
                return Err(ts.err("expected `equal to <val>` after `a number of counters`"));
            };
            let final_cond = if ts.eat_word("if") {
                let c = parse_condition(ts)?;
                Some(match condition {
                    Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                    None => c,
                })
            } else {
                condition
            };
            return Ok(StaticAbility::EntersWithCountersStatic {
                subject,
                kind,
                count,
                additional,
                condition: final_cond,
            });
        }
    }

    // `<subj> must be blocked [if able | by all creatures able to block it]`
    if ts.eat_phrase(&["must", "be", "blocked", "if", "able"])
        || ts.eat_phrase(&[
            "must",
            "be",
            "blocked",
            "by",
            "all",
            "creatures",
            "able",
            "to",
            "block",
            "it",
        ])
    {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject),
            rule: RuleRestrictionKind::MustBeBlocked,
            condition,
        });
    }

    // `<subj> can't attack you [or planeswalkers you control] unless their controller pays {M} [for each of those creatures]` (Ghostly Prison / Archon of Absolution)
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["can't", "attack"])
            && let Ok(target) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("unless")
            && (ts.eat_phrase(&["their", "controller", "pays"])
                || ts.eat_phrase(&["its", "controller", "pays"]))
            && let Some(mc) = ts.eat_mana_cost()
        {
            let per_creature = ts.eat_phrase(&["for", "each", "of", "those", "creatures"])
                || ts.eat_phrase(&[
                    "for",
                    "each",
                    "creature",
                    "they",
                    "control",
                    "that's",
                    "attacking",
                    "you",
                ]);
            return Ok(StaticAbility::AttackTax {
                target,
                cost: crate::v2::ir::Cost::Mana(mc),
                per_creature,
            });
        }
        ts.restore(acp);
    }

    // `<subj> can't ...` (supports compound `and can't ...` and `, and its activated abilities can't be activated`)
    if ts.eat_word("can't") {
        let first_rule = parse_cant_rule(ts)?;
        let mut parts = vec![StaticAbility::RuleRestriction {
            subject: Some(subject.clone()),
            rule: first_rule,
            condition: condition.clone(),
        }];
        loop {
            if ts.eat_phrase(&["and", "can't"]) {
                let next_rule = parse_cant_rule(ts)?;
                parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: next_rule,
                    condition: condition.clone(),
                });
                continue;
            }
            let acp = ts.checkpoint();
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
                parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(acp);
            break;
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
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantAttackOrBlockAlone);
        }
        if ts.eat_word("unless") {
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackOrBlockUnless(Box::new(cond)));
        }
        return Ok(RuleRestrictionKind::CantAttackOrBlock);
    }
    if ts.eat_word("attack") {
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantAttackAlone);
        }
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
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("you"))
            && let Ok(tgt) = try_parse(ts, parse_subject_ref)
        {
            return Ok(RuleRestrictionKind::CantAttackTarget(Box::new(tgt)));
        }
        return Ok(RuleRestrictionKind::CantAttack);
    }
    if ts.eat_word("block") {
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantBlockAlone);
        }
        if ts.eat_word("unless") {
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantBlockUnless(Box::new(cond)));
        }
        if let Ok(blocked_subj) = try_parse(ts, parse_subject_ref) {
            return Ok(RuleRestrictionKind::CantBlockSubject(Box::new(
                blocked_subj,
            )));
        }
        return Ok(RuleRestrictionKind::CantBlock);
    }
    if ts.eat_phrase(&["be", "blocked"]) {
        if ts.eat_phrase(&["by", "more", "than", "one", "creature"]) {
            return Ok(RuleRestrictionKind::CantBeBlockedByMoreThan(1));
        }
        if ts.eat_phrase(&["except", "by"]) {
            let ncp = ts.checkpoint();
            if let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&["or", "more", "creatures"])
            {
                return Ok(RuleRestrictionKind::CantBeBlockedExceptByCount(
                    u8::try_from(n).unwrap_or(2),
                ));
            }
            ts.restore(ncp);
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
    if ts.eat_phrase(&["be", "cast"]) {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    Err(ts.err("expected restriction after `can't`"))
}

fn parse_cost_modifier(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let _ = ts.eat_word("each");
    let mut nth_spell: Option<u8> = None;
    if ts.eat_phrase(&["the", "first"]) {
        nth_spell = Some(1);
    } else if ts.eat_phrase(&["the", "second"]) {
        nth_spell = Some(2);
    }

    let mut flt: ObjectFilter = if ts.eat_tilde()
        || ts.eat_phrase(&["this", "spell"])
        || ts.eat_phrase(&["those", "spells"])
        || ts.eat_word("it")
    {
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
