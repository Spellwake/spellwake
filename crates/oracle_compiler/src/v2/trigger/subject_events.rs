//! Subject-headed trigger verbs and post-subject trigger qualifiers for `v2`.

use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    CounterKind, CounterTriggerKind, FilterPredicate, ObjectFilter, PlayerObjectTriggerKind,
    PlayerRef, PlayerTriggerKind, SubjectRef, SubjectTriggerKind, TriggerEvent, ZoneKind,
    ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

/// Parse specialized non-subject trigger events (most-common-color state trigger, etc.).
pub(super) fn try_parse_extra_trigger_event(ts: &mut TokenStream<'_>) -> Option<Vec<TriggerEvent>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&[
        "the", "chosen", "color", "isn't", "the", "most", "common", "color", "among",
    ]) && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        let _ = ts.eat_phrase(&["or", "is", "tied", "for", "most", "common"]);
        return Some(vec![TriggerEvent::PlayerObjectEvent {
            player: PlayerRef::ChosenPlayer,
            kind: PlayerObjectTriggerKind::ControlNo,
            filter: Some(flt),
        }]);
    }
    ts.restore(cp);
    None
}

/// Consume optional post-subject relative qualifiers before a subject-headed trigger verb.
pub(super) fn eat_subject_post_qualifiers(ts: &mut TokenStream<'_>, _subject: &mut SubjectRef) {
    loop {
        if ts.eat_phrase(&["that's", "enchanted"])
            || ts.eat_phrase(&["that's", "equipped"])
            || ts.eat_phrase(&["that", "was", "turned", "face", "up", "this", "turn"])
            || ts.eat_phrase(&["but", "don't", "control"])
            || ts.eat_phrase(&["it's", "paired", "with"])
            || ts.eat_phrase(&["you've", "cast"])
            || ts.eat_phrase(&["you", "cast"])
        {
            continue;
        }
        {
            let ccp = ts.checkpoint();
            if ts.eat_phrase(&["crewed", "by"])
                && try_parse(ts, parse_subject_ref).is_ok()
                && ts.eat_phrase(&["this", "turn"])
            {
                continue;
            }
            ts.restore(ccp);
        }
        {
            let wcp = ts.checkpoint();
            if ts.eat_word("with")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && (ts.eat_phrase(&["attached", "to", "it"])
                    || flt
                        .predicates
                        .iter()
                        .any(|p| matches!(p, FilterPredicate::AttachedTo(_))))
            {
                continue;
            }
            ts.restore(wcp);
        }
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["dealt", "damage"]) {
                let _ = ts.eat_phrase(&["this", "turn"]);
                if ts.eat_word("by") && try_parse(ts, parse_subject_ref).is_ok() {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    continue;
                }
            }
            ts.restore(dcp);
            if ts.eat_word("by") && try_parse(ts, parse_subject_ref).is_ok() {
                let _ = ts.eat_phrase(&["this", "turn"]);
                if ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("dies") || w.eq_ignore_ascii_case("die")
                }) {
                    continue;
                }
            }
            ts.restore(dcp);
            if (ts.eat_phrase(&["equipped", "creature", "this", "turn"])
                || ts.eat_phrase(&["enchanted", "creature", "this", "turn"]))
                && ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("dies") || w.eq_ignore_ascii_case("die")
                })
            {
                continue;
            }
            ts.restore(dcp);
        }
        {
            let ecp = ts.checkpoint();
            if ts.eat_phrase(&["with", "a", "name", "originally", "printed", "in", "the"]) {
                let mut words = 0u8;
                let mut matched = false;
                while words < 5 {
                    if ts.eat_word("expansion") {
                        matched = words > 0;
                        break;
                    }
                    if ts.next_word().is_some() {
                        words += 1;
                    } else {
                        break;
                    }
                }
                if matched {
                    continue;
                }
            }
            ts.restore(ecp);
        }
        {
            let pcp = ts.checkpoint();
            if ts.eat_phrase(&["put", "onto", "the", "battlefield", "with"]) && ts.eat_tilde() {
                continue;
            }
            ts.restore(pcp);
        }
        break;
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_subject_trigger_verb(
    ts: &mut TokenStream<'_>,
    subject: SubjectRef,
) -> PResult<TriggerEvent> {
    // Possessive subject triggers (`~'s cumulative upkeep is paid`, `~'s echo cost is paid`, `~'s power is 7 or greater`)
    {
        let pcp = ts.checkpoint();
        if ts.eat_word("'s") {
            if ts.eat_phrase(&["cumulative", "upkeep", "is", "paid"])
                || ts.eat_phrase(&["echo", "cost", "is", "paid"])
            {
                return Ok(TriggerEvent::AbilityActivated {
                    player: PlayerRef::You,
                    non_mana_only: true,
                });
            }
            if (ts.eat_word("power")
                || ts.eat_word("toughness")
                || ts.eat_phrase(&["mana", "value"]))
                && ts.eat_word("is")
                && ts.next_number(false).is_some()
            {
                let _ = ts.eat_phrase(&["or", "greater"])
                    || ts.eat_phrase(&["or", "more"])
                    || ts.eat_phrase(&["or", "less"])
                    || ts.eat_phrase(&["or", "fewer"]);
                return Ok(TriggerEvent::SubjectState {
                    subject,
                    event: SubjectTriggerKind::Transforms,
                });
            }
        }
        ts.restore(pcp);
    }

    // Trigger-causing triggers (`entering [under an opponent's control] causes a triggered ability [of that creature] to trigger`,
    // `attacking causes a triggered ability [of that creature] to trigger`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_word("entering") || ts.eat_word("attacking") {
            let _ = ts.eat_phrase(&["under", "an", "opponent's", "control"])
                || ts.eat_phrase(&["under", "your", "control"]);
            if ts.eat_phrase(&["causes", "a", "triggered", "ability"]) {
                if ts.eat_word("of") {
                    let _ = opt(ts, parse_subject_ref);
                }
                if ts.eat_phrase(&["to", "trigger"]) {
                    return Ok(TriggerEvent::AbilityActivated {
                        player: PlayerRef::AnyPlayer,
                        non_mana_only: true,
                    });
                }
            }
        }
        ts.restore(tcp);
    }

    if ts.eat_phrase(&["enters", "the", "battlefield"])
        || ts.eat_phrase(&["enter", "the", "battlefield"])
        || ts.eat_word("enters")
        || ts.eat_word("enter")
    {
        let mut from = None;
        let _ = ts.eat_word("untapped")
            || ts.eat_word("tapped")
            || ts.eat_word("transformed")
            || ts.eat_word("attacking");
        let _ = ts.eat_phrase(&["under", "your", "control"])
            || ts.eat_phrase(&["under", "an", "opponent's", "control"])
            || ts.eat_phrase(&["under", "their", "control"])
            || ts.eat_phrase(&["under", "a", "player's", "control"]);
        let _ = ts.eat_word("untapped")
            || ts.eat_word("tapped")
            || ts.eat_word("transformed")
            || ts.eat_word("attacking");
        let _ = ts.eat_phrase(&["without", "being", "played"]);
        if ts.eat_phrase(&["attached", "to"]) {
            let _ = opt(ts, parse_object_filter);
        }
        if ts.eat_phrase(&["from", "anywhere", "other", "than"]) {
            let _ = try_parse(ts, parse_zone_ref);
            let ocp = ts.checkpoint();
            if !(ts.eat_word("or") || ts.eat_word("and/or"))
                || try_parse(ts, parse_zone_ref).is_err()
            {
                ts.restore(ocp);
            }
        } else if ts.eat_word("from")
            && let Ok(z) = try_parse(ts, parse_zone_ref)
        {
            from = Some(z.zone);
            let ocp = ts.checkpoint();
            if !(ts.eat_word("or") || ts.eat_word("and/or"))
                || try_parse(ts, parse_zone_ref).is_err()
            {
                ts.restore(ocp);
            }
        }
        let _ = ts.eat_phrase(&["during", "the", "declare", "attackers", "step"])
            || ts.eat_phrase(&["during", "combat"]);
        return Ok(TriggerEvent::ZoneChange {
            from,
            to: Some(ZoneKind::Battlefield),
            subject,
        });
    }
    if ts.eat_word("leaves") || ts.eat_word("leave") {
        if ts.eat_phrase(&["the", "battlefield"]) {
            let _ = ts.eat_phrase(&["without", "dying"]);
            return Ok(TriggerEvent::ZoneChange {
                from: Some(ZoneKind::Battlefield),
                to: None,
                subject,
            });
        }
        if let Ok(z) = try_parse(ts, parse_zone_ref) {
            return Ok(TriggerEvent::ZoneChange {
                from: Some(z.zone),
                to: None,
                subject,
            });
        }
    }
    if ts.eat_phrase(&["becomes", "unattached", "from"])
        && {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            true
        }
        && try_parse(ts, parse_object_filter).is_ok()
    {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesUnattached,
        });
    }
    if ts.eat_phrase(&["becomes", "attached", "to"]) {
        if ts.eat_tilde() {
            return Ok(TriggerEvent::BecomesAttached {
                subject,
                target: ObjectFilter {
                    any_permanent: true,
                    ..Default::default()
                },
            });
        }
        let _ = ts.eat_word("a") || ts.eat_word("an");
        if let Ok(target) = try_parse(ts, parse_object_filter) {
            return Ok(TriggerEvent::BecomesAttached { subject, target });
        }
    }
    if ts.eat_phrase(&["phases", "in"]) || ts.eat_phrase(&["phase", "in"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::PhasesIn,
        });
    }
    if ts.eat_phrase(&["phases", "out"]) || ts.eat_phrase(&["phase", "out"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::PhasesOut,
        });
    }
    if ts.eat_word("mutates") || ts.eat_word("mutate") {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Mutates,
        });
    }
    if ts.eat_phrase(&["crews", "a", "vehicle"])
        || ts.eat_phrase(&["crew", "a", "vehicle"])
        || ts.eat_phrase(&["saddles", "a", "mount"])
        || ts.eat_phrase(&["saddle", "a", "mount"])
    {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::CrewsVehicle,
        });
    }
    // Subject keyword-action triggers: explores, connives, evolves, fights, enlists, mentors, trains,
    // becomes renowned/plotted/saddled, is championed with ~, is countered
    if ts.eat_word("explores") || ts.eat_word("explore") {
        let ecp = ts.checkpoint();
        if !matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
            && try_parse(ts, parse_object_filter).is_err()
        {
            ts.restore(ecp);
        }
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Transforms,
        });
    }
    if ts.eat_word("connives") || ts.eat_word("connive") {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Transforms,
        });
    }
    if ts.eat_word("evolves") || ts.eat_word("evolve") {
        return Ok(TriggerEvent::CounterPlaced {
            subject,
            kind: Some(CounterKind::PlusOnePlusOne),
        });
    }
    if ts.eat_word("fights") || ts.eat_word("fight") {
        return Ok(TriggerEvent::DealsDamage {
            combat_only: false,
            source: Some(subject),
            target: None,
        });
    }
    if ts.eat_phrase(&["enlists", "a", "creature"])
        || ts.eat_phrase(&["enlist", "a", "creature"])
        || ts.eat_phrase(&["mentors", "a", "creature"])
        || ts.eat_phrase(&["mentor", "a", "creature"])
        || ts.eat_word("trains")
        || ts.eat_word("train")
    {
        return Ok(TriggerEvent::Attacks {
            subject: Some(subject),
            player: None,
        });
    }
    if ts.eat_phrase(&["becomes", "renowned"]) || ts.eat_phrase(&["become", "renowned"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesMonstrous,
        });
    }
    if ts.eat_phrase(&["becomes", "plotted"]) || ts.eat_phrase(&["become", "plotted"]) {
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Hand),
            to: Some(ZoneKind::Exile),
            subject,
        });
    }
    if ts.eat_phrase(&["becomes", "saddled"]) || ts.eat_phrase(&["become", "saddled"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesCrewed,
        });
    }
    if ts.eat_phrase(&["is", "championed", "with"]) && ts.eat_tilde() {
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Battlefield),
            to: Some(ZoneKind::Exile),
            subject,
        });
    }
    if ts.eat_phrase(&["is", "countered"]) || ts.eat_phrase(&["are", "countered"]) {
        let spell = match subject {
            SubjectRef::Group(f) | SubjectRef::All(f) | SubjectRef::Each(f) => Some(*f),
            _ => None,
        };
        return Ok(TriggerEvent::SpellCast {
            caster: PlayerRef::You,
            spell,
            from_outside_hand: false,
        });
    }
    {
        let rcp = ts.checkpoint();
        if (ts.eat_phrase(&["is", "returned", "to"]) || ts.eat_phrase(&["are", "returned", "to"]))
            && (ts.eat_phrase(&["your", "hand"])
                || ts.eat_phrase(&["its", "owner's", "hand"])
                || ts.eat_phrase(&["their", "owners'", "hands"])
                || ts.eat_phrase(&["a", "player's", "hand"])
                || ts.eat_word("hand"))
        {
            let from = if ts.eat_word("from") {
                try_parse(ts, parse_zone_ref).ok().map(|z| z.zone)
            } else {
                None
            };
            return Ok(TriggerEvent::ZoneChange {
                from,
                to: Some(ZoneKind::Hand),
                subject,
            });
        }
        ts.restore(rcp);
    }
    if ts.eat_phrase(&["is", "exiled", "from", "the", "battlefield"])
        || ts.eat_phrase(&["are", "exiled", "from", "the", "battlefield"])
    {
        let _ = ts.eat_phrase(&["while", "you're", "activating", "a", "craft", "ability"]);
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Battlefield),
            to: Some(ZoneKind::Exile),
            subject,
        });
    }
    {
        let hcp = ts.checkpoint();
        if (ts.eat_phrase(&["has", "no"]) || ts.eat_phrase(&["have", "no"]))
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
        {
            return Ok(TriggerEvent::CounterEvent {
                subject,
                kind,
                event: CounterTriggerKind::HasNone,
            });
        }
        ts.restore(hcp);
        if (ts.eat_word("has") || ts.eat_word("have"))
            && let Some(n) = ts.next_number(false)
            && (ts.eat_phrase(&["or", "more"])
                || ts.eat_phrase(&["or", "fewer"])
                || ts.eat_phrase(&["or", "less"]))
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && (ts.eat_phrase(&["on", "it"])
                || ts.eat_phrase(&["on", "them"])
                || ts.eat_phrase(&["on", "him"])
                || ts.eat_phrase(&["on", "her"]))
        {
            return Ok(TriggerEvent::CounterEvent {
                subject,
                kind,
                event: CounterTriggerKind::Threshold {
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                },
            });
        }
        ts.restore(hcp);
        if (ts.eat_word("has") || ts.eat_word("have")) && try_parse(ts, parse_keyword_kind).is_ok()
        {
            return Ok(TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::Transforms,
            });
        }
        ts.restore(hcp);
    }
    if ts.eat_phrase(&["becomes", "crewed"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesCrewed,
        });
    }
    if ts.eat_phrase(&["becomes", "monstrous"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesMonstrous,
        });
    }
    if ts.eat_phrase(&["becomes", "level"])
        && let Some(level) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(TriggerEvent::ClassBecomesLevel { level });
    }
    if ts.eat_word("transforms") || ts.eat_word("transform") {
        if ts.eat_word("into") {
            let _ = ts.eat_tilde() || try_parse(ts, parse_object_filter).is_ok();
        }
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Transforms,
        });
    }
    if ts.eat_phrase(&["is", "sacrificed", "or", "destroyed"])
        || ts.eat_phrase(&["are", "sacrificed", "or", "destroyed"])
    {
        return Ok(TriggerEvent::Dies { subject });
    }
    if (ts.eat_word("exploits") || ts.eat_word("exploit"))
        && try_parse(ts, parse_object_filter).is_ok()
    {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Exploits,
        });
    }
    if ts.eat_phrase(&["is", "cast"]) || ts.eat_phrase(&["are", "cast"]) {
        let spell = match subject {
            SubjectRef::Group(f) | SubjectRef::All(f) | SubjectRef::Each(f) => Some(*f),
            _ => None,
        };
        return Ok(TriggerEvent::SpellCast {
            caster: PlayerRef::AnyPlayer,
            spell,
            from_outside_hand: false,
        });
    }
    if ts.eat_phrase(&["is", "put", "onto", "the", "stack"])
        || ts.eat_phrase(&["are", "put", "onto", "the", "stack"])
    {
        return Ok(TriggerEvent::ZoneChange {
            from: None,
            to: Some(ZoneKind::Stack),
            subject,
        });
    }
    if ts.eat_word("dies") || ts.eat_word("die") {
        let _ = ts.eat_phrase(&["during", "combat"]) || ts.eat_phrase(&["this", "turn"]);
        return Ok(TriggerEvent::Dies { subject });
    }
    if ts.eat_phrase(&["is", "put", "into"]) || ts.eat_phrase(&["are", "put", "into"]) {
        let z = if ts.eat_phrase(&["a", "player's", "graveyard"]) {
            ZoneRef {
                zone: ZoneKind::Graveyard,
                owner: ZoneOwner::Any,
            }
        } else {
            parse_zone_ref(ts)?
        };
        let from = if ts.eat_phrase(&["from", "anywhere", "other", "than"]) {
            let _ = try_parse(ts, parse_zone_ref);
            None
        } else if ts.eat_phrase(&["from", "anywhere"]) {
            None
        } else if ts.eat_word("from") {
            let first_from = try_parse(ts, parse_zone_ref).ok().map(|zr| zr.zone);
            let ocp = ts.checkpoint();
            if !(ts.eat_word("and/or") || ts.eat_word("or"))
                || try_parse(ts, parse_zone_ref).is_err()
            {
                ts.restore(ocp);
            }
            first_from
        } else {
            None
        };
        return Ok(TriggerEvent::ZoneChange {
            from,
            to: Some(z.zone),
            subject,
        });
    }
    if ts.eat_phrase(&["attacks", "and", "isn't", "blocked"])
        || ts.eat_phrase(&["attack", "and", "aren't", "blocked"])
    {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::AttacksAndNotBlocked,
        });
    }
    if ts.eat_word("attacks") || ts.eat_word("attack") {
        let _ = ts.eat_word("alone");
        if !ts.is_eof()
            && !matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
            && !ts.peek_word().is_some_and(|w| {
                w.eq_ignore_ascii_case("or")
                    || w.eq_ignore_ascii_case("and")
                    || w.eq_ignore_ascii_case("while")
                    || w.eq_ignore_ascii_case("during")
                    || w.eq_ignore_ascii_case("for")
            })
        {
            let _ = ts.eat_phrase(&["one", "or", "more", "of"])
                || ts.eat_phrase(&["one", "or", "more"]);
            let _ = ts.eat_phrase(&["the", "monarch"]) || opt(ts, parse_subject_ref).is_some();
            let _ = ts.eat_phrase(&["who", "has", "more", "life", "than", "you"])
                || ts.eat_phrase(&["who", "has", "the", "initiative"]);
        }
        let _ = ts.eat_word("alone");
        if ts.eat_phrase(&["and", "isn't", "blocked"])
            || ts.eat_phrase(&["and", "aren't", "blocked"])
        {
            return Ok(TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::AttacksAndNotBlocked,
            });
        }
        return Ok(TriggerEvent::Attacks {
            subject: Some(subject),
            player: None,
        });
    }
    if ts.eat_phrase(&["becomes", "blocked"]) || ts.eat_phrase(&["become", "blocked"]) {
        let by = if ts.eat_word("by") {
            if ts.next_number(false).is_some() {
                let _ = ts.eat_phrase(&["or", "more"]);
            }
            opt(ts, parse_subject_ref).map(Box::new)
        } else {
            None
        };
        return Ok(TriggerEvent::BecomesBlocked { subject, by });
    }
    if ts.eat_word("blocks") || ts.eat_word("block") {
        let blocked = if ts.is_eof()
            || matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
            || ts.peek_phrase(&["or", "becomes", "blocked"])
            || ts.peek_phrase(&["or", "become", "blocked"])
        {
            None
        } else {
            if ts.next_number(false).is_some() {
                let _ = ts.eat_phrase(&["or", "more"]);
            }
            opt(ts, parse_subject_ref).map(Box::new)
        };
        return Ok(TriggerEvent::Blocks { subject, blocked });
    }
    if ts.eat_phrase(&["is", "turned", "face", "up"])
        || ts.eat_phrase(&["are", "turned", "face", "up"])
    {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::TurnedFaceUp,
        });
    }
    if ts.eat_phrase(&["is", "tapped", "for", "mana"])
        || ts.eat_phrase(&["are", "tapped", "for", "mana"])
    {
        let mut flt = match subject {
            SubjectRef::Group(f) | SubjectRef::All(f) | SubjectRef::Each(f) => *f,
            _ => ObjectFilter::default(),
        };
        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
            flt.predicates.push(FilterPredicate::OfChosenColor);
        }
        return Ok(TriggerEvent::PlayerObjectEvent {
            player: PlayerRef::AnyPlayer,
            kind: PlayerObjectTriggerKind::TappedForMana,
            filter: Some(flt),
        });
    }
    // `causes ...` and `destroys <subj>` triggers (`Firesong and Sunspeaker`, `Sacred Ground`, `Widespread Panic`, `Karmic Justice`)
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("causes") {
            if ts.eat_phrase(&["a", "player", "to", "shuffle", "their", "library"])
                || ts.eat_phrase(&["an", "opponent", "to", "shuffle", "their", "library"])
                || ts.eat_phrase(&["its", "controller", "to", "shuffle", "their", "library"])
            {
                return Ok(TriggerEvent::PlayerAction {
                    player: PlayerRef::AnyPlayer,
                    action: PlayerTriggerKind::ShufflesLibrary,
                });
            }
            if let Ok(p) = try_parse(ts, parse_player_ref)
                && ts.eat_phrase(&["to", "gain", "life"])
            {
                return Ok(TriggerEvent::LifeChanged {
                    player: p,
                    gained: true,
                });
            }
            ts.restore(ccp);
            if ts.eat_word("causes")
                && let Ok(moved) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["to", "be", "put", "into"])
                && let Ok(z) = try_parse(ts, parse_zone_ref)
            {
                let from = if ts.eat_word("from") {
                    try_parse(ts, parse_zone_ref).ok().map(|zr| zr.zone)
                } else {
                    None
                };
                return Ok(TriggerEvent::ZoneChange {
                    from,
                    to: Some(z.zone),
                    subject: moved,
                });
            }
        }
        ts.restore(ccp);
    }
    if ts.eat_word("destroys")
        && let Ok(destroyed) = try_parse(ts, parse_subject_ref)
    {
        return Ok(TriggerEvent::Dies { subject: destroyed });
    }
    if ts.eat_phrase(&["becomes", "tapped"]) || ts.eat_phrase(&["become", "tapped"]) {
        let _ = ts.eat_phrase(&["to", "pay", "a"])
            && try_parse(ts, parse_keyword_kind).is_ok()
            && ts.eat_word("cost");
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesTapped,
        });
    }
    if ts.eat_phrase(&["becomes", "untapped"]) || ts.eat_phrase(&["become", "untapped"]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesUntapped,
        });
    }
    if ts.eat_phrase(&["is", "dealt"]) || ts.eat_phrase(&["are", "dealt"]) {
        if ts.next_number(false).is_some() {
            let _ = ts.eat_phrase(&["or", "more"]);
        }
        let _ = ts.eat_word("excess");
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
            return Ok(TriggerEvent::DealtDamage {
                subject,
                combat_only,
            });
        }
    }
    if ts.eat_phrase(&["becomes", "the", "target", "of"])
        || ts.eat_phrase(&["become", "the", "target", "of"])
    {
        let acp = ts.checkpoint();
        if (ts.eat_word("a") || ts.eat_word("an"))
            && try_parse(ts, parse_keyword_kind).is_ok()
            && ts.eat_word("ability")
        {
            return Ok(TriggerEvent::BecomesTarget {
                subject,
                source: ObjectFilter {
                    is_ability: true,
                    ..Default::default()
                },
            });
        }
        ts.restore(acp);
        if ts.eat_phrase(&["an", "ability", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(TriggerEvent::BecomesTarget {
                subject,
                source: flt,
            });
        }
        ts.restore(acp);
        let source = parse_object_filter(ts)?;
        return Ok(TriggerEvent::BecomesTarget { subject, source });
    }
    if ts.eat_phrase(&["is", "on", "the", "battlefield"])
        || ts.eat_phrase(&["are", "on", "the", "battlefield"])
    {
        return Ok(TriggerEvent::ZoneChange {
            from: None,
            to: Some(ZoneKind::Battlefield),
            subject,
        });
    }
    if ts.eat_phrase(&["is", "milled"]) || ts.eat_phrase(&["are", "milled"]) {
        return Ok(TriggerEvent::PlayerObjectEvent {
            player: PlayerRef::You,
            kind: PlayerObjectTriggerKind::MillsCard,
            filter: None,
        });
    }
    {
        let dcp = ts.checkpoint();
        if ts.eat_word("deals") || ts.eat_word("deal") {
            if ts.next_number(false).is_some() {
                let _ = ts.eat_phrase(&["or", "more"]);
            }
            let combat_only = ts.eat_word("combat");
            let _ = ts.eat_word("noncombat");
            if ts.eat_word("damage") {
                let target = if ts.eat_word("to") {
                    let _ = ts.eat_phrase(&["one", "or", "more", "of"])
                        || ts.eat_phrase(&["one", "or", "more"]);
                    if ts.eat_phrase(&["permanents", "and/or", "players"]) || ts.eat_word("players")
                    {
                        Some(SubjectRef::Player(PlayerRef::EachPlayer))
                    } else if ts.eat_phrase(&["the", "player", "who", "cast"]) && ts.eat_tilde() {
                        Some(SubjectRef::Player(PlayerRef::ThatPlayer))
                    } else {
                        let first_t = parse_subject_ref(ts)?;
                        let ccp = ts.checkpoint();
                        let tgt = if ts.eat_word("or") {
                            if let Ok(second_t) = try_parse(ts, parse_subject_ref) {
                                SubjectRef::Compound(vec![first_t, second_t])
                            } else if ts.eat_word("opponent") {
                                SubjectRef::Compound(vec![
                                    first_t,
                                    SubjectRef::Player(PlayerRef::AnyOpponent),
                                ])
                            } else if ts.eat_word("player") {
                                SubjectRef::Compound(vec![
                                    first_t,
                                    SubjectRef::Player(PlayerRef::AnyPlayer),
                                ])
                            } else if ts.eat_word("planeswalker") {
                                SubjectRef::Compound(vec![
                                    first_t,
                                    SubjectRef::Group(Box::new(ObjectFilter {
                                        any_types: crate::ast::CardTypeMask::PLANESWALKER,
                                        ..Default::default()
                                    })),
                                ])
                            } else {
                                ts.restore(ccp);
                                first_t
                            }
                        } else {
                            ts.restore(ccp);
                            first_t
                        };
                        if ts.eat_phrase(&["who", "controls", "more"])
                            && try_parse(ts, parse_object_filter).is_ok()
                        {
                            let _ = ts.eat_phrase(&["than", "you"]);
                        }
                        Some(tgt)
                    }
                } else {
                    None
                };
                return Ok(TriggerEvent::DealsDamage {
                    combat_only,
                    source: Some(subject),
                    target,
                });
            }
        }
        ts.restore(dcp);
    }

    Err(ts.err("expected trigger event verb"))
}
