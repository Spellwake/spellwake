//! Extended zone-operation and counter-movement verb helpers for `v2`.
//!
//! Keeps `clause/zone_verbs.rs` well below the 2,500 LOC ceiling while adding strictly-typed
//! parsers for `put the rest/all/those/top ...`, `put <subj> <dest1> or <dest2>`,
//! `sacrifice half the <flt>`, `move all/any number of counters ... onto a second target ...`,
//! `exile the bottom/top ...`, and `return the top <flt> of <zone>`.

use crate::ast::Rounding;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_subject_ref, parse_zone_dest, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ChoiceKind, CountSpec, CounterKind, Duration, Effect, EntryModifiers,
    FilterPredicate, ObjectFilter, ObjectMutation, PlayerRef, PriorActionKind, SubjectRef,
    TargetCount, ValueExpr, ZoneDest, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;

use super::{
    all_zone_cards_subject, counted_zone_op, eat_rounding, exact_count, exile_zone_op,
    move_zone_op, mutate_obj, prompt_choice, shuffled_zone_dest, simple_zone_dest, simple_zone_op,
    zone_cards_subject,
};

/// Parse trailing `under <owner> control` qualifiers that `parse_zone_dest` may leave unconsumed.
pub(super) fn parse_trailing_under_control(ts: &mut TokenStream<'_>, dest: &mut ZoneDest) {
    const TABLE: &[(&[&str], ZoneOwner)] = &[
        (&["under", "their", "control"], ZoneOwner::ThatPlayer),
        (
            &["under", "that", "player's", "control"],
            ZoneOwner::ThatPlayer,
        ),
        (
            &["under", "an", "opponent's", "control"],
            ZoneOwner::Opponent,
        ),
        (
            &["under", "target", "player's", "control"],
            ZoneOwner::TargetPlayer,
        ),
        (
            &["under", "target", "opponent's", "control"],
            ZoneOwner::TargetOpponent,
        ),
        (
            &["under", "the", "control", "of", "that", "card's", "owner"],
            ZoneOwner::ItsOwner,
        ),
        (&["under", "its", "owner's", "control"], ZoneOwner::ItsOwner),
        (
            &["under", "their", "owners'", "control"],
            ZoneOwner::TheirOwners,
        ),
        (&["under", "your", "control"], ZoneOwner::You),
    ];
    if let Some(owner) = TABLE
        .iter()
        .find_map(|&(phrase, owner)| ts.eat_phrase(phrase).then_some(owner))
    {
        dest.under_control = Some(owner);
    }
}

/// Parse extended `put(s)` clauses before general `put <subj> <dest>` dispatch.
pub(super) fn try_parse_put_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 1. `put(s) those counters on <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["those", "counters", "on"])
            && let Ok(to) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    SubjectRef::SelfObject,
                    ObjectMutation::MoveCounters {
                        from: Box::new(SubjectRef::SelfObject),
                        to: Box::new(to),
                        kind: CounterKind::Any,
                        count: ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
                    },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. `put(s) that number of <kind> counters on <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["that", "number", "of"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters {
                        kind,
                        count: ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
                    },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 3. `put(s) the rest of [the [exiled|revealed] | those [exiled|revealed]] cards [revealed/exiled this way] <dest>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "rest", "of"])
            && ts.eat_any_phrase(&[
                &["the", "cards", "revealed", "this", "way"],
                &["the", "cards", "exiled", "this", "way"],
                &["the", "revealed", "cards"],
                &["the", "exiled", "cards"],
                &["those", "revealed", "cards"],
                &["those", "exiled", "cards"],
                &["those", "cards"],
            ])
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            return Some(Ok((
                vec![move_zone_op(
                    actor,
                    SubjectRef::Anaphora(AnaphoraRef::TheRest),
                    Vec::new(),
                    dest,
                    modifiers,
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 4. `put(s) all [the] [revealed | nonland | land] cards [...] <dest> [and the rest <dest2>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("all") {
            let _ = ts.eat_word("the");
            let mut from = Vec::new();
            let subj_opt = if ts.eat_phrase(&["revealed", "cards", "not", "cast", "this", "way"]) {
                Some(SubjectRef::All(Box::new(ObjectFilter {
                    any_card: true,
                    predicates: vec![FilterPredicate::AffectedByPriorAction(
                        PriorActionKind::RevealedThisWay,
                    )],
                    ..Default::default()
                })))
            } else if ts.eat_any_phrase(&[
                &["cards", "from", "the", "pile", "of", "your", "choice"],
                &["cards", "from", "the", "pile", "of", "their", "choice"],
            ]) {
                Some(SubjectRef::Anaphora(AnaphoraRef::OnePile))
            } else if ts.eat_phrase(&["nonland", "cards", "chosen", "this", "way"]) {
                Some(SubjectRef::All(Box::new(ObjectFilter {
                    any_card: true,
                    forbidden_types: crate::ast::CardTypeMask::LAND,
                    ..Default::default()
                })))
            } else if ts.eat_phrase(&["land", "cards", "chosen", "this", "way"]) {
                Some(SubjectRef::All(Box::new(ObjectFilter {
                    any_types: crate::ast::CardTypeMask::LAND,
                    ..Default::default()
                })))
            } else if ts.eat_phrase(&["cards", "from"])
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
            {
                from.push(zr);
                Some(all_zone_cards_subject(zr))
            } else {
                None
            };
            if let Some(subject) = subj_opt
                && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut dest);
                let mut out = vec![move_zone_op(actor, subject, from, dest, modifiers, None)];
                append_optional_and_the_rest(ts, actor, &mut out);
                return Some(Ok((out, ())));
            }
        }
        ts.restore(cp);
    }

    // 5. `put(s) those <flt> <dest> [and the rest <dest2>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("those")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            let mut out = vec![move_zone_op(
                actor,
                SubjectRef::Group(Box::new(flt)),
                Vec::new(),
                dest,
                modifiers,
                None,
            )];
            append_optional_and_the_rest(ts, actor, &mut out);
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
    }

    // 6. `put(s) the top <flt> of <zone> <dest>` AND `put(s) the top <val> cards of <zone> <dest>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top"]) {
            let num_cp = ts.checkpoint();
            if let Ok(cnt) = try_parse(ts, parse_value_expr)
                && ts.eat_any_word(&["cards", "card"])
                && ts.eat_word("of")
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
                && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut dest);
                return Some(Ok((
                    vec![move_zone_op(
                        actor,
                        zone_cards_subject(zr),
                        vec![zr],
                        dest,
                        modifiers,
                        Some(exact_count(cnt)),
                    )],
                    (),
                )));
            }
            ts.restore(num_cp);
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_word("of")
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
                && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut dest);
                return Some(Ok((
                    vec![move_zone_op(
                        actor,
                        SubjectRef::Group(Box::new(flt)),
                        vec![zr],
                        dest,
                        modifiers,
                        Some(exact_count(ValueExpr::Const(1))),
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 7. `put(s) that pile <dest> [and the other <dest2>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["that", "pile"])
            && let Ok((mut d1, m1)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut d1);
            let mut out = vec![move_zone_op(
                actor,
                SubjectRef::Anaphora(AnaphoraRef::OnePile),
                Vec::new(),
                d1,
                m1,
                None,
            )];
            let ocp = ts.checkpoint();
            if ts.eat_any_phrase(&[&["and", "the", "other", "pile"], &["and", "the", "other"]])
                && let Ok((mut d2, m2)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut d2);
                out.push(move_zone_op(
                    actor,
                    SubjectRef::Anaphora(AnaphoraRef::OtherPile),
                    Vec::new(),
                    d2,
                    m2,
                    None,
                ));
            } else {
                ts.restore(ocp);
            }
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
    }

    // 8. `put(s) a card from the chosen pile <dest>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "card", "from", "the", "chosen", "pile"])
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            return Some(Ok((
                vec![move_zone_op(
                    actor,
                    SubjectRef::Anaphora(AnaphoraRef::OnePile),
                    Vec::new(),
                    dest,
                    modifiers,
                    Some(exact_count(ValueExpr::Const(1))),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 9. `put(s) the same number (and kind of counters | of each kind of counter) on <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &[
                "the", "same", "number", "and", "kind", "of", "counters", "on",
            ],
            &[
                "the", "same", "number", "of", "each", "kind", "of", "counter", "on",
            ],
        ]) && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
                    },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 10. Extended counter-placement `put(s)` forms
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["an", "additional", "counter", "of", "that", "kind", "on"],
            &["a", "counter", "of", "that", "kind", "on"],
        ]) {
            let subj = if ts.eat_any_phrase(&[
                &["either", "of", "those", "tokens"],
                &["either", "of", "them"],
            ]) {
                SubjectRef::Anaphora(AnaphoraRef::Them)
            } else if let Ok(s) = try_parse(ts, parse_subject_ref) {
                s
            } else {
                ts.restore(cp);
                return None;
            };
            return Some(Ok((
                vec![mutate_obj(
                    subj,
                    ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::Const(1),
                    },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_any_phrase(&[&["your", "choice", "of"], &["their", "choice", "of"]])
            && ts.eat_any_word(&["a", "an"])
            && let Ok(k1) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counter", "or", "a", "counter", "of", "that", "kind", "on"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![prompt_choice(
                    actor,
                    ChoiceKind::ChooseBranch(vec![
                        vec![mutate_obj(
                            subject.clone(),
                            ObjectMutation::PutCounters {
                                kind: k1,
                                count: ValueExpr::Const(1),
                            },
                            Duration::Instant,
                        )],
                        vec![mutate_obj(
                            subject,
                            ObjectMutation::PutCounters {
                                kind: CounterKind::Any,
                                count: ValueExpr::Const(1),
                            },
                            Duration::Instant,
                        )],
                    ]),
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "one", "of", "each", "of", "those", "kinds", "of", "counters", "on",
        ]) && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::Const(1),
                    },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["an", "additional"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counter", "counters"])
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters {
                        kind,
                        count: ValueExpr::Const(1),
                    },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["up", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters { kind, count },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
        if let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["equal", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters { kind, count },
                    Duration::Instant,
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["that", "many"])
            && let Ok(k1) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counters", "or"])
            && let Ok(k2) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counters", "on"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let cnt = ValueExpr::PriorActionCount(PriorActionKind::ThatMany);
            return Some(Ok((
                vec![prompt_choice(
                    actor,
                    ChoiceKind::ChooseBranch(vec![
                        vec![mutate_obj(
                            subject.clone(),
                            ObjectMutation::PutCounters {
                                kind: k1,
                                count: cnt.clone(),
                            },
                            Duration::Instant,
                        )],
                        vec![mutate_obj(
                            subject,
                            ObjectMutation::PutCounters {
                                kind: k2,
                                count: cnt,
                            },
                            Duration::Instant,
                        )],
                    ]),
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_any_word(&["a", "an"])
            && let Ok(first_k) = try_parse(ts, parse_counter_kind)
        {
            if ts.eat_phrase(&["counter", "on", "either", "of", "them"]) {
                return Some(Ok((
                    vec![mutate_obj(
                        SubjectRef::Anaphora(AnaphoraRef::Them),
                        ObjectMutation::PutCounters {
                            kind: first_k,
                            count: ValueExpr::Const(1),
                        },
                        Duration::Instant,
                    )],
                    (),
                )));
            }
            let mut kinds = vec![first_k];
            while ts.eat_comma() || ts.eat_word("or") {
                let _ = ts.eat_word("or");
                if let Ok(k) = try_parse(ts, parse_counter_kind) {
                    kinds.push(k);
                } else {
                    break;
                }
            }
            if kinds.len() >= 2
                && ts.eat_any_word(&["counter", "counters"])
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                let branches = kinds
                    .into_iter()
                    .map(|kind| {
                        vec![mutate_obj(
                            subject.clone(),
                            ObjectMutation::PutCounters {
                                kind,
                                count: ValueExpr::Const(1),
                            },
                            Duration::Instant,
                        )]
                    })
                    .collect();
                return Some(Ok((
                    vec![prompt_choice(actor, ChoiceKind::ChooseBranch(branches))],
                    (),
                )));
            }
        }
        ts.restore(cp);
        if (ts.eat_tilde() && ts.eat_word("'s")) || ts.eat_word("its") {
            let kind = try_parse(ts, parse_counter_kind).unwrap_or(CounterKind::Any);
            if ts.eat_any_word(&["counters", "counter"])
                && ts.eat_word("on")
                && let Ok(to) = try_parse(ts, parse_subject_ref)
            {
                return Some(Ok((
                    vec![mutate_obj(
                        SubjectRef::SelfObject,
                        ObjectMutation::MoveCounters {
                            from: Box::new(SubjectRef::SelfObject),
                            to: Box::new(to),
                            kind,
                            count: ValueExpr::Var(crate::v2::ir::VarName::X),
                        },
                        Duration::Instant,
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 11. `put(s) (up to <val> | a power and toughness) [name|ability|art] sticker(s) on <subj>`
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["a", "power", "and", "toughness", "sticker", "on"])
            || (ts.eat_phrase(&["up", "to"])
                && ts.next_number(false).is_some()
                && ts.eat_any_phrase(&[
                    &["name", "stickers", "on"],
                    &["ability", "stickers", "on"],
                    &["art", "stickers", "on"],
                    &["stickers", "on"],
                ])))
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(crate::v2::ir::ObjectStateAction::PutSticker),
                    Duration::Permanent,
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["one", "of", "those", "cards", "with"]) {
            let matched = (ts.eat_phrase(&["mana", "value"])
                && ts.next_number(false).is_some()
                && ts.eat_phrase(&["or", "less"]))
                || ts.eat_phrase(&["that", "name"]);
            if matched && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest) {
                parse_trailing_under_control(ts, &mut dest);
                let _ = ts.eat_phrase(&["instead", "of", "putting", "it", "into", "your", "hand"]);
                return Some(Ok((
                    vec![move_zone_op(
                        actor,
                        SubjectRef::OneOfThem(ValueExpr::Const(1)),
                        Vec::new(),
                        dest,
                        modifiers,
                        None,
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 12. Destination-first `put(s) onto the battlefield [...] <subj>`
    {
        let cp = ts.checkpoint();
        if ts.peek_phrase(&["onto", "the", "battlefield"])
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            let _ = ts.eat_phrase(&["any", "number", "of"]);
            if let Ok(subject) = try_parse(ts, parse_subject_ref) {
                let mut from = Vec::new();
                if ts.eat_word("from")
                    && let Ok(z1) = try_parse(ts, parse_zone_ref)
                {
                    from.push(z1);
                    let zcp = ts.checkpoint();
                    if ts.eat_any_word(&["and/or", "or", "and"])
                        && let Some(wk) = ts.peek_word().map(|w| w.to_ascii_lowercase())
                        && matches!(wk.as_str(), "hand" | "graveyard" | "library")
                    {
                        ts.next_word();
                    } else {
                        ts.restore(zcp);
                    }
                }
                return Some(Ok((
                    vec![move_zone_op(actor, subject, from, dest, modifiers, None)],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 13. `put(s) any number of (them | those <flt> | <flt> [from among them]) <dest> [and the rest <dest2>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["any", "number", "of"]) {
            let subj_opt = if ts.eat_word("them") {
                Some(SubjectRef::Anaphora(AnaphoraRef::Them))
            } else if ts.eat_word("those")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                Some(SubjectRef::Group(Box::new(flt)))
            } else if let Ok(flt) = try_parse(ts, parse_object_filter) {
                let _ = ts.eat_phrase(&["from", "among", "them"]);
                Some(SubjectRef::Group(Box::new(flt)))
            } else {
                None
            };
            if let Some(subject) = subj_opt
                && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut dest);
                let mut out = vec![move_zone_op(
                    actor,
                    subject,
                    Vec::new(),
                    dest,
                    modifiers,
                    Some(CountSpec {
                        count: ValueExpr::Const(0),
                        up_to: false,
                        any_number: true,
                    }),
                )];
                append_optional_and_the_rest(ts, actor, &mut out);
                return Some(Ok((out, ())));
            }
        }
        ts.restore(cp);
    }

    // 14. `put(s) <val> cards from (among them | the top of <zone>) <dest> [and the rest <dest2>]`
    {
        let cp = ts.checkpoint();
        if let Ok(count) = try_parse(ts, parse_value_expr)
            && ts.eat_any_word(&["cards", "card"])
        {
            let mut from = Vec::new();
            let src_ok = if ts.eat_phrase(&["from", "among", "them"]) {
                true
            } else if ts.eat_phrase(&["from", "the", "top", "of"])
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
            {
                from.push(zr);
                true
            } else {
                false
            };
            if src_ok && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest) {
                parse_trailing_under_control(ts, &mut dest);
                let mut out = vec![move_zone_op(
                    actor,
                    SubjectRef::Anaphora(AnaphoraRef::Them),
                    from,
                    dest,
                    modifiers,
                    Some(exact_count(count)),
                )];
                append_optional_and_the_rest(ts, actor, &mut out);
                return Some(Ok((out, ())));
            }
        }
        ts.restore(cp);
    }

    // 15. `put(s) (the exiled cards not cast this way | ... | the bottom card of <zone>) <dest>`
    {
        let cp = ts.checkpoint();
        let mut from = Vec::new();
        let subj_opt = if ts.eat_any_phrase(&[
            &["the", "exiled", "cards", "not", "cast", "this", "way"],
            &["the", "revealed", "cards", "not", "cast", "this", "way"],
            &["the", "cards", "you", "revealed", "this", "way"],
            &["any", "of", "those", "cards", "you", "didn't", "play"],
            &["both", "cards"],
        ]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::Them))
        } else if ts.eat_phrase(&["the", "last", "chosen", "card"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatCard))
        } else if ts.eat_phrase(&["the", "bottom", "card", "of"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            from.push(zr);
            Some(zone_cards_subject(zr))
        } else {
            None
        };
        if let Some(subject) = subj_opt
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            return Some(Ok((
                vec![move_zone_op(actor, subject, from, dest, modifiers, None)],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 16. `put(s) one back and the rest <dest>` / `put(s) it back on top of <zone>` / `put(s) one of the chosen cards <d1>, ...`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "back"]) {
            let mut out = vec![simple_zone_op(
                actor,
                ZoneOpKind::Move,
                SubjectRef::OneOfThem(ValueExpr::Const(1)),
                Vec::new(),
                Some(ZoneDest {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                    library_position: Some(crate::v2::ir::LibraryPosition::Top),
                    library_order: None,
                    under_control: None,
                }),
            )];
            append_optional_and_the_rest(ts, actor, &mut out);
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["it", "back", "on", "top", "of"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::Move,
                    SubjectRef::Anaphora(AnaphoraRef::It),
                    Vec::new(),
                    Some(ZoneDest {
                        zone: zr.zone,
                        owner: zr.owner,
                        library_position: Some(crate::v2::ir::LibraryPosition::Top),
                        library_order: None,
                        under_control: None,
                    }),
                )],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["one", "of", "the", "chosen", "cards"])
            && let Ok((d1, m1)) = try_parse(ts, parse_zone_dest)
            && ts.eat_comma()
            && ts.eat_phrase(&["the", "other", "chosen", "cards"])
            && let Ok((d2, m2)) = try_parse(ts, parse_zone_dest)
            && ts.eat_comma()
        {
            let mut out = vec![
                move_zone_op(
                    actor,
                    SubjectRef::OneOfThem(ValueExpr::Const(1)),
                    Vec::new(),
                    d1,
                    m1,
                    None,
                ),
                move_zone_op(
                    actor,
                    SubjectRef::Anaphora(AnaphoraRef::Them),
                    Vec::new(),
                    d2,
                    m2,
                    None,
                ),
            ];
            append_optional_and_the_rest(ts, actor, &mut out);
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
    }

    None
}

/// Consume optional `and the rest <dest>` after a `put` move effect.
pub(super) fn append_optional_and_the_rest(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
    out: &mut Vec<Effect>,
) {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["and", "the", "rest"])
        && let Ok((mut d2, m2)) = try_parse(ts, parse_zone_dest)
    {
        parse_trailing_under_control(ts, &mut d2);
        out.push(move_zone_op(
            actor,
            SubjectRef::Anaphora(AnaphoraRef::TheRest),
            Vec::new(),
            d2,
            m2,
            None,
        ));
    } else {
        ts.restore(cp);
    }
}

/// Wrap `put <subj> <dest1> or <dest2>` into a binary choice when `or <dest2>` immediately follows.
pub(super) fn maybe_wrap_put_or_dest(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
    first_eff: Effect,
    subject: &SubjectRef,
    from: &[ZoneRef],
    put_count: &Option<CountSpec>,
) -> Vec<Effect> {
    let cp = ts.checkpoint();
    if ts.eat_word("or")
        && let Ok((mut d2, m2)) = try_parse(ts, parse_zone_dest)
    {
        parse_trailing_under_control(ts, &mut d2);
        let second_eff = move_zone_op(
            actor,
            subject.clone(),
            from.to_vec(),
            d2,
            m2,
            put_count.clone(),
        );
        let mut out = vec![prompt_choice(
            actor,
            ChoiceKind::ChooseBranch(vec![vec![first_eff], vec![second_eff]]),
        )];
        append_optional_and_the_rest(ts, actor, &mut out);
        return out;
    }
    ts.restore(cp);
    vec![first_eff]
}

/// Parse `sacrifice(s) half the <flt> [of their/your choice] [, rounded down/up]`.
pub(super) fn try_parse_sacrifice_half(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["half", "the"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        let _ = ts.eat_any_phrase(&[&["of", "their", "choice"], &["of", "your", "choice"]]);
        let rounding = eat_rounding(ts, Rounding::Down);
        let count = ValueExpr::Div {
            expr: Box::new(ValueExpr::Count(Box::new(flt.clone()))),
            denom: 2,
            rounding,
        };
        return Some(Ok((
            vec![counted_zone_op(
                actor,
                ZoneOpKind::Sacrifice,
                SubjectRef::Group(Box::new(flt)),
                vec![ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                }],
                None,
                Some(exact_count(count)),
            )],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Parse extended `move(s)` counter clauses.
pub(super) fn try_parse_move_ext(ts: &mut TokenStream<'_>) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    let count = if ts.eat_word("all") || ts.eat_phrase(&["any", "number", "of"]) {
        ValueExpr::Const(0)
    } else if ts.eat_any_word(&["a", "an"]) {
        ValueExpr::Const(1)
    } else if let Ok(v) = try_parse(ts, parse_value_expr) {
        v
    } else {
        ts.restore(cp);
        return None;
    };
    let kind = if ts.eat_any_word(&["counter", "counters"]) {
        CounterKind::Any
    } else if let Ok(ck) = try_parse(ts, parse_counter_kind)
        && ts.eat_any_word(&["counter", "counters"])
    {
        ck
    } else {
        ts.restore(cp);
        return None;
    };
    if !ts.eat_word("from") {
        ts.restore(cp);
        return None;
    }
    let _ = ts.eat_word("among");
    let Ok(from) = try_parse(ts, parse_subject_ref) else {
        ts.restore(cp);
        return None;
    };
    if !ts.eat_any_word(&["onto", "to"]) {
        ts.restore(cp);
        return None;
    }
    let to = if ts.eat_phrase(&["a", "second", "target"]) {
        let Ok(flt) = try_parse(ts, parse_object_filter) else {
            ts.restore(cp);
            return None;
        };
        SubjectRef::TargetObject {
            filter: Box::new(flt),
            count: TargetCount::Exact(1),
            another: true,
        }
    } else if let Ok(dst) = try_parse(ts, parse_subject_ref) {
        dst
    } else {
        ts.restore(cp);
        return None;
    };
    Some(Ok((
        vec![mutate_obj(
            from.clone(),
            ObjectMutation::MoveCounters {
                from: Box::new(from),
                to: Box::new(to),
                kind,
                count,
            },
            Duration::Instant,
        )],
        (),
    )))
}

/// Parse extended `exile(s)` clauses before general `exile <subj>` dispatch.
pub(super) fn try_parse_exile_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 1. `exile(s) the bottom card of <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "bottom", "card", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![exile_zone_op(
                    actor,
                    zone_cards_subject(zref),
                    vec![zref],
                    false,
                    None,
                    Some(exact_count(ValueExpr::Const(1))),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. `exile(s) the top [<val>] card(s)` WITHOUT `of <lib>` (defaults to actor's library)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top"]) {
            let cnt_opt = if ts.eat_word("card") {
                Some(ValueExpr::Const(1))
            } else if let Ok(n) = try_parse(ts, parse_value_expr)
                && ts.eat_any_word(&["cards", "card"])
            {
                Some(n)
            } else {
                None
            };
            if let Some(count) = cnt_opt
                && !ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("of"))
            {
                let zref = ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                };
                let face_down = ts.eat_phrase(&["face", "down"]);
                return Some(Ok((
                    vec![exile_zone_op(
                        actor,
                        zone_cards_subject(zref),
                        vec![zref],
                        face_down,
                        None,
                        Some(exact_count(count)),
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 3. `exile(s) cards from [the top of] <zone> equal to <val>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["cards", "from"]) {
            let _ = ts.eat_phrase(&["the", "top", "of"]);
            if let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
            {
                return Some(Ok((
                    vec![exile_zone_op(
                        actor,
                        zone_cards_subject(zref),
                        vec![zref],
                        false,
                        None,
                        Some(exact_count(count)),
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 4. `exile(s) the pile of an opponent's/your/their choice`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["the", "pile", "of", "an", "opponent's", "choice"],
            &["the", "pile", "of", "your", "choice"],
            &["the", "pile", "of", "their", "choice"],
        ]) {
            return Some(Ok((
                vec![exile_zone_op(
                    actor,
                    SubjectRef::Anaphora(AnaphoraRef::OnePile),
                    Vec::new(),
                    false,
                    None,
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 5. `exile(s) the top half of their/your library [, rounded up/down]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top", "half", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            let rounding = eat_rounding(ts, Rounding::Up);
            let count = ValueExpr::Div {
                expr: Box::new(ValueExpr::PlayerStat {
                    player: actor,
                    stat: crate::ast::PlayerStatKind::CardsInLibrary,
                }),
                denom: 2,
                rounding,
            };
            return Some(Ok((
                vec![exile_zone_op(
                    actor,
                    zone_cards_subject(zref),
                    vec![zref],
                    false,
                    None,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 6. `exile(s) all but the bottom card of <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["all", "but", "the", "bottom", "card", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![exile_zone_op(
                    actor,
                    all_zone_cards_subject(zref),
                    vec![zref],
                    false,
                    None,
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 7. `exile(s) the top <N> <flt> of <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("card") || w.eq_ignore_ascii_case("cards"))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_word("of")
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![exile_zone_op(
                    actor,
                    SubjectRef::Group(Box::new(flt)),
                    vec![zref],
                    false,
                    None,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 8. `exile(s) <val> cards from the top` (without `of <lib>`)
    {
        let cp = ts.checkpoint();
        if let Ok(count) = try_parse(ts, parse_value_expr)
            && ts.eat_any_word(&["cards", "card"])
            && ts.eat_phrase(&["from", "the", "top"])
            && !ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("of"))
        {
            let zr = ZoneRef {
                zone: ZoneKind::Library,
                owner: ZoneOwner::Default,
            };
            return Some(Ok((
                vec![exile_zone_op(
                    actor,
                    zone_cards_subject(zr),
                    vec![zr],
                    false,
                    None,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Consume optional trailing `in [a|another] face-down/face-up pile [and the rest in a face-up pile]` on `exile`.
pub(super) fn eat_exile_pile_suffix(ts: &mut TokenStream<'_>) -> bool {
    if ts.eat_any_phrase(&[
        &["in", "a", "face-down", "pile"],
        &["in", "another", "face-down", "pile"],
        &["in", "a", "face-up", "pile"],
    ]) {
        let _ = ts.eat_any_phrase(&[
            &["and", "the", "rest", "in", "a", "face-up", "pile"],
            &["and", "the", "rest", "in", "a", "face-down", "pile"],
        ]);
        return true;
    }
    false
}

/// Consume optional trailing `and the top [<val>] card(s) of <zone>` on `exile` or `reveal`.
pub(super) fn try_eat_and_top_cards(ts: &mut TokenStream<'_>) -> Option<(ZoneRef, ValueExpr)> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["and", "the", "top"]) {
        let count = if ts.eat_word("card") {
            Some(ValueExpr::Const(1))
        } else if let Ok(v) = try_parse(ts, parse_value_expr)
            && ts.eat_any_word(&["cards", "card"])
        {
            Some(v)
        } else {
            None
        };
        if let Some(cnt) = count
            && ts.eat_word("of")
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            return Some((zr, cnt));
        }
    }
    ts.restore(cp);
    None
}

/// Parse extended `discard(s)` clauses.
pub(super) fn try_parse_discard_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let default_hand = || {
        vec![ZoneRef {
            zone: ZoneKind::Hand,
            owner: ZoneOwner::Default,
        }]
    };
    let disc_op = ZoneOpKind::Discard {
        hand: false,
        random: false,
    };
    // 1. `discard(s) (half | a third of) the cards in their/your hand [, rounded up/down]`
    {
        let cp = ts.checkpoint();
        let denom = if ts.eat_phrase(&["half", "the", "cards", "in"]) {
            Some(2)
        } else if ts.eat_phrase(&["a", "third", "of", "the", "cards", "in"]) {
            Some(3)
        } else {
            None
        };
        if let Some(d) = denom
            && ts.eat_any_phrase(&[&["their", "hand"], &["your", "hand"]])
        {
            let rounding = eat_rounding(ts, Rounding::Up);
            let count = ValueExpr::Div {
                expr: Box::new(ValueExpr::PlayerStat {
                    player: actor,
                    stat: crate::ast::PlayerStatKind::CardsInHand,
                }),
                denom: d,
                rounding,
            };
            return Some(Ok((
                vec![counted_zone_op(
                    actor,
                    disc_op,
                    SubjectRef::Player(actor),
                    default_hand(),
                    None,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. `discard(s) one or more <flt>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Some(Ok((
                vec![counted_zone_op(
                    actor,
                    disc_op,
                    SubjectRef::Group(Box::new(flt)),
                    default_hand(),
                    None,
                    Some(CountSpec {
                        count: ValueExpr::Const(1),
                        up_to: false,
                        any_number: true,
                    }),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 3. `discard(s) cards equal to <val>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["cards", "equal", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
        {
            return Some(Ok((
                vec![counted_zone_op(
                    actor,
                    disc_op,
                    SubjectRef::Player(actor),
                    default_hand(),
                    None,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Parse extended `return(s)` clauses.
pub(super) fn try_parse_return_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 1. `return(s) the top <flt> of <zone> to <dest>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_word("of")
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            return Some(Ok((
                vec![move_zone_op(
                    actor,
                    SubjectRef::Group(Box::new(flt)),
                    vec![zr],
                    dest,
                    modifiers,
                    Some(exact_count(ValueExpr::Const(1))),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. `return(s) half the <flt> <dest> [, rounded up/down]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["half", "the"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            let rounding = eat_rounding(ts, Rounding::Up);
            let count = ValueExpr::Div {
                expr: Box::new(ValueExpr::Count(Box::new(flt.clone()))),
                denom: 2,
                rounding,
            };
            return Some(Ok((
                vec![move_zone_op(
                    actor,
                    SubjectRef::Group(Box::new(flt)),
                    Vec::new(),
                    dest,
                    modifiers,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 3. `return(s) <subj> under your control [with a <kind> counter on it]`
    {
        let cp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["under", "your", "control"])
        {
            let mut modifiers = EntryModifiers::default();
            if ts.eat_word("with")
                && ts.eat_any_word(&["a", "an"])
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counter", "counters"])
                && ts.eat_phrase(&["on", "it"])
            {
                modifiers.with_counter = Some((ck, 1));
            }
            return Some(Ok((
                vec![move_zone_op(
                    actor,
                    subject,
                    Vec::new(),
                    ZoneDest {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::You,
                        library_position: None,
                        library_order: None,
                        under_control: Some(ZoneOwner::You),
                    },
                    modifiers,
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 4. `return(s) (each <special_flt> | one of those <flt>) <dest>`
    {
        let cp = ts.checkpoint();
        let subj_opt = if ts.eat_phrase(&["each", "chosen"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            Some(SubjectRef::Each(Box::new(flt)))
        } else if ts.eat_phrase(&[
            "each", "card", "with", "the", "most", "votes", "or", "tied", "for", "most", "votes",
        ]) {
            Some(SubjectRef::Each(Box::new(ObjectFilter {
                any_card: true,
                ..Default::default()
            })))
        } else if ts.eat_phrase(&[
            "each", "creature", "with", "power", "of", "the", "chosen", "quality",
        ]) {
            Some(SubjectRef::Each(Box::new(ObjectFilter {
                any_types: crate::ast::CardTypeMask::CREATURE,
                ..Default::default()
            })))
        } else if ts.eat_phrase(&["each", "nonland", "permanent", "that's", "not", "historic"]) {
            Some(SubjectRef::Each(Box::new(ObjectFilter {
                any_permanent: true,
                ..Default::default()
            })))
        } else if ts.eat_phrase(&["one", "of", "those"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            Some(SubjectRef::Group(Box::new(flt)))
        } else {
            None
        };
        if let Some(subject) = subj_opt
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            return Some(Ok((
                vec![move_zone_op(
                    actor,
                    subject,
                    Vec::new(),
                    dest,
                    modifiers,
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Parse extended `shuffle(s)` clauses.
pub(super) fn try_parse_shuffle_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["that", "pile"]) {
        return Some(Ok((
            vec![simple_zone_op(
                actor,
                ZoneOpKind::Shuffle,
                SubjectRef::Anaphora(AnaphoraRef::OnePile),
                Vec::new(),
                None,
            )],
            (),
        )));
    }
    if ts.eat_phrase(&["the", "rest", "of", "that", "pile", "into"])
        && let Ok(zref) = try_parse(ts, parse_zone_ref)
    {
        return Some(Ok((
            vec![simple_zone_op(
                actor,
                ZoneOpKind::Shuffle,
                SubjectRef::Anaphora(AnaphoraRef::TheRest),
                Vec::new(),
                Some(shuffled_zone_dest(zref.zone, zref.owner)),
            )],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Parse extended `separate(s)` pile specs (`into <N> [face-down] piles`).
pub(super) fn try_parse_separate_piles_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
    subject: SubjectRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_word("into")
        && let Some(n) = ts.next_number(false).and_then(|v| u8::try_from(v).ok())
    {
        let face_down = ts.eat_word("face-down");
        if ts.eat_any_word(&["piles", "pile"]) {
            return Some(Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::SeparateIntoPiles {
                        pile_count: n,
                        face_down_piles: if face_down { n } else { 0 },
                    },
                    subject,
                    Vec::new(),
                    None,
                )],
                (),
            )));
        }
    }
    ts.restore(cp);
    None
}

/// Append an optional `and the top <N> card(s) of <zone>` effect onto an `exile` output list.
pub(super) fn push_extra_top_exile(
    out: &mut Vec<Effect>,
    actor: PlayerRef,
    face_down: bool,
    extra: Option<(ZoneRef, ValueExpr)>,
) {
    if let Some((zr, cnt)) = extra {
        out.push(exile_zone_op(
            actor,
            zone_cards_subject(zr),
            vec![zr],
            face_down,
            None,
            Some(exact_count(cnt)),
        ));
    }
}

/// Append an optional `and the top <N> card(s) of <zone>` effect onto a `reveal` output list.
pub(super) fn push_extra_top_reveal(
    out: &mut Vec<Effect>,
    actor: PlayerRef,
    extra: Option<(ZoneRef, ValueExpr)>,
) {
    if let Some((zr, cnt)) = extra {
        out.push(counted_zone_op(
            actor,
            ZoneOpKind::LookTop { reveal: true },
            SubjectRef::Player(actor),
            vec![zr],
            None,
            Some(exact_count(cnt)),
        ));
    }
}

/// Parse extended `look(s) at` clauses (`the cards in the other pile`, `the cards in each pile`).
pub(super) fn try_parse_look_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_any_phrase(&[
        &["the", "cards", "in", "the", "other", "pile"],
        &["the", "cards", "in", "each", "pile"],
    ]) {
        return Some(Ok((
            vec![simple_zone_op(
                actor,
                ZoneOpKind::LookAt,
                SubjectRef::Anaphora(AnaphoraRef::OtherPile),
                Vec::new(),
                None,
            )],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Extended `draw(s)` clauses.
pub(super) fn try_parse_draw_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    let (count, up_to) = if ts.eat_phrase(&["up", "to"])
        && let Ok(v) = try_parse(ts, parse_value_expr)
        && ts.eat_any_word(&["cards", "card"])
    {
        (v, true)
    } else if ts.eat_phrase(&["another", "card"]) {
        (ValueExpr::Const(1), false)
    } else if ts.eat_any_phrase(&[
        &[
            "as",
            "many",
            "cards",
            "as",
            "they",
            "discarded",
            "this",
            "way",
        ],
        &[
            "as",
            "many",
            "cards",
            "as",
            "you",
            "discarded",
            "this",
            "way",
        ],
    ]) {
        (
            ValueExpr::PriorActionCount(PriorActionKind::DiscardedThisWay),
            false,
        )
    } else if ts.eat_phrase(&["cards", "equal", "to", "its", "total", "toxic", "value"]) {
        (
            ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
            false,
        )
    } else {
        ts.restore(cp);
        return None;
    };
    Some(Ok((
        vec![counted_zone_op(
            actor,
            ZoneOpKind::Draw,
            SubjectRef::Player(actor),
            vec![ZoneRef {
                zone: ZoneKind::Library,
                owner: ZoneOwner::Default,
            }],
            Some(simple_zone_dest(ZoneKind::Hand, ZoneOwner::Default)),
            Some(CountSpec {
                count,
                up_to,
                any_number: false,
            }),
        )],
        (),
    )))
}

/// Extended `reveal(s)` clauses.
pub(super) fn try_parse_reveal_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if let Ok(zr) = try_parse(ts, parse_zone_ref)
        && zr.zone == ZoneKind::Library
    {
        return Some(Ok((
            vec![simple_zone_op(
                actor,
                ZoneOpKind::Reveal,
                SubjectRef::Player(actor),
                vec![zr],
                None,
            )],
            (),
        )));
    }
    ts.restore(cp);
    if let Ok(count) = try_parse(ts, parse_value_expr)
        && ts.eat_any_word(&["cards", "card"])
    {
        let top = ts.eat_phrase(&["from", "the", "top", "of"]);
        let rand = !top && ts.eat_phrase(&["at", "random", "from"]);
        if (top || rand)
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![counted_zone_op(
                    actor,
                    if top {
                        ZoneOpKind::LookTop { reveal: true }
                    } else {
                        ZoneOpKind::Reveal
                    },
                    SubjectRef::Player(actor),
                    vec![zr],
                    None,
                    Some(exact_count(count)),
                )],
                (),
            )));
        }
    }
    ts.restore(cp);
    None
}

/// Extended `search <zone> for a number of <flt> (less than or equal to | equal to) <val>`.
pub(super) fn try_parse_search_for_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
    libs: &[ZoneRef],
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["a", "number", "of"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        let up_to = ts.eat_phrase(&["less", "than", "or", "equal", "to"]);
        let eq = !up_to && ts.eat_phrase(&["equal", "to"]);
        if up_to || eq {
            let count = if ts.eat_any_phrase(&[&["the", "difference"], &["the", "other", "result"]])
            {
                ValueExpr::PriorActionCount(PriorActionKind::ThatMany)
            } else if let Ok(v) = try_parse(ts, parse_value_expr) {
                v
            } else {
                ts.restore(cp);
                return None;
            };
            return Some(Ok((
                vec![counted_zone_op(
                    actor,
                    ZoneOpKind::SearchLibrary {
                        reveal: false,
                        shuffle: false,
                        exact: false,
                    },
                    SubjectRef::Group(Box::new(flt)),
                    libs.to_vec(),
                    None,
                    Some(CountSpec {
                        count,
                        up_to,
                        any_number: false,
                    }),
                )],
                (),
            )));
        }
    }
    ts.restore(cp);
    None
}
