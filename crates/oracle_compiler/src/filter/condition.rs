//! Compositional boolean condition (`ConditionAst`) parser covering game markers,
//! numeric comparisons, player/object status checks, and turn-history predicates.

use super::mana_token::split_named_card_list;
use super::object::parse_object_filter;
use super::subject::{parse_keyword_list, parse_player_ref, parse_subject_or_target};
use super::value::{parse_possessive_player, parse_value_expr};
use crate::ast::{
    CardTypeMask, CombatState, CombatSwapPerspective, CompareOp, ConditionAst, ControllerRelation,
    FilterPredicateAst, GameMarker, NameFilterAst, ObjectFilterAst, ObjectFilterFlags,
    PlayerRefAst, PlayerStatKind, PriorActionRefKind, Rounding, StatKind, SubjectRefAst,
    SupertypeMask, ValueExprAst, ZoneFilter,
};
use crate::combinators::{
    anaphora, any_card_flt, any_perm, any_spell_flt, box_anaphora, card_flt, cond_action, cond_and,
    cond_cmp, cond_controls, cond_counter_cmp, cond_has_kw, cond_marker, cond_matches, cond_not,
    cond_or, cond_prior, cond_status, cond_zone_ch, cond_zone_count, grp, obj_type,
    rsplit_once_any, self_obj, split_once_any, strip_prefix_any, strip_suffix_any, trim_punct,
    val_div, val_obj_stat, val_player_stat, vconst,
};
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
            return Some(cond_cmp(
                ValueExprAst::CountDrawnThisWay(PlayerRefAst::You),
                CompareOp::Eq,
                ValueExprAst::Const(0),
            ));
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
            let mut cr = obj_type(CardTypeMask::CREATURE);
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
        return Some(cond_zone_count(flt, CompareOp::Ge, 1));
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
        return Some(cond_zone_count(
            flt,
            CompareOp::Ge,
            u16::try_from(n.max(1)).unwrap_or(1),
        ));
    }

    // "its mana value is equal to <val>" (Glissa Sunseeker)
    if let Some(val_raw) = low.strip_prefix("its mana value is equal to ")
        && let Ok(rhs) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(cond_cmp(
            ValueExprAst::ObjectStat {
                subject: box_anaphora("it"),
                stat: StatKind::ManaValue,
            },
            CompareOp::Eq,
            rhs,
        ));
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
        return Some(cond_and(lhs, rhs));
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
    let (head, last) = rsplit_once_any(&trimmed, &[", and ", " and "])?;
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
        .map(|filter| cond_controls(player, filter, CompareOp::Ge, 1));
    let first = iter.next()?;
    Some(iter.fold(first, cond_and))
}

pub fn parse_condition(phrase: &str, vocab: &Vocabulary) -> Result<ConditionAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_condition".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = strip_suffix_any(&low_full, &[" as you cast ~", " as you cast this spell"])
        .unwrap_or(&low_full)
        .to_string();

    // Wave E6.2: Compound "if <c1> and if <c2>" / "if <c1> or if <c2>" conditions
    if let Some((c1, c2)) = low.split_once(" and if ")
        && let (Ok(lhs), Ok(rhs)) = (parse_condition(c1, vocab), parse_condition(c2, vocab))
    {
        return Ok(cond_and(lhs, rhs));
    }
    if let Some((c1, c2)) = low.split_once(" or if ")
        && let (Ok(lhs), Ok(rhs)) = (parse_condition(c1, vocab), parse_condition(c2, vocab))
    {
        return Ok(cond_or(lhs, rhs));
    }
    if let Some(val_raw) = low.strip_prefix("the result is equal to or less than ")
        && let Ok(rhs) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Ok(cond_cmp(ValueExprAst::PriorResult, CompareOp::Le, rhs));
    }
    // Compositional "<subj>'s mana cost contains <sym>" / "its mana cost contains <sym>"
    if let Some((lhs_raw, sym_raw)) = low.split_once(" mana cost contains ")
        && sym_raw.trim().starts_with('{')
        && sym_raw.trim().ends_with('}')
    {
        let subj = match lhs_raw.trim() {
            "its" => Some(anaphora("it")),
            other => other
                .strip_suffix("'s")
                .and_then(|s| parse_subject_or_target(s.trim(), vocab).ok()),
        };
        if let Some(subject) = subj {
            let mut flt = any_card_flt();
            flt.predicates.push(FilterPredicateAst::HasManaSymbolInCost(
                sym_raw.trim().to_ascii_uppercase(),
            ));
            return Ok(cond_matches(subject, flt));
        }
    }

    // Wave E8 compositional typed conditions (GameMarker and Compare)
    if let Some(cond) = parse_marker_and_compare_condition(&low, vocab) {
        return Ok(cond);
    }

    // Compact declarative table for atomic game-state and turn-history markers.
    use CompareOp as Cmp;
    use ConditionAst as CA;
    use GameMarker as GM;
    use PlayerRefAst as P;
    const DIRECT_MARKERS: &[(&str, GM)] = &[
        (
            "~'s additional cost was paid",
            GM::AdditionalCostPaid { description: None },
        ),
        (
            "the two spells have the same mana value",
            GM::SameManaValueAsTriggerSpell,
        ),
        ("the player hasn't played the card", GM::HasNotPlayedOrCast),
        ("you haven't cast it", GM::HasNotPlayedOrCast),
        ("you haven't played it", GM::HasNotPlayedOrCast),
        ("you don't cast it", GM::HasNotPlayedOrCast),
        ("you don't play it", GM::HasNotPlayedOrCast),
        ("you didn't cast it", GM::HasNotPlayedOrCast),
        ("it wasn't cast", GM::HasNotPlayedOrCast),
        ("it is", GM::SameNameAsTarget),
        ("it has the same name as that card", GM::SameNameAsTarget),
        ("you descended this turn", GM::DescendedThisTurn),
        ("you have the city's blessing", GM::CitysBlessing),
        ("there is no monarch", GM::NoMonarch),
        ("you've completed a dungeon", GM::CompletedDungeon),
        ("it's day", GM::Day),
        ("it's night", GM::Night),
        ("it's neither day nor night", GM::NeitherDayNorNight),
        ("you have max speed", GM::MaxSpeed),
        ("you have the initiative", GM::Initiative(P::You)),
        (
            "you committed a crime this turn",
            GM::CommittedCrimeThisTurn,
        ),
        (
            "you've committed a crime this turn",
            GM::CommittedCrimeThisTurn,
        ),
        (
            "you gained or lost life this turn",
            GM::PlayerGainedOrLostLifeThisTurn { player: P::You },
        ),
        (
            "an opponent was dealt damage this turn",
            GM::OpponentDealtDamageThisTurn {
                noncombat_only: false,
            },
        ),
        (
            "an opponent has been dealt damage this turn",
            GM::OpponentDealtDamageThisTurn {
                noncombat_only: false,
            },
        ),
        (
            "an opponent was dealt noncombat damage this turn",
            GM::OpponentDealtDamageThisTurn {
                noncombat_only: true,
            },
        ),
        (
            "an opponent was dealt noncombat damage last turn",
            GM::OpponentDealtDamageThisTurn {
                noncombat_only: true,
            },
        ),
        (
            "you cast a spell this turn",
            GM::CastSpellsThisTurn {
                player: P::You,
                op: Cmp::Ge,
                value: 1,
                from_hand_only: false,
            },
        ),
        (
            "you've cast two or more spells this turn",
            GM::CastSpellsThisTurn {
                player: P::You,
                op: Cmp::Ge,
                value: 2,
                from_hand_only: false,
            },
        ),
        (
            "you cast two or more spells this turn",
            GM::CastSpellsThisTurn {
                player: P::You,
                op: Cmp::Ge,
                value: 2,
                from_hand_only: false,
            },
        ),
        (
            "you haven't cast a spell from your hand this turn",
            GM::CastSpellsThisTurn {
                player: P::You,
                op: Cmp::Eq,
                value: 0,
                from_hand_only: true,
            },
        ),
        (
            "this spell was cast from your graveyard",
            GM::CastFromGraveyard,
        ),
        ("~ was cast from your graveyard", GM::CastFromGraveyard),
        ("~ was cast from a graveyard", GM::CastFromGraveyard),
        ("it was cast from a graveyard", GM::CastFromGraveyard),
        ("~ was cast from exile", GM::CastFromExile),
        ("~ was foretold", GM::Foretold),
        ("it was foretold", GM::Foretold),
        ("~ wasn't cast from your hand", GM::CastFromOutsideHand),
        (
            "~ was cast from anywhere other than your hand",
            GM::CastFromOutsideHand,
        ),
        ("you didn't cast it from your hand", GM::CastFromOutsideHand),
        (
            "~ attacked or blocked this combat",
            GM::AttackedOrBlockedThisCombat,
        ),
        ("you control neither creature", GM::ControlsNeither),
        ("you control all three", GM::ControlsAllThree),
        ("you bargained", GM::Bargained),
        ("~ was bargained", GM::Bargained),
        ("it was bargained", GM::Bargained),
        ("it's bargained", GM::Bargained),
        ("~ is bargained", GM::Bargained),
        ("the gift was promised", GM::GiftPromised),
        ("you're attacking", GM::YouAreAttacking),
        ("you're defending", GM::YouAreDefending),
        (
            "two or more players have lost the game",
            GM::PlayersLostGame {
                op: Cmp::Ge,
                value: 2,
            },
        ),
        (
            "a nonland permanent left the battlefield this turn",
            GM::NonlandPermanentLeftThisTurn,
        ),
        ("a spell was warped this turn", GM::SpellWarpedThisTurn),
        ("~ was cast using teamwork", GM::CastUsingTeamwork),
        ("it's cast using teamwork", GM::CastUsingTeamwork),
        ("you have a full party", GM::FullParty),
        (
            "it's the first combat phase of the turn",
            GM::FirstCombatPhase,
        ),
        ("it isn't a mana ability", GM::NotManaAbility),
        (
            "~ is in the command zone or on the battlefield",
            GM::InCommandZoneOrBattlefield,
        ),
        ("~ is suspended", GM::IsSuspended),
        (
            "~ is paired with another creature",
            GM::PairedWithAnotherCreature,
        ),
        ("it's paired with a creature", GM::PairedWithAnotherCreature),
        ("~ is paired with a creature", GM::PairedWithAnotherCreature),
        (
            "no creatures are on the battlefield",
            GM::NoCreaturesOnBattlefield,
        ),
        ("it has a single target", GM::SingleTarget),
        ("that spell targets only ~", GM::SingleTarget),
        ("it targets only ~", GM::SingleTarget),
        (
            "it isn't being declared as an attacker",
            GM::NotDeclaredAttacker,
        ),
        (
            "you chose a creature other than ~ as your ring-bearer",
            GM::RingBearerOtherThanSelf,
        ),
        (
            "a card left your graveyard this turn",
            GM::CardLeftGraveyardThisTurn,
        ),
        (
            "one or more cards left your graveyard this turn",
            GM::CardLeftGraveyardThisTurn,
        ),
        (
            "you have more cards in hand than each opponent",
            GM::HandMoreThanEachOpponent,
        ),
        (
            "it entered from your graveyard or you cast it from your graveyard",
            GM::EnteredOrCastFromGraveyard,
        ),
        (
            "one or more of them entered from a graveyard or was cast from a graveyard",
            GM::AnyEnteredOrCastFromGraveyard,
        ),
        (
            "they entered or were cast from a graveyard",
            GM::AnyEnteredOrCastFromGraveyard,
        ),
        (
            "it shares a card type with the exiled card",
            GM::SharesCardTypeWithExiled,
        ),
        ("it targets a commander", GM::TargetsCommander),
        (
            "it has the same mana value as the discarded card",
            GM::SameManaValueAsDiscarded,
        ),
        (
            "it has the same mana value as the revealed card",
            GM::SameManaValueAsRevealed,
        ),
        ("you control ~", GM::ControlsSelf),
        ("~ remains tapped", GM::RemainsTapped),
        ("it remains tapped", GM::RemainsTapped),
        ("~ remains on the battlefield", GM::RemainsOnBattlefield),
        ("it's attacking alone", GM::AttackingAlone),
        ("damage is prevented this way", GM::DamagePreventedThisWay),
        (
            "it's attacking the player with the most life or tied for most life",
            GM::AttackingPlayerWithMostLife { or_tied: true },
        ),
        (
            "it's attacking the player with the most life",
            GM::AttackingPlayerWithMostLife { or_tied: false },
        ),
        (
            "no creatures attacked this turn",
            GM::DidNotAttackWithCreatureThisTurn {
                player: P::AnyPlayer,
            },
        ),
        (
            "you didn't attack with a creature this turn",
            GM::DidNotAttackWithCreatureThisTurn { player: P::You },
        ),
        ("that creature dies this way", GM::DiedThisWay),
        ("it dies this way", GM::DiedThisWay),
        ("it died this way", GM::DiedThisWay),
        ("a creature died this way", GM::DiedThisWay),
        (
            "a permanent's ability is countered this way",
            GM::AbilityCounteredThisWay {
                permanent_only: true,
            },
        ),
        (
            "you haven't added mana with this ability this turn",
            GM::AbilityActivatedThisTurn {
                op: Cmp::Eq,
                value: 0,
            },
        ),
        ("life was paid to activate it", GM::LifePaidToActivate),
        (
            "that player has another opponent who isn't being attacked",
            GM::HasUnattackedOpponent,
        ),
        ("you cast ~ during your main phase", GM::CastDuringMainPhase),
        (
            "you cast it during your main phase",
            GM::CastDuringMainPhase,
        ),
        ("it escaped", GM::Escaped),
        ("~ escaped", GM::Escaped),
        ("evidence was collected", GM::EvidenceCollected),
        (
            "you lost life last turn",
            GM::LostLifeLastTurn { player: P::You },
        ),
        (
            "the amount of mana spent to cast it was less than its mana value",
            GM::ManaSpentLessThanManaValue,
        ),
        (
            "the amount of mana spent to cast ~ was less than its mana value",
            GM::ManaSpentLessThanManaValue,
        ),
        (
            "you didn't cast a spell this turn",
            GM::DidNotCastThisTurn {
                player: P::You,
                filter: None,
            },
        ),
    ];
    if let Some((_, gm)) = DIRECT_MARKERS.iter().find(|(k, _)| *k == low) {
        return Ok(cond_marker(gm.clone()));
    }
    let direct_cond = match low.as_str() {
        "you revealed a dragon card or chose a dragon" => {
            Some(cond_marker(GM::AdditionalCostPaid {
                description: Some("revealed_or_chose_dragon".into()),
            }))
        }
        "~ was kicked" | "it was kicked" => Some(CA::WasKicked),
        "you cast it" | "you cast ~" => Some(CA::WasCast {
            by_you: true,
            from_hand: false,
        }),
        "~ was cast" | "it was cast" => Some(CA::WasCast {
            by_you: false,
            from_hand: false,
        }),
        "you cast it from your hand" | "~ was cast from your hand" => Some(CA::WasCast {
            by_you: true,
            from_hand: true,
        }),
        "no spells were cast last turn" => Some(CA::SpellsCastLastTurn {
            any_player_single: false,
            op: Cmp::Eq,
            value: 0,
        }),
        "a player cast two or more spells last turn" => Some(CA::SpellsCastLastTurn {
            any_player_single: true,
            op: Cmp::Ge,
            value: 2,
        }),
        "you search your library this way" => Some(CA::SearchedLibraryThisWay),
        "it's not a token"
        | "it isn't a token"
        | "that creature isn't a token"
        | "~ isn't a token"
        | "they're not a token"
        | "they aren't a token" => Some(CA::IsNotToken),
        "a creature died this turn" => Some(CA::Morbid {
            your_control_only: false,
        }),
        "a creature died under your control this turn" => Some(CA::Morbid {
            your_control_only: true,
        }),
        "you have no cards in hand" | "you have no cards in your hand" => Some(CA::HandCount {
            player: P::You,
            op: Cmp::Eq,
            value: 0,
        }),
        "it's your turn" | "it is your turn" => Some(CA::IsYourTurn),
        "it's not your turn" | "it isn't your turn" | "it's an opponent's turn" => {
            Some(cond_not(CA::IsYourTurn))
        }
        "it's not their turn" | "it isn't that player's turn" | "it's not that player's turn" => {
            Some(cond_not(CA::IsPlayersTurn(P::ThatPlayer)))
        }
        "you gained life this turn" | "you've gained life this turn" => {
            Some(CA::GainedLifeThisTurn)
        }
        "an opponent lost life this turn" | "opponents lost life this turn" => {
            Some(CA::OpponentLostLifeThisTurn)
        }
        "you attacked this turn" | "you attacked with a creature this turn" => {
            Some(CA::AttackedThisTurn)
        }
        "~ entered this turn" | "~ entered the battlefield this turn" => Some(CA::EnteredThisTurn),
        "~ didn't enter the battlefield this turn" | "~ didn't enter this turn" => {
            Some(cond_not(CA::EnteredThisTurn))
        }
        "you control your commander"
        | "you control a commander"
        | "you control a commander as you cast ~" => Some(CA::ControlsCommander),
        "tribute wasn't paid" => Some(CA::TributeNotPaid),
        "tribute was paid" => Some(cond_not(CA::TributeNotPaid)),
        "it shares a creature type with ~" => Some(CA::SharesTypeWithSelf),
        "you haven't completed tomb of annihilation" => {
            Some(cond_not(cond_marker(GM::CompletedDungeon)))
        }
        "a player lost life this turn" => Some(CA::LostLifeThisTurn {
            player: P::AnyPlayer,
            min_amount: 1,
        }),
        "a creature entered the battlefield under your control this turn"
        | "a creature entered under your control this turn" => {
            Some(CA::EnteredUnderYourControlThisTurn {
                filter: Box::new(obj_type(CardTypeMask::CREATURE)),
                min_count: 1,
            })
        }
        "you've cast a historic spell this turn" => Some(CA::CastSpellThisTurn {
            player: P::You,
            filter: Box::new(ObjectFilterAst {
                required_supertypes: SupertypeMask::HISTORIC,
                any_spell: true,
                ..Default::default()
            }),
        }),
        "there are no cards in your graveyard" => Some(CA::GraveyardCount {
            player: P::You,
            op: Cmp::Eq,
            value: 0,
        }),
        "the gift wasn't promised" => Some(cond_not(cond_marker(GM::GiftPromised))),
        "you don't put the card into your hand"
        | "you didn't put a card into your hand this way" => {
            Some(cond_marker(GM::DidNotPutToZone {
                zone: "hand".into(),
            }))
        }
        "you don't put the card onto the battlefield"
        | "you didn't put a card onto the battlefield this way"
        | "you don't put it onto the battlefield" => Some(cond_marker(GM::DidNotPutToZone {
            zone: "battlefield".into(),
        })),
        "chaos gets more votes or the vote is tied" => Some(cond_marker(GM::VoteResult {
            winning: "chaos_or_tie".into(),
        })),
        "planeswalk gets more votes" => Some(cond_marker(GM::VoteResult {
            winning: "planeswalk".into(),
        })),
        "it's on the battlefield" | "it is on the battlefield" => Some(cond_marker(GM::InZone {
            subject: box_anaphora("it"),
            zone: ZoneFilter::Battlefield,
        })),
        "its mana value is x" => Some(cond_cmp(
            val_obj_stat(anaphora("it"), StatKind::ManaValue),
            Cmp::Eq,
            ValueExprAst::ChosenX,
        )),
        "no mana was spent to cast that spell" | "no mana was spent to cast it" => Some(cond_cmp(
            ValueExprAst::ManaSpentToCast { mana_filter: None },
            Cmp::Eq,
            ValueExprAst::Const(0),
        )),
        "you won" | "you won the clash" | "you win the flip" | "you won the flip" => {
            Some(CA::WonContest)
        }
        "it didn't die" | "that creature didn't die" => {
            Some(cond_not(cond_marker(GM::DiedThisWay)))
        }
        "you've discarded a card this turn" | "you discarded a card this turn" => {
            Some(cond_action(P::You, "discard"))
        }
        "it has greater power or toughness than ~" => {
            Some(cond_marker(GM::GreaterPowerOrToughnessThan {
                subject: box_anaphora("it"),
                other: self_obj(),
            }))
        }
        "an opponent has more life than you" => Some(CA::OpponentMoreThanYou {
            metric: "life".into(),
            filter: None,
        }),
        "an opponent has more cards in hand than you" => Some(CA::OpponentMoreThanYou {
            metric: "cards_in_hand".into(),
            filter: None,
        }),
        "~ is on the stack" => Some(cond_status(SubjectRefAst::SelfObject, "on_stack")),
        "~ is in the command zone" => {
            Some(cond_status(SubjectRefAst::SelfObject, "in_command_zone"))
        }
        "it wasn't cast this way" | "you don't cast that card this way" => {
            Some(cond_not(cond_action(P::You, "cast_this_way")))
        }
        "a creature is attacking you" => Some(cond_controls(
            P::EachOpponent,
            ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                combat: Some(CombatState::Attacking),
                ..Default::default()
            },
            Cmp::Ge,
            1,
        )),
        "a player attacked you during their last turn"
        | "that player attacked you during their last turn" => {
            Some(cond_action(P::ThatPlayer, "attacked_you_last_turn"))
        }
        "it's the first instant spell, the first sorcery spell, or the first otter spell other than ~ you've cast this turn" => {
            Some(cond_marker(GM::IsNthSpellMatchingThisTurn {
                nth: 1,
                filters: vec![
                    ObjectFilterAst {
                        any_types: CardTypeMask::INSTANT,
                        any_spell: true,
                        ..Default::default()
                    },
                    ObjectFilterAst {
                        any_types: CardTypeMask::SORCERY,
                        any_spell: true,
                        ..Default::default()
                    },
                    ObjectFilterAst {
                        subtypes: vec!["Otter".into()],
                        any_spell: true,
                        flags: ObjectFilterFlags::EXCLUDE_SELF,
                        ..Default::default()
                    },
                ],
            }))
        }
        "~ remains tapped and that creature's power remains less than or equal to ~'s power" => {
            Some(cond_and(
                cond_marker(GM::RemainsTapped),
                cond_cmp(
                    val_obj_stat(anaphora("that creature"), StatKind::Power),
                    Cmp::Le,
                    val_obj_stat(SubjectRefAst::SelfObject, StatKind::Power),
                ),
            ))
        }
        "it targets you or a permanent you control" => {
            let mut your_perm = any_perm();
            your_perm.controller = Some(ControllerRelation::You);
            Some(cond_matches(
                anaphora("it"),
                ObjectFilterAst {
                    predicates: vec![FilterPredicateAst::TargetsSubject(Box::new(
                        SubjectRefAst::Compound(vec![
                            SubjectRefAst::Player(P::You),
                            grp(your_perm),
                        ]),
                    ))],
                    ..Default::default()
                },
            ))
        }
        "it has the least power or is tied for least power among creatures on the battlefield" => {
            let mut all_cr = obj_type(CardTypeMask::CREATURE);
            all_cr.zone = Some(ZoneFilter::Battlefield);
            Some(cond_cmp(
                val_obj_stat(anaphora("it"), StatKind::Power),
                Cmp::Le,
                ValueExprAst::ExtremeStatAmong {
                    stat: StatKind::Power,
                    greatest: false,
                    filter: Box::new(all_cr),
                    or_toughness: false,
                },
            ))
        }
        "it doesn't have a counter of that kind on it" => Some(cond_not(cond_counter_cmp(
            anaphora("it"),
            "same",
            Cmp::Ge,
            1,
        ))),
        _ => None,
    };
    if let Some(c) = direct_cond {
        return Ok(c);
    }

    // Compositional marker & comparison patterns (replacing hundreds of single-card arms).
    if let Some(who_raw) = strip_suffix_any(
        &low,
        &[" is the monarch", "'re the monarch", " are the monarch"],
    ) && let Some(p) = parse_player_ref(who_raw.trim())
    {
        return Ok(cond_marker(GameMarker::Monarch(p)));
    }
    if let Some(who_raw) = low.strip_suffix(" is poisoned")
        && let Some(p) = parse_player_ref(who_raw.trim())
    {
        return Ok(cond_marker(GameMarker::PlayerIsPoisoned { player: p }));
    }
    if let Some(who_raw) = low.strip_suffix(" has a rad counter")
        && let Some(p) = parse_player_ref(who_raw.trim())
    {
        return Ok(cond_marker(GameMarker::PlayerHasRadCounter { player: p }));
    }
    if let Some(who_raw) =
        strip_suffix_any(&low, &[" owns a card in exile", " own a card in exile"])
        && let Some(p) = parse_player_ref(who_raw.trim())
    {
        return Ok(cond_marker(GameMarker::OwnerHasCardInExile { player: p }));
    }
    if let Some(subj_raw) = strip_suffix_any(&low, &[" remains exiled", " remain exiled"]) {
        let (any_of, core) = match subj_raw.strip_prefix("any of ") {
            Some(c) => (true, c.trim()),
            None => (false, subj_raw.trim()),
        };
        if matches!(core, "it" | "they" | "that card" | "those cards") {
            return Ok(cond_marker(GameMarker::SubjectRemainsExiled {
                subject: box_anaphora(core),
                any_of,
            }));
        }
    }
    if let Some((subj_raw, is_combat)) = low
        .strip_suffix(" attacked this turn")
        .map(|s| (s, false))
        .or_else(|| low.strip_suffix(" attacked this combat").map(|s| (s, true)))
        && matches!(subj_raw.trim(), "~" | "it")
    {
        let subject = if subj_raw.trim() == "~" {
            self_obj()
        } else {
            box_anaphora("it")
        };
        return Ok(cond_marker(if is_combat {
            GameMarker::AttackedThisCombat { subject }
        } else {
            GameMarker::AttackedThisTurn { subject }
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" attacked or blocked this turn")
        && matches!(subj_raw.trim(), "it" | "that creature")
    {
        return Ok(cond_marker(GameMarker::AttackedOrBlockedThisTurn {
            subject: box_anaphora("it"),
        }));
    }
    if let Some((subj_raw, or_entered)) = low
        .strip_suffix(" didn't attack or come under your control this turn")
        .map(|s| (s, true))
        .or_else(|| {
            low.strip_suffix(" didn't attack this turn")
                .map(|s| (s, false))
        })
        && subj_raw.trim() == "~"
    {
        return Ok(cond_marker(GameMarker::DidNotAttackThisTurn {
            subject: self_obj(),
            or_entered_this_turn: or_entered,
        }));
    }
    if let Some(ord) = low
        .strip_prefix("this is the ")
        .and_then(|r| r.strip_suffix(" time this ability has resolved this turn"))
        .or_else(|| {
            low.strip_prefix("it's the ")
                .and_then(|r| r.strip_suffix(" time"))
        })
        && let Some(nth) = match ord.trim() {
            "first" => Some(1),
            "second" => Some(2),
            "third" => Some(3),
            _ => None,
        }
    {
        return Ok(cond_marker(GameMarker::AbilityResolvedNthTimeThisTurn(nth)));
    }
    if let Some(after) = low.strip_prefix("you've been dealt damage by ")
        && let Some((cnt_raw, tail)) = after.split_once(" or more ")
        && let Some(flt_raw) = tail.strip_suffix(" this turn")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
        && let Ok(min_sources) = u16::try_from(n)
    {
        return Ok(cond_marker(GameMarker::PlayerDealtDamageBy {
            player: PlayerRefAst::You,
            source_filter: Box::new(flt),
            min_sources,
            combat_only: false,
            this_game: false,
        }));
    }
    if let Some((who_raw, rest)) = low.split_once(" had ")
        && let Some((cnt_raw, _)) =
            rest.split_once(" or more cards put into their graveyard from anywhere this turn")
        && let (Some(player), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(min_count) = u16::try_from(n)
    {
        return Ok(cond_marker(
            GameMarker::PlayerHadCardsPutIntoGraveyardThisTurn { player, min_count },
        ));
    }
    if let Some(after) = low.strip_prefix("the top card of ")
        && let Some((zone_raw, flt_raw)) = after.split_once(" is ")
        && zone_raw.trim() == "target player's graveyard"
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(cond_marker(GameMarker::TopCardOfZoneMatches {
            zone: ZoneFilter::TargetPlayerGraveyard,
            filter: Box::new(flt),
        }));
    }
    if let Some((who_raw, rest)) = low.split_once(" discarded the card with the greatest ")
        && rest.ends_with(" among those cards or tied for greatest")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let stat = if rest.starts_with("power") {
            StatKind::Power
        } else if rest.starts_with("toughness") {
            StatKind::Toughness
        } else {
            StatKind::ManaValue
        };
        return Ok(cond_marker(GameMarker::PlayerActionExtremeStat {
            player,
            action: PriorActionRefKind::DiscardedThisWay,
            stat,
            greatest: true,
            allow_tie: true,
        }));
    }
    if let Some(after) = low.strip_prefix("you've cycled a card named ~ ")
        && let Some(cnt_raw) = after.strip_suffix(" or more times this game")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && let Ok(min_times) = u16::try_from(n)
    {
        return Ok(cond_marker(GameMarker::CycledCardNamedTimesThisGame {
            name: NameFilterAst::SelfName,
            min_times,
        }));
    }
    if let Some(cnt_raw) = low
        .strip_prefix("you reveal ")
        .and_then(|r| r.strip_suffix(" cards with different names"))
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && let Ok(min_count) = u16::try_from(n)
    {
        return Ok(cond_marker(GameMarker::RevealedDistinctNames { min_count }));
    }
    if let Some(cnt_raw) = low.strip_suffix(" or more {e} was paid this way")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && let Ok(min_e) = u16::try_from(n)
    {
        return Ok(cond_marker(GameMarker::EnergyPaidThisWayAtLeast(min_e)));
    }
    if let Some(cnt_raw) = low.strip_suffix(" or more cards were returned to your hand this way")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && let Ok(min_count) = u16::try_from(n)
    {
        return Ok(cond_marker(GameMarker::ZoneActionCountThisWay {
            action: PriorActionRefKind::ReturnedToHandThisWay,
            min_count,
        }));
    }
    if let Some(cnt_raw) = low
        .strip_prefix("there were ")
        .and_then(|r| r.strip_suffix(" or more card types among the sacrificed permanents"))
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && let Ok(min_count) = u16::try_from(n)
    {
        let mut sac_perm = any_perm();
        sac_perm
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::SacrificedThisWay,
            ));
        return Ok(cond_marker(GameMarker::DistinctCardTypesAmong {
            filter: Box::new(sac_perm),
            min_count,
        }));
    }
    if let Some((flt_raw, action)) = low
        .strip_suffix(" dies this way")
        .map(|f| (f, "dies"))
        .or_else(|| {
            low.strip_suffix(" is put into exile this way")
                .map(|f| (f, "exiled"))
        })
        .or_else(|| {
            low.strip_suffix(" was destroyed this way")
                .map(|f| (f, "destroyed"))
        })
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(cond_marker(GameMarker::ZoneActionHappenedThisWay {
            filter: Box::new(flt),
            action: action.into(),
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" is the chosen type")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(cond_matches(
            subj,
            ObjectFilterAst {
                flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                ..Default::default()
            },
        ));
    }
    if let Some((who_raw, rest)) = low.split_once(" has more than ")
        && let Some(cnt_raw) = rest.strip_suffix(" cards in hand")
        && let (Some(player), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(value) = u16::try_from(n)
    {
        return Ok(ConditionAst::HandCount {
            player,
            op: CompareOp::Gt,
            value,
        });
    }
    if let Some(cnt_raw) = low
        .strip_prefix("cards with ")
        .and_then(|r| r.strip_suffix(" or more different mana values are exiled with ~"))
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        let mut exiled_flt = any_card_flt();
        exiled_flt.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        return Ok(cond_cmp(
            ValueExprAst::DistinctCountAmong {
                category: "mana_values".into(),
                filter: Box::new(exiled_flt),
            },
            CompareOp::Ge,
            ValueExprAst::Const(n),
        ));
    }
    if let Some((p1_raw, p2_raw)) =
        split_once_any(&low, &[" has more life than ", " have more life than "])
        && let (Some(p1), Some(p2)) = (
            parse_player_ref(p1_raw.trim()),
            parse_player_ref(p2_raw.trim()),
        )
        && !matches!(p1, PlayerRefAst::AnyOpponent)
    {
        let rhs_p = if matches!(p2, PlayerRefAst::EachOpponent) {
            PlayerRefAst::AnyOpponent
        } else {
            p2
        };
        return Ok(cond_cmp(
            val_player_stat(p1, PlayerStatKind::LifeTotal),
            CompareOp::Gt,
            val_player_stat(rhs_p, PlayerStatKind::LifeTotal),
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" controlled by the chosen player")
        && let Some(core) = strip_suffix_any(&subj_raw, &["'s", " is"])
        && let Ok(subj) = parse_subject_or_target(core.trim(), vocab)
    {
        return Ok(cond_matches(
            subj,
            ObjectFilterAst {
                controller: Some(ControllerRelation::ChosenPlayer),
                ..Default::default()
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you control fewer ")
        && let Some(flt_raw) = after.strip_suffix(" than an opponent")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(cond_marker(GameMarker::ControlsFewerThanOpponent {
            filter: Box::new(flt),
        }));
    }
    if let Some(after) = low.strip_prefix("you control more ")
        && let Some((flt_raw, than_each_other)) = after
            .strip_suffix(" than each other player")
            .map(|f| (f, true))
            .or_else(|| {
                after
                    .strip_suffix(" than that spell's controller")
                    .map(|f| (f, true))
            })
            .or_else(|| {
                after
                    .strip_suffix(" than defending player")
                    .map(|f| (f, false))
            })
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(cond_marker(GameMarker::ControlsMoreThanOpponents {
            filter: Box::new(flt),
            than_each_other,
        }));
    }
    if let Some((who_raw, rest)) = low.split_once(" life total is ")
        && let Some((op, tail)) = rest
            .strip_prefix("less than or equal to half ")
            .map(|t| (CompareOp::Le, t))
            .or_else(|| {
                rest.strip_prefix("less than half ")
                    .map(|t| (CompareOp::Lt, t))
            })
        && tail.ends_with(" starting life total")
    {
        let player = parse_possessive_player(who_raw.trim());
        return Ok(cond_cmp(
            val_player_stat(player, PlayerStatKind::LifeTotal),
            op,
            val_div(ValueExprAst::StartingLifeTotal(player), 2, Rounding::Down),
        ));
    }
    const KNOWN_TURN_EVENT_CONDITIONS: &[&str] = &[
        "excess damage was dealt this way",
        "it's greater",
        "that spell is countered this way",
        "you have an enduring story",
        "that creature attacked this turn",
        "it's a spell with lesser mana value",
        "the amount of mana spent to cast that spell was less than its mana value",
        "a card is exiled with it",
        "a card is exiled with ~",
        "it was cast from your graveyard",
        "they were cast using web-slinging",
        "~ was cast using web-slinging",
        "you have a card in hand",
        "it was the second spell you cast this turn",
        "it was blocked this turn",
        "~ was dealt damage this turn",
        "it was dealt damage this turn",
        "it dealt combat damage to a player this turn",
        "an opponent lost life last turn",
        "they aren't attacking you",
        "that player is you",
        "it entered under your control",
        "that cost wasn't paid",
        "the copy is a permanent spell",
        "it's your first, second, or third turn of the game",
        "you've activated a loyalty ability this turn",
        "~ attacked or blocked this turn",
        "it was attacking or blocking alone",
        "an aura you controlled was attached to it",
        "you didn't play a land this turn",
        "you didn't lose life this turn",
        "you didn't play a card from exile this turn",
        "you put a counter on ~ this turn",
        "a counter was put on ~ this turn",
        "you put a counter on a creature this turn",
        "~ dealt damage to an opponent this turn",
        "~ dealt damage to it this turn",
        "no creatures died this turn",
        "you attacked with exactly one other creature this combat",
        "it's the second creature spell you cast this turn",
        "~ is in your graveyard or on the battlefield",
        "you're the defending player",
        "enchanted equipment is attached to a creature",
        "it didn't attack this turn",
        "an opponent searched their library this turn",
        "three or more creatures attacked this turn",
        "there are five colors among permanents you control",
        "it devoured a creature",
        "it hasn't dealt damage yet",
        "your team gained life this turn",
        "you've drawn more than one card this turn",
        "one or more cards were put into exile this turn",
        "no permanents left the battlefield this turn",
        "a creature died under an opponent's control this turn",
        "a creature card left your graveyard this turn",
        "its power was different from its base power",
        "that creature was dealt excess damage this turn",
        "you haven't been dealt combat damage since your last turn",
        "no other creatures are attacking that player",
        "you were the monarch as the turn began",
        "they attacked you and/or a planeswalker you control",
        "it has the same name as one of the cards exiled with ~",
        "~ is in your graveyard with a creature card directly above it",
        "another creature died this turn",
        "no cards are in that graveyard",
        "you have less life than an opponent",
        "its mana value is odd",
        "its mana value is even",
        "an opponent protects it",
        "~ is your ring-bearer",
        "enchanted creature is attacking alone",
        "you've visited an attraction this turn",
        "~ hasn't dealt damage yet",
        "you cast it from anywhere other than your hand",
        "that creature has greater power or toughness than ~",
        "that player controls more lands than each other player",
        "you removed two or more loyalty counters to activate it",
        "you don't draw a card this way",
        "that card has the chosen name",
        "that land was nonbasic",
        "exactly one creature is attacking",
        "a land you controlled was put into a graveyard from the battlefield this turn",
        "you or a player you're attacking has the initiative",
        "two or more of those creatures are attacking you and/or planeswalkers you control",
        "it enlisted a creature this combat",
        "that player had another land enter the battlefield under their control this turn",
        "excess damage was dealt to that creature this way",
        "excess damage was dealt to a creature this way",
        "excess damage was dealt to that permanent this way",
        "the creature the opponent controls is dealt excess damage this way",
        "~ attacked a battle this turn",
        "it attacked during its controller's last turn",
        "it attacked during your last turn",
        "~'s power is even",
        "~'s power is odd",
        "that artifact is put into a graveyard this way",
        "that creature is put into a graveyard this way",
        "~ is still on the battlefield",
        "they control two or more basic lands",
        "they control a commander",
        "an opponent has been dealt noncombat damage this turn",
        "you've scried or surveilled this turn",
        "~ is on the battlefield or in your graveyard",
        "an opponent was dealt combat damage by a legendary creature this turn",
        "it's a land or double-faced card",
        "three or more creature cards are above ~",
        "you've paid or lost four or more {e} this turn",
        "red sources you controlled dealt 4 or more noncombat damage this turn",
        "~ blocked or was blocked by a blue creature this turn",
        "you've rolled three or more dice this turn",
        "you've been attacked this step",
        "you're attacking two or more opponents",
        "at least one creature is blocking ~",
        "ten or more lands are on the battlefield",
        "there are twenty-five or more loyalty counters among jaces you control",
        "mana from a treasure was spent to cast it or activate it",
        "two or more equipment are attached to it",
        "an enchantment was put into your graveyard from the battlefield this turn",
        "~ regenerated this turn",
        "it shares a card type with the card exiled this way",
        "no player does",
        "the player doesn't",
        "you win",
        "you paid one or more {e} this way",
        "one or more +1/+1 counters are moved this way",
        "two cards that share a card type are discarded this way",
        "that creature was cast for its warp cost",
        "that card is returned to its owner's hand this way",
        "you gain control of a creature this way",
        "a creature spell you cast this turn was countered by a spell or ability an opponent controlled",
        "a player other than you lost life this turn",
        "an enchantment is on the battlefield",
        "excess damage was dealt to a permanent this way",
        "~ is enchanted, equipped, and has a counter on it",
        "this is the first or second time this ability has resolved this turn",
        "they control fewer lands than you",
        "the discovered card's mana value is less than 10",
        "that creature shares a color with the mana that land produced",
        "that spell targets a commander you control",
        "you control two or more nonland, nontoken permanents with the same name as one another",
        "your library has no cards in it",
        "you control more creatures than attacking player",
        "you control more lands than attacking player",
    ];
    if KNOWN_TURN_EVENT_CONDITIONS.contains(&low.as_str()) {
        return Ok(cond_action(PlayerRefAst::You, low));
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
        return Ok(cond_action(PlayerRefAst::You, low));
    }
    if let Some(flt_raw) = strip_suffix_any(&low, &[" is in your graveyard", " is in a graveyard"])
        && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
    {
        flt.zone = Some(if low.ends_with(" is in a graveyard") {
            ZoneFilter::AnyGraveyard
        } else {
            ZoneFilter::YourGraveyard
        });
        return Ok(cond_zone_count(flt, CompareOp::Ge, 1));
    }

    // Orthogonal negated subtype/status/mana-spent/keyword conditions:
    // "it's not a <flt>", "it's not <status>", "<mana> wasn't spent to cast it/~", "it didn't have <kw>"
    if let Some(flt_raw) = strip_prefix_any(&low, &["it's not ", "it isn't "]) {
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
            return Ok(ConditionAst::Not(Box::new(cond_status(
                anaphora("it"),
                flt_raw.to_string(),
            ))));
        }
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: box_anaphora("it"),
                filter: Box::new(flt),
            })));
        }
    }
    if let Some(spent) = strip_suffix_any(
        &low,
        &[" wasn't spent to cast it", " wasn't spent to cast ~"],
    ) {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
            spent.trim().to_string(),
        ))));
    }
    if let Some(kw) = low.strip_prefix("it didn't have ")
        && parse_keyword_list(kw, vocab).is_some()
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: box_anaphora("it"),
            keyword: kw.to_string(),
        })));
    }
    if let Some(after) = strip_prefix_any(&low, &["there's ", "there is "])
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(cond_zone_count(flt, CompareOp::Ge, 1));
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        let flt_clean = flt_raw.replace(" total in all graveyards", " in all graveyards");
        if let Ok(flt) = parse_object_filter(&flt_clean, vocab) {
            return Ok(cond_zone_count(
                flt,
                CompareOp::Ge,
                u16::try_from(n.max(0)).unwrap_or(1),
            ));
        }
    }
    if let Some((who_raw, flt_raw)) = split_once_any(&low, &[" controls no ", " control no "])
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(cond_controls(player, flt, CompareOp::Eq, 0));
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
        return Ok(cond_action(PlayerRefAst::You, low));
    }
    // "<N> or more creatures died [under your control] this turn" / "<N> or more cards have been exiled with ~" / "<N> or more <flt> are in your graveyard"
    if let Some((cnt_raw, tail)) = low.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        if tail == "creatures died this turn"
            || tail == "creatures died under your control this turn"
        {
            let creature_flt = obj_type(CardTypeMask::CREATURE);
            return Ok(cond_zone_ch(
                creature_flt,
                Some(ZoneFilter::Battlefield),
                Some(ZoneFilter::AnyGraveyard),
                if tail.contains("under your control") {
                    Some(PlayerRefAst::You)
                } else {
                    None
                },
                u16::try_from(n.max(0)).unwrap_or(1),
            ));
        }
        if tail == "cards have been exiled with ~" {
            let exiled_flt = ObjectFilterAst {
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            };
            return Ok(cond_zone_count(
                exiled_flt,
                CompareOp::Ge,
                u16::try_from(n.max(0)).unwrap_or(1),
            ));
        }
        if let Some(flt_raw) = tail.strip_suffix(" are in your graveyard")
            && let Ok(flt) = parse_object_filter(&format!("{flt_raw} in your graveyard"), vocab)
        {
            return Ok(cond_zone_count(
                flt,
                CompareOp::Ge,
                u16::try_from(n.max(0)).unwrap_or(1),
            ));
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
        return Ok(cond_zone_count(
            flt,
            CompareOp::Lt,
            u16::try_from(n.max(0)).unwrap_or(1),
        ));
    }
    // "you sacrificed <N> or more <flt> this turn" / "<player> was dealt <N> or more [combat] damage this turn" / "you've been dealt <N> or more damage this turn" / "it has dealt <N> or more damage to that player this turn"
    if (low.starts_with("you sacrificed ") && low.ends_with(" this turn"))
        || (low.contains(" was dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("you've been dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("it has dealt ") && low.ends_with(" damage to that player this turn"))
    {
        return Ok(cond_action(PlayerRefAst::You, low));
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
            subjects: box_anaphora("those creatures"),
            perspective: CombatSwapPerspective::BlockedAttackers,
        });
    }
    if low == "each of those creatures could block all creatures that the other is blocking" {
        return Ok(ConditionAst::CanLegallySwapBlockAssignments {
            subjects: box_anaphora("those creatures"),
            perspective: CombatSwapPerspective::BlockingCreatures,
        });
    }
    if low == "the chosen card is cast as a spell" {
        return Ok(ConditionAst::Marker(GameMarker::ChosenCardCastAsSpell));
    }

    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(cnt_raw) = after.strip_suffix(" or more cards in your library")
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(cond_cmp(
                val_player_stat(PlayerRefAst::You, PlayerStatKind::CardsInLibrary),
                CompareOp::Ge,
                ValueExprAst::Const(n),
            ));
        }
        if let Some(rest) = after.strip_prefix("exactly ")
            && let Some(cnt_raw) =
                strip_suffix_any(&rest, &[" cards in your hand", " cards in hand"])
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
            return Ok(cond_controls(player, flt, CompareOp::Ge, 1));
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
                cond_controls(PlayerRefAst::You, f, CompareOp::Ge, 1)
            };
            let mut acc = make_cond(first_name);
            for next_name in iter {
                acc = if is_disjunction {
                    cond_or(acc, make_cond(next_name))
                } else {
                    cond_and(acc, make_cond(next_name))
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
            return Ok(cond_controls(PlayerRefAst::You, flt, CompareOp::Ge, 1));
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
                    return Ok(cond_action(PlayerRefAst::You, low));
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
        return Ok(cond_action(PlayerRefAst::You, low));
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
                return Ok(ConditionAst::Not(Box::new(cond_action(
                    PlayerRefAst::You,
                    low,
                ))));
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
                return Ok(cond_action(PlayerRefAst::You, low));
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
                return Ok(cond_action(player, low));
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
        return Ok(cond_zone_ch(
            flt,
            None,
            Some(ZoneFilter::Battlefield),
            Some(PlayerRefAst::AnyOpponent),
            1,
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" left the battlefield under your control this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(cond_zone_ch(
            flt,
            Some(ZoneFilter::Battlefield),
            None,
            Some(PlayerRefAst::You),
            1,
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" left the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(cond_zone_ch(
            flt,
            Some(ZoneFilter::Battlefield),
            None,
            None,
            1,
        ));
    }
    if let Some(flt_raw) =
        low.strip_suffix(" was put into a graveyard from the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(cond_zone_ch(
            flt,
            Some(ZoneFilter::Battlefield),
            Some(ZoneFilter::AnyGraveyard),
            None,
            1,
        ));
    }
    if let Some(flt_raw) = low.strip_prefix("any of that damage was dealt by ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(cond_action(PlayerRefAst::You, low));
    }
    if low.starts_with("it was crewed by ") {
        return Ok(cond_action(PlayerRefAst::You, low));
    }

    // "<mana> was spent to cast it/~"
    if let Some(spent) = strip_suffix_any(&low, &[" was spent to cast it", " was spent to cast ~"])
    {
        return Ok(ConditionAst::ManaSpentToCast(spent.trim().to_string()));
    }

    // "it was kicked with its {M} kicker" (CR 702.33c multi-slot kicker)
    if let Some(slot_raw) =
        strip_prefix_any(&low, &["~ was kicked with its ", "it was kicked with its "])
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
        return Ok(cond_controls(PlayerRefAst::You, flt, CompareOp::Ge, 1));
    }
    if let Some(flt_raw) = low.strip_prefix("you revealed ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(cond_action(PlayerRefAst::You, low));
    }

    // "it targets <filter>" / "~ targets <filter>"
    if let Some(flt_raw) = strip_prefix_any(&low, &["it targets ", "~ targets "])
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }

    let parse_total_stat_cmp = |rhs: &str| -> Option<(CompareOp, i32)> {
        let trimmed = strip_suffix_any(&rhs, &[" this combat", " this turn"])
            .unwrap_or(rhs)
            .trim();
        if let Some(n_tok) = strip_suffix_any(&trimmed, &[" or greater", " or more"]) {
            return parse_number_word(n_tok).map(|n| (CompareOp::Ge, n));
        }
        if let Some(n_tok) = strip_suffix_any(&trimmed, &[" or less", " or fewer"]) {
            return parse_number_word(n_tok).map(|n| (CompareOp::Le, n));
        }
        if let Some(n_tok) = trimmed.strip_prefix("greater than ") {
            return parse_number_word(n_tok).map(|n| (CompareOp::Gt, n));
        }
        if let Some(n_tok) = strip_prefix_any(&trimmed, &["less than ", "fewer than "]) {
            return parse_number_word(n_tok).map(|n| (CompareOp::Lt, n));
        }
        let eq_tok = strip_prefix_any(&trimmed, &["equal to ", "exactly "]).unwrap_or(trimmed);
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
            return Ok(cond_action(player, act.to_string()));
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
        if let Some(rest) = strip_suffix_any(
            &rhs,
            &[
                " cards in hand",
                " cards in their hand",
                " cards in your hand",
            ],
        ) {
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
            return Ok(cond_cmp(
                val_player_stat(player, PlayerStatKind::LifeTotal),
                CompareOp::Ge,
                ValueExprAst::Add(Box::new(ValueExprAst::StartingLifeTotal(player)), vconst(n)),
            ));
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
        if let Some(before_cnt) = strip_suffix_any(&rhs, &[" counters", " counter"])
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
        return Ok(cond_cmp(
            val_player_stat(
                parse_possessive_player(who_raw),
                PlayerStatKind::CardsInLibrary,
            ),
            CompareOp::Le,
            ValueExprAst::Const(n),
        ));
    }

    if let Some(rest) = strip_prefix_any(&low, &["you've gained ", "you gained "])
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
    if let Some(rest) = strip_prefix_any(&low, &["an opponent has cast ", "an opponent cast "])
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::AnyOpponent,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = strip_prefix_any(&low, &["you've cast ", "you cast "])
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        });
    }

    if low.contains("four or more card types among cards in your graveyard") {
        return Ok(ConditionAst::Delirium);
    }
    if low.contains("three or more artifacts") && low.contains("you control") {
        return Ok(ConditionAst::Metalcraft);
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
    if let Some((who_raw, rest)) = split_once_any(
        &low,
        &[" has drawn ", " have drawn ", "'ve drawn ", " drew "],
    ) && let Some(n_tok) = rest.strip_suffix(" or more cards this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
        }));
    }

    // "<player> cast / has cast N or more spells this turn"
    if let Some((who_raw, rest)) =
        split_once_any(&low, &[" has cast ", " have cast ", "'ve cast ", " cast "])
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
    if let Some((who_raw, rest)) = split_once_any(
        &low,
        &["'ve sacrificed ", " has sacrificed ", " sacrificed "],
    ) && let Some(flt_raw) = rest.strip_suffix(" this turn")
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
        return Ok(ConditionAst::Not(Box::new(cond_has_kw(
            subj,
            kw_raw.trim().to_string(),
        ))));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(cond_has_kw(subj, kw_raw.trim().to_string()));
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
            anaphora("it"),
            ZoneFilter::YourGraveyard,
        ),
        (
            "it's in your graveyard",
            anaphora("it"),
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
            return Ok(cond_status(subj, status.to_string()));
        }
        if let Some(subj_raw) = low
            .strip_suffix(&format!(" isn't {status}"))
            .or_else(|| low.strip_suffix(&format!(" aren't {status}")))
            .or_else(|| low.strip_suffix(&format!(" is not {status}")))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ConditionAst::Not(Box::new(cond_status(
                subj,
                status.to_string(),
            ))));
        }
        if low == format!("it's {status}")
            || low == format!("he's {status}")
            || low == format!("she's {status}")
        {
            return Ok(cond_status(anaphora("it"), status.to_string()));
        }
        if low == format!("it isn't {status}") || low == format!("it's not {status}") {
            return Ok(ConditionAst::Not(Box::new(cond_status(
                anaphora("it"),
                status.to_string(),
            ))));
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
        && let Some(cnt_raw) = strip_suffix_any(&rest, &[" times this turn", " time this turn"])
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
    if let Some(subj_raw) = strip_suffix_any(
        &low,
        &[" entered this turn", " entered the battlefield this turn"],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(
            Box::new(subj),
        )));
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
            && let Some(n_tok) = strip_suffix_any(&rest, &[" cards in hand", " cards in your hand"])
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
                return Ok(cond_controls(player, flt, CompareOp::Eq, 1));
            }
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("no ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(cond_controls(player, flt, CompareOp::Eq, 0));
            }
            if let Some(after_al) = rest.strip_prefix("at least ")
                && let Some((cnt_tok, flt_raw)) = after_al.split_once(' ')
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                let c = cond_controls(
                    player,
                    flt,
                    CompareOp::Ge,
                    u16::try_from(n.max(0)).unwrap_or(1),
                );
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
                let c = cond_controls(
                    player,
                    flt,
                    CompareOp::Ge,
                    u16::try_from(n.max(0)).unwrap_or(1),
                );
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
                let c = cond_controls(
                    player,
                    flt,
                    CompareOp::Le,
                    u16::try_from(n.max(0)).unwrap_or(0),
                );
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
            if let Ok(flt) = parse_object_filter(rest, vocab) {
                let c = cond_controls(player, flt, CompareOp::Ge, 1);
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
        && let Some((kind_part, subj_raw)) =
            split_once_any(&after, &[" counters on ", " counter on "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(cond_counter_cmp(
            subj,
            if kind_part.trim().is_empty() {
                "any".to_string()
            } else {
                kind_part.trim().to_string()
            },
            CompareOp::Eq,
            0,
        ));
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, rest)) = after.split_once(" or more ")
        && let Some((kind_part, subj_raw)) =
            split_once_any(&rest, &[" counters on ", " counter on "])
        && let (Some(n), Ok(subj)) = (
            parse_number_word(cnt_tok),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(cond_counter_cmp(
            subj,
            kind_part.trim().to_string(),
            CompareOp::Ge,
            u16::try_from(n.max(0)).unwrap_or(1),
        ));
    }

    // "there are no <filter>" / "there are <filter>" / "there are N or more <filter>"
    if let Some(flt_raw) = low.strip_prefix("there are no ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(cond_zone_count(flt, CompareOp::Eq, 0));
    }
    if let Some(flt_raw) = low.strip_prefix("there are ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(cond_zone_count(flt, CompareOp::Ge, 1));
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
    if let Some((subj_raw, rest)) = split_once_any(&low, &[" isn't ", " wasn't "])
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::Not(Box::new(cond_matches(subj, flt))));
    }
    if let Some(rest) = low.strip_prefix("it's ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: box_anaphora("it"),
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
        return Ok(cond_matches(subj, flt));
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
                    subject: box_anaphora("it"),
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
            && let Some(before_cnt) = strip_suffix_any(&before_on, &[" counters", " counter"])
                .or_else(|| (before_on == "counters").then_some(""))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if let Some(no_kind) = before_cnt.strip_prefix("no ") {
                return Ok(cond_counter_cmp(
                    subj,
                    if no_kind.trim().is_empty() {
                        "any".to_string()
                    } else {
                        no_kind.trim().to_string()
                    },
                    CompareOp::Eq,
                    0,
                ));
            }
            if before_cnt == "no" {
                return Ok(cond_counter_cmp(subj, "any", CompareOp::Eq, 0));
            }
            if let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(cond_counter_cmp(
                    subj,
                    kind.trim().to_string(),
                    CompareOp::Ge,
                    u16::try_from(n.max(0)).unwrap_or(1),
                ));
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
                return Ok(cond_counter_cmp(
                    subj,
                    kclean.to_string(),
                    CompareOp::Lt,
                    u16::try_from(n.max(0)).unwrap_or(0),
                ));
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
                return Ok(cond_counter_cmp(
                    subj,
                    kclean.to_string(),
                    CompareOp::Gt,
                    u16::try_from(n.max(0)).unwrap_or(0),
                ));
            }
            let kind =
                strip_prefix_any(&before_cnt, &["one or more ", "a ", "an "]).unwrap_or(before_cnt);
            let kclean =
                if kind.trim().is_empty() || kind.trim() == "one or more" || kind.trim() == "a" {
                    "any"
                } else {
                    kind.trim()
                };
            return Ok(cond_counter_cmp(subj, kclean.to_string(), CompareOp::Ge, 1));
        }
    }

    // "<subject> doesn't have a <kind> counter on it"
    if let Some((subj_raw, rhs)) = low.split_once(" doesn't have ")
        && let Some(before_on) = rhs.strip_suffix(" on it")
        && let Some(before_cnt) = strip_suffix_any(&before_on, &[" counters", " counter"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = strip_prefix_any(&before_cnt, &["a ", "an "])
            .unwrap_or(before_cnt)
            .trim();
        return Ok(cond_counter_cmp(
            subj,
            if kind.is_empty() {
                "any".to_string()
            } else {
                kind.to_string()
            },
            CompareOp::Eq,
            0,
        ));
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
                    return Ok(cond_cmp(val_obj_stat(subj, stat), op, val));
                }
            }
        }
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
    if let Some(after) = strip_prefix_any(
        &low,
        &["it targets one or more ", "that spell targets one or more "],
    ) && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }
    if let Some(after) = strip_prefix_any(&low, &["~ is attached to ", "~ is enchanted by "])
        && !after.is_empty()
    {
        if let Ok(host) = parse_subject_or_target(after, vocab) {
            return Ok(ConditionAst::AttachedTo {
                subject: self_obj(),
                host: Box::new(host),
            });
        }
        return Ok(cond_status(
            SubjectRefAst::SelfObject,
            format!("attached:{after}"),
        ));
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        if let Some(flt_raw) = rest.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(cond_cmp(
                ValueExprAst::CardTypesAmong(Box::new(flt)),
                CompareOp::Ge,
                ValueExprAst::Const(n),
            ));
        }
        if let Some((_, flt_raw)) = rest.split_once(" counters among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(cond_zone_count(
                flt,
                CompareOp::Ge,
                u16::try_from(n.max(0)).unwrap_or(0),
            ));
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
    if let Some(cost_kind) = strip_prefix_any(&low, &["the ", "his ", "her ", "~'s "])
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
        return Ok(cond_zone_count(
            gflt,
            CompareOp::Ge,
            u16::try_from(n.max(0)).unwrap_or(0),
        ));
    }

    // Generic voting result condition: "<option> gets more votes [or the vote is tied]"
    if let Some(opt) = strip_suffix_any(
        &low,
        &[" gets more votes or the vote is tied", " gets more votes"],
    ) && !opt.is_empty()
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
    if let Some(after) = low.strip_prefix("you had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under your control this turn")
    {
        let clean = strip_prefix_any(&flt_raw, &["a ", "an ", "one or more "]).unwrap_or(flt_raw);
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
                strip_prefix_any(&flt_raw, &["a ", "an "]).unwrap_or(flt_raw),
            )
        };
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(cond_zone_ch(
                flt,
                None,
                Some(ZoneFilter::Battlefield),
                Some(PlayerRefAst::AnyOpponent),
                min_count,
            ));
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
        return Ok(cond_has_kw(subj, kw));
    }
    if let Some((subj_raw, flt_raw)) =
        split_once_any(&low, &[" was a ", " was an ", " were a ", " were an "])
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(cond_matches(subj, flt));
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
            return Ok(cond_cmp(
                lhs,
                CompareOp::Ge,
                ValueExprAst::Add(Box::new(base), vconst(amount)),
            ));
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
        } else if let Some(r) = strip_suffix_any(&rhs_raw, &[" or greater", " or more"]) {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = strip_suffix_any(&rhs_raw, &[" or less", " or fewer"]) {
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
                    Ok(val_obj_stat(owner_subj, *stat))
                } else {
                    Err("no_rhs".to_string())
                }
            });
            if let Ok(rhs) = rhs_res {
                return Ok(cond_cmp(lhs, op, rhs));
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
            return Ok(cond_action(PlayerRefAst::You, low));
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
                subject: box_anaphora("that creature"),
                filter: Box::new(ObjectFilterAst {
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }),
            });
        }
        "that player controls three or fewer lands" => {
            return Ok(cond_controls(
                PlayerRefAst::ThatPlayer,
                obj_type(CardTypeMask::LAND),
                CompareOp::Le,
                3,
            ));
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
                filter: Box::new(any_card_flt()),
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
                filter: Box::new(obj_type(CardTypeMask::CREATURE)),
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
    if let Some(after) = strip_prefix_any(&low, &["at least one ", "no "])
        && let Some((flt_raw, subj_raw)) =
            split_once_any(&after, &[" is blocking ", " are blocking "])
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
                let cond = cond_status(subj, st_name.to_string());
                return Ok(if neg {
                    ConditionAst::Not(Box::new(cond))
                } else {
                    cond
                });
            }
        }
    }
    if let Some((subj_raw, tgt_raw)) =
        split_once_any(&low, &[" is attached to ", "'s attached to "])
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

    // Typed morbid condition ("a non-Zombie creature died this turn", "a Goblin died this turn")
    if let Some(flt_raw) =
        strip_prefix_any(&low, &["a ", "an "]).and_then(|r| r.strip_suffix(" died this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        if flt_raw.trim() == "creature" {
            return Ok(ConditionAst::Morbid {
                your_control_only: false,
            });
        }
        return Ok(cond_zone_ch(
            flt,
            Some(ZoneFilter::Battlefield),
            Some(ZoneFilter::AnyGraveyard),
            None,
            1,
        ));
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
                return Ok(cond_and(lc, rc));
            }
            if l.starts_with("you ")
                && let (Ok(lc), Ok(rc)) = (
                    parse_condition(l, vocab),
                    parse_condition(&format!("you {r}"), vocab),
                )
            {
                return Ok(cond_and(lc, rc));
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
                return Ok(cond_or(lc, rc));
            }
            if l.starts_with("you ")
                && let (Ok(lc), Ok(rc)) = (
                    parse_condition(l, vocab),
                    parse_condition(&format!("you {r}"), vocab),
                )
            {
                return Ok(cond_or(lc, rc));
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
        return Ok(cond_controls(
            PlayerRefAst::EachPlayer,
            flt,
            CompareOp::Ge,
            1,
        ));
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
            rhs: vconst(n),
        });
    }
    if let Some(rest) = low.strip_prefix("your library has ")
        && let Some(n_str) = rest.strip_suffix(" or more cards in it")
        && let Some(n) = parse_number_word(n_str.trim())
    {
        return Ok(ConditionAst::ExprCompare {
            lhs: Box::new(val_player_stat(
                PlayerRefAst::You,
                PlayerStatKind::CardsInLibrary,
            )),
            op: CompareOp::Ge,
            rhs: vconst(n),
        });
    }
    if let Some(rest) = low.strip_prefix("any of ")
        && let Some((grp_raw, pred_raw)) = split_once_any(&rest, &[" have ", " has "])
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
        return Ok(cond_cmp(
            ValueExprAst::DomainCount,
            CompareOp::Ge,
            ValueExprAst::Const(5),
        ));
    }
    if let Some(after_ex) = low.strip_prefix("you control exactly ")
        && let Some((cnt_tok, flt_raw)) = after_ex.split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_tok.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Ok(cond_controls(
            PlayerRefAst::You,
            flt,
            CompareOp::Eq,
            u16::try_from(n.max(0)).unwrap_or(0),
        ));
    }
    if low == "you control eight or more artifacts with the same name as one another" {
        let flt = ObjectFilterAst {
            any_types: CardTypeMask::ARTIFACT,
            predicates: vec![FilterPredicateAst::StateQualification(
                "same_name_as_one_another".to_string(),
            )],
            ..Default::default()
        };
        return Ok(cond_controls(PlayerRefAst::You, flt, CompareOp::Ge, 8));
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
        return Ok(cond_zone_count(flt, CompareOp::Ge, 1));
    }
    if low == "the player is your opponent and has four or more cards in hand" {
        return Ok(cond_and(
            ConditionAst::EventOccurredThisTurn {
                event: "the player is your opponent".to_string(),
            },
            ConditionAst::HandCount {
                player: PlayerRefAst::ThatPlayer,
                op: CompareOp::Ge,
                value: 4,
            },
        ));
    }
    if let Some(after_roll) = low.strip_prefix("the roll was ")
        && let Some(num_raw) = after_roll.strip_suffix(" or higher")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Ok(cond_cmp(
            ValueExprAst::PriorResult,
            CompareOp::Ge,
            ValueExprAst::Const(n),
        ));
    }
    if low == "that player lost less than 8 life this turn" {
        return Ok(cond_cmp(
            ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::ThatPlayer,
                gained: false,
            },
            CompareOp::Lt,
            ValueExprAst::Const(8),
        ));
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
    // Compositional typed status, zone-change, dice, parity, and prior-action conditions.
    if let Some((subj_raw, rest)) = low
        .split_once(" dealt combat damage to ")
        .map(|(s, r)| (s, (false, true, r)))
        .or_else(|| {
            low.split_once(" didn't deal combat damage to ")
                .map(|(s, r)| (s, (true, true, r)))
        })
        .or_else(|| {
            low.split_once(" dealt damage to ")
                .map(|(s, r)| (s, (false, false, r)))
        })
        && let (neg, combat_only, tgt_tail) = rest
        && let Some(flt_raw) = tgt_tail.strip_suffix(" this turn")
        && let (Ok(source), Ok(flt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        let c = ConditionAst::DamageDealtThisTurn {
            source: Box::new(source),
            target_filter: Some(Box::new(flt)),
            combat_only,
            min_damage: Some(1),
        };
        return Some(if neg { cond_not(c) } else { c });
    }
    if let Some((subj_raw, rest)) = low.split_once(" has dealt ")
        && let Some(cnt_raw) = rest.strip_suffix(" or more damage this turn")
        && let (Ok(source), Some(n)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(min_d) = u16::try_from(n)
    {
        return Some(ConditionAst::DamageDealtThisTurn {
            source: Box::new(source),
            target_filter: None,
            combat_only: false,
            min_damage: Some(min_d),
        });
    }
    if let Some(subj_raw) = low.strip_suffix(" wasn't dealt damage this turn")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(cond_not(cond_matches(
            subj,
            ObjectFilterAst {
                flags: ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                ..Default::default()
            },
        )));
    }
    if let Some((subj_raw, _)) = split_once_any(&low, &[" was all colors", " is all colors"])
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(ConditionAst::AllColors(Box::new(subj)));
    }
    if let Some((subj_raw, rest)) = low.split_once(" shares a ")
        && let Some((prop_raw, other_raw)) = rest.split_once(" with ")
        && let (Ok(subj), Ok(other)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(other_raw.trim(), vocab),
        )
    {
        return Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(subj),
            other: Box::new(other),
            property: prop_raw.trim().replace(' ', "_"),
        });
    }
    if let Some(after) = low.strip_prefix("you have ")
        && let Some(mid) = strip_suffix_any(&after, &[" in hand", " in your hand"])
    {
        let parsed = if let Some(flt_raw) = mid.strip_prefix("no ") {
            parse_object_filter(flt_raw.trim(), vocab)
                .ok()
                .map(|f| (f, CompareOp::Eq, 0u16))
        } else if let Some((cnt_raw, flt_raw)) = mid.split_once(" or more ")
            && let (Some(n), Ok(f)) = (
                parse_number_word(cnt_raw.trim()),
                parse_object_filter(flt_raw.trim(), vocab),
            )
            && let Ok(v) = u16::try_from(n)
        {
            Some((f, CompareOp::Ge, v))
        } else {
            None
        };
        if let Some((mut flt, op, value)) = parsed {
            flt.zone = Some(ZoneFilter::YourHand);
            return Some(cond_zone_count(flt, op, value));
        }
    }
    if let Some(subj_raw) = low.strip_suffix("'s power is negative")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(cond_cmp(
            val_obj_stat(subj, StatKind::Power),
            CompareOp::Lt,
            ValueExprAst::Const(0),
        ));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" doesn't have ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && parse_keyword_list(kw_raw.trim(), vocab).is_some()
    {
        return Some(cond_not(cond_has_kw(subj, kw_raw.trim())));
    }
    if let Some((subj_raw, flt_raw)) = low
        .split_once(" isn't ")
        .map(|(s, f)| (s, (true, f)))
        .or_else(|| low.split_once(" was ").map(|(s, f)| (s, (false, f))))
        .or_else(|| low.split_once(" enters as ").map(|(s, f)| (s, (false, f))))
        && let (neg, f_str) = flt_raw
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_object_filter(f_str.trim(), vocab),
        )
    {
        let c = ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        };
        return Some(if neg { cond_not(c) } else { c });
    }
    if let Some(cnt_raw) = low
        .strip_prefix("you rolled ")
        .and_then(|r| r.strip_suffix(" or higher"))
        && let Some(value) = parse_number_word(cnt_raw.trim())
    {
        return Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
            any_of_group: false,
            exact_times: None,
        });
    }
    if let Some(cnt_raw) = low
        .strip_prefix("any of those results was ")
        .and_then(|r| r.strip_suffix(" or higher"))
        && let Some(value) = parse_number_word(cnt_raw.trim())
    {
        return Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
            any_of_group: true,
            exact_times: None,
        });
    }

    match low {
        "three or more cards were put into your graveyard from anywhere other than the battlefield this turn" => {
            Some(cond_zone_ch(
                any_card_flt(),
                None,
                Some(ZoneFilter::YourGraveyard),
                Some(PlayerRefAst::You),
                3,
            ))
        }
        "three or more creature cards were put into graveyards from anywhere this turn" => {
            Some(cond_zone_ch(
                card_flt(CardTypeMask::CREATURE),
                None,
                Some(ZoneFilter::AnyGraveyard),
                None,
                3,
            ))
        }
        "a permanent was put into your hand from the battlefield this turn" => Some(cond_zone_ch(
            any_perm(),
            Some(ZoneFilter::Battlefield),
            Some(ZoneFilter::YourHand),
            Some(PlayerRefAst::You),
            1,
        )),
        "that creature entered from a graveyard or you cast it from a graveyard" => Some(cond_or(
            cond_zone_ch(
                obj_type(CardTypeMask::CREATURE),
                Some(ZoneFilter::AnyGraveyard),
                Some(ZoneFilter::Battlefield),
                None,
                1,
            ),
            ConditionAst::WasCastFromZone {
                subject: box_anaphora("that creature"),
                zone: ZoneFilter::AnyGraveyard,
            },
        )),
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
        "there are exactly two permanents named ~ on the battlefield" => {
            Some(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SelfName,
                filter: Box::new(any_perm()),
                zone: ZoneFilter::Battlefield,
                op: CompareOp::Eq,
                count: 2,
            })
        }
        "no permanents named ~ are on the battlefield" => Some(ConditionAst::CardWithNameInZone {
            name_filter: NameFilterAst::SelfName,
            filter: Box::new(any_perm()),
            zone: ZoneFilter::Battlefield,
            op: CompareOp::Eq,
            count: 0,
        }),
        "an instant card and a sorcery card are in your graveyard" => {
            let mut inst = card_flt(CardTypeMask::INSTANT);
            inst.zone = Some(ZoneFilter::YourGraveyard);
            let mut sorc = card_flt(CardTypeMask::SORCERY);
            sorc.zone = Some(ZoneFilter::YourGraveyard);
            Some(cond_and(
                cond_zone_count(inst, CompareOp::Ge, 1),
                cond_zone_count(sorc, CompareOp::Ge, 1),
            ))
        }
        "you own a card in exile that has an adventure" => Some(cond_zone_count(
            ObjectFilterAst {
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                owner: Some(ControllerRelation::You),
                predicates: vec![FilterPredicateAst::StateQualification(
                    "has_adventure".into(),
                )],
                ..Default::default()
            },
            CompareOp::Ge,
            1,
        )),
        "a card exiled with ~ is a creature card" => Some(cond_zone_count(
            ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            },
            CompareOp::Ge,
            1,
        )),
        "that creature remains tapped" => Some(ConditionAst::MatchesFilter {
            subject: box_anaphora("that creature"),
            filter: Box::new(ObjectFilterAst {
                tapped: Some(true),
                ..Default::default()
            }),
        }),
        "that attraction is on the battlefield" => Some(ConditionAst::SubjectInZone {
            subject: box_anaphora("that attraction"),
            zone: ZoneFilter::Battlefield,
        }),
        "that card remains on top of your library" => Some(ConditionAst::SubjectInZone {
            subject: box_anaphora("that card"),
            zone: ZoneFilter::YourLibrary,
        }),
        "that card remains exiled" => Some(cond_marker(GameMarker::SubjectRemainsExiled {
            subject: box_anaphora("that card"),
            any_of: false,
        })),
        "~ is in your opening hand" => Some(ConditionAst::SubjectInZone {
            subject: self_obj(),
            zone: ZoneFilter::YourHand,
        }),
        "~ is in your opening hand and you're not the starting player" => Some(cond_and(
            ConditionAst::SubjectInZone {
                subject: self_obj(),
                zone: ZoneFilter::YourHand,
            },
            cond_not(ConditionAst::IsStartingPlayer(PlayerRefAst::You)),
        )),
        "you're not the starting player" => {
            Some(cond_not(ConditionAst::IsStartingPlayer(PlayerRefAst::You)))
        }
        "~ is in a graveyard" => Some(ConditionAst::SubjectInZone {
            subject: self_obj(),
            zone: ZoneFilter::AnyGraveyard,
        }),
        "you haven't cast a spell this turn" => Some(cond_not(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(any_spell_flt()),
        })),
        "two or more creatures are blocking it" => Some(ConditionAst::BlockedByFilter {
            subject: box_anaphora("it"),
            blocker_filter: Box::new(obj_type(CardTypeMask::CREATURE)),
            min_count: 2,
        }),
        "they all share a color" => Some(ConditionAst::SharesPropertyWith {
            subject: box_anaphora("they"),
            other: box_anaphora("they"),
            property: "color".into(),
        }),
        "it's suspected" => Some(ConditionAst::HasStatus {
            subject: box_anaphora("it"),
            status: "suspected".into(),
        }),
        "any of them are suspected" => Some(ConditionAst::AnyOfGroupMatches {
            group: box_anaphora("them"),
            predicate: "suspected".into(),
        }),
        "you don't control it" => Some(cond_not(ConditionAst::SubjectControlledBy {
            subject: box_anaphora("it"),
            player: PlayerRefAst::You,
        })),
        "an opponent controls that creature" => Some(ConditionAst::SubjectControlledBy {
            subject: box_anaphora("that creature"),
            player: PlayerRefAst::AnyOpponent,
        }),
        "it came under your control this turn" => {
            Some(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(any_perm()),
                min_count: 1,
            })
        }
        "they lose the flip" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::ThatPlayer,
            outcome: "lose".into(),
        }),
        "both coins come up tails" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::You,
            outcome: "both_tails".into(),
        }),
        "both coins come up heads" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::You,
            outcome: "both_heads".into(),
        }),
        "you rolled 6 exactly seven times" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: 6,
            any_of_group: false,
            exact_times: Some(7),
        }),
        "the number is even" => Some(ConditionAst::NumberParity { even: true }),
        "the number is odd" => Some(ConditionAst::NumberParity { even: false }),
        "it isn't your main phase" => Some(cond_not(ConditionAst::IsPhaseOrStep {
            player: Some(PlayerRefAst::You),
            phase: "main_phase".into(),
            first_only: false,
        })),
        "it's the first end step of the turn" => Some(ConditionAst::IsPhaseOrStep {
            player: None,
            phase: "end_step".into(),
            first_only: true,
        }),
        "this is the fourth time this ability has resolved this turn" => {
            Some(ConditionAst::ResolutionCountThisTurn { count: 4 })
        }
        "~ is unblocked" => Some(ConditionAst::MatchesFilter {
            subject: self_obj(),
            filter: Box::new(ObjectFilterAst {
                combat: Some(CombatState::Unblocked),
                ..Default::default()
            }),
        }),
        "that creature has haste and ~ doesn't" => Some(cond_and(
            ConditionAst::HasKeyword {
                subject: box_anaphora("that creature"),
                keyword: "haste".into(),
            },
            cond_not(ConditionAst::HasKeyword {
                subject: self_obj(),
                keyword: "haste".into(),
            }),
        )),
        "they search their library this way" => Some(ConditionAst::SearchedLibraryThisWay),
        "one or more of the chosen permanents are still on the battlefield" => Some(cond_prior(
            "chosen_permanents_on_battlefield",
            Some(any_perm()),
            1,
        )),
        "one or more creature cards were put into that graveyard this way" => Some(cond_prior(
            "put_into_graveyard_this_way",
            Some(card_flt(CardTypeMask::CREATURE)),
            1,
        )),
        "two or more cards are exiled this way" => {
            Some(cond_prior("exiled_this_way", Some(any_card_flt()), 2))
        }
        "at least one creature card was exiled this way" => Some(cond_prior(
            "exiled_this_way",
            Some(card_flt(CardTypeMask::CREATURE)),
            1,
        )),
        "two cards that share a color were milled this way" => Some(cond_prior(
            "milled_sharing_color_this_way",
            Some(any_card_flt()),
            2,
        )),
        "you gain control of a creature this way" => Some(cond_prior(
            "gain_control_this_way",
            Some(obj_type(CardTypeMask::CREATURE)),
            1,
        )),
        "you lost life this way" => Some(cond_prior("lost_life_this_way", None, 1)),
        "you don't cast it this way" => Some(cond_not(cond_prior("cast_this_way", None, 1))),
        "you cast a spell this way" => Some(cond_prior("cast_this_way", Some(any_spell_flt()), 1)),
        "you don't put it into your hand" => {
            Some(cond_not(cond_prior("put_into_hand_this_way", None, 1)))
        }
        "they didn't put the card on top of their library" => Some(cond_not(cond_prior(
            "put_on_top_of_library_this_way",
            None,
            1,
        ))),
        "you draw one or more cards this way" => Some(cond_prior("draw_this_way", None, 1)),
        "that player discards an artifact card this way" => Some(cond_prior(
            "discard_this_way",
            Some(card_flt(CardTypeMask::ARTIFACT)),
            1,
        )),
        "that spell is countered this way" => {
            Some(cond_prior("countered_this_way", Some(any_spell_flt()), 1))
        }
        "those permanents are exchanged this way" => {
            Some(cond_prior("exchanged_this_way", Some(any_perm()), 1))
        }
        "that permanent is destroyed this way" => {
            Some(cond_prior("destroyed_this_way", Some(any_perm()), 1))
        }
        _ => None,
    }
}
