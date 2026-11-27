//! Zone-operation and counter-placement player verb frames (`draw`, `discard`, `sacrifice`,
//! `create`, `destroy`, `exile`, `return`, `put`, `move`, `distribute`, `scry`/`surveil`/`shuffle`,
//! `search`, `reveal`, `look`, `separate`) for `v2`.

use crate::v2::clause::subject_verbs::parse_trailing_duration;
use crate::v2::clause::tokens::parse_token_creation;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_dest,
    parse_zone_ref,
};
use crate::v2::ir::{
    CountSpec, CounterKind, Duration, Effect, EntryModifiers, ObjectFilter, ObjectMutation,
    ObjectStateAction, PlayerMutation, PlayerRef, ScaledPlayerAction, SubjectRef, ValueExpr,
    ZoneDest, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Dispatch zone-operation and counter-manipulation verbs with a known player `actor`.
pub(super) fn try_parse_zone_verb_frame(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let w = ts.peek_word()?.to_ascii_lowercase();
    if !matches!(
        w.as_str(),
        "draw"
            | "draws"
            | "discard"
            | "discards"
            | "sacrifice"
            | "sacrifices"
            | "create"
            | "creates"
            | "destroy"
            | "destroys"
            | "exile"
            | "exiles"
            | "return"
            | "returns"
            | "put"
            | "puts"
            | "move"
            | "moves"
            | "distribute"
            | "distributes"
            | "scry"
            | "scries"
            | "surveil"
            | "surveils"
            | "shuffle"
            | "shuffles"
            | "search"
            | "searches"
            | "reveal"
            | "reveals"
            | "look"
            | "looks"
            | "separate"
            | "separates"
    ) {
        return None;
    }
    Some(parse_zone_verb_frame_inner(ts, actor))
}

fn parse_zone_verb_frame_inner(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    // 7. `draw(s) ...`
    if ts.eat_word("draw") || ts.eat_word("draws") {
        let count = if ts.eat_phrase(&["a", "number", "of", "cards", "equal", "to"])
            || ts.eat_phrase(&["cards", "equal", "to"])
        {
            parse_value_expr(ts)?
        } else {
            let base = if ts.eat_word("a") || ts.eat_word("an") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let _ = ts.eat_word("additional");
            if !(ts.eat_word("card") || ts.eat_word("cards")) {
                return Err(ts.err("expected `card(s)` after `draw`"));
            }
            let adjusted = if ts.eat_word("plus") {
                ValueExpr::Add(Box::new(base), Box::new(parse_value_expr(ts)?))
            } else if ts.eat_word("minus") {
                ValueExpr::Sub(Box::new(base), Box::new(parse_value_expr(ts)?))
            } else {
                base
            };
            if let Some(fe) = opt(ts, parse_for_each_expr) {
                if adjusted == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(adjusted), Box::new(fe))
                }
            } else {
                adjusted
            }
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Draw,
                subject: SubjectRef::Player(actor),
                from: vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                to: Some(ZoneDest {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::Default,
                    library_position: None,
                    library_order: None,
                    under_control: None,
                }),
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 8. `discard(s) ...`
    if ts.eat_word("discard") || ts.eat_word("discards") {
        if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Discard {
                        hand: true,
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
                    count: None,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["any", "number", "of"]) {
            let flt = parse_object_filter(ts)?;
            return Ok((
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
                        count: ValueExpr::Const(0),
                        up_to: false,
                        any_number: true,
                    }),
                }],
                (),
            ));
        }
        let up_to = ts.eat_phrase(&["up", "to"]);
        // Targeted/filtered discard (`discards that card`, `discards a creature card`, `discards a nonland card`)
        {
            let dcp = ts.checkpoint();
            if !up_to && let Ok(subj) = try_parse(ts, parse_subject_ref) {
                let random = ts.eat_phrase(&["at", "random"]);
                if !matches!(subj, SubjectRef::Counted { .. }) {
                    return Ok((
                        vec![Effect::ZoneOp {
                            actor,
                            op: ZoneOpKind::Discard {
                                hand: false,
                                random,
                            },
                            subject: subj,
                            from: vec![ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: ZoneOwner::Default,
                            }],
                            to: None,
                            modifiers: EntryModifiers::default(),
                            except: None,
                            count: None,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(dcp);
        }
        let cnt = if ts.eat_word("a") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        if !(ts.eat_word("card") || ts.eat_word("cards")) {
            return Err(ts.err("expected `card(s)` after `discard`"));
        }
        let random = ts.eat_phrase(&["at", "random"]);
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Discard {
                    hand: false,
                    random,
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
                    count: cnt,
                    up_to,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 9. `sacrifice(s) ...`
    if ts.eat_word("sacrifice") || ts.eat_word("sacrifices") {
        if ts.eat_phrase(&["the", "rest"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Sacrifice,
                    subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::TheRest),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::Default,
                    }],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        // Handle anaphoric / `~` / `all <flt>` subjects (`Sacrifice it`, `Sacrifice ~`, `sacrifices all permanents in the pile of their choice`)
        // Guard against `"that many"` which is a dynamic count (`sacrifice that many permanents`), not an explicit subject!
        let is_that_many = ts.peek_phrase(&["that", "many"]);
        let is_explicit_subject = !is_that_many
            && (matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Tilde))
                || ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("it")
                        || w.eq_ignore_ascii_case("them")
                        || w.eq_ignore_ascii_case("all")
                        || w.eq_ignore_ascii_case("that")
                        || w.eq_ignore_ascii_case("those")
                }));
        if is_explicit_subject {
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Sacrifice,
                    subject,
                    from: vec![ZoneRef {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::Default,
                    }],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        let any_number = ts.eat_phrase(&["any", "number", "of"]);
        let count = if any_number {
            ValueExpr::Const(0)
        } else if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let _ = ts.eat_phrase(&["of", "those"]);
        let flt = parse_object_filter(ts)?;
        let mut out = vec![Effect::ZoneOp {
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
                any_number,
            }),
        }];
        // Support conjoined `sacrifices <flt1> and <flt2> [of their choice]` (`Perilous Predicament`, `To the Slaughter`)
        loop {
            let scp = ts.checkpoint();
            if ts.eat_word("and") {
                let c2 = if ts.eat_word("a") || ts.eat_word("an") {
                    ValueExpr::Const(1)
                } else {
                    try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                };
                if let Ok(flt2) = try_parse(ts, parse_object_filter) {
                    out.push(Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Sacrifice,
                        subject: SubjectRef::Group(Box::new(flt2)),
                        from: vec![ZoneRef {
                            zone: ZoneKind::Battlefield,
                            owner: ZoneOwner::Default,
                        }],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: Some(CountSpec {
                            count: c2,
                            up_to: false,
                            any_number: false,
                        }),
                    });
                    continue;
                }
            }
            ts.restore(scp);
            break;
        }
        return Ok((out, ()));
    }

    // 10. `create(s) <token1> [, <token2>, and <token3>]`
    if ts.eat_word("create") || ts.eat_word("creates") {
        let (count, spec, modifiers) = parse_token_creation(ts)?;
        let mut out = vec![Effect::CreateToken {
            controller: actor,
            count,
            spec,
            modifiers,
        }];
        loop {
            let tcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_and = ts.eat_word("and");
            if (had_comma || had_and)
                && let Ok((c2, s2, m2)) = try_parse(ts, parse_token_creation)
            {
                out.push(Effect::CreateToken {
                    controller: actor,
                    count: c2,
                    spec: s2,
                    modifiers: m2,
                });
                continue;
            }
            ts.restore(tcp);
            break;
        }
        return Ok((out, ()));
    }

    // 11. `destroy(s) <subj>`
    if ts.eat_word("destroy") || ts.eat_word("destroys") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Destroy {
                    can_regenerate: true,
                },
                subject,
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 12. `exile(s) ...`
    if ts.eat_word("exile") || ts.eat_word("exiles") {
        // `exile their hand` / `exile all cards from their hand [face down]`
        if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]) {
            let face_down = ts.eat_phrase(&["face", "down"]);
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down,
                        counters: None,
                    },
                    subject: SubjectRef::All(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![ZoneRef {
                            zone: ZoneKind::Hand,
                            owner: ZoneOwner::ThatPlayer,
                        }],
                        ..Default::default()
                    })),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
                        owner: ZoneOwner::ThatPlayer,
                    }],
                    to: None,
                    modifiers: EntryModifiers {
                        face_down,
                        ..EntryModifiers::default()
                    },
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        // `exile <zone>` (`exile target player's graveyard`, `exile all graveyards`, `exile each opponent's graveyard`)
        {
            let zcp = ts.checkpoint();
            if let Ok(zref) = try_parse(ts, parse_zone_ref)
                && zref.zone == ZoneKind::Graveyard
            {
                return Ok((
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
                ));
            }
            ts.restore(zcp);
        }
        // `exile cards from the top of <lib> until <player> reveal(s)/exile(s) <flt>`
        {
            let ecp = ts.checkpoint();
            if ts.eat_phrase(&["cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("until")
                && opt(ts, parse_player_ref).is_some()
                && (ts.eat_word("reveal")
                    || ts.eat_word("reveals")
                    || ts.eat_word("exile")
                    || ts.eat_word("exiles"))
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::ExileUntilReveal {
                            filter: Box::new(flt),
                        },
                        subject: SubjectRef::Player(actor),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(ecp);
        }
        // `exile <val> card(s) from the top of <lib>` (`Crumbling Sanctuary`: `exiles that many cards from the top of their library`)
        {
            let ttop_cp = ts.checkpoint();
            if let Ok(count) = try_parse(ts, parse_value_expr)
                && (ts.eat_word("cards") || ts.eat_word("card"))
                && ts.eat_phrase(&["from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                let face_down = ts.eat_phrase(&["face", "down"]);
                let counters = parse_exile_with_counters(ts);
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Exile {
                            face_down,
                            counters,
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
                ));
            }
            ts.restore(ttop_cp);
        }
        // `exile a number of cards from the top of <lib> equal to <val>`
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of", "cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
            {
                return Ok((
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
                ));
            }
            ts.restore(ncp);
        }
        // `exile the top [N] card(s) of <owner>'s library`
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("card") || ts.eat_word("cards")) {
                    return Err(ts.err("expected `card(s)` after `the top <N>`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of` after `the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            let face_down = ts.eat_phrase(&["face", "down"]);
            let counters = parse_exile_with_counters(ts);
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down,
                        counters,
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
            ));
        }

        // Compound exile (`Exile ~ and those creature cards`, `Exile all enchantments from the battlefield, all instant cards from all graveyards, and all artifact cards from all hands`)
        let first = parse_subject_ref(ts)?;
        let mut subjects = vec![first];
        loop {
            let cp = ts.checkpoint();
            if ts.eat_comma() {
                let _ = ts.eat_word("and");
                if ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("all"))
                    && let Ok(next_s) = try_parse(ts, parse_subject_ref)
                {
                    subjects.push(next_s);
                    continue;
                }
            }
            ts.restore(cp);
            if ts.eat_word("and")
                && !ts.peek_phrase(&["that", "player"])
                && !ts.peek_phrase(&["that", "opponent"])
                && ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("those")
                        || w.eq_ignore_ascii_case("that")
                        || w.eq_ignore_ascii_case("all")
                })
                && let Ok(next_s) = try_parse(ts, parse_subject_ref)
            {
                subjects.push(next_s);
                continue;
            }
            ts.restore(cp);
            break;
        }
        let subject = if subjects.len() == 1 {
            subjects.remove(0)
        } else {
            SubjectRef::Compound(subjects)
        };
        let mut from = Vec::new();
        if ts.eat_word("from") {
            let z1 = parse_zone_ref(ts)?;
            from.push(z1);
            loop {
                let zcp = ts.checkpoint();
                if ts.eat_word("or") || ts.eat_word("and") {
                    let _ = ts.eat_word("from");
                    if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                        from.push(z2);
                        continue;
                    }
                    let bare = if ts.eat_word("graveyard") || ts.eat_word("graveyards") {
                        Some(ZoneKind::Graveyard)
                    } else if ts.eat_word("hand") || ts.eat_word("hands") {
                        Some(ZoneKind::Hand)
                    } else if ts.eat_word("library") || ts.eat_word("libraries") {
                        Some(ZoneKind::Library)
                    } else if ts.eat_word("exile") {
                        Some(ZoneKind::Exile)
                    } else {
                        None
                    };
                    if let Some(zone) = bare {
                        from.push(ZoneRef {
                            zone,
                            owner: z1.owner,
                        });
                        continue;
                    }
                }
                ts.restore(zcp);
                break;
            }
        }
        let face_down = ts.eat_phrase(&["face", "down"]);
        // Optional `with a/an/<N> <kind> counter(s) on it` / `with a number of <kind> counters on it equal to <val>`
        let counters = parse_exile_with_counters(ts);
        let ex_dur = parse_trailing_duration(ts);
        let ate_instead_gy =
            ts.eat_phrase(&[
                "instead",
                "of",
                "putting",
                "it",
                "into",
                "its",
                "owner's",
                "graveyard",
            ]) || ts.eat_phrase(&[
                "instead",
                "of",
                "putting",
                "them",
                "into",
                "their",
                "owners'",
                "graveyards",
            ]) || ts.eat_phrase(&[
                "instead",
                "of",
                "putting",
                "it",
                "into",
                "your",
                "graveyard",
            ]) || ts.eat_phrase(&["instead", "of", "putting", "it", "into", "a", "graveyard"])
                || ts.eat_phrase(&[
                    "instead",
                    "of",
                    "putting",
                    "it",
                    "into",
                    "their",
                    "graveyard",
                ]);
        if ate_instead_gy {
            let _ = ts.eat_phrase(&["as", "it", "resolves"]);
        }
        let eff = Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Exile {
                face_down,
                counters,
            },
            subject,
            from,
            to: None,
            modifiers: EntryModifiers {
                face_down,
                ..EntryModifiers::default()
            },
            except: None,
            count: None,
        };
        let eff = match ex_dur {
            Some(duration) => Effect::ScopedDuration {
                duration,
                body: vec![eff],
            },
            None => eff,
        };
        // Compound `exile <subj> and <graveyard>` (`Strategic Betrayal`)
        let extra_exile_zone = {
            let zcp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
                && zr.zone == ZoneKind::Graveyard
            {
                Some(zr)
            } else {
                ts.restore(zcp);
                None
            }
        };
        let mut out_exile = vec![eff];
        if let Some(zr) = extra_exile_zone {
            out_exile.push(Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Exile {
                    face_down: false,
                    counters: None,
                },
                subject: SubjectRef::All(Box::new(ObjectFilter {
                    any_card: true,
                    zones: vec![zr],
                    ..Default::default()
                })),
                from: vec![zr],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            });
        }
        return Ok((out_exile, ()));
    }

    // 13. `return(s) ...` (both `return <subj> [from <zone>] [except <exc>] to <dest> [from <zone>]` AND `returns to their hand <subj>`)
    if ts.eat_word("return") || ts.eat_word("returns") {
        // Inverted destination-first: `returns to their hand each card they exiled this way`
        if ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("to")) {
            let (dest, modifiers) = parse_zone_dest(ts)?;
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        let return_count = {
            let rcp = ts.checkpoint();
            if ts.eat_phrase(&["up", "to"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
            {
                Some(CountSpec {
                    count: val,
                    up_to: true,
                    any_number: false,
                })
            } else {
                ts.restore(rcp);
                None
            }
        };
        let first_subj = parse_subject_ref(ts)?;
        let subject = {
            let ccp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(second_subj) = try_parse(ts, parse_subject_ref)
                && ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("from")
                        || w.eq_ignore_ascii_case("to")
                        || w.eq_ignore_ascii_case("onto")
                })
            {
                SubjectRef::Compound(vec![first_subj, second_subj])
            } else {
                ts.restore(ccp);
                first_subj
            }
        };
        let mut from = Vec::new();
        if ts.eat_word("from") {
            let z1 = parse_zone_ref(ts)?;
            from.push(z1);
            loop {
                let zcp = ts.checkpoint();
                if ts.eat_word("or") || ts.eat_word("and") {
                    let _ = ts.eat_word("from");
                    if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                        from.push(z2);
                        continue;
                    }
                    let bare = if ts.eat_word("graveyard") || ts.eat_word("graveyards") {
                        Some(ZoneKind::Graveyard)
                    } else if ts.eat_word("hand") || ts.eat_word("hands") {
                        Some(ZoneKind::Hand)
                    } else if ts.eat_word("library") || ts.eat_word("libraries") {
                        Some(ZoneKind::Library)
                    } else if ts.eat_word("exile") {
                        Some(ZoneKind::Exile)
                    } else {
                        None
                    };
                    if let Some(zone) = bare {
                        from.push(ZoneRef {
                            zone,
                            owner: z1.owner,
                        });
                        continue;
                    }
                }
                ts.restore(zcp);
                break;
            }
        }
        let except = if ts.eat_word("except") {
            Some(Box::new(parse_subject_ref(ts)?))
        } else {
            None
        };
        let (dest, modifiers) = parse_zone_dest(ts)?;
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let _ = ts.eat_phrase(&[
            "instead",
            "of",
            "putting",
            "it",
            "into",
            "your",
            "graveyard",
        ]) || ts.eat_phrase(&[
            "instead",
            "of",
            "putting",
            "it",
            "into",
            "its",
            "owner's",
            "graveyard",
        ]) || ts.eat_phrase(&["instead", "of", "into", "that", "player's", "graveyard"])
            || ts.eat_phrase(&["instead", "of", "into", "its", "owner's", "graveyard"])
            || ts.eat_phrase(&["instead", "of", "into", "their", "graveyard"])
            || ts.eat_phrase(&["instead", "of", "into", "your", "graveyard"]);
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject,
                from,
                to: Some(dest),
                modifiers,
                except,
                count: return_count,
            }],
            (),
        ));
    }

    // 14. `put(s) ...` (BOTH `put <cnt> <kind> counter(s) on <subj>` AND `put a <kind> sticker on <subj>` AND `put <subj> into/onto/on <dest>`)
    if ts.eat_word("put") || ts.eat_word("puts") {
        // 14a0. `put a [name|ability|art] sticker on <subj>` (`Sword-Swallowing Seraph` + 7+ Unfinity cards)
        {
            let scp = ts.checkpoint();
            if (ts.eat_word("a") || ts.eat_word("an"))
                && (ts.eat_phrase(&["name", "sticker", "on"])
                    || ts.eat_phrase(&["ability", "sticker", "on"])
                    || ts.eat_phrase(&["art", "sticker", "on"])
                    || ts.eat_phrase(&["sticker", "on"]))
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::StateAction(ObjectStateAction::PutSticker),
                        duration: Duration::Permanent,
                    }],
                    (),
                ));
            }
            ts.restore(scp);
        }
        // 14a. `put a number of <kind> counters on <subj> equal to <val>`
        //      OR `put a number of <kind> counters equal to <val> on <subj>` (`Ravenous Slime`)
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
            {
                if ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                    && ts.eat_phrase(&["equal", "to"])
                    && let Ok(count) = try_parse(ts, parse_value_expr)
                {
                    return Ok((
                        vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::PutCounters { kind, count },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
                ts.restore(cp);
                if ts.eat_phrase(&["a", "number", "of"])
                    && let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["equal", "to"])
                    && let Ok(count) = try_parse(ts, parse_value_expr)
                    && ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Ok((
                        vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::PutCounters { kind, count },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(cp);
        }
        // 14b. `put <cnt> <kind> counter(s) [and <cnt2> <kind2> counter(s)] on <subj> [for each/every ...]`
        {
            let cp = ts.checkpoint();
            let base_cnt = if ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("another") {
                Some(ValueExpr::Const(1))
            } else {
                opt(ts, parse_value_expr)
            };
            if let Some(base) = base_cnt
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                let mut pairs = vec![(base, kind)];
                loop {
                    let acp = ts.checkpoint();
                    if ts.eat_word("and") {
                        let c2 = if ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("another")
                        {
                            Some(ValueExpr::Const(1))
                        } else {
                            opt(ts, parse_value_expr)
                        };
                        if let Some(c2) = c2
                            && let Ok(k2) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counter") || ts.eat_word("counters"))
                        {
                            pairs.push((c2, k2));
                            continue;
                        }
                    }
                    ts.restore(acp);
                    break;
                }
                if ts.eat_word("on") {
                    let subject = parse_subject_ref(ts)?;
                    let fe = opt(ts, parse_for_each_expr);
                    let mut out = Vec::with_capacity(pairs.len());
                    for (b, k) in pairs {
                        let count = if let Some(ref f) = fe {
                            if b == ValueExpr::Const(1) {
                                f.clone()
                            } else {
                                ValueExpr::Mul(Box::new(b), Box::new(f.clone()))
                            }
                        } else {
                            b
                        };
                        out.push(Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::PutCounters { kind: k, count },
                            duration: Duration::Instant,
                        });
                    }
                    // Support `put a +1/+1 counter on that creature and a +1/+1 counter on ~` (`Juniper Order Ranger`)
                    {
                        let jcp = ts.checkpoint();
                        if ts.eat_word("and") {
                            let c2 = if ts.eat_word("a") || ts.eat_word("an") {
                                Some(ValueExpr::Const(1))
                            } else {
                                opt(ts, parse_value_expr)
                            };
                            if let Some(c2) = c2
                                && let Ok(k2) = try_parse(ts, parse_counter_kind)
                                && (ts.eat_word("counter") || ts.eat_word("counters"))
                                && ts.eat_word("on")
                                && let Ok(subj2) = try_parse(ts, parse_subject_ref)
                            {
                                out.push(Effect::MutateObject {
                                    subject: subj2,
                                    mutation: ObjectMutation::PutCounters {
                                        kind: k2,
                                        count: c2,
                                    },
                                    duration: Duration::Instant,
                                });
                            } else {
                                ts.restore(jcp);
                            }
                        } else {
                            ts.restore(jcp);
                        }
                    }
                    return Ok((out, ()));
                }
            }
            ts.restore(cp);
        }
        // 14c0. `put its counters on <tgt>` (Star Pupil / Modular-like)
        if ts.eat_phrase(&["its", "counters", "on"]) {
            let to = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::MutateObject {
                    subject: SubjectRef::SelfObject,
                    mutation: ObjectMutation::MoveCounters {
                        from: Box::new(SubjectRef::SelfObject),
                        to: Box::new(to),
                        kind: CounterKind::Any,
                        count: ValueExpr::Var(crate::v2::ir::VarName::X),
                    },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        // 14c. `put them back in any order` / `put <subj> on top` / `put <subj> [from <zone>] into/onto/on <dest> [from <zone>]`
        let subject = if let Ok(s) = try_parse(ts, parse_subject_ref) {
            s
        } else if ts.eat_word("one") {
            SubjectRef::OneOfThem(ValueExpr::Const(1))
        } else {
            return Err(ts.err("expected subject after `put`"));
        };
        if ts.eat_phrase(&["back", "in", "any", "order"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(ZoneDest {
                        zone: ZoneKind::Library,
                        owner: ZoneOwner::Default,
                        library_position: Some(crate::v2::ir::LibraryPosition::Top),
                        library_order: Some(crate::v2::ir::LibraryOrder::AnyOrder),
                        under_control: None,
                    }),
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        {
            let t_cp = ts.checkpoint();
            if ts.eat_phrase(&["on", "top"])
                && !ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("of"))
            {
                let order = if ts.eat_phrase(&["in", "any", "order"]) {
                    Some(crate::v2::ir::LibraryOrder::AnyOrder)
                } else if ts.eat_phrase(&["in", "a", "random", "order"]) {
                    Some(crate::v2::ir::LibraryOrder::RandomOrder)
                } else {
                    None
                };
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject,
                        from: Vec::new(),
                        to: Some(ZoneDest {
                            zone: ZoneKind::Library,
                            owner: ZoneOwner::Default,
                            library_position: Some(crate::v2::ir::LibraryPosition::Top),
                            library_order: order,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(t_cp);
        }
        let mut from = Vec::new();
        if ts.eat_word("from") {
            let z1 = parse_zone_ref(ts)?;
            from.push(z1);
            loop {
                let zcp = ts.checkpoint();
                if ts.eat_word("or") || ts.eat_word("and") {
                    let _ = ts.eat_word("from");
                    if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                        from.push(z2);
                        continue;
                    }
                    let bare = if ts.eat_word("graveyard") || ts.eat_word("graveyards") {
                        Some(ZoneKind::Graveyard)
                    } else if ts.eat_word("hand") || ts.eat_word("hands") {
                        Some(ZoneKind::Hand)
                    } else if ts.eat_word("library") || ts.eat_word("libraries") {
                        Some(ZoneKind::Library)
                    } else if ts.eat_word("exile") {
                        Some(ZoneKind::Exile)
                    } else {
                        None
                    };
                    if let Some(zone) = bare {
                        from.push(ZoneRef {
                            zone,
                            owner: z1.owner,
                        });
                        continue;
                    }
                }
                ts.restore(zcp);
                break;
            }
        }
        let (dest, modifiers) = parse_zone_dest(ts)?;
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let _ = ts.eat_phrase(&[
            "instead",
            "of",
            "putting",
            "it",
            "into",
            "your",
            "graveyard",
        ]) || ts.eat_phrase(&[
            "instead",
            "of",
            "putting",
            "it",
            "into",
            "its",
            "owner's",
            "graveyard",
        ]) || ts.eat_phrase(&["instead", "of", "into", "that", "player's", "graveyard"])
            || ts.eat_phrase(&["instead", "of", "into", "its", "owner's", "graveyard"])
            || ts.eat_phrase(&["instead", "of", "into", "their", "graveyard"])
            || ts.eat_phrase(&["instead", "of", "into", "your", "graveyard"]);
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
        let cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(s2) = try_parse(ts, parse_subject_ref)
            && let Ok((d2, m2)) = try_parse(ts, parse_zone_dest)
        {
            out.push(Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: s2,
                from: Vec::new(),
                to: Some(d2),
                modifiers: m2,
                except: None,
                count: None,
            });
        } else {
            ts.restore(cp);
        }
        return Ok((out, ()));
    }

    // 15. `move(s) <cnt> [<kind>] counter(s) from <src> onto/to <dst>` (NEVER stores `"a"`!)
    if ts.eat_word("move") || ts.eat_word("moves") {
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        let kind = if ts.eat_word("counter") || ts.eat_word("counters") {
            CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !(ts.eat_word("counter") || ts.eat_word("counters")) {
                return Err(ts.err("expected `counter(s)` after counter kind"));
            }
            ck
        };
        if !ts.eat_word("from") {
            return Err(ts.err("expected `from` in move counters"));
        }
        let from = parse_subject_ref(ts)?;
        if !(ts.eat_word("onto") || ts.eat_word("to")) {
            return Err(ts.err("expected `onto` or `to` in move counters"));
        }
        let to = parse_subject_ref(ts)?;
        return Ok((
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
        ));
    }

    // 16. `distribute a number of <kind> counters equal to <val> among <subj>`
    if ts.eat_word("distribute") || ts.eat_word("distributes") {
        let count_first = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else {
            Some(parse_value_expr(ts)?)
        };
        let kind = parse_counter_kind(ts)?;
        if !(ts.eat_word("counters") || ts.eat_word("counter")) {
            return Err(ts.err("expected `counters` in distribute"));
        }
        let count = match count_first {
            Some(c) => c,
            None => {
                if !ts.eat_phrase(&["equal", "to"]) {
                    return Err(ts.err("expected `equal to` in distribute"));
                }
                parse_value_expr(ts)?
            }
        };
        if !ts.eat_word("among") {
            return Err(ts.err("expected `among` in distribute"));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::DistributeCounters { kind, count },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 17. `scry(ies) <N>` / `surveil(s) <N>` / `shuffle`
    if ts.eat_word("scry") || ts.eat_word("scries") {
        let amt = parse_value_expr(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::ScaledAction {
                    kind: ScaledPlayerAction::Scry,
                    amount: amt,
                },
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("surveil") || ts.eat_word("surveils") {
        let amt = parse_value_expr(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::ScaledAction {
                    kind: ScaledPlayerAction::Surveil,
                    amount: amt,
                },
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("shuffle") || ts.eat_word("shuffles") {
        // `shuffles their hand and graveyard into their library` / `shuffle <subj> into <lib>`
        {
            let scp = ts.checkpoint();
            if (ts.eat_phrase(&["their", "hand", "and", "graveyard"])
                || ts.eat_phrase(&["your", "hand", "and", "graveyard"]))
                && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject: SubjectRef::HandOf(actor),
                        from: vec![
                            ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: zref.owner,
                            },
                            ZoneRef {
                                zone: ZoneKind::Graveyard,
                                owner: zref.owner,
                            },
                        ],
                        to: Some(ZoneDest {
                            zone: ZoneKind::Library,
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
                ));
            }
            ts.restore(scp);
        }
        // `shuffle <zone> into <lib>` (`shuffle your graveyard into your library`)
        {
            let scp = ts.checkpoint();
            if let Ok(from_z) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("into")
                && let Ok(to_z) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject: SubjectRef::All(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![from_z],
                            ..Default::default()
                        })),
                        from: vec![from_z],
                        to: Some(ZoneDest {
                            zone: to_z.zone,
                            owner: to_z.owner,
                            library_position: Some(crate::v2::ir::LibraryPosition::Shuffled),
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(scp);
        }
        {
            let rcp = ts.checkpoint();
            if (ts.eat_phrase(&["the", "rest", "of", "the", "revealed", "cards"])
                || ts.eat_phrase(&["the", "rest", "of", "those", "cards"])
                || ts.eat_phrase(&["the", "rest"]))
                && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::TheRest),
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
                ));
            }
            ts.restore(rcp);
        }
        {
            let scp = ts.checkpoint();
            if let Ok(subject) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject,
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
                ));
            }
            ts.restore(scp);
        }
        let _ = ts.eat_phrase(&["your", "library"]) || ts.eat_phrase(&["their", "library"]);
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Shuffle,
                subject: SubjectRef::Player(actor),
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 18. `search <zone> [, <zone>, and/or <zone>] for [all | any number of | up to | exactly] <N|X> <flt>`
    if ts.eat_word("search") || ts.eat_word("searches") {
        let mut libs = vec![parse_zone_ref(ts)?];
        loop {
            let zcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_conj = ts.eat_word("and/or") || ts.eat_word("and") || ts.eat_word("or");
            if (had_comma || had_conj)
                && let Ok(z2) = try_parse(ts, parse_zone_ref)
            {
                libs.push(z2);
                continue;
            }
            if (had_comma || had_conj)
                && let Some(wk) = ts.peek_word().map(|w| w.to_ascii_lowercase())
                && matches!(wk.as_str(), "hand" | "graveyard" | "library")
            {
                ts.next_word();
                let zone = match wk.as_str() {
                    "hand" => ZoneKind::Hand,
                    "graveyard" => ZoneKind::Graveyard,
                    _ => ZoneKind::Library,
                };
                let owner = libs.first().map_or(ZoneOwner::Default, |z| z.owner);
                libs.push(ZoneRef { zone, owner });
                continue;
            }
            ts.restore(zcp);
            break;
        }
        if !ts.eat_word("for") {
            return Err(ts.err("expected `for` in search library"));
        }
        let any_number = ts.eat_phrase(&["any", "number", "of"]) || ts.eat_word("all");
        let up_to = !any_number && ts.eat_phrase(&["up", "to"]);
        let exact = !any_number && !up_to && ts.eat_word("exactly");
        let count = if any_number {
            ValueExpr::Const(0)
        } else if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let flt = parse_object_filter(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::SearchLibrary {
                    reveal: false,
                    shuffle: false,
                    exact,
                },
                subject: SubjectRef::Group(Box::new(flt)),
                from: libs,
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: up_to || (!exact && !any_number),
                    any_number,
                }),
            }],
            (),
        ));
    }

    // 19. `reveal(s) ...`
    if ts.eat_word("reveal") || ts.eat_word("reveals") {
        // `reveal(s) [a number of cards | <val> cards] from their/your hand [equal to <val>]` (`Acquisitions Expert`, `Taster of Wares`)
        {
            let hcp = ts.checkpoint();
            let mut deferred = false;
            let cnt_opt = if ts.eat_phrase(&["a", "number", "of", "cards"]) {
                deferred = true;
                Some(ValueExpr::Const(1))
            } else if let Ok(v) = try_parse(ts, parse_value_expr)
                && (ts.eat_word("cards") || ts.eat_word("card"))
            {
                Some(v)
            } else {
                None
            };
            if let Some(cnt) = cnt_opt
                && ts.eat_word("from")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                let final_cnt = if deferred && ts.eat_phrase(&["equal", "to"]) {
                    parse_value_expr(ts)?
                } else {
                    cnt
                };
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Reveal,
                        subject: SubjectRef::Player(actor),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: Some(CountSpec {
                            count: final_cnt,
                            up_to: false,
                            any_number: false,
                        }),
                    }],
                    (),
                ));
            }
            ts.restore(hcp);
        }
        {
            let rcp = ts.checkpoint();
            if ts.eat_phrase(&["cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("until")
                && opt(ts, parse_player_ref).is_some()
                && (ts.eat_word("reveal") || ts.eat_word("reveals"))
            {
                let _ = opt(ts, parse_value_expr);
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    return Ok((
                        vec![Effect::ZoneOp {
                            actor,
                            op: ZoneOpKind::ExileUntilReveal {
                                filter: Box::new(flt),
                            },
                            subject: SubjectRef::Player(actor),
                            from: vec![zref],
                            to: None,
                            modifiers: EntryModifiers::default(),
                            except: None,
                            count: None,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(rcp);
        }
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("cards") || ts.eat_word("card")) {
                    return Err(ts.err("expected `card(s)` after `reveal the top`"));
                }
                n
            };
            let zref = if ts.eat_word("of") {
                parse_zone_ref(ts)?
            } else {
                ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::You,
                }
            };
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: true },
                    subject: SubjectRef::Player(actor),
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
            ));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Reveal,
                subject,
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }
    if ts.eat_phrase(&["look", "at"]) || ts.eat_phrase(&["looks", "at"]) {
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("cards") || ts.eat_word("card")) {
                    return Err(ts.err("expected `card(s)` after `look at the top`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of <library>` after `look at the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: false },
                    subject: SubjectRef::Player(actor),
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
            ));
        }
        // `look at <val> cards from the top of <lib>` (e.g. `Look at that many cards from the top of your library`)
        let lcp = ts.checkpoint();
        if let Ok(count) = try_parse(ts, parse_value_expr)
            && (ts.eat_word("cards") || ts.eat_word("card"))
            && ts.eat_phrase(&["from", "the", "top", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: false },
                    subject: SubjectRef::Player(actor),
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
            ));
        }
        ts.restore(lcp);
        // `look at <subj>` (`Look at target opponent's hand`, `Look at a card at random in target player's hand`)
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookAt,
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        return Err(ts.err("expected target after `look at`"));
    }

    // 20. `separate(s) them into a face-down pile and a face-up pile`
    if ts.eat_word("separate") || ts.eat_word("separates") {
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&[
            "into",
            "a",
            "face-down",
            "pile",
            "and",
            "a",
            "face-up",
            "pile",
        ]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 1,
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["into", "two", "piles"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 0,
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        return Err(ts.err("expected pile specification after `separate`"));
    }

    Err(ts.err("expected player verb frame"))
}

fn parse_exile_with_counters(ts: &mut TokenStream<'_>) -> Option<(CounterKind, ValueExpr)> {
    if !ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("with"))
    {
        return None;
    }
    let wcp = ts.checkpoint();
    ts.eat_word("with");
    if ts.eat_phrase(&["a", "number", "of"]) {
        if let Ok(ck) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_phrase(&["on", "it", "equal", "to"])
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            return Some((ck, val));
        }
        ts.restore(wcp);
        return None;
    }
    let val = if ts.eat_word("a") || ts.eat_word("an") {
        ValueExpr::Const(1)
    } else if let Ok(v) = try_parse(ts, parse_value_expr) {
        v
    } else {
        ts.restore(wcp);
        return None;
    };
    if let Ok(ck) = try_parse(ts, parse_counter_kind)
        && (ts.eat_word("counters") || ts.eat_word("counter"))
    {
        let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
        return Some((ck, val));
    }
    ts.restore(wcp);
    None
}
