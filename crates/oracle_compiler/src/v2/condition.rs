//! Strictly-typed recursive [`ConditionExpr`] parser for `v2`.

use crate::ast::CompareOp;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ObjectStateKind, PlayerRef, PriorActionKind, SubjectRef,
    TurnEventKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed boolean [`ConditionExpr`].
pub fn parse_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let mut cond = parse_condition_atom(ts)?;
    loop {
        let cp = ts.checkpoint();
        if ts.eat_word("and") {
            if let Ok(rhs) = try_parse(ts, parse_condition_atom) {
                cond = ConditionExpr::And(Box::new(cond), Box::new(rhs));
                continue;
            }
            ts.restore(cp);
            break;
        }
        if ts.eat_word("or") {
            if let Ok(rhs) = try_parse(ts, parse_condition_atom) {
                cond = ConditionExpr::Or(Box::new(cond), Box::new(rhs));
                continue;
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
    if ts.eat_phrase(&[
        "it's", "the", "first", "combat", "phase", "of", "the", "turn",
    ]) || ts.eat_phrase(&[
        "it's", "the", "first", "combat", "phase", "of", "your", "turn",
    ]) {
        return Ok(ConditionExpr::IsFirstCombatPhase);
    }
    if ts.eat_phrase(&["it's", "your", "turn"]) {
        return Ok(ConditionExpr::IsYourTurn);
    }
    if ts.eat_phrase(&["it's", "not", "your", "turn"])
        || ts.eat_phrase(&["it's", "not", "their", "turn"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::IsYourTurn)));
    }
    if ts.eat_phrase(&["you", "have", "the", "city's", "blessing"]) {
        return Ok(ConditionExpr::HasCitysBlessing);
    }
    if ts.eat_phrase(&["you", "win", "the", "flip"]) || ts.eat_phrase(&["you", "win"]) {
        return Ok(ConditionExpr::WonCoinFlip);
    }
    if ts.eat_phrase(&["you", "lose", "the", "flip"]) || ts.eat_phrase(&["you", "lose"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::WonCoinFlip)));
    }
    if ts.eat_phrase(&["you", "search", "your", "library", "this", "way"])
        || ts.eat_phrase(&[
            "that", "player", "searches", "their", "library", "this", "way",
        ])
        || ts.eat_phrase(&["they", "search", "their", "library", "this", "way"])
    {
        return Ok(ConditionExpr::PriorObjectEvent(
            PriorActionKind::SearchedThisWay,
        ));
    }
    if ts.eat_phrase(&["no", "spells", "were", "cast", "last", "turn"]) {
        return Ok(ConditionExpr::SpellsCastLastTurn {
            op: CompareOp::Eq,
            count: 0,
        });
    }
    if ts.eat_phrase(&[
        "two", "or", "more", "spells", "were", "cast", "last", "turn",
    ]) {
        return Ok(ConditionExpr::SpellsCastLastTurn {
            op: CompareOp::Ge,
            count: 2,
        });
    }
    // `you don't put <subj> onto the battlefield / into your hand [this way]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "don't", "put"])
            && try_parse(ts, parse_subject_ref).is_ok()
            && let Ok((dest, _)) = try_parse(ts, crate::v2::filter::parse_zone_dest)
        {
            let _ = ts.eat_phrase(&["this", "way"]);
            return Ok(ConditionExpr::DidntPutInZone { zone: dest.zone });
        }
        ts.restore(cp);
    }
    // `<zone> has <N> or more cards in it` (`a graveyard has twenty or more cards in it`)
    {
        let cp = ts.checkpoint();
        if let Ok(zone) = try_parse(ts, parse_zone_ref)
            && ts.eat_word("has")
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "in", "it"])
        {
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(crate::v2::ir::ObjectFilter {
                    any_card: true,
                    ..Default::default()
                }),
                zone,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["the", "gift", "was", "promised"]) {
        return Ok(ConditionExpr::GiftWasPromised);
    }
    if ts.eat_phrase(&["it", "was", "bargained"])
        || ts.eat_phrase(&["this", "spell", "was", "bargained"])
    {
        return Ok(ConditionExpr::WasBargained);
    }
    if ts.eat_phrase(&["no", "colored", "mana", "was", "spent", "to", "cast", "it"])
        || ts.eat_phrase(&[
            "no", "colored", "mana", "was", "spent", "to", "cast", "this", "spell",
        ])
    {
        return Ok(ConditionExpr::ManaSpentNoColor);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "bargained"]) {
            return Ok(ConditionExpr::WasBargained);
        }
        ts.restore(cp);
    }
    // `a <flt> was/is milled/exiled/discarded/destroyed/revealed this way`
    {
        let cp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("was")
                || ts.eat_word("were")
                || ts.eat_word("is")
                || ts.eat_word("are"))
        {
            let act = if ts.eat_phrase(&["milled", "this", "way"]) {
                Some(PriorActionKind::MilledThisWay)
            } else if ts.eat_phrase(&["exiled", "this", "way"]) {
                Some(PriorActionKind::ExiledThisWay)
            } else if ts.eat_phrase(&["discarded", "this", "way"]) {
                Some(PriorActionKind::DiscardedThisWay)
            } else if ts.eat_phrase(&["destroyed", "this", "way"]) {
                Some(PriorActionKind::DestroyedThisWay)
            } else if ts.eat_phrase(&["revealed", "this", "way"]) {
                Some(PriorActionKind::RevealedThisWay)
            } else if ts.eat_phrase(&["sacrificed", "this", "way"]) {
                Some(PriorActionKind::SacrificedThisWay)
            } else if ts.eat_phrase(&[
                "put",
                "into",
                "a",
                "graveyard",
                "from",
                "the",
                "battlefield",
                "this",
                "turn",
            ]) {
                Some(PriorActionKind::DestroyedThisWay)
            } else {
                None
            };
            if let Some(action) = act {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["it", "was", "kicked"]) || ts.eat_phrase(&["that", "spell", "was", "kicked"])
    {
        let _ = ts.eat_word("twice");
        return Ok(ConditionExpr::WasKicked);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "cast", "it"])
            || (ts.eat_phrase(&["you", "cast"]) && ts.eat_tilde())
        {
            let _ = ts.eat_phrase(&["from", "your", "hand"])
                || ts.eat_phrase(&["during", "your", "main", "phase"]);
            return Ok(ConditionExpr::YouCastIt);
        }
        ts.restore(cp);
    }
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
    ]) || ts.eat_phrase(&[
        "a",
        "permanent",
        "you",
        "controlled",
        "left",
        "the",
        "battlefield",
        "this",
        "turn",
    ]) || ts.eat_phrase(&[
        "a",
        "permanent",
        "left",
        "the",
        "battlefield",
        "this",
        "turn",
    ]) {
        return Ok(ConditionExpr::PermanentLeftThisTurn);
    }
    if ts.eat_phrase(&["that", "creature", "dies", "this", "way"])
        || ts.eat_phrase(&["that", "creature", "dies", "this", "turn"])
    {
        return Ok(ConditionExpr::PriorObjectEvent(
            PriorActionKind::DestroyedThisWay,
        ));
    }
    if ts.eat_phrase(&["that", "spell", "is", "countered", "this", "way"]) {
        return Ok(ConditionExpr::PriorObjectEvent(
            PriorActionKind::ExiledThisWay,
        ));
    }
    if ts.eat_phrase(&["a", "creature", "died", "this", "turn"]) {
        return Ok(ConditionExpr::PermanentDiedThisTurn);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "kicked"]) {
            let _ = ts.eat_word("twice");
            return Ok(ConditionExpr::WasKicked);
        }
        ts.restore(cp);
    }

    // `there are [at least] <N> [or more] card types among cards in <zone>` (Delirium)
    // OR `there are [at least] <N> [or more] <flt> in <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are"]) {
            let _ = ts.eat_phrase(&["at", "least"]);
            let min_count = ts
                .next_number(false)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(1);
            let _ = ts.eat_phrase(&["or", "more"]);
            if ts.eat_phrase(&["card", "types", "among", "cards", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::DistinctCardTypesInZone {
                    zone,
                    min_count: u8::try_from(min_count).unwrap_or(4),
                });
            }
            // `there are <N> or more <kind> counters on <subj>`
            {
                let ccp = ts.checkpoint();
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                            subject: Box::new(subject),
                            kind,
                        }),
                        op: CompareOp::Ge,
                        rhs: Box::new(crate::v2::ir::ValueExpr::Const(i32::from(min_count))),
                    });
                }
                ts.restore(ccp);
            }
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
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
                        min_count,
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `your life total is <N> or less/more`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["your", "life", "total", "is"])
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            let op = if ts.eat_phrase(&["or", "less"]) {
                CompareOp::Le
            } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
                CompareOp::Ge
            } else {
                CompareOp::Eq
            };
            return Ok(ConditionExpr::LifeCompare {
                player: PlayerRef::You,
                op,
                value: Box::new(val),
            });
        }
        ts.restore(cp);
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

    // Player-headed conditions (`you control ...`, `you've gained life this turn`, `an opponent has cast a ... spell this turn`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("control") || ts.eat_word("controls") {
                if player == PlayerRef::You && ts.eat_tilde() {
                    return Ok(ConditionExpr::ControlsSelf);
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
                let filter = parse_object_filter(ts)?;
                if is_fewer {
                    return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: min_count.saturating_add(1),
                    })));
                }
                return Ok(ConditionExpr::Controls {
                    player,
                    filter: Box::new(filter),
                    min_count,
                });
            }
            if ts.eat_word("has") || ts.eat_word("have") {
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
                if let Some(n) = ts.next_number(false) {
                    let op = if ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "fewer"]) {
                        CompareOp::Le
                    } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
                        CompareOp::Ge
                    } else {
                        CompareOp::Eq
                    };
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
                    if ts.eat_word("life") {
                        return Ok(ConditionExpr::LifeCompare {
                            player,
                            op,
                            value: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
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
            if (ts.eat_word("own") || ts.eat_word("owns"))
                && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            {
                let zone = flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                    zone: crate::v2::ir::ZoneKind::Exile,
                    owner: crate::v2::ir::ZoneOwner::Default,
                });
                return Ok(ConditionExpr::ZoneHasCount {
                    filter: Box::new(flt),
                    zone,
                    min_count: 1,
                });
            }
            if ts.eat_phrase(&["don't", "control"]) || ts.eat_phrase(&["doesn't", "control"]) {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                let filter = parse_object_filter(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                    player,
                    filter: Box::new(filter),
                    min_count: 1,
                })));
            }
            if ts.eat_phrase(&["has", "cast"]) || ts.eat_word("cast") {
                let filter = opt(ts, parse_object_filter).map(Box::new);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::SpellCast,
                        filter,
                        min_count: 1,
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

    // `you've cast <flt> this turn`
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

    // General numeric value comparison (`<val1> is greater than / less than / less than or equal to / equal to <val2>`)
    {
        let cp = ts.checkpoint();
        if let Ok(lhs) = try_parse(ts, parse_value_expr)
            && ts.eat_word("is")
        {
            let op = if ts.eat_phrase(&["less", "than", "or", "equal", "to"]) {
                Some(CompareOp::Le)
            } else if ts.eat_phrase(&["greater", "than", "or", "equal", "to"]) {
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

    // Subject-headed conditions (`~ is in your graveyard`, `~ is untapped`, `<subj> has/doesn't have <kw>`, `it's a <filter>`)
    {
        let cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("targets")
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
                let _ = ts.eat_phrase(&["under", "your", "control"])
                    || ts.eat_phrase(&["under", "their", "control"])
                    || ts.eat_phrase(&["under", "an", "opponent's", "control"]);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::EnteredThisTurn(Box::new(subj)));
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
            if ts.eat_phrase(&["is", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone,
                });
            }
            if ts.eat_phrase(&["doesn't", "have"]) || ts.eat_phrase(&["don't", "have"]) {
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                })));
            }
            if ts.eat_word("has") || ts.eat_word("have") {
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
                // `<subj> has <N> or more <kind> counters on it`
                let hcp = ts.checkpoint();
                if let Some(n) = ts.next_number(false) {
                    let op = if ts.eat_phrase(&["or", "more"]) {
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
                    let base = ConditionExpr::SubjectState {
                        subject: Box::new(subj),
                        state,
                    };
                    return Ok(if negated {
                        ConditionExpr::Not(Box::new(base))
                    } else {
                        base
                    });
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    let base = ConditionExpr::SubjectMatches {
                        subject: Box::new(subj),
                        filter: Box::new(flt),
                    };
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

    // `it's a <filter>` / `it's tapped` / `it's untapped`
    if ts.eat_word("it's") {
        let negated = ts.eat_word("not");
        if let Some(state) = parse_object_state(ts) {
            let base = ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                state,
            };
            return Ok(if negated {
                ConditionExpr::Not(Box::new(base))
            } else {
                base
            });
        }
        if let Ok(flt) = try_parse(ts, parse_object_filter) {
            let base = ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                filter: Box::new(flt),
            };
            return Ok(if negated {
                ConditionExpr::Not(Box::new(base))
            } else {
                base
            });
        }
    }

    Err(ts.err("expected boolean condition expression"))
}

fn parse_object_state(ts: &mut TokenStream<'_>) -> Option<ObjectStateKind> {
    if ts.eat_word("tapped") {
        Some(ObjectStateKind::Tapped)
    } else if ts.eat_word("untapped") {
        Some(ObjectStateKind::Untapped)
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
