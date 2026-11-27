//! Strictly-typed [`StaticAbility`] parser for `v2`.

use crate::ast::ControllerRelation;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_keyword_list, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    ConditionExpr, FilterPredicate, ObjectFilter, RuleRestrictionKind, StaticAbility,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_for_each_expr;

/// Parse one strictly-typed [`StaticAbility`] sentence (fail-closed: zero leftover tokens).
pub fn parse_static_ability(ts: &mut TokenStream<'_>) -> PResult<StaticAbility> {
    // Optional leading `As long as <condition>, `
    let condition = if ts.eat_phrase(&["as", "long", "as"]) {
        let c = parse_condition(ts)?;
        if !ts.eat_comma() {
            return Err(ts.err("expected `,` after `As long as <condition>`"));
        }
        Some(c)
    } else {
        None
    };

    let st = parse_static_core(ts, condition)?;
    let _ = ts.eat_period();
    ts.expect_eof()?;
    Ok(st)
}

fn parse_static_core(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 1. `The next <flt> you cast this turn can't be countered`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["you", "cast", "this", "turn"])
        {
            flt.controller = Some(ControllerRelation::You);
            if ts.eat_phrase(&["can't", "be", "countered"]) {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt.clone(),
                    effect: Box::new(StaticAbility::RuleRestriction {
                        subject: None,
                        rule: RuleRestrictionKind::CantBeCountered,
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

    // `<subj> gets/get +X/+Y [and has/have <kws>]`
    if ts.eat_word("gets") || ts.eat_word("get") {
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let mut grant_keywords = Vec::new();
        if (ts.eat_phrase(&["and", "have"]) || ts.eat_phrase(&["and", "has"]))
            && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
        {
            grant_keywords = kws;
        }
        return Ok(StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale: None,
            condition,
        });
    }

    // `<subj> has/have <kws>`
    if ts.eat_word("has") || ts.eat_word("have") {
        let (keywords, _) = parse_keyword_list(ts)?;
        return Ok(StaticAbility::GrantKeywords {
            subject,
            keywords,
            condition,
        });
    }

    // `<subj> can't ...`
    if ts.eat_word("can't") {
        if ts.eat_phrase(&["be", "the", "targets", "of"])
            || ts.eat_phrase(&["be", "the", "target", "of"])
        {
            let src = parse_object_filter(ts)?;
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::CantBeTargetedBy(Box::new(src)),
                condition,
            });
        }
        if ts.eat_phrase(&["attack", "or", "block"]) {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::CantAttackOrBlock,
                condition,
            });
        }
        if ts.eat_word("attack") {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::CantAttack,
                condition,
            });
        }
        if ts.eat_word("block") {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::CantBlock,
                condition,
            });
        }
        if ts.eat_phrase(&["be", "blocked"]) {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::CantBeBlocked,
                condition,
            });
        }
        if ts.eat_phrase(&["be", "countered"]) {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::CantBeCountered,
                condition,
            });
        }
    }

    Err(ts.err("expected static ability predicate"))
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

    let mut flt: ObjectFilter = parse_object_filter(ts)?;

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

    Ok(StaticAbility::CostModifier {
        subject: flt,
        delta,
        less,
        scale,
        min_mana: None,
        condition,
    })
}
