//! Strictly-typed [`TriggerSpec`] and [`TriggerEvent`] parser for `v2`.

use crate::ast::TurnStepAst;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_subject_ref};
use crate::v2::ir::{StepOwner, TriggerEvent, TriggerSpec, ZoneKind};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

/// Parse a `When ...` / `Whenever ...` / `At the beginning of ...` trigger header (up to the comma
/// before the effect body, including optional `, if <cond>` intervening-if).
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

    let ev = parse_trigger_event(ts)?;
    let intervening_if = parse_optional_intervening_if(ts);
    Ok(TriggerSpec {
        events: vec![ev],
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

/// Parse `<owner> <step>` (e.g. `your upkeep`, `your next upkeep`, `the next end step`, `your end step`).
pub fn parse_step_with_owner(ts: &mut TokenStream<'_>) -> PResult<(TurnStepAst, StepOwner)> {
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
    } else {
        return Err(ts.err("expected step owner (`your`, `the next`, etc.)"));
    };

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

/// Parse one strictly-typed [`TriggerEvent`] (always preserves true player actor; never defaults
/// opponent triggers to `You`).
pub fn parse_trigger_event(ts: &mut TokenStream<'_>) -> PResult<TriggerEvent> {
    // Player-initiated events (`<player> casts ...`, `<player> sacrifices ...`, `<player> draws ...`, etc.)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("cast") || ts.eat_word("casts") {
                let spell = opt(ts, parse_object_filter);
                let from_outside_hand = ts
                    .eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"])
                    || ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"]);
                return Ok(TriggerEvent::SpellCast {
                    caster: player,
                    spell,
                    from_outside_hand,
                });
            }
            if ts.eat_word("sacrifice") || ts.eat_word("sacrifices") {
                let subject = parse_subject_ref(ts)?;
                return Ok(TriggerEvent::PermanentSacrificed { player, subject });
            }
            if (ts.eat_word("draw") || ts.eat_word("draws"))
                && (ts.eat_phrase(&["a", "card"]) || ts.eat_word("cards"))
            {
                return Ok(TriggerEvent::CardDrawn { player });
            }
            if (ts.eat_word("discard") || ts.eat_word("discards"))
                && (ts.eat_phrase(&["a", "card"]) || ts.eat_word("cards"))
            {
                return Ok(TriggerEvent::CardDiscarded { player });
            }
            if (ts.eat_word("gain") || ts.eat_word("gains")) && ts.eat_word("life") {
                return Ok(TriggerEvent::LifeChanged {
                    player,
                    gained: true,
                });
            }
            if (ts.eat_word("lose") || ts.eat_word("loses")) && ts.eat_word("life") {
                return Ok(TriggerEvent::LifeChanged {
                    player,
                    gained: false,
                });
            }
            if (ts.eat_word("play") || ts.eat_word("plays")) && ts.eat_phrase(&["a", "land"]) {
                return Ok(TriggerEvent::LandPlayed { player });
            }
        }
        ts.restore(cp);
    }

    // Subject-initiated events (`~ enters`, `~ leaves the battlefield`, `that creature dies [this turn]`, `equipped creature attacks`)
    let subject = parse_subject_ref(ts)?;

    if ts.eat_phrase(&["enters", "the", "battlefield"])
        || ts.eat_phrase(&["enter", "the", "battlefield"])
        || ts.eat_word("enters")
        || ts.eat_word("enter")
    {
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
    if ts.eat_word("dies") || ts.eat_word("die") {
        let _ = ts.eat_phrase(&["this", "turn"]);
        return Ok(TriggerEvent::Dies { subject });
    }
    if ts.eat_word("attacks") || ts.eat_word("attack") {
        return Ok(TriggerEvent::Attacks {
            subject: Some(subject),
            player: None,
        });
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
