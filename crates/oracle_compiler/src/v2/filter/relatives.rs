//! Extended post-nominal relative clauses, choice qualifiers, exclusions, and
//! stat/characteristic predicates for [`ObjectFilter`].

use crate::ast::{
    CardTypeMask, ColorArity, CombatState, CompareOp, ControllerRelation, StatKind, SupertypeMask,
};
use crate::v2::ir::{
    AnaphoraRef, FilterFlags, FilterPredicate, ManaSymbol, ObjectFilter, PlayerRef,
    PriorActionKind, SharedProperty, StatBound, SubjectRef, ValueExpr, ZoneKind, ZoneOwner,
    ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::vocab::{lookup_card_type, lookup_color};

use super::post_modifiers::{parse_single_stat_kind, parse_stat_comparison};
use super::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};

/// Try to consume one extended post-nominal modifier or relative clause on `flt`.
/// Every speculative branch checkpoints and restores on failure to preserve zero regressions.
pub(super) fn try_parse_extended_post_modifier(
    ts: &mut TokenStream<'_>,
    flt: &mut ObjectFilter,
) -> PResult<bool> {
    // 1. Past-tense LKI controller/owner clauses & additional ownership/control forms
    if ts.eat_phrase(&["you", "controlled"]) {
        flt.controller = Some(ControllerRelation::You);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["your", "opponents", "controlled"])
        || ts.eat_phrase(&["opponents", "controlled"])
    {
        flt.controller = Some(ControllerRelation::Opponents);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["an", "opponent", "controlled"]) {
        flt.controller = Some(ControllerRelation::Opponent);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["that", "player", "controlled"])
        || ts.eat_phrase(&["that", "opponent", "controlled"])
    {
        flt.controller = Some(ControllerRelation::ThatPlayer);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["your", "opponents", "own"]) {
        flt.owner = Some(ControllerRelation::OwnerOpponent);
        return Ok(true);
    }
    if ts.eat_phrase(&["you", "owned"]) {
        flt.owner = Some(ControllerRelation::OwnerYou);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "owned"]) || ts.eat_phrase(&["that", "player", "owned"]) {
        flt.owner = Some(ControllerRelation::OwnerThatPlayer);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["or", "control"]) && flt.owner == Some(ControllerRelation::OwnerYou) {
        flt.controller = Some(ControllerRelation::You);
        return Ok(true);
    }
    if ts.eat_phrase(&[
        "that",
        "opponent",
        "or",
        "that",
        "planeswalker's",
        "controller",
        "controls",
    ]) {
        flt.controller = Some(ControllerRelation::ThatPlayerOrPlaneswalkerController);
        return Ok(true);
    }
    if ts.eat_phrase(&["that", "player", "casts"])
        || ts.eat_phrase(&["they", "cast"])
        || ts.eat_phrase(&["enchanted", "player", "casts"])
    {
        flt.controller = Some(ControllerRelation::ThatPlayer);
        return Ok(true);
    }
    if ts.eat_phrase(&["a", "player", "casts"]) || ts.eat_phrase(&["players", "cast"]) {
        flt.controller = Some(ControllerRelation::ActivePlayer);
        return Ok(true);
    }
    if ts.eat_phrase(&["you", "already", "control"]) {
        flt.controller = Some(ControllerRelation::You);
        return Ok(true);
    }
    if ts.eat_phrase(&["you", "neither", "own", "nor", "control"]) {
        flt.controller = Some(ControllerRelation::NotYou);
        flt.owner = Some(ControllerRelation::NotYou);
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "don't", "control"])
        || ts.eat_phrase(&["that", "player", "doesn't", "control"])
    {
        flt.controller = Some(ControllerRelation::NotYou);
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "don't", "own"])
        || ts.eat_phrase(&["that", "player", "doesn't", "own"])
    {
        flt.owner = Some(ControllerRelation::NotYou);
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "protect"])
        || ts.eat_phrase(&["that", "player", "protects"])
        || ts.eat_phrase(&["an", "opponent", "protects"])
        || ts.eat_phrase(&["your", "opponents", "protect"])
    {
        flt.controller = Some(ControllerRelation::Opponent);
        return Ok(true);
    }
    if ts.eat_phrase(&["you", "protect"]) {
        flt.controller = Some(ControllerRelation::You);
        return Ok(true);
    }
    if ts.eat_phrase(&["that", "one", "of", "your", "opponents", "controls"])
        || ts.eat_phrase(&["each", "opponent", "controls"])
        || ts.eat_phrase(&["the", "other", "controls"])
    {
        flt.controller = Some(ControllerRelation::Opponent);
        return Ok(true);
    }
    if ts.eat_phrase(&["a", "player", "controls"])
        || ts.eat_phrase(&["each", "player", "controls"])
        || ts.eat_phrase(&["the", "monarch", "controls"])
        || ts.eat_phrase(&["that", "creature's", "controller", "controls"])
        || ts.eat_phrase(&["that", "permanent's", "controller", "controls"])
    {
        flt.controller = Some(ControllerRelation::ThatPlayer);
        return Ok(true);
    }
    if ts.eat_phrase(&["the", "opponent", "to", "your", "left", "controls"])
        || ts.eat_phrase(&["the", "player", "to", "your", "left", "controls"])
    {
        flt.controller = Some(ControllerRelation::PlayerToLeft);
        return Ok(true);
    }
    if ts.eat_phrase(&["the", "opponent", "to", "your", "right", "controls"])
        || ts.eat_phrase(&["the", "player", "to", "your", "right", "controls"])
    {
        flt.controller = Some(ControllerRelation::PlayerToRight);
        return Ok(true);
    }
    if ts.eat_phrase(&[
        "the",
        "active",
        "player",
        "has",
        "controlled",
        "continuously",
        "since",
        "the",
        "beginning",
        "of",
        "the",
        "turn",
    ]) || ts.eat_phrase(&[
        "the",
        "player",
        "hasn't",
        "controlled",
        "continuously",
        "since",
        "the",
        "beginning",
        "of",
        "the",
        "turn",
    ]) || ts.eat_phrase(&[
        "that",
        "they",
        "controlled",
        "since",
        "the",
        "beginning",
        "of",
        "the",
        "turn",
    ]) {
        flt.controller = Some(ControllerRelation::ActivePlayer);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&[
        "you",
        "had",
        "enter",
        "the",
        "battlefield",
        "under",
        "your",
        "control",
        "this",
        "turn",
    ]) {
        flt.controller = Some(ControllerRelation::You);
        flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&["that", "player", "has", "drawn", "this", "turn"])
        || ts.eat_phrase(&["an", "opponent", "has", "drawn", "this", "turn"])
        || ts.eat_phrase(&["they", "drew", "this", "way"])
    {
        flt.predicates.push(FilterPredicate::DrawnThisTurn);
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "discarded", "this", "way"])
        || ts.eat_phrase(&["you", "discarded", "this", "way"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::DiscardedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "revealed", "this", "way"])
        || ts.eat_phrase(&["you", "revealed", "this", "way"])
        || ts.eat_phrase(&["they", "revealed"])
        || ts.eat_phrase(&["you", "revealed"])
        || ts.eat_phrase(&["revealed", "by", "the", "other", "player"])
        || ts.eat_phrase(&["revealed", "by", "an", "opponent"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::RevealedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "exiled", "with"]) && (ts.eat_tilde() || ts.eat_word("it")) {
        let _ = ts.eat_phrase(&["this", "turn"]);
        flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
        return Ok(true);
    }
    if ts.eat_phrase(&["they", "exiled"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ExiledThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&[
        "controlled",
        "by",
        "the",
        "next",
        "player",
        "in",
        "the",
        "chosen",
        "direction",
    ]) {
        flt.controller = Some(ControllerRelation::NextPlayerInDirection);
        return Ok(true);
    }
    if ts.eat_phrase(&["controlled", "by", "the", "player", "to", "their", "right"])
        || ts.eat_phrase(&["controlled", "by", "the", "player", "to", "your", "right"])
    {
        flt.controller = Some(ControllerRelation::PlayerToRight);
        return Ok(true);
    }
    if ts.eat_phrase(&["controlled", "by", "the", "player", "to", "their", "left"])
        || ts.eat_phrase(&["controlled", "by", "the", "player", "to", "your", "left"])
    {
        flt.controller = Some(ControllerRelation::PlayerToLeft);
        return Ok(true);
    }
    if ts.eat_phrase(&[
        "controlled",
        "by",
        "that",
        "player",
        "or",
        "by",
        "that",
        "planeswalker's",
        "controller",
    ]) {
        flt.controller = Some(ControllerRelation::ThatPlayerOrPlaneswalkerController);
        return Ok(true);
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["controlled", "by"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            let _ = super::subject::try_eat_player_relative_tail(ts);
            flt.controller = Some(player_ref_to_controller(p));
            return Ok(true);
        }
        ts.restore(ccp);
        if ts.eat_phrase(&["owned", "by"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
        {
            let _ = super::subject::try_eat_player_relative_tail(ts);
            flt.owner = Some(player_ref_to_owner(p));
            return Ok(true);
        }
        ts.restore(ccp);
        if ts.eat_tilde() && ts.eat_word("haunts") {
            flt.predicates.push(FilterPredicate::AttachedTo(Box::new(
                SubjectRef::SelfObject,
            )));
            return Ok(true);
        }
        ts.restore(ccp);
    }

    // 2. Choice and category qualifiers
    if ts.eat_phrase(&["of", "the", "chosen", "color", "and", "type"])
        || ts.eat_phrase(&["of", "the", "chosen", "color", "and", "creature", "type"])
    {
        flt.predicates.push(FilterPredicate::OfChosenColor);
        flt.predicates.push(FilterPredicate::OfChosenCreatureType);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "defending", "player's", "choice"]) {
        flt.predicates
            .push(FilterPredicate::OfChosenPlayer(PlayerRef::DefendingPlayer));
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "target", "opponent's", "choice"]) {
        flt.predicates
            .push(FilterPredicate::OfChosenPlayer(PlayerRef::TargetOpponent));
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "target", "player's", "choice"]) {
        flt.predicates
            .push(FilterPredicate::OfChosenPlayer(PlayerRef::TargetPlayer));
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "that", "player's", "choice"])
        || ts.eat_phrase(&["of", "its", "controller's", "choice"])
    {
        flt.predicates
            .push(FilterPredicate::OfChosenPlayer(PlayerRef::ThatPlayer));
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "the", "color", "of", "your", "choice"])
        || ts.eat_phrase(&["of", "the", "last", "chosen", "color"])
    {
        flt.predicates.push(FilterPredicate::OfChosenColor);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "the", "chosen", "kind"]) {
        flt.predicates.push(FilterPredicate::OfChosenCreatureType);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "each", "card", "type"])
        || ts.eat_phrase(&["of", "each", "permanent", "type"])
    {
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "each", "basic", "land", "type"]) {
        flt.predicates.push(FilterPredicate::HasBasicLandType);
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "each", "color"]) {
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "the", "other", "type"])
        || ts.eat_phrase(&["of", "a", "type", "chosen", "this", "way"])
    {
        flt.predicates.push(FilterPredicate::OfChosenCreatureType);
        return Ok(true);
    }
    // `of each of the sacrificed creature's colors` / `of each of that creature's colors`
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["of", "each", "of"])
            && (ts.eat_phrase(&["the", "sacrificed", "creature's"])
                || ts.eat_phrase(&["the", "sacrificed", "permanent's"])
                || ts.eat_phrase(&["that", "creature's"])
                || ts.eat_phrase(&["that", "permanent's"])
                || ts.eat_word("its"))
            && (ts.eat_word("colors") || ts.eat_word("color"))
        {
            flt.predicates.push(FilterPredicate::SharesPropertyWith {
                property: SharedProperty::Color,
                reference: Box::new(SubjectRef::Anaphora(AnaphoraRef::SacrificedObject)),
                negated: false,
            });
            return Ok(true);
        }
        ts.restore(ccp);
    }

    // 3. Prior-action / craft / pile / created / zone participles
    if ts.eat_phrase(&["chosen", "this", "way"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["returned", "to", "your", "hand", "this", "way"])
        || ts.eat_phrase(&["returned", "to", "its", "owner's", "hand", "this", "way"])
        || ts.eat_phrase(&["returned", "to", "their", "owners'", "hands", "this", "way"])
        || ts.eat_phrase(&["put", "onto", "the", "battlefield", "this", "way"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ReturnedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["sacrificed", "as", "it", "entered"])
        || ts.eat_phrase(&["devoured", "this", "way"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::SacrificedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["cast", "before", "that", "spell", "this", "turn"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::CastThisWay,
        ));
        return Ok(true);
    }
    {
        let cf_cp = ts.checkpoint();
        if ts.eat_phrase(&["cast", "from"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            flt.zones.push(zr);
            return Ok(true);
        }
        ts.restore(cf_cp);
    }
    if ts.eat_phrase(&[
        "dealt", "damage", "by", "equipped", "creature", "this", "turn",
    ]) || ts.eat_phrase(&["dealt", "damage", "this", "turn"])
    {
        flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
        return Ok(true);
    }
    {
        let bb_cp = ts.checkpoint();
        if ts.eat_phrase(&["blocking", "or", "being", "blocked", "by"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
        {
            flt.combat = Some(CombatState::Blocking);
            flt.predicates
                .push(FilterPredicate::TargetsSubject(Box::new(subj)));
            return Ok(true);
        }
        ts.restore(bb_cp);
    }
    if ts.eat_phrase(&[
        "not",
        "declared",
        "as",
        "an",
        "attacking",
        "or",
        "blocking",
        "creature",
        "this",
        "combat",
    ]) {
        flt.combat = Some(CombatState::NotAttacking);
        return Ok(true);
    }
    if (ts.eat_phrase(&["directly", "above"]) || ts.eat_word("above"))
        && (ts.eat_tilde() || ts.eat_word("it"))
    {
        flt.zones.push(ZoneRef {
            zone: ZoneKind::Graveyard,
            owner: ZoneOwner::You,
        });
        return Ok(true);
    }
    if (flt.any_card
        || flt.any_permanent
        || !flt.any_types.is_empty()
        || !flt.all_types.is_empty()
        || !flt.subtypes.is_empty())
        && ts.eat_phrase(&["in", "it"])
    {
        flt.predicates.push(FilterPredicate::AmongPriorCards);
        return Ok(true);
    }
    {
        let top_cp = ts.checkpoint();
        if ts.eat_phrase(&["from", "the", "top", "of"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            flt.zones.push(zr);
            return Ok(true);
        }
        ts.restore(top_cp);
    }
    // `from among <object_filter>` (`The Legend of Yangchen`, `Tasha, the Witch Queen`, `Draugr Necromancer`)
    {
        let fa_cp = ts.checkpoint();
        if ts.eat_phrase(&["from", "among"])
            && !ts.peek_phrase(&["the", "cards"])
            && !ts.peek_phrase(&["cards", "revealed"])
            && !ts.peek_phrase(&["cards", "exiled"])
            && !ts.peek_phrase(&["them"])
            && let Ok(src_flt) = try_parse(ts, parse_object_filter)
        {
            flt.zones.extend(src_flt.zones);
            flt.predicates.push(FilterPredicate::AmongPriorCards);
            return Ok(true);
        }
        ts.restore(fa_cp);
    }
    if ts.eat_phrase(&["in", "the", "pile", "of", "that", "player's", "choice"])
        || ts.eat_phrase(&["in", "the", "pile", "of", "an", "opponent's", "choice"])
    {
        flt.predicates.push(FilterPredicate::InChosenPile);
        return Ok(true);
    }
    if ts.eat_phrase(&["created", "with"]) && (ts.eat_tilde() || ts.eat_word("it")) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::CreatedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["used", "to", "craft"]) && (ts.eat_tilde() || ts.eat_word("it")) {
        flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
        return Ok(true);
    }
    if ts.eat_phrase(&["not", "chosen", "by", "any", "player", "this", "way"])
        || ts.eat_phrase(&["not", "chosen", "this", "way"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["countered", "this", "way"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["in", "the", "chosen", "piles"])
        || ts.eat_phrase(&["in", "the", "chosen", "pile"])
        || ts.eat_phrase(&["in", "the", "other", "piles"])
        || ts.eat_phrase(&["in", "the", "other", "pile"])
    {
        flt.predicates.push(FilterPredicate::InChosenPile);
        return Ok(true);
    }

    // 4. `[,] except [for] <subject/filter>` (strictly guarded against CopyException and CantBeBlockedExceptBy)
    {
        let ecp = ts.checkpoint();
        let had_comma = ts.eat_comma();
        if ts.eat_word("except") {
            let had_for = ts.eat_word("for");
            if had_for {
                if ts.eat_tilde() {
                    flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
                    return Ok(true);
                }
                if ts.next_number(false).is_some() {
                    flt.predicates
                        .push(FilterPredicate::Exclude(Box::new(SubjectRef::SelfObject)));
                    return Ok(true);
                }
                if let Ok(excl) = try_parse(ts, parse_object_filter) {
                    flt.predicates
                        .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                            Box::new(excl),
                        ))));
                    return Ok(true);
                }
                if let Ok(excl_subj) = try_parse(ts, parse_subject_ref) {
                    flt.predicates
                        .push(FilterPredicate::Exclude(Box::new(excl_subj)));
                    return Ok(true);
                }
            } else if !had_comma {
                if ts.eat_phrase(&["those", "that"]) {
                    let mut ex_flt = ObjectFilter::default();
                    if try_parse_that_relative(ts, &mut ex_flt)? {
                        flt.predicates
                            .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                                Box::new(ex_flt),
                            ))));
                        return Ok(true);
                    }
                }
                let blocked_starter = ts.peek_word().is_some_and(|w| {
                    matches!(
                        w.to_ascii_lowercase().as_str(),
                        "it" | "it's"
                            | "its"
                            | "they"
                            | "they're"
                            | "their"
                            | "that"
                            | "those"
                            | "by"
                            | "during"
                            | "as"
                            | "to"
                            | "if"
                            | "when"
                    )
                });
                if !blocked_starter {
                    if ts.eat_phrase(&["mana", "abilities"]) {
                        flt.predicates.push(FilterPredicate::NotManaAbility);
                        return Ok(true);
                    }
                    if let Ok(excl) = try_parse(ts, parse_object_filter) {
                        flt.predicates
                            .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                                Box::new(excl),
                            ))));
                        return Ok(true);
                    }
                }
            }
        }
        ts.restore(ecp);
    }

    // 5. Extended `without` clauses (`without {X} in its/their mana cost(s)`, `without mana abilities`)
    {
        let wcp = ts.checkpoint();
        if ts.eat_phrase(&["without", "mana", "abilities"])
            || ts.eat_phrase(&["without", "a", "mana", "ability"])
        {
            flt.predicates.push(FilterPredicate::NotManaAbility);
            return Ok(true);
        }
        ts.restore(wcp);
        if ts.eat_word("without")
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::VariableX))
            )
        {
            ts.next_tok();
            if ts.eat_phrase(&["in", "its", "mana", "cost"])
                || ts.eat_phrase(&["in", "its", "mana", "costs"])
                || ts.eat_phrase(&["in", "their", "mana", "cost"])
                || ts.eat_phrase(&["in", "their", "mana", "costs"])
            {
                flt.predicates
                    .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                        Box::new(ObjectFilter {
                            flags: FilterFlags::HAS_X_IN_MANA_COST,
                            ..Default::default()
                        }),
                    ))));
                return Ok(true);
            }
        }
        ts.restore(wcp);
    }

    // 6. Extended `with` clauses
    if ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("with"))
    {
        let wcp = ts.checkpoint();
        ts.eat_word("with");
        if try_parse_extended_with_body(ts, flt)? {
            return Ok(true);
        }
        ts.restore(wcp);
    }

    // 7. Extended `that's` / `that` / `whose` relative clauses
    if ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("that's"))
    {
        let cp = ts.checkpoint();
        ts.eat_word("that's");
        if try_parse_thats_predicate(ts, flt)? {
            return Ok(true);
        }
        ts.restore(cp);
    }
    if ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("that"))
    {
        let cp = ts.checkpoint();
        ts.eat_word("that");
        if try_parse_that_relative(ts, flt)? {
            return Ok(true);
        }
        ts.restore(cp);
    }
    if ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("whose"))
    {
        let cp = ts.checkpoint();
        ts.eat_word("whose");
        if try_parse_whose_relative(ts, flt)? {
            return Ok(true);
        }
        ts.restore(cp);
    }

    // 8. Trailing `and/or <object_filter>` union after post-modifiers (`creatures with power 2 or less and/or Walls`)
    {
        let ucp = ts.checkpoint();
        if ts.eat_word("and/or")
            && !ts.peek_word().is_some_and(|w| {
                matches!(
                    w.to_ascii_lowercase().as_str(),
                    "player" | "players" | "opponent" | "opponents" | "target"
                )
            })
            && let Ok(other) = try_parse(ts, parse_object_filter)
        {
            flt.predicates.push(FilterPredicate::AnyOf(vec![other]));
            return Ok(true);
        }
        ts.restore(ucp);
    }

    Ok(false)
}

fn try_parse_extended_with_body(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<bool> {
    if ts.eat_phrase(&["total", "power", "and", "toughness"]) {
        let (op, val) = parse_stat_comparison(ts)?;
        flt.stat_bounds.push(StatBound {
            stat: StatKind::Power,
            op,
            value: Box::new(val),
        });
        return Ok(true);
    }
    // `with power/toughness greater/less than (that creature's|its|~'s) [base] (power|toughness)`
    {
        let bcp = ts.checkpoint();
        if let Some(stat) = parse_single_stat_kind(ts) {
            let op_opt = if ts.eat_phrase(&["greater", "than"]) {
                Some(CompareOp::Gt)
            } else if ts.eat_phrase(&["less", "than"]) {
                Some(CompareOp::Lt)
            } else {
                None
            };
            if let Some(op) = op_opt {
                let subj_opt = if ts.eat_phrase(&["that", "creature's"])
                    || ts.eat_phrase(&["that", "permanent's"])
                    || ts.eat_word("its")
                    || ts.eat_word("their")
                {
                    Some(SubjectRef::Anaphora(AnaphoraRef::ThatCreature))
                } else if ts.eat_tilde() && ts.eat_word("'s") {
                    Some(SubjectRef::SelfObject)
                } else {
                    None
                };
                if let Some(subj) = subj_opt {
                    let rhs_stat = parse_single_stat_kind(ts).unwrap_or(stat);
                    flt.stat_bounds.push(StatBound {
                        stat,
                        op,
                        value: Box::new(ValueExpr::ObjectStat {
                            subject: Box::new(subj),
                            stat: rhs_stat,
                        }),
                    });
                    return Ok(true);
                }
            }
        }
        ts.restore(bcp);
    }
    // `with lesser mana value than <subj>` (`Death's Oasis`, `Triple Triad`)
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["lesser", "mana", "value", "than"])
            && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
        {
            flt.stat_bounds.push(StatBound {
                stat: StatKind::ManaValue,
                op: CompareOp::Lt,
                value: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(ref_subj),
                    stat: StatKind::ManaValue,
                }),
            });
            return Ok(true);
        }
        ts.restore(lcp);
    }
    // `with equal power/toughness/mana value` (`V.A.T.S.`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("equal")
            && let Some(stat) = parse_single_stat_kind(ts)
        {
            flt.stat_bounds.push(StatBound {
                stat,
                op: CompareOp::Eq,
                value: Box::new(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    stat,
                }),
            });
            return Ok(true);
        }
        ts.restore(ecp);
    }
    if ts.eat_phrase(&["equal", "or", "lesser", "power"]) {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::Power,
            op: CompareOp::Le,
            value: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::SelfObject),
                stat: StatKind::Power,
            }),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["equal", "or", "lesser", "toughness"])
        || ts.eat_phrase(&["lesser", "toughness"])
    {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::Toughness,
            op: CompareOp::Le,
            value: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::SelfObject),
                stat: StatKind::Toughness,
            }),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["greater", "mana", "value"])
        || ts.eat_phrase(&["equal", "or", "greater", "mana", "value"])
    {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::ManaValue,
            op: CompareOp::Gt,
            value: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                stat: StatKind::ManaValue,
            }),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["equal", "or", "greater", "power"]) {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::Power,
            op: CompareOp::Ge,
            value: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::SelfObject),
                stat: StatKind::Power,
            }),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["an", "even", "mana", "value"])
        || ts.eat_phrase(&["even", "mana", "values"])
        || ts.eat_phrase(&["even", "mana", "value"])
    {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::ManaValue,
            op: CompareOp::Eq,
            value: Box::new(ValueExpr::Const(0)),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["an", "odd", "mana", "value"])
        || ts.eat_phrase(&["odd", "mana", "values"])
        || ts.eat_phrase(&["odd", "mana", "value"])
    {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::ManaValue,
            op: CompareOp::Eq,
            value: Box::new(ValueExpr::Const(1)),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["a", "mana", "cost", "that", "contains"])
        && matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(ManaSymbol::VariableX))
        )
    {
        ts.next_tok();
        flt.flags = flt.flags.union(FilterFlags::HAS_X_IN_MANA_COST);
        return Ok(true);
    }
    if ts.eat_phrase(&["no", "abilities"]) {
        flt.predicates
            .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                Box::new(ObjectFilter {
                    is_ability: true,
                    ..Default::default()
                }),
            ))));
        return Ok(true);
    }
    if ts.eat_word("abilities") {
        flt.is_ability = true;
        return Ok(true);
    }
    {
        let ncp = ts.checkpoint();
        if ts.eat_word("no") {
            let ck = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
            {
                if let Some(kind) = ck {
                    flt.predicates
                        .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                            Box::new(ObjectFilter {
                                predicates: vec![FilterPredicate::HasCounterKind(kind)],
                                ..Default::default()
                            }),
                        ))));
                } else {
                    flt.flags = flt.flags.union(FilterFlags::WITHOUT_COUNTERS);
                }
                return Ok(true);
            }
        }
        ts.restore(ncp);
    }
    if ts.eat_phrase(&["the", "lowest"])
        && let Some(stat) = parse_single_stat_kind(ts)
    {
        if ts.eat_phrase(&["or", "tied", "for", "the", "lowest"])
            || ts.eat_phrase(&["or", "tied", "for", "lowest"])
        {
            let _ = parse_single_stat_kind(ts);
        }
        let scope = if ts.eat_word("among") {
            try_parse(ts, parse_object_filter).unwrap_or_default()
        } else {
            ObjectFilter::default()
        };
        flt.predicates.push(FilterPredicate::LeastStat {
            stat,
            scope: Box::new(scope),
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["the", "highest"])
        && let Some(stat) = parse_single_stat_kind(ts)
    {
        if ts.eat_phrase(&["or", "tied", "for", "the", "highest"])
            || ts.eat_phrase(&["or", "tied", "for", "highest"])
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
        return Ok(true);
    }
    if ts.eat_phrase(&["a", "different", "name", "than"])
        && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
    {
        flt.predicates.push(FilterPredicate::SharesPropertyWith {
            property: SharedProperty::Name,
            reference: Box::new(ref_subj),
            negated: true,
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["those", "names"]) {
        flt.predicates.push(FilterPredicate::OfChosenName);
        return Ok(true);
    }
    {
        let ccp = ts.checkpoint();
        let _ = ts.eat_word("exactly");
        if let Some(n) = ts.next_number(false) {
            if ts.eat_word("colors") || ts.eat_word("color") {
                flt.color_arity = Some(if n >= 2 {
                    ColorArity::Multicolored
                } else if n == 1 {
                    ColorArity::Monocolored
                } else {
                    ColorArity::Colorless
                });
                return Ok(true);
            }
            if ts.eat_phrase(&["or", "more", "card", "types"])
                || ts.eat_phrase(&["or", "more", "creature", "types"])
                || ts.eat_phrase(&["or", "more", "subtypes"])
            {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
                return Ok(true);
            }
            if ts.eat_phrase(&["or", "more"])
                && let Some(col_w) = ts.peek_word()
                && let Some(col) = lookup_color(&col_w.to_ascii_lowercase())
            {
                ts.next_word();
                if ts.eat_phrase(&["mana", "symbols", "among", "their", "mana", "costs"])
                    || ts.eat_phrase(&["mana", "symbols", "in", "its", "mana", "cost"])
                {
                    flt.any_colors = flt.any_colors.union(col);
                    return Ok(true);
                }
            }
        }
        ts.restore(ccp);
    }
    if ts.eat_phrase(&["the", "most", "votes", "or", "tied", "for", "most", "votes"])
        || ts.eat_phrase(&[
            "the", "most", "votes", "or", "tied", "for", "the", "most", "votes",
        ])
        || ts.eat_phrase(&["the", "most", "votes"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["different", "controllers"]) {
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
        return Ok(true);
    }
    // `with a name originally printed in the <Set Name> expansion`
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["a", "name", "originally", "printed", "in", "the"]) {
            let mut words = 0usize;
            while words < 5 {
                if ts.eat_word("expansion") && words > 0 {
                    flt.predicates.push(FilterPredicate::OfChosenName);
                    return Ok(true);
                }
                if ts.next_word().is_some() {
                    words += 1;
                } else {
                    break;
                }
            }
        }
        ts.restore(ecp);
    }
    // `with a/an <filter> attached to it/them` (note: `parse_object_filter` may already have consumed `attached to it/them`)
    {
        let acp = ts.checkpoint();
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("a") || w.eq_ignore_ascii_case("an"))
            && let Ok(att) = try_parse(ts, parse_object_filter)
            && (att
                .predicates
                .iter()
                .any(|p| matches!(p, FilterPredicate::AttachedTo(_)))
                || ts.eat_phrase(&["attached", "to", "it"])
                || ts.eat_phrase(&["attached", "to", "them"]))
        {
            if att
                .subtypes
                .iter()
                .any(|s| s.eq_ignore_ascii_case("Equipment"))
            {
                flt.flags = flt.flags.union(FilterFlags::EQUIPPED);
            } else {
                flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
            }
            flt.predicates
                .push(FilterPredicate::AttachedTo(Box::new(SubjectRef::Group(
                    Box::new(att),
                ))));
            return Ok(true);
        }
        ts.restore(acp);
    }
    Ok(false)
}

fn try_parse_thats_predicate(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<bool> {
    if ts.eat_word("attacking")
        && let Ok(tgt) = try_parse(ts, parse_subject_ref)
    {
        flt.combat = Some(CombatState::Attacking);
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
        return Ok(true);
    }
    if ts.eat_word("blocking")
        && let Ok(tgt) = try_parse(ts, parse_subject_ref)
    {
        flt.combat = Some(CombatState::Blocking);
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
        return Ok(true);
    }
    if ts.eat_phrase(&["enchanted", "by"])
        && let Ok(aura_flt) = try_parse(ts, parse_object_filter)
    {
        flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
        flt.predicates
            .push(FilterPredicate::AttachedTo(Box::new(SubjectRef::Group(
                Box::new(aura_flt),
            ))));
        return Ok(true);
    }
    if ts.eat_phrase(&["equipped", "by"])
        && let Ok(eq_flt) = try_parse(ts, parse_object_filter)
    {
        flt.flags = flt.flags.union(FilterFlags::EQUIPPED);
        flt.predicates
            .push(FilterPredicate::AttachedTo(Box::new(SubjectRef::Group(
                Box::new(eq_flt),
            ))));
        return Ok(true);
    }
    if ts.eat_phrase(&["exactly", "those", "colors"]) {
        flt.color_arity = Some(ColorArity::Multicolored);
        return Ok(true);
    }
    if ts.eat_word("exactly")
        && let Some(n) = ts.next_number(false)
        && (ts.eat_word("colors") || ts.eat_word("color"))
    {
        flt.color_arity = Some(if n >= 2 {
            ColorArity::Multicolored
        } else if n == 1 {
            ColorArity::Monocolored
        } else {
            ColorArity::Colorless
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["still", "exiled"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ExiledThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["the", "first", "spell", "cast", "this", "turn"]) {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(1));
        return Ok(true);
    }
    if ts.eat_phrase(&["the", "second", "spell", "cast", "this", "turn"]) {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(2));
        return Ok(true);
    }
    if ts.eat_phrase(&["the", "third", "spell", "cast", "this", "turn"]) {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(3));
        return Ok(true);
    }
    // `that's a token or a <subtype/type>` (`Finneas, Ace Archer`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "token", "or"]) {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            if let Ok(other) = try_parse(ts, parse_object_filter) {
                flt.predicates.push(FilterPredicate::AnyOf(vec![
                    ObjectFilter {
                        token: Some(true),
                        ..Default::default()
                    },
                    other,
                ]));
                return Ok(true);
            }
        }
        ts.restore(tcp);
    }
    // `that's not a commander` / `that's no longer an Equipment` / `that's not <flt>`
    if ts.eat_word("not") || ts.eat_phrase(&["no", "longer"]) {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        if ts.eat_word("commander") {
            flt.flags = flt.flags.union(FilterFlags::NOT_COMMANDER);
            return Ok(true);
        }
        if let Some(w) = ts.peek_word() {
            let low = w.to_ascii_lowercase();
            if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                ts.next_word();
                flt.forbidden_subtypes.push(sub.to_string());
                return Ok(true);
            }
            if let Some(ct) = lookup_card_type(&low) {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(ct);
                return Ok(true);
            }
        }
    }
    try_parse_be_adjective(ts, flt)
}

fn try_parse_be_adjective(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<bool> {
    if ts.eat_word("tapped") {
        flt.tapped = Some(true);
        return Ok(true);
    }
    if ts.eat_word("untapped") {
        flt.tapped = Some(false);
        return Ok(true);
    }
    if ts.eat_word("modified") {
        flt.flags = flt.flags.union(FilterFlags::MODIFIED);
        return Ok(true);
    }
    if ts.eat_word("face-down") || ts.eat_phrase(&["face", "down"]) {
        flt.flags = flt.flags.union(FilterFlags::FACE_DOWN);
        return Ok(true);
    }
    if ts.eat_word("face-up") || ts.eat_phrase(&["face", "up"]) {
        flt.flags = flt.flags.union(FilterFlags::FACE_UP);
        return Ok(true);
    }
    if ts.eat_word("colorless") {
        flt.color_arity = Some(ColorArity::Colorless);
        return Ok(true);
    }
    if ts.eat_word("multicolored") {
        flt.color_arity = Some(ColorArity::Multicolored);
        return Ok(true);
    }
    if ts.eat_word("monocolored") {
        flt.color_arity = Some(ColorArity::Monocolored);
        return Ok(true);
    }
    if ts.eat_word("nonland") {
        flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND);
        return Ok(true);
    }
    if ts.eat_word("nontoken") {
        flt.token = Some(false);
        return Ok(true);
    }
    if ts.eat_phrase(&["snow", "or", "could", "produce"]) && ts.eat_mana_cost().is_some() {
        flt.required_supertypes = flt.required_supertypes.union(SupertypeMask::SNOW);
        return Ok(true);
    }
    if ts.eat_phrase(&["every", "basic", "land", "type"]) {
        flt.predicates.push(FilterPredicate::HasBasicLandType);
        return Ok(true);
    }
    Ok(false)
}

fn try_parse_that_relative(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<bool> {
    // `that (is|are) attacking/blocking <subj>` and `that (is|are) <adj>`
    if ts.eat_word("is") || ts.eat_word("are") {
        return try_parse_thats_predicate(ts, flt);
    }
    // `that (isn't|aren't) ...`
    if ts.eat_word("isn't") || ts.eat_word("aren't") {
        if ts.eat_phrase(&["of", "a", "type", "chosen", "this", "way"])
            || ts.eat_phrase(&["of", "the", "chosen", "creature", "type"])
            || ts.eat_phrase(&["of", "the", "chosen", "type"])
        {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::OfChosenCreatureType],
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        if ts.eat_phrase(&["the", "target", "of"])
            && let Ok(src) = try_parse(ts, parse_object_filter)
        {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(src),
                ))));
            return Ok(true);
        }
        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::OfChosenColor],
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        if ts.eat_phrase(&["a", "commander"])
            || ts.eat_word("commanders")
            || ts.eat_word("commander")
        {
            flt.flags = flt.flags.union(FilterFlags::NOT_COMMANDER);
            return Ok(true);
        }
        if ts.eat_word("enchanted") {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        flags: FilterFlags::ENCHANTED,
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        if ts.eat_word("equipped") {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        flags: FilterFlags::EQUIPPED,
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        if ts.eat_word("tapped") {
            flt.tapped = Some(false);
            return Ok(true);
        }
        if ts.eat_word("untapped") {
            flt.tapped = Some(true);
            return Ok(true);
        }
        if ts.eat_word("modified") {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        flags: FilterFlags::MODIFIED,
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        if ts.eat_phrase(&["a", "token"]) || ts.eat_word("tokens") {
            flt.token = Some(false);
            return Ok(true);
        }
        if ts.eat_word("exactly")
            && let Some(_n) = ts.next_number(false)
            && (ts.eat_word("colors") || ts.eat_word("color"))
        {
            flt.color_arity = Some(ColorArity::Colored);
            return Ok(true);
        }
        return Ok(false);
    }

    // `that shares? a permanent type with <subj>` / `that shares? one of those types with <subj>`
    if ts.eat_phrase(&["shares", "a", "permanent", "type", "with"])
        || ts.eat_phrase(&["share", "a", "permanent", "type", "with"])
        || ts.eat_phrase(&["shares", "one", "of", "those", "types", "with"])
        || ts.eat_phrase(&["share", "one", "of", "those", "types", "with"])
    {
        let ref_subj = parse_subject_ref(ts)?;
        flt.predicates.push(FilterPredicate::SharesPropertyWith {
            property: SharedProperty::PermanentType,
            reference: Box::new(ref_subj),
            negated: false,
        });
        return Ok(true);
    }

    // `that (doesn't|don't) ...`
    if ts.eat_word("doesn't") || ts.eat_word("don't") {
        if ts.eat_phrase(&["have", "a", "name"]) {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::HasNameSticker],
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        if ts.eat_phrase(&["have", "the", "same", "name", "as"])
            && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
        {
            flt.predicates.push(FilterPredicate::SharesPropertyWith {
                property: SharedProperty::Name,
                reference: Box::new(ref_subj),
                negated: true,
            });
            return Ok(true);
        }
        if ts.eat_phrase(&["share", "a"]) || ts.eat_phrase(&["share", "an"]) {
            let prop = if ts.eat_phrase(&["land", "type"]) {
                Some(SharedProperty::LandType)
            } else if ts.eat_phrase(&["creature", "type"]) {
                Some(SharedProperty::CreatureType)
            } else if ts.eat_phrase(&["card", "type"]) {
                Some(SharedProperty::CardType)
            } else if ts.eat_phrase(&["permanent", "type"]) {
                Some(SharedProperty::PermanentType)
            } else if ts.eat_word("color") {
                Some(SharedProperty::Color)
            } else {
                None
            };
            if let Some(property) = prop
                && ts.eat_word("with")
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property,
                    reference: Box::new(ref_subj),
                    negated: true,
                });
                return Ok(true);
            }
        }
        if ts.eat_word("have")
            && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
        {
            flt.forbidden_keywords
                .extend(kws.into_iter().map(|k| k.kind));
            return Ok(true);
        }
        return Ok(false);
    }

    if ts.eat_phrase(&["share", "no", "creature", "types"])
        || ts.eat_phrase(&["shares", "no", "creature", "types"])
    {
        flt.predicates.push(FilterPredicate::SharesPropertyWith {
            property: SharedProperty::CreatureType,
            reference: Box::new(SubjectRef::SelfObject),
            negated: true,
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["share", "no", "colors"]) || ts.eat_phrase(&["shares", "no", "colors"]) {
        flt.predicates.push(FilterPredicate::SharesPropertyWith {
            property: SharedProperty::Color,
            reference: Box::new(SubjectRef::SelfObject),
            negated: true,
        });
        return Ok(true);
    }

    // `that each have ...` / `that have different names|powers`
    if ts.eat_phrase(&["each", "have"]) {
        if ts.eat_phrase(&["a", "basic", "land", "type"]) {
            flt.predicates.push(FilterPredicate::HasBasicLandType);
            return Ok(true);
        }
        if ts.eat_phrase(&["different", "names"]) {
            flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
            return Ok(true);
        }
        if ts.eat_phrase(&["a", "different"]) {
            flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
        }
        if let Some(stat) = parse_single_stat_kind(ts) {
            let (op, val) = parse_stat_comparison(ts)?;
            flt.stat_bounds.push(StatBound {
                stat,
                op,
                value: Box::new(val),
            });
            return Ok(true);
        }
        return Ok(false);
    }
    if ts.eat_phrase(&["have", "different", "names"]) {
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
        return Ok(true);
    }
    if ts.eat_phrase(&["have", "different", "powers"]) {
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_POWERS);
        return Ok(true);
    }

    // `that (was|were) ...` past-tense relative predicates
    if ts.eat_word("was") || ts.eat_word("were") {
        if ts.eat_phrase(&["attached", "to"])
            && let Ok(host) = try_parse(ts, parse_subject_ref)
        {
            flt.predicates
                .push(FilterPredicate::AttachedTo(Box::new(host)));
            return Ok(true);
        }
        if ts.eat_phrase(&["put", "into"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
            && ts.eat_phrase(&["this", "way"])
        {
            flt.zones.push(zr);
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DestroyedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["put", "there", "from", "anywhere", "this", "turn"]) {
            flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
            return Ok(true);
        }
        if ts.eat_phrase(&[
            "discarded",
            "or",
            "put",
            "there",
            "from",
            "a",
            "library",
            "this",
            "turn",
        ]) || ts.eat_phrase(&["discarded", "this", "turn"])
            || ts.eat_phrase(&["discarded", "this", "way"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["blocked", "by"])
            && let Ok(blocker) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["this", "combat"]);
            flt.combat = Some(CombatState::Blocked);
            flt.predicates
                .push(FilterPredicate::TargetsSubject(Box::new(blocker)));
            return Ok(true);
        }
        if ts.eat_phrase(&["destroyed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DestroyedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["sacrificed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::SacrificedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["revealed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::RevealedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["cast", "this", "way"]) || ts.eat_phrase(&["cast", "this", "turn"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CastThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["created", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CreatedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["chosen", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ThatMany,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["milled", "this", "turn"]) || ts.eat_phrase(&["milled", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::MilledThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&[
            "exiled",
            "under",
            "your",
            "opponents'",
            "control",
            "this",
            "turn",
        ]) || ts.eat_phrase(&["exiled", "this", "turn"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["activated", "this", "turn"]) {
            flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
            return Ok(true);
        }
        return Ok(false);
    }

    // `that had [a/an] [<kind>] counter(s) put on it/them this way` (`Fangs of Kalonia`, `Spectacular Showdown`, `Agitator Ant`)
    if ts.eat_word("had") {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let ck = opt(ts, parse_counter_kind);
        if (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["put", "on", "them", "this", "way"])
                || ts.eat_phrase(&["put", "on", "it", "this", "way"]))
        {
            flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
            if let Some(kind) = ck {
                flt.predicates.push(FilterPredicate::HasCounterKind(kind));
            }
            return Ok(true);
        }
        return Ok(false);
    }
    // `that (has|have) <kind> counter(s) on (it|them)` / `that (has|have) <stat> <cmp>` / `that (has|have) an activated ability with {T} in its cost`
    if ts.eat_word("has") || ts.eat_word("have") {
        if ts.eat_phrase(&["an", "activated", "ability", "with"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Tap))
            )
        {
            ts.next_tok();
            if ts.eat_phrase(&["in", "its", "cost"]) {
                flt.is_ability = true;
                return Ok(true);
            }
        }
        if let Some(stat) = parse_single_stat_kind(ts)
            && let Ok((op, val)) = try_parse(ts, parse_stat_comparison)
        {
            flt.stat_bounds.push(StatBound {
                stat,
                op,
                value: Box::new(val),
            });
            return Ok(true);
        }
        if let Ok(ck) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && (ts.eat_phrase(&["on", "them"]) || ts.eat_phrase(&["on", "it"]))
        {
            flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
            flt.predicates.push(FilterPredicate::HasCounterKind(ck));
            return Ok(true);
        }
        return Ok(false);
    }
    // `that didn't attack [or enter] [this turn]` / `that didn't enter this turn`
    if ts.eat_word("didn't") {
        if ts.eat_phrase(&["attack", "or", "enter", "this", "turn"])
            || ts.eat_phrase(&["enter", "this", "turn"])
            || ts.eat_word("attack")
        {
            flt.predicates
                .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                    Box::new(ObjectFilter {
                        flags: FilterFlags::ATTACKED_THIS_TURN,
                        ..Default::default()
                    }),
                ))));
            return Ok(true);
        }
        return Ok(false);
    }
    // `that wasn't/weren't put there this combat`
    if (ts.eat_word("wasn't") || ts.eat_word("weren't"))
        && ts.eat_phrase(&["put", "there", "this", "combat"])
    {
        flt.predicates
            .push(FilterPredicate::Exclude(Box::new(SubjectRef::Group(
                Box::new(ObjectFilter {
                    flags: FilterFlags::DIED_THIS_TURN,
                    ..Default::default()
                }),
            ))));
        return Ok(true);
    }
    // `that hasn't/haven't been chosen this way` / `that hasn't been phased out with this Attraction`
    if (ts.eat_word("hasn't") || ts.eat_word("haven't"))
        && (ts.eat_phrase(&["been", "chosen", "this", "way"])
            || ts.eat_phrase(&["been", "phased", "out", "with", "this", "attraction"]))
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["attacked", "or", "blocked", "this", "turn"]) {
        flt.flags = flt.flags.union(FilterFlags::ATTACKED_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&["blocked", "or", "was", "blocked", "this", "turn"])
        || ts.eat_phrase(&["blocked", "or", "were", "blocked", "this", "turn"])
    {
        flt.combat = Some(CombatState::Blocking);
        return Ok(true);
    }
    if ts.eat_word("blocked")
        && (ts.eat_tilde() || ts.eat_word("it"))
        && (ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["this", "combat"]))
    {
        flt.combat = Some(CombatState::BlockingSelf);
        return Ok(true);
    }
    if ts.eat_phrase(&["became", "a", "creature", "this", "way"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::CreatedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["phased", "out", "this", "way"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ExiledThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["died", "this", "way"]) {
        flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&[
        "left",
        "the",
        "battlefield",
        "under",
        "your",
        "control",
        "this",
        "turn",
    ]) {
        flt.controller = Some(ControllerRelation::You);
        flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&["left", "the", "battlefield", "this", "turn"]) {
        flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&["you", "cycled", "or", "discarded", "this", "turn"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::DiscardedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["can", "target", "only"])
        && let Ok(tgt) = try_parse(ts, parse_object_filter)
    {
        flt.flags = flt.flags.union(FilterFlags::SINGLE_TARGET);
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(
                SubjectRef::Group(Box::new(tgt)),
            )));
        return Ok(true);
    }
    // `that shares at least one creature type with <subj>` (`Coat of Arms`)
    if (ts.eat_phrase(&["shares", "at", "least", "one", "creature", "type", "with"])
        || ts.eat_phrase(&["share", "at", "least", "one", "creature", "type", "with"]))
        && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
    {
        flt.predicates.push(FilterPredicate::SharesPropertyWith {
            property: SharedProperty::CreatureType,
            reference: Box::new(ref_subj),
            negated: false,
        });
        return Ok(true);
    }
    // `that shares a color with the mana spent on this activation cost` / `with the spell most recently cast this turn`
    if (ts.eat_phrase(&["shares", "a", "color", "with"])
        || ts.eat_phrase(&["share", "a", "color", "with"]))
        && (ts.eat_phrase(&["the", "mana", "spent", "on", "this", "activation", "cost"])
            || ts.eat_phrase(&["the", "spell", "most", "recently", "cast", "this", "turn"]))
    {
        flt.predicates.push(FilterPredicate::SharesPropertyWith {
            property: SharedProperty::Color,
            reference: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatSpell)),
            negated: false,
        });
        return Ok(true);
    }
    // `that could produce <mana>` / `that could produce any type of mana that land could produce`
    if ts.eat_phrase(&["could", "produce"]) {
        if ts.eat_mana_cost().is_some()
            || ts.eat_phrase(&[
                "any", "type", "of", "mana", "that", "land", "could", "produce",
            ])
        {
            flt.predicates.push(FilterPredicate::HasManaAbility);
            return Ok(true);
        }
        return Ok(false);
    }
    if ts.eat_phrase(&["fought", "this", "turn"]) {
        flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&["dealt", "damage", "to"]) {
        if (ts.eat_tilde() || ts.eat_word("it") || try_parse(ts, parse_player_ref).is_ok())
            && ts.eat_phrase(&["this", "turn"])
        {
            flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
            return Ok(true);
        }
        return Ok(false);
    }
    if ts.eat_phrase(&["dealt", "combat", "damage", "to"]) {
        if try_parse(ts, parse_player_ref).is_ok() && ts.eat_phrase(&["this", "turn"]) {
            flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
            return Ok(true);
        }
        return Ok(false);
    }
    if ts.eat_phrase(&["couldn't", "attack"]) {
        flt.combat = Some(CombatState::NotAttacking);
        return Ok(true);
    }
    if ts.eat_phrase(&["spell", "could", "target"])
        || ts.eat_phrase(&["ability", "could", "target"])
        || ts.eat_phrase(&["that", "spell", "could", "target"])
        || ts.eat_phrase(&["the", "spell", "could", "target"])
        || ts.eat_phrase(&["that", "ability", "could", "target"])
        || ts.eat_phrase(&["the", "ability", "could", "target"])
    {
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(
                SubjectRef::Anaphora(AnaphoraRef::ThatSpell),
            )));
        return Ok(true);
    }
    // `that you've/you put one or more [<kind>] counter(s) on this turn`
    {
        let pcp = ts.checkpoint();
        if (ts.eat_phrase(&["you've", "put"]) || ts.eat_phrase(&["you", "put"]))
            && (ts.eat_phrase(&["one", "or", "more"])
                || ts.eat_word("a")
                || ts.eat_word("an")
                || ts.next_number(false).is_some())
        {
            let ck = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["on", "this", "turn"])
            {
                flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                if let Some(kind) = ck {
                    flt.predicates.push(FilterPredicate::HasCounterKind(kind));
                }
                return Ok(true);
            }
        }
        ts.restore(pcp);
    }
    if ts.eat_phrase(&["could", "enchant"])
        && let Ok(host) = try_parse(ts, parse_subject_ref)
    {
        flt.predicates
            .push(FilterPredicate::AttachedTo(Box::new(host)));
        return Ok(true);
    }
    if (ts.eat_tilde() && ts.eat_phrase(&["could", "enchant"]))
        || ts.eat_phrase(&["it", "could", "enchant"])
    {
        flt.predicates.push(FilterPredicate::AttachedTo(Box::new(
            SubjectRef::SelfObject,
        )));
        return Ok(true);
    }
    // `that entered since your last turn ended` / `that entered [the battlefield] under <owner> control this turn`
    if ts.eat_word("entered") {
        if ts.eat_phrase(&["since", "your", "last", "turn", "ended"]) {
            flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
            return Ok(true);
        }
        let _ = ts.eat_phrase(&["the", "battlefield"]);
        if ts.eat_phrase(&["under", "your", "control", "this", "turn"]) {
            flt.controller = Some(ControllerRelation::You);
            flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
            return Ok(true);
        }
        if ts.eat_phrase(&["under", "an", "opponent's", "control", "this", "turn"])
            || ts.eat_phrase(&["under", "your", "opponents'", "control", "this", "turn"])
        {
            flt.controller = Some(ControllerRelation::Opponent);
            flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
            return Ok(true);
        }
        if ts.eat_phrase(&["under", "their", "control", "this", "turn"])
            || ts.eat_phrase(&["under", "that", "player's", "control", "this", "turn"])
        {
            flt.controller = Some(ControllerRelation::ThatPlayer);
            flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
            return Ok(true);
        }
    }

    Ok(false)
}

fn try_parse_whose_relative(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<bool> {
    if ts.eat_phrase(&["coin", "comes", "up", "tails"])
        || ts.eat_phrase(&["coin", "comes", "up", "heads"])
    {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_phrase(&["controller", "was", "dealt", "combat", "damage", "by"])
        && (ts.eat_tilde() || ts.eat_word("it"))
        && ts.eat_phrase(&["this", "turn"])
    {
        flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
        return Ok(true);
    }
    if ts.eat_phrase(&["controller", "has"])
        && ts.next_number(false).is_some()
        && ts.eat_phrase(&["or", "more", "cards", "in", "their", "graveyard"])
    {
        flt.zones.push(ZoneRef {
            zone: ZoneKind::Graveyard,
            owner: ZoneOwner::ThatPlayer,
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["controller", "controls"])
        && let Ok(req_flt) = try_parse(ts, parse_object_filter)
    {
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(
                SubjectRef::Group(Box::new(req_flt)),
            )));
        return Ok(true);
    }
    if ts.eat_phrase(&["power", "and", "toughness", "aren't", "equal"]) {
        flt.stat_bounds.push(StatBound {
            stat: StatKind::Power,
            op: CompareOp::Lt,
            value: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                stat: StatKind::Toughness,
            }),
        });
        return Ok(true);
    }
    Ok(false)
}

fn player_ref_to_controller(p: PlayerRef) -> ControllerRelation {
    match p {
        PlayerRef::You => ControllerRelation::You,
        PlayerRef::TargetOpponent => ControllerRelation::TargetOpponent,
        PlayerRef::TargetPlayer => ControllerRelation::TargetPlayer,
        PlayerRef::EachOpponent | PlayerRef::EachOtherOpponent => ControllerRelation::Opponents,
        PlayerRef::AnyOpponent => ControllerRelation::Opponent,
        PlayerRef::DefendingPlayer => ControllerRelation::DefendingPlayer,
        PlayerRef::ActivePlayer => ControllerRelation::ActivePlayer,
        PlayerRef::ChosenPlayer => ControllerRelation::ChosenPlayer,
        PlayerRef::EachOtherPlayer => ControllerRelation::NotYou,
        _ => ControllerRelation::ThatPlayer,
    }
}

fn player_ref_to_owner(p: PlayerRef) -> ControllerRelation {
    match p {
        PlayerRef::You => ControllerRelation::OwnerYou,
        PlayerRef::EachOpponent
        | PlayerRef::EachOtherOpponent
        | PlayerRef::AnyOpponent
        | PlayerRef::TargetOpponent => ControllerRelation::OwnerOpponent,
        PlayerRef::EachOtherPlayer => ControllerRelation::NotYou,
        _ => ControllerRelation::OwnerThatPlayer,
    }
}
