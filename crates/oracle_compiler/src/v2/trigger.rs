//! Strictly-typed [`TriggerSpec`] and [`TriggerEvent`] parser for `v2`.

use crate::ast::TurnStepAst;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{
    ConditionExpr, ObjectFilter, StepOwner, SubjectRef, TriggerEvent, TriggerSpec, ZoneKind,
};
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

    // Optional chained `and whenever <event>` / `or whenever <event>` / `and at the beginning of <step>`
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
        let cp2 = ts.checkpoint();
        if (ts.eat_word("and") || ts.eat_word("or"))
            && ts.eat_phrase(&["at", "the", "beginning", "of"])
            && let Ok((step, owner)) = try_parse(ts, parse_step_with_owner)
        {
            events.push(TriggerEvent::StepBegins { step, owner });
            continue;
        }
        ts.restore(cp2);
        break;
    }

    let mut intervening_if = parse_optional_intervening_if(ts);
    loop {
        if ts.eat_phrase(&["for", "the", "first", "time", "each", "turn"]) {
            intervening_if = Some(merge_cond(intervening_if, ConditionExpr::FirstTimeEachTurn));
            continue;
        }
        if ts.eat_phrase(&["while", "saddled"]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::SelfObject),
                    state: crate::v2::ir::ObjectStateKind::Saddled,
                },
            ));
            continue;
        }
        {
            let wcp = ts.checkpoint();
            if ts.eat_word("while")
                && let Ok(c) = try_parse(ts, parse_condition)
            {
                intervening_if = Some(merge_cond(intervening_if, c));
                continue;
            }
            ts.restore(wcp);
        }
        if ts.eat_phrase(&["during", "your", "turn"]) {
            intervening_if = Some(merge_cond(intervening_if, ConditionExpr::IsYourTurn));
            continue;
        }
        if ts.eat_phrase(&["during", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "each", "opponent's", "turn"])
        {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Not(Box::new(ConditionExpr::IsYourTurn)),
            ));
            continue;
        }
        if let Some(c) = parse_optional_intervening_if(ts) {
            intervening_if = Some(merge_cond(intervening_if, c));
            continue;
        }
        break;
    }
    Ok(TriggerSpec {
        events,
        intervening_if,
    })
}

fn merge_cond(prev: Option<ConditionExpr>, next: ConditionExpr) -> ConditionExpr {
    match prev {
        Some(p) => ConditionExpr::And(Box::new(p), Box::new(next)),
        None => next,
    }
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
        if ts.eat_phrase(&["of", "enchanted", "creature's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "permanent's", "controller"])
            || ts.eat_phrase(&["of", "equipped", "creature's", "controller"])
        {
            return Ok((TurnStepAst::Upkeep, StepOwner::EnchantedController));
        }
        let _ = ts.eat_phrase(&["of", "enchanted", "player"])
            || ts.eat_phrase(&["of", "each", "opponent"])
            || ts.eat_phrase(&["of", "each", "player"]);
        return Ok((TurnStepAst::Upkeep, StepOwner::The));
    }

    let owner = if ts.eat_phrase(&["your", "next"]) {
        StepOwner::YourNext
    } else if ts.eat_phrase(&["the", "next", "turn's"]) {
        StepOwner::NextTurn
    } else if ts.eat_phrase(&["the", "next"]) {
        StepOwner::Next
    } else if ts.eat_phrase(&["enchanted", "player's"])
        || ts.eat_phrase(&["enchanted", "creature's", "controller's"])
    {
        StepOwner::EnchantedController
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

    let _ = ts.eat_word("first")
        || ts.eat_word("second")
        || ts.eat_word("precombat")
        || ts.eat_word("postcombat");

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
            if ts.eat_phrase(&["cast", "or", "copy"]) || ts.eat_phrase(&["casts", "or", "copies"]) {
                let spell = opt(ts, parse_object_filter);
                return Ok(vec![
                    TriggerEvent::SpellCast {
                        caster: player,
                        spell: spell.clone(),
                        from_outside_hand: false,
                    },
                    TriggerEvent::SpellCopied {
                        caster: player,
                        spell,
                    },
                ]);
            }
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
            if (ts.eat_phrase(&["cycle", "or", "discard"])
                || ts.eat_phrase(&["cycles", "or", "discards"]))
                && (ts.eat_phrase(&["a", "card"]) || ts.eat_phrase(&["one", "or", "more", "cards"]))
            {
                return Ok(vec![
                    TriggerEvent::Cycled {
                        player,
                        self_card: false,
                    },
                    TriggerEvent::CardDiscarded { player },
                ]);
            }
            if (ts.eat_word("attack") || ts.eat_word("attacks")) && ts.eat_word("with") {
                let min_count = ts
                    .next_number(false)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_phrase(&["or", "more"]);
                if let Ok(filter) = try_parse(ts, parse_object_filter) {
                    return Ok(vec![TriggerEvent::PlayerAttacksWith {
                        player,
                        filter,
                        min_count,
                    }]);
                }
            }
            if (ts.eat_word("roll") || ts.eat_word("rolls"))
                && ts.eat_word("a")
                && let Some(v) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
            {
                return Ok(vec![TriggerEvent::RolledDie { player, value: v }]);
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
            if (ts.eat_word("tap") || ts.eat_word("taps"))
                && let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["for", "mana"])
            {
                return Ok(vec![TriggerEvent::TappedForMana { player, filter }]);
            }
            if (ts.eat_word("activate") || ts.eat_word("activates"))
                && ts.eat_phrase(&["an", "ability"])
            {
                let non_mana_only = ts.eat_phrase(&["that", "isn't", "a", "mana", "ability"]);
                return Ok(vec![TriggerEvent::AbilityActivated {
                    player,
                    non_mana_only,
                }]);
            }
            if ts.eat_word("discard") || ts.eat_word("discards") {
                if ts.eat_phrase(&["one", "or", "more", "cards"])
                    || ts.eat_phrase(&["a", "card"])
                    || ts.eat_word("cards")
                {
                    return Ok(vec![TriggerEvent::CardDiscarded { player }]);
                }
                if let Ok(filter) = try_parse(ts, parse_object_filter) {
                    return Ok(vec![TriggerEvent::CardDiscardedMatching { player, filter }]);
                }
            }
            if (ts.eat_word("gain") || ts.eat_word("gains")) && ts.eat_word("life") {
                return Ok(vec![TriggerEvent::LifeChanged {
                    player,
                    gained: true,
                }]);
            }
            if (ts.eat_phrase(&["lose", "control", "of"])
                || ts.eat_phrase(&["loses", "control", "of"]))
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(vec![TriggerEvent::LoseControl { player, subject }]);
            }
            if (ts.eat_word("expend") || ts.eat_word("expends"))
                && let Some(amt) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
            {
                return Ok(vec![TriggerEvent::Expend {
                    player,
                    amount: amt,
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
            if (ts.eat_word("commit") || ts.eat_word("commits")) && ts.eat_phrase(&["a", "crime"]) {
                return Ok(vec![TriggerEvent::CommitCrime { player }]);
            }
            if (ts.eat_word("unlock") || ts.eat_word("unlocks")) && ts.eat_phrase(&["this", "door"])
            {
                return Ok(vec![TriggerEvent::UnlockDoor {
                    player,
                    fully_unlocked: false,
                }]);
            }
            if (ts.eat_phrase(&["fully", "unlock"]) || ts.eat_phrase(&["fully", "unlocks"]))
                && ts.eat_phrase(&["a", "room"])
            {
                return Ok(vec![TriggerEvent::UnlockDoor {
                    player,
                    fully_unlocked: true,
                }]);
            }
            if (ts.eat_word("control") || ts.eat_word("controls")) && ts.eat_word("no") {
                let filter = parse_object_filter(ts)?;
                return Ok(vec![TriggerEvent::ControlNo { player, filter }]);
            }
            if (ts.eat_word("put") || ts.eat_word("puts")) && ts.eat_phrase(&["one", "or", "more"])
            {
                let kind = opt(ts, parse_counter_kind);
                if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_word("on") {
                    let subject = parse_subject_ref(ts)?;
                    return Ok(vec![TriggerEvent::CounterPlaced { subject, kind }]);
                }
            }
        }
        ts.restore(cp);
    }

    // `one or more [<kind>] counters are put on <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"]) {
            let kind = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["are", "put", "on"])
            {
                let subject = parse_subject_ref(ts)?;
                return Ok(vec![TriggerEvent::CounterPlaced { subject, kind }]);
            }
        }
        ts.restore(cp);
    }

    // `day becomes night or night becomes day`
    if ts.eat_phrase(&["day", "becomes", "night", "or", "night", "becomes", "day"]) {
        return Ok(vec![TriggerEvent::DayNightChanged]);
    }

    // `a [color] source you control deals [noncombat|combat] damage to <tgt>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("a") || ts.eat_word("an") {
            let mut src_flt = ObjectFilter::default();
            if let Some(w) = ts.peek_word()
                && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                src_flt.any_colors = src_flt.any_colors.union(c);
            }
            if ts.eat_word("source") {
                if ts.eat_phrase(&["you", "control"]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::You);
                } else if ts.eat_phrase(&["an", "opponent", "controls"]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::Opponent);
                }
                let combat_only = if ts.eat_phrase(&["deals", "combat", "damage"]) {
                    Some(true)
                } else if ts.eat_phrase(&["deals", "noncombat", "damage"])
                    || ts.eat_phrase(&["deals", "damage"])
                {
                    Some(false)
                } else {
                    None
                };
                if let Some(combat_only) = combat_only {
                    let target = if ts.eat_word("to") {
                        Some(parse_subject_ref(ts)?)
                    } else {
                        None
                    };
                    return Ok(vec![TriggerEvent::DealsDamage {
                        combat_only,
                        source: Some(SubjectRef::Group(Box::new(src_flt))),
                        target,
                    }]);
                }
            }
        }
        ts.restore(cp);
    }

    // `~ and at least <N> other <flt> attack` (Battalion / Enlist-style)
    {
        let cp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["and", "at", "least"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_word("other")
            && try_parse(ts, parse_object_filter).is_ok()
            && (ts.eat_word("attack") || ts.eat_word("attacks"))
        {
            return Ok(vec![TriggerEvent::AttacksWithOthers {
                subject,
                min_others: u8::try_from(n).unwrap_or(2),
            }]);
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
        let mut from = None;
        let _ = ts.eat_phrase(&["under", "your", "control"]);
        if ts.eat_word("from")
            && let Ok(z) = try_parse(ts, crate::v2::filter::parse_zone_ref)
        {
            from = Some(z.zone);
        }
        return Ok(TriggerEvent::ZoneChange {
            from,
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
    if ts.eat_phrase(&["becomes", "crewed"]) {
        return Ok(TriggerEvent::BecomesCrewed { subject });
    }
    if ts.eat_phrase(&["becomes", "monstrous"]) {
        return Ok(TriggerEvent::BecomesMonstrous { subject });
    }
    if ts.eat_phrase(&["becomes", "level"])
        && let Some(level) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(TriggerEvent::ClassBecomesLevel { level });
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
    if ts.eat_phrase(&["is", "put", "into"]) || ts.eat_phrase(&["are", "put", "into"]) {
        let z = crate::v2::filter::parse_zone_ref(ts)?;
        if z.zone == ZoneKind::Graveyard {
            let from = if ts.eat_phrase(&["from", "the", "battlefield"]) {
                Some(ZoneKind::Battlefield)
            } else {
                let _ = ts.eat_phrase(&["from", "anywhere"]);
                None
            };
            return Ok(TriggerEvent::PutIntoGraveyard { subject, from });
        }
        return Ok(TriggerEvent::ZoneChange {
            from: None,
            to: Some(z.zone),
            subject,
        });
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
