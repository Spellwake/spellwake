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

    // 6. `put(s) the top <flt> of <zone> <dest>` AND `put(s) the top <val> cards of <zone> <dest>` (`Cybership`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top"]) {
            let num_cp = ts.checkpoint();
            if let Ok(cnt) = try_parse(ts, parse_value_expr)
                && (ts.eat_word("cards") || ts.eat_word("card"))
                && ts.eat_word("of")
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
                && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut dest);
                return Some(Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject: SubjectRef::Group(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zr],
                            ..Default::default()
                        })),
                        from: vec![zr],
                        to: Some(dest),
                        modifiers,
                        except: None,
                        count: Some(CountSpec {
                            count: cnt,
                            up_to: false,
                            any_number: false,
                        }),
                    }],
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
        }
        ts.restore(cp);
    }

    // 7. `put(s) that pile <dest> [and the other <dest2>]` (`Steam Augury`, `Epiphany at the Drownyard`, `Split the Spoils`, `Riddles in the Dark`, `Curator of Destinies`, `Hostile Negotiations`, `Abstract Performance`, `The Celestial Toymaker`, `Intrude on the Mind`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["that", "pile"])
            && let Ok((mut d1, m1)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut d1);
            let mut out = vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: SubjectRef::Anaphora(AnaphoraRef::OnePile),
                from: Vec::new(),
                to: Some(d1),
                modifiers: m1,
                except: None,
                count: None,
            }];
            let ocp = ts.checkpoint();
            if (ts.eat_phrase(&["and", "the", "other", "pile"])
                || ts.eat_phrase(&["and", "the", "other"]))
                && let Ok((mut d2, m2)) = try_parse(ts, parse_zone_dest)
            {
                parse_trailing_under_control(ts, &mut d2);
                out.push(Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Anaphora(AnaphoraRef::OtherPile),
                    from: Vec::new(),
                    to: Some(d2),
                    modifiers: m2,
                    except: None,
                    count: None,
                });
            } else {
                ts.restore(ocp);
            }
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
    }

    // 8. `put(s) a card from the chosen pile <dest>` (`Truth or Tale`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "card", "from", "the", "chosen", "pile"])
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Anaphora(AnaphoraRef::OnePile),
                    from: Vec::new(),
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

    // 9. `put(s) the same number (and kind of counters | of each kind of counter) on <subj>` (`Captain Marvel`, `Bold Plagiarist`, `Denry Klin`)
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&[
            "the", "same", "number", "and", "kind", "of", "counters", "on",
        ]) || ts.eat_phrase(&[
            "the", "same", "number", "of", "each", "kind", "of", "counter", "on",
        ])) && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters {
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

    // 10. `put(s) (an additional | a) counter of that kind on (either of those tokens | <subj>)`
    //     OR `put(s) your choice of a +1/+1 counter or a counter of that kind on <subj>` (`Bribe Taker`)
    //     OR `put(s) one of each of those kinds of counters on <subj>` (`Aragorn, Company Leader`)
    //     OR `put(s) an additional <kind> counter on <subj>` (`Toph`, `Cursed Wombat`)
    //     OR `put(s) up to <val> <kind> counter(s) on <subj>` (`Clockwork Steed/Avian/Swarm/Beast`, `Terra`)
    //     OR `put(s) <kind> counters on <subj> equal to <val>` (`Experiment Twelve`)
    //     OR `put(s) that many <k1> counters or <k2> counters on <subj>` (`Dismantle`)
    //     OR `put(s) a <k1>, <k2>, or <k3> counter on <subj>` (`Reluctant Role Model`)
    //     OR `put(s) [~'s | its] <kind> counters on <subj>` (`Zack Fair`, `Selfless Police Captain`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["an", "additional", "counter", "of", "that", "kind", "on"])
            || ts.eat_phrase(&["a", "counter", "of", "that", "kind", "on"])
        {
            let subj = if ts.eat_phrase(&["either", "of", "those", "tokens"])
                || ts.eat_phrase(&["either", "of", "them"])
            {
                SubjectRef::Anaphora(AnaphoraRef::Them)
            } else if let Ok(s) = try_parse(ts, parse_subject_ref) {
                s
            } else {
                ts.restore(cp);
                return None;
            };
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject: subj,
                    mutation: ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::Const(1),
                    },
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
        if (ts.eat_phrase(&["your", "choice", "of"]) || ts.eat_phrase(&["their", "choice", "of"]))
            && (ts.eat_word("a") || ts.eat_word("an"))
            && let Ok(k1) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counter", "or", "a", "counter", "of", "that", "kind", "on"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseBranch(vec![
                        vec![Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::PutCounters {
                                kind: k1,
                                count: ValueExpr::Const(1),
                            },
                            duration: Duration::Instant,
                        }],
                        vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::PutCounters {
                                kind: CounterKind::Any,
                                count: ValueExpr::Const(1),
                            },
                            duration: Duration::Instant,
                        }],
                    ]),
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "one", "of", "each", "of", "those", "kinds", "of", "counters", "on",
        ]) && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::Const(1),
                    },
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["an", "additional"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters {
                        kind,
                        count: ValueExpr::Const(1),
                    },
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["up", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters { kind, count },
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
        if let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["equal", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters { kind, count },
                    duration: Duration::Instant,
                }],
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
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseBranch(vec![
                        vec![Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::PutCounters {
                                kind: k1,
                                count: cnt.clone(),
                            },
                            duration: Duration::Instant,
                        }],
                        vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::PutCounters {
                                kind: k2,
                                count: cnt,
                            },
                            duration: Duration::Instant,
                        }],
                    ]),
                }],
                (),
            )));
        }
        ts.restore(cp);
        if (ts.eat_word("a") || ts.eat_word("an"))
            && let Ok(first_k) = try_parse(ts, parse_counter_kind)
        {
            if ts.eat_phrase(&["counter", "on", "either", "of", "them"]) {
                return Some(Ok((
                    vec![Effect::MutateObject {
                        subject: SubjectRef::Anaphora(AnaphoraRef::Them),
                        mutation: ObjectMutation::PutCounters {
                            kind: first_k,
                            count: ValueExpr::Const(1),
                        },
                        duration: Duration::Instant,
                    }],
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
                && (ts.eat_word("counter") || ts.eat_word("counters"))
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                let branches = kinds
                    .into_iter()
                    .map(|kind| {
                        vec![Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::PutCounters {
                                kind,
                                count: ValueExpr::Const(1),
                            },
                            duration: Duration::Instant,
                        }]
                    })
                    .collect();
                return Some(Ok((
                    vec![Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseBranch(branches),
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
        if (ts.eat_tilde() && ts.eat_word("'s")) || ts.eat_word("its") {
            let kind = try_parse(ts, parse_counter_kind).unwrap_or(CounterKind::Any);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
                && let Ok(to) = try_parse(ts, parse_subject_ref)
            {
                return Some(Ok((
                    vec![Effect::MutateObject {
                        subject: SubjectRef::SelfObject,
                        mutation: ObjectMutation::MoveCounters {
                            from: Box::new(SubjectRef::SelfObject),
                            to: Box::new(to),
                            kind,
                            count: ValueExpr::Var(crate::v2::ir::VarName::X),
                        },
                        duration: Duration::Instant,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 11. `put(s) up to <val> [name|ability|art] stickers on <subj>` (`_____ _____ Rocketship`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["up", "to"])
            && ts.next_number(false).is_some()
            && (ts.eat_phrase(&["name", "stickers", "on"])
                || ts.eat_phrase(&["ability", "stickers", "on"])
                || ts.eat_phrase(&["art", "stickers", "on"])
                || ts.eat_phrase(&["stickers", "on"]))
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::PutSticker,
                    ),
                    duration: Duration::Permanent,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 12. Destination-first `put(s) onto the battlefield [under your control | attached to <subj>] <subj>`
    //     (`Thrilling Encore`, `Supper for Spiders`, `Myojin of Grim Betrayal`, `Bruna`)
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
                    if (ts.eat_word("and/or") || ts.eat_word("or") || ts.eat_word("and"))
                        && let Some(wk) = ts.peek_word().map(|w| w.to_ascii_lowercase())
                        && matches!(wk.as_str(), "hand" | "graveyard" | "library")
                    {
                        ts.next_word();
                    } else {
                        ts.restore(zcp);
                    }
                }
                return Some(Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject,
                        from,
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

    // 13. `put(s) any number of (them | those <flt> | <flt> [from among them]) <dest> [and the rest <dest2>]`
    //     (`Ransack`, `Blex`, `Gishath`, `Aurora Awakener`)
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
                let mut out = vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: Some(CountSpec {
                        count: ValueExpr::Const(0),
                        up_to: false,
                        any_number: true,
                    }),
                }];
                append_optional_and_the_rest(ts, actor, &mut out);
                return Some(Ok((out, ())));
            }
        }
        ts.restore(cp);
    }

    // 14. `put(s) <val> cards from (among them | the top of <zone>) <dest> [and the rest <dest2>]` (`Stargaze`, `Scroll Rack`)
    {
        let cp = ts.checkpoint();
        if let Ok(count) = try_parse(ts, parse_value_expr)
            && (ts.eat_word("cards") || ts.eat_word("card"))
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
                let mut out = vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Anaphora(AnaphoraRef::Them),
                    from,
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }];
                append_optional_and_the_rest(ts, actor, &mut out);
                return Some(Ok((out, ())));
            }
        }
        ts.restore(cp);
    }

    // 15. `put(s) (the exiled cards not cast this way | the cards you revealed this way | the bottom card of <zone> | both cards | the last chosen card | any of those cards you didn't play) <dest>`
    //     (`Ryan Sinclair`, `Collected Conjuring`, `Sanity Grinding`, `Grenzo`, `Sword of Hearth and Home`, `Forgotten Lore`, `Shrouded Lore`, `Three Wishes`)
    {
        let cp = ts.checkpoint();
        let mut from = Vec::new();
        let subj_opt = if ts.eat_phrase(&["the", "exiled", "cards", "not", "cast", "this", "way"])
            || ts.eat_phrase(&["the", "revealed", "cards", "not", "cast", "this", "way"])
            || ts.eat_phrase(&["the", "cards", "you", "revealed", "this", "way"])
            || ts.eat_phrase(&["any", "of", "those", "cards", "you", "didn't", "play"])
            || ts.eat_phrase(&["both", "cards"])
        {
            Some(SubjectRef::Anaphora(AnaphoraRef::Them))
        } else if ts.eat_phrase(&["the", "last", "chosen", "card"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatCard))
        } else if ts.eat_phrase(&["the", "bottom", "card", "of"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            from.push(zr);
            Some(SubjectRef::Group(Box::new(ObjectFilter {
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
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from,
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: None,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 16. `put(s) one back and the rest <dest>` (`Dimir Charm`) / `put(s) it back on top of <zone>` (`Triumph of Saint Katherine`) / `put(s) one of the chosen cards <d1>, the other chosen cards <d2>, and the rest <d3>` (`Selective Adaptation`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "back"]) {
            let mut out = vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: SubjectRef::OneOfThem(ValueExpr::Const(1)),
                from: Vec::new(),
                to: Some(ZoneDest {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                    library_position: Some(crate::v2::ir::LibraryPosition::Top),
                    library_order: None,
                    under_control: None,
                }),
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }];
            append_optional_and_the_rest(ts, actor, &mut out);
            return Some(Ok((out, ())));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["it", "back", "on", "top", "of"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Anaphora(AnaphoraRef::It),
                    from: Vec::new(),
                    to: Some(ZoneDest {
                        zone: zr.zone,
                        owner: zr.owner,
                        library_position: Some(crate::v2::ir::LibraryPosition::Top),
                        library_order: None,
                        under_control: None,
                    }),
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
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
                Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::OneOfThem(ValueExpr::Const(1)),
                    from: Vec::new(),
                    to: Some(d1),
                    modifiers: m1,
                    except: None,
                    count: None,
                },
                Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Anaphora(AnaphoraRef::Them),
                    from: Vec::new(),
                    to: Some(d2),
                    modifiers: m2,
                    except: None,
                    count: None,
                },
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

    // 5. `exile(s) the top half of their/your library [, rounded up/down]` (`Ulamog, the Defiler`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top", "half", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            let rcp = ts.checkpoint();
            let rounding = if ts.eat_comma() && ts.eat_phrase(&["rounded", "up"]) {
                Rounding::Up
            } else if ts.eat_comma() && ts.eat_phrase(&["rounded", "down"]) {
                Rounding::Down
            } else {
                ts.restore(rcp);
                Rounding::Up
            };
            let count = ValueExpr::Div {
                expr: Box::new(ValueExpr::PlayerStat {
                    player: actor,
                    stat: crate::ast::PlayerStatKind::CardsInLibrary,
                }),
                denom: 2,
                rounding,
            };
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
        ts.restore(cp);
    }

    // 6. `exile(s) all but the bottom card of <zone>` (`Nicol Bolas, the Arisen`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["all", "but", "the", "bottom", "card", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down: false,
                        counters: None,
                    },
                    subject: SubjectRef::All(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![zref],
                        ..Default::default()
                    })),
                    from: vec![zref],
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

    // 7. `exile(s) the top <N> <flt> of <zone>` (`Spinning Darkness`: `the top three black cards of your graveyard`)
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
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down: false,
                        counters: None,
                    },
                    subject: SubjectRef::Group(Box::new(flt)),
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
        ts.restore(cp);
    }

    None
}

/// Consume optional trailing `in [a|another] face-down/face-up pile [and the rest in a face-up pile]` on `exile`.
pub(super) fn eat_exile_pile_suffix(ts: &mut TokenStream<'_>) -> bool {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["in", "a", "face-down", "pile"])
        || ts.eat_phrase(&["in", "another", "face-down", "pile"])
        || ts.eat_phrase(&["in", "a", "face-up", "pile"])
    {
        let _ = ts.eat_phrase(&["and", "the", "rest", "in", "a", "face-up", "pile"])
            || ts.eat_phrase(&["and", "the", "rest", "in", "a", "face-down", "pile"]);
        return true;
    }
    ts.restore(cp);
    false
}

/// Consume optional trailing `and the top [<val>] card(s) of <zone>` on `exile` or `reveal`
/// (`Psychotic Episode`, `Jeskai Infiltrator`, `Triumph of Saint Katherine`, `Suspend Aggression`, `Become Anonymous`).
pub(super) fn try_eat_and_top_cards(ts: &mut TokenStream<'_>) -> Option<(ZoneRef, ValueExpr)> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["and", "the", "top"]) {
        let count = if ts.eat_word("card") {
            Some(ValueExpr::Const(1))
        } else if let Ok(v) = try_parse(ts, parse_value_expr)
            && (ts.eat_word("cards") || ts.eat_word("card"))
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

/// Parse extended `discard(s)` clauses (`discard half the cards in their/your hand [, rounded up/down]`,
/// `discard a third of the cards in their/your hand`, `discard one or more <flt>`).
pub(super) fn try_parse_discard_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 1. `discard(s) (half | a third of) the cards in their/your hand [, rounded up/down]`
    //    (`Rush of Dread`, `Fraying Omnipotence`, `Pox Plague`, `Lord Xander`, `Pox`)
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
            && (ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]))
        {
            let rcp = ts.checkpoint();
            let rounding = if ts.eat_comma() && ts.eat_phrase(&["rounded", "up"]) {
                Rounding::Up
            } else if ts.eat_comma() && ts.eat_phrase(&["rounded", "down"]) {
                Rounding::Down
            } else {
                ts.restore(rcp);
                Rounding::Up
            };
            let count = ValueExpr::Div {
                expr: Box::new(ValueExpr::PlayerStat {
                    player: actor,
                    stat: crate::ast::PlayerStatKind::CardsInHand,
                }),
                denom: d,
                rounding,
            };
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Discard {
                        hand: false,
                        random: false,
                    },
                    subject: SubjectRef::Player(actor),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
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
    }

    // 2. `discard(s) one or more <flt>` (`Loamcrafter Faun`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Discard {
                        hand: false,
                        random: false,
                    },
                    subject: SubjectRef::Group(Box::new(flt)),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
                        owner: ZoneOwner::Default,
                    }],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: Some(CountSpec {
                        count: ValueExpr::Const(1),
                        up_to: false,
                        any_number: true,
                    }),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Parse extended `return(s)` clauses (`return the top <flt> of <zone> to <dest>`,
/// `return half the <flt> <dest> [, rounded up/down]`, `return that card under your control [with a <kind> counter on it]`).
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

    // 2. `return(s) half the <flt> <dest> [, rounded up/down]` (`Split the Party`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["half", "the"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && let Ok((mut dest, modifiers)) = try_parse(ts, parse_zone_dest)
        {
            parse_trailing_under_control(ts, &mut dest);
            let rcp = ts.checkpoint();
            let rounding = if ts.eat_comma() && ts.eat_phrase(&["rounded", "up"]) {
                Rounding::Up
            } else if ts.eat_comma() && ts.eat_phrase(&["rounded", "down"]) {
                Rounding::Down
            } else {
                ts.restore(rcp);
                Rounding::Up
            };
            let count = ValueExpr::Div {
                expr: Box::new(ValueExpr::Count(Box::new(flt.clone()))),
                denom: 2,
                rounding,
            };
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject: SubjectRef::Group(Box::new(flt)),
                    from: Vec::new(),
                    to: Some(dest),
                    modifiers,
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
    }

    // 3. `return(s) <subj> under your control [with a <kind> counter on it]` (missing `to the battlefield`, e.g. `Meathook Massacre II`)
    {
        let cp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["under", "your", "control"])
        {
            let mut modifiers = EntryModifiers::default();
            if ts.eat_word("with")
                && (ts.eat_word("a") || ts.eat_word("an"))
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
                && ts.eat_phrase(&["on", "it"])
            {
                modifiers.with_counter = Some((ck, 1));
            }
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(ZoneDest {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::You,
                        library_position: None,
                        library_order: None,
                        under_control: Some(ZoneOwner::You),
                    }),
                    modifiers,
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

/// Parse extended `shuffle(s)` clauses (`shuffle that pile`, `shuffle the rest of that pile into <zone>`).
pub(super) fn try_parse_shuffle_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["that", "pile"]) {
        return Some(Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Shuffle,
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
    if ts.eat_phrase(&["the", "rest", "of", "that", "pile", "into"])
        && let Ok(zref) = try_parse(ts, parse_zone_ref)
    {
        return Some(Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Shuffle,
                subject: SubjectRef::Anaphora(AnaphoraRef::TheRest),
                from: Vec::new(),
                to: Some(ZoneDest {
                    zone: zref.zone,
                    owner: zref.owner,
                    library_position: Some(crate::v2::ir::LibraryPosition::Shuffled),
                    library_order: None,
                    under_control: None,
                }),
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
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
        if ts.eat_word("piles") || ts.eat_word("pile") {
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: n,
                        face_down_piles: if face_down { n } else { 0 },
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
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
        out.push(Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Exile {
                face_down,
                counters: None,
            },
            subject: SubjectRef::Group(Box::new(ObjectFilter {
                any_card: true,
                zones: vec![zr],
                ..Default::default()
            })),
            from: vec![zr],
            to: None,
            modifiers: EntryModifiers {
                face_down,
                ..EntryModifiers::default()
            },
            except: None,
            count: Some(CountSpec {
                count: cnt,
                up_to: false,
                any_number: false,
            }),
        });
    }
}

/// Append an optional `and the top <N> card(s) of <zone>` effect onto a `reveal` output list.
pub(super) fn push_extra_top_reveal(
    out: &mut Vec<Effect>,
    actor: PlayerRef,
    extra: Option<(ZoneRef, ValueExpr)>,
) {
    if let Some((zr, cnt)) = extra {
        out.push(Effect::ZoneOp {
            actor,
            op: ZoneOpKind::LookTop { reveal: true },
            subject: SubjectRef::Player(actor),
            from: vec![zr],
            to: None,
            modifiers: EntryModifiers::default(),
            except: None,
            count: Some(CountSpec {
                count: cnt,
                up_to: false,
                any_number: false,
            }),
        });
    }
}

/// Parse extended `look(s) at` clauses (`the cards in the other pile`, `the cards in each pile`).
pub(super) fn try_parse_look_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["the", "cards", "in", "the", "other", "pile"])
        || ts.eat_phrase(&["the", "cards", "in", "each", "pile"])
    {
        return Some(Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::LookAt,
                subject: SubjectRef::Anaphora(AnaphoraRef::OtherPile),
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
    None
}
