//! Strictly-typed [`Replacement`] parser for `v2`.

use crate::v2::clause::parse_effect_sentence;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{ObjectFilter, PlayerRef, Replacement, SubjectRef, TriggerEvent, ValueExpr};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed [`Replacement`] sentence (`As ~ enters, ...`,
/// `If <event> would ..., <instead> instead`,
/// `The next <flt> you cast this turn enters with ...`).
pub fn parse_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 0. `As [~ | it] enters, <body (one or more sentences)>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("as")
            && (ts.eat_tilde() || ts.eat_word("it"))
            && ts.eat_word("enters")
            && ts.eat_comma()
        {
            let mut body = Vec::new();
            let mut cur_start = ts.pos;
            for idx in ts.pos..ts.tokens.len() {
                if matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Period) {
                    if idx > cur_start {
                        let mut sub = ts.sub_stream(
                            &ts.tokens[cur_start..idx],
                            crate::v2::ir::ParseStage::Effect,
                        );
                        body.extend(parse_effect_sentence(&mut sub)?);
                        sub.expect_eof()?;
                    }
                    cur_start = idx + 1;
                }
            }
            if cur_start < ts.tokens.len() {
                let mut sub =
                    ts.sub_stream(&ts.tokens[cur_start..], crate::v2::ir::ParseStage::Effect);
                body.extend(parse_effect_sentence(&mut sub)?);
                sub.expect_eof()?;
            }
            ts.pos = ts.tokens.len();
            return Ok(Replacement::AsEnters(body));
        }
        ts.restore(cp);
    }

    // 0b. `You may have ~ enter as a copy of <flt> [, except ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "have"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["enter", "as", "a", "copy", "of"])
        {
            let _ = ts.eat_word("any");
            let filter = parse_object_filter(ts)?;
            let mut exceptions = Vec::new();
            let ecp = ts.checkpoint();
            if ts.eat_comma() && ts.eat_word("except") {
                loop {
                    let item_cp = ts.checkpoint();
                    if ts.eat_phrase(&["it", "has", "this", "ability"]) {
                        exceptions.push(crate::v2::ir::CopyException::HasThisAbility);
                    } else if ((ts.eat_tilde() && ts.eat_phrase(&["'s", "name", "is"]))
                        || ts.eat_phrase(&["its", "name", "is"]))
                        && ts.eat_tilde()
                    {
                        exceptions.push(crate::v2::ir::CopyException::KeepOwnName);
                    } else if ts.eat_phrase(&["it", "has"]) || ts.eat_word("has") {
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
                                exceptions.push(crate::v2::ir::CopyException::AddAbility(
                                    Box::new(compiled),
                                ));
                            } else {
                                ts.restore(item_cp);
                                break;
                            }
                        } else if let Ok((kws, _)) =
                            try_parse(ts, crate::v2::filter::parse_keyword_list)
                        {
                            exceptions.push(crate::v2::ir::CopyException::AddKeywords(kws));
                        } else {
                            ts.restore(item_cp);
                            break;
                        }
                    } else if ts.eat_phrase(&["it", "isn't", "legendary"])
                        || ts.eat_phrase(&["it's", "not", "legendary"])
                    {
                        exceptions.push(crate::v2::ir::CopyException::NotLegendary);
                    } else if ts.eat_phrase(&["it's", "a"])
                        || ts.eat_phrase(&["it's", "an"])
                        || ts.eat_word("it's")
                    {
                        let pt = ts.next_base_pt();
                        let mut subtypes = Vec::new();
                        let mut types = crate::ast::CardTypeMask::EMPTY;
                        while let Some(w) = ts.peek_word() {
                            let low = w.to_ascii_lowercase();
                            if let Some(ct) = crate::vocab::lookup_card_type(&low) {
                                ts.next_word();
                                types = types.union(ct);
                            } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                                ts.next_word();
                                subtypes.push(sub.to_string());
                            } else {
                                break;
                            }
                        }
                        let keep_existing_types =
                            ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
                        if pt.is_some() || !subtypes.is_empty() || !types.is_empty() {
                            exceptions.push(crate::v2::ir::CopyException::SetBasePTAndTypes {
                                power: pt.map(|(p, _)| p),
                                toughness: pt.map(|(_, t)| t),
                                subtypes,
                                types,
                                keep_existing_types,
                            });
                        } else {
                            ts.restore(item_cp);
                            break;
                        }
                    } else if let Some(quoted) = ts.next_quoted() {
                        let compiled = crate::v2::card::compile_face_v2(
                            crate::FaceInput {
                                name: "~",
                                type_line: "Permanent",
                                oracle_text: quoted,
                            },
                            ts.vocab,
                        );
                        if compiled.is_ok() {
                            exceptions
                                .push(crate::v2::ir::CopyException::AddAbility(Box::new(compiled)));
                        } else {
                            ts.restore(item_cp);
                            break;
                        }
                    } else {
                        ts.restore(item_cp);
                        break;
                    }
                    let sep_cp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let had_and = ts.eat_word("and");
                    if !had_comma && !had_and {
                        ts.restore(sep_cp);
                        break;
                    }
                }
                if exceptions.is_empty() {
                    ts.restore(ecp);
                }
            }
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EnterAsCopy {
                optional: true,
                filter,
                exceptions,
            });
        }
        ts.restore(cp);
    }

    // 0c. `All damage that would be dealt to <from> is dealt to <to> instead.` (Pariah)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["all", "damage", "that", "would", "be", "dealt", "to"])
            && let Ok(from) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["is", "dealt", "to"])
            && let Ok(to) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("instead")
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::RedirectAllDamage { from, to });
            }
        }
        ts.restore(cp);
    }

    // 1. `If ... would ...`
    if ts.eat_word("if") {
        // 1a0. Leyline opening hand (`If ~ is in your opening hand, you may begin the game with it on the battlefield.`)
        {
            let cp = ts.checkpoint();
            if ts.eat_tilde()
                && ts.eat_phrase(&["is", "in", "your", "opening", "hand"])
                && ts.eat_comma()
                && ts.eat_phrase(&[
                    "you",
                    "may",
                    "begin",
                    "the",
                    "game",
                    "with",
                    "it",
                    "on",
                    "the",
                    "battlefield",
                ])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::OpeningHandOnBattlefield);
                }
            }
            ts.restore(cp);
        }

        // 1a0b. Daybound enter (`If it's neither day nor night, it becomes day as ~ enters.`)
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["it's", "neither", "day", "nor", "night"])
                && ts.eat_comma()
                && ts.eat_phrase(&["it", "becomes", "day", "as"])
                && ts.eat_tilde()
                && ts.eat_word("enters")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::BecomesDayOnEnter);
                }
            }
            ts.restore(cp);
        }

        // 1a. `If an effect would create one or more tokens under your control, it creates twice that many of those tokens instead.`
        //     OR `If one or more tokens would be created under your control, twice that many of those tokens are created instead.`
        //     OR `If one or more tokens would be created under your control, those tokens plus [an additional | a] <Predefined> token are created instead.`
        //     OR `If you would create one or more <Predefined> tokens, instead create those tokens plus an additional <Predefined> token.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&[
                "an", "effect", "would", "create", "one", "or", "more", "tokens", "under", "your",
                "control",
            ]) && ts.eat_comma()
                && ts.eat_phrase(&[
                    "it", "creates", "twice", "that", "many", "of", "those", "tokens", "instead",
                ])
            {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::MultiplyTokens {
                    controller: PlayerRef::You,
                    factor: 2,
                });
            }
            ts.restore(cp);
            if ts.eat_phrase(&[
                "one", "or", "more", "tokens", "would", "be", "created", "under", "your", "control",
            ]) && ts.eat_comma()
            {
                if ts.eat_phrase(&[
                    "twice", "that", "many", "of", "those", "tokens", "are", "created", "instead",
                ]) {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::MultiplyTokens {
                        controller: PlayerRef::You,
                        factor: 2,
                    });
                }
                if ts.eat_phrase(&["those", "tokens", "plus"]) {
                    let _ = ts.eat_phrase(&["an", "additional"])
                        || ts.eat_word("a")
                        || ts.eat_word("an");
                    if let Some(w) = ts.peek_word()
                        && let Some(pt) = lookup_predefined_token(&w.to_ascii_lowercase())
                    {
                        ts.next_word();
                        if (ts.eat_word("token") || ts.eat_word("tokens"))
                            && ts.eat_phrase(&["are", "created", "instead"])
                        {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::BonusTokens {
                                    filter: None,
                                    controller: PlayerRef::You,
                                    bonus: Box::new(crate::v2::ir::TokenSpec::Predefined(pt)),
                                    bonus_count: ValueExpr::Const(1),
                                });
                            }
                        }
                    }
                }
            }
            ts.restore(cp);
            if ts.eat_phrase(&["you", "would", "create", "one", "or", "more"])
                && let Some(w) = ts.peek_word()
                && let Some(pt) = lookup_predefined_token(&w.to_ascii_lowercase())
            {
                ts.next_word();
                if (ts.eat_word("tokens") || ts.eat_word("token"))
                    && ts.eat_comma()
                    && ts.eat_phrase(&[
                        "instead",
                        "create",
                        "those",
                        "tokens",
                        "plus",
                        "an",
                        "additional",
                    ])
                    && let Some(w2) = ts.peek_word()
                    && let Some(pt2) = lookup_predefined_token(&w2.to_ascii_lowercase())
                {
                    ts.next_word();
                    if ts.eat_word("token") || ts.eat_word("tokens") {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::BonusTokens {
                                filter: Some(ObjectFilter {
                                    token: Some(true),
                                    subtypes: vec![format!("{pt:?}")],
                                    ..Default::default()
                                }),
                                controller: PlayerRef::You,
                                bonus: Box::new(crate::v2::ir::TokenSpec::Predefined(pt2)),
                                bonus_count: ValueExpr::Const(1),
                            });
                        }
                    }
                }
            }
            ts.restore(cp);
        }
        // 1b. `If an effect would put one or more counters on <flt>, it puts twice that many of those counters on that permanent instead.`
        if ts.eat_phrase(&[
            "an", "effect", "would", "put", "one", "or", "more", "counters", "on",
        ]) {
            let flt = parse_object_filter(ts)?;
            if ts.eat_comma()
                && ts.eat_phrase(&[
                    "it",
                    "puts",
                    "twice",
                    "that",
                    "many",
                    "of",
                    "those",
                    "counters",
                    "on",
                    "that",
                    "permanent",
                    "instead",
                ])
            {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::MultiplyCounters {
                    filter: flt,
                    factor: 2,
                });
            }
            return Err(ts.err("unsupported counter-doubling replacement clause"));
        }

        // 1c. `If you would gain life [while <cond>], you gain that much life plus <N> / twice that much life instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["you", "would", "gain", "life"]) {
                let while_cond = if ts.eat_word("while") {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                if ts.eat_comma() && ts.eat_phrase(&["you", "gain"]) {
                    if ts.eat_phrase(&["twice", "that", "much", "life", "instead"]) {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        let rep = Replacement::ModifyLifeGain {
                            player: PlayerRef::You,
                            add: 0,
                            factor: 2,
                        };
                        return Ok(if let Some(condition) = while_cond {
                            Replacement::ConditionalEnters {
                                condition,
                                inner: Box::new(rep),
                            }
                        } else {
                            rep
                        });
                    }
                    if ts.eat_phrase(&["that", "much", "life", "plus"])
                        && let Some(n) = ts.next_number(false)
                        && ts.eat_word("instead")
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        let rep = Replacement::ModifyLifeGain {
                            player: PlayerRef::You,
                            add: i16::try_from(n).unwrap_or(1),
                            factor: 1,
                        };
                        return Ok(if let Some(condition) = while_cond {
                            Replacement::ConditionalEnters {
                                condition,
                                inner: Box::new(rep),
                            }
                        } else {
                            rep
                        });
                    }
                }
            }
            ts.restore(cp);
        }

        // 1d. `If one or more [<kind>] counters would be put on [a/an] <flt>, that many plus <N> / twice that many [of those | <kind>] counters are put on it/that permanent/that creature instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["one", "or", "more"]) {
                let kind = opt(ts, parse_counter_kind);
                if (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["would", "be", "put", "on"])
                {
                    let _ = ts.eat_word("a") || ts.eat_word("an");
                    if let Ok(flt) = try_parse(ts, parse_object_filter)
                        && ts.eat_comma()
                    {
                        if ts.eat_phrase(&["twice", "that", "many"]) {
                            let _ = ts.eat_phrase(&[
                                "of", "each", "of", "those", "kinds", "of", "counters",
                            ]) || ts.eat_phrase(&["of", "those", "counters"])
                                || (opt(ts, parse_counter_kind).is_some()
                                    && (ts.eat_word("counters") || ts.eat_word("counter")));
                            if ts.eat_phrase(&["are", "put", "on", "it", "instead"])
                                || ts.eat_phrase(&[
                                    "are",
                                    "put",
                                    "on",
                                    "that",
                                    "permanent",
                                    "instead",
                                ])
                                || ts.eat_phrase(&[
                                    "are", "put", "on", "that", "creature", "instead",
                                ])
                            {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(Replacement::MultiplyCounters {
                                    filter: flt,
                                    factor: 2,
                                });
                            }
                        } else if ts.eat_phrase(&["that", "many", "plus"])
                            && let Some(n) = ts.next_number(false)
                        {
                            let _ = ts.eat_phrase(&["of", "those", "counters"])
                                || (opt(ts, parse_counter_kind).is_some()
                                    && (ts.eat_word("counters") || ts.eat_word("counter")));
                            if ts.eat_phrase(&["are", "put", "on", "it", "instead"])
                                || ts.eat_phrase(&[
                                    "are",
                                    "put",
                                    "on",
                                    "that",
                                    "permanent",
                                    "instead",
                                ])
                                || ts.eat_phrase(&[
                                    "are", "put", "on", "that", "creature", "instead",
                                ])
                            {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(Replacement::ModifyCountersPlaced {
                                    filter: flt,
                                    kind,
                                    add: i16::try_from(n).unwrap_or(1),
                                });
                            }
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        // 1e. `If <source> would deal [combat|noncombat] damage [to <tgt>], <prevent | modify | instead>`
        //     Supports `a [another] [color] source [you control]`, `enchanted creature`, `~`, or any `ObjectFilter`!
        {
            let cp = ts.checkpoint();
            let mut src_flt_opt: Option<ObjectFilter> = None;
            {
                let scp = ts.checkpoint();
                let mut src_flt = ObjectFilter::default();
                let mut matched_src_word = false;
                let _ = ts.eat_word("a") || ts.eat_word("an");
                if ts.eat_word("another") {
                    src_flt.flags = src_flt
                        .flags
                        .union(crate::v2::ir::FilterFlags::EXCLUDE_SELF);
                }
                if let Some(w) = ts.peek_word()
                    && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
                {
                    ts.next_word();
                    src_flt.any_colors = src_flt.any_colors.union(c);
                }
                if ts.eat_word("source") {
                    matched_src_word = true;
                    if ts.eat_phrase(&["you", "control"]) {
                        src_flt.controller = Some(crate::ast::ControllerRelation::You);
                    } else if ts.eat_phrase(&["an", "opponent", "controls"]) {
                        src_flt.controller = Some(crate::ast::ControllerRelation::Opponent);
                    }
                }
                if matched_src_word {
                    src_flt_opt = Some(src_flt);
                } else {
                    ts.restore(scp);
                    if ts.eat_tilde() || ts.eat_phrase(&["enchanted", "creature"]) {
                        src_flt_opt = Some(ObjectFilter::default());
                    } else if let Ok(f) = try_parse(ts, parse_object_filter) {
                        src_flt_opt = Some(f);
                    }
                }
            }
            if let Some(src_flt) = src_flt_opt
                && ts.eat_phrase(&["would", "deal"])
            {
                let combat_only = ts.eat_word("combat");
                let _ = ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    let target = if ts.eat_word("to") {
                        opt(ts, parse_subject_ref)
                    } else {
                        None
                    };
                    if ts.eat_comma() {
                        if ts.eat_phrase(&["prevent", "all", "but"])
                            && let Some(n) = ts.next_number(false)
                            && ts.eat_phrase(&["of", "that", "damage"])
                        {
                            let _ = ts.eat_period();
                            ts.expect_eof()?;
                            return Ok(Replacement::PreventAllButDamage {
                                source: Some(src_flt),
                                target,
                                remaining: u16::try_from(n).unwrap_or(1),
                            });
                        }
                        {
                            let pcp = ts.checkpoint();
                            if ts.eat_word("prevent")
                                && let Ok(amount) = try_parse(ts, parse_value_expr)
                                && ts.eat_phrase(&["of", "that", "damage"])
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::PreventFixedDamage {
                                        source: Some(src_flt),
                                        target,
                                        amount,
                                    });
                                }
                            }
                            ts.restore(pcp);
                        }
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            let rider = if ts.eat_word("and") || ts.eat_comma() {
                                let _ = ts.eat_word("and");
                                parse_effect_sentence(ts)?
                            } else {
                                Vec::new()
                            };
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::PreventDamageWithRider {
                                    target: target.unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                    condition: None,
                                    rider,
                                });
                            }
                        }
                        if ts.eat_phrase(&["it", "deals"]) || ts.eat_phrase(&["they", "deal"]) {
                            if ts.eat_phrase(&["double", "that", "damage"])
                                || ts.eat_phrase(&["twice", "that", "much", "damage"])
                            {
                                if ts.eat_word("to") {
                                    let _ = opt(ts, parse_subject_ref);
                                }
                                if ts.eat_word("instead") {
                                    let _ = ts.eat_period();
                                    ts.expect_eof()?;
                                    return Ok(Replacement::ModifyDamage {
                                        source: Some(src_flt),
                                        target,
                                        add: 0,
                                        factor: 2,
                                    });
                                }
                            }
                            if ts.eat_phrase(&["triple", "that", "damage"]) {
                                if ts.eat_word("to") {
                                    let _ = opt(ts, parse_subject_ref);
                                }
                                if ts.eat_word("instead") {
                                    let _ = ts.eat_period();
                                    ts.expect_eof()?;
                                    return Ok(Replacement::ModifyDamage {
                                        source: Some(src_flt),
                                        target,
                                        add: 0,
                                        factor: 3,
                                    });
                                }
                            }
                            if ts.eat_phrase(&["that", "much", "damage", "plus"])
                                && let Some(n) = ts.next_number(false)
                            {
                                if ts.eat_word("to") {
                                    let _ = opt(ts, parse_subject_ref);
                                }
                                if ts.eat_word("instead") {
                                    let _ = ts.eat_period();
                                    ts.expect_eof()?;
                                    return Ok(Replacement::ModifyDamage {
                                        source: Some(src_flt),
                                        target,
                                        add: i16::try_from(n).unwrap_or(1),
                                        factor: 1,
                                    });
                                }
                            }
                        }
                        // General damage replacement (`If a source you control would deal noncombat damage to ..., put that many -1/-1 counters on that creature instead.`)
                        if let Ok(instead) = parse_instead_clause(ts) {
                            let _ = ts.eat_period();
                            ts.expect_eof()?;
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::DealsDamage {
                                    combat_only,
                                    source: Some(SubjectRef::Group(Box::new(src_flt))),
                                    target,
                                },
                                instead,
                            });
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        // 1f. `If [combat] damage would be dealt to <subj> [while <cond>], prevent that damage [and/, <eff>] | <instead>.`
        {
            let cp = ts.checkpoint();
            let combat_only = ts.eat_word("combat");
            if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
                && let Ok(target) = try_parse(ts, parse_subject_ref)
            {
                let cond = if ts.eat_word("while") {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                if ts.eat_comma() {
                    if ts.eat_phrase(&["prevent", "that", "damage"]) {
                        if ts.eat_word("and") || ts.eat_comma() {
                            let _ = ts.eat_word("and");
                            let rider = parse_effect_sentence(ts)?;
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::PreventDamageWithRider {
                                    target,
                                    condition: cond,
                                    rider,
                                });
                            }
                        } else {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(if cond.is_some() {
                                    Replacement::PreventDamageWithRider {
                                        target,
                                        condition: cond,
                                        rider: Vec::new(),
                                    }
                                } else {
                                    Replacement::PreventDamageReplacement { target }
                                });
                            }
                        }
                    } else if let Ok(instead) = parse_instead_clause(ts) {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::DealtDamage {
                                    subject: target,
                                    combat_only,
                                },
                                instead,
                            });
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        // 1f2. `If you would begin your draw step, [you may] skip that step instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["you", "would", "begin", "your", "draw", "step"]) && ts.eat_comma() {
                let optional = ts.eat_phrase(&["you", "may"]);
                if ts.eat_phrase(&["skip", "that", "step", "instead"]) {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::SkipStepReplacement {
                            player: PlayerRef::You,
                            step: crate::ast::TurnStepAst::DrawStep,
                            optional,
                        });
                    }
                }
            }
            ts.restore(cp);
        }

        // 1f3. `If a spell or ability an opponent controls causes you to discard ~, put it onto the battlefield instead of putting it into your graveyard.` (`Loxodon Smiter`)
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&[
                "a", "spell", "or", "ability", "an", "opponent", "controls", "causes", "you", "to",
                "discard",
            ]) && ts.eat_tilde()
                && ts.eat_comma()
            {
                let remaining = &ts.tokens[ts.pos..];
                let mut end = remaining.len();
                if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period)
                {
                    end -= 1;
                }
                let mut sub = ts.sub_stream(&remaining[..end], crate::v2::ir::ParseStage::Effect);
                if let Ok(instead) = parse_effect_sentence(&mut sub)
                    && sub.is_eof()
                {
                    ts.pos = ts.tokens.len();
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::ForcedDiscard {
                            player: PlayerRef::You,
                            subject: SubjectRef::SelfObject,
                        },
                        instead,
                    });
                }
            }
            ts.restore(cp);
        }

        // 1f4. `If an opponent / you would draw a card except the first one they/you draw in each of their/your draw steps, <instead>` (`Teferi's Ageless Insight`, `Hullbreacher`)
        {
            let cp = ts.checkpoint();
            let draw_player = if ts.eat_phrase(&[
                "you", "would", "draw", "a", "card", "except", "the", "first", "one", "you",
                "draw", "in", "each", "of", "your", "draw", "steps",
            ]) {
                Some(PlayerRef::You)
            } else if ts.eat_phrase(&[
                "an", "opponent", "would", "draw", "a", "card", "except", "the", "first", "one",
                "they", "draw", "in", "each", "of", "their", "draw", "steps",
            ]) {
                Some(PlayerRef::AnyOpponent)
            } else {
                None
            };
            if let Some(player) = draw_player
                && ts.eat_comma()
                && let Ok(instead) = parse_instead_clause(ts)
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::CardDrawn { player },
                        instead,
                    });
                }
            }
            ts.restore(cp);
        }

        // 1f5. `If an opponent would draw two or more cards, <instead>` (`Alms Collector`)
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&[
                "an", "opponent", "would", "draw", "two", "or", "more", "cards",
            ]) && ts.eat_comma()
                && let Ok(instead) = parse_instead_clause(ts)
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::CardDrawn {
                            player: PlayerRef::AnyOpponent,
                        },
                        instead,
                    });
                }
            }
            ts.restore(cp);
        }

        // 1g. `If you would draw a card [while <cond>], <instead>.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["you", "would", "draw", "a", "card"]) {
                let while_cond = if ts
                    .eat_phrase(&["while", "your", "library", "has", "no", "cards", "in", "it"])
                {
                    Some(crate::v2::ir::ConditionExpr::LibraryEmpty(PlayerRef::You))
                } else if ts.eat_word("while") {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                if ts.eat_comma()
                    && let Ok(instead) = parse_instead_clause(ts)
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    let rep = Replacement::EventInstead {
                        event: TriggerEvent::CardDrawn {
                            player: PlayerRef::You,
                        },
                        instead,
                    };
                    return Ok(if let Some(condition) = while_cond {
                        Replacement::ConditionalEnters {
                            condition,
                            inner: Box::new(rep),
                        }
                    } else {
                        rep
                    });
                }
            }
            ts.restore(cp);
        }

        // 1g1. `If <flt> would deal [combat] damage to <tgt>, prevent <N> of that damage.`
        {
            let cp = ts.checkpoint();
            if let Ok(src_flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "deal"])
            {
                let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    let target = if ts.eat_word("to") {
                        opt(ts, parse_subject_ref)
                    } else {
                        None
                    };
                    if ts.eat_comma()
                        && ts.eat_word("prevent")
                        && let Ok(amount) = try_parse(ts, parse_value_expr)
                        && ts.eat_phrase(&["of", "that", "damage"])
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::PreventFixedDamage {
                                source: Some(src_flt),
                                target,
                                amount,
                            });
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        // 1g2. Mill doubling (`If an opponent/a player would mill one or more cards, they mill twice that many cards instead.`)
        {
            let cp = ts.checkpoint();
            if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
                && ts.eat_phrase(&["would", "mill", "one", "or", "more", "cards"])
                && ts.eat_comma()
                && ts.eat_phrase(&["they", "mill", "twice", "that", "many", "cards", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::MultiplyMill { player, factor: 2 });
                }
            }
            ts.restore(cp);
        }

        // 1g3. Extra turn skip (`If a player would begin an extra turn, that player skips that turn instead.`)
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["a", "player", "would", "begin", "an", "extra", "turn"])
                && ts.eat_comma()
                && ts.eat_phrase(&["that", "player", "skips", "that", "turn", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::StepBegins {
                            step: crate::ast::TurnStepAst::Upkeep,
                            owner: crate::v2::ir::StepOwner::EachPlayer,
                        },
                        instead: vec![crate::v2::ir::Effect::MutatePlayer {
                            player: PlayerRef::ThatPlayer,
                            mutation: crate::v2::ir::PlayerMutation::SkipNextTurn,
                            duration: crate::v2::ir::Duration::Instant,
                        }],
                    });
                }
            }
            ts.restore(cp);
        }

        // 1g4. Explore doubling (`If <flt> would explore, instead it explores, then it explores again.`)
        {
            let cp = ts.checkpoint();
            if let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "explore"])
                && ts.eat_comma()
                && ts.eat_phrase(&["instead", "it", "explores"])
                && ts.eat_comma()
                && ts.eat_phrase(&["then", "it", "explores", "again"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ExploreAgain { filter });
                }
            }
            ts.restore(cp);
        }

        // 1h. `If <cond>, it/~ enters with <N> <kind> counters on it / enters tapped`
        {
            let cp = ts.checkpoint();
            if let Ok(condition) = try_parse(ts, parse_condition) {
                let _ = ts.eat_phrase(&["with", "its"])
                    && ts.eat_mana_cost().is_some()
                    && ts.eat_word("kicker");
                if ts.eat_comma() && (ts.eat_word("it") || ts.eat_tilde()) {
                    if ts.eat_phrase(&["enters", "tapped"]) {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::ConditionalEnters {
                                condition,
                                inner: Box::new(Replacement::EntersTapped { unless: None }),
                            });
                        }
                    } else if ts.eat_phrase(&["enters", "with"]) {
                        let count = if ts.eat_word("a") || ts.eat_word("an") {
                            ValueExpr::Const(1)
                        } else {
                            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                        };
                        let additional = ts.eat_word("additional");
                        if let Ok(kind) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counters") || ts.eat_word("counter"))
                            && ts.eat_phrase(&["on", "it"])
                        {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::ConditionalEnters {
                                    condition,
                                    inner: Box::new(Replacement::EntersWithCounters {
                                        subject: None,
                                        kind,
                                        count,
                                        additional,
                                    }),
                                });
                            }
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        let subj = parse_subject_ref(ts)?;
        if (ts.eat_phrase(&["would", "enter", "the", "battlefield"])
            || ts.eat_phrase(&["would", "enter"]))
            && {
                let _ = ts.eat_phrase(&["this", "turn"]);
                ts.eat_comma()
            }
        {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::ZoneChange {
                    from: None,
                    to: Some(crate::v2::ir::ZoneKind::Battlefield),
                    subject: subj,
                },
                instead,
            });
        }
        if ts.eat_phrase(&["would", "die"]) && {
            let _ = ts.eat_phrase(&["this", "turn"]);
            ts.eat_comma()
        } {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::Dies { subject: subj },
                instead,
            });
        }
        if ts.eat_phrase(&["would", "leave", "the", "battlefield"]) && {
            let _ = ts.eat_phrase(&["this", "turn"]);
            ts.eat_comma()
        } {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::ZoneChange {
                    from: Some(crate::v2::ir::ZoneKind::Battlefield),
                    to: None,
                    subject: subj,
                },
                instead,
            });
        }
        if ts.eat_phrase(&["would", "be", "put", "into"]) {
            let z = crate::v2::filter::parse_zone_ref(ts)?;
            if z.zone == crate::v2::ir::ZoneKind::Graveyard {
                let from = if ts.eat_phrase(&["from", "the", "battlefield"]) {
                    Some(crate::v2::ir::ZoneKind::Battlefield)
                } else {
                    let _ = ts.eat_phrase(&["from", "anywhere"]);
                    None
                };
                let _ = ts.eat_phrase(&["this", "turn"]);
                if ts.eat_comma() {
                    let instead = parse_instead_clause(ts)?;
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PutIntoGraveyard {
                            subject: subj,
                            from,
                        },
                        instead,
                    });
                }
            }
        }
        return Err(ts.err("unsupported `If ... would ...` replacement event"));
    }

    // 2a0. `The first time you would draw a card each turn, <instead>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "the", "first", "time", "you", "would", "draw", "a", "card", "each", "turn",
        ]) && ts.eat_comma()
            && let Ok(instead) = parse_instead_clause(ts)
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::ConditionalEnters {
                    condition: crate::v2::ir::ConditionExpr::FirstTimeEachTurn,
                    inner: Box::new(Replacement::EventInstead {
                        event: TriggerEvent::CardDrawn {
                            player: PlayerRef::You,
                        },
                        instead,
                    }),
                });
            }
        }
        ts.restore(cp);
    }

    // 2a1. `The next <val> damage that would be dealt to <from> this turn is dealt to <to> instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next"])
            && let Ok(amount) = try_parse(ts, parse_value_expr)
            && ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "to"])
            && let Ok(from) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_phrase(&["is", "dealt", "to"])
                && let Ok(to) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("instead")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::RedirectNextDamage { amount, from, to });
                }
            }
        }
        ts.restore(cp);
    }

    // 2a. `The next time <src> would deal damage [to <tgt>] [this turn], <instead>` (Circle of Protection / Rune of Protection / Deflecting Palm)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next", "time"]) {
            let mut src_flt = ObjectFilter::default();
            let mut parsed_source = false;
            {
                let scp = ts.checkpoint();
                if ts.eat_word("a") || ts.eat_word("an") {
                    if let Some(w) = ts.peek_word()
                        && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
                    {
                        ts.next_word();
                        src_flt.any_colors = src_flt.any_colors.union(c);
                    } else if let Some(w) = ts.peek_word()
                        && let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase())
                    {
                        ts.next_word();
                        src_flt.any_types = src_flt.any_types.union(ct);
                    }
                    if ts.eat_word("source") {
                        parsed_source = true;
                        let _ = ts.eat_phrase(&["of", "your", "choice"]);
                        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
                            src_flt
                                .predicates
                                .push(crate::v2::ir::FilterPredicate::OfChosenColor);
                        } else if ts.eat_phrase(&["you", "control"]) {
                            src_flt.controller = Some(crate::ast::ControllerRelation::You);
                        }
                    }
                }
                if !parsed_source {
                    ts.restore(scp);
                }
            }
            if !parsed_source && let Ok(f) = try_parse(ts, parse_object_filter) {
                src_flt = f;
            }
            if ts.eat_phrase(&["would", "deal"]) {
                let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    let target = if ts.eat_word("to") {
                        opt(ts, parse_subject_ref)
                    } else {
                        None
                    };
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma() {
                        {
                            let pcp = ts.checkpoint();
                            if ts.eat_word("prevent")
                                && let Ok(amount) = try_parse(ts, parse_value_expr)
                                && ts.eat_phrase(&["of", "that", "damage"])
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::PreventFixedDamage {
                                        source: Some(src_flt),
                                        target,
                                        amount,
                                    });
                                }
                            }
                            ts.restore(pcp);
                        }
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::NextDamageReplacement {
                                    source: Some(src_flt),
                                    target,
                                    instead: Vec::new(),
                                });
                            }
                        } else if let Ok(instead) = parse_instead_clause(ts) {
                            let _ = ts.eat_period();
                            ts.expect_eof()?;
                            return Ok(Replacement::NextDamageReplacement {
                                source: Some(src_flt),
                                target,
                                instead,
                            });
                        }
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 2. `The next <flt> you cast this turn enters with [a number of] [additional] <kind> counters on it equal to <val>`
    if ts.eat_phrase(&["the", "next"]) {
        let flt = parse_object_filter(ts)?;
        if ts.eat_phrase(&["you", "cast", "this", "turn", "enters", "with"])
            || ts.eat_phrase(&["this", "turn", "enters", "with"])
        {
            let _ = ts.eat_phrase(&["a", "number", "of"]);
            let additional = ts.eat_word("additional");
            let kind = parse_counter_kind(ts)?;
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["on", "it", "equal", "to"])
            {
                let count = parse_value_expr(ts)?;
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::EntersWithCounters {
                    subject: Some(SubjectRef::Group(Box::new(flt))),
                    kind,
                    count,
                    additional,
                });
            }
        }
        return Err(ts.err("unsupported `The next ... enters with ...` replacement"));
    }

    // 3. `~ escapes with N <kind> counters on it` / `~ enters tapped [unless <cond>]` / `~ enters with N <kind> counters on it`
    if ts.eat_tilde() {
        if ts.eat_phrase(&["escapes", "with"]) {
            let count = if ts.eat_word("a") || ts.eat_word("an") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let kind = parse_counter_kind(ts)?;
            if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_phrase(&["on", "it"]) {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::EscapesWithCounters { kind, count });
            }
        }
        if ts.eat_phrase(&["enters", "tapped"]) {
            // Support `~ enters tapped with N <kind> counters on it` (`Vivid Marsh`, `Hickory Woodlot`, `Sleep-Cursed Faerie`)
            if ts.eat_word("with") {
                let count = if ts.eat_word("a") || ts.eat_word("an") {
                    ValueExpr::Const(1)
                } else {
                    try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                };
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["on", "it"])
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::EntersWithCounters {
                        subject: None,
                        kind,
                        count,
                        additional: false,
                    });
                }
            }
            let unless = if ts.eat_word("unless") {
                Some(parse_condition(ts)?)
            } else {
                None
            };
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EntersTapped { unless });
        }
        if ts.eat_phrase(&["enters", "with"]) {
            let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
                None
            } else {
                Some(try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1)))
            };
            let kind = parse_counter_kind(ts)?;
            if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_phrase(&["on", "it"]) {
                let count = if let Some(c) = count_opt {
                    if let Some(scale) = opt(ts, crate::v2::value::parse_for_each_expr) {
                        ValueExpr::Mul(Box::new(c), Box::new(scale))
                    } else {
                        c
                    }
                } else if ts.eat_phrase(&["equal", "to"]) {
                    parse_value_expr(ts)?
                } else {
                    return Err(ts.err(
                        "expected `equal to <val>` after `a number of <kind> counters on it`",
                    ));
                };
                let cond = if ts.eat_word("if") {
                    Some(parse_condition(ts)?)
                } else {
                    None
                };
                let _ = ts.eat_period();
                ts.expect_eof()?;
                let base = Replacement::EntersWithCounters {
                    subject: None,
                    kind,
                    count,
                    additional: false,
                };
                return Ok(if let Some(condition) = cond {
                    Replacement::ConditionalEnters {
                        condition,
                        inner: Box::new(base),
                    }
                } else {
                    base
                });
            }
        }
    }

    Err(ts.err("expected replacement effect"))
}

fn lookup_predefined_token(low: &str) -> Option<crate::v2::ir::PredefinedToken> {
    use crate::v2::ir::PredefinedToken;
    match low {
        "treasure" => Some(PredefinedToken::Treasure),
        "clue" => Some(PredefinedToken::Clue),
        "food" => Some(PredefinedToken::Food),
        "blood" => Some(PredefinedToken::Blood),
        "map" => Some(PredefinedToken::Map),
        "powerstone" => Some(PredefinedToken::Powerstone),
        "shard" => Some(PredefinedToken::Shard),
        "lander" => Some(PredefinedToken::Lander),
        "junk" => Some(PredefinedToken::Junk),
        "gold" => Some(PredefinedToken::Gold),
        "incubator" => Some(PredefinedToken::Incubator),
        "mutagen" => Some(PredefinedToken::Mutagen),
        _ => None,
    }
}

fn parse_instead_clause(ts: &mut TokenStream<'_>) -> PResult<Vec<crate::v2::ir::Effect>> {
    // Support both leading `instead <effect>` and trailing `<effect> instead`
    if ts.eat_word("instead") {
        let remaining = &ts.tokens[ts.pos..];
        let mut end = remaining.len();
        if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
            end -= 1;
        }
        let body_slice = &remaining[..end];
        let mut sub = ts.sub_stream(body_slice, ts.stage);
        let effects = parse_effect_sentence(&mut sub)?;
        sub.expect_eof()?;
        ts.pos += end;
        return Ok(effects);
    }
    let remaining = &ts.tokens[ts.pos..];
    let mut end = remaining.len();
    if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
        end -= 1;
    }
    if end == 0
        || !matches!(
            remaining[end - 1].kind,
            crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        )
    {
        return Err(ts.err("expected trailing `instead` on replacement clause"));
    }
    let body_slice = &remaining[..end - 1];
    let mut sub = ts.sub_stream(body_slice, ts.stage);
    let effects = parse_effect_sentence(&mut sub)?;
    sub.expect_eof()?;
    ts.pos += end;
    Ok(effects)
}
