//! Strictly-typed recursive [`ConditionExpr`] parser for `v2`.

use crate::v2::filter::{
    parse_keyword_kind, parse_object_filter, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{AnaphoraRef, ConditionExpr, PlayerRef, SubjectRef};
use crate::v2::parser::{PResult, TokenStream, try_parse};

/// Parse a strictly-typed boolean [`ConditionExpr`].
pub fn parse_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let mut cond = parse_condition_atom(ts)?;
    loop {
        if ts.eat_word("and") {
            let rhs = parse_condition_atom(ts)?;
            cond = ConditionExpr::And(Box::new(cond), Box::new(rhs));
            continue;
        }
        if ts.eat_word("or") {
            let rhs = parse_condition_atom(ts)?;
            cond = ConditionExpr::Or(Box::new(cond), Box::new(rhs));
            continue;
        }
        break;
    }
    Ok(cond)
}

fn parse_condition_atom(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // `it's the first combat phase of the turn`
    if ts.eat_phrase(&[
        "it's", "the", "first", "combat", "phase", "of", "the", "turn",
    ]) {
        return Ok(ConditionExpr::IsFirstCombatPhase);
    }

    // `its mana cost contains {R}`
    if ts.eat_phrase(&["its", "mana", "cost", "contains"]) {
        let sym = ts
            .next_mana()
            .ok_or_else(|| ts.err("expected mana symbol after `its mana cost contains`"))?;
        return Ok(ConditionExpr::ManaCostContains {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            symbol: sym,
        });
    }

    // `you control ~` / `<player> control(s) [N or more] <filter>`
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && (ts.eat_word("control") || ts.eat_word("controls"))
        {
            if player == PlayerRef::You && ts.eat_tilde() {
                return Ok(ConditionExpr::ControlsSelf);
            }
            let min_count = if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                u16::try_from(n.max(1)).unwrap_or(1)
            } else {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                1
            };
            let filter = parse_object_filter(ts)?;
            return Ok(ConditionExpr::Controls {
                player,
                filter: Box::new(filter),
                min_count,
            });
        }
        ts.restore(cp);
    }

    // `<subj> doesn't have <kw>` / `<subj> has <kw>`
    {
        let cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["doesn't", "have"]) || ts.eat_phrase(&["don't", "have"]) {
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                })));
            }
            if ts.eat_word("has") || ts.eat_word("have") {
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                });
            }
        }
        ts.restore(cp);
    }

    Err(ts.err("expected boolean condition expression"))
}
