//! Strictly-typed recursive [`ConditionExpr`] parser for `v2`.

use crate::ast::CompareOp;
use crate::v2::filter::{
    parse_keyword_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ObjectStateKind, PlayerRef, SubjectRef, TurnEventKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

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
    // Turn / cast / kicker checks
    if ts.eat_phrase(&[
        "it's", "the", "first", "combat", "phase", "of", "the", "turn",
    ]) {
        return Ok(ConditionExpr::IsFirstCombatPhase);
    }
    if ts.eat_phrase(&["it's", "your", "turn"]) {
        return Ok(ConditionExpr::IsYourTurn);
    }
    if ts.eat_phrase(&["it's", "not", "your", "turn"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::IsYourTurn)));
    }
    if ts.eat_phrase(&["you", "cast", "it"]) {
        return Ok(ConditionExpr::YouCastIt);
    }
    if ts.eat_phrase(&["a", "creature", "died", "this", "turn"]) {
        return Ok(ConditionExpr::PermanentDiedThisTurn);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "kicked"]) {
            return Ok(ConditionExpr::WasKicked);
        }
        ts.restore(cp);
    }

    // `there are <N> or more <flt> in <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are"]) {
            let min_count = ts
                .next_number(false)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(1);
            let _ = ts.eat_phrase(&["or", "more"]);
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                let zone = if let Some(z) = flt.zones.pop() {
                    Some(z)
                } else if ts.eat_word("in") {
                    try_parse(ts, parse_zone_ref).ok()
                } else {
                    None
                };
                if let Some(zone) = zone {
                    return Ok(ConditionExpr::ZoneHasCount {
                        filter: Box::new(flt),
                        zone,
                        min_count,
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `your life total is <N> or less/more`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["your", "life", "total", "is"])
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            let op = if ts.eat_phrase(&["or", "less"]) {
                CompareOp::Le
            } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
                CompareOp::Ge
            } else {
                CompareOp::Eq
            };
            return Ok(ConditionExpr::LifeCompare {
                player: PlayerRef::You,
                op,
                value: Box::new(val),
            });
        }
        ts.restore(cp);
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

    // Player-headed conditions (`you control ...`, `you've gained life this turn`, `an opponent has cast a ... spell this turn`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("control") || ts.eat_word("controls") {
                if player == PlayerRef::You && ts.eat_tilde() {
                    return Ok(ConditionExpr::ControlsSelf);
                }
                let (min_count, is_fewer) = if let Some(n) = ts.next_number(false) {
                    let fewer = ts.eat_phrase(&["or", "fewer"]);
                    if !fewer {
                        let _ = ts.eat_phrase(&["or", "more"]);
                    }
                    (u16::try_from(n.max(1)).unwrap_or(1), fewer)
                } else {
                    let _ = ts.eat_word("a") || ts.eat_word("an");
                    (1, false)
                };
                let filter = parse_object_filter(ts)?;
                if is_fewer {
                    return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: min_count.saturating_add(1),
                    })));
                }
                return Ok(ConditionExpr::Controls {
                    player,
                    filter: Box::new(filter),
                    min_count,
                });
            }
            if ts.eat_phrase(&["don't", "control"]) || ts.eat_phrase(&["doesn't", "control"]) {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                let filter = parse_object_filter(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                    player,
                    filter: Box::new(filter),
                    min_count: 1,
                })));
            }
            if ts.eat_phrase(&["has", "cast"]) || ts.eat_word("cast") {
                let filter = opt(ts, parse_object_filter).map(Box::new);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::SpellCast,
                        filter,
                        min_count: 1,
                    });
                }
            }
            if (ts.eat_phrase(&["gained", "life"]) || ts.eat_phrase(&["has", "gained", "life"]))
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player,
                    event: TurnEventKind::LifeGained,
                    filter: None,
                    min_count: 1,
                });
            }
            if (ts.eat_phrase(&["lost", "life"]) || ts.eat_phrase(&["has", "lost", "life"]))
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player,
                    event: TurnEventKind::LifeLost,
                    filter: None,
                    min_count: 1,
                });
            }
        }
        ts.restore(cp);
    }

    // `you've gained [N or more] life this turn`
    if ts.eat_phrase(&["you've", "gained"]) {
        let min_count = ts
            .next_number(false)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(1);
        let _ = ts.eat_phrase(&["or", "more"]);
        if ts.eat_phrase(&["life", "this", "turn"]) {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::LifeGained,
                filter: None,
                min_count,
            });
        }
    }

    // Subject-headed conditions (`~ is untapped`, `<subj> has/doesn't have <kw>`, `it's a <filter>`)
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
            let negated = ts.eat_word("isn't")
                || ts.eat_phrase(&["is", "not"])
                || ts.eat_word("aren't")
                || ts.eat_phrase(&["are", "not"]);
            let positive = !negated && (ts.eat_word("is") || ts.eat_word("are"));
            if negated || positive {
                if let Some(state) = parse_object_state(ts) {
                    let base = ConditionExpr::SubjectState {
                        subject: Box::new(subj),
                        state,
                    };
                    return Ok(if negated {
                        ConditionExpr::Not(Box::new(base))
                    } else {
                        base
                    });
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    let base = ConditionExpr::SubjectMatches {
                        subject: Box::new(subj),
                        filter: Box::new(flt),
                    };
                    return Ok(if negated {
                        ConditionExpr::Not(Box::new(base))
                    } else {
                        base
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `it's a <filter>` / `it's tapped` / `it's untapped`
    if ts.eat_word("it's") {
        let negated = ts.eat_word("not");
        if let Some(state) = parse_object_state(ts) {
            let base = ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                state,
            };
            return Ok(if negated {
                ConditionExpr::Not(Box::new(base))
            } else {
                base
            });
        }
        if let Ok(flt) = try_parse(ts, parse_object_filter) {
            let base = ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                filter: Box::new(flt),
            };
            return Ok(if negated {
                ConditionExpr::Not(Box::new(base))
            } else {
                base
            });
        }
    }

    Err(ts.err("expected boolean condition expression"))
}

fn parse_object_state(ts: &mut TokenStream<'_>) -> Option<ObjectStateKind> {
    if ts.eat_word("tapped") {
        Some(ObjectStateKind::Tapped)
    } else if ts.eat_word("untapped") {
        Some(ObjectStateKind::Untapped)
    } else if ts.eat_word("attacking") {
        Some(ObjectStateKind::Attacking)
    } else if ts.eat_word("blocking") {
        Some(ObjectStateKind::Blocking)
    } else if ts.eat_word("blocked") {
        Some(ObjectStateKind::Blocked)
    } else if ts.eat_word("unblocked") {
        Some(ObjectStateKind::Unblocked)
    } else if ts.eat_word("equipped") {
        Some(ObjectStateKind::Equipped)
    } else if ts.eat_word("enchanted") {
        Some(ObjectStateKind::Enchanted)
    } else if ts.eat_word("modified") {
        Some(ObjectStateKind::Modified)
    } else if ts.eat_phrase(&["face", "down"]) || ts.eat_word("face-down") {
        Some(ObjectStateKind::FaceDown)
    } else {
        None
    }
}
