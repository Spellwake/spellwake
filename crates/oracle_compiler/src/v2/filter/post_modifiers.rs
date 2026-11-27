//! Post-nominal modifiers for [`ObjectFilter`] (controller, owner, zone, stat bounds,
//! counter predicates, keywords, relative clauses).

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation, StatKind,
    SupertypeMask,
};
use crate::v2::ir::{
    AnaphoraRef, FilterFlags, FilterPredicate, ObjectFilter, PlayerRef, PriorActionKind,
    SharedProperty, StatBound, SubjectRef, ValueExpr, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;
use crate::vocab::{Vocabulary, lookup_card_type, lookup_color, lookup_supertype};

use super::{
    eat_phrase_table, parse_counter_kind, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_ref, parse_zone_ref, push_exclude_filter, push_shares_property,
    push_stat_bound,
};

const CTRL_EARLY: &[(&[&str], ControllerRelation)] = &[
    (&["you", "don't", "control"], ControllerRelation::NotYou),
    (&["your", "team", "controls"], ControllerRelation::You),
    (
        &["those", "opponents", "control"],
        ControllerRelation::Opponent,
    ),
    (
        &["that", "opponent", "controls"],
        ControllerRelation::Opponent,
    ),
    (
        &["defending", "player", "controls"],
        ControllerRelation::DefendingPlayer,
    ),
    (
        &["enchanted", "player", "controls"],
        ControllerRelation::Opponent,
    ),
];

const COMBAT_PHRASES: &[(&[&str], CombatState)] = &[
    (&["that", "blocked", "this", "turn"], CombatState::Blocking),
    (
        &["that", "blocked", "this", "combat"],
        CombatState::Blocking,
    ),
    (
        &["it", "blocked", "this", "combat"],
        CombatState::BlockedBySelf,
    ),
    (
        &["it", "blocked", "this", "turn"],
        CombatState::BlockedBySelf,
    ),
    (
        &["that", "was", "blocked", "by", "it", "this", "combat"],
        CombatState::BlockedBySelf,
    ),
    (
        &["that", "attacked", "this", "turn"],
        CombatState::Attacking,
    ),
    (
        &["that", "attacked", "this", "combat"],
        CombatState::Attacking,
    ),
    (&["that's", "attacking", "alone"], CombatState::Attacking),
    (
        &["that", "are", "attacking", "alone"],
        CombatState::Attacking,
    ),
];

const PRIOR_ACTION_MID: &[(&[&str], PriorActionKind)] = &[
    (
        &["returned", "this", "way"],
        PriorActionKind::ReturnedThisWay,
    ),
    (
        &["sacrificed", "this", "way"],
        PriorActionKind::SacrificedThisWay,
    ),
    (
        &["revealed", "this", "way"],
        PriorActionKind::RevealedThisWay,
    ),
];

const CTRL_MID: &[(&[&str], ControllerRelation)] = &[
    (
        &["your", "opponents", "control"],
        ControllerRelation::Opponents,
    ),
    (&["opponents", "control"], ControllerRelation::Opponents),
    (
        &["an", "opponent", "controls"],
        ControllerRelation::Opponent,
    ),
    (
        &["target", "player", "controls"],
        ControllerRelation::TargetPlayer,
    ),
    (
        &["target", "opponent", "controls"],
        ControllerRelation::TargetOpponent,
    ),
    (&["they", "control"], ControllerRelation::ThatPlayer),
    (
        &["that", "player", "controls"],
        ControllerRelation::ThatPlayer,
    ),
    (
        &["the", "chosen", "player", "controls"],
        ControllerRelation::ThatPlayer,
    ),
    (
        &[
            "that",
            "player",
            "or",
            "that",
            "planeswalker's",
            "controller",
            "controls",
        ],
        ControllerRelation::ThatPlayer,
    ),
];

const CHOSEN_PLAYER_PHRASES: &[(&[&str], PlayerRef)] = &[
    (
        &["of", "an", "opponent's", "choice"],
        PlayerRef::AnyOpponent,
    ),
    (&["of", "opponents'", "choice"], PlayerRef::AnyOpponent),
    (&["of", "their", "choice"], PlayerRef::ThatPlayer),
    (&["of", "your", "choice"], PlayerRef::You),
];

const THAT_FLAG_TAIL: &[(&[&str], FilterFlags)] = &[
    (&["are", "equipped"], FilterFlags::EQUIPPED),
    (&["is", "equipped"], FilterFlags::EQUIPPED),
    (&["are", "enchanted"], FilterFlags::ENCHANTED),
    (&["is", "enchanted"], FilterFlags::ENCHANTED),
    (
        &["aren't", "on", "the", "battlefield"],
        FilterFlags::NOT_ON_BATTLEFIELD,
    ),
    (
        &["isn't", "on", "the", "battlefield"],
        FilterFlags::NOT_ON_BATTLEFIELD,
    ),
];

const SHARES_WITH_PROPS: &[(&[&str], SharedProperty)] = &[
    (
        &["shares", "a", "creature", "type", "with"],
        SharedProperty::CreatureType,
    ),
    (
        &["share", "a", "creature", "type", "with"],
        SharedProperty::CreatureType,
    ),
    (&["shares", "a", "color", "with"], SharedProperty::Color),
    (&["share", "a", "color", "with"], SharedProperty::Color),
    (
        &["shares", "a", "card", "type", "with"],
        SharedProperty::CardType,
    ),
    (
        &["share", "a", "card", "type", "with"],
        SharedProperty::CardType,
    ),
    (
        &["shares", "a", "land", "type", "with"],
        SharedProperty::LandType,
    ),
    (
        &["share", "a", "land", "type", "with"],
        SharedProperty::LandType,
    ),
];

const SHARE_SELF_PROPS: &[(&[&str], SharedProperty)] = &[
    (&["share", "a", "land", "type"], SharedProperty::LandType),
    (&["share", "a", "color"], SharedProperty::Color),
    (
        &["share", "a", "creature", "type"],
        SharedProperty::CreatureType,
    ),
];

const PRIOR_ACTION_TAIL: &[(&[&str], PriorActionKind)] = &[
    (
        &["they", "exiled", "this", "way"],
        PriorActionKind::ExiledThisWay,
    ),
    (
        &["you", "exiled", "this", "way"],
        PriorActionKind::ExiledThisWay,
    ),
    (
        &["that", "player", "exiled", "this", "way"],
        PriorActionKind::ExiledThisWay,
    ),
    (&["exiled", "this", "way"], PriorActionKind::ExiledThisWay),
    (
        &["revealed", "this", "way"],
        PriorActionKind::RevealedThisWay,
    ),
    (
        &["destroyed", "this", "way"],
        PriorActionKind::DestroyedThisWay,
    ),
    (&["milled", "this", "way"], PriorActionKind::MilledThisWay),
    (
        &["discarded", "this", "way"],
        PriorActionKind::DiscardedThisWay,
    ),
    (
        &["sacrificed", "this", "way"],
        PriorActionKind::SacrificedThisWay,
    ),
    (&["tapped", "this", "way"], PriorActionKind::TappedThisWay),
    (
        &["returned", "this", "way"],
        PriorActionKind::ReturnedThisWay,
    ),
    (&["created", "this", "way"], PriorActionKind::CreatedThisWay),
    (&["cast", "this", "way"], PriorActionKind::CastThisWay),
];

fn try_eat_counter_kind_on(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> bool {
    let cp = ts.checkpoint();
    if let Ok(ck) = try_parse(ts, parse_counter_kind)
        && ts.eat_any_word(&["counters", "counter"])
    {
        let _ = ts.eat_any_phrase(&[&["on", "them"], &["on", "it"]]);
        flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
        flt.predicates.push(FilterPredicate::HasCounterKind(ck));
        return true;
    }
    ts.restore(cp);
    false
}

fn is_type_color_subtype_word(w: &str, vocab: &Vocabulary) -> bool {
    let nl = w.to_ascii_lowercase();
    lookup_color(&nl).is_some()
        || lookup_supertype(&nl).is_some()
        || lookup_card_type(&nl).is_some()
        || vocab.lookup_subtype(&nl).is_some()
}

fn try_eat_type_color_subtype_conjunct(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> bool {
    let mut subs = Vec::new();
    let mut ctypes = Vec::new();
    let mut added_colors = ColorMask::EMPTY;
    let mut added_supers = SupertypeMask::EMPTY;
    let mut saw_or = false;
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if let Some(sub) = ts.vocab.lookup_subtype(&low) {
            ts.next_word();
            subs.push(sub.to_string());
        } else if let Some(ct) = lookup_card_type(&low) {
            ts.next_word();
            ctypes.push(ct);
        } else if let Some(st) = lookup_supertype(&low) {
            ts.next_word();
            added_supers = added_supers.union(st);
        } else if let Some(col) = lookup_color(&low) {
            ts.next_word();
            added_colors = added_colors.union(col);
        } else {
            break;
        }
        let sep_cp = ts.checkpoint();
        let had_comma = ts.eat_comma();
        if ts.eat_any_word(&["or", "and/or"]) {
            let _ = ts.eat_any_word(&["a", "an"]);
            if ts
                .peek_word()
                .is_some_and(|nw| is_type_color_subtype_word(nw, ts.vocab))
            {
                saw_or = true;
                continue;
            }
            ts.restore(sep_cp);
            break;
        }
        if had_comma
            && ts
                .peek_word()
                .is_some_and(|nw| is_type_color_subtype_word(nw, ts.vocab))
        {
            continue;
        }
        ts.restore(sep_cp);
        break;
    }
    if subs.is_empty() && ctypes.is_empty() && added_colors.is_empty() && added_supers.is_empty() {
        return false;
    }
    if saw_or && subs.len() >= 2 {
        flt.subtypes_match_any = true;
    }
    flt.subtypes.extend(subs);
    flt.any_colors = flt.any_colors.union(added_colors);
    flt.required_supertypes = flt.required_supertypes.union(added_supers);
    for ct in ctypes {
        if saw_or {
            flt.any_types = flt.any_types.union(ct);
        } else {
            flt.all_types = flt.all_types.union(ct);
        }
    }
    let pcp = ts.checkpoint();
    if !(ts.eat_phrase(&["or", "could", "produce"]) && ts.eat_mana_cost().is_some()) {
        ts.restore(pcp);
    }
    true
}

pub(super) fn parse_filter_post_modifiers(
    ts: &mut TokenStream<'_>,
    flt: &mut ObjectFilter,
) -> PResult<()> {
    loop {
        if ts.eat_any_phrase(&[
            &["exiled", "from", "their", "hand", "this", "way"],
            &["exiled", "from", "your", "hand", "this", "way"],
            &["exiled", "from", "that", "player's", "hand", "this", "way"],
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            flt.zones.push(ZoneRef {
                zone: ZoneKind::Hand,
                owner: ZoneOwner::ThatPlayer,
            });
            continue;
        }
        if ts.eat_phrase(&[
            "that",
            "doesn't",
            "have",
            "the",
            "same",
            "name",
            "as",
            "another",
            "permanent",
            "you",
            "control",
        ]) {
            flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
            continue;
        }
        {
            let ncp = ts.checkpoint();
            if ts.eat_any_phrase(&[&["that", "doesn't", "have"], &["that", "don't", "have"]])
                && ts.eat_any_word(&["a", "an"])
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counter", "counters"])
                && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
            {
                push_exclude_filter(
                    flt,
                    ObjectFilter {
                        predicates: vec![FilterPredicate::HasCounterKind(ck)],
                        ..Default::default()
                    },
                );
                continue;
            }
            ts.restore(ncp);
        }
        if ts.eat_any_phrase(&[
            &["not", "cast", "this", "way"],
            &["not", "put", "onto", "the", "battlefield", "this", "way"],
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            continue;
        }
        // `except for <type/subtype/token list>`
        {
            let exc_cp = ts.checkpoint();
            if ts.eat_phrase(&["except", "for"]) {
                let mut ex_flt = ObjectFilter::default();
                let mut saw_any = false;
                loop {
                    let _ = ts.eat_any_word(&["a", "an"]);
                    if ts.eat_any_word(&["tokens", "token"]) {
                        ex_flt.token = Some(true);
                        saw_any = true;
                    } else if let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(ct) = lookup_card_type(&low) {
                            ts.next_word();
                            ex_flt.any_types = ex_flt.any_types.union(ct);
                            saw_any = true;
                        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            ex_flt.subtypes.push(sub.to_string());
                            saw_any = true;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                    let sep_cp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let had_conj = ts.eat_any_word(&["and", "or", "and/or"]);
                    if !had_comma && !had_conj {
                        ts.restore(sep_cp);
                        break;
                    }
                }
                if saw_any {
                    push_exclude_filter(flt, ex_flt);
                    continue;
                }
            }
            ts.restore(exc_cp);
        }
        if ts.eat_any_phrase(&[
            &["that", "was", "exiled", "with"],
            &["that", "were", "exiled", "with"],
        ]) && ts.eat_tilde()
        {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }
        if ts.eat_phrase(&["that", "didn't", "attack", "this", "turn"]) {
            push_exclude_filter(
                flt,
                ObjectFilter {
                    flags: FilterFlags::ATTACKED_THIS_TURN,
                    ..Default::default()
                },
            );
            continue;
        }
        if ts.eat_any_phrase(&[
            &["with", "an", "activated", "ability"],
            &["with", "activated", "abilities"],
        ]) {
            flt.is_ability = true;
            continue;
        }
        // `with <kw> abilities`
        {
            let kw_cp = ts.checkpoint();
            if ts.eat_word("with")
                && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
                && ts.eat_any_word(&["abilities", "ability"])
            {
                flt.predicates.push(FilterPredicate::HasNamedAbility(kw));
                continue;
            }
            ts.restore(kw_cp);
        }
        // `with total power/toughness/mana value <val> or less/greater`
        {
            let tp_cp = ts.checkpoint();
            if ts.eat_phrase(&["with", "total"]) {
                const TOTAL_STATS: &[(&[&str], StatKind)] = &[
                    (&["power"], StatKind::Power),
                    (&["toughness"], StatKind::Toughness),
                    (&["mana", "value"], StatKind::ManaValue),
                ];
                if let Some(stat) = eat_phrase_table(ts, TOTAL_STATS)
                    && let Ok(val) = try_parse(ts, parse_value_expr)
                {
                    let op = if ts.eat_phrase(&["or", "less"]) {
                        CompareOp::Le
                    } else if ts.eat_phrase(&["or", "greater"]) {
                        CompareOp::Ge
                    } else {
                        CompareOp::Eq
                    };
                    push_stat_bound(flt, stat, op, val);
                    continue;
                }
            }
            ts.restore(tp_cp);
        }
        // Controller / owner clauses
        if let Some(ctrl) = eat_phrase_table(ts, CTRL_EARLY) {
            flt.controller = Some(ctrl);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["that's", "enchanted", "or", "equipped"],
            &["that", "is", "enchanted", "or", "equipped"],
            &["that", "are", "enchanted", "or", "equipped"],
            &["that's", "equipped", "or", "enchanted"],
            &["that", "are", "equipped", "or", "enchanted"],
        ]) {
            flt.predicates.push(FilterPredicate::AnyOf(vec![
                ObjectFilter {
                    flags: FilterFlags::ENCHANTED,
                    ..Default::default()
                },
                ObjectFilter {
                    flags: FilterFlags::EQUIPPED,
                    ..Default::default()
                },
            ]));
            continue;
        }
        if ts.eat_phrase(&["on", "the", "battlefield"]) {
            flt.zones.push(ZoneRef {
                zone: ZoneKind::Battlefield,
                owner: ZoneOwner::Default,
            });
            if ts.eat_phrase(&["or", "in", "the", "command", "zone"]) {
                flt.zones.push(ZoneRef {
                    zone: ZoneKind::CommandZone,
                    owner: ZoneOwner::Default,
                });
            }
            continue;
        }
        if ts.eat_any_phrase(&[&["you", "exiled"], &["you've", "exiled"]]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["at", "the", "beginning", "of", "this", "turn"]) {
            flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
            continue;
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_word("attacking")
                && let Ok(p) = try_parse(ts, parse_player_ref)
            {
                flt.combat = Some(CombatState::Attacking);
                flt.predicates
                    .push(FilterPredicate::AttackedPlayerThisTurn(p));
                continue;
            }
            ts.restore(acp);
        }
        if ts.eat_phrase(&["you've", "cast", "this", "turn"]) {
            flt.controller = Some(ControllerRelation::You);
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CastThisWay,
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &["you've", "drawn", "this", "turn"],
            &["you", "drew", "this", "turn"],
        ]) {
            flt.predicates.push(FilterPredicate::DrawnThisTurn);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["you've", "discarded", "this", "turn"],
            &["you", "discarded", "this", "turn"],
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["blocking", "or", "blocked", "by"])
            && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
        {
            flt.combat = Some(CombatState::Blocking);
            flt.predicates
                .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
            continue;
        }
        {
            let bcp = ts.checkpoint();
            if let Some(combat) = eat_phrase_table(
                ts,
                &[
                    (&["blocking"], CombatState::Blocking),
                    (&["blocked", "by"], CombatState::Blocked),
                ],
            ) && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.combat = Some(combat);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
                continue;
            }
            ts.restore(bcp);
        }
        if ts.eat_any_phrase(&[&["chosen", "at", "random"], &["at", "random"]]) {
            flt.flags = flt.flags.union(FilterFlags::AT_RANDOM);
            continue;
        }
        // `with the same <name|mana value|power|toughness|total power and toughness> as <subj>`
        {
            let scp = ts.checkpoint();
            if ts.eat_phrase(&["with", "the", "same"]) {
                const SAME_PROPS: &[(&[&str], SharedProperty)] = &[
                    (&["name"], SharedProperty::Name),
                    (&["mana", "value"], SharedProperty::ManaValue),
                    (
                        &["total", "power", "and", "toughness"],
                        SharedProperty::Power,
                    ),
                    (
                        &["power", "and/or", "same", "toughness"],
                        SharedProperty::Power,
                    ),
                    (&["power", "and/or", "toughness"], SharedProperty::Power),
                    (&["power"], SharedProperty::Power),
                    (&["toughness"], SharedProperty::Toughness),
                ];
                if let Some(property) = eat_phrase_table(ts, SAME_PROPS) {
                    let reference = if ts.eat_word("as") {
                        try_parse(ts, parse_subject_ref).ok()
                    } else {
                        Some(SubjectRef::Anaphora(AnaphoraRef::It))
                    };
                    if let Some(reference) = reference {
                        push_shares_property(flt, property, reference, false);
                        continue;
                    }
                }
            }
            ts.restore(scp);
        }
        if ts.eat_phrase(&["with", "that", "spell's", "mana", "value"]) {
            push_stat_bound(
                flt,
                StatKind::ManaValue,
                CompareOp::Eq,
                ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatSpell)),
                    stat: StatKind::ManaValue,
                },
            );
            continue;
        }
        if let Some(flag) = eat_phrase_table(
            ts,
            &[
                (
                    &["with", "another", "aura", "attached", "to", "it"],
                    FilterFlags::ENCHANTED,
                ),
                (
                    &["with", "one", "or", "more", "targets"],
                    FilterFlags::SINGLE_TARGET,
                ),
            ],
        ) {
            flt.flags = flt.flags.union(flag);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["from", "among", "cards", "revealed", "this", "way"],
            &["from", "among", "cards", "exiled", "this", "way"],
            &["from", "among", "those", "exiled", "cards"],
            &["from", "among", "those", "cards"],
            &["from", "among", "them"],
        ]) {
            flt.predicates.push(FilterPredicate::AmongPriorCards);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["that", "player", "milled", "this", "way"],
            &["they", "milled", "this", "way"],
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::MilledThisWay,
            ));
            continue;
        }
        // `from [a/an] <source filter>`
        {
            let fcp = ts.checkpoint();
            if ts.eat_word("from")
                && try_parse(ts, parse_zone_ref).is_err()
                && !ts.peek_phrase(&["among"])
                && !ts.peek_phrase(&["anywhere"])
                && let Ok(src_flt) = try_parse(ts, parse_object_filter)
                && (src_flt.any_permanent
                    || !src_flt.any_colors.is_empty()
                    || !src_flt.forbidden_colors.is_empty()
                    || !src_flt.any_types.is_empty()
                    || !src_flt.all_types.is_empty()
                    || !src_flt.forbidden_types.is_empty()
                    || src_flt.color_arity.is_some())
            {
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(
                        SubjectRef::Group(Box::new(src_flt)),
                    )));
                continue;
            }
            ts.restore(fcp);
        }
        {
            let bcp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["that", "blocked", "or", "was", "blocked", "by"],
                &["that", "blocked", "or", "were", "blocked", "by"],
            ]) && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "combat"]]);
                flt.combat = Some(CombatState::Blocking);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
                continue;
            }
            ts.restore(bcp);
        }
        if let Some(combat) = eat_phrase_table(ts, COMBAT_PHRASES) {
            flt.combat = Some(combat);
            continue;
        }
        // `that were/was put into <z1> from <z2> this turn`
        {
            let gy_cp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["that", "were", "put", "into"],
                &["that", "was", "put", "into"],
            ]) && let Ok(z1) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("from")
                && (ts.eat_word("anywhere") || try_parse(ts, parse_zone_ref).is_ok())
                && ts.eat_phrase(&["this", "turn"])
            {
                flt.zones.push(z1);
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                continue;
            }
            ts.restore(gy_cp);
        }
        if ts.eat_phrase(&["controlled", "by", "different", "players"]) {
            flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
            continue;
        }
        if ts.eat_phrase(&["goaded", "this", "way"]) {
            flt.flags = flt.flags.union(FilterFlags::GOADED);
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ThatMany,
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &["that", "were", "milled", "this", "turn"],
            &["milled", "this", "turn"],
            &[
                "that", "were", "put", "there", "from", "your", "library", "this", "turn",
            ],
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::MilledThisWay,
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &["from", "anywhere", "other", "than", "your", "hand"],
            &["from", "anywhere", "other", "than", "their", "hand"],
            &["from", "anywhere", "other", "than", "exile"],
        ]) {
            flt.predicates.push(FilterPredicate::FromOutsideHand);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["that's", "one", "or", "more", "colors"],
            &["that", "are", "one", "or", "more", "colors"],
        ]) {
            flt.color_arity = Some(ColorArity::Colored);
            continue;
        }
        if ts.eat_phrase(&["in", "your", "party"]) {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if ts.eat_phrase(&["of", "that", "type"]) {
            flt.predicates.push(FilterPredicate::OfChosenCreatureType);
            continue;
        }
        if ts.eat_phrase(&["of", "that", "color"]) {
            flt.predicates.push(FilterPredicate::OfThatColor);
            continue;
        }
        if ts.eat_phrase(&["you", "both", "own", "and", "control"]) {
            flt.controller = Some(ControllerRelation::You);
            flt.owner = Some(ControllerRelation::OwnerYou);
            flt.predicates
                .push(FilterPredicate::OwnedAndControlledByYou);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["you", "own", "that", "your", "opponents", "control"],
            &["you", "own", "that", "an", "opponent", "controls"],
        ]) {
            flt.owner = Some(ControllerRelation::OwnerYou);
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["controlled", "by", "the", "same", "opponent"],
            &["controlled", "by", "the", "same", "player"],
        ]) {
            flt.predicates.push(FilterPredicate::SameController);
            continue;
        }
        if ts.eat_phrase(&["you", "control"]) {
            flt.controller = Some(ControllerRelation::You);
            if ts.eat_phrase(&["but", "don't", "own"]) {
                flt.owner = Some(ControllerRelation::Opponent);
            }
            continue;
        }
        if ts.eat_phrase(&["you", "cast"]) {
            flt.controller = Some(ControllerRelation::You);
            if ts.peek_phrase(&["this", "turn"])
                && ts.peek_n(2).is_some_and(|t| {
                    matches!(
                        t.kind,
                        TokenKind::Word(w)
                            if w.eq_ignore_ascii_case("that") || w.eq_ignore_ascii_case("of")
                    )
                })
            {
                ts.eat_phrase(&["this", "turn"]);
            }
            if ts.eat_phrase(&["but", "don't", "own"]) {
                flt.owner = Some(ControllerRelation::Opponent);
            }
            continue;
        }
        {
            let ocp = ts.checkpoint();
            if ts.eat_phrase(&["other", "than"]) {
                if ts.eat_tilde() {
                    flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
                    continue;
                }
                let blocked_zone_or_ordinal = ts.peek_phrase(&["the", "first"])
                    || ts.peek_phrase(&["your", "first"])
                    || ts.peek_phrase(&["the", "battlefield"])
                    || ts.peek_phrase(&["your", "hand"])
                    || ts.peek_phrase(&["their", "hand"])
                    || ts.peek_phrase(&["a", "basic", "land", "card", "name"])
                    || ts
                        .peek_word()
                        .is_some_and(|w| w.eq_ignore_ascii_case("exile"));
                if !blocked_zone_or_ordinal {
                    if let Ok(excl) = try_parse(ts, parse_object_filter) {
                        push_exclude_filter(flt, excl);
                        let or_cp = ts.checkpoint();
                        if ts.eat_word("or") && ts.eat_tilde() {
                            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
                        } else {
                            ts.restore(or_cp);
                        }
                        continue;
                    }
                    if let Ok(excl_subj) = try_parse(ts, parse_subject_ref) {
                        flt.predicates
                            .push(FilterPredicate::Exclude(Box::new(excl_subj)));
                        let or_cp = ts.checkpoint();
                        if ts.eat_word("or") {
                            if ts.eat_tilde() {
                                flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
                            } else if let Ok(excl_subj2) = try_parse(ts, parse_subject_ref) {
                                flt.predicates
                                    .push(FilterPredicate::Exclude(Box::new(excl_subj2)));
                            } else {
                                ts.restore(or_cp);
                            }
                        }
                        continue;
                    }
                }
            }
            ts.restore(ocp);
        }
        if ts.eat_phrase(&["not", "named"]) && ts.eat_tilde() {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            continue;
        }
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("named"))
        {
            let ncp = ts.checkpoint();
            ts.eat_word("named");
            if ts.eat_tilde() {
                flt.predicates.push(FilterPredicate::NamedSelf);
                continue;
            }
            if let Some(names) = parse_proper_card_names(ts) {
                for name in names {
                    flt.predicates.push(FilterPredicate::Named(name));
                }
                continue;
            }
            ts.restore(ncp);
        }
        if ts.eat_phrase(&["dealt", "damage", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DealtDamageThisWay,
            ));
            continue;
        }
        {
            let dd_cp = ts.checkpoint();
            if ts.eat_any_phrase(&[&["dealt", "damage"], &["dealt", "combat", "damage"]]) {
                let saw_turn_first = ts.eat_phrase(&["this", "turn"]);
                if ts.eat_word("by") {
                    let is_self = ts.eat_tilde();
                    let matched_src = is_self || try_parse(ts, parse_subject_ref).is_ok();
                    let saw_turn_after = ts.eat_phrase(&["this", "turn"]);
                    if matched_src && (saw_turn_first || saw_turn_after) {
                        flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
                        if is_self {
                            flt.predicates
                                .push(FilterPredicate::DealtDamageBySelfThisTurn);
                        }
                        continue;
                    }
                }
            }
            ts.restore(dd_cp);
        }
        if ts.eat_any_phrase(&[&["your", "opponents", "cast"], &["opponents", "cast"]]) {
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "casts"]) {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["you", "control", "but", "don't", "own"],
            &["you", "cast", "but", "don't", "own"],
        ]) {
            flt.controller = Some(ControllerRelation::You);
            flt.owner = Some(ControllerRelation::NotYou);
            continue;
        }
        if let Some(owner) = eat_phrase_table(
            ts,
            &[
                (&["you", "don't", "own"], ControllerRelation::NotYou),
                (&["you", "own"], ControllerRelation::OwnerYou),
                (&["an", "opponent", "owns"], ControllerRelation::Opponent),
                (&["opponents", "own"], ControllerRelation::Opponent),
            ],
        ) {
            flt.owner = Some(owner);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["attacking", "you"],
            &["attacking", "an", "opponent"],
            &["attacking", "that", "player"],
        ]) {
            flt.combat = Some(CombatState::Attacking);
            continue;
        }
        if ts.eat_phrase(&["you", "controlled", "as", "you", "cast"]) && ts.eat_tilde() {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde())
            || ts.eat_phrase(&["as", "you", "activate", "this", "ability"])
        {
            continue;
        }
        if let Some(kind) = eat_phrase_table(ts, PRIOR_ACTION_MID) {
            flt.predicates
                .push(FilterPredicate::AffectedByPriorAction(kind));
            continue;
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_comma()
                && ts.eat_phrase(&["activated", "ability"])
                && ts.eat_comma()
                && ts.eat_phrase(&["or", "triggered", "ability"])
            {
                flt.is_ability = true;
                continue;
            }
            ts.restore(acp);
            if ts.eat_any_phrase(&[
                &["from", "an", "artifact", "or", "enchantment", "source"],
                &["from", "an", "artifact", "source"],
                &["from", "a", "creature", "source"],
                &["from", "a", "noncreature", "source"],
            ]) {
                flt.is_ability = true;
                continue;
            }
            ts.restore(acp);
        }
        {
            let pcp = ts.checkpoint();
            if ts.eat_phrase(&["put", "into"])
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
            {
                if ts.eat_phrase(&["this", "way"]) {
                    flt.zones.push(zr);
                    flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                        PriorActionKind::ReturnedThisWay,
                    ));
                    continue;
                }
                if ts.eat_phrase(&["from", "the", "battlefield", "this", "turn"]) {
                    flt.zones.push(zr);
                    flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                    continue;
                }
            }
            ts.restore(pcp);
        }
        {
            let scp = ts.checkpoint();
            if ts.eat_phrase(&["with", "the", "same", "name", "as"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                push_shares_property(flt, SharedProperty::Name, ref_subj, false);
                continue;
            }
            ts.restore(scp);
        }
        if ts.eat_any_phrase(&[&["they", "own"], &["that", "player", "owns"]]) {
            flt.owner = Some(ControllerRelation::OwnerThatPlayer);
            continue;
        }
        if let Some(ctrl) = eat_phrase_table(ts, CTRL_MID) {
            flt.controller = Some(ctrl);
            continue;
        }
        if ts.eat_phrase(&["they", "controlled"]) {
            flt.controller = Some(ControllerRelation::ThatPlayer);
            flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
            continue;
        }
        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
            flt.predicates.push(FilterPredicate::OfChosenColor);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["of", "the", "creature", "type", "of", "your", "choice"],
            &["of", "the", "chosen", "creature", "type"],
            &["of", "the", "chosen", "type"],
        ]) {
            flt.predicates.push(FilterPredicate::OfChosenCreatureType);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["in", "the", "pile", "of", "their", "choice"],
            &["in", "the", "pile", "of", "your", "choice"],
        ]) {
            flt.predicates.push(FilterPredicate::InChosenPile);
            continue;
        }
        if ts.eat_phrase(&["drawn", "this", "turn"]) {
            flt.predicates.push(FilterPredicate::DrawnThisTurn);
            continue;
        }
        if let Some(p) = eat_phrase_table(ts, CHOSEN_PLAYER_PHRASES) {
            flt.predicates.push(FilterPredicate::OfChosenPlayer(p));
            continue;
        }
        if ts.eat_phrase(&["exiled", "with"]) && (ts.eat_tilde() || ts.eat_word("it")) {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["from", "among", "the", "cards", "milled", "this", "way"],
            &["from", "among", "the", "cards", "exiled", "this", "way"],
            &["from", "among", "the", "cards", "revealed", "this", "way"],
            &["among", "the", "cards", "milled", "this", "way"],
            &["among", "the", "cards", "exiled", "this", "way"],
            &["among", "the", "cards", "revealed", "this", "way"],
            &["from", "among", "them"],
            &["from", "among", "those", "cards"],
            &["from", "among", "the", "milled", "cards"],
            &["from", "among", "the", "exiled", "cards"],
            &["from", "among", "the", "revealed", "cards"],
            &["from", "among", "the", "copies"],
            &["among", "them"],
            &["from", "it"],
            &["among", "those", "cards"],
        ]) {
            flt.predicates.push(FilterPredicate::AmongPriorCards);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["from", "among", "cards", "exiled", "with"],
            &["among", "cards", "exiled", "with"],
        ]) && (ts.eat_tilde() || ts.eat_word("it"))
        {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }

        // Attached-to relative clause
        if ts.eat_any_phrase(&[&["that's", "attached", "to"], &["attached", "to"]]) {
            let host = parse_subject_ref(ts)?;
            flt.predicates
                .push(FilterPredicate::AttachedTo(Box::new(host)));
            continue;
        }

        // `without a/an [<kind>] counter(s) on it`
        {
            let wcp = ts.checkpoint();
            if ts.eat_word("without") {
                let _ = ts.eat_any_word(&["a", "an"]);
                let _ = opt(ts, parse_counter_kind);
                if ts.eat_any_word(&["counter", "counters"])
                    && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
                {
                    flt.flags = flt.flags.union(FilterFlags::WITHOUT_COUNTERS);
                    continue;
                }
            }
            ts.restore(wcp);
        }
        // `from anywhere other than their/your hand(s)`
        if ts.eat_any_phrase(&[
            &["from", "anywhere", "other", "than", "their", "hands"],
            &["from", "anywhere", "other", "than", "their", "hand"],
            &["from", "anywhere", "other", "than", "your", "hand"],
        ]) {
            flt.predicates.push(FilterPredicate::FromOutsideHand);
            continue;
        }
        if ts.eat_phrase(&[
            "that",
            "were",
            "put",
            "there",
            "from",
            "anywhere",
            "other",
            "than",
            "the",
            "battlefield",
            "this",
            "turn",
        ]) {
            flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
            continue;
        }
        // `without <kw>` (e.g. `creature without flying`) -- MUST checkpoint so `without paying its mana cost` is never eaten!
        {
            let wcp = ts.checkpoint();
            if ts.eat_word("without")
                && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            {
                flt.forbidden_keywords
                    .extend(kws.into_iter().map(|k| k.kind));
                continue;
            }
            ts.restore(wcp);
        }

        // Stat bounds & `with different names` & `with <kw>` & `with <counter>` (supports optional `each with ...`)
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with") || w.eq_ignore_ascii_case("each"))
        {
            let cp = ts.checkpoint();
            let _ = ts.eat_word("each");
            if !ts.eat_word("with") {
                ts.restore(cp);
                break;
            }
            if let Some(or_equal) = eat_phrase_table(
                ts,
                &[
                    (&["equal", "or", "lesser", "mana", "value"], true),
                    (&["lesser", "mana", "value"], false),
                ],
            ) {
                flt.predicates
                    .push(FilterPredicate::LesserManaValue { or_equal });
                continue;
            }
            if ts.eat_any_phrase(&[&["the", "chosen", "name"], &["that", "name"]]) {
                flt.predicates.push(FilterPredicate::OfChosenName);
                continue;
            }
            if let Some(flag) = eat_phrase_table(
                ts,
                &[
                    (&["different", "names"], FilterFlags::DIFFERENT_NAMES),
                    (&["different", "powers"], FilterFlags::DIFFERENT_POWERS),
                ],
            ) {
                flt.flags = flt.flags.union(flag);
                continue;
            }
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(crate::v2::ir::ManaSymbol::VariableX))
            ) {
                let xcp = ts.checkpoint();
                ts.next_tok();
                if ts.eat_any_phrase(&[
                    &["in", "its", "mana", "cost"],
                    &["in", "its", "mana", "costs"],
                    &["in", "their", "mana", "cost"],
                    &["in", "their", "mana", "costs"],
                ]) {
                    flt.flags = flt.flags.union(FilterFlags::HAS_X_IN_MANA_COST);
                    continue;
                }
                ts.restore(xcp);
            }
            if ts.eat_word("its")
                && let Some(mc) = ts.eat_mana_cost()
                && ts.eat_word("kicker")
            {
                flt.predicates.push(FilterPredicate::KickedWith(mc));
                continue;
            }
            if ts.eat_phrase(&["the", "same", "name", "as"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                push_shares_property(flt, SharedProperty::Name, ref_subj, false);
                continue;
            }
            if ts.eat_phrase(&["the", "same", "name"]) {
                push_shares_property(flt, SharedProperty::Name, SubjectRef::SelfObject, false);
                continue;
            }
            if ts.eat_phrase(&["the", "same", "controller"]) {
                flt.predicates.push(FilterPredicate::SameController);
                continue;
            }
            if ts.eat_phrase(&["the", "same", "mana", "value"]) {
                let ref_subj = if ts.eat_word("as") {
                    try_parse(ts, parse_subject_ref).unwrap_or(SubjectRef::SelfObject)
                } else {
                    SubjectRef::SelfObject
                };
                push_shares_property(flt, SharedProperty::ManaValue, ref_subj, false);
                continue;
            }
            if let Some(op) = eat_phrase_table(
                ts,
                &[
                    (&["greater", "power"], CompareOp::Gt),
                    (&["lesser", "power"], CompareOp::Lt),
                ],
            ) {
                push_stat_bound(
                    flt,
                    StatKind::Power,
                    op,
                    ValueExpr::ObjectStat {
                        subject: Box::new(SubjectRef::SelfObject),
                        stat: StatKind::Power,
                    },
                );
                continue;
            }
            if let Some(is_greatest) = eat_phrase_table(
                ts,
                &[(&["the", "greatest"], true), (&["the", "least"], false)],
            ) && let Some(stat) = parse_single_stat_kind(ts)
            {
                if is_greatest
                    && ts.eat_any_phrase(&[
                        &["or", "tied", "for", "the", "greatest"],
                        &["or", "tied", "for", "greatest"],
                    ])
                {
                    let _ = parse_single_stat_kind(ts);
                }
                let scope = Box::new(if ts.eat_word("among") {
                    try_parse(ts, parse_object_filter).unwrap_or_default()
                } else {
                    ObjectFilter::default()
                });
                flt.predicates.push(if is_greatest {
                    FilterPredicate::GreatestStat { stat, scope }
                } else {
                    FilterPredicate::LeastStat { stat, scope }
                });
                continue;
            }
            if ts.eat_any_phrase(&[
                &["a", "name", "sticker", "on", "it"],
                &["a", "name", "sticker", "on", "them"],
            ]) {
                flt.predicates.push(FilterPredicate::HasNameSticker);
                continue;
            }
            if ts.eat_phrase(&["a", "basic", "land", "type"]) {
                flt.predicates.push(FilterPredicate::HasBasicLandType);
                continue;
            }
            if ts.eat_any_phrase(&[&["a", "mana", "ability"], &["mana", "abilities"]]) {
                flt.predicates.push(FilterPredicate::HasManaAbility);
                continue;
            }
            {
                let abcp = ts.checkpoint();
                if ts.eat_any_word(&["a", "an"])
                    && let Ok(kw) = try_parse(ts, crate::v2::keywords::parse_keyword_kind)
                    && ts.eat_word("ability")
                {
                    flt.predicates.push(FilterPredicate::HasNamedAbility(kw));
                    continue;
                }
                ts.restore(abcp);
            }
            if ts.eat_phrase(&["a", "single", "target"]) {
                flt.flags = flt.flags.union(FilterFlags::SINGLE_TARGET);
                continue;
            }
            // `with counters on them/it` / `with <kind> counters on them/it` (bare plural without article!)
            {
                let ccp = ts.checkpoint();
                if ts.eat_word("counters") && ts.eat_any_phrase(&[&["on", "them"], &["on", "it"]]) {
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    continue;
                }
                ts.restore(ccp);
                if try_eat_counter_kind_on(ts, flt) {
                    continue;
                }
            }
            // `with <N> or more <kind> counters on it/them`
            {
                let ncp = ts.checkpoint();
                if ts.next_number(false).is_some()
                    && ts.eat_phrase(&["or", "more"])
                    && try_eat_counter_kind_on(ts, flt)
                {
                    continue;
                }
                ts.restore(ncp);
            }
            // `with a [kind] counter on it` / `with one or more [kind] counters on it`
            if ts.eat_any_phrase(&[&["one", "or", "more"], &["a"], &["an"]]) {
                if ts.eat_any_word(&["counter", "counters"]) {
                    let _ = ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    continue;
                }
                if try_eat_counter_kind_on(ts, flt) {
                    continue;
                }
                ts.restore(cp);
                let _ = ts.eat_word("each");
                ts.eat_word("with");
            }
            if ts.eat_phrase(&["mana", "cost"]) {
                let mut costs = Vec::new();
                if let Some(first_mc) = ts.eat_mana_cost() {
                    costs.push(first_mc);
                    while ts.eat_word("or") {
                        if let Some(next_mc) = ts.eat_mana_cost() {
                            costs.push(next_mc);
                        } else {
                            break;
                        }
                    }
                    flt.predicates.push(FilterPredicate::ManaCostIn(costs));
                    continue;
                }
            }
            // `with [base] power and toughness <P>/<T>`
            if ts.eat_any_phrase(&[
                &["base", "power", "and", "toughness"],
                &["power", "and", "toughness"],
            ]) && let Some((p, t)) = ts.next_base_pt()
            {
                push_stat_bound(flt, StatKind::Power, CompareOp::Eq, ValueExpr::Const(p));
                push_stat_bound(flt, StatKind::Toughness, CompareOp::Eq, ValueExpr::Const(t));
                continue;
            }
            // `with <stat1>[, <stat2>][,] [or <stat3>] <cmp> <val>`
            if let Some(first_stat) = parse_single_stat_kind(ts) {
                let mut stats = vec![first_stat];
                loop {
                    let scp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let had_or = ts.eat_word("or");
                    if (had_comma || had_or)
                        && let Some(next_stat) = parse_single_stat_kind(ts)
                    {
                        stats.push(next_stat);
                        continue;
                    }
                    ts.restore(scp);
                    break;
                }
                if let Ok((op, val)) = try_parse(ts, parse_stat_comparison) {
                    if stats.len() == 1 {
                        push_stat_bound(flt, stats[0], op, val);
                    } else {
                        let branches = stats
                            .into_iter()
                            .map(|stat| ObjectFilter {
                                stat_bounds: vec![StatBound {
                                    stat,
                                    op,
                                    value: Box::new(val.clone()),
                                }],
                                ..Default::default()
                            })
                            .collect();
                        flt.predicates.push(FilterPredicate::AnyOf(branches));
                    }
                    continue;
                }
            }
            // `with flying` / `with flying or reach`
            if let Ok((kws, is_or)) = try_parse(ts, parse_keyword_list) {
                if is_or && kws.len() >= 2 {
                    flt.keywords_match_any = true;
                }
                flt.required_keywords
                    .extend(kws.into_iter().map(|k| k.kind));
                continue;
            }
            ts.restore(cp);
        }

        // Relative clauses starting with `that's` (`that's a Food or a Golem`, `that's both red and white`)
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("that's"))
        {
            let cp = ts.checkpoint();
            ts.eat_word("that's");
            if ts.eat_word("both") {
                let mut colors = ColorMask::EMPTY;
                if let Some(w1) = ts.peek_word()
                    && let Some(c1) = lookup_color(&w1.to_ascii_lowercase())
                {
                    ts.next_word();
                    colors = colors.union(c1);
                    if ts.eat_word("and")
                        && let Some(w2) = ts.peek_word()
                        && let Some(c2) = lookup_color(&w2.to_ascii_lowercase())
                    {
                        ts.next_word();
                        colors = colors.union(c2);
                        flt.any_colors = flt.any_colors.union(colors);
                        flt.flags = flt.flags.union(FilterFlags::BOTH_COLORS);
                        continue;
                    }
                }
                ts.restore(cp);
            } else {
                let _ = ts.eat_any_word(&["a", "an"]);
                if try_eat_type_color_subtype_conjunct(ts, flt) {
                    continue;
                }
                ts.restore(cp);
            }
        }

        // Relative clauses starting with `[or] that`
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("that"))
            || ts.peek_phrase(&["or", "that"])
        {
            let cp = ts.checkpoint();
            let _ = ts.eat_word("or");
            ts.eat_word("that");
            if ts.eat_any_phrase(&[
                &[
                    "was", "put", "there", "from", "your", "library", "this", "turn",
                ],
                &[
                    "were", "put", "there", "from", "your", "library", "this", "turn",
                ],
                &[
                    "was", "put", "there", "from", "their", "library", "this", "turn",
                ],
                &[
                    "were", "put", "there", "from", "their", "library", "this", "turn",
                ],
                &[
                    "was", "put", "there", "from", "a", "library", "this", "turn",
                ],
                &["was", "put", "there", "this", "turn"],
                &["were", "put", "there", "this", "turn"],
            ]) {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::MilledThisWay,
                ));
                continue;
            }
            if ts.eat_any_phrase(&[&["has", "a"], &["has", "an"]])
                && try_eat_counter_kind_on(ts, flt)
            {
                continue;
            }
            if ts.eat_any_phrase(&[
                &["weren't", "cast", "this", "way"],
                &["wasn't", "cast", "this", "way"],
                &["weren't", "cast"],
                &["wasn't", "cast"],
                &["weren't", "put", "onto", "the", "battlefield"],
                &[
                    "weren't",
                    "put",
                    "onto",
                    "the",
                    "battlefield",
                    "this",
                    "way",
                ],
                &["weren't", "put", "there", "this", "way"],
                &["weren't", "put", "into", "your", "hand"],
                &["weren't", "chosen"],
            ]) {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
                continue;
            }
            if ts.eat_any_phrase(&[
                &["isn't", "a", "mana", "ability"],
                &["aren't", "mana", "abilities"],
            ]) {
                flt.predicates.push(FilterPredicate::NotManaAbility);
                continue;
            }
            if ts.eat_any_phrase(&[&["isn't", "legendary"], &["aren't", "legendary"]]) {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
                continue;
            }
            if ts.eat_any_phrase(&[&["isn't", "all", "colors"], &["aren't", "all", "colors"]]) {
                flt.color_arity = Some(ColorArity::Monocolored);
                continue;
            }
            {
                let neg_cp = ts.checkpoint();
                if ts.eat_any_phrase(&[&["isn't", "a"], &["isn't", "an"], &["isn't"], &["aren't"]])
                {
                    let mut f_subs = Vec::new();
                    let mut f_types = CardTypeMask::EMPTY;
                    while let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            f_subs.push(sub.to_string());
                        } else if let Some(ct) = lookup_card_type(&low) {
                            ts.next_word();
                            f_types = f_types.union(ct);
                        } else {
                            break;
                        }
                        let _ = ts.eat_comma();
                        if ts.eat_any_word(&["or", "and/or"]) {
                            let _ = ts.eat_any_word(&["a", "an"]);
                        }
                    }
                    if !f_subs.is_empty() || !f_types.is_empty() {
                        flt.forbidden_subtypes.extend(f_subs);
                        flt.forbidden_types = flt.forbidden_types.union(f_types);
                        continue;
                    }
                }
                ts.restore(neg_cp);
            }
            if ts.eat_any_phrase(&[
                &["were", "exiled", "this", "way"],
                &["was", "exiled", "this", "way"],
            ]) {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
                continue;
            }
            if ts.eat_any_phrase(&[
                &["was", "dealt", "damage", "this", "turn"],
                &["were", "dealt", "damage", "this", "turn"],
                &["has", "been", "dealt", "damage", "this", "turn"],
                &["have", "been", "dealt", "damage", "this", "turn"],
                &["dealt", "damage", "to", "you", "this", "turn"],
                &["dealt", "combat", "damage", "to", "you", "this", "turn"],
                &["dealt", "damage", "to", "a", "player", "this", "turn"],
                &["dealt", "damage", "this", "turn"],
            ]) {
                flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
                continue;
            }
            if ts.eat_any_phrase(&[
                &["entered", "the", "battlefield", "this", "turn"],
                &["entered", "this", "turn"],
            ]) {
                flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["died", "under", "your", "control", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                flt.controller = Some(ControllerRelation::You);
                continue;
            }
            if ts.eat_any_phrase(&[
                &["died", "this", "turn"],
                &[
                    "were",
                    "put",
                    "there",
                    "from",
                    "the",
                    "battlefield",
                    "this",
                    "turn",
                ],
                &[
                    "was",
                    "put",
                    "there",
                    "from",
                    "the",
                    "battlefield",
                    "this",
                    "turn",
                ],
            ]) {
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["attacked", "you", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::ATTACKED_THIS_TURN);
                flt.predicates
                    .push(FilterPredicate::AttackedPlayerThisTurn(PlayerRef::You));
                continue;
            }
            if let Some(flag) = eat_phrase_table(
                ts,
                &[
                    (
                        &["attacked", "this", "turn"],
                        FilterFlags::ATTACKED_THIS_TURN,
                    ),
                    (&["has", "an", "adventure"], FilterFlags::HAS_ADVENTURE),
                    (&["have", "an", "adventure"], FilterFlags::HAS_ADVENTURE),
                ],
            ) {
                flt.flags = flt.flags.union(flag);
                continue;
            }
            {
                let ecp = ts.checkpoint();
                if ts.eat_any_phrase(&[&["are", "enchanted", "by"], &["is", "enchanted", "by"]])
                    && try_parse(ts, parse_object_filter).is_ok()
                {
                    flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
                    continue;
                }
                ts.restore(ecp);
            }
            if let Some(flag) = eat_phrase_table(ts, THAT_FLAG_TAIL) {
                flt.flags = flt.flags.union(flag);
                continue;
            }
            {
                let kwcp = ts.checkpoint();
                if ts.eat_any_word(&["has", "have"]) {
                    if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                        flt.required_keywords
                            .extend(kws.into_iter().map(|k| k.kind));
                        continue;
                    }
                    if let Some(stat) = parse_single_stat_kind(ts)
                        && let Ok((op, val)) = try_parse(ts, parse_stat_comparison)
                    {
                        push_stat_bound(flt, stat, op, val);
                        continue;
                    }
                }
                ts.restore(kwcp);
            }
            if ts.eat_any_word(&["targets", "target"]) {
                if ts.eat_any_phrase(&[&["only", "a", "single"], &["only"]]) {
                    flt.flags = flt.flags.union(FilterFlags::SINGLE_TARGET);
                } else {
                    let _ = ts.eat_phrase(&["one", "or", "more"]);
                }
                if let Ok(tgt) = try_parse(ts, parse_subject_ref) {
                    flt.predicates
                        .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
                    continue;
                }
            }
            if ts.eat_any_phrase(&[&["is", "a"], &["is", "an"], &["is"], &["are"]])
                && try_eat_type_color_subtype_conjunct(ts, flt)
            {
                continue;
            }
            if let Some(property) = eat_phrase_table(
                ts,
                &[
                    (&["share", "a", "card", "type"], SharedProperty::CardType),
                    (
                        &["share", "a", "permanent", "type"],
                        SharedProperty::PermanentType,
                    ),
                ],
            ) && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("with"))
            {
                push_shares_property(flt, property, SubjectRef::SelfObject, false);
                continue;
            }
            if let Some(property) = eat_phrase_table(ts, SHARES_WITH_PROPS) {
                let ref_subj = parse_subject_ref(ts)?;
                push_shares_property(flt, property, ref_subj, false);
                continue;
            }
            if let Some(property) = eat_phrase_table(ts, SHARE_SELF_PROPS) {
                push_shares_property(flt, property, SubjectRef::SelfObject, false);
                continue;
            }
            ts.restore(cp);
        }

        // Trailing prior-action participles
        if let Some(kind) = eat_phrase_table(ts, PRIOR_ACTION_TAIL) {
            flt.predicates
                .push(FilterPredicate::AffectedByPriorAction(kind));
            continue;
        }

        if ts.eat_any_phrase(&[&["it's", "blocking"], &["~", "is", "blocking"]]) {
            flt.combat = Some(CombatState::Blocked);
            continue;
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["that's", "attacking"],
                &["that", "is", "attacking"],
                &["that", "are", "attacking"],
            ]) && let Ok(tgt) = try_parse(ts, parse_subject_ref)
            {
                flt.combat = Some(CombatState::Attacking);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
                continue;
            }
            ts.restore(acp);
        }
        if ts.eat_any_phrase(&[
            &["that's", "attacking", "you"],
            &["that", "are", "attacking", "you"],
            &["it's", "attacking"],
            &["~", "is", "attacking"],
        ]) {
            flt.combat = Some(CombatState::Attacking);
            continue;
        }
        if ts.eat_phrase(&["it", "devoured"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::SacrificedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["chosen", "at", "random"]) {
            flt.flags = flt.flags.union(FilterFlags::AT_RANDOM);
            continue;
        }
        if ts.eat_phrase(&["that", "convoked", "it"])
            || (ts.eat_phrase(&["that", "convoked"]) && ts.eat_tilde())
            || ts.eat_any_phrase(&[
                &["that", "crewed", "it", "this", "turn"],
                &["that", "saddled", "it", "this", "turn"],
            ])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::TappedThisWay,
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &[
                "that",
                "were",
                "put",
                "into",
                "graveyards",
                "from",
                "the",
                "battlefield",
                "this",
                "turn",
            ],
            &[
                "that",
                "was",
                "put",
                "into",
                "a",
                "graveyard",
                "from",
                "the",
                "battlefield",
                "this",
                "turn",
            ],
        ]) {
            flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
            continue;
        }
        if ts.eat_phrase(&["a", "player", "discarded", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            continue;
        }
        if ts.eat_any_phrase(&[
            &["its", "controller", "controls"],
            &["their", "controller", "controls"],
        ]) {
            flt.controller = Some(ControllerRelation::ThatPlayer);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["that's", "attacking", "or", "blocking"],
            &["that", "are", "attacking", "or", "blocking"],
        ]) {
            flt.combat = Some(CombatState::AttackingOrBlocking);
            continue;
        }
        if ts.eat_any_phrase(&[
            &["that", "has", "an", "adventure"],
            &["that", "have", "an", "adventure"],
        ]) {
            flt.flags = flt.flags.union(FilterFlags::HAS_ADVENTURE);
            continue;
        }
        if ts.eat_phrase(&["with", "power", "greater", "than", "its", "base", "power"]) {
            push_stat_bound(
                flt,
                StatKind::Power,
                CompareOp::Gt,
                ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    stat: StatKind::Power,
                },
            );
            continue;
        }
        // `[from] among cards you own [exiled with ~ | in exile [with <k> counters on them]]`
        {
            let acp = ts.checkpoint();
            let _ = ts.eat_word("from");
            if ts.eat_phrase(&["among", "cards", "you", "own"]) {
                if ts.eat_phrase(&["exiled", "with"]) && ts.eat_tilde() {
                    flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
                    continue;
                }
                if ts.eat_phrase(&["in", "exile"]) {
                    flt.zones.push(ZoneRef {
                        zone: ZoneKind::Exile,
                        owner: ZoneOwner::You,
                    });
                    continue;
                }
            }
            ts.restore(acp);
        }

        // Zone source clauses (`from your graveyard`, `from exile and from your graveyard`,
        // `from your hand or from the command zone`, `in your graveyard`)
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("from") || w.eq_ignore_ascii_case("in"))
        {
            let cp = ts.checkpoint();
            let prep = ts.next_word().expect("peeked").to_ascii_lowercase();
            // Do not steal `from <src> onto <dst>` when inside `MoveCounters`
            if let Ok(zref) = try_parse(ts, parse_zone_ref) {
                flt.zones.push(zref);
                // Multi-zone conjunction (`and from <z2>`, `or from <z2>`, `or graveyard`, `and exile`)
                loop {
                    let m_cp = ts.checkpoint();
                    if ts.eat_any_word(&["and", "or", "and/or"]) {
                        let _ = ts.eat_word(&prep) || ts.eat_any_word(&["from", "in"]);
                        if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                            flt.zones.push(z2);
                            continue;
                        }
                        const BARE_ZONES: &[(&[&str], ZoneKind)] = &[
                            (&["graveyard"], ZoneKind::Graveyard),
                            (&["graveyards"], ZoneKind::Graveyard),
                            (&["hand"], ZoneKind::Hand),
                            (&["hands"], ZoneKind::Hand),
                            (&["library"], ZoneKind::Library),
                            (&["libraries"], ZoneKind::Library),
                            (&["exile"], ZoneKind::Exile),
                        ];
                        if let Some(zone) = eat_phrase_table(ts, BARE_ZONES) {
                            flt.zones.push(ZoneRef {
                                zone,
                                owner: zref.owner,
                            });
                            continue;
                        }
                    }
                    ts.restore(m_cp);
                    break;
                }
                continue;
            }
            ts.restore(cp);
        }

        if super::relatives::try_parse_extended_post_modifier(ts, flt)? {
            continue;
        }

        break;
    }
    Ok(())
}

fn parse_proper_card_names(ts: &mut TokenStream<'_>) -> Option<Vec<String>> {
    let mut names = vec![parse_one_proper_card_name(ts)?];
    let next_name = |ts: &mut TokenStream<'_>| {
        if ts.eat_tilde() {
            Some("~".to_string())
        } else {
            parse_one_proper_card_name(ts)
        }
    };
    loop {
        let cp = ts.checkpoint();
        let had_comma = ts.eat_comma();
        let had_conj = ts.eat_any_word(&["or", "and"]);
        if had_conj {
            if let Some(next) = next_name(ts) {
                names.push(next);
                continue;
            }
        } else if had_comma {
            // Only consume a bare comma in a 3+ element Oxford list (must be followed by another `,` or `and`/`or`).
            if let Some(next) = next_name(ts)
                && (matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
                    || ts.peek_word().is_some_and(|w| {
                        w.eq_ignore_ascii_case("and") || w.eq_ignore_ascii_case("or")
                    }))
            {
                names.push(next);
                continue;
            }
        }
        ts.restore(cp);
        break;
    }
    Some(names)
}

fn parse_one_proper_card_name(ts: &mut TokenStream<'_>) -> Option<String> {
    let start_cp = ts.checkpoint();
    let mut parts: Vec<String> = Vec::new();
    let mut saw_title_comma = false;
    while parts.len() < 8 {
        if let Some(w) = ts.peek_word() {
            let has_upper = w.chars().any(|c| c.is_ascii_uppercase());
            if has_upper {
                parts.push(w.to_string());
                ts.next_word();
                continue;
            }
            // Lowercase connector inside a proper card name (`of`, `the`, `in`, `to`, `from`, `at`, `on`, `for`, `a`, `an`, `by`),
            // including two-word connectors like `of the <Capitalized>` (`Advocate of the Beast`, `Spirit of the Night`, `Mark of the Rani`)
            // and `of ~` (`Kobolds of ~`)!
            if !parts.is_empty()
                && matches!(
                    w,
                    "of" | "the" | "in" | "to" | "from" | "at" | "on" | "for" | "a" | "an" | "by"
                )
            {
                let next_is_upper_or_tilde = ts.peek_n(1).is_some_and(|t| match t.kind {
                    TokenKind::Word(nw) => {
                        nw.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    }
                    TokenKind::Tilde => true,
                    _ => false,
                });
                let next_is_article_then_upper = ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if matches!(nw, "the" | "a" | "an"))
                }) && ts.peek_n(2).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if nw.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
                });
                if next_is_upper_or_tilde || next_is_article_then_upper {
                    parts.push(w.to_string());
                    ts.next_word();
                    if matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Tilde)) {
                        ts.next_tok();
                        parts.push("~".to_string());
                    }
                    continue;
                }
            }
            break;
        }
        // Possessive `'s` after a proper name (e.g. `Nissa's Chosen` tokenized as `Nissa` + `'s` + `Chosen`)
        if !parts.is_empty() && ts.eat_word("'s") {
            if let Some(last) = parts.last_mut() {
                last.push_str("'s");
            }
            continue;
        }
        // Legendary title comma (e.g. `Vraska, Scheming Gorgon`, `Nicol Bolas, the Deceiver`):
        // ONLY consume comma if we haven't already seen a title comma AND next token is a capitalized word
        // (or `the <Capitalized>`) that is NOT a sentence/clause starter verb!
        if !parts.is_empty() && !saw_title_comma {
            let ccp = ts.checkpoint();
            if ts.eat_comma() {
                let next_is_title = ts.peek_word().is_some_and(|nw| {
                    let is_upper = nw.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                    let is_clause_verb = matches!(
                        nw,
                        "Put" | "Reveal" | "Exile" | "Draw" | "If" | "Then" | "Shuffle"
                            | "Sacrifice" | "Target" | "Destroy" | "Return" | "You" | "Each"
                            | "When" | "Whenever" | "At" | "Create" | "Choose" | "Discard"
                            | "Counter" | "Tap" | "Untap"
                    );
                    (is_upper && !is_clause_verb)
                        || (nw == "the"
                            && ts.peek_n(1).is_some_and(|t| {
                                matches!(t.kind, TokenKind::Word(w2) if w2.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
                            }))
                });
                if next_is_title {
                    saw_title_comma = true;
                    if let Some(last) = parts.last_mut() {
                        last.push(',');
                    }
                    continue;
                }
            }
            ts.restore(ccp);
        }
        break;
    }
    if parts.is_empty() {
        ts.restore(start_cp);
        None
    } else {
        Some(parts.join(" "))
    }
}

pub(super) fn parse_single_stat_kind(ts: &mut TokenStream<'_>) -> Option<StatKind> {
    const STATS: &[(&[&str], StatKind)] = &[
        (&["mana", "value"], StatKind::ManaValue),
        (&["total", "mana", "value"], StatKind::ManaValue),
        (&["base", "power"], StatKind::Power),
        (&["power"], StatKind::Power),
        (&["base", "toughness"], StatKind::Toughness),
        (&["toughness"], StatKind::Toughness),
        (&["loyalty"], StatKind::Loyalty),
    ];
    eat_phrase_table(ts, STATS)
}

pub(super) fn parse_stat_comparison(ts: &mut TokenStream<'_>) -> PResult<(CompareOp, ValueExpr)> {
    const OPS: &[(&[&str], CompareOp)] = &[
        (&["equal", "to"], CompareOp::Eq),
        (&["less", "than", "or", "equal", "to"], CompareOp::Le),
        (&["greater", "than", "or", "equal", "to"], CompareOp::Ge),
        (&["less", "than"], CompareOp::Lt),
        (&["greater", "than"], CompareOp::Gt),
    ];
    if let Some(op) = eat_phrase_table(ts, OPS) {
        return Ok((op, parse_value_expr(ts)?));
    }
    let val = parse_value_expr(ts)?;
    if ts.eat_phrase(&["or", "less"]) {
        Ok((CompareOp::Le, val))
    } else if ts.eat_phrase(&["or", "greater"]) {
        Ok((CompareOp::Ge, val))
    } else {
        let ocp = ts.checkpoint();
        if ts.eat_word("or")
            && let Ok(hi) = try_parse(ts, parse_value_expr)
        {
            Ok((CompareOp::Le, hi))
        } else {
            ts.restore(ocp);
            Ok((CompareOp::Eq, val))
        }
    }
}
