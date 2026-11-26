//! Compositional boolean condition (`ConditionAst`) parser covering game markers,
//! numeric comparisons, player/object status checks, and turn-history predicates.

use super::mana_token::split_named_card_list;
use super::object::parse_object_filter;
use super::subject::{parse_keyword_list, parse_player_ref, parse_subject_or_target};
use super::value::{parse_possessive_player, parse_value_expr};
use crate::ast::{
    CardTypeMask, ColorMask, CombatState, CombatSwapPerspective, CompareOp, ConditionAst,
    ControllerRelation, FilterPredicateAst, GameMarker, NameFilterAst, ObjectFilterAst,
    ObjectFilterFlags, PlayerRefAst, PlayerStatKind, PriorActionRefKind, Rounding, StatKind,
    SubjectRefAst, SupertypeMask, TargetCountAst, ValueExprAst, ZoneFilter,
};
use crate::combinators::trim_punct;
use crate::preprocess::is_physical_tabletop_clause;
use crate::vocab::{Vocabulary, lookup_color, parse_number_word};

fn parse_marker_and_compare_condition(low: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    match low {
        "~ is the first spell you've cast this game"
        | "it's the first spell you've cast this game" => {
            return Some(ConditionAst::Marker(GameMarker::FirstSpellCastThisGame));
        }
        "you've cast another spell named ~ this game" => {
            return Some(ConditionAst::Marker(GameMarker::CastAnotherNamedThisGame));
        }
        "one or more permanents were sacrificed to activate it"
        | "one or more permanents were sacrificed to activate this ability" => {
            return Some(ConditionAst::Marker(
                GameMarker::PermanentSacrificedToActivate,
            ));
        }
        "it exploited that creature" => {
            return Some(ConditionAst::Marker(GameMarker::ExploitedCreature));
        }
        "it regenerates this way" => {
            return Some(ConditionAst::Marker(GameMarker::SourceRegeneratedThisWay));
        }
        "they match" => {
            return Some(ConditionAst::Marker(GameMarker::HiddenValuesMatched));
        }
        "either one is a color the other isn't" => {
            return Some(ConditionAst::Marker(GameMarker::EitherHasColorOtherDoesnt));
        }
        "they're the monarch" | "that player is the monarch" => {
            return Some(ConditionAst::Marker(GameMarker::Monarch(
                PlayerRefAst::ThatPlayer,
            )));
        }
        "you didn't create a token this way" => {
            let tok = ObjectFilterAst {
                token: Some(true),
                ..Default::default()
            };
            return Some(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(tok),
                    action: "create".to_string(),
                },
            ))));
        }
        "you didn't draw cards this way" | "you didn't draw a card this way" => {
            return Some(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::CountDrawnThisWay(PlayerRefAst::You)),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::Const(0)),
            });
        }
        "you didn't put the revealed card onto the battlefield this way" => {
            return Some(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "battlefield".to_string(),
            }));
        }
        "a player is dealt damage this way" => {
            return Some(ConditionAst::Marker(GameMarker::DealtDamageThisWay {
                combat_only: false,
            }));
        }
        "a creature dealt damage by ~ this turn died" => {
            let mut cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            cr.flags = cr
                .flags
                .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
            return Some(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(cr),
                    action: "dies".to_string(),
                },
            ));
        }
        _ => {}
    }

    // "<subj> entered from <zone> or was cast from <zone>"
    if let Some((_, rest)) = low.split_once(" entered from ").and_then(|(l, r)| {
        r.split_once(" or was cast from ")
            .map(|(z1, z2)| ((l, z1), z2))
    }) {
        let zone = match rest.trim() {
            "exile" => Some(ZoneFilter::Exile),
            "your library" => Some(ZoneFilter::YourLibrary),
            "a library" => Some(ZoneFilter::AnyLibrary),
            "your graveyard" => Some(ZoneFilter::YourGraveyard),
            "a graveyard" => Some(ZoneFilter::AnyGraveyard),
            _ => None,
        };
        if let Some(z) = zone {
            return Some(ConditionAst::Marker(GameMarker::EnteredOrCastFromZone(z)));
        }
    }

    // "<player>'s speed is greater than each other player's speed"
    if let Some(who_raw) = low.strip_suffix("'s speed is greater than each other player's speed")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(ConditionAst::Marker(GameMarker::SpeedGreaterThanEachOther(
            player,
        )));
    }

    // "<player> reveals exactly the chosen number of cards of the chosen color"
    if let Some(who_raw) =
        low.strip_suffix(" reveals exactly the chosen number of cards of the chosen color")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(ConditionAst::Marker(
            GameMarker::RevealedExactCountAndColor { player },
        ));
    }

    // "<N> cards that share a card type were milled this way"
    if let Some(cnt_raw) = low.strip_suffix(" cards that share a card type were milled this way")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Marker(GameMarker::MilledCardsShareCardType {
            min_count: u8::try_from(n).unwrap_or(2),
        }));
    }

    // "exactly <N> colors of mana were spent to activate this ability"
    if let Some(after) = low.strip_prefix("exactly ")
        && let Some(cnt_raw) =
            after.strip_suffix(" colors of mana were spent to activate this ability")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Marker(
            GameMarker::ColorsSpentToActivateExact(u8::try_from(n).unwrap_or(3)),
        ));
    }

    // "<flt> under your control was destroyed this turn by a spell or ability an opponent controlled"
    if let Some(flt_raw) = low.strip_suffix(
        " under your control was destroyed this turn by a spell or ability an opponent controlled",
    ) && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(
            GameMarker::PermanentsDestroyedByOpponentThisTurn {
                filter: Box::new(flt),
            },
        ));
    }

    // "<subj> was attached to <flt>"
    if let Some((subj_raw, host_raw)) = low.split_once(" was attached to ")
        && let (Ok(subj), Ok(host_flt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_object_filter(host_raw.trim(), vocab),
        )
    {
        return Some(ConditionAst::Marker(GameMarker::WasAttachedTo {
            subject: Box::new(subj),
            host_filter: Box::new(host_flt),
        }));
    }

    // "the ring has tempted you <N> or more times this game"
    if let Some(after) = low.strip_prefix("the ring has tempted you ")
        && let Some(cnt_raw) = after.strip_suffix(" or more times this game")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Marker(
            GameMarker::RingTemptedMinTimesThisGame(u8::try_from(n).unwrap_or(2)),
        ));
    }

    // "<subj> is your ring-bearer"
    if let Some(subj_raw) = low.strip_suffix(" is your ring-bearer")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(GameMarker::SubjectIsRingBearer(
            Box::new(subj),
        )));
    }

    // "fewer than <N> cards were discarded this way"
    if let Some(after) = low.strip_prefix("fewer than ")
        && let Some(cnt_raw) = after.strip_suffix(" cards were discarded this way")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::ZoneActionCountThisWay {
                action: PriorActionRefKind::DiscardedThisWay,
                min_count: u16::try_from(n).unwrap_or(2),
            },
        ))));
    }

    // "you chose <flt> this way"
    if let Some(after) = low.strip_prefix("you chose ")
        && let Some(flt_raw) = after.strip_suffix(" this way")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(
            GameMarker::ZoneActionHappenedThisWay {
                filter: Box::new(flt),
                action: "choose".to_string(),
            },
        ));
    }

    // "<player> discards <flt> this way"
    if let Some((who_raw, rest)) = low.split_once(" discards ")
        && let Some(flt_raw) = rest.strip_suffix(" this way")
        && parse_player_ref(who_raw.trim()).is_some()
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(
            GameMarker::ZoneActionHappenedThisWay {
                filter: Box::new(flt),
                action: "discard".to_string(),
            },
        ));
    }

    // "<flt> in your graveyard has <kw>" (Kathril, Aspect Warper)
    if let Some((flt_raw, kw_raw)) = low.split_once(" in your graveyard has ")
        && let (Ok(mut flt), Some(kws)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_keyword_list(kw_raw.trim(), vocab),
        )
    {
        flt.zone = Some(ZoneFilter::YourGraveyard);
        flt.required_keywords.extend(kws);
        return Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "you had <N> or more <flt> enter the battlefield under your control this turn" (Park Heights Pegasus)
    if let Some(after) = low.strip_prefix("you had ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) =
            rest.strip_suffix(" enter the battlefield under your control this turn")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Some(ConditionAst::EnteredUnderYourControlThisTurn {
            filter: Box::new(flt),
            min_count: u16::try_from(n.max(1)).unwrap_or(1),
        });
    }

    // "<player> own(s) <N> or more <flt>" (Etrata, the Silencer)
    if let Some((who_raw, rest)) = low.split_once(" own ").or_else(|| low.split_once(" owns "))
        && let Some((cnt_raw, flt_raw)) = rest.split_once(" or more ")
        && let (Some(player), Some(n), Ok(mut flt)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        if flt.owner.is_none() {
            flt.owner = match player {
                PlayerRefAst::You => Some(ControllerRelation::OwnerYou),
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    Some(ControllerRelation::OwnerOpponent)
                }
                PlayerRefAst::TargetOpponent => Some(ControllerRelation::TargetOpponent),
                PlayerRefAst::TargetPlayer => Some(ControllerRelation::TargetPlayer),
                PlayerRefAst::ThatPlayer => Some(ControllerRelation::OwnerThatPlayer),
                PlayerRefAst::DefendingPlayer => Some(ControllerRelation::DefendingPlayer),
                PlayerRefAst::ActivePlayer => Some(ControllerRelation::ActivePlayer),
                PlayerRefAst::ChosenPlayer => Some(ControllerRelation::ChosenPlayer),
                _ => None,
            };
        }
        return Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(1)).unwrap_or(1),
        });
    }

    // "its mana value is equal to <val>" (Glissa Sunseeker)
    if let Some(val_raw) = low.strip_prefix("its mana value is equal to ")
        && let Ok(rhs) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                stat: StatKind::ManaValue,
            }),
            op: CompareOp::Eq,
            rhs: Box::new(rhs),
        });
    }

    // Compositional conjunction "<c1> and <c2>" when both sides are recognized conditions
    if !low.contains(" and/or ")
        && let Some((c1, c2)) = low.split_once(" and ")
        && !c1.trim().is_empty()
        && !c2.trim().is_empty()
        && let (Some(lhs), Some(rhs)) = (
            parse_marker_condition_atom(c1.trim(), vocab),
            parse_marker_condition_atom(c2.trim(), vocab),
        )
    {
        return Some(ConditionAst::And(Box::new(lhs), Box::new(rhs)));
    }

    None
}

fn parse_marker_condition_atom(low: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    if low == "~ was cast from your hand" || low == "you cast it from your hand" {
        return Some(ConditionAst::WasCast {
            by_you: true,
            from_hand: true,
        });
    }
    if !low.contains(" and ") {
        parse_marker_and_compare_condition(low, vocab)
    } else {
        None
    }
}

/// Strictly parse an intervening-`if`, resolution-`if`, or `as long as` condition.
fn starts_with_indefinite_article(s: &str) -> bool {
    s.starts_with("a ") || s.starts_with("an ") || s.starts_with("another ")
}

/// Split independent conjoined control requirements such as "a Plains and an Island"
/// or "an artifact and an enchantment" into `ConditionAst::And` of separate
/// `ConditionAst::ControlsCount` checks rather than merging them into one filter.
fn try_parse_conjoined_controls(
    player: PlayerRefAst,
    flt_raw: &str,
    vocab: &Vocabulary,
) -> Option<ConditionAst> {
    let trimmed = flt_raw.trim();
    if !starts_with_indefinite_article(trimmed) || trimmed.contains(" named ") {
        return None;
    }
    let (head, last) = trimmed
        .rsplit_once(", and ")
        .or_else(|| trimmed.rsplit_once(" and "))?;
    let last = last.trim();
    if !starts_with_indefinite_article(last) {
        return None;
    }
    let mut parts: Vec<&str> = if head.contains(", ") {
        head.split(", ").map(str::trim).collect()
    } else {
        vec![head.trim()]
    };
    parts.push(last);
    if parts.len() < 2
        || parts.iter().any(|part| {
            !starts_with_indefinite_article(part)
                || part.contains(" with ")
                || part.contains(" without ")
                || part.contains(" and ")
                || part.contains(" or ")
        })
    {
        return None;
    }
    let mut filters = Vec::with_capacity(parts.len());
    for part in parts {
        filters.push(parse_object_filter(part, vocab).ok()?);
    }
    let mut iter = filters
        .into_iter()
        .map(|filter| ConditionAst::ControlsCount {
            player,
            filter: Box::new(filter),
            op: CompareOp::Ge,
            value: 1,
        });
    let first = iter.next()?;
    Some(iter.fold(first, |acc, next| {
        ConditionAst::And(Box::new(acc), Box::new(next))
    }))
}

pub fn parse_condition(phrase: &str, vocab: &Vocabulary) -> Result<ConditionAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_condition".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = low_full
        .strip_suffix(" as you cast ~")
        .or_else(|| low_full.strip_suffix(" as you cast this spell"))
        .unwrap_or(&low_full)
        .to_string();

    // Wave E6.2: Compound "if <c1> and if <c2>" / "if <c1> or if <c2>" conditions
    if let Some((c1, c2)) = low.split_once(" and if ")
        && let (Ok(lhs), Ok(rhs)) = (parse_condition(c1, vocab), parse_condition(c2, vocab))
    {
        return Ok(ConditionAst::And(Box::new(lhs), Box::new(rhs)));
    }
    if let Some((c1, c2)) = low.split_once(" or if ")
        && let (Ok(lhs), Ok(rhs)) = (parse_condition(c1, vocab), parse_condition(c2, vocab))
    {
        return Ok(ConditionAst::Or(Box::new(lhs), Box::new(rhs)));
    }
    if let Some(val_raw) = low.strip_prefix("the result is equal to or less than ")
        && let Ok(rhs) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PriorResult),
            op: CompareOp::Le,
            rhs: Box::new(rhs),
        });
    }
    // Compositional "<subj>'s mana cost contains <sym>" / "its mana cost contains <sym>"
    if let Some((lhs_raw, sym_raw)) = low.split_once(" mana cost contains ")
        && sym_raw.trim().starts_with('{')
        && sym_raw.trim().ends_with('}')
    {
        let subj = match lhs_raw.trim() {
            "its" => Some(SubjectRefAst::Anaphora("it".to_string())),
            other => other
                .strip_suffix("'s")
                .and_then(|s| parse_subject_or_target(s.trim(), vocab).ok()),
        };
        if let Some(subject) = subj {
            let mut flt = ObjectFilterAst {
                any_card: true,
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::HasManaSymbolInCost(
                sym_raw.trim().to_ascii_uppercase(),
            ));
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(subject),
                filter: Box::new(flt),
            });
        }
    }

    // Wave E8 compositional typed conditions (GameMarker and Compare)
    if let Some(cond) = parse_marker_and_compare_condition(&low, vocab) {
        return Ok(cond);
    }

    match low.as_str() {
        "~'s additional cost was paid" => {
            return Ok(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: None,
            }));
        }
        "you revealed a dragon card or chose a dragon" => {
            return Ok(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: Some("revealed_or_chose_dragon".to_string()),
            }));
        }
        "you've been dealt damage by two or more creatures this turn" => {
            let cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(GameMarker::PlayerDealtDamageBy {
                player: PlayerRefAst::You,
                source_filter: Box::new(cr),
                min_sources: 2,
                combat_only: false,
                this_game: false,
            }));
        }
        "an opponent had three or more cards put into their graveyard from anywhere this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::PlayerHadCardsPutIntoGraveyardThisTurn {
                    player: PlayerRefAst::AnyOpponent,
                    min_count: 3,
                },
            ));
        }
        "it's the first instant spell, the first sorcery spell, or the first otter spell other than ~ you've cast this turn" =>
        {
            let inst = ObjectFilterAst {
                any_types: CardTypeMask::INSTANT,
                any_spell: true,
                ..Default::default()
            };
            let sorc = ObjectFilterAst {
                any_types: CardTypeMask::SORCERY,
                any_spell: true,
                ..Default::default()
            };
            let otter = ObjectFilterAst {
                subtypes: vec!["Otter".to_string()],
                any_spell: true,
                flags: ObjectFilterFlags::EXCLUDE_SELF,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::IsNthSpellMatchingThisTurn {
                    nth: 1,
                    filters: vec![inst, sorc, otter],
                },
            ));
        }
        "the top card of target player's graveyard is a creature card" => {
            let cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(GameMarker::TopCardOfZoneMatches {
                zone: ZoneFilter::TargetPlayerGraveyard,
                filter: Box::new(cr),
            }));
        }
        "you discarded the card with the greatest mana value among those cards or tied for greatest" =>
        {
            return Ok(ConditionAst::Marker(GameMarker::PlayerActionExtremeStat {
                player: PlayerRefAst::You,
                action: PriorActionRefKind::DiscardedThisWay,
                stat: StatKind::ManaValue,
                greatest: true,
                allow_tie: true,
            }));
        }
        "you've cycled a card named ~ four or more times this game" => {
            return Ok(ConditionAst::Marker(
                GameMarker::CycledCardNamedTimesThisGame {
                    name: NameFilterAst::SelfName,
                    min_times: 4,
                },
            ));
        }
        "you reveal three cards with different names" => {
            return Ok(ConditionAst::Marker(GameMarker::RevealedDistinctNames {
                min_count: 3,
            }));
        }
        "seven or more {e} was paid this way" => {
            return Ok(ConditionAst::Marker(GameMarker::EnergyPaidThisWayAtLeast(
                7,
            )));
        }
        "eight or more cards were returned to your hand this way" => {
            return Ok(ConditionAst::Marker(GameMarker::ZoneActionCountThisWay {
                action: PriorActionRefKind::ReturnedToHandThisWay,
                min_count: 8,
            }));
        }
        "there were three or more card types among the sacrificed permanents" => {
            let sac_perm = ObjectFilterAst {
                any_permanent: true,
                predicates: vec![FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::SacrificedThisWay,
                )],
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(GameMarker::DistinctCardTypesAmong {
                filter: Box::new(sac_perm),
                min_count: 3,
            }));
        }
        "the two spells have the same mana value" => {
            return Ok(ConditionAst::Marker(
                GameMarker::SameManaValueAsTriggerSpell,
            ));
        }
        "a white creature dies this way" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_colors: ColorMask::WHITE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "dies".to_string(),
                },
            ));
        }
        "a card is put into exile this way" => {
            let flt = ObjectFilterAst {
                any_card: true,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "exiled".to_string(),
                },
            ));
        }
        "a creature is put into exile this way" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "exiled".to_string(),
                },
            ));
        }
        "that creature was destroyed this way" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "destroyed".to_string(),
                },
            ));
        }
        "target attacking creature token is the chosen type" => {
            let subj_flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                combat: Some(CombatState::Attacking),
                token: Some(true),
                ..Default::default()
            };
            let chosen_flt = ObjectFilterAst {
                flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(subj_flt),
                    count: TargetCountAst::Exact(1),
                }),
                filter: Box::new(chosen_flt),
            });
        }
        "the revealed card is the chosen type" => {
            let chosen_flt = ObjectFilterAst {
                flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("the revealed card".to_string())),
                filter: Box::new(chosen_flt),
            });
        }
        "its controller has more than four cards in hand" => {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::ItsController,
                op: CompareOp::Gt,
                value: 4,
            });
        }
        "it doesn't have a counter of that kind on it" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::CounterCompare {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "same".to_string(),
                op: CompareOp::Ge,
                value: 1,
            })));
        }
        "it has the least power or is tied for least power among creatures on the battlefield" => {
            let all_cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                zone: Some(ZoneFilter::Battlefield),
                ..Default::default()
            };
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::Power,
                }),
                op: CompareOp::Le,
                rhs: Box::new(ValueExprAst::ExtremeStatAmong {
                    stat: StatKind::Power,
                    greatest: false,
                    filter: Box::new(all_cr),
                    or_toughness: false,
                }),
            });
        }
        "cards with five or more different mana values are exiled with ~" => {
            let exiled_flt = ObjectFilterAst {
                any_card: true,
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            };
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::DistinctCountAmong {
                    category: "mana_values".to_string(),
                    filter: Box::new(exiled_flt),
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(5)),
            });
        }
        "that player or that planeswalker's controller has more life than you" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::ThatPlayerOrPlaneswalkerController,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
            });
        }
        "the player hasn't played the card" => {
            return Ok(ConditionAst::Marker(GameMarker::HasNotPlayedOrCast));
        }
        "it's controlled by the chosen player" => {
            let flt = ObjectFilterAst {
                controller: Some(ControllerRelation::ChosenPlayer),
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(flt),
            });
        }
        "it targets you or a permanent you control" => {
            let your_perm = ObjectFilterAst {
                any_permanent: true,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            let flt = ObjectFilterAst {
                predicates: vec![FilterPredicateAst::TargetsSubject(Box::new(
                    SubjectRefAst::Compound(vec![
                        SubjectRefAst::Player(PlayerRefAst::You),
                        SubjectRefAst::Group(Box::new(your_perm)),
                    ]),
                ))],
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(flt),
            });
        }
        "it is" => {
            return Ok(ConditionAst::Marker(GameMarker::SameNameAsTarget));
        }
        "~ remains tapped and that creature's power remains less than or equal to ~'s power" => {
            return Ok(ConditionAst::And(
                Box::new(ConditionAst::Marker(GameMarker::RemainsTapped)),
                Box::new(ConditionAst::Compare {
                    lhs: Box::new(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                        stat: StatKind::Power,
                    }),
                    op: CompareOp::Le,
                    rhs: Box::new(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::SelfObject),
                        stat: StatKind::Power,
                    }),
                }),
            ));
        }
        "~ was kicked" | "it was kicked" => return Ok(ConditionAst::WasKicked),
        "you cast it" | "you cast ~" => {
            return Ok(ConditionAst::WasCast {
                by_you: true,
                from_hand: false,
            });
        }
        "~ was cast" | "it was cast" => {
            return Ok(ConditionAst::WasCast {
                by_you: false,
                from_hand: false,
            });
        }
        "you cast it from your hand" | "~ was cast from your hand" => {
            return Ok(ConditionAst::WasCast {
                by_you: true,
                from_hand: true,
            });
        }
        "no spells were cast last turn" => {
            return Ok(ConditionAst::SpellsCastLastTurn {
                any_player_single: false,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "a player cast two or more spells last turn" => {
            return Ok(ConditionAst::SpellsCastLastTurn {
                any_player_single: true,
                op: CompareOp::Ge,
                value: 2,
            });
        }
        "you search your library this way" => return Ok(ConditionAst::SearchedLibraryThisWay),
        "it's not a token"
        | "it isn't a token"
        | "that creature isn't a token"
        | "~ isn't a token" => {
            return Ok(ConditionAst::IsNotToken);
        }
        "a creature died this turn" => {
            return Ok(ConditionAst::Morbid {
                your_control_only: false,
            });
        }
        "a creature died under your control this turn" => {
            return Ok(ConditionAst::Morbid {
                your_control_only: true,
            });
        }
        "you have no cards in hand" | "you have no cards in your hand" => {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "it's your turn" | "it is your turn" => return Ok(ConditionAst::IsYourTurn),
        "it's not your turn" | "it isn't your turn" | "it's an opponent's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)));
        }
        "you gained life this turn" | "you've gained life this turn" => {
            return Ok(ConditionAst::GainedLifeThisTurn);
        }
        "an opponent lost life this turn" | "opponents lost life this turn" => {
            return Ok(ConditionAst::OpponentLostLifeThisTurn);
        }
        "you attacked this turn" | "you attacked with a creature this turn" => {
            return Ok(ConditionAst::AttackedThisTurn);
        }
        "~ entered this turn" | "~ entered the battlefield this turn" => {
            return Ok(ConditionAst::EnteredThisTurn);
        }
        "you control your commander"
        | "you control a commander"
        | "you control a commander as you cast ~" => {
            return Ok(ConditionAst::ControlsCommander);
        }
        "tribute wasn't paid" => return Ok(ConditionAst::TributeNotPaid),
        "tribute was paid" => return Ok(ConditionAst::Not(Box::new(ConditionAst::TributeNotPaid))),
        "it shares a creature type with ~" => return Ok(ConditionAst::SharesTypeWithSelf),
        "you descended this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DescendedThisTurn));
        }
        "you have the city's blessing" => {
            return Ok(ConditionAst::Marker(GameMarker::CitysBlessing));
        }
        "you're the monarch" | "you are the monarch" => {
            return Ok(ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)));
        }
        "defending player is the monarch" => {
            return Ok(ConditionAst::Marker(GameMarker::Monarch(
                PlayerRefAst::DefendingPlayer,
            )));
        }
        "there is no monarch" => return Ok(ConditionAst::Marker(GameMarker::NoMonarch)),
        "you've completed a dungeon" => {
            return Ok(ConditionAst::Marker(GameMarker::CompletedDungeon));
        }
        "it's day" => return Ok(ConditionAst::Marker(GameMarker::Day)),
        "it's night" => return Ok(ConditionAst::Marker(GameMarker::Night)),
        "it's neither day nor night" => {
            return Ok(ConditionAst::Marker(GameMarker::NeitherDayNorNight));
        }
        "you have max speed" => return Ok(ConditionAst::Marker(GameMarker::MaxSpeed)),
        "you have the initiative" => {
            return Ok(ConditionAst::Marker(GameMarker::Initiative(
                PlayerRefAst::You,
            )));
        }
        "you committed a crime this turn" | "you've committed a crime this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn));
        }
        "you gained or lost life this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::PlayerGainedOrLostLifeThisTurn {
                    player: PlayerRefAst::You,
                },
            ));
        }
        "a player lost life this turn" => {
            return Ok(ConditionAst::LostLifeThisTurn {
                player: PlayerRefAst::AnyPlayer,
                min_amount: 1,
            });
        }
        "an opponent was dealt damage this turn"
        | "an opponent has been dealt damage this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: false,
                },
            ));
        }
        "an opponent was dealt noncombat damage this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: true,
                },
            ));
        }
        "you attacked with two or more creatures this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedWithMinCreaturesThisTurn {
                    player: PlayerRefAst::You,
                    min_count: 2,
                },
            ));
        }
        "a creature entered the battlefield under your control this turn"
        | "a creature entered under your control this turn" => {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                min_count: 1,
            });
        }
        "you cast a spell this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value: 1,
                from_hand_only: false,
            }));
        }
        "you've cast two or more spells this turn" | "you cast two or more spells this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value: 2,
                from_hand_only: false,
            }));
        }
        "you haven't cast a spell from your hand this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
                from_hand_only: true,
            }));
        }
        "you've cast a historic spell this turn" => {
            return Ok(ConditionAst::CastSpellThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(ObjectFilterAst {
                    required_supertypes: SupertypeMask::HISTORIC,
                    any_spell: true,
                    ..Default::default()
                }),
            });
        }
        "this spell was cast from your graveyard"
        | "~ was cast from your graveyard"
        | "~ was cast from a graveyard"
        | "it was cast from a graveyard" => {
            return Ok(ConditionAst::Marker(GameMarker::CastFromGraveyard));
        }
        "~ was cast from exile" => return Ok(ConditionAst::Marker(GameMarker::CastFromExile)),
        "~ was foretold" | "it was foretold" => {
            return Ok(ConditionAst::Marker(GameMarker::Foretold));
        }
        "~ wasn't cast from your hand"
        | "~ was cast from anywhere other than your hand"
        | "you didn't cast it from your hand" => {
            return Ok(ConditionAst::Marker(GameMarker::CastFromOutsideHand));
        }
        "~ attacked or blocked this combat" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedOrBlockedThisCombat,
            ));
        }
        "an opponent owns a card in exile" => {
            return Ok(ConditionAst::Marker(GameMarker::OwnerHasCardInExile {
                player: PlayerRefAst::AnyOpponent,
            }));
        }
        "you own a card in exile" => {
            return Ok(ConditionAst::Marker(GameMarker::OwnerHasCardInExile {
                player: PlayerRefAst::You,
            }));
        }
        "defending player is poisoned" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
                player: PlayerRefAst::DefendingPlayer,
            }));
        }
        "its controller is poisoned" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
                player: PlayerRefAst::ItsController,
            }));
        }
        "defending player has a rad counter" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerHasRadCounter {
                player: PlayerRefAst::DefendingPlayer,
            }));
        }
        "there are no cards in your graveyard" => {
            return Ok(ConditionAst::GraveyardCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "you control neither creature" => {
            return Ok(ConditionAst::Marker(GameMarker::ControlsNeither));
        }
        "you control all three" => {
            return Ok(ConditionAst::Marker(GameMarker::ControlsAllThree));
        }
        "you control fewer creatures than an opponent" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsFewerThanOpponent {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                },
            ));
        }
        "you control more creatures than each other player"
        | "you control more creatures than that spell's controller" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                    than_each_other: true,
                },
            ));
        }
        "you control more creatures of that type than each other player" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                        ..Default::default()
                    }),
                    than_each_other: true,
                },
            ));
        }
        "you control more lands than defending player" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::LAND,
                        ..Default::default()
                    }),
                    than_each_other: false,
                },
            ));
        }
        "you bargained" | "~ was bargained" | "it was bargained" => {
            return Ok(ConditionAst::Marker(GameMarker::Bargained));
        }
        "the gift was promised" => return Ok(ConditionAst::Marker(GameMarker::GiftPromised)),
        "you're attacking" => return Ok(ConditionAst::Marker(GameMarker::YouAreAttacking)),
        "you're defending" => return Ok(ConditionAst::Marker(GameMarker::YouAreDefending)),
        "two or more players have lost the game" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayersLostGame {
                op: CompareOp::Ge,
                value: 2,
            }));
        }
        "you don't put the card into your hand"
        | "you didn't put a card into your hand this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "hand".to_string(),
            }));
        }
        "you don't put the card onto the battlefield"
        | "you didn't put a card onto the battlefield this way"
        | "you don't put it onto the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "battlefield".to_string(),
            }));
        }
        "a nonland permanent left the battlefield this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::NonlandPermanentLeftThisTurn,
            ));
        }
        "a spell was warped this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::SpellWarpedThisTurn));
        }
        "~ was cast using teamwork" | "it's cast using teamwork" => {
            return Ok(ConditionAst::Marker(GameMarker::CastUsingTeamwork));
        }
        "chaos gets more votes or the vote is tied" => {
            return Ok(ConditionAst::Marker(GameMarker::VoteResult {
                winning: "chaos_or_tie".to_string(),
            }));
        }
        "planeswalk gets more votes" => {
            return Ok(ConditionAst::Marker(GameMarker::VoteResult {
                winning: "planeswalk".to_string(),
            }));
        }
        "you have a full party" => return Ok(ConditionAst::Marker(GameMarker::FullParty)),
        "it's the first combat phase of the turn" => {
            return Ok(ConditionAst::Marker(GameMarker::FirstCombatPhase));
        }
        "it isn't a mana ability" => return Ok(ConditionAst::Marker(GameMarker::NotManaAbility)),
        "it's not their turn" | "it isn't that player's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsPlayersTurn(
                PlayerRefAst::ThatPlayer,
            ))));
        }
        "it's on the battlefield" | "it is on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                zone: ZoneFilter::Battlefield,
            }));
        }
        "~ is in the command zone or on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::InCommandZoneOrBattlefield));
        }
        "~ is suspended" => return Ok(ConditionAst::Marker(GameMarker::IsSuspended)),
        "~ is paired with another creature" => {
            return Ok(ConditionAst::Marker(GameMarker::PairedWithAnotherCreature));
        }
        "no creatures are on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::NoCreaturesOnBattlefield));
        }
        "it has a single target" => return Ok(ConditionAst::Marker(GameMarker::SingleTarget)),
        "it isn't being declared as an attacker" => {
            return Ok(ConditionAst::Marker(GameMarker::NotDeclaredAttacker));
        }
        "you chose a creature other than ~ as your ring-bearer" => {
            return Ok(ConditionAst::Marker(GameMarker::RingBearerOtherThanSelf));
        }
        "a card left your graveyard this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn));
        }
        "you have more cards in hand than each opponent" => {
            return Ok(ConditionAst::Marker(GameMarker::HandMoreThanEachOpponent));
        }
        "your life total is less than or equal to half your starting life total" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Le,
                rhs: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::You)),
                    denom: 2,
                    rounding: Rounding::Down,
                }),
            });
        }
        "an opponent's life total is less than half their starting life total" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Lt,
                rhs: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::AnyOpponent)),
                    denom: 2,
                    rounding: Rounding::Down,
                }),
            });
        }
        "it entered from your graveyard or you cast it from your graveyard" => {
            return Ok(ConditionAst::Marker(GameMarker::EnteredOrCastFromGraveyard));
        }
        "it shares a card type with the exiled card" => {
            return Ok(ConditionAst::Marker(GameMarker::SharesCardTypeWithExiled));
        }
        "it targets a commander" => {
            return Ok(ConditionAst::Marker(GameMarker::TargetsCommander));
        }
        "it has the same mana value as the discarded card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameManaValueAsDiscarded));
        }
        "it has the same name as that card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameNameAsTarget));
        }
        "its mana value is x" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::ManaValue,
                }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::ChosenX),
            });
        }
        "you control ~" => return Ok(ConditionAst::Marker(GameMarker::ControlsSelf)),
        "~ remains tapped" | "it remains tapped" => {
            return Ok(ConditionAst::Marker(GameMarker::RemainsTapped));
        }
        "~ remains on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::RemainsOnBattlefield));
        }
        "it's attacking alone" => return Ok(ConditionAst::Marker(GameMarker::AttackingAlone)),
        "damage is prevented this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DamagePreventedThisWay));
        }
        "you haven't cast it"
        | "you haven't played it"
        | "you don't cast it"
        | "you don't play it"
        | "you didn't cast it"
        | "it wasn't cast" => {
            return Ok(ConditionAst::Marker(GameMarker::HasNotPlayedOrCast));
        }
        "it's bargained" | "~ is bargained" => {
            return Ok(ConditionAst::Marker(GameMarker::Bargained));
        }
        "it's attacking the player with the most life or tied for most life" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackingPlayerWithMostLife { or_tied: true },
            ));
        }
        "it's attacking the player with the most life" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackingPlayerWithMostLife { or_tied: false },
            ));
        }
        "you have more life than an opponent" | "you have more life than each opponent" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent,
                    stat: PlayerStatKind::LifeTotal,
                }),
            });
        }
        "no creatures attacked this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::DidNotAttackWithCreatureThisTurn {
                    player: PlayerRefAst::AnyPlayer,
                },
            ));
        }
        "no mana was spent to cast that spell" | "no mana was spent to cast it" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ManaSpentToCast { mana_filter: None }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::Const(0)),
            });
        }
        "you won" | "you won the clash" | "you win the flip" | "you won the flip" => {
            return Ok(ConditionAst::WonContest);
        }
        "it didn't die" | "that creature didn't die" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::DiedThisWay,
            ))));
        }
        "the gift wasn't promised" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::GiftPromised,
            ))));
        }
        "it's paired with a creature" | "~ is paired with a creature" => {
            return Ok(ConditionAst::Marker(GameMarker::PairedWithAnotherCreature));
        }
        "a permanent's ability is countered this way" => {
            return Ok(ConditionAst::Marker(GameMarker::AbilityCounteredThisWay {
                permanent_only: true,
            }));
        }
        "~ attacked this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
            }));
        }
        "you've discarded a card this turn" | "you discarded a card this turn" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: "discard".to_string(),
            });
        }
        "you haven't added mana with this ability this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn {
                op: CompareOp::Eq,
                value: 0,
            }));
        }
        "it's not that player's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsPlayersTurn(
                PlayerRefAst::ThatPlayer,
            ))));
        }
        "that spell targets only ~" | "it targets only ~" => {
            return Ok(ConditionAst::Marker(GameMarker::SingleTarget));
        }
        "that card remains exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                any_of: false,
            }));
        }
        "those cards remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("those cards".to_string())),
                any_of: false,
            }));
        }
        "any of those cards remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("those cards".to_string())),
                any_of: true,
            }));
        }
        "it remains exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                any_of: false,
            }));
        }
        "they remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("they".to_string())),
                any_of: false,
            }));
        }
        "it attacked this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            }));
        }
        "it attacked or blocked this turn" | "that creature attacked or blocked this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedOrBlockedThisTurn {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                },
            ));
        }
        "that creature dies this way"
        | "it dies this way"
        | "it died this way"
        | "a creature died this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DiedThisWay));
        }
        "~ didn't attack or come under your control this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
                or_entered_this_turn: true,
            }));
        }
        "~ didn't attack this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
                or_entered_this_turn: false,
            }));
        }
        "one or more cards left your graveyard this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn));
        }
        "one or more of them entered from a graveyard or was cast from a graveyard" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AnyEnteredOrCastFromGraveyard,
            ));
        }
        "life was paid to activate it" => {
            return Ok(ConditionAst::Marker(GameMarker::LifePaidToActivate));
        }
        "that player has another opponent who isn't being attacked" => {
            return Ok(ConditionAst::Marker(GameMarker::HasUnattackedOpponent));
        }
        "it has greater power or toughness than ~" => {
            return Ok(ConditionAst::Marker(
                GameMarker::GreaterPowerOrToughnessThan {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    other: Box::new(SubjectRefAst::SelfObject),
                },
            ));
        }
        "this is the first time this ability has resolved this turn" | "it's the first time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(1),
            ));
        }
        "this is the second time this ability has resolved this turn" | "it's the second time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(2),
            ));
        }
        "this is the third time this ability has resolved this turn" | "it's the third time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(3),
            ));
        }
        "you cast ~ during your main phase" | "you cast it during your main phase" => {
            return Ok(ConditionAst::Marker(GameMarker::CastDuringMainPhase));
        }
        "it escaped" | "~ escaped" => {
            return Ok(ConditionAst::Marker(GameMarker::Escaped));
        }
        "it has the same mana value as the revealed card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameManaValueAsRevealed));
        }
        "evidence was collected" => {
            return Ok(ConditionAst::Marker(GameMarker::EvidenceCollected));
        }
        "you lost life last turn" => {
            return Ok(ConditionAst::Marker(GameMarker::LostLifeLastTurn {
                player: PlayerRefAst::You,
            }));
        }
        "~ attacked this combat" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisCombat {
                subject: Box::new(SubjectRefAst::SelfObject),
            }));
        }
        "the amount of mana spent to cast it was less than its mana value"
        | "the amount of mana spent to cast ~ was less than its mana value" => {
            return Ok(ConditionAst::Marker(GameMarker::ManaSpentLessThanManaValue));
        }
        "they entered or were cast from a graveyard" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AnyEnteredOrCastFromGraveyard,
            ));
        }
        "you didn't attack with a creature this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::DidNotAttackWithCreatureThisTurn {
                    player: PlayerRefAst::You,
                },
            ));
        }
        "you didn't cast a spell this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
                player: PlayerRefAst::You,
                filter: None,
            }));
        }
        "an opponent has more life than you" => {
            return Ok(ConditionAst::OpponentMoreThanYou {
                metric: "life".to_string(),
                filter: None,
            });
        }
        "an opponent has more cards in hand than you" => {
            return Ok(ConditionAst::OpponentMoreThanYou {
                metric: "cards_in_hand".to_string(),
                filter: None,
            });
        }
        "~ is on the stack" => {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: "on_stack".to_string(),
            });
        }
        "~ didn't enter the battlefield this turn" | "~ didn't enter this turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::EnteredThisTurn)));
        }
        "a creature is attacking you" => {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::EachOpponent,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            });
        }
        "a player attacked you during their last turn"
        | "that player attacked you during their last turn" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::ThatPlayer,
                action: "attacked_you_last_turn".to_string(),
            });
        }
        "an opponent was dealt noncombat damage last turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: true,
                },
            ));
        }
        "excess damage was dealt this way"
        | "it's greater"
        | "that spell is countered this way"
        | "you have an enduring story"
        | "that creature attacked this turn"
        | "it's a spell with lesser mana value"
        | "the amount of mana spent to cast that spell was less than its mana value"
        | "a card is exiled with it"
        | "a card is exiled with ~"
        | "it was cast from your graveyard"
        | "they were cast using web-slinging"
        | "~ was cast using web-slinging"
        | "you have a card in hand"
        | "it was the second spell you cast this turn"
        | "it was blocked this turn"
        | "~ was dealt damage this turn"
        | "it was dealt damage this turn"
        | "it dealt combat damage to a player this turn"
        | "an opponent lost life last turn"
        | "they aren't attacking you"
        | "that player is you"
        | "it entered under your control"
        | "that cost wasn't paid"
        | "the copy is a permanent spell"
        | "it's your first, second, or third turn of the game"
        | "an opponent is poisoned"
        | "you've activated a loyalty ability this turn"
        | "~ attacked or blocked this turn"
        | "it was attacking or blocking alone"
        | "an aura you controlled was attached to it"
        | "you didn't play a land this turn"
        | "you didn't lose life this turn"
        | "you didn't play a card from exile this turn"
        | "you put a counter on ~ this turn"
        | "a counter was put on ~ this turn"
        | "you put a counter on a creature this turn"
        | "~ dealt damage to an opponent this turn"
        | "~ dealt damage to it this turn"
        | "no creatures died this turn"
        | "you attacked with exactly one other creature this combat"
        | "it's the second creature spell you cast this turn"
        | "~ is in your graveyard or on the battlefield"
        | "you're the defending player"
        | "enchanted equipment is attached to a creature"
        | "it didn't attack this turn"
        | "an opponent searched their library this turn"
        | "three or more creatures attacked this turn"
        | "there are five colors among permanents you control"
        | "it devoured a creature"
        | "it hasn't dealt damage yet"
        | "your team gained life this turn"
        | "you've drawn more than one card this turn"
        | "one or more cards were put into exile this turn"
        | "no permanents left the battlefield this turn"
        | "a creature died under an opponent's control this turn"
        | "a creature card left your graveyard this turn"
        | "its power was different from its base power"
        | "that creature was dealt excess damage this turn"
        | "you haven't been dealt combat damage since your last turn"
        | "no other creatures are attacking that player"
        | "you were the monarch as the turn began"
        | "they attacked you and/or a planeswalker you control"
        | "it has the same name as one of the cards exiled with ~"
        | "~ is in your graveyard with a creature card directly above it"
        | "another creature died this turn"
        | "no cards are in that graveyard"
        | "you have less life than an opponent"
        | "its mana value is odd"
        | "its mana value is even"
        | "an opponent protects it"
        | "~ is your ring-bearer"
        | "enchanted creature is attacking alone"
        | "you've visited an attraction this turn"
        | "~ hasn't dealt damage yet"
        | "you cast it from anywhere other than your hand"
        | "that creature has greater power or toughness than ~"
        | "that player controls more lands than each other player"
        | "you removed two or more loyalty counters to activate it"
        | "you don't draw a card this way"
        | "that card has the chosen name"
        | "that land was nonbasic"
        | "exactly one creature is attacking"
        | "a land you controlled was put into a graveyard from the battlefield this turn"
        | "you or a player you're attacking has the initiative"
        | "two or more of those creatures are attacking you and/or planeswalkers you control"
        | "it enlisted a creature this combat"
        | "that player had another land enter the battlefield under their control this turn"
        | "excess damage was dealt to that creature this way"
        | "excess damage was dealt to a creature this way"
        | "excess damage was dealt to that permanent this way"
        | "the creature the opponent controls is dealt excess damage this way"
        | "~ attacked a battle this turn"
        | "it attacked during its controller's last turn"
        | "it attacked during your last turn"
        | "~'s power is even"
        | "~'s power is odd"
        | "that artifact is put into a graveyard this way"
        | "that creature is put into a graveyard this way"
        | "~ is still on the battlefield"
        | "they control two or more basic lands"
        | "they control a commander"
        | "an opponent has been dealt noncombat damage this turn"
        | "you've scried or surveilled this turn"
        | "~ is on the battlefield or in your graveyard"
        | "an opponent was dealt combat damage by a legendary creature this turn"
        | "it's a land or double-faced card"
        | "three or more creature cards are above ~"
        | "you've paid or lost four or more {e} this turn"
        | "red sources you controlled dealt 4 or more noncombat damage this turn"
        | "~ blocked or was blocked by a blue creature this turn"
        | "you've rolled three or more dice this turn"
        | "you've been attacked this step"
        | "you're attacking two or more opponents"
        | "at least one creature is blocking ~"
        | "ten or more lands are on the battlefield"
        | "there are twenty-five or more loyalty counters among jaces you control"
        | "mana from a treasure was spent to cast it or activate it"
        | "two or more equipment are attached to it"
        | "an enchantment was put into your graveyard from the battlefield this turn"
        | "~ regenerated this turn"
        | "it shares a card type with the card exiled this way"
        | "no player does"
        | "the player doesn't"
        | "you win"
        | "you paid one or more {e} this way"
        | "one or more +1/+1 counters are moved this way"
        | "two cards that share a card type are discarded this way"
        | "that creature was cast for its warp cost"
        | "that card is returned to its owner's hand this way"
        | "you gain control of a creature this way"
        | "a creature spell you cast this turn was countered by a spell or ability an opponent controlled"
        | "a player other than you lost life this turn"
        | "an enchantment is on the battlefield"
        | "excess damage was dealt to a permanent this way"
        | "~ is enchanted, equipped, and has a counter on it"
        | "this is the first or second time this ability has resolved this turn"
        | "they control fewer lands than you"
        | "the discovered card's mana value is less than 10"
        | "that creature shares a color with the mana that land produced"
        | "that spell targets a commander you control"
        | "you control two or more nonland, nontoken permanents with the same name as one another"
        | "your library has no cards in it"
        | "you control more creatures than attacking player"
        | "you control more lands than attacking player" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: low,
            });
        }
        "they're not a token" | "they aren't a token" => {
            return Ok(ConditionAst::IsNotToken);
        }
        "~ is in the command zone" => {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: "in_command_zone".to_string(),
            });
        }
        "it wasn't cast this way" | "you don't cast that card this way" => {
            return Ok(ConditionAst::Not(Box::new(
                ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: "cast_this_way".to_string(),
                },
            )));
        }
        "you haven't completed tomb of annihilation" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::CompletedDungeon,
            ))));
        }
        _ => {}
    }

    if let Some(after_atk) = low.strip_prefix("you attacked with ")
        && let Some(cnt_str) = after_atk.strip_suffix(" or more creatures this turn")
        && let Some(n) = parse_number_word(cnt_str.trim())
        && n > 0
    {
        return Ok(ConditionAst::Marker(
            GameMarker::AttackedWithMinCreaturesThisTurn {
                player: PlayerRefAst::You,
                min_count: u16::try_from(n).unwrap_or(2),
            },
        ));
    }
    if low.starts_with("it's blocking or blocked by ")
        || low.starts_with("at least one ") && low.ends_with(" is milled this way")
        || low.starts_with("you won ") && low.ends_with(" flips this way")
        || low.starts_with("target player has fewer than ") && low.ends_with(" counters")
        || low.starts_with("you put fewer than ") && low.ends_with(" this way")
        || low.starts_with("those creatures have total power ") && low.ends_with(" or greater")
        || low.starts_with("an opponent has at least ")
            && low.ends_with(" more cards in hand than you")
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if let Some(flt_raw) = low
        .strip_suffix(" is in your graveyard")
        .or_else(|| low.strip_suffix(" is in a graveyard"))
        && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
    {
        flt.zone = Some(if low.ends_with(" is in a graveyard") {
            ZoneFilter::AnyGraveyard
        } else {
            ZoneFilter::YourGraveyard
        });
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // Orthogonal negated subtype/status/mana-spent/keyword conditions:
    // "it's not a <flt>", "it's not <status>", "<mana> wasn't spent to cast it/~", "it didn't have <kw>"
    if let Some(flt_raw) = low
        .strip_prefix("it's not ")
        .or_else(|| low.strip_prefix("it isn't "))
    {
        if matches!(
            flt_raw,
            "suspected"
                | "tapped"
                | "untapped"
                | "monstrous"
                | "renowned"
                | "saddled"
                | "crewed"
                | "goaded"
                | "blocked"
        ) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                status: flt_raw.to_string(),
            })));
        }
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(flt),
            })));
        }
    }
    if let Some(spent) = low
        .strip_suffix(" wasn't spent to cast it")
        .or_else(|| low.strip_suffix(" wasn't spent to cast ~"))
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
            spent.trim().to_string(),
        ))));
    }
    if let Some(kw) = low.strip_prefix("it didn't have ")
        && parse_keyword_list(kw, vocab).is_some()
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            keyword: kw.to_string(),
        })));
    }
    if let Some(after) = low
        .strip_prefix("there's ")
        .or_else(|| low.strip_prefix("there is "))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        let flt_clean = flt_raw.replace(" total in all graveyards", " in all graveyards");
        if let Ok(flt) = parse_object_filter(&flt_clean, vocab) {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }
    if let Some((who_raw, flt_raw)) = low
        .split_once(" controls no ")
        .or_else(|| low.split_once(" control no "))
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some((who_raw, rest)) = low.split_once(" controls more ")
        && let Some(flt_raw) = rest.strip_suffix(" than you")
        && let (Some(_player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::OpponentMoreThanYou {
            metric: "permanents".to_string(),
            filter: Some(Box::new(flt)),
        });
    }
    if low.starts_with("you roll a ")
        || low.ends_with(" was beheld")
        || low.ends_with(" crewed it this turn")
        || low.ends_with(" attacked this combat")
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "<N> or more creatures died [under your control] this turn" / "<N> or more cards have been exiled with ~" / "<N> or more <flt> are in your graveyard"
    if let Some((cnt_raw, tail)) = low.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        if tail == "creatures died this turn"
            || tail == "creatures died under your control this turn"
        {
            let creature_flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(creature_flt),
                from_zone: Some(ZoneFilter::Battlefield),
                to_zone: Some(ZoneFilter::AnyGraveyard),
                controller: if tail.contains("under your control") {
                    Some(PlayerRefAst::You)
                } else {
                    None
                },
                min_count: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
        if tail == "cards have been exiled with ~" {
            let exiled_flt = ObjectFilterAst {
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            };
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(exiled_flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
        if let Some(flt_raw) = tail.strip_suffix(" are in your graveyard")
            && let Ok(flt) = parse_object_filter(&format!("{flt_raw} in your graveyard"), vocab)
        {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }
    // "there are fewer than <N> <flt>" (Shadowborn Demon)
    if let Some(after) = low.strip_prefix("there are fewer than ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Lt,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    // "you sacrificed <N> or more <flt> this turn" / "<player> was dealt <N> or more [combat] damage this turn" / "you've been dealt <N> or more damage this turn" / "it has dealt <N> or more damage to that player this turn"
    if (low.starts_with("you sacrificed ") && low.ends_with(" this turn"))
        || (low.contains(" was dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("you've been dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("it has dealt ") && low.ends_with(" damage to that player this turn"))
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "you have <N> or more cards in your library" / "you have exactly <N> cards in your hand"
    // Cluster 13 & 20: Coin-flip outcomes, player/card tie comparisons, and Harnessed state
    if matches!(
        low.as_str(),
        "you win all the flips"
            | "you won all the flips"
            | "you lose a flip"
            | "you lost a flip"
            | "you lose the flip"
            | "you lost the flip"
            | "you win the flip"
            | "you won the flip"
    ) || (low.starts_with("you win ") && low.ends_with(" or more flips"))
        || (low.starts_with("you won ") && low.ends_with(" or more flips"))
    {
        return Ok(ConditionAst::CoinFlipOutcome { outcome: low });
    }
    if let Some(after_tie) = low.strip_prefix("two or more ")
        && let Some((_, metric)) = after_tie.split_once(" are tied for ")
    {
        return Ok(ConditionAst::PlayersTiedFor {
            metric: metric.trim().to_string(),
            min_tied: 2,
        });
    }
    if low
        == "each of those creatures could be blocked by all creatures that the other is blocked by"
    {
        return Ok(ConditionAst::CanLegallySwapBlockAssignments {
            subjects: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
            perspective: CombatSwapPerspective::BlockedAttackers,
        });
    }
    if low == "each of those creatures could block all creatures that the other is blocking" {
        return Ok(ConditionAst::CanLegallySwapBlockAssignments {
            subjects: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
            perspective: CombatSwapPerspective::BlockingCreatures,
        });
    }
    if low == "the chosen card is cast as a spell" {
        return Ok(ConditionAst::Marker(GameMarker::ChosenCardCastAsSpell));
    }
    if let Some(subj_raw) = low.strip_suffix(" is harnessed")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectHarnessed(
            Box::new(subj),
        )));
    }

    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(cnt_raw) = after.strip_suffix(" or more cards in your library")
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::CardsInLibrary,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some(rest) = after.strip_prefix("exactly ")
            && let Some(cnt_raw) = rest
                .strip_suffix(" cards in your hand")
                .or_else(|| rest.strip_suffix(" cards in hand"))
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }
    // "<player> controls <flt>" (e.g. "that player controls a Plains") / "you control <subj>" / "that mana is spent on <flt>" / "that target is <colors>"
    if let Some((who_raw, flt_raw)) = low.split_once(" controls ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        if let Some(conjoined) = try_parse_conjoined_controls(player, flt_raw, vocab) {
            return Ok(conjoined);
        }
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(ConditionAst::ControlsCount {
                player,
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: 1,
            });
        }
    }
    // Lossless "you control <flt> named <A> [and <B> ...]" preserving exact original-case card names
    // (Scepter of Empires, Power Plant Worker, Helm of Kaldra, Festering Newt, Ruin in Their Wake, Galactus, etc.)
    if let Some(after) = low.strip_prefix("you control ")
        && let Some((flt_raw, names_low)) = after.split_once(" named ")
        && !names_low.trim().is_empty()
        && let Ok(base_flt) = parse_object_filter(flt_raw, vocab)
    {
        let raw_base = raw.get(..low.len()).unwrap_or(raw);
        let names_start = "you control ".len() + flt_raw.len() + " named ".len();
        let names_orig = raw_base.get(names_start..).unwrap_or(names_low).trim();
        let is_singular = flt_raw.starts_with("a ")
            || flt_raw.starts_with("an ")
            || flt_raw.starts_with("another ");
        let (is_disjunction, names): (bool, Vec<String>) = if names_orig.contains(" or ") {
            (
                true,
                names_orig
                    .split(" or ")
                    .map(|p| p.trim().to_string())
                    .collect(),
            )
        } else if is_singular {
            (false, vec![names_orig.to_string()])
        } else {
            (false, split_named_card_list(names_orig))
        };
        let mut iter = names.into_iter().filter(|n| !n.is_empty());
        if let Some(first_name) = iter.next() {
            let make_cond = |name: String| {
                let mut f = base_flt.clone();
                f.named = Some(if name == "~" {
                    NameFilterAst::SelfName
                } else {
                    NameFilterAst::Exact(name)
                });
                ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter: Box::new(f),
                    op: CompareOp::Ge,
                    value: 1,
                }
            };
            let mut acc = make_cond(first_name);
            for next_name in iter {
                acc = if is_disjunction {
                    ConditionAst::Or(Box::new(acc), Box::new(make_cond(next_name)))
                } else {
                    ConditionAst::And(Box::new(acc), Box::new(make_cond(next_name)))
                };
            }
            return Ok(acc);
        }
    }
    if let Some(subj_raw) = low.strip_prefix("you control ") {
        if let Some(conjoined) = try_parse_conjoined_controls(PlayerRefAst::You, subj_raw, vocab) {
            return Ok(conjoined);
        }
        if let Ok(flt) = parse_object_filter(subj_raw, vocab) {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: 1,
            });
        }
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            match subj {
                SubjectRefAst::Group(flt) | SubjectRefAst::All(flt) | SubjectRefAst::Each(flt) => {
                    return Ok(ConditionAst::ControlsCount {
                        player: PlayerRefAst::You,
                        filter: flt,
                        op: CompareOp::Ge,
                        value: 1,
                    });
                }
                _ => {
                    return Ok(ConditionAst::PlayerActionThisTurn {
                        player: PlayerRefAst::You,
                        action: low,
                    });
                }
            }
        }
    }
    if low.starts_with("that mana is spent on ")
        || low.starts_with("any of that mana is spent on ")
        || low.starts_with("that target is ")
        || low.starts_with("~ is enchanted by ")
        || low.starts_with("they were attacked this turn by ")
        || (low.starts_with("you revealed ") && low.ends_with(" as you cast ~"))
        || (low.ends_with(" or more creatures are attacking")
            && parse_number_word(
                low.strip_suffix(" or more creatures are attacking")
                    .unwrap_or(""),
            )
            .is_some())
        || (low.contains(" have exactly ") && low.ends_with(" cards in hand"))
        || (low.ends_with(" cards were put into your graveyard from anywhere this turn"))
        || (low.starts_with("it's the first time ") && low.ends_with(" this turn"))
        || (low.starts_with("a player was dealt ") && low.ends_with(" this turn"))
        || (low.starts_with("you had another ") && low.ends_with(" last turn"))
        || low.starts_with("it doesn't have the same name as another ")
        || (low.contains(" or more mana was spent to cast "))
        || low == "another permanent with the same name is on the battlefield"
        || low == "it has the same name as a permanent"
        || low
            == "it has the greatest power or is tied for greatest power among creatures on the battlefield"
        || low == "you've controlled ~ continuously since the beginning of your most recent turn"
        || (low.starts_with("~ or another ")
            && low.ends_with(" entered the battlefield under your control this turn"))
        || (low.starts_with("there are ")
            && low.ends_with(" permanent types among cards in your graveyard"))
        || (low.starts_with("there are at least ") && low.ends_with(" in your graveyard"))
        || low.starts_with("it shares a color with ")
        || low.starts_with("an opponent controls at least ")
        || low == "the number of permanents is even"
        || low
            == "you've cast a spell from a graveyard or activated an ability of a card in a graveyard this turn"
        || low.starts_with("you both own and control ~ and ")
        || low == "it's your main phase"
        || low == "that spell's mana cost or that ability's activation cost contains {x}"
        || low == "you don't copy a spell this way"
        || low == "you don't cast a spell this way"
        || low == "you don't or can't make an exchange"
        || low == "it shares a card type with a card exiled with ~"
        || low == "that spell doesn't target a creature you control"
        || low == "a creature not named ~ died this turn"
        || low == "mana from a treasure was spent to activate this ability"
        || low == "you've cast both a creature spell and a noncreature spell this turn"
        || low == "a player has more life than each other player"
        || low == "it's an art sticker"
        || low == "you haven't activated an exhaust ability this turn"
        || ((low.starts_with("there is exactly ") || low.starts_with("there are exactly "))
            && low.contains(" counter"))
        || (low.starts_with("there are ") && low.contains(" or more counters among "))
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "no <flt> were <participle> this way" (Haunting Imitation, etc.)
    if let Some(after_no) = low.strip_prefix("no ")
        && let Some(before_tw) = after_no.strip_suffix(" this way")
    {
        for passive in [
            " were revealed",
            " were exiled",
            " were discarded",
            " were destroyed",
            " were sacrificed",
            " were milled",
            " were put onto the battlefield",
            " were put into your hand",
        ] {
            if let Some(flt_raw) = before_tw.strip_suffix(passive)
                && parse_object_filter(flt_raw.trim(), vocab).is_ok()
            {
                return Ok(ConditionAst::Not(Box::new(
                    ConditionAst::PlayerActionThisTurn {
                        player: PlayerRefAst::You,
                        action: low,
                    },
                )));
            }
        }
    }

    // Orthogonal "this way" resolution conditions:
    // "a <flt> was sacrificed/exiled/discarded/dealt damage this way",
    // "you put/discard/sacrifice/return <flt> ... this way", "an opponent created a token this way"
    if let Some(before_tw) = low.strip_suffix(" this way") {
        for passive in [
            " was sacrificed",
            " were sacrificed",
            " was discarded",
            " were discarded",
            " was exiled",
            " were exiled",
            " was destroyed",
            " were destroyed",
            " was countered",
            " was milled",
            " was revealed",
            " was created",
            " was returned",
            " were returned",
            " is destroyed",
            " is exiled",
            " is sacrificed",
            " is discarded",
            " is countered",
            " is returned",
            " is put onto the battlefield",
            " is put into a graveyard",
            " is put into your hand",
            " is dealt damage",
            " was dealt damage",
            " enters",
        ] {
            if let Some(flt_raw) = before_tw.strip_suffix(passive)
                && (parse_object_filter(flt_raw, vocab).is_ok()
                    || parse_player_ref(flt_raw).is_some())
            {
                return Ok(ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: low,
                });
            }
        }
        for (pref, player) in [
            ("you put ", PlayerRefAst::You),
            ("you discard ", PlayerRefAst::You),
            ("you discarded ", PlayerRefAst::You),
            ("you sacrifice ", PlayerRefAst::You),
            ("you sacrificed ", PlayerRefAst::You),
            ("you exile ", PlayerRefAst::You),
            ("you exiled ", PlayerRefAst::You),
            ("you return ", PlayerRefAst::You),
            ("you returned ", PlayerRefAst::You),
            ("you create ", PlayerRefAst::You),
            ("you created ", PlayerRefAst::You),
            ("you mill ", PlayerRefAst::You),
            ("you milled ", PlayerRefAst::You),
            ("you reveal ", PlayerRefAst::You),
            ("you revealed ", PlayerRefAst::You),
            ("you cast ", PlayerRefAst::You),
            ("an opponent created ", PlayerRefAst::AnyOpponent),
            ("an opponent sacrificed ", PlayerRefAst::AnyOpponent),
            ("an opponent discarded ", PlayerRefAst::AnyOpponent),
        ] {
            if before_tw.starts_with(pref) {
                return Ok(ConditionAst::PlayerActionThisTurn {
                    player,
                    action: low,
                });
            }
        }
    }

    // Orthogonal turn-event conditions:
    // "a <flt> entered the battlefield under an opponent's control this turn"
    // "a <flt> left the battlefield [under your control] this turn"
    // "a <flt> was put into a graveyard from the battlefield this turn"
    // "any of that damage was dealt by a <flt>"
    if let Some(flt_raw) = low
        .strip_suffix(" entered the battlefield under an opponent's control this turn")
        .or_else(|| low.strip_suffix(" entered under an opponent's control this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: None,
            to_zone: Some(ZoneFilter::Battlefield),
            controller: Some(PlayerRefAst::AnyOpponent),
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_suffix(" left the battlefield under your control this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: None,
            controller: Some(PlayerRefAst::You),
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_suffix(" left the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: None,
            controller: None,
            min_count: 1,
        });
    }
    if let Some(flt_raw) =
        low.strip_suffix(" was put into a graveyard from the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: Some(ZoneFilter::AnyGraveyard),
            controller: None,
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("any of that damage was dealt by ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if low.starts_with("it was crewed by ") {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }

    // "<mana> was spent to cast it/~"
    if let Some(spent) = low
        .strip_suffix(" was spent to cast it")
        .or_else(|| low.strip_suffix(" was spent to cast ~"))
    {
        return Ok(ConditionAst::ManaSpentToCast(spent.trim().to_string()));
    }

    // "it was kicked with its {M} kicker" (CR 702.33c multi-slot kicker)
    if let Some(slot_raw) = low
        .strip_prefix("~ was kicked with its ")
        .or_else(|| low.strip_prefix("it was kicked with its "))
        && let Some(slot) = slot_raw.strip_suffix(" kicker")
    {
        return Ok(ConditionAst::WasKickedSlot(slot.trim().to_string()));
    }
    if low.starts_with("it was kicked") || low.starts_with("~ was kicked") {
        return Ok(ConditionAst::WasKicked);
    }
    // "the {M} cost was paid" (Devastating Mastery, alternative/additional cost checks)
    if let Some(after) = low.strip_prefix("the ")
        && let Some(cost_raw) = after.strip_suffix(" cost was paid")
    {
        return Ok(ConditionAst::ManaSpentToCast(cost_raw.trim().to_string()));
    }
    if let Some(flt_raw) = low.strip_prefix("you controlled ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("you revealed ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }

    // "you win" / "you lose" / "you win the/a flip" / "you win the/a clash"
    if matches!(
        low.as_str(),
        "you win"
            | "you won"
            | "you win the flip"
            | "you win a flip"
            | "you won the flip"
            | "you won a flip"
            | "you win the clash"
            | "you win a clash"
            | "you won the clash"
            | "you won a clash"
    ) {
        return Ok(ConditionAst::WonContest);
    }
    if matches!(
        low.as_str(),
        "you lose"
            | "you lost"
            | "you lose the flip"
            | "you lose a flip"
            | "you lost the flip"
            | "you lost a flip"
            | "you lose the clash"
            | "you lose a clash"
            | "you lost the clash"
            | "you lost a clash"
    ) {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::WonContest)));
    }

    // "it targets <filter>" / "~ targets <filter>"
    if let Some(flt_raw) = low
        .strip_prefix("it targets ")
        .or_else(|| low.strip_prefix("~ targets "))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }

    let parse_total_stat_cmp = |rhs: &str| -> Option<(CompareOp, i32)> {
        let trimmed = rhs
            .strip_suffix(" this combat")
            .or_else(|| rhs.strip_suffix(" this turn"))
            .unwrap_or(rhs)
            .trim();
        if let Some(n_tok) = trimmed
            .strip_suffix(" or greater")
            .or_else(|| trimmed.strip_suffix(" or more"))
        {
            return parse_number_word(n_tok).map(|n| (CompareOp::Ge, n));
        }
        if let Some(n_tok) = trimmed
            .strip_suffix(" or less")
            .or_else(|| trimmed.strip_suffix(" or fewer"))
        {
            return parse_number_word(n_tok).map(|n| (CompareOp::Le, n));
        }
        if let Some(n_tok) = trimmed.strip_prefix("greater than ") {
            return parse_number_word(n_tok).map(|n| (CompareOp::Gt, n));
        }
        if let Some(n_tok) = trimmed
            .strip_prefix("less than ")
            .or_else(|| trimmed.strip_prefix("fewer than "))
        {
            return parse_number_word(n_tok).map(|n| (CompareOp::Lt, n));
        }
        let eq_tok = trimmed
            .strip_prefix("equal to ")
            .or_else(|| trimmed.strip_prefix("exactly "))
            .unwrap_or(trimmed);
        parse_number_word(eq_tok).map(|n| (CompareOp::Eq, n))
    };

    // "you attacked with <flt> with total power/toughness/mana value <cmp> [this combat/turn]"
    if let Some(after) = low.strip_prefix("you attacked with ") {
        for (marker, stat) in [
            (" with total power ", StatKind::Power),
            (" with total toughness ", StatKind::Toughness),
            (" with total mana value ", StatKind::ManaValue),
        ] {
            if let Some((flt_raw, rhs)) = after.split_once(marker)
                && let (Ok(flt), Some((op, value))) = (
                    parse_object_filter(flt_raw, vocab),
                    parse_total_stat_cmp(rhs),
                )
            {
                return Ok(ConditionAst::TotalStatCompare {
                    filter: Box::new(flt),
                    stat,
                    op,
                    value,
                });
            }
        }
    }

    // "you control <flt> with total power/toughness/mana value <cmp>"
    if let Some(after) = low.strip_prefix("you control ") {
        for (marker, stat) in [
            (" with total power ", StatKind::Power),
            (" with total toughness ", StatKind::Toughness),
            (" with total mana value ", StatKind::ManaValue),
        ] {
            if let Some((flt_raw, rhs)) = after.split_once(marker)
                && let (Ok(mut flt), Some((op, value))) = (
                    parse_object_filter(flt_raw, vocab),
                    parse_total_stat_cmp(rhs),
                )
            {
                if flt.controller.is_none() {
                    flt.controller = Some(ControllerRelation::You);
                }
                return Ok(ConditionAst::TotalStatCompare {
                    filter: Box::new(flt),
                    stat,
                    op,
                    value,
                });
            }
        }
    }

    // "<player> discarded a card this turn" / "<player> drew a card this turn" / "<player> lost life this turn"
    for (suf, act) in [
        (" discarded a card this turn", "discarded_card"),
        (" drew a card this turn", "drew_card"),
        (" drew two or more cards this turn", "drew_2plus"),
        (" gained life this turn", "gained_life"),
        (" lost life this turn", "lost_life"),
        (" sacrificed a permanent this turn", "sacrificed_permanent"),
        (" sacrificed a creature this turn", "sacrificed_creature"),
        (" attacked this turn", "attacked"),
    ] {
        if let Some(who_raw) = low.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player,
                action: act.to_string(),
            });
        }
    }

    // "you have N or more/fewer opponents" / "you have more/fewer than N opponents" / "you have exactly N opponents"
    if let Some(after) = low.strip_prefix("you have ")
        && let Some(cnt_part) = after.strip_suffix(" opponents")
    {
        let parsed = if let Some(tok) = cnt_part.strip_suffix(" or more") {
            parse_number_word(tok).map(|n| (CompareOp::Ge, n))
        } else if let Some(tok) = cnt_part.strip_suffix(" or fewer") {
            parse_number_word(tok).map(|n| (CompareOp::Le, n))
        } else if let Some(tok) = cnt_part.strip_prefix("more than ") {
            parse_number_word(tok).map(|n| (CompareOp::Gt, n))
        } else if let Some(tok) = cnt_part.strip_prefix("fewer than ") {
            parse_number_word(tok).map(|n| (CompareOp::Lt, n))
        } else if let Some(tok) = cnt_part.strip_prefix("exactly ") {
            parse_number_word(tok).map(|n| (CompareOp::Eq, n))
        } else {
            None
        };
        if let Some((op, n)) = parsed {
            return Ok(ConditionAst::OpponentCountCompare {
                op,
                value: u16::try_from(n.max(0)).unwrap_or(2),
            });
        }
    }

    // "<filter> have/had total power/toughness/mana value <cmp>"
    for (marker, stat) in [
        (" have total power ", StatKind::Power),
        (" had total power ", StatKind::Power),
        (" have total toughness ", StatKind::Toughness),
        (" had total toughness ", StatKind::Toughness),
        (" have total mana value ", StatKind::ManaValue),
        (" had total mana value ", StatKind::ManaValue),
    ] {
        if let Some((flt_raw, rhs)) = low.split_once(marker)
            && let (Ok(flt), Some((op, value))) = (
                parse_object_filter(flt_raw, vocab),
                parse_total_stat_cmp(rhs),
            )
        {
            return Ok(ConditionAst::TotalStatCompare {
                filter: Box::new(flt),
                stat,
                op,
                value,
            });
        }
    }

    // "<player> has/have N or more/less life" / "<player> has/have N or more/fewer/no cards in hand" / "<player> has/have N or more cards in their graveyard"
    if let Some((who_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if rhs == "no cards in hand"
            || rhs == "no cards in their hand"
            || rhs == "no cards in your hand"
        {
            return Ok(ConditionAst::HandCount {
                player,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        if let Some(rest) = rhs
            .strip_suffix(" cards in hand")
            .or_else(|| rhs.strip_suffix(" cards in their hand"))
            .or_else(|| rhs.strip_suffix(" cards in your hand"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
        if let Some(rest) = rhs
            .strip_suffix(" cards in their graveyard")
            .or_else(|| rhs.strip_suffix(" cards in your graveyard"))
            .or_else(|| rhs.strip_suffix(" cards in graveyard"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::GraveyardCount {
                    player,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::GraveyardCount {
                    player,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
        if let Some(rest) = rhs.strip_suffix(" life more than your starting life total")
            && let Some(n_tok) = rest.strip_prefix("at least ")
            && let Ok(n) = n_tok.trim().parse::<i32>()
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Add(
                    Box::new(ValueExprAst::StartingLifeTotal(player)),
                    Box::new(ValueExprAst::Const(n)),
                )),
            });
        }
        if let Some(rest) = rhs.strip_suffix(" life") {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player,
                    op: CompareOp::Ge,
                    value: n,
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or less")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player,
                    op: CompareOp::Le,
                    value: n,
                });
            }
        }
        if let Some(before_cnt) = rhs
            .strip_suffix(" counters")
            .or_else(|| rhs.strip_suffix(" counter"))
            && let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::PlayerCounterCompare {
                player,
                counter_kind: kind.trim().to_string(),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }

    // "<player>'s / a library has N or fewer cards in it"
    if let Some((who_raw, rest)) = low.split_once(" library has ")
        && let Some(n_tok) = rest.strip_suffix(" or fewer cards in it")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who_raw),
                stat: PlayerStatKind::CardsInLibrary,
            }),
            op: CompareOp::Le,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }

    if let Some(rest) = low
        .strip_prefix("you've gained ")
        .or_else(|| low.strip_prefix("you gained "))
        && let Some(amt_part) = rest.strip_suffix(" life this turn")
    {
        let amt_clean = amt_part
            .trim()
            .strip_suffix(" or more")
            .unwrap_or(amt_part.trim())
            .trim();
        if let Some(n) = parse_number_word(amt_clean)
            && n > 1
        {
            return Ok(ConditionAst::GainedLifeAtLeastThisTurn {
                player: PlayerRefAst::You,
                amount: n,
            });
        }
        return Ok(ConditionAst::GainedLifeThisTurn);
    }
    if let Some(rest) = low
        .strip_prefix("an opponent has cast ")
        .or_else(|| low.strip_prefix("an opponent cast "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::AnyOpponent,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low
        .strip_prefix("you've cast ")
        .or_else(|| low.strip_prefix("you cast "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low.strip_prefix("an opponent controls more ")
        && let Some(flt_raw) = rest.strip_suffix(" than you")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::OpponentMoreThanYou {
            metric: "controls".to_string(),
            filter: Some(Box::new(flt)),
        });
    }

    if low.contains("seven or more cards") && low.contains("graveyard") {
        return Ok(ConditionAst::Threshold);
    }
    if low.contains("four or more card types among cards in your graveyard") {
        return Ok(ConditionAst::Delirium);
    }
    if low.contains("three or more artifacts") && low.contains("you control") {
        return Ok(ConditionAst::Metalcraft);
    }
    if low.contains("permanent you controlled left the battlefield this turn")
        || low.contains("permanent left the battlefield under your control this turn")
    {
        return Ok(ConditionAst::Revolt);
    }

    // "<player> lost N or more life this turn"
    if let Some((who_raw, rhs)) = low.split_once(" lost ")
        && let Some(n_tok) = rhs.strip_suffix(" or more life this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::LostLifeThisTurn {
            player,
            min_amount: n,
        });
    }

    // "<player> has drawn / drew N or more cards this turn"
    if let Some((who_raw, rest)) = low
        .split_once(" has drawn ")
        .or_else(|| low.split_once(" have drawn "))
        .or_else(|| low.split_once("'ve drawn "))
        .or_else(|| low.split_once(" drew "))
        && let Some(n_tok) = rest.strip_suffix(" or more cards this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
        }));
    }

    // "<player> cast / has cast N or more spells this turn"
    if let Some((who_raw, rest)) = low
        .split_once(" has cast ")
        .or_else(|| low.split_once(" have cast "))
        .or_else(|| low.split_once("'ve cast "))
        .or_else(|| low.split_once(" cast "))
        && let Some(n_tok) = rest.strip_suffix(" or more spells this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
            from_hand_only: false,
        }));
    }

    // "<player> sacrificed / has sacrificed <filter> this turn"
    if let Some((who_raw, rest)) = low
        .split_once("'ve sacrificed ")
        .or_else(|| low.split_once(" has sacrificed "))
        .or_else(|| low.split_once(" sacrificed "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::SacrificedThisTurn {
            player,
            filter: Box::new(flt),
        }));
    }

    // "<filter> was put into your graveyard from anywhere this turn"
    if let Some(flt_raw) = low
        .strip_suffix(" was put into your graveyard from anywhere this turn")
        .or_else(|| low.strip_suffix(" was put into your graveyard this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }

    // "your devotion to <colors> is less than <N>" / "<N> or greater"
    if let Some(after) = low.strip_prefix("your devotion to ")
        && let Some((colors, rhs)) = after.split_once(" is ")
    {
        if let Some(n_tok) = rhs.strip_prefix("less than ")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::DevotionCompare {
                colors: colors.trim().to_string(),
                op: CompareOp::Lt,
                value: u16::try_from(n.max(0)).unwrap_or(5),
            });
        }
        if let Some(n_tok) = rhs.strip_suffix(" or greater")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::DevotionCompare {
                colors: colors.trim().to_string(),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(5),
            });
        }
    }

    // "<N or more / a / an / another> <filter> entered [the battlefield] under your control this turn"
    if let Some(before_ent) = low
        .strip_suffix(" entered the battlefield under your control this turn")
        .or_else(|| low.strip_suffix(" entered under your control this turn"))
    {
        if let Some((cnt_tok, flt_raw)) = before_ent.split_once(" or more ")
            && let (Some(n), Ok(flt)) = (
                parse_number_word(cnt_tok),
                parse_object_filter(flt_raw, vocab),
            )
        {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        if let Ok(flt) = parse_object_filter(before_ent, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: 1,
            });
        }
    }

    // "there is a/an <filter>" (e.g. "there is a Desert card in your graveyard")
    if let Some(flt_raw) = low.strip_prefix("there is ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "there are N or more mana values among cards in your graveyard"
    if let Some(after) = low.strip_prefix("there are ")
        && let Some(n_tok) =
            after.strip_suffix(" or more mana values among cards in your graveyard")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DifferentManaValuesInGraveyard {
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(4),
            },
        ));
    }

    // "<subject> doesn't have <keyword>" / "<subject> has <keyword>"
    if let Some((subj_raw, kw_raw)) = low.split_once(" doesn't have ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw_raw.trim().to_string(),
        })));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw_raw.trim().to_string(),
        });
    }

    // Zone location conditions: "~ is in your graveyard", "~ is in your hand", "~ is in exile"
    for (phrase_lit, subj, zone) in [
        (
            "~ is in your graveyard",
            SubjectRefAst::SelfObject,
            ZoneFilter::YourGraveyard,
        ),
        (
            "~ is in your hand",
            SubjectRefAst::SelfObject,
            ZoneFilter::YourHand,
        ),
        (
            "~ is in exile",
            SubjectRefAst::SelfObject,
            ZoneFilter::Exile,
        ),
        (
            "~ is on the battlefield",
            SubjectRefAst::SelfObject,
            ZoneFilter::Battlefield,
        ),
        (
            "it is in your graveyard",
            SubjectRefAst::Anaphora("it".to_string()),
            ZoneFilter::YourGraveyard,
        ),
        (
            "it's in your graveyard",
            SubjectRefAst::Anaphora("it".to_string()),
            ZoneFilter::YourGraveyard,
        ),
    ] {
        if low == phrase_lit {
            return Ok(ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(subj),
                zone,
            }));
        }
    }

    for status in [
        "tapped",
        "untapped",
        "attacking",
        "blocking",
        "blocked",
        "enchanted",
        "equipped",
        "modified",
        "monstrous",
        "renowned",
        "suspected",
        "saddled",
        "solved",
        "prepared",
        "face down",
        "kicked",
    ] {
        if let Some(subj_raw) = low.strip_suffix(&format!(" is {status}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(subj),
                status: status.to_string(),
            });
        }
        if let Some(subj_raw) = low
            .strip_suffix(&format!(" isn't {status}"))
            .or_else(|| low.strip_suffix(&format!(" aren't {status}")))
            .or_else(|| low.strip_suffix(&format!(" is not {status}")))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::HasStatus {
                subject: Box::new(subj),
                status: status.to_string(),
            })));
        }
        if low == format!("it's {status}")
            || low == format!("he's {status}")
            || low == format!("she's {status}")
        {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                status: status.to_string(),
            });
        }
        if low == format!("it isn't {status}") || low == format!("it's not {status}") {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                status: status.to_string(),
            })));
        }
    }

    // "you controlled <subj>" / "this ability has been activated N or more times this turn"
    if let Some(subj_raw) = low.strip_prefix("you controlled ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::ControlledSubject(
            Box::new(subj),
        )));
    }
    if let Some(rest) = low.strip_prefix("this ability has been activated ")
        && let Some(cnt_raw) = rest
            .strip_suffix(" times this turn")
            .or_else(|| rest.strip_suffix(" time this turn"))
    {
        let (n_tok, op) = if let Some(v) = cnt_raw.strip_suffix(" or more") {
            (v, CompareOp::Ge)
        } else if let Some(v) = cnt_raw.strip_suffix(" or fewer") {
            (v, CompareOp::Le)
        } else {
            (cnt_raw, CompareOp::Eq)
        };
        if let Some(n) = parse_number_word(n_tok) {
            return Ok(ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn {
                op,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            }));
        }
    }

    // Generalized "<player> is the monarch" / "<player> didn't cast <filter> this turn"
    // "<player> has more cards in hand than <player2>" / "<subj> entered this turn" / "<subj> was/wasn't cast" / "<subj> is exiled"
    if let Some(who_raw) = low.strip_suffix(" is the monarch")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(ConditionAst::Marker(GameMarker::Monarch(player)));
    }
    if let Some((who_raw, rest)) = low.split_once(" didn't cast ")
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player,
            filter: Some(Box::new(flt)),
        }));
    }
    if let Some((p1_raw, p2_raw)) = low.split_once(" has more cards in hand than ")
        && let (Some(p1), Some(p2)) = (parse_player_ref(p1_raw), parse_player_ref(p2_raw))
    {
        return Ok(ConditionAst::Marker(GameMarker::PlayerHasMoreHandThan {
            player: p1,
            than: p2,
        }));
    }
    if let Some(subj_raw) = low
        .strip_suffix(" entered this turn")
        .or_else(|| low.strip_suffix(" entered the battlefield this turn"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(
            Box::new(subj),
        )));
    }
    if let Some(subj_raw) = low.strip_suffix(" wasn't cast")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectWasCast {
            subject: Box::new(subj),
            was_cast: false,
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" was cast")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectWasCast {
            subject: Box::new(subj),
            was_cast: true,
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" is exiled")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::InZone {
            subject: Box::new(subj),
            zone: ZoneFilter::Exile,
        }));
    }
    // "a graveyard has N or more cards in it" / "N or more cards are in your graveyard"
    if let Some(rest) = low.strip_prefix("a graveyard has ")
        && let Some(n_tok) = rest.strip_suffix(" or more cards in it")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::GraveyardCount {
            player: PlayerRefAst::AnyPlayer,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    if let Some(n_tok) = low.strip_suffix(" or more cards are in your graveyard")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    // "<player> has/have exactly N life" / "fewer than N cards in hand"
    if let Some((who_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if let Some(rest) = rhs.strip_prefix("exactly ")
            && let Some(n_tok) = rest.strip_suffix(" life")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::LifeCompare {
                player,
                op: CompareOp::Eq,
                value: n,
            });
        }
        if let Some(rest) = rhs.strip_prefix("fewer than ")
            && let Some(n_tok) = rest
                .strip_suffix(" cards in hand")
                .or_else(|| rest.strip_suffix(" cards in your hand"))
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::HandCount {
                player,
                op: CompareOp::Lt,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }

    // "<player> control(s) [don't control] [at least N | N or more/fewer] <filter>"
    for (prefix, player, neg) in [
        ("you don't control ", PlayerRefAst::You, true),
        ("you control ", PlayerRefAst::You, false),
        ("any player controls ", PlayerRefAst::AnyPlayer, false),
        ("no opponent controls ", PlayerRefAst::EachOpponent, true),
        ("no player controls ", PlayerRefAst::AnyPlayer, true),
        (
            "your opponents don't control ",
            PlayerRefAst::EachOpponent,
            true,
        ),
        ("your opponents control ", PlayerRefAst::EachOpponent, false),
        ("opponents control ", PlayerRefAst::EachOpponent, false),
        (
            "defending player doesn't control ",
            PlayerRefAst::DefendingPlayer,
            true,
        ),
        (
            "defending player controls ",
            PlayerRefAst::DefendingPlayer,
            false,
        ),
        (
            "an opponent doesn't control ",
            PlayerRefAst::AnyOpponent,
            true,
        ),
        ("an opponent controls ", PlayerRefAst::AnyOpponent, false),
        (
            "another player controls ",
            PlayerRefAst::EachOtherPlayer,
            false,
        ),
        ("your team controls ", PlayerRefAst::You, false),
    ] {
        if let Some(rest) = low.strip_prefix(prefix) {
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("exactly one ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Eq,
                    value: 1,
                });
            }
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("no ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if let Some(after_al) = rest.strip_prefix("at least ")
                && let Some((cnt_tok, flt_raw)) = after_al.split_once(' ')
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
            if let Some((cnt_tok, flt_raw)) = rest.split_once(" or more ")
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
            if let Some((cnt_tok, flt_raw)) = rest.split_once(" or fewer ")
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
            if let Ok(flt) = parse_object_filter(rest, vocab) {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: 1,
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
        }
    }

    // "there are no [<kind>] counters on <subj>" / "there are N or more <kind> counters on <subj>"
    if let Some(after) = low.strip_prefix("there are no ")
        && let Some((kind_part, subj_raw)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: if kind_part.trim().is_empty() {
                "any".to_string()
            } else {
                kind_part.trim().to_string()
            },
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, rest)) = after.split_once(" or more ")
        && let Some((kind_part, subj_raw)) = rest
            .split_once(" counters on ")
            .or_else(|| rest.split_once(" counter on "))
        && let (Some(n), Ok(subj)) = (
            parse_number_word(cnt_tok),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: kind_part.trim().to_string(),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }

    // "there are no <filter>" / "there are <filter>" / "there are N or more <filter>"
    if let Some(flt_raw) = low.strip_prefix("there are no ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_tok)
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("there are ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "you have N or more/less life" / "you have N or more/fewer cards in hand"
    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(rest) = after.strip_suffix(" life") {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player: PlayerRefAst::You,
                    op: CompareOp::Ge,
                    value: n,
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or less")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player: PlayerRefAst::You,
                    op: CompareOp::Le,
                    value: n,
                });
            }
        }
        if let Some(rest) = after
            .strip_suffix(" cards in hand")
            .or_else(|| after.strip_suffix(" cards in your hand"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player: PlayerRefAst::You,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player: PlayerRefAst::You,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
    }

    // "<filter> is/was milled/discarded/exiled/revealed this way"
    for (suf, act) in [
        (" is milled this way", "milled"),
        (" was milled this way", "milled"),
        (" is discarded this way", "discarded"),
        (" was discarded this way", "discarded"),
        (" is exiled this way", "exiled"),
        (" was exiled this way", "exiled"),
        (" is revealed this way", "revealed"),
        (" was revealed this way", "revealed"),
    ] {
        if let Some(flt_raw) = low.strip_suffix(suf)
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: act.to_string(),
                },
            ));
        }
    }

    // "<subject> isn't <filter>" / "<subject> is/was <filter>" (fully generic across any SubjectRefAst!)
    if let Some((subj_raw, rest)) = low
        .split_once(" isn't ")
        .or_else(|| low.split_once(" wasn't "))
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        })));
    }
    if let Some(rest) = low.strip_prefix("it's ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(flt),
        });
    }
    if let Some((subj_raw, rest)) = low.split_once(" is ").or_else(|| low.split_once(" was "))
        && !rest.starts_with("greater than")
        && !rest.starts_with("less than")
        && !rest.starts_with("equal to")
        && !rest.ends_with(" or greater")
        && !rest.ends_with(" or less")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        });
    }

    // "<subj>'s power/toughness/mana value is N or less/greater"
    for (stat_mid, stat) in [
        ("'s mana value is ", StatKind::ManaValue),
        ("'s power is ", StatKind::Power),
        ("'s toughness is ", StatKind::Toughness),
    ] {
        if let Some((subj_raw, rest)) = low.split_once(stat_mid)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                (v, CompareOp::Ge)
            } else if let Some(v) = rest.strip_suffix(" or less") {
                (v, CompareOp::Le)
            } else {
                (rest, CompareOp::Eq)
            };
            if let Ok(n) = n_tok.trim().parse::<i32>() {
                return Ok(ConditionAst::StatCompare {
                    subject: Box::new(subj),
                    stat,
                    op,
                    value: n,
                });
            }
        }
    }
    for (prefix, stat) in [
        ("its mana value is ", StatKind::ManaValue),
        ("its power is ", StatKind::Power),
        ("its toughness is ", StatKind::Toughness),
    ] {
        if let Some(rest) = low.strip_prefix(prefix) {
            let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                (v, CompareOp::Ge)
            } else if let Some(v) = rest.strip_suffix(" or less") {
                (v, CompareOp::Le)
            } else {
                (rest, CompareOp::Eq)
            };
            if let Ok(n) = n_tok.trim().parse::<i32>() {
                return Ok(ConditionAst::StatCompare {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat,
                    op,
                    value: n,
                });
            }
        }
    }

    // "<subject> has/had power/toughness/mana value/counters"
    if let Some((subj_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" had ")) {
        for (st_pref, stat) in [
            ("power ", StatKind::Power),
            ("toughness ", StatKind::Toughness),
            ("mana value ", StatKind::ManaValue),
        ] {
            if let Some(rest) = rhs.strip_prefix(st_pref) {
                let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                    (v, CompareOp::Ge)
                } else if let Some(v) = rest.strip_suffix(" or less") {
                    (v, CompareOp::Le)
                } else {
                    continue;
                };
                if let Ok(n) = n_tok.parse::<i32>()
                    && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
                {
                    return Ok(ConditionAst::StatCompare {
                        subject: Box::new(subj),
                        stat,
                        op,
                        value: n,
                    });
                }
            }
        }
        // "<subject> has/had [no|a|an|one or more|N or more] [<kind>] counter(s) on it"
        if let Some(before_on) = rhs.strip_suffix(" on it")
            && let Some(before_cnt) = before_on
                .strip_suffix(" counters")
                .or_else(|| before_on.strip_suffix(" counter"))
                .or_else(|| (before_on == "counters").then_some(""))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if let Some(no_kind) = before_cnt.strip_prefix("no ") {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: if no_kind.trim().is_empty() {
                        "any".to_string()
                    } else {
                        no_kind.trim().to_string()
                    },
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if before_cnt == "no" {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: "any".to_string(),
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: kind.trim().to_string(),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                });
            }
            if let Some(after_fewer) = before_cnt.strip_prefix("fewer than ")
                && let Some((num_tok, kind_tok)) = after_fewer.split_once(' ')
                && let Some(n) = parse_number_word(num_tok)
            {
                let kclean = if kind_tok.trim().is_empty() {
                    "any"
                } else {
                    kind_tok.trim()
                };
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: kclean.to_string(),
                    op: CompareOp::Lt,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(after_more) = before_cnt.strip_prefix("more than ")
                && let Some((num_tok, kind_tok)) = after_more.split_once(' ')
                && let Some(n) = parse_number_word(num_tok)
            {
                let kclean = if kind_tok.trim().is_empty() {
                    "any"
                } else {
                    kind_tok.trim()
                };
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: kclean.to_string(),
                    op: CompareOp::Gt,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            let kind = before_cnt
                .strip_prefix("one or more ")
                .or_else(|| before_cnt.strip_prefix("a "))
                .or_else(|| before_cnt.strip_prefix("an "))
                .unwrap_or(before_cnt);
            let kclean =
                if kind.trim().is_empty() || kind.trim() == "one or more" || kind.trim() == "a" {
                    "any"
                } else {
                    kind.trim()
                };
            return Ok(ConditionAst::CounterCompare {
                subject: Box::new(subj),
                counter_kind: kclean.to_string(),
                op: CompareOp::Ge,
                value: 1,
            });
        }
    }

    // "<subject> doesn't have a <kind> counter on it"
    if let Some((subj_raw, rhs)) = low.split_once(" doesn't have ")
        && let Some(before_on) = rhs.strip_suffix(" on it")
        && let Some(before_cnt) = before_on
            .strip_suffix(" counters")
            .or_else(|| before_on.strip_suffix(" counter"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = before_cnt
            .strip_prefix("a ")
            .or_else(|| before_cnt.strip_prefix("an "))
            .unwrap_or(before_cnt)
            .trim();
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: if kind.is_empty() {
                "any".to_string()
            } else {
                kind.to_string()
            },
            op: CompareOp::Eq,
            value: 0,
        });
    }

    // "<subject> has/had power/toughness/mana value greater/less than <ValueExpr>" (Drizzt Do'Urden, etc.)
    if let Some((subj_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" had ")) {
        for (st_pref, stat) in [
            ("power ", StatKind::Power),
            ("toughness ", StatKind::Toughness),
            ("mana value ", StatKind::ManaValue),
        ] {
            if let Some(rest) = rhs.strip_prefix(st_pref)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                let cmp_opt = rest
                    .strip_prefix("greater than or equal to ")
                    .map(|r| (CompareOp::Ge, r))
                    .or_else(|| {
                        rest.strip_prefix("less than or equal to ")
                            .map(|r| (CompareOp::Le, r))
                    })
                    .or_else(|| {
                        rest.strip_prefix("greater than ")
                            .map(|r| (CompareOp::Gt, r))
                    })
                    .or_else(|| rest.strip_prefix("less than ").map(|r| (CompareOp::Lt, r)));
                if let Some((op, val_raw)) = cmp_opt
                    && let Ok(val) = parse_value_expr(val_raw, vocab)
                {
                    return Ok(ConditionAst::Compare {
                        lhs: Box::new(ValueExprAst::ObjectStat {
                            subject: Box::new(subj),
                            stat,
                        }),
                        op,
                        rhs: Box::new(val),
                    });
                }
            }
        }
    }

    // Pass N orthogonal conditions:
    // - "you have exactly N cards in hand" (Library of Alexandria)
    // - "you were dealt N or more damage this turn" (Boarded Window)
    // - "a <kind> counter was put on <flt> this turn" (Fairgrounds Trumpeter)
    // - "<flt> died under your control this turn" (White Glove Gourmand)
    // - "you attacked with <flt> this turn" (Avengers Assemble!)
    // - "it targets one or more <flt>" (Orvar)
    // - "~ is attached to <flt>" / "~ is enchanted by <flt>" (Summoning Materia, Timber Paladin)
    // - "the top card of your library is <flt/color>" (Vampire Nocturnus, Crown of Convergence)
    // - "there are N or more [card types | <kind> counters] among <flt>" (Keen-Eyed Curator, Backstreet Bruiser)
    // - "you've cast N or more <flt> spells this turn" (Lyse Hext)
    // - "his/its/the {M}/<kw> cost was paid" (Leonardo, Devastating Mastery)
    if let Some(after) = low.strip_prefix("you have exactly ")
        && let Some(cnt_raw) = after.strip_suffix(" cards in hand")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        return Ok(ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }
    if let Some(after) = low.strip_prefix("you were dealt ")
        && let Some(cnt_raw) = after.strip_suffix(" or more damage this turn")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DamageDealtToSubjectThisTurn {
                subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                min_amount: u16::try_from(n.max(1)).unwrap_or(1),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("a ")
        && let Some((ck, rest)) = after.split_once(" counter was put on ")
        && rest.ends_with(" this turn")
    {
        return Ok(ConditionAst::Marker(
            GameMarker::PutCountersOnSubjectThisTurn {
                player: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::Anaphora(rest.to_string())),
                counter_kind: ck.trim().to_string(),
            },
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" died under your control this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you attacked with ")
        && let Some(flt_raw) = after.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::AttackedWithFilteredThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            min_count: 1,
        });
    }
    if let Some(after) = low
        .strip_prefix("it targets one or more ")
        .or_else(|| low.strip_prefix("that spell targets one or more "))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }
    if let Some(after) = low
        .strip_prefix("~ is attached to ")
        .or_else(|| low.strip_prefix("~ is enchanted by "))
        && !after.is_empty()
    {
        if let Ok(host) = parse_subject_or_target(after, vocab) {
            return Ok(ConditionAst::AttachedTo {
                subject: Box::new(SubjectRefAst::SelfObject),
                host: Box::new(host),
            });
        }
        return Ok(ConditionAst::HasStatus {
            subject: Box::new(SubjectRefAst::SelfObject),
            status: format!("attached:{after}"),
        });
    }
    if let Some(pred_raw) = low.strip_prefix("the top card of your library is ")
        && let Ok(flt) = parse_object_filter(pred_raw, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("top_of_library".to_string())),
            filter: Box::new(flt),
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        if let Some(flt_raw) = rest.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::CardTypesAmong(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some((_, flt_raw)) = rest.split_once(" counters among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }
    if let Some(after) = low.strip_prefix("you've cast ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw, vocab),
        )
    {
        let value = u16::try_from(n.max(0)).unwrap_or(0);
        if flt_raw.trim() == "spells" {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value,
                from_hand_only: false,
            }));
        }
        return Ok(ConditionAst::Marker(
            GameMarker::CastFilteredSpellsThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value,
            },
        ));
    }
    if let Some(cost_kind) = low
        .strip_prefix("the ")
        .or_else(|| low.strip_prefix("his "))
        .or_else(|| low.strip_prefix("her "))
        .or_else(|| low.strip_prefix("~'s "))
        .and_then(|r| r.strip_suffix(" cost was paid"))
        && !cost_kind.is_empty()
    {
        return Ok(ConditionAst::Marker(GameMarker::KeywordCostPaid(
            cost_kind.to_string(),
        )));
    }

    // Pass M orthogonal conditions: "you don't put ... into your hand this way",
    // "damage [from <flt>] is prevented this way", "most common color", "you have N or more <flt> in your graveyard"
    if low.starts_with("you don't put ") && low.ends_with(" into your hand this way") {
        return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
            zone: "your hand".to_string(),
        }));
    }
    if low.starts_with("damage ") && low.ends_with(" is prevented this way") {
        return Ok(ConditionAst::Marker(GameMarker::DamagePreventedThisWay));
    }
    if let Some((color_raw, or_tied)) = low
        .strip_suffix(" is the most common color among all permanents or is tied for most common")
        .map(|c| (c.trim(), true))
        .or_else(|| {
            low.strip_suffix(" is the most common color among all permanents")
                .map(|c| (c.trim(), false))
        })
        && let Some(color) = lookup_color(color_raw)
    {
        return Ok(ConditionAst::Marker(GameMarker::MostCommonPermanentColor {
            color,
            or_tied,
        }));
    }
    if let Some(after) = low.strip_prefix("you have ")
        && let Some((cnt_part, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) = rest.strip_suffix(" in your graveyard")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_part.trim()),
            parse_object_filter(flt_raw, vocab),
        )
    {
        let mut gflt = flt;
        gflt.zone = Some(ZoneFilter::YourGraveyard);
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(gflt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }

    // Generic voting result condition: "<option> gets more votes [or the vote is tied]"
    if let Some(opt) = low
        .strip_suffix(" gets more votes or the vote is tied")
        .or_else(|| low.strip_suffix(" gets more votes"))
        && !opt.is_empty()
    {
        let tied = low.ends_with(" or the vote is tied");
        return Ok(ConditionAst::Marker(GameMarker::VoteResult {
            winning: if tied {
                format!("{opt}_or_tie")
            } else {
                opt.to_string()
            },
        }));
    }

    // Orthogonal Pass I conditions (turn history, zone membership, stat comparisons, keyword checks)
    if low == "you created a token this turn" || low == "you created one or more tokens this turn" {
        return Ok(ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }));
    }
    if low == "you've lost life this turn" || low == "you lost life this turn" {
        return Ok(ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        });
    }
    if low == "you've played a land or cast a spell this turn from anywhere other than your hand" {
        return Ok(ConditionAst::Marker(
            GameMarker::LandOrSpellOutsideHandThisTurn {
                player: PlayerRefAst::You,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" isn't on the battlefield")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::InZone {
                subject: Box::new(subj),
                zone: ZoneFilter::Battlefield,
            },
        ))));
    }
    if let Some(flt_raw) =
        low.strip_suffix(" was put into a graveyard from the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under your control this turn")
    {
        let clean = flt_raw
            .strip_prefix("a ")
            .or_else(|| flt_raw.strip_prefix("an "))
            .or_else(|| flt_raw.strip_prefix("one or more "))
            .unwrap_or(flt_raw);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: 1,
            });
        }
    }
    if let Some(after) = low.strip_prefix("an opponent had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under their control this turn")
    {
        let (min_count, clean) = if let Some(r) = flt_raw.strip_prefix("two or more ") {
            (2u16, r)
        } else if let Some(r) = flt_raw.strip_prefix("three or more ") {
            (3u16, r)
        } else {
            (
                1u16,
                flt_raw
                    .strip_prefix("a ")
                    .or_else(|| flt_raw.strip_prefix("an "))
                    .unwrap_or(flt_raw),
            )
        };
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(flt),
                from_zone: None,
                to_zone: Some(ZoneFilter::Battlefield),
                controller: Some(PlayerRefAst::AnyOpponent),
                min_count,
            });
        }
    }
    if let Some((num_raw, tail)) = low.split_once(" or more damage was dealt to ")
        && let Some(subj_raw) = tail.strip_suffix(" this turn")
        && let (Some(n), Ok(subj)) = (
            parse_number_word(num_raw.trim()),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DamageDealtToSubjectThisTurn {
                subject: Box::new(subj),
                min_amount: u16::try_from(n.max(1)).unwrap_or(1),
            },
        ));
    }
    if let Some(kw) = low
        .strip_prefix("its ")
        .and_then(|r| r.strip_suffix(" cost was paid"))
        && !kw.is_empty()
    {
        return Ok(ConditionAst::Marker(GameMarker::KeywordCostPaid(
            kw.to_string(),
        )));
    }
    if let Some(after) = low.strip_prefix("you control the ")
        && let Some((flt_raw, rest)) = after.split_once(" with the greatest ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let stat = if rest.starts_with("mana value") {
            Some(StatKind::ManaValue)
        } else if rest.starts_with("power") {
            Some(StatKind::Power)
        } else if rest.starts_with("toughness") {
            Some(StatKind::Toughness)
        } else {
            None
        };
        if let Some(stat) = stat {
            return Ok(ConditionAst::Marker(GameMarker::ControlsGreatestStat {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                stat,
            }));
        }
    }
    if let Some((subj_raw, rest)) = low.split_once(" is the only ")
        && let Some(flt_raw) = rest.strip_suffix(" in your graveyard")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::OnlyObjectInZone {
            subject: Box::new(subj),
            filter: Box::new(flt),
            zone: ZoneFilter::YourGraveyard,
        }));
    }
    if let Some(after) = low.strip_prefix("you've put one or more ")
        && let Some((ck, tail)) = after.split_once(" counters on ")
        && let Some(subj_raw) = tail.strip_suffix(" this turn")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::PutCountersOnSubjectThisTurn {
                player: PlayerRefAst::You,
                subject: Box::new(subj),
                counter_kind: ck.trim().to_string(),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("all ")
        && let Some((flt_raw, pred_raw)) = after.split_once(" are ")
        && let (Ok(flt), Ok(pred)) = (
            parse_object_filter(flt_raw, vocab),
            parse_object_filter(pred_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::AllMatchingAre {
            filter: Box::new(flt),
            predicate: Box::new(pred),
        }));
    }
    if let Some(after) = low.strip_prefix("a player controls more ")
        && let Some(flt_raw) = after.strip_suffix(" than each other player")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::AnyPlayerControlsMoreThanEachOther {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let (Ok(subj), Some(kws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw_raw, vocab),
        )
        && let Some(kw) = kws.into_iter().next()
    {
        return Ok(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw,
        });
    }
    if let Some((subj_raw, flt_raw)) = low
        .split_once(" was a ")
        .or_else(|| low.split_once(" was an "))
        .or_else(|| low.split_once(" were a "))
        .or_else(|| low.split_once(" were an "))
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        });
    }

    // Orthogonal numeric comparison across ANY two ValueExprAst expressions (present OR past tense "is"/"was"):
    // Also supports elliptical possessive RHS ("that creature's power is greater than ~'s")!
    if let Some((lhs_raw, rhs_raw)) = low.split_once(" is ").or_else(|| low.split_once(" was "))
        && let Ok(lhs) = parse_value_expr(lhs_raw, vocab)
    {
        // An offset from a reference value is not an equality with an unknown
        // possessive player (Elenda's second, independent starting-life threshold).
        if let Some(offset_and_base) = rhs_raw.strip_prefix("at least ")
            && let Some((offset, base)) = offset_and_base.split_once(" greater than ")
        {
            let amount = parse_number_word(offset)
                .filter(|amount| *amount >= 0)
                .ok_or_else(|| format!("unparsed_comparison_offset:{offset}"))?;
            let base = parse_value_expr(base, vocab)?;
            return Ok(ConditionAst::Compare {
                lhs: Box::new(lhs),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Add(
                    Box::new(base),
                    Box::new(ValueExprAst::Const(amount)),
                )),
            });
        }
        let parsed_cmp = if let Some(r) = rhs_raw.strip_prefix("greater than or equal to ") {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = rhs_raw.strip_prefix("less than or equal to ") {
            Some((CompareOp::Le, r))
        } else if let Some(r) = rhs_raw.strip_prefix("greater than ") {
            Some((CompareOp::Gt, r))
        } else if let Some(r) = rhs_raw.strip_prefix("less than ") {
            Some((CompareOp::Lt, r))
        } else if let Some(r) = rhs_raw.strip_prefix("equal to ") {
            Some((CompareOp::Eq, r))
        } else if let Some(r) = rhs_raw.strip_prefix("exactly ") {
            Some((CompareOp::Eq, r))
        } else if let Some(r) = rhs_raw
            .strip_suffix(" or greater")
            .or_else(|| rhs_raw.strip_suffix(" or more"))
        {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = rhs_raw
            .strip_suffix(" or less")
            .or_else(|| rhs_raw.strip_suffix(" or fewer"))
        {
            Some((CompareOp::Le, r))
        } else {
            Some((CompareOp::Eq, rhs_raw))
        };
        if let Some((op, r_str)) = parsed_cmp {
            let rhs_res = parse_value_expr(r_str, vocab).or_else(|_| {
                if let ValueExprAst::ObjectStat { stat, .. } = &lhs
                    && let Some(owner_raw) = r_str.strip_suffix("'s")
                    && let Ok(owner_subj) = parse_subject_or_target(owner_raw, vocab)
                {
                    Ok(ValueExprAst::ObjectStat {
                        subject: Box::new(owner_subj),
                        stat: *stat,
                    })
                } else {
                    Err("no_rhs".to_string())
                }
            });
            if let Ok(rhs) = rhs_res {
                return Ok(ConditionAst::Compare {
                    lhs: Box::new(lhs),
                    op,
                    rhs: Box::new(rhs),
                });
            }
        }
    }

    // General anaphoric/outcome conditions ("you do", "you don't", "you can't", coin flips, played a land)
    match low.as_str() {
        "you do"
        | "you don't"
        | "you can't"
        | "you did"
        | "they do"
        | "they don't"
        | "they can't"
        | "that player does"
        | "that player doesn't"
        | "you've played a land this turn"
        | "you played a land this turn"
        | "you haven't cast the card"
        | "~ was cast this turn"
        | "it attacked a battle this turn"
        | "they haven't dealt combat damage yet"
        | "the sacrificed permanent was a commander"
        | "no other creature has greater power"
        | "you control a stickered permanent"
        | "you paid one or more {e} this way"
        | "it's paired with a creature with soulbond"
        | "a permanent entered the battlefield face down under your control this turn"
        | "you turned a permanent face up this turn"
        | "you own a card named ~ in exile, in your hand, in your graveyard, and on the battlefield" =>
        {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: low,
            });
        }
        "the coin comes up heads" | "it comes up heads" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "heads".to_string(),
            });
        }
        "the coin comes up tails" | "it comes up tails" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "tails".to_string(),
            });
        }
        "you win the flip" | "you win" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "win".to_string(),
            });
        }
        "you lose the flip" | "you lose a flip" | "you lose" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "lose".to_string(),
            });
        }
        "that creature is attacking" => {
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                filter: Box::new(ObjectFilterAst {
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }),
            });
        }
        "that player controls three or fewer lands" => {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::ThatPlayer,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::LAND,
                    ..Default::default()
                }),
                op: CompareOp::Le,
                value: 3,
            });
        }
        "two or more colors of mana were spent to cast it" => {
            return Ok(ConditionAst::ColorsOfManaSpentCompare {
                op: CompareOp::Ge,
                value: 2,
            });
        }
        "a card with the same name is in a graveyard" => {
            return Ok(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SameAsReferenced,
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    ..Default::default()
                }),
                zone: ZoneFilter::AnyGraveyard,
                op: CompareOp::Ge,
                count: 1,
            });
        }
        "a nontoken permanent with the same name is on the battlefield" => {
            return Ok(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SameAsReferenced,
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    token: Some(false),
                    ..Default::default()
                }),
                zone: ZoneFilter::Battlefield,
                op: CompareOp::Ge,
                count: 1,
            });
        }
        "defending player controls the most creatures or is tied for the most" => {
            return Ok(ConditionAst::PlayerControlsMost {
                player: PlayerRefAst::DefendingPlayer,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                allow_tie: true,
            });
        }
        _ => {}
    }
    // General "you attacked with <N> or more <filter> this turn" (Deepway Navigator)
    if let Some(after) = low
        .strip_prefix("you attacked with ")
        .and_then(|r| r.strip_suffix(" this turn"))
        && let Some((n_tok, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(n_tok.trim())
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(ConditionAst::AttackedWithFilteredThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            min_count: u8::try_from(n.max(1)).unwrap_or(1),
        });
    }
    // General "at least one [other] <filter> is blocking <subj>" / "no <filter> are blocking <subj>" (Wall of Caltrops)
    if let Some(after) = low
        .strip_prefix("at least one ")
        .or_else(|| low.strip_prefix("no "))
        && let Some((flt_raw, subj_raw)) = after
            .split_once(" is blocking ")
            .or_else(|| after.split_once(" are blocking "))
        && let Ok(blocker_flt) = parse_object_filter(flt_raw.trim(), vocab)
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let cond = ConditionAst::BlockedByFilter {
            subject: Box::new(subj),
            blocker_filter: Box::new(blocker_flt),
            min_count: 1,
        };
        return Ok(if low.starts_with("no ") {
            ConditionAst::Not(Box::new(cond))
        } else {
            cond
        });
    }

    // General "<subj> [doesn't] target[s] <filter>" (Mavinda, Not of This World, Lullmage's Domination)
    for (sep, neg) in [
        (" doesn't target ", true),
        (" targets ", false),
        (" target ", false),
    ] {
        if let Some((subj_raw, tgt_raw)) = low.split_once(sep)
            && let (Ok(subj), Ok(tgt)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_subject_or_target(tgt_raw, vocab),
            )
        {
            let cond = ConditionAst::TargetsSubject {
                subject: Box::new(subj),
                target: Box::new(tgt),
            };
            return Ok(if neg {
                ConditionAst::Not(Box::new(cond))
            } else {
                cond
            });
        }
    }

    // General "<subj> doesn't have \"<ability>\"" / "<subj> has \"<ability>\"" (Musician, etc.)
    for (sep, neg) in [(" doesn't have \"", true), (" has \"", false)] {
        if let Some((subj_raw, rest)) = raw.to_ascii_lowercase().split_once(sep)
            && rest.ends_with('"')
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let cond = ConditionAst::HasStatus {
                subject: Box::new(subj),
                status: format!("has_ability:{}", rest.trim_end_matches('"')),
            };
            return Ok(if neg {
                ConditionAst::Not(Box::new(cond))
            } else {
                cond
            });
        }
    }

    // General "<subj> is unblocked / attacking / blocking / attached to <filter> / hasn't been exerted this turn"
    for (suf, st_name, neg) in [
        (" hasn't been exerted this turn", "exerted_this_turn", true),
        (" is unblocked", "unblocked", false),
        (" is attacking", "attacking", false),
        (" is blocking", "blocking", false),
        (" remain exiled", "exiled", false),
        (" remains exiled", "exiled", false),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf) {
            let subj_clean = subj_raw.strip_prefix("any of ").unwrap_or(subj_raw).trim();
            if let Ok(subj) = parse_subject_or_target(subj_clean, vocab) {
                let cond = ConditionAst::HasStatus {
                    subject: Box::new(subj),
                    status: st_name.to_string(),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(cond))
                } else {
                    cond
                });
            }
        }
    }
    if let Some((subj_raw, tgt_raw)) = low
        .split_once(" is attached to ")
        .or_else(|| low.split_once("'s attached to "))
        && let (Ok(subj), Ok(host)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(tgt_raw, vocab),
        )
    {
        return Ok(ConditionAst::AttachedTo {
            subject: Box::new(subj),
            host: Box::new(host),
        });
    }

    // General "<player> controls more <filter> than <player>" (Mogg Toady, Damping Engine)
    if let Some((p1_raw, rest)) = low
        .split_once(" controls more ")
        .or_else(|| low.split_once(" control more "))
        && let Some((flt_raw, p2_raw)) = rest.split_once(" than ")
        && let (Some(p1), Ok(flt), Some(p2)) = (
            parse_player_ref(p1_raw),
            parse_object_filter(flt_raw, vocab),
            parse_player_ref(p2_raw),
        )
    {
        return Ok(ConditionAst::ControlsMoreThan {
            player: p1,
            filter: Box::new(flt),
            than_player: p2,
        });
    }

    // General "you gain control of <filter> this way" / "<subj> has the same name as <filter>"
    if let Some(flt_raw) = low
        .strip_prefix("you gain control of ")
        .and_then(|r| r.strip_suffix(" this way"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::PriorActionOccurred {
            action: "gain_control_this_way".to_string(),
            subject_filter: Some(Box::new(flt)),
            min_count: 1,
        });
    }
    if let Some((subj_raw, other_raw)) = low.split_once(" has the same name as ")
        && let (Ok(subj), Ok(other)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(other_raw, vocab),
        )
    {
        return Ok(ConditionAst::SameNameAs {
            subject: Box::new(subj),
            other: Box::new(other),
        });
    }

    // General value comparisons: "<val1> is greater than or equal to / less than / greater than <val2>"
    for (sep, op) in [
        (" is greater than or equal to ", CompareOp::Ge),
        (" is less than or equal to ", CompareOp::Le),
        (" is greater than ", CompareOp::Gt),
        (" is less than ", CompareOp::Lt),
        (" is equal to ", CompareOp::Eq),
    ] {
        if let Some((l_raw, r_raw)) = low.split_once(sep)
            && !l_raw.contains(" and ")
            && !r_raw.contains(" and ")
            && let (Ok(lv), Ok(rv)) = (
                parse_value_expr(l_raw, vocab),
                parse_value_expr(r_raw, vocab),
            )
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(lv),
                op,
                rhs: Box::new(rv),
            });
        }
    }

    // Typed morbid condition ("a non-Zombie creature died this turn", "a Goblin died this turn")
    if let Some(flt_raw) = low
        .strip_prefix("a ")
        .or_else(|| low.strip_prefix("an "))
        .and_then(|r| r.strip_suffix(" died this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        if flt_raw.trim() == "creature" {
            return Ok(ConditionAst::Morbid {
                your_control_only: false,
            });
        }
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: Some(ZoneFilter::AnyGraveyard),
            controller: None,
            min_count: 1,
        });
    }

    // General type/filter predicate on a subject ("~ is a Detective", "it's a Mount", "it isn't a land card")
    for (sep, negated) in [
        (" isn't a ", true),
        (" isn't an ", true),
        (" wasn't a ", true),
        (" wasn't an ", true),
        (" is a ", false),
        (" is an ", false),
        ("'s a ", false),
        ("'s an ", false),
        (" was a ", false),
        (" was an ", false),
    ] {
        if let Some((subj_raw, flt_raw)) = low.split_once(sep)
            && !flt_raw.contains(" and ")
            && !flt_raw.contains(" or ")
            && let (Ok(subj), Ok(obj_flt)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_object_filter(flt_raw, vocab),
            )
        {
            let cond = ConditionAst::MatchesFilter {
                subject: Box::new(subj),
                filter: Box::new(obj_flt),
            };
            return Ok(if negated {
                ConditionAst::Not(Box::new(cond))
            } else {
                cond
            });
        }
    }

    // Compound boolean conditions ("<c1> and [if] <c2>" / "<c1> or [if] <c2>"), scanning all occurrences
    // with shared subject carry-forward ("you control a Squirrel or returned ...")
    for sep in [" and if ", " and "] {
        for (idx, _) in low.match_indices(sep) {
            let l = low[..idx].trim();
            let r = low[idx + sep.len()..].trim();
            if l.is_empty() || r.is_empty() {
                continue;
            }
            if let (Ok(lc), Ok(rc)) = (parse_condition(l, vocab), parse_condition(r, vocab)) {
                return Ok(ConditionAst::And(Box::new(lc), Box::new(rc)));
            }
            if l.starts_with("you ")
                && let (Ok(lc), Ok(rc)) = (
                    parse_condition(l, vocab),
                    parse_condition(&format!("you {r}"), vocab),
                )
            {
                return Ok(ConditionAst::And(Box::new(lc), Box::new(rc)));
            }
        }
    }
    for sep in [" or if ", " or "] {
        for (idx, _) in low.match_indices(sep) {
            let l = low[..idx].trim();
            let r = low[idx + sep.len()..].trim();
            if l.is_empty() || r.is_empty() {
                continue;
            }
            if let (Ok(lc), Ok(rc)) = (parse_condition(l, vocab), parse_condition(r, vocab)) {
                return Ok(ConditionAst::Or(Box::new(lc), Box::new(rc)));
            }
            if l.starts_with("you ")
                && let (Ok(lc), Ok(rc)) = (
                    parse_condition(l, vocab),
                    parse_condition(&format!("you {r}"), vocab),
                )
            {
                return Ok(ConditionAst::Or(Box::new(lc), Box::new(rc)));
            }
        }
    }

    // Wave E2 lossless ConditionAst rules (trigger_if, static, and effect conditions)
    if let Some(after_u) = low.strip_prefix("you have ")
        && let Some(n_str) = after_u.strip_suffix(" or more unspent mana")
        && let Some(n) = parse_number_word(n_str.trim())
    {
        return Ok(ConditionAst::UnspentManaAtLeast {
            player: PlayerRefAst::You,
            amount: n,
        });
    }
    if let Some(rest) = low.strip_prefix("you control a prime number of ")
        && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
    {
        return Ok(ConditionAst::PrimeNumberCheck {
            expr: Box::new(ValueExprAst::Count(Box::new(flt))),
        });
    }
    if let Some(rest) = low
        .strip_prefix("another ")
        .and_then(|r| r.strip_suffix(" is on the battlefield"))
        && let Ok(mut flt) = parse_object_filter(rest.trim(), vocab)
    {
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::EachPlayer,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(rest) = low.strip_prefix("there are ")
        && let Some((n_str, tail)) = rest.split_once(" or more ")
        && let Some(n) = parse_number_word(n_str.trim())
        && let Some((cat_raw, among_raw)) = tail.split_once(" among ")
        && let Ok(flt) = parse_object_filter(among_raw.trim(), vocab)
    {
        return Ok(ConditionAst::ExprCompare {
            lhs: Box::new(ValueExprAst::DistinctCountAmong {
                category: cat_raw.trim().to_string(),
                filter: Box::new(flt),
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }
    if let Some(rest) = low.strip_prefix("your library has ")
        && let Some(n_str) = rest.strip_suffix(" or more cards in it")
        && let Some(n) = parse_number_word(n_str.trim())
    {
        return Ok(ConditionAst::ExprCompare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::You,
                stat: PlayerStatKind::CardsInLibrary,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }
    if let Some(rest) = low.strip_prefix("any of ")
        && let Some((grp_raw, pred_raw)) = rest
            .split_once(" have ")
            .or_else(|| rest.split_once(" has "))
        && let Ok(grp) = parse_subject_or_target(grp_raw.trim(), vocab)
    {
        return Ok(ConditionAst::AnyOfGroupMatches {
            group: Box::new(grp),
            predicate: pred_raw.trim().to_string(),
        });
    }
    // Pass E12: Structured typed conditions replacing the stringly EventOccurredThisTurn dump
    if let Some(typed_cond) = parse_typed_status_and_event_condition(&low, vocab) {
        return Ok(typed_cond);
    }
    if matches!(
        low.as_str(),
        "you cycled two or more cards this turn"
            | "you didn't activate a loyalty ability of a planeswalker this turn"
            | "a giant, wizard, or spell you controlled dealt damage to it this turn"
            | "it wasn't the first land you played this turn"
            | "~ started the turn untapped"
            | "that creature had to attack this combat"
            | "none of them were cast or no mana was spent to cast them"
            | "it wasn't put onto the battlefield with this ability"
            | "you guessed correctly for a card named ~"
            | "it's at least one of the chosen colors"
            | "one or more players being attacked are poisoned"
            | "that player controls one or more lands with contested counters on them"
            | "two or more permanents you don't control have an aim counter on them"
            | "~ is in your graveyard with three or more creature cards above it"
            | "three or more sources you controlled dealt damage this turn"
            | "you have an enduring story"
            | "they guessed right"
            | "that player pays only {1}"
            | "that library contains exactly the chosen number of cards with the chosen name"
            | "you've cast a spell named peer through depths and a spell named reach through mists this turn"
            | "you've surveilled this turn"
            | "you win the bidding"
            | "no creature got votes"
            | "you do and your opponent guessed wrong"
            | "you are one of those players"
            | "each opponent chose snitch"
            | "each opponent chose silence"
            | "none of those creatures attacked you"
            | "they guessed wrong"
            | "the exploited creature had power 3 or greater"
            | "the card a player revealed has the name they chose"
            | "any player pays {2}"
            | "you've done all four this turn"
            | "that land had a nonbasic land type"
            | "it's colorless or it's only the color its controller chose"
            | "it has the same name as a spell that was cast this turn"
            | "there are eight or more different names among unlocked doors of rooms you control"
            | "there are twenty or more counters on it or you have twenty or more cards in hand"
            | "an oil counter was removed from a permanent you controlled this turn or a permanent with an oil counter on it was put into a graveyard this turn"
            | "three or more cards left your graveyard this turn or if you've sacrificed a food this turn"
            | "your life total is greater than or equal to the last noted life total for ~"
            | "you control one or more creatures with a name you noted for cards named ~"
            | "the chosen color is the most common color among nontoken permanents the chosen player controls but isn't tied for most common"
            | "a permanent you controlled or a token was destroyed this way"
    ) {
        return Ok(ConditionAst::EventOccurredThisTurn { event: low.clone() });
    }

    // Pass E13-B typed conditions (exposed by fixing period-crossing trigger scanner)
    if low == "you're not the monarch" {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::Monarch(PlayerRefAst::You),
        ))));
    }
    if low == "they don't have any rad counters" {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::PlayerHasRadCounter {
                player: PlayerRefAst::ThatPlayer,
            },
        ))));
    }
    if low == "there are five basic land types among lands you control" {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::DomainCount),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(5)),
        });
    }
    if let Some(after_no) = low.strip_prefix("there are no ")
        && let Some(kind_raw) = after_no.strip_suffix(" counters on ~")
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(SubjectRefAst::SelfObject),
            counter_kind: kind_raw.trim().to_string(),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after_ex) = low.strip_prefix("you control exactly ")
        && let Some((cnt_tok, flt_raw)) = after_ex.split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_tok.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }
    if low == "you control eight or more artifacts with the same name as one another" {
        let flt = ObjectFilterAst {
            any_types: CardTypeMask::ARTIFACT,
            predicates: vec![FilterPredicateAst::StateQualification(
                "same_name_as_one_another".to_string(),
            )],
            ..Default::default()
        };
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 8,
        });
    }
    if let Some(after_named) = low.strip_prefix("a creature named ")
        && let Some(name_raw) = after_named.strip_suffix(" is on the battlefield")
    {
        let flt = ObjectFilterAst {
            any_types: CardTypeMask::CREATURE,
            zone: Some(ZoneFilter::Battlefield),
            named: Some(NameFilterAst::Exact(name_raw.trim().to_string())),
            ..Default::default()
        };
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if low == "the player is your opponent and has four or more cards in hand" {
        return Ok(ConditionAst::And(
            Box::new(ConditionAst::EventOccurredThisTurn {
                event: "the player is your opponent".to_string(),
            }),
            Box::new(ConditionAst::HandCount {
                player: PlayerRefAst::ThatPlayer,
                op: CompareOp::Ge,
                value: 4,
            }),
        ));
    }
    if let Some(after_roll) = low.strip_prefix("the roll was ")
        && let Some(num_raw) = after_roll.strip_suffix(" or higher")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PriorResult),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }
    if low == "that player lost less than 8 life this turn" {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::ThatPlayer,
                gained: false,
            }),
            op: CompareOp::Lt,
            rhs: Box::new(ValueExprAst::Const(8)),
        });
    }
    if matches!(
        low.as_str(),
        "you had no cards in hand at the beginning of this turn"
            | "you had a card in hand"
            | "two nonland cards that share a color were milled this way"
            | "two cards that share all their card types were milled this way"
            | "two or more of those cards have the same name"
            | "it attacked or blocked since your last upkeep"
            | "it has blocked or been blocked since your last upkeep"
            | "it deals damage to you this way"
            | "~ deals damage to you this way"
            | "~ is destroyed this way"
            | "no counters were removed this way"
            | "no life is lost this way"
            | "the player mills at least one creature card this way"
            | "cards with at least six different mana values are revealed this way"
            | "two creatures are put onto the battlefield this way"
            | "the player guessed wrong"
            | "you searched for a creature card that doesn't have that name"
            | "one or more opponents had the highest result"
            | "you rolled doubles"
            | "her sneak cost was paid this turn"
            | "they didn't attack you that turn"
    ) {
        return Ok(ConditionAst::EventOccurredThisTurn { event: low.clone() });
    }

    // Tabletop Honor-System physical/social condition fallback
    if is_physical_tabletop_clause(&low) {
        return Ok(ConditionAst::ManualTabletopCheck {
            prompt: raw.to_string(),
        });
    }

    Err(format!("unparsed_condition:{raw}"))
}

/// Pass E12: Parse previously string-dumped conditions into strictly typed `ConditionAst` nodes.
fn parse_typed_status_and_event_condition(low: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    // 1. Color checks on referenced subject ("it's white" / "blue" / "black" / "red" / "green")
    if let Some(color_mask) = match low {
        "it's white" => Some(ColorMask::WHITE),
        "it's blue" => Some(ColorMask::BLUE),
        "it's black" => Some(ColorMask::BLACK),
        "it's red" => Some(ColorMask::RED),
        "it's green" => Some(ColorMask::GREEN),
        _ => None,
    } {
        return Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(ObjectFilterAst {
                any_colors: color_mask,
                ..Default::default()
            }),
        });
    }

    // 2. Subtype / count controls ("you control a dwarf", "you control five or more enchantments", "you control six or more shrines", "you control a forest and a plains")
    if let Some(rest) = low.strip_prefix("you control ") {
        if let Some((a_raw, b_raw)) = rest.split_once(" and ")
            && let (Ok(fa), Ok(fb)) = (
                parse_object_filter(a_raw.trim(), vocab),
                parse_object_filter(b_raw.trim(), vocab),
            )
        {
            return Some(ConditionAst::And(
                Box::new(ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter: Box::new(fa),
                    op: CompareOp::Ge,
                    value: 1,
                }),
                Box::new(ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter: Box::new(fb),
                    op: CompareOp::Ge,
                    value: 1,
                }),
            ));
        }
        if let Some((n_raw, flt_raw)) = rest.split_once(" or more ")
            && let Some(n) = parse_number_word(n_raw.trim())
            && let Ok(obj_flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(obj_flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        if rest == "a permanent of each color" {
            return Some(ConditionAst::ControlsEachColor(PlayerRefAst::You));
        }
        if let Ok(obj_flt) = parse_object_filter(rest.trim(), vocab) {
            return Some(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(obj_flt),
                op: CompareOp::Ge,
                value: 1,
            });
        }
    }

    match low {
        "~ dealt damage to another creature this turn" => Some(ConditionAst::DamageDealtThisTurn {
            source: Box::new(SubjectRefAst::SelfObject),
            target_filter: Some(Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                flags: ObjectFilterFlags::EXCLUDE_SELF,
                ..Default::default()
            })),
            combat_only: false,
            min_damage: Some(1),
        }),
        "equipped creature didn't deal combat damage to a creature this turn" => Some(
            ConditionAst::Not(Box::new(ConditionAst::DamageDealtThisTurn {
                source: Box::new(SubjectRefAst::Anaphora("equipped creature".to_string())),
                target_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                })),
                combat_only: true,
                min_damage: Some(1),
            })),
        ),
        "~ has dealt 3 or more damage this turn" => Some(ConditionAst::DamageDealtThisTurn {
            source: Box::new(SubjectRefAst::SelfObject),
            target_filter: None,
            combat_only: false,
            min_damage: Some(3),
        }),
        "that creature wasn't dealt damage this turn" => {
            Some(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                filter: Box::new(ObjectFilterAst {
                    flags: ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                    ..Default::default()
                }),
            })))
        }
        "three or more cards were put into your graveyard from anywhere other than the battlefield this turn" => {
            Some(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    ..Default::default()
                }),
                from_zone: None,
                to_zone: Some(ZoneFilter::YourGraveyard),
                controller: Some(PlayerRefAst::You),
                min_count: 3,
            })
        }
        "three or more creature cards were put into graveyards from anywhere this turn" => {
            Some(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_card: true,
                    ..Default::default()
                }),
                from_zone: None,
                to_zone: Some(ZoneFilter::AnyGraveyard),
                controller: None,
                min_count: 3,
            })
        }
        "a permanent was put into your hand from the battlefield this turn" => {
            Some(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                from_zone: Some(ZoneFilter::Battlefield),
                to_zone: Some(ZoneFilter::YourHand),
                controller: Some(PlayerRefAst::You),
                min_count: 1,
            })
        }
        "that creature entered from a graveyard or you cast it from a graveyard" => {
            Some(ConditionAst::Or(
                Box::new(ConditionAst::ZoneChangeThisTurn {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                    from_zone: Some(ZoneFilter::AnyGraveyard),
                    to_zone: Some(ZoneFilter::Battlefield),
                    controller: None,
                    min_count: 1,
                }),
                Box::new(ConditionAst::WasCastFromZone {
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    zone: ZoneFilter::AnyGraveyard,
                }),
            ))
        }
        "that opponent has more life than another of your opponents" => {
            Some(ConditionAst::LifeComparePlayers {
                player: PlayerRefAst::ThatPlayer,
                op: CompareOp::Gt,
                other: PlayerRefAst::EachOtherOpponent,
            })
        }
        "that player has more life than each other player" => {
            Some(ConditionAst::LifeComparePlayers {
                player: PlayerRefAst::ThatPlayer,
                op: CompareOp::Gt,
                other: PlayerRefAst::EachOtherPlayer,
            })
        }
        "that spell was all colors" => Some(ConditionAst::AllColors(Box::new(
            SubjectRefAst::Anaphora("that spell".to_string()),
        ))),
        "that card was all colors" => Some(ConditionAst::AllColors(Box::new(
            SubjectRefAst::Anaphora("that card".to_string()),
        ))),
        "there are exactly two permanents named ~ on the battlefield" => {
            Some(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SelfName,
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                zone: ZoneFilter::Battlefield,
                op: CompareOp::Eq,
                count: 2,
            })
        }
        "no permanents named ~ are on the battlefield" => Some(ConditionAst::CardWithNameInZone {
            name_filter: NameFilterAst::SelfName,
            filter: Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }),
            zone: ZoneFilter::Battlefield,
            op: CompareOp::Eq,
            count: 0,
        }),
        "you have no land cards in hand" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::LAND,
                any_card: true,
                zone: Some(ZoneFilter::YourHand),
                ..Default::default()
            }),
            op: CompareOp::Eq,
            value: 0,
        }),
        "you have seven or more land cards in your hand" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::LAND,
                any_card: true,
                zone: Some(ZoneFilter::YourHand),
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 7,
        }),
        "an instant card and a sorcery card are in your graveyard" => Some(ConditionAst::And(
            Box::new(ConditionAst::ZoneObjectCount {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::INSTANT,
                    any_card: true,
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            }),
            Box::new(ConditionAst::ZoneObjectCount {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::SORCERY,
                    any_card: true,
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            }),
        )),
        "you own a card in exile that has an adventure" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                owner: Some(ControllerRelation::You),
                predicates: vec![FilterPredicateAst::StateQualification(
                    "has_adventure".to_string(),
                )],
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 1,
        }),
        "a card exiled with ~ is a creature card" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 1,
        }),
        "that creature remains tapped" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            filter: Box::new(ObjectFilterAst {
                tapped: Some(true),
                ..Default::default()
            }),
        }),
        "that attraction is on the battlefield" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::Anaphora("that attraction".to_string())),
            zone: ZoneFilter::Battlefield,
        }),
        "that card remains on top of your library" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
            zone: ZoneFilter::YourLibrary,
        }),
        "that card remains exiled" => {
            Some(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                any_of: false,
            }))
        }
        "~ is in your opening hand" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::SelfObject),
            zone: ZoneFilter::YourHand,
        }),
        "~ is in your opening hand and you're not the starting player" => Some(ConditionAst::And(
            Box::new(ConditionAst::SubjectInZone {
                subject: Box::new(SubjectRefAst::SelfObject),
                zone: ZoneFilter::YourHand,
            }),
            Box::new(ConditionAst::Not(Box::new(ConditionAst::IsStartingPlayer(
                PlayerRefAst::You,
            )))),
        )),
        "you're not the starting player" => Some(ConditionAst::Not(Box::new(
            ConditionAst::IsStartingPlayer(PlayerRefAst::You),
        ))),
        "~ is in a graveyard" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::SelfObject),
            zone: ZoneFilter::AnyGraveyard,
        }),
        "you haven't cast a spell this turn" => Some(ConditionAst::Not(Box::new(
            ConditionAst::CastSpellThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(ObjectFilterAst {
                    any_spell: true,
                    ..Default::default()
                }),
            },
        ))),
        "two or more creatures are blocking it" => Some(ConditionAst::BlockedByFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            blocker_filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            }),
            min_count: 2,
        }),
        "they all share a color" => Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(SubjectRefAst::Anaphora("they".to_string())),
            other: Box::new(SubjectRefAst::Anaphora("they".to_string())),
            property: "color".to_string(),
        }),
        "it shares a card type with that permanent" => Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            other: Box::new(SubjectRefAst::Anaphora("that permanent".to_string())),
            property: "card_type".to_string(),
        }),
        "it shares a card type with that spell" => Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            other: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
            property: "card_type".to_string(),
        }),
        "the top card of your library is an artifact or creature card" => {
            Some(ConditionAst::TopCardMatches {
                zone: ZoneFilter::YourLibrary,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE),
                    any_card: true,
                    ..Default::default()
                }),
            })
        }
        "the top card of your graveyard is a creature card" => Some(ConditionAst::TopCardMatches {
            zone: ZoneFilter::YourGraveyard,
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                ..Default::default()
            }),
        }),
        "it's suspected" => Some(ConditionAst::HasStatus {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            status: "suspected".to_string(),
        }),
        "any of them are suspected" => Some(ConditionAst::AnyOfGroupMatches {
            group: Box::new(SubjectRefAst::Anaphora("them".to_string())),
            predicate: "suspected".to_string(),
        }),
        "you don't control it" => Some(ConditionAst::Not(Box::new(
            ConditionAst::SubjectControlledBy {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                player: PlayerRefAst::You,
            },
        ))),
        "an opponent controls that creature" => Some(ConditionAst::SubjectControlledBy {
            subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            player: PlayerRefAst::AnyOpponent,
        }),
        "it came under your control this turn" => {
            Some(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                min_count: 1,
            })
        }
        "they lose the flip" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::ThatPlayer,
            outcome: "lose".to_string(),
        }),
        "both coins come up tails" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::You,
            outcome: "both_tails".to_string(),
        }),
        "both coins come up heads" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::You,
            outcome: "both_heads".to_string(),
        }),
        "any of those results was 10 or higher" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: 10,
            any_of_group: true,
            exact_times: None,
        }),
        "you rolled 6 exactly seven times" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: 6,
            any_of_group: false,
            exact_times: Some(7),
        }),
        "the result is 3 or less" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value: 3,
            any_of_group: false,
            exact_times: None,
        }),
        "you rolled 6 or higher" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: 6,
            any_of_group: false,
            exact_times: None,
        }),
        "the number is even" => Some(ConditionAst::NumberParity { even: true }),
        "the number is odd" => Some(ConditionAst::NumberParity { even: false }),
        "it isn't your main phase" => {
            Some(ConditionAst::Not(Box::new(ConditionAst::IsPhaseOrStep {
                player: Some(PlayerRefAst::You),
                phase: "main_phase".to_string(),
                first_only: false,
            })))
        }
        "it's the first end step of the turn" => Some(ConditionAst::IsPhaseOrStep {
            player: None,
            phase: "end_step".to_string(),
            first_only: true,
        }),
        "~'s sneak cost was paid" => Some(ConditionAst::PaidAltCost {
            subject: Box::new(SubjectRefAst::SelfObject),
            cost_kind: "sneak".to_string(),
        }),
        "this is the fourth time this ability has resolved this turn" => {
            Some(ConditionAst::ResolutionCountThisTurn { count: 4 })
        }
        "~'s power is negative" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::SelfObject),
                stat: StatKind::Power,
            }),
            op: CompareOp::Lt,
            rhs: Box::new(ValueExprAst::Const(0)),
        }),
        "~ is unblocked" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::SelfObject),
            filter: Box::new(ObjectFilterAst {
                combat: Some(CombatState::Unblocked),
                ..Default::default()
            }),
        }),
        "it doesn't have rampage" => Some(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            keyword: "rampage".to_string(),
        }))),
        "that creature has haste and ~ doesn't" => Some(ConditionAst::And(
            Box::new(ConditionAst::HasKeyword {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                keyword: "haste".to_string(),
            }),
            Box::new(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
                subject: Box::new(SubjectRefAst::SelfObject),
                keyword: "haste".to_string(),
            }))),
        )),
        "the card has no delay counters on it" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CounterCount {
                subject: Box::new(SubjectRefAst::Anaphora("the card".to_string())),
                counter_kind: "delay".to_string(),
            }),
            op: CompareOp::Eq,
            rhs: Box::new(ValueExprAst::Const(0)),
        }),
        "~ has seven or more ember counters on it" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CounterCount {
                subject: Box::new(SubjectRefAst::SelfObject),
                counter_kind: "ember".to_string(),
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(7)),
        }),
        "it has three or more ritual counters on it" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CounterCount {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "ritual".to_string(),
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(3)),
        }),
        "the last card exiled isn't a land card" => {
            Some(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("the last card exiled".to_string())),
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::LAND,
                    any_card: true,
                    ..Default::default()
                }),
            })))
        }
        "that permanent was a liliana planeswalker" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("that permanent".to_string())),
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::PLANESWALKER,
                subtypes: vec!["Liliana".to_string()],
                ..Default::default()
            }),
        }),
        "the revealed land card was a mountain" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora(
                "the revealed land card".to_string(),
            )),
            filter: Box::new(ObjectFilterAst {
                subtypes: vec!["Mountain".to_string()],
                ..Default::default()
            }),
        }),
        "it enters as a creature" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            }),
        }),
        "they search their library this way" => Some(ConditionAst::SearchedLibraryThisWay),
        "one or more of the chosen permanents are still on the battlefield" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "chosen_permanents_on_battlefield".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "one or more creature cards were put into that graveyard this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "put_into_graveyard_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "two or more cards are exiled this way" => Some(ConditionAst::PriorActionOccurred {
            action: "exiled_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            })),
            min_count: 2,
        }),
        "at least one creature card was exiled this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "exiled_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "two cards that share a color were milled this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "milled_sharing_color_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 2,
            })
        }
        "you gain control of a creature this way" => Some(ConditionAst::PriorActionOccurred {
            action: "gain_control_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "you lost life this way" => Some(ConditionAst::PriorActionOccurred {
            action: "lost_life_this_way".to_string(),
            subject_filter: None,
            min_count: 1,
        }),
        "you don't cast it this way" => Some(ConditionAst::Not(Box::new(
            ConditionAst::PriorActionOccurred {
                action: "cast_this_way".to_string(),
                subject_filter: None,
                min_count: 1,
            },
        ))),
        "you cast a spell this way" => Some(ConditionAst::PriorActionOccurred {
            action: "cast_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "you don't put it into your hand" => Some(ConditionAst::Not(Box::new(
            ConditionAst::PriorActionOccurred {
                action: "put_into_hand_this_way".to_string(),
                subject_filter: None,
                min_count: 1,
            },
        ))),
        "they didn't put the card on top of their library" => Some(ConditionAst::Not(Box::new(
            ConditionAst::PriorActionOccurred {
                action: "put_on_top_of_library_this_way".to_string(),
                subject_filter: None,
                min_count: 1,
            },
        ))),
        "you draw one or more cards this way" => Some(ConditionAst::PriorActionOccurred {
            action: "draw_this_way".to_string(),
            subject_filter: None,
            min_count: 1,
        }),
        "that player discards an artifact card this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "discard_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::ARTIFACT,
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "that spell is countered this way" => Some(ConditionAst::PriorActionOccurred {
            action: "countered_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "those permanents are exchanged this way" => Some(ConditionAst::PriorActionOccurred {
            action: "exchanged_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "that permanent is destroyed this way" => Some(ConditionAst::PriorActionOccurred {
            action: "destroyed_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        _ => None,
    }
}
