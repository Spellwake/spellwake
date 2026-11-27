//! Strictly-typed recursive [`ConditionExpr`] parser for `v2`.

use crate::ast::{CompareOp, PlayerStatKind, StatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, EventCheckKind, FilterConditionKind,
    PlayerConditionKind, PlayerRef, PriorActionKind, SubjectConditionKind, SubjectRef,
    TurnEventKind, ValueExpr, ZoneMetricKind,
};
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed boolean [`ConditionExpr`].
mod player_subject;

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
        return Ok(ConditionExpr::Flag(ConditionFlag::IsFirstCombatPhase));
    }
    if ts.eat_phrase(&["it's", "your", "turn"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsYourTurn));
    }
    if ts.eat_phrase(&["it's", "night"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsNight));
    }
    if ts.eat_phrase(&["it's", "day"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsDay));
    }
    if ts.eat_phrase(&["it's", "not", "your", "turn"])
        || ts.eat_phrase(&["it's", "not", "their", "turn"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::IsYourTurn,
        ))));
    }
    if ts.eat_phrase(&["excess", "damage", "was", "dealt"]) {
        if ts.eat_word("to") {
            let _ = try_parse(ts, parse_subject_ref);
        }
        if ts.eat_phrase(&["this", "way"]) {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
                PriorActionKind::ExcessDamageThisWay,
            )));
        }
    }
    {
        let fcp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it")) && ts.eat_word("was") {
            if ts.eat_word("foretold") {
                return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                    crate::v2::ir::KeywordKind::Foretell,
                )));
            }
            if ts.eat_phrase(&["cast", "from", "anywhere", "other", "than", "your", "hand"])
                || ts.eat_phrase(&[
                    "cast", "from", "anywhere", "other", "than", "its", "owner's", "hand",
                ])
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                    ConditionFlag::WasCastFromHand,
                ))));
            }
        }
        ts.restore(fcp);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["your", "life", "total", "is"]) {
            if ts.eat_phrase(&["greater", "than", "your", "starting", "life", "total"]) {
                return Ok(ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::LifeAboveStarting { margin: 1 },
                });
            }
            if ts.eat_phrase(&[
                "less", "than", "or", "equal", "to", "half", "your", "starting", "life", "total",
            ]) {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: PlayerConditionKind::LifeAboveStarting { margin: 0 },
                })));
            }
        }
        ts.restore(lcp);
    }
    {
        let hcp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_phrase(&["has", "more", "cards", "in", "hand", "than", "you"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
            });
        }
        ts.restore(hcp);
    }
    {
        let icp = ts.checkpoint();
        if ts.eat_phrase(&["it", "isn't", "attacking", "or", "blocking"])
            || ts.eat_phrase(&["it's", "not", "attacking", "or", "blocking"])
        {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Or(
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    state: crate::v2::ir::ObjectStateKind::Attacking,
                }),
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    state: crate::v2::ir::ObjectStateKind::Blocking,
                }),
            ))));
        }
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "creatures", "are", "blocking"])
            && try_parse(ts, parse_subject_ref).is_ok()
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    combat: Some(crate::ast::CombatState::Blocking),
                    ..Default::default()
                }))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        ts.restore(icp);
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("is")
                && let Some((p, t)) = ts.next_base_pt()
            {
                return Ok(ConditionExpr::And(
                    Box::new(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::ObjectStat {
                            subject: Box::new(subj.clone()),
                            stat: StatKind::Power,
                        }),
                        op: CompareOp::Eq,
                        rhs: Box::new(ValueExpr::Const(p)),
                    }),
                    Box::new(ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::ObjectStat {
                            subject: Box::new(subj),
                            stat: StatKind::Toughness,
                        }),
                        op: CompareOp::Eq,
                        rhs: Box::new(ValueExpr::Const(t)),
                    }),
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
    if ts.eat_phrase(&["you", "have", "the", "city's", "blessing"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::HasCitysBlessing));
    }
    if ts.eat_phrase(&["you", "have", "the", "initiative"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::HasInitiative));
    }
    if ts.eat_phrase(&["you", "have", "an", "enduring", "story"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::HasEnduringStory));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&[
            "you", "have", "more", "cards", "in", "hand", "than", "each", "opponent",
        ]) || ts.eat_phrase(&[
            "you", "have", "more", "cards", "in", "hand", "than", "an", "opponent",
        ]) {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::EachOpponent,
                    stat: PlayerStatKind::CardsInHand,
                }),
            });
        }
        if ts.eat_phrase(&["you", "have", "fewer", "than"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["cards", "in", "hand"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op: CompareOp::Lt,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
        if ts.eat_phrase(&["you", "have", "exactly"])
            && let Some(n1) = ts.next_number(false)
        {
            let n2 = if ts.eat_word("or") {
                ts.next_number(false)
            } else {
                None
            };
            if ts.eat_phrase(&["cards", "in", "hand"]) || ts.eat_phrase(&["card", "in", "hand"]) {
                let c1 = ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::PlayerStat {
                        player: PlayerRef::You,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    op: CompareOp::Eq,
                    rhs: Box::new(ValueExpr::Const(n1)),
                };
                if let Some(m) = n2 {
                    let c2 = ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::PlayerStat {
                            player: PlayerRef::You,
                            stat: PlayerStatKind::CardsInHand,
                        }),
                        op: CompareOp::Eq,
                        rhs: Box::new(ValueExpr::Const(m)),
                    };
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
            && (ts.eat_phrase(&["onto", "the", "battlefield", "this", "way"])
                || ts.eat_phrase(&["into", "your", "hand", "this", "way"])
                || ts.eat_phrase(&["into", "your", "graveyard", "this", "way"]))
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::ReturnedThisWay,
                filter: Box::new(flt),
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
        if ts.eat_phrase(&["you", "revealed"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["or", "controlled"])
            && let Ok(flt2) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["as", "you", "cast"])
            && ts.eat_tilde()
        {
            return Ok(ConditionExpr::Or(
                Box::new(ConditionExpr::FilterCheck {
                    filter: Box::new(flt),
                    kind: FilterConditionKind::RevealedThisWay,
                    value: 1,
                }),
                Box::new(ConditionExpr::Controls {
                    player: PlayerRef::You,
                    filter: Box::new(flt2),
                    min_count: 1,
                }),
            ));
        }
        ts.restore(pcp);
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && (ts.eat_phrase(&["didn't", "cast"])
                || ts.eat_phrase(&["haven't", "cast"])
                || ts.eat_phrase(&["hasn't", "cast"]))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::Not(Box::new(
                ConditionExpr::PlayerTurnEvent {
                    player,
                    event: TurnEventKind::SpellCast,
                    filter: Some(Box::new(flt)),
                    min_count: 1,
                },
            )));
        }
        ts.restore(pcp);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "have", "at", "least"])
            && let Some(margin) = ts.next_number(false)
            && ts.eat_phrase(&["life", "more", "than", "your", "starting", "life", "total"])
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::LifeAboveStarting { margin },
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "an", "opponent", "has", "more", "cards", "in", "hand", "than", "you",
        ]) {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::AnyOpponent,
                    stat: PlayerStatKind::CardsInHand,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
            });
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
                value: u16::try_from(margin.max(1)).unwrap_or(1),
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["mana", "from"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
        {
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::ManaSpentFrom,
                value: 1,
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["its", "mana", "value"])
            && (ts.eat_word("was") || ts.eat_word("is"))
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            let op = if ts.eat_phrase(&["or", "less"]) {
                CompareOp::Le
            } else if ts.eat_phrase(&["or", "greater"]) {
                CompareOp::Ge
            } else {
                CompareOp::Eq
            };
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    stat: StatKind::ManaValue,
                }),
                op,
                rhs: Box::new(val),
            });
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
                value: u16::try_from(n.max(1)).unwrap_or(1),
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
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                ConditionFlag::PermanentLeftThisTurn,
            ))));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "opponent", "has", "more", "life", "than"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: p,
                kind: PlayerConditionKind::NoOpponentHasMoreLifeThan,
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "creatures", "are", "on", "the", "battlefield"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::NoCreaturesOnBattlefield));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "other", "creatures", "are", "attacking"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            return Ok(ConditionExpr::PlayerCheck {
                player: p,
                kind: PlayerConditionKind::NoOtherCreaturesAttacking,
            });
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["no", "other", "creature", "has", "greater", "power"]) {
            return Ok(ConditionExpr::Flag(
                ConditionFlag::NoOtherCreatureHasGreaterPower,
            ));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "you", "didn't", "attack", "with", "a", "creature", "this", "turn",
        ]) {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::AttackedThisTurn,
            })));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["you", "didn't", "play", "a", "land", "this", "turn"]) {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::PlayedLandThisTurn,
            })));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "you", "didn't", "play", "a", "card", "from", "exile", "this", "turn",
        ]) {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::PlayedFromExileThisTurn,
            })));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["you", "didn't", "lose", "life", "this", "turn"]) {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
                player: PlayerRef::You,
                kind: PlayerConditionKind::LostLifeThisTurn,
            })));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&["it's", "a", "spell", "with", "lesser", "mana", "value"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::LesserManaValueSpell));
        }
        ts.restore(lcp);
        // `~'s <keyword> cost was paid` / `~ was cast using <keyword>` / `<subj> was cast for its <keyword> cost`
        if ts.eat_tilde()
            && ts.eat_word("'s")
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
            && ts.eat_phrase(&["cost", "was", "paid"])
        {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(lcp);
        if (ts.eat_tilde()
            || ts.eat_phrase(&["that", "creature"])
            || ts.eat_phrase(&["this", "spell"])
            || ts.eat_word("it"))
            && (ts.eat_phrase(&["was", "cast", "for", "its"])
                || ts.eat_phrase(&["was", "cast", "using"]))
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
        {
            let _ = ts.eat_word("cost");
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(lcp);
        if (ts.eat_phrase(&["there's", "a"]) || ts.eat_phrase(&["there's", "an"]))
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
        if ts.eat_phrase(&["you", "sacrificed"]) || ts.eat_phrase(&["you've", "sacrificed"]) {
            let _ = ts.eat_word("a") || ts.eat_word("an");
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
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
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
            return Ok(ConditionExpr::PlayerCheck {
                player: p,
                kind: PlayerConditionKind::ControlsMostCreatures,
            });
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
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        any_types: crate::ast::CardTypeMask::INSTANT,
                        any_card: true,
                        ..Default::default()
                    }),
                    zone,
                    min_count: 1,
                }),
                Box::new(ConditionExpr::ZoneHasCount {
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        any_types: crate::ast::CardTypeMask::SORCERY,
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
            && (ts.eat_phrase(&["not", "named"]) && ts.eat_tilde() || true)
            && ts.eat_phrase(&["died", "this", "turn"])
        {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentDiedThisTurn));
        }
        ts.restore(lcp);
    }
    if ts.eat_phrase(&["you've", "been", "attacked", "this", "step"])
        || ts.eat_phrase(&["you", "were", "attacked", "this", "step"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::AttackedThisStep,
        });
    }
    if ts.eat_phrase(&["you", "attacked", "this", "turn"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::AttackedThisTurn,
        });
    }
    if ts.eat_phrase(&["you're", "the", "monarch"])
        || ts.eat_phrase(&["you", "are", "the", "monarch"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::IsMonarch,
        });
    }
    if ts.eat_phrase(&["you", "have", "a", "full", "party"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::HasFullParty,
        });
    }
    if ts.eat_phrase(&["you've", "completed", "a", "dungeon"])
        || ts.eat_phrase(&["you", "have", "completed", "a", "dungeon"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::CompletedDungeon,
        });
    }
    if ts.eat_phrase(&["you", "descended", "this", "turn"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::DescendedThisTurn,
        });
    }
    if ts.eat_phrase(&[
        "you", "haven't", "cast", "a", "spell", "from", "your", "hand", "this", "turn",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::CastSpellFromHandThisTurn,
        })));
    }
    if ts.eat_phrase(&["you", "control", "your", "commander"])
        || ts.eat_phrase(&["you", "control", "a", "commander"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::ControlsCommander,
        });
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
    if ts.eat_phrase(&[
        "this", "is", "the", "first", "time", "this", "ability", "has", "resolved", "this", "turn",
    ]) || ts.eat_phrase(&["it's", "the", "first", "time"])
    {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::ResolutionCountThisTurn(1),
        ));
    }
    if ts.eat_phrase(&[
        "this", "is", "the", "second", "time", "this", "ability", "has", "resolved", "this", "turn",
    ]) || ts.eat_phrase(&["it's", "the", "second", "time"])
    {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::ResolutionCountThisTurn(2),
        ));
    }
    if ts.eat_phrase(&[
        "this", "is", "the", "third", "time", "this", "ability", "has", "resolved", "this", "turn",
    ]) || ts.eat_phrase(&["it's", "the", "third", "time"])
    {
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
    }
    {
        let tcp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "cast", "using", "teamwork"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::CastUsingTeamwork));
        }
        ts.restore(tcp);
    }
    {
        let gcp = ts.checkpoint();
        if ts.eat_phrase(&["it", "shares", "a", "creature", "type", "with"])
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::SharesTypeWith {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                other: Box::new(other),
                kind: crate::v2::ir::SharedProperty::CreatureType,
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
    // `there are no <kind> counters on <subj>` / `there are <N> or more mana values among cards in <zone>`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                    subject: Box::new(subject),
                    kind,
                }),
                op: CompareOp::Eq,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(0)),
            });
        }
        ts.restore(ncp);
    }
    if ts.eat_phrase(&["you", "win", "the", "flip"]) || ts.eat_phrase(&["you", "win"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::WonCoinFlip));
    }
    if ts.eat_phrase(&["you", "lose", "the", "flip"]) || ts.eat_phrase(&["you", "lose"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::WonCoinFlip,
        ))));
    }
    if ts.eat_phrase(&["you", "search", "your", "library", "this", "way"])
        || ts.eat_phrase(&[
            "that", "player", "searches", "their", "library", "this", "way",
        ])
        || ts.eat_phrase(&["they", "search", "their", "library", "this", "way"])
    {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::SearchedThisWay,
        )));
    }
    if ts.eat_phrase(&["no", "spells", "were", "cast", "last", "turn"]) {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::SpellsCastLastTurn {
                op: CompareOp::Eq,
                count: 0,
            },
        ));
    }
    if ts.eat_phrase(&[
        "two", "or", "more", "spells", "were", "cast", "last", "turn",
    ]) || ts.eat_phrase(&[
        "a", "player", "cast", "two", "or", "more", "spells", "last", "turn",
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
                if ts.eat_phrase(&["mana", "was", "spent", "to", "cast"])
                    && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
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
                && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(ValueExpr::ManaPaidThisWay),
                    op: CompareOp::Ge,
                    rhs: Box::new(val),
                });
            }
        }
        ts.restore(mcp);
    }
    if ts.eat_phrase(&[
        "it", "wasn't", "cast", "or", "no", "mana", "was", "spent", "to", "cast", "it",
    ]) || ts.eat_phrase(&["no", "mana", "was", "spent", "to", "cast", "it"])
    {
        return Ok(ConditionExpr::SubjectCheck {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            kind: SubjectConditionKind::NotCastOrNoManaSpent,
        });
    }
    if ts.eat_phrase(&["an", "opponent", "is", "poisoned"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::AnyOpponent,
            kind: PlayerConditionKind::Poisoned,
        });
    }
    if ts.eat_phrase(&["a", "player", "is", "poisoned"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::AnyPlayer,
            kind: PlayerConditionKind::Poisoned,
        });
    }
    if ts.eat_phrase(&["you", "are", "poisoned"]) || ts.eat_phrase(&["you're", "poisoned"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::Poisoned,
        });
    }
    // `<N> or more <flt> entered [the battlefield] [under your control] this turn`
    {
        let ecp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["entered", "the", "battlefield"]) || ts.eat_word("entered"))
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
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                });
            }
        }
        ts.restore(ecp);
    }
    // `you don't/didn't put <subj> onto the battlefield / into your hand [this way]`
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["you", "don't", "put"]) || ts.eat_phrase(&["you", "didn't", "put"]))
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
        return Ok(ConditionExpr::Flag(ConditionFlag::GiftWasPromised));
    }
    if ts.eat_phrase(&["it", "was", "bargained"])
        || ts.eat_phrase(&["this", "spell", "was", "bargained"])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::WasBargained));
    }
    if ts.eat_phrase(&["no", "colored", "mana", "was", "spent", "to", "cast", "it"])
        || ts.eat_phrase(&[
            "no", "colored", "mana", "was", "spent", "to", "cast", "this", "spell",
        ])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::ManaSpentNoColor));
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "bargained"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::WasBargained));
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
            ]) || ts.eat_phrase(&[
                "put",
                "into",
                "your",
                "graveyard",
                "from",
                "anywhere",
                "this",
                "turn",
            ]) || ts.eat_phrase(&[
                "put",
                "into",
                "a",
                "graveyard",
                "from",
                "anywhere",
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
        if ts.eat_phrase(&["you", "cast", "it"])
            || (ts.eat_phrase(&["you", "cast"]) && ts.eat_tilde())
        {
            let _ = ts.eat_phrase(&["from", "your", "hand"])
                || ts.eat_phrase(&["during", "your", "main", "phase"]);
            return Ok(ConditionExpr::Flag(ConditionFlag::YouCastIt));
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
        return Ok(ConditionExpr::Flag(ConditionFlag::PermanentLeftThisTurn));
    }
    if ts.eat_phrase(&["that", "creature", "dies", "this", "way"])
        || ts.eat_phrase(&["that", "creature", "dies", "this", "turn"])
    {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::DestroyedThisWay,
        )));
    }
    if ts.eat_phrase(&["that", "spell", "is", "countered", "this", "way"]) {
        return Ok(ConditionExpr::EventCheck(EventCheckKind::PriorObjectEvent(
            PriorActionKind::ExiledThisWay,
        )));
    }
    if ts.eat_phrase(&["a", "creature", "died", "this", "turn"])
        || ts.eat_phrase(&[
            "a", "creature", "died", "under", "your", "control", "this", "turn",
        ])
        || ts.eat_phrase(&["a", "creature", "you", "controlled", "died", "this", "turn"])
    {
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
    // `{M} was spent to cast it/~` (`{U} was spent to cast it`, `{W}{U} was spent to cast ~`)
    {
        let mscp = ts.checkpoint();
        if let Some(mc) = ts.eat_mana_cost()
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
        {
            return Ok(ConditionExpr::EventCheck(EventCheckKind::ManaCostSpent(mc)));
        }
        ts.restore(mscp);
    }
    // `creatures you control have total power <N> or greater` (Formidable / Ferocious)
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&["creatures", "you", "control", "have", "total", "power"])
            && let Some(n) = ts.next_number(false)
            && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(crate::v2::ir::ValueExpr::AggregateStat {
                    op: crate::v2::ir::AggregateOp::Sum,
                    stat: crate::ast::StatKind::Power,
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        any_types: crate::ast::CardTypeMask::CREATURE,
                        controller: Some(crate::ast::ControllerRelation::You),
                        ..Default::default()
                    }),
                }),
                op: CompareOp::Ge,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
            });
        }
        ts.restore(fcp);
    }
    // `<N> or more <flt> are in <zone>` (`eight or more cards are in your graveyard`)
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
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(zcp);
    }
    // `<N> or more creatures are attacking` (`four or more creatures are attacking`)
    {
        let acp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["are", "attacking"])
        {
            flt.combat = Some(crate::ast::CombatState::Attacking);
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(crate::v2::ir::ValueExpr::Count(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
            });
        }
        ts.restore(acp);
    }
    // `<N> or more creatures died this turn` (`five or more creatures died this turn`)
    {
        let dcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && (ts.eat_word("creatures") || ts.eat_word("permanents"))
            && ts.eat_phrase(&["died", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::EachPlayer,
                event: TurnEventKind::CreatureDied,
                filter: None,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(dcp);
    }

    if ts.eat_phrase(&[
        "one",
        "or",
        "more",
        "cards",
        "left",
        "your",
        "graveyard",
        "this",
        "turn",
    ]) || ts.eat_phrase(&["a", "card", "left", "your", "graveyard", "this", "turn"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::CardsLeftGraveyardThisTurn,
        });
    }
    if ts.eat_phrase(&[
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
    ]) || ts.eat_phrase(&["a", "land", "you", "control", "entered", "this", "turn"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::LandEnteredThisTurn,
        });
    }
    // `<N> or more mana was spent to cast <subj>` (`Tellah`)
    {
        let m2cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "mana", "was", "spent", "to", "cast"])
            && try_parse(ts, parse_subject_ref).is_ok()
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::ManaPaidThisWay),
                op: CompareOp::Ge,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
            });
        }
        ts.restore(m2cp);
    }
    // `<N> or more damage was dealt to <subj> this turn` (`Burning-Eye Zubera`)
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
                    min_damage: u16::try_from(n.max(1)).unwrap_or(1),
                },
            });
        }
        ts.restore(dmg_cp);
    }
    // `there is a/an <flt> in <zone>` (`Wretched Camel`)
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
                return Ok(ConditionExpr::ZoneMetric {
                    zone,
                    kind: ZoneMetricKind::DistinctCardTypes,
                    min_count: u16::from(u8::try_from(min_count).unwrap_or(4)),
                });
            }
            if ts.eat_phrase(&["mana", "values", "among", "cards", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(crate::v2::ir::ValueExpr::DistinctManaValues(zone)),
                    op: CompareOp::Ge,
                    rhs: Box::new(crate::v2::ir::ValueExpr::Const(i32::from(min_count))),
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
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op,
                rhs: Box::new(val),
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

    player_subject::parse_player_or_subject_condition(ts)
}
