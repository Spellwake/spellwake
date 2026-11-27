//! Additional compositional [`ConditionExpr`] grammar families for `v2`.

use crate::ast::{CardTypeMask, CompareOp, PlayerStatKind, StatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, FilterFlags, FilterPredicate, ObjectFilter,
    PlayerConditionKind, PlayerRef, PriorActionKind, SubjectConditionKind, SubjectRef,
    TurnEventKind, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, try_parse};

use super::{
    cmp_cond, eat_suffix_cmp_op, eat_table, not_cond, obj_stat_val, player_check, player_stat_val,
    prior_event,
};

/// Parse elided right-hand conjuncts following `and` in compound conditions.
pub(super) fn try_parse_elided_and_rhs(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["have", "no", "cards", "in", "hand"]) {
        return Some(cmp_cond(
            player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
            CompareOp::Eq,
            ValueExpr::Const(0),
        ));
    }
    if ts.eat_any_word(&["has", "have"])
        && let Some(n) = ts.next_number(false)
    {
        let op = eat_suffix_cmp_op(ts, true, true);
        if ts.eat_any_phrase(&[&["cards", "in", "hand"], &["card", "in", "hand"]]) {
            return Some(cmp_cond(
                player_stat_val(PlayerRef::ThatPlayer, PlayerStatKind::CardsInHand),
                op,
                ValueExpr::Const(n),
            ));
        }
    }
    ts.restore(cp);
    None
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_extended_extra(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // 1. Life-rank, exact hand-size disjunction, and prior-action checks
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "have", "the", "most", "life"])
            && ts.eat_any_phrase(&[
                &["or", "are", "tied", "for", "the", "most", "life"],
                &["or", "are", "tied", "for", "most", "life"],
            ])
        {
            return Ok(player_check(
                PlayerRef::You,
                PlayerConditionKind::NoOpponentHasMoreLifeThan,
            ));
        }
        ts.restore(lcp);
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&["they", "have", "exactly"])
            && let Some(n1) = ts.next_number(false)
            && ts.eat_phrase(&["or", "exactly"])
            && let Some(n2) = ts.next_number(false)
            && ts.eat_phrase(&["cards", "in", "hand"])
        {
            return Ok(ConditionExpr::Or(
                Box::new(cmp_cond(
                    player_stat_val(PlayerRef::ThatPlayer, PlayerStatKind::CardsInHand),
                    CompareOp::Eq,
                    ValueExpr::Const(n1),
                )),
                Box::new(cmp_cond(
                    player_stat_val(PlayerRef::ThatPlayer, PlayerStatKind::CardsInHand),
                    CompareOp::Eq,
                    ValueExpr::Const(n2),
                )),
            ));
        }
        ts.restore(hcp);
    }

    // 2. Additional-cost and colored-mana-spent conditions
    if ts.eat_any_phrase(&[
        &["her", "additional", "cost", "was", "paid"],
        &["his", "additional", "cost", "was", "paid"],
        &["its", "additional", "cost", "was", "paid"],
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
    }
    {
        let ccp = ts.checkpoint();
        let exact = ts.eat_word("exactly");
        if let Some(n) = ts.next_number(false) {
            let or_more = !exact && ts.eat_phrase(&["or", "more"]);
            if (exact || or_more)
                && ts.eat_phrase(&["colors", "of", "mana", "were", "spent", "to"])
                && ts.eat_any_phrase(&[&["activate", "this", "ability"], &["cast", "it"]])
            {
                return Ok(cmp_cond(
                    ValueExpr::ColorsOfManaSpent,
                    if exact { CompareOp::Eq } else { CompareOp::Ge },
                    ValueExpr::Const(n),
                ));
            }
        }
        ts.restore(ccp);
    }

    // 3. Damage-source and group-state checks
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["any", "of", "that", "damage", "was", "dealt", "by"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::DealtDamageThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(ccp);
    }
    if ts.eat_phrase(&["any", "of", "them", "are", "suspected"]) {
        return Ok(ConditionExpr::SubjectMatches {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            filter: Box::new(ObjectFilter {
                flags: FilterFlags::SUSPECTED,
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&[
        "the", "last", "card", "exiled", "isn't", "a", "land", "card",
    ]) {
        return Ok(not_cond(ConditionExpr::SubjectMatches {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::TheExiledCard)),
            filter: Box::new(ObjectFilter {
                any_types: CardTypeMask::LAND,
                ..Default::default()
            }),
        }));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&["that", "permanent", "or", "card", "has"])
            && ts.eat_any_word(&["a", "an"])
            && let Ok(ck) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counter", "on", "it"])
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatPermanent)),
                kind: SubjectConditionKind::HasCounters(Some(ck)),
            });
        }
        ts.restore(hcp);
        if ts.eat_phrase(&["the", "exploited", "creature", "had", "power"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_any_phrase(&[&["or", "greater"], &["or", "more"]])
        {
            return Ok(cmp_cond(
                obj_stat_val(
                    SubjectRef::Anaphora(AnaphoraRef::SacrificedObject),
                    StatKind::Power,
                ),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(hcp);
    }

    // 4. Subject-headed and compositional predicates
    if ts.eat_phrase(&["you", "revealed", "it", "this", "way"]) {
        return Ok(prior_event(PriorActionKind::RevealedThisWay));
    }
    {
        let cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["blocked", "or", "was", "blocked", "by"])
                && let Ok(other) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ConditionExpr::SubjectCombatWith {
                    subject: Box::new(subj),
                    other: Box::new(other),
                });
            }
            if ts.eat_any_phrase(&[&["was", "all", "colors"], &["is", "all", "colors"]]) {
                return Ok(cmp_cond(
                    ValueExpr::ColorCount(Box::new(subj)),
                    CompareOp::Ge,
                    ValueExpr::Const(5),
                ));
            }
            if ts.eat_any_phrase(&[
                &["is", "the", "chosen", "type"],
                &["was", "the", "chosen", "type"],
            ]) {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::OfChosenCreatureType],
                        ..Default::default()
                    }),
                });
            }
        }
        ts.restore(cp);
        // `<flt1> or <flt2> (was|were|is|are) (destroyed|exiled) this way`
        if let Ok(f1) = try_parse(ts, parse_object_filter)
            && ts.eat_word("or")
            && let Ok(f2) = try_parse(ts, parse_object_filter)
            && ts.eat_any_word(&["was", "were", "is", "are"])
        {
            const OR_ACTS: &[(&[&str], PriorActionKind)] = &[
                (
                    &["destroyed", "this", "way"],
                    PriorActionKind::DestroyedThisWay,
                ),
                (&["exiled", "this", "way"], PriorActionKind::ExiledThisWay),
            ];
            if let Some(action) = eat_table(ts, OR_ACTS) {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::AnyOf(vec![f1, f2])],
                        ..Default::default()
                    }),
                });
            }
        }
        ts.restore(cp);
        // `<N> or more <flt> (were|was) returned this way`
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_word(&["were", "was"])
            && ts.eat_phrase(&["returned", "this", "way"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ReturnedThisWay,
            ));
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(flt)),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(cp);
        // `you searched for <flt>`
        if ts.eat_phrase(&["you", "searched", "for"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::SearchedThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(cp);
        // `you've cast <flt1> and <flt2> this turn`
        if ts.eat_any_phrase(&[&["you've", "cast"], &["you", "cast"]])
            && let Ok(f1) = try_parse(ts, parse_object_filter)
            && ts.eat_word("and")
            && let Ok(f2) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::And(
                Box::new(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter: Some(Box::new(f1)),
                    min_count: 1,
                }),
                Box::new(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter: Some(Box::new(f2)),
                    min_count: 1,
                }),
            ));
        }
        ts.restore(cp);
        // `<player> controls more <flt> than each other player` / `has more life than each other player`
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            let p_after = ts.checkpoint();
            if ts.eat_any_phrase(&[&["controls", "more"], &["control", "more"]])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["than", "each", "other", "player"])
            {
                return Ok(ConditionExpr::ControlsMoreThan {
                    player,
                    other: PlayerRef::EachOtherPlayer,
                    filter: Box::new(flt),
                });
            }
            ts.restore(p_after);
            if ts.eat_any_phrase(&[
                &["has", "more", "life", "than", "each", "other", "player"],
                &["have", "more", "life", "than", "each", "other", "player"],
            ]) {
                return Ok(cmp_cond(
                    player_stat_val(player, PlayerStatKind::LifeTotal),
                    CompareOp::Gt,
                    player_stat_val(PlayerRef::EachOtherPlayer, PlayerStatKind::LifeTotal),
                ));
            }
        }
        ts.restore(cp);
        // `<N> or more <flt> have (a|an) <kind> counter on them`
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_word(&["have", "has"])
            && ts.eat_any_word(&["a", "an"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counter", "counters"])
            && ts.eat_any_phrase(&[&["on", "them"], &["on", "it"]])
        {
            flt.predicates.push(FilterPredicate::HasCounterKind(kind));
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(flt)),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(cp);
    }

    Err(ts.err("expected boolean condition expression"))
}
