//! Player-headed and subject-headed [`ConditionExpr`] predicates for `v2`.

use crate::ast::{CompareOp, PlayerStatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, EventCheckKind, FilterConditionKind,
    ObjectStateKind, PlayerConditionKind, PlayerRef, PriorActionKind, SubjectConditionKind,
    SubjectRef, TurnEventKind, ValueExpr, ZoneMetricKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

pub(super) fn parse_player_or_subject_condition(
    ts: &mut TokenStream<'_>,
) -> PResult<ConditionExpr> {
    // Player-headed conditions (`you control ...`, `you've gained life this turn`, `an opponent has cast a ... spell this turn`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("control") || ts.eat_word("controls") {
                if player == PlayerRef::You && ts.eat_tilde() {
                    return Ok(ConditionExpr::Flag(ConditionFlag::ControlsSelf));
                }
                if ts.eat_word("more")
                    && let Ok(filter) = try_parse(ts, parse_object_filter)
                    && ts.eat_word("than")
                    && let Ok(other) = try_parse(ts, parse_player_ref)
                {
                    return Ok(ConditionExpr::ControlsMoreThan {
                        player,
                        other,
                        filter: Box::new(filter),
                    });
                }
                if ts.eat_word("no") {
                    let filter = parse_object_filter(ts)?;
                    return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: 1,
                    })));
                }
                let _ = ts.eat_phrase(&["at", "least"]) || ts.eat_word("exactly");
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
                let filter = match try_parse(ts, parse_object_filter) {
                    Ok(f) => f,
                    Err(e) => {
                        if !is_fewer
                            && min_count == 1
                            && let Ok(subj) = try_parse(ts, parse_subject_ref)
                        {
                            return Ok(ConditionExpr::ControlsSubject {
                                player,
                                subject: Box::new(subj),
                            });
                        }
                        return Err(e);
                    }
                };
                // Support `you control a permanent of each color`
                if ts.eat_phrase(&["of", "each", "color"]) {
                    return Ok(ConditionExpr::FilterCheck {
                        filter: Box::new(filter),
                        kind: FilterConditionKind::ColorsAmong,
                        value: 5,
                    });
                }
                let first_cond = if is_fewer {
                    ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: min_count.saturating_add(1),
                    }))
                } else {
                    ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count,
                    }
                };
                // Support `you control <flt1> and <flt2>` (`Okiba Salvage`, `Of One Mind`, `Raka Sanctuary`, `When We Were Young`)
                let acp = ts.checkpoint();
                if ts.eat_word("and") {
                    let min2 = if let Some(n2) = ts.next_number(false) {
                        let _ = ts.eat_phrase(&["or", "more"]);
                        u16::try_from(n2.max(1)).unwrap_or(1)
                    } else {
                        1
                    };
                    if let Ok(flt2) = try_parse(ts, parse_object_filter) {
                        return Ok(ConditionExpr::And(
                            Box::new(first_cond),
                            Box::new(ConditionExpr::Controls {
                                player,
                                filter: Box::new(flt2),
                                min_count: min2,
                            }),
                        ));
                    }
                }
                ts.restore(acp);
                return Ok(first_cond);
            }
            if ts.eat_word("has") || ts.eat_word("have") {
                if ts.eat_phrase(&["more", "life", "than"])
                    && let Ok(other) = try_parse(ts, parse_player_ref)
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                            player,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        }),
                        op: CompareOp::Gt,
                        rhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                            player: other,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        }),
                    });
                }
                if ts.eat_phrase(&["no", "cards", "in", "hand"])
                    || ts.eat_phrase(&["no", "cards", "in", "their", "hand"])
                    || ts.eat_phrase(&["no", "cards", "in", "your", "hand"])
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                            player,
                            stat: crate::ast::PlayerStatKind::CardsInHand,
                        }),
                        op: CompareOp::Eq,
                        rhs: Box::new(crate::v2::ir::ValueExpr::Const(0)),
                    });
                }
                let _ = ts.eat_word("exactly");
                if let Some(n) = ts.next_number(false) {
                    let op = if ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "fewer"]) {
                        CompareOp::Le
                    } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
                        CompareOp::Ge
                    } else {
                        CompareOp::Eq
                    };
                    if ts.eat_phrase(&["cards", "in", "their", "graveyard"])
                        || ts.eat_phrase(&["cards", "in", "your", "graveyard"])
                        || ts.eat_phrase(&["card", "in", "their", "graveyard"])
                    {
                        return Ok(ConditionExpr::ZoneHasCount {
                            filter: Box::new(crate::v2::ir::ObjectFilter {
                                any_card: true,
                                ..Default::default()
                            }),
                            zone: crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Graveyard,
                                owner: if player == PlayerRef::You {
                                    crate::v2::ir::ZoneOwner::You
                                } else {
                                    crate::v2::ir::ZoneOwner::ThatPlayer
                                },
                            },
                            min_count: u16::try_from(n.max(1)).unwrap_or(1),
                        });
                    }
                    if ts.eat_phrase(&["cards", "in", "hand"])
                        || ts.eat_phrase(&["card", "in", "hand"])
                        || ts.eat_phrase(&["cards", "in", "their", "hand"])
                        || ts.eat_phrase(&["cards", "in", "your", "hand"])
                    {
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                                player,
                                stat: crate::ast::PlayerStatKind::CardsInHand,
                            }),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                    if ts.eat_word("opponents") || ts.eat_word("opponent") {
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(crate::v2::ir::ValueExpr::OpponentsCount),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                    if ts.eat_word("life") {
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(ValueExpr::PlayerStat {
                                player,
                                stat: PlayerStatKind::LifeTotal,
                            }),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                    {
                        let fcp = ts.checkpoint();
                        if let Ok(mut flt) = try_parse(ts, parse_object_filter)
                            && let Some(zone) = flt.zones.pop()
                        {
                            return Ok(ConditionExpr::ZoneHasCount {
                                filter: Box::new(flt),
                                zone,
                                min_count: u16::try_from(n.max(1)).unwrap_or(1),
                            });
                        }
                        ts.restore(fcp);
                    }
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && (ts.eat_word("counters") || ts.eat_word("counter"))
                    {
                        return Ok(ConditionExpr::PlayerHasCounters {
                            player,
                            kind,
                            min_count: u16::try_from(n.max(1)).unwrap_or(1),
                        });
                    }
                }
            }
            if ts.eat_word("own") || ts.eat_word("owns") {
                let min_count = if let Some(n) = ts.next_number(false) {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    u16::try_from(n.max(1)).unwrap_or(1)
                } else {
                    1
                };
                if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                    let zone = flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Exile,
                        owner: crate::v2::ir::ZoneOwner::Default,
                    });
                    return Ok(ConditionExpr::ZoneHasCount {
                        filter: Box::new(flt),
                        zone,
                        min_count,
                    });
                }
            }
            if ts.eat_phrase(&["don't", "have"]) || ts.eat_phrase(&["doesn't", "have"]) {
                let hcp = ts.checkpoint();
                let _ = ts.eat_word("any") || ts.eat_word("a") || ts.eat_word("an");
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                {
                    return Ok(ConditionExpr::Not(Box::new(
                        ConditionExpr::PlayerHasCounters {
                            player,
                            kind,
                            min_count: 1,
                        },
                    )));
                }
                ts.restore(hcp);
            }
            if ts.eat_phrase(&["don't", "control"]) || ts.eat_phrase(&["doesn't", "control"]) {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                if let Ok(filter) = try_parse(ts, parse_object_filter) {
                    return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: 1,
                    })));
                }
                if let Ok(subj) = try_parse(ts, parse_subject_ref) {
                    return Ok(ConditionExpr::Not(Box::new(
                        ConditionExpr::ControlsSubject {
                            player,
                            subject: Box::new(subj),
                        },
                    )));
                }
            }
            if ts.eat_phrase(&["has", "cast"]) || ts.eat_word("cast") {
                let min_count = if let Some(n) = ts.next_number(false) {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    u16::try_from(n.max(1)).unwrap_or(1)
                } else {
                    1
                };
                let filter = opt(ts, parse_object_filter).map(Box::new);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::SpellCast,
                        filter,
                        min_count,
                    });
                }
            }
            if ts.eat_phrase(&["attacked", "with", "creatures", "with", "total", "power"])
                && let Some(n) = ts.next_number(false)
                && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]))
            {
                let _ = ts.eat_phrase(&["this", "combat"]) || ts.eat_phrase(&["this", "turn"]);
                return Ok(ConditionExpr::PlayerCheck {
                    player,
                    kind: PlayerConditionKind::AttackedWithTotalPower { min_power: n },
                });
            }
            if ts.eat_phrase(&["gained", "and", "lost", "life", "this", "turn"]) {
                return Ok(ConditionExpr::And(
                    Box::new(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeGained,
                        filter: None,
                        min_count: 1,
                    }),
                    Box::new(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeLost,
                        filter: None,
                        min_count: 1,
                    }),
                ));
            }
            if ts.eat_phrase(&["gained", "or", "lost", "life", "this", "turn"]) {
                return Ok(ConditionExpr::Or(
                    Box::new(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeGained,
                        filter: None,
                        min_count: 1,
                    }),
                    Box::new(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeLost,
                        filter: None,
                        min_count: 1,
                    }),
                ));
            }
            if ts.eat_phrase(&["lost", "life", "last", "turn"]) {
                return Ok(ConditionExpr::PlayerCheck {
                    player,
                    kind: PlayerConditionKind::LostLifeLastTurn,
                });
            }
            if ts.eat_phrase(&["didn't", "cast", "a", "spell", "this", "turn"]) {
                return Ok(ConditionExpr::Not(Box::new(
                    ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::SpellCast,
                        filter: None,
                        min_count: 1,
                    },
                )));
            }
            if ts.eat_phrase(&["didn't", "cast", "it", "from", "your", "hand"])
                || ts.eat_phrase(&["didn't", "cast", "it", "from", "their", "hand"])
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                    ConditionFlag::WasCastFromHand,
                ))));
            }
            if ts.eat_word("controlled")
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        controller: Some(crate::ast::ControllerRelation::You),
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&["attacked", "with"])
                && let Some(n) = ts.next_number(false)
            {
                let _ = ts.eat_phrase(&["or", "more"]);
                if (ts.eat_word("creatures") || ts.eat_word("creature"))
                    && ts.eat_phrase(&["this", "turn"])
                {
                    return Ok(ConditionExpr::PlayerCheck {
                        player,
                        kind: PlayerConditionKind::AttackedWithCount {
                            min_count: u16::try_from(n.max(1)).unwrap_or(1),
                        },
                    });
                }
            }
            if ts.eat_phrase(&["was", "dealt"]) || ts.eat_phrase(&["were", "dealt"]) {
                let combat_only = ts.eat_word("combat");
                let _ = ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    if ts.eat_word("by") {
                        let _ = opt(ts, parse_subject_ref);
                    }
                    if ts.eat_phrase(&["this", "turn"]) {
                        return Ok(ConditionExpr::PlayerCheck {
                            player,
                            kind: PlayerConditionKind::DealtDamageThisTurn { combat_only },
                        });
                    }
                }
            }
            if ts.eat_word("revealed")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["this", "way"])
            {
                return Ok(ConditionExpr::FilterCheck {
                    filter: Box::new(flt),
                    kind: FilterConditionKind::RevealedThisWay,
                    value: 1,
                });
            }
            if ts.eat_word("had") {
                let min_count = if let Some(n) = ts.next_number(false) {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    u16::try_from(n.max(1)).unwrap_or(1)
                } else {
                    1
                };
                if let Ok(flt) = try_parse(ts, parse_object_filter)
                    && (ts.eat_phrase(&[
                        "enter",
                        "the",
                        "battlefield",
                        "under",
                        "your",
                        "control",
                        "this",
                        "turn",
                    ]) || ts.eat_phrase(&[
                        "enter",
                        "the",
                        "battlefield",
                        "under",
                        "their",
                        "control",
                        "this",
                        "turn",
                    ]) || ts.eat_phrase(&["enter", "under", "your", "control", "this", "turn"])
                        || ts.eat_phrase(&["enter", "under", "their", "control", "this", "turn"])
                        || ts.eat_phrase(&["enter", "the", "battlefield", "this", "turn"]))
                {
                    return Ok(ConditionExpr::PermanentEnteredThisTurn {
                        filter: Box::new(flt),
                        controller: player,
                        min_count,
                    });
                }
            }
            if ts.eat_word("gained") || ts.eat_phrase(&["has", "gained"]) {
                let min_count = ts
                    .next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_phrase(&["life", "this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeGained,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_word("discarded") || ts.eat_phrase(&["has", "discarded"]) {
                let min_count = if ts.eat_word("a") {
                    1
                } else {
                    ts.next_number(false)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(1)
                };
                let _ = ts.eat_phrase(&["or", "more"]);
                if (ts.eat_word("card") || ts.eat_word("cards")) && ts.eat_phrase(&["this", "turn"])
                {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::CardDiscarded,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_word("lost") || ts.eat_phrase(&["has", "lost"]) {
                let min_count = ts
                    .next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_phrase(&["life", "this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeLost,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_phrase(&["you've", "cast"]) {
                let filter = opt(ts, parse_object_filter).map(Box::new);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player: PlayerRef::You,
                        event: TurnEventKind::SpellCast,
                        filter,
                        min_count: 1,
                    });
                }
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

    // `you've drawn [more than] <N> [or more] cards this turn` / `you drew <N> [or more] cards this turn`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "drawn"]) || ts.eat_phrase(&["you", "drew"]) {
            let more_than = ts.eat_phrase(&["more", "than"]);
            if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                if (ts.eat_word("cards") || ts.eat_word("card")) && ts.eat_phrase(&["this", "turn"])
                {
                    let min_val = if more_than { n.saturating_add(1) } else { n };
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player: PlayerRef::You,
                        event: TurnEventKind::CardDrawn,
                        filter: None,
                        min_count: u16::try_from(min_val.max(1)).unwrap_or(1),
                    });
                }
            }
        }
        ts.restore(dcp);
    }

    // `<subj> is in the command zone or on the battlefield` (`Inalla`, `Edgar Markov`, `The Ur-Dragon`, Eminence cycle)
    {
        let ecp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&[
                "is",
                "in",
                "the",
                "command",
                "zone",
                "or",
                "on",
                "the",
                "battlefield",
            ])
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(subj),
                kind: SubjectConditionKind::InCommandZoneOrBattlefield,
            });
        }
        ts.restore(ecp);
    }
    // `it isn't a mana ability` (`Harsh Mentor` + 7+ cards)
    if ts.eat_phrase(&["it", "isn't", "a", "mana", "ability"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsNotManaAbility));
    }
    // `there are no <flt> [on the battlefield | in <zone>]` (`Spirit Mirror` + 10+ cards)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            let zone = flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                zone: crate::v2::ir::ZoneKind::Battlefield,
                owner: crate::v2::ir::ZoneOwner::Default,
            });
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone,
                min_count: 1,
            })));
        }
        ts.restore(ncp);
    }
    // `two or more <flt> are tied for greatest/least <stat>` (`Desecrator Hag`, `Drop of Honey`, `Purging Scythe`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["two", "or", "more"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["are", "tied", "for"])
        {
            let _ = ts.eat_word("the");
            let greatest = if ts.eat_word("greatest") || ts.eat_word("highest") {
                Some(true)
            } else if ts.eat_word("least") || ts.eat_word("lowest") {
                Some(false)
            } else {
                None
            };
            let stat = if ts.eat_word("power") {
                Some(crate::ast::StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(crate::ast::StatKind::Toughness)
            } else if ts.eat_phrase(&["mana", "value"]) {
                Some(crate::ast::StatKind::ManaValue)
            } else {
                None
            };
            if let (Some(greatest), Some(stat)) = (greatest, stat) {
                return Ok(ConditionExpr::EventCheck(
                    EventCheckKind::TiedForExtremeStat { stat, greatest },
                ));
            }
        }
        ts.restore(tcp);
    }
    // `<flt> is/are put into a graveyard this way` (`Come Back Wrong` + 8+ cards)
    {
        let gcp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["is", "put", "into", "a", "graveyard", "this", "way"])
                || ts.eat_phrase(&["are", "put", "into", "a", "graveyard", "this", "way"])
                || ts.eat_phrase(&["is", "put", "into", "your", "graveyard", "this", "way"]))
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::MilledThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(gcp);
    }
    // `no opponent controls <flt>` (`Kavu Runner`, `Vexing Beetle`)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["no", "opponent", "controls"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                player: PlayerRef::AnyOpponent,
                filter: Box::new(flt),
                min_count: 1,
            })));
        }
        ts.restore(ncp);
    }
    // `a library has <N> or fewer cards in it` (`Isleback Spawn`)
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "library", "has"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "fewer", "cards", "in", "it"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInLibrary,
                }),
                op: CompareOp::Le,
                rhs: Box::new(ValueExpr::Const(i32::from(
                    u16::try_from(n.max(0)).unwrap_or(0),
                ))),
            });
        }
        ts.restore(lcp);
    }
    // `<subj> had counters on it` / `<subj> has [a|an|<kind>] counters on it` / `<subj> doesn't have a <kind> counter on it`
    // / `<subj> has the chosen name` / `<subj> dealt [combat] damage [to <tgt>] this turn` / `<subj> hasn't/haven't dealt [combat] damage yet`
    // / `<subj> attacked [or blocked] this combat` / `<subj> didn't enter the battlefield this turn` / `<subj> isn't prepared` / `<subj> was[n't] cast`
    {
        let scp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["had", "counters", "on", "it"])
                || ts.eat_phrase(&["had", "counters", "on", "them"])
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HadCountersOnIt,
                });
            }
            if ts.eat_phrase(&["has", "counters", "on", "it"])
                || ts.eat_phrase(&["have", "counters", "on", "them"])
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasCounters(None),
                });
            }
            {
                let hcp = ts.checkpoint();
                if (ts.eat_word("has") || ts.eat_word("have"))
                    && {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        true
                    }
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
                {
                    return Ok(ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::HasCounters(Some(ck)),
                    });
                }
                ts.restore(hcp);
            }
            {
                let ncp = ts.checkpoint();
                if (ts.eat_phrase(&["doesn't", "have"]) || ts.eat_phrase(&["don't", "have"]))
                    && {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        true
                    }
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
                {
                    return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::HasCounters(Some(ck)),
                    })));
                }
                ts.restore(ncp);
            }
            if ts.eat_phrase(&["has", "the", "chosen", "name"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasChosenName,
                });
            }
            if ts.eat_phrase(&["was", "cast"])
                && !ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("from")
                        || w.eq_ignore_ascii_case("for")
                        || w.eq_ignore_ascii_case("using")
                })
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::WasCast,
                });
            }
            if ts.eat_phrase(&["wasn't", "cast"]) {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::WasCast,
                })));
            }
            if ts.eat_phrase(&["isn't", "prepared"]) {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Modified,
                })));
            }
            if ts.eat_phrase(&["didn't", "enter", "the", "battlefield", "this", "turn"])
                || ts.eat_phrase(&["didn't", "enter", "this", "turn"])
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::EnteredThisTurn,
                })));
            }
            if ts.eat_phrase(&["attacked", "or", "blocked", "this", "combat"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::AttackedThisCombat { or_blocked: true },
                });
            }
            if ts.eat_phrase(&["attacked", "this", "combat"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::AttackedThisCombat { or_blocked: false },
                });
            }
            if ts.eat_phrase(&["hasn't", "dealt", "combat", "damage", "yet"])
                || ts.eat_phrase(&["haven't", "dealt", "combat", "damage", "yet"])
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasntDealtDamageYet { combat_only: true },
                });
            }
            if ts.eat_phrase(&["hasn't", "dealt", "damage", "yet"])
                || ts.eat_phrase(&["haven't", "dealt", "damage", "yet"])
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasntDealtDamageYet { combat_only: false },
                });
            }
            if ts.eat_word("dealt") {
                let combat_only = ts.eat_word("combat");
                if ts.eat_word("damage") {
                    if ts.eat_word("to") {
                        let _ = opt(ts, parse_subject_ref);
                    }
                    if ts.eat_phrase(&["this", "turn"]) {
                        return Ok(ConditionExpr::SubjectCheck {
                            subject: Box::new(subj),
                            kind: SubjectConditionKind::DealtDamageThisTurn { combat_only },
                        });
                    }
                }
            }
        }
        ts.restore(scp);
    }

    // `you've committed a crime this turn` / `you've discarded a card this turn` / `there are N colors among <flt>` / `there are N or more permanent types among cards in <zone>` / `~ or another <flt> entered ...`
    if ts.eat_phrase(&["you've", "committed", "a", "crime", "this", "turn"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::CommittedCrimeThisTurn,
        });
    }
    if ts.eat_phrase(&["you've", "discarded", "a", "card", "this", "turn"])
        || ts.eat_phrase(&[
            "you've",
            "discarded",
            "one",
            "or",
            "more",
            "cards",
            "this",
            "turn",
        ])
    {
        return Ok(ConditionExpr::PlayerTurnEvent {
            player: PlayerRef::You,
            event: TurnEventKind::CardDiscarded,
            filter: None,
            min_count: 1,
        });
    }
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are"])
            && let Some(n) = ts.next_number(false)
        {
            let _ = ts.eat_phrase(&["or", "more"]);
            if ts.eat_phrase(&["colors", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::FilterCheck {
                    filter: Box::new(flt),
                    kind: FilterConditionKind::ColorsAmong,
                    value: u16::from(u8::try_from(n.max(1)).unwrap_or(5)),
                });
            }
            if ts.eat_phrase(&["permanent", "types", "among", "cards", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::ZoneMetric {
                    zone,
                    kind: ZoneMetricKind::DistinctPermanentTypes,
                    min_count: u16::from(u8::try_from(n.max(1)).unwrap_or(4)),
                });
            }
        }
        ts.restore(tcp);
    }
    {
        let ecp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_word("or")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&[
                "entered",
                "the",
                "battlefield",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ]) || ts.eat_phrase(&["entered", "under", "your", "control", "this", "turn"])
                || ts.eat_phrase(&["entered", "the", "battlefield", "this", "turn"]))
        {
            return Ok(ConditionExpr::PermanentEnteredThisTurn {
                filter: Box::new(flt),
                controller: PlayerRef::You,
                min_count: 1,
            });
        }
        ts.restore(ecp);
    }
    if ts.eat_phrase(&["a", "creature", "is", "attacking", "you"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::CreatureAttackingYou));
    }
    {
        let lcp = ts.checkpoint();
        if (ts.eat_word("a") || ts.eat_word("an"))
            && try_parse(ts, parse_object_filter).is_ok()
            && (ts.eat_phrase(&[
                "left",
                "the",
                "battlefield",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ]) || ts.eat_phrase(&["left", "the", "battlefield", "this", "turn"]))
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentLeftThisTurn));
        }
        ts.restore(lcp);
    }
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "sacrificed"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::SacrificedPermanentThisTurn {
                player: PlayerRef::You,
                filter: Box::new(flt),
            });
        }
        ts.restore(scp);
    }

    // `you've cast [<N> or more] <flt> this turn`
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "cast"]) {
            let min_count = if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                u16::try_from(n.max(1)).unwrap_or(1)
            } else {
                1
            };
            let filter = opt(ts, parse_object_filter).map(Box::new);
            if ts.eat_phrase(&["this", "turn"]) {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter,
                    min_count,
                });
            }
        }
        ts.restore(ccp);
    }

    // General numeric value comparison (`<val1> is greater than / less than / less than or equal to / equal to <val2>`)
    {
        let cp = ts.checkpoint();
        if let Ok(lhs) = try_parse(ts, parse_value_expr)
            && ts.eat_word("is")
        {
            let op = if ts.eat_phrase(&["less", "than", "or", "equal", "to"])
                || ts.eat_phrase(&["equal", "to", "or", "less", "than"])
            {
                Some(CompareOp::Le)
            } else if ts.eat_phrase(&["greater", "than", "or", "equal", "to"])
                || ts.eat_phrase(&["equal", "to", "or", "greater", "than"])
            {
                Some(CompareOp::Ge)
            } else if ts.eat_phrase(&["less", "than"]) {
                Some(CompareOp::Lt)
            } else if ts.eat_phrase(&["greater", "than"]) {
                Some(CompareOp::Gt)
            } else if ts.eat_phrase(&["equal", "to"]) {
                Some(CompareOp::Eq)
            } else {
                None
            };
            if let Some(op) = op
                && let Ok(rhs) = try_parse(ts, parse_value_expr)
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(lhs),
                    op,
                    rhs: Box::new(rhs),
                });
            }
            // Suffix form: `<val1> is <val2> or greater / or less` (e.g. `that spell's mana value is 5 or greater`)
            if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                let suffix_op =
                    if ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]) {
                        CompareOp::Ge
                    } else if ts.eat_phrase(&["or", "less"]) {
                        CompareOp::Le
                    } else {
                        CompareOp::Eq
                    };
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(lhs),
                    op: suffix_op,
                    rhs: Box::new(rhs),
                });
            }
        }
        ts.restore(cp);
    }

    // `a card is exiled with it/~` / `one or more cards are exiled with it/~` (checked BEFORE `parse_subject_ref` so `is exiled` doesn't stop early!)
    {
        let ecp = ts.checkpoint();
        if (ts.eat_phrase(&["a", "card", "is", "exiled", "with"])
            || ts.eat_phrase(&["one", "or", "more", "cards", "are", "exiled", "with"]))
            && (ts.eat_word("it") || ts.eat_tilde())
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::CardsExiledWithSelf));
        }
        ts.restore(ecp);
    }

    // Subject-headed conditions (`~ is in your graveyard`, `~ is untapped`, `<subj> has/doesn't have <kw>`, `it's a <filter>`)
    {
        let cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("targets")
                && {
                    let _ = ts.eat_phrase(&["one", "or", "more"]);
                    true
                }
                && let Ok(filter) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::TargetsFilter {
                    subject: Box::new(subj),
                    filter: Box::new(filter),
                });
            }
            if ts.eat_phrase(&["doesn't", "target"])
                && let Ok(filter) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::TargetsFilter {
                    subject: Box::new(subj),
                    filter: Box::new(filter),
                })));
            }
            if ts.eat_phrase(&["attacked", "or", "blocked", "this", "turn"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::AttackedOrBlockedThisTurn,
                });
            }
            if ts.eat_phrase(&["is", "paired", "with", "another", "creature"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Paired,
                });
            }
            if ts.eat_phrase(&["entered", "the", "battlefield"]) || ts.eat_word("entered") {
                let under_ctrl = ts.eat_phrase(&["under", "your", "control"])
                    || ts.eat_phrase(&["under", "their", "control"])
                    || ts.eat_phrase(&["under", "an", "opponent's", "control"]);
                if ts.eat_phrase(&["this", "turn"]) || under_ctrl {
                    return Ok(ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::EnteredThisTurn,
                    });
                }
            }
            if ts.eat_phrase(&["is", "on", "the", "battlefield"])
                || ts.eat_phrase(&["are", "on", "the", "battlefield"])
                || ts.eat_phrase(&["remains", "on", "the", "battlefield"])
            {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone: crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Battlefield,
                        owner: crate::v2::ir::ZoneOwner::Default,
                    },
                });
            }
            if ts.eat_phrase(&["remains", "attached", "to"])
                || ts.eat_phrase(&["is", "attached", "to"])
                || ts.eat_phrase(&["was", "attached", "to"])
            {
                if let Ok(target) = try_parse(ts, parse_object_filter) {
                    return Ok(ConditionExpr::SubjectAttachedTo {
                        subject: Box::new(subj),
                        target: Box::new(target),
                    });
                }
                if ts.eat_word("it") {
                    return Ok(ConditionExpr::SubjectState {
                        subject: Box::new(subj),
                        state: ObjectStateKind::Enchanted,
                    });
                }
            }
            if ts.eat_phrase(&["remains", "exiled"]) || ts.eat_phrase(&["remain", "exiled"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::RemainsExiled,
                });
            }
            if ts.eat_phrase(&["remains", "tapped"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Tapped,
                });
            }
            if ts.eat_phrase(&["remains", "untapped"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Untapped,
                });
            }
            if ts.eat_phrase(&["is", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                // Optional graveyard-order suffix (`with a creature card directly above it` / `with three or more creature cards above it`)
                {
                    let gcp = ts.checkpoint();
                    if ts.eat_word("with") {
                        let matched_order = ts
                            .eat_phrase(&["a", "creature", "card", "directly", "above", "it"])
                            || (ts.next_number(false).is_some()
                                && ts.eat_phrase(&[
                                    "or", "more", "creature", "cards", "above", "it",
                                ]));
                        if !matched_order {
                            ts.restore(gcp);
                        }
                    }
                }
                let base = ConditionExpr::InZone {
                    subject: Box::new(subj.clone()),
                    zone,
                };
                if ts.eat_phrase(&["or", "on", "the", "battlefield"]) {
                    return Ok(ConditionExpr::Or(
                        Box::new(base),
                        Box::new(ConditionExpr::InZone {
                            subject: Box::new(subj),
                            zone: crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Battlefield,
                                owner: crate::v2::ir::ZoneOwner::Default,
                            },
                        }),
                    ));
                }
                let ocp = ts.checkpoint();
                if ts.eat_phrase(&["or", "in"])
                    && let Ok(zone2) = try_parse(ts, parse_zone_ref)
                {
                    return Ok(ConditionExpr::Or(
                        Box::new(base),
                        Box::new(ConditionExpr::InZone {
                            subject: Box::new(subj),
                            zone: zone2,
                        }),
                    ));
                }
                ts.restore(ocp);
                return Ok(base);
            }
            if ts.eat_phrase(&["doesn't", "have"])
                || ts.eat_phrase(&["don't", "have"])
                || ts.eat_phrase(&["didn't", "have"])
            {
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                })));
            }
            if ts.eat_word("has") || ts.eat_word("have") || ts.eat_word("had") {
                // `<subj> has the same mana value / power / toughness / color / name / card type as <other>` (`Counterbalance`)
                {
                    let smcp = ts.checkpoint();
                    if ts.eat_phrase(&["the", "same"]) {
                        let prop = if ts.eat_phrase(&["mana", "value"]) {
                            Some(crate::v2::ir::SharedProperty::ManaValue)
                        } else if ts.eat_word("power") {
                            Some(crate::v2::ir::SharedProperty::Power)
                        } else if ts.eat_word("toughness") {
                            Some(crate::v2::ir::SharedProperty::Toughness)
                        } else if ts.eat_word("color") {
                            Some(crate::v2::ir::SharedProperty::Color)
                        } else if ts.eat_word("name") {
                            Some(crate::v2::ir::SharedProperty::Name)
                        } else if ts.eat_phrase(&["card", "type"]) {
                            Some(crate::v2::ir::SharedProperty::CardType)
                        } else {
                            None
                        };
                        if let Some(kind) = prop
                            && ts.eat_word("as")
                            && let Ok(other) = try_parse(ts, parse_subject_ref)
                        {
                            return Ok(ConditionExpr::SharesTypeWith {
                                subject: Box::new(subj),
                                other: Box::new(other),
                                kind,
                            });
                        }
                    }
                    ts.restore(smcp);
                }
                // `<subj> had/has no [<kind>] counters on it`
                {
                    let ncp = ts.checkpoint();
                    if ts.eat_word("no") {
                        let kind = if ts.eat_word("counters") || ts.eat_word("counter") {
                            Some(crate::v2::ir::CounterKind::Any)
                        } else if let Ok(ck) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counters") || ts.eat_word("counter"))
                        {
                            Some(ck)
                        } else {
                            None
                        };
                        if let Some(kind) = kind {
                            let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                            return Ok(ConditionExpr::Compare {
                                lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                                    subject: Box::new(subj),
                                    kind,
                                }),
                                op: CompareOp::Eq,
                                rhs: Box::new(crate::v2::ir::ValueExpr::Const(0)),
                            });
                        }
                    }
                    ts.restore(ncp);
                }
                // `<subj> has mana value/power/toughness <cmp> <val>` (`that spell has mana value 5 or greater`)
                {
                    let scp = ts.checkpoint();
                    let stat = if ts.eat_phrase(&["mana", "value"]) {
                        Some(crate::ast::StatKind::ManaValue)
                    } else if ts.eat_word("power") {
                        Some(crate::ast::StatKind::Power)
                    } else if ts.eat_word("toughness") {
                        Some(crate::ast::StatKind::Toughness)
                    } else {
                        None
                    };
                    if let Some(stat) = stat {
                        let op_opt = if ts.eat_phrase(&["less", "than", "or", "equal", "to"]) {
                            Some(CompareOp::Le)
                        } else if ts.eat_phrase(&["greater", "than", "or", "equal", "to"]) {
                            Some(CompareOp::Ge)
                        } else if ts.eat_phrase(&["less", "than"]) {
                            Some(CompareOp::Lt)
                        } else if ts.eat_phrase(&["greater", "than"]) {
                            Some(CompareOp::Gt)
                        } else {
                            None
                        };
                        if let Some(op) = op_opt
                            && let Ok(rhs) = try_parse(ts, parse_value_expr)
                        {
                            return Ok(ConditionExpr::Compare {
                                lhs: Box::new(crate::v2::ir::ValueExpr::ObjectStat {
                                    subject: Box::new(subj),
                                    stat,
                                }),
                                op,
                                rhs: Box::new(rhs),
                            });
                        }
                        if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                            let op = if ts.eat_phrase(&["or", "greater"])
                                || ts.eat_phrase(&["or", "more"])
                            {
                                CompareOp::Ge
                            } else if ts.eat_phrase(&["or", "less"]) {
                                CompareOp::Le
                            } else {
                                CompareOp::Eq
                            };
                            return Ok(ConditionExpr::Compare {
                                lhs: Box::new(crate::v2::ir::ValueExpr::ObjectStat {
                                    subject: Box::new(subj),
                                    stat,
                                }),
                                op,
                                rhs: Box::new(rhs),
                            });
                        }
                    }
                    ts.restore(scp);
                }
                // `<subj> has dealt <N> or more damage [to <tgt>] this turn` (`Hidetsugu Consumes All`)
                {
                    let dcp = ts.checkpoint();
                    if ts.eat_word("dealt")
                        && let Some(min_d) = ts.next_number(false)
                        && ts.eat_phrase(&["or", "more", "damage"])
                    {
                        if ts.eat_word("to") {
                            let _ = try_parse(ts, parse_subject_ref);
                        }
                        if ts.eat_phrase(&["this", "turn"]) {
                            return Ok(ConditionExpr::SubjectCheck {
                                subject: Box::new(subj),
                                kind: crate::v2::ir::SubjectConditionKind::DamageDealtThisTurn {
                                    min_damage: u16::try_from(min_d.max(1)).unwrap_or(1),
                                },
                            });
                        }
                    }
                    ts.restore(dcp);
                }
                // `<subj> has [greater power [or toughness] than <other>]` (`Evolving Adaptive`, `Pelt Collector`)
                {
                    let gcp = ts.checkpoint();
                    if ts.eat_word("greater") {
                        let stat = if ts.eat_phrase(&["power", "or", "toughness"])
                            || ts.eat_phrase(&["power", "and/or", "toughness"])
                        {
                            None
                        } else if ts.eat_word("power") {
                            Some(crate::ast::StatKind::Power)
                        } else if ts.eat_word("toughness") {
                            Some(crate::ast::StatKind::Toughness)
                        } else {
                            None
                        };
                        if ts.eat_word("than")
                            && let Ok(other) = try_parse(ts, parse_subject_ref)
                        {
                            return Ok(ConditionExpr::HasGreaterStatThan {
                                subject: Box::new(subj),
                                other: Box::new(other),
                                stat,
                            });
                        }
                    }
                    ts.restore(gcp);
                }
                // `<subj> has [fewer than | at least | exactly | <a/an/N> [or more/less]] <kind> counter(s) on it` (`Ayara's Oathsworn`)
                let hcp = ts.checkpoint();
                let prefix_op = if ts.eat_phrase(&["fewer", "than"]) {
                    Some(CompareOp::Lt)
                } else if ts.eat_phrase(&["at", "least"]) {
                    Some(CompareOp::Ge)
                } else if ts.eat_word("exactly") {
                    Some(CompareOp::Eq)
                } else {
                    None
                };
                if let Some(n) = ts.next_number(prefix_op.is_none()) {
                    let op = if let Some(pop) = prefix_op {
                        pop
                    } else if ts.eat_phrase(&["or", "more"]) {
                        CompareOp::Ge
                    } else if ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "fewer"]) {
                        CompareOp::Le
                    } else {
                        CompareOp::Eq
                    };
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && (ts.eat_word("counters") || ts.eat_word("counter"))
                    {
                        let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                                subject: Box::new(subj),
                                kind,
                            }),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                }
                ts.restore(hcp);
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                });
            }
            if ts.eat_phrase(&["was", "cast", "from"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::CastFromZone {
                    subject: Box::new(subj),
                    zone,
                });
            }
            let negated = ts.eat_word("isn't")
                || ts.eat_phrase(&["is", "not"])
                || ts.eat_word("aren't")
                || ts.eat_phrase(&["are", "not"])
                || ts.eat_word("wasn't")
                || ts.eat_phrase(&["was", "not"])
                || ts.eat_word("weren't")
                || ts.eat_phrase(&["were", "not"]);
            let positive = !negated
                && (ts.eat_word("is")
                    || ts.eat_word("are")
                    || ts.eat_word("was")
                    || ts.eat_word("were"));
            if negated || positive {
                if let Some(state) = parse_object_state(ts) {
                    if state == ObjectStateKind::Attacking {
                        let pcp = ts.checkpoint();
                        if try_parse(ts, parse_player_ref).is_err() {
                            ts.restore(pcp);
                        }
                    }
                    // `is enchanted by (exactly <N> | <N> or more) <flt>`
                    if state == ObjectStateKind::Enchanted {
                        let bcp = ts.checkpoint();
                        if ts.eat_word("by") {
                            let _ = ts.eat_word("exactly");
                            if let Some(n) = ts.next_number(false) {
                                let _ = ts.eat_phrase(&["or", "more"]);
                                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                                    let base = ConditionExpr::AttachedCount {
                                        subject: Box::new(subj),
                                        filter: Box::new(flt),
                                        min_count: u16::try_from(n.max(1)).unwrap_or(1),
                                    };
                                    return Ok(if negated {
                                        ConditionExpr::Not(Box::new(base))
                                    } else {
                                        base
                                    });
                                }
                            }
                        }
                        ts.restore(bcp);
                    }
                    let mut base = ConditionExpr::SubjectState {
                        subject: Box::new(subj.clone()),
                        state,
                    };
                    let ocp = ts.checkpoint();
                    if ts.eat_word("or")
                        && let Some(state2) = parse_object_state(ts)
                    {
                        let _ = ts.eat_word("alone");
                        base = ConditionExpr::Or(
                            Box::new(base),
                            Box::new(ConditionExpr::SubjectState {
                                subject: Box::new(subj),
                                state: state2,
                            }),
                        );
                    } else {
                        ts.restore(ocp);
                    }
                    return Ok(if negated {
                        ConditionExpr::Not(Box::new(base))
                    } else {
                        base
                    });
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    let mut base = ConditionExpr::SubjectMatches {
                        subject: Box::new(subj.clone()),
                        filter: Box::new(flt),
                    };
                    let acp = ts.checkpoint();
                    if ts.eat_word("and")
                        && let Some(state) = parse_object_state(ts)
                    {
                        base = ConditionExpr::And(
                            Box::new(base),
                            Box::new(ConditionExpr::SubjectState {
                                subject: Box::new(subj),
                                state,
                            }),
                        );
                    } else {
                        ts.restore(acp);
                    }
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

    // `it's/they're [not] ...` (`it's enchanted or equipped`, `they're not a token`, `it's a <filter>`)
    {
        let it_cp = ts.checkpoint();
        let subj_anaph = if ts.eat_word("it's") {
            Some(AnaphoraRef::It)
        } else if ts.eat_word("they're") {
            Some(AnaphoraRef::Them)
        } else {
            None
        };
        if let Some(anaph) = subj_anaph {
            let negated = ts.eat_word("not");
            {
                let bcp = ts.checkpoint();
                if !negated
                    && (ts.eat_phrase(&["blocking", "or", "blocked", "by"])
                        || ts.eat_phrase(&["blocked", "by"])
                        || ts.eat_word("blocking"))
                    && let Ok(other) = try_parse(ts, parse_object_filter)
                {
                    return Ok(ConditionExpr::SubjectCombatWith {
                        subject: Box::new(SubjectRef::Anaphora(anaph)),
                        other: Box::new(other),
                    });
                }
                ts.restore(bcp);
            }
            if let Some(state) = parse_object_state(ts) {
                let mut base = ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::Anaphora(anaph)),
                    state,
                };
                let ocp = ts.checkpoint();
                if ts.eat_word("or")
                    && let Some(state2) = parse_object_state(ts)
                {
                    let _ = ts.eat_word("alone");
                    base = ConditionExpr::Or(
                        Box::new(base),
                        Box::new(ConditionExpr::SubjectState {
                            subject: Box::new(SubjectRef::Anaphora(anaph)),
                            state: state2,
                        }),
                    );
                } else {
                    ts.restore(ocp);
                }
                return Ok(if negated {
                    ConditionExpr::Not(Box::new(base))
                } else {
                    base
                });
            }
            if let Ok(flt) = try_parse(ts, parse_object_filter) {
                let base = ConditionExpr::SubjectMatches {
                    subject: Box::new(SubjectRef::Anaphora(anaph)),
                    filter: Box::new(flt),
                };
                return Ok(if negated {
                    ConditionExpr::Not(Box::new(base))
                } else {
                    base
                });
            }
        }
        ts.restore(it_cp);
    }

    super::extended::parse_extended_condition(ts)
}

fn parse_object_state(ts: &mut TokenStream<'_>) -> Option<ObjectStateKind> {
    if ts.eat_word("tapped") {
        Some(ObjectStateKind::Tapped)
    } else if ts.eat_word("untapped") {
        Some(ObjectStateKind::Untapped)
    } else if ts.eat_phrase(&["attacking", "alone"]) {
        Some(ObjectStateKind::AttackingAlone)
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
    } else if ts.eat_word("saddled") {
        Some(ObjectStateKind::Saddled)
    } else if ts.eat_word("monstrous") {
        Some(ObjectStateKind::Monstrous)
    } else if ts.eat_phrase(&["paired", "with", "a", "creature"])
        || ts.eat_phrase(&["paired", "with", "another", "creature"])
    {
        Some(ObjectStateKind::Paired)
    } else if ts.eat_phrase(&["face", "down"]) || ts.eat_word("face-down") {
        Some(ObjectStateKind::FaceDown)
    } else {
        None
    }
}
