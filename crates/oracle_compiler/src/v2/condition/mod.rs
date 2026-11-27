//! Strictly-typed recursive [`ConditionExpr`] parser for `v2`.

use crate::ast::{
    CardTypeMask, CombatState, CompareOp, ControllerRelation, PlayerStatKind, StatKind,
};
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AggregateOp, AnaphoraRef, ConditionExpr, ConditionFlag, CounterKind, EventCheckKind,
    FilterConditionKind, FilterFlags, FilterPredicate, KeywordKind, ObjectFilter, ObjectStateKind,
    PlayerConditionKind, PlayerRef, PriorActionKind, SharedProperty, SubjectConditionKind,
    SubjectRef, TurnEventKind, ValueExpr, ZoneKind, ZoneMetricKind, ZoneOwner, ZoneRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed boolean [`ConditionExpr`].
mod extended;
mod extended_extra;
mod player_subject;

pub(super) fn eat_table<T: Copy>(ts: &mut TokenStream<'_>, table: &[(&[&str], T)]) -> Option<T> {
    table
        .iter()
        .find_map(|&(phrase, val)| ts.eat_phrase(phrase).then_some(val))
}

pub(super) fn cmp_cond(lhs: ValueExpr, op: CompareOp, rhs: ValueExpr) -> ConditionExpr {
    ConditionExpr::Compare {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    }
}

pub(super) fn not_cond(inner: ConditionExpr) -> ConditionExpr {
    ConditionExpr::Not(Box::new(inner))
}

pub(super) fn player_check(player: PlayerRef, kind: PlayerConditionKind) -> ConditionExpr {
    ConditionExpr::PlayerCheck { player, kind }
}

pub(super) fn player_stat_val(player: PlayerRef, stat: PlayerStatKind) -> ValueExpr {
    ValueExpr::PlayerStat { player, stat }
}

pub(super) fn obj_stat_val(subject: SubjectRef, stat: StatKind) -> ValueExpr {
    ValueExpr::ObjectStat {
        subject: Box::new(subject),
        stat,
    }
}

pub(super) fn prior_event(kind: PriorActionKind) -> ConditionExpr {
    ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(kind))
}

pub(super) fn to_min_u16(n: i32) -> u16 {
    u16::try_from(n.max(1)).unwrap_or(1)
}

pub(super) fn eat_stat_kind(ts: &mut TokenStream<'_>) -> Option<StatKind> {
    if ts.eat_phrase(&["mana", "value"]) {
        Some(StatKind::ManaValue)
    } else if ts.eat_word("power") {
        Some(StatKind::Power)
    } else if ts.eat_word("toughness") {
        Some(StatKind::Toughness)
    } else {
        None
    }
}

pub(super) fn eat_optional_counter_kind(ts: &mut TokenStream<'_>) -> Option<CounterKind> {
    if ts.eat_any_word(&["counter", "counters"]) {
        Some(CounterKind::Any)
    } else if let Ok(ck) = try_parse(ts, parse_counter_kind)
        && ts.eat_any_word(&["counter", "counters"])
    {
        Some(ck)
    } else {
        None
    }
}

pub(super) fn eat_was_cast_target(ts: &mut TokenStream<'_>) -> bool {
    ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"])
}

pub(super) fn eat_prefix_cmp_op(
    ts: &mut TokenStream<'_>,
    allow_eq: bool,
    allow_exactly: bool,
) -> Option<CompareOp> {
    if ts.eat_any_phrase(&[
        &["less", "than", "or", "equal", "to"],
        &["equal", "to", "or", "less", "than"],
    ]) {
        Some(CompareOp::Le)
    } else if ts.eat_any_phrase(&[
        &["greater", "than", "or", "equal", "to"],
        &["equal", "to", "or", "greater", "than"],
    ]) {
        Some(CompareOp::Ge)
    } else if ts.eat_phrase(&["less", "than"]) {
        Some(CompareOp::Lt)
    } else if ts.eat_phrase(&["greater", "than"]) {
        Some(CompareOp::Gt)
    } else if (allow_exactly && ts.eat_word("exactly"))
        || (allow_eq && ts.eat_phrase(&["equal", "to"]))
    {
        Some(CompareOp::Eq)
    } else {
        None
    }
}

pub(super) fn eat_suffix_cmp_op(
    ts: &mut TokenStream<'_>,
    allow_more: bool,
    allow_fewer: bool,
) -> CompareOp {
    if ts.eat_phrase(&["or", "less"]) || (allow_fewer && ts.eat_phrase(&["or", "fewer"])) {
        CompareOp::Le
    } else if ts.eat_phrase(&["or", "greater"]) || (allow_more && ts.eat_phrase(&["or", "more"])) {
        CompareOp::Ge
    } else {
        CompareOp::Eq
    }
}

pub fn parse_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // Compound reveal-or-control/choose additional-cost condition (`Silumgar's Scorn`, `Dragonlord's Prerogative`, `Dragon's Fire`)
    // Checked BEFORE top-level `or` splitting so `or controlled a Dragon` isn't split apart!
    // Note: `parse_object_filter`'s post-modifier loop may already consume `as you cast ~` on the second filter.
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "revealed"])
            && try_parse(ts, parse_object_filter).is_ok()
            && (ts.eat_phrase(&["or", "controlled"]) || ts.eat_phrase(&["or", "chose"]))
            && try_parse(ts, parse_object_filter).is_ok()
        {
            let _ = ts.eat_phrase(&["as", "you", "cast"])
                && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]));
            return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
        }
        ts.restore(dcp);
    }
    // Compound graveyard/library entry-or-cast condition
    {
        let ecp = ts.checkpoint();
        if (ts.eat_word("it")
            || ts.eat_tilde()
            || ts.eat_phrase(&["that", "creature"])
            || ts.eat_phrase(&["that", "permanent"])
            || ts.eat_phrase(&["that", "card"]))
            && ts.eat_phrase(&["entered", "from"])
            && let Ok(z1) = try_parse(ts, parse_zone_ref)
            && (ts.eat_phrase(&["or", "you", "cast", "it", "from"])
                || ts.eat_phrase(&["or", "was", "cast", "from"]))
            && let Ok(z2) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ConditionExpr::Or(
                Box::new(ConditionExpr::InZone {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    zone: z1,
                }),
                Box::new(ConditionExpr::CastFromZone {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    zone: z2,
                }),
            ));
        }
        ts.restore(ecp);
    }
    let mut cond = parse_condition_atom(ts)?;
    loop {
        let cp = ts.checkpoint();
        let _ = ts.eat_comma();
        if ts.eat_word("and") {
            let _ = ts.eat_word("if");
            if let Ok(rhs) = try_parse(ts, parse_condition_atom) {
                cond = ConditionExpr::And(Box::new(cond), Box::new(rhs));
                continue;
            }
            if let Some(rhs) = extended_extra::try_parse_elided_and_rhs(ts) {
                cond = ConditionExpr::And(Box::new(cond), Box::new(rhs));
                continue;
            }
            ts.restore(cp);
            break;
        }
        ts.restore(cp);
        if ts.eat_word("or") {
            if let Ok(rhs) = try_parse(ts, parse_condition_atom) {
                cond = ConditionExpr::Or(Box::new(cond), Box::new(rhs));
                continue;
            }
            if ts.eat_word("if")
                && let Ok(rhs) = try_parse(ts, parse_condition_atom)
            {
                cond = ConditionExpr::Or(Box::new(cond), Box::new(rhs));
                continue;
            }
            // Elided `you` on second disjunct (`if you control a Squirrel or returned a Squirrel card to your hand this way`)
            {
                let rcp = ts.checkpoint();
                if ts.eat_word("returned")
                    && let Ok(flt) = try_parse(ts, parse_object_filter)
                {
                    let _ = crate::v2::parser::opt(ts, crate::v2::filter::parse_zone_dest);
                    if ts.eat_phrase(&["this", "way"]) {
                        cond = ConditionExpr::Or(
                            Box::new(cond),
                            Box::new(ConditionExpr::PriorActionMatched {
                                action: PriorActionKind::ReturnedThisWay,
                                filter: Box::new(flt),
                            }),
                        );
                        continue;
                    }
                }
                ts.restore(rcp);
            }
            ts.restore(cp);
            break;
        }
        break;
    }
    Ok(cond)
}

fn parse_condition_atom(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // Turn / cast / kicker checks
    const TURN_FLAGS: &[(&[&str], ConditionFlag, bool)] = &[
        (
            &[
                "it's", "the", "first", "combat", "phase", "of", "the", "turn",
            ],
            ConditionFlag::IsFirstCombatPhase,
            false,
        ),
        (
            &[
                "it's", "the", "first", "combat", "phase", "of", "your", "turn",
            ],
            ConditionFlag::IsFirstCombatPhase,
            false,
        ),
        (&["it's", "your", "turn"], ConditionFlag::IsYourTurn, false),
        (&["it's", "night"], ConditionFlag::IsNight, false),
        (&["it's", "day"], ConditionFlag::IsDay, false),
        (
            &["it's", "not", "your", "turn"],
            ConditionFlag::IsYourTurn,
            true,
        ),
        (
            &["it's", "not", "their", "turn"],
            ConditionFlag::IsYourTurn,
            true,
        ),
    ];
    for &(phrase, flag, neg) in TURN_FLAGS {
        if ts.eat_phrase(phrase) {
            let base = ConditionExpr::Flag(flag);
            return Ok(if neg { not_cond(base) } else { base });
        }
    }
    // `it had [a/an/one or more] [<kind>] counter(s) on it`
    {
        let hcp = ts.checkpoint();
        if (ts.eat_word("it") || ts.eat_tilde())
            && ts.eat_word("had")
            && ts.eat_any_phrase(&[&["one", "or", "more"], &["a"], &["an"]])
            && let Some(kind) = eat_optional_counter_kind(ts)
            && ts.eat_phrase(&["on", "it"])
        {
            return Ok(cmp_cond(
                ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    kind,
                },
                CompareOp::Ge,
                ValueExpr::Const(1),
            ));
        }
        ts.restore(hcp);
    }
    // `this ability has been activated <N> or more times this turn`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["this", "ability", "has", "been", "activated"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "times", "this", "turn"])
        {
            return Ok(ConditionExpr::EventCheck(
                EventCheckKind::ResolutionCountThisTurn(u8::try_from(n).unwrap_or(1)),
            ));
        }
        ts.restore(acp);
    }
    // `~ / it didn't attack [or come under your control] this turn`
    {
        let mcp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it")) && ts.eat_phrase(&["didn't", "attack"]) {
            let _ = ts.eat_phrase(&["or", "come", "under", "your", "control"]);
            if ts.eat_phrase(&["this", "turn"]) {
                return Ok(not_cond(ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::SelfObject),
                    state: ObjectStateKind::AttackedOrBlockedThisTurn,
                }));
            }
        }
        ts.restore(mcp);
    }
    // `a/an <flt> is on the battlefield`
    {
        let bf_cp = ts.checkpoint();
        if ts.eat_any_word(&["a", "an"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["is", "on", "the", "battlefield"])
        {
            return Ok(ConditionExpr::Controls {
                player: PlayerRef::EachPlayer,
                filter: Box::new(flt),
                min_count: 1,
            });
        }
        ts.restore(bf_cp);
    }
    // `~ isn't on the battlefield`
    {
        let bcp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it"))
            && ts.eat_phrase(&["isn't", "on", "the", "battlefield"])
        {
            return Ok(ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::SelfObject),
                filter: Box::new(ObjectFilter {
                    flags: FilterFlags::NOT_ON_BATTLEFIELD,
                    ..Default::default()
                }),
            });
        }
        ts.restore(bcp);
    }
    // `defending player is poisoned / the monarch`
    if ts.eat_phrase(&["defending", "player", "is", "poisoned"]) {
        return Ok(player_check(
            PlayerRef::DefendingPlayer,
            PlayerConditionKind::Poisoned,
        ));
    }
    if ts.eat_phrase(&["defending", "player", "is", "the", "monarch"]) {
        return Ok(player_check(
            PlayerRef::DefendingPlayer,
            PlayerConditionKind::IsMonarch,
        ));
    }
    if ts.eat_phrase(&["excess", "damage", "was", "dealt"]) {
        if ts.eat_word("to") {
            let _ = try_parse(ts, parse_subject_ref);
        }
        if ts.eat_phrase(&["this", "way"]) {
            return Ok(prior_event(PriorActionKind::ExcessDamageThisWay));
        }
    }
    {
        let fcp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it")) && ts.eat_word("was") {
            if ts.eat_word("foretold") {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                    KeywordKind::Foretell,
                )));
            }
            if ts.eat_any_phrase(&[
                &["cast", "from", "anywhere", "other", "than", "your", "hand"],
                &[
                    "cast", "from", "anywhere", "other", "than", "its", "owner's", "hand",
                ],
            ]) {
                return Ok(not_cond(ConditionExpr::Flag(
                    ConditionFlag::WasCastFromHand,
                )));
            }
        }
        ts.restore(fcp);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["your", "life", "total", "is"]) {
            if ts.eat_phrase(&["greater", "than", "your", "starting", "life", "total"]) {
                return Ok(player_check(
                    PlayerRef::You,
                    PlayerConditionKind::LifeAboveStarting { margin: 1 },
                ));
            }
            if ts.eat_phrase(&[
                "less", "than", "or", "equal", "to", "half", "your", "starting", "life", "total",
            ]) {
                return Ok(not_cond(player_check(
                    PlayerRef::You,
                    PlayerConditionKind::LifeAboveStarting { margin: 0 },
                )));
            }
        }
        ts.restore(lcp);
    }
    {
        let hcp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_phrase(&["has", "more", "cards", "in", "hand", "than", "you"])
        {
            return Ok(cmp_cond(
                player_stat_val(player, PlayerStatKind::CardsInHand),
                CompareOp::Gt,
                player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
            ));
        }
        ts.restore(hcp);
    }
    {
        let icp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["it", "isn't", "attacking", "or", "blocking"],
            &["it's", "not", "attacking", "or", "blocking"],
        ]) {
            return Ok(not_cond(ConditionExpr::Or(
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    state: ObjectStateKind::Attacking,
                }),
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    state: ObjectStateKind::Blocking,
                }),
            )));
        }
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "creatures", "are", "blocking"])
            && try_parse(ts, parse_subject_ref).is_ok()
        {
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    combat: Some(CombatState::Blocking),
                    ..Default::default()
                })),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(icp);
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("is")
                && let Some((p, t)) = ts.next_base_pt()
            {
                return Ok(ConditionExpr::And(
                    Box::new(cmp_cond(
                        obj_stat_val(subj.clone(), StatKind::Power),
                        CompareOp::Eq,
                        ValueExpr::Const(p),
                    )),
                    Box::new(cmp_cond(
                        obj_stat_val(subj, StatKind::Toughness),
                        CompareOp::Eq,
                        ValueExpr::Const(t),
                    )),
                ));
            }
            if ts.eat_phrase(&["has", "greater", "power", "than"])
                && let Ok(other) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ConditionExpr::HasGreaterStatThan {
                    subject: Box::new(subj),
                    other: Box::new(other),
                    stat: Some(StatKind::Power),
                });
            }
        }
        ts.restore(icp);
    }
    const YOU_HAVE_FLAGS: &[(&[&str], ConditionFlag)] = &[
        (
            &["you", "have", "the", "city's", "blessing"],
            ConditionFlag::HasCitysBlessing,
        ),
        (
            &["you", "have", "the", "initiative"],
            ConditionFlag::HasInitiative,
        ),
        (
            &["you", "have", "an", "enduring", "story"],
            ConditionFlag::HasEnduringStory,
        ),
    ];
    if let Some(flag) = eat_table(ts, YOU_HAVE_FLAGS) {
        return Ok(ConditionExpr::Flag(flag));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &[
                "you", "have", "more", "cards", "in", "hand", "than", "each", "opponent",
            ],
            &[
                "you", "have", "more", "cards", "in", "hand", "than", "an", "opponent",
            ],
        ]) {
            return Ok(cmp_cond(
                player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
                CompareOp::Gt,
                player_stat_val(PlayerRef::EachOpponent, PlayerStatKind::CardsInHand),
            ));
        }
        if ts.eat_phrase(&["you", "have", "fewer", "than"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["cards", "in", "hand"])
        {
            return Ok(cmp_cond(
                player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
                CompareOp::Lt,
                ValueExpr::Const(n),
            ));
        }
        if ts.eat_phrase(&["you", "have", "exactly"])
            && let Some(n1) = ts.next_number(false)
        {
            let n2 = if ts.eat_word("or") {
                ts.next_number(false)
            } else {
                None
            };
            if ts.eat_any_phrase(&[&["cards", "in", "hand"], &["card", "in", "hand"]]) {
                let c1 = cmp_cond(
                    player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
                    CompareOp::Eq,
                    ValueExpr::Const(n1),
                );
                if let Some(m) = n2 {
                    let c2 = cmp_cond(
                        player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
                        CompareOp::Eq,
                        ValueExpr::Const(m),
                    );
                    return Ok(ConditionExpr::Or(Box::new(c1), Box::new(c2)));
                }
                return Ok(c1);
            }
        }
        ts.restore(hcp);
    }
    {
        let pcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "put"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_phrase(&[
                &["onto", "the", "battlefield", "this", "way"],
                &["into", "your", "hand", "this", "way"],
                &["into", "your", "graveyard", "this", "way"],
            ])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::ReturnedThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(pcp);
        if ts.eat_any_phrase(&[
            &["you", "haven't", "cast", "it"],
            &["you", "haven't", "cast", "the", "card"],
            &["you", "haven't", "played", "it"],
            &["you", "haven't", "played", "the", "card"],
        ]) {
            return Ok(not_cond(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                kind: SubjectConditionKind::WasCast,
            }));
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["any", "of", "that", "mana", "is", "spent", "on"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::ManaSpentFrom,
                value: 1,
            });
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["that", "creature's", "power", "is"]) {
            if ts.eat_phrase(&["greater", "than"])
                && let Ok(other) = try_parse(ts, parse_subject_ref)
            {
                let _ = ts.eat_word("'s");
                let _ = ts.eat_any_word(&["power", "toughness"]);
                return Ok(ConditionExpr::HasGreaterStatThan {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                    other: Box::new(other),
                    stat: Some(StatKind::Power),
                });
            }
            if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                let op = eat_suffix_cmp_op(ts, false, false);
                return Ok(cmp_cond(
                    obj_stat_val(
                        SubjectRef::Anaphora(AnaphoraRef::ThatCreature),
                        StatKind::Power,
                    ),
                    op,
                    rhs,
                ));
            }
        }
        ts.restore(pcp);
        if ts.eat_phrase(&[
            "that", "creature", "was", "dealt", "excess", "damage", "this", "turn",
        ]) {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                kind: SubjectConditionKind::DealtDamageThisTurn { combat_only: false },
            });
        }
        ts.restore(pcp);
        if ts.eat_phrase(&[
            "that", "creature", "wasn't", "dealt", "damage", "this", "turn",
        ]) {
            return Ok(not_cond(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                kind: SubjectConditionKind::DealtDamageThisTurn { combat_only: false },
            }));
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["that", "creature", "entered", "from"])
            && let Ok(z) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ConditionExpr::CastFromZone {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                zone: z,
            });
        }
        ts.restore(pcp);
        if let Ok(flt) = try_parse(ts, parse_object_filter) {
            const FLT_THIS_WAY: &[(&[&str], PriorActionKind)] = &[
                (
                    &["is", "dealt", "damage", "this", "way"],
                    PriorActionKind::DealtDamageThisWay,
                ),
                (
                    &["was", "dealt", "damage", "this", "way"],
                    PriorActionKind::DealtDamageThisWay,
                ),
                (
                    &["is", "put", "into", "exile", "this", "way"],
                    PriorActionKind::ExiledThisWay,
                ),
                (&["enters", "this", "way"], PriorActionKind::ReturnedThisWay),
                (
                    &["entered", "this", "way"],
                    PriorActionKind::ReturnedThisWay,
                ),
            ];
            if let Some(action) = eat_table(ts, FLT_THIS_WAY) {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(pcp);
        if ts.eat_any_phrase(&[
            &[
                "you", "cast", "it", "from", "anywhere", "other", "than", "your", "hand",
            ],
            &[
                "you", "cast", "it", "from", "anywhere", "other", "than", "exile",
            ],
        ]) {
            return Ok(not_cond(ConditionExpr::CastFromZone {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                zone: ZoneRef {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::You,
                },
            }));
        }
        ts.restore(pcp);
        if ts.eat_any_phrase(&[&["it's", "attacking"], &["it", "is", "attacking"]])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            return Ok(ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                filter: Box::new(ObjectFilter {
                    combat: Some(CombatState::Attacking),
                    predicates: vec![FilterPredicate::AttackedPlayerThisTurn(p)],
                    ..Default::default()
                }),
            });
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["you", "cast"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "way"])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::CastThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(pcp);
        if ts.eat_any_phrase(&[&["you", "discard"], &["you", "discarded"]])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "way"])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::DiscardedThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(pcp);
        if ts.eat_word("damage") {
            let src_flt = if ts.eat_word("from") {
                try_parse(ts, parse_object_filter).ok()
            } else {
                None
            };
            if ts.eat_phrase(&["is", "prevented", "this", "way"]) {
                return Ok(ConditionExpr::PriorActionMatched {
                    action: PriorActionKind::DamagePreventedThisWay,
                    filter: Box::new(src_flt.unwrap_or_default()),
                });
            }
        }
        ts.restore(pcp);
        if ts.eat_phrase(&[
            "a",
            "permanent's",
            "ability",
            "is",
            "countered",
            "this",
            "way",
        ]) {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::ThatMany,
                filter: Box::new(ObjectFilter {
                    is_ability: true,
                    ..Default::default()
                }),
            });
        }
        ts.restore(pcp);
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_any_phrase(&[
                &["didn't", "cast"],
                &["haven't", "cast"],
                &["hasn't", "cast"],
            ])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(not_cond(ConditionExpr::PlayerTurnEvent {
                player,
                event: TurnEventKind::SpellCast,
                filter: Some(Box::new(flt)),
                min_count: 1,
            }));
        }
        ts.restore(pcp);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "have", "at", "least"])
            && let Some(margin) = ts.next_number(false)
            && ts.eat_phrase(&["life", "more", "than", "your", "starting", "life", "total"])
        {
            return Ok(player_check(
                PlayerRef::You,
                PlayerConditionKind::LifeAboveStarting { margin },
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "an", "opponent", "has", "more", "cards", "in", "hand", "than", "you",
        ]) {
            return Ok(cmp_cond(
                player_stat_val(PlayerRef::AnyOpponent, PlayerStatKind::CardsInHand),
                CompareOp::Gt,
                player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["an", "opponent", "controls", "at", "least"])
            && let Some(margin) = ts.next_number(false)
            && ts.eat_word("more")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["than", "you"])
        {
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::OpponentControlsMoreBy,
                value: to_min_u16(margin),
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["mana", "from"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && eat_was_cast_target(ts)
        {
            let _ = ts.eat_phrase(&["or", "activate", "it"]);
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::ManaSpentFrom,
                value: 1,
            });
        }
        ts.restore(lcp);
        // `its / that spell's / that creature's / that card's <stat> is/was <val> [or less|or greater]`
        {
            const POSS_SUBJS: &[(&[&str], AnaphoraRef)] = &[
                (&["its"], AnaphoraRef::It),
                (&["that", "spell's"], AnaphoraRef::ThatSpell),
                (&["that", "creature's"], AnaphoraRef::ThatCreature),
                (&["that", "card's"], AnaphoraRef::ThatCard),
                (&["that", "permanent's"], AnaphoraRef::ThatPermanent),
            ];
            if let Some(anaph) = eat_table(ts, POSS_SUBJS)
                && let Some(stat) = eat_stat_kind(ts)
                && ts.eat_any_word(&["was", "is"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
            {
                let op = eat_suffix_cmp_op(ts, true, false);
                return Ok(cmp_cond(
                    obj_stat_val(SubjectRef::Anaphora(anaph), stat),
                    op,
                    val,
                ));
            }
        }
        ts.restore(lcp);
        // `you don't/didn't cast/play that card this way`
        if ts.eat_any_phrase(&[
            &["you", "don't", "cast"],
            &["you", "didn't", "cast"],
            &["you", "don't", "play"],
            &["you", "didn't", "play"],
        ]) && ts.eat_any_phrase(&[
            &["that", "card", "this", "way"],
            &["it", "this", "way"],
            &["a", "card", "this", "way"],
        ]) {
            return Ok(not_cond(prior_event(PriorActionKind::CastThisWay)));
        }
        ts.restore(lcp);
        // `you sacrificed/exiled/discarded/returned <N or more | a> <flt> this way`
        const YOU_PRIOR_ACTS: &[(&[&str], PriorActionKind)] = &[
            (&["sacrificed"], PriorActionKind::SacrificedThisWay),
            (&["exiled"], PriorActionKind::ExiledThisWay),
            (&["discarded"], PriorActionKind::DiscardedThisWay),
            (&["returned"], PriorActionKind::ReturnedThisWay),
            (&["return"], PriorActionKind::ReturnedThisWay),
        ];
        if ts.eat_word("you")
            && let Some(action) = eat_table(ts, YOU_PRIOR_ACTS)
        {
            if ts.next_number(false).is_some() {
                let _ = ts.eat_any_phrase(&[&["or", "more"], &["or", "fewer"]]);
            }
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["this", "way"])
            {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["there", "are", "cards", "exiled", "with"])
            && (ts.eat_word("it") || ts.eat_tilde())
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::CardsExiledWithSelf));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["there", "are"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "counters", "among"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::CountersAmong,
                value: to_min_u16(n),
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "cards", "are", "in"])
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ConditionExpr::ZoneMetric {
                zone,
                kind: ZoneMetricKind::IsEmpty,
                min_count: 0,
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "creatures", "attacked", "this", "turn"]) {
            return Ok(ConditionExpr::Flag(
                ConditionFlag::NoCreaturesAttackedThisTurn,
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "no",
            "permanents",
            "left",
            "the",
            "battlefield",
            "this",
            "turn",
        ]) {
            return Ok(not_cond(ConditionExpr::Flag(
                ConditionFlag::PermanentLeftThisTurn,
            )));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "opponent", "has", "more", "life", "than"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            return Ok(player_check(
                p,
                PlayerConditionKind::NoOpponentHasMoreLifeThan,
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "creatures", "are", "on", "the", "battlefield"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::NoCreaturesOnBattlefield));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "other", "creatures", "are", "attacking"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            return Ok(player_check(
                p,
                PlayerConditionKind::NoOtherCreaturesAttacking,
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "other", "creature", "has", "greater", "power"]) {
            return Ok(ConditionExpr::Flag(
                ConditionFlag::NoOtherCreatureHasGreaterPower,
            ));
        }
        ts.restore(lcp);
        const YOU_DIDNT_CHECKS: &[(&[&str], PlayerConditionKind)] = &[
            (
                &[
                    "you", "didn't", "attack", "with", "a", "creature", "this", "turn",
                ],
                PlayerConditionKind::AttackedThisTurn,
            ),
            (
                &["you", "didn't", "play", "a", "land", "this", "turn"],
                PlayerConditionKind::PlayedLandThisTurn,
            ),
            (
                &[
                    "you", "didn't", "play", "a", "card", "from", "exile", "this", "turn",
                ],
                PlayerConditionKind::PlayedFromExileThisTurn,
            ),
            (
                &["you", "didn't", "lose", "life", "this", "turn"],
                PlayerConditionKind::LostLifeThisTurn,
            ),
        ];
        if let Some(kind) = eat_table(ts, YOU_DIDNT_CHECKS) {
            return Ok(not_cond(player_check(PlayerRef::You, kind)));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["it's", "a", "spell", "with", "lesser", "mana", "value"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::LesserManaValueSpell));
        }
        ts.restore(lcp);
        // `~'s / its <keyword> cost was paid` / `the {M} cost was paid` / `<subj> was cast for its <keyword> cost`
        if ((ts.eat_tilde() && ts.eat_word("'s")) || ts.eat_word("its"))
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
            && ts.eat_phrase(&["cost", "was", "paid"])
        {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(lcp);
        if ts.eat_word("the")
            && ts.eat_mana_cost().is_some()
            && ts.eat_phrase(&["cost", "was", "paid"])
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
        }
        ts.restore(lcp);
        if (ts.eat_tilde()
            || ts.eat_any_phrase(&[&["that", "creature"], &["this", "spell"], &["it"]]))
            && ts.eat_any_phrase(&[&["was", "cast", "for", "its"], &["was", "cast", "using"]])
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
        {
            let _ = ts.eat_word("cost");
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(lcp);
        if ts.eat_any_phrase(&[&["there's", "a"], &["there's", "an"]])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && let Some(zone) = flt.zones.pop()
        {
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone,
                min_count: 1,
            });
        }
        ts.restore(lcp);
        if ts.eat_any_phrase(&[&["you", "sacrificed"], &["you've", "sacrificed"]]) {
            let _ = ts.eat_any_phrase(&[&["one", "or", "more"], &["a"], &["an"]])
                || (ts.next_number(false).is_some() && {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    true
                });
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ConditionExpr::SacrificedPermanentThisTurn {
                    player: PlayerRef::You,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(lcp);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["are", "attached", "to"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::AttachedCount {
                subject: Box::new(subj),
                filter: Box::new(flt),
                min_count: to_min_u16(n),
            });
        }
        ts.restore(lcp);
        if let Some(w) = ts.peek_word()
            && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&[
                "is",
                "the",
                "most",
                "common",
                "color",
                "among",
                "all",
                "permanents",
            ]) {
                let _ = ts.eat_phrase(&["or", "is", "tied", "for", "most", "common"]);
                return Ok(ConditionExpr::EventCheck(EventCheckKind::MostCommonColor(
                    c,
                )));
            }
        }
        ts.restore(lcp);
        if let Ok(p) = try_parse(ts, parse_player_ref)
            && ts.eat_phrase(&["controls", "the", "most", "creatures"])
        {
            let _ = ts.eat_phrase(&["or", "is", "tied", "for", "the", "most"]);
            return Ok(player_check(p, PlayerConditionKind::ControlsMostCreatures));
        }
        ts.restore(lcp);
        if (ts.eat_word("it") || ts.eat_tilde())
            && ts.eat_phrase(&["attacked", "a", "battle", "this", "turn"])
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::SelfObject),
                kind: SubjectConditionKind::AttackedBattleThisTurn,
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "an", "instant", "card", "and", "a", "sorcery", "card", "are", "in",
        ]) && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ConditionExpr::And(
                Box::new(ConditionExpr::ZoneHasCount {
                    filter: Box::new(ObjectFilter {
                        any_types: CardTypeMask::INSTANT,
                        any_card: true,
                        ..Default::default()
                    }),
                    zone,
                    min_count: 1,
                }),
                Box::new(ConditionExpr::ZoneHasCount {
                    filter: Box::new(ObjectFilter {
                        any_types: CardTypeMask::SORCERY,
                        any_card: true,
                        ..Default::default()
                    }),
                    zone,
                    min_count: 1,
                }),
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "a",
            "permanent",
            "left",
            "the",
            "battlefield",
            "under",
            "your",
            "control",
            "this",
            "turn",
        ]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentLeftThisTurn));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["a", "creature"])
            && {
                let _ = ts.eat_phrase(&["not", "named"]) && ts.eat_tilde();
                true
            }
            && ts.eat_phrase(&["died", "this", "turn"])
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentDiedThisTurn));
        }
        ts.restore(lcp);
    }
    const YOU_PLAYER_CHECKS: &[(&[&str], PlayerConditionKind, bool)] = &[
        (
            &["you've", "been", "attacked", "this", "step"],
            PlayerConditionKind::AttackedThisStep,
            false,
        ),
        (
            &["you", "were", "attacked", "this", "step"],
            PlayerConditionKind::AttackedThisStep,
            false,
        ),
        (
            &["you", "attacked", "this", "turn"],
            PlayerConditionKind::AttackedThisTurn,
            false,
        ),
        (
            &["you're", "the", "monarch"],
            PlayerConditionKind::IsMonarch,
            false,
        ),
        (
            &["you", "are", "the", "monarch"],
            PlayerConditionKind::IsMonarch,
            false,
        ),
        (
            &["you", "have", "a", "full", "party"],
            PlayerConditionKind::HasFullParty,
            false,
        ),
        (
            &["you've", "completed", "a", "dungeon"],
            PlayerConditionKind::CompletedDungeon,
            false,
        ),
        (
            &["you", "have", "completed", "a", "dungeon"],
            PlayerConditionKind::CompletedDungeon,
            false,
        ),
        (
            &["you", "descended", "this", "turn"],
            PlayerConditionKind::DescendedThisTurn,
            false,
        ),
        (
            &[
                "you", "haven't", "cast", "a", "spell", "from", "your", "hand", "this", "turn",
            ],
            PlayerConditionKind::CastSpellFromHandThisTurn,
            true,
        ),
    ];
    for &(phrase, kind, neg) in YOU_PLAYER_CHECKS {
        if ts.eat_phrase(phrase) {
            let base = player_check(PlayerRef::You, kind);
            return Ok(if neg { not_cond(base) } else { base });
        }
    }
    if ts.eat_any_phrase(&[
        &["you", "control", "your", "commander"],
        &["you", "control", "a", "commander"],
    ]) {
        let _ = ts.eat_phrase(&["that's", "a", "creature", "or", "planeswalker"]);
        return Ok(player_check(
            PlayerRef::You,
            PlayerConditionKind::ControlsCommander,
        ));
    }
    if ts.eat_phrase(&["tribute", "wasn't", "paid"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::TributeNotPaid));
    }
    if ts.eat_phrase(&[
        "two", "or", "more", "players", "have", "lost", "the", "game",
    ]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PlayersLostGame(
            2,
        )));
    }
    if ts.eat_any_phrase(&[
        &[
            "this", "is", "the", "first", "time", "this", "ability", "has", "resolved", "this",
            "turn",
        ],
        &["it's", "the", "first", "time"],
    ]) {
        // Optional trailing event description
        {
            let fcp = ts.checkpoint();
            let _ = opt(ts, parse_counter_kind);
            if ts.eat_any_word(&["counters", "counter"])
                && ts.eat_phrase(&["have", "been", "put", "on"])
                && try_parse(ts, parse_subject_ref).is_ok()
                && ts.eat_phrase(&["this", "turn"])
            {
                // Consumed trailing counter placement detail.
            } else {
                ts.restore(fcp);
                if try_parse(ts, parse_subject_ref).is_ok()
                    && ts.eat_phrase(&["has", "become", "tapped", "this", "turn"])
                {
                    // Consumed trailing tap detail.
                } else {
                    ts.restore(fcp);
                }
            }
        }
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::ResolutionCountThisTurn(1),
        ));
    }
    if ts.eat_any_phrase(&[
        &[
            "this", "is", "the", "second", "time", "this", "ability", "has", "resolved", "this",
            "turn",
        ],
        &["it's", "the", "second", "time"],
    ]) {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::ResolutionCountThisTurn(2),
        ));
    }
    if ts.eat_any_phrase(&[
        &[
            "this", "is", "the", "third", "time", "this", "ability", "has", "resolved", "this",
            "turn",
        ],
        &["it's", "the", "third", "time"],
    ]) {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::ResolutionCountThisTurn(3),
        ));
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["'s", "additional", "cost", "was", "paid"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
        }
        ts.restore(acp);
        if ts.eat_tilde() && ts.eat_phrase(&["was", "cast", "using", "teamwork"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::CastUsingTeamwork));
        }
        ts.restore(acp);
    }
    {
        let gcp = ts.checkpoint();
        if ts.eat_phrase(&["it", "shares", "a", "creature", "type", "with"])
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::SharesTypeWith {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                other: Box::new(other),
                kind: SharedProperty::CreatureType,
            });
        }
        ts.restore(gcp);
        if ts.eat_phrase(&["it", "has", "greater", "power", "or", "toughness", "than"])
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::HasGreaterStatThan {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                other: Box::new(other),
                stat: None,
            });
        }
        ts.restore(gcp);
    }
    // `there are no [more] <kind> counters on <subj>`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"]) {
            let _ = ts.eat_word("more");
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counters", "counter"])
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(cmp_cond(
                    ValueExpr::CounterCount {
                        subject: Box::new(subject),
                        kind,
                    },
                    CompareOp::Eq,
                    ValueExpr::Const(0),
                ));
            }
        }
        ts.restore(ncp);
    }
    {
        let wcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "win", "the", "flip"])
            || (ts.eat_phrase(&["you", "win"]) && at_cond_boundary(ts))
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::WonCoinFlip));
        }
        ts.restore(wcp);
        if ts.eat_phrase(&["you", "lose", "the", "flip"])
            || (ts.eat_phrase(&["you", "lose"]) && at_cond_boundary(ts))
        {
            return Ok(not_cond(ConditionExpr::Flag(ConditionFlag::WonCoinFlip)));
        }
        ts.restore(wcp);
    }
    if ts.eat_any_phrase(&[
        &["you", "search", "your", "library", "this", "way"],
        &[
            "that", "player", "searches", "their", "library", "this", "way",
        ],
        &["they", "search", "their", "library", "this", "way"],
    ]) {
        return Ok(prior_event(PriorActionKind::SearchedThisWay));
    }
    if ts.eat_phrase(&["no", "spells", "were", "cast", "last", "turn"]) {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::SpellsCastLastTurn {
                op: CompareOp::Eq,
                count: 0,
            },
        ));
    }
    if ts.eat_any_phrase(&[
        &[
            "two", "or", "more", "spells", "were", "cast", "last", "turn",
        ],
        &[
            "a", "player", "cast", "two", "or", "more", "spells", "last", "turn",
        ],
    ]) {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::SpellsCastLastTurn {
                op: CompareOp::Ge,
                count: 2,
            },
        ));
    }
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["at", "least"])
            && let Some(n) = ts.next_number(false)
        {
            if let Some(w) = ts.peek_word()
                && let Some(color) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                if ts.eat_phrase(&["mana", "was", "spent", "to", "cast"]) && eat_was_cast_target(ts)
                {
                    return Ok(ConditionExpr::ColoredManaSpent {
                        color,
                        min_count: u8::try_from(n.max(1)).unwrap_or(1),
                    });
                }
            }
            ts.restore(mcp);
            if ts.eat_phrase(&["at", "least"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
                && ts.eat_phrase(&["mana", "was", "spent", "to", "cast"])
                && eat_was_cast_target(ts)
            {
                return Ok(cmp_cond(ValueExpr::ManaPaidThisWay, CompareOp::Ge, val));
            }
        }
        ts.restore(mcp);
    }
    if ts.eat_any_phrase(&[
        &[
            "it", "wasn't", "cast", "or", "no", "mana", "was", "spent", "to", "cast", "it",
        ],
        &["no", "mana", "was", "spent", "to", "cast", "it"],
    ]) {
        return Ok(ConditionExpr::SubjectCheck {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            kind: SubjectConditionKind::NotCastOrNoManaSpent,
        });
    }
    const POISONED_PLAYERS: &[(&[&str], PlayerRef)] = &[
        (
            &["an", "opponent", "is", "poisoned"],
            PlayerRef::AnyOpponent,
        ),
        (&["a", "player", "is", "poisoned"], PlayerRef::AnyPlayer),
        (&["you", "are", "poisoned"], PlayerRef::You),
        (&["you're", "poisoned"], PlayerRef::You),
    ];
    if let Some(player) = eat_table(ts, POISONED_PLAYERS) {
        return Ok(player_check(player, PlayerConditionKind::Poisoned));
    }
    // `<N> or more <flt> entered [the battlefield] [under your control] this turn`
    {
        let ecp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_phrase(&[&["entered", "the", "battlefield"], &["entered"]])
        {
            let controller = if ts.eat_phrase(&["under", "your", "control"]) {
                PlayerRef::You
            } else {
                PlayerRef::EachPlayer
            };
            if ts.eat_phrase(&["this", "turn"]) {
                return Ok(ConditionExpr::PermanentEnteredThisTurn {
                    filter: Box::new(flt),
                    controller,
                    min_count: to_min_u16(n),
                });
            }
        }
        ts.restore(ecp);
    }
    // `you don't/didn't put <subj> onto the battlefield / into your hand [this way]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["you", "don't", "put"], &["you", "didn't", "put"]])
            && try_parse(ts, parse_subject_ref).is_ok()
            && let Ok((dest, _)) = try_parse(ts, crate::v2::filter::parse_zone_dest)
        {
            let _ = ts.eat_phrase(&["this", "way"]);
            return Ok(ConditionExpr::EventCheck(EventCheckKind::DidntPutInZone(
                dest.zone,
            )));
        }
        ts.restore(cp);
    }
    // `<zone> has <N> or more cards in it`
    {
        let cp = ts.checkpoint();
        if let Ok(zone) = try_parse(ts, parse_zone_ref)
            && ts.eat_word("has")
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "in", "it"])
        {
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(ObjectFilter {
                    any_card: true,
                    ..Default::default()
                }),
                zone,
                min_count: to_min_u16(n),
            });
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["the", "gift", "was", "promised"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::GiftWasPromised));
    }
    if ts.eat_phrase(&["the", "gift", "wasn't", "promised"]) {
        return Ok(not_cond(ConditionExpr::Flag(
            ConditionFlag::GiftWasPromised,
        )));
    }
    if ts.eat_any_phrase(&[
        &["a", "player", "is", "dealt", "damage", "this", "way"],
        &["an", "opponent", "is", "dealt", "damage", "this", "way"],
    ]) {
        return Ok(prior_event(PriorActionKind::DealtDamageThisWay));
    }
    // `it's on the battlefield` / `~ is on the battlefield`
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["it's", "on", "the", "battlefield"])
            || (ts.eat_tilde() && ts.eat_phrase(&["is", "on", "the", "battlefield"]))
        {
            return Ok(ConditionExpr::InZone {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                zone: ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                },
            });
        }
        ts.restore(bcp);
    }
    // `it escaped` / `~ escaped`
    {
        let ecp = ts.checkpoint();
        if (ts.eat_word("it") || ts.eat_tilde()) && ts.eat_word("escaped") {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                KeywordKind::Escape,
            )));
        }
        ts.restore(ecp);
    }
    // `<zone> has no cards in it`
    {
        let zcp = ts.checkpoint();
        if let Ok(zone) = try_parse(ts, parse_zone_ref)
            && ts.eat_phrase(&["has", "no", "cards", "in", "it"])
        {
            return Ok(ConditionExpr::ZoneMetric {
                zone,
                kind: ZoneMetricKind::IsEmpty,
                min_count: 0,
            });
        }
        ts.restore(zcp);
    }
    // `you / you've put [a/an/one or more] [<kind>] counter(s) on <subj> this turn`
    {
        let pcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["you", "put"], &["you've", "put"]])
            && ts.eat_any_phrase(&[&["one", "or", "more"], &["a"], &["an"]])
        {
            let kind_opt = if ts.eat_any_word(&["counter", "counters"]) {
                Some(None)
            } else if let Ok(ck) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counter", "counters"])
            {
                Some(Some(ck))
            } else {
                None
            };
            if let Some(kind) = kind_opt
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subject),
                    kind: SubjectConditionKind::HasCounters(kind),
                });
            }
        }
        ts.restore(pcp);
    }
    // `it / ~ attacked this turn` or `it attacked during [its controller's | your] last turn`
    {
        let acp = ts.checkpoint();
        if (ts.eat_word("it") || ts.eat_tilde())
            && ts.eat_word("attacked")
            && (ts.eat_phrase(&["this", "turn"])
                || (ts.eat_word("during")
                    && ts.eat_any_phrase(&[
                        &["its", "controller's", "last", "turn"],
                        &["your", "last", "turn"],
                    ])))
        {
            return Ok(ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                state: ObjectStateKind::AttackedOrBlockedThisTurn,
            });
        }
        ts.restore(acp);
    }
    // `you attacked with [a/an | exactly <N> | <N> or more] <flt> this turn/combat`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["you", "attacked", "with"]) {
            let _ = ts.eat_word("exactly");
            let n = if let Some(num) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                num
            } else {
                let _ = ts.eat_any_word(&["a", "an"]);
                1
            };
            if try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_any_phrase(&[&["this", "turn"], &["this", "combat"]])
            {
                return Ok(player_check(
                    PlayerRef::You,
                    PlayerConditionKind::AttackedWithCount {
                        min_count: to_min_u16(n),
                    },
                ));
            }
        }
        ts.restore(acp);
    }
    // `it has the greatest/least power/toughness [or is tied for greatest/least ...] among <flt>`
    {
        let gcp = ts.checkpoint();
        if (ts.eat_word("it") || ts.eat_tilde()) && ts.eat_phrase(&["has", "the"]) {
            let greatest = if ts.eat_any_word(&["greatest", "highest"]) {
                Some(true)
            } else if ts.eat_any_word(&["least", "lowest"]) {
                Some(false)
            } else {
                None
            };
            let stat = if ts.eat_word("power") {
                Some(StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if let (Some(greatest), Some(stat)) = (greatest, stat) {
                let _ = ts.eat_any_phrase(&[
                    &["or", "is", "tied", "for", "greatest", "power"],
                    &["or", "is", "tied", "for", "greatest", "toughness"],
                    &["or", "is", "tied", "for", "greatest"],
                    &["or", "is", "tied", "for", "least", "power"],
                    &["or", "is", "tied", "for", "least", "toughness"],
                    &["or", "is", "tied", "for", "least"],
                ]);
                if ts.eat_word("among") && try_parse(ts, parse_object_filter).is_ok() {
                    return Ok(ConditionExpr::EventCheck(
                        EventCheckKind::TiedForExtremeStat { stat, greatest },
                    ));
                }
            }
        }
        ts.restore(gcp);
    }
    if ts.eat_any_phrase(&[
        &["you", "created", "a", "token", "this", "turn"],
        &["you've", "created", "a", "token", "this", "turn"],
    ]) {
        return Ok(prior_event(PriorActionKind::CreatedThisWay));
    }
    if ts.eat_any_phrase(&[
        &[
            "an",
            "opponent",
            "has",
            "been",
            "dealt",
            "noncombat",
            "damage",
            "this",
            "turn",
        ],
        &[
            "an",
            "opponent",
            "was",
            "dealt",
            "noncombat",
            "damage",
            "this",
            "turn",
        ],
    ]) {
        return Ok(cmp_cond(
            ValueExpr::DamageDealtToOpponentsThisTurn {
                noncombat_only: true,
            },
            CompareOp::Ge,
            ValueExpr::Const(1),
        ));
    }
    {
        let dcp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["died", "under", "your", "control", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::CreatureDied,
                filter: Some(Box::new(flt)),
                min_count: 1,
            });
        }
        ts.restore(dcp);
    }
    // `<subj> has [a/an | <N> or more] [<kind>] counter(s) on it`
    {
        let hcp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it")) && ts.eat_word("has") {
            let min_n = if ts.eat_any_word(&["a", "an"]) {
                Some(1i32)
            } else if let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&["or", "more"])
            {
                Some(n)
            } else {
                None
            };
            if let Some(n) = min_n
                && let Some(kind) = eat_optional_counter_kind(ts)
                && ts.eat_phrase(&["on", "it"])
            {
                return Ok(cmp_cond(
                    ValueExpr::CounterCount {
                        subject: Box::new(SubjectRef::SelfObject),
                        kind,
                    },
                    CompareOp::Ge,
                    ValueExpr::Const(n),
                ));
            }
        }
        ts.restore(hcp);
    }
    if ts.eat_any_phrase(&[
        &["it", "was", "bargained"],
        &["this", "spell", "was", "bargained"],
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::WasBargained));
    }
    if ts.eat_any_phrase(&[
        &["no", "colored", "mana", "was", "spent", "to", "cast", "it"],
        &[
            "no", "colored", "mana", "was", "spent", "to", "cast", "this", "spell",
        ],
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::ManaSpentNoColor));
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "bargained"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::WasBargained));
        }
        ts.restore(cp);
    }
    // `[at least one | two or more | one or more] <flt> was/is milled/exiled/discarded/destroyed/revealed/sacrificed/returned/beheld [this way]`
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_any_phrase(&[
            &["at", "least", "one"],
            &["two", "or", "more"],
            &["one", "or", "more"],
        ]);
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_word(&["was", "were", "is", "are"])
        {
            if ts.eat_word("beheld") {
                return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
            }
            const PRIOR_ACTS: &[(&[&str], PriorActionKind)] = &[
                (&["milled", "this", "way"], PriorActionKind::MilledThisWay),
                (&["exiled", "this", "way"], PriorActionKind::ExiledThisWay),
                (
                    &["discarded", "this", "way"],
                    PriorActionKind::DiscardedThisWay,
                ),
                (
                    &["destroyed", "this", "way"],
                    PriorActionKind::DestroyedThisWay,
                ),
                (
                    &["revealed", "this", "way"],
                    PriorActionKind::RevealedThisWay,
                ),
                (
                    &["sacrificed", "this", "way"],
                    PriorActionKind::SacrificedThisWay,
                ),
                (
                    &["returned", "this", "way"],
                    PriorActionKind::ReturnedThisWay,
                ),
                (&["created", "this", "way"], PriorActionKind::CreatedThisWay),
                (&["cast", "this", "way"], PriorActionKind::CastThisWay),
                (
                    &[
                        "put",
                        "into",
                        "a",
                        "graveyard",
                        "from",
                        "the",
                        "battlefield",
                        "this",
                        "turn",
                    ],
                    PriorActionKind::DestroyedThisWay,
                ),
                (
                    &[
                        "put",
                        "into",
                        "your",
                        "graveyard",
                        "from",
                        "anywhere",
                        "this",
                        "turn",
                    ],
                    PriorActionKind::DestroyedThisWay,
                ),
                (
                    &[
                        "put",
                        "into",
                        "a",
                        "graveyard",
                        "from",
                        "anywhere",
                        "this",
                        "turn",
                    ],
                    PriorActionKind::DestroyedThisWay,
                ),
            ];
            if let Some(action) = eat_table(ts, PRIOR_ACTS) {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(cp);
    }
    if ts.eat_any_phrase(&[
        &["it", "was", "kicked"],
        &["that", "spell", "was", "kicked"],
    ]) {
        if ts.eat_phrase(&["with", "its"])
            && let Some(mc) = ts.eat_mana_cost()
            && ts.eat_word("kicker")
        {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KickedWith(mc)));
        }
        let _ = ts.eat_word("twice");
        return Ok(ConditionExpr::Flag(ConditionFlag::WasKicked));
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["you", "cast", "it"], &["you", "cast", "them"]])
            || (ts.eat_phrase(&["you", "cast"]) && ts.eat_tilde())
        {
            let _ = ts.eat_any_phrase(&[
                &["from", "your", "hand"],
                &["during", "your", "main", "phase"],
                &["this", "way"],
            ]);
            return Ok(ConditionExpr::Flag(ConditionFlag::YouCastIt));
        }
        ts.restore(cp);
    }
    if ts.eat_any_phrase(&[
        &[
            "a",
            "permanent",
            "left",
            "the",
            "battlefield",
            "under",
            "your",
            "control",
            "this",
            "turn",
        ],
        &[
            "a",
            "permanent",
            "you",
            "controlled",
            "left",
            "the",
            "battlefield",
            "this",
            "turn",
        ],
        &[
            "a",
            "permanent",
            "left",
            "the",
            "battlefield",
            "this",
            "turn",
        ],
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::PermanentLeftThisTurn));
    }
    if ts.eat_any_phrase(&[
        &["that", "creature", "dies", "this", "way"],
        &["that", "creature", "dies", "this", "turn"],
    ]) {
        return Ok(prior_event(PriorActionKind::DestroyedThisWay));
    }
    if ts.eat_phrase(&["that", "spell", "is", "countered", "this", "way"]) {
        return Ok(prior_event(PriorActionKind::ExiledThisWay));
    }
    if ts.eat_phrase(&[
        "a", "player", "other", "than", "you", "lost", "life", "this", "turn",
    ]) {
        return Ok(player_check(
            PlayerRef::EachOtherPlayer,
            PlayerConditionKind::LostLifeThisTurn,
        ));
    }
    // `an opponent had <a/an/N [or more]> <flt> enter [the battlefield] under their control this turn`
    {
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["an", "opponent", "had"]) {
            let min_count = if ts.eat_any_word(&["a", "an"]) {
                1u16
            } else if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                to_min_u16(n)
            } else {
                1u16
            };
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_any_phrase(&[
                    &[
                        "enter",
                        "the",
                        "battlefield",
                        "under",
                        "their",
                        "control",
                        "this",
                        "turn",
                    ],
                    &["enter", "under", "their", "control", "this", "turn"],
                ])
            {
                return Ok(ConditionExpr::PermanentEnteredThisTurn {
                    filter: Box::new(flt),
                    controller: PlayerRef::AnyOpponent,
                    min_count,
                });
            }
        }
        ts.restore(ocp);
    }
    // `an opponent drew <N> or more cards this turn`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["an", "opponent", "drew"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::AnyOpponent,
                event: TurnEventKind::CardDrawn,
                filter: None,
                min_count: to_min_u16(n),
            });
        }
        ts.restore(dcp);
    }
    // `exactly <N> <flt> is/are attacking`
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("exactly")
            && let Some(n) = ts.next_number(false)
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_phrase(&[&["is", "attacking"], &["are", "attacking"]])
        {
            flt.combat = Some(CombatState::Attacking);
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(flt)),
                CompareOp::Eq,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(ecp);
    }
    // `it shares a color / creature type with <flt>`
    {
        let scp = ts.checkpoint();
        if (ts.eat_word("it") || ts.eat_tilde()) && ts.eat_phrase(&["shares", "a"]) {
            const PROPS: &[(&[&str], SharedProperty)] = &[
                (&["color"], SharedProperty::Color),
                (&["creature", "type"], SharedProperty::CreatureType),
                (&["card", "type"], SharedProperty::CardType),
            ];
            if let Some(kind) = eat_table(ts, PROPS)
                && ts.eat_word("with")
                && let Ok(other) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ConditionExpr::SharesTypeWith {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    other: Box::new(other),
                    kind,
                });
            }
        }
        ts.restore(scp);
    }
    // `~ is the only <flt> in <zone>`
    {
        let ncp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it"))
            && ts.eat_phrase(&["is", "the", "only"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            if flt.zones.is_empty()
                && ts.eat_word("in")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                flt.zones.push(zref);
            }
            if !flt.zones.is_empty() {
                return Ok(cmp_cond(
                    ValueExpr::Count(Box::new(flt)),
                    CompareOp::Eq,
                    ValueExpr::Const(1),
                ));
            }
        }
        ts.restore(ncp);
    }

    if ts.eat_any_phrase(&[
        &["a", "creature", "died", "this", "turn"],
        &[
            "a", "creature", "died", "under", "your", "control", "this", "turn",
        ],
        &["a", "creature", "you", "controlled", "died", "this", "turn"],
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::PermanentDiedThisTurn));
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "kicked"]) {
            if ts.eat_phrase(&["with", "its"])
                && let Some(mc) = ts.eat_mana_cost()
                && ts.eat_word("kicker")
            {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::KickedWith(mc)));
            }
            let _ = ts.eat_word("twice");
            return Ok(ConditionExpr::Flag(ConditionFlag::WasKicked));
        }
        ts.restore(cp);
    }
    // `{M} was spent to cast it/~`
    {
        let mscp = ts.checkpoint();
        if let Some(mc) = ts.eat_mana_cost()
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && eat_was_cast_target(ts)
        {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::ManaCostSpent(mc)));
        }
        ts.restore(mscp);
    }
    // `creatures you control have total power <N> or greater`
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&["creatures", "you", "control", "have", "total", "power"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_any_phrase(&[&["or", "greater"], &["or", "more"]])
        {
            return Ok(cmp_cond(
                ValueExpr::AggregateStat {
                    op: AggregateOp::Sum,
                    stat: StatKind::Power,
                    filter: Box::new(ObjectFilter {
                        any_types: CardTypeMask::CREATURE,
                        controller: Some(ControllerRelation::You),
                        ..Default::default()
                    }),
                },
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(fcp);
    }
    // `<N> or more <flt> are in <zone>`
    {
        let zcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["are", "in"])
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone,
                min_count: to_min_u16(n),
            });
        }
        ts.restore(zcp);
    }
    // `<N> or more creatures are attacking`
    {
        let acp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["are", "attacking"])
        {
            flt.combat = Some(CombatState::Attacking);
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(flt)),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(acp);
    }
    // `<N> or more creatures died this turn`
    {
        let dcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && ts.eat_any_word(&["creatures", "permanents"])
            && ts.eat_phrase(&["died", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::EachPlayer,
                event: TurnEventKind::CreatureDied,
                filter: None,
                min_count: to_min_u16(n),
            });
        }
        ts.restore(dcp);
    }

    if ts.eat_any_phrase(&[
        &[
            "one",
            "or",
            "more",
            "cards",
            "left",
            "your",
            "graveyard",
            "this",
            "turn",
        ],
        &["a", "card", "left", "your", "graveyard", "this", "turn"],
    ]) {
        return Ok(player_check(
            PlayerRef::You,
            PlayerConditionKind::CardsLeftGraveyardThisTurn,
        ));
    }
    if ts.eat_any_phrase(&[
        &[
            "a",
            "land",
            "entered",
            "the",
            "battlefield",
            "under",
            "your",
            "control",
            "this",
            "turn",
        ],
        &["a", "land", "you", "control", "entered", "this", "turn"],
    ]) {
        return Ok(player_check(
            PlayerRef::You,
            PlayerConditionKind::LandEnteredThisTurn,
        ));
    }
    // `<N> or more mana was spent to cast <subj>`
    {
        let m2cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "mana", "was", "spent", "to", "cast"])
            && try_parse(ts, parse_subject_ref).is_ok()
        {
            return Ok(cmp_cond(
                ValueExpr::ManaPaidThisWay,
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(m2cp);
    }
    // `<N> or more damage was dealt to <subj> this turn`
    {
        let dmg_cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "damage", "was", "dealt", "to"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(subject),
                kind: SubjectConditionKind::DamageDealtThisTurn {
                    min_damage: to_min_u16(n),
                },
            });
        }
        ts.restore(dmg_cp);
    }
    // `there is a/an <flt> in <zone>`
    {
        let ticp = ts.checkpoint();
        if ts.eat_phrase(&["there", "is"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
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
                    min_count: 1,
                });
            }
        }
        ts.restore(ticp);
    }
    // Delegated `there is / there are ...` existential and threshold conditions
    if let Some(cond) = extended::parse_there_are_condition(ts) {
        return Ok(cond);
    }

    // `your life total is <N> or less/more`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["your", "life", "total", "is"])
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            let op = eat_suffix_cmp_op(ts, true, false);
            return Ok(cmp_cond(
                player_stat_val(PlayerRef::You, PlayerStatKind::LifeTotal),
                op,
                val,
            ));
        }
        ts.restore(cp);
    }

    if ts.eat_any_phrase(&[
        &["you've", "lost", "life", "this", "turn"],
        &["you", "lost", "life", "this", "turn"],
    ]) {
        return Ok(player_check(
            PlayerRef::You,
            PlayerConditionKind::LostLifeThisTurn,
        ));
    }
    {
        let acp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "creatures", "attacked", "this", "turn"])
        {
            return Ok(player_check(
                PlayerRef::You,
                PlayerConditionKind::AttackedWithCount {
                    min_count: to_min_u16(n),
                },
            ));
        }
        ts.restore(acp);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["attacked", "this", "turn"]) {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::SelfObject),
                kind: SubjectConditionKind::AttackedThisCombat { or_blocked: false },
            });
        }
        ts.restore(lcp);
    }
    if ts.eat_phrase(&["evidence", "was", "collected"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
    }
    if ts.eat_any_phrase(&[
        &[
            "one", "or", "more", "cards", "were", "put", "into", "exile", "this", "turn",
        ],
        &[
            "one", "or", "more", "of", "them", "entered", "from", "exile", "or", "was", "cast",
            "from", "exile",
        ],
        &[
            "one",
            "or",
            "more",
            "of",
            "them",
            "entered",
            "from",
            "a",
            "graveyard",
            "or",
            "was",
            "cast",
            "from",
            "a",
            "graveyard",
        ],
    ]) {
        return Ok(prior_event(PriorActionKind::ExiledThisWay));
    }
    // `the top card of your library is <color/flt>`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top", "card", "of"])
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
            && ts.eat_word("is")
        {
            let _ = ts.eat_any_word(&["a", "an"]);
            if let Ok(flt) = try_parse(ts, parse_object_filter) {
                return Ok(ConditionExpr::ZoneHasCount {
                    filter: Box::new(flt),
                    zone,
                    min_count: 1,
                });
            }
            if let Some(w) = ts.peek_word()
                && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                return Ok(ConditionExpr::ZoneHasCount {
                    filter: Box::new(ObjectFilter {
                        any_colors: c,
                        ..Default::default()
                    }),
                    zone,
                    min_count: 1,
                });
            }
        }
        ts.restore(tcp);
    }
    // `~ is still on the battlefield`
    {
        let scp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["is", "still", "on", "the", "battlefield"]) {
            return Ok(ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::SelfObject),
                filter: Box::new(ObjectFilter {
                    any_permanent: true,
                    ..Default::default()
                }),
            });
        }
        ts.restore(scp);
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

    player_subject::parse_player_or_subject_condition(ts)
}

pub(super) fn at_cond_boundary(ts: &TokenStream<'_>) -> bool {
    matches!(
        ts.peek().map(|t| &t.kind),
        None | Some(crate::v2::lexer::TokenKind::Comma | crate::v2::lexer::TokenKind::Period)
    ) || ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("and") || w.eq_ignore_ascii_case("or"))
}
