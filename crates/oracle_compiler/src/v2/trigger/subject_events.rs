//! Subject-headed trigger verbs and post-subject trigger qualifiers for `v2`.

use crate::ast::{CardTypeMask, ControllerRelation};
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    ConditionExpr, CounterTriggerKind, FilterFlags, FilterPredicate, ObjectFilter, ObjectStateKind,
    PlayerObjectTriggerKind, PlayerRef, SubjectRef, SubjectTriggerKind, TriggerEvent, ZoneKind,
    ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

fn subject_filter_mut(subject: &mut SubjectRef) -> Option<&mut ObjectFilter> {
    match subject {
        SubjectRef::Group(flt) | SubjectRef::All(flt) | SubjectRef::Each(flt) => Some(flt),
        _ => None,
    }
}

/// Consume optional post-subject relative qualifiers before a subject-headed trigger verb
/// ONLY when they can be encoded faithfully into the subject's [`ObjectFilter`].
pub(super) fn eat_subject_post_qualifiers(ts: &mut TokenStream<'_>, subject: &mut SubjectRef) {
    while let Some(flt) = subject_filter_mut(subject) {
        if ts.eat_phrase(&["that's", "enchanted"]) {
            flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
            continue;
        }
        if ts.eat_phrase(&["that's", "equipped"]) {
            flt.flags = flt.flags.union(FilterFlags::EQUIPPED);
            continue;
        }
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["dealt", "damage"]) {
                let saw_turn_first = ts.eat_phrase(&["this", "turn"]);
                if ts.eat_word("by") && ts.eat_tilde() {
                    let saw_turn_after = ts.eat_phrase(&["this", "turn"]);
                    if saw_turn_first || saw_turn_after {
                        flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
                        flt.predicates
                            .push(FilterPredicate::DealtDamageBySelfThisTurn);
                        continue;
                    }
                }
            }
            ts.restore(dcp);
        }
        break;
    }
}

fn eat_optional_from_zone(ts: &mut TokenStream<'_>, allow_bare_anywhere: bool) -> Option<ZoneKind> {
    if allow_bare_anywhere && ts.eat_phrase(&["from", "anywhere"]) {
        return None;
    }
    let cp = ts.checkpoint();
    if ts.eat_word("from")
        && let Ok(z) = try_parse(ts, parse_zone_ref)
    {
        return Some(z.zone);
    }
    ts.restore(cp);
    None
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_subject_trigger_verb(
    ts: &mut TokenStream<'_>,
    mut subject: SubjectRef,
) -> PResult<(TriggerEvent, Option<ConditionExpr>)> {
    if ts.eat_any_phrase(&[
        &["enters", "the", "battlefield"],
        &["enter", "the", "battlefield"],
        &["enters"],
        &["enter"],
    ]) {
        if let Some(flt) = subject_filter_mut(&mut subject)
            && flt.controller.is_none()
        {
            if ts.eat_phrase(&["under", "your", "control"]) {
                flt.controller = Some(ControllerRelation::You);
            } else if ts.eat_phrase(&["under", "an", "opponent's", "control"]) {
                flt.controller = Some(ControllerRelation::Opponent);
            } else if ts.eat_phrase(&["under", "their", "control"]) {
                flt.controller = Some(ControllerRelation::ThatPlayer);
            }
        }
        let from = eat_optional_from_zone(ts, false);
        return Ok((
            TriggerEvent::ZoneChange {
                from,
                to: Some(ZoneKind::Battlefield),
                subject,
            },
            None,
        ));
    }
    if ts.eat_any_word(&["leaves", "leave"]) {
        if ts.eat_phrase(&["the", "battlefield"]) {
            return Ok((
                TriggerEvent::ZoneChange {
                    from: Some(ZoneKind::Battlefield),
                    to: None,
                    subject,
                },
                None,
            ));
        }
        if let Ok(z) = try_parse(ts, parse_zone_ref) {
            return Ok((
                TriggerEvent::ZoneChange {
                    from: Some(z.zone),
                    to: None,
                    subject,
                },
                None,
            ));
        }
    }
    if ts.eat_phrase(&["becomes", "unattached"]) {
        let ucp = ts.checkpoint();
        if !(ts.eat_phrase(&["from", "a", "permanent"])
            || ts.eat_phrase(&["from", "a", "creature"]))
        {
            ts.restore(ucp);
        }
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::BecomesUnattached,
            },
            None,
        ));
    }
    if ts.eat_phrase(&["becomes", "attached", "to"]) {
        if ts.eat_tilde() {
            return Ok((
                TriggerEvent::BecomesAttached {
                    subject,
                    target: ObjectFilter {
                        any_permanent: true,
                        ..Default::default()
                    },
                },
                None,
            ));
        }
        let _ = ts.eat_any_word(&["a", "an"]);
        if let Ok(target) = try_parse(ts, parse_object_filter) {
            return Ok((TriggerEvent::BecomesAttached { subject, target }, None));
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
    ];
    if let Some(event) = super::eat_table(ts, SUBJ_STATE_1) {
        return Ok((TriggerEvent::SubjectState { subject, event }, None));
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
            let from = eat_optional_from_zone(ts, false);
            return Ok((
                TriggerEvent::ZoneChange {
                    from,
                    to: Some(ZoneKind::Hand),
                    subject,
                },
                None,
            ));
        }
        ts.restore(rcp);
    }
    if ts.eat_any_phrase(&[
        &["is", "exiled", "from", "the", "battlefield"],
        &["are", "exiled", "from", "the", "battlefield"],
    ]) {
        return Ok((
            TriggerEvent::ZoneChange {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Exile),
                subject,
            },
            None,
        ));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["has", "no"], &["have", "no"]])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
        {
            return Ok((
                TriggerEvent::CounterEvent {
                    subject,
                    kind,
                    event: CounterTriggerKind::HasNone,
                },
                None,
            ));
        }
        ts.restore(hcp);
        if ts.eat_any_word(&["has", "have"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_any_phrase(&[
                &["on", "it"],
                &["on", "them"],
                &["on", "him"],
                &["on", "her"],
            ])
        {
            return Ok((
                TriggerEvent::CounterEvent {
                    subject,
                    kind,
                    event: CounterTriggerKind::Threshold {
                        min_count: u16::try_from(n.max(1)).unwrap_or(1),
                    },
                },
                None,
            ));
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
        return Ok((TriggerEvent::SubjectState { subject, event }, None));
    }
    if ts.eat_phrase(&["becomes", "level"])
        && let Some(level) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok((TriggerEvent::ClassBecomesLevel { level }, None));
    }
    if ts.eat_any_word(&["transforms", "transform"]) {
        let tcp = ts.checkpoint();
        if !(ts.eat_word("into") && ts.eat_tilde()) {
            ts.restore(tcp);
        }
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::Transforms,
            },
            None,
        ));
    }
    if ts.eat_any_word(&["exploits", "exploit"]) && ts.eat_phrase(&["a", "creature"]) {
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::Exploits,
            },
            None,
        ));
    }
    if ts.eat_any_phrase(&[&["is", "cast"], &["are", "cast"]]) {
        let spell = match subject {
            SubjectRef::Group(f) | SubjectRef::All(f) | SubjectRef::Each(f) => Some(*f),
            _ => None,
        };
        return Ok((
            TriggerEvent::SpellCast {
                caster: PlayerRef::AnyPlayer,
                spell,
                from_outside_hand: false,
            },
            None,
        ));
    }
    if ts.eat_any_phrase(&[
        &["is", "put", "onto", "the", "stack"],
        &["are", "put", "onto", "the", "stack"],
    ]) {
        return Ok((
            TriggerEvent::ZoneChange {
                from: None,
                to: Some(ZoneKind::Stack),
                subject,
            },
            None,
        ));
    }
    if ts.eat_any_word(&["dies", "die"]) {
        return Ok((TriggerEvent::Dies { subject }, None));
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
        return Ok((
            TriggerEvent::ZoneChange {
                from,
                to: Some(z.zone),
                subject,
            },
            None,
        ));
    }
    if ts.eat_any_phrase(&[
        &["attacks", "and", "isn't", "blocked"],
        &["attack", "and", "aren't", "blocked"],
    ]) {
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::AttacksAndNotBlocked,
            },
            None,
        ));
    }
    if ts.eat_any_word(&["attacks", "attack"]) {
        let mut alone = ts.eat_word("alone");
        let mut attacked_player = None;
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
            let pcp = ts.checkpoint();
            if let Ok(p) = try_parse(ts, parse_player_ref) {
                if ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("who") || w.eq_ignore_ascii_case("and/or")
                }) {
                    ts.restore(pcp);
                } else {
                    attacked_player = Some(p);
                }
            }
        }
        if !alone && ts.eat_word("alone") {
            alone = true;
        }
        let alone_cond = alone.then(|| ConditionExpr::SubjectState {
            subject: Box::new(subject.clone()),
            state: ObjectStateKind::AttackingAlone,
        });
        if ts.eat_any_phrase(&[&["and", "isn't", "blocked"], &["and", "aren't", "blocked"]]) {
            return Ok((
                TriggerEvent::SubjectState {
                    subject,
                    event: SubjectTriggerKind::AttacksAndNotBlocked,
                },
                alone_cond,
            ));
        }
        return Ok((
            TriggerEvent::Attacks {
                subject: Some(subject),
                player: attacked_player,
            },
            alone_cond,
        ));
    }
    if ts.eat_any_phrase(&[&["becomes", "blocked"], &["become", "blocked"]]) {
        let by = if ts.eat_word("by") {
            opt(ts, parse_subject_ref).map(Box::new)
        } else {
            None
        };
        return Ok((TriggerEvent::BecomesBlocked { subject, by }, None));
    }
    if ts.eat_any_word(&["blocks", "block"]) {
        let blocked = if ts.is_eof()
            || matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
            || ts.peek_phrase(&["or", "becomes", "blocked"])
            || ts.peek_phrase(&["or", "become", "blocked"])
        {
            None
        } else {
            opt(ts, parse_subject_ref).map(Box::new)
        };
        return Ok((TriggerEvent::Blocks { subject, blocked }, None));
    }
    if ts.eat_any_phrase(&[
        &["is", "turned", "face", "up"],
        &["are", "turned", "face", "up"],
    ]) {
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::TurnedFaceUp,
            },
            None,
        ));
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
        return Ok((
            TriggerEvent::PlayerObjectEvent {
                player: PlayerRef::AnyPlayer,
                kind: PlayerObjectTriggerKind::TappedForMana,
                filter: Some(flt),
            },
            None,
        ));
    }
    if ts.eat_any_phrase(&[&["becomes", "tapped"], &["become", "tapped"]]) {
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::BecomesTapped,
            },
            None,
        ));
    }
    if ts.eat_any_phrase(&[&["becomes", "untapped"], &["become", "untapped"]]) {
        return Ok((
            TriggerEvent::SubjectState {
                subject,
                event: SubjectTriggerKind::BecomesUntapped,
            },
            None,
        ));
    }
    if ts.eat_any_phrase(&[&["is", "dealt"], &["are", "dealt"]]) {
        let combat_only = ts.eat_word("combat");
        if ts.eat_word("damage") {
            return Ok((
                TriggerEvent::DealtDamage {
                    subject,
                    combat_only,
                },
                None,
            ));
        }
    }
    if ts.eat_any_phrase(&[
        &["becomes", "the", "target", "of"],
        &["become", "the", "target", "of"],
    ]) {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["an", "ability", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok((
                TriggerEvent::BecomesTarget {
                    subject,
                    source: flt,
                },
                None,
            ));
        }
        ts.restore(acp);
        let source = parse_object_filter(ts)?;
        return Ok((TriggerEvent::BecomesTarget { subject, source }, None));
    }
    {
        let dcp = ts.checkpoint();
        if ts.eat_word("deals") || ts.eat_word("deal") {
            let combat_only = ts.eat_word("combat");
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
                                        any_types: CardTypeMask::PLANESWALKER,
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
                        Some(tgt)
                    }
                } else {
                    None
                };
                return Ok((
                    TriggerEvent::DealsDamage {
                        combat_only,
                        source: Some(subject),
                        target,
                    },
                    None,
                ));
            }
        }
        ts.restore(dcp);
    }

    Err(ts.err("expected trigger event verb"))
}
