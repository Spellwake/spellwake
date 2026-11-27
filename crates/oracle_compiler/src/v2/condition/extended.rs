//! Extended compositional [`ConditionExpr`] grammar families for `v2`.

use crate::ast::{CardTypeMask, CombatState, CompareOp, PlayerStatKind, StatKind, TurnStepAst};
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_dest, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, CounterKind, EventCheckKind, FilterConditionKind,
    FilterFlags, FilterPredicate, ManaSymbol, ObjectFilter, PlayerConditionKind, PlayerRef,
    PriorActionKind, SharedProperty, SubjectConditionKind, SubjectRef, TurnEventKind, ValueExpr,
    ZoneKind, ZoneMetricKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse `there is / there are ...` existential and threshold conditions.
pub(super) fn parse_there_are_condition(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["there", "is", "no", "monarch"]) {
        return Some(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::EachPlayer,
            kind: PlayerConditionKind::IsMonarch,
        })));
    }
    if ts.eat_phrase(&[
        "there", "is", "an", "instant", "card", "and", "a", "sorcery", "card", "in",
    ]) && let Ok(zone) = try_parse(ts, parse_zone_ref)
    {
        return Some(ConditionExpr::And(
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
    ts.restore(cp);
    if ts.eat_phrase(&["there", "is", "exactly"]) || ts.eat_phrase(&["there", "are", "exactly"]) {
        if let Some(n) = ts.next_number(false) {
            let kcp = ts.checkpoint();
            let kind_opt = if ts.eat_word("counters") || ts.eat_word("counter") {
                Some(CounterKind::Any)
            } else if let Ok(k) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
            {
                Some(k)
            } else {
                None
            };
            if let Some(kind) = kind_opt
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Some(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::CounterCount {
                        subject: Box::new(subject),
                        kind,
                    }),
                    op: CompareOp::Eq,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
            ts.restore(kcp);
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                if flt.zones.is_empty()
                    && ts.eat_word("in")
                    && let Ok(zone) = try_parse(ts, parse_zone_ref)
                {
                    flt.zones.push(zone);
                }
                return Some(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                    op: CompareOp::Eq,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["there", "are", "fewer", "than"])
        && let Some(n) = ts.next_number(false)
        && let Ok(mut flt) = try_parse(ts, parse_object_filter)
    {
        if flt.zones.is_empty()
            && ts.eat_word("in")
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            flt.zones.push(zone);
        }
        return Some(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(flt))),
            op: CompareOp::Lt,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);
    if ts.eat_phrase(&["there", "are"]) {
        let _ = ts.eat_phrase(&["at", "least"]);
        let min_count = ts
            .next_number(false)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(1);
        let _ = ts.eat_phrase(&["or", "more"]);
        if (ts.eat_phrase(&["card", "types", "among", "cards", "exiled", "with"])
            || ts.eat_phrase(&["permanent", "types", "among", "cards", "exiled", "with"]))
            && (ts.eat_tilde() || ts.eat_word("it"))
        {
            return Some(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::DistinctCardTypesAmong(Box::new(ObjectFilter {
                    any_card: true,
                    flags: FilterFlags::EXILED_WITH_SELF,
                    ..Default::default()
                }))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
            });
        }
        if (ts.eat_phrase(&["card", "types", "among", "cards", "in"])
            || ts.eat_phrase(&["permanent", "types", "among", "cards", "in"]))
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Some(ConditionExpr::ZoneMetric {
                zone,
                kind: ZoneMetricKind::DistinctCardTypes,
                min_count: u16::from(u8::try_from(min_count).unwrap_or(4)),
            });
        }
        if ts.eat_phrase(&["unlocked", "doors", "among", "rooms", "you", "control"])
            || ts.eat_phrase(&[
                "different",
                "names",
                "among",
                "unlocked",
                "doors",
                "of",
                "rooms",
                "you",
                "control",
            ])
        {
            return Some(ConditionExpr::Controls {
                player: PlayerRef::You,
                filter: Box::new(ObjectFilter {
                    subtypes: vec!["Room".to_string()],
                    ..Default::default()
                }),
                min_count,
            });
        }
        if ts.eat_phrase(&["basic", "land", "types", "among", "lands"])
            && let Ok(player) = try_parse(ts, parse_player_ref)
            && (ts.eat_word("control") || ts.eat_word("controls"))
        {
            return Some(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::DomainCount(player)),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
            });
        }
        if ts.eat_phrase(&["mana", "values", "among", "cards", "in"])
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Some(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::DistinctManaValues(zone)),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
            });
        }
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["different", "kinds", "of", "counters", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Some(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::DistinctCounterKinds(Box::new(flt))),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
                });
            }
            ts.restore(dcp);
        }
        // `there are <N> or more [<kind>] counters (on <subj> | among <flt>)`
        {
            let ccp = ts.checkpoint();
            let kind_opt = if ts.eat_word("counters") || ts.eat_word("counter") {
                Some(CounterKind::Any)
            } else if let Ok(k) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
            {
                Some(k)
            } else {
                None
            };
            if let Some(kind) = kind_opt {
                if ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Some(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::CounterCount {
                            subject: Box::new(subject),
                            kind,
                        }),
                        op: CompareOp::Ge,
                        rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
                    });
                }
                if ts.eat_word("among")
                    && let Ok(flt) = try_parse(ts, parse_object_filter)
                {
                    return Some(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::CounterCount {
                            subject: Box::new(SubjectRef::All(Box::new(flt))),
                            kind,
                        }),
                        op: CompareOp::Ge,
                        rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
                    });
                }
            }
            ts.restore(ccp);
        }
        if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
            let _ = ts.eat_word("total");
            let zone = if let Some(z) = flt.zones.pop() {
                Some(z)
            } else if ts.eat_word("in") {
                try_parse(ts, parse_zone_ref).ok()
            } else {
                None
            };
            if let Some(zone) = zone {
                return Some(ConditionExpr::ZoneHasCount {
                    filter: Box::new(flt),
                    zone,
                    min_count,
                });
            }
            return Some(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(i32::from(min_count))),
            });
        }
    }
    ts.restore(cp);
    None
}

/// Parse extended compositional condition families (called as fallback after core/player/subject rules).
pub(super) fn parse_extended_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // Turn / phase / step checks
    if ts.eat_phrase(&["it", "isn't", "that", "player's", "turn"])
        || ts.eat_phrase(&["it's", "not", "that", "player's", "turn"])
        || ts.eat_phrase(&["it", "isn't", "your", "turn"])
        || ts.eat_phrase(&["it's", "an", "opponent's", "turn"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::IsYourTurn,
        ))));
    }
    if ts.eat_phrase(&["it's", "your", "main", "phase"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
            TurnStepAst::MainPhase,
        )));
    }
    if ts.eat_phrase(&["it", "isn't", "your", "main", "phase"])
        || ts.eat_phrase(&["it's", "not", "your", "main", "phase"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::EventCheck(
            EventCheckKind::DuringStep(TurnStepAst::MainPhase),
        ))));
    }
    if ts.eat_phrase(&["it's", "your", "combat", "phase"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
            TurnStepAst::BeginningOfCombat,
        )));
    }
    if ts.eat_phrase(&["it's", "the", "first", "end", "step", "of", "the", "turn"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
            TurnStepAst::EndStep,
        )));
    }

    // Meld ownership + control check: `you both own and control ~ and <flt>`
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "both", "own", "and", "control"])
            && ts.eat_tilde()
            && ts.eat_word("and")
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            flt.predicates
                .push(FilterPredicate::OwnedAndControlledByYou);
            return Ok(ConditionExpr::And(
                Box::new(ConditionExpr::Flag(ConditionFlag::ControlsSelf)),
                Box::new(ConditionExpr::Controls {
                    player: PlayerRef::You,
                    filter: Box::new(flt),
                    min_count: 1,
                }),
            ));
        }
        ts.restore(mcp);
    }

    // Cards exiled with source: `a card is exiled with it/~` / `one or more cards are exiled with it/~`
    // / `<N> or more cards have been exiled with ~`
    {
        let ecp = ts.checkpoint();
        if (ts.eat_phrase(&["a", "card", "is", "exiled", "with"])
            || ts.eat_phrase(&["one", "or", "more", "cards", "are", "exiled", "with"]))
            && (ts.eat_word("it") || ts.eat_tilde())
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::CardsExiledWithSelf));
        }
        ts.restore(ecp);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "have", "been", "exiled", "with"])
            && (ts.eat_tilde() || ts.eat_word("it"))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                    any_card: true,
                    flags: FilterFlags::EXILED_WITH_SELF,
                    ..Default::default()
                }))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(ecp);
    }

    // Life total vs starting life total
    if ts.eat_phrase(&[
        "your", "life", "total", "is", "less", "than", "your", "starting", "life", "total",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::LifeAboveStarting { margin: 0 },
        })));
    }
    if ts.eat_phrase(&[
        "your", "life", "total", "is", "greater", "than", "or", "equal", "to", "your", "starting",
        "life", "total",
    ]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::LifeAboveStarting { margin: 0 },
        });
    }
    if ts.eat_phrase(&[
        "an",
        "opponent's",
        "life",
        "total",
        "is",
        "less",
        "than",
        "half",
        "their",
        "starting",
        "life",
        "total",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::AnyOpponent,
            kind: PlayerConditionKind::LifeAboveStarting { margin: 0 },
        })));
    }

    // Monarch & poisoned checks across any player reference
    if ts.eat_phrase(&["you're", "not", "the", "monarch"])
        || ts.eat_phrase(&["you", "aren't", "the", "monarch"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::IsMonarch,
        })));
    }
    if ts.eat_phrase(&[
        "you", "were", "the", "monarch", "as", "the", "turn", "began",
    ]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::IsMonarch,
        });
    }
    {
        let pcp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_phrase(&["is", "the", "monarch"]) || ts.eat_phrase(&["are", "the", "monarch"])
            {
                return Ok(ConditionExpr::PlayerCheck {
                    player,
                    kind: PlayerConditionKind::IsMonarch,
                });
            }
            if ts.eat_phrase(&["is", "poisoned"]) || ts.eat_phrase(&["are", "poisoned"]) {
                return Ok(ConditionExpr::PlayerCheck {
                    player,
                    kind: PlayerConditionKind::Poisoned,
                });
            }
        }
        ts.restore(pcp);
    }

    // Mana spent comparisons (`the amount of mana spent to cast it/that spell was less than its mana value`,
    // `no mana was spent to cast that spell`, `none of them were cast or no mana was spent to cast them`,
    // `at least <N> [colorless] mana [of the same color] was spent to cast it/~`,
    // `<N> or more mana from <flt> was spent to cast it`, `{M} wasn't spent to cast it/~`)
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "amount", "of", "mana", "spent", "to", "cast"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["was", "less", "than", "its", "mana", "value"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::ManaPaidThisWay),
                op: CompareOp::Lt,
                rhs: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(subj),
                    stat: StatKind::ManaValue,
                }),
            });
        }
        ts.restore(mcp);
        if ts.eat_phrase(&["no", "mana", "was", "spent", "to", "cast"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(subj),
                kind: SubjectConditionKind::NotCastOrNoManaSpent,
            });
        }
        ts.restore(mcp);
        if ts.eat_phrase(&[
            "none", "of", "them", "were", "cast", "or", "no", "mana", "was", "spent", "to", "cast",
            "them",
        ]) {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
                kind: SubjectConditionKind::NotCastOrNoManaSpent,
            });
        }
        ts.restore(mcp);
        if ts.eat_phrase(&["at", "least"])
            && let Some(n) = ts.next_number(false)
        {
            let _ = ts.eat_word("colorless");
            if ts.eat_word("mana") {
                let _ = ts.eat_phrase(&["of", "the", "same", "color"]);
                if ts.eat_phrase(&["was", "spent", "to", "cast"])
                    && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::ManaPaidThisWay),
                        op: CompareOp::Ge,
                        rhs: Box::new(ValueExpr::Const(n)),
                    });
                }
            }
        }
        ts.restore(mcp);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "mana", "from"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::ManaSpentFromFilter(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(mcp);
        if let Some(mc) = ts.eat_mana_cost()
            && ts.eat_phrase(&["wasn't", "spent", "to", "cast"])
            && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
        {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::EventCheck(
                EventCheckKind::ManaCostSpent(mc),
            ))));
        }
        ts.restore(mcp);
        if ts.eat_phrase(&["mana", "from"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["was", "spent", "to", "activate", "this", "ability"])
                || ts.eat_phrase(&["was", "spent", "to", "cast", "it", "or", "activate", "it"]))
        {
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::ManaSpentFrom,
                value: 1,
            });
        }
        ts.restore(mcp);
    }

    // Energy paid this way (`you paid <N> or more {E} this way` / `<N> or more {E} was paid this way`)
    {
        let ecp = ts.checkpoint();
        let _ = ts.eat_phrase(&["you", "paid"]);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            if ts.eat_phrase(&["this", "way"]) || ts.eat_phrase(&["was", "paid", "this", "way"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PriorActionCount(
                        PriorActionKind::EnergyPaidThisWay,
                    )),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(ecp);
    }

    // Possessive stat comparisons (`the card's / the sacrificed creature's / the discovered card's / his / her / its <stat> is/was [less than | greater than | exactly] <val> [or less | or greater]`)
    {
        let scp = ts.checkpoint();
        let subj_opt = if ts.eat_word("its") || ts.eat_word("his") || ts.eat_word("her") {
            Some(SubjectRef::Anaphora(AnaphoraRef::It))
        } else if ts.eat_phrase(&["the", "card's"])
            || ts.eat_phrase(&["the", "discovered", "card's"])
            || ts.eat_phrase(&["the", "revealed", "card's"])
        {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatCard))
        } else if ts.eat_phrase(&["the", "sacrificed", "creature's"])
            || ts.eat_phrase(&["the", "sacrificed", "permanent's"])
        {
            Some(SubjectRef::Anaphora(AnaphoraRef::SacrificedObject))
        } else if ts.eat_phrase(&["that", "creature's"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatCreature))
        } else if ts.eat_phrase(&["that", "card's"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatCard))
        } else if ts.eat_phrase(&["that", "spell's"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatSpell))
        } else {
            None
        };
        if let Some(subj) = subj_opt {
            let stat_opt = if ts.eat_phrase(&["mana", "value"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_word("power") {
                Some(StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if let Some(stat) = stat_opt
                && (ts.eat_word("is") || ts.eat_word("was"))
            {
                if ts.eat_phrase(&["greater", "than", "each", "other", "creature's", "power"]) {
                    return Ok(ConditionExpr::Flag(
                        ConditionFlag::NoOtherCreatureHasGreaterPower,
                    ));
                }
                let prefix_op = if ts.eat_phrase(&["less", "than", "or", "equal", "to"])
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
                } else if ts.eat_word("exactly") || ts.eat_phrase(&["equal", "to"]) {
                    Some(CompareOp::Eq)
                } else {
                    None
                };
                if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                    let op = if let Some(pop) = prefix_op {
                        pop
                    } else if ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "fewer"]) {
                        CompareOp::Le
                    } else if ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]) {
                        CompareOp::Ge
                    } else {
                        CompareOp::Eq
                    };
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::ObjectStat {
                            subject: Box::new(subj),
                            stat,
                        }),
                        op,
                        rhs: Box::new(rhs),
                    });
                }
            }
        }
        ts.restore(scp);
    }

    // Creature / permanent death & damage-dealt turn checks
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["no", "creatures", "died", "this", "turn"]) {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                ConditionFlag::PermanentDiedThisTurn,
            ))));
        }
        if ts.eat_phrase(&["another", "creature", "died", "this", "turn"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentDiedThisTurn));
        }
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&[
                "or",
                "more",
                "creatures",
                "died",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::CreatureDied,
                filter: None,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(dcp);
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&[
                "died",
                "under",
                "an",
                "opponent's",
                "control",
                "this",
                "turn",
            ])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::AnyOpponent,
                event: TurnEventKind::CreatureDied,
                filter: Some(Box::new(flt)),
                min_count: 1,
            });
        }
        ts.restore(dcp);
    }

    // Player / subject dealt damage this turn (`an opponent / a player / you was/were dealt <N> or more [combat|noncombat] damage this turn`,
    // `it / ~ was dealt [combat|noncombat] damage this turn`)
    {
        let dmg_cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && (ts.eat_phrase(&["was", "dealt"]) || ts.eat_phrase(&["were", "dealt"]))
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
        {
            let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
            if ts.eat_phrase(&["damage", "this", "turn"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::DamageDealtToPlayerThisTurn(player)),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(dmg_cp);
        if (ts.eat_word("it") || ts.eat_tilde()) && ts.eat_phrase(&["was", "dealt"]) {
            let combat_only = ts.eat_word("combat");
            let _ = ts.eat_word("noncombat");
            if ts.eat_phrase(&["damage", "this", "turn"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    kind: SubjectConditionKind::DealtDamageThisTurn { combat_only },
                });
            }
        }
        ts.restore(dmg_cp);
    }

    // Cards left graveyard this turn (`<N> or more <flt> left your graveyard this turn`, `a <flt> left your graveyard this turn`)
    {
        let gcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false) {
            let _ = ts.eat_phrase(&["or", "more"]);
            if try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_phrase(&["left", "your", "graveyard", "this", "turn"])
                && n >= 1
            {
                return Ok(ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::CardsLeftGraveyardThisTurn,
                });
            }
        }
        ts.restore(gcp);
        if (ts.eat_word("a") || ts.eat_word("an"))
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["left", "your", "graveyard", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::CardsLeftGraveyardThisTurn,
            });
        }
        ts.restore(gcp);
    }

    // `you played a land this turn` / `you've cast both a <flt1> and a <flt2> this turn`
    if ts.eat_phrase(&["you", "played", "a", "land", "this", "turn"])
        || ts.eat_phrase(&["you've", "played", "a", "land", "this", "turn"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::PlayedLandThisTurn,
        });
    }
    {
        let ccp = ts.checkpoint();
        if (ts.eat_phrase(&["you've", "cast", "both"]) || ts.eat_phrase(&["you", "cast", "both"]))
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
        ts.restore(ccp);
    }

    // Hand size / hand contents / comparative control & life (`you have a card in hand`, `you have no <flt> in hand`,
    // `<player> has more than <N> cards in hand`, `<player> has more cards in hand than each other player`,
    // `<p1> control(s) fewer <flt> than <p2>`, `<p1> has/have less life than <p2>`, `<p1> has more life than another of your opponents`)
    if ts.eat_phrase(&["you", "have", "a", "card", "in", "hand"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::PlayerStat {
                player: PlayerRef::You,
                stat: PlayerStatKind::CardsInHand,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "have", "no"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["in", "hand"])
        {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone: ZoneRef {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::You,
                },
                min_count: 1,
            })));
        }
        ts.restore(hcp);
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if (ts.eat_phrase(&["has", "more", "than"]) || ts.eat_phrase(&["have", "more", "than"]))
                && let Some(n) = ts.next_number(false)
                && (ts.eat_phrase(&["cards", "in", "hand"])
                    || ts.eat_phrase(&["card", "in", "hand"]))
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    op: CompareOp::Gt,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
            ts.restore(hcp);
            if let Ok(player) = try_parse(ts, parse_player_ref)
                && (ts.eat_phrase(&[
                    "has", "more", "cards", "in", "hand", "than", "each", "other", "player",
                ]) || ts.eat_phrase(&[
                    "have", "more", "cards", "in", "hand", "than", "each", "other", "player",
                ]))
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    op: CompareOp::Gt,
                    rhs: Box::new(ValueExpr::PlayerStat {
                        player: PlayerRef::EachOtherPlayer,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                });
            }
            ts.restore(hcp);
            if let Ok(player) = try_parse(ts, parse_player_ref)
                && (ts.eat_phrase(&["control", "fewer"]) || ts.eat_phrase(&["controls", "fewer"]))
                && let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_word("than")
                && let Ok(other) = try_parse(ts, parse_player_ref)
            {
                return Ok(ConditionExpr::ControlsMoreThan {
                    player: other,
                    other: player,
                    filter: Box::new(filter),
                });
            }
            ts.restore(hcp);
            if let Ok(player) = try_parse(ts, parse_player_ref)
                && (ts.eat_phrase(&["has", "less", "life", "than"])
                    || ts.eat_phrase(&["have", "less", "life", "than"]))
                && let Ok(other) = try_parse(ts, parse_player_ref)
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                    op: CompareOp::Lt,
                    rhs: Box::new(ValueExpr::PlayerStat {
                        player: other,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                });
            }
            ts.restore(hcp);
            if let Ok(player) = try_parse(ts, parse_player_ref)
                && ts.eat_phrase(&[
                    "has",
                    "more",
                    "life",
                    "than",
                    "another",
                    "of",
                    "your",
                    "opponents",
                ])
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                    op: CompareOp::Gt,
                    rhs: Box::new(ValueExpr::PlayerStat {
                        player: PlayerRef::EachOtherOpponent,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                });
            }
        }
        ts.restore(hcp);
    }

    // Cast / keyword cost / not-cast-from-hand
    if (ts.eat_tilde() || ts.eat_word("it"))
        && ts.eat_phrase(&["wasn't", "cast", "from", "your", "hand"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::WasCastFromHand,
        ))));
    }
    {
        let kcp = ts.checkpoint();
        if (ts.eat_word("his") || ts.eat_word("her"))
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
            && ts.eat_phrase(&["cost", "was", "paid"])
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(kcp);
        if ts.eat_phrase(&["they", "were", "cast", "using"])
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
        {
            let _ = ts.eat_word("cost");
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(kcp);
    }

    // `any of those cards remain(s) exiled` / `any of those cards shares a <prop> with <subj>`
    if ts.eat_phrase(&["any", "of", "those", "cards", "remain", "exiled"])
        || ts.eat_phrase(&["any", "of", "them", "remain", "exiled"])
    {
        return Ok(ConditionExpr::SubjectCheck {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            kind: SubjectConditionKind::RemainsExiled,
        });
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["any", "of", "those", "cards", "shares", "a"]) {
            let kind = if ts.eat_phrase(&["card", "type"]) {
                Some(SharedProperty::CardType)
            } else if ts.eat_phrase(&["creature", "type"]) {
                Some(SharedProperty::CreatureType)
            } else if ts.eat_word("color") {
                Some(SharedProperty::Color)
            } else {
                None
            };
            if let Some(kind) = kind
                && ts.eat_word("with")
                && let Ok(other) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ConditionExpr::SharesTypeWith {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThoseCards)),
                    other: Box::new(other),
                    kind,
                });
            }
        }
        ts.restore(acp);
    }

    // `two [nonland] cards that share (a|all their) <prop> (were|are) (milled|discarded) this way`
    {
        let tcp = ts.checkpoint();
        if ts.eat_word("two") {
            let nonland = ts.eat_word("nonland");
            if ts.eat_phrase(&["cards", "that", "share"]) {
                let _ = ts.eat_word("a") || ts.eat_phrase(&["all", "their"]);
                let prop = if ts.eat_phrase(&["card", "type"]) || ts.eat_phrase(&["card", "types"])
                {
                    Some(SharedProperty::CardType)
                } else if ts.eat_word("color") || ts.eat_word("colors") {
                    Some(SharedProperty::Color)
                } else {
                    None
                };
                if let Some(property) = prop
                    && (ts.eat_word("were") || ts.eat_word("are"))
                {
                    let act = if ts.eat_phrase(&["milled", "this", "way"]) {
                        Some(PriorActionKind::MilledThisWay)
                    } else if ts.eat_phrase(&["discarded", "this", "way"]) {
                        Some(PriorActionKind::DiscardedThisWay)
                    } else {
                        None
                    };
                    if let Some(action) = act {
                        let mut flt = ObjectFilter {
                            any_card: true,
                            predicates: vec![FilterPredicate::SharesPropertyWith {
                                property,
                                reference: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThoseCards)),
                                negated: false,
                            }],
                            ..Default::default()
                        };
                        if nonland {
                            flt.forbidden_types = CardTypeMask::LAND;
                        }
                        return Ok(ConditionExpr::PriorActionMatched {
                            action,
                            filter: Box::new(flt),
                        });
                    }
                }
            }
        }
        ts.restore(tcp);
    }

    // `two or more of those creatures are attacking you [and/or planeswalkers you control]`
    {
        let acp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "of", "those", "creatures", "are", "attacking"])
            && try_parse(ts, parse_player_ref).is_ok()
        {
            if ts.eat_word("and/or") || ts.eat_word("or") {
                let _ = opt(ts, parse_object_filter);
            }
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(acp);
    }

    // `this is the (first or second | fourth..tenth) time this ability has resolved this turn`
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["this", "is", "the"]) {
            if ts.eat_phrase(&[
                "first", "or", "second", "time", "this", "ability", "has", "resolved", "this",
                "turn",
            ]) {
                return Ok(ConditionExpr::Or(
                    Box::new(ConditionExpr::EventCheck(
                        EventCheckKind::ResolutionCountThisTurn(1),
                    )),
                    Box::new(ConditionExpr::EventCheck(
                        EventCheckKind::ResolutionCountThisTurn(2),
                    )),
                ));
            }
            let nth = if ts.eat_word("fourth") {
                Some(4)
            } else if ts.eat_word("fifth") {
                Some(5)
            } else if ts.eat_word("sixth") {
                Some(6)
            } else {
                None
            };
            if let Some(n) = nth
                && ts.eat_phrase(&["time", "this", "ability", "has", "resolved", "this", "turn"])
            {
                return Ok(ConditionExpr::EventCheck(
                    EventCheckKind::ResolutionCountThisTurn(n),
                ));
            }
        }
        ts.restore(rcp);
    }

    // `they lose the flip` / `that player loses the flip`
    if ts.eat_phrase(&["they", "lose", "the", "flip"])
        || ts.eat_phrase(&["that", "player", "loses", "the", "flip"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::WonCoinFlip,
        ))));
    }

    // `there's another <flt> on the battlefield`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["there's", "another"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["on", "the", "battlefield"])
        {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone: ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                },
                min_count: 1,
            });
        }
        ts.restore(tcp);
    }

    // `no <flt> were revealed/removed/discarded this way` / `fewer than <N> <flt> were discarded this way`
    {
        let ncp = ts.checkpoint();
        if ts.eat_word("no") {
            if ts.eat_phrase(&["counters", "were", "removed", "this", "way"]) {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::EventCheck(
                    EventCheckKind::PriorObjectEvent(PriorActionKind::CountersRemovedThisWay),
                ))));
            }
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && (ts.eat_word("were") || ts.eat_word("was"))
            {
                let act = if ts.eat_phrase(&["revealed", "this", "way"]) {
                    Some(PriorActionKind::RevealedThisWay)
                } else if ts.eat_phrase(&["discarded", "this", "way"]) {
                    Some(PriorActionKind::DiscardedThisWay)
                } else if ts.eat_phrase(&["exiled", "this", "way"]) {
                    Some(PriorActionKind::ExiledThisWay)
                } else if ts.eat_phrase(&["destroyed", "this", "way"]) {
                    Some(PriorActionKind::DestroyedThisWay)
                } else {
                    None
                };
                if let Some(action) = act {
                    return Ok(ConditionExpr::Not(Box::new(
                        ConditionExpr::PriorActionMatched {
                            action,
                            filter: Box::new(flt),
                        },
                    )));
                }
            }
        }
        ts.restore(ncp);
        if ts.eat_phrase(&["fewer", "than"])
            && let Some(n) = ts.next_number(false)
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["were", "discarded", "this", "way"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                    predicates: vec![FilterPredicate::AffectedByPriorAction(
                        PriorActionKind::DiscardedThisWay,
                    )],
                    ..flt
                }))),
                op: CompareOp::Lt,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(ncp);
    }

    // `<player> [didn't] <verb> [no | fewer than N | at least N | N or more | one or more] <flt> [to <zone>] this way`
    {
        let pcp = ts.checkpoint();
        if let Ok(_player) = try_parse(ts, parse_player_ref) {
            let neg = ts.eat_word("didn't") || ts.eat_word("don't") || ts.eat_word("doesn't");
            let act_opt = if ts.eat_word("discard")
                || ts.eat_word("discards")
                || ts.eat_word("discarded")
            {
                Some(PriorActionKind::DiscardedThisWay)
            } else if ts.eat_word("return") || ts.eat_word("returns") || ts.eat_word("returned") {
                Some(PriorActionKind::ReturnedThisWay)
            } else if ts.eat_word("reveal") || ts.eat_word("reveals") || ts.eat_word("revealed") {
                Some(PriorActionKind::RevealedThisWay)
            } else if ts.eat_word("sacrifice")
                || ts.eat_word("sacrifices")
                || ts.eat_word("sacrificed")
            {
                Some(PriorActionKind::SacrificedThisWay)
            } else if ts.eat_word("mill") || ts.eat_word("mills") || ts.eat_word("milled") {
                Some(PriorActionKind::MilledThisWay)
            } else if ts.eat_word("draw") || ts.eat_word("draws") || ts.eat_word("drew") {
                Some(PriorActionKind::ThatMany)
            } else if ts.eat_word("put") || ts.eat_word("puts") {
                Some(PriorActionKind::ReturnedThisWay)
            } else {
                None
            };
            if let Some(action) = act_opt {
                let is_no = ts.eat_word("no");
                let is_fewer = if !is_no && ts.eat_phrase(&["fewer", "than"]) {
                    let _ = ts.next_number(false);
                    true
                } else {
                    false
                };
                if !is_no && !is_fewer {
                    let _ = ts.eat_phrase(&["at", "least"]);
                    if ts.next_number(false).is_some() {
                        let _ = ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "fewer"]);
                    }
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    let _ = opt(ts, parse_zone_dest);
                    if ts.eat_phrase(&["this", "way"]) {
                        let base = ConditionExpr::PriorActionMatched {
                            action,
                            filter: Box::new(flt),
                        };
                        return Ok(if neg || is_no || is_fewer {
                            ConditionExpr::Not(Box::new(base))
                        } else {
                            base
                        });
                    }
                }
            }
        }
        ts.restore(pcp);
    }

    // `<subj | flt> is/was/are/were (put into <zone> | returned to <zone> | destroyed | countered | dies | put onto the battlefield) this way`
    // and `<N or more | a> <flt> was/were put into <zone> from (the battlefield | anywhere [other than the battlefield]) this turn`
    {
        let wcp = ts.checkpoint();
        let _ = ts.eat_phrase(&["one", "or", "more"]);
        let min_n = if let Some(n) = ts.next_number(false) {
            let _ = ts.eat_phrase(&["or", "more"]);
            Some(n)
        } else {
            None
        };
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("is")
                || ts.eat_word("are")
                || ts.eat_word("was")
                || ts.eat_word("were"))
        {
            if ts.eat_phrase(&["put", "into"]) || ts.eat_phrase(&["put", "onto"]) {
                if let Ok(_zone) = try_parse(ts, parse_zone_ref) {
                    if ts.eat_phrase(&["this", "way"]) {
                        return Ok(ConditionExpr::PriorActionMatched {
                            action: PriorActionKind::ReturnedThisWay,
                            filter: Box::new(flt),
                        });
                    }
                    if ts.eat_word("from") {
                        let _ = ts.eat_phrase(&["anywhere", "other", "than", "the", "battlefield"])
                            || ts.eat_phrase(&["the", "battlefield"])
                            || ts.eat_word("anywhere");
                        if ts.eat_phrase(&["this", "turn"]) {
                            return Ok(ConditionExpr::PlayerTurnEvent {
                                player: PlayerRef::You,
                                event: TurnEventKind::CreatureDied,
                                filter: Some(Box::new(flt)),
                                min_count: u16::try_from(min_n.unwrap_or(1).max(1)).unwrap_or(1),
                            });
                        }
                    }
                }
            } else if ts.eat_phrase(&["countered", "this", "way"]) {
                return Ok(ConditionExpr::PriorActionMatched {
                    action: PriorActionKind::ExiledThisWay,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(wcp);
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["dies", "this", "way"])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::DestroyedThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(wcp);
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["is", "put", "into", "a", "graveyard", "this", "way"])
                || ts.eat_phrase(&["is", "destroyed", "this", "way"])
                || ts.eat_phrase(&["was", "destroyed", "this", "way"])
            {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                    PriorActionKind::DestroyedThisWay,
                )));
            }
            if ts.eat_phrase(&[
                "is", "returned", "to", "its", "owner's", "hand", "this", "way",
            ]) || ts.eat_phrase(&["is", "returned", "to", "your", "hand", "this", "way"])
            {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                    PriorActionKind::ReturnedThisWay,
                )));
            }
            if ts.eat_phrase(&["is", "countered", "this", "way"]) {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                    PriorActionKind::ExiledThisWay,
                )));
            }
            if ts.eat_phrase(&["was", "sacrificed"]) {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                    PriorActionKind::SacrificedThisWay,
                )));
            }
            if ts.eat_phrase(&["is", "on", "the", "stack"]) {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone: ZoneRef {
                        zone: ZoneKind::Stack,
                        owner: ZoneOwner::Default,
                    },
                });
            }
            if ts.eat_phrase(&["has", "a", "single", "target"]) {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        flags: FilterFlags::SINGLE_TARGET,
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&["attacked", "you"]) {
                if ts.eat_word("and/or") || ts.eat_word("or") {
                    let _ = opt(ts, parse_object_filter);
                }
                let _ = ts.eat_phrase(&["during", "their", "last", "turn"]);
                return Ok(ConditionExpr::PlayerCheck {
                    player: PlayerRef::ThatPlayer,
                    kind: PlayerConditionKind::AttackedThisTurn,
                });
            }
        }
        ts.restore(wcp);
    }

    // `creatures you control / those creatures have total toughness/power <N> or greater`
    {
        let tcp = ts.checkpoint();
        let is_those = ts.eat_phrase(&["those", "creatures", "have", "total"]);
        let is_yours =
            !is_those && ts.eat_phrase(&["creatures", "you", "control", "have", "total"]);
        if is_those || is_yours {
            let stat = if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else if ts.eat_word("power") {
                Some(StatKind::Power)
            } else {
                None
            };
            if let Some(stat) = stat
                && let Some(n) = ts.next_number(false)
                && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]))
            {
                let mut flt = ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                };
                if is_yours {
                    flt.controller = Some(crate::ast::ControllerRelation::You);
                }
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::AggregateStat {
                        op: crate::v2::ir::AggregateOp::Sum,
                        stat,
                        filter: Box::new(flt),
                    }),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(tcp);
    }

    super::extended_extra::parse_extended_extra(ts)
}
