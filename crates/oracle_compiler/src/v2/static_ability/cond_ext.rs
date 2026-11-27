//! Extended static-ability condition and scaling-expression parsers for `v2`.
//!
//! Wraps [`crate::v2::condition::parse_condition`] and [`crate::v2::value::parse_for_each_expr`]
//! with strictly-typed static-clause conditions and `for each` scales that faithfully match
//! existing `v2::ir` variants without proxy-enum substitution.

use crate::ast::{CompareOp, ControllerRelation, StatKind};
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, FilterFlags, ObjectFilter, ObjectStateKind,
    PlayerConditionKind, PlayerRef, SharedProperty, SubjectConditionKind, SubjectRef,
    TurnEventKind, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Parse a boolean condition for static/replacement clauses: tries the core condition parser
/// first, then falls back to strictly-typed static-specific boolean condition forms.
pub(crate) fn parse_static_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let cp = ts.checkpoint();
    if let Ok(c) = try_parse(ts, parse_condition) {
        let core_cp = ts.checkpoint();
        let core_end = ts.pos;
        // Check if a static-specific fallback consumes strictly MORE tokens than the core parse
        // (e.g. `~'s power is negative`, `~ is enchanted, equipped, and has a counter on it`,
        // `you control a permanent named ... or ...`).
        let should_probe_longer = matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Comma)
        ) || ts.peek_word().is_some_and(|w| {
            matches!(
                w.to_ascii_lowercase().as_str(),
                "'s" | "player" | "this" | "with" | "of" | "by" | "or" | "and"
            )
        });
        if should_probe_longer {
            ts.restore(cp);
            if let Ok(fb) = try_parse(ts, parse_static_condition_fallback)
                && ts.pos > core_end
            {
                return Ok(fb);
            }
            ts.restore(core_cp);
        }
        // Ensure we didn't leave a dangling `and ...` / `or ...` that belongs to a static form
        let tail_cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(rhs) = try_parse(ts, parse_static_condition_fallback)
        {
            return Ok(ConditionExpr::And(Box::new(c), Box::new(rhs)));
        }
        ts.restore(tail_cp);
        if ts.eat_word("or")
            && let Ok(rhs) = try_parse(ts, parse_static_condition_fallback)
        {
            return Ok(ConditionExpr::Or(Box::new(c), Box::new(rhs)));
        }
        ts.restore(tail_cp);
        return Ok(c);
    }
    ts.restore(cp);
    parse_static_condition_fallback(ts)
}

fn parse_static_condition_fallback(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let cp = ts.checkpoint();

    // `you've visited an Attraction this turn`
    if ts.eat_phrase(&["you've", "visited", "an", "attraction", "this", "turn"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::VisitedAttractionsThisTurn),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }

    // `you've played a land this turn`
    if ts.eat_phrase(&["you've", "played", "a", "land", "this", "turn"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::PlayedLandThisTurn,
        });
    }

    // `~'s power is negative`
    if ts.eat_tilde() && ts.eat_phrase(&["'s", "power", "is", "negative"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::SelfObject),
                stat: StatKind::Power,
            }),
            op: CompareOp::Lt,
            rhs: Box::new(ValueExpr::Const(0)),
        });
    }
    ts.restore(cp);

    // `~ is enchanted, equipped, and has a counter on it`
    if (ts.eat_tilde() || ts.eat_word("it"))
        && ts.eat_phrase(&["is", "enchanted"])
        && ts.eat_comma()
        && ts.eat_word("equipped")
        && ts.eat_comma()
        && ts.eat_phrase(&["and", "has", "a", "counter", "on", "it"])
    {
        let s = SubjectRef::SelfObject;
        return Ok(ConditionExpr::And(
            Box::new(ConditionExpr::And(
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(s.clone()),
                    state: ObjectStateKind::Enchanted,
                }),
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(s.clone()),
                    state: ObjectStateKind::Equipped,
                }),
            )),
            Box::new(ConditionExpr::SubjectCheck {
                subject: Box::new(s),
                kind: SubjectConditionKind::HasCounters(None),
            }),
        ));
    }
    ts.restore(cp);

    // `you have max speed`
    if ts.eat_phrase(&["you", "have", "max", "speed"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Speed(PlayerRef::You)),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(4)),
        });
    }

    // `an opponent has been dealt damage this turn`
    if ts.eat_phrase(&[
        "an", "opponent", "has", "been", "dealt", "damage", "this", "turn",
    ]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::DamageDealtToOpponentsThisTurn {
                noncombat_only: false,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }

    // `you control more <flt> than defending player`
    if ts.eat_phrase(&["you", "control", "more"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["than", "defending", "player"])
    {
        return Ok(ConditionExpr::ControlsMoreThan {
            player: PlayerRef::You,
            other: PlayerRef::DefendingPlayer,
            filter: Box::new(flt),
        });
    }
    ts.restore(cp);

    // `<N> or more colors of mana were spent to cast it/~`
    if let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&[
            "or", "more", "colors", "of", "mana", "were", "spent", "to", "cast",
        ])
        && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::ColorsOfManaSpent),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);

    // `<N> or more <flt> are on the battlefield`
    if let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&["or", "more"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["are", "on", "the", "battlefield"])
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(flt))),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);

    // `no permanents named ~ are on the battlefield`
    if ts.eat_phrase(&["no", "permanents", "named"])
        && ts.eat_tilde()
        && ts.eat_phrase(&["are", "on", "the", "battlefield"])
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                any_permanent: true,
                predicates: vec![crate::v2::ir::FilterPredicate::NamedSelf],
                ..Default::default()
            }))),
            op: CompareOp::Eq,
            rhs: Box::new(ValueExpr::Const(0)),
        });
    }
    ts.restore(cp);

    // `there's another <flt> on the battlefield`
    if ts.eat_phrase(&["there's", "another"])
        && let Ok(mut flt) = try_parse(ts, parse_object_filter)
    {
        let _ = ts.eat_phrase(&["on", "the", "battlefield"]);
        flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(flt))),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }
    ts.restore(cp);

    // `it's bargained` / `it was bargained` / `it's cast using teamwork`
    if ts.eat_any_phrase(&[&["it's", "bargained"], &["it", "was", "bargained"]]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::WasBargained));
    }
    if ts.eat_phrase(&["it's", "cast", "using", "teamwork"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::CastUsingTeamwork));
    }

    // `creatures you control have total toughness/power <N> or greater`
    if let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["have", "total"])
    {
        let stat = if ts.eat_word("toughness") {
            Some(StatKind::Toughness)
        } else if ts.eat_word("power") {
            Some(StatKind::Power)
        } else {
            None
        };
        if let Some(stat) = stat
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "greater"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::AggregateStat {
                    op: crate::v2::ir::AggregateOp::Sum,
                    stat,
                    filter: Box::new(flt),
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
    }
    ts.restore(cp);

    // `your life total is at least <N> greater than your starting life total`
    if ts.eat_phrase(&["your", "life", "total", "is", "at", "least"])
        && let Some(margin) = ts.next_number(false)
        && ts.eat_phrase(&["greater", "than", "your", "starting", "life", "total"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::LifeAboveStarting { margin },
        });
    }
    ts.restore(cp);

    // `<flt> died this turn`
    if let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["died", "this", "turn"])
    {
        return Ok(ConditionExpr::PlayerTurnEvent {
            player: PlayerRef::EachPlayer,
            event: TurnEventKind::CreatureDied,
            filter: Some(Box::new(flt)),
            min_count: 1,
        });
    }
    ts.restore(cp);

    // `they [all] share a color`
    if ts.eat_word("they") {
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["share", "a", "color"]) {
            return Ok(ConditionExpr::SharesTypeWith {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
                other: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
                kind: SharedProperty::Color,
            });
        }
    }
    ts.restore(cp);

    // `you control a permanent/creature named <Name> [or a permanent/creature named <Name2>]`
    if ts.eat_any_phrase(&[
        &["you", "control", "a", "permanent", "named"],
        &["you", "control", "a", "creature", "named"],
    ]) && let Some(cond) = parse_named_permanent_check_tail(ts)
    {
        return Ok(cond);
    }
    ts.restore(cp);

    Err(ts.err("expected boolean condition expression"))
}

fn parse_named_permanent_check_tail(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
    let mut name = Vec::new();
    while name.len() < 8 {
        let step_cp = ts.checkpoint();
        if ts.eat_comma() {
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(crate::v2::lexer::TokenKind::Tilde)
            ) || ts
                .peek_word()
                .is_some_and(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
            {
                continue;
            }
            ts.restore(step_cp);
            break;
        }
        if ts.eat_tilde() {
            let _ = ts.eat_word("'s");
            name.push("~".to_string());
            continue;
        }
        if let Some(w) = ts.peek_word()
            && (w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                || matches!(w.to_ascii_lowercase().as_str(), "of" | "the" | "in"))
        {
            ts.next_word();
            name.push(w.to_string());
        } else {
            break;
        }
    }
    if name.is_empty() {
        return None;
    }
    let first = ConditionExpr::Controls {
        player: PlayerRef::You,
        filter: Box::new(ObjectFilter {
            any_permanent: true,
            predicates: vec![crate::v2::ir::FilterPredicate::Named(name.join(" "))],
            ..Default::default()
        }),
        min_count: 1,
    };
    let ocp = ts.checkpoint();
    if (ts.eat_phrase(&["or", "a", "permanent", "named"])
        || ts.eat_phrase(&["or", "a", "creature", "named"]))
        && let Some(second) = parse_named_permanent_check_tail(ts)
    {
        return Some(ConditionExpr::Or(Box::new(first), Box::new(second)));
    }
    ts.restore(ocp);
    Some(first)
}

/// Parse a `for each ...` scaling expression for static abilities and cost modifiers.
pub(crate) fn parse_static_scale_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["for", "each"]) {
        // `for each other spell that player has cast this turn`
        if ts.eat_phrase(&[
            "other", "spell", "that", "player", "has", "cast", "this", "turn",
        ]) {
            return Ok(ValueExpr::EventCountThisTurn {
                event: TurnEventKind::SpellCast,
                player: Some(PlayerRef::ThatPlayer),
                exclude_first: true,
            });
        }
        // `for each <kind> counter on the creature it targets`
        {
            let kcp = ts.checkpoint();
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_phrase(&["counter", "on", "the", "creature", "it", "targets"])
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                    kind,
                });
            }
            ts.restore(kcp);
        }
        // `for each <flt> you already control`
        {
            let acp = ts.checkpoint();
            if let Ok(mut flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["you", "already", "control"])
            {
                flt.controller = Some(ControllerRelation::You);
                return Ok(ValueExpr::Count(Box::new(flt)));
            }
            ts.restore(acp);
        }
    }
    ts.restore(cp);
    parse_for_each_expr(ts)
}

/// Parse a static value expression for dynamic P/T and hand-size rules.
pub(crate) fn parse_static_value_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    parse_value_expr(ts)
}
