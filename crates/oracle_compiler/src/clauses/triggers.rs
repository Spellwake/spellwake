//! Trigger header (`TriggerSpecAst`) and trigger event (`TriggerEventAst`) grammar parser.

use crate::ast::{
    CardTypeMask, ControllerRelation, ObjectFilterAst, PlayerRefAst, SubjectRefAst,
    TriggerEventAst, TriggerSpecAst, TurnStepAst,
};
use crate::clauses::costs::peel_leading_count;
use crate::clauses::statics::player_ref_to_controller_relation;
use crate::combinators::{
    any_card_flt, any_perm, cond_action, cond_counter_cmp, grp, obj_type, self_obj, split_once_any,
    strip_prefix_any, strip_suffix_any,
};
use crate::filter::{
    parse_condition, parse_object_filter, parse_player_ref, parse_subject_or_target,
};
use crate::preprocess::is_physical_tabletop_clause;
use crate::vocab::{Vocabulary, parse_number_word};

/// Infer the triggering player actor from a trigger clause so opponent or any-player triggers
/// are never silently misattributed to `PlayerRefAst::You`.
fn infer_trigger_actor(blow: &str) -> PlayerRefAst {
    if blow.starts_with("an opponent ")
        || blow.starts_with("another player ")
        || blow.starts_with("one or more opponents ")
        || blow.starts_with("one or more of your opponents ")
        || blow.starts_with("one or more creatures an opponent ")
        || blow.starts_with("one or more creatures your opponents ")
        || blow.starts_with("a spell or ability an opponent ")
        || blow.starts_with("a creature entering under an opponent's ")
        || blow.starts_with("one or more lands enter under an opponent's ")
    {
        PlayerRefAst::AnyOpponent
    } else if blow.starts_with("enchanted player ") || blow.starts_with("enchanted opponent ") {
        PlayerRefAst::EnchantedPlayer
    } else if blow.starts_with("a player ")
        || blow.starts_with("one or more players ")
        || blow.starts_with("each player ")
    {
        PlayerRefAst::AnyPlayer
    } else {
        PlayerRefAst::You
    }
}

/// Parse a triggered-ability header (including compound events, Planechase, Archenemy,
/// Tabletop Honor-System claims, and intervening-`if`).
pub fn parse_trigger_header(header: &str, vocab: &Vocabulary) -> Result<TriggerSpecAst, String> {
    let raw = header.trim().trim_end_matches(',');
    let low = raw.to_ascii_lowercase();

    let (core_part, intervening_if) = if let Some(idx) = low.find(", if ") {
        let cpart = &raw[..idx];
        let cond_raw = &raw[idx + 5..];
        let cond = parse_condition(cond_raw, vocab).map_err(|e| format!("trigger_if:{e}"))?;
        (cpart.trim(), Some(cond))
    } else if let Some(idx) = low.find(" while ")
        && let Ok(cond) = parse_condition(&raw[idx + 7..], vocab)
    {
        (raw[..idx].trim(), Some(cond))
    } else {
        (raw, None)
    };

    // Compound trigger header: "<ev1> and/or whenever/when <ev2>" / "<ev1> and at the beginning of <step>"
    let clow = core_part.to_ascii_lowercase();
    if let Some((left, right)) = split_once_any(
        &clow,
        &[
            ", and whenever ",
            " and whenever ",
            ", and when ",
            " and when ",
            ", or whenever ",
            " or whenever ",
            ", or when ",
        ],
    ) {
        let mut evs = parse_trigger_events(left, vocab)?;
        let right_trig = format!("whenever {right}");
        evs.extend(parse_trigger_events(&right_trig, vocab)?);
        return Ok(TriggerSpecAst {
            events: evs,
            intervening_if,
        });
    }
    if let Some((left, right)) = clow.split_once(" and at the beginning of ") {
        let mut evs = parse_trigger_events(left, vocab)?;
        let right_step = format!("at the beginning of {right}");
        evs.extend(parse_trigger_events(&right_step, vocab)?);
        return Ok(TriggerSpecAst {
            events: evs,
            intervening_if,
        });
    }

    let events = parse_trigger_events(core_part, vocab).or_else(|orig_err| {
        // Compound "When/Whenever <ev1>, <ev2>, or <ev3>" and "<ev1> or <ev2>" trigger splits
        if let Some((pref, rest)) = clow
            .strip_prefix("whenever ")
            .map(|r| ("whenever ", r))
            .or_else(|| clow.strip_prefix("when ").map(|r| ("when ", r)))
        {
            if rest.contains(", or ") {
                let comma_parts: Vec<&str> = rest.split(", ").collect();
                if comma_parts.len() >= 2
                    && comma_parts.len() <= 4
                    && comma_parts
                        .last()
                        .is_some_and(|p| p.trim().starts_with("or "))
                {
                    let mut combined = Vec::new();
                    let mut all_ok = true;
                    for (i, part) in comma_parts.iter().enumerate() {
                        let p_clean = if i > 0 {
                            part.trim().strip_prefix("or ").unwrap_or(part.trim())
                        } else {
                            part.trim()
                        };
                        match parse_trigger_events(&format!("{pref}{p_clean}"), vocab) {
                            Ok(evs) => combined.extend(evs),
                            Err(_) => {
                                all_ok = false;
                                break;
                            }
                        }
                    }
                    if all_ok && combined.len() >= 2 {
                        return Ok(combined);
                    }
                }
            }
            for (idx, _) in rest.match_indices(" or ") {
                let left_ev = format!("{pref}{}", rest[..idx].trim());
                let right_ev = format!("{pref}{}", rest[idx + 4..].trim());
                if let (Ok(mut l), Ok(r)) = (
                    parse_trigger_events(&left_ev, vocab),
                    parse_trigger_events(&right_ev, vocab),
                ) {
                    l.extend(r);
                    return Ok(l);
                }
            }
        }
        Err(orig_err)
    })?;
    Ok(TriggerSpecAst {
        events,
        intervening_if,
    })
}

pub(crate) fn parse_trigger_events(
    core: &str,
    vocab: &Vocabulary,
) -> Result<Vec<TriggerEventAst>, String> {
    let c = core.trim().trim_end_matches(',');
    let low = c.to_ascii_lowercase();

    // Planechase & Archenemy native triggers
    if low == "whenever chaos ensues" || low == "when chaos ensues" {
        return Ok(vec![TriggerEventAst::ChaosEnsues]);
    }
    if low == "at end of combat" || low == "at end of combat on your turn" {
        return Ok(vec![trig_step(
            TurnStepAst::EndOfCombat,
            if low.contains("your turn") {
                "your turn".to_string()
            } else {
                "any".to_string()
            },
        )]);
    }
    if low == "when you planeswalk to ~"
        || low == "when you planeswalk here"
        || low == "whenever you planeswalk to ~"
        || low == "whenever you planeswalk here"
    {
        return Ok(vec![TriggerEventAst::PlaneswalkToSelf]);
    }
    if low == "when you planeswalk away from ~"
        || low == "when you planeswalk away from here"
        || low == "whenever you planeswalk away from ~"
        || low == "whenever you planeswalk away from here"
    {
        return Ok(vec![TriggerEventAst::PlaneswalkAwayFromSelf]);
    }
    if low == "when you encounter ~" {
        return Ok(vec![TriggerEventAst::EncounterPhenomenon]);
    }
    if low == "when you set ~ in motion" || low == "when you set this scheme in motion" {
        return Ok(vec![TriggerEventAst::SchemeSetInMotion]);
    }
    // CR 603.8 Counter threshold state trigger ("When there are 1,000 or more time counters on ~", The Millennium Calendar)
    if let Some(after) = low.strip_prefix("when there are ")
        && let Some((cnt_part, rest)) = after.split_once(" or more ")
        && let Some((kind, subj_raw)) = split_once_any(&rest, &[" counters on ", " counter on "])
        && let (Some(n), Ok(subj)) = (
            parse_number_word(cnt_part.trim()),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(
            cond_counter_cmp(
                subj,
                kind.trim().to_string(),
                crate::ast::CompareOp::Ge,
                u16::try_from(n.max(0)).unwrap_or(u16::MAX),
            ),
        ))]);
    }
    // Mana-production triggers (Caged Sun, Gauntlet of Power) & Scry-bottom trigger (The Temporal Anchor)
    if low == "whenever a land's ability causes you to add one or more mana of the chosen color" {
        return Ok(vec![TriggerEventAst::LandTappedForManaColor {
            filter: Box::new(obj_type(CardTypeMask::LAND)),
            mana_color_filter: "chosen_color".to_string(),
            by_you: true,
        }]);
    }
    if low == "whenever a basic land is tapped for mana of the chosen color" {
        return Ok(vec![TriggerEventAst::LandTappedForManaColor {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::LAND,
                required_supertypes: crate::ast::SupertypeMask::BASIC,
                ..Default::default()
            }),
            mana_color_filter: "chosen_color".to_string(),
            by_you: false,
        }]);
    }
    if low
        == "whenever you choose to put one or more cards on the bottom of your library while scrying"
    {
        return Ok(vec![trig_kw(PlayerRefAst::You, "scry_bottom")]);
    }
    if low == "whenever you put one or more counters on a permanent or player" {
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(SubjectRefAst::AnyTarget),
            counter_kind: "any".to_string(),
            added: true,
            by_you: true,
        }]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("whenever ")
        .and_then(|r| r.strip_suffix(" dies or is put into the command zone"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![trig_zone(
            Some("battlefield"),
            Some("graveyard_or_command"),
            subj,
        )]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("whenever ")
        .and_then(|r| r.strip_suffix(" becomes blocked by two or more creatures"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesBlockedByCount {
            subject: Box::new(subj),
            min_blockers: 2,
        }]);
    }
    if let Some(after_cause) = low
        .strip_prefix("whenever a spell or ability an opponent controls causes you to ")
        .or_else(|| low.strip_prefix("when a spell or ability an opponent controls causes you to "))
        && (after_cause.starts_with("discard ") || after_cause.starts_with("sacrifice "))
    {
        return Ok(vec![trig_kw(
            PlayerRefAst::You,
            format!("opponent_caused:{after_cause}"),
        )]);
    }
    if let Some((a_part, b_part)) = low.split_once(" or when ")
        && let (Ok(mut ev1), Ok(mut ev2)) = (
            parse_trigger_events(a_part.trim(), vocab),
            parse_trigger_events(&format!("when {}", b_part.trim()), vocab),
        )
    {
        ev1.append(&mut ev2);
        return Ok(ev1);
    }
    // General "<when/whenever> <player> attack[s] <subj> or a planeswalker they control" (Tenuous Truce)
    if let Some(after_w) = strip_prefix_any(&low, &["when ", "whenever "])
        && let Some((p_raw, tgt_raw)) = split_once_any(&after_w, &[" attack ", " attacks "])
        && let Some(actor) = parse_player_ref(p_raw.trim())
        && tgt_raw.contains(" or a planeswalker ")
    {
        return Ok(vec![trig_attacks_ext(
            None,
            Some(actor),
            Some(tgt_raw.to_string()),
            true,
        )]);
    }
    // General CR 603.8 state triggers (Call to Arms, City in a Bottle)
    if low
        == "when the chosen color isn't the most common color among nontoken permanents the chosen player controls or is tied for most common"
        || (low.starts_with("whenever one or more ") && low.ends_with(" are on the battlefield"))
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond_action(
            PlayerRefAst::You,
            low,
        )))]);
    }
    if matches!(
        low.as_str(),
        "whenever you win a clash"
            | "whenever ~'s cumulative upkeep is paid"
            | "whenever ~ saddles a mount or crews a vehicle during your main phase"
            | "whenever an opponent who controls an artifact named ~ draws a card during their turn"
            | "whenever an opponent who controls an artifact named ~ gains life during their turn"
    ) {
        let actor = if low.starts_with("whenever an opponent ") {
            PlayerRefAst::AnyOpponent
        } else {
            PlayerRefAst::You
        };
        return Ok(vec![trig_kw(actor, low)]);
    }
    if low == "whenever a player casts their second spell during their turn" {
        return Ok(vec![trig_cast_ext(
            PlayerRefAst::AnyPlayer,
            None,
            false,
            None,
            Some(2),
            false,
        )]);
    }
    if low == "whenever two or more creatures you control attack a player" {
        return Ok(vec![trig_attacks_ext(
            None,
            Some(PlayerRefAst::You),
            Some("two_or_more".to_string()),
            true,
        )]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("whenever ")
        .and_then(|r| r.strip_suffix(" enters transformed"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![trig_zone(None, Some("battlefield_transformed"), subj)]);
    }
    if let Some(flt_raw) = low.strip_prefix("whenever you play ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![trig_cast(PlayerRefAst::You, Some(flt))]);
    }

    // StepBegan: "at the beginning of ..." / "at the end of ..."
    if low.starts_with("at the beginning of ") || low.starts_with("at the end of ") {
        let is_end = low.starts_with("at the end of ");
        let mut rest = if is_end {
            &low["at the end of ".len()..]
        } else {
            &low["at the beginning of ".len()..]
        };

        if let Some(owner) = rest.strip_prefix("the upkeep of ") {
            return Ok(vec![trig_step(TurnStepAst::Upkeep, owner.to_string())]);
        }
        if let Some(owner) = rest.strip_prefix("the end step of ") {
            return Ok(vec![trig_step(TurnStepAst::EndStep, owner.to_string())]);
        }

        // Longest possessive match FIRST
        let whose = if let Some(r) = rest.strip_prefix("each of enchanted player's ") {
            rest = r;
            "enchanted_player"
        } else if let Some(r) = rest.strip_prefix("each of that player's ") {
            rest = r;
            "that_player"
        } else if let Some(r) = rest.strip_prefix("enchanted opponent's ") {
            rest = r;
            "enchanted_opponent"
        } else if let Some(r) = rest.strip_prefix("the monarch's ") {
            rest = r;
            "monarch"
        } else if let Some(r) = rest.strip_prefix("each other player's ") {
            rest = r;
            "each_other_player"
        } else if let Some(r) = rest.strip_prefix("each player's ") {
            rest = r;
            "each_player"
        } else if let Some(r) = rest.strip_prefix("each opponent's ") {
            rest = r;
            "each_opponent"
        } else if let Some(r) = rest.strip_prefix("an opponent's ") {
            rest = r;
            "opponent"
        } else if let Some(r) = rest.strip_prefix("that player's ") {
            rest = r;
            "that_player"
        } else if let Some(r) = rest.strip_prefix("enchanted player's ") {
            rest = r;
            "enchanted_player"
        } else if let Some(r) = rest.strip_prefix("the chosen player's ") {
            rest = r;
            "chosen_player"
        } else if let Some(r) = rest.strip_prefix("your next ") {
            rest = r;
            "your_next"
        } else if let Some(r) = rest.strip_prefix("that turn's ") {
            rest = r;
            "that_turn"
        } else if let Some(r) = rest.strip_prefix("the first ") {
            rest = r;
            "first"
        } else if let Some(r) = rest.strip_prefix("each of your ") {
            rest = r;
            "you"
        } else if let Some(r) = rest.strip_prefix("your ") {
            rest = r;
            "you"
        } else if let Some(r) = rest.strip_prefix("the next ") {
            rest = r;
            "next"
        } else if let Some(r) = rest.strip_prefix("each ") {
            rest = r;
            "each_player"
        } else if let Some(r) = rest.strip_prefix("every ") {
            rest = r;
            "each_player"
        } else if let Some(r) = rest.strip_prefix("the ") {
            rest = r;
            "any"
        } else if rest.starts_with("combat on your turn") {
            return Ok(vec![trig_step(TurnStepAst::BeginningOfCombat, "you")]);
        } else if rest.starts_with("combat on each opponent's turn") {
            return Ok(vec![trig_step(
                TurnStepAst::BeginningOfCombat,
                "each_opponent",
            )]);
        } else if rest.starts_with("combat on each player's turn") {
            return Ok(vec![trig_step(
                TurnStepAst::BeginningOfCombat,
                "each_player",
            )]);
        } else {
            "any"
        };

        let rest = rest.strip_prefix("first ").unwrap_or(rest);
        if rest.contains('.') {
            return Err(format!("multi_sentence_step_trigger:{c}"));
        }
        let step = if rest.starts_with("upkeep") {
            TurnStepAst::Upkeep
        } else if rest.starts_with("end step") {
            TurnStepAst::EndStep
        } else if rest.starts_with("draw step") {
            TurnStepAst::DrawStep
        } else if rest.starts_with("precombat main") || rest.starts_with("first main") {
            TurnStepAst::PrecombatMain
        } else if rest.starts_with("postcombat main") || rest.starts_with("second main") {
            TurnStepAst::PostcombatMain
        } else if rest.starts_with("main phase") {
            TurnStepAst::MainPhase
        } else if rest.starts_with("untap step") {
            TurnStepAst::UntapStep
        } else if rest.starts_with("combat") {
            if is_end {
                TurnStepAst::EndOfCombat
            } else {
                TurnStepAst::BeginningOfCombat
            }
        } else if rest == "the monarch's end step" {
            TurnStepAst::EndStep
        } else {
            return Err(format!("unparsed_step_trigger:{c}"));
        };
        return Ok(vec![trig_step(step, whose.to_string())]);
    }

    let blow_raw = strip_prefix_any(&low, &["whenever ", "when "])
        .unwrap_or(&low)
        .trim();
    let blow = blow_raw.strip_suffix(" this turn").unwrap_or(blow_raw);

    // Battalion: "~ and at least two other creatures attack"
    if let Some(after) = blow.strip_prefix("~ and at least ")
        && let Some((cnt_tok, rest)) = after.split_once(" other ")
        && let Some(flt_raw) = rest.strip_suffix(" attack")
        && let Some(n) = parse_number_word(cnt_tok)
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::AttacksWithOthers {
            subject: self_obj(),
            min_others: u8::try_from(n.max(1)).unwrap_or(2),
            other_filter: Box::new(flt),
        }]);
    }

    // Generalized recursive "~ or <other_subject_and_event>" (Bestow, Reconfigure, Morph, Typal Lords)
    if let Some(rest) = blow.strip_prefix("~ or ")
        && !rest.starts_with("another ")
        && let Ok(mut evs) = parse_trigger_events(rest, vocab)
    {
        if let Some(first) = evs.first().cloned() {
            evs.insert(0, with_trigger_subject_self(first));
        }
        return Ok(evs);
    }
    // Compound "~ or another <filter> <event>" (generalized!)
    if let Some(after) = blow.strip_prefix("~ or another ") {
        for (suf, kind) in [
            (" enters the battlefield", "enter"),
            (" enter the battlefield", "enter"),
            (" enters", "enter"),
            (" enter", "enter"),
            (" dies", "die"),
            (" die", "die"),
            (" leaves the battlefield", "leave"),
            (" leave the battlefield", "leave"),
            (" attacks", "attack"),
            (" attack", "attack"),
        ] {
            if let Some(flt_raw) = after.strip_suffix(suf)
                && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
            {
                flt.flags = flt.flags.union(crate::ast::ObjectFilterFlags::EXCLUDE_SELF);
                let other_subj = Box::new(SubjectRefAst::Group(Box::new(flt)));
                let self_subj = self_obj();
                return Ok(match kind {
                    "enter" => vec![
                        TriggerEventAst::ZoneChange {
                            from: None,
                            to: Some("battlefield".to_string()),
                            subject: self_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                        TriggerEventAst::ZoneChange {
                            from: None,
                            to: Some("battlefield".to_string()),
                            subject: other_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                    ],
                    "die" => vec![
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: Some("graveyard".to_string()),
                            subject: self_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: Some("graveyard".to_string()),
                            subject: other_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                    ],
                    "leave" => vec![
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: None,
                            subject: self_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: None,
                            subject: other_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                    ],
                    _ => vec![
                        TriggerEventAst::Attacks {
                            subject: Some(self_subj),
                            actor: None,
                            qualifier: None,
                            grouped: false,
                        },
                        TriggerEventAst::Attacks {
                            subject: Some(other_subj),
                            actor: None,
                            qualifier: None,
                            grouped: false,
                        },
                    ],
                });
            }
        }
        // Fallback: recursively parse "another <after>" for deals combat damage, turned face up, etc.!
        if let Ok(mut evs) = parse_trigger_events(&format!("another {after}"), vocab) {
            if let Some(first) = evs.first().cloned() {
                evs.insert(0, first);
            }
            return Ok(evs);
        }
    }

    // Compound enters-or-attacks / enters-or-dies / enters-or-leaves for any subject
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or attacks")
        .or_else(|| blow.strip_suffix(" enters the battlefield or attacks"))
        .or_else(|| blow.strip_suffix(" enter or attack"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            trig_zone(None, Some("battlefield"), subj.clone()),
            trig_attacks(Some(subj), None, None),
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or dies")
        .or_else(|| blow.strip_suffix(" enters the battlefield or dies"))
        .or_else(|| blow.strip_suffix(" enters or is put into a graveyard from the battlefield"))
        .or_else(|| {
            blow.strip_suffix(
                " enters the battlefield or is put into a graveyard from the battlefield",
            )
        })
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            trig_zone(None, Some("battlefield"), subj.clone()),
            trig_zone(Some("battlefield"), Some("graveyard"), subj),
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or leaves the battlefield")
        .or_else(|| blow.strip_suffix(" enters the battlefield or leaves the battlefield"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            trig_zone(None, Some("battlefield"), subj.clone()),
            trig_zone(Some("battlefield"), None, subj),
        ]);
    }

    if let Some(subj_raw) = blow
        .strip_suffix(" dies or is put into exile from the battlefield")
        .or_else(|| blow.strip_suffix(" die or are put into exile from the battlefield"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            trig_zone(Some("battlefield"), Some("graveyard"), subj.clone()),
            trig_zone(Some("battlefield"), Some("exile"), subj),
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or deals combat damage to a player")
        .or_else(|| blow.strip_suffix(" enters the battlefield or deals combat damage to a player"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            trig_zone(None, Some("battlefield"), subj.clone()),
            trig_dmg(
                true,
                false,
                Some(subj),
                Some(SubjectRefAst::Player(PlayerRefAst::AnyPlayer)),
                false,
                false,
            ),
        ]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" is attacked", " are attacked"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::PlayerAttacked { player }]);
    }
    if blow == "you roll the planar die" {
        return Ok(vec![TriggerEventAst::RollDie {
            player: PlayerRefAst::You,
            exact_result: None,
        }]);
    }
    if blow.starts_with("you roll a ") && blow.ends_with(" or higher on a die") {
        return Ok(vec![TriggerEventAst::RollDie {
            player: PlayerRefAst::You,
            exact_result: None,
        }]);
    }
    if let Some((subj_raw, _)) = split_once_any(
        &blow,
        &[" becomes attached to ", " becomes unattached from "],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesAttached {
            subject: Box::new(subj),
            attachment_kind: if blow.contains("unattached") {
                "unattached".to_string()
            } else {
                "attached".to_string()
            },
        }]);
    }
    if blow == "the ring tempts you" {
        return Ok(vec![trig_kw(PlayerRefAst::You, "ring_tempts_you")]);
    }
    if blow == "you activate an exhaust ability" {
        return Ok(vec![trig_kw(PlayerRefAst::You, "exhaust")]);
    }
    if let Some(who_raw) = strip_suffix_any(
        &blow,
        &[" shuffles their library", " shuffle their library"],
    ) && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![trig_kw(player, "shuffle_library")]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" becomes the monarch", " become the monarch"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![trig_kw(player, "become_monarch")]);
    }
    if let Some(who_raw) = blow.strip_suffix(" activates a loyalty ability")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::QualifiedAbilityActivated {
            player,
            source_filter: Box::new(any_perm()),
            loyalty_only: true,
            nonmana_only: false,
        }]);
    }
    if let Some(who_raw) = blow.strip_suffix(" activates an ability that isn't a mana ability")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::QualifiedAbilityActivated {
            player,
            source_filter: Box::new(any_perm()),
            loyalty_only: false,
            nonmana_only: true,
        }]);
    }
    if let Some((who_raw, flt_raw)) = split_once_any(
        &blow,
        &[" activates an ability of ", " activate an ability of "],
    ) {
        let (flt_clean, nonmana_only) = match flt_raw.strip_suffix(" that isn't a mana ability") {
            Some(c) => (c, true),
            None => (flt_raw, false),
        };
        if let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_clean, vocab),
        ) {
            if nonmana_only {
                return Ok(vec![TriggerEventAst::QualifiedAbilityActivated {
                    player,
                    source_filter: Box::new(flt),
                    loyalty_only: false,
                    nonmana_only: true,
                }]);
            }
            return Ok(vec![TriggerEventAst::AbilityActivated {
                player,
                source_filter: Box::new(flt),
            }]);
        }
    }
    if blow == "a spell or ability is put onto the stack" {
        return Ok(vec![TriggerEventAst::SpellOrAbilityOnStack]);
    }
    if blow == "you play a land or cast a spell" {
        return Ok(vec![
            TriggerEventAst::LandPlayed {
                player: PlayerRefAst::You,
            },
            trig_cast(PlayerRefAst::You, None),
        ]);
    }

    if blow == "~ enters or the creature it haunts dies"
        || blow == "~ enters the battlefield or the creature it haunts dies"
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: self_obj(),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::HauntedCreatureDies,
        ]);
    }
    if blow == "the creature ~ haunts dies" {
        return Ok(vec![TriggerEventAst::HauntedCreatureDies]);
    }
    if blow == "you lose control of ~" {
        return Ok(vec![trig_kw(PlayerRefAst::You, "lose_control_self")]);
    }
    if blow == "players finish voting" {
        return Ok(vec![trig_kw(PlayerRefAst::EachPlayer, "finish_voting")]);
    }
    if blow == "you get one or more {e}" || blow == "you get one or more {tk}" {
        return Ok(vec![trig_kw(PlayerRefAst::You, blow.to_string())]);
    }
    if blow == "you play a land from exile or cast a spell from exile" {
        return Ok(vec![
            TriggerEventAst::LandPlayed {
                player: PlayerRefAst::You,
            },
            TriggerEventAst::CastFromOutsideHand {
                player: PlayerRefAst::You,
            },
        ]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you cycle or discard ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::Cycled {
                subject: Box::new(grp(flt.clone())),
            },
            TriggerEventAst::CardDiscarded {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
            },
        ]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you tap ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(grp(flt)),
            tapped: true,
            for_mana: false,
        }]);
    }
    if let Some(after) = blow.strip_prefix("excess damage is dealt to ")
        && let Ok(subj) =
            parse_subject_or_target(after.strip_suffix(" this way").unwrap_or(after), vocab)
    {
        let ev = TriggerEventAst::DamageDealt {
            combat_only: false,
            noncombat_only: false,
            source: None,
            recipient: Some(Box::new(subj)),
            grouped: false,
            excess_only: true,
        };
        if after.ends_with(" this way") {
            return Ok(vec![TriggerEventAst::QualifiedTrigger {
                inner: Box::new(ev),
                first_time_each_turn: false,
                timing: Some("this_way".to_string()),
            }]);
        }
        return Ok(vec![ev]);
    }
    if low.starts_with("when there are no ")
        && let Ok(cond) = parse_condition(&low[5..], vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }
    if blow == "you scry or surveil" {
        return Ok(vec![
            trig_kw(PlayerRefAst::You, "scry"),
            trig_kw(PlayerRefAst::You, "surveil"),
        ]);
    }
    if blow == "you attack a player" || blow == "you attack an opponent" {
        return Ok(vec![trig_attacks(
            None,
            Some(PlayerRefAst::You),
            Some("player".to_string()),
        )]);
    }
    if let Some((who_raw, flt_raw)) = split_once_any(&blow, &[" mills ", " mill "])
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(peel_one_or_more(flt_raw).0, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::CardMilled {
            player,
            filter: Box::new(flt),
        }]);
    }
    if blow == "a player loses the game" || blow == "an opponent loses the game" {
        return Ok(vec![trig_kw(
            if blow.starts_with("an opponent") {
                PlayerRefAst::AnyOpponent
            } else {
                PlayerRefAst::AnyPlayer
            },
            "loses_game",
        )]);
    }
    if blow == "a player kicks a spell" || blow == "you kick a spell" {
        return Ok(vec![trig_kw(
            if blow.starts_with("you") {
                PlayerRefAst::You
            } else {
                PlayerRefAst::AnyPlayer
            },
            "kick_spell",
        )]);
    }
    if let Some((who_raw, rest)) = split_once_any(&blow, &[" puts ", " put "])
        && let Some(flt_raw) = rest.strip_suffix(" onto the battlefield")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        let entering_controller = player_ref_to_controller_relation(player);
        return Ok(vec![trig_zone_ext(
            None,
            Some("battlefield"),
            grp(flt),
            false,
            entering_controller,
        )]);
    }
    if blow == "a player plays a land or casts a spell"
        || blow == "a player plays a land from exile or casts a spell from exile"
    {
        return Ok(vec![
            TriggerEventAst::LandPlayed {
                player: PlayerRefAst::AnyPlayer,
            },
            trig_cast(PlayerRefAst::AnyPlayer, None),
        ]);
    }
    if let Some((who_raw, subj_raw)) = split_once_any(&blow, &[" cycles ", " cycle "])
        && let (Some(_player), Ok(subj)) = (
            parse_player_ref(who_raw),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::Cycled {
            subject: Box::new(subj),
        }]);
    }

    // Compound self triggers
    match blow {
        "~ enters or attacks" | "~ enters the battlefield or attacks" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::Attacks {
                    subject: Some(self_obj()),
                    actor: None,
                    qualifier: None,
                    grouped: false,
                },
            ]);
        }
        "~ enters or dies" | "~ enters the battlefield or dies" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: Some("graveyard".to_string()),
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
            ]);
        }
        "~ enters or transforms into ~" | "~ enters the battlefield or transforms into ~" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::Transforms {
                    subject: self_obj(),
                },
            ]);
        }
        "~ enters or is turned face up" | "~ enters the battlefield or is turned face up" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::TurnedFaceUp {
                    subject: self_obj(),
                },
            ]);
        }
        "~ enters or leaves the battlefield"
        | "~ enters the battlefield or leaves the battlefield" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: None,
                    subject: self_obj(),
                    grouped: false,
                    entering_controller: None,
                },
            ]);
        }
        _ => {}
    }

    if let Some(subj_raw) = blow.strip_suffix(" attacks or blocks")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            trig_attacks(Some(subj.clone()), None, None),
            TriggerEventAst::Blocks {
                subject: Box::new(subj),
                attacker: None,
            },
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" blocks or becomes blocked")
        .or_else(|| blow.strip_suffix(" blocks or becomes blocked by a creature"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::Blocks {
                subject: Box::new(subj.clone()),
                attacker: None,
            },
            TriggerEventAst::BecomesBlocked {
                subject: Box::new(subj),
                blocker: None,
            },
        ]);
    }
    // Filtered / multi-creature blocks or becomes-blocked-by triggers (Serra Inquisitors, Giant Shark, Dwarven Soldier, Rashka, Amphibious Kavu, Hedron Blade, Lairwatch Giant, Godsend)
    for (needle, blocks, becomes_blocked) in [
        (" blocks or becomes blocked by ", true, true),
        (" becomes blocked by ", false, true),
        (" blocks ", true, false),
    ] {
        for cand in [blow_raw, blow] {
            if let Some((subj_raw, rhs_raw)) = cand.split_once(needle)
                && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
            {
                let (min_count, flt_raw) = if let Some(r) = rhs_raw.strip_prefix("two or more ") {
                    (2u8, r)
                } else if let Some(r) = rhs_raw.strip_prefix("one or more ") {
                    (1u8, r)
                } else if let Some(r) = rhs_raw.strip_prefix("three or more ") {
                    (3u8, r)
                } else {
                    (1u8, rhs_raw)
                };
                if let Ok(other_filter) = parse_object_filter(flt_raw.trim(), vocab) {
                    return Ok(vec![TriggerEventAst::BlocksOrBecomesBlockedBy {
                        subject: Box::new(subj),
                        other_filter: Box::new(other_filter),
                        blocks,
                        becomes_blocked,
                        min_count,
                    }]);
                }
            }
            if blow_raw == blow {
                break;
            }
        }
    }

    // First / second spell each turn
    if blow == "day becomes night or night becomes day" {
        return Ok(vec![TriggerEventAst::DayNightChanged]);
    }
    if blow == "you cast a spell during an opponent's turn"
        || blow == "you cast your first spell during each opponent's turn"
    {
        return Ok(vec![trig_cast_ext(
            PlayerRefAst::You,
            None,
            false,
            None,
            if blow.contains("first") {
                Some(1)
            } else {
                None
            },
            false,
        )]);
    }
    if let Some(after) = blow.strip_prefix("you cast your first ")
        && let Some(flt_raw) = after.strip_suffix(" each turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![trig_cast_ext(
            PlayerRefAst::You,
            Some(flt),
            false,
            None,
            Some(1),
            false,
        )]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you next cast ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![trig_cast_ext(
            PlayerRefAst::You,
            Some(flt),
            false,
            None,
            Some(1),
            false,
        )]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" searches their library")
        .or_else(|| blow.strip_suffix(" search their library"))
        .or_else(|| blow.strip_suffix(" searches a library"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LibrarySearched { player }]);
    }
    if blow == "you cast your second spell each turn"
        || blow == "a player casts their second spell each turn"
    {
        return Ok(vec![trig_cast_ext(
            if blow.starts_with("you") {
                PlayerRefAst::You
            } else {
                PlayerRefAst::AnyPlayer
            },
            None,
            false,
            None,
            Some(2),
            false,
        )]);
    }
    if blow == "you cast your third spell each turn" {
        return Ok(vec![trig_cast_ext(
            PlayerRefAst::You,
            None,
            false,
            None,
            Some(3),
            false,
        )]);
    }
    if blow == "you cast a spell other than your first spell each turn" {
        return Ok(vec![TriggerEventAst::QualifiedTrigger {
            inner: Box::new(trig_cast(PlayerRefAst::You, None)),
            first_time_each_turn: false,
            timing: Some("other_than_first_each_turn".to_string()),
        }]);
    }
    if blow == "you cast a spell during your turn" || blow == "you cast a spell during combat" {
        return Ok(vec![TriggerEventAst::QualifiedTrigger {
            inner: Box::new(trig_cast(PlayerRefAst::You, None)),
            first_time_each_turn: false,
            timing: Some(if blow.ends_with("during combat") {
                "during_combat".to_string()
            } else {
                "during_your_turn".to_string()
            }),
        }]);
    }

    // Cast or copy (any player)
    if let Some((who_raw, spell_raw)) =
        split_once_any(&blow, &[" casts or copies ", " cast or copy "])
        && let (Some(caster), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(spell_raw, vocab),
        )
    {
        return Ok(vec![trig_cast_ext(
            caster,
            Some(flt),
            false,
            None,
            None,
            true,
        )]);
    }
    // "<player> casts their first/second [<filter>] spell each turn"
    if let Some((who_raw, rest)) = split_once_any(&blow, &[" casts their ", " cast their "])
        && let Some(caster) = parse_player_ref(who_raw)
        && let Some(core) = rest.strip_suffix(" each turn")
    {
        let (nth, flt_opt) = if let Some(f) = core.strip_prefix("first ") {
            (Some(1u8), Some(f))
        } else if let Some(f) = core.strip_prefix("second ") {
            (Some(2u8), Some(f))
        } else {
            (None, None)
        };
        if let (Some(n), Some(flt_raw)) = (nth, flt_opt) {
            let spell = if flt_raw == "spell" {
                None
            } else if let Ok(f) = parse_object_filter(flt_raw, vocab) {
                Some(Box::new(f))
            } else {
                None
            };
            if spell.is_some() || flt_raw == "spell" {
                return Ok(vec![TriggerEventAst::SpellCast {
                    caster,
                    spell,
                    self_spell: false,
                    targets: None,
                    nth_each_turn: Some(n),
                    include_copy: false,
                }]);
            }
        }
    }

    // Attacks and isn't blocked
    if let Some(subj_raw) = strip_suffix_any(
        &blow,
        &[" attacks and isn't blocked", " attack and isn't blocked"],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::AttacksUnblocked {
            subject: Box::new(subj),
        }]);
    }

    // Enters
    if !blow.starts_with("you ")
        && !blow.starts_with("a player ")
        && !blow.starts_with("an opponent ")
    {
        let (core_enter, entering_ctrl) = if let Some(p) = blow.strip_suffix(" under your control")
        {
            (p, Some(ControllerRelation::You))
        } else if let Some(p) = blow.strip_suffix(" under an opponent's control") {
            (p, Some(ControllerRelation::Opponent))
        } else {
            (blow, None)
        };
        let (core_enter, from_zone, enter_timing) =
            if let Some(p) = core_enter.strip_suffix(" from a graveyard") {
                (p, Some("graveyard".to_string()), None)
            } else if let Some(p) = core_enter.strip_suffix(" from your graveyard") {
                (p, Some("your_graveyard".to_string()), None)
            } else if let Some(p) = core_enter.strip_suffix(" from exile") {
                (p, Some("exile".to_string()), None)
            } else if let Some(p) = core_enter.strip_suffix(" during the declare attackers step") {
                (p, None, Some("during_declare_attackers_step".to_string()))
            } else if let Some(p) = core_enter.strip_suffix(" during combat") {
                (p, None, Some("during_combat".to_string()))
            } else {
                (core_enter, None, None)
            };
        let (subj_opt, tapped_qual) = if let Some(p) = strip_suffix_any(
            &core_enter,
            &[" enters the battlefield untapped", " enters untapped"],
        ) {
            (Some(p), Some("untapped"))
        } else if let Some(p) = strip_suffix_any(
            &core_enter,
            &[" enters the battlefield tapped", " enters tapped"],
        ) {
            (Some(p), Some("tapped"))
        } else {
            (
                strip_suffix_any(
                    &core_enter,
                    &[
                        " enters the battlefield",
                        " enter the battlefield",
                        " enters",
                        " enter",
                    ],
                ),
                None,
            )
        };
        if let Some(subj_raw) = subj_opt {
            let (sraw, grouped) = peel_one_or_more(subj_raw);
            if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
                let base_ev = TriggerEventAst::ZoneChange {
                    from: from_zone,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(subj),
                    grouped,
                    entering_controller: entering_ctrl,
                };
                let qual_timing = match (enter_timing, tapped_qual) {
                    (Some(t), Some(tq)) => Some(format!("{t}:{tq}")),
                    (Some(t), None) => Some(t),
                    (None, Some(tq)) => Some(format!("enters_{tq}")),
                    (None, None) => None,
                };
                let ev = if let Some(timing) = qual_timing {
                    TriggerEventAst::QualifiedTrigger {
                        inner: Box::new(base_ev),
                        first_time_each_turn: false,
                        timing: Some(timing),
                    }
                } else {
                    base_ev
                };
                return Ok(vec![ev]);
            }
        }
    }

    // Dies
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" dies", " die"]) {
        let (sraw, grouped) = peel_one_or_more(subj_raw);
        if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
            return Ok(vec![trig_zone_ext(
                Some("battlefield"),
                Some("graveyard"),
                subj,
                grouped,
                None,
            )]);
        }
    }

    // Leaves battlefield / graveyard (with optional trailing "during your turn")
    let blow_noturn = strip_suffix_any(&blow, &[" during your turn", " during an opponent's turn"])
        .unwrap_or(blow);
    for (suf, from) in [
        (" leaves the battlefield", "battlefield"),
        (" leave the battlefield", "battlefield"),
        (" leaves your graveyard", "your_graveyard"),
        (" leave your graveyard", "your_graveyard"),
        (" leaves an opponent's graveyard", "opponent_graveyard"),
        (" leave an opponent's graveyard", "opponent_graveyard"),
        (" leaves a player's graveyard", "any_graveyard"),
        (" leave a player's graveyard", "any_graveyard"),
        (" leaves a graveyard", "any_graveyard"),
        (" leave a graveyard", "any_graveyard"),
    ] {
        if let Some(subj_raw) = blow_noturn.strip_suffix(suf) {
            let (sraw, grouped) = peel_one_or_more(subj_raw);
            if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
                return Ok(vec![TriggerEventAst::ZoneChange {
                    from: Some(from.to_string()),
                    to: None,
                    subject: Box::new(subj),
                    grouped,
                    entering_controller: None,
                }]);
            }
        }
    }

    // Put into a graveyard / exile / library / milled (with peel_one_or_more!)
    let blow_noturn_zone =
        strip_suffix_any(&blow, &[" during your turn", " during an opponent's turn"])
            .unwrap_or(blow);
    if let Some(subj_raw) = strip_suffix_any(&blow_noturn_zone, &[" are milled", " is milled"]) {
        let (sraw, _) = peel_one_or_more(subj_raw);
        if let Ok(flt) = parse_object_filter(sraw, vocab) {
            return Ok(vec![TriggerEventAst::CardMilled {
                player: PlayerRefAst::AnyPlayer,
                filter: Box::new(flt),
            }]);
        }
    }
    if let Some((subj_raw, rest)) =
        split_once_any(&blow_noturn_zone, &[" is put into ", " are put into "])
        && !rest.contains(", or ")
    {
        let (sraw, grouped) = peel_one_or_more(subj_raw);
        let dest_zone = if rest.contains("graveyard") {
            Some("graveyard")
        } else if rest.contains("exile") {
            Some("exile")
        } else if rest.contains("library") {
            Some("library")
        } else {
            None
        };
        let from_zone = if rest.contains("from anywhere other than the battlefield") {
            "not_battlefield"
        } else {
            "any"
        };
        if let (Some(to_z), Ok(subj)) = (dest_zone, parse_subject_or_target(sraw, vocab)) {
            return Ok(vec![TriggerEventAst::ZoneChange {
                from: Some(from_zone.to_string()),
                to: Some(to_z.to_string()),
                subject: Box::new(subj),
                grouped,
                entering_controller: None,
            }]);
        }
    }

    // Deals combat / noncombat / any damage
    for (needle, combat_only, noncombat_only) in [
        (" deals combat damage", true, false),
        (" deal combat damage", true, false),
        (" deals noncombat damage", false, true),
        (" deal noncombat damage", false, true),
        (" deals damage", false, false),
        (" deal damage", false, false),
    ] {
        if let Some((src_raw, tail)) = blow.split_once(needle) {
            let (sraw, grouped) = peel_one_or_more(src_raw);
            if let Ok(src) = parse_subject_or_target(sraw, vocab) {
                let recipient = tail
                    .trim()
                    .strip_prefix("to ")
                    .and_then(|r| parse_subject_or_target(r, vocab).ok())
                    .map(Box::new);
                return Ok(vec![TriggerEventAst::DamageDealt {
                    combat_only,
                    noncombat_only,
                    source: Some(Box::new(src)),
                    recipient,
                    grouped,
                    excess_only: false,
                }]);
            }
        }
    }

    // Is dealt damage (including excess / excess noncombat / excess combat damage: Fall of Cair Andros, Aegar)
    for (suf, combat_only, noncombat_only) in [
        (" is dealt excess combat damage", true, false),
        (" are dealt excess combat damage", true, false),
        (" is dealt excess noncombat damage", false, true),
        (" are dealt excess noncombat damage", false, true),
        (" is dealt excess damage", false, false),
        (" are dealt excess damage", false, false),
        (" is dealt combat damage", true, false),
        (" are dealt combat damage", true, false),
        (" is dealt noncombat damage", false, true),
        (" are dealt noncombat damage", false, true),
        (" is dealt damage", false, false),
        (" are dealt damage", false, false),
        ("'re dealt damage", false, false),
    ] {
        if let Some(tgt_raw) = blow.strip_suffix(suf)
            && let Ok(tgt) = parse_subject_or_target(tgt_raw, vocab)
        {
            return Ok(vec![trig_dmg(
                combat_only,
                noncombat_only,
                None,
                Some(tgt),
                false,
                suf.contains("excess "),
            )]);
        }
    }

    // Attacks
    if blow == "you attack" {
        return Ok(vec![trig_attacks(None, Some(PlayerRefAst::You), None)]);
    }
    if let Some(after) = blow.strip_prefix("you attack with ") {
        let keep_as_qualifier = after.starts_with("two or more")
            || after.starts_with("three or more")
            || after.starts_with("four or more")
            || after.starts_with("five or more")
            || after.starts_with("creatures with total power ")
            || after == "one or more equipped creatures"
            || after.contains(" or more creatures with ");
        if !keep_as_qualifier {
            let (flt_raw, grouped) = peel_one_or_more(after);
            if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                return Ok(vec![trig_attacks_ext(
                    Some(grp(flt)),
                    Some(PlayerRefAst::You),
                    None,
                    grouped,
                )]);
            }
        }
        return Ok(vec![trig_attacks(
            None,
            Some(PlayerRefAst::You),
            Some(after.to_string()),
        )]);
    }
    if let Some(qual) = strip_prefix_any(&blow, &["an opponent attacks ", "a player attacks "]) {
        return Ok(vec![trig_attacks(
            None,
            Some(if blow.starts_with("a player") {
                PlayerRefAst::AnyPlayer
            } else {
                PlayerRefAst::AnyOpponent
            }),
            Some(qual.to_string()),
        )]);
    }
    for (suf, qual) in [
        (
            " attacks for the first time each turn",
            Some("first_time_each_turn"),
        ),
        (" attacks while saddled", Some("while_saddled")),
        (" attacks while exerted", Some("while_exerted")),
        (" attacks a player alone", Some("alone")),
        (" attacks alone", Some("alone")),
        (" attacks enchanted player", Some("enchanted_player")),
        (
            " attacks you or a planeswalker you control",
            Some("you_or_planeswalker"),
        ),
        (
            " attacks one of your opponents or a planeswalker an opponent controls",
            Some("opponent_or_planeswalker"),
        ),
        (" attacks you", Some("you")),
        (" attack you", Some("you")),
        (" attacks an opponent", Some("opponent")),
        (" attack an opponent", Some("opponent")),
        (" attacks one of your opponents", Some("opponent")),
        (" attack one of your opponents", Some("opponent")),
        (
            " attack one of your opponents or a planeswalker they control",
            Some("opponent_or_planeswalker"),
        ),
        (" attack one or more players", Some("player")),
        (" attacks a player", Some("player")),
        (" attack a player", Some("player")),
        (" attacks a planeswalker", Some("planeswalker")),
        (" attack a planeswalker", Some("planeswalker")),
        (" attacks", None),
        (" attack", None),
    ] {
        if let Some(subj_raw) = blow.strip_suffix(suf) {
            let (sraw, grouped) = peel_one_or_more(subj_raw);
            if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
                return Ok(vec![trig_attacks_ext(
                    Some(subj),
                    None,
                    qual.map(ToString::to_string),
                    grouped,
                )]);
            }
        }
    }

    // Becomes blocked / blocks (with peel_one_or_more! Do not swallow compound ", " / " or " tails.)
    if let Some((subj_raw, rest)) = split_once_any(&blow, &[" becomes blocked", " become blocked"])
        && !rest.starts_with(',')
        && !rest.trim_start().starts_with("or ")
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        let rest_trim = rest.trim();
        let blocker = rest_trim
            .strip_prefix("by ")
            .and_then(|r| parse_subject_or_target(r, vocab).ok())
            .map(Box::new);
        if rest_trim.is_empty() || blocker.is_some() {
            return Ok(vec![TriggerEventAst::BecomesBlocked {
                subject: Box::new(subj),
                blocker,
            }]);
        }
    }
    if let Some((subj_raw, rest)) = split_once_any(&blow, &[" blocks", " block"])
        && (rest.is_empty() || rest.starts_with(' '))
        && !rest.trim_start().starts_with("or ")
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        let rest_trim = rest.trim();
        let attacker = if rest_trim.is_empty() {
            None
        } else {
            parse_subject_or_target(rest_trim, vocab).ok().map(Box::new)
        };
        if rest_trim.is_empty() || attacker.is_some() {
            return Ok(vec![TriggerEventAst::Blocks {
                subject: Box::new(subj),
                attacker,
            }]);
        }
    }

    // SpellCast: "<player> cast(s) <spell>"
    if let Some((who_raw, spell_raw)) = split_once_any(&blow, &[" casts ", " cast "])
        && let Some(caster) = parse_player_ref(who_raw)
    {
        if spell_raw == "~" {
            return Ok(vec![trig_cast_ext(caster, None, true, None, None, false)]);
        }
        if let Some((flt_raw, tgt_raw)) =
            split_once_any(&spell_raw, &[" that targets only ", " that targets "])
            && let (Ok(flt), Ok(tgt)) = (
                parse_object_filter(flt_raw, vocab),
                parse_subject_or_target(
                    peel_one_or_more(tgt_raw.strip_prefix("a single ").unwrap_or(tgt_raw)).0,
                    vocab,
                ),
            )
        {
            return Ok(vec![trig_cast_ext(
                caster,
                Some(flt),
                false,
                Some(tgt),
                None,
                false,
            )]);
        }
        let (spell_clean, cast_qual) =
            if let Some(p) = spell_raw.strip_suffix(" using mana produced by ~") {
                (p, Some("using_mana_produced_by_self".to_string()))
            } else if let Some(p) = spell_raw.strip_suffix(" during an opponent's turn") {
                (p, Some("during_opponents_turn".to_string()))
            } else if let Some(p) = spell_raw.strip_suffix(" during your turn") {
                (p, Some("during_your_turn".to_string()))
            } else if let Some(p) = spell_raw.strip_suffix(" using teamwork") {
                (p, Some("using_teamwork".to_string()))
            } else {
                (spell_raw, None)
            };
        let wrap_cast_qual = |ev: TriggerEventAst| -> TriggerEventAst {
            if let Some(q) = cast_qual.clone() {
                TriggerEventAst::QualifiedTrigger {
                    inner: Box::new(ev),
                    first_time_each_turn: false,
                    timing: Some(q),
                }
            } else {
                ev
            }
        };
        if let Some((flt_raw, kw_raw)) = spell_clean.split_once(" that has ")
            && vocab.is_evergreen_keyword(kw_raw.trim())
            && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
        {
            flt.required_keywords.push(kw_raw.trim().to_string());
            return Ok(vec![wrap_cast_qual(trig_cast(caster, Some(flt)))]);
        }
        if let Some((flt_raw, colors_raw)) = spell_clean.split_once(" that's both ")
            && let Ok(flt) = parse_object_filter(&format!("{flt_raw} {colors_raw}"), vocab)
        {
            return Ok(vec![wrap_cast_qual(trig_cast(caster, Some(flt)))]);
        }
        if spell_clean.contains(" in its mana cost")
            || spell_clean.contains("mana cost that contains")
        {
            let base_ev = TriggerEventAst::SpellCast {
                caster,
                spell: Some(Box::new(ObjectFilterAst {
                    any_spell: true,
                    predicates: vec![crate::ast::FilterPredicateAst::HasManaSymbolInCost(
                        spell_clean.to_string(),
                    )],
                    ..Default::default()
                })),
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            };
            return Ok(vec![wrap_cast_qual(base_ev)]);
        }
        if let Ok(flt) = parse_object_filter(spell_clean, vocab) {
            return Ok(vec![wrap_cast_qual(trig_cast(caster, Some(flt)))]);
        }
    }

    // Player actions: draw / discard / sacrifice / gain-lose life / cycle
    if let Some((who_raw, qual)) = split_once_any(&blow, &[" draws ", " draw "])
        && let Some(player) = parse_player_ref(who_raw)
        && qual.contains("card")
    {
        return Ok(vec![TriggerEventAst::CardDrawn {
            player,
            qualifier: qual.to_string(),
        }]);
    }
    if let Some((who_raw, flt_raw)) = split_once_any(&blow, &[" discards ", " discard "])
        && let Some(player) = parse_player_ref(who_raw)
    {
        let flt = parse_object_filter(flt_raw, vocab).unwrap_or_else(|_| any_card_flt());
        return Ok(vec![TriggerEventAst::CardDiscarded {
            player,
            filter: Box::new(flt),
        }]);
    }
    if let Some((who_raw, subj_raw)) = split_once_any(&blow, &[" sacrifices ", " sacrifice "])
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::PermanentSacrificed {
            player,
            subject: Box::new(subj),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" gain life for the first time each turn")
        .or_else(|| blow.strip_suffix(" gains life for the first time each turn"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: true,
            first_each_turn: true,
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" lose life for the first time each turn")
        .or_else(|| blow.strip_suffix(" loses life for the first time each turn"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: false,
            first_each_turn: true,
        }]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" gain life", " gains life"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: true,
            first_each_turn: false,
        }]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" lose life", " loses life"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: false,
            first_each_turn: false,
        }]);
    }

    // Tap / Untap / Mana / Targeted / Face-up / Transform / Counters
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" becomes tapped", " become tapped"])
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: true,
            for_mana: false,
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" becomes untapped", " become untapped"])
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: false,
            for_mana: false,
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" is tapped for mana")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: true,
            for_mana: true,
        }]);
    }
    if let Some(rest) = blow.strip_suffix(" for mana")
        && let Some((_, subj_raw)) = split_once_any(&rest, &[" taps ", " tap "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: true,
            for_mana: true,
        }]);
    }
    if let Some((subj_raw, by_raw)) = split_once_any(
        &blow,
        &[" becomes the target of ", " become the target of "],
    ) && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesTargeted {
            subject: Box::new(subj),
            by_opponent: by_raw.contains("opponent"),
            by_text: Some(by_raw.to_string()),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is exiled from the battlefield")
        .or_else(|| blow.strip_suffix(" are exiled from the battlefield"))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![trig_zone(Some("battlefield"), Some("exile"), subj)]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" fights", " fight"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::PermanentFights {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" is turned face up", " are turned face up"])
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::TurnedFaceUp {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) =
        strip_suffix_any(&blow, &[" transforms into ~", " transforms", " transform"])
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::Transforms {
            subject: Box::new(subj),
        }]);
    }
    if let Some(after) = blow.strip_prefix("you put ")
        && let Some((kind_raw, subj_raw)) =
            split_once_any(&after, &[" counters on ", " counter on "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = strip_prefix_any(&kind_raw, &["one or more ", "a ", "an "]).unwrap_or(kind_raw);
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(subj),
            counter_kind: kind.trim().to_string(),
            added: true,
            by_you: true,
        }]);
    }
    let (blow_noexile, cnt_first_time, cnt_timing) =
        if let Some(p) = blow.strip_suffix(" while it's exiled") {
            (p, false, Some("while_exiled".to_string()))
        } else if let Some(p) = blow.strip_suffix(" for the first time each turn") {
            (p, true, None)
        } else {
            (blow, false, None)
        };
    if let Some((left, subj_raw)) = split_once_any(
        &blow_noexile,
        &[
            " is put on ",
            " are put on ",
            " is placed on ",
            " are placed on ",
            " is removed from ",
            " are removed from ",
        ],
    ) && left.contains("counter")
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        let added = !blow_noexile.contains("removed from");
        let kind = strip_prefix_any(&left, &["one or more ", "a ", "an "])
            .unwrap_or(left)
            .trim_end_matches(" counters")
            .trim_end_matches(" counter")
            .trim();
        let base_ev = TriggerEventAst::CountersChanged {
            subject: Box::new(subj),
            counter_kind: kind.to_string(),
            added,
            by_you: false,
        };
        let ev = if cnt_first_time || cnt_timing.is_some() {
            TriggerEventAst::QualifiedTrigger {
                inner: Box::new(base_ev),
                first_time_each_turn: cnt_first_time,
                timing: cnt_timing,
            }
        } else {
            base_ev
        };
        return Ok(vec![ev]);
    }

    // State trigger
    if (blow.starts_with("you control no ") || blow.starts_with("there are no "))
        && let Ok(cond) = parse_condition(blow, vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }

    if blow == "you cast a spell from anywhere other than your hand" {
        return Ok(vec![TriggerEventAst::CastFromOutsideHand {
            player: PlayerRefAst::You,
        }]);
    }
    if let Some(n_raw) = blow.strip_prefix("you expend ")
        && let Ok(amount) = n_raw.trim().parse::<u16>()
    {
        return Ok(vec![TriggerEventAst::Expend {
            player: PlayerRefAst::You,
            amount,
        }]);
    }

    // Modern & supplemental Magic keyword/mechanic triggers
    if blow == "you crank this contraption" || blow == "you crank ~" {
        return Ok(vec![TriggerEventAst::CrankContraption]);
    }
    if blow == "you unlock this door" || blow == "you unlock a door" || blow == "you unlock ~" {
        return Ok(vec![TriggerEventAst::UnlockDoor {
            subject: self_obj(),
        }]);
    }
    if blow == "you fully unlock a room" {
        return Ok(vec![TriggerEventAst::RoomFullyUnlocked {
            player: PlayerRefAst::You,
        }]);
    }
    if let Some(lvl_raw) =
        strip_prefix_any(&blow, &["~ becomes level ", "this class becomes level "])
        && let Ok(level) = lvl_raw.trim().parse::<u8>()
    {
        return Ok(vec![TriggerEventAst::ClassLevelReached { level }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" mutates", " mutate"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Mutates {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" becomes monstrous")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesMonstrous {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" becomes renowned")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesRenowned {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) =
        strip_suffix_any(&blow, &[" exploits a creature", " exploit a creature"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Exploits {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes exerted")
        .or_else(|| blow.strip_prefix("you exert "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Exerted {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" becomes crewed", " crews a vehicle"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Crewed {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" explores", " explore"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Explores {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" connives", " connive"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Connives {
            subject: Box::new(subj),
        }]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" commit a crime", " commits a crime"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::CrimeCommitted { player }]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" play a land", " plays a land"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LandPlayed { player }]);
    }
    for (suf, action) in [
        (" scry", "scry"),
        (" scries", "scry"),
        (" surveil", "surveil"),
        (" surveils", "surveil"),
        (" investigate", "investigate"),
        (" investigates", "investigate"),
        (" proliferate", "proliferate"),
        (" proliferates", "proliferate"),
        (" populate", "populate"),
        (" populates", "populate"),
        (" forage", "forage"),
        (" forages", "forage"),
        (" venture into the dungeon", "venture"),
        (" ventures into the dungeon", "venture"),
        (" complete a dungeon", "complete_dungeon"),
        (" completes a dungeon", "complete_dungeon"),
        (" manifest dread", "manifest_dread"),
        (" manifests dread", "manifest_dread"),
    ] {
        if let Some(who_raw) = blow.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(vec![trig_kw(player, action.to_string())]);
        }
    }
    if let Some((who_raw, roll_raw)) = split_once_any(&blow, &[" roll a ", " rolls a "])
        && let (Some(player), Ok(n)) = (parse_player_ref(who_raw), roll_raw.trim().parse::<u16>())
    {
        return Ok(vec![TriggerEventAst::RollDie {
            player,
            exact_result: Some(n),
        }]);
    }
    if let Some(who_raw) = strip_suffix_any(
        &blow,
        &[
            " roll one or more dice",
            " rolls one or more dice",
            " roll a die",
            " rolls a die",
        ],
    ) && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::RollDie {
            player,
            exact_result: None,
        }]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" win a coin flip", " wins a coin flip"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::FlipCoin {
            player,
            won: Some(true),
        }]);
    }
    if let Some(who_raw) = strip_suffix_any(&blow, &[" lose a coin flip", " loses a coin flip"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::FlipCoin {
            player,
            won: Some(false),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" phases out", " phase out"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::PhaseChanged {
            subject: Box::new(subj),
            phased_out: true,
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" phases in", " phase in"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::PhaseChanged {
            subject: Box::new(subj),
            phased_out: false,
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" is exiled", " are exiled"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Exiled {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" regenerates this way", " regenerates"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::PermanentRegenerates {
            subject: Box::new(subj),
            this_way: blow.ends_with(" this way"),
        }]);
    }

    if blow == "you cast that spell" || blow == "you next cast that spell" {
        return Ok(vec![trig_cast(PlayerRefAst::You, None)]);
    }

    // Orthogonal Pass I triggers: token creation, loyalty/nonmana ability activation, crewed, last counter, state triggers, keyword actions
    if let Some((who_raw, rest)) =
        split_once_any(&blow, &[" create or sacrifice ", " creates or sacrifices "])
        && let Some(player) = parse_player_ref(who_raw)
    {
        let (flt_raw, _) = peel_one_or_more(rest);
        let (_, clean) = peel_leading_count(flt_raw, vocab);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(vec![
                trig_zone(None, Some("battlefield"), grp(flt.clone())),
                TriggerEventAst::PermanentSacrificed {
                    player,
                    subject: Box::new(grp(flt)),
                },
            ]);
        }
    }
    if let Some((who_raw, rest)) = split_once_any(&blow, &[" create ", " creates "])
        && let Some(player) = parse_player_ref(who_raw)
    {
        let (flt_raw, grouped) = peel_one_or_more(rest);
        let (_, clean) = peel_leading_count(flt_raw, vocab);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(vec![trig_zone_ext(
                None,
                Some("battlefield"),
                grp(flt),
                grouped,
                player_ref_to_controller_relation(player),
            )]);
        }
    }
    if let Some((who_raw, rest)) = split_once_any(&blow, &[" activate ", " activates "])
        && let Some(player) = parse_player_ref(who_raw)
        && (rest.starts_with("a loyalty ability of ")
            || rest.starts_with("an ability that isn't a mana ability")
            || rest.starts_with("an ability that targets "))
    {
        let src_flt = if let Some(subj_raw) = rest.strip_prefix("a loyalty ability of ") {
            parse_object_filter(subj_raw, vocab).unwrap_or_else(|_| any_perm())
        } else {
            any_perm()
        };
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player,
            source_filter: Box::new(src_flt),
        }]);
    }
    if let Some((who_raw, rest)) = blow.split_once(" remove the last ")
        && let Some((ck, subj_raw)) = rest.split_once(" counter from ")
        && parse_player_ref(who_raw).is_some()
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(subj),
            counter_kind: ck.trim().to_string(),
            added: false,
            by_you: true,
        }]);
    }
    if let Some((who_raw, rest)) = split_once_any(&blow, &[" roll a ", " rolls a "])
        && let Some(player) = parse_player_ref(who_raw)
        && rest
            .split(|c: char| !c.is_ascii_digit())
            .any(|tok| !tok.is_empty())
    {
        let first_num = rest
            .split(|c: char| !c.is_ascii_digit())
            .find(|tok| !tok.is_empty())
            .and_then(|t| t.parse::<u16>().ok());
        return Ok(vec![TriggerEventAst::RollDie {
            player,
            exact_result: first_num,
        }]);
    }
    for (kw_act, label) in [
        (" discover", "discover"),
        (" discovers", "discover"),
        (" clash", "clash"),
        (" clashes", "clash"),
        (" foretell a card", "foretell"),
        (" foretells a card", "foretell"),
    ] {
        if let Some(who_raw) = blow.strip_suffix(kw_act)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(vec![trig_kw(player, label.to_string())]);
        }
    }
    if blow == "you cast or cycle ~" {
        return Ok(vec![
            trig_cast_ext(PlayerRefAst::You, None, true, None, None, false),
            TriggerEventAst::Cycled {
                subject: self_obj(),
            },
        ]);
    }
    if let Some(subj_raw) = strip_suffix_any(
        &blow,
        &[
            " is sacrificed or destroyed",
            " are sacrificed or destroyed",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::PermanentSacrificed {
                player: PlayerRefAst::AnyPlayer,
                subject: Box::new(subj.clone()),
            },
            trig_zone(Some("battlefield"), Some("graveyard"), subj),
        ]);
    }
    if let Some(after) = blow.strip_prefix("you cast ~ from ")
        && !after.is_empty()
    {
        return Ok(vec![trig_cast_ext(
            PlayerRefAst::You,
            None,
            true,
            None,
            None,
            false,
        )]);
    }
    if let Some(after) = blow
        .strip_prefix("~ and/or one or more other ")
        .and_then(|r| strip_suffix_any(r, &[" enter", " enter the battlefield"]))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(vec![trig_zone_ext(
            None,
            Some("battlefield"),
            grp(flt),
            true,
            None,
        )]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is returned to its owner's hand")
        .or_else(|| blow.strip_suffix(" is returned to your hand"))
        .or_else(|| blow.strip_suffix(" are returned to their owners' hands"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![trig_zone(Some("battlefield"), Some("hand"), subj)]);
    }
    if let Some(subj_raw) = blow
        .strip_prefix("an ability of ")
        .and_then(|r| r.strip_suffix(" is activated"))
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player: PlayerRefAst::AnyPlayer,
            source_filter: Box::new(any_perm()),
        }]);
    }
    if blow == "you activate a boast ability" {
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player: PlayerRefAst::You,
            source_filter: Box::new(any_perm()),
        }]);
    }
    if let Some(subj_raw) = strip_suffix_any(&blow, &[" trains", " train"])
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![trig_kw(PlayerRefAst::You, "train")]);
    }
    if let Some(after) = blow.strip_prefix("you untap ") {
        let clean = after
            .strip_suffix(" during your untap step")
            .unwrap_or(after);
        let (flt_raw, _) = peel_one_or_more(clean);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(vec![TriggerEventAst::TapStateChanged {
                subject: Box::new(grp(flt)),
                tapped: false,
                for_mana: false,
            }]);
        }
    }
    if blow == "a spell or ability causes a player to shuffle their library"
        || blow == "a player shuffles their library"
    {
        return Ok(vec![trig_kw(
            PlayerRefAst::AnyPlayer,
            if blow.starts_with("a spell or ability") {
                "spell_or_ability_shuffle_library".to_string()
            } else {
                "shuffle_library".to_string()
            },
        )]);
    }
    if blow == "one or more of your opponents are dealt combat damage"
        || blow == "your opponents are dealt combat damage"
    {
        return Ok(vec![trig_dmg(
            true,
            false,
            None,
            Some(SubjectRefAst::Player(PlayerRefAst::EachOpponent)),
            true,
            false,
        )]);
    }
    if let Some(after) = strip_prefix_any(
        &blow,
        &[
            "you attack a player or planeswalker with ",
            "you attack with ",
        ],
    ) {
        let (flt_raw, grouped) = peel_one_or_more(after);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(vec![trig_attacks_ext(
                Some(grp(flt)),
                Some(PlayerRefAst::You),
                None,
                grouped,
            )]);
        }
    }
    if let Some(subj_raw) = blow
        .strip_prefix("you gain control of ")
        .and_then(|r| r.strip_suffix(" from another player"))
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![trig_kw(
            PlayerRefAst::You,
            format!("gain_control:{subj_raw}"),
        )]);
    }

    // State trigger: "When ~ has no <kind> counters on it" / "When there are <N> or more <kind> counters on <subj>" (Nine Lives, Deadly Designs, Afiya Grove)
    if ((blow.contains(" has no ") && blow.ends_with(" on it"))
        || (blow.starts_with("there are ") && blow.contains(" counter")))
        && let Ok(cond) = parse_condition(blow, vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }
    // "<subj> attacks a player who <cond>" / "<subj> attacks the player with the most life [or tied for most life]"
    if let Some((subj_raw, rest)) = split_once_any(
        &blow,
        &[" attacks a player who ", " attacks the player with "],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![trig_attacks(Some(subj), None, Some(rest.to_string()))]);
    }

    // Peel trailing temporal/frequency qualifier and preserve it via `QualifiedTrigger`
    for (suf, first_time, timing_tag) in [
        (" for the first time each turn", true, None),
        (" during your turn", false, Some("during_your_turn")),
        (" during their turn", false, Some("during_their_turn")),
        (" during combat", false, Some("during_combat")),
        (" this turn", false, Some("this_turn")),
        (" this way", false, Some("this_way")),
    ] {
        if let Some(stripped) = blow.strip_suffix(suf)
            && let Ok(evs) = parse_trigger_events(&format!("whenever {stripped}"), vocab)
        {
            return Ok(evs
                .into_iter()
                .map(|ev| TriggerEventAst::QualifiedTrigger {
                    inner: Box::new(ev),
                    first_time_each_turn: first_time,
                    timing: timing_tag.map(str::to_string),
                })
                .collect());
        }
    }

    // Pass Q orthogonal trigger events:
    // Evolves, plotted, saddled, play a card/land, pay life, give a gift, clash and win,
    // attacks a battle, cast your commander, enlists, collect evidence, dealt combat damage,
    // visit an Attraction, sacrifice one or more <flt>, two or more <flt> attack, returned to hand
    if blow == "~ evolves" {
        return Ok(vec![trig_kw(PlayerRefAst::You, "evolve")]);
    }
    if blow == "~ becomes plotted" || blow == "~ becomes saddled" {
        return Ok(vec![trig_kw(PlayerRefAst::You, blow.to_string())]);
    }
    if matches!(
        blow,
        "you play a card"
            | "you pay life"
            | "you give a gift"
            | "you clash and win"
            | "you collect evidence"
            | "you visit an attraction"
    ) {
        return Ok(vec![trig_kw(PlayerRefAst::You, blow.to_string())]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you play ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        if flt_raw.trim() == "a land" || flt_raw.trim() == "land" {
            return Ok(vec![TriggerEventAst::LandPlayed {
                player: PlayerRefAst::You,
            }]);
        }
        return Ok(vec![TriggerEventAst::CardPlayed {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        }]);
    }
    if blow == "you cast your commander" {
        let cmdr_flt = crate::ast::ObjectFilterAst {
            any_spell: true,
            flags: crate::ast::ObjectFilterFlags::IS_COMMANDER,
            ..Default::default()
        };
        return Ok(vec![trig_cast(PlayerRefAst::You, Some(cmdr_flt))]);
    }
    if blow == "you're dealt combat damage" || blow == "you are dealt combat damage" {
        return Ok(vec![trig_dmg(
            true,
            false,
            None,
            Some(SubjectRefAst::Player(PlayerRefAst::You)),
            false,
            false,
        )]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" attacks a battle")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![trig_attacks(
            Some(subj),
            None,
            Some("battle".to_string()),
        )]);
    }
    if blow.starts_with("~ enlists ") {
        return Ok(vec![trig_kw(PlayerRefAst::You, "enlist")]);
    }
    if let Some(after) = blow.strip_prefix("you sacrifice one or more ")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Ok(vec![TriggerEventAst::PermanentSacrificed {
            player: PlayerRefAst::You,
            subject: Box::new(subj),
        }]);
    }
    if let Some(after) = blow.strip_prefix("two or more ")
        && let Some(flt_raw) = after.strip_suffix(" attack")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![trig_attacks_ext(
            Some(grp(flt)),
            None,
            Some("two_or_more".to_string()),
            true,
        )]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" is returned to a player's hand")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![trig_zone(Some("battlefield"), Some("hand"), subj)]);
    }
    if matches!(
        blow,
        "~ and another creature attack different players"
            | "~ or a creature it's paired with is dealt damage"
            | "you cast an instant spell during your main phase"
            | "you cast a sorcery spell during your main phase"
    ) || (blow.starts_with("a creature has ") && blow.ends_with(" counters on it"))
        || (blow.starts_with("you cast your ") && blow.ends_with(" spell each turn"))
    {
        return Ok(vec![trig_kw(PlayerRefAst::You, blow.to_string())]);
    }
    // CR 603.8 State trigger & Pass R orthogonal triggers
    if matches!(
        blow,
        "~'s echo cost is paid"
            | "you pay ~'s cumulative upkeep"
            | "you tap a permanent for {c}"
            | "you attack enchanted player"
            | "you turn a permanent face up"
            | "combat damage is dealt to you"
            | "you activate a loyalty ability"
            | "you activate a ninjutsu ability"
            | "you activate ~'s outlast ability"
            | "one or more opponents lose life"
            | "enchanted opponent draws a card"
            | "a spell you've cast is countered"
            | "you roll your third die each turn"
            | "the fourth spell of a turn is cast"
            | "you cast your third spell in a turn"
            | "a player plays a card exiled with ~"
            | "you put an art sticker on a creature"
            | "you put a name sticker on a creature"
            | "~ transforms into ~"
            | "~ transforms into ~ or dies"
            | "~ phases out"
            | "~ phases out or leaves the battlefield"
            | "you gain or lose life"
            | "equipped creature mentors a creature"
            | "equipped creature attacks the monarch"
            | "you attack the player who has the initiative"
            | "one or more of your opponents are attacked"
            | "you roll a die's highest natural result"
            | "~ becomes tapped to pay a teamwork cost"
            | "you put an ability sticker on a creature"
            | "you choose a creature as your ring-bearer"
            | "you cast a spell with one or more targets"
            | "you activate an eternalize or embalm ability"
            | "you sacrifice ~ while casting a spell with emerge"
            | "the first noncreature spell of a turn is cast"
            | "one or more players discard one or more cards"
            | "one or more opponents each lose exactly 1 life"
            | "damage that would be dealt to you is prevented"
            | "a spell or ability you control counters a spell"
            | "~ enters from your hand"
            | "a player other than ~'s owner controls it"
            | "a player doesn't pay ~'s cumulative upkeep"
            | "you gain life for the first time during each of your turns"
            | "an opponent loses life for the first time during each of their turns"
            | "the final chapter ability of a saga you control resolves"
            | "you solve a case"
            | "you place a sticker"
            | "you put a sticker on ~"
            | "you roll to visit your attractions"
            | "you activate a power-up ability"
            | "an instant or sorcery spell is cast during your turn"
            | "an opponent plays a nonbasic land"
            | "a permanent owned by another player is put into a graveyard from the battlefield"
            | "~ or another creature you control leaves the battlefield without dying"
            | "combat damage is dealt to you or a planeswalker you control"
            | "another player casts a spell from anywhere other than their hand"
            | "an opponent sacrifices a nontoken permanent or discards a permanent card"
            | "an opponent puts one or more counters on a creature they control"
            | "you cycle ~ or cycle another card"
            | "one or more creatures your opponents control are dealt excess noncombat damage"
            | "one or more creatures an opponent controls attack you and aren't blocked"
            | "a creature attacks and isn't blocked this combat"
            | "~ enters attached to a creature"
            | "you and/or at least one permanent you control becomes the target of a spell or ability an opponent controls"
            | "a spell or ability an opponent controls causes you to discard cards this turn"
            | "a spell or ability causes its controller to shuffle their library"
            | "a player chooses one or more targets"
            | "~ attacks while you have the most life or are tied for most life"
            | "a vehicle crewed by ~ this turn attacks"
            | "you cast an instant or sorcery spell that targets only ~ or activate an ability that targets only ~"
            | "~ is exiled from the battlefield while you're activating a craft ability"
            | "a land you control enters from anywhere other than your hand or you cast a spell from anywhere other than your hand"
            | "enchanted player casts a spell other than the first spell they cast each turn or copies a spell"
            | "an opponent casts an instant spell other than the first instant spell that player casts each turn"
            | "you activate an ability"
            | "you cast an instant or sorcery spell or activate an ability"
            | "you cast your first spell during each of your turns"
            | "you tap a land for {c}"
            | "equipped creature and at least one other creature attack"
            | "you attack a player with one or more equipped creatures"
            | "a creature you own but don't control attacks"
            | "damage is dealt to one of the chosen players"
            | "you tap one or more untapped creatures your opponents control"
            | "a creature you control exploits a nontoken creature"
            | "you cast a spell that's exactly two colors"
            | "your commander is put into the command zone from anywhere"
            | "a creature dealt damage by equipped creature this turn dies"
            | "~ or another creature is returned to your hand from the battlefield"
            | "one or more noncreature permanents are returned to hand"
            | "a source you control deals 5 or more damage to a player"
            | "another source you control deals exactly 1 damage to a permanent or player"
            | "a spell or ability an opponent controls destroys a noncreature permanent you control"
            | "a spell or ability an opponent controls causes a land to be put into your graveyard from the battlefield"
            | "you cast a spell during your turn other than your first spell that turn"
            | "one or more creatures you control fight or become blocked"
            | "another player attacks with two or more creatures"
            | "three or more creatures you control with flying attack"
            | "you cast a spell from your graveyard or activate an ability of a card in your graveyard"
            | "you remove a time counter from ~"
            | "another creature you control leaves the battlefield without dying"
            | "you waterbend"
            | "you earthbend"
            | "you firebend"
            | "you airbend"
            | "a creature you control that was turned face up this turn deals combat damage to a player"
            | "~ enters from anywhere other than a graveyard or exile"
            | "an opponent casts a spell or activates an ability"
            | "~ becomes untapped during your untap step"
            | "the final chapter ability of a saga you control triggers"
            | "one or more other creatures you control leave the battlefield without dying"
            | "~ attacks a player and isn't blocked"
            | "one or more lands enter under an opponent's control without being played"
            | "a creature you control attacks or enters attacking"
            | "one or more other permanents phase out"
            | "a spell or ability an opponent controls causes you to discard a card"
            | "you open an attraction"
            | "you claim the prize of an attraction"
            | "a permanent entering causes a triggered ability to trigger"
            | "a creature entering under an opponent's control causes a triggered ability of that creature to trigger"
            | "a creature you control attacking causes a triggered ability of that creature to trigger"
            | "a creature you control exploits a non-human creature"
            | "an artifact becomes tapped or a player activates an artifact's ability without {t} in its activation cost"
            | "enchanted artifact becomes tapped or a player activates an ability of enchanted artifact without {t} in its activation cost"
            | "an artifact an opponent controls becomes tapped or an opponent activates an artifact's ability without {t} in its activation cost"
            | "an opponent is dealt damage by a red instant or sorcery spell you control or by a red planeswalker you control"
            | "an opponent casts a creature or planeswalker spell with the same name as a card in their graveyard"
            | "you cast an instant or sorcery spell that has the same name as a card in your graveyard"
            | "an opponent casts a vampire spell with the same name as a card exiled with ~"
            | "an instant or sorcery spell you control that targets only a single creature deals damage to that creature"
            | "a player casts a spell that shares a color or mana value with the exiled card"
            | "an opponent casts a spell with mana value, power, or toughness equal to the chosen number"
            | "you cast a spell with power, toughness, or mana value 4"
            | "a permanent other than a basic land enters"
            | "a player casts a spell they don't own"
            | "you cast an instant or sorcery spell with a single target"
            | "one or more opponents are dealt noncombat damage"
            | "you attack a player with one or more creatures with power 4 or greater"
            | "you put one or more +1/+1 counters on one or more other heroes you control"
            | "you cast a creature spell that doesn't share a creature type with a creature you control or a creature card in your graveyard"
            | "an opponent gains control of a permanent from you"
            | "one or more creatures you control leave the battlefield without dying"
            | "one or more players sacrifice one or more creatures"
            | "an opponent casts a spell from anywhere other than their hand"
            | "you play a card with the same name as one of the exiled cards"
            | "a white instant or sorcery spell causes you to gain life"
    ) || (blow.starts_with("the ") && blow.contains(" counter is put on "))
        || blow.starts_with("a permanent you control transforms into ")
        || blow.starts_with("a creature you control explores ")
        || (blow.starts_with("you play a ") && blow.contains(" or cast a "))
        || (blow.starts_with("an opponent is dealt ")
            && blow.ends_with(" or more damage by a single source"))
    {
        return Ok(vec![trig_kw(infer_trigger_actor(blow), blow.to_string())]);
    }
    if blow.ends_with(" is championed with ~")
        || blow.starts_with("~ deals ")
            && (blow.ends_with(" or more damage")
                || blow.ends_with(" or more damage to an opponent"))
        || blow.starts_with("~ is dealt ") && blow.ends_with(" or more damage")
        || blow.starts_with("you copy ")
        || blow.ends_with(" loses the game")
        || (blow.starts_with("~ and at least ") && blow.ends_with(" attack"))
    {
        return Ok(vec![trig_kw(infer_trigger_actor(blow), blow.to_string())]);
    }
    // CR 603.8 State trigger: "When <condition>" (Floodgate, Opal Avenger, Phyrexian Devourer, Rune-Tail, Goblins of the Flarg)
    if low.starts_with("when ")
        && let Ok(cond) = parse_condition(blow, vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }

    // Generic N-way comma+OR shared-subject compound event combinator:
    // "<subj> <verb1>, <verb2>, or <verb3>"
    if blow.contains(", or ") {
        const TRIGGER_VERBS: &[&str] = &[
            " enters the battlefield",
            " enter the battlefield",
            " enters tapped",
            " enter tapped",
            " enters",
            " enter",
            " leaves the battlefield",
            " leave the battlefield",
            " dies",
            " die",
            " attacks",
            " attack",
            " blocks",
            " block",
            " becomes blocked",
            " become blocked",
            " becomes tapped",
            " become tapped",
            " becomes untapped",
            " become untapped",
            " becomes the target of ",
            " become the target of ",
            " becomes attached",
            " becomes enchanted",
            " becomes equipped",
            " is turned face up",
            " are turned face up",
            " transforms into ~",
            " transforms",
            " transform",
            " is exiled from the battlefield",
            " are exiled from the battlefield",
            " is exiled",
            " are exiled",
            " is put into ",
            " are put into ",
            " deals combat damage",
            " deal combat damage",
            " deals noncombat damage",
            " deal noncombat damage",
            " deals damage",
            " deal damage",
            " is dealt damage",
            " are dealt damage",
            " phases out",
            " phase out",
            " fights",
            " fight",
            " explores",
            " explore",
            " connives",
            " connive",
            " mutates",
            " mutate",
            " waterbends",
            " waterbend",
            " earthbends",
            " earthbend",
            " firebends",
            " firebend",
            " airbends",
            " airbend",
            " scries",
            " scry",
            " surveils",
            " surveil",
            " searches",
            " search",
        ];
        let mut earliest: Option<usize> = None;
        for v in TRIGGER_VERBS {
            if let Some(pos) = blow.find(v) {
                earliest = Some(match earliest {
                    Some(cur) => cur.min(pos),
                    None => pos,
                });
            }
        }
        if let Some(v_idx) = earliest {
            let subj_part = blow[..v_idx].trim();
            let verbs_part = blow[v_idx..].trim();
            if !subj_part.is_empty() && verbs_part.contains(", or ") {
                let parts: Vec<&str> = verbs_part.split(", ").collect();
                if parts.len() >= 2
                    && parts.len() <= 4
                    && parts.last().is_some_and(|p| p.trim().starts_with("or "))
                {
                    let mut combined = Vec::new();
                    let mut all_ok = true;
                    for (i, part) in parts.iter().enumerate() {
                        let v_clean = if i + 1 == parts.len() {
                            part.trim().strip_prefix("or ").unwrap_or(part.trim())
                        } else {
                            part.trim()
                        };
                        if v_clean.is_empty() {
                            all_ok = false;
                            break;
                        }
                        match parse_trigger_events(
                            &format!("whenever {subj_part} {v_clean}"),
                            vocab,
                        ) {
                            Ok(evs) => combined.extend(evs),
                            Err(_) => {
                                all_ok = false;
                                break;
                            }
                        }
                    }
                    if all_ok && combined.len() >= 2 {
                        return Ok(combined);
                    }
                }
            }
        }
    }

    // Generic 2-way compound event combinator: "<subj> <verb1> or <verb2>"
    for verb1 in [
        " enters the battlefield",
        " enter the battlefield",
        " enters tapped",
        " enter tapped",
        " enters",
        " enter",
        " attacks alone",
        " attacks a player",
        " attack a player",
        " attacks",
        " attack",
        " blocks",
        " block",
        " becomes tapped",
        " become tapped",
        " becomes untapped",
        " become untapped",
        " becomes blocked",
        " become blocked",
        " becomes attached",
        " becomes enchanted",
        " becomes equipped",
        " is turned face up",
        " are turned face up",
        " transforms into ~",
        " transforms",
        " transform",
        " dies",
        " die",
        " leaves the battlefield",
        " leave the battlefield",
        " is exiled from the battlefield",
        " are exiled from the battlefield",
        " is exiled",
        " are exiled",
        " is put into exile",
        " are put into exile",
        " is put into a graveyard",
        " is put into your graveyard",
        " deals combat damage to a player",
        " deal combat damage to a player",
        " deals combat damage",
        " deal combat damage",
        " is dealt damage",
        " are dealt damage",
        " phases out",
        " phase out",
        " fights",
        " fight",
        " explores",
        " explore",
        " connives",
        " connive",
        " mutates",
        " mutate",
    ] {
        let needle = format!("{verb1} or ");
        if let Some((subj_part, verb2)) = blow.split_once(&needle)
            && !subj_part.trim().is_empty()
            && !verb2.trim().is_empty()
            && let (Ok(mut e1), Ok(e2)) = (
                parse_trigger_events(&format!("whenever {subj_part}{verb1}"), vocab),
                parse_trigger_events(&format!("whenever {subj_part} {verb2}"), vocab),
            )
        {
            e1.extend(e2);
            return Ok(e1);
        }
    }

    // Compound trigger across different subjects: "<event1> or <event2>"
    // (e.g. "Whenever you cast a blue spell or an Island you control enters")
    if blow.contains(" or ")
        && let Some((left_ev, right_ev)) = blow.split_once(" or ")
        && left_ev.trim().contains(' ')
        && right_ev.trim().contains(' ')
        && let (Ok(mut e1), Ok(e2)) = (
            parse_trigger_events(&format!("whenever {}", left_ev.trim()), vocab),
            parse_trigger_events(&format!("whenever {}", right_ev.trim()), vocab),
        )
    {
        e1.extend(e2);
        return Ok(e1);
    }

    // Tabletop Honor-System trigger claim (Gotcha / voice / social Un-card triggers)
    if is_physical_tabletop_clause(&low) {
        return Ok(vec![TriggerEventAst::ManualTabletopClaim {
            prompt: c.to_string(),
            any_player_can_trigger: true,
        }]);
    }

    Err(format!("unparsed_trigger:{c}"))
}

pub(crate) fn peel_one_or_more(s: &str) -> (&str, bool) {
    match s.strip_prefix("one or more ") {
        Some(rest) => (rest.trim(), true),
        None => (s.trim(), false),
    }
}

// ---------------------------------------------------------------------------
// 3. EFFECT PARSER
// ---------------------------------------------------------------------------

/// Pass E12: Replace the primary subject of a parsed trigger event with `SelfObject` (for `"~ or <other>"`).
fn with_trigger_subject_self(ev: TriggerEventAst) -> TriggerEventAst {
    match ev {
        TriggerEventAst::ZoneChange {
            from,
            to,
            grouped,
            entering_controller,
            ..
        } => TriggerEventAst::ZoneChange {
            from,
            to,
            subject: self_obj(),
            grouped,
            entering_controller,
        },
        TriggerEventAst::DamageDealt {
            combat_only,
            noncombat_only,
            recipient,
            grouped,
            excess_only,
            ..
        } => TriggerEventAst::DamageDealt {
            combat_only,
            noncombat_only,
            source: Some(self_obj()),
            recipient,
            grouped,
            excess_only,
        },
        TriggerEventAst::Attacks {
            actor,
            grouped,
            qualifier,
            ..
        } => TriggerEventAst::Attacks {
            subject: Some(self_obj()),
            actor,
            grouped,
            qualifier,
        },
        TriggerEventAst::Blocks { attacker, .. } => TriggerEventAst::Blocks {
            subject: self_obj(),
            attacker,
        },
        TriggerEventAst::BecomesBlocked { blocker, .. } => TriggerEventAst::BecomesBlocked {
            subject: self_obj(),
            blocker,
        },
        TriggerEventAst::BecomesBlockedByCount { min_blockers, .. } => {
            TriggerEventAst::BecomesBlockedByCount {
                subject: self_obj(),
                min_blockers,
            }
        }
        TriggerEventAst::BecomesTargeted {
            by_opponent,
            by_text,
            ..
        } => TriggerEventAst::BecomesTargeted {
            subject: self_obj(),
            by_opponent,
            by_text,
        },
        TriggerEventAst::TapStateChanged {
            tapped, for_mana, ..
        } => TriggerEventAst::TapStateChanged {
            subject: self_obj(),
            tapped,
            for_mana,
        },
        TriggerEventAst::CountersChanged {
            counter_kind,
            added,
            by_you,
            ..
        } => TriggerEventAst::CountersChanged {
            subject: self_obj(),
            counter_kind,
            added,
            by_you,
        },
        TriggerEventAst::QualifiedTrigger {
            inner,
            first_time_each_turn,
            timing,
        } => TriggerEventAst::QualifiedTrigger {
            inner: Box::new(with_trigger_subject_self(*inner)),
            first_time_each_turn,
            timing,
        },
        other => other,
    }
}

#[inline]
fn trig_zone(from: Option<&str>, to: Option<&str>, subject: SubjectRefAst) -> TriggerEventAst {
    TriggerEventAst::ZoneChange {
        from: from.map(str::to_string),
        to: to.map(str::to_string),
        subject: Box::new(subject),
        grouped: false,
        entering_controller: None,
    }
}

#[inline]
fn trig_cast(caster: PlayerRefAst, spell: Option<ObjectFilterAst>) -> TriggerEventAst {
    TriggerEventAst::SpellCast {
        caster,
        spell: spell.map(Box::new),
        self_spell: false,
        targets: None,
        nth_each_turn: None,
        include_copy: false,
    }
}

#[inline]
fn trig_kw(player: PlayerRefAst, action: impl Into<String>) -> TriggerEventAst {
    TriggerEventAst::KeywordActionPerformed {
        player,
        action: action.into(),
    }
}

#[inline]
fn trig_attacks(
    subject: Option<SubjectRefAst>,
    actor: Option<PlayerRefAst>,
    qualifier: Option<String>,
) -> TriggerEventAst {
    TriggerEventAst::Attacks {
        subject: subject.map(Box::new),
        actor,
        qualifier,
        grouped: false,
    }
}

#[inline]
fn trig_step(step: TurnStepAst, whose: impl Into<String>) -> TriggerEventAst {
    TriggerEventAst::StepBegan {
        step,
        whose: whose.into(),
    }
}

#[inline]
fn trig_zone_ext(
    from: Option<&str>,
    to: Option<&str>,
    subject: SubjectRefAst,
    grouped: bool,
    entering_controller: Option<ControllerRelation>,
) -> TriggerEventAst {
    TriggerEventAst::ZoneChange {
        from: from.map(str::to_string),
        to: to.map(str::to_string),
        subject: Box::new(subject),
        grouped,
        entering_controller,
    }
}

#[inline]
fn trig_cast_ext(
    caster: PlayerRefAst,
    spell: Option<ObjectFilterAst>,
    self_spell: bool,
    targets: Option<SubjectRefAst>,
    nth_each_turn: Option<u8>,
    include_copy: bool,
) -> TriggerEventAst {
    TriggerEventAst::SpellCast {
        caster,
        spell: spell.map(Box::new),
        self_spell,
        targets: targets.map(Box::new),
        nth_each_turn,
        include_copy,
    }
}

#[inline]
fn trig_attacks_ext(
    subject: Option<SubjectRefAst>,
    actor: Option<PlayerRefAst>,
    qualifier: Option<String>,
    grouped: bool,
) -> TriggerEventAst {
    TriggerEventAst::Attacks {
        subject: subject.map(Box::new),
        actor,
        qualifier,
        grouped,
    }
}

#[inline]
fn trig_dmg(
    combat_only: bool,
    noncombat_only: bool,
    source: Option<SubjectRefAst>,
    recipient: Option<SubjectRefAst>,
    grouped: bool,
    excess_only: bool,
) -> TriggerEventAst {
    TriggerEventAst::DamageDealt {
        combat_only,
        noncombat_only,
        source: source.map(Box::new),
        recipient: recipient.map(Box::new),
        grouped,
        excess_only,
    }
}
