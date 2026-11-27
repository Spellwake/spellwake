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
        if ts.eat_any_phrase(&[
            &["that's", "enchanted"],
            &["that's", "equipped"],
            &["that", "was", "turned", "face", "up", "this", "turn"],
            &["but", "don't", "control"],
            &["it's", "paired", "with"],
            &["you've", "cast"],
            &["you", "cast"],
        ]) {
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

fn eat_optional_from_zone(ts: &mut TokenStream<'_>, allow_bare_anywhere: bool) -> Option<ZoneKind> {
    if ts.eat_phrase(&["from", "anywhere", "other", "than"]) {
        let _ = try_parse(ts, parse_zone_ref);
        if !allow_bare_anywhere {
            let ocp = ts.checkpoint();
            if !ts.eat_any_word(&["or", "and/or"]) || try_parse(ts, parse_zone_ref).is_err() {
                ts.restore(ocp);
            }
        }
        None
    } else if allow_bare_anywhere && ts.eat_phrase(&["from", "anywhere"]) {
        None
    } else if ts.eat_word("from") {
        let from = try_parse(ts, parse_zone_ref).ok().map(|z| z.zone);
        if from.is_some() || allow_bare_anywhere {
            let ocp = ts.checkpoint();
            if !ts.eat_any_word(&["and/or", "or"]) || try_parse(ts, parse_zone_ref).is_err() {
                ts.restore(ocp);
            }
        }
        from
    } else {
        None
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
            if ts.eat_any_phrase(&[
                &["cumulative", "upkeep", "is", "paid"],
                &["echo", "cost", "is", "paid"],
            ]) {
                return Ok(TriggerEvent::AbilityActivated {
                    player: PlayerRef::You,
                    non_mana_only: true,
                });
            }
            if ts.eat_any_phrase(&[&["power"], &["toughness"], &["mana", "value"]])
                && ts.eat_word("is")
                && ts.next_number(false).is_some()
            {
                let _ = ts.eat_any_phrase(&[
                    &["or", "greater"],
                    &["or", "more"],
                    &["or", "less"],
                    &["or", "fewer"],
                ]);
                return Ok(TriggerEvent::SubjectState {
                    subject,
                    event: SubjectTriggerKind::Transforms,
                });
            }
        }
        ts.restore(pcp);
    }

    // Trigger-causing triggers (`entering/attacking [under ...] causes a triggered ability [of ...] to trigger`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_any_word(&["entering", "attacking"]) {
            let _ = ts.eat_any_phrase(&[
                &["under", "an", "opponent's", "control"],
                &["under", "your", "control"],
            ]);
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

    if ts.eat_any_phrase(&[
        &["enters", "the", "battlefield"],
        &["enter", "the", "battlefield"],
        &["enters"],
        &["enter"],
    ]) {
        const ENTER_MODS: &[&str] = &["untapped", "tapped", "transformed", "attacking"];
        let _ = ts.eat_any_word(ENTER_MODS);
        let _ = ts.eat_any_phrase(&[
            &["under", "your", "control"],
            &["under", "an", "opponent's", "control"],
            &["under", "their", "control"],
            &["under", "a", "player's", "control"],
        ]);
        let _ = ts.eat_any_word(ENTER_MODS);
        let _ = ts.eat_phrase(&["without", "being", "played"]);
        if ts.eat_phrase(&["attached", "to"]) {
            let _ = opt(ts, parse_object_filter);
        }
        let from = eat_optional_from_zone(ts, false);
        let _ = ts.eat_any_phrase(&[
            &["during", "the", "declare", "attackers", "step"],
            &["during", "combat"],
        ]);
        return Ok(TriggerEvent::ZoneChange {
            from,
            to: Some(ZoneKind::Battlefield),
            subject,
        });
    }
    if ts.eat_any_word(&["leaves", "leave"]) {
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
            let _ = ts.eat_any_word(&["a", "an"]);
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
        let _ = ts.eat_any_word(&["a", "an"]);
        if let Ok(target) = try_parse(ts, parse_object_filter) {
            return Ok(TriggerEvent::BecomesAttached { subject, target });
        }
    }
    const SUBJ_STATE_1: &[(&[&str], SubjectTriggerKind)] = &[
        (&["phases", "in"], SubjectTriggerKind::PhasesIn),
        (&["phase", "in"], SubjectTriggerKind::PhasesIn),
        (&["phases", "out"], SubjectTriggerKind::PhasesOut),
        (&["phase", "out"], SubjectTriggerKind::PhasesOut),
        (&["mutates"], SubjectTriggerKind::Mutates),
        (&["mutate"], SubjectTriggerKind::Mutates),
        (&["crews", "a", "vehicle"], SubjectTriggerKind::CrewsVehicle),
        (&["crew", "a", "vehicle"], SubjectTriggerKind::CrewsVehicle),
        (&["saddles", "a", "mount"], SubjectTriggerKind::CrewsVehicle),
        (&["saddle", "a", "mount"], SubjectTriggerKind::CrewsVehicle),
    ];
    if let Some(event) = super::eat_table(ts, SUBJ_STATE_1) {
        return Ok(TriggerEvent::SubjectState { subject, event });
    }
    // Subject keyword-action triggers: explores, connives, evolves, fights, enlists, mentors, trains,
    // becomes renowned/plotted/saddled, is championed with ~, is countered
    if ts.eat_any_word(&["explores", "explore"]) {
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
    #[derive(Clone, Copy)]
    enum SubjVerbTag {
        State(SubjectTriggerKind),
        Evolve,
        Fight,
        Attacks,
        Plotted,
    }
    const SUBJ_VERB_1: &[(&[&str], SubjVerbTag)] = &[
        (
            &["connives"],
            SubjVerbTag::State(SubjectTriggerKind::Transforms),
        ),
        (
            &["connive"],
            SubjVerbTag::State(SubjectTriggerKind::Transforms),
        ),
        (&["evolves"], SubjVerbTag::Evolve),
        (&["evolve"], SubjVerbTag::Evolve),
        (&["fights"], SubjVerbTag::Fight),
        (&["fight"], SubjVerbTag::Fight),
        (&["enlists", "a", "creature"], SubjVerbTag::Attacks),
        (&["enlist", "a", "creature"], SubjVerbTag::Attacks),
        (&["mentors", "a", "creature"], SubjVerbTag::Attacks),
        (&["mentor", "a", "creature"], SubjVerbTag::Attacks),
        (&["trains"], SubjVerbTag::Attacks),
        (&["train"], SubjVerbTag::Attacks),
        (
            &["becomes", "renowned"],
            SubjVerbTag::State(SubjectTriggerKind::BecomesMonstrous),
        ),
        (
            &["become", "renowned"],
            SubjVerbTag::State(SubjectTriggerKind::BecomesMonstrous),
        ),
        (&["becomes", "plotted"], SubjVerbTag::Plotted),
        (&["become", "plotted"], SubjVerbTag::Plotted),
        (
            &["becomes", "saddled"],
            SubjVerbTag::State(SubjectTriggerKind::BecomesCrewed),
        ),
        (
            &["become", "saddled"],
            SubjVerbTag::State(SubjectTriggerKind::BecomesCrewed),
        ),
    ];
    if let Some(tag) = super::eat_table(ts, SUBJ_VERB_1) {
        return Ok(match tag {
            SubjVerbTag::State(event) => TriggerEvent::SubjectState { subject, event },
            SubjVerbTag::Evolve => TriggerEvent::CounterPlaced {
                subject,
                kind: Some(CounterKind::PlusOnePlusOne),
            },
            SubjVerbTag::Fight => TriggerEvent::DealsDamage {
                combat_only: false,
                source: Some(subject),
                target: None,
            },
            SubjVerbTag::Attacks => TriggerEvent::Attacks {
                subject: Some(subject),
                player: None,
            },
            SubjVerbTag::Plotted => TriggerEvent::ZoneChange {
                from: Some(ZoneKind::Hand),
                to: Some(ZoneKind::Exile),
                subject,
            },
        });
    }
    if ts.eat_phrase(&["is", "championed", "with"]) && ts.eat_tilde() {
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Battlefield),
            to: Some(ZoneKind::Exile),
            subject,
        });
    }
    if ts.eat_any_phrase(&[&["is", "countered"], &["are", "countered"]]) {
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
        if ts.eat_any_phrase(&[&["is", "returned", "to"], &["are", "returned", "to"]])
            && ts.eat_any_phrase(&[
                &["your", "hand"],
                &["its", "owner's", "hand"],
                &["their", "owners'", "hands"],
                &["a", "player's", "hand"],
                &["hand"],
            ])
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
    if ts.eat_any_phrase(&[
        &["is", "exiled", "from", "the", "battlefield"],
        &["are", "exiled", "from", "the", "battlefield"],
    ]) {
        let _ = ts.eat_phrase(&["while", "you're", "activating", "a", "craft", "ability"]);
        return Ok(TriggerEvent::ZoneChange {
            from: Some(ZoneKind::Battlefield),
            to: Some(ZoneKind::Exile),
            subject,
        });
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["has", "no"], &["have", "no"]])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
        {
            return Ok(TriggerEvent::CounterEvent {
                subject,
                kind,
                event: CounterTriggerKind::HasNone,
            });
        }
        ts.restore(hcp);
        if ts.eat_any_word(&["has", "have"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_any_phrase(&[&["or", "more"], &["or", "fewer"], &["or", "less"]])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_any_phrase(&[
                &["on", "it"],
                &["on", "them"],
                &["on", "him"],
                &["on", "her"],
            ])
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
        if ts.eat_any_word(&["has", "have"]) && try_parse(ts, parse_keyword_kind).is_ok() {
            return Ok(TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::Transforms,
            });
        }
        ts.restore(hcp);
    }
    const SUBJ_STATE_2: &[(&[&str], SubjectTriggerKind)] = &[
        (&["becomes", "crewed"], SubjectTriggerKind::BecomesCrewed),
        (
            &["becomes", "monstrous"],
            SubjectTriggerKind::BecomesMonstrous,
        ),
    ];
    if let Some(event) = super::eat_table(ts, SUBJ_STATE_2) {
        return Ok(TriggerEvent::SubjectState { subject, event });
    }
    if ts.eat_phrase(&["becomes", "level"])
        && let Some(level) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(TriggerEvent::ClassBecomesLevel { level });
    }
    if ts.eat_any_word(&["transforms", "transform"]) {
        if ts.eat_word("into") {
            let _ = ts.eat_tilde() || try_parse(ts, parse_object_filter).is_ok();
        }
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Transforms,
        });
    }
    if ts.eat_any_phrase(&[
        &["is", "sacrificed", "or", "destroyed"],
        &["are", "sacrificed", "or", "destroyed"],
    ]) {
        return Ok(TriggerEvent::Dies { subject });
    }
    if ts.eat_any_word(&["exploits", "exploit"]) && try_parse(ts, parse_object_filter).is_ok() {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::Exploits,
        });
    }
    if ts.eat_any_phrase(&[&["is", "cast"], &["are", "cast"]]) {
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
    if ts.eat_any_phrase(&[
        &["is", "put", "onto", "the", "stack"],
        &["are", "put", "onto", "the", "stack"],
    ]) {
        return Ok(TriggerEvent::ZoneChange {
            from: None,
            to: Some(ZoneKind::Stack),
            subject,
        });
    }
    if ts.eat_any_word(&["dies", "die"]) {
        let _ = ts.eat_any_phrase(&[&["during", "combat"], &["this", "turn"]]);
        return Ok(TriggerEvent::Dies { subject });
    }
    if ts.eat_any_phrase(&[&["is", "put", "into"], &["are", "put", "into"]]) {
        let z = if ts.eat_phrase(&["a", "player's", "graveyard"]) {
            ZoneRef {
                zone: ZoneKind::Graveyard,
                owner: ZoneOwner::Any,
            }
        } else {
            parse_zone_ref(ts)?
        };
        let from = eat_optional_from_zone(ts, true);
        return Ok(TriggerEvent::ZoneChange {
            from,
            to: Some(z.zone),
            subject,
        });
    }
    if ts.eat_any_phrase(&[
        &["attacks", "and", "isn't", "blocked"],
        &["attack", "and", "aren't", "blocked"],
    ]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::AttacksAndNotBlocked,
        });
    }
    if ts.eat_any_word(&["attacks", "attack"]) {
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
            let _ = ts.eat_any_phrase(&[&["one", "or", "more", "of"], &["one", "or", "more"]]);
            let _ = ts.eat_phrase(&["the", "monarch"]) || opt(ts, parse_subject_ref).is_some();
            let _ = ts.eat_any_phrase(&[
                &["who", "has", "more", "life", "than", "you"],
                &["who", "has", "the", "initiative"],
            ]);
        }
        let _ = ts.eat_word("alone");
        if ts.eat_any_phrase(&[&["and", "isn't", "blocked"], &["and", "aren't", "blocked"]]) {
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
    if ts.eat_any_phrase(&[&["becomes", "blocked"], &["become", "blocked"]]) {
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
    if ts.eat_any_word(&["blocks", "block"]) {
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
    if ts.eat_any_phrase(&[
        &["is", "turned", "face", "up"],
        &["are", "turned", "face", "up"],
    ]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::TurnedFaceUp,
        });
    }
    if ts.eat_any_phrase(&[
        &["is", "tapped", "for", "mana"],
        &["are", "tapped", "for", "mana"],
    ]) {
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
    // `causes ...` and `destroys <subj>` triggers
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("causes") {
            if ts.eat_any_phrase(&[
                &["a", "player", "to", "shuffle", "their", "library"],
                &["an", "opponent", "to", "shuffle", "their", "library"],
                &["its", "controller", "to", "shuffle", "their", "library"],
            ]) {
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
    if ts.eat_any_phrase(&[&["becomes", "tapped"], &["become", "tapped"]]) {
        let _ = ts.eat_phrase(&["to", "pay", "a"])
            && try_parse(ts, parse_keyword_kind).is_ok()
            && ts.eat_word("cost");
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesTapped,
        });
    }
    if ts.eat_any_phrase(&[&["becomes", "untapped"], &["become", "untapped"]]) {
        return Ok(TriggerEvent::SubjectState {
            subject,
            event: SubjectTriggerKind::BecomesUntapped,
        });
    }
    if ts.eat_any_phrase(&[&["is", "dealt"], &["are", "dealt"]]) {
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
    if ts.eat_any_phrase(&[
        &["becomes", "the", "target", "of"],
        &["become", "the", "target", "of"],
    ]) {
        let acp = ts.checkpoint();
        if ts.eat_any_word(&["a", "an"])
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
    if ts.eat_any_phrase(&[
        &["is", "on", "the", "battlefield"],
        &["are", "on", "the", "battlefield"],
    ]) {
        return Ok(TriggerEvent::ZoneChange {
            from: None,
            to: Some(ZoneKind::Battlefield),
            subject,
        });
    }
    if ts.eat_any_phrase(&[&["is", "milled"], &["are", "milled"]]) {
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
