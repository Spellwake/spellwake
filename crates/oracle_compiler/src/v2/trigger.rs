//! Strictly-typed [`TriggerSpec`] and [`TriggerEvent`] parser for `v2`.

use crate::ast::TurnStepAst;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_subject_ref};
use crate::v2::ir::{StepOwner, SubjectRef, TriggerEvent, TriggerSpec, ZoneKind};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

/// Parse a `When ...` / `Whenever ...` / `At the beginning of ...` trigger header (up to the comma
/// before the effect body, including compound triggers and optional `, if <cond>` intervening-if).
pub fn parse_trigger_header(ts: &mut TokenStream<'_>) -> PResult<TriggerSpec> {
    if ts.eat_phrase(&["at", "the", "beginning", "of"]) {
        let (step, owner) = parse_step_with_owner(ts)?;
        let intervening_if = parse_optional_intervening_if(ts);
        return Ok(TriggerSpec {
            events: vec![TriggerEvent::StepBegins { step, owner }],
            intervening_if,
        });
    }

    if !(ts.eat_word("when") || ts.eat_word("whenever")) {
        return Err(ts.err("expected `when`, `whenever`, or `at the beginning of`"));
    }

    let mut events = parse_trigger_event_list(ts)?;

    // Optional chained `and whenever <event>` / `or whenever <event>`
    loop {
        let cp = ts.checkpoint();
        if (ts.eat_word("and") || ts.eat_word("or"))
            && (ts.eat_word("whenever") || ts.eat_word("when"))
            && let Ok(more) = try_parse(ts, parse_trigger_event_list)
        {
            events.extend(more);
            continue;
        }
        ts.restore(cp);
        break;
    }

    let intervening_if = parse_optional_intervening_if(ts);
    Ok(TriggerSpec {
        events,
        intervening_if,
    })
}

fn parse_optional_intervening_if(ts: &mut TokenStream<'_>) -> Option<crate::v2::ir::ConditionExpr> {
    let cp = ts.checkpoint();
    if ts.eat_comma()
        && ts.eat_word("if")
        && let Ok(c) = parse_condition(ts)
    {
        return Some(c);
    }
    ts.restore(cp);
    None
}

/// Parse `<owner> <step>` (e.g. `your upkeep`, `combat on your turn`, `each upkeep`, `each end step`, `the end step`).
pub fn parse_step_with_owner(ts: &mut TokenStream<'_>) -> PResult<(TurnStepAst, StepOwner)> {
    // `combat on your turn` / `combat on each player's turn` / `combat on each opponent's turn`
    if ts.eat_phrase(&["combat", "on"]) {
        let owner = if ts.eat_phrase(&["your", "turn"]) {
            StepOwner::Your
        } else if ts.eat_phrase(&["each", "player's", "turn"]) {
            StepOwner::EachPlayer
        } else if ts.eat_phrase(&["each", "opponent's", "turn"]) {
            StepOwner::EachOpponent
        } else if ts.eat_phrase(&["that", "player's", "turn"]) {
            StepOwner::ThatPlayer
        } else {
            return Err(ts.err("expected turn owner after `combat on`"));
        };
        return Ok((TurnStepAst::BeginningOfCombat, owner));
    }

    // `the upkeep of <player>` / `the end step` / `each upkeep` / `each end step` / `each combat`
    if ts.eat_phrase(&["each", "upkeep"]) {
        return Ok((TurnStepAst::Upkeep, StepOwner::Each));
    }
    if ts.eat_phrase(&["each", "end", "step"]) {
        return Ok((TurnStepAst::EndStep, StepOwner::Each));
    }
    if ts.eat_phrase(&["each", "combat"]) {
        return Ok((TurnStepAst::BeginningOfCombat, StepOwner::Each));
    }
    if ts.eat_phrase(&["the", "end", "step"]) {
        return Ok((TurnStepAst::EndStep, StepOwner::The));
    }
    if ts.eat_phrase(&["the", "upkeep"]) {
        let _ = ts.eat_phrase(&["of", "enchanted", "player"])
            || ts.eat_phrase(&["of", "each", "opponent"])
            || ts.eat_phrase(&["of", "each", "player"]);
        return Ok((TurnStepAst::Upkeep, StepOwner::The));
    }

    let owner = if ts.eat_phrase(&["your", "next"]) {
        StepOwner::YourNext
    } else if ts.eat_phrase(&["the", "next"]) {
        StepOwner::Next
    } else if ts.eat_word("your") {
        StepOwner::Your
    } else if ts.eat_phrase(&["each", "player's"]) {
        StepOwner::EachPlayer
    } else if ts.eat_phrase(&["each", "opponent's"]) {
        StepOwner::EachOpponent
    } else if ts.eat_phrase(&["that", "player's"]) {
        StepOwner::ThatPlayer
    } else if ts.eat_phrase(&["defending", "player's"]) {
        StepOwner::DefendingPlayer
    } else {
        return Err(ts.err("expected step owner (`your`, `the next`, etc.)"));
    };

    let _ = ts.eat_word("first") || ts.eat_word("precombat") || ts.eat_word("postcombat");

    let step = if ts.eat_word("upkeep") {
        TurnStepAst::Upkeep
    } else if ts.eat_phrase(&["end", "step"]) {
        TurnStepAst::EndStep
    } else if ts.eat_phrase(&["draw", "step"]) {
        TurnStepAst::DrawStep
    } else if ts.eat_phrase(&["untap", "step"]) {
        TurnStepAst::UntapStep
    } else if ts.eat_phrase(&["cleanup", "step"]) {
        TurnStepAst::CleanupStep
    } else if ts.eat_phrase(&["main", "phase"]) {
        TurnStepAst::MainPhase
    } else if ts.eat_word("combat") {
        TurnStepAst::BeginningOfCombat
    } else {
        return Err(ts.err("expected turn step name"));
    };
    Ok((step, owner))
}

/// Parse one or more trigger events sharing a subject (`When ~ enters or attacks`, `Whenever ~ blocks or becomes blocked`).
pub fn parse_trigger_event_list(ts: &mut TokenStream<'_>) -> PResult<Vec<TriggerEvent>> {
    // Player-initiated events (`<player> casts ...`, `<player> sacrifices ...`, `<player> draws ...`, `you cycle ~`, etc.)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("cast") || ts.eat_word("casts") {
                let _ = ts.eat_phrase(&["your", "first"])
                    || ts.eat_phrase(&["their", "first"])
                    || ts.eat_phrase(&["your", "second"])
                    || ts.eat_phrase(&["their", "second"]);
                let spell = opt(ts, parse_object_filter);
                let from_outside_hand = ts
                    .eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"])
                    || ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"]);
                let _ = ts.eat_phrase(&["each", "turn"]);
                return Ok(vec![TriggerEvent::SpellCast {
                    caster: player,
                    spell,
                    from_outside_hand,
                }]);
            }
            if ts.eat_word("sacrifice") || ts.eat_word("sacrifices") {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                let subject = parse_subject_ref(ts)?;
                return Ok(vec![TriggerEvent::PermanentSacrificed { player, subject }]);
            }
            if ts.eat_word("cycle") || ts.eat_word("cycles") {
                if ts.eat_tilde() {
                    return Ok(vec![TriggerEvent::Cycled {
                        player,
                        self_card: true,
                    }]);
                }
                if ts.eat_phrase(&["a", "card"]) || ts.eat_phrase(&["another", "card"]) {
                    return Ok(vec![TriggerEvent::Cycled {
                        player,
                        self_card: false,
                    }]);
                }
            }
            if ts.eat_word("draw") || ts.eat_word("draws") {
                if (ts.eat_phrase(&["your", "second", "card"])
                    || ts.eat_phrase(&["their", "second", "card"]))
                    && ts.eat_phrase(&["each", "turn"])
                {
                    return Ok(vec![TriggerEvent::NthCardDrawn { player, nth: 2 }]);
                }
                if ts.eat_phrase(&["one", "or", "more", "cards"])
                    || ts.eat_phrase(&["a", "card"])
                    || ts.eat_word("cards")
                {
                    return Ok(vec![TriggerEvent::CardDrawn { player }]);
                }
            }
            if (ts.eat_word("discard") || ts.eat_word("discards"))
                && (ts.eat_phrase(&["one", "or", "more", "cards"])
                    || ts.eat_phrase(&["a", "card"])
                    || ts.eat_word("cards"))
            {
                return Ok(vec![TriggerEvent::CardDiscarded { player }]);
            }
            if (ts.eat_word("gain") || ts.eat_word("gains")) && ts.eat_word("life") {
                return Ok(vec![TriggerEvent::LifeChanged {
                    player,
                    gained: true,
                }]);
            }
            if (ts.eat_word("lose") || ts.eat_word("loses")) && ts.eat_word("life") {
                return Ok(vec![TriggerEvent::LifeChanged {
                    player,
                    gained: false,
                }]);
            }
            if (ts.eat_word("play") || ts.eat_word("plays")) && ts.eat_phrase(&["a", "land"]) {
                return Ok(vec![TriggerEvent::LandPlayed { player }]);
            }
        }
        ts.restore(cp);
    }

    // Subject-initiated events: support `one or more <flt>` and `~ or another <flt>` compound subjects!
    let _ = ts.eat_phrase(&["one", "or", "more"]);
    let first_subj = parse_subject_ref(ts)?;
    let subject = {
        let cp = ts.checkpoint();
        if ts.eat_word("or")
            && let Ok(other_subj) = try_parse(ts, parse_subject_ref)
        {
            SubjectRef::Compound(vec![first_subj, other_subj])
        } else {
            ts.restore(cp);
            first_subj
        }
    };

    let first_ev = parse_subject_trigger_verb(ts, subject.clone())?;
    let mut events = vec![first_ev];

    // Optional compound trigger verbs sharing the same subject (`enters or attacks`, `attacks or blocks`, `enters or dies`, `blocks or becomes blocked`)
    loop {
        let cp = ts.checkpoint();
        if ts.eat_word("or")
            && let Ok(next_ev) = try_parse(ts, |s| parse_subject_trigger_verb(s, subject.clone()))
        {
            events.push(next_ev);
            continue;
        }
        ts.restore(cp);
        break;
    }

    Ok(events)
}

fn parse_subject_trigger_verb(
    ts: &mut TokenStream<'_>,
    subject: SubjectRef,
) -> PResult<TriggerEvent> {
    if ts.eat_phrase(&["enters", "the", "battlefield"])
        || ts.eat_phrase(&["enter", "the", "battlefield"])
        || ts.eat_word("enters")
        || ts.eat_word("enter")
    {
        let _ = ts.eat_phrase(&["under", "your", "control"]);
        return Ok(TriggerEvent::ZoneChange {
            from: None,
            to: Some(ZoneKind::Battlefield),
            subject,
        });
    }
    if ts.eat_phrase(&["leaves", "the", "battlefield"])
        || ts.eat_phrase(&["leave", "the", "battlefield"])
    {
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Battlefield),
            to: None,
            subject,
        });
    }
    if ts.eat_phrase(&["leaves", "your", "graveyard"])
        || ts.eat_phrase(&["leave", "your", "graveyard"])
        || ts.eat_phrase(&["leaves", "a", "graveyard"])
        || ts.eat_phrase(&["leave", "a", "graveyard"])
    {
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Graveyard),
            to: None,
            subject,
        });
    }
    if ts.eat_phrase(&["transforms", "into"]) && ts.eat_tilde() {
        return Ok(TriggerEvent::Transforms { subject });
    }
    if ts.eat_phrase(&["exploits", "a", "creature"]) {
        return Ok(TriggerEvent::Exploits { subject });
    }
    if ts.eat_phrase(&["is", "tapped", "for", "mana"]) {
        return Ok(TriggerEvent::BecomesTapped { subject });
    }
    if ts.eat_word("dies") || ts.eat_word("die") {
        let _ = ts.eat_phrase(&["this", "turn"]);
        return Ok(TriggerEvent::Dies { subject });
    }
    if ts.eat_phrase(&["is", "put", "into", "a", "graveyard"])
        || ts.eat_phrase(&["are", "put", "into", "a", "graveyard"])
    {
        let from = if ts.eat_phrase(&["from", "the", "battlefield"]) {
            Some(ZoneKind::Battlefield)
        } else {
            let _ = ts.eat_phrase(&["from", "anywhere"]);
            None
        };
        return Ok(TriggerEvent::PutIntoGraveyard { subject, from });
    }
    if ts.eat_phrase(&["attacks", "and", "isn't", "blocked"])
        || ts.eat_phrase(&["attack", "and", "aren't", "blocked"])
    {
        return Ok(TriggerEvent::AttacksAndNotBlocked { subject });
    }
    if ts.eat_word("attacks") || ts.eat_word("attack") {
        let _ = ts.eat_word("alone");
        return Ok(TriggerEvent::Attacks {
            subject: Some(subject),
            player: None,
        });
    }
    if ts.eat_phrase(&["becomes", "blocked"]) || ts.eat_phrase(&["become", "blocked"]) {
        let by = if ts.eat_word("by") {
            opt(ts, parse_subject_ref).map(Box::new)
        } else {
            None
        };
        return Ok(TriggerEvent::BecomesBlocked { subject, by });
    }
    if ts.eat_word("blocks") || ts.eat_word("block") {
        let blocked = opt(ts, parse_subject_ref).map(Box::new);
        return Ok(TriggerEvent::Blocks { subject, blocked });
    }
    if ts.eat_phrase(&["is", "turned", "face", "up"])
        || ts.eat_phrase(&["are", "turned", "face", "up"])
    {
        return Ok(TriggerEvent::TurnedFaceUp { subject });
    }
    if ts.eat_phrase(&["becomes", "tapped"]) || ts.eat_phrase(&["become", "tapped"]) {
        return Ok(TriggerEvent::BecomesTapped { subject });
    }
    if ts.eat_phrase(&["becomes", "untapped"]) || ts.eat_phrase(&["become", "untapped"]) {
        return Ok(TriggerEvent::BecomesUntapped { subject });
    }
    if ts.eat_phrase(&["is", "dealt", "combat", "damage"])
        || ts.eat_phrase(&["are", "dealt", "combat", "damage"])
    {
        return Ok(TriggerEvent::DealtDamage {
            subject,
            combat_only: true,
        });
    }
    if ts.eat_phrase(&["is", "dealt", "damage"]) || ts.eat_phrase(&["are", "dealt", "damage"]) {
        return Ok(TriggerEvent::DealtDamage {
            subject,
            combat_only: false,
        });
    }
    if ts.eat_phrase(&["becomes", "the", "target", "of"])
        || ts.eat_phrase(&["become", "the", "target", "of"])
    {
        let source = parse_object_filter(ts)?;
        return Ok(TriggerEvent::BecomesTarget { subject, source });
    }
    if ts.eat_phrase(&["deals", "combat", "damage"]) || ts.eat_phrase(&["deal", "combat", "damage"])
    {
        let target = if ts.eat_word("to") {
            Some(parse_subject_ref(ts)?)
        } else {
            None
        };
        return Ok(TriggerEvent::DealsDamage {
            combat_only: true,
            source: Some(subject),
            target,
        });
    }
    if ts.eat_phrase(&["deals", "damage"]) || ts.eat_phrase(&["deal", "damage"]) {
        let target = if ts.eat_word("to") {
            Some(parse_subject_ref(ts)?)
        } else {
            None
        };
        return Ok(TriggerEvent::DealsDamage {
            combat_only: false,
            source: Some(subject),
            target,
        });
    }

    Err(ts.err("expected trigger event verb"))
}
