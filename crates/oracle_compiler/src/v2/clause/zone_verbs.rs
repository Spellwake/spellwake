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
    AnaphoraRef, ChoiceKind, CountSpec, CounterKind, Duration, Effect, ObjectFilter,
    ObjectMutation, ObjectStateAction, PlayerActionKind, PlayerMutation, PlayerRef,
    ScaledPlayerAction, SubjectRef, ValueExpr, ZoneDest, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

use super::{
    all_zone_cards_subject, counted_zone_op, exact_count, exile_zone_op, move_zone_op, mutate_obj,
    mutate_player, parse_from_zone_list, player_action, prompt_choice, shuffled_zone_dest,
    simple_zone_dest, simple_zone_op, zone_cards_subject,
};

const ZONE_VERBS: &[&str] = &[
    "draw",
    "draws",
    "discard",
    "discards",
    "sacrifice",
    "sacrifices",
    "create",
    "creates",
    "destroy",
    "destroys",
    "exile",
    "exiles",
    "return",
    "returns",
    "put",
    "puts",
    "move",
    "moves",
    "distribute",
    "distributes",
    "scry",
    "scries",
    "surveil",
    "surveils",
    "shuffle",
    "shuffles",
    "search",
    "searches",
    "reveal",
    "reveals",
    "look",
    "looks",
    "separate",
    "separates",
];

const INSTEAD_OF_GY: &[&[&str]] = &[
    &[
        "instead",
        "of",
        "putting",
        "it",
        "into",
        "your",
        "graveyard",
    ],
    &[
        "instead",
        "of",
        "putting",
        "it",
        "into",
        "its",
        "owner's",
        "graveyard",
    ],
    &["instead", "of", "into", "that", "player's", "graveyard"],
    &["instead", "of", "into", "its", "owner's", "graveyard"],
    &["instead", "of", "into", "their", "graveyard"],
    &["instead", "of", "into", "your", "graveyard"],
];

/// Dispatch zone-operation and counter-manipulation verbs with a known player `actor`.
pub(super) fn try_parse_zone_verb_frame(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let w = ts.peek_word()?.to_ascii_lowercase();
    if !ZONE_VERBS.contains(&w.as_str()) {
        return None;
    }
    Some(parse_zone_verb_frame_inner(ts, actor))
}

fn parse_zone_verb_frame_inner(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    let default_hand = || {
        vec![ZoneRef {
            zone: ZoneKind::Hand,
            owner: ZoneOwner::Default,
        }]
    };
    let default_bf = || {
        vec![ZoneRef {
            zone: ZoneKind::Battlefield,
            owner: ZoneOwner::Default,
        }]
    };
    // 7. `draw(s) ...`
    if ts.eat_any_word(&["draw", "draws"]) {
        if let Some(res) = super::zone_ext::try_parse_draw_ext(ts, actor) {
            return res;
        }
        let count = if ts.eat_any_phrase(&[
            &["a", "number", "of", "cards", "equal", "to"],
            &["cards", "equal", "to"],
        ]) {
            parse_value_expr(ts)?
        } else {
            let base = if ts.eat_any_word(&["a", "an"]) {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let _ = ts.eat_word("additional");
            if !ts.eat_any_word(&["card", "cards"]) {
                return Err(ts.err("expected `card(s)` after `draw`"));
            }
            let _ = ts.eat_rounded_modifier();
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
            vec![counted_zone_op(
                actor,
                ZoneOpKind::Draw,
                SubjectRef::Player(actor),
                vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                Some(simple_zone_dest(ZoneKind::Hand, ZoneOwner::Default)),
                Some(exact_count(count)),
            )],
            (),
        ));
    }

    // 8. `discard(s) ...`
    if ts.eat_any_word(&["discard", "discards"]) {
        if let Some(res) = super::zone_ext::try_parse_discard_ext(ts, actor) {
            return res;
        }
        if ts.eat_any_phrase(&[&["their", "hand"], &["your", "hand"]]) {
            return Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::Discard {
                        hand: true,
                        random: false,
                    },
                    SubjectRef::Player(actor),
                    default_hand(),
                    None,
                )],
                (),
            ));
        }
        if ts.eat_phrase(&["any", "number", "of"]) {
            let flt = parse_object_filter(ts)?;
            return Ok((
                vec![counted_zone_op(
                    actor,
                    ZoneOpKind::Discard {
                        hand: false,
                        random: false,
                    },
                    SubjectRef::Group(Box::new(flt)),
                    default_hand(),
                    None,
                    Some(CountSpec {
                        count: ValueExpr::Const(0),
                        up_to: false,
                        any_number: true,
                    }),
                )],
                (),
            ));
        }
        let up_to = ts.eat_phrase(&["up", "to"]);
        {
            let dcp = ts.checkpoint();
            if !up_to && let Ok(subj) = try_parse(ts, parse_subject_ref) {
                let random = ts.eat_phrase(&["at", "random"]);
                if !matches!(subj, SubjectRef::Counted { .. }) {
                    return Ok((
                        vec![simple_zone_op(
                            actor,
                            ZoneOpKind::Discard {
                                hand: false,
                                random,
                            },
                            subj,
                            default_hand(),
                            None,
                        )],
                        (),
                    ));
                }
            }
            ts.restore(dcp);
        }
        let (cnt, random) = if ts.eat_phrase(&["a", "number", "of", "cards", "equal", "to"]) {
            let v = parse_value_expr(ts)?;
            (v, ts.eat_phrase(&["at", "random"]))
        } else {
            let c = if ts.eat_word("a") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            if !ts.eat_any_word(&["card", "cards"]) {
                return Err(ts.err("expected `card(s)` after `discard`"));
            }
            (c, ts.eat_phrase(&["at", "random"]))
        };
        return Ok((
            vec![counted_zone_op(
                actor,
                ZoneOpKind::Discard {
                    hand: false,
                    random,
                },
                SubjectRef::Player(actor),
                default_hand(),
                None,
                Some(CountSpec {
                    count: cnt,
                    up_to,
                    any_number: false,
                }),
            )],
            (),
        ));
    }

    // 9. `sacrifice(s) ...`
    if ts.eat_any_word(&["sacrifice", "sacrifices"]) {
        if let Some(res) = super::zone_ext::try_parse_sacrifice_half(ts, actor) {
            return res;
        }
        if ts.eat_phrase(&["the", "rest"]) {
            return Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::Sacrifice,
                    SubjectRef::Anaphora(AnaphoraRef::TheRest),
                    default_bf(),
                    None,
                )],
                (),
            ));
        }
        let is_that_many = ts.peek_phrase(&["that", "many"]);
        let is_explicit_subject = !is_that_many
            && (matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Tilde))
                || ts.peek_word().is_some_and(|w| {
                    ["it", "them", "all", "that", "those"]
                        .iter()
                        .any(|&kw| w.eq_ignore_ascii_case(kw))
                }));
        if is_explicit_subject {
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::Sacrifice,
                    subject,
                    default_bf(),
                    None,
                )],
                (),
            ));
        }
        let any_number = ts.eat_phrase(&["any", "number", "of"]);
        let count = if any_number {
            ValueExpr::Const(0)
        } else if ts.eat_any_word(&["a", "an"]) {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let _ = ts.eat_phrase(&["of", "those"]);
        let flt = parse_object_filter(ts)?;
        let mut out = vec![counted_zone_op(
            actor,
            ZoneOpKind::Sacrifice,
            SubjectRef::Group(Box::new(flt)),
            default_bf(),
            None,
            Some(CountSpec {
                count,
                up_to: false,
                any_number,
            }),
        )];
        loop {
            let scp = ts.checkpoint();
            if ts.eat_word("and") {
                let c2 = if ts.eat_any_word(&["a", "an"]) {
                    ValueExpr::Const(1)
                } else {
                    try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                };
                if let Ok(flt2) = try_parse(ts, parse_object_filter) {
                    out.push(counted_zone_op(
                        actor,
                        ZoneOpKind::Sacrifice,
                        SubjectRef::Group(Box::new(flt2)),
                        default_bf(),
                        None,
                        Some(exact_count(c2)),
                    ));
                    continue;
                }
            }
            ts.restore(scp);
            break;
        }
        return Ok((out, ()));
    }

    // 10. `create(s) [your choice of] <token1> [, <token2>, and/or <token3>]`
    if ts.eat_any_word(&["create", "creates"]) {
        let is_choice = ts.eat_any_phrase(&[&["your", "choice", "of"], &["their", "choice", "of"]]);
        let (count, spec, modifiers) = parse_token_creation(ts)?;
        let mut out = vec![Effect::CreateToken {
            controller: actor,
            count,
            spec,
            modifiers,
        }];
        let mut saw_or = is_choice;
        loop {
            let tcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_or = ts.eat_word("or");
            let had_and = !had_or && ts.eat_word("and");
            if (had_comma || had_and || had_or)
                && let Ok((c2, s2, m2)) = try_parse(ts, parse_token_creation)
            {
                if had_or {
                    saw_or = true;
                }
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
        if saw_or && out.len() >= 2 {
            return Ok((
                vec![prompt_choice(
                    actor,
                    ChoiceKind::ChooseBranch(out.into_iter().map(|e| vec![e]).collect()),
                )],
                (),
            ));
        }
        return Ok((out, ()));
    }

    // 11. `destroy(s) <subj>`
    if ts.eat_any_word(&["destroy", "destroys"]) {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![simple_zone_op(
                actor,
                ZoneOpKind::Destroy {
                    can_regenerate: true,
                },
                subject,
                Vec::new(),
                None,
            )],
            (),
        ));
    }

    // 12. `exile(s) ...`
    if ts.eat_any_word(&["exile", "exiles"]) {
        if let Some(res) = super::zone_ext::try_parse_exile_ext(ts, actor) {
            return res;
        }
        if ts.eat_any_phrase(&[&["their", "hand"], &["your", "hand"]]) {
            let face_down = ts.eat_phrase(&["face", "down"]);
            let zr = ZoneRef {
                zone: ZoneKind::Hand,
                owner: ZoneOwner::ThatPlayer,
            };
            return Ok((
                vec![exile_zone_op(
                    actor,
                    all_zone_cards_subject(zr),
                    vec![zr],
                    face_down,
                    None,
                    None,
                )],
                (),
            ));
        }
        if ts.eat_any_phrase(&[
            &["any", "number", "of", "target", "players'", "graveyards"],
            &["any", "number", "of", "target", "opponents'", "graveyards"],
            &["all", "opponents'", "graveyards"],
            &["all", "players'", "graveyards"],
        ]) {
            let zref = ZoneRef {
                zone: ZoneKind::Graveyard,
                owner: ZoneOwner::Opponents,
            };
            return Ok((
                vec![exile_zone_op(
                    actor,
                    all_zone_cards_subject(zref),
                    vec![zref],
                    false,
                    None,
                    None,
                )],
                (),
            ));
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_phrase(&["any", "number", "of"])
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                let from = parse_from_zone_list(ts)?;
                let face_down =
                    ts.eat_phrase(&["face", "down"]) || super::zone_ext::eat_exile_pile_suffix(ts);
                let counters = parse_exile_with_counters(ts);
                let ex_dur = parse_trailing_duration(ts);
                let eff = exile_zone_op(
                    actor,
                    subject,
                    from,
                    face_down,
                    counters,
                    Some(CountSpec {
                        count: ValueExpr::Const(0),
                        up_to: false,
                        any_number: true,
                    }),
                );
                let eff = match ex_dur {
                    Some(duration) => Effect::ScopedDuration {
                        duration,
                        body: vec![eff],
                    },
                    None => eff,
                };
                return Ok((vec![eff], ()));
            }
            ts.restore(acp);
        }
        {
            let zcp = ts.checkpoint();
            if let Ok(zref) = try_parse(ts, parse_zone_ref)
                && zref.zone == ZoneKind::Graveyard
            {
                let mut zones = vec![zref];
                loop {
                    let lcp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let had_conj = ts.eat_any_word(&["and/or", "and", "or"]);
                    if (had_comma || had_conj)
                        && let Ok(mut z2) = try_parse(ts, parse_zone_ref)
                    {
                        if z2.owner == ZoneOwner::Default
                            && matches!(
                                z2.zone,
                                ZoneKind::Graveyard | ZoneKind::Hand | ZoneKind::Library
                            )
                        {
                            z2.owner = zref.owner;
                        }
                        zones.push(z2);
                        continue;
                    }
                    ts.restore(lcp);
                    break;
                }
                let subject = SubjectRef::All(Box::new(ObjectFilter {
                    any_card: true,
                    zones: zones.clone(),
                    ..Default::default()
                }));
                return Ok((
                    vec![exile_zone_op(actor, subject, zones, false, None, None)],
                    (),
                ));
            }
            ts.restore(zcp);
        }
        {
            let ecp = ts.checkpoint();
            if ts.eat_phrase(&["cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("until")
                && opt(ts, parse_player_ref).is_some()
                && ts.eat_any_word(&["reveal", "reveals", "exile", "exiles"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok((
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::ExileUntilReveal {
                            filter: Box::new(flt),
                        },
                        SubjectRef::Player(actor),
                        vec![zref],
                        None,
                    )],
                    (),
                ));
            }
            ts.restore(ecp);
        }
        {
            let ttop_cp = ts.checkpoint();
            if let Ok(count) = try_parse(ts, parse_value_expr)
                && ts.eat_any_word(&["cards", "card"])
                && ts.eat_phrase(&["from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                let face_down =
                    ts.eat_phrase(&["face", "down"]) || super::zone_ext::eat_exile_pile_suffix(ts);
                let counters = parse_exile_with_counters(ts);
                return Ok((
                    vec![exile_zone_op(
                        actor,
                        zone_cards_subject(zref),
                        vec![zref],
                        face_down,
                        counters,
                        Some(exact_count(count)),
                    )],
                    (),
                ));
            }
            ts.restore(ttop_cp);
        }
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of", "cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
            {
                return Ok((
                    vec![exile_zone_op(
                        actor,
                        zone_cards_subject(zref),
                        vec![zref],
                        false,
                        None,
                        Some(exact_count(count)),
                    )],
                    (),
                ));
            }
            ts.restore(ncp);
        }
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !ts.eat_any_word(&["card", "cards"]) {
                    return Err(ts.err("expected `card(s)` after `the top <N>`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of` after `the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            let face_down =
                ts.eat_phrase(&["face", "down"]) || super::zone_ext::eat_exile_pile_suffix(ts);
            let counters = parse_exile_with_counters(ts);
            return Ok((
                vec![exile_zone_op(
                    actor,
                    zone_cards_subject(zref),
                    vec![zref],
                    face_down,
                    counters,
                    Some(exact_count(count)),
                )],
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
        let from = parse_from_zone_list(ts)?;
        let extra_top_exile = super::zone_ext::try_eat_and_top_cards(ts);
        let face_down =
            ts.eat_phrase(&["face", "down"]) || super::zone_ext::eat_exile_pile_suffix(ts);
        let counters = parse_exile_with_counters(ts);
        let ex_dur = parse_trailing_duration(ts);
        let ate_instead_gy = ts.eat_any_phrase(&[
            &[
                "instead",
                "of",
                "putting",
                "it",
                "into",
                "its",
                "owner's",
                "graveyard",
            ],
            &[
                "instead",
                "of",
                "putting",
                "them",
                "into",
                "their",
                "owners'",
                "graveyards",
            ],
            &[
                "instead",
                "of",
                "putting",
                "it",
                "into",
                "your",
                "graveyard",
            ],
            &["instead", "of", "putting", "it", "into", "a", "graveyard"],
            &[
                "instead",
                "of",
                "putting",
                "it",
                "into",
                "their",
                "graveyard",
            ],
        ]);
        if ate_instead_gy {
            let _ = ts.eat_phrase(&["as", "it", "resolves"]);
        }
        let eff = exile_zone_op(actor, subject, from, face_down, counters, None);
        let eff = match ex_dur {
            Some(duration) => Effect::ScopedDuration {
                duration,
                body: vec![eff],
            },
            None => eff,
        };
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
        super::zone_ext::push_extra_top_exile(&mut out_exile, actor, face_down, extra_top_exile);
        if let Some(zr) = extra_exile_zone {
            out_exile.push(exile_zone_op(
                actor,
                all_zone_cards_subject(zr),
                vec![zr],
                false,
                None,
                None,
            ));
        }
        return Ok((out_exile, ()));
    }

    // 13. `return(s) ...`
    if ts.eat_any_word(&["return", "returns"]) {
        if let Some(res) = super::zone_ext::try_parse_return_ext(ts, actor) {
            return res;
        }
        if ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("to")) {
            let (dest, modifiers) = parse_zone_dest(ts)?;
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![move_zone_op(
                    actor,
                    subject,
                    Vec::new(),
                    dest,
                    modifiers,
                    None,
                )],
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
        let mut from = parse_from_zone_list(ts)?;
        let mut except = {
            let ecp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("except") {
                let _ = ts.eat_word("for");
                Some(Box::new(parse_subject_ref(ts)?))
            } else {
                ts.restore(ecp);
                None
            }
        };
        let (mut dest, modifiers) = parse_zone_dest(ts)?;
        super::zone_ext::parse_trailing_under_control(ts, &mut dest);
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        if except.is_none() {
            let ecp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("except") {
                let _ = ts.eat_word("for");
                if let Ok(ex_subj) = try_parse(ts, parse_subject_ref) {
                    except = Some(Box::new(ex_subj));
                } else {
                    ts.restore(ecp);
                }
            } else {
                ts.restore(ecp);
            }
        }
        let _ = ts.eat_any_phrase(INSTEAD_OF_GY);
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

    // 14. `put(s) ...`
    if ts.eat_any_word(&["put", "puts"]) {
        {
            let scp = ts.checkpoint();
            if ts.eat_any_word(&["a", "an"])
                && ts.eat_any_phrase(&[
                    &["name", "sticker", "on"],
                    &["ability", "sticker", "on"],
                    &["art", "sticker", "on"],
                    &["sticker", "on"],
                ])
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![mutate_obj(
                        subject,
                        ObjectMutation::StateAction(ObjectStateAction::PutSticker),
                        Duration::Permanent,
                    )],
                    (),
                ));
            }
            ts.restore(scp);
        }
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counters", "counter"])
            {
                let pair = if ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                    && ts.eat_phrase(&["equal", "to"])
                    && let Ok(count) = try_parse(ts, parse_value_expr)
                {
                    Some((subject, count))
                } else if ts.eat_phrase(&["equal", "to"])
                    && let Ok(count) = try_parse(ts, parse_value_expr)
                    && ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    Some((subject, count))
                } else {
                    None
                };
                if let Some((subject, count)) = pair {
                    return Ok((
                        vec![mutate_obj(
                            subject,
                            ObjectMutation::PutCounters { kind, count },
                            Duration::Instant,
                        )],
                        (),
                    ));
                }
            }
            ts.restore(cp);
        }
        {
            let ccp = ts.checkpoint();
            if ts.eat_any_phrase(&[&["your", "choice", "of"], &["their", "choice", "of"]]) {
                let mut pairs: Vec<(ValueExpr, CounterKind)> = Vec::new();
                loop {
                    let icp = ts.checkpoint();
                    let cnt = if ts.eat_any_word(&["a", "an"]) {
                        Some(ValueExpr::Const(1))
                    } else if let Ok(v) = try_parse(ts, parse_value_expr) {
                        Some(v)
                    } else if !pairs.is_empty() {
                        Some(ValueExpr::Const(1))
                    } else {
                        None
                    };
                    if let Some(c) = cnt
                        && let Ok(k) = try_parse(ts, parse_counter_kind)
                    {
                        let _ = ts.eat_any_word(&["counter", "counters"]);
                        pairs.push((c, k));
                        let sep_cp = ts.checkpoint();
                        let had_comma = ts.eat_comma();
                        let had_or = ts.eat_word("or");
                        if had_comma || had_or {
                            continue;
                        }
                        ts.restore(sep_cp);
                        break;
                    }
                    ts.restore(icp);
                    break;
                }
                if pairs.len() >= 2 && ts.eat_word("on") {
                    let subject = parse_subject_ref(ts)?;
                    let branches = pairs
                        .into_iter()
                        .map(|(count, kind)| {
                            vec![mutate_obj(
                                subject.clone(),
                                ObjectMutation::PutCounters { kind, count },
                                Duration::Instant,
                            )]
                        })
                        .collect();
                    return Ok((
                        vec![prompt_choice(actor, ChoiceKind::ChooseBranch(branches))],
                        (),
                    ));
                }
            }
            ts.restore(ccp);
        }
        {
            let cp = ts.checkpoint();
            let base_cnt = if ts.eat_any_word(&["a", "an", "another"]) {
                Some(ValueExpr::Const(1))
            } else {
                opt(ts, parse_value_expr)
            };
            let kind_opt = if ts.eat_any_phrase(&[
                &["counter", "of", "that", "kind"],
                &["counters", "of", "that", "kind"],
            ]) {
                Some(CounterKind::Any)
            } else if let Ok(k) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counter", "counters"])
            {
                Some(k)
            } else {
                None
            };
            if let Some(base) = base_cnt
                && let Some(kind) = kind_opt
            {
                let mut pairs = vec![(base, kind)];
                let mut choice_branches: Vec<Vec<CounterKind>> = Vec::new();
                let mut is_choice = false;
                loop {
                    let acp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let sep_or = ts.eat_word("or");
                    let sep_and = ts.eat_word("and");
                    if had_comma || sep_or || sep_and {
                        let fcp = ts.checkpoint();
                        if ts.eat_any_phrase(&[
                            &["a", "counter", "from", "among"],
                            &["your", "choice", "of", "a", "counter", "from", "among"],
                        ]) {
                            let mut opts = Vec::new();
                            while let Ok(k) = try_parse(ts, parse_counter_kind) {
                                opts.push(k);
                                let _ = ts.eat_comma();
                                let _ = ts.eat_any_word(&["or", "and"]);
                            }
                            if !opts.is_empty() {
                                choice_branches.push(opts);
                                continue;
                            }
                        }
                        ts.restore(fcp);
                        let c2 = if ts.eat_any_word(&["a", "an", "another"]) {
                            Some(ValueExpr::Const(1))
                        } else {
                            opt(ts, parse_value_expr)
                        };
                        if let Some(c2) = c2
                            && let Ok(k2) = try_parse(ts, parse_counter_kind)
                            && ts.eat_any_word(&["counter", "counters"])
                        {
                            if sep_or {
                                is_choice = true;
                            }
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
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::PutCounters { kind: k, count },
                            Duration::Instant,
                        ));
                    }
                    for opts in choice_branches {
                        let branches = opts
                            .into_iter()
                            .map(|k| {
                                vec![mutate_obj(
                                    subject.clone(),
                                    ObjectMutation::PutCounters {
                                        kind: k,
                                        count: ValueExpr::Const(1),
                                    },
                                    Duration::Instant,
                                )]
                            })
                            .collect();
                        out.push(prompt_choice(actor, ChoiceKind::ChooseBranch(branches)));
                    }
                    loop {
                        let ccp = ts.checkpoint();
                        if ts.eat_word("and") {
                            let c2 = if ts.eat_any_word(&["a", "an"]) {
                                Some(ValueExpr::Const(1))
                            } else {
                                opt(ts, parse_value_expr)
                            };
                            if let Some(c2) = c2
                                && let Ok(k2) = try_parse(ts, parse_counter_kind)
                                && ts.eat_any_word(&["counter", "counters"])
                                && ts.eat_word("on")
                                && let Ok(subj2) = try_parse(ts, parse_subject_ref)
                            {
                                out.push(mutate_obj(
                                    subj2,
                                    ObjectMutation::PutCounters {
                                        kind: k2,
                                        count: c2,
                                    },
                                    Duration::Instant,
                                ));
                                continue;
                            }
                        }
                        ts.restore(ccp);
                        break;
                    }
                    if is_choice {
                        return Ok((
                            vec![prompt_choice(
                                actor,
                                ChoiceKind::ChooseBranch(
                                    out.into_iter().map(|e| vec![e]).collect(),
                                ),
                            )],
                            (),
                        ));
                    }
                    {
                        let rcp = ts.checkpoint();
                        if out.len() == 1
                            && ts.eat_any_phrase(&[
                                &["or", "remove", "one", "from", "it"],
                                &["or", "remove", "a", "counter", "from", "it"],
                            ])
                        {
                            let put_eff = out.remove(0);
                            let kind = match &put_eff {
                                Effect::MutateObject {
                                    mutation: ObjectMutation::PutCounters { kind, .. },
                                    ..
                                } => kind.clone(),
                                _ => CounterKind::Any,
                            };
                            let rem_eff = mutate_obj(
                                subject.clone(),
                                ObjectMutation::RemoveCounters {
                                    kind,
                                    count: Some(ValueExpr::Const(1)),
                                    all: false,
                                },
                                Duration::Instant,
                            );
                            return Ok((
                                vec![prompt_choice(
                                    actor,
                                    ChoiceKind::ChooseBranch(vec![vec![put_eff], vec![rem_eff]]),
                                )],
                                (),
                            ));
                        }
                        ts.restore(rcp);
                    }
                    return Ok((out, ()));
                }
            }
            ts.restore(cp);
        }
        if ts.eat_phrase(&["its", "counters", "on"]) {
            let to = parse_subject_ref(ts)?;
            return Ok((
                vec![mutate_obj(
                    SubjectRef::SelfObject,
                    ObjectMutation::MoveCounters {
                        from: Box::new(SubjectRef::SelfObject),
                        to: Box::new(to),
                        kind: CounterKind::Any,
                        count: ValueExpr::Var(crate::v2::ir::VarName::X),
                    },
                    Duration::Instant,
                )],
                (),
            ));
        }
        if let Some(res) = super::zone_ext::try_parse_put_ext(ts, actor) {
            return res;
        }
        let put_count = {
            let pcp = ts.checkpoint();
            if ts.eat_phrase(&["up", "to"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
                && !ts.peek_phrase(&["of", "them"])
            {
                Some(CountSpec {
                    count: val,
                    up_to: true,
                    any_number: false,
                })
            } else {
                ts.restore(pcp);
                None
            }
        };
        let is_random_put = ts.eat_phrase(&["a", "random"]) || ts.eat_word("random");
        let subject = if let Ok(mut s) = try_parse(ts, parse_subject_ref) {
            if is_random_put && let SubjectRef::Group(ref mut flt) = s {
                flt.flags = flt.flags.union(crate::v2::ir::FilterFlags::AT_RANDOM);
            }
            s
        } else if ts.eat_word("one") {
            SubjectRef::OneOfThem(ValueExpr::Const(1))
        } else {
            return Err(ts.err("expected subject after `put`"));
        };
        if ts.eat_phrase(&["back", "in", "any", "order"]) {
            return Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::Move,
                    subject,
                    Vec::new(),
                    Some(ZoneDest {
                        zone: ZoneKind::Library,
                        owner: ZoneOwner::Default,
                        library_position: Some(crate::v2::ir::LibraryPosition::Top),
                        library_order: Some(crate::v2::ir::LibraryOrder::AnyOrder),
                        under_control: None,
                    }),
                )],
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
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::Move,
                        subject,
                        Vec::new(),
                        Some(ZoneDest {
                            zone: ZoneKind::Library,
                            owner: ZoneOwner::Default,
                            library_position: Some(crate::v2::ir::LibraryPosition::Top),
                            library_order: order,
                            under_control: None,
                        }),
                    )],
                    (),
                ));
            }
            ts.restore(t_cp);
        }
        let mut from = parse_from_zone_list(ts)?;
        let (mut dest, modifiers) = parse_zone_dest(ts)?;
        super::zone_ext::parse_trailing_under_control(ts, &mut dest);
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let _ = ts.eat_any_phrase(INSTEAD_OF_GY);
        let first_put = move_zone_op(
            actor,
            subject.clone(),
            from.clone(),
            dest,
            modifiers,
            put_count.clone(),
        );
        let mut out = super::zone_ext::maybe_wrap_put_or_dest(
            ts, actor, first_put, &subject, &from, &put_count,
        );
        let cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(s2) = try_parse(ts, parse_subject_ref)
            && let Ok((mut d2, m2)) = try_parse(ts, parse_zone_dest)
        {
            super::zone_ext::parse_trailing_under_control(ts, &mut d2);
            out.push(move_zone_op(actor, s2, Vec::new(), d2, m2, None));
        } else {
            ts.restore(cp);
        }
        return Ok((out, ()));
    }

    // 15. `move(s) [all | any number of | <cnt>] [<kind>] counter(s) from [among] <src> onto/to <dst>`
    if ts.eat_any_word(&["move", "moves"]) {
        if let Some(res) = super::zone_ext::try_parse_move_ext(ts) {
            return res;
        }
        let count = if ts.eat_phrase(&["any", "number", "of"]) {
            ValueExpr::Const(0)
        } else if ts.eat_any_word(&["a", "an"]) {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        let kind = if ts.eat_any_word(&["counter", "counters"]) {
            CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !ts.eat_any_word(&["counter", "counters"]) {
                return Err(ts.err("expected `counter(s)` after counter kind"));
            }
            ck
        };
        if !ts.eat_word("from") {
            return Err(ts.err("expected `from` in move counters"));
        }
        let from = parse_subject_ref(ts)?;
        if !ts.eat_any_word(&["onto", "to"]) {
            return Err(ts.err("expected `onto` or `to` in move counters"));
        }
        let to = parse_subject_ref(ts)?;
        return Ok((
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
        ));
    }

    // 16. `distribute a number of <kind> counters equal to <val> among <subj>`
    if ts.eat_any_word(&["distribute", "distributes"]) {
        let count_first = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else {
            Some(parse_value_expr(ts)?)
        };
        let kind = parse_counter_kind(ts)?;
        if !ts.eat_any_word(&["counters", "counter"]) {
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
            vec![mutate_obj(
                subject,
                ObjectMutation::DistributeCounters { kind, count },
                Duration::Instant,
            )],
            (),
        ));
    }

    // 17. `scry(ies) <N>` / `surveil(s) <N>` / `shuffle`
    if ts.eat_any_word(&["scry", "scries"]) {
        let amount = parse_value_expr(ts)?;
        return Ok((
            vec![mutate_player(
                actor,
                PlayerMutation::ScaledAction {
                    kind: ScaledPlayerAction::Scry,
                    amount,
                },
                Duration::Instant,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["surveil", "surveils"]) {
        let amount = parse_value_expr(ts)?;
        return Ok((
            vec![mutate_player(
                actor,
                PlayerMutation::ScaledAction {
                    kind: ScaledPlayerAction::Surveil,
                    amount,
                },
                Duration::Instant,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["shuffle", "shuffles"]) {
        if let Some(res) = super::zone_ext::try_parse_shuffle_ext(ts, actor) {
            return res;
        }
        {
            let scp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["their", "hand", "and", "graveyard"],
                &["your", "hand", "and", "graveyard"],
                &["their", "graveyard", "and", "hand"],
                &["your", "graveyard", "and", "hand"],
            ]) && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::Shuffle,
                        SubjectRef::HandOf(actor),
                        vec![
                            ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: zref.owner,
                            },
                            ZoneRef {
                                zone: ZoneKind::Graveyard,
                                owner: zref.owner,
                            },
                        ],
                        Some(shuffled_zone_dest(ZoneKind::Library, zref.owner)),
                    )],
                    (),
                ));
            }
            ts.restore(scp);
        }
        {
            let scp = ts.checkpoint();
            if let Ok(from_z) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("into")
                && let Ok(to_z) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::Shuffle,
                        all_zone_cards_subject(from_z),
                        vec![from_z],
                        Some(shuffled_zone_dest(to_z.zone, to_z.owner)),
                    )],
                    (),
                ));
            }
            ts.restore(scp);
        }
        {
            let rcp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["the", "rest", "of", "the", "revealed", "cards"],
                &["the", "rest", "of", "those", "cards"],
                &["the", "rest"],
            ]) && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::Shuffle,
                        SubjectRef::Anaphora(AnaphoraRef::TheRest),
                        Vec::new(),
                        Some(shuffled_zone_dest(zref.zone, zref.owner)),
                    )],
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
                let mut from = Vec::new();
                if ts.eat_word("from")
                    && let Ok(fz) = try_parse(ts, parse_zone_ref)
                {
                    from.push(fz);
                }
                return Ok((
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::Shuffle,
                        subject,
                        from,
                        Some(shuffled_zone_dest(zref.zone, zref.owner)),
                    )],
                    (),
                ));
            }
            ts.restore(scp);
        }
        let _ = ts.eat_any_phrase(&[&["your", "library"], &["their", "library"]]);
        return Ok((
            vec![simple_zone_op(
                actor,
                ZoneOpKind::Shuffle,
                SubjectRef::Player(actor),
                Vec::new(),
                None,
            )],
            (),
        ));
    }

    // 18. `search <zone> [, <zone>, and/or <zone>] for [all | any number of | up to | exactly] <N|X> <flt>`
    if ts.eat_any_word(&["search", "searches"]) {
        let first_zone = parse_zone_ref(ts)?;
        let first_owner = first_zone.owner;
        let mut libs = vec![first_zone];
        loop {
            let zcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_conj = ts.eat_any_word(&["and/or", "and", "or"]);
            if (had_comma || had_conj)
                && let Ok(mut z2) = try_parse(ts, parse_zone_ref)
            {
                if z2.owner == ZoneOwner::Default
                    && matches!(
                        z2.zone,
                        ZoneKind::Graveyard | ZoneKind::Hand | ZoneKind::Library
                    )
                {
                    z2.owner = first_owner;
                }
                libs.push(z2);
                continue;
            }
            ts.restore(zcp);
            break;
        }
        if !ts.eat_word("for") {
            return Err(ts.err("expected `for` in search library"));
        }
        if let Some(res) = super::zone_ext::try_parse_search_for_ext(ts, actor, &libs) {
            return res;
        }
        let any_number = ts.eat_phrase(&["any", "number", "of"]) || ts.eat_word("all");
        let up_to = !any_number && ts.eat_phrase(&["up", "to"]);
        let exact = !any_number && !up_to && ts.eat_word("exactly");
        let count = if any_number {
            ValueExpr::Const(0)
        } else if ts.eat_phrase(&["an", "additional"]) || ts.eat_any_word(&["a", "an"]) {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let first_flt = parse_object_filter(ts)?;
        let mut flts = vec![first_flt];
        loop {
            let fcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_conj = ts.eat_any_word(&["and/or", "or", "and"]);
            if (had_comma || had_conj)
                && !ts.peek_phrase(&["put", "it"])
                && !ts.peek_phrase(&["put", "them"])
                && !ts.peek_phrase(&["put", "that"])
                && !ts.peek_phrase(&["put", "those"])
                && !ts.peek_word().is_some_and(|w| {
                    matches!(
                        w.to_ascii_lowercase().as_str(),
                        "reveal" | "shuffle" | "exile" | "cast" | "play" | "then"
                    )
                })
                && let Ok(next_flt) = try_parse(ts, parse_object_filter)
            {
                flts.push(next_flt);
                continue;
            }
            ts.restore(fcp);
            break;
        }
        let flt = if flts.len() == 1 {
            flts.remove(0)
        } else {
            ObjectFilter {
                predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(flts)],
                ..Default::default()
            }
        };
        return Ok((
            vec![counted_zone_op(
                actor,
                ZoneOpKind::SearchLibrary {
                    reveal: false,
                    shuffle: false,
                    exact,
                },
                SubjectRef::Group(Box::new(flt)),
                libs,
                None,
                Some(CountSpec {
                    count,
                    up_to: up_to || (!exact && !any_number),
                    any_number,
                }),
            )],
            (),
        ));
    }

    // 19. `reveal(s) ...`
    if ts.eat_any_word(&["reveal", "reveals"]) {
        if let Some(res) = super::zone_ext::try_parse_reveal_ext(ts, actor) {
            return res;
        }
        {
            let hcp = ts.checkpoint();
            let mut deferred = false;
            let cnt_opt = if ts.eat_phrase(&["a", "number", "of", "cards"]) {
                deferred = true;
                Some(ValueExpr::Const(1))
            } else if let Ok(v) = try_parse(ts, parse_value_expr)
                && ts.eat_any_word(&["cards", "card"])
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
                    vec![counted_zone_op(
                        actor,
                        ZoneOpKind::Reveal,
                        SubjectRef::Player(actor),
                        vec![zref],
                        None,
                        Some(exact_count(final_cnt)),
                    )],
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
            {
                let mut count_spec = None;
                let flt_opt = if opt(ts, parse_player_ref).is_some()
                    && ts.eat_any_word(&["reveal", "reveals"])
                {
                    let deferred_num = ts.eat_phrase(&["a", "number", "of"]);
                    if !deferred_num && let Some(v) = opt(ts, parse_value_expr) {
                        count_spec = Some(exact_count(v));
                    }
                    let f = try_parse(ts, parse_object_filter).ok();
                    if deferred_num
                        && ts.eat_phrase(&["equal", "to"])
                        && let Ok(v) = try_parse(ts, parse_value_expr)
                    {
                        count_spec = Some(exact_count(v));
                    }
                    f
                } else {
                    if let Some(v) = opt(ts, parse_value_expr) {
                        count_spec = Some(exact_count(v));
                    }
                    if let Ok(f) = try_parse(ts, parse_object_filter)
                        && ts.eat_any_phrase(&[&["is", "revealed"], &["are", "revealed"]])
                    {
                        let _ = ts.eat_any_phrase(&[
                            &["and", "exiles", "that", "card"],
                            &["and", "exile", "that", "card"],
                        ]);
                        Some(f)
                    } else {
                        None
                    }
                };
                if let Some(flt) = flt_opt {
                    return Ok((
                        vec![counted_zone_op(
                            actor,
                            ZoneOpKind::ExileUntilReveal {
                                filter: Box::new(flt),
                            },
                            SubjectRef::Player(actor),
                            vec![zref],
                            None,
                            count_spec,
                        )],
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
                if !ts.eat_any_word(&["cards", "card"]) {
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
                vec![counted_zone_op(
                    actor,
                    ZoneOpKind::LookTop { reveal: true },
                    SubjectRef::Player(actor),
                    vec![zref],
                    None,
                    Some(exact_count(count)),
                )],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        let extra_top = super::zone_ext::try_eat_and_top_cards(ts);
        let mut out_rev = vec![simple_zone_op(
            actor,
            ZoneOpKind::Reveal,
            subject,
            Vec::new(),
            None,
        )];
        super::zone_ext::push_extra_top_reveal(&mut out_rev, actor, extra_top);
        return Ok((out_rev, ()));
    }
    if ts.eat_any_phrase(&[&["look", "at"], &["looks", "at"]]) {
        if let Some(res) = super::zone_ext::try_parse_look_ext(ts, actor) {
            return res;
        }
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !ts.eat_any_word(&["cards", "card"]) {
                    return Err(ts.err("expected `card(s)` after `look at the top`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of <library>` after `look at the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            let _ = ts.eat_phrase(&["any", "time"]);
            return Ok((
                vec![counted_zone_op(
                    actor,
                    ZoneOpKind::LookTop { reveal: false },
                    SubjectRef::Player(actor),
                    vec![zref],
                    None,
                    Some(exact_count(count)),
                )],
                (),
            ));
        }
        let lcp = ts.checkpoint();
        if let Ok(count) = try_parse(ts, parse_value_expr)
            && ts.eat_any_word(&["cards", "card"])
            && ts.eat_phrase(&["from", "the", "top", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Ok((
                vec![counted_zone_op(
                    actor,
                    ZoneOpKind::LookTop { reveal: false },
                    SubjectRef::Player(actor),
                    vec![zref],
                    None,
                    Some(exact_count(count)),
                )],
                (),
            ));
        }
        ts.restore(lcp);
        {
            let lp_cp = ts.checkpoint();
            if ts.eat_word("and") {
                let cast_only = if ts.eat_any_word(&["cast", "casts"]) {
                    Some(true)
                } else if ts.eat_any_word(&["play", "plays"]) {
                    Some(false)
                } else {
                    None
                };
                if let Some(cast_only) = cast_only
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    let free = ts.eat_any_phrase(&[
                        &["without", "paying", "its", "mana", "cost"],
                        &["without", "paying", "their", "mana", "costs"],
                    ]);
                    let dur = parse_trailing_duration(ts).unwrap_or(Duration::ForAsLongAsExiled);
                    let mut out = vec![
                        Effect::ScopedDuration {
                            duration: dur.clone(),
                            body: vec![simple_zone_op(
                                actor,
                                ZoneOpKind::LookAt,
                                subject.clone(),
                                Vec::new(),
                                None,
                            )],
                        },
                        mutate_player(
                            actor,
                            PlayerMutation::GrantPlayPermission {
                                cast_only,
                                subject: Box::new(subject),
                                free,
                            },
                            dur.clone(),
                        ),
                    ];
                    let mcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if ts.eat_any_phrase(&[
                        &[
                            "and", "you", "may", "spend", "mana", "as", "though", "it", "were",
                            "mana", "of", "any", "color", "to", "cast",
                        ],
                        &[
                            "and", "you", "may", "spend", "mana", "as", "though", "it", "were",
                            "mana", "of", "any", "type", "to", "cast",
                        ],
                        &[
                            "and", "mana", "of", "any", "type", "can", "be", "spent", "to", "cast",
                        ],
                    ]) && ts.eat_any_phrase(&[
                        &["those", "spells"],
                        &["that", "spell"],
                        &["it"],
                        &["them"],
                    ]) {
                        out.push(player_action(
                            actor,
                            PlayerActionKind::SpendManaAsAnyType,
                            dur,
                        ));
                    } else {
                        ts.restore(mcp);
                    }
                    return Ok((out, ()));
                }
            }
            ts.restore(lp_cp);
        }
        {
            let zcp = ts.checkpoint();
            if let Ok(zref) = try_parse(ts, parse_zone_ref) {
                return Ok((
                    vec![simple_zone_op(
                        actor,
                        ZoneOpKind::LookAt,
                        SubjectRef::Player(actor),
                        vec![zref],
                        None,
                    )],
                    (),
                ));
            }
            ts.restore(zcp);
        }
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            let eff = simple_zone_op(actor, ZoneOpKind::LookAt, subject, Vec::new(), None);
            if let Some(dur) = parse_trailing_duration(ts) {
                return Ok((
                    vec![Effect::ScopedDuration {
                        duration: dur,
                        body: vec![eff],
                    }],
                    (),
                ));
            }
            return Ok((vec![eff], ()));
        }
        return Err(ts.err("expected target after `look at`"));
    }

    // 20. `separate(s) them into a face-down pile and a face-up pile`
    if ts.eat_any_word(&["separate", "separates"]) {
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
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 1,
                    },
                    subject,
                    Vec::new(),
                    None,
                )],
                (),
            ));
        }
        if ts.eat_phrase(&["into", "two", "piles"]) {
            return Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 0,
                    },
                    subject,
                    Vec::new(),
                    None,
                )],
                (),
            ));
        }
        if let Some(res) = super::zone_ext::try_parse_separate_piles_ext(ts, actor, subject) {
            return res;
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
