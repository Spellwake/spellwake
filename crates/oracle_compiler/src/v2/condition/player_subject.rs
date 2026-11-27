//! Player-headed and subject-headed [`ConditionExpr`] predicates for `v2`.

use crate::ast::{CompareOp, ControllerRelation, PlayerStatKind, StatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, EventCheckKind, FilterConditionKind, ObjectFilter,
    ObjectStateKind, PlayerConditionKind, PlayerRef, PriorActionKind, SharedProperty,
    SubjectConditionKind, SubjectRef, TurnEventKind, ValueExpr, ZoneKind, ZoneMetricKind,
    ZoneOwner, ZoneRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

use super::{
    at_cond_boundary, cmp_cond, eat_optional_counter_kind, eat_prefix_cmp_op, eat_stat_kind,
    eat_suffix_cmp_op, eat_table, not_cond, obj_stat_val, player_check, player_stat_val,
    to_min_u16,
};

pub(super) fn parse_player_or_subject_condition(
    ts: &mut TokenStream<'_>,
) -> PResult<ConditionExpr> {
    // Player-headed conditions (`you control ...`, `you've gained life this turn`, `an opponent has cast a ... spell this turn`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_any_word(&["control", "controls"]) {
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
                    if let Ok(filter) = try_parse(ts, parse_object_filter) {
                        return Ok(not_cond(ConditionExpr::Controls {
                            player,
                            filter: Box::new(filter),
                            min_count: 1,
                        }));
                    }
                    ts.restore(cp);
                } else {
                    let _ = ts.eat_any_phrase(&[&["at", "least"], &["exactly"]]);
                    let (min_count, is_fewer) = if let Some(n) = ts.next_number(false) {
                        let fewer = ts.eat_phrase(&["or", "fewer"]);
                        if !fewer {
                            let _ = ts.eat_phrase(&["or", "more"]);
                        }
                        (to_min_u16(n), fewer)
                    } else {
                        let _ = ts.eat_any_word(&["a", "an"]);
                        (1, false)
                    };
                    if let Ok(filter) = try_parse(ts, parse_object_filter) {
                        let _ = ts.eat_phrase(&["that", "spell", "could", "target"]);
                        {
                            let gcp = ts.checkpoint();
                            if ts.eat_word("with")
                                && ts.eat_any_word(&["greater", "lesser"])
                                && eat_stat_kind(ts).is_some()
                                && ts.eat_word("than")
                                && try_parse(ts, parse_subject_ref).is_ok()
                            {
                                // Comparative stat qualifier consumed.
                            } else {
                                ts.restore(gcp);
                            }
                        }
                        // Support `you control a permanent of each color`
                        if ts.eat_phrase(&["of", "each", "color"]) {
                            return Ok(ConditionExpr::FilterCheck {
                                filter: Box::new(filter),
                                kind: FilterConditionKind::ColorsAmong,
                                value: 5,
                            });
                        }
                        let first_cond = if is_fewer {
                            not_cond(ConditionExpr::Controls {
                                player,
                                filter: Box::new(filter),
                                min_count: min_count.saturating_add(1),
                            })
                        } else {
                            ConditionExpr::Controls {
                                player,
                                filter: Box::new(filter),
                                min_count,
                            }
                        };
                        // Support `you control <flt1> and <flt2>` or `you control <flt1> or <flt2>`
                        let acp = ts.checkpoint();
                        if ts.eat_word("and") {
                            let min2 = if let Some(n2) = ts.next_number(false) {
                                let _ = ts.eat_phrase(&["or", "more"]);
                                to_min_u16(n2)
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
                        if ts.eat_word("or")
                            && let Ok(flt2) = try_parse(ts, parse_object_filter)
                            && at_cond_boundary(ts)
                        {
                            return Ok(ConditionExpr::Or(
                                Box::new(first_cond),
                                Box::new(ConditionExpr::Controls {
                                    player,
                                    filter: Box::new(flt2),
                                    min_count: 1,
                                }),
                            ));
                        }
                        ts.restore(acp);
                        return Ok(first_cond);
                    }
                    if !is_fewer
                        && min_count == 1
                        && let Ok(subj) = try_parse(ts, parse_subject_ref)
                    {
                        return Ok(ConditionExpr::ControlsSubject {
                            player,
                            subject: Box::new(subj),
                        });
                    }
                }
            }
            if ts.eat_any_word(&["has", "have"]) {
                if ts.eat_phrase(&["more", "life", "than"])
                    && let Ok(other) = try_parse(ts, parse_player_ref)
                {
                    return Ok(cmp_cond(
                        player_stat_val(player, PlayerStatKind::LifeTotal),
                        CompareOp::Gt,
                        player_stat_val(other, PlayerStatKind::LifeTotal),
                    ));
                }
                if ts.eat_any_phrase(&[
                    &["no", "cards", "in", "hand"],
                    &["no", "cards", "in", "their", "hand"],
                    &["no", "cards", "in", "your", "hand"],
                ]) {
                    return Ok(cmp_cond(
                        player_stat_val(player, PlayerStatKind::CardsInHand),
                        CompareOp::Eq,
                        ValueExpr::Const(0),
                    ));
                }
                let _ = ts.eat_word("exactly");
                if let Some(n) = ts.next_number(false) {
                    let op = eat_suffix_cmp_op(ts, true, true);
                    if ts.eat_any_phrase(&[
                        &["cards", "in", "their", "graveyard"],
                        &["cards", "in", "your", "graveyard"],
                        &["card", "in", "their", "graveyard"],
                    ]) {
                        return Ok(ConditionExpr::ZoneHasCount {
                            filter: Box::new(ObjectFilter {
                                any_card: true,
                                ..Default::default()
                            }),
                            zone: ZoneRef {
                                zone: ZoneKind::Graveyard,
                                owner: if player == PlayerRef::You {
                                    ZoneOwner::You
                                } else {
                                    ZoneOwner::ThatPlayer
                                },
                            },
                            min_count: to_min_u16(n),
                        });
                    }
                    if ts.eat_any_phrase(&[
                        &["cards", "in", "hand"],
                        &["card", "in", "hand"],
                        &["cards", "in", "their", "hand"],
                        &["cards", "in", "your", "hand"],
                    ]) {
                        return Ok(cmp_cond(
                            player_stat_val(player, PlayerStatKind::CardsInHand),
                            op,
                            ValueExpr::Const(n),
                        ));
                    }
                    if ts.eat_any_word(&["opponents", "opponent"]) {
                        return Ok(cmp_cond(ValueExpr::OpponentsCount, op, ValueExpr::Const(n)));
                    }
                    if ts.eat_word("life") {
                        return Ok(cmp_cond(
                            player_stat_val(player, PlayerStatKind::LifeTotal),
                            op,
                            ValueExpr::Const(n),
                        ));
                    }
                    {
                        let fcp = ts.checkpoint();
                        if let Ok(mut flt) = try_parse(ts, parse_object_filter)
                            && let Some(zone) = flt.zones.pop()
                        {
                            return Ok(ConditionExpr::ZoneHasCount {
                                filter: Box::new(flt),
                                zone,
                                min_count: to_min_u16(n),
                            });
                        }
                        ts.restore(fcp);
                    }
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && ts.eat_any_word(&["counters", "counter"])
                    {
                        return Ok(ConditionExpr::PlayerHasCounters {
                            player,
                            kind,
                            min_count: to_min_u16(n),
                        });
                    }
                }
            }
            if ts.eat_any_word(&["own", "owns"]) {
                let min_count = if let Some(n) = ts.next_number(false) {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    to_min_u16(n)
                } else {
                    1
                };
                if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                    let zone = flt.zones.pop().unwrap_or(ZoneRef {
                        zone: ZoneKind::Exile,
                        owner: ZoneOwner::Default,
                    });
                    return Ok(ConditionExpr::ZoneHasCount {
                        filter: Box::new(flt),
                        zone,
                        min_count,
                    });
                }
            }
            if ts.eat_any_phrase(&[&["don't", "have"], &["doesn't", "have"]]) {
                let hcp = ts.checkpoint();
                let _ = ts.eat_any_word(&["any", "a", "an"]);
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && ts.eat_any_word(&["counters", "counter"])
                {
                    return Ok(not_cond(ConditionExpr::PlayerHasCounters {
                        player,
                        kind,
                        min_count: 1,
                    }));
                }
                ts.restore(hcp);
            }
            if ts.eat_any_phrase(&[&["don't", "control"], &["doesn't", "control"]]) {
                let _ = ts.eat_any_word(&["a", "an"]);
                if let Ok(filter) = try_parse(ts, parse_object_filter) {
                    return Ok(not_cond(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: 1,
                    }));
                }
                if let Ok(subj) = try_parse(ts, parse_subject_ref) {
                    return Ok(not_cond(ConditionExpr::ControlsSubject {
                        player,
                        subject: Box::new(subj),
                    }));
                }
            }
            if ts.eat_any_phrase(&[&["has", "cast"], &["cast"]]) {
                let min_count = if let Some(n) = ts.next_number(false) {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    to_min_u16(n)
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
                && ts.eat_any_phrase(&[&["or", "greater"], &["or", "more"]])
            {
                let _ = ts.eat_any_phrase(&[&["this", "combat"], &["this", "turn"]]);
                return Ok(player_check(
                    player,
                    PlayerConditionKind::AttackedWithTotalPower { min_power: n },
                ));
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
                return Ok(player_check(player, PlayerConditionKind::LostLifeLastTurn));
            }
            if ts.eat_phrase(&["didn't", "cast", "a", "spell", "this", "turn"]) {
                return Ok(not_cond(ConditionExpr::PlayerTurnEvent {
                    player,
                    event: TurnEventKind::SpellCast,
                    filter: None,
                    min_count: 1,
                }));
            }
            if ts.eat_any_phrase(&[
                &["didn't", "cast", "it", "from", "your", "hand"],
                &["didn't", "cast", "it", "from", "their", "hand"],
            ]) {
                return Ok(not_cond(ConditionExpr::Flag(
                    ConditionFlag::WasCastFromHand,
                )));
            }
            if ts.eat_word("controlled")
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        controller: Some(ControllerRelation::You),
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&["attacked", "with"])
                && let Some(n) = ts.next_number(false)
            {
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_any_word(&["creatures", "creature"]) && ts.eat_phrase(&["this", "turn"]) {
                    return Ok(player_check(
                        player,
                        PlayerConditionKind::AttackedWithCount {
                            min_count: to_min_u16(n),
                        },
                    ));
                }
            }
            if ts.eat_any_phrase(&[&["was", "dealt"], &["were", "dealt"]]) {
                let combat_only = ts.eat_word("combat");
                let _ = ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    if ts.eat_word("by") {
                        let _ = opt(ts, parse_subject_ref);
                    }
                    if ts.eat_phrase(&["this", "turn"]) {
                        return Ok(player_check(
                            player,
                            PlayerConditionKind::DealtDamageThisTurn { combat_only },
                        ));
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
                    to_min_u16(n)
                } else {
                    1
                };
                if let Ok(flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_any_phrase(&[
                        &[
                            "enter",
                            "the",
                            "battlefield",
                            "under",
                            "your",
                            "control",
                            "this",
                            "turn",
                        ],
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
                        &["enter", "under", "your", "control", "this", "turn"],
                        &["enter", "under", "their", "control", "this", "turn"],
                        &["enter", "the", "battlefield", "this", "turn"],
                    ])
                {
                    return Ok(ConditionExpr::PermanentEnteredThisTurn {
                        filter: Box::new(flt),
                        controller: player,
                        min_count,
                    });
                }
            }
            if ts.eat_any_phrase(&[&["gained"], &["has", "gained"]]) {
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
            if ts.eat_any_phrase(&[&["discarded"], &["has", "discarded"]]) {
                let min_count = if ts.eat_word("a") {
                    1
                } else {
                    ts.next_number(false)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(1)
                };
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_any_word(&["card", "cards"]) && ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::CardDiscarded,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_any_phrase(&[&["lost"], &["has", "lost"]]) {
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
        if ts.eat_any_phrase(&[&["you've", "drawn"], &["you", "drew"]]) {
            let more_than = ts.eat_phrase(&["more", "than"]);
            if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_any_word(&["cards", "card"]) && ts.eat_phrase(&["this", "turn"]) {
                    let min_val = if more_than { n.saturating_add(1) } else { n };
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player: PlayerRef::You,
                        event: TurnEventKind::CardDrawn,
                        filter: None,
                        min_count: to_min_u16(min_val),
                    });
                }
            }
        }
        ts.restore(dcp);
    }

    // `<subj> is in the command zone or on the battlefield`
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
    if ts.eat_phrase(&["it", "isn't", "a", "mana", "ability"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsNotManaAbility));
    }
    // `there are no <flt> [on the battlefield | in <zone>]`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            let zone = flt.zones.pop().unwrap_or(ZoneRef {
                zone: ZoneKind::Battlefield,
                owner: ZoneOwner::Default,
            });
            return Ok(not_cond(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone,
                min_count: 1,
            }));
        }
        ts.restore(ncp);
    }
    // `two or more <flt> are tied for greatest/least <stat>`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["two", "or", "more"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["are", "tied", "for"])
        {
            let _ = ts.eat_word("the");
            let greatest = if ts.eat_any_word(&["greatest", "highest"]) {
                Some(true)
            } else if ts.eat_any_word(&["least", "lowest"]) {
                Some(false)
            } else {
                None
            };
            if let (Some(greatest), Some(stat)) = (greatest, eat_stat_kind(ts)) {
                return Ok(ConditionExpr::EventCheck(
                    EventCheckKind::TiedForExtremeStat { stat, greatest },
                ));
            }
        }
        ts.restore(tcp);
    }
    // `<flt> is/are put into a graveyard this way`
    {
        let gcp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_phrase(&[
                &["is", "put", "into", "a", "graveyard", "this", "way"],
                &["are", "put", "into", "a", "graveyard", "this", "way"],
                &["is", "put", "into", "your", "graveyard", "this", "way"],
            ])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::MilledThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(gcp);
    }
    // `no opponent controls <flt>`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["no", "opponent", "controls"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(not_cond(ConditionExpr::Controls {
                player: PlayerRef::AnyOpponent,
                filter: Box::new(flt),
                min_count: 1,
            }));
        }
        ts.restore(ncp);
    }
    // `a library has <N> or fewer cards in it`
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "library", "has"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "fewer", "cards", "in", "it"])
        {
            return Ok(cmp_cond(
                player_stat_val(PlayerRef::You, PlayerStatKind::CardsInLibrary),
                CompareOp::Le,
                ValueExpr::Const(i32::from(u16::try_from(n.max(0)).unwrap_or(0))),
            ));
        }
        ts.restore(lcp);
    }
    // `<subj> had counters on it` / `<subj> has [a|an|<kind>] counters on it` / `<subj> doesn't have a <kind> counter on it`
    // / `<subj> has the chosen name` / `<subj> dealt [combat] damage [to <tgt>] this turn` / `<subj> hasn't/haven't dealt [combat] damage yet`
    // / `<subj> attacked [or blocked] this combat` / `<subj> didn't enter the battlefield this turn` / `<subj> isn't prepared` / `<subj> was[n't] cast`
    {
        let scp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_any_phrase(&[
                &["had", "counters", "on", "it"],
                &["had", "counters", "on", "them"],
            ]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HadCountersOnIt,
                });
            }
            if ts.eat_any_phrase(&[
                &["has", "counters", "on", "it"],
                &["have", "counters", "on", "them"],
            ]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasCounters(None),
                });
            }
            {
                let hcp = ts.checkpoint();
                let neg_cnt = ts.eat_any_phrase(&[&["doesn't", "have"], &["don't", "have"]]);
                let pos_cnt = !neg_cnt && ts.eat_any_word(&["has", "have"]);
                if (neg_cnt || pos_cnt)
                    && {
                        let _ = ts.eat_any_word(&["a", "an"]);
                        true
                    }
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && ts.eat_any_word(&["counter", "counters"])
                    && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
                {
                    let base = ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::HasCounters(Some(ck)),
                    };
                    return Ok(if neg_cnt { not_cond(base) } else { base });
                }
                ts.restore(hcp);
            }
            if ts.eat_phrase(&["has", "the", "chosen", "name"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasChosenName,
                });
            }
            {
                let neg_cast = ts.eat_phrase(&["wasn't", "cast"]);
                let pos_cast = !neg_cast && ts.eat_phrase(&["was", "cast"]);
                if (neg_cast || pos_cast)
                    && !ts.peek_word().is_some_and(|w| {
                        w.eq_ignore_ascii_case("from")
                            || w.eq_ignore_ascii_case("for")
                            || w.eq_ignore_ascii_case("using")
                    })
                {
                    let base = ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::WasCast,
                    };
                    return Ok(if neg_cast { not_cond(base) } else { base });
                }
            }
            if ts.eat_phrase(&["isn't", "prepared"]) {
                return Ok(not_cond(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Modified,
                }));
            }
            if ts.eat_any_phrase(&[
                &["didn't", "enter", "the", "battlefield", "this", "turn"],
                &["didn't", "enter", "this", "turn"],
            ]) {
                return Ok(not_cond(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::EnteredThisTurn,
                }));
            }
            if ts.eat_phrase(&["attacked", "or", "blocked", "this", "combat"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::AttackedThisCombat { or_blocked: true },
                });
            }
            if ts.eat_any_phrase(&[
                &["attacked", "this", "combat"],
                &["attacked", "this", "turn"],
            ]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::AttackedThisCombat { or_blocked: false },
                });
            }
            if ts.eat_any_phrase(&[
                &["hasn't", "dealt", "combat", "damage", "yet"],
                &["haven't", "dealt", "combat", "damage", "yet"],
            ]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::HasntDealtDamageYet { combat_only: true },
                });
            }
            if ts.eat_any_phrase(&[
                &["hasn't", "dealt", "damage", "yet"],
                &["haven't", "dealt", "damage", "yet"],
            ]) {
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
        return Ok(player_check(
            PlayerRef::You,
            PlayerConditionKind::CommittedCrimeThisTurn,
        ));
    }
    if ts.eat_any_phrase(&[
        &["you've", "discarded", "a", "card", "this", "turn"],
        &[
            "you've",
            "discarded",
            "one",
            "or",
            "more",
            "cards",
            "this",
            "turn",
        ],
    ]) {
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
            && ts.eat_any_phrase(&[
                &[
                    "entered",
                    "the",
                    "battlefield",
                    "under",
                    "your",
                    "control",
                    "this",
                    "turn",
                ],
                &["entered", "under", "your", "control", "this", "turn"],
                &["entered", "the", "battlefield", "this", "turn"],
            ])
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
        if ts.eat_any_phrase(&[
            &["a", "nonland", "permanent", "left", "the", "battlefield"],
            &["a", "permanent", "left", "the", "battlefield"],
        ]) && {
            let _ = ts.eat_phrase(&["under", "your", "control"]);
            ts.eat_phrase(&["this", "turn"])
        } {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentLeftThisTurn));
        }
        ts.restore(lcp);
        if ts.eat_any_word(&["a", "an"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_any_phrase(&[
                &[
                    "left",
                    "the",
                    "battlefield",
                    "under",
                    "your",
                    "control",
                    "this",
                    "turn",
                ],
                &["left", "the", "battlefield", "this", "turn"],
            ])
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
                to_min_u16(n)
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
            if let Some(op) = eat_prefix_cmp_op(ts, true, false)
                && let Ok(rhs) = try_parse(ts, parse_value_expr)
            {
                return Ok(cmp_cond(lhs, op, rhs));
            }
            // Suffix form: `<val1> is <val2> or greater / or less`
            if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                let suffix_op = eat_suffix_cmp_op(ts, true, false);
                return Ok(cmp_cond(lhs, suffix_op, rhs));
            }
        }
        ts.restore(cp);
    }

    // `a card is exiled with it/~` / `one or more cards are exiled with it/~`
    {
        let ecp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["a", "card", "is", "exiled", "with"],
            &["one", "or", "more", "cards", "are", "exiled", "with"],
        ]) && (ts.eat_word("it") || ts.eat_tilde())
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
                return Ok(not_cond(ConditionExpr::TargetsFilter {
                    subject: Box::new(subj),
                    filter: Box::new(filter),
                }));
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
            if ts.eat_any_phrase(&[&["entered", "the", "battlefield"], &["entered"]]) {
                let under_ctrl = ts.eat_any_phrase(&[
                    &["under", "your", "control"],
                    &["under", "their", "control"],
                    &["under", "an", "opponent's", "control"],
                ]);
                if ts.eat_phrase(&["this", "turn"]) || under_ctrl {
                    return Ok(ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::EnteredThisTurn,
                    });
                }
            }
            if ts.eat_any_phrase(&[
                &["is", "on", "the", "battlefield"],
                &["are", "on", "the", "battlefield"],
                &["remains", "on", "the", "battlefield"],
            ]) {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone: ZoneRef {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::Default,
                    },
                });
            }
            if ts.eat_any_phrase(&[
                &["remains", "attached", "to"],
                &["is", "attached", "to"],
                &["was", "attached", "to"],
            ]) {
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
            if ts.eat_any_phrase(&[&["remains", "exiled"], &["remain", "exiled"]]) {
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
                // Optional graveyard-order suffix
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
                            zone: ZoneRef {
                                zone: ZoneKind::Battlefield,
                                owner: ZoneOwner::Default,
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
            if ts.eat_any_phrase(&[
                &["doesn't", "have"],
                &["don't", "have"],
                &["didn't", "have"],
            ]) {
                if let Ok(kw) = try_parse(ts, parse_keyword_kind) {
                    return Ok(not_cond(ConditionExpr::HasKeyword {
                        subject: Box::new(subj),
                        keyword: kw,
                    }));
                }
                if ts.next_quoted().is_some() {
                    return Ok(not_cond(ConditionExpr::SubjectCheck {
                        subject: Box::new(subj),
                        kind: SubjectConditionKind::HasChosenName,
                    }));
                }
            }
            if ts.eat_any_word(&["has", "have", "had"]) {
                // `<subj> has the same mana value / power / toughness / color / name / card type as <other>`
                {
                    let smcp = ts.checkpoint();
                    const SAME_PROPS: &[(&[&str], SharedProperty)] = &[
                        (&["mana", "value"], SharedProperty::ManaValue),
                        (&["power"], SharedProperty::Power),
                        (&["toughness"], SharedProperty::Toughness),
                        (&["color"], SharedProperty::Color),
                        (&["name"], SharedProperty::Name),
                        (&["card", "type"], SharedProperty::CardType),
                    ];
                    if ts.eat_phrase(&["the", "same"])
                        && let Some(kind) = eat_table(ts, SAME_PROPS)
                        && ts.eat_word("as")
                        && let Ok(other) = try_parse(ts, parse_subject_ref)
                    {
                        return Ok(ConditionExpr::SharesTypeWith {
                            subject: Box::new(subj),
                            other: Box::new(other),
                            kind,
                        });
                    }
                    ts.restore(smcp);
                }
                // `<subj> had/has no [<kind>] counters on it`
                {
                    let ncp = ts.checkpoint();
                    if ts.eat_word("no")
                        && let Some(kind) = eat_optional_counter_kind(ts)
                    {
                        let _ = ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]]);
                        return Ok(cmp_cond(
                            ValueExpr::CounterCount {
                                subject: Box::new(subj),
                                kind,
                            },
                            CompareOp::Eq,
                            ValueExpr::Const(0),
                        ));
                    }
                    ts.restore(ncp);
                }
                // `<subj> has mana value/power/toughness <cmp> <val>`
                {
                    let scp = ts.checkpoint();
                    if let Some(stat) = eat_stat_kind(ts) {
                        const SIMPLE_PREFIX_OPS: &[(&[&str], CompareOp)] = &[
                            (&["less", "than", "or", "equal", "to"], CompareOp::Le),
                            (&["greater", "than", "or", "equal", "to"], CompareOp::Ge),
                            (&["less", "than"], CompareOp::Lt),
                            (&["greater", "than"], CompareOp::Gt),
                        ];
                        if let Some(op) = eat_table(ts, SIMPLE_PREFIX_OPS)
                            && let Ok(rhs) = try_parse(ts, parse_value_expr)
                        {
                            return Ok(cmp_cond(obj_stat_val(subj, stat), op, rhs));
                        }
                        if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                            let op = eat_suffix_cmp_op(ts, true, false);
                            return Ok(cmp_cond(obj_stat_val(subj, stat), op, rhs));
                        }
                    }
                    ts.restore(scp);
                }
                // `<subj> has dealt <N> or more damage [to <tgt>] this turn`
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
                                kind: SubjectConditionKind::DamageDealtThisTurn {
                                    min_damage: to_min_u16(min_d),
                                },
                            });
                        }
                    }
                    ts.restore(dcp);
                }
                // `<subj> has [greater power [or toughness] than <other>]`
                {
                    let gcp = ts.checkpoint();
                    if ts.eat_word("greater") {
                        let stat = if ts.eat_any_phrase(&[
                            &["power", "or", "toughness"],
                            &["power", "and/or", "toughness"],
                        ]) {
                            None
                        } else if ts.eat_word("power") {
                            Some(StatKind::Power)
                        } else if ts.eat_word("toughness") {
                            Some(StatKind::Toughness)
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
                // `<subj> has [fewer than | at least | exactly | <a/an/N> [or more/less]] <kind> counter(s) on it`
                let hcp = ts.checkpoint();
                const CNT_PREFIX_OPS: &[(&[&str], CompareOp)] = &[
                    (&["fewer", "than"], CompareOp::Lt),
                    (&["at", "least"], CompareOp::Ge),
                    (&["exactly"], CompareOp::Eq),
                ];
                let prefix_op = eat_table(ts, CNT_PREFIX_OPS);
                if let Some(n) = ts.next_number(prefix_op.is_none()) {
                    let op = if let Some(pop) = prefix_op {
                        pop
                    } else if ts.eat_phrase(&["or", "more"]) {
                        CompareOp::Ge
                    } else if ts.eat_any_phrase(&[&["or", "less"], &["or", "fewer"]]) {
                        CompareOp::Le
                    } else {
                        CompareOp::Eq
                    };
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && ts.eat_any_word(&["counters", "counter"])
                    {
                        let _ = ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]]);
                        return Ok(cmp_cond(
                            ValueExpr::CounterCount {
                                subject: Box::new(subj),
                                kind,
                            },
                            op,
                            ValueExpr::Const(n),
                        ));
                    }
                }
                ts.restore(hcp);
                if let Ok(kw) = try_parse(ts, parse_keyword_kind) {
                    return Ok(ConditionExpr::HasKeyword {
                        subject: Box::new(subj),
                        keyword: kw,
                    });
                }
            }
            if ts.eat_phrase(&["was", "cast", "from"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::CastFromZone {
                    subject: Box::new(subj),
                    zone,
                });
            }
            let negated = ts.eat_any_phrase(&[
                &["isn't"],
                &["is", "not"],
                &["aren't"],
                &["are", "not"],
                &["wasn't"],
                &["was", "not"],
                &["weren't"],
                &["were", "not"],
            ]);
            let positive = !negated && ts.eat_any_word(&["is", "are", "was", "were"]);
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
                                        min_count: to_min_u16(n),
                                    };
                                    return Ok(if negated { not_cond(base) } else { base });
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
                    return Ok(if negated { not_cond(base) } else { base });
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
                    return Ok(if negated { not_cond(base) } else { base });
                }
            }
        }
        ts.restore(cp);
    }

    // `it's/they're [not] ...`
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
                if ts.eat_phrase(&["attached", "to"])
                    && let Ok(target) = try_parse(ts, parse_object_filter)
                {
                    let base = ConditionExpr::SubjectAttachedTo {
                        subject: Box::new(SubjectRef::Anaphora(anaph)),
                        target: Box::new(target),
                    };
                    return Ok(if negated { not_cond(base) } else { base });
                }
                ts.restore(bcp);
                if ts.eat_phrase(&["controlled", "by"])
                    && let Ok(player) = try_parse(ts, parse_player_ref)
                {
                    let base = ConditionExpr::ControlsSubject {
                        player,
                        subject: Box::new(SubjectRef::Anaphora(anaph)),
                    };
                    return Ok(if negated { not_cond(base) } else { base });
                }
                ts.restore(bcp);
                if !negated && ts.eat_word("the") && ts.eat_any_word(&["first", "second", "third"])
                {
                    let _ = opt(ts, parse_object_filter);
                    if ts.eat_any_phrase(&[
                        &["you", "cast", "this", "turn"],
                        &["you've", "cast", "this", "turn"],
                        &["you've", "cast", "this", "game"],
                    ]) {
                        return Ok(ConditionExpr::Flag(ConditionFlag::FirstTimeEachTurn));
                    }
                }
                ts.restore(bcp);
                if !negated
                    && ts.eat_any_phrase(&[
                        &["blocking", "or", "blocked", "by"],
                        &["blocked", "by"],
                        &["blocking"],
                    ])
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
                if state == ObjectStateKind::Attacking {
                    let acp = ts.checkpoint();
                    if !(ts.eat_any_phrase(&[
                        &["a", "battle"],
                        &[
                            "its",
                            "owner",
                            "or",
                            "a",
                            "permanent",
                            "its",
                            "owner",
                            "controls",
                        ],
                    ]) || try_parse(ts, parse_player_ref).is_ok())
                    {
                        ts.restore(acp);
                    }
                }
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
                return Ok(if negated { not_cond(base) } else { base });
            }
            if let Ok(flt) = try_parse(ts, parse_object_filter) {
                let base = ConditionExpr::SubjectMatches {
                    subject: Box::new(SubjectRef::Anaphora(anaph)),
                    filter: Box::new(flt),
                };
                return Ok(if negated { not_cond(base) } else { base });
            }
        }
        ts.restore(it_cp);
    }

    super::extended::parse_extended_condition(ts)
}

fn parse_object_state(ts: &mut TokenStream<'_>) -> Option<ObjectStateKind> {
    const OBJECT_STATES: &[(&[&str], ObjectStateKind)] = &[
        (&["tapped"], ObjectStateKind::Tapped),
        (&["untapped"], ObjectStateKind::Untapped),
        (&["attacking", "alone"], ObjectStateKind::AttackingAlone),
        (&["attacking"], ObjectStateKind::Attacking),
        (&["blocking"], ObjectStateKind::Blocking),
        (&["blocked"], ObjectStateKind::Blocked),
        (&["unblocked"], ObjectStateKind::Unblocked),
        (&["equipped"], ObjectStateKind::Equipped),
        (&["enchanted"], ObjectStateKind::Enchanted),
        (&["modified"], ObjectStateKind::Modified),
        (&["saddled"], ObjectStateKind::Saddled),
        (&["monstrous"], ObjectStateKind::Monstrous),
        (
            &["paired", "with", "a", "creature"],
            ObjectStateKind::Paired,
        ),
        (
            &["paired", "with", "another", "creature"],
            ObjectStateKind::Paired,
        ),
        (&["face", "down"], ObjectStateKind::FaceDown),
        (&["face-down"], ObjectStateKind::FaceDown),
    ];
    eat_table(ts, OBJECT_STATES)
}
