//! Post-nominal modifiers for [`ObjectFilter`] (controller, owner, zone, stat bounds,
//! counter predicates, keywords, relative clauses).

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation, StatKind,
    SupertypeMask,
};
use crate::v2::ir::{
    FilterFlags, FilterPredicate, ObjectFilter, PlayerRef, PriorActionKind, SharedProperty,
    StatBound, SubjectRef, ValueExpr, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;
use crate::vocab::{lookup_card_type, lookup_color, lookup_supertype};

use super::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};

pub(super) fn parse_filter_post_modifiers(
    ts: &mut TokenStream<'_>,
    flt: &mut ObjectFilter,
) -> PResult<()> {
    loop {
        if ts.eat_phrase(&["exiled", "from", "their", "hand", "this", "way"])
            || ts.eat_phrase(&["exiled", "from", "your", "hand", "this", "way"])
            || ts.eat_phrase(&["exiled", "from", "that", "player's", "hand", "this", "way"])
        {
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
            if (ts.eat_phrase(&["that", "doesn't", "have"])
                || ts.eat_phrase(&["that", "don't", "have"]))
                && (ts.eat_word("a") || ts.eat_word("an"))
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
                && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
            {
                flt.predicates
                    .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                        Box::new(ObjectFilter {
                            predicates: vec![FilterPredicate::HasCounterKind(ck)],
                            ..Default::default()
                        }),
                    ))));
                continue;
            }
            ts.restore(ncp);
        }
        if ts.eat_phrase(&["not", "cast", "this", "way"])
            || ts.eat_phrase(&["not", "put", "onto", "the", "battlefield", "this", "way"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["with", "an", "activated", "ability"])
            || ts.eat_phrase(&["with", "activated", "abilities"])
        {
            flt.is_ability = true;
            continue;
        }
        // Controller / owner clauses
        if ts.eat_phrase(&["you", "don't", "control"]) {
            flt.controller = Some(ControllerRelation::NotYou);
            continue;
        }
        if ts.eat_phrase(&["your", "team", "controls"]) {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if ts.eat_phrase(&["those", "opponents", "control"])
            || ts.eat_phrase(&["that", "opponent", "controls"])
        {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["defending", "player", "controls"]) {
            flt.controller = Some(ControllerRelation::DefendingPlayer);
            continue;
        }
        if ts.eat_phrase(&["enchanted", "player", "controls"]) {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["that's", "enchanted", "or", "equipped"])
            || ts.eat_phrase(&["that", "is", "enchanted", "or", "equipped"])
            || ts.eat_phrase(&["that's", "equipped", "or", "enchanted"])
        {
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
        if ts.eat_phrase(&["you", "exiled"]) || ts.eat_phrase(&["you've", "exiled"]) {
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
        if ts.eat_phrase(&["you've", "drawn", "this", "turn"])
            || ts.eat_phrase(&["you", "drew", "this", "turn"])
        {
            flt.predicates.push(FilterPredicate::DrawnThisTurn);
            continue;
        }
        if ts.eat_phrase(&["you've", "discarded", "this", "turn"])
            || ts.eat_phrase(&["you", "discarded", "this", "turn"])
        {
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
            if ts.eat_word("blocking")
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.combat = Some(CombatState::Blocking);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
                continue;
            }
            ts.restore(bcp);
            if ts.eat_phrase(&["blocked", "by"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.combat = Some(CombatState::Blocked);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
                continue;
            }
            ts.restore(bcp);
        }
        if ts.eat_phrase(&["chosen", "at", "random"]) || ts.eat_phrase(&["at", "random"]) {
            flt.flags = flt.flags.union(FilterFlags::AT_RANDOM);
            continue;
        }
        if ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"])
            || ts.eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"])
            || ts.eat_phrase(&["from", "anywhere", "other", "than", "exile"])
        {
            flt.predicates.push(FilterPredicate::FromOutsideHand);
            continue;
        }
        if ts.eat_phrase(&["that's", "one", "or", "more", "colors"])
            || ts.eat_phrase(&["that", "are", "one", "or", "more", "colors"])
        {
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
        if ts.eat_phrase(&["you", "own", "that", "your", "opponents", "control"])
            || ts.eat_phrase(&["you", "own", "that", "an", "opponent", "controls"])
        {
            flt.owner = Some(ControllerRelation::OwnerYou);
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["controlled", "by", "the", "same", "opponent"])
            || ts.eat_phrase(&["controlled", "by", "the", "same", "player"])
        {
            flt.predicates.push(FilterPredicate::SameController);
            continue;
        }
        if ts.eat_phrase(&["you", "control"]) || ts.eat_phrase(&["you", "cast"]) {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if ts.eat_phrase(&["other", "than"]) && ts.eat_tilde() {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            continue;
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
        if ts.eat_phrase(&[
            "dealt",
            "damage",
            "this",
            "turn",
            "by",
            "a",
            "source",
            "you",
            "controlled",
        ]) || ts.eat_phrase(&[
            "dealt", "damage", "this", "turn", "by", "a", "source", "you", "control",
        ]) || ts.eat_phrase(&[
            "dealt",
            "damage",
            "by",
            "enchanted",
            "creature",
            "this",
            "turn",
        ]) {
            flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
            continue;
        }
        if ts.eat_phrase(&["dealt", "damage", "by"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["this", "turn"])
        {
            flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
            flt.predicates
                .push(FilterPredicate::DealtDamageBySelfThisTurn);
            continue;
        }
        if ts.eat_phrase(&["your", "opponents", "cast"]) || ts.eat_phrase(&["opponents", "cast"]) {
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "casts"]) {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["you", "control", "but", "don't", "own"])
            || ts.eat_phrase(&["you", "cast", "but", "don't", "own"])
        {
            flt.controller = Some(ControllerRelation::You);
            flt.owner = Some(ControllerRelation::NotYou);
            continue;
        }
        if ts.eat_phrase(&["you", "don't", "own"]) {
            flt.owner = Some(ControllerRelation::NotYou);
            continue;
        }
        if ts.eat_phrase(&["you", "own"]) {
            flt.owner = Some(ControllerRelation::OwnerYou);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "owns"]) || ts.eat_phrase(&["opponents", "own"]) {
            flt.owner = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["attacking", "you"])
            || ts.eat_phrase(&["attacking", "an", "opponent"])
            || ts.eat_phrase(&["attacking", "that", "player"])
        {
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
        if ts.eat_phrase(&["returned", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ReturnedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["sacrificed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::SacrificedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["revealed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::RevealedThisWay,
            ));
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
            if ts.eat_phrase(&["from", "an", "artifact", "or", "enchantment", "source"])
                || ts.eat_phrase(&["from", "an", "artifact", "source"])
                || ts.eat_phrase(&["from", "a", "creature", "source"])
                || ts.eat_phrase(&["from", "a", "noncreature", "source"])
            {
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
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Name,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            ts.restore(scp);
        }
        if ts.eat_phrase(&["they", "own"]) || ts.eat_phrase(&["that", "player", "owns"]) {
            flt.owner = Some(ControllerRelation::OwnerThatPlayer);
            continue;
        }
        if ts.eat_phrase(&["your", "opponents", "control"])
            || ts.eat_phrase(&["opponents", "control"])
        {
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "controls"]) {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["target", "player", "controls"]) {
            flt.controller = Some(ControllerRelation::TargetPlayer);
            continue;
        }
        if ts.eat_phrase(&["target", "opponent", "controls"]) {
            flt.controller = Some(ControllerRelation::TargetOpponent);
            continue;
        }
        if ts.eat_phrase(&["they", "control"])
            || ts.eat_phrase(&["that", "player", "controls"])
            || ts.eat_phrase(&["the", "chosen", "player", "controls"])
            || ts.eat_phrase(&[
                "that",
                "player",
                "or",
                "that",
                "planeswalker's",
                "controller",
                "controls",
            ])
        {
            flt.controller = Some(ControllerRelation::ThatPlayer);
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
        if ts.eat_phrase(&["of", "the", "creature", "type", "of", "your", "choice"])
            || ts.eat_phrase(&["of", "the", "chosen", "creature", "type"])
            || ts.eat_phrase(&["of", "the", "chosen", "type"])
        {
            flt.predicates.push(FilterPredicate::OfChosenCreatureType);
            continue;
        }
        if ts.eat_phrase(&["in", "the", "pile", "of", "their", "choice"])
            || ts.eat_phrase(&["in", "the", "pile", "of", "your", "choice"])
        {
            flt.predicates.push(FilterPredicate::InChosenPile);
            continue;
        }
        if ts.eat_phrase(&["drawn", "this", "turn"]) {
            flt.predicates.push(FilterPredicate::DrawnThisTurn);
            continue;
        }
        if ts.eat_phrase(&["of", "an", "opponent's", "choice"])
            || ts.eat_phrase(&["of", "opponents'", "choice"])
        {
            flt.predicates
                .push(FilterPredicate::OfChosenPlayer(PlayerRef::AnyOpponent));
            continue;
        }
        if ts.eat_phrase(&["of", "their", "choice"]) {
            flt.predicates
                .push(FilterPredicate::OfChosenPlayer(PlayerRef::ThatPlayer));
            continue;
        }
        if ts.eat_phrase(&["of", "your", "choice"]) {
            flt.predicates
                .push(FilterPredicate::OfChosenPlayer(PlayerRef::You));
            continue;
        }
        if ts.eat_phrase(&["exiled", "with"]) && (ts.eat_tilde() || ts.eat_word("it")) {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }
        if ts.eat_phrase(&["from", "among", "the", "cards", "milled", "this", "way"])
            || ts.eat_phrase(&["from", "among", "the", "cards", "exiled", "this", "way"])
            || ts.eat_phrase(&["from", "among", "the", "cards", "revealed", "this", "way"])
            || ts.eat_phrase(&["among", "the", "cards", "milled", "this", "way"])
            || ts.eat_phrase(&["among", "the", "cards", "exiled", "this", "way"])
            || ts.eat_phrase(&["among", "the", "cards", "revealed", "this", "way"])
            || ts.eat_phrase(&["from", "among", "them"])
            || ts.eat_phrase(&["from", "among", "those", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "milled", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "exiled", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "revealed", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "copies"])
            || ts.eat_phrase(&["among", "them"])
            || ts.eat_phrase(&["from", "it"])
            || ts.eat_phrase(&["among", "those", "cards"])
        {
            flt.predicates.push(FilterPredicate::AmongPriorCards);
            continue;
        }
        if (ts.eat_phrase(&["from", "among", "cards", "exiled", "with"])
            || ts.eat_phrase(&["among", "cards", "exiled", "with"]))
            && (ts.eat_tilde() || ts.eat_word("it"))
        {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }

        // Attached-to relative clause (`attached to a Dragon you control`, `that's attached to a permanent`)
        if ts.eat_phrase(&["that's", "attached", "to"]) || ts.eat_phrase(&["attached", "to"]) {
            let host = parse_subject_ref(ts)?;
            flt.predicates
                .push(FilterPredicate::AttachedTo(Box::new(host)));
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
            if ts.eat_phrase(&["equal", "or", "lesser", "mana", "value"]) {
                flt.predicates
                    .push(FilterPredicate::LesserManaValue { or_equal: true });
                continue;
            }
            if ts.eat_phrase(&["lesser", "mana", "value"]) {
                flt.predicates
                    .push(FilterPredicate::LesserManaValue { or_equal: false });
                continue;
            }
            if ts.eat_phrase(&["the", "chosen", "name"]) || ts.eat_phrase(&["that", "name"]) {
                flt.predicates.push(FilterPredicate::OfChosenName);
                continue;
            }
            if ts.eat_phrase(&["different", "names"]) {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
                continue;
            }
            if ts.eat_phrase(&["different", "powers"]) {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_POWERS);
                continue;
            }
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(crate::v2::ir::ManaSymbol::VariableX))
            ) {
                let xcp = ts.checkpoint();
                ts.next_tok();
                if ts.eat_phrase(&["in", "its", "mana", "cost"]) {
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
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Name,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["the", "same", "name"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Name,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
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
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::ManaValue,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["greater", "power"]) {
                flt.stat_bounds.push(StatBound {
                    stat: StatKind::Power,
                    op: CompareOp::Gt,
                    value: Box::new(ValueExpr::ObjectStat {
                        subject: Box::new(SubjectRef::SelfObject),
                        stat: StatKind::Power,
                    }),
                });
                continue;
            }
            if ts.eat_phrase(&["lesser", "power"]) {
                flt.stat_bounds.push(StatBound {
                    stat: StatKind::Power,
                    op: CompareOp::Lt,
                    value: Box::new(ValueExpr::ObjectStat {
                        subject: Box::new(SubjectRef::SelfObject),
                        stat: StatKind::Power,
                    }),
                });
                continue;
            }
            if ts.eat_phrase(&["the", "greatest"])
                && let Some(stat) = parse_single_stat_kind(ts)
            {
                if ts.eat_phrase(&["or", "tied", "for", "the", "greatest"])
                    || ts.eat_phrase(&["or", "tied", "for", "greatest"])
                {
                    let _ = parse_single_stat_kind(ts);
                }
                let scope = if ts.eat_word("among") {
                    try_parse(ts, parse_object_filter).unwrap_or_default()
                } else {
                    ObjectFilter::default()
                };
                flt.predicates.push(FilterPredicate::GreatestStat {
                    stat,
                    scope: Box::new(scope),
                });
                continue;
            }
            if ts.eat_phrase(&["the", "least"])
                && let Some(stat) = parse_single_stat_kind(ts)
            {
                let scope = if ts.eat_word("among") {
                    try_parse(ts, parse_object_filter).unwrap_or_default()
                } else {
                    ObjectFilter::default()
                };
                flt.predicates.push(FilterPredicate::LeastStat {
                    stat,
                    scope: Box::new(scope),
                });
                continue;
            }
            if ts.eat_phrase(&["a", "name", "sticker", "on", "it"])
                || ts.eat_phrase(&["a", "name", "sticker", "on", "them"])
            {
                flt.predicates.push(FilterPredicate::HasNameSticker);
                continue;
            }
            if ts.eat_phrase(&["a", "basic", "land", "type"]) {
                flt.predicates.push(FilterPredicate::HasBasicLandType);
                continue;
            }
            if ts.eat_phrase(&["a", "mana", "ability"]) {
                flt.predicates.push(FilterPredicate::HasManaAbility);
                continue;
            }
            {
                let abcp = ts.checkpoint();
                if (ts.eat_word("a") || ts.eat_word("an"))
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
                if ts.eat_word("counters")
                    && (ts.eat_phrase(&["on", "them"]) || ts.eat_phrase(&["on", "it"]))
                {
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    continue;
                }
                ts.restore(ccp);
                if let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                {
                    let _ = ts.eat_phrase(&["on", "them"]) || ts.eat_phrase(&["on", "it"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                    continue;
                }
                ts.restore(ccp);
            }
            // `with <N> or more <kind> counters on it/them` (`Champion's Drake`)
            {
                let ncp = ts.checkpoint();
                if let Some(_n) = ts.next_number(false)
                    && ts.eat_phrase(&["or", "more"])
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                    continue;
                }
                ts.restore(ncp);
            }
            // `with a [kind] counter on it` / `with one or more [kind] counters on it`
            if ts.eat_phrase(&["one", "or", "more"]) || ts.eat_word("a") || ts.eat_word("an") {
                if ts.eat_word("counter") || ts.eat_word("counters") {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    continue;
                }
                if let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    flt.predicates.push(FilterPredicate::HasCounterKind(ck));
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
            // `with power or toughness <cmp> <val>` -> explicit AnyOf([Power, Toughness])
            if ts.eat_phrase(&["power", "or", "toughness"])
                || ts.eat_phrase(&["base", "power", "or", "toughness"])
            {
                let (op, val) = parse_stat_comparison(ts)?;
                let p_branch = ObjectFilter {
                    stat_bounds: vec![StatBound {
                        stat: StatKind::Power,
                        op,
                        value: Box::new(val.clone()),
                    }],
                    ..Default::default()
                };
                let t_branch = ObjectFilter {
                    stat_bounds: vec![StatBound {
                        stat: StatKind::Toughness,
                        op,
                        value: Box::new(val),
                    }],
                    ..Default::default()
                };
                flt.predicates
                    .push(FilterPredicate::AnyOf(vec![p_branch, t_branch]));
                continue;
            }
            if let Some(stat) = parse_single_stat_kind(ts) {
                let (op, val) = parse_stat_comparison(ts)?;
                flt.stat_bounds.push(StatBound {
                    stat,
                    op,
                    value: Box::new(val),
                });
                continue;
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
                let _ = ts.eat_word("a") || ts.eat_word("an");
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
                    if ts.eat_word("or") || ts.eat_word("and/or") {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        saw_or = true;
                        continue;
                    }
                    if had_comma
                        && ts.peek_word().is_some_and(|nw| {
                            let nl = nw.to_ascii_lowercase();
                            lookup_color(&nl).is_some()
                                || lookup_card_type(&nl).is_some()
                                || ts.vocab.lookup_subtype(&nl).is_some()
                        })
                    {
                        continue;
                    }
                    ts.restore(sep_cp);
                    break;
                }
                if !subs.is_empty()
                    || !ctypes.is_empty()
                    || !added_colors.is_empty()
                    || !added_supers.is_empty()
                {
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
                    continue;
                }
                ts.restore(cp);
            }
        }

        // Relative clauses starting with `that`
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("that"))
        {
            let cp = ts.checkpoint();
            ts.eat_word("that");
            if ts.eat_phrase(&[
                "was", "put", "there", "from", "your", "library", "this", "turn",
            ]) || ts.eat_phrase(&[
                "were", "put", "there", "from", "your", "library", "this", "turn",
            ]) || ts.eat_phrase(&[
                "was", "put", "there", "from", "their", "library", "this", "turn",
            ]) || ts.eat_phrase(&[
                "were", "put", "there", "from", "their", "library", "this", "turn",
            ]) || ts.eat_phrase(&[
                "was", "put", "there", "from", "a", "library", "this", "turn",
            ]) || ts.eat_phrase(&["was", "put", "there", "this", "turn"])
                || ts.eat_phrase(&["were", "put", "there", "this", "turn"])
            {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::MilledThisWay,
                ));
                continue;
            }
            if (ts.eat_phrase(&["has", "a"]) || ts.eat_phrase(&["has", "an"]))
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                continue;
            }
            if ts.eat_phrase(&["weren't", "cast", "this", "way"])
                || ts.eat_phrase(&["wasn't", "cast", "this", "way"])
                || ts.eat_phrase(&["weren't", "cast"])
                || ts.eat_phrase(&["wasn't", "cast"])
                || ts.eat_phrase(&["weren't", "put", "onto", "the", "battlefield"])
                || ts.eat_phrase(&[
                    "weren't",
                    "put",
                    "onto",
                    "the",
                    "battlefield",
                    "this",
                    "way",
                ])
                || ts.eat_phrase(&["weren't", "put", "there", "this", "way"])
                || ts.eat_phrase(&["weren't", "put", "into", "your", "hand"])
                || ts.eat_phrase(&["weren't", "chosen"])
            {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
                continue;
            }
            if ts.eat_phrase(&["isn't", "a", "mana", "ability"])
                || ts.eat_phrase(&["aren't", "mana", "abilities"])
            {
                flt.predicates.push(FilterPredicate::NotManaAbility);
                continue;
            }
            if ts.eat_phrase(&["isn't", "legendary"]) || ts.eat_phrase(&["aren't", "legendary"]) {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
                continue;
            }
            if ts.eat_phrase(&["isn't", "all", "colors"])
                || ts.eat_phrase(&["aren't", "all", "colors"])
            {
                flt.color_arity = Some(ColorArity::Monocolored);
                continue;
            }
            {
                let neg_cp = ts.checkpoint();
                if ts.eat_phrase(&["isn't", "a"])
                    || ts.eat_phrase(&["isn't", "an"])
                    || ts.eat_word("isn't")
                    || ts.eat_word("aren't")
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
                        if ts.eat_word("or") || ts.eat_word("and/or") {
                            let _ = ts.eat_word("a") || ts.eat_word("an");
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
            if ts.eat_phrase(&["were", "exiled", "this", "way"])
                || ts.eat_phrase(&["was", "exiled", "this", "way"])
            {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
                continue;
            }
            if ts.eat_phrase(&["was", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["were", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["has", "been", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["have", "been", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["dealt", "damage", "to", "you", "this", "turn"])
                || ts.eat_phrase(&["dealt", "combat", "damage", "to", "you", "this", "turn"])
                || ts.eat_phrase(&["dealt", "damage", "to", "a", "player", "this", "turn"])
                || ts.eat_phrase(&["dealt", "damage", "this", "turn"])
            {
                flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["entered", "the", "battlefield", "this", "turn"])
                || ts.eat_phrase(&["entered", "this", "turn"])
            {
                flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["died", "under", "your", "control", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                flt.controller = Some(ControllerRelation::You);
                continue;
            }
            if ts.eat_phrase(&["died", "this", "turn"])
                || ts.eat_phrase(&[
                    "were",
                    "put",
                    "there",
                    "from",
                    "the",
                    "battlefield",
                    "this",
                    "turn",
                ])
                || ts.eat_phrase(&[
                    "was",
                    "put",
                    "there",
                    "from",
                    "the",
                    "battlefield",
                    "this",
                    "turn",
                ])
            {
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["attacked", "you", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::ATTACKED_THIS_TURN);
                flt.predicates
                    .push(FilterPredicate::AttackedPlayerThisTurn(PlayerRef::You));
                continue;
            }
            if ts.eat_phrase(&["attacked", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::ATTACKED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["has", "an", "adventure"])
                || ts.eat_phrase(&["have", "an", "adventure"])
            {
                flt.flags = flt.flags.union(FilterFlags::HAS_ADVENTURE);
                continue;
            }
            {
                let ecp = ts.checkpoint();
                if (ts.eat_phrase(&["are", "enchanted", "by"])
                    || ts.eat_phrase(&["is", "enchanted", "by"]))
                    && try_parse(ts, parse_object_filter).is_ok()
                {
                    flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
                    continue;
                }
                ts.restore(ecp);
            }
            if ts.eat_phrase(&["are", "equipped"]) || ts.eat_phrase(&["is", "equipped"]) {
                flt.flags = flt.flags.union(FilterFlags::EQUIPPED);
                continue;
            }
            if ts.eat_phrase(&["are", "enchanted"]) || ts.eat_phrase(&["is", "enchanted"]) {
                flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
                continue;
            }
            if ts.eat_phrase(&["aren't", "on", "the", "battlefield"])
                || ts.eat_phrase(&["isn't", "on", "the", "battlefield"])
            {
                flt.flags = flt.flags.union(FilterFlags::NOT_ON_BATTLEFIELD);
                continue;
            }
            {
                let kwcp = ts.checkpoint();
                if (ts.eat_word("has") || ts.eat_word("have"))
                    && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                {
                    flt.required_keywords
                        .extend(kws.into_iter().map(|k| k.kind));
                    continue;
                }
                ts.restore(kwcp);
            }
            if ts.eat_word("targets") || ts.eat_word("target") {
                if ts.eat_phrase(&["only", "a", "single"]) || ts.eat_word("only") {
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
            if ts.eat_phrase(&["is", "a"])
                || ts.eat_phrase(&["is", "an"])
                || ts.eat_word("is")
                || ts.eat_word("are")
            {
                let mut subs = Vec::new();
                let mut ctypes = Vec::new();
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
                        flt.required_supertypes = flt.required_supertypes.union(st);
                    } else if let Some(col) = lookup_color(&low) {
                        ts.next_word();
                        flt.any_colors = flt.any_colors.union(col);
                    } else {
                        break;
                    }
                    let _ = ts.eat_comma();
                    if ts.eat_word("or") || ts.eat_word("and/or") {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        saw_or = true;
                    }
                }
                if !subs.is_empty() || !ctypes.is_empty() {
                    if saw_or && subs.len() >= 2 {
                        flt.subtypes_match_any = true;
                    }
                    flt.subtypes.extend(subs);
                    for ct in ctypes {
                        if saw_or {
                            flt.any_types = flt.any_types.union(ct);
                        } else {
                            flt.all_types = flt.all_types.union(ct);
                        }
                    }
                    continue;
                }
            }
            if ts.eat_phrase(&["share", "a", "card", "type"])
                && !ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("with"))
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CardType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "permanent", "type"])
                && !ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("with"))
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::PermanentType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "creature", "type", "with"])
                || ts.eat_phrase(&["share", "a", "creature", "type", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CreatureType,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "color", "with"])
                || ts.eat_phrase(&["share", "a", "color", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Color,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "card", "type", "with"])
                || ts.eat_phrase(&["share", "a", "card", "type", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CardType,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "land", "type", "with"])
                || ts.eat_phrase(&["share", "a", "land", "type", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::LandType,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "land", "type"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::LandType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "color"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Color,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "creature", "type"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CreatureType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            ts.restore(cp);
        }

        // Trailing prior-action participles (`revealed this way`, `destroyed this way`, `milled this way`, `discarded this way`, `exiled this way`)
        if ts.eat_phrase(&["they", "exiled", "this", "way"])
            || ts.eat_phrase(&["you", "exiled", "this", "way"])
            || ts.eat_phrase(&["that", "player", "exiled", "this", "way"])
            || ts.eat_phrase(&["exiled", "this", "way"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["revealed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::RevealedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["destroyed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DestroyedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["milled", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::MilledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["discarded", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["sacrificed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::SacrificedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["tapped", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::TappedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["returned", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ReturnedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["created", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CreatedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["cast", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CastThisWay,
            ));
            continue;
        }

        if ts.eat_phrase(&["it's", "blocking"]) || ts.eat_phrase(&["~", "is", "blocking"]) {
            flt.combat = Some(crate::ast::CombatState::Blocked);
            continue;
        }
        if ts.eat_phrase(&["that's", "attacking", "you"])
            || ts.eat_phrase(&["that", "are", "attacking", "you"])
            || ts.eat_phrase(&["it's", "attacking"])
            || ts.eat_phrase(&["~", "is", "attacking"])
        {
            flt.combat = Some(crate::ast::CombatState::Attacking);
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
            || ts.eat_phrase(&["that", "crewed", "it", "this", "turn"])
            || ts.eat_phrase(&["that", "saddled", "it", "this", "turn"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::TappedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&[
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
        ]) || ts.eat_phrase(&[
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
        if ts.eat_phrase(&["its", "controller", "controls"])
            || ts.eat_phrase(&["their", "controller", "controls"])
        {
            flt.controller = Some(crate::ast::ControllerRelation::ThatPlayer);
            continue;
        }
        if ts.eat_phrase(&["that's", "attacking", "or", "blocking"])
            || ts.eat_phrase(&["that", "are", "attacking", "or", "blocking"])
        {
            flt.combat = Some(crate::ast::CombatState::AttackingOrBlocking);
            continue;
        }
        if ts.eat_phrase(&["that", "has", "an", "adventure"])
            || ts.eat_phrase(&["that", "have", "an", "adventure"])
        {
            flt.flags = flt.flags.union(FilterFlags::HAS_ADVENTURE);
            continue;
        }
        if ts.eat_phrase(&["with", "power", "greater", "than", "its", "base", "power"]) {
            flt.stat_bounds.push(StatBound {
                stat: StatKind::Power,
                op: CompareOp::Gt,
                value: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It)),
                    stat: StatKind::Power,
                }),
            });
            continue;
        }
        // `[from] among cards you own [exiled with ~ | in exile [with <k> counters on them]]` (`Boiling Rock Rioter`, `Grolnok`, `Kaya the Inexorable`)
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
                    if ts.eat_word("and") || ts.eat_word("or") {
                        let _ = ts.eat_word(&prep);
                        if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                            flt.zones.push(z2);
                            continue;
                        }
                        let bare_z = if ts.eat_word("graveyard") || ts.eat_word("graveyards") {
                            Some(ZoneKind::Graveyard)
                        } else if ts.eat_word("hand") || ts.eat_word("hands") {
                            Some(ZoneKind::Hand)
                        } else if ts.eat_word("library") || ts.eat_word("libraries") {
                            Some(ZoneKind::Library)
                        } else if ts.eat_word("exile") {
                            Some(ZoneKind::Exile)
                        } else {
                            None
                        };
                        if let Some(zone) = bare_z {
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

        break;
    }
    Ok(())
}

fn parse_proper_card_names(ts: &mut TokenStream<'_>) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let first = parse_one_proper_card_name(ts)?;
    names.push(first);
    loop {
        let cp = ts.checkpoint();
        if (ts.eat_word("or") || ts.eat_word("and"))
            && let Some(next) = parse_one_proper_card_name(ts)
        {
            names.push(next);
            continue;
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
            let starts_upper = w.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            if starts_upper {
                parts.push(w.to_string());
                ts.next_word();
                continue;
            }
            // Lowercase connector inside a proper card name (`of`, `the`, `in`, `to`, `from`, `at`, `on`, `for`, `a`, `an`, `by`, `with`)
            // ONLY when followed by a capitalized word!
            if !parts.is_empty()
                && matches!(
                    w,
                    "of" | "the" | "in" | "to" | "from" | "at" | "on" | "for" | "a" | "an" | "by"
                )
                && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if nw.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
                })
            {
                parts.push(w.to_string());
                ts.next_word();
                continue;
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

fn parse_single_stat_kind(ts: &mut TokenStream<'_>) -> Option<StatKind> {
    if ts.eat_phrase(&["mana", "value"]) || ts.eat_phrase(&["total", "mana", "value"]) {
        Some(StatKind::ManaValue)
    } else if ts.eat_phrase(&["base", "power"]) || ts.eat_word("power") {
        Some(StatKind::Power)
    } else if ts.eat_phrase(&["base", "toughness"]) || ts.eat_word("toughness") {
        Some(StatKind::Toughness)
    } else if ts.eat_word("loyalty") {
        Some(StatKind::Loyalty)
    } else {
        None
    }
}

fn parse_stat_comparison(ts: &mut TokenStream<'_>) -> PResult<(CompareOp, ValueExpr)> {
    if ts.eat_phrase(&["equal", "to"]) {
        return Ok((CompareOp::Eq, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["less", "than", "or", "equal", "to"]) {
        return Ok((CompareOp::Le, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["greater", "than", "or", "equal", "to"]) {
        return Ok((CompareOp::Ge, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["less", "than"]) {
        return Ok((CompareOp::Lt, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["greater", "than"]) {
        return Ok((CompareOp::Gt, parse_value_expr(ts)?));
    }
    let val = parse_value_expr(ts)?;
    if ts.eat_phrase(&["or", "less"]) {
        Ok((CompareOp::Le, val))
    } else if ts.eat_phrase(&["or", "greater"]) {
        Ok((CompareOp::Ge, val))
    } else {
        Ok((CompareOp::Eq, val))
    }
}
