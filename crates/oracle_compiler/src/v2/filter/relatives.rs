//! Extended post-nominal relative clauses, choice qualifiers, exclusions, and
//! stat/characteristic predicates for [`ObjectFilter`].

use crate::ast::{
    CardTypeMask, ColorArity, CombatState, CompareOp, ControllerRelation, StatKind, SupertypeMask,
};
use crate::v2::ir::{
    AnaphoraRef, FilterFlags, FilterPredicate, ManaSymbol, ObjectFilter, PlayerRef,
    PriorActionKind, SharedProperty, SubjectRef, ValueExpr, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::vocab::{lookup_card_type, lookup_color};

use super::post_modifiers::{parse_single_stat_kind, parse_stat_comparison};
use super::{
    eat_phrase_groups, eat_phrase_table, parse_counter_kind, parse_keyword_list,
    parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref, push_exclude_filter,
    push_shares_property, push_stat_bound,
};

fn color_arity_from_count(n: i32) -> ColorArity {
    if n >= 2 {
        ColorArity::Multicolored
    } else if n == 1 {
        ColorArity::Monocolored
    } else {
        ColorArity::Colorless
    }
}

/// Try to consume one extended post-nominal modifier or relative clause on `flt`.
/// Every speculative branch checkpoints and restores on failure to preserve zero regressions.
pub(super) fn try_parse_extended_post_modifier(
    ts: &mut TokenStream<'_>,
    flt: &mut ObjectFilter,
) -> PResult<bool> {
    // 1. Past-tense LKI controller/owner clauses & additional ownership/control forms
    const PAST_CTRL: &[(&[&[&str]], ControllerRelation)] = &[
        (&[&["you", "controlled"]], ControllerRelation::You),
        (
            &[
                &["your", "opponents", "controlled"],
                &["opponents", "controlled"],
            ],
            ControllerRelation::Opponents,
        ),
        (
            &[&["an", "opponent", "controlled"]],
            ControllerRelation::Opponent,
        ),
        (
            &[
                &["that", "player", "controlled"],
                &["that", "opponent", "controlled"],
            ],
            ControllerRelation::ThatPlayer,
        ),
    ];
    if let Some(ctrl) = eat_phrase_groups(ts, PAST_CTRL) {
        flt.controller = Some(ctrl);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["your", "opponents", "own"]) {
        flt.owner = Some(ControllerRelation::OwnerOpponent);
        return Ok(true);
    }
    const PAST_OWNER: &[(&[&[&str]], ControllerRelation)] = &[
        (&[&["you", "owned"]], ControllerRelation::OwnerYou),
        (
            &[&["they", "owned"], &["that", "player", "owned"]],
            ControllerRelation::OwnerThatPlayer,
        ),
    ];
    if let Some(owner) = eat_phrase_groups(ts, PAST_OWNER) {
        flt.owner = Some(owner);
        flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
        return Ok(true);
    }
    if ts.eat_phrase(&["or", "control"]) && flt.owner == Some(ControllerRelation::OwnerYou) {
        flt.controller = Some(ControllerRelation::You);
        return Ok(true);
    }
    const CTRL_REL_1: &[(&[&[&str]], ControllerRelation)] = &[
        (
            &[&[
                "that",
                "opponent",
                "or",
                "that",
                "planeswalker's",
                "controller",
                "controls",
            ]],
            ControllerRelation::ThatPlayerOrPlaneswalkerController,
        ),
        (
            &[
                &["that", "player", "casts"],
                &["they", "cast"],
                &["enchanted", "player", "casts"],
            ],
            ControllerRelation::ThatPlayer,
        ),
        (
            &[&["a", "player", "casts"], &["players", "cast"]],
            ControllerRelation::ActivePlayer,
        ),
        (&[&["you", "already", "control"]], ControllerRelation::You),
    ];
    if let Some(ctrl) = eat_phrase_groups(ts, CTRL_REL_1) {
        flt.controller = Some(ctrl);
        return Ok(true);
    }
    if ts.eat_phrase(&["you", "neither", "own", "nor", "control"]) {
        flt.controller = Some(ControllerRelation::NotYou);
        flt.owner = Some(ControllerRelation::NotYou);
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["they", "don't", "control"],
        &["that", "player", "doesn't", "control"],
    ]) {
        flt.controller = Some(ControllerRelation::NotYou);
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["they", "don't", "own"],
        &["that", "player", "doesn't", "own"],
    ]) {
        flt.owner = Some(ControllerRelation::NotYou);
        return Ok(true);
    }
    const CTRL_REL_2: &[(&[&[&str]], ControllerRelation)] = &[
        (
            &[
                &["they", "protect"],
                &["that", "player", "protects"],
                &["an", "opponent", "protects"],
                &["your", "opponents", "protect"],
            ],
            ControllerRelation::Opponent,
        ),
        (&[&["you", "protect"]], ControllerRelation::You),
        (
            &[
                &["that", "one", "of", "your", "opponents", "controls"],
                &["each", "opponent", "controls"],
                &["the", "other", "controls"],
            ],
            ControllerRelation::Opponent,
        ),
        (
            &[
                &["a", "player", "controls"],
                &["each", "player", "controls"],
                &["the", "monarch", "controls"],
                &["that", "creature's", "controller", "controls"],
                &["that", "permanent's", "controller", "controls"],
            ],
            ControllerRelation::ThatPlayer,
        ),
        (
            &[
                &["the", "opponent", "to", "your", "left", "controls"],
                &["the", "player", "to", "your", "left", "controls"],
            ],
            ControllerRelation::PlayerToLeft,
        ),
        (
            &[
                &["the", "opponent", "to", "your", "right", "controls"],
                &["the", "player", "to", "your", "right", "controls"],
            ],
            ControllerRelation::PlayerToRight,
        ),
    ];
    if let Some(ctrl) = eat_phrase_groups(ts, CTRL_REL_2) {
        flt.controller = Some(ctrl);
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &[
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
        ],
        &[
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
        ],
        &[
            "that",
            "they",
            "controlled",
            "since",
            "the",
            "beginning",
            "of",
            "the",
            "turn",
        ],
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
    if ts.eat_any_phrase(&[
        &["that", "player", "has", "drawn", "this", "turn"],
        &["an", "opponent", "has", "drawn", "this", "turn"],
        &["they", "drew", "this", "way"],
    ]) {
        flt.predicates.push(FilterPredicate::DrawnThisTurn);
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["they", "discarded", "this", "way"],
        &["you", "discarded", "this", "way"],
    ]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::DiscardedThisWay,
        ));
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["they", "revealed", "this", "way"],
        &["you", "revealed", "this", "way"],
        &["they", "revealed"],
        &["you", "revealed"],
        &["revealed", "by", "the", "other", "player"],
        &["revealed", "by", "an", "opponent"],
    ]) {
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
    const CTRL_BY_PHRASES: &[(&[&[&str]], ControllerRelation)] = &[
        (
            &[&[
                "controlled",
                "by",
                "the",
                "next",
                "player",
                "in",
                "the",
                "chosen",
                "direction",
            ]],
            ControllerRelation::NextPlayerInDirection,
        ),
        (
            &[
                &["controlled", "by", "the", "player", "to", "their", "right"],
                &["controlled", "by", "the", "player", "to", "your", "right"],
            ],
            ControllerRelation::PlayerToRight,
        ),
        (
            &[
                &["controlled", "by", "the", "player", "to", "their", "left"],
                &["controlled", "by", "the", "player", "to", "your", "left"],
            ],
            ControllerRelation::PlayerToLeft,
        ),
        (
            &[&[
                "controlled",
                "by",
                "that",
                "player",
                "or",
                "by",
                "that",
                "planeswalker's",
                "controller",
            ]],
            ControllerRelation::ThatPlayerOrPlaneswalkerController,
        ),
    ];
    if let Some(ctrl) = eat_phrase_groups(ts, CTRL_BY_PHRASES) {
        flt.controller = Some(ctrl);
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
    if ts.eat_any_phrase(&[
        &["of", "the", "chosen", "color", "and", "type"],
        &["of", "the", "chosen", "color", "and", "creature", "type"],
    ]) {
        flt.predicates.push(FilterPredicate::OfChosenColor);
        flt.predicates.push(FilterPredicate::OfChosenCreatureType);
        return Ok(true);
    }
    const OF_PLAYER_CHOICE: &[(&[&str], PlayerRef)] = &[
        (
            &["of", "defending", "player's", "choice"],
            PlayerRef::DefendingPlayer,
        ),
        (
            &["of", "target", "opponent's", "choice"],
            PlayerRef::TargetOpponent,
        ),
        (
            &["of", "target", "player's", "choice"],
            PlayerRef::TargetPlayer,
        ),
        (&["of", "that", "player's", "choice"], PlayerRef::ThatPlayer),
        (
            &["of", "its", "controller's", "choice"],
            PlayerRef::ThatPlayer,
        ),
    ];
    if let Some(p) = eat_phrase_table(ts, OF_PLAYER_CHOICE) {
        flt.predicates.push(FilterPredicate::OfChosenPlayer(p));
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["of", "the", "color", "of", "your", "choice"],
        &["of", "the", "last", "chosen", "color"],
    ]) {
        flt.predicates.push(FilterPredicate::OfChosenColor);
        return Ok(true);
    }
    if ts.eat_phrase(&["of", "the", "chosen", "kind"]) {
        flt.predicates.push(FilterPredicate::OfChosenCreatureType);
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["of", "each", "card", "type"],
        &["of", "each", "permanent", "type"],
    ]) {
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
    if ts.eat_any_phrase(&[
        &["of", "the", "other", "type"],
        &["of", "a", "type", "chosen", "this", "way"],
    ]) {
        flt.predicates.push(FilterPredicate::OfChosenCreatureType);
        return Ok(true);
    }
    // `of each of the sacrificed creature's colors` / `of each of that creature's colors`
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["of", "each", "of"])
            && ts.eat_any_phrase(&[
                &["the", "sacrificed", "creature's"],
                &["the", "sacrificed", "permanent's"],
                &["that", "creature's"],
                &["that", "permanent's"],
                &["its"],
            ])
            && ts.eat_any_word(&["colors", "color"])
        {
            push_shares_property(
                flt,
                SharedProperty::Color,
                SubjectRef::Anaphora(AnaphoraRef::SacrificedObject),
                false,
            );
            return Ok(true);
        }
        ts.restore(ccp);
    }

    // 3. Prior-action / craft / pile / created / zone participles
    const EXT_PRIOR_ACTION: &[(&[&[&str]], PriorActionKind)] = &[
        (&[&["chosen", "this", "way"]], PriorActionKind::ThatMany),
        (
            &[
                &["returned", "to", "your", "hand", "this", "way"],
                &["returned", "to", "its", "owner's", "hand", "this", "way"],
                &["returned", "to", "their", "owners'", "hands", "this", "way"],
                &["put", "onto", "the", "battlefield", "this", "way"],
            ],
            PriorActionKind::ReturnedThisWay,
        ),
        (
            &[
                &["sacrificed", "as", "it", "entered"],
                &["devoured", "this", "way"],
            ],
            PriorActionKind::SacrificedThisWay,
        ),
        (
            &[&["cast", "before", "that", "spell", "this", "turn"]],
            PriorActionKind::CastThisWay,
        ),
    ];
    if let Some(kind) = eat_phrase_groups(ts, EXT_PRIOR_ACTION) {
        flt.predicates
            .push(FilterPredicate::AffectedByPriorAction(kind));
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
    if ts.eat_any_phrase(&[
        &[
            "dealt", "damage", "by", "equipped", "creature", "this", "turn",
        ],
        &["dealt", "damage", "this", "turn"],
    ]) {
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
    if ts.eat_any_phrase(&[&["directly", "above"], &["above"]])
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
    // `from among <object_filter>`
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
    if ts.eat_any_phrase(&[
        &["in", "the", "pile", "of", "that", "player's", "choice"],
        &["in", "the", "pile", "of", "an", "opponent's", "choice"],
    ]) {
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
    if ts.eat_any_phrase(&[
        &["not", "chosen", "by", "any", "player", "this", "way"],
        &["not", "chosen", "this", "way"],
        &["countered", "this", "way"],
    ]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ThatMany,
        ));
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["in", "the", "chosen", "piles"],
        &["in", "the", "chosen", "pile"],
        &["in", "the", "other", "piles"],
        &["in", "the", "other", "pile"],
    ]) {
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
                    push_exclude_filter(flt, excl);
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
                        push_exclude_filter(flt, ex_flt);
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
                        push_exclude_filter(flt, excl);
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
        if ts.eat_any_phrase(&[
            &["without", "mana", "abilities"],
            &["without", "a", "mana", "ability"],
        ]) {
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
            if ts.eat_any_phrase(&[
                &["in", "its", "mana", "cost"],
                &["in", "its", "mana", "costs"],
                &["in", "their", "mana", "cost"],
                &["in", "their", "mana", "costs"],
            ]) {
                push_exclude_filter(
                    flt,
                    ObjectFilter {
                        flags: FilterFlags::HAS_X_IN_MANA_COST,
                        ..Default::default()
                    },
                );
                return Ok(true);
            }
        }
        ts.restore(wcp);
    }

    // 6-7. Extended `with` / `that's` / `that` / `whose` clauses
    type RelParser = fn(&mut TokenStream<'_>, &mut ObjectFilter) -> PResult<bool>;
    const REL_DISPATCH: &[(&str, RelParser)] = &[
        ("with", try_parse_extended_with_body),
        ("that's", try_parse_thats_predicate),
        ("that", try_parse_that_relative),
        ("whose", try_parse_whose_relative),
    ];
    for &(lead, parser) in REL_DISPATCH {
        if ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case(lead)) {
            let cp = ts.checkpoint();
            ts.eat_word(lead);
            if parser(ts, flt)? {
                return Ok(true);
            }
            ts.restore(cp);
        }
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
        push_stat_bound(flt, StatKind::Power, op, val);
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
                let subj_opt = if ts.eat_any_phrase(&[
                    &["that", "creature's"],
                    &["that", "permanent's"],
                    &["its"],
                    &["their"],
                ]) {
                    Some(SubjectRef::Anaphora(AnaphoraRef::ThatCreature))
                } else if ts.eat_tilde() && ts.eat_word("'s") {
                    Some(SubjectRef::SelfObject)
                } else {
                    None
                };
                if let Some(subj) = subj_opt {
                    let rhs_stat = parse_single_stat_kind(ts).unwrap_or(stat);
                    push_stat_bound(
                        flt,
                        stat,
                        op,
                        ValueExpr::ObjectStat {
                            subject: Box::new(subj),
                            stat: rhs_stat,
                        },
                    );
                    return Ok(true);
                }
            }
        }
        ts.restore(bcp);
    }
    // `with lesser mana value than <subj>`
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["lesser", "mana", "value", "than"])
            && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
        {
            push_stat_bound(
                flt,
                StatKind::ManaValue,
                CompareOp::Lt,
                ValueExpr::ObjectStat {
                    subject: Box::new(ref_subj),
                    stat: StatKind::ManaValue,
                },
            );
            return Ok(true);
        }
        ts.restore(lcp);
    }
    // `with equal power/toughness/mana value`
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("equal")
            && let Some(stat) = parse_single_stat_kind(ts)
        {
            push_stat_bound(
                flt,
                stat,
                CompareOp::Eq,
                ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    stat,
                },
            );
            return Ok(true);
        }
        ts.restore(ecp);
    }
    type RelStatBoundEntry = (
        &'static [&'static [&'static str]],
        (StatKind, CompareOp, bool),
    );
    const REL_STAT_BOUNDS: &[RelStatBoundEntry] = &[
        (
            &[&["equal", "or", "lesser", "power"]],
            (StatKind::Power, CompareOp::Le, true),
        ),
        (
            &[
                &["equal", "or", "lesser", "toughness"],
                &["lesser", "toughness"],
            ],
            (StatKind::Toughness, CompareOp::Le, true),
        ),
        (
            &[
                &["greater", "mana", "value"],
                &["equal", "or", "greater", "mana", "value"],
            ],
            (StatKind::ManaValue, CompareOp::Gt, false),
        ),
        (
            &[&["equal", "or", "greater", "power"]],
            (StatKind::Power, CompareOp::Ge, true),
        ),
    ];
    if let Some((stat, op, is_self)) = eat_phrase_groups(ts, REL_STAT_BOUNDS) {
        let subject = if is_self {
            SubjectRef::SelfObject
        } else {
            SubjectRef::Anaphora(AnaphoraRef::It)
        };
        push_stat_bound(
            flt,
            stat,
            op,
            ValueExpr::ObjectStat {
                subject: Box::new(subject),
                stat,
            },
        );
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["an", "even", "mana", "value"],
        &["even", "mana", "values"],
        &["even", "mana", "value"],
    ]) {
        push_stat_bound(flt, StatKind::ManaValue, CompareOp::Eq, ValueExpr::Const(0));
        return Ok(true);
    }
    if ts.eat_any_phrase(&[
        &["an", "odd", "mana", "value"],
        &["odd", "mana", "values"],
        &["odd", "mana", "value"],
    ]) {
        push_stat_bound(flt, StatKind::ManaValue, CompareOp::Eq, ValueExpr::Const(1));
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
        push_exclude_filter(
            flt,
            ObjectFilter {
                is_ability: true,
                ..Default::default()
            },
        );
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
            if ts.eat_any_word(&["counters", "counter"])
                && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
            {
                if let Some(kind) = ck {
                    push_exclude_filter(
                        flt,
                        ObjectFilter {
                            predicates: vec![FilterPredicate::HasCounterKind(kind)],
                            ..Default::default()
                        },
                    );
                } else {
                    flt.flags = flt.flags.union(FilterFlags::WITHOUT_COUNTERS);
                }
                return Ok(true);
            }
        }
        ts.restore(ncp);
    }
    let extremum = if ts.eat_phrase(&["the", "lowest"]) {
        Some(false)
    } else if ts.eat_phrase(&["the", "highest"]) {
        Some(true)
    } else {
        None
    };
    if let Some(is_highest) = extremum
        && let Some(stat) = parse_single_stat_kind(ts)
    {
        let tied = if is_highest {
            ts.eat_any_phrase(&[
                &["or", "tied", "for", "the", "highest"],
                &["or", "tied", "for", "highest"],
            ])
        } else {
            ts.eat_any_phrase(&[
                &["or", "tied", "for", "the", "lowest"],
                &["or", "tied", "for", "lowest"],
            ])
        };
        if tied {
            let _ = parse_single_stat_kind(ts);
        }
        let scope = Box::new(if ts.eat_word("among") {
            try_parse(ts, parse_object_filter).unwrap_or_default()
        } else {
            ObjectFilter::default()
        });
        flt.predicates.push(if is_highest {
            FilterPredicate::GreatestStat { stat, scope }
        } else {
            FilterPredicate::LeastStat { stat, scope }
        });
        return Ok(true);
    }
    if ts.eat_phrase(&["a", "different", "name", "than"])
        && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
    {
        push_shares_property(flt, SharedProperty::Name, ref_subj, true);
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
            if ts.eat_any_word(&["colors", "color"]) {
                flt.color_arity = Some(color_arity_from_count(n));
                return Ok(true);
            }
            if ts.eat_any_phrase(&[
                &["or", "more", "card", "types"],
                &["or", "more", "creature", "types"],
                &["or", "more", "subtypes"],
            ]) {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
                return Ok(true);
            }
            if ts.eat_phrase(&["or", "more"])
                && let Some(col_w) = ts.peek_word()
                && let Some(col) = lookup_color(&col_w.to_ascii_lowercase())
            {
                ts.next_word();
                if ts.eat_any_phrase(&[
                    &["mana", "symbols", "among", "their", "mana", "costs"],
                    &["mana", "symbols", "in", "its", "mana", "cost"],
                ]) {
                    flt.any_colors = flt.any_colors.union(col);
                    return Ok(true);
                }
            }
        }
        ts.restore(ccp);
    }
    if ts.eat_any_phrase(&[
        &["the", "most", "votes", "or", "tied", "for", "most", "votes"],
        &[
            "the", "most", "votes", "or", "tied", "for", "the", "most", "votes",
        ],
        &["the", "most", "votes"],
    ]) {
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
                || ts.eat_any_phrase(&[&["attached", "to", "it"], &["attached", "to", "them"]]))
        {
            let flag = if att
                .subtypes
                .iter()
                .any(|s| s.eq_ignore_ascii_case("Equipment"))
            {
                FilterFlags::EQUIPPED
            } else {
                FilterFlags::ENCHANTED
            };
            flt.flags = flt.flags.union(flag);
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
    if let Some(combat) = eat_phrase_table(
        ts,
        &[
            (&["attacking"], CombatState::Attacking),
            (&["blocking"], CombatState::Blocking),
        ],
    ) && let Ok(tgt) = try_parse(ts, parse_subject_ref)
    {
        flt.combat = Some(combat);
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
        return Ok(true);
    }
    if let Some(flag) = eat_phrase_table(
        ts,
        &[
            (&["enchanted", "by"], FilterFlags::ENCHANTED),
            (&["equipped", "by"], FilterFlags::EQUIPPED),
        ],
    ) && let Ok(att_flt) = try_parse(ts, parse_object_filter)
    {
        flt.flags = flt.flags.union(flag);
        flt.predicates
            .push(FilterPredicate::AttachedTo(Box::new(SubjectRef::Group(
                Box::new(att_flt),
            ))));
        return Ok(true);
    }
    if ts.eat_phrase(&["exactly", "those", "colors"]) {
        flt.color_arity = Some(ColorArity::Multicolored);
        return Ok(true);
    }
    if ts.eat_word("exactly")
        && let Some(n) = ts.next_number(false)
        && ts.eat_any_word(&["colors", "color"])
    {
        flt.color_arity = Some(color_arity_from_count(n));
        return Ok(true);
    }
    if ts.eat_phrase(&["still", "exiled"]) {
        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
            PriorActionKind::ExiledThisWay,
        ));
        return Ok(true);
    }
    const NTH_SPELL: &[(&[&str], u8)] = &[
        (&["the", "first", "spell", "cast", "this", "turn"], 1),
        (&["the", "second", "spell", "cast", "this", "turn"], 2),
        (&["the", "third", "spell", "cast", "this", "turn"], 3),
    ];
    if let Some(n) = eat_phrase_table(ts, NTH_SPELL) {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(n));
        return Ok(true);
    }
    // `that's a token or a <subtype/type>`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "token", "or"]) {
            let _ = ts.eat_any_word(&["a", "an"]);
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
    if ts.eat_any_phrase(&[&["not"], &["no", "longer"]]) {
        let _ = ts.eat_any_word(&["a", "an"]);
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
    const BE_FLAGS: &[(&[&str], FilterFlags)] = &[
        (&["modified"], FilterFlags::MODIFIED),
        (&["face-down"], FilterFlags::FACE_DOWN),
        (&["face", "down"], FilterFlags::FACE_DOWN),
        (&["face-up"], FilterFlags::FACE_UP),
        (&["face", "up"], FilterFlags::FACE_UP),
    ];
    if let Some(flag) = eat_phrase_table(ts, BE_FLAGS) {
        flt.flags = flt.flags.union(flag);
        return Ok(true);
    }
    const BE_ARITIES: &[(&[&str], ColorArity)] = &[
        (&["colorless"], ColorArity::Colorless),
        (&["multicolored"], ColorArity::Multicolored),
        (&["monocolored"], ColorArity::Monocolored),
    ];
    if let Some(arity) = eat_phrase_table(ts, BE_ARITIES) {
        flt.color_arity = Some(arity);
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
    if ts.eat_any_word(&["is", "are"]) {
        return try_parse_thats_predicate(ts, flt);
    }
    // `that (isn't|aren't) ...`
    if ts.eat_any_word(&["isn't", "aren't"]) {
        if ts.eat_any_phrase(&[
            &["of", "a", "type", "chosen", "this", "way"],
            &["of", "the", "chosen", "creature", "type"],
            &["of", "the", "chosen", "type"],
        ]) {
            push_exclude_filter(
                flt,
                ObjectFilter {
                    predicates: vec![FilterPredicate::OfChosenCreatureType],
                    ..Default::default()
                },
            );
            return Ok(true);
        }
        if ts.eat_phrase(&["the", "target", "of"])
            && let Ok(src) = try_parse(ts, parse_object_filter)
        {
            push_exclude_filter(flt, src);
            return Ok(true);
        }
        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
            push_exclude_filter(
                flt,
                ObjectFilter {
                    predicates: vec![FilterPredicate::OfChosenColor],
                    ..Default::default()
                },
            );
            return Ok(true);
        }
        if ts.eat_any_phrase(&[&["a", "commander"], &["commanders"], &["commander"]]) {
            flt.flags = flt.flags.union(FilterFlags::NOT_COMMANDER);
            return Ok(true);
        }
        if let Some(flag) = eat_phrase_table(
            ts,
            &[
                (&["enchanted"], FilterFlags::ENCHANTED),
                (&["equipped"], FilterFlags::EQUIPPED),
            ],
        ) {
            push_exclude_filter(
                flt,
                ObjectFilter {
                    flags: flag,
                    ..Default::default()
                },
            );
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
            push_exclude_filter(
                flt,
                ObjectFilter {
                    flags: FilterFlags::MODIFIED,
                    ..Default::default()
                },
            );
            return Ok(true);
        }
        if ts.eat_any_phrase(&[&["a", "token"], &["tokens"]]) {
            flt.token = Some(false);
            return Ok(true);
        }
        if ts.eat_word("exactly")
            && let Some(_n) = ts.next_number(false)
            && ts.eat_any_word(&["colors", "color"])
        {
            flt.color_arity = Some(ColorArity::Colored);
            return Ok(true);
        }
        return Ok(false);
    }

    // `that shares? a permanent type with <subj>` / `that shares? one of those types with <subj>`
    if ts.eat_any_phrase(&[
        &["shares", "a", "permanent", "type", "with"],
        &["share", "a", "permanent", "type", "with"],
        &["shares", "one", "of", "those", "types", "with"],
        &["share", "one", "of", "those", "types", "with"],
    ]) {
        let ref_subj = parse_subject_ref(ts)?;
        push_shares_property(flt, SharedProperty::PermanentType, ref_subj, false);
        return Ok(true);
    }

    // `that (doesn't|don't) ...`
    if ts.eat_any_word(&["doesn't", "don't"]) {
        if ts.eat_phrase(&["have", "a", "name"]) {
            push_exclude_filter(
                flt,
                ObjectFilter {
                    predicates: vec![FilterPredicate::HasNameSticker],
                    ..Default::default()
                },
            );
            return Ok(true);
        }
        if ts.eat_phrase(&["have", "the", "same", "name", "as"])
            && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
        {
            push_shares_property(flt, SharedProperty::Name, ref_subj, true);
            return Ok(true);
        }
        if ts.eat_any_phrase(&[&["share", "a"], &["share", "an"]]) {
            const SHARED_PROPS: &[(&[&str], SharedProperty)] = &[
                (&["land", "type"], SharedProperty::LandType),
                (&["creature", "type"], SharedProperty::CreatureType),
                (&["card", "type"], SharedProperty::CardType),
                (&["permanent", "type"], SharedProperty::PermanentType),
                (&["color"], SharedProperty::Color),
            ];
            if let Some(property) = eat_phrase_table(ts, SHARED_PROPS)
                && ts.eat_word("with")
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                push_shares_property(flt, property, ref_subj, true);
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

    if ts.eat_any_phrase(&[
        &["share", "no", "creature", "types"],
        &["shares", "no", "creature", "types"],
    ]) {
        push_shares_property(
            flt,
            SharedProperty::CreatureType,
            SubjectRef::SelfObject,
            true,
        );
        return Ok(true);
    }
    if ts.eat_any_phrase(&[&["share", "no", "colors"], &["shares", "no", "colors"]]) {
        push_shares_property(flt, SharedProperty::Color, SubjectRef::SelfObject, true);
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
            push_stat_bound(flt, stat, op, val);
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
    if ts.eat_any_word(&["was", "were"]) {
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
        if ts.eat_any_phrase(&[
            &[
                "discarded",
                "or",
                "put",
                "there",
                "from",
                "a",
                "library",
                "this",
                "turn",
            ],
            &["discarded", "this", "turn"],
            &["discarded", "this", "way"],
        ]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            return Ok(true);
        }
        if ts.eat_phrase(&["blocked", "by"])
            && let Ok(blocker) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "combat"]]);
            flt.combat = Some(CombatState::Blocked);
            flt.predicates
                .push(FilterPredicate::TargetsSubject(Box::new(blocker)));
            return Ok(true);
        }
        const WAS_PRIOR_ACTIONS: &[(&[&[&str]], PriorActionKind)] = &[
            (
                &[&["destroyed", "this", "way"]],
                PriorActionKind::DestroyedThisWay,
            ),
            (
                &[&["sacrificed", "this", "way"]],
                PriorActionKind::SacrificedThisWay,
            ),
            (
                &[&["revealed", "this", "way"]],
                PriorActionKind::RevealedThisWay,
            ),
            (
                &[&["cast", "this", "way"], &["cast", "this", "turn"]],
                PriorActionKind::CastThisWay,
            ),
            (
                &[&["created", "this", "way"]],
                PriorActionKind::CreatedThisWay,
            ),
            (&[&["chosen", "this", "way"]], PriorActionKind::ThatMany),
            (
                &[&["milled", "this", "turn"], &["milled", "this", "way"]],
                PriorActionKind::MilledThisWay,
            ),
            (
                &[
                    &[
                        "exiled",
                        "under",
                        "your",
                        "opponents'",
                        "control",
                        "this",
                        "turn",
                    ],
                    &["exiled", "this", "turn"],
                ],
                PriorActionKind::ExiledThisWay,
            ),
        ];
        if let Some(kind) = eat_phrase_groups(ts, WAS_PRIOR_ACTIONS) {
            flt.predicates
                .push(FilterPredicate::AffectedByPriorAction(kind));
            return Ok(true);
        }
        if ts.eat_phrase(&["activated", "this", "turn"]) {
            flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
            return Ok(true);
        }
        return Ok(false);
    }

    // `that had [a/an] [<kind>] counter(s) put on it/them this way`
    if ts.eat_word("had") {
        let _ = ts.eat_any_word(&["a", "an"]);
        let ck = opt(ts, parse_counter_kind);
        if ts.eat_any_word(&["counter", "counters"])
            && ts.eat_any_phrase(&[
                &["put", "on", "them", "this", "way"],
                &["put", "on", "it", "this", "way"],
            ])
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
    if ts.eat_any_word(&["has", "have"]) {
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
            push_stat_bound(flt, stat, op, val);
            return Ok(true);
        }
        if let Ok(ck) = try_parse(ts, parse_counter_kind)
            && ts.eat_any_word(&["counters", "counter"])
            && ts.eat_any_phrase(&[&["on", "them"], &["on", "it"]])
        {
            flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
            flt.predicates.push(FilterPredicate::HasCounterKind(ck));
            return Ok(true);
        }
        return Ok(false);
    }
    // `that didn't attack [or enter] [this turn]` / `that didn't enter this turn`
    if ts.eat_word("didn't") {
        if ts.eat_any_phrase(&[
            &["attack", "or", "enter", "this", "turn"],
            &["enter", "this", "turn"],
            &["attack"],
        ]) {
            push_exclude_filter(
                flt,
                ObjectFilter {
                    flags: FilterFlags::ATTACKED_THIS_TURN,
                    ..Default::default()
                },
            );
            return Ok(true);
        }
        return Ok(false);
    }
    // `that wasn't/weren't put there this combat`
    if ts.eat_any_word(&["wasn't", "weren't"]) && ts.eat_phrase(&["put", "there", "this", "combat"])
    {
        push_exclude_filter(
            flt,
            ObjectFilter {
                flags: FilterFlags::DIED_THIS_TURN,
                ..Default::default()
            },
        );
        return Ok(true);
    }
    // `that hasn't/haven't been chosen this way` / `that hasn't been phased out with this Attraction`
    if ts.eat_any_word(&["hasn't", "haven't"])
        && ts.eat_any_phrase(&[
            &["been", "chosen", "this", "way"],
            &["been", "phased", "out", "with", "this", "attraction"],
        ])
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
    if ts.eat_any_phrase(&[
        &["blocked", "or", "was", "blocked", "this", "turn"],
        &["blocked", "or", "were", "blocked", "this", "turn"],
    ]) {
        flt.combat = Some(CombatState::Blocking);
        return Ok(true);
    }
    if ts.eat_word("blocked")
        && (ts.eat_tilde() || ts.eat_word("it"))
        && ts.eat_any_phrase(&[&["this", "turn"], &["this", "combat"]])
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
    // `that shares at least one creature type with <subj>`
    if ts.eat_any_phrase(&[
        &["shares", "at", "least", "one", "creature", "type", "with"],
        &["share", "at", "least", "one", "creature", "type", "with"],
    ]) && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
    {
        push_shares_property(flt, SharedProperty::CreatureType, ref_subj, false);
        return Ok(true);
    }
    // `that shares a color with the mana spent on this activation cost` / `with the spell most recently cast this turn`
    if ts.eat_any_phrase(&[
        &["shares", "a", "color", "with"],
        &["share", "a", "color", "with"],
    ]) && ts.eat_any_phrase(&[
        &["the", "mana", "spent", "on", "this", "activation", "cost"],
        &["the", "spell", "most", "recently", "cast", "this", "turn"],
    ]) {
        push_shares_property(
            flt,
            SharedProperty::Color,
            SubjectRef::Anaphora(AnaphoraRef::ThatSpell),
            false,
        );
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
    if ts.eat_any_phrase(&[
        &["spell", "could", "target"],
        &["ability", "could", "target"],
        &["that", "spell", "could", "target"],
        &["the", "spell", "could", "target"],
        &["that", "ability", "could", "target"],
        &["the", "ability", "could", "target"],
    ]) {
        flt.predicates
            .push(FilterPredicate::TargetsSubject(Box::new(
                SubjectRef::Anaphora(AnaphoraRef::ThatSpell),
            )));
        return Ok(true);
    }
    // `that you've/you put one or more [<kind>] counter(s) on this turn`
    {
        let pcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["you've", "put"], &["you", "put"]])
            && (ts.eat_any_phrase(&[&["one", "or", "more"], &["a"], &["an"]])
                || ts.next_number(false).is_some())
        {
            let ck = opt(ts, parse_counter_kind);
            if ts.eat_any_word(&["counters", "counter"]) && ts.eat_phrase(&["on", "this", "turn"]) {
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
        const ENTERED_UNDER: &[(&[&[&str]], ControllerRelation)] = &[
            (
                &[&["under", "your", "control", "this", "turn"]],
                ControllerRelation::You,
            ),
            (
                &[
                    &["under", "an", "opponent's", "control", "this", "turn"],
                    &["under", "your", "opponents'", "control", "this", "turn"],
                ],
                ControllerRelation::Opponent,
            ),
            (
                &[
                    &["under", "their", "control", "this", "turn"],
                    &["under", "that", "player's", "control", "this", "turn"],
                ],
                ControllerRelation::ThatPlayer,
            ),
        ];
        if let Some(ctrl) = eat_phrase_groups(ts, ENTERED_UNDER) {
            flt.controller = Some(ctrl);
            flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
            return Ok(true);
        }
    }

    Ok(false)
}

fn try_parse_whose_relative(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<bool> {
    if ts.eat_any_phrase(&[
        &["coin", "comes", "up", "tails"],
        &["coin", "comes", "up", "heads"],
    ]) {
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
        push_stat_bound(
            flt,
            StatKind::Power,
            CompareOp::Lt,
            ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                stat: StatKind::Toughness,
            },
        );
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
