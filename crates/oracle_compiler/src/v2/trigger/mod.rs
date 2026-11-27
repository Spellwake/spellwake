//! Strictly-typed [`TriggerSpec`] and [`TriggerEvent`] parser for `v2`.

mod subject_events;

use crate::ast::{ControllerRelation, TurnStepAst};
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
    let mut intervening_if: Option<ConditionExpr> = None;
    let mut events = if ts.eat_phrase(&["at", "the", "beginning", "of"]) {
        let (step, owner) = parse_step_with_owner(ts)?;
        vec![TriggerEvent::StepBegins { step, owner }]
    } else if ts.eat_phrase(&["at", "the", "end", "of"]) {
        let (step, owner) = parse_step_with_owner(ts)?;
        vec![TriggerEvent::StepEnds { step, owner }]
    } else if ts.eat_phrase(&["at", "end", "of", "combat"]) {
        let owner = if ts.eat_word("on") {
            parse_turn_owner(ts).unwrap_or(StepOwner::Each)
        } else {
            StepOwner::Each
        };
        vec![TriggerEvent::StepEnds {
            step: TurnStepAst::EndOfCombat,
            owner,
        }]
    } else if ts.eat_any_word(&["when", "whenever"]) {
        let (evs, extra_cond) = parse_trigger_event_list_with_cond(ts)?;
        if let Some(c) = extra_cond {
            intervening_if = Some(merge_cond(intervening_if, c));
        }
        evs
    } else {
        return Err(ts.err("expected `when`, `whenever`, or `at the beginning of`"));
    };

    // Optional chained `and whenever <event>` / `or whenever <event>` / `or <event>` / `and at the beginning of <step>`
    while eat_chained_trigger(ts, &mut events, &mut intervening_if, true) {}

    if let Some(c) = parse_optional_intervening_if(ts) {
        intervening_if = Some(merge_cond(intervening_if, c));
    }
    loop {
        if ts.eat_any_phrase(&[
            &["this", "way"],
            &["this", "turn"],
            &["this", "combat"],
            &["each", "turn"],
        ]) {
            continue;
        }
        if ts.eat_any_phrase(&[
            &["for", "the", "first", "time", "each", "turn"],
            &[
                "for", "the", "first", "time", "during", "each", "of", "your", "turns",
            ],
            &[
                "for", "the", "first", "time", "during", "each", "of", "their", "turns",
            ],
            &["for", "the", "first", "time", "during", "each", "turn"],
        ]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Flag(ConditionFlag::FirstTimeEachTurn),
            ));
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
        const DURING_STEPS: &[(&[&str], TurnStepAst)] = &[
            (&["during", "combat"], TurnStepAst::BeginningOfCombat),
            (&["during", "your", "main", "phase"], TurnStepAst::MainPhase),
            (&["during", "your", "untap", "step"], TurnStepAst::UntapStep),
        ];
        if let Some(step) = eat_table(ts, DURING_STEPS) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::EventCheck(EventCheckKind::DuringStep(step)),
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &["during", "your", "turn"],
            &["during", "each", "of", "your", "turns"],
        ]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Flag(ConditionFlag::IsYourTurn),
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &["during", "an", "opponent's", "turn"],
            &["during", "each", "opponent's", "turn"],
            &["during", "their", "turn"],
            &["during", "each", "of", "their", "turns"],
        ]) {
            intervening_if = Some(merge_cond(
                intervening_if,
                ConditionExpr::Not(Box::new(ConditionExpr::Flag(ConditionFlag::IsYourTurn))),
            ));
            continue;
        }
        if eat_chained_trigger(ts, &mut events, &mut intervening_if, false) {
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

fn eat_chained_trigger(
    ts: &mut TokenStream<'_>,
    events: &mut Vec<TriggerEvent>,
    intervening_if: &mut Option<ConditionExpr>,
    allow_at_beginning: bool,
) -> bool {
    let cp = ts.checkpoint();
    if ts.eat_any_word(&["and", "or"])
        && ts.eat_any_word(&["whenever", "when"])
        && let Ok((more, extra_cond)) = try_parse(ts, parse_trigger_event_list_with_cond)
    {
        events.extend(more);
        if let Some(c) = extra_cond {
            *intervening_if = Some(merge_cond(intervening_if.take(), c));
        }
        return true;
    }
    ts.restore(cp);
    if allow_at_beginning {
        if ts.eat_any_word(&["and", "or"])
            && ts.eat_phrase(&["at", "the", "beginning", "of"])
            && let Ok((step, owner)) = try_parse(ts, parse_step_with_owner)
        {
            events.push(TriggerEvent::StepBegins { step, owner });
            return true;
        }
        ts.restore(cp);
    }
    if ts.eat_word("or")
        && !ts.peek_phrase(&["if"])
        && let Ok((more, extra_cond)) = try_parse(ts, parse_trigger_event_list_with_cond)
    {
        events.extend(more);
        if let Some(c) = extra_cond {
            *intervening_if = Some(merge_cond(intervening_if.take(), c));
        }
        return true;
    }
    ts.restore(cp);
    false
}

fn eat_table<T: Copy>(ts: &mut TokenStream<'_>, table: &[(&[&str], T)]) -> Option<T> {
    table
        .iter()
        .find_map(|&(phrase, val)| ts.eat_phrase(phrase).then_some(val))
}

fn parse_turn_owner(ts: &mut TokenStream<'_>) -> Option<StepOwner> {
    const TURN_OWNERS: &[(&[&str], StepOwner)] = &[
        (&["your", "turn"], StepOwner::Your),
        (&["each", "player's", "turn"], StepOwner::EachPlayer),
        (&["each", "opponent's", "turn"], StepOwner::EachOpponent),
        (&["that", "player's", "turn"], StepOwner::ThatPlayer),
        (
            &["enchanted", "opponent's", "turn"],
            StepOwner::EnchantedController,
        ),
        (
            &["enchanted", "player's", "turn"],
            StepOwner::EnchantedController,
        ),
    ];
    eat_table(ts, TURN_OWNERS)
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
    if ts.eat_any_phrase(&[
        &["the", "first", "combat", "phase", "on"],
        &["combat", "on"],
    ]) {
        let owner =
            parse_turn_owner(ts).ok_or_else(|| ts.err("expected turn owner after `combat on`"))?;
        return Ok((TurnStepAst::BeginningOfCombat, owner));
    }

    const DIRECT_STEPS: &[(&[&str], TurnStepAst, StepOwner)] = &[
        (&["each", "upkeep"], TurnStepAst::Upkeep, StepOwner::Each),
        (
            &["each", "end", "step"],
            TurnStepAst::EndStep,
            StepOwner::Each,
        ),
        (
            &["each", "combat"],
            TurnStepAst::BeginningOfCombat,
            StepOwner::Each,
        ),
        (
            &["each", "postcombat", "main", "phase"],
            TurnStepAst::MainPhase,
            StepOwner::Each,
        ),
        (
            &["each", "precombat", "main", "phase"],
            TurnStepAst::MainPhase,
            StepOwner::Each,
        ),
        (
            &["each", "first", "main", "phase"],
            TurnStepAst::MainPhase,
            StepOwner::Each,
        ),
        (
            &["each", "second", "main", "phase"],
            TurnStepAst::MainPhase,
            StepOwner::Each,
        ),
        (
            &["each", "main", "phase"],
            TurnStepAst::MainPhase,
            StepOwner::Each,
        ),
        (
            &["that", "combat", "phase"],
            TurnStepAst::BeginningOfCombat,
            StepOwner::ThatCombat,
        ),
        (
            &["that", "combat"],
            TurnStepAst::BeginningOfCombat,
            StepOwner::ThatCombat,
        ),
        (
            &["the", "first", "upkeep"],
            TurnStepAst::Upkeep,
            StepOwner::Next,
        ),
        (
            &["your", "first", "upkeep"],
            TurnStepAst::Upkeep,
            StepOwner::YourNext,
        ),
        (
            &["your", "first", "main", "phase", "of", "the", "game"],
            TurnStepAst::MainPhase,
            StepOwner::YourNext,
        ),
    ];
    for &(phrase, step, owner) in DIRECT_STEPS {
        if ts.eat_phrase(phrase) {
            return Ok((step, owner));
        }
    }
    const OF_ENCHANTED_CTRL: &[&[&str]] = &[
        &["of", "enchanted", "creature's", "controller"],
        &["of", "enchanted", "land's", "controller"],
        &["of", "enchanted", "permanent's", "controller"],
        &["of", "enchanted", "player"],
    ];
    if ts.eat_phrase(&["the", "draw", "step"]) {
        if ts.eat_any_phrase(OF_ENCHANTED_CTRL) {
            return Ok((TurnStepAst::DrawStep, StepOwner::EnchantedController));
        }
        if ts.eat_phrase(&["of", "each", "opponent"]) {
            return Ok((TurnStepAst::DrawStep, StepOwner::EachOpponent));
        }
        if ts.eat_phrase(&["of", "each", "player"]) {
            return Ok((TurnStepAst::DrawStep, StepOwner::EachPlayer));
        }
        return Ok((TurnStepAst::DrawStep, StepOwner::The));
    }
    if ts.eat_phrase(&["the", "end", "step"]) {
        if ts.eat_phrase(&["on", "your", "next", "turn"]) {
            return Ok((TurnStepAst::EndStep, StepOwner::YourNext));
        }
        if ts.eat_any_phrase(OF_ENCHANTED_CTRL) {
            return Ok((TurnStepAst::EndStep, StepOwner::EnchantedController));
        }
        return Ok((TurnStepAst::EndStep, StepOwner::The));
    }
    if ts.eat_phrase(&["the", "next", "combat", "phase"]) {
        let _ = ts.eat_phrase(&["this", "turn"]);
        return Ok((TurnStepAst::BeginningOfCombat, StepOwner::Next));
    }
    if ts.eat_phrase(&["the", "upkeep"]) {
        if ts.eat_any_phrase(&[
            &["of", "enchanted", "creature's", "controller"],
            &["of", "enchanted", "land's", "controller"],
            &["of", "enchanted", "permanent's", "controller"],
            &["of", "enchanted", "enchantment's", "controller"],
            &["of", "enchanted", "artifact's", "controller"],
            &["of", "equipped", "creature's", "controller"],
            &["of", "enchanted", "player"],
        ]) {
            return Ok((TurnStepAst::Upkeep, StepOwner::EnchantedController));
        }
        if ts.eat_phrase(&["of", "each", "opponent"]) {
            return Ok((TurnStepAst::Upkeep, StepOwner::EachOpponent));
        }
        if ts.eat_phrase(&["of", "each", "player"]) {
            return Ok((TurnStepAst::Upkeep, StepOwner::EachPlayer));
        }
        return Ok((TurnStepAst::Upkeep, StepOwner::The));
    }

    let _ = ts.eat_phrase(&["each", "of"]);
    const STEP_OWNERS: &[(&[&str], StepOwner)] = &[
        (&["your", "next"], StepOwner::YourNext),
        (&["their", "next"], StepOwner::ThatPlayer),
        (&["this", "turn's", "next"], StepOwner::Next),
        (&["the", "next", "turn's"], StepOwner::NextTurn),
        (&["the", "next"], StepOwner::Next),
        (&["the", "chosen", "player's"], StepOwner::ChosenPlayer),
        (&["that", "turn's"], StepOwner::ThatTurn),
        (&["enchanted", "player's"], StepOwner::EnchantedController),
        (&["enchanted", "opponent's"], StepOwner::EnchantedController),
        (
            &["enchanted", "creature's", "controller's"],
            StepOwner::EnchantedController,
        ),
        (
            &["enchanted", "enchantment's", "controller's"],
            StepOwner::EnchantedController,
        ),
        (&["your"], StepOwner::Your),
        (&["each", "player's"], StepOwner::EachPlayer),
        (&["each", "opponent's"], StepOwner::EachOpponent),
        (&["that", "player's"], StepOwner::ThatPlayer),
        (&["defending", "player's"], StepOwner::DefendingPlayer),
    ];
    let owner = eat_table(ts, STEP_OWNERS)
        .ok_or_else(|| ts.err("expected step owner (`your`, `the next`, etc.)"))?;

    let _ = ts.eat_any_word(&["first", "second", "precombat", "postcombat"]);

    const STEP_NAMES: &[(&[&str], TurnStepAst)] = &[
        (&["upkeep"], TurnStepAst::Upkeep),
        (&["upkeeps"], TurnStepAst::Upkeep),
        (&["end", "step"], TurnStepAst::EndStep),
        (&["end", "steps"], TurnStepAst::EndStep),
        (&["draw", "step"], TurnStepAst::DrawStep),
        (&["draw", "steps"], TurnStepAst::DrawStep),
        (&["untap", "step"], TurnStepAst::UntapStep),
        (&["untap", "steps"], TurnStepAst::UntapStep),
        (&["cleanup", "step"], TurnStepAst::CleanupStep),
        (&["main", "phase"], TurnStepAst::MainPhase),
        (&["main", "phases"], TurnStepAst::MainPhase),
        (&["end", "of", "combat"], TurnStepAst::EndOfCombat),
        (&["combat"], TurnStepAst::BeginningOfCombat),
    ];
    let step = eat_table(ts, STEP_NAMES).ok_or_else(|| ts.err("expected turn step name"))?;
    Ok((step, owner))
}

/// Parse one or more trigger events sharing a subject (`When ~ enters or attacks`, `Whenever ~ blocks or becomes blocked`).
pub fn parse_trigger_event_list(ts: &mut TokenStream<'_>) -> PResult<Vec<TriggerEvent>> {
    let (evs, cond) = parse_trigger_event_list_with_cond(ts)?;
    if cond.is_some() {
        return Err(ts.err("trigger event requires intervening condition context"));
    }
    Ok(evs)
}

fn parse_trigger_event_list_with_cond(
    ts: &mut TokenStream<'_>,
) -> PResult<(Vec<TriggerEvent>, Option<ConditionExpr>)> {
    // `a player other than ~'s owner controls it`
    {
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["a", "player", "other", "than"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["'s", "owner", "controls", "it"])
        {
            return Ok((
                vec![TriggerEvent::LoseControl {
                    player: PlayerRef::AnyPlayer,
                    subject: SubjectRef::SelfObject,
                }],
                None,
            ));
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
                return Ok((
                    vec![TriggerEvent::CounterEvent {
                        subject,
                        kind,
                        event: CounterTriggerKind::Removed {
                            while_exiled: false,
                        },
                    }],
                    None,
                ));
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
        if let Some(player) = player_opt
            && let Some(evs) = try_parse_player_trigger_event(ts, player)?
        {
            return Ok((evs, None));
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
            return Ok((
                vec![TriggerEvent::CounterEvent {
                    subject,
                    kind,
                    event: CounterTriggerKind::NthPut { nth },
                }],
                None,
            ));
        }
        ts.restore(ocp);
    }

    // `there are no creatures on the battlefield`
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
        return Ok((
            vec![TriggerEvent::Global(
                GlobalTriggerKind::NoCreaturesOnBattlefield,
            )],
            None,
        ));
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
                return Ok((
                    vec![TriggerEvent::CounterEvent {
                        subject,
                        kind,
                        event: CounterTriggerKind::Threshold {
                            min_count: u16::try_from(n.max(1)).unwrap_or(u16::MAX),
                        },
                    }],
                    None,
                ));
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
            return Ok((
                vec![TriggerEvent::CounterEvent {
                    subject,
                    kind,
                    event: CounterTriggerKind::Removed { while_exiled },
                }],
                None,
            ));
        }
        ts.restore(rcp);
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
                return Ok((vec![TriggerEvent::CounterPlaced { subject, kind }], None));
            }
        }
        ts.restore(cp);
    }

    // `the Ring tempts you`
    if ts.eat_phrase(&["the", "ring", "tempts", "you"]) {
        return Ok((
            vec![TriggerEvent::Global(GlobalTriggerKind::RingTemptsYou)],
            None,
        ));
    }

    // `a spell or ability an opponent controls causes you to discard <subj>`
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "spell", "or", "ability"])
            && ts.eat_phrase(&["an", "opponent", "controls"])
            && ts.eat_phrase(&["causes", "you", "to", "discard"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok((
                vec![TriggerEvent::ForcedDiscard {
                    player: PlayerRef::You,
                    subject,
                }],
                None,
            ));
        }
        ts.restore(fcp);
    }

    // `you're dealt [combat] damage` / `[combat] damage is dealt to you`
    const DEALT_TO_YOU: &[(&[&str], bool)] = &[
        (&["combat", "damage", "is", "dealt", "to", "you"], true),
        (&["damage", "is", "dealt", "to", "you"], false),
    ];
    if let Some(combat_only) = eat_table(ts, DEALT_TO_YOU) {
        return Ok((
            vec![TriggerEvent::DealtDamage {
                subject: SubjectRef::Player(PlayerRef::You),
                combat_only,
            }],
            None,
        ));
    }
    if ts.eat_phrase(&[
        "damage", "is", "dealt", "to", "one", "of", "the", "chosen", "players",
    ]) {
        return Ok((
            vec![TriggerEvent::DealtDamage {
                subject: SubjectRef::Player(PlayerRef::ChosenPlayer),
                combat_only: false,
            }],
            None,
        ));
    }
    if ts.eat_any_phrase(&[&["you're", "dealt"], &["you", "are", "dealt"]]) {
        let combat_only = ts.eat_word("combat");
        if ts.eat_word("damage") {
            return Ok((
                vec![TriggerEvent::DealtDamage {
                    subject: SubjectRef::Player(PlayerRef::You),
                    combat_only,
                }],
                None,
            ));
        }
    }

    // `one or more opponents [each] lose life`
    {
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more", "opponents"]) {
            let _ = ts.eat_word("each");
            if ts.eat_phrase(&["lose", "life"]) {
                return Ok((
                    vec![TriggerEvent::LifeChanged {
                        player: PlayerRef::EachOpponent,
                        gained: false,
                    }],
                    None,
                ));
            }
        }
        ts.restore(ocp);
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
            return Ok((
                vec![TriggerEvent::ZoneChange {
                    from,
                    to: Some(ZoneKind::Exile),
                    subject,
                }],
                None,
            ));
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
            return Ok((
                vec![TriggerEvent::BecomesTarget {
                    subject: SubjectRef::Compound(vec![
                        SubjectRef::Player(PlayerRef::You),
                        SubjectRef::Group(Box::new(flt)),
                    ]),
                    source,
                }],
                None,
            ));
        }
        ts.restore(lcp);
    }

    // `day becomes night or night becomes day` / `it becomes day` / `it becomes night`
    if ts.eat_any_phrase(&[
        &["day", "becomes", "night", "or", "night", "becomes", "day"],
        &["day", "becomes", "night"],
        &["night", "becomes", "day"],
        &["it", "becomes", "day"],
        &["it", "becomes", "night"],
    ]) {
        return Ok((
            vec![TriggerEvent::Global(GlobalTriggerKind::DayNightChanged)],
            None,
        ));
    }

    // `a/an [another] [noncreature] [color] source [you control] deals [combat] damage [to <tgt>]`
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
                    let is_combat = ts.eat_word("combat");
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
                    return Ok((
                        vec![TriggerEvent::DealsDamage {
                            combat_only,
                            source: Some(SubjectRef::Group(Box::new(src_flt))),
                            target,
                        }],
                        None,
                    ));
                }
            }
        }
        ts.restore(cp);
    }

    // `~ and (at least <N> [other] creatures | another creature) attack`
    {
        let cp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            let min_opt = if ts.eat_phrase(&["and", "at", "least"])
                && let Some(n) = ts.next_number(false)
            {
                let _ = ts.eat_word("other");
                Some(u8::try_from(n).unwrap_or(2))
            } else if ts.eat_phrase(&["and", "another"]) {
                Some(2)
            } else {
                None
            };
            if let Some(min_count) = min_opt
                && ts.eat_any_word(&["creatures", "creature"])
                && ts.eat_any_word(&["attack", "attacks"])
            {
                return Ok((
                    vec![TriggerEvent::AttacksWithOthers {
                        subject: Some(subject),
                        min_count,
                    }],
                    None,
                ));
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
        // Comma-separated subject list ending in `, or <flt>`
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

    let (first_ev, mut cond) = subject_events::parse_subject_trigger_verb(ts, subject.clone())?;
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
            && let Ok((next_ev, next_cond)) = try_parse(ts, |s| {
                subject_events::parse_subject_trigger_verb(s, subject.clone())
            })
        {
            events.push(next_ev);
            if let Some(c) = next_cond {
                cond = Some(merge_cond(cond, c));
            }
            continue;
        }
        ts.restore(cp);
        break;
    }

    Ok((events, cond))
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
            && let Ok(first_tgt) = try_parse(ts, parse_subject_ref)
            && !ts.peek_word().is_some_and(|w| {
                w.eq_ignore_ascii_case("with")
                    || w.eq_ignore_ascii_case("who")
                    || w.eq_ignore_ascii_case("and/or")
                    || w.eq_ignore_ascii_case("or")
            })
        {
            return Ok(Some(vec![TriggerEvent::Attacks {
                subject: Some(first_tgt),
                player: Some(player),
            }]));
        }
        ts.restore(acp);
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
            let _ = ts.eat_phrase(&["at", "least"]);
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
    // `pay life`
    {
        let pcp = ts.checkpoint();
        if (ts.eat_word("pay") || ts.eat_word("pays")) && ts.eat_word("life") {
            return Ok(Some(vec![TriggerEvent::LifeChanged {
                player,
                gained: false,
            }]));
        }
        ts.restore(pcp);
    }
    // Die roll triggers
    {
        let rcp = ts.checkpoint();
        if (ts.eat_word("roll") || ts.eat_word("rolls"))
            && (ts.eat_phrase(&["one", "or", "more", "dice"]) || ts.eat_phrase(&["a", "die"]))
        {
            return Ok(Some(vec![TriggerEvent::RolledDie { player, value: 0 }]));
        }
        ts.restore(rcp);
        if (ts.eat_word("roll") || ts.eat_word("rolls"))
            && ts.eat_word("a")
            && let Some(v) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
        {
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
    const PLAYER_ACTION_1: &[(&[&str], PlayerTriggerKind)] = &[
        (
            &["become", "the", "monarch"],
            PlayerTriggerKind::BecomesMonarch,
        ),
        (
            &["becomes", "the", "monarch"],
            PlayerTriggerKind::BecomesMonarch,
        ),
        (&["win", "a", "clash"], PlayerTriggerKind::WinClash),
        (&["wins", "a", "clash"], PlayerTriggerKind::WinClash),
        (&["clash", "and", "win"], PlayerTriggerKind::WinClash),
        (&["clashes", "and", "wins"], PlayerTriggerKind::WinClash),
        (
            &["win", "a", "coin", "flip"],
            PlayerTriggerKind::WinCoinFlip,
        ),
        (
            &["wins", "a", "coin", "flip"],
            PlayerTriggerKind::WinCoinFlip,
        ),
        (&["win", "the", "flip"], PlayerTriggerKind::WinCoinFlip),
        (&["wins", "the", "flip"], PlayerTriggerKind::WinCoinFlip),
        (&["is", "attacked"], PlayerTriggerKind::IsAttacked),
        (&["are", "attacked"], PlayerTriggerKind::IsAttacked),
    ];
    if let Some(action) = eat_table(ts, PLAYER_ACTION_1) {
        return Ok(Some(vec![TriggerEvent::PlayerAction { player, action }]));
    }
    {
        let dcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["is", "dealt"], &["are", "dealt"]]) {
            let combat_only = ts.eat_word("combat");
            if ts.eat_word("damage") {
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
        if ts.eat_any_word(&["remove", "removes"])
            && ts.eat_phrase(&["the", "last"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counter", "counters"])
            && ts.eat_word("from")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(Some(vec![TriggerEvent::LastCounterRemoved {
                subject,
                kind,
            }]));
        }
        ts.restore(lcp);
        if ts.eat_any_word(&["remove", "removes"])
            && ts.eat_any_phrase(&[&["a"], &["an"], &["one", "or", "more"]])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counter", "counters"])
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
        if ts.eat_any_word(&["turn", "turns"])
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
    {
        let ucp = ts.checkpoint();
        if ts.eat_any_word(&["untap", "untaps"])
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
    const PLAYER_ACTION_2: &[(&[&str], PlayerTriggerKind)] = &[
        (&["scry", "or", "surveil"], PlayerTriggerKind::ScryOrSurveil),
        (
            &["scries", "or", "surveils"],
            PlayerTriggerKind::ScryOrSurveil,
        ),
        (&["scry"], PlayerTriggerKind::Scry),
        (&["scries"], PlayerTriggerKind::Scry),
        (&["surveil"], PlayerTriggerKind::Surveil),
        (&["surveils"], PlayerTriggerKind::Surveil),
        (
            &["create", "or", "sacrifice", "a", "token"],
            PlayerTriggerKind::CreateOrSacrificeToken,
        ),
        (
            &["creates", "or", "sacrifices", "a", "token"],
            PlayerTriggerKind::CreateOrSacrificeToken,
        ),
    ];
    if let Some(action) = eat_table(ts, PLAYER_ACTION_2) {
        return Ok(Some(vec![TriggerEvent::PlayerAction { player, action }]));
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
    {
        let tap_cp = ts.checkpoint();
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
            if matches!(
                filter.controller,
                Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
            ) {
                return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                    player,
                    kind: PlayerObjectTriggerKind::TapOpponentCreature,
                    filter: Some(filter),
                }]));
            }
        }
        ts.restore(tap_cp);
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
    if ts.eat_any_word(&["draw", "draws"]) {
        {
            let dcp = ts.checkpoint();
            if ts.eat_any_word(&["your", "their"])
                && let Some(nth) = eat_ordinal(ts)
                && ts.eat_word("card")
                && ts.eat_any_phrase(&[
                    &["each", "turn"],
                    &["in", "a", "turn"],
                    &["during", "their", "turn"],
                    &["during", "your", "turn"],
                    &["during", "each", "of", "your", "draw", "steps"],
                    &["during", "each", "of", "their", "draw", "steps"],
                ])
            {
                return Ok(Some(vec![TriggerEvent::NthCardDrawn { player, nth }]));
            }
            ts.restore(dcp);
        }
        if ts.eat_any_phrase(&[&["one", "or", "more", "cards"], &["a", "card"], &["cards"]]) {
            return Ok(Some(vec![TriggerEvent::PlayerAction {
                player,
                action: PlayerTriggerKind::CardDrawn,
            }]));
        }
    }
    if ts.eat_any_word(&["activate", "activates"]) {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["an", "ability"]) {
            let non_mana_only = ts.eat_phrase(&["that", "isn't", "a", "mana", "ability"]);
            return Ok(Some(vec![TriggerEvent::AbilityActivated {
                player,
                non_mana_only,
            }]));
        }
        ts.restore(acp);
    }
    if ts.eat_any_word(&["discard", "discards"]) {
        let first_ev = if ts.eat_tilde()
            || ts.eat_any_phrase(&[&["one", "or", "more", "cards"], &["a", "card"], &["cards"]])
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
            if ts.eat_any_phrase(&[&["or", "mills"], &["or", "mill"]]) {
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
    if ts.eat_any_word(&["gain", "gains"]) && ts.eat_word("life") {
        return Ok(Some(vec![TriggerEvent::LifeChanged {
            player,
            gained: true,
        }]));
    }
    if ts.eat_any_phrase(&[&["lose", "control", "of"], &["loses", "control", "of"]])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        return Ok(Some(vec![TriggerEvent::LoseControl { player, subject }]));
    }
    if ts.eat_any_word(&["expend", "expends"])
        && let Some(amt) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(Some(vec![TriggerEvent::Expend {
            player,
            amount: amt,
        }]));
    }
    if ts.eat_any_word(&["lose", "loses"]) && ts.eat_word("life") {
        return Ok(Some(vec![TriggerEvent::LifeChanged {
            player,
            gained: false,
        }]));
    }
    if ts.eat_any_word(&["play", "plays"]) {
        if ts.eat_phrase(&["a", "land"]) {
            let from_exile = ts.eat_any_phrase(&[
                &["from", "exile"],
                &["from", "anywhere", "other", "than", "your", "hand"],
                &["from", "anywhere", "other", "than", "their", "hand"],
            ]);
            if ts.eat_any_phrase(&[
                &["or", "cast", "a", "spell"],
                &["or", "casts", "a", "spell"],
            ]) {
                let spell_from_exile = ts.eat_any_phrase(&[
                    &["from", "exile"],
                    &["from", "anywhere", "other", "than", "your", "hand"],
                    &["from", "anywhere", "other", "than", "their", "hand"],
                ]);
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
        let _ = ts.eat_any_word(&["a", "an"]);
        if let Ok(filter) = try_parse(ts, parse_object_filter) {
            let mut evs = vec![TriggerEvent::PlayerObjectEvent {
                player,
                kind: PlayerObjectTriggerKind::CardPlayed,
                filter: Some(filter),
            }];
            let ccp = ts.checkpoint();
            if ts.eat_any_phrase(&[&["or", "cast"], &["or", "casts"]]) {
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
    if ts.eat_any_word(&["exert", "exerts"])
        && (ts.eat_phrase(&["a", "creature"]) || ts.eat_tilde())
    {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::ExertsCreature,
        }]));
    }
    const PLAYER_ACTION_3: &[(&[&str], PlayerTriggerKind)] = &[
        (
            &["complete", "a", "dungeon"],
            PlayerTriggerKind::CompletesDungeon,
        ),
        (
            &["completes", "a", "dungeon"],
            PlayerTriggerKind::CompletesDungeon,
        ),
        (
            &["shuffle", "their", "library"],
            PlayerTriggerKind::ShufflesLibrary,
        ),
        (
            &["shuffle", "your", "library"],
            PlayerTriggerKind::ShufflesLibrary,
        ),
        (
            &["shuffles", "their", "library"],
            PlayerTriggerKind::ShufflesLibrary,
        ),
        (
            &["shuffles", "your", "library"],
            PlayerTriggerKind::ShufflesLibrary,
        ),
        (
            &["search", "their", "library"],
            PlayerTriggerKind::SearchesLibrary,
        ),
        (
            &["search", "your", "library"],
            PlayerTriggerKind::SearchesLibrary,
        ),
        (
            &["search", "a", "library"],
            PlayerTriggerKind::SearchesLibrary,
        ),
        (
            &["searches", "their", "library"],
            PlayerTriggerKind::SearchesLibrary,
        ),
        (
            &["searches", "your", "library"],
            PlayerTriggerKind::SearchesLibrary,
        ),
        (
            &["searches", "a", "library"],
            PlayerTriggerKind::SearchesLibrary,
        ),
    ];
    if let Some(action) = eat_table(ts, PLAYER_ACTION_3) {
        return Ok(Some(vec![TriggerEvent::PlayerAction { player, action }]));
    }
    if ts.eat_any_word(&["mill", "mills"]) {
        let _ = ts.eat_phrase(&["one", "or", "more"]);
        let filter = opt(ts, parse_object_filter);
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::MillsCard,
            filter,
        }]));
    }
    if ts.eat_any_word(&["create", "creates"])
        && ts.eat_phrase(&["one", "or", "more"])
        && let Ok(filter) = try_parse(ts, parse_object_filter)
    {
        return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
            player,
            kind: PlayerObjectTriggerKind::CreatesTokens,
            filter: Some(filter),
        }]));
    }
    if ts.eat_any_word(&["commit", "commits"]) && ts.eat_phrase(&["a", "crime"]) {
        return Ok(Some(vec![TriggerEvent::PlayerAction {
            player,
            action: PlayerTriggerKind::CommitCrime,
        }]));
    }
    if ts.eat_any_word(&["unlock", "unlocks"]) && ts.eat_phrase(&["this", "door"]) {
        return Ok(Some(vec![TriggerEvent::UnlockDoor {
            player,
            fully_unlocked: false,
        }]));
    }
    if ts.eat_any_phrase(&[&["fully", "unlock"], &["fully", "unlocks"]])
        && ts.eat_phrase(&["a", "room"])
    {
        return Ok(Some(vec![TriggerEvent::UnlockDoor {
            player,
            fully_unlocked: true,
        }]));
    }
    // State trigger: `<player> control(s) no <flt>` / `don't control <flt>`
    {
        let ccp = ts.checkpoint();
        if (ts.eat_any_word(&["control", "controls"]) && ts.eat_word("no"))
            || ts.eat_any_phrase(&[&["don't", "control"], &["doesn't", "control"]])
        {
            let _ = ts.eat_any_word(&["a", "an"]);
            if let Ok(filter) = try_parse(ts, parse_object_filter) {
                return Ok(Some(vec![TriggerEvent::PlayerObjectEvent {
                    player,
                    kind: PlayerObjectTriggerKind::ControlNo,
                    filter: Some(filter),
                }]));
            }
        }
        ts.restore(ccp);
    }
    if ts.eat_any_word(&["put", "puts"]) {
        let pcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["one", "or", "more"], &["a"], &["an"]]) {
            let kind = opt(ts, parse_counter_kind);
            if ts.eat_any_word(&["counters", "counter"]) && ts.eat_word("on") {
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
    Ok(None)
}

const FROM_OUTSIDE_HAND: &[&[&str]] = &[
    &["from", "anywhere", "other", "than", "their", "hand"],
    &["from", "anywhere", "other", "than", "your", "hand"],
];

fn parse_player_cast_trigger(
    ts: &mut TokenStream<'_>,
    player: PlayerRef,
) -> PResult<Vec<TriggerEvent>> {
    if ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]) || ts.eat_word("it") {
        let from_outside_hand = ts.eat_any_phrase(FROM_OUTSIDE_HAND);
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
    // `your/their <ordinal> [<flt>] spell each turn` -> FilterPredicate::NthSpellCastThisTurn(nth)
    let nth_cast = {
        let ncp = ts.checkpoint();
        if ts.eat_any_word(&["your", "their"])
            && let Some(nth) = eat_ordinal(ts)
        {
            Some((nth, ncp))
        } else {
            ts.restore(ncp);
            None
        }
    };
    // Guard against `casts a spell or <other event>` where `parse_object_filter` would swallow `or a creature`
    let simple_spell_before_or = nth_cast.is_none() && {
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
            let is_other_event = try_parse(ts, parse_trigger_event_list_with_cond).is_ok();
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
    let mut spell = if simple_spell_before_or {
        Some(ObjectFilter {
            any_spell: true,
            ..Default::default()
        })
    } else if let Some(flt) = opt(ts, parse_object_filter) {
        Some(flt)
    } else if ts.eat_phrase(&["a", "spell"]) || (nth_cast.is_some() && ts.eat_word("spell")) {
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
        || ts.eat_any_phrase(FROM_OUTSIDE_HAND);
    let ate_each_turn = ts.eat_any_phrase(&[
        &["each", "turn"],
        &["in", "a", "turn"],
        &["during", "each", "turn"],
    ]);
    if let Some((nth, ncp)) = nth_cast {
        if ate_each_turn && let Some(ref mut flt) = spell {
            flt.predicates
                .push(FilterPredicate::NthSpellCastThisTurn(nth));
        } else {
            ts.restore(ncp);
            return Err(ts.err("expected `each turn` after ordinal spell cast trigger"));
        }
    }
    let mut evs = vec![TriggerEvent::SpellCast {
        caster: player,
        spell,
        from_outside_hand,
    }];
    {
        let bcp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["or", "copies", "a", "spell"],
            &["or", "copy", "a", "spell"],
        ]) {
            evs.push(TriggerEvent::SpellCopied {
                caster: player,
                spell: None,
            });
        } else if ts.eat_any_phrase(&[
            &["or", "activate", "an", "ability"],
            &["or", "activates", "an", "ability"],
        ]) {
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
