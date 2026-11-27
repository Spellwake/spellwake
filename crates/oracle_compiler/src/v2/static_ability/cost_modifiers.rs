//! Static spell cost-modifier and next-spell-modifier parsers for `v2`.

use crate::ast::{ControllerRelation, TurnStepAst};
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_keyword_list, parse_object_filter};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, ConditionFlag, EventCheckKind, FilterPredicate,
    ObjectFilter, PlayerRef, RuleRestrictionKind, StaticAbility, SubjectRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_for_each_expr;

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
            || ts.eat_phrase(&["they", "cast", "this", "turn"])
            || ts.eat_phrase(&["they", "cast", "each", "turn"])
            || (flt.controller == Some(ControllerRelation::You)
                && (ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["each", "turn"]))))
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
            let scale = opt(ts, parse_for_each_expr);
            return Ok(StaticAbility::NextSpellModifier {
                filter: flt.clone(),
                effect: Box::new(StaticAbility::CostModifier {
                    subject: flt,
                    delta,
                    less: true,
                    scale,
                    min_mana: None,
                    condition,
                }),
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
    } else {
        let first = parse_object_filter(ts)?;
        // Support `<flt1> and <flt2> [you cast] cost ...` (`Elemental spells and Warrior spells`, `Goblin spells and Shaman spells`)
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
    let extra_cond = if ts.eat_word("if") {
        Some(parse_condition(ts)?)
    } else if ts.eat_phrase(&["during", "your", "end", "step"]) {
        Some(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
            TurnStepAst::EndStep,
        )))
    } else if ts.eat_phrase(&["during", "your", "turn"]) {
        Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn))
    } else {
        None
    };
    let final_cond = match (condition, extra_cond) {
        (Some(prev), Some(c)) => Some(ConditionExpr::And(Box::new(prev), Box::new(c))),
        (None, Some(c)) => Some(c),
        (prev, None) => prev,
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
