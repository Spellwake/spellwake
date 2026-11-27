//! Strictly-typed [`Replacement`] parser for `v2`.

mod if_would;

use crate::v2::clause::parse_effect_sentence;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    DamageReplacement, ObjectFilter, PlayerObjectTriggerKind, PlayerRef, PlayerTriggerKind,
    Replacement, ReplacementRuleKind, SubjectRef, TriggerEvent, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed [`Replacement`] sentence (`As ~ enters, ...`,
/// `If <event> would ..., <instead> instead`,
/// `The next <flt> you cast this turn enters with ...`).
pub fn parse_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 0. `As [~ | it] enters [or is turned face up] / is turned face up / transforms into ~, <body (one or more sentences)>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("as")
            && (ts.eat_tilde() || ts.eat_word("it"))
            && (ts.eat_phrase(&["enters", "or", "is", "turned", "face", "up"])
                || ts.eat_word("enters")
                || ts.eat_phrase(&["is", "turned", "face", "up"])
                || (ts.eat_phrase(&["transforms", "into"]) && ts.eat_tilde()))
            && ts.eat_comma()
        {
            let mut body = Vec::new();
            let mut cur_start = ts.pos;
            let mut sentences: Vec<(usize, usize)> = Vec::new();
            for idx in ts.pos..ts.tokens.len() {
                if matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Period) {
                    if idx > cur_start {
                        sentences.push((cur_start, idx));
                    }
                    cur_start = idx + 1;
                }
            }
            if cur_start < ts.tokens.len() {
                sentences.push((cur_start, ts.tokens.len()));
            }
            for (s_idx, (a, b)) in sentences.into_iter().enumerate() {
                let sl = &ts.tokens[a..b];
                let mut sub = ts.sub_stream(sl, crate::v2::ir::ParseStage::Effect);
                if let Ok(effs) = parse_effect_sentence(&mut sub)
                    && sub.is_eof()
                {
                    body.extend(effs);
                    continue;
                }
                if s_idx > 0 {
                    let mut rts = ts.sub_stream(sl, crate::v2::ir::ParseStage::Replacement);
                    if let Ok(rep) = parse_replacement(&mut rts) {
                        body.push(crate::v2::ir::Effect::ApplyReplacement {
                            replacement: Box::new(rep),
                            duration: crate::v2::ir::Duration::Permanent,
                        });
                        continue;
                    }
                    let mut sts = ts.sub_stream(sl, crate::v2::ir::ParseStage::Static);
                    if let Ok(st) = crate::v2::static_ability::parse_static_ability(&mut sts) {
                        body.push(crate::v2::ir::Effect::ApplyContinuous {
                            effect: Box::new(st),
                            duration: crate::v2::ir::Duration::Permanent,
                        });
                        continue;
                    }
                }
                let mut err_sub = ts.sub_stream(sl, crate::v2::ir::ParseStage::Effect);
                let _ = parse_effect_sentence(&mut err_sub)?;
                err_sub.expect_eof()?;
            }
            ts.pos = ts.tokens.len();
            return Ok(Replacement::AsEnters(body));
        }
        ts.restore(cp);
    }

    // 0a. `Double all damage <subj> would deal.` (`Wolverine, Best There Is`, `Collective Inferno`)
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["double", "all", "damage"]) {
            let _ = ts.eat_word("that");
            if let Ok(src_flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "deal"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ModifyDamage {
                        source: Some(src_flt),
                        target: None,
                        add: 0,
                        factor: 2,
                    });
                }
            }
            ts.restore(dcp);
            if ts.eat_phrase(&["double", "all", "damage"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["would", "deal"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ModifyDamage {
                        source: None,
                        target: None,
                        add: 0,
                        factor: 2,
                    });
                }
            }
        }
        ts.restore(dcp);
    }

    // 0b. `You may have ~ enter [tapped] as a copy of <flt> [, except ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "have"]) && ts.eat_tilde() && ts.eat_word("enter") && {
            let _ = ts.eat_word("tapped");
            ts.eat_phrase(&["as", "a", "copy", "of"])
        } {
            let _ = ts.eat_word("any");
            let filter = parse_object_filter(ts)?;
            let mut exceptions = Vec::new();
            let ecp = ts.checkpoint();
            if ts.eat_comma() && ts.eat_word("except") {
                loop {
                    let item_cp = ts.checkpoint();
                    if ts.eat_phrase(&["it", "has", "this", "ability"]) {
                        exceptions.push(crate::v2::ir::CopyException::HasThisAbility);
                    } else if ts.eat_phrase(&["it", "enters", "with"]) {
                        let _ = ts.eat_word("an") || ts.eat_word("a");
                        let _ = ts.eat_word("additional");
                        let count = if ts.eat_word("a") || ts.eat_word("an") {
                            ValueExpr::Const(1)
                        } else {
                            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                        };
                        let _ = ts.eat_word("additional");
                        if let Ok(kind) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counter") || ts.eat_word("counters"))
                            && ts.eat_phrase(&["on", "it"])
                        {
                            let cond = if ts.eat_word("if") {
                                opt(ts, parse_condition)
                            } else {
                                None
                            };
                            exceptions.push(crate::v2::ir::CopyException::EntersWithCounters {
                                kind,
                                count,
                                condition: cond,
                            });
                        } else {
                            ts.restore(item_cp);
                            break;
                        }
                    } else if ts.peek_phrase(&["it", "has"])
                        && matches!(
                            ts.peek_n(2).map(|t| &t.kind),
                            Some(crate::v2::lexer::TokenKind::Tilde)
                        )
                        && ts.eat_phrase(&["it", "has"])
                        && ts.eat_tilde()
                        && ts.eat_phrase(&["'s", "other", "abilities"])
                    {
                        exceptions.push(crate::v2::ir::CopyException::KeepOwnOtherAbilities);
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
                            if ts.eat_word("if")
                                && let Ok(cond) = try_parse(ts, parse_condition)
                            {
                                exceptions.push(
                                    crate::v2::ir::CopyException::AddKeywordsConditional {
                                        keywords: kws,
                                        condition: cond,
                                    },
                                );
                            } else {
                                exceptions.push(crate::v2::ir::CopyException::AddKeywords(kws));
                            }
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
                        || ts.eat_phrase(&["is", "a"])
                        || ts.eat_phrase(&["is", "an"])
                    {
                        let pt = ts.next_base_pt();
                        let mut supertypes = crate::ast::SupertypeMask::EMPTY;
                        let mut subtypes = Vec::new();
                        let mut types = crate::ast::CardTypeMask::EMPTY;
                        while let Some(w) = ts.peek_word() {
                            let low = w.to_ascii_lowercase();
                            if let Some(st) = crate::vocab::lookup_supertype(&low) {
                                ts.next_word();
                                supertypes = supertypes.union(st);
                                let _ = ts.eat_word("and");
                            } else if crate::vocab::lookup_color(&low).is_some() {
                                ts.next_word();
                                let _ = ts.eat_word("and");
                            } else if let Some(ct) = crate::vocab::lookup_card_type(&low) {
                                ts.next_word();
                                types = types.union(ct);
                            } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                                ts.next_word();
                                subtypes.push(sub.to_string());
                            } else {
                                break;
                            }
                        }
                        let keep_existing_types = ts.eat_phrase(&[
                            "in", "addition", "to", "its", "other", "colors", "and", "types",
                        ]) || ts.eat_phrase(&[
                            "in", "addition", "to", "its", "other", "creature", "types",
                        ]) || ts
                            .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                            || ts.eat_phrase(&["in", "addition", "to", "his", "other", "types"])
                            || ts.eat_phrase(&["in", "addition", "to", "her", "other", "types"]);
                        if !supertypes.is_empty()
                            && pt.is_none()
                            && subtypes.is_empty()
                            && types.is_empty()
                        {
                            exceptions.push(crate::v2::ir::CopyException::AddSupertypes {
                                supertypes,
                                keep_existing_types,
                            });
                        } else if pt.is_some() || !subtypes.is_empty() || !types.is_empty() {
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
                    } else {
                        let qcp = ts.checkpoint();
                        let _ = ts.eat_phrase(&["it", "has"]) || ts.eat_phrase(&["they", "have"]);
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
                        } else {
                            ts.restore(qcp);
                            ts.restore(item_cp);
                            break;
                        }
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

    // 0c. `All damage / Damage that would be dealt to <from> [by <flt>] is dealt to <to> instead.` (Pariah, Martyrs of Korlis, Veteran Bodyguard)
    //     `Damage that would reduce your life total to less than <N> reduces it to <N> instead.` (Worship, Ali from Cairo)
    //     `Damage that would be dealt by ~ can't be prevented.` (Excruciator)
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&[
            "damage", "that", "would", "reduce", "your", "life", "total", "to", "less", "than",
        ]) && let Some(floor) = ts.next_number(false)
            && ts.eat_phrase(&["reduces", "it", "to"])
            && ts.next_number(false).is_some()
            && ts.eat_word("instead")
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(ReplacementRuleKind::LifeFloor(floor)));
            }
        }
        ts.restore(cp);
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "by"])
            && let Ok(src) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["can't", "be", "prevented"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::DamageRule(
                    DamageReplacement::UnpreventableBySource(src),
                ));
            }
        }
        ts.restore(cp);
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "to"])
            && let Ok(from) = try_parse(ts, parse_subject_ref)
        {
            if ts.eat_word("by") {
                let _ = opt(ts, parse_object_filter);
            }
            if ts.eat_phrase(&["is", "dealt", "to"])
                && let Ok(to) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("instead")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::DamageRule(DamageReplacement::RedirectAll {
                        from,
                        to,
                    }));
                }
            }
        }
        ts.restore(cp);
    }

    // 1. `If ... would ...`
    {
        let if_cp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(rep) = try_parse(ts, if_would::parse_if_would_replacement)
        {
            return Ok(rep);
        }
        ts.restore(if_cp);
    }

    // 2a0. `The first time you would draw a card / create one or more tokens each turn, [you may] <instead>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "the", "first", "time", "you", "would", "draw", "a", "card", "each", "turn",
        ]) && ts.eat_comma()
        {
            let optional = ts.eat_phrase(&["you", "may"]);
            if let Ok(instead) = parse_instead_clause(ts) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    let inner_eff = if optional {
                        vec![crate::v2::ir::Effect::Optional {
                            actor: PlayerRef::You,
                            body: instead,
                        }]
                    } else {
                        instead
                    };
                    return Ok(Replacement::ConditionalEnters {
                        condition: crate::v2::ir::ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::FirstTimeEachTurn,
                        ),
                        inner: Box::new(Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player: PlayerRef::You,
                                action: PlayerTriggerKind::CardDrawn,
                            },
                            instead: inner_eff,
                        }),
                    });
                }
            }
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "the", "first", "time", "you", "would", "create", "one", "or", "more", "tokens",
            "each", "turn",
        ]) && ts.eat_comma()
        {
            let optional = ts.eat_phrase(&["you", "may"]);
            if let Ok(instead) = parse_instead_clause(ts) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    let inner_eff = if optional {
                        vec![crate::v2::ir::Effect::Optional {
                            actor: PlayerRef::You,
                            body: instead,
                        }]
                    } else {
                        instead
                    };
                    return Ok(Replacement::ConditionalEnters {
                        condition: crate::v2::ir::ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::FirstTimeEachTurn,
                        ),
                        inner: Box::new(Replacement::EventInstead {
                            event: TriggerEvent::PlayerObjectEvent {
                                player: PlayerRef::You,
                                kind: PlayerObjectTriggerKind::CreatesTokens,
                                filter: Some(ObjectFilter {
                                    token: Some(true),
                                    ..Default::default()
                                }),
                            },
                            instead: inner_eff,
                        }),
                    });
                }
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
                    return Ok(Replacement::DamageRule(DamageReplacement::RedirectNext {
                        amount,
                        from,
                        to,
                    }));
                }
            }
        }
        ts.restore(cp);
    }

    // 2a. `The next time ... [this turn], <instead>` (damage redirect/prevent/half, draw replacement, lose-game replacement, enter-as-copy)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next", "time"]) {
            // 2a-draw: `The next time you would draw a card this turn, <instead>` (`Words of Waste`, `Words of Wind`, etc.)
            {
                let dcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "would", "draw", "a", "card"]) {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::PlayerAction {
                                    player: PlayerRef::You,
                                    action: PlayerTriggerKind::CardDrawn,
                                },
                                instead,
                            });
                        }
                    }
                }
                ts.restore(dcp);
            }
            // 2a-lose: `The next time you would lose the game this turn, <instead>` (`Stunning Reversal`)
            {
                let lcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "would", "lose", "the", "game"]) {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::LifeChanged {
                                    player: PlayerRef::You,
                                    gained: false,
                                },
                                instead,
                            });
                        }
                    }
                }
                ts.restore(lcp);
            }
            // 2a-dmg-to: `The next time [combat] damage would be dealt to <tgt> this turn, <instead>` (`Kill-Suit Cultist`)
            {
                let d2cp = ts.checkpoint();
                let combat_only = ts.eat_word("combat");
                if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
                    && let Ok(tgt) = try_parse(ts, parse_subject_ref)
                {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::DealtDamage {
                                    subject: tgt,
                                    combat_only,
                                },
                                instead,
                            });
                        }
                    }
                }
                ts.restore(d2cp);
            }
            let mut src_flt = ObjectFilter::default();
            let mut parsed_source = false;
            {
                let scp = ts.checkpoint();
                if ts.eat_tilde() {
                    parsed_source = true;
                } else if ts.eat_word("a") || ts.eat_word("an") {
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
                parsed_source = true;
            }
            if !parsed_source && try_parse(ts, parse_subject_ref).is_ok() {
                parsed_source = true;
            }
            if parsed_source && ts.eat_phrase(&["would", "deal"]) {
                let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    let target = if ts.eat_word("to") {
                        opt(ts, parse_subject_ref)
                    } else {
                        None
                    };
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma() {
                        // `prevent half that damage, rounded down/up` (`Dark Sphere`)
                        {
                            let hcp = ts.checkpoint();
                            if ts.eat_phrase(&["prevent", "half", "that", "damage"]) {
                                let _ = ts.eat_comma();
                                let _ = ts.eat_phrase(&["rounded", "down"])
                                    || ts.eat_phrase(&["rounded", "up"]);
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::PreventFixed {
                                            source: Some(src_flt),
                                            target,
                                            amount: ValueExpr::Div {
                                                expr: Box::new(ValueExpr::PriorActionCount(
                                                    crate::v2::ir::PriorActionKind::ThatMany,
                                                )),
                                                denom: 2,
                                                rounding: crate::ast::Rounding::Down,
                                            },
                                        },
                                    ));
                                }
                            }
                            ts.restore(hcp);
                        }
                        // `that damage is dealt to <to> instead` (`Reflect Damage`, `Shaman en-Kor`)
                        {
                            let rcp = ts.checkpoint();
                            if ts.eat_phrase(&["that", "damage", "is", "dealt", "to"])
                                && let Ok(to) = try_parse(ts, parse_subject_ref)
                                && ts.eat_word("instead")
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::RedirectAll {
                                            from: target
                                                .unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                            to,
                                        },
                                    ));
                                }
                            }
                            ts.restore(rcp);
                        }
                        // `that source/creature / it deals that damage to <to> instead` (`Jade Monolith`, `Shield Dancer`, `Soltari Guerrillas`)
                        {
                            let rcp = ts.checkpoint();
                            if (ts.eat_phrase(&["that", "source", "deals", "that", "damage", "to"])
                                || ts.eat_phrase(&[
                                    "that", "creature", "deals", "that", "damage", "to",
                                ])
                                || ts.eat_phrase(&["it", "deals", "that", "damage", "to"]))
                                && let Ok(to) = try_parse(ts, parse_subject_ref)
                                && ts.eat_word("instead")
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::RedirectAll {
                                            from: target
                                                .unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                            to,
                                        },
                                    ));
                                }
                            }
                            ts.restore(rcp);
                        }
                        {
                            let pcp = ts.checkpoint();
                            if ts.eat_word("prevent")
                                && let Ok(amount) = try_parse(ts, parse_value_expr)
                                && ts.eat_phrase(&["of", "that", "damage"])
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::PreventFixed {
                                            source: Some(src_flt),
                                            target,
                                            amount,
                                        },
                                    ));
                                }
                            }
                            ts.restore(pcp);
                        }
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::DamageRule(
                                    DamageReplacement::ReplaceNext {
                                        source: Some(src_flt),
                                        target,
                                        instead: Vec::new(),
                                    },
                                ));
                            }
                        } else if let Ok(instead) = parse_instead_clause(ts) {
                            let _ = ts.eat_period();
                            ts.expect_eof()?;
                            return Ok(Replacement::DamageRule(DamageReplacement::ReplaceNext {
                                source: Some(src_flt),
                                target,
                                instead,
                            }));
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
                return Ok(Replacement::Rule(
                    ReplacementRuleKind::EscapesWithCounters { kind, count },
                ));
            }
        }
        if ts.eat_phrase(&["enters", "tapped"]) {
            // Support `~ enters tapped [and] with N <kind> counters on it` (`Vivid Marsh`, `Sphere of the Suns`)
            if ts.eat_phrase(&["and", "with"]) || ts.eat_word("with") {
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
            // `~ enters with your choice of a/an <k1> counter or a/an <k2> counter on it` (`Flycatcher Giraffid` Ikoria cycle)
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["your", "choice", "of"])
                    && (ts.eat_word("a") || ts.eat_word("an"))
                    && let Ok(kind1) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && ts.eat_word("or")
                    && (ts.eat_word("a") || ts.eat_word("an"))
                    && let Ok(kind2) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && ts.eat_phrase(&["on", "it"])
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::Rule(
                        ReplacementRuleKind::EntersWithChoiceCounter { kind1, kind2 },
                    ));
                }
                ts.restore(ccp);
            }
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
                let extra_kws = {
                    let kwcp = ts.checkpoint();
                    if ts.eat_phrase(&["and", "with"])
                        && let Ok((kws, _)) = try_parse(ts, crate::v2::keywords::parse_keyword_list)
                    {
                        Some(kws)
                    } else {
                        ts.restore(kwcp);
                        None
                    }
                };
                let cond = if ts.eat_word("if") {
                    Some(parse_condition(ts)?)
                } else {
                    None
                };
                let _ = ts.eat_period();
                ts.expect_eof()?;
                let base_counters = Replacement::EntersWithCounters {
                    subject: None,
                    kind,
                    count,
                    additional: false,
                };
                let base = if let Some(kws) = extra_kws {
                    Replacement::Compound(vec![
                        base_counters,
                        Replacement::Rule(ReplacementRuleKind::EntersWithKeywords(kws)),
                    ])
                } else {
                    base_counters
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

pub(super) fn lookup_predefined_token(low: &str) -> Option<crate::v2::ir::PredefinedToken> {
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

pub(super) fn parse_instead_clause(
    ts: &mut TokenStream<'_>,
) -> PResult<Vec<crate::v2::ir::Effect>> {
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
    // Support trailing `instead of putting it/them anywhere else` / `instead of putting it/them into your/its owner's/a graveyard [as it resolves]`
    let mut instead_idx: Option<usize> = None;
    for i in (0..end).rev() {
        if matches!(
            remaining[i].kind,
            crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        ) {
            if i + 1 == end {
                instead_idx = Some(i);
                break;
            }
            let tail = &remaining[i + 1..end];
            let mut tail_ts = ts.sub_stream(tail, ts.stage);
            let ok_tail = (tail_ts.eat_phrase(&["of", "putting", "it", "anywhere", "else"])
                || tail_ts.eat_phrase(&["of", "putting", "them", "anywhere", "else"])
                || ((tail_ts.eat_phrase(&["of", "putting", "it", "into"])
                    || tail_ts.eat_phrase(&["of", "putting", "them", "into"]))
                    && try_parse(&mut tail_ts, crate::v2::filter::parse_zone_ref).is_ok()
                    && {
                        let _ = tail_ts.eat_phrase(&["as", "it", "resolves"]);
                        true
                    }))
                && tail_ts.is_eof();
            if ok_tail {
                instead_idx = Some(i);
                break;
            }
        }
    }
    let Some(idx) = instead_idx else {
        return Err(ts.err("expected trailing `instead` on replacement clause"));
    };
    let body_slice = &remaining[..idx];
    let mut sub = ts.sub_stream(body_slice, ts.stage);
    let effects = parse_effect_sentence(&mut sub)?;
    sub.expect_eof()?;
    ts.pos += end;
    Ok(effects)
}
