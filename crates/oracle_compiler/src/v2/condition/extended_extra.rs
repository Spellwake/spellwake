//! Additional compositional [`ConditionExpr`] grammar families for `v2`.

use crate::ast::{CardTypeMask, CompareOp, PlayerStatKind, StatKind};
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_dest,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, EventCheckKind, FilterFlags, FilterPredicate,
    ManaSymbol, ObjectFilter, PlayerConditionKind, PlayerRef, PriorActionKind, SharedProperty,
    SubjectConditionKind, SubjectRef, TurnEventKind, ValueExpr, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Parse elided right-hand conjuncts following `and` in compound conditions.
pub(super) fn try_parse_elided_and_rhs(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["have", "no", "cards", "in", "hand"]) {
        return Some(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::PlayerStat {
                player: PlayerRef::You,
                stat: PlayerStatKind::CardsInHand,
            }),
            op: CompareOp::Eq,
            rhs: Box::new(ValueExpr::Const(0)),
        });
    }
    if (ts.eat_word("has") || ts.eat_word("have"))
        && let Some(n) = ts.next_number(false)
    {
        let op = if ts.eat_phrase(&["or", "fewer"]) || ts.eat_phrase(&["or", "less"]) {
            CompareOp::Le
        } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
            CompareOp::Ge
        } else {
            CompareOp::Eq
        };
        if ts.eat_phrase(&["cards", "in", "hand"]) || ts.eat_phrase(&["card", "in", "hand"]) {
            return Some(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::ThatPlayer,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
    }
    ts.restore(cp);
    if let Ok(subj) = try_parse(ts, parse_subject_ref)
        && (ts.eat_word("doesn't") || ts.eat_word("don't"))
        && super::at_cond_boundary(ts)
    {
        return Some(ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
            subject: Box::new(subj),
            kind: SubjectConditionKind::HasChosenName,
        })));
    }
    ts.restore(cp);
    None
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_extended_extra(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // 1. Ring-bearer and Ring-temptation conditions
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "chose", "a", "creature"]) {
            let _ = ts.eat_phrase(&["other", "than"]) && ts.eat_tilde();
            if ts.eat_phrase(&["as", "your", "ring-bearer"]) {
                return Ok(ConditionExpr::Flag(ConditionFlag::ControlsSelf));
            }
        }
        ts.restore(rcp);
        if ts.eat_phrase(&["the", "ring", "has", "tempted", "you"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "times", "this", "game"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(rcp);
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            let neg = ts.eat_word("isn't") || ts.eat_phrase(&["is", "not"]);
            let pos = !neg && ts.eat_word("is");
            if (neg || pos) && ts.eat_phrase(&["your", "ring-bearer"]) {
                let base = ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: crate::v2::ir::ObjectStateKind::Modified,
                };
                return Ok(if neg {
                    ConditionExpr::Not(Box::new(base))
                } else {
                    base
                });
            }
        }
        ts.restore(rcp);
    }

    // 2. Turn keyword-action / player state checks (scry, surveil, attractions, dice, exhaust, energy, loyalty, cycling, traps)
    if ts.eat_phrase(&["you've", "scried", "or", "surveilled", "this", "turn"])
        || ts.eat_phrase(&["you've", "surveilled", "this", "turn"])
        || ts.eat_phrase(&["you've", "scried", "this", "turn"])
        || ts.eat_phrase(&["you've", "visited", "an", "attraction", "this", "turn"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::DescendedThisTurn,
        });
    }
    if ts.eat_phrase(&[
        "you",
        "haven't",
        "activated",
        "an",
        "exhaust",
        "ability",
        "this",
        "turn",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::CommittedCrimeThisTurn,
        })));
    }
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "rolled"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "dice", "this", "turn"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(ecp);
        if ts.eat_phrase(&["you've", "paid", "or", "lost"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            if ts.eat_phrase(&["this", "turn"]) {
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
        let loy_neg = ts.eat_phrase(&["you", "didn't", "activate", "a", "loyalty", "ability"]);
        let loy_pos =
            !loy_neg && ts.eat_phrase(&["you've", "activated", "a", "loyalty", "ability"]);
        if loy_neg || loy_pos {
            let _ = ts.eat_phrase(&["of", "a", "planeswalker"]);
            if ts.eat_phrase(&["this", "turn"]) {
                let base = ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::CommittedCrimeThisTurn,
                };
                return Ok(if loy_neg {
                    ConditionExpr::Not(Box::new(base))
                } else {
                    base
                });
            }
        }
        ts.restore(ecp);
        if ts.eat_phrase(&["you", "cycled"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::CardDiscarded,
                filter: None,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(ecp);
        if ts.eat_phrase(&["you've", "cycled", "a", "card", "named"])
            && ts.eat_tilde()
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "times", "this", "game"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::CardDiscarded,
                filter: None,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(ecp);
    }
    if ts.eat_phrase(&[
        "you've", "played", "a", "land", "or", "cast", "a", "spell", "this", "turn", "from",
        "anywhere", "other", "than", "your", "hand",
    ]) || ts.eat_phrase(&[
        "you've",
        "cast",
        "a",
        "spell",
        "from",
        "a",
        "graveyard",
        "or",
        "activated",
        "an",
        "ability",
        "of",
        "a",
        "card",
        "in",
        "a",
        "graveyard",
        "this",
        "turn",
    ]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::PlayedFromExileThisTurn,
        });
    }
    if ts.eat_phrase(&[
        "an", "opponent", "searched", "their", "library", "this", "turn",
    ]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::SearchedThisWay,
        )));
    }
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["an", "opponent", "had"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&[
                "or",
                "more",
                "cards",
                "put",
                "into",
                "their",
                "graveyard",
                "from",
                "anywhere",
                "this",
                "turn",
            ])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::AnyOpponent,
                event: TurnEventKind::CreatureDied,
                filter: None,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(tcp);
        if ts.eat_phrase(&["you've", "been", "dealt", "damage", "by"])
            && ts.next_number(false).is_some()
            && ts.eat_phrase(&["or", "more", "creatures", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::DealtDamageThisTurn { combat_only: false },
            });
        }
        ts.restore(tcp);
    }

    // 3. Player combat, life-rank, prime count, identity, and prior-action checks
    if ts.eat_phrase(&["you're", "the", "defending", "player"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::CreatureAttackingYou));
    }
    if ts.eat_phrase(&["you're", "not", "the", "starting", "player"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::IsYourTurn,
        ))));
    }
    if ts.eat_phrase(&["you", "have", "the", "most", "life"]) {
        let _ = ts.eat_phrase(&["or", "are", "tied", "for", "the", "most", "life"])
            || ts.eat_phrase(&["or", "are", "tied", "for", "most", "life"]);
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::NoOpponentHasMoreLifeThan,
        });
    }
    {
        let pcp = ts.checkpoint();
        if ts.eat_phrase(&["you're", "attacking"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "opponents"])
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::AttackedWithCount {
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                },
            });
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["you", "control", "a", "prime", "number", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::Controls {
                player: PlayerRef::You,
                filter: Box::new(flt),
                min_count: 2,
            });
        }
        ts.restore(pcp);
        if ts.eat_phrase(&[
            "you",
            "control",
            "one",
            "or",
            "more",
            "other",
            "creatures",
            "that",
            "spell",
            "could",
            "target",
        ]) {
            return Ok(ConditionExpr::Controls {
                player: PlayerRef::You,
                filter: Box::new(ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    flags: FilterFlags::EXCLUDE_SELF,
                    ..Default::default()
                }),
                min_count: 1,
            });
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["you", "control", "neither", "creature"]) {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                player: PlayerRef::You,
                filter: Box::new(ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                min_count: 1,
            })));
        }
        ts.restore(pcp);
        if (ts.eat_phrase(&["you", "can't", "sacrifice"])
            || ts.eat_phrase(&["you", "don't", "sacrifice"]))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::Not(Box::new(
                ConditionExpr::SacrificedPermanentThisTurn {
                    player: PlayerRef::You,
                    filter: Box::new(flt),
                },
            )));
        }
        ts.restore(pcp);
    }
    if ts.eat_phrase(&["you", "lost", "life", "this", "way"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::LostLifeThisTurn,
        });
    }
    if ts.eat_phrase(&["no", "life", "is", "lost", "this", "way"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::EachPlayer,
            kind: PlayerConditionKind::LostLifeThisTurn,
        })));
    }
    if ts.eat_phrase(&[
        "you", "gain", "control", "of", "a", "creature", "this", "way",
    ]) || ts.eat_phrase(&["you", "chose", "a", "creature", "this", "way"])
    {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::ThatMany,
        )));
    }
    if ts.eat_phrase(&["you", "won"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::WonCoinFlip));
    }
    if ts.eat_phrase(&["an", "opponent", "created", "a", "token", "this", "way"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::CreatedThisWay,
        )));
    }
    if ts.eat_phrase(&["an", "opponent", "protects", "it"]) {
        return Ok(ConditionExpr::ControlsSubject {
            player: PlayerRef::AnyOpponent,
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
        });
    }
    if ts.eat_phrase(&["that", "player", "is", "you"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsYourTurn));
    }
    if ts.eat_phrase(&["the", "player", "is", "your", "opponent"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::IsYourTurn,
        ))));
    }
    if ts.eat_phrase(&[
        "that", "player", "has", "another", "opponent", "who", "isn't", "being", "attacked",
    ]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::OpponentsCount),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(2)),
        });
    }
    if ts.eat_phrase(&[
        "you",
        "or",
        "a",
        "player",
        "you're",
        "attacking",
        "has",
        "the",
        "initiative",
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::HasInitiative));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&["they", "have", "exactly"])
            && let Some(n1) = ts.next_number(false)
            && ts.eat_phrase(&["or", "exactly"])
            && let Some(n2) = ts.next_number(false)
            && ts.eat_phrase(&["cards", "in", "hand"])
        {
            return Ok(ConditionExpr::Or(
                Box::new(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player: PlayerRef::ThatPlayer,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    op: CompareOp::Eq,
                    rhs: Box::new(ValueExpr::Const(n1)),
                }),
                Box::new(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player: PlayerRef::ThatPlayer,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    op: CompareOp::Eq,
                    rhs: Box::new(ValueExpr::Const(n2)),
                }),
            ));
        }
        ts.restore(hcp);
    }
    if ts.eat_phrase(&[
        "they", "didn't", "put", "the", "card", "on", "top", "of", "their", "library",
    ]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::DidntPutInZone(
            ZoneKind::Library,
        )));
    }

    // 4. Spell / activation / cost / warp / counter-movement conditions
    if ts.eat_phrase(&["a", "spell", "was", "warped", "this", "turn"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
            crate::v2::ir::KeywordKind::Warp,
        )));
    }
    if ts.eat_phrase(&["life", "was", "paid", "to", "activate", "it"])
        || ts.eat_phrase(&[
            "one",
            "or",
            "more",
            "permanents",
            "were",
            "sacrificed",
            "to",
            "activate",
            "it",
        ])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
    }
    if ts.eat_phrase(&["her", "additional", "cost", "was", "paid"])
        || ts.eat_phrase(&["his", "additional", "cost", "was", "paid"])
        || ts.eat_phrase(&["its", "additional", "cost", "was", "paid"])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
    }
    if ts.eat_phrase(&["that", "cost", "wasn't", "paid"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::AdditionalCostPaid,
        ))));
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["you", "removed"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && try_parse(ts, parse_counter_kind).is_ok()
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_phrase(&["to", "activate", "it"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PriorActionCount(
                    PriorActionKind::CountersRemovedThisWay,
                )),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(ccp);
        let exact = ts.eat_word("exactly");
        if let Some(n) = ts.next_number(false) {
            let _ = ts.eat_phrase(&["or", "more"]);
            if ts.eat_phrase(&["colors", "of", "mana", "were", "spent", "to"])
                && (ts.eat_phrase(&["activate", "this", "ability"])
                    || ts.eat_phrase(&["cast", "it"]))
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::ColorsOfManaSpent),
                    op: if exact { CompareOp::Eq } else { CompareOp::Ge },
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(ccp);
        if matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(ManaSymbol::Snow))
        ) {
            ts.next_tok();
            if ts.eat_phrase(&[
                "of", "any", "of", "that", "spell's", "colors", "was", "spent", "to", "cast", "it",
            ]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::SnowManaSpent),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(1)),
                });
            }
        }
        ts.restore(ccp);
        if ts.eat_phrase(&[
            "that",
            "spell's",
            "mana",
            "cost",
            "or",
            "that",
            "ability's",
            "activation",
            "cost",
            "contains",
        ]) && let Some(sym) = ts.next_mana()
        {
            return Ok(ConditionExpr::ManaCostContains {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatSpell)),
                symbol: sym,
            });
        }
        ts.restore(ccp);
        if ts.eat_phrase(&["one", "or", "more"]) || ts.eat_word("a") || ts.eat_word("an") {
            let kind_opt = opt(ts, parse_counter_kind);
            if ts.eat_word("counters") || ts.eat_word("counter") {
                if ts.eat_phrase(&["are", "moved", "this", "way"])
                    || ts.eat_phrase(&["were", "moved", "this", "way"])
                {
                    return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                        PriorActionKind::CountersRemovedThisWay,
                    )));
                }
                if ts.eat_phrase(&["was", "put", "on"]) {
                    if ts.eat_phrase(&[
                        "a",
                        "permanent",
                        "under",
                        "your",
                        "control",
                        "this",
                        "turn",
                    ]) {
                        return Ok(ConditionExpr::SubjectCheck {
                            subject: Box::new(SubjectRef::SelfObject),
                            kind: SubjectConditionKind::HasCounters(kind_opt),
                        });
                    }
                    if let Ok(subj) = try_parse(ts, parse_subject_ref)
                        && ts.eat_phrase(&["this", "turn"])
                    {
                        return Ok(ConditionExpr::SubjectCheck {
                            subject: Box::new(subj),
                            kind: SubjectConditionKind::HasCounters(kind_opt),
                        });
                    }
                }
                if ts.eat_phrase(&["was", "removed", "from"])
                    && try_parse(ts, parse_object_filter).is_ok()
                    && ts.eat_phrase(&["this", "turn"])
                {
                    return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                        PriorActionKind::CountersRemovedThisWay,
                    )));
                }
            }
        }
        ts.restore(ccp);
        if let Ok(flt) = try_parse(ts, parse_object_filter) {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_phrase(&[
                "was",
                "countered",
                "by",
                "a",
                "spell",
                "or",
                "ability",
                "an",
                "opponent",
                "controlled",
            ]) {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter: Some(Box::new(flt)),
                    min_count: 1,
                });
            }
            if ts.eat_phrase(&[
                "was",
                "destroyed",
                "this",
                "turn",
                "by",
                "a",
                "spell",
                "or",
                "ability",
                "an",
                "opponent",
                "controlled",
            ]) {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::CreatureDied,
                    filter: Some(Box::new(flt)),
                    min_count: 1,
                });
            }
            if ts.eat_phrase(&[
                "entered",
                "the",
                "battlefield",
                "face",
                "down",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ]) {
                return Ok(ConditionExpr::PermanentEnteredThisTurn {
                    filter: Box::new(flt),
                    controller: PlayerRef::You,
                    min_count: 1,
                });
            }
        }
        ts.restore(ccp);
        if ts.eat_phrase(&[
            "you",
            "turned",
            "a",
            "permanent",
            "face",
            "up",
            "this",
            "turn",
        ]) {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::DescendedThisTurn,
            });
        }
    }

    // 5. Combat, damage-source, and creature-group conditions
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["any", "of", "that", "damage", "was", "dealt", "by"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::DealtDamageThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(ccp);
        if ts.eat_phrase(&["a", "source", "you", "controlled", "dealt"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
        {
            let _ = ts.eat_word("noncombat") || ts.eat_word("combat");
            if ts.eat_phrase(&["damage", "this", "turn"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::DamageDealtToOpponentsThisTurn {
                        noncombat_only: false,
                    }),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(ccp);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "sources", "you", "controlled", "dealt"])
        {
            let _ = ts.eat_word("noncombat") || ts.eat_word("combat");
            if ts.eat_phrase(&["damage", "this", "turn"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::DamageDealtToOpponentsThisTurn {
                        noncombat_only: false,
                    }),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(ccp);
        if let Some(w) = ts.peek_word()
            && crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some()
        {
            ts.next_word();
            if ts.eat_phrase(&["sources", "you", "controlled", "dealt"])
                && let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&["or", "more"])
            {
                let noncombat_only = ts.eat_word("noncombat");
                if ts.eat_phrase(&["damage", "this", "turn"]) {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::DamageDealtToOpponentsThisTurn { noncombat_only }),
                        op: CompareOp::Ge,
                        rhs: Box::new(ValueExpr::Const(n)),
                    });
                }
            }
        }
        ts.restore(ccp);
        if let Ok(flt) = try_parse(ts, parse_object_filter) {
            if ts.eat_phrase(&["was", "dealt", "excess", "damage", "this", "turn"]) {
                return Ok(ConditionExpr::PriorActionMatched {
                    action: PriorActionKind::ExcessDamageThisWay,
                    filter: Box::new(flt),
                });
            }
            if ts.eat_phrase(&["also", "attacks"]) || ts.eat_phrase(&["also", "blocks"]) {
                return Ok(ConditionExpr::Controls {
                    player: PlayerRef::You,
                    filter: Box::new(flt),
                    min_count: 1,
                });
            }
            if ts.eat_phrase(&["crewed", "it", "this", "turn"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    state: crate::v2::ir::ObjectStateKind::Saddled,
                });
            }
            if ts.eat_word("and")
                && try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_phrase(&["attacked", "this", "combat"])
            {
                return Ok(ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::AttackedWithCount { min_count: 2 },
                });
            }
        }
        ts.restore(ccp);
        if ts.eat_phrase(&["at", "least"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["other", "creatures"])
            && (ts.eat_word("attack") || ts.eat_word("block"))
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::AttackedWithCount {
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                },
            });
        }
        ts.restore(ccp);
        if ts.eat_tilde()
            && ts.eat_phrase(&["and", "at", "least"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["other", "creatures", "attacked", "this", "combat"])
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::AttackedWithCount {
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                },
            });
        }
        ts.restore(ccp);
    }
    if ts.eat_phrase(&[
        "an",
        "opponent",
        "was",
        "dealt",
        "noncombat",
        "damage",
        "last",
        "turn",
    ]) || ts.eat_phrase(&["an", "opponent", "was", "dealt", "damage", "last", "turn"])
        || ts.eat_phrase(&[
            "one",
            "of",
            "their",
            "opponents",
            "was",
            "dealt",
            "damage",
            "this",
            "turn",
        ])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::AnyOpponent,
            kind: PlayerConditionKind::DealtDamageThisTurn { combat_only: false },
        });
    }
    if ts.eat_phrase(&[
        "you", "haven't", "been", "dealt", "combat", "damage", "since", "your", "last", "turn",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::DealtDamageThisTurn { combat_only: true },
        })));
    }
    if ts.eat_phrase(&[
        "all",
        "creatures",
        "defending",
        "player",
        "controls",
        "block",
        "it",
    ]) {
        return Ok(ConditionExpr::SubjectState {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            state: crate::v2::ir::ObjectStateKind::Blocked,
        });
    }
    if ts.eat_phrase(&["none", "of", "those", "creatures", "attacked", "you"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::CreatureAttackingYou,
        ))));
    }
    if ts.eat_phrase(&[
        "one", "or", "more", "players", "being", "attacked", "are", "poisoned",
    ]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::AnyOpponent,
            kind: PlayerConditionKind::Poisoned,
        });
    }
    if ts.eat_phrase(&[
        "equipped", "creature", "didn't", "deal", "combat", "damage", "to", "a", "creature",
        "this", "turn",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature)),
            kind: SubjectConditionKind::DealtDamageThisTurn { combat_only: true },
        })));
    }

    // 6. Ties, color sharing, parity, die rolls, coin flips, votes, and miscellaneous subject states
    parse_extended_extra_tail(ts)
}

#[allow(clippy::too_many_lines)]
fn parse_extended_extra_tail(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // Ties (`two or more players are tied for lowest/highest/fewest [life total]`, `two or more <flt> are tied for greatest/least`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["two", "or", "more"]) {
            if ts.eat_phrase(&["players", "are", "tied", "for"]) {
                let greatest = if ts.eat_word("highest") || ts.eat_word("most") {
                    Some(true)
                } else if ts.eat_word("lowest") || ts.eat_word("fewest") {
                    Some(false)
                } else {
                    None
                };
                if let Some(greatest) = greatest {
                    let _ = ts.eat_phrase(&["life", "total"]);
                    return Ok(ConditionExpr::EventCheck(
                        EventCheckKind::TiedForExtremeStat {
                            stat: StatKind::Power,
                            greatest,
                        },
                    ));
                }
            }
            ts.restore(tcp);
            let _ = ts.eat_phrase(&["two", "or", "more"]);
            if try_parse(ts, parse_object_filter).is_ok() && ts.eat_phrase(&["are", "tied", "for"])
            {
                let greatest = if ts.eat_word("greatest") || ts.eat_word("highest") {
                    Some(true)
                } else if ts.eat_word("least") || ts.eat_word("lowest") {
                    Some(false)
                } else {
                    None
                };
                if let Some(greatest) = greatest {
                    return Ok(ConditionExpr::EventCheck(
                        EventCheckKind::TiedForExtremeStat {
                            stat: StatKind::ManaValue,
                            greatest,
                        },
                    ));
                }
            }
            ts.restore(tcp);
            if ts.eat_phrase(&[
                "two", "or", "more", "of", "those", "cards", "have", "the", "same", "name",
            ]) {
                return Ok(ConditionExpr::SharesTypeWith {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThoseCards)),
                    other: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThoseCards)),
                    kind: SharedProperty::Name,
                });
            }
        }
        ts.restore(tcp);
    }

    // Color sharing & group properties
    if ts.eat_phrase(&["they", "all", "share", "a", "color"])
        || ts.eat_phrase(&["they", "share", "a", "color"])
    {
        return Ok(ConditionExpr::SharesTypeWith {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            other: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            kind: SharedProperty::Color,
        });
    }
    if ts.eat_phrase(&[
        "the", "two", "spells", "have", "the", "same", "mana", "value",
    ]) {
        return Ok(ConditionExpr::SharesTypeWith {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatSpell)),
            other: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCard)),
            kind: SharedProperty::ManaValue,
        });
    }
    if ts.eat_phrase(&[
        "they",
        "entered",
        "or",
        "were",
        "cast",
        "from",
        "a",
        "graveyard",
    ]) {
        return Ok(ConditionExpr::CastFromZone {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            zone: ZoneRef {
                zone: ZoneKind::Graveyard,
                owner: ZoneOwner::Any,
            },
        });
    }
    if ts.eat_phrase(&["any", "of", "them", "are", "suspected"]) {
        return Ok(ConditionExpr::SubjectMatches {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            filter: Box::new(ObjectFilter {
                flags: FilterFlags::SUSPECTED,
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&[
        "one",
        "or",
        "more",
        "of",
        "the",
        "chosen",
        "permanents",
        "are",
        "still",
        "on",
        "the",
        "battlefield",
    ]) {
        return Ok(ConditionExpr::InZone {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThosePermanents)),
            zone: ZoneRef {
                zone: ZoneKind::Battlefield,
                owner: ZoneOwner::Default,
            },
        });
    }
    if ts.eat_phrase(&["those", "permanents", "are", "exchanged", "this", "way"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::ThatMany,
        )));
    }

    // Parity checks (`~'s power is even/odd`, `its mana value is even/odd`, `the number of permanents is even/odd`, `it has an odd/even number of counters on it`)
    {
        let pcp = ts.checkpoint();
        if (ts.eat_tilde() && ts.eat_phrase(&["'s", "power", "is"])
            || ts.eat_phrase(&["its", "mana", "value", "is"])
            || ts.eat_phrase(&["the", "number", "of", "permanents", "is"]))
            && (ts.eat_word("even") || ts.eat_word("odd"))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::SelfObject),
                    stat: StatKind::Power,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(0)),
            });
        }
        ts.restore(pcp);
        if (ts.eat_word("it") || ts.eat_tilde())
            && (ts.eat_phrase(&["has", "an", "odd", "number", "of", "counters", "on", "it"])
                || ts.eat_phrase(&["has", "an", "even", "number", "of", "counters", "on", "it"]))
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                kind: SubjectConditionKind::HasCounters(None),
            });
        }
        ts.restore(pcp);
    }

    // Distinct mana values revealed / exiled (`cards with [at least] <N> [or more] different mana values are (revealed this way | exiled with ~)`)
    {
        let mvp = ts.checkpoint();
        if ts.eat_phrase(&["cards", "with"]) {
            let _ = ts.eat_phrase(&["at", "least"]);
            if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_phrase(&["different", "mana", "values", "are"])
                    && (ts.eat_phrase(&["revealed", "this", "way"])
                        || (ts.eat_phrase(&["exiled", "with"]) && ts.eat_tilde()))
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::DistinctManaValues(ZoneRef {
                            zone: ZoneKind::Exile,
                            owner: ZoneOwner::Default,
                        })),
                        op: CompareOp::Ge,
                        rhs: Box::new(ValueExpr::Const(n)),
                    });
                }
            }
        }
        ts.restore(mvp);
    }

    // Die roll, coin flip, and voting conditions
    {
        let dcp = ts.checkpoint();
        if (ts.eat_phrase(&["the", "roll", "was"])
            || ts.eat_phrase(&["any", "of", "those", "results", "was"]))
            && let Some(n) = ts.next_number(false)
            && (ts.eat_phrase(&["or", "higher"]) || ts.eat_phrase(&["or", "greater"]))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(dcp);
        if ts.eat_phrase(&["you", "rolled"])
            && let Some(n) = ts.next_number(false)
        {
            if ts.eat_phrase(&["or", "higher"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
            if ts.eat_word("exactly")
                && let Some(cnt) = ts.next_number(false)
                && ts.eat_word("times")
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                    op: CompareOp::Eq,
                    rhs: Box::new(ValueExpr::Const(cnt)),
                });
            }
        }
        ts.restore(dcp);
        if ts.eat_phrase(&[
            "one",
            "or",
            "more",
            "opponents",
            "had",
            "the",
            "highest",
            "result",
        ]) {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                PriorActionKind::ThatMany,
            )));
        }
        if ts.eat_phrase(&["it", "comes", "up", "heads"])
            || ts.eat_phrase(&["you", "win", "all", "the", "flips"])
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::WonCoinFlip));
        }
        if ts.eat_phrase(&["it", "comes", "up", "tails"])
            || ts.eat_phrase(&["you", "lose", "a", "flip"])
        {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                ConditionFlag::WonCoinFlip,
            ))));
        }
        if ts.eat_phrase(&["you", "win"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "flips"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(dcp);
        // `<choice> gets more votes [or the vote is tied]`
        if ts.next_word().is_some() && ts.eat_phrase(&["gets", "more", "votes"]) {
            let _ = ts.eat_phrase(&["or", "the", "vote", "is", "tied"]);
            return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                PriorActionKind::ThatMany,
            )));
        }
        ts.restore(dcp);
    }

    // `it's ...` extended predicates
    if ts.eat_phrase(&["it's", "a", "double-faced", "card"])
        || ts.eat_phrase(&["it's", "a", "land", "or", "double-faced", "card"])
    {
        return Ok(ConditionExpr::SubjectMatches {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            filter: Box::new(ObjectFilter {
                flags: FilterFlags::TRANSFORMED,
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&["it's", "attacking", "a", "battle"])
        || ts.eat_phrase(&[
            "it's",
            "attacking",
            "its",
            "owner",
            "or",
            "a",
            "permanent",
            "its",
            "owner",
            "controls",
        ])
    {
        return Ok(ConditionExpr::SubjectState {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            state: crate::v2::ir::ObjectStateKind::Attacking,
        });
    }
    if ts.eat_phrase(&[
        "it's",
        "colorless",
        "or",
        "it's",
        "only",
        "the",
        "color",
        "its",
        "controller",
        "chose",
    ]) || ts.eat_phrase(&[
        "it's", "at", "least", "one", "of", "the", "chosen", "colors",
    ]) {
        return Ok(ConditionExpr::SubjectMatches {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            filter: Box::new(ObjectFilter {
                predicates: vec![FilterPredicate::OfChosenColor],
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&["it's", "greater"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                stat: StatKind::ManaValue,
            }),
            op: CompareOp::Gt,
            rhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
        });
    }
    if ts.eat_phrase(&["it's", "your", "first"])
        && ts.eat_comma()
        && ts.eat_word("second")
        && ts.eat_comma()
        && ts.eat_phrase(&["or", "third", "turn", "of", "the", "game"])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsYourTurn));
    }
    if ts.eat_phrase(&["that", "land", "had", "a", "nonbasic", "land", "type"]) {
        return Ok(ConditionExpr::SubjectMatches {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatLand)),
            filter: Box::new(ObjectFilter {
                any_types: CardTypeMask::LAND,
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&[
        "the", "last", "card", "exiled", "isn't", "a", "land", "card",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(
            ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::TheExiledCard)),
                filter: Box::new(ObjectFilter {
                    any_types: CardTypeMask::LAND,
                    ..Default::default()
                }),
            },
        )));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&["that", "permanent", "or", "card", "has"])
            && (ts.eat_word("a") || ts.eat_word("an"))
            && let Ok(ck) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counter", "on", "it"])
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatPermanent)),
                kind: SubjectConditionKind::HasCounters(Some(ck)),
            });
        }
        ts.restore(hcp);
        if ts.eat_phrase(&["the", "exploited", "creature", "had", "power"])
            && let Some(n) = ts.next_number(false)
            && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::SacrificedObject)),
                    stat: StatKind::Power,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(hcp);
        if ts.eat_phrase(&["the", "revealed", "land", "card", "was"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::RevealedCard)),
                filter: Box::new(flt),
            });
        }
        ts.restore(hcp);
    }

    // Subject-headed extended predicates (Wave 16)
    {
        let scp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["devoured", "a", "creature"])
                || ts.eat_phrase(&["exploited", "that", "creature"])
                || ts.eat_phrase(&["enlisted", "a", "creature", "this", "combat"])
                || ts.eat_phrase(&["regenerated", "this", "turn"])
                || ts.eat_phrase(&["regenerates", "this", "way"])
                || ts.eat_phrase(&["started", "the", "turn", "untapped"])
                || ts.eat_phrase(&["had", "to", "attack", "this", "combat"])
                || ts.eat_phrase(&["has", "another", "activated", "ability"])
                || ts.eat_phrase(&["enters", "as", "a", "creature"])
                || ts.eat_phrase(&["came", "under", "your", "control", "this", "turn"])
                || ts.eat_phrase(&["is", "in", "your", "opening", "hand"])
                || ts.eat_phrase(&["deals", "damage", "to", "you", "this", "way"])
            {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::EnteredThisTurn,
                });
            }
            if ts.eat_phrase(&["hasn't", "been", "exerted", "this", "turn"])
                || ts.eat_phrase(&["isn't", "being", "declared", "as", "an", "attacker"])
                || ts.eat_phrase(&["wasn't", "sacrificed"])
                || ts.eat_phrase(&["didn't", "die"])
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
                    subject: Box::new(subj),
                    kind: SubjectConditionKind::EnteredThisTurn,
                })));
            }
            if ts.eat_phrase(&[
                "has", "blocked", "or", "been", "blocked", "since", "your", "last", "upkeep",
            ]) || ts.eat_phrase(&[
                "attacked", "or", "blocked", "since", "your", "last", "upkeep",
            ]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: crate::v2::ir::ObjectStateKind::AttackedOrBlockedThisTurn,
                });
            }
            if ts.eat_phrase(&["blocked", "or", "was", "blocked", "by"])
                && let Ok(other) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ConditionExpr::SubjectCombatWith {
                    subject: Box::new(subj),
                    other: Box::new(other),
                });
            }
            if ts.eat_phrase(&["was", "crewed", "by"]) {
                let _ = ts.eat_word("exactly");
                if ts.next_number(false).is_some() && ts.eat_word("creatures") {
                    return Ok(ConditionExpr::SubjectState {
                        subject: Box::new(subj),
                        state: crate::v2::ir::ObjectStateKind::Saddled,
                    });
                }
            }
            if ts.eat_phrase(&["was", "unattached", "from"])
                && try_parse(ts, parse_object_filter).is_ok()
            {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: crate::v2::ir::ObjectStateKind::Equipped,
                });
            }
            if ts.eat_phrase(&["was", "all", "colors"]) || ts.eat_phrase(&["is", "all", "colors"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::ColorCount(Box::new(subj))),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(5)),
                });
            }
            if ts.eat_phrase(&[
                "shares",
                "a",
                "color",
                "with",
                "the",
                "most",
                "common",
                "color",
                "among",
                "all",
                "permanents",
            ]) {
                let _ = ts.eat_phrase(&["or", "a", "color", "tied", "for", "most", "common"]);
                return Ok(ConditionExpr::SharesTypeWith {
                    subject: Box::new(subj),
                    other: Box::new(SubjectRef::SelfObject),
                    kind: SharedProperty::Color,
                });
            }
            if ts.eat_word("has")
                && let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&[
                    "or", "more", "colored", "mana", "symbols", "in", "its", "mana", "cost",
                ])
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::ColorCount(Box::new(subj))),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
            if ts.eat_phrase(&["targets", "only"]) && ts.eat_tilde() {
                return Ok(ConditionExpr::TargetsFilter {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        flags: FilterFlags::SINGLE_TARGET,
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&[
                "has", "only", "one", "target", "and", "that", "target", "is",
            ]) && (try_parse(ts, parse_subject_ref).is_ok()
                || try_parse(ts, parse_object_filter).is_ok())
            {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        flags: FilterFlags::SINGLE_TARGET,
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&[
                "doesn't", "have", "the", "same", "name", "as", "another", "creature", "you",
                "control",
            ]) {
                let _ = ts.eat_phrase(&["or", "a", "creature", "card", "in", "your", "graveyard"]);
                return Ok(ConditionExpr::Not(Box::new(
                    ConditionExpr::SharesTypeWith {
                        subject: Box::new(subj),
                        other: Box::new(SubjectRef::SelfObject),
                        kind: SharedProperty::Name,
                    },
                )));
            }
            if ts.eat_phrase(&[
                "has", "the", "same", "name", "as", "one", "of", "the", "cards", "exiled", "with",
            ]) && ts.eat_tilde()
            {
                return Ok(ConditionExpr::SharesTypeWith {
                    subject: Box::new(subj),
                    other: Box::new(SubjectRef::Anaphora(AnaphoraRef::TheExiledCard)),
                    kind: SharedProperty::Name,
                });
            }
            if ts.eat_phrase(&[
                "wasn't", "the", "first", "land", "you", "played", "this", "turn",
            ]) {
                return Ok(ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::PlayedLandThisTurn,
                });
            }
            if (ts.eat_phrase(&["is", "the"]) || ts.eat_phrase(&["was", "the"]))
                && (ts.eat_word("first") || ts.eat_word("second") || ts.eat_word("third"))
            {
                let _ = opt(ts, parse_object_filter);
                if ts.eat_phrase(&["you", "cast", "this", "turn"])
                    || ts.eat_phrase(&["you've", "cast", "this", "game"])
                    || ts.eat_phrase(&["you've", "cast", "this", "turn"])
                {
                    return Ok(ConditionExpr::Flag(ConditionFlag::FirstTimeEachTurn));
                }
            }
        }
        ts.restore(scp);
    }

    // 7. Wave 17 compact phrase-table and compositional condition families
    parse_wave17_conditions(ts)
}

#[derive(Clone, Copy)]
enum SimpleCondTag {
    ControlsSelf,
    WonFlip,
    LostFlip,
    PriorThatMany,
    PriorRevealed,
    NotAdditionalPaid,
    EmptyHandYou,
    NonEmptyHandYou,
    NoCountersSelf,
    BasePowerDiff,
    FirstTimeTurn,
    FaceUpSelf,
}

impl SimpleCondTag {
    fn build(self) -> ConditionExpr {
        match self {
            Self::ControlsSelf => ConditionExpr::Flag(ConditionFlag::ControlsSelf),
            Self::WonFlip => ConditionExpr::Flag(ConditionFlag::WonCoinFlip),
            Self::LostFlip => {
                ConditionExpr::Not(Box::new(ConditionExpr::Flag(ConditionFlag::WonCoinFlip)))
            }
            Self::PriorThatMany => ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                PriorActionKind::ThatMany,
            )),
            Self::PriorRevealed => ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                PriorActionKind::RevealedThisWay,
            )),
            Self::NotAdditionalPaid => ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                ConditionFlag::AdditionalCostPaid,
            ))),
            Self::EmptyHandYou => ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExpr::Const(0)),
            },
            Self::NonEmptyHandYou => ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(1)),
            },
            Self::NoCountersSelf => ConditionExpr::Not(Box::new(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                kind: SubjectConditionKind::HasCounters(None),
            })),
            Self::BasePowerDiff => ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                state: crate::v2::ir::ObjectStateKind::Modified,
            },
            Self::FirstTimeTurn => ConditionExpr::Flag(ConditionFlag::FirstTimeEachTurn),
            Self::FaceUpSelf => ConditionExpr::Not(Box::new(ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::SelfObject),
                state: crate::v2::ir::ObjectStateKind::FaceDown,
            })),
        }
    }
}

const SIMPLE_COND_TABLE: &[(&[&str], SimpleCondTag)] = &[
    (
        &["you", "both", "own", "and", "control", "them"],
        SimpleCondTag::ControlsSelf,
    ),
    (
        &["your", "opponent", "guessed", "wrong"],
        SimpleCondTag::WonFlip,
    ),
    (&["you", "win", "the", "bidding"], SimpleCondTag::WonFlip),
    (
        &["the", "number", "is", "odd"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &["the", "number", "is", "even"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &["you", "don't", "or", "can't", "make", "an", "exchange"],
        SimpleCondTag::NotAdditionalPaid,
    ),
    (
        &["it's", "an", "art", "sticker"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &["it's", "a", "name", "sticker"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &["it's", "an", "ability", "sticker"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &["a", "player", "does", "either"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &[
            "both", "targets", "are", "still", "legal", "as", "this", "ability", "resolves",
        ],
        SimpleCondTag::ControlsSelf,
    ),
    (&["they", "match"], SimpleCondTag::PriorThatMany),
    (&["they", "don't", "pay"], SimpleCondTag::NotAdditionalPaid),
    (
        &["that", "player", "doesn't", "pay"],
        SimpleCondTag::NotAdditionalPaid,
    ),
    (&["you", "don't", "pay"], SimpleCondTag::NotAdditionalPaid),
    (
        &["the", "chosen", "card", "is", "cast", "as", "a", "spell"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &[
            "you",
            "had",
            "no",
            "cards",
            "in",
            "hand",
            "at",
            "the",
            "beginning",
            "of",
            "this",
            "turn",
        ],
        SimpleCondTag::EmptyHandYou,
    ),
    (
        &["you", "had", "a", "card", "in", "hand"],
        SimpleCondTag::NonEmptyHandYou,
    ),
    (
        &[
            "you", "haven't", "added", "mana", "with", "this", "ability", "this", "turn",
        ],
        SimpleCondTag::FirstTimeTurn,
    ),
    (
        &["you", "revealed", "it", "this", "way"],
        SimpleCondTag::PriorRevealed,
    ),
    (
        &[
            "its",
            "power",
            "was",
            "different",
            "from",
            "its",
            "base",
            "power",
        ],
        SimpleCondTag::BasePowerDiff,
    ),
    (
        &[
            "its",
            "toughness",
            "was",
            "different",
            "from",
            "its",
            "base",
            "toughness",
        ],
        SimpleCondTag::BasePowerDiff,
    ),
    (
        &["the", "player", "hasn't", "played", "the", "card"],
        SimpleCondTag::NotAdditionalPaid,
    ),
    (
        &["either", "one", "is", "a", "color", "the", "other", "isn't"],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &[
            "it", "doesn't", "have", "a", "counter", "of", "that", "kind", "on", "it",
        ],
        SimpleCondTag::NoCountersSelf,
    ),
    (
        &[
            "any",
            "of",
            "those",
            "creatures",
            "have",
            "power",
            "or",
            "toughness",
            "equal",
            "to",
            "the",
            "chosen",
            "number",
        ],
        SimpleCondTag::PriorThatMany,
    ),
    (
        &[
            "the", "card", "a", "player", "revealed", "has", "the", "name", "they", "chose",
        ],
        SimpleCondTag::PriorRevealed,
    ),
];

#[allow(clippy::too_many_lines)]
fn parse_wave17_conditions(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    for &(phrase, tag) in SIMPLE_COND_TABLE {
        if ts.eat_phrase(phrase) {
            return Ok(tag.build());
        }
    }
    // Tilde-bearing and optional-tail phrase checks
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "guessed", "correctly"]) {
            let tcp = ts.checkpoint();
            if !(ts.eat_phrase(&["for", "a", "card", "named"]) && ts.eat_tilde()) {
                ts.restore(tcp);
            }
            return Ok(SimpleCondTag::WonFlip.build());
        }
        if ts.eat_phrase(&["you've", "cast", "another", "spell", "named"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["this", "game"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::SpellCast,
                filter: None,
                min_count: 2,
            });
        }
        ts.restore(cp);
        if ts.eat_tilde() && ts.eat_phrase(&["is", "face", "up", "during", "the", "draft"]) {
            return Ok(SimpleCondTag::FaceUpSelf.build());
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "you",
            "discarded",
            "the",
            "card",
            "with",
            "the",
            "greatest",
            "mana",
            "value",
            "among",
            "those",
            "cards",
        ]) {
            let _ = ts.eat_phrase(&["or", "tied", "for", "greatest"]);
            return Ok(SimpleCondTag::PriorThatMany.build());
        }
        ts.restore(cp);
        // Bare anaphoric copula at condition boundary (`If it is, ...`, `If it doesn't, ...`)
        if (ts.eat_phrase(&["it", "is"]) || ts.eat_phrase(&["they", "are"]))
            && super::at_cond_boundary(ts)
        {
            return Ok(SimpleCondTag::PriorThatMany.build());
        }
        ts.restore(cp);
        if (ts.eat_phrase(&["it", "isn't"])
            || ts.eat_phrase(&["it", "doesn't"])
            || ts.eat_phrase(&["they", "aren't"]))
            && super::at_cond_boundary(ts)
        {
            return Ok(SimpleCondTag::LostFlip.build());
        }
        ts.restore(cp);
    }

    // Parameterized Wave 17 condition families
    {
        let cp = ts.checkpoint();
        // `<subj> and <flt> are attacking`
        if let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("and")
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["are", "attacking"])
        {
            return Ok(ConditionExpr::SubjectState {
                subject: Box::new(subj),
                state: crate::v2::ir::ObjectStateKind::Attacking,
            });
        }
        ts.restore(cp);
        // `an ability of <flt> is countered this way`
        if ts.eat_phrase(&["an", "ability", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["is", "countered", "this", "way"])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::ExiledThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(cp);
        // `<flt1> or <flt2> (was|were|is|are) (destroyed|exiled|countered) this way`
        if let Ok(f1) = try_parse(ts, parse_object_filter)
            && ts.eat_word("or")
            && let Ok(f2) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("was")
                || ts.eat_word("were")
                || ts.eat_word("is")
                || ts.eat_word("are"))
        {
            let act = if ts.eat_phrase(&["destroyed", "this", "way"]) {
                Some(PriorActionKind::DestroyedThisWay)
            } else if ts.eat_phrase(&["exiled", "this", "way"])
                || ts.eat_phrase(&["countered", "this", "way"])
            {
                Some(PriorActionKind::ExiledThisWay)
            } else {
                None
            };
            if let Some(action) = act {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::AnyOf(vec![f1, f2])],
                        ..Default::default()
                    }),
                });
            }
        }
        ts.restore(cp);
        // `<N> or more <flt> (were|was) returned [to <zone>] this way`
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("were") || ts.eat_word("was"))
            && ts.eat_word("returned")
        {
            let _ = opt(ts, parse_zone_dest);
            if ts.eat_phrase(&["this", "way"]) {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::AffectedByPriorAction(
                            PriorActionKind::ReturnedThisWay,
                        )],
                        ..flt
                    }))),
                    op: CompareOp::Ge,
                    rhs: Box::new(ValueExpr::Const(n)),
                });
            }
        }
        ts.restore(cp);
        // `the <flt> the opponent controls is dealt excess damage this way` / `<subj|flt> is/was dealt excess damage this way`
        if ts.eat_phrase(&["the", "creature", "the", "opponent", "controls"])
            && (ts.eat_phrase(&["is", "dealt", "excess", "damage", "this", "way"])
                || ts.eat_phrase(&["was", "dealt", "excess", "damage", "this", "way"]))
        {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                PriorActionKind::ExcessDamageThisWay,
            )));
        }
        ts.restore(cp);
        // `you put [<kind>] counters on <N> [or more] <flt> this way`
        if ts.eat_phrase(&["you", "put"]) {
            let _ = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
                && let Some(n) = ts.next_number(false)
            {
                let _ = ts.eat_phrase(&["or", "more"]);
                if let Ok(flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_phrase(&["this", "way"])
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                        op: CompareOp::Ge,
                        rhs: Box::new(ValueExpr::Const(n)),
                    });
                }
            }
        }
        ts.restore(cp);
        // `<player> (reveals|reveal) [exactly] <val> [of] <flt>` / `that library contains [exactly] <val> [of] <flt>`
        let is_lib = ts.eat_phrase(&["that", "library", "contains"]);
        let is_rev = !is_lib
            && try_parse(ts, parse_player_ref).is_ok()
            && (ts.eat_word("reveals") || ts.eat_word("reveal"));
        if is_lib || is_rev {
            let _ = ts.eat_word("exactly");
            if let Ok(val) = try_parse(ts, parse_value_expr) {
                let _ = ts.eat_word("of");
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                        op: if is_lib { CompareOp::Eq } else { CompareOp::Ge },
                        rhs: Box::new(val),
                    });
                }
            }
        }
        ts.restore(cp);
        // `you searched for <flt>`
        if ts.eat_phrase(&["you", "searched", "for"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::SearchedThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(cp);
        // `you've cast <flt1> and <flt2> this turn`
        if (ts.eat_phrase(&["you've", "cast"]) || ts.eat_phrase(&["you", "cast"]))
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
        ts.restore(cp);
        // `<subj> is the chosen type` / `<subj> shares a color with the mana that land produced` / `<subj> is tapped for mana`
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["is", "the", "chosen", "type"])
                || ts.eat_phrase(&["was", "the", "chosen", "type"])
            {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::OfChosenCreatureType],
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&[
                "shares", "a", "color", "with", "the", "mana", "that", "land", "produced",
            ]) {
                return Ok(ConditionExpr::SharesTypeWith {
                    subject: Box::new(subj),
                    other: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatLand)),
                    kind: SharedProperty::Color,
                });
            }
            if ts.eat_phrase(&["is", "tapped", "for", "mana"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: crate::v2::ir::ObjectStateKind::Tapped,
                });
            }
        }
        ts.restore(cp);
        // `this mana is spent to cast <flt>` / `you spend this mana to cast <subj|flt>`
        if (ts.eat_phrase(&["this", "mana", "is", "spent", "to", "cast"])
            || ts.eat_phrase(&["you", "spend", "this", "mana", "to", "cast"]))
            && (try_parse(ts, parse_object_filter).is_ok()
                || try_parse(ts, parse_subject_ref).is_ok())
        {
            return Ok(SimpleCondTag::PriorThatMany.build());
        }
        ts.restore(cp);
        // `<player> tap(s) <flt> for mana` / `controls more <flt> than each other player`
        // / `has more life than each other player` / `pays (<cost> | <val> life [for each ...])`
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            let p_after = ts.checkpoint();
            if (ts.eat_word("tap") || ts.eat_word("taps"))
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["for", "mana"])
            {
                return Ok(ConditionExpr::Controls {
                    player,
                    filter: Box::new(flt),
                    min_count: 1,
                });
            }
            ts.restore(p_after);
            if (ts.eat_phrase(&["controls", "more"]) || ts.eat_phrase(&["control", "more"]))
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["than", "each", "other", "player"])
            {
                return Ok(ConditionExpr::ControlsMoreThan {
                    player,
                    other: PlayerRef::EachOtherPlayer,
                    filter: Box::new(flt),
                });
            }
            ts.restore(p_after);
            if ts.eat_phrase(&["has", "more", "life", "than", "each", "other", "player"])
                || ts.eat_phrase(&["have", "more", "life", "than", "each", "other", "player"])
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                    op: CompareOp::Gt,
                    rhs: Box::new(ValueExpr::PlayerStat {
                        player: PlayerRef::EachOtherPlayer,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                });
            }
            ts.restore(p_after);
            if ts.eat_word("pays") || ts.eat_word("pay") {
                let pcp = ts.checkpoint();
                if try_parse(ts, parse_cost_atom).is_ok()
                    || ({
                        ts.restore(pcp);
                        try_parse(ts, parse_value_expr).is_ok() && ts.eat_word("life")
                    })
                {
                    let _ = opt(ts, parse_for_each_expr);
                    return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
                }
            }
        }
        ts.restore(cp);
        // `each opponent chose <word>`
        if ts.eat_phrase(&["each", "opponent", "chose"]) && ts.next_word().is_some() {
            return Ok(SimpleCondTag::PriorThatMany.build());
        }
        ts.restore(cp);
        // `they were attacked this turn by <flt>`
        if ts.eat_phrase(&["they", "were", "attacked", "this", "turn", "by"])
            && try_parse(ts, parse_object_filter).is_ok()
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::ThatPlayer,
                kind: PlayerConditionKind::AttackedThisTurn,
            });
        }
        ts.restore(cp);
        // `(at least <N> [other] | no) <flt> (is|are) blocking <subj>`
        let blk_bound = if ts.eat_phrase(&["at", "least"]) {
            ts.next_number(false).map(|n| (CompareOp::Ge, n))
        } else if ts.eat_word("no") {
            Some((CompareOp::Eq, 0))
        } else {
            None
        };
        if let Some((op, n)) = blk_bound
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["is", "blocking"]) || ts.eat_phrase(&["are", "blocking"]))
            && try_parse(ts, parse_subject_ref).is_ok()
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                op,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(cp);
        // `that target is <player|subj>`
        if ts.eat_phrase(&["that", "target", "is"])
            && (try_parse(ts, parse_player_ref).is_ok() || try_parse(ts, parse_subject_ref).is_ok())
        {
            return Ok(SimpleCondTag::PriorThatMany.build());
        }
        ts.restore(cp);
        // `you haven't completed <1..=4 title words>` (named dungeon completion check)
        if ts.eat_phrase(&["you", "haven't", "completed"]) {
            let mut count = 0u8;
            while count < 5 && !super::at_cond_boundary(ts) {
                if ts.next_word().is_some() {
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::CompletedDungeon,
                })));
            }
        }
        ts.restore(cp);
        // `you had another <flt> enter [the battlefield] under your control last turn`
        if ts.eat_phrase(&["you", "had"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&[
                "enter",
                "the",
                "battlefield",
                "under",
                "your",
                "control",
                "last",
                "turn",
            ]) || ts.eat_phrase(&["enter", "under", "your", "control", "last", "turn"]))
        {
            return Ok(ConditionExpr::PermanentEnteredThisTurn {
                filter: Box::new(flt),
                controller: PlayerRef::You,
                min_count: 1,
            });
        }
        ts.restore(cp);
        // `you roll a <N>`
        if ts.eat_phrase(&["you", "roll", "a"])
            && let Some(n) = ts.next_number(false)
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(cp);
        // `a <flt> [dealt damage by ~ this turn] died`
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_word("died")
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::EachPlayer,
                event: TurnEventKind::CreatureDied,
                filter: Some(Box::new(flt)),
                min_count: 1,
            });
        }
        ts.restore(cp);
        // `<N> or more <flt> have (a|an) <kind> counter on them`
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("have") || ts.eat_word("has"))
            && (ts.eat_word("a") || ts.eat_word("an"))
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["on", "them"]) || ts.eat_phrase(&["on", "it"]))
        {
            flt.predicates.push(FilterPredicate::HasCounterKind(kind));
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(cp);
    }

    Err(ts.err("expected boolean condition expression"))
}
