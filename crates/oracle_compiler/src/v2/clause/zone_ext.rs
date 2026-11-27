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

/// Parse trailing `under <owner> control` qualifiers that `parse_zone_dest` may leave unconsumed.
pub(super) fn parse_trailing_under_control(ts: &mut TokenStream<'_>, dest: &mut ZoneDest) {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["under", "their", "control"])
        || ts.eat_phrase(&["under", "that", "player's", "control"])
    {
        dest.under_control = Some(ZoneOwner::ThatPlayer);
    } else if ts.eat_phrase(&["under", "an", "opponent's", "control"]) {
        dest.under_control = Some(ZoneOwner::Opponent);
    } else if ts.eat_phrase(&["under", "target", "player's", "control"]) {
        dest.under_control = Some(ZoneOwner::TargetPlayer);
    } else if ts.eat_phrase(&["under", "target", "opponent's", "control"]) {
        dest.under_control = Some(ZoneOwner::TargetOpponent);
    } else if ts.eat_phrase(&["under", "the", "control", "of", "that", "card's", "owner"])
        || ts.eat_phrase(&["under", "its", "owner's", "control"])
    {
        dest.under_control = Some(ZoneOwner::ItsOwner);
    } else if ts.eat_phrase(&["under", "their", "owners'", "control"]) {
        dest.under_control = Some(ZoneOwner::TheirOwners);
    } else if ts.eat_phrase(&["under", "your", "control"]) {
        dest.under_control = Some(ZoneOwner::You);
    } else {
        ts.restore(cp);
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
                vec![Effect::MutateObject {
                    subject: SubjectRef::SelfObject,
                    mutation: ObjectMutation::MoveCounters {
                        from: Box::new(SubjectRef::SelfObject),
                        to: Box::new(to),
                        kind: CounterKind::Any,
                        count: ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
                    },
                    duration: Duration::Instant,
                }],
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
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters {
                        kind,
                        count: ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
                    },
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 3. `put(s) the rest of [the [exiled|revealed] | those [exiled|revealed]] cards [revealed/exiled this way] <dest>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "rest", "of"]) {
            let matched = ts.eat_phrase(&["the", "cards", "revealed", "this", "way"])
                || ts.eat_phrase(&["the", "cards", "exiled", "this", "way"])
                || ts.eat_phrase(&["the", "revealed", "cards"])
                || ts.eat_phrase(&["the", "exiled", "cards"])
                || ts.eat_phrase(&["those", "revealed", "cards"])
                || ts.eat_phrase(&["those", "exiled", "cards"])
                || ts.eat_phrase(&["those", "cards"]);
            if matched && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest) {
                parse_trailing_under_control(ts, &mut dest);
                return Some(Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject: SubjectRef::Anaphora(AnaphoraRef::TheRest),
                        from: Vec::new(),
                        to: Some(dest),
                        modifiers,
                        except: None,
                        count: None,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 4. `put(s) all [the] [revealed | nonland | land] cards [not cast this way | chosen this way | from the pile of your choice | from <zone>] <dest> [and the rest <dest2>]`
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
            } else if ts.eat_phrase(&["cards", "from", "the", "pile", "of", "your", "choice"])
                || ts.eat_phrase(&["cards", "from", "the", "pile", "of", "their", "choice"])
            {
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
                Some(SubjectRef::All(Box::new(ObjectFilter {
                    any_card: true,
                    zones: vec![zr],
                    ..Default::default()
                })))
            } else {
                None
            };
            if let Some(subject) = subj_opt
                && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut dest);
                let mut out = vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from,
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: None,
                }];
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
            let mut out = vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: SubjectRef::Group(Box::new(flt)),
                from: Vec::new(),
                to: Some(dest),
                modifiers,
                except: None,
                count: None,
            }];
            append_optional_and_the_rest(ts, actor, &mut out);
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
    }

    // 6. `put(s) the top <flt> of <zone> <dest>`
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
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Group(Box::new(flt)),
                    from: vec![zr],
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: Some(CountSpec {
                        count: ValueExpr::Const(1),
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            )));
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
        out.push(Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Move,
            subject: SubjectRef::Anaphora(AnaphoraRef::TheRest),
            from: Vec::new(),
            to: Some(d2),
            modifiers: m2,
            except: None,
            count: None,
        });
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
        let second_eff = Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Move,
            subject: subject.clone(),
            from: from.to_vec(),
            to: Some(d2),
            modifiers: m2,
            except: None,
            count: put_count.clone(),
        };
        let mut out = vec![Effect::PromptChoice {
            chooser: actor,
            kind: ChoiceKind::ChooseBranch(vec![vec![first_eff], vec![second_eff]]),
        }];
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
        let _ =
            ts.eat_phrase(&["of", "their", "choice"]) || ts.eat_phrase(&["of", "your", "choice"]);
        let _ = ts.eat_comma();
        let rounding = if ts.eat_phrase(&["rounded", "up"]) {
            Rounding::Up
        } else {
            let _ = ts.eat_phrase(&["rounded", "down"]);
            Rounding::Down
        };
        let count = ValueExpr::Div {
            expr: Box::new(ValueExpr::Count(Box::new(flt.clone()))),
            denom: 2,
            rounding,
        };
        return Some(Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Sacrifice,
                subject: SubjectRef::Group(Box::new(flt)),
                from: vec![ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                }],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Parse extended `move(s)` counter clauses:
/// `move [all | any number of | a/an | <val>] [<kind>] counter(s) from [among] <src> onto/to [a second target <flt> | <dst>]`.
pub(super) fn try_parse_move_ext(ts: &mut TokenStream<'_>) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    let count = if ts.eat_word("all") || ts.eat_phrase(&["any", "number", "of"]) {
        ValueExpr::Const(0)
    } else if ts.eat_word("a") || ts.eat_word("an") {
        ValueExpr::Const(1)
    } else if let Ok(v) = try_parse(ts, parse_value_expr) {
        v
    } else {
        ts.restore(cp);
        return None;
    };
    let kind = if ts.eat_word("counter") || ts.eat_word("counters") {
        CounterKind::Any
    } else if let Ok(ck) = try_parse(ts, parse_counter_kind)
        && (ts.eat_word("counter") || ts.eat_word("counters"))
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
    if !(ts.eat_word("onto") || ts.eat_word("to")) {
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
        vec![Effect::MutateObject {
            subject: from.clone(),
            mutation: ObjectMutation::MoveCounters {
                from: Box::new(from),
                to: Box::new(to),
                kind,
                count,
            },
            duration: Duration::Instant,
        }],
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
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down: false,
                        counters: None,
                    },
                    subject: SubjectRef::Group(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![zref],
                        ..Default::default()
                    })),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: Some(CountSpec {
                        count: ValueExpr::Const(1),
                        up_to: false,
                        any_number: false,
                    }),
                }],
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
                && (ts.eat_word("cards") || ts.eat_word("card"))
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
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Exile {
                            face_down,
                            counters: None,
                        },
                        subject: SubjectRef::Group(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zref],
                            ..Default::default()
                        })),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers {
                            face_down,
                            ..EntryModifiers::default()
                        },
                        except: None,
                        count: Some(CountSpec {
                            count,
                            up_to: false,
                            any_number: false,
                        }),
                    }],
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
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Exile {
                            face_down: false,
                            counters: None,
                        },
                        subject: SubjectRef::Group(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zref],
                            ..Default::default()
                        })),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: Some(CountSpec {
                            count,
                            up_to: false,
                            any_number: false,
                        }),
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 4. `exile(s) the pile of an opponent's/your/their choice`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "pile", "of", "an", "opponent's", "choice"])
            || ts.eat_phrase(&["the", "pile", "of", "your", "choice"])
            || ts.eat_phrase(&["the", "pile", "of", "their", "choice"])
        {
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down: false,
                        counters: None,
                    },
                    subject: SubjectRef::Anaphora(AnaphoraRef::OnePile),
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Parse `return(s) the top <flt> of <zone> to <dest>`.
pub(super) fn try_parse_return_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["the", "top"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_word("of")
        && let Ok(zr) = try_parse(ts, parse_zone_ref)
        && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
    {
        parse_trailing_under_control(ts, &mut dest);
        return Some(Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: SubjectRef::Group(Box::new(flt)),
                from: vec![zr],
                to: Some(dest),
                modifiers,
                except: None,
                count: Some(CountSpec {
                    count: ValueExpr::Const(1),
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        )));
    }
    ts.restore(cp);
    None
}
