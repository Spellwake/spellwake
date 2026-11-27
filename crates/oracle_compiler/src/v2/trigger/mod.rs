//! Strictly-typed [`TriggerSpec`] and [`TriggerEvent`] parser for `v2`.

mod subject_events;

use crate::ast::TurnStepAst;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, CounterKind, CounterTriggerKind, EventCheckKind,
    FilterFlags, FilterPredicate, GlobalTriggerKind, ManaSymbol, ObjectFilter, ObjectStateKind,
    PlayerObjectTriggerKind, PlayerRef, PlayerTriggerKind, StepOwner, SubjectRef,
    SubjectTriggerKind, TriggerEvent, TriggerSpec, ZoneKind,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

/// Parse a `When ...` / `Whenever ...` / `At the beginning of ...` trigger header (up to the comma
/// before the effect body, including compound triggers and optional `, if <cond>` intervening-if).
pub fn parse_trigger_header(ts: &mut TokenStream<'_>) -> PResult<TriggerSpec> {
    let mut events = if ts.eat_phrase(&["at", "the", "beginning", "of"]) {
        let (step, owner) = parse_step_with_owner(ts)?;
        vec![TriggerEvent::StepBegins { step, owner }]
    } else if ts.eat_phrase(&["at", "the", "end", "of"]) {
        let (step, owner) = parse_step_with_owner(ts)?;
        vec![TriggerEvent::StepEnds { step, owner }]
    } else if ts.eat_phrase(&["at", "end", "of", "combat"]) {
        let owner = if ts.eat_word("on") {
            if ts.eat_phrase(&["your", "turn"]) {
                StepOwner::Your
            } else if ts.eat_phrase(&["each", "player's", "turn"]) {
                StepOwner::EachPlayer
            } else if ts.eat_phrase(&["each", "opponent's", "turn"]) {
                StepOwner::EachOpponent
            } else if ts.eat_phrase(&["that", "player's", "turn"]) {
                StepOwner::ThatPlayer
            } else if ts.eat_phrase(&["enchanted", "opponent's", "turn"])
                || ts.eat_phrase(&["enchanted", "player's", "turn"])
            {
                StepOwner::EnchantedController
            } else {
                StepOwner::Each
            }
        } else {
            StepOwner::Each
        };
        vec![TriggerEvent::StepEnds {
            step: TurnStepAst::EndOfCombat,
            owner,
        }]
    } else if ts.eat_word("when") || ts.eat_word("whenever") {
        parse_trigger_event_list(ts)?
    } else {
        return Err(ts.err("expected `when`, `whenever`, or `at the beginning of`"));
    };

    // Optional chained `and whenever <event>` / `or whenever <event>` / `or <event>` / `and at the beginning of <step>`
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
        let cp3 = ts.checkpoint();
        if ts.eat_word("or")
            && !ts.peek_phrase(&["if"])
            && let Ok(more) = try_parse(ts, parse_trigger_event_list)
        {
            events.extend(more);
            continue;
        }
        ts.restore(cp3);
        break;
    }

    let mut intervening_if = parse_optional_intervening_if(ts);
    loop {
        if ts.eat_phrase(&["this", "way"])
            || ts.eat_phrase(&["this", "turn"])
            || ts.eat_phrase(&["this", "combat"])
        {
            continue;
        }
        if ts.eat_phrase(&["for", "the", "first", "time", "each", "turn"])
            || ts.eat_phrase(&[
                "for", "the", "first", "time", "during", "each", "of", "your", "turns",
            ])
            || ts.eat_phrase(&[
                "for", "the", "first", "time", "during", "each", "of", "their", "turns",
            ])
            || ts.eat_phrase(&["for", "the", "first", "time", "during", "each", "turn"])
        {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Flag(ConditionFlag::FirstTimeEachTurn),
            ));
            continue;
        }
        if ts.eat_phrase(&["each", "turn"]) {
            continue;
        }
        {
            let wcp = ts.checkpoint();
            if ts.eat_phrase(&["while", "casting", "a", "spell", "with"])
                && let Ok(kw) = try_parse(ts, parse_keyword_kind)
            {
                intervening_if = Some(merge_cond(
                    intervening_if,
                    ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(kw)),
                ));
                continue;
            }
            ts.restore(wcp);
        }
        if ts.eat_phrase(&[
            "except", "the", "first", "one", "they", "draw", "in", "each", "of", "their", "draw",
            "steps",
        ]) || ts.eat_phrase(&[
            "except", "the", "first", "one", "you", "draw", "in", "each", "of", "your", "draw",
            "steps",
        ]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Flag(ConditionFlag::FirstTimeEachTurn),
            ));
            continue;
        }
        if ts.eat_phrase(&["while", "saddled"]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::SubjectState {
                    subject: Box::new(SubjectRef::SelfObject),
                    state: ObjectStateKind::Saddled,
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
        if ts.eat_phrase(&["during", "combat"]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::EventCheck(EventCheckKind::DuringStep(
                    TurnStepAst::BeginningOfCombat,
                )),
            ));
            continue;
        }
        if ts.eat_phrase(&["during", "your", "turn"])
            || ts.eat_phrase(&["during", "each", "of", "your", "turns"])
        {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Flag(ConditionFlag::IsYourTurn),
            ));
            continue;
        }
        if ts.eat_phrase(&["during", "your", "main", "phase"]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::EventCheck(EventCheckKind::DuringStep(TurnStepAst::MainPhase)),
            ));
            continue;
        }
        if ts.eat_phrase(&["during", "your", "untap", "step"]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::EventCheck(EventCheckKind::DuringStep(TurnStepAst::UntapStep)),
            ));
            continue;
        }
        if ts.eat_phrase(&["during", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "each", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "their", "turn"])
            || ts.eat_phrase(&["during", "each", "of", "their", "turns"])
        {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Not(Box::new(ConditionExpr::Flag(ConditionFlag::IsYourTurn))),
            ));
            continue;
        }
        {
            let cp = ts.checkpoint();
            if (ts.eat_word("and") || ts.eat_word("or"))
                && (ts.eat_word("whenever") || ts.eat_word("when"))
                && let Ok(more) = try_parse(ts, parse_trigger_event_list)
            {
                events.extend(more);
                continue;
            }
            ts.restore(cp);
        }
        {
            let cp = ts.checkpoint();
            if ts.eat_word("or")
                && !ts.peek_phrase(&["if"])
                && let Ok(more) = try_parse(ts, parse_trigger_event_list)
            {
                events.extend(more);
                continue;
            }
            ts.restore(cp);
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

fn parse_optional_intervening_if(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
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
    // `combat on your turn` / `the first combat phase on your turn` / `combat on each player's turn`
    if ts.eat_phrase(&["the", "first", "combat", "phase", "on"]) || ts.eat_phrase(&["combat", "on"])
    {
        let owner = if ts.eat_phrase(&["your", "turn"]) {
            StepOwner::Your
        } else if ts.eat_phrase(&["each", "player's", "turn"]) {
            StepOwner::EachPlayer
        } else if ts.eat_phrase(&["each", "opponent's", "turn"]) {
            StepOwner::EachOpponent
        } else if ts.eat_phrase(&["that", "player's", "turn"]) {
            StepOwner::ThatPlayer
        } else if ts.eat_phrase(&["enchanted", "opponent's", "turn"])
            || ts.eat_phrase(&["enchanted", "player's", "turn"])
        {
            StepOwner::EnchantedController
        } else {
            return Err(ts.err("expected turn owner after `combat on`"));
        };
        return Ok((TurnStepAst::BeginningOfCombat, owner));
    }

    // `the upkeep of <player>` / `the end step` / `each upkeep` / `each end step` / `each combat` / `each [postcombat|precombat] main phase`
    if ts.eat_phrase(&["each", "upkeep"]) {
        return Ok((TurnStepAst::Upkeep, StepOwner::Each));
    }
    if ts.eat_phrase(&["each", "end", "step"]) {
        return Ok((TurnStepAst::EndStep, StepOwner::Each));
    }
    if ts.eat_phrase(&["each", "combat"]) {
        return Ok((TurnStepAst::BeginningOfCombat, StepOwner::Each));
    }
    if ts.eat_phrase(&["each", "postcombat", "main", "phase"])
        || ts.eat_phrase(&["each", "precombat", "main", "phase"])
        || ts.eat_phrase(&["each", "first", "main", "phase"])
        || ts.eat_phrase(&["each", "second", "main", "phase"])
        || ts.eat_phrase(&["each", "main", "phase"])
    {
        return Ok((TurnStepAst::MainPhase, StepOwner::Each));
    }
    if ts.eat_phrase(&["the", "draw", "step"]) {
        if ts.eat_phrase(&["of", "enchanted", "creature's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "land's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "permanent's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "player"])
        {
            return Ok((TurnStepAst::DrawStep, StepOwner::EnchantedController));
        }
        let _ =
            ts.eat_phrase(&["of", "each", "opponent"]) || ts.eat_phrase(&["of", "each", "player"]);
        return Ok((TurnStepAst::DrawStep, StepOwner::The));
    }
    if ts.eat_phrase(&["that", "combat"]) || ts.eat_phrase(&["that", "combat", "phase"]) {
        return Ok((TurnStepAst::BeginningOfCombat, StepOwner::ThatCombat));
    }
    if ts.eat_phrase(&["the", "end", "step"]) {
        if ts.eat_phrase(&["on", "your", "next", "turn"]) {
            return Ok((TurnStepAst::EndStep, StepOwner::YourNext));
        }
        if ts.eat_phrase(&["of", "enchanted", "creature's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "land's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "permanent's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "player"])
        {
            return Ok((TurnStepAst::EndStep, StepOwner::EnchantedController));
        }
        return Ok((TurnStepAst::EndStep, StepOwner::The));
    }
    if ts.eat_phrase(&["the", "declare", "attackers", "step"]) {
        let owner = if ts.eat_phrase(&["on", "your", "next", "turn"]) {
            StepOwner::YourNext
        } else {
            StepOwner::The
        };
        return Ok((TurnStepAst::BeginningOfCombat, owner));
    }
    if ts.eat_phrase(&["the", "first", "upkeep"]) {
        return Ok((TurnStepAst::Upkeep, StepOwner::Next));
    }
    if ts.eat_phrase(&["your", "first", "upkeep"]) {
        return Ok((TurnStepAst::Upkeep, StepOwner::YourNext));
    }
    if ts.eat_phrase(&["your", "first", "main", "phase", "of", "the", "game"]) {
        return Ok((TurnStepAst::MainPhase, StepOwner::YourNext));
    }
    if ts.eat_phrase(&["the", "next", "combat", "phase"]) {
        let _ = ts.eat_phrase(&["this", "turn"]);
        return Ok((TurnStepAst::BeginningOfCombat, StepOwner::Next));
    }
    if ts.eat_phrase(&["the", "upkeep"]) {
        if ts.eat_phrase(&["of", "enchanted", "creature's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "land's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "permanent's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "enchantment's", "controller"])
            || ts.eat_phrase(&["of", "enchanted", "artifact's", "controller"])
            || ts.eat_phrase(&["of", "equipped", "creature's", "controller"])
        {
            return Ok((TurnStepAst::Upkeep, StepOwner::EnchantedController));
        }
        let _ = ts.eat_phrase(&["of", "enchanted", "player"])
            || ts.eat_phrase(&["of", "each", "opponent"])
            || ts.eat_phrase(&["of", "each", "player"]);
        return Ok((TurnStepAst::Upkeep, StepOwner::The));
    }

    let _ = ts.eat_phrase(&["each", "of"]);
    let owner = if ts.eat_phrase(&["your", "next"]) {
        StepOwner::YourNext
    } else if ts.eat_phrase(&["their", "next"]) {
        StepOwner::ThatPlayer
    } else if ts.eat_phrase(&["this", "turn's", "next"]) {
        StepOwner::Next
    } else if ts.eat_phrase(&["the", "next", "turn's"]) {
        StepOwner::NextTurn
    } else if ts.eat_phrase(&["the", "next"]) {
        StepOwner::Next
    } else if ts.eat_phrase(&["the", "chosen", "player's"]) {
        StepOwner::ChosenPlayer
    } else if ts.eat_phrase(&["that", "turn's"]) {
        StepOwner::ThatTurn
    } else if ts.eat_phrase(&["enchanted", "player's"])
        || ts.eat_phrase(&["enchanted", "opponent's"])
        || ts.eat_phrase(&["enchanted", "creature's", "controller's"])
        || ts.eat_phrase(&["enchanted", "enchantment's", "controller's"])
    {
        StepOwner::EnchantedController
    } else if ts.eat_word("your") {
        StepOwner::Your
    } else if ts.eat_phrase(&["each", "other", "player's"]) || ts.eat_phrase(&["each", "player's"])
    {
        StepOwner::EachPlayer
    } else if ts.eat_phrase(&["each", "opponent's"]) {
        StepOwner::EachOpponent
    } else if ts.eat_phrase(&["that", "player's"]) || ts.eat_phrase(&["the", "monarch's"]) {
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

    let step = if ts.eat_word("upkeep") || ts.eat_word("upkeeps") {
        TurnStepAst::Upkeep
    } else if ts.eat_phrase(&["end", "step"]) || ts.eat_phrase(&["end", "steps"]) {
        TurnStepAst::EndStep
    } else if ts.eat_phrase(&["draw", "step"]) || ts.eat_phrase(&["draw", "steps"]) {
        TurnStepAst::DrawStep
    } else if ts.eat_phrase(&["untap", "step"]) || ts.eat_phrase(&["untap", "steps"]) {
        TurnStepAst::UntapStep
    } else if ts.eat_phrase(&["cleanup", "step"]) {
        TurnStepAst::CleanupStep
    } else if ts.eat_phrase(&["main", "phase"]) || ts.eat_phrase(&["main", "phases"]) {
        TurnStepAst::MainPhase
    } else if ts.eat_phrase(&["end", "of", "combat"]) {
        TurnStepAst::EndOfCombat
    } else if ts.eat_word("combat") {
        TurnStepAst::BeginningOfCombat
    } else {
        return Err(ts.err("expected turn step name"));
    };
    Ok((step, owner))
}

/// Parse one or more trigger events sharing a subject (`When ~ enters or attacks`, `Whenever ~ blocks or becomes blocked`).
pub fn parse_trigger_event_list(ts: &mut TokenStream<'_>) -> PResult<Vec<TriggerEvent>> {
    // Passive prevention / counter-removal reflexive triggers
    if ts.eat_phrase(&["damage", "is", "prevented"]) {
        return Ok(vec![TriggerEvent::DealtDamage {
            subject: SubjectRef::SelfObject,
            combat_only: false,
        }]);
    }
    if ts.eat_phrase(&[
        "damage",
        "that",
        "would",
        "be",
        "dealt",
        "to",
        "you",
        "is",
        "prevented",
    ]) {
        return Ok(vec![TriggerEvent::DealtDamage {
            subject: SubjectRef::Player(PlayerRef::You),
            combat_only: false,
        }]);
    }
    // `players finish voting`
    if ts.eat_phrase(&["players", "finish", "voting"]) {
        return Ok(vec![TriggerEvent::AbilityActivated {
            player: PlayerRef::EachPlayer,
            non_mana_only: true,
        }]);
    }
    // `a land's ability causes you to add one or more mana of the chosen color`
    if ts.eat_phrase(&[
        "a", "land's", "ability", "causes", "you", "to", "add", "one", "or", "more", "mana", "of",
        "the", "chosen", "color",
    ]) {
        return Ok(vec![TriggerEvent::PlayerObjectEvent {
            player: PlayerRef::You,
            kind: PlayerObjectTriggerKind::TappedForMana,
            filter: Some(ObjectFilter {
                predicates: vec![FilterPredicate::OfChosenColor],
                ..Default::default()
            }),
        }]);
    }
    // `a mana ability of <subj> resolves`
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "mana", "ability", "of"])
            && try_parse(ts, parse_subject_ref).is_ok()
            && ts.eat_word("resolves")
        {
            return Ok(vec![TriggerEvent::AbilityActivated {
                player: PlayerRef::You,
                non_mana_only: false,
            }]);
        }
        ts.restore(mcp);
    }
    // `a player other than ~'s owner controls it`
    {
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["a", "player", "other", "than"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["'s", "owner", "controls", "it"])
        {
            return Ok(vec![TriggerEvent::LoseControl {
                player: PlayerRef::AnyPlayer,
                subject: SubjectRef::SelfObject,
            }]);
        }
        ts.restore(ocp);
    }
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"]) {
            let kind = opt(ts, parse_counter_kind).unwrap_or(CounterKind::Any);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["are", "removed", "from"])
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(vec![TriggerEvent::CounterEvent {
                    subject,
                    kind,
                    event: CounterTriggerKind::Removed {
                        while_exiled: false,
                    },
                }]);
            }
        }
        ts.restore(rcp);
    }

    // Player-initiated events (`<player> casts ...`, `<player> sacrifices ...`, `<player> draws ...`, `you cycle ~`, etc.)
    {
        let cp = ts.checkpoint();
        let player_opt = if ts.eat_phrase(&["one", "or", "more", "players"]) {
            Some(PlayerRef::EachPlayer)
        } else if ts.eat_phrase(&["one", "or", "more", "of", "your", "opponents"])
            || ts.eat_phrase(&["one", "or", "more", "opponents"])
        {
            Some(PlayerRef::EachOpponent)
        } else {
            try_parse(ts, parse_player_ref).ok()
        };
        if let Some(player) = player_opt {
            // Optional relative filter on player (`an opponent who controls <flt> draws a card / gains life`)
            {
                let wcp = ts.checkpoint();
                if ts.eat_phrase(&["who", "controls"]) && try_parse(ts, parse_object_filter).is_ok()
                {
                    // Relative qualifier consumed cleanly.
                } else {
                    ts.restore(wcp);
                }
            }
            if let Some(evs) = try_parse_player_trigger_event(ts, player)? {
                return Ok(evs);
            }
        }
        ts.restore(cp);
    }

    // `the <ordinal> <kind> counter is put on <subj>`
    {
        let ocp = ts.checkpoint();
        if ts.eat_word("the")
            && let Some(nth) = eat_ordinal(ts)
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_phrase(&["counter", "is", "put", "on"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(vec![TriggerEvent::CounterEvent {
                subject,
                kind,
                event: CounterTriggerKind::NthPut { nth },
            }]);
        }
        ts.restore(ocp);
    }

    // `the <ordinal> [<flt>] spell of a turn is cast`
    {
        let ocp = ts.checkpoint();
        if ts.eat_word("the") && eat_ordinal(ts).is_some() {
            let spell = if ts.eat_word("spell") {
                None
            } else {
                opt(ts, parse_object_filter)
            };
            if ts.eat_phrase(&["of", "a", "turn", "is", "cast"]) {
                return Ok(vec![TriggerEvent::SpellCast {
                    caster: PlayerRef::AnyPlayer,
                    spell,
                    from_outside_hand: false,
                }]);
            }
        }
        ts.restore(ocp);
    }

    // `there are no creatures on the battlefield` / `there are no <flt> (on the battlefield | in <zone>)`
    if ts.eat_phrase(&[
        "there",
        "are",
        "no",
        "creatures",
        "on",
        "the",
        "battlefield",
    ]) || ts.eat_phrase(&["no", "creatures", "are", "on", "the", "battlefield"])
    {
        return Ok(vec![TriggerEvent::Global(
            GlobalTriggerKind::NoCreaturesOnBattlefield,
        )]);
    }
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            if flt.zones.is_empty()
                && ts.eat_word("in")
                && let Ok(z) = try_parse(ts, parse_zone_ref)
            {
                flt.zones.push(z);
            }
            let _ = ts.eat_phrase(&["on", "the", "battlefield"]);
            return Ok(vec![TriggerEvent::PlayerObjectEvent {
                player: PlayerRef::You,
                kind: PlayerObjectTriggerKind::ControlNo,
                filter: Some(flt),
            }]);
        }
        ts.restore(ncp);
    }

    // `there are <N>[,<N2>] or more <kind> counters on <subj>`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are"])
            && let Some(mut n) = ts.next_number(false)
        {
            let comma_cp = ts.checkpoint();
            if ts.eat_comma() {
                if let Some(lo) = ts.next_number(false) {
                    n = n.saturating_mul(1000).saturating_add(lo);
                } else {
                    ts.restore(comma_cp);
                }
            }
            if ts.eat_phrase(&["or", "more"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(vec![TriggerEvent::CounterEvent {
                    subject,
                    kind,
                    event: CounterTriggerKind::Threshold {
                        min_count: u16::try_from(n.max(1)).unwrap_or(u16::MAX),
                    },
                }]);
            }
        }
        ts.restore(ncp);
    }

    // `[a/an | the last | ] <kind> counter(s) is/are removed from <subj> [while it's exiled]`
    {
        let rcp = ts.checkpoint();
        let _ = ts.eat_phrase(&["one", "or", "more"])
            || ts.eat_phrase(&["the", "last"])
            || ts.eat_word("a")
            || ts.eat_word("an");
        if let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["is", "removed", "from"])
                || ts.eat_phrase(&["are", "removed", "from"]))
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let while_exiled = ts.eat_phrase(&["while", "it's", "exiled"]);
            return Ok(vec![TriggerEvent::CounterEvent {
                subject,
                kind,
                event: CounterTriggerKind::Removed { while_exiled },
            }]);
        }
        ts.restore(rcp);
    }

    // `<N> or more <flt> attack [one or more <tgt>] [and aren't blocked]`
    {
        let mcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_word("attack")
        {
            if !ts.is_eof()
                && !matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
                && !ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("and")
                        || w.eq_ignore_ascii_case("or")
                        || w.eq_ignore_ascii_case("while")
                        || w.eq_ignore_ascii_case("during")
                })
            {
                let tcp = ts.checkpoint();
                let ate_oom = ts.eat_phrase(&["one", "or", "more", "of"])
                    || ts.eat_phrase(&["one", "or", "more"]);
                let got_tgt = opt(ts, parse_subject_ref).is_some()
                    || (ate_oom && (ts.eat_word("players") || ts.eat_word("opponents")));
                if !got_tgt {
                    ts.restore(tcp);
                }
            }
            if ts.eat_phrase(&["and", "aren't", "blocked"])
                || ts.eat_phrase(&["and", "isn't", "blocked"])
            {
                return Ok(vec![TriggerEvent::SubjectState {
                    subject: SubjectRef::Group(Box::new(flt)),
                    event: SubjectTriggerKind::AttacksAndNotBlocked,
                }]);
            }
            return Ok(vec![TriggerEvent::AttacksWithOthers {
                subject: None,
                min_count: u8::try_from(n.max(2)).unwrap_or(2),
            }]);
        }
        ts.restore(mcp);
    }

    // `[one or more | a | an] [<kind>] counter(s) is/are put on <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"]) || ts.eat_word("a") || ts.eat_word("an") {
            let kind = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && (ts.eat_phrase(&["are", "put", "on"]) || ts.eat_phrase(&["is", "put", "on"]))
            {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                let subject = parse_subject_ref(ts)?;
                return Ok(vec![TriggerEvent::CounterPlaced { subject, kind }]);
            }
        }
        ts.restore(cp);
    }

    // `the Ring tempts you`
    if ts.eat_phrase(&["the", "ring", "tempts", "you"]) {
        return Ok(vec![TriggerEvent::Global(GlobalTriggerKind::RingTemptsYou)]);
    }

    // `a spell or ability an opponent controls causes you to discard <subj>`
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&[
            "a", "spell", "or", "ability", "an", "opponent", "controls", "causes", "you", "to",
            "discard",
        ]) && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(vec![TriggerEvent::ForcedDiscard {
                player: PlayerRef::You,
                subject,
            }]);
        }
        ts.restore(fcp);
    }

    // `you're dealt [combat|noncombat] damage` / `[combat|noncombat] damage is dealt to <tgt>`
    if ts.eat_phrase(&["combat", "damage", "is", "dealt", "to", "you"]) {
        let ocp = ts.checkpoint();
        if !ts.eat_word("or") || try_parse(ts, parse_subject_ref).is_err() {
            ts.restore(ocp);
        }
        return Ok(vec![TriggerEvent::DealtDamage {
            subject: SubjectRef::Player(PlayerRef::You),
            combat_only: true,
        }]);
    }
    if ts.eat_phrase(&["damage", "is", "dealt", "to", "you"])
        || ts.eat_phrase(&["noncombat", "damage", "is", "dealt", "to", "you"])
    {
        let ocp = ts.checkpoint();
        if !ts.eat_word("or") || try_parse(ts, parse_subject_ref).is_err() {
            ts.restore(ocp);
        }
        return Ok(vec![TriggerEvent::DealtDamage {
            subject: SubjectRef::Player(PlayerRef::You),
            combat_only: false,
        }]);
    }
    if ts.eat_phrase(&[
        "damage", "is", "dealt", "to", "one", "of", "the", "chosen", "players",
    ]) {
        return Ok(vec![TriggerEvent::DealtDamage {
            subject: SubjectRef::Player(PlayerRef::ChosenPlayer),
            combat_only: false,
        }]);
    }
    if ts.eat_phrase(&["you're", "dealt"]) || ts.eat_phrase(&["you", "are", "dealt"]) {
        let combat_only = ts.eat_word("combat");
        let _ = ts.eat_word("noncombat");
        if ts.eat_word("damage") {
            return Ok(vec![TriggerEvent::DealtDamage {
                subject: SubjectRef::Player(PlayerRef::You),
                combat_only,
            }]);
        }
    }

    // `one or more opponents [each] lose [exactly <N>] life`
    {
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more", "opponents"]) {
            let _ = ts.eat_word("each");
            if ts.eat_word("lose") {
                if ts.eat_word("exactly") {
                    let _ = ts.next_number(false);
                }
                if ts.eat_word("life") {
                    return Ok(vec![TriggerEvent::LifeChanged {
                        player: PlayerRef::EachOpponent,
                        gained: false,
                    }]);
                }
            }
        }
        ts.restore(ocp);
    }

    // `an ability of <subj> is activated`
    {
        let ab_cp = ts.checkpoint();
        if ts.eat_phrase(&["an", "ability", "of"])
            && try_parse(ts, parse_subject_ref).is_ok()
            && ts.eat_phrase(&["is", "activated"])
        {
            return Ok(vec![TriggerEvent::AbilityActivated {
                player: PlayerRef::You,
                non_mana_only: false,
            }]);
        }
        ts.restore(ab_cp);
    }

    // `a spell or ability you control counters a spell`
    if ts.eat_phrase(&[
        "a", "spell", "or", "ability", "you", "control", "counters", "a", "spell",
    ]) {
        return Ok(vec![TriggerEvent::SpellCast {
            caster: PlayerRef::AnyOpponent,
            spell: None,
            from_outside_hand: false,
        }]);
    }

    // `the final chapter ability of a Saga you control resolves / triggers`
    if ts.eat_phrase(&[
        "the", "final", "chapter", "ability", "of", "a", "saga", "you", "control",
    ]) && (ts.eat_word("resolves") || ts.eat_word("triggers"))
    {
        return Ok(vec![TriggerEvent::AbilityActivated {
            player: PlayerRef::You,
            non_mana_only: true,
        }]);
    }

    // `a spell or ability you control exiles one or more permanents from the battlefield`
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["a", "spell", "or", "ability", "you", "control", "exiles"])
            && {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                true
            }
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let from = if ts.eat_word("from") {
                try_parse(ts, parse_zone_ref).ok().map(|z| z.zone)
            } else {
                None
            };
            return Ok(vec![TriggerEvent::ZoneChange {
                from,
                to: Some(ZoneKind::Exile),
                subject,
            }]);
        }
        ts.restore(ecp);
    }

    // `you and/or at least one permanent you control becomes the target of <flt>`
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "and/or", "at", "least", "one"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["becomes", "the", "target", "of"])
                || ts.eat_phrase(&["become", "the", "target", "of"]))
            && let Ok(source) = try_parse(ts, parse_object_filter)
        {
            return Ok(vec![TriggerEvent::BecomesTarget {
                subject: SubjectRef::Compound(vec![
                    SubjectRef::Player(PlayerRef::You),
                    SubjectRef::Group(Box::new(flt)),
                ]),
                source,
            }]);
        }
        ts.restore(lcp);
    }

    // `day becomes night or night becomes day`
    if ts.eat_phrase(&["day", "becomes", "night", "or", "night", "becomes", "day"]) {
        return Ok(vec![TriggerEvent::Global(
            GlobalTriggerKind::DayNightChanged,
        )]);
    }

    // `a/an [another] [noncreature] [color] source [you control] deals [noncombat|combat] [exactly <N>] damage [to <tgt>]`
    {
        let cp = ts.checkpoint();
        let started_another = ts.eat_word("another");
        if started_another || ts.eat_word("a") || ts.eat_word("an") {
            let mut src_flt = ObjectFilter::default();
            if started_another || ts.eat_word("another") {
                src_flt.flags = src_flt.flags.union(FilterFlags::EXCLUDE_SELF);
            }
            if ts.eat_word("noncreature") {
                src_flt.forbidden_types = src_flt
                    .forbidden_types
                    .union(crate::ast::CardTypeMask::CREATURE);
            }
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
                if ts.eat_phrase(&["other", "than"]) && ts.eat_tilde() {
                    src_flt.flags = src_flt.flags.union(FilterFlags::EXCLUDE_SELF);
                }
                let combat_only = if ts.eat_word("deals") {
                    if ts.next_number(false).is_some() {
                        let _ = ts.eat_phrase(&["or", "more"]);
                    }
                    let is_combat = ts.eat_word("combat");
                    let _ = ts.eat_word("noncombat");
                    if ts.eat_word("exactly") {
                        let _ = ts.next_number(false);
                    }
                    if ts.eat_word("damage") {
                        Some(is_combat)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(combat_only) = combat_only {
                    let target = if ts.eat_word("to") {
                        let _ = ts.eat_phrase(&["one", "or", "more", "of"])
                            || ts.eat_phrase(&["one", "or", "more"]);
                        if ts.eat_phrase(&["permanents", "and/or", "players"])
                            || ts.eat_word("players")
                        {
                            Some(SubjectRef::Player(PlayerRef::EachPlayer))
                        } else {
                            Some(parse_subject_ref(ts)?)
                        }
                    } else {
                        None
                    };
                    if ts.eat_phrase(&["equal", "to"]) {
                        let _ = try_parse(ts, crate::v2::value::parse_value_expr);
                    }
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

    // `~ and (at least <N> [other] <flt> | another <flt>) attack [different players]`
    {
        let cp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["and", "at", "least"])
                && let Some(n) = ts.next_number(false)
            {
                let _ = ts.eat_word("other");
                if try_parse(ts, parse_object_filter).is_ok()
                    && (ts.eat_word("attack") || ts.eat_word("attacks"))
                {
                    let _ = ts.eat_phrase(&["different", "players"]);
                    return Ok(vec![TriggerEvent::AttacksWithOthers {
                        subject: Some(subject),
                        min_count: u8::try_from(n).unwrap_or(2),
                    }]);
                }
            } else {
                ts.restore(cp);
                if let Ok(subject) = try_parse(ts, parse_subject_ref)
                    && ts.eat_phrase(&["and", "another"])
                    && try_parse(ts, parse_object_filter).is_ok()
                    && (ts.eat_word("attack") || ts.eat_word("attacks"))
                {
                    let _ = ts.eat_phrase(&["different", "players"]);
                    return Ok(vec![TriggerEvent::AttacksWithOthers {
                        subject: Some(subject),
                        min_count: 2,
                    }]);
                }
            }
        }
        ts.restore(cp);
    }

    // Subject-initiated events: support `one or more <flt>`, `fortified land`, compound `or`/`and/or` subjects,
    // comma-separated subject lists (`a Swamp, Mountain, black permanent, or red permanent`), and post-subject qualifiers.
    let _ = ts.eat_phrase(&["one", "or", "more"]);
    let first_subj = if ts.eat_phrase(&["fortified", "land"]) {
        SubjectRef::Anaphora(AnaphoraRef::EnchantedPermanent)
    } else {
        parse_subject_ref(ts)?
    };
    let mut subject = {
        let cp = ts.checkpoint();
        let mut parts = vec![first_subj.clone()];
        // Comma-separated subject list ending in `, or <flt>` (`Royal Decree`)
        let mut list_ok = false;
        loop {
            let lcp = ts.checkpoint();
            if ts.eat_comma() {
                if ts.eat_word("or") {
                    if let Ok(last_s) = try_parse(ts, parse_subject_ref) {
                        parts.push(last_s);
                        list_ok = true;
                        break;
                    }
                    ts.restore(lcp);
                    break;
                }
                if let Ok(next_s) = try_parse(ts, parse_subject_ref) {
                    parts.push(next_s);
                    continue;
                }
                ts.restore(lcp);
                break;
            }
            break;
        }
        if list_ok {
            SubjectRef::Compound(parts)
        } else {
            ts.restore(cp);
            if ts.eat_word("or") || ts.eat_word("and/or") {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                let _ = ts.eat_phrase(&["another", "double-faced"]);
                if let Ok(other_subj) = try_parse(ts, parse_subject_ref) {
                    SubjectRef::Compound(vec![first_subj, other_subj])
                } else {
                    ts.restore(cp);
                    first_subj
                }
            } else {
                ts.restore(cp);
                first_subj
            }
        }
    };

    subject_events::eat_subject_post_qualifiers(ts, &mut subject);

    let first_ev = subject_events::parse_subject_trigger_verb(ts, subject.clone())?;
    let mut events = vec![first_ev];

    // Optional compound trigger verbs sharing the same subject (`enters or attacks`, `enters or the creature it haunts dies`, `attacks or blocks`)
    loop {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["or", "the", "creature", "it", "haunts", "dies"]) {
            events.push(TriggerEvent::SubjectState {
                subject: subject.clone(),
                event: SubjectTriggerKind::DiesOrHauntedDies,
            });
            continue;
        }
        ts.restore(cp);
        if ts.eat_word("or")
            && let Ok(next_ev) = try_parse(ts, |s| {
                subject_events::parse_subject_trigger_verb(s, subject.clone())
            })
        {
            events.push(next_ev);
            continue;
        }
        ts.restore(cp);
        break;
    }

    Ok(events)
}

#[allow(clippy::too_many_lines)]
fn try_parse_player_trigger_event(
    ts: &mut TokenStream<'_>,
    player: PlayerRef,
) -> PResult<Option<Vec<TriggerEvent>>> {
    if ts.eat_phrase(&["cast", "or", "copy"]) || ts.eat_phrase(&["casts", "or", "copies"]) {
        let spell = opt(ts, parse_object_filter);
        return Ok(Some(vec![
            TriggerEvent::SpellCast {
                caster: player,
                spell: spell.clone(),
                from_outside_hand: false,
            },
            TriggerEvent::SpellCopied {
                caster: player,
                spell,
            },
        ]));
    }
    if ts.eat_word("copy") || ts.eat_word("copies") {
        let spell = opt(ts, parse_object_filter);
        return Ok(Some(vec![TriggerEvent::SpellCopied {
            caster: player,
            spell,
        }]));
    }
    if (ts.eat_phrase(&["cast", "or", "cycle"]) || ts.eat_phrase(&["casts", "or", "cycles"]))
        && ts.eat_tilde()
    {
        return Ok(Some(vec![
            TriggerEvent::SpellCast {
                caster: player,
                spell: None,
                from_outside_hand: false,
            },
            TriggerEvent::Cycled {
                player,
                self_card: true,
            },
        ]));
    }
    if ts.eat_word("cast") || ts.eat_word("casts") {
        return parse_player_cast_trigger(ts, player).map(Some);
    }
    if ts.eat_word("sacrifice") || ts.eat_word("sacrifices") {
        let _ = ts.eat_phrase(&["one", "or", "more"]);
        let first = parse_subject_ref(ts)?;
        let ocp = ts.checkpoint();
        if (ts.eat_phrase(&["or", "discards"]) || ts.eat_phrase(&["or", "discard"]))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(Some(vec![
                TriggerEvent::PermanentSacrificed {
                    player,
                    subject: first,
                },
                TriggerEvent::PlayerObjectEvent {
                    player,
                    kind: PlayerObjectTriggerKind::CardDiscarded,
                    filter: Some(flt),
                },
            ]));
        }
        ts.restore(ocp);
        let subject = if (ts.eat_word("or") || ts.eat_word("and/or"))
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            SubjectRef::Compound(vec![first, other])
        } else {
            ts.restore(ocp);
            first
        };
        return Ok(Some(vec![TriggerEvent::PermanentSacrificed {
            player,
            subject,
        }]));
    }
    if (ts.eat_phrase(&["cycle", "or", "discard"]) || ts.eat_phrase(&["cycles", "or", "discards"]))
        && (ts.eat_phrase(&["a", "card"])
            || ts.eat_phrase(&["another", "card"])
            || ts.eat_phrase(&["one", "or", "more", "cards"]))
    {
        return Ok(Some(vec![
            TriggerEvent::Cycled {
                player,
                self_card: false,
            },
            TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerKind::CardDiscarded,
            },
        ]));
    }
    if ts.eat_word("attack") || ts.eat_word("attacks") {
        let acp = ts.checkpoint();
        if !matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let _ = ts.eat_phrase(&["another", "one", "of"])
                || ts.eat_phrase(&["one", "or", "more", "of"])
                || ts.eat_phrase(&["one", "or", "more"]);
            let first_tgt = if ts.eat_phrase(&["the", "monarch"]) {
                Some(SubjectRef::Player(PlayerRef::ThatPlayer))
            } else {
                opt(ts, parse_subject_ref)
            };
            if first_tgt.is_some() {
                let ocp = ts.checkpoint();
                if ts.eat_word("and/or") || ts.eat_word("or") {
                    let _ = ts.eat_phrase(&["one", "or", "more"]);
                    if try_parse(ts, parse_subject_ref).is_err() {
                        ts.restore(ocp);
                    }
                }
            }
            let _ = ts.eat_phrase(&["who", "has", "the", "initiative"])
                || ts.eat_phrase(&["who", "has", "more", "life", "than", "you"]);
            if !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("with"))
                && first_tgt.is_some()
            {
                return Ok(Some(vec![TriggerEvent::Attacks {
                    subject: first_tgt,
                    player: Some(player),
                }]));
            }
        }
        if ts.eat_word("with") {
            let mw_cp = ts.checkpoint();
            if ts.eat_tilde()
                && (ts.eat_word("and/or") || ts.eat_word("and"))
                && let Ok(filter) = try_parse(ts, parse_object_filter)
            {
                return Ok(Some(vec![TriggerEvent::PlayerAttacksWith {
                    player,
                    filter,
                    min_count: 2,
                }]));
            }
            ts.restore(mw_cp);
            let _ = ts.eat_word("exactly") || ts.eat_phrase(&["at", "least"]);
            let min_count = ts
                .next_number(false)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(1);
            let _ = ts.eat_phrase(&["or", "more"]);
            if let Ok(filter) = try_parse(ts, parse_object_filter) {
                return Ok(Some(vec![TriggerEvent::PlayerAttacksWith {
                    player,
                    filter,
                    min_count,
                }]));
            }
        }
        ts.restore(acp);
        return Ok(Some(vec![TriggerEvent::Attacks {
            subject: None,
            player: Some(player),
        }]));
    }
    if ts.eat_phrase(&["pay", "this", "cost", "one", "or", "more", "times"]) {
        return Ok(Some(vec![TriggerEvent::AbilityActivated {
            player,
            non_mana_only: false,
        }]));
    }
    // `pay life` / `pay(s) ~'s cumulative upkeep` / `doesn't pay ~'s cumulative upkeep`
    {
        let pcp = ts.checkpoint();
        if (ts.eat_word("pay") || ts.eat_word("pays")) && ts.eat_word("life") {
            return Ok(Some(vec![TriggerEvent::LifeChanged {
                player,
                gained: false,
            }]));
        }
        ts.restore(pcp);
        let neg_cu = ts.eat_phrase(&["doesn't", "pay"]) || ts.eat_phrase(&["don't", "pay"]);
        if (neg_cu || ts.eat_word("pay") || ts.eat_word("pays"))
            && ts.eat_tilde()
            && ts.eat_phrase(&["'s", "cumulative", "upkeep"])
        {
            return Ok(Some(vec![TriggerEvent::AbilityActivated {
                player,
                non_mana_only: true,
            }]));
        }
        ts.restore(pcp);
    }
    // Die roll triggers (including Attractions `roll to visit your Attractions`)
    {
        let rcp = ts.checkpoint();
        if (ts.eat_word("roll") || ts.eat_word("rolls"))
            && (ts.eat_phrase(&["one", "or", "more", "dice"])
                || ts.eat_phrase(&["a", "die's", "highest", "natural", "result"])
                || ts.eat_phrase(&["a", "die"])
                || ts.eat_phrase(&["to", "visit", "your", "attractions"])
                || ts.eat_phrase(&["to", "visit", "their", "attractions"]))
        {
            return Ok(Some(vec![TriggerEvent::RolledDie { player, value: 0 }]));
        }
        ts.restore(rcp);
        if (ts.eat_word("roll") || ts.eat_word("rolls"))
            && (ts.eat_word("your") || ts.eat_word("their"))
            && let Some(nth) = eat_ordinal(ts)
            && ts.eat_phrase(&["die", "each", "turn"])
        {
            return Ok(Some(vec![TriggerEvent::RolledDie { player, value: nth }]));
        }
        ts.restore(rcp);
        if (ts.eat_word("roll") || ts.eat_word("rolls"))
            && ts.eat_word("a")
            && let Some(v) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
        {
            let _ = ts.eat_phrase(&["or", "higher"]);
            let mut evs = vec![TriggerEvent::RolledDie { player, value: v }];
            while ts.eat_word("or") {
                if let Some(v2) = ts.next_number(false).and_then(|n| u8::try_from(n).ok()) {
                    evs.push(TriggerEvent::RolledDie { player, value: v2 });
                } else {
                    break;
                }
            }
            return Ok(Some(evs));
        }
        ts.restore(rcp);
    }
    if ts.eat_phrase(&["become", "the", "monarch"]) || ts.eat_phrase(&["becomes", "the", "monarch"])
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::BecomesMonarch,
        }]));
    }
    if ts.eat_phrase(&["win", "a", "clash"])
        || ts.eat_phrase(&["wins", "a", "clash"])
        || ts.eat_phrase(&["clash", "and", "win"])
        || ts.eat_phrase(&["clashes", "and", "wins"])
        || ts.eat_word("clash")
        || ts.eat_word("clashes")
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::WinClash,
        }]));
    }
    if ts.eat_phrase(&["win", "a", "coin", "flip"])
        || ts.eat_phrase(&["wins", "a", "coin", "flip"])
        || ts.eat_phrase(&["lose", "a", "coin", "flip"])
        || ts.eat_phrase(&["loses", "a", "coin", "flip"])
        || ts.eat_phrase(&["win", "the", "flip"])
        || ts.eat_phrase(&["wins", "the", "flip"])
        || ts.eat_phrase(&["lose", "the", "flip"])
        || ts.eat_phrase(&["loses", "the", "flip"])
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::WinCoinFlip,
        }]));
    }
    if ts.eat_phrase(&["is", "attacked"]) || ts.eat_phrase(&["are", "attacked"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::IsAttacked,
        }]));
    }
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["is", "dealt"]) || ts.eat_phrase(&["are", "dealt"]) {
            if ts.next_number(false).is_some() {
                let _ = ts.eat_phrase(&["or", "more"]);
            }
            let combat_only = ts.eat_word("combat");
            let _ = ts.eat_word("noncombat");
            if ts.eat_word("damage") {
                if ts.eat_word("by") {
                    let _ = ts.eat_phrase(&["a", "single", "source"])
                        || opt(ts, parse_subject_ref).is_some();
                    if ts.eat_phrase(&["or", "by"]) {
                        let _ = opt(ts, parse_subject_ref);
                    }
                }
                return Ok(Some(vec![TriggerEvent::DealtDamage {
                    subject: SubjectRef::Player(player),
                    combat_only,
                }]));
            }
        }
        ts.restore(dcp);
    }
    {
        let lcp = ts.checkpoint();
        if (ts.eat_word("remove") || ts.eat_word("removes"))
            && ts.eat_phrase(&["the", "last"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && ts.eat_word("from")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(Some(vec![TriggerEvent::LastCounterRemoved {
                subject,
                kind,
            }]));
        }
        ts.restore(lcp);
        if (ts.eat_word("remove") || ts.eat_word("removes"))
            && (ts.eat_word("a") || ts.eat_word("an") || ts.eat_phrase(&["one", "or", "more"]))
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && ts.eat_word("from")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let while_exiled = ts.eat_phrase(&["while", "it's", "exiled"]);
            return Ok(Some(vec![TriggerEvent::CounterEvent {
                subject,
                kind,
                event: CounterTriggerKind::Removed { while_exiled },
            }]));
        }
        ts.restore(lcp);
    }
    {
        let fcp = ts.checkpoint();
        if (ts.eat_word("turn") || ts.eat_word("turns"))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["face", "up"])
        {
            return Ok(Some(vec![TriggerEvent::SubjectState {
                subject: SubjectRef::Group(Box::new(flt)),
                event: SubjectTriggerKind::TurnedFaceUp,
            }]));
        }
        ts.restore(fcp);
    }
    // Player keyword actions: proliferate, collect evidence, forage, discover, investigate, manifest dread,
    // foretell a card, give a gift, solve a Case, place/put a sticker, open/visit/claim Attraction, bend, kick, choose Ring-bearer/targets/scry-bottom, untap
    if ts.eat_word("proliferate") || ts.eat_word("proliferates") {
        return Ok(Some(vec![TriggerEvent::CounterPlaced {
            subject: SubjectRef::Player(player),
            kind: None,
        }]));
    }
    if ts.eat_phrase(&["collect", "evidence"]) || ts.eat_phrase(&["collects", "evidence"]) {
        return Ok(Some(vec![TriggerEvent::AbilityActivated {
            player,
            non_mana_only: true,
        }]));
    }
    if ts.eat_word("forage")
        || ts.eat_word("forages")
        || ts.eat_word("discover")
        || ts.eat_word("discovers")
    {
        return Ok(Some(vec![TriggerEvent::AbilityActivated {
            player,
            non_mana_only: true,
        }]));
    }
    if ts.eat_word("investigate") || ts.eat_word("investigates") {
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::CreatesTokens,
            filter: None,
        }]));
    }
    if ts.eat_phrase(&["manifest", "dread"]) || ts.eat_phrase(&["manifests", "dread"]) {
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::CardPlayed,
            filter: None,
        }]));
    }
    if ts.eat_phrase(&["foretell", "a", "card"]) || ts.eat_phrase(&["foretells", "a", "card"]) {
        return Ok(Some(vec![TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Hand),
            to: Some(ZoneKind::Exile),
            subject: SubjectRef::Player(player),
        }]));
    }
    if ts.eat_phrase(&["give", "a", "gift"]) || ts.eat_phrase(&["gives", "a", "gift"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::CreateOrSacrificeToken,
        }]));
    }
    if ts.eat_phrase(&["solve", "a", "case"]) || ts.eat_phrase(&["solves", "a", "case"]) {
        return Ok(Some(vec![TriggerEvent::UnlockDoor {
            player,
            fully_unlocked: true,
        }]));
    }
    if ts.eat_phrase(&["kicks", "a", "spell"]) || ts.eat_phrase(&["kick", "a", "spell"]) {
        return Ok(Some(vec![TriggerEvent::SpellCast {
            caster: player,
            spell: None,
            from_outside_hand: false,
        }]));
    }
    if ts.eat_phrase(&["place", "a", "sticker"]) || ts.eat_phrase(&["places", "a", "sticker"]) {
        return Ok(Some(vec![TriggerEvent::CounterPlaced {
            subject: SubjectRef::Player(player),
            kind: None,
        }]));
    }
    if ts.eat_phrase(&["open", "an", "attraction"])
        || ts.eat_phrase(&["opens", "an", "attraction"])
        || ts.eat_phrase(&["visit", "an", "attraction"])
        || ts.eat_phrase(&["visits", "an", "attraction"])
        || ts.eat_phrase(&["claim", "the", "prize", "of", "an", "attraction"])
        || ts.eat_phrase(&["claims", "the", "prize", "of", "an", "attraction"])
    {
        return Ok(Some(vec![TriggerEvent::RolledDie { player, value: 0 }]));
    }
    if ts.eat_phrase(&["waterbend"]) {
        while ts.eat_comma() || ts.eat_word("or") {
            if !(ts.eat_word("earthbend")
                || ts.eat_word("firebend")
                || ts.eat_word("airbend")
                || ts.eat_word("waterbend"))
            {
                break;
            }
        }
        return Ok(Some(vec![TriggerEvent::AbilityActivated {
            player,
            non_mana_only: true,
        }]));
    }
    {
        let chcp = ts.checkpoint();
        if ts.eat_word("choose") || ts.eat_word("chooses") {
            if ts.eat_phrase(&["a", "creature", "as", "your", "ring-bearer"])
                || ts.eat_phrase(&["a", "creature", "as", "their", "ring-bearer"])
            {
                return Ok(Some(vec![TriggerEvent::Global(
                    GlobalTriggerKind::RingTemptsYou,
                )]));
            }
            if ts.eat_phrase(&[
                "to", "put", "one", "or", "more", "cards", "on", "the", "bottom", "of", "your",
                "library", "while", "scrying",
            ]) || ts.eat_phrase(&[
                "to", "put", "one", "or", "more", "cards", "on", "the", "bottom", "of", "their",
                "library", "while", "scrying",
            ]) {
                return Ok(Some(vec![TriggerEvent::PlayerAction {
                    player,
                    action: PlayerTriggerKind::Scry,
                }]));
            }
            if ts.eat_phrase(&["one", "or", "more", "targets"]) {
                return Ok(Some(vec![TriggerEvent::AbilityActivated {
                    player,
                    non_mana_only: true,
                }]));
            }
        }
        ts.restore(chcp);
    }
    {
        let ucp = ts.checkpoint();
        if (ts.eat_word("untap") || ts.eat_word("untaps"))
            && {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                true
            }
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(Some(vec![TriggerEvent::SubjectState {
                subject: SubjectRef::Group(Box::new(flt)),
                event: SubjectTriggerKind::BecomesUntapped,
            }]));
        }
        ts.restore(ucp);
    }
    if ts.eat_phrase(&["scry", "or", "surveil"]) || ts.eat_phrase(&["scries", "or", "surveils"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::ScryOrSurveil,
        }]));
    }
    if ts.eat_word("scry") || ts.eat_word("scries") {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::Scry,
        }]));
    }
    if ts.eat_word("surveil") || ts.eat_word("surveils") {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::Surveil,
        }]));
    }
    if ts.eat_phrase(&["create", "or", "sacrifice", "a", "token"])
        || ts.eat_phrase(&["creates", "or", "sacrifices", "a", "token"])
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::CreateOrSacrificeToken,
        }]));
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("create") || ts.eat_word("creates") {
            let _ = ts.eat_phrase(&["one", "or", "more"]) || ts.eat_word("a") || ts.eat_word("an");
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && flt.token == Some(true)
            {
                return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                    player,
                    kind: PlayerObjectTriggerKind::CreatesTokens,
                    filter: Some(flt),
                }]));
            }
        }
        ts.restore(ccp);
    }
    {
        let ecp = ts.checkpoint();
        if (ts.eat_word("activate") || ts.eat_word("activates"))
            && ts.eat_phrase(&["an", "exhaust", "ability"])
        {
            return Ok(Some(vec![TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerKind::ActivateExhaust,
            }]));
        }
        ts.restore(ecp);
    }
    {
        let gcp = ts.checkpoint();
        if (ts.eat_word("get") || ts.eat_word("gets"))
            && ts.eat_phrase(&["one", "or", "more"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            return Ok(Some(vec![TriggerEvent::CounterPlaced {
                subject: SubjectRef::Player(player),
                kind: Some(CounterKind::Energy),
            }]));
        }
        ts.restore(gcp);
    }
    if ts.eat_phrase(&["gain", "or", "lose", "life"])
        || ts.eat_phrase(&["gains", "or", "loses", "life"])
    {
        return Ok(Some(vec![
            TriggerEvent::LifeChanged {
                player,
                gained: true,
            },
            TriggerEvent::LifeChanged {
                player,
                gained: false,
            },
        ]));
    }
    if ts.eat_phrase(&["lose", "the", "game"]) || ts.eat_phrase(&["loses", "the", "game"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::LosesGame,
        }]));
    }
    {
        let tap_cp = ts.checkpoint();
        if (ts.eat_word("tap") || ts.eat_word("taps"))
            && ts.eat_tilde()
            && ts.eat_phrase(&["for", "mana"])
        {
            return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                player,
                kind: PlayerObjectTriggerKind::TappedForMana,
                filter: None,
            }]));
        }
        ts.restore(tap_cp);
    }
    if (ts.eat_word("tap") || ts.eat_word("taps"))
        && {
            let _ = ts.eat_phrase(&["one", "or", "more"]);
            true
        }
        && let Ok(mut filter) = try_parse(ts, parse_object_filter)
    {
        if ts.eat_phrase(&["for", "mana"]) {
            if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
                filter.predicates.push(FilterPredicate::OfChosenColor);
            }
            return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                player,
                kind: PlayerObjectTriggerKind::TappedForMana,
                filter: Some(filter),
            }]));
        }
        if ts.eat_word("for") && ts.eat_mana_cost().is_some() {
            return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                player,
                kind: PlayerObjectTriggerKind::TappedForMana,
                filter: Some(filter),
            }]));
        }
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::TapOpponentCreature,
            filter: Some(filter),
        }]));
    }
    if ts.eat_word("cycle") || ts.eat_word("cycles") {
        if ts.eat_tilde() {
            if (ts.eat_phrase(&["or", "cycle"]) || ts.eat_phrase(&["or", "cycles"]))
                && ts.eat_phrase(&["another", "card"])
            {
                return Ok(Some(vec![
                    TriggerEvent::Cycled {
                        player,
                        self_card: true,
                    },
                    TriggerEvent::Cycled {
                        player,
                        self_card: false,
                    },
                ]));
            }
            return Ok(Some(vec![TriggerEvent::Cycled {
                player,
                self_card: true,
            }]));
        }
        if ts.eat_phrase(&["a", "card"]) || ts.eat_phrase(&["another", "card"]) {
            return Ok(Some(vec![TriggerEvent::Cycled {
                player,
                self_card: false,
            }]));
        }
    }
    if ts.eat_word("draw") || ts.eat_word("draws") {
        {
            let dcp = ts.checkpoint();
            if (ts.eat_word("your") || ts.eat_word("their"))
                && let Some(nth) = if ts.eat_phrase(&["first", "or", "second"]) {
                    Some(2)
                } else {
                    eat_ordinal(ts)
                }
                && ts.eat_word("card")
                && (ts.eat_phrase(&["each", "turn"])
                    || ts.eat_phrase(&["in", "a", "turn"])
                    || ts.eat_phrase(&["during", "their", "turn"])
                    || ts.eat_phrase(&["during", "your", "turn"])
                    || ts.eat_phrase(&["during", "each", "of", "your", "draw", "steps"])
                    || ts.eat_phrase(&["during", "each", "of", "their", "draw", "steps"]))
            {
                return Ok(Some(vec![TriggerEvent::NthCardDrawn { player, nth }]));
            }
            ts.restore(dcp);
        }
        if ts.eat_phrase(&["one", "or", "more", "cards"])
            || ts.eat_phrase(&["a", "card"])
            || ts.eat_word("cards")
        {
            let _ = ts.eat_phrase(&["with", "the", "chosen", "name"]);
            return Ok(Some(vec![TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerKind::CardDrawn,
            }]));
        }
    }
    if ts.eat_word("activate") || ts.eat_word("activates") {
        let acp = ts.checkpoint();
        let mut matched_ab = ts.eat_phrase(&["an", "ability"])
            || ts.eat_phrase(&["a", "loyalty", "ability"])
            || ts.eat_phrase(&["a", "boast", "ability"])
            || ts.eat_phrase(&["an", "exhaust", "ability"])
            || ts.eat_phrase(&["a", "mana", "ability"])
            || ts.eat_phrase(&["a", "power-up", "ability"]);
        if !matched_ab
            && (ts.eat_word("a") || ts.eat_word("an"))
            && try_parse(ts, parse_keyword_kind).is_ok()
        {
            if ts.eat_word("or") {
                let _ = try_parse(ts, parse_keyword_kind);
            }
            if ts.eat_word("ability") {
                matched_ab = true;
            }
        }
        if !matched_ab {
            ts.restore(acp);
            if ts.eat_tilde()
                && ts.eat_word("'s")
                && try_parse(ts, parse_keyword_kind).is_ok()
                && ts.eat_word("ability")
            {
                matched_ab = true;
            }
        }
        if !matched_ab {
            ts.restore(acp);
            if ts.eat_phrase(&["an", "artifact's", "ability"])
                || ts.eat_phrase(&["a", "creature's", "ability"])
                || ts.eat_phrase(&["a", "permanent's", "ability"])
                || ts.eat_phrase(&["a", "land's", "ability"])
            {
                matched_ab = true;
            }
        }
        if matched_ab {
            if ts.eat_word("of") {
                let _ = opt(ts, parse_subject_ref);
            }
            if ts.eat_phrase(&["that", "targets"]) {
                let _ = opt(ts, parse_subject_ref);
            }
            {
                let wcp = ts.checkpoint();
                if ts.eat_word("without")
                    && ts.next_mana().is_some()
                    && ts.eat_phrase(&["in", "its", "activation", "cost"])
                {
                    // Activation cost exclusion consumed.
                } else {
                    ts.restore(wcp);
                }
            }
            let non_mana_only = ts.eat_phrase(&["that", "isn't", "a", "mana", "ability"]);
            return Ok(Some(vec![TriggerEvent::AbilityActivated {
                player,
                non_mana_only,
            }]));
        }
        ts.restore(acp);
    }
    if ts.eat_word("discard") || ts.eat_word("discards") {
        let first_ev = if ts.eat_tilde()
            || ts.eat_phrase(&["one", "or", "more", "cards"])
            || ts.eat_phrase(&["a", "card"])
            || ts.eat_word("cards")
        {
            Some(TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerKind::CardDiscarded,
            })
        } else {
            let _ = ts.eat_phrase(&["one", "or", "more"]);
            try_parse(ts, parse_object_filter)
                .ok()
                .map(|filter| TriggerEvent::PlayerObjectEvent {
                    player,
                    kind: PlayerObjectTriggerKind::CardDiscarded,
                    filter: Some(filter),
                })
        };
        if let Some(ev1) = first_ev {
            let mcp = ts.checkpoint();
            if ts.eat_phrase(&["or", "mills"]) || ts.eat_phrase(&["or", "mill"]) {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                let filter = opt(ts, parse_object_filter);
                return Ok(Some(vec![
                    ev1,
                    TriggerEvent::PlayerObjectEvent {
                        player,
                        kind: PlayerObjectTriggerKind::MillsCard,
                        filter,
                    },
                ]));
            }
            ts.restore(mcp);
            return Ok(Some(vec![ev1]));
        }
    }
    if (ts.eat_phrase(&["gain", "control", "of"]) || ts.eat_phrase(&["gains", "control", "of"]))
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        let _ = ts.eat_phrase(&["from", "another", "player"]) || ts.eat_phrase(&["from", "you"]);
        return Ok(Some(vec![TriggerEvent::LoseControl { player, subject }]));
    }
    if (ts.eat_word("gain") || ts.eat_word("gains")) && ts.eat_word("life") {
        return Ok(Some(vec![TriggerEvent::LifeChanged {
            player,
            gained: true,
        }]));
    }
    if (ts.eat_phrase(&["lose", "control", "of"]) || ts.eat_phrase(&["loses", "control", "of"]))
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        return Ok(Some(vec![TriggerEvent::LoseControl { player, subject }]));
    }
    if (ts.eat_word("expend") || ts.eat_word("expends"))
        && let Some(amt) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(Some(vec![TriggerEvent::Expend {
            player,
            amount: amt,
        }]));
    }
    if (ts.eat_word("lose") || ts.eat_word("loses")) && ts.eat_word("life") {
        return Ok(Some(vec![TriggerEvent::LifeChanged {
            player,
            gained: false,
        }]));
    }
    if ts.eat_word("play") || ts.eat_word("plays") {
        if ts.eat_phrase(&["a", "land"]) {
            let from_exile = ts.eat_phrase(&["from", "exile"])
                || ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"])
                || ts.eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"]);
            if ts.eat_phrase(&["or", "cast", "a", "spell"])
                || ts.eat_phrase(&["or", "casts", "a", "spell"])
            {
                let spell_from_exile = ts.eat_phrase(&["from", "exile"])
                    || ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"])
                    || ts.eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"])
                    || ts.eat_phrase(&["from", "anywhere", "other", "than", "exile"]);
                return Ok(Some(vec![
                    TriggerEvent::PlayerAction {
                        player,
                        action: PlayerTriggerKind::LandPlayed,
                    },
                    TriggerEvent::SpellCast {
                        caster: player,
                        spell: None,
                        from_outside_hand: from_exile || spell_from_exile,
                    },
                ]));
            }
            return Ok(Some(vec![TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerKind::LandPlayed,
            }]));
        }
        let _ = ts.eat_word("a") || ts.eat_word("an");
        if let Ok(filter) = try_parse(ts, parse_object_filter) {
            let _ = ts.eat_phrase(&["with", "two", "or", "more", "card", "types"])
                || ts.eat_phrase(&[
                    "with", "the", "same", "name", "as", "one", "of", "the", "exiled", "cards",
                ]);
            let mut evs = vec![TriggerEvent::PlayerObjectEvent {
                player,
                kind: PlayerObjectTriggerKind::CardPlayed,
                filter: Some(filter),
            }];
            let ccp = ts.checkpoint();
            if ts.eat_phrase(&["or", "cast"]) || ts.eat_phrase(&["or", "casts"]) {
                let spell = opt(ts, parse_object_filter);
                evs.push(TriggerEvent::SpellCast {
                    caster: player,
                    spell,
                    from_outside_hand: false,
                });
            } else {
                ts.restore(ccp);
            }
            return Ok(Some(evs));
        }
    }
    if (ts.eat_word("exert") || ts.eat_word("exerts"))
        && (ts.eat_phrase(&["a", "creature"]) || ts.eat_tilde())
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::ExertsCreature,
        }]));
    }
    if (ts.eat_word("complete") || ts.eat_word("completes")) && ts.eat_phrase(&["a", "dungeon"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::CompletesDungeon,
        }]));
    }
    if (ts.eat_word("shuffle") || ts.eat_word("shuffles"))
        && (ts.eat_phrase(&["their", "library"]) || ts.eat_phrase(&["your", "library"]))
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::ShufflesLibrary,
        }]));
    }
    if (ts.eat_word("search") || ts.eat_word("searches"))
        && (ts.eat_phrase(&["their", "library"])
            || ts.eat_phrase(&["your", "library"])
            || ts.eat_phrase(&["a", "library"]))
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::SearchesLibrary,
        }]));
    }
    if ts.eat_word("mill") || ts.eat_word("mills") {
        let _ = ts.eat_phrase(&["one", "or", "more"]);
        let filter = opt(ts, parse_object_filter);
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::MillsCard,
            filter,
        }]));
    }
    if (ts.eat_word("create") || ts.eat_word("creates"))
        && ts.eat_phrase(&["one", "or", "more"])
        && let Ok(filter) = try_parse(ts, parse_object_filter)
    {
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::CreatesTokens,
            filter: Some(filter),
        }]));
    }
    if (ts.eat_word("commit") || ts.eat_word("commits")) && ts.eat_phrase(&["a", "crime"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::CommitCrime,
        }]));
    }
    if (ts.eat_word("unlock") || ts.eat_word("unlocks")) && ts.eat_phrase(&["this", "door"]) {
        return Ok(Some(vec![TriggerEvent::UnlockDoor {
            player,
            fully_unlocked: false,
        }]));
    }
    if (ts.eat_phrase(&["fully", "unlock"]) || ts.eat_phrase(&["fully", "unlocks"]))
        && ts.eat_phrase(&["a", "room"])
    {
        return Ok(Some(vec![TriggerEvent::UnlockDoor {
            player,
            fully_unlocked: true,
        }]));
    }
    // State triggers: `<player> control(s) [no | <N> or more] <flt>` / `<player> has/have <N> or (less|more) life` / `has no cards in hand`
    if ((ts.eat_word("control") || ts.eat_word("controls")) && ts.eat_word("no"))
        || ts.eat_phrase(&["don't", "control"])
        || ts.eat_phrase(&["doesn't", "control"])
    {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let filter = parse_object_filter(ts)?;
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::ControlNo,
            filter: Some(filter),
        }]));
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("control") || ts.eat_word("controls") {
            if ts.next_number(false).is_some() {
                let _ = ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "fewer"]);
            }
            if let Ok(filter) = try_parse(ts, parse_object_filter) {
                return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                    player,
                    kind: PlayerObjectTriggerKind::CardPlayed,
                    filter: Some(filter),
                }]));
            }
        }
        ts.restore(ccp);
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_word("has") || ts.eat_word("have") {
            if ts.eat_phrase(&["no", "cards", "in", "hand"]) {
                return Ok(Some(vec![TriggerEvent::PlayerAction {
                    player,
                    action: PlayerTriggerKind::CardDiscarded,
                }]));
            }
            if ts.next_number(false).is_some()
                && (ts.eat_phrase(&["or", "less"])
                    || ts.eat_phrase(&["or", "more"])
                    || ts.eat_phrase(&["or", "fewer"]))
                && ts.eat_word("life")
            {
                return Ok(Some(vec![TriggerEvent::LifeChanged {
                    player,
                    gained: true,
                }]));
            }
        }
        ts.restore(hcp);
    }
    if ts.eat_word("put") || ts.eat_word("puts") {
        let pcp = ts.checkpoint();
        if (ts.eat_phrase(&["a", "sticker"])
            || ts.eat_phrase(&["an", "ability", "sticker"])
            || ts.eat_phrase(&["a", "name", "sticker"])
            || ts.eat_phrase(&["an", "art", "sticker"]))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(Some(vec![TriggerEvent::CounterPlaced {
                subject,
                kind: None,
            }]));
        }
        ts.restore(pcp);
        if ts.eat_phrase(&["one", "or", "more"]) || ts.eat_word("a") || ts.eat_word("an") {
            let kind = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_word("on") {
                let _ = ts.eat_phrase(&["one", "or", "more"]);
                let subject = parse_subject_ref(ts)?;
                return Ok(Some(vec![TriggerEvent::CounterPlaced { subject, kind }]));
            }
        }
        ts.restore(pcp);
        let _ = ts.eat_phrase(&["one", "or", "more"]);
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["onto", "the", "battlefield"])
        {
            return Ok(Some(vec![TriggerEvent::ZoneChange {
                from: None,
                to: Some(ZoneKind::Battlefield),
                subject,
            }]));
        }
    }
    if (ts.eat_word("reveal") || ts.eat_word("reveals"))
        && let Ok(filter) = try_parse(ts, parse_object_filter)
    {
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::CardPlayed,
            filter: Some(filter),
        }]));
    }
    Ok(None)
}

fn parse_player_cast_trigger(
    ts: &mut TokenStream<'_>,
    player: PlayerRef,
) -> PResult<Vec<TriggerEvent>> {
    if ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]) || ts.eat_word("it") {
        let from_outside_hand = ts.eat_phrase(&["from", "anywhere", "other", "than", "exile"])
            || ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"])
            || ts.eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"]);
        if ts.eat_phrase(&["or", "cycle"]) && ts.eat_tilde() {
            return Ok(vec![
                TriggerEvent::SpellCast {
                    caster: player,
                    spell: None,
                    from_outside_hand,
                },
                TriggerEvent::Cycled {
                    player,
                    self_card: true,
                },
            ]);
        }
        return Ok(vec![TriggerEvent::SpellCast {
            caster: player,
            spell: None,
            from_outside_hand,
        }]);
    }
    let _ = ts.eat_phrase(&["your", "next"])
        || ts.eat_phrase(&["their", "next"])
        || ts.eat_phrase(&["your", "first"])
        || ts.eat_phrase(&["their", "first"])
        || ts.eat_phrase(&["your", "second"])
        || ts.eat_phrase(&["their", "second"])
        || ts.eat_phrase(&["your", "third"])
        || ts.eat_phrase(&["their", "third"])
        || ts.eat_phrase(&["your", "fourth"])
        || ts.eat_phrase(&["their", "fourth"])
        || ts.eat_phrase(&["your", "fifth"])
        || ts.eat_phrase(&["their", "fifth"]);
    // Guard against `casts a spell or <other event>` where `parse_object_filter` would swallow `or a creature`
    let simple_spell_before_or = {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["a", "spell"])
            && ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("or"))
            && !ts.peek_phrase(&["or", "activate"])
            && !ts.peek_phrase(&["or", "activates"])
            && !ts.peek_phrase(&["or", "copy"])
            && !ts.peek_phrase(&["or", "copies"])
        {
            let after_spell = ts.checkpoint();
            let _ = ts.eat_word("or");
            let is_other_event = try_parse(ts, parse_trigger_event_list).is_ok();
            ts.restore(after_spell);
            if is_other_event {
                true
            } else {
                ts.restore(scp);
                false
            }
        } else {
            ts.restore(scp);
            false
        }
    };
    let spell = if simple_spell_before_or
        || ts.eat_phrase(&["a", "prepared", "spell"])
        || ts.eat_phrase(&["a", "modal", "spell"])
    {
        Some(ObjectFilter {
            any_spell: true,
            ..Default::default()
        })
    } else if let Some(flt) = opt(ts, parse_object_filter) {
        Some(flt)
    } else if ts.eat_phrase(&["a", "spell"]) {
        Some(ObjectFilter {
            any_spell: true,
            ..Default::default()
        })
    } else {
        None
    };
    let from_outside_hand = spell
        .as_ref()
        .is_some_and(|s| s.predicates.contains(&FilterPredicate::FromOutsideHand))
        || ts.eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"])
        || ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"])
        || ts.eat_phrase(&["from", "anywhere", "other", "than", "exile"]);
    let _ = ts.eat_phrase(&["using", "mana", "produced", "by"]) && ts.eat_tilde();
    let _ = ts.eat_phrase(&["using", "teamwork"]) || ts.eat_phrase(&["they", "don't", "own"]);
    // Optional mana-cost / mana-value / relational qualifiers on cast spells
    loop {
        if ts.eat_phrase(&["with", "an", "even", "mana", "value"])
            || ts.eat_phrase(&["with", "an", "odd", "mana", "value"])
            || ts.eat_phrase(&["with", "mana", "value", "of", "the", "chosen", "quality"])
            || (ts.eat_phrase(&["with", "a", "mana", "cost", "that", "contains"])
                && ts.eat_mana_cost().is_some())
            || ts.eat_phrase(&[
                "that",
                "has",
                "the",
                "same",
                "name",
                "as",
                "a",
                "card",
                "in",
                "your",
                "graveyard",
            ])
            || ts.eat_phrase(&[
                "that", "shares", "a", "color", "or", "mana", "value", "with", "the", "exiled",
                "card",
            ])
        {
            continue;
        }
        // `with mana value, power, or toughness equal to the chosen number` / `with power, toughness, or mana value 4`
        {
            let qcp = ts.checkpoint();
            if ts.eat_word("with")
                && (ts.eat_phrase(&["mana", "value"])
                    || ts.eat_word("power")
                    || ts.eat_word("toughness"))
            {
                let mut saw_list = false;
                while ts.eat_comma() || ts.eat_word("or") {
                    if ts.eat_phrase(&["mana", "value"])
                        || ts.eat_word("power")
                        || ts.eat_word("toughness")
                    {
                        saw_list = true;
                    } else {
                        break;
                    }
                }
                if saw_list {
                    let _ = ts.eat_phrase(&["equal", "to"]);
                    if ts.eat_phrase(&["the", "chosen", "number"])
                        || ts.next_number(false).is_some()
                    {
                        continue;
                    }
                }
            }
            ts.restore(qcp);
        }
        // `or a creature card in your graveyard` (trailing disjunctive clause on `doesn't share a creature type with a creature you control or ...`)
        {
            let gcp = ts.checkpoint();
            if ts.eat_word("or")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && !flt.zones.is_empty()
            {
                continue;
            }
            ts.restore(gcp);
        }
        {
            let mcp = ts.checkpoint();
            if ts.eat_word("with")
                && ts.eat_mana_cost().is_some()
                && ts.eat_phrase(&["in", "its", "mana", "cost"])
            {
                continue;
            }
            ts.restore(mcp);
        }
        {
            let ccp = ts.checkpoint();
            if ts.eat_phrase(&["with", "one", "or", "more"])
                && let Some(w) = ts.peek_word()
                && crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some()
            {
                ts.next_word();
                if ts.eat_phrase(&["mana", "symbols", "in", "its", "mana", "cost"]) {
                    continue;
                }
            }
            ts.restore(ccp);
        }
        break;
    }
    let _ =
        ts.eat_phrase(&["during", "your", "turn"]) || ts.eat_phrase(&["during", "their", "turn"]);
    // Optional `other than (your|their|the) first [<flt>] (each turn | that turn | (that player|they) cast(s) each turn)`
    {
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["other", "than"])
            && (ts.eat_phrase(&["your", "first"])
                || ts.eat_phrase(&["their", "first"])
                || ts.eat_phrase(&["the", "first"]))
        {
            let _ = opt(ts, parse_object_filter);
            let _ = ts.eat_phrase(&["that", "player", "casts"])
                || ts.eat_phrase(&["that", "player", "cast"])
                || ts.eat_phrase(&["they", "cast"]);
            let _ = ts.eat_phrase(&["each", "turn"])
                || ts.eat_phrase(&["that", "turn"])
                || ts.eat_phrase(&["in", "a", "turn"]);
        } else {
            ts.restore(ocp);
        }
    }
    let _ = ts.eat_phrase(&["each", "turn"]) || ts.eat_phrase(&["in", "a", "turn"]);
    let mut evs = vec![TriggerEvent::SpellCast {
        caster: player,
        spell,
        from_outside_hand,
    }];
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["or", "copies", "a", "spell"])
            || ts.eat_phrase(&["or", "copy", "a", "spell"])
        {
            evs.push(TriggerEvent::SpellCopied {
                caster: player,
                spell: None,
            });
        } else if ts.eat_phrase(&["or", "activate", "an", "ability"])
            || ts.eat_phrase(&["or", "activates", "an", "ability"])
        {
            if ts.eat_word("of") {
                let _ = opt(ts, parse_object_filter);
            }
            if ts.eat_phrase(&["that", "targets", "only"]) {
                let _ = ts.eat_tilde();
            }
            evs.push(TriggerEvent::AbilityActivated {
                player,
                non_mana_only: false,
            });
        } else {
            ts.restore(bcp);
        }
    }
    Ok(evs)
}

pub(super) fn eat_ordinal(ts: &mut TokenStream<'_>) -> Option<u8> {
    let w = ts.peek_word()?.to_ascii_lowercase();
    let val = match w.as_str() {
        "first" => 1,
        "second" => 2,
        "third" => 3,
        "fourth" => 4,
        "fifth" => 5,
        "sixth" => 6,
        "seventh" => 7,
        "eighth" => 8,
        "ninth" => 9,
        "tenth" => 10,
        "eleventh" => 11,
        "twelfth" => 12,
        "thirteenth" => 13,
        "fourteenth" => 14,
        "fifteenth" => 15,
        "sixteenth" => 16,
        "seventeenth" => 17,
        "eighteenth" => 18,
        "nineteenth" => 19,
        "twentieth" => 20,
        _ => return None,
    };
    ts.next_word();
    Some(val)
}
