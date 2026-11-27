//! Static spell cost-modifier and next-spell-modifier parsers for `v2`.

use crate::ast::{ControllerRelation, TurnStepAst};
use crate::v2::filter::{parse_keyword_list, parse_object_filter};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, ConditionFlag, EventCheckKind, FilterPredicate,
    ObjectFilter, PlayerRef, RuleRestrictionKind, StaticAbility, SubjectRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

use super::cond_ext::{parse_static_condition, parse_static_scale_expr};

/// Parse `The next/first <flt> you/they cast this/each turn can't be countered / can be cast ... / has <kws> / costs {M} less to cast`.
pub(super) fn parse_next_spell_modifier(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();
    if (ts.eat_phrase(&["the", "next"]) || ts.eat_phrase(&["the", "first"]))
        && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        && (ts.eat_phrase(&["you", "cast", "this", "turn"])
            || ts.eat_phrase(&["you", "cast", "each", "turn"])
            || ts.eat_phrase(&["you", "cast", "during", "each", "of", "your", "turns"])
            || ts.eat_phrase(&["they", "cast", "this", "turn"])
            || ts.eat_phrase(&["they", "cast", "each", "turn"])
            || (flt.controller.is_some()
                && (ts.eat_phrase(&["this", "turn"])
                    || ts.eat_phrase(&["each", "turn"])
                    || ts.eat_phrase(&["during", "each", "of", "your", "turns"]))))
    {
        flt.controller = Some(ControllerRelation::You);
        if ts.eat_phrase(&[
            "that", "mana", "from", "a", "treasure", "was", "spent", "to", "cast",
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                crate::v2::ir::PriorActionKind::CreatedThisWay,
            ));
        }
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
        if ts.eat_phrase(&["can", "be", "cast", "as", "though", "it", "had", "flash"]) {
            return Ok(StaticAbility::NextSpellModifier {
                filter: flt.clone(),
                effect: Box::new(StaticAbility::ZoneCastPermission {
                    player: PlayerRef::You,
                    filter: Some(flt),
                    subject: None,
                    zone: None,
                    mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                    condition,
                }),
            });
        }
        if ts.eat_phrase(&[
            "can", "be", "cast", "without", "paying", "its", "mana", "cost",
        ]) {
            return Ok(StaticAbility::NextSpellModifier {
                filter: flt.clone(),
                effect: Box::new(StaticAbility::ZoneCastPermission {
                    player: PlayerRef::You,
                    filter: Some(flt),
                    subject: None,
                    zone: None,
                    mode: CastPermissionMode::WithoutPaying,
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
            let scale = opt(ts, parse_static_scale_expr);
            let cost_mod = StaticAbility::CostModifier {
                subject: flt.clone(),
                delta,
                less: true,
                scale,
                min_mana: None,
                condition: condition.clone(),
            };
            let effect = if ts.eat_phrase(&[
                "and", "can", "be", "cast", "as", "though", "it", "had", "flash",
            ]) {
                StaticAbility::Compound(vec![
                    cost_mod,
                    StaticAbility::ZoneCastPermission {
                        player: PlayerRef::You,
                        filter: Some(flt.clone()),
                        subject: None,
                        zone: None,
                        mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                        condition,
                    },
                ])
            } else {
                cost_mod
            };
            return Ok(StaticAbility::NextSpellModifier {
                filter: flt,
                effect: Box::new(effect),
            });
        }
    }
    ts.restore(cp);
    Err(ts.err("not a next-spell modifier"))
}

/// Parse `[The first ]<filter> [<caster> cast(s)] [each turn] cost(s) {M} more/less to cast [for each <expr>]`.
pub(super) fn parse_cost_modifier(
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
    } else if ts.eat_phrase(&["your", "commander"]) {
        ObjectFilter {
            any_spell: true,
            controller: Some(ControllerRelation::You),
            flags: crate::v2::ir::FilterFlags::IS_COMMANDER,
            ..Default::default()
        }
    } else {
        let first = parse_object_filter(ts)?;
        // Support `<flt1> and <flt2> [you cast] cost ...`
        let acp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(second) = try_parse(ts, parse_object_filter)
        {
            ObjectFilter {
                predicates: vec![FilterPredicate::AnyOf(vec![first, second])],
                ..Default::default()
            }
        } else {
            ts.restore(acp);
            first
        }
    };

    // Preserve caster relation explicitly!
    if ts.eat_phrase(&["you", "cast"]) {
        flt.controller = Some(ControllerRelation::You);
    } else if ts.eat_phrase(&["your", "opponents", "cast"]) || ts.eat_phrase(&["opponents", "cast"])
    {
        flt.controller = Some(ControllerRelation::Opponents);
    } else if ts.eat_phrase(&["an", "opponent", "casts"]) {
        flt.controller = Some(ControllerRelation::Opponent);
    } else if ts.eat_phrase(&["enchanted", "player", "casts"]) {
        flt.controller = Some(ControllerRelation::ThatPlayer);
    } else if ts.eat_phrase(&["players", "cast"]) {
        // Any player
    }
    let _ = ts.eat_phrase(&["each", "turn"]);
    let _ = ts.eat_phrase(&[
        "that", "share", "a", "card", "type", "with", "the", "exiled", "card",
    ]);

    let mut inline_cond: Option<ConditionExpr> = None;
    if ts.eat_phrase(&["during", "your", "turn"]) {
        inline_cond = Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn));
    }

    if let Some(n) = nth_spell {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(n));
    }

    if !(ts.eat_word("cost") || ts.eat_word("costs")) {
        return Err(ts.err("expected `cost` or `costs` in cost modifier"));
    }

    // `<flt> cost(s) an additional <cost | N life | "<quoted>"> to cast [for each ...]`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["an", "additional"]) {
            let costs_opt = if let Some(n) = ts.next_number(false)
                && ts.eat_word("life")
            {
                Some(vec![crate::v2::ir::Cost::PayLife(
                    crate::v2::ir::ValueExpr::Const(n),
                )])
            } else if let Some(quoted) = ts.next_quoted()
                && let Ok(toks) = crate::v2::lexer::tokenize(quoted)
            {
                let mut qts =
                    TokenStream::new(quoted, &toks, ts.vocab, crate::v2::ir::ParseStage::Cost);
                crate::v2::cost::parse_cost_list(&mut qts).ok()
            } else {
                try_parse(ts, crate::v2::cost::parse_cost_list).ok()
            };
            if let Some(costs) = costs_opt
                && ts.eat_phrase(&["to", "cast"])
            {
                let _ = parse_cost_scale_expr(ts);
                return Ok(StaticAbility::AdditionalCost(costs));
            }
        }
        ts.restore(acp);
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

    let scale = parse_cost_scale_expr(ts);
    let extra_cond = if ts.eat_word("if") {
        Some(parse_static_condition(ts)?)
    } else if ts.eat_phrase(&["during", "your", "end", "step"]) {
        Some(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
            TurnStepAst::EndStep,
        )))
    } else if ts.eat_phrase(&["during", "your", "turn"]) {
        Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn))
    } else if ts.eat_phrase(&["except", "during", "its", "controller's", "turn"])
        || ts.eat_phrase(&["except", "during", "their", "controllers'", "turns"])
    {
        Some(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::IsYourTurn,
        ))))
    } else {
        None
    };
    let mut final_cond = condition;
    for c in [inline_cond, extra_cond].into_iter().flatten() {
        final_cond = Some(match final_cond {
            Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
            None => c,
        });
    }

    let first_mod = StaticAbility::CostModifier {
        subject: flt.clone(),
        delta,
        less,
        scale,
        min_mana: None,
        condition: final_cond.clone(),
    };
    // Optional second reduction conjunct (`and {2} less to cast for each other artifact or creature you've sacrificed this turn`)
    let and_cp = ts.checkpoint();
    if ts.eat_word("and")
        && let Some(delta2) = ts.eat_mana_cost()
        && (ts.eat_phrase(&["less", "to", "cast"]) || ts.eat_phrase(&["more", "to", "cast"]))
    {
        let scale2 = parse_cost_scale_expr(ts);
        return Ok(StaticAbility::Compound(vec![
            first_mod,
            StaticAbility::CostModifier {
                subject: flt,
                delta: delta2,
                less,
                scale: scale2,
                min_mana: None,
                condition: final_cond,
            },
        ]));
    }
    ts.restore(and_cp);

    Ok(first_mod)
}

fn parse_cost_scale_expr(ts: &mut TokenStream<'_>) -> Option<crate::v2::ir::ValueExpr> {
    use crate::v2::ir::{PriorActionKind, TurnEventKind, ValueExpr};
    let cp = ts.checkpoint();
    if ts.eat_phrase(&[
        "for", "each", "time", "it's", "been", "cast", "from", "the", "command", "zone", "this",
        "game",
    ]) {
        return Some(ValueExpr::CommanderCastCount(PlayerRef::You));
    }
    if ts.eat_phrase(&["for", "each", "other", "spell", "cast", "this", "turn"]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::SpellCast,
            player: None,
            exclude_first: true,
        });
    }
    if ts.eat_phrase(&["for", "each", "permanent", "sacrificed", "this", "way"])
        || ts.eat_phrase(&["for", "each", "permanent", "sacrificed", "this", "turn"])
    {
        return Some(ValueExpr::PriorActionCount(
            PriorActionKind::SacrificedThisWay,
        ));
    }
    if ts.eat_phrase(&["for", "each", "other"])
        && try_parse(ts, parse_object_filter).is_ok()
        && ts.eat_phrase(&["you've", "sacrificed", "this", "turn"])
    {
        return Some(ValueExpr::PriorActionCount(
            PriorActionKind::SacrificedThisWay,
        ));
    }
    {
        let c_cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each"])
            && let Some(w) = ts.peek_word()
            && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&["mana", "symbol", "in", "their", "mana", "costs"])
                || ts.eat_phrase(&["mana", "symbol", "in", "their", "activation", "costs"])
            {
                return Some(ValueExpr::Devotion(c));
            }
        }
        ts.restore(c_cp);
    }
    if ts.eat_phrase(&["for", "each", "counter", "removed", "this", "way"]) {
        return Some(ValueExpr::PriorActionCount(
            PriorActionKind::CountersRemovedThisWay,
        ));
    }
    if ts.eat_phrase(&[
        "for",
        "each",
        "card",
        "you've",
        "cycled",
        "or",
        "discarded",
        "this",
        "turn",
    ]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::CardDiscarded,
            player: Some(PlayerRef::You),
            exclude_first: false,
        });
    }
    if ts.eat_phrase(&[
        "for",
        "each",
        "card",
        "your",
        "opponents",
        "have",
        "drawn",
        "this",
        "turn",
    ]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::CardDrawn,
            player: Some(PlayerRef::EachOpponent),
            exclude_first: false,
        });
    }
    if ts.eat_phrase(&[
        "for", "each", "creature", "you", "attacked", "with", "this", "turn",
    ]) {
        return Some(ValueExpr::Count(Box::new(ObjectFilter {
            any_types: crate::ast::CardTypeMask::CREATURE,
            controller: Some(ControllerRelation::You),
            flags: crate::v2::ir::FilterFlags::ATTACKED_THIS_TURN,
            ..Default::default()
        })));
    }
    if ts.eat_phrase(&["for", "each", "creature", "it", "targets"]) {
        return Some(ValueExpr::TargetsBeyondFirst);
    }
    ts.restore(cp);
    opt(ts, parse_static_scale_expr)
}
