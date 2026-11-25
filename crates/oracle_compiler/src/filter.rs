//! Strict zero-regex parsers for `ObjectFilterAst`, `SubjectRefAst`, `ValueExprAst`,
//! `ConditionAst`, `ManaSpecAst`, and `TokenSpecAst`.

use crate::ast::{
    AggregateOp, CardTypeMask, ColorArity, ColorMask, CombatState, CombatSwapPerspective,
    CompareOp, ConditionAst, ControllerRelation, DraftLedgerQueryKind, FilterPredicateAst,
    GameMarker, ManaSpecAst, NameFilterAst, ObjectFilterAst, ObjectFilterFlags,
    ObjectPartitionKind, PartitionMembershipAst, PileRefAst, PlayerRefAst, PlayerStatKind,
    PriorActionRefKind, Rounding, StatBoundAst, StatKind, SubjectRefAst, SupertypeMask,
    TargetCountAst, TokenSpecAst, ValueExprAst, ZoneFilter,
};
use crate::preprocess::{
    is_mana_sequence, is_physical_tabletop_clause, parse_base_pt, split_top_level,
};
use crate::vocab::{
    Vocabulary, lookup_card_type, lookup_color, lookup_supertype, parse_number_word,
};

fn trim_punct(s: &str) -> &str {
    s.trim().trim_end_matches(['.', ',', ';'])
}

/// Shared longest-first zone suffix table used by both `parse_object_filter` and `parse_subject_or_target`.
pub(crate) const ZONE_SUFFIXES: &[(&str, ZoneFilter)] = &[
    // Multi-zone suffixes MUST precede shorter single-zone suffixes.
    (
        " from your graveyard, hand, and/or library",
        ZoneFilter::YourGraveyardHandOrLibrary,
    ),
    (
        " in your graveyard, hand, and/or library",
        ZoneFilter::YourGraveyardHandOrLibrary,
    ),
    (
        " from your hand, graveyard, and/or library",
        ZoneFilter::YourGraveyardHandOrLibrary,
    ),
    (
        " from your library and/or graveyard",
        ZoneFilter::YourLibraryOrGraveyard,
    ),
    (
        " from your graveyard and/or library",
        ZoneFilter::YourLibraryOrGraveyard,
    ),
    (
        " from your library or graveyard",
        ZoneFilter::YourLibraryOrGraveyard,
    ),
    (
        " from your graveyard or library",
        ZoneFilter::YourLibraryOrGraveyard,
    ),
    (
        " in your library or graveyard",
        ZoneFilter::YourLibraryOrGraveyard,
    ),
    (
        " in your graveyard or library",
        ZoneFilter::YourLibraryOrGraveyard,
    ),
    (
        " you own in exile and in your graveyard",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " from your graveyard or from exile",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " from exile or from your graveyard",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in your graveyard or in exile",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in exile or in your graveyard",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in your graveyard and in exile",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in exile and in your graveyard",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " from your graveyard or exile",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " from exile or your graveyard",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in your graveyard or exile",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in exile or your graveyard",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " in your graveyard and exile",
        ZoneFilter::YourGraveyardOrExile,
    ),
    (
        " from any graveyard or from exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from exile or from any graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from a graveyard or from exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from exile or from a graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " in any graveyard or in exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " in exile or in any graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " in a graveyard or in exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " in exile or in a graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from any graveyard or exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from exile or any graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from a graveyard or exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " from exile or a graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " in any graveyard or exile",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (
        " in exile or any graveyard",
        ZoneFilter::AnyGraveyardOrExile,
    ),
    (" in a graveyard or exile", ZoneFilter::AnyGraveyardOrExile),
    (" in exile or a graveyard", ZoneFilter::AnyGraveyardOrExile),
    (
        " from your hand or from your graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your graveyard or from your hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your hand or in your graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your graveyard or in your hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your hand or your graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your graveyard or your hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your hand or your graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your graveyard or your hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your hand and/or graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your graveyard and/or hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your hand and/or graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your graveyard and/or hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your hand or graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from your graveyard or hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your hand or graveyard",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " in your graveyard or hand",
        ZoneFilter::YourHandOrGraveyard,
    ),
    (
        " from target opponent's graveyard",
        ZoneFilter::TargetOpponentGraveyard,
    ),
    (
        " in target opponent's graveyard",
        ZoneFilter::TargetOpponentGraveyard,
    ),
    (
        " from your opponents' graveyards",
        ZoneFilter::OpponentsGraveyards,
    ),
    (
        " in your opponents' graveyards",
        ZoneFilter::OpponentsGraveyards,
    ),
    (
        " from each opponent's graveyard",
        ZoneFilter::OpponentsGraveyards,
    ),
    (
        " in each opponent's graveyard",
        ZoneFilter::OpponentsGraveyards,
    ),
    (
        " from opponents' graveyards",
        ZoneFilter::OpponentsGraveyards,
    ),
    (" in opponents' graveyards", ZoneFilter::OpponentsGraveyards),
    (
        " from an opponent's graveyard",
        ZoneFilter::OpponentGraveyard,
    ),
    (" in an opponent's graveyard", ZoneFilter::OpponentGraveyard),
    (
        " from that opponent's graveyard",
        ZoneFilter::OpponentGraveyard,
    ),
    (
        " in that opponent's graveyard",
        ZoneFilter::OpponentGraveyard,
    ),
    (
        " from that player's graveyard",
        ZoneFilter::ThatPlayerGraveyard,
    ),
    (
        " in that player's graveyard",
        ZoneFilter::ThatPlayerGraveyard,
    ),
    (
        " from target player's graveyard",
        ZoneFilter::TargetPlayerGraveyard,
    ),
    (
        " in target player's graveyard",
        ZoneFilter::TargetPlayerGraveyard,
    ),
    (
        " from defending player's graveyard",
        ZoneFilter::DefendingPlayerGraveyard,
    ),
    (
        " in defending player's graveyard",
        ZoneFilter::DefendingPlayerGraveyard,
    ),
    (
        " from the defending player's graveyard",
        ZoneFilter::DefendingPlayerGraveyard,
    ),
    (
        " in the defending player's graveyard",
        ZoneFilter::DefendingPlayerGraveyard,
    ),
    (" from a single graveyard", ZoneFilter::SingleGraveyard),
    (" in a single graveyard", ZoneFilter::SingleGraveyard),
    (
        " in the chosen player's graveyard",
        ZoneFilter::ThatPlayerGraveyard,
    ),
    (
        " from the chosen player's graveyard",
        ZoneFilter::ThatPlayerGraveyard,
    ),
    (" from all players' graveyards", ZoneFilter::AllGraveyards),
    (" in all players' graveyards", ZoneFilter::AllGraveyards),
    (" from each player's graveyard", ZoneFilter::AllGraveyards),
    (" in each player's graveyard", ZoneFilter::AllGraveyards),
    (" you cast from your graveyard", ZoneFilter::YourGraveyard),
    (" you cast from exile", ZoneFilter::Exile),
    (" cast from your graveyard", ZoneFilter::YourGraveyard),
    (" cast from any graveyard", ZoneFilter::AnyGraveyard),
    (" cast from a graveyard", ZoneFilter::AnyGraveyard),
    (" cast from exile", ZoneFilter::Exile),
    (
        " that were put into your graveyard from your library this turn",
        ZoneFilter::YourGraveyard,
    ),
    (" from your graveyard", ZoneFilter::YourGraveyard),
    (" in your graveyard", ZoneFilter::YourGraveyard),
    (" from their graveyard", ZoneFilter::ThatPlayerGraveyard),
    (" in their graveyard", ZoneFilter::ThatPlayerGraveyard),
    (" from any player's graveyard", ZoneFilter::AnyGraveyard),
    (" in any player's graveyard", ZoneFilter::AnyGraveyard),
    (" from a player's graveyard", ZoneFilter::AnyGraveyard),
    (" in a player's graveyard", ZoneFilter::AnyGraveyard),
    (" in its owner's graveyard", ZoneFilter::ItsOwnerGraveyard),
    (" from its owner's graveyard", ZoneFilter::ItsOwnerGraveyard),
    (" from any graveyard", ZoneFilter::AnyGraveyard),
    (" in any graveyard", ZoneFilter::AnyGraveyard),
    (" from a graveyard", ZoneFilter::AnyGraveyard),
    (" in a graveyard", ZoneFilter::AnyGraveyard),
    (" from all graveyards", ZoneFilter::AllGraveyards),
    (" in all graveyards", ZoneFilter::AllGraveyards),
    (" in graveyards", ZoneFilter::AllGraveyards),
    (" from graveyards", ZoneFilter::AllGraveyards),
    (" from among the milled cards", ZoneFilter::YourGraveyard),
    (
        " from among the cards milled this way",
        ZoneFilter::YourGraveyard,
    ),
    (" from among cards exiled this way", ZoneFilter::Exile),
    (" from among cards exiled with ~", ZoneFilter::Exile),
    (" from among the exiled cards", ZoneFilter::Exile),
    (
        " from among the nonland cards exiled this way",
        ZoneFilter::Exile,
    ),
    (" from among those nonland cards", ZoneFilter::Exile),
    (
        " from among cards revealed this way",
        ZoneFilter::YourLibrary,
    ),
    (" from among those cards", ZoneFilter::YourLibrary),
    (" from among them", ZoneFilter::YourLibrary),
    (" from it", ZoneFilter::YourHand),
    (" in it", ZoneFilter::YourHand),
    (" from outside the game", ZoneFilter::OutsideTheGame),
    (" outside the game", ZoneFilter::OutsideTheGame),
    (" your opponents own in exile", ZoneFilter::Exile),
    (" an opponent owns in exile", ZoneFilter::Exile),
    (" you own in exile", ZoneFilter::Exile),
    (
        " from target opponent's hand",
        ZoneFilter::TargetOpponentHand,
    ),
    (" in target opponent's hand", ZoneFilter::TargetOpponentHand),
    (" from target player's hand", ZoneFilter::TargetPlayerHand),
    (" in target player's hand", ZoneFilter::TargetPlayerHand),
    (" from an opponent's hand", ZoneFilter::OpponentHand),
    (" in an opponent's hand", ZoneFilter::OpponentHand),
    (" from that opponent's hand", ZoneFilter::OpponentHand),
    (" in that opponent's hand", ZoneFilter::OpponentHand),
    (" from your opponents' hands", ZoneFilter::OpponentsHands),
    (" in your opponents' hands", ZoneFilter::OpponentsHands),
    (" from opponents' hands", ZoneFilter::OpponentsHands),
    (" in opponents' hands", ZoneFilter::OpponentsHands),
    (" from each opponent's hand", ZoneFilter::OpponentsHands),
    (" in each opponent's hand", ZoneFilter::OpponentsHands),
    (" from each player's hand", ZoneFilter::EachPlayerHand),
    (" in each player's hand", ZoneFilter::EachPlayerHand),
    (" from all players' hands", ZoneFilter::EachPlayerHand),
    (" in all players' hands", ZoneFilter::EachPlayerHand),
    (" from the chosen player's hand", ZoneFilter::ThatPlayerHand),
    (" in the chosen player's hand", ZoneFilter::ThatPlayerHand),
    (" from that player's hand", ZoneFilter::ThatPlayerHand),
    (" in that player's hand", ZoneFilter::ThatPlayerHand),
    (" from your hand", ZoneFilter::YourHand),
    (" in your hand", ZoneFilter::YourHand),
    (" from their hand", ZoneFilter::ThatPlayerHand),
    (" in their hand", ZoneFilter::ThatPlayerHand),
    (" from a player's hand", ZoneFilter::AnyHand),
    (" in a player's hand", ZoneFilter::AnyHand),
    (" from any player's hand", ZoneFilter::AnyHand),
    (" in any player's hand", ZoneFilter::AnyHand),
    (" from a hand", ZoneFilter::AnyHand),
    (" in a hand", ZoneFilter::AnyHand),
    (" from any hand", ZoneFilter::AnyHand),
    (" in any hand", ZoneFilter::AnyHand),
    (
        " from target opponent's library",
        ZoneFilter::TargetOpponentLibrary,
    ),
    (
        " in target opponent's library",
        ZoneFilter::TargetOpponentLibrary,
    ),
    (
        " from target player's library",
        ZoneFilter::TargetPlayerLibrary,
    ),
    (
        " in target player's library",
        ZoneFilter::TargetPlayerLibrary,
    ),
    (" from an opponent's library", ZoneFilter::OpponentLibrary),
    (" in an opponent's library", ZoneFilter::OpponentLibrary),
    (" from that opponent's library", ZoneFilter::OpponentLibrary),
    (" in that opponent's library", ZoneFilter::OpponentLibrary),
    (
        " from the chosen player's library",
        ZoneFilter::ThatPlayerLibrary,
    ),
    (
        " in the chosen player's library",
        ZoneFilter::ThatPlayerLibrary,
    ),
    (" from that player's library", ZoneFilter::ThatPlayerLibrary),
    (" in that player's library", ZoneFilter::ThatPlayerLibrary),
    (" from their library", ZoneFilter::ThatPlayerLibrary),
    (" in their library", ZoneFilter::ThatPlayerLibrary),
    (" from each player's library", ZoneFilter::EachPlayerLibrary),
    (" in each player's library", ZoneFilter::EachPlayerLibrary),
    (" from your library", ZoneFilter::YourLibrary),
    (" in your library", ZoneFilter::YourLibrary),
    (" from a library", ZoneFilter::AnyLibrary),
    (" in a library", ZoneFilter::AnyLibrary),
    (" from any library", ZoneFilter::AnyLibrary),
    (" in any library", ZoneFilter::AnyLibrary),
    (" from exile", ZoneFilter::Exile),
    (" in exile", ZoneFilter::Exile),
    (" from the command zone", ZoneFilter::CommandZone),
    (" in the command zone", ZoneFilter::CommandZone),
    (
        " with the greatest power among creatures on the battlefield",
        ZoneFilter::Battlefield,
    ),
    (" from the battlefield", ZoneFilter::Battlefield),
    (" on the battlefield", ZoneFilter::Battlefield),
    (" from the stack", ZoneFilter::Stack),
    (" on the stack", ZoneFilter::Stack),
];

/// Strictly parse a noun phrase into a 13-dimension `ObjectFilterAst`.
pub fn parse_object_filter(phrase: &str, vocab: &Vocabulary) -> Result<ObjectFilterAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_filter".to_string());
    }

    let mut flt = ObjectFilterAst::default();
    let mut s = raw
        .to_ascii_lowercase()
        .replace("time lords", "timelords")
        .replace("time lord", "timelord");

    // Fast path for "<kind> stickers on <subj>" (Pin Collection Layer 6 ability inheritance).
    if let Some((kind_raw, host_raw)) = s
        .split_once(" stickers on ")
        .or_else(|| s.split_once(" sticker on "))
        && matches!(
            kind_raw.trim(),
            "ability" | "art" | "name" | "power and toughness" | "a" | "an"
        )
        && let Ok(host) = parse_subject_or_target(host_raw.trim(), vocab)
    {
        let norm_kind = match kind_raw.trim() {
            "power and toughness" => "power_and_toughness",
            "a" | "an" => "any",
            other => other,
        };
        flt.predicates
            .push(FilterPredicateAst::HasStickerKind(norm_kind.to_string()));
        flt.predicates
            .push(FilterPredicateAst::AttachedTo(Box::new(host)));
        return Ok(flt);
    }

    // Repeatedly peel trailing zone, controller/owner, relational, stat-bound, counter, and keyword suffixes.
    // Longest match MUST precede shorter suffix (e.g. "your opponents control" before "opponents control").
    loop {
        let mut changed = false;

        if let Some(prefix) = s.strip_suffix(" with abilities") {
            flt.predicates.push(FilterPredicateAst::HasAbilities {
                has_abilities: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with no abilities") {
            flt.predicates.push(FilterPredicateAst::HasAbilities {
                has_abilities: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" the monarch controls") {
            flt.predicates.push(FilterPredicateAst::ControlledByMonarch);
            s = prefix.trim().to_string();
            continue;
        }
        // Pass E10 typed partition, phase-out, draft-ledger, first-printed-set, sticker, and mana-subset predicates
        // (peeled BEFORE generic "of your choice", "named ~", and zone suffixes so nothing shadows them).
        if let Some(prefix) = s.strip_suffix(" in the sector of your choice") {
            flt.predicates.push(FilterPredicateAst::InObjectPartition {
                kind: ObjectPartitionKind::Sector,
                membership: PartitionMembershipAst::ChosenByController,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" in a pile with the chosen label") {
            flt.predicates.push(FilterPredicateAst::InObjectPartition {
                kind: ObjectPartitionKind::LabeledPiles(Vec::new()),
                membership: PartitionMembershipAst::MatchingSubjectTag(Box::new(
                    SubjectRefAst::Anaphora("that creature".to_string()),
                )),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that hasn't been phased out with this attraction")
            .or_else(|| s.strip_suffix(" that hasn't been phased out with ~"))
        {
            flt.predicates
                .push(FilterPredicateAst::PhasedOutBySource { negated: true });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" of a type you noted for cards named ~")
            .or_else(|| s.strip_suffix(" of a type noted as you drafted cards named ~"))
        {
            flt.predicates.push(FilterPredicateAst::MatchesDraftLedger {
                kind: DraftLedgerQueryKind::NotedCreatureType,
                source_card_name: Box::new(SubjectRefAst::SelfObject),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" with a name you noted for cards named ~")
            .or_else(|| s.strip_suffix(" with a name noted as you drafted cards named ~"))
        {
            flt.predicates.push(FilterPredicateAst::MatchesDraftLedger {
                kind: DraftLedgerQueryKind::NotedName,
                source_card_name: Box::new(SubjectRefAst::SelfObject),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that's one or more of the colors chosen as you drafted cards named ~")
            .or_else(|| {
                s.strip_suffix(
                    " that are one or more of the colors chosen as you drafted cards named ~",
                )
            })
            .or_else(|| s.strip_suffix(" of a color chosen as you drafted cards named ~"))
        {
            flt.predicates.push(FilterPredicateAst::MatchesDraftLedger {
                kind: DraftLedgerQueryKind::NotedColor,
                source_card_name: Box::new(SubjectRefAst::SelfObject),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, exp_tail)) = s.rsplit_once(" with a name originally printed in the ")
            && let Some(exp_low) = exp_tail.strip_suffix(" expansion")
            && !prefix.trim().is_empty()
            && !exp_low.trim().is_empty()
        {
            let exp_name = exp_low
                .split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        Some(first) => {
                            let mut word = first.to_uppercase().to_string();
                            word.push_str(chars.as_str());
                            word
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            flt.predicates
                .push(FilterPredicateAst::FirstPrintedInExpansion(exp_name));
            s = prefix.trim().to_string();
            continue;
        }
        for (suf, kind) in [
            (
                " with a power and toughness sticker on it",
                "power_and_toughness",
            ),
            (" with a power and toughness sticker", "power_and_toughness"),
            (" with an ability sticker on it", "ability"),
            (" with an ability sticker", "ability"),
            (" with a name sticker on it", "name"),
            (" with a name sticker", "name"),
            (" with an art sticker on it", "art"),
            (" with an art sticker", "art"),
        ] {
            if let Some(prefix) = s.strip_suffix(suf)
                && !prefix.trim().is_empty()
            {
                flt.predicates
                    .push(FilterPredicateAst::HasStickerKind(kind.to_string()));
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(
                " whose mana cost could be paid by some amount of, or all of, the mana you spent on {x}",
            )
            .or_else(|| s.strip_suffix(" whose mana cost could be paid by some or all of the mana you spent on {x}"))
        {
            flt.predicates
                .push(FilterPredicateAst::ManaCostPayableBySpentMana {
                    x_mana_only: true,
                    subset_allowed: true,
                });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that can block additional creatures") {
            flt.predicates
                .push(FilterPredicateAst::CanBlockAdditionalCreatures);
            s = prefix.trim().to_string();
            continue;
        }

        if let Some(prefix) = s.strip_suffix(" of the chosen card type") {
            flt.flags = flt.flags.union(ObjectFilterFlags::MATCHES_CHOSEN_CARD_TYPE);
            s = prefix.trim().to_string();
            continue;
        }

        // Distinctness qualifiers ("with different names", "with different mana values", etc.)
        for (suf, flag) in [
            (
                " that each have different names",
                ObjectFilterFlags::DIFFERENT_NAMES,
            ),
            (" with different names", ObjectFilterFlags::DIFFERENT_NAMES),
            (
                " that each have different mana values",
                ObjectFilterFlags::DIFFERENT_MANA_VALUES,
            ),
            (
                " with different mana values",
                ObjectFilterFlags::DIFFERENT_MANA_VALUES,
            ),
            (
                " with different colors",
                ObjectFilterFlags::DIFFERENT_COLORS,
            ),
            (
                " with different card types",
                ObjectFilterFlags::DIFFERENT_CARD_TYPES,
            ),
        ] {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.flags = flt.flags.union(flag);
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        // Generalized "with the same name as <subj>"
        if let Some((prefix, same_as_raw)) = s.rsplit_once(" with the same name as ")
            && !prefix.trim().is_empty()
            && !same_as_raw.contains(" another card in their hand")
        {
            let target_raw = same_as_raw.trim();
            if target_raw == "~" {
                flt.named = Some(NameFilterAst::SelfName);
                s = prefix.trim().to_string();
                continue;
            } else if matches!(
                target_raw,
                "that card" | "the exiled card" | "the chosen card"
            ) {
                flt.named = Some(NameFilterAst::SameAsReferenced);
                s = prefix.trim().to_string();
                continue;
            } else if let Ok(ref_subj) = parse_subject_or_target(target_raw, vocab) {
                flt.named = Some(NameFilterAst::SameAs(Box::new(ref_subj)));
                s = prefix.trim().to_string();
                continue;
            }
        }
        if let Some(prefix) = s.strip_suffix(" with the same name as another card in their hand") {
            flt.named = Some(NameFilterAst::SameAsReferenced);
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "same_name_as_another_card_in_their_hand".to_string(),
            ));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that aren't on the battlefield")
            .or_else(|| s.strip_suffix(" that isn't on the battlefield"))
        {
            s = prefix.trim().to_string();
            continue;
        }
        // Pass E12: Lossless zone suffixes that carry owner, origin, prior-action, or nonland constraints
        if let Some(prefix) =
            s.strip_suffix(" that were put into your graveyard from your library this turn")
        {
            flt.zone = Some(ZoneFilter::YourGraveyard);
            flt.predicates
                .push(FilterPredicateAst::EnteredZoneFromThisTurn {
                    from_zone: ZoneFilter::YourLibrary,
                });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" you own in exile and in your graveyard") {
            flt.zone = Some(ZoneFilter::YourGraveyardOrExile);
            flt.owner = Some(ControllerRelation::You);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" your opponents own in exile") {
            flt.zone = Some(ZoneFilter::Exile);
            flt.owner = Some(ControllerRelation::Opponents);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" an opponent owns in exile") {
            flt.zone = Some(ZoneFilter::Exile);
            flt.owner = Some(ControllerRelation::Opponent);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" you own in exile") {
            flt.zone = Some(ZoneFilter::Exile);
            flt.owner = Some(ControllerRelation::You);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" from among the nonland cards exiled this way") {
            flt.zone = Some(ZoneFilter::Exile);
            flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND);
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::ExiledThisWay,
                ));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" from among cards exiled this way")
            .or_else(|| s.strip_suffix(" from among the exiled cards"))
        {
            flt.zone = Some(ZoneFilter::Exile);
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::ExiledThisWay,
                ));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" from among cards exiled with ~") {
            flt.zone = Some(ZoneFilter::Exile);
            flt.flags = flt.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" from among those nonland cards") {
            flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND);
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "from_among_those_cards".to_string(),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" from among the cards milled this way")
            .or_else(|| s.strip_suffix(" from among the milled cards"))
        {
            flt.zone = Some(ZoneFilter::YourGraveyard);
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "milled_this_way".to_string(),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" from among cards revealed this way") {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::RevealedThisWay,
                ));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" from among those cards")
            .or_else(|| s.strip_suffix(" from among them"))
        {
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "from_among_those".to_string(),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" from it")
            .or_else(|| s.strip_suffix(" in it"))
        {
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "in_referenced_zone".to_string(),
            });
            s = prefix.trim().to_string();
            continue;
        }
        for (suf, z) in ZONE_SUFFIXES {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.zone = Some(*z);
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        // Try trailing stat bound BEFORE CTRL_SUFFIXES so "creature you control with toughness less than
        // the number of basic land types among lands you control" doesn't strip "you control" off the RHS!
        if s.contains(" with ")
            && let Some((prefix, bounds)) = peel_stat_range_list(&s)
        {
            flt.stat_bounds.extend(bounds);
            s = prefix.trim().to_string();
            continue;
        }
        if s.contains(" with ")
            && let Some((prefix, bound)) = peel_stat_bound(&s, vocab)
        {
            flt.stat_bounds.push(bound);
            s = prefix.trim().to_string();
            continue;
        }

        // Wave E6.2: Multi-zone, disjunctive, combat-target, blocking, attached-to, prior-action,
        // superlative, exclusion, and relational suffixes peeled BEFORE CTRL_SUFFIXES so inner
        // controller phrases ("an aura you control", "permanents you control") are not stripped early.
        if let Some(prefix) = s.strip_suffix(" from the command zone and from your graveyard") {
            flt.zone = Some(ZoneFilter::CommandZone);
            flt.extra_zones.push(ZoneFilter::YourGraveyard);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" from your hand or from the command zone") {
            flt.zone = Some(ZoneFilter::YourHand);
            flt.extra_zones.push(ZoneFilter::CommandZone);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" in exile and in your graveyard") {
            flt.zone = Some(ZoneFilter::Exile);
            flt.extra_zones.push(ZoneFilter::YourGraveyard);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" from its owner's graveyard") {
            flt.zone = Some(ZoneFilter::ItsOwnerGraveyard);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that's an artifact or that has mana value 3 or less")
        {
            let f_art = ObjectFilterAst {
                any_types: CardTypeMask::ARTIFACT,
                ..Default::default()
            };
            let f_mv3 = ObjectFilterAst {
                stat_bounds: vec![StatBoundAst {
                    stat: StatKind::ManaValue,
                    op: CompareOp::Le,
                    value: Box::new(ValueExprAst::Const(3)),
                }],
                ..Default::default()
            };
            flt.predicates
                .push(FilterPredicateAst::AnyOf(vec![f_art, f_mv3]));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(
            " that's an instant card, a sorcery card, or a card that has an adventure",
        ) {
            let f_inst = ObjectFilterAst {
                any_types: CardTypeMask::INSTANT,
                any_card: true,
                ..Default::default()
            };
            let f_sorc = ObjectFilterAst {
                any_types: CardTypeMask::SORCERY,
                any_card: true,
                ..Default::default()
            };
            let f_adv = ObjectFilterAst {
                any_card: true,
                subtypes: vec!["Adventure".to_string()],
                ..Default::default()
            };
            flt.predicates
                .push(FilterPredicateAst::AnyOf(vec![f_inst, f_sorc, f_adv]));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that's attacking you or a planeswalker you control")
        {
            let your_pw = ObjectFilterAst {
                any_types: CardTypeMask::PLANESWALKER,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::AttackingTarget {
                target: Box::new(SubjectRefAst::Compound(vec![
                    SubjectRefAst::Player(PlayerRefAst::You),
                    SubjectRefAst::Group(Box::new(your_pw)),
                ])),
                this_turn: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that's attacking you") {
            flt.predicates.push(FilterPredicateAst::AttackingTarget {
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                this_turn: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" from that player's hand and graveyard")
            .or_else(|| s.strip_suffix(" from their hand and graveyard"))
        {
            flt.zone = Some(ZoneFilter::ThatPlayerHand);
            flt.extra_zones.push(ZoneFilter::ThatPlayerGraveyard);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with an activated ability that isn't a mana ability")
        {
            flt.predicates.push(FilterPredicateAst::HasAbilityKind {
                kind: "activated_nonmana".to_string(),
                negated: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that's attacking one of your opponents") {
            flt.predicates.push(FilterPredicateAst::AttackingTarget {
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::EachOpponent)),
                this_turn: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" attacking your opponents and/or planeswalkers they control")
        {
            let opp_pw = ObjectFilterAst {
                any_types: CardTypeMask::PLANESWALKER,
                controller: Some(ControllerRelation::Opponents),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::AttackingTarget {
                target: Box::new(SubjectRefAst::Compound(vec![
                    SubjectRefAst::Player(PlayerRefAst::EachOpponent),
                    SubjectRefAst::All(Box::new(opp_pw)),
                ])),
                this_turn: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" attacking a player") {
            flt.predicates.push(FilterPredicateAst::AttackingTarget {
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::AnyPlayer)),
                this_turn: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that attacked you this turn") {
            flt.predicates.push(FilterPredicateAst::AttackingTarget {
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                this_turn: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that's blocking it")
            .or_else(|| s.strip_suffix(" blocking that creature"))
        {
            flt.predicates.push(FilterPredicateAst::Blocking(Box::new(
                SubjectRefAst::Anaphora("that creature".to_string()),
            )));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that's enchanted by an aura you control") {
            let aura = ObjectFilterAst {
                subtypes: vec!["Aura".to_string()],
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            flt.predicates
                .push(FilterPredicateAst::AttachedBy(Box::new(aura)));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" attached to permanents you control") {
            let host = ObjectFilterAst {
                any_permanent: true,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::Group(Box::new(host)),
            )));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" attached to attacking creatures your opponents control")
        {
            let host = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                combat: Some(CombatState::Attacking),
                controller: Some(ControllerRelation::Opponents),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::Group(Box::new(host)),
            )));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that were dealt damage this turn")
            .or_else(|| s.strip_suffix(" that has been dealt damage this turn"))
        {
            flt.flags = flt.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that were blocked by that creature this turn") {
            flt.predicates.push(FilterPredicateAst::BlockedBy {
                blocker: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                this_turn: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(
                " that the player hasn't controlled continuously since the beginning of the turn",
            )
            .or_else(|| {
                s.strip_suffix(
                    " the player hasn't controlled continuously since the beginning of the turn",
                )
            })
            .or_else(|| {
                s.strip_suffix(
                    " the player didn't control continuously since the beginning of the turn",
                )
            })
            .or_else(|| {
                s.strip_suffix(
                    " that they haven't controlled continuously since the beginning of the turn",
                )
            })
            .or_else(|| {
                s.strip_suffix(
                    " that you haven't controlled continuously since the beginning of the turn",
                )
            })
        {
            flt.predicates
                .push(FilterPredicateAst::ControlledSinceTurnStart { negated: true });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that they controlled continuously since the beginning of the turn")
            .or_else(|| s.strip_suffix(" that they controlled since the beginning of the turn"))
            .or_else(|| {
                s.strip_suffix(" that you controlled continuously since the beginning of the turn")
            })
            .or_else(|| s.strip_suffix(" that you controlled since the beginning of the turn"))
        {
            flt.predicates
                .push(FilterPredicateAst::ControlledSinceTurnStart { negated: false });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, after_no_cnt)) = s
            .split_once(" that doesn't have a ")
            .or_else(|| s.split_once(" that don't have a "))
            .or_else(|| s.split_once(" that doesn't have an "))
            .or_else(|| s.split_once(" that don't have an "))
            && let Some(kind_raw) = after_no_cnt
                .strip_suffix(" counter on it")
                .or_else(|| after_no_cnt.strip_suffix(" counter on them"))
        {
            flt.predicates.push(FilterPredicateAst::WithoutCounterKind(
                kind_raw.trim().to_string(),
            ));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that didn't attack or enter this turn") {
            flt.flags = flt.flags.union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN);
            flt.predicates.push(FilterPredicateAst::DidNotEnterThisTurn);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that didn't attack") {
            flt.flags = flt.flags.union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the same controller") {
            flt.predicates
                .push(FilterPredicateAst::WithSameControllerAs(Box::new(
                    SubjectRefAst::Anaphora("target permanent".to_string()),
                )));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, host_raw)) = s.rsplit_once(" enchanting ")
            && !prefix.trim().is_empty()
            && let Ok(host_subj) = parse_subject_or_target(host_raw.trim(), vocab)
        {
            flt.predicates
                .push(FilterPredicateAst::AttachedTo(Box::new(host_subj)));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" attached to a creature or land") {
            let host = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE.union(CardTypeMask::LAND),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::Group(Box::new(host)),
            )));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" you exiled with cards named ~")
            .or_else(|| s.strip_suffix(" that you exiled with cards named ~"))
        {
            flt.zone = Some(ZoneFilter::Exile);
            flt.flags = flt.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with equal or lesser toughness") {
            flt.predicates.push(FilterPredicateAst::StatRelationTo {
                stat: StatKind::Toughness,
                cmp: CompareOp::Le,
                expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::Toughness,
                }),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that one of your opponents controls")
            .or_else(|| s.strip_suffix(" one of their opponents controls"))
            .or_else(|| s.strip_suffix(" one of your opponents controls"))
        {
            flt.controller = Some(ControllerRelation::Opponent);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" another player controls") {
            flt.controller = Some(ControllerRelation::NotYou);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that dealt damage to it this turn") {
            flt.flags = flt.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "dealt_damage_to_it_this_turn".to_string(),
            ));
            s = prefix.trim().to_string();
            continue;
        }
        for (ch_sep, ch_player) in [
            (" of that player's choice ", PlayerRefAst::ThatPlayer),
            (" of their choice ", PlayerRefAst::ThatPlayer),
            (" of your choice ", PlayerRefAst::You),
        ] {
            if let Some((before_ch, after_ch)) = s.split_once(ch_sep)
                && !before_ch.trim().is_empty()
                && !after_ch.trim().is_empty()
            {
                flt.predicates
                    .push(FilterPredicateAst::ChosenByPlayer(ch_player));
                s = format!("{} {}", before_ch.trim(), after_ch.trim());
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        if let Some((prefix, ab_src_raw)) =
            s.split_once(" that isn't the target of an ability from ")
            && let Ok(src_flt) = parse_object_filter(ab_src_raw.trim(), vocab)
        {
            let mut ab_flt = ObjectFilterAst {
                is_ability: true,
                ..Default::default()
            };
            ab_flt
                .predicates
                .push(FilterPredicateAst::FromSource(Box::new(src_flt)));
            flt.predicates
                .push(FilterPredicateAst::NotTargetedBy(Box::new(ab_flt)));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, other_raw)) = s
            .split_once(" that blocked or was blocked by ")
            .or_else(|| s.split_once(" that blocked or were blocked by "))
            && let Some(clean_other) = other_raw.strip_suffix(" this turn")
            && let Ok(other_flt) = parse_object_filter(clean_other.trim(), vocab)
        {
            flt.predicates.push(FilterPredicateAst::BlockedOrBlockedBy {
                other: Box::new(other_flt),
                this_turn: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" whose controller was dealt combat damage by ~ this turn")
        {
            flt.predicates
                .push(FilterPredicateAst::ControllerDealtCombatDamageBySelfThisTurn);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that was put there from anywhere this turn") {
            flt.predicates.push(FilterPredicateAst::PutIntoZoneThisTurn);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that shares a card type with it") {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "card_type".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                negated: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with an art sticker on it") {
            flt.predicates
                .push(FilterPredicateAst::HasStickerKind("art".to_string()));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with different controllers") {
            flt.predicates
                .push(FilterPredicateAst::DifferentControllers);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that was activated this turn") {
            flt.predicates.push(FilterPredicateAst::ActivatedThisTurn);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" you own that your opponents control") {
            flt.owner = Some(ControllerRelation::You);
            flt.controller = Some(ControllerRelation::Opponents);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" the opponent to your left controls") {
            flt.controller = Some(ControllerRelation::PlayerToLeft);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" controlled by the player to their right") {
            flt.controller = Some(ControllerRelation::PlayerToRight);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, kw_part)) = s
            .split_once(" that doesn't have ")
            .or_else(|| s.split_once(" that don't have "))
            && !kw_part.contains(" counter")
            && let Some(kws) = parse_keyword_list(kw_part.trim(), vocab)
        {
            flt.forbidden_keywords.extend(kws);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that attacked or blocked this turn") {
            flt.flags = flt
                .flags
                .union(ObjectFilterFlags::ATTACKED_OR_BLOCKED_THIS_TURN);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that aren't of a type chosen this way") {
            flt.predicates
                .push(FilterPredicateAst::MatchesChosen { negated: true });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" that shares a card type with the sacrificed permanent")
        {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "card_type".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora(
                    "the sacrificed permanent".to_string(),
                )),
                negated: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" that doesn't share a creature type with a creature you control")
        {
            let your_cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Group(Box::new(your_cr))),
                negated: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that don't share a creature type with the chosen creature they control")
        {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora("the chosen creature".to_string())),
                negated: true,
            });
            flt.predicates.push(FilterPredicateAst::Exclude(Box::new(
                SubjectRefAst::Anaphora("the chosen creature".to_string()),
            )));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" they control but don't own") {
            flt.controller = Some(ControllerRelation::ThatPlayer);
            flt.predicates
                .push(FilterPredicateAst::NotOwnedByController);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" from another legendary source that's not a commander")
        {
            let src_flt = ObjectFilterAst {
                any_permanent: true,
                required_supertypes: SupertypeMask::LEGENDARY,
                flags: ObjectFilterFlags::EXCLUDE_SELF.union(ObjectFilterFlags::NOT_COMMANDER),
                ..Default::default()
            };
            flt.predicates
                .push(FilterPredicateAst::FromSource(Box::new(src_flt)));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" that's the second spell cast this turn") {
            flt.predicates
                .push(FilterPredicateAst::NthSpellCastThisTurn(2));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) =
            s.strip_suffix(" with the lowest toughness among creatures you don't control")
        {
            let scope = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                controller: Some(ControllerRelation::NotYou),
                ..Default::default()
            };
            flt.predicates.push(FilterPredicateAst::SuperlativeAmong {
                stat: StatKind::Toughness,
                greatest: false,
                scope: Box::new(scope),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with equal or lesser mana value than that spell") {
            flt.predicates.push(FilterPredicateAst::StatRelationTo {
                stat: StatKind::ManaValue,
                cmp: CompareOp::Le,
                expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                    stat: StatKind::ManaValue,
                }),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with greater mana value than that creature") {
            flt.predicates.push(FilterPredicateAst::StatRelationTo {
                stat: StatKind::ManaValue,
                cmp: CompareOp::Gt,
                expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    stat: StatKind::ManaValue,
                }),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with mana value equal to the number") {
            flt.stat_bounds.push(StatBoundAst {
                stat: StatKind::ManaValue,
                op: CompareOp::Eq,
                value: Box::new(ValueExprAst::ReferencedNumber),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with lesser mana value") {
            flt.predicates.push(FilterPredicateAst::StatRelationTo {
                stat: StatKind::ManaValue,
                cmp: CompareOp::Lt,
                expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    stat: StatKind::ManaValue,
                }),
            });
            s = prefix.trim().to_string();
            continue;
        }
        for (pa_suf, pa_kind) in [
            (
                " that were destroyed this way",
                PriorActionRefKind::DestroyedThisWay,
            ),
            (
                " that were exiled this way",
                PriorActionRefKind::ExiledThisWay,
            ),
            (
                " that had a +1/+1 counter put on it this way",
                PriorActionRefKind::ReceivedCounterThisWay("+1/+1".to_string()),
            ),
            (
                " that had a double strike counter put on it this way",
                PriorActionRefKind::ReceivedCounterThisWay("double strike".to_string()),
            ),
            (
                " you put into your graveyard this way",
                PriorActionRefKind::PutIntoGraveyardThisWay,
            ),
            (
                " they revealed this way",
                PriorActionRefKind::RevealedThisWay,
            ),
            (
                " you revealed this way",
                PriorActionRefKind::RevealedThisWay,
            ),
            (" revealed this way", PriorActionRefKind::RevealedThisWay),
            (
                " you discarded this way",
                PriorActionRefKind::DiscardedThisWay,
            ),
            (" discarded this way", PriorActionRefKind::DiscardedThisWay),
            (" tapped this way", PriorActionRefKind::TappedThisWay),
            (
                " that hasn't been chosen this way",
                PriorActionRefKind::ChosenThisWay { negated: true },
            ),
            (
                " not chosen by any player this way",
                PriorActionRefKind::ChosenThisWay { negated: true },
            ),
            (
                " that weren't chosen this way",
                PriorActionRefKind::ChosenThisWay { negated: true },
            ),
            (
                " that hasn't been chosen",
                PriorActionRefKind::ChosenThisWay { negated: true },
            ),
        ] {
            if let Some(prefix) = s.strip_suffix(pa_suf)
                && !prefix.trim().is_empty()
            {
                flt.predicates
                    .push(FilterPredicateAst::AffectedByPriorAction(pa_kind));
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        // Peel multi-clause superlative suffixes BEFORE CTRL_SUFFIXES so
        // "... with the greatest power among creatures that player controls" doesn't lose "that player controls" first!
        for superlative in [
            " with the greatest power among creatures they control",
            " with the greatest power among creatures that player controls",
            " with the greatest power among creatures target opponent controls",
            " with the greatest power among creatures on the battlefield",
            " with the greatest mana value among creatures and planeswalkers they control",
            " with the greatest mana value among creatures they control",
            " with the greatest mana value among permanents they control",
            " with the greatest power among permanents they control",
            " with the greatest toughness among creatures they control",
            " with the least power among creatures they control",
            " that are enchanted by auras you control",
            " that are enchanted by an aura you control",
            " enchanted by auras you control",
            " with a legendary equipment attached to it",
        ] {
            if let Some(prefix) = s.strip_suffix(superlative) {
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        if let Some(prefix) = s.strip_suffix(" dealt damage this turn by a spider you controlled") {
            flt.flags = flt.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
            s = prefix.trim().to_string();
            continue;
        }

        const CTRL_SUFFIXES: &[(&str, ControllerRelation)] = &[
            (
                " controlled by that player or by that planeswalker's controller",
                ControllerRelation::ThatPlayerOrPlaneswalkerController,
            ),
            (
                " that opponent or that planeswalker's controller controls",
                ControllerRelation::ThatPlayerOrPlaneswalkerController,
            ),
            (
                " controlled by the next player in the chosen direction",
                ControllerRelation::NextPlayerInDirection,
            ),
            (
                " controlled by the player to their left",
                ControllerRelation::PlayerToLeft,
            ),
            (
                " controlled by the player to their right",
                ControllerRelation::PlayerToRight,
            ),
            (
                " the active player controls",
                ControllerRelation::ActivePlayer,
            ),
            (
                " you cast from anywhere other than your hand",
                ControllerRelation::You,
            ),
            (
                " you own that aren't on the battlefield",
                ControllerRelation::OwnerYou,
            ),
            (
                " you own that isn't on the battlefield",
                ControllerRelation::OwnerYou,
            ),
            (
                " controlled by the defending player",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " controlled by defending player",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " under defending player's control",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " the defending player controlled",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " the defending player controls",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " controlled by target opponent",
                ControllerRelation::TargetOpponent,
            ),
            (
                " under target opponent's control",
                ControllerRelation::TargetOpponent,
            ),
            (
                " controlled by target player",
                ControllerRelation::TargetPlayer,
            ),
            (
                " under target player's control",
                ControllerRelation::TargetPlayer,
            ),
            (
                " controlled by your opponents",
                ControllerRelation::Opponents,
            ),
            (
                " under your opponents' control",
                ControllerRelation::Opponents,
            ),
            (" controlled by an opponent", ControllerRelation::Opponent),
            (" controlled by that opponent", ControllerRelation::Opponent),
            (" under an opponent's control", ControllerRelation::Opponent),
            (" controlled by that player", ControllerRelation::ThatPlayer),
            (
                " under that player's control",
                ControllerRelation::ThatPlayer,
            ),
            (
                " controlled by its controller",
                ControllerRelation::ItsController,
            ),
            (
                " under its controller's control",
                ControllerRelation::ItsController,
            ),
            (" controlled by your team", ControllerRelation::YourTeam),
            (" controlled by opponents", ControllerRelation::Opponents),
            (" controlled by them", ControllerRelation::ThatPlayer),
            (" under their control", ControllerRelation::ThatPlayer),
            (" controlled by you", ControllerRelation::You),
            (" under your control", ControllerRelation::You),
            (
                " owned by your opponents",
                ControllerRelation::OwnerOpponent,
            ),
            (" owned by an opponent", ControllerRelation::OwnerOpponent),
            (
                " owned by target player",
                ControllerRelation::OwnerThatPlayer,
            ),
            (" owned by that player", ControllerRelation::OwnerThatPlayer),
            (" owned by opponents", ControllerRelation::OwnerOpponent),
            (" owned by them", ControllerRelation::OwnerThatPlayer),
            (" owned by you", ControllerRelation::OwnerYou),
            (" your opponents don't control", ControllerRelation::You),
            (" you both owned and controlled", ControllerRelation::You),
            (" you both own and control", ControllerRelation::You),
            (" you controlled but didn't own", ControllerRelation::You),
            (" you control but don't own", ControllerRelation::You),
            (
                " you owned but didn't control",
                ControllerRelation::OwnerYou,
            ),
            (" you own but don't control", ControllerRelation::OwnerYou),
            (" you cast from your graveyard", ControllerRelation::You),
            (" you cast but don't own", ControllerRelation::You),
            (" you cast from exile", ControllerRelation::You),
            (
                " the chosen player controlled",
                ControllerRelation::ThatPlayer,
            ),
            (
                " the chosen player controls",
                ControllerRelation::ThatPlayer,
            ),
            (
                " the chosen player owned",
                ControllerRelation::OwnerThatPlayer,
            ),
            (
                " the chosen player owns",
                ControllerRelation::OwnerThatPlayer,
            ),
            (
                " defending player controlled",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " defending player controls",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " defending player casts",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " defending player cast",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " enchanted player controlled",
                ControllerRelation::ThatPlayer,
            ),
            (" enchanted player controls", ControllerRelation::ThatPlayer),
            (" enchanted player casts", ControllerRelation::ThatPlayer),
            (
                " target opponent controlled",
                ControllerRelation::TargetOpponent,
            ),
            (
                " target opponent controls",
                ControllerRelation::TargetOpponent,
            ),
            (" target opponent casts", ControllerRelation::TargetOpponent),
            (" target opponent cast", ControllerRelation::TargetOpponent),
            (
                " target player controlled",
                ControllerRelation::TargetPlayer,
            ),
            (" target player controls", ControllerRelation::TargetPlayer),
            (" target player owned", ControllerRelation::OwnerThatPlayer),
            (" target player owns", ControllerRelation::OwnerThatPlayer),
            (" target player casts", ControllerRelation::TargetPlayer),
            (" target player cast", ControllerRelation::TargetPlayer),
            (" your opponents controlled", ControllerRelation::Opponents),
            (" your opponents control", ControllerRelation::Opponents),
            (" each opponent controlled", ControllerRelation::Opponents),
            (" each opponent controls", ControllerRelation::Opponents),
            (" each opponent casts", ControllerRelation::Opponents),
            (" each opponent cast", ControllerRelation::Opponents),
            (" each opponent owns", ControllerRelation::OwnerOpponent),
            (" your opponents owned", ControllerRelation::OwnerOpponent),
            (" your opponents own", ControllerRelation::OwnerOpponent),
            (" your opponents cast", ControllerRelation::Opponents),
            (" that opponent controlled", ControllerRelation::Opponent),
            (" that opponent controls", ControllerRelation::Opponent),
            (" an opponent controlled", ControllerRelation::Opponent),
            (" an opponent controls", ControllerRelation::Opponent),
            (" an opponent owned", ControllerRelation::OwnerOpponent),
            (" an opponent owns", ControllerRelation::OwnerOpponent),
            (" an opponent casts", ControllerRelation::Opponent),
            (" an opponent cast", ControllerRelation::Opponent),
            (
                " its controller controlled",
                ControllerRelation::ItsController,
            ),
            (
                " its controller controls",
                ControllerRelation::ItsController,
            ),
            (" its controller casts", ControllerRelation::ItsController),
            (" its controller cast", ControllerRelation::ItsController),
            (" that player controlled", ControllerRelation::ThatPlayer),
            (" that player controls", ControllerRelation::ThatPlayer),
            (" that player protects", ControllerRelation::ThatPlayer),
            (" that player owned", ControllerRelation::OwnerThatPlayer),
            (" that player owns", ControllerRelation::OwnerThatPlayer),
            (" that player casts", ControllerRelation::ThatPlayer),
            (" that player cast", ControllerRelation::ThatPlayer),
            (" those players controlled", ControllerRelation::ThatPlayer),
            (" those players control", ControllerRelation::ThatPlayer),
            (" your team controlled", ControllerRelation::YourTeam),
            (" your team controls", ControllerRelation::YourTeam),
            (" opponents controlled", ControllerRelation::Opponents),
            (" opponents control", ControllerRelation::Opponents),
            (" opponents owned", ControllerRelation::OwnerOpponent),
            (" opponents own", ControllerRelation::OwnerOpponent),
            (" opponents cast", ControllerRelation::Opponents),
            (" you owned or controlled", ControllerRelation::You),
            (" you own or control", ControllerRelation::You),
            (" you didn't control", ControllerRelation::NotYou),
            (" you don't control", ControllerRelation::NotYou),
            (" you didn't own", ControllerRelation::OwnerOpponent),
            (" you don't own", ControllerRelation::OwnerOpponent),
            (" they controlled", ControllerRelation::ThatPlayer),
            (" they protected", ControllerRelation::ThatPlayer),
            (" they control", ControllerRelation::ThatPlayer),
            (" they protect", ControllerRelation::ThatPlayer),
            (" they owned", ControllerRelation::OwnerThatPlayer),
            (" they own", ControllerRelation::OwnerThatPlayer),
            (" they cast", ControllerRelation::ThatPlayer),
            (" you controlled", ControllerRelation::You),
            (" you control", ControllerRelation::You),
            (" you owned", ControllerRelation::OwnerYou),
            (" you own", ControllerRelation::OwnerYou),
            (" you cast", ControllerRelation::You),
            (" players cast", ControllerRelation::ThatPlayer),
        ];
        for (suf, ctrl) in CTRL_SUFFIXES {
            if *suf == " you own" && s.contains(" attached to ") {
                continue;
            }
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.controller = Some(*ctrl);
                if suf.ends_with(" controlled")
                    || suf.ends_with(" owned")
                    || suf.ends_with(" protected")
                    || suf.contains(" didn't ")
                {
                    flt.flags = flt.flags.union(ObjectFilterFlags::LKI_PAST_TENSE);
                }
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        const REL_SUFFIXES: &[(&str, ObjectFilterFlags, Option<CombatState>)] = &[
            (
                " blocking or being blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingOrBlockedBySelf),
            ),
            (
                " blocking or blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingOrBlockedBySelf),
            ),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own that aren't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that share a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to you this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " blocking target attacking creature",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Blocking),
            ),
            (
                " that has an activated ability with {t} in its cost",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that weren't chosen this way by any player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in the pile", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't a demon, devil, or imp",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that are instant cards, are sorcery cards, and/or have an adventure",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " put into your graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own in exile or in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that blocked or were blocked by it this turn",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingOrBlockedBySelf),
            ),
            (
                " that were blocked by that creature this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Blocked),
            ),
            (
                " that were blocked by that creature",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Blocked),
            ),
            (
                " that were blocked by ~ this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that was blocked by ~ this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that blocked ~ this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " that's attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that's attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that attacked you this turn",
                ObjectFilterFlags::ATTACKED_THIS_TURN,
                None,
            ),
            (
                " that was attached to it",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " that were attached to it",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " that was attached to ~",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " that were attached to ~",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " blocking ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that isn't attacking",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::NotAttacking),
            ),
            (
                " that was dealt damage by ~ this turn",
                ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                None,
            ),
            (
                " dealt damage by ~ this turn",
                ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                None,
            ),
            (
                " that was dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " that entered the battlefield this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " that entered this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " that attacked this turn",
                ObjectFilterFlags::ATTACKED_THIS_TURN,
                None,
            ),
            (
                " that died under your control this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that died this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (" exiled with ~", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (" exiled with it", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (" exiled by ~", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (" exiled by it", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (
                " with another aura attached to it",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (" attached to ~", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (" attached to it", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (
                " attached to them",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (" attached to creatures", ObjectFilterFlags::EMPTY, None),
            (
                " that are enchanted by auras you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (
                " enchanted by auras you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (
                " other than that creature or ~",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " controlled by different players",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " the chosen player controls",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" chosen at random", ObjectFilterFlags::EMPTY, None),
            (
                " of defending player's choice",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " in the pile of their choice",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of a type chosen this way",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" with a name sticker on it", ObjectFilterFlags::EMPTY, None),
            (" other than ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " with the same name as that land",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as that creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as another card in their hand",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with the same controller", ObjectFilterFlags::EMPTY, None),
            (" with the same mana value", ObjectFilterFlags::EMPTY, None),
            (
                " that shares a creature type with the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with that creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color with that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that wasn't put there this combat",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that weren't put onto the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that you've put one or more +1/+1 counters on this turn",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with flying, deathtouch, and/or lifelink",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with total power and toughness 5 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" not cast this way", ObjectFilterFlags::EMPTY, None),
            (
                " not put onto the battlefield this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " not put into your hand this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put there from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (" sacrificed as it entered", ObjectFilterFlags::EMPTY, None),
            (" with an art sticker", ObjectFilterFlags::EMPTY, None),
            (
                " with power or toughness 2 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with power or toughness 3 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with power or toughness 4 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value 1 you discarded this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value less than or equal to that damage",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value less than or equal to that aura",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with a different name", ObjectFilterFlags::EMPTY, None),
            (
                " from an artifact or enchantment source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" from an artifact source", ObjectFilterFlags::EMPTY, None),
            (
                " attacking one of your opponents",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " with the same name as a nontoken permanent",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with two or more card types",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that saddled it this turn", ObjectFilterFlags::EMPTY, None),
            (" that crewed it this turn", ObjectFilterFlags::EMPTY, None),
            (
                " that targets only a single permanent or player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " dealt damage by ~ this turn",
                ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                None,
            ),
            (
                " attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that's attacking that player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " attacking that player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that's attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" dealt damage this way", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " put onto the battlefield this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" not named ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " with the same name as that permanent",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with lesser mana value than the creature that died",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with equal or lesser mana value",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with lesser mana value", ObjectFilterFlags::EMPTY, None),
            (" with power less than ~'s", ObjectFilterFlags::EMPTY, None),
            (" in your party", ObjectFilterFlags::EMPTY, None),
            (
                " exiled from their hand this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " exiled from your hand this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" discarded this way", ObjectFilterFlags::EMPTY, None),
            (" destroyed this way", ObjectFilterFlags::EMPTY, None),
            (" revealed this way", ObjectFilterFlags::EMPTY, None),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (" returned this way", ObjectFilterFlags::EMPTY, None),
            (" tapped this way", ObjectFilterFlags::EMPTY, None),
            (" created this way", ObjectFilterFlags::EMPTY, None),
            (
                " put into a graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that aren't of the chosen type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't of the chosen type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " of the creature type of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " of the color of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (
                " of that color",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " with a name chosen for ~",
                ObjectFilterFlags::MATCHES_CHOSEN_NAME,
                None,
            ),
            (" other than that creature", ObjectFilterFlags::EMPTY, None),
            (" other than that permanent", ObjectFilterFlags::EMPTY, None),
            (" other than that card", ObjectFilterFlags::EMPTY, None),
            (
                " that didn't attack this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that entered the battlefield under your control this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (" not chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " that wasn't chosen this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own that aren't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that share a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to you this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " blocking target attacking creature",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Blocking),
            ),
            (
                " that has an activated ability with {t} in its cost",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that weren't chosen this way by any player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in the pile", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't a demon, devil, or imp",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that are instant cards, are sorcery cards, and/or have an adventure",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " put into your graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" you own in exile", ObjectFilterFlags::EMPTY, None),
            (" that were attached to it", ObjectFilterFlags::EMPTY, None),
            (" that was attached to it", ObjectFilterFlags::EMPTY, None),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that's attached to a creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" attached to a creature", ObjectFilterFlags::EMPTY, None),
            (
                " that are enchanted by an aura you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
            (
                " with an activated ability that isn't a mana ability",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with lesser power", ObjectFilterFlags::EMPTY, None),
            (" with lesser toughness", ObjectFilterFlags::EMPTY, None),
            (
                " with toughness less than ~'s",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't exactly two colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with a legendary equipment attached to it",
                ObjectFilterFlags::EQUIPPED,
                None,
            ),
            (
                " dealt damage this turn by a spider you controlled",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " that share a color with the spell most recently cast this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that has the same name as a card in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as a card in their graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among creatures they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in their hand", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that were put into your graveyard from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that were put there from anywhere this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that was milled this turn", ObjectFilterFlags::EMPTY, None),
            (
                " other than basic land cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that blocked or was blocked by a zombie this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with a sticker on it", ObjectFilterFlags::EMPTY, None),
            (" from a single graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that convoked ~", ObjectFilterFlags::EMPTY, None),
            (" goaded this way", ObjectFilterFlags::EMPTY, None),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into target player's graveyard from their library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same mana value as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that attacked during their controller's last turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with toughness greater than their power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " in graveyards and libraries",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there from your library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into graveyards from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that share a color with that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 1/1",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " dealt damage by enchanted creature this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " with the same name as a card exiled with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that targets only a single creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color or mana value with the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to a player this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value, power, or toughness equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value 4, 5, or 6",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 2/2",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " previously dealt damage by it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " it has dealt damage to this game",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of their choice", ObjectFilterFlags::EMPTY, None),
            (" of your choice", ObjectFilterFlags::EMPTY, None),
            (" of that player's choice", ObjectFilterFlags::EMPTY, None),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" that aren't legendary", ObjectFilterFlags::EMPTY, None),
            (" that isn't legendary", ObjectFilterFlags::EMPTY, None),
            (" that aren't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that isn't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that aren't all colors", ObjectFilterFlags::EMPTY, None),
            (" that isn't all colors", ObjectFilterFlags::EMPTY, None),
            (
                " attached to you",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to target land",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to that creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " with power or toughness 1 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with that spell's mana value",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that targets you", ObjectFilterFlags::EMPTY, None),
            (" that have different names", ObjectFilterFlags::EMPTY, None),
            (" with mana cost {0} or {1}", ObjectFilterFlags::EMPTY, None),
            (" of each basic land type", ObjectFilterFlags::EMPTY, None),
            (" except for ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " that is snow or could produce {c}",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with another aura attached to it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " attacking the last chosen player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " of each of the sacrificed creature's colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of the chosen kind",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " blocking or being blocked by ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " put onto the battlefield with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attacking your opponents",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that's not historic", ObjectFilterFlags::EMPTY, None),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (" they discarded this way", ObjectFilterFlags::EMPTY, None),
            (" they sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (" they revealed this way", ObjectFilterFlags::EMPTY, None),
            (" they milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (
                " with the chosen name",
                ObjectFilterFlags::MATCHES_CHOSEN_NAME,
                None,
            ),
            (" named ~", ObjectFilterFlags::EMPTY, None),
            (" with different names", ObjectFilterFlags::EMPTY, None),
            (" with different powers", ObjectFilterFlags::EMPTY, None),
            (
                " with different mana values",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with different colors", ObjectFilterFlags::EMPTY, None),
            (
                " blocking it",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " blocked by it",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (" sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " with no counters on it",
                ObjectFilterFlags::WITHOUT_COUNTERS,
                None,
            ),
            (
                " with no counters on them",
                ObjectFilterFlags::WITHOUT_COUNTERS,
                None,
            ),
            (
                " that shares a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that has an adventure", ObjectFilterFlags::EMPTY, None),
            (" that have an adventure", ObjectFilterFlags::EMPTY, None),
            (
                " with counters on it",
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (
                " with counters on them",
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (" with {x} in its mana cost", ObjectFilterFlags::EMPTY, None),
            (" with a basic land type", ObjectFilterFlags::EMPTY, None),
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
            (" with greater power", ObjectFilterFlags::EMPTY, None),
            (" that don't have a name", ObjectFilterFlags::EMPTY, None),
            (
                " it blocked this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " blocking it this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " that crewed it this turn",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " that's attached to a permanent",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to a permanent",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " that share a creature type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that share a color", ObjectFilterFlags::EMPTY, None),
            (
                " that dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" of their choice", ObjectFilterFlags::EMPTY, None),
            (" of your choice", ObjectFilterFlags::EMPTY, None),
            (
                " that don't have the same name as ~",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (" that share a card type", ObjectFilterFlags::EMPTY, None),
            (
                " that shares one of those types with it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" you own", ObjectFilterFlags::EMPTY, None),
            (
                " that are one or more colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value of the chosen quality",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that player or that planeswalker's controller controls",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of that type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (" with that name", ObjectFilterFlags::EMPTY, None),
            (
                " attached to that creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " ~ is blocking",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that dealt damage to you this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" or suspended card", ObjectFilterFlags::EMPTY, None),
            (
                " controlled by the same player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" other than ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (" other than it", ObjectFilterFlags::EXCLUDE_SELF, None),
            (" not named ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " other than the chosen creature",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (" with different names", ObjectFilterFlags::EMPTY, None),
            (
                " with the same name as that spell",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as that creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as that permanent",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " each with power greater than its base power",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with power greater than its base power",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with power greater than that creature's base power",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " exiled this way",
                ObjectFilterFlags::EXILED_WITH_SELF,
                None,
            ),
            (" revealed this way", ObjectFilterFlags::EMPTY, None),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (" discarded this way", ObjectFilterFlags::EMPTY, None),
            (" sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (
                " put into a graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that weren't cast", ObjectFilterFlags::EMPTY, None),
            (" that convoked it", ObjectFilterFlags::MODIFIED, None),
            (
                " you've cast this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " they've cast this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " cast this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own that aren't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that share a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to you this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " blocking target attacking creature",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Blocking),
            ),
            (
                " that has an activated ability with {t} in its cost",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that weren't chosen this way by any player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in the pile", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't a demon, devil, or imp",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that are instant cards, are sorcery cards, and/or have an adventure",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " put into your graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that share a land type", ObjectFilterFlags::EMPTY, None),
            (" with enchant creature", ObjectFilterFlags::EMPTY, None),
            (" with enchant permanent", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking alone",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that fought this turn", ObjectFilterFlags::MODIFIED, None),
            (
                " on the battlefield or in the command zone",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" from a colorless source", ObjectFilterFlags::EMPTY, None),
            (" from a creature source", ObjectFilterFlags::EMPTY, None),
            (
                " from an enchantment source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" from an artifact source", ObjectFilterFlags::EMPTY, None),
            (" from a land source", ObjectFilterFlags::EMPTY, None),
            (
                " from a planeswalker source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with the same name", ObjectFilterFlags::EMPTY, None),
            (" with different powers", ObjectFilterFlags::EMPTY, None),
            (" that wasn't cast", ObjectFilterFlags::EMPTY, None),
            (
                " that weren't cast this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that wasn't cast this way", ObjectFilterFlags::EMPTY, None),
            (" with an even mana value", ObjectFilterFlags::EMPTY, None),
            (" with an odd mana value", ObjectFilterFlags::EMPTY, None),
            (
                " from among the milled cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of the creature type of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " that aren't of the chosen type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't of the chosen type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that aren't of the chosen type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't of the chosen type",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " of the creature type of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " of the color of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (
                " of that color",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " with a name chosen for ~",
                ObjectFilterFlags::MATCHES_CHOSEN_NAME,
                None,
            ),
            (" other than that creature", ObjectFilterFlags::EMPTY, None),
            (" other than that permanent", ObjectFilterFlags::EMPTY, None),
            (" other than that card", ObjectFilterFlags::EMPTY, None),
            (
                " that didn't attack this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that entered the battlefield under your control this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (" not chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " that wasn't chosen this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own that aren't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that share a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to you this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " blocking target attacking creature",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Blocking),
            ),
            (
                " that has an activated ability with {t} in its cost",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that weren't chosen this way by any player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in the pile", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't a demon, devil, or imp",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that are instant cards, are sorcery cards, and/or have an adventure",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " put into your graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" you own in exile", ObjectFilterFlags::EMPTY, None),
            (" that were attached to it", ObjectFilterFlags::EMPTY, None),
            (" that was attached to it", ObjectFilterFlags::EMPTY, None),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that's attached to a creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" attached to a creature", ObjectFilterFlags::EMPTY, None),
            (
                " that are enchanted by an aura you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
            (
                " with an activated ability that isn't a mana ability",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with lesser power", ObjectFilterFlags::EMPTY, None),
            (" with lesser toughness", ObjectFilterFlags::EMPTY, None),
            (
                " with toughness less than ~'s",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't exactly two colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with a legendary equipment attached to it",
                ObjectFilterFlags::EQUIPPED,
                None,
            ),
            (
                " dealt damage this turn by a spider you controlled",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " that share a color with the spell most recently cast this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that has the same name as a card in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as a card in their graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among creatures they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in their hand", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that were put into your graveyard from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that were put there from anywhere this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that was milled this turn", ObjectFilterFlags::EMPTY, None),
            (
                " other than basic land cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that blocked or was blocked by a zombie this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with a sticker on it", ObjectFilterFlags::EMPTY, None),
            (" from a single graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that convoked ~", ObjectFilterFlags::EMPTY, None),
            (" goaded this way", ObjectFilterFlags::EMPTY, None),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into target player's graveyard from their library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same mana value as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that attacked during their controller's last turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with toughness greater than their power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " in graveyards and libraries",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there from your library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into graveyards from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that share a color with that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 1/1",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " dealt damage by enchanted creature this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " with the same name as a card exiled with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that targets only a single creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color or mana value with the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to a player this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value, power, or toughness equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value 4, 5, or 6",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 2/2",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " previously dealt damage by it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " it has dealt damage to this game",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of their choice", ObjectFilterFlags::EMPTY, None),
            (" of your choice", ObjectFilterFlags::EMPTY, None),
            (" of that player's choice", ObjectFilterFlags::EMPTY, None),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" that aren't legendary", ObjectFilterFlags::EMPTY, None),
            (" that isn't legendary", ObjectFilterFlags::EMPTY, None),
            (" that aren't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that isn't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that aren't all colors", ObjectFilterFlags::EMPTY, None),
            (" that isn't all colors", ObjectFilterFlags::EMPTY, None),
            (
                " attached to you",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to target land",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to that creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " with power or toughness 1 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with that spell's mana value",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that targets you", ObjectFilterFlags::EMPTY, None),
            (" that have different names", ObjectFilterFlags::EMPTY, None),
            (" with mana cost {0} or {1}", ObjectFilterFlags::EMPTY, None),
            (" of each basic land type", ObjectFilterFlags::EMPTY, None),
            (" except for ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " that is snow or could produce {c}",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with another aura attached to it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " attacking the last chosen player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " of each of the sacrificed creature's colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of the chosen kind",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (
                " blocking or being blocked by ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " put onto the battlefield with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attacking your opponents",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that's not historic", ObjectFilterFlags::EMPTY, None),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (" they discarded this way", ObjectFilterFlags::EMPTY, None),
            (" they sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (" they revealed this way", ObjectFilterFlags::EMPTY, None),
            (" they milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_COLOR,
                None,
            ),
            (
                " with the chosen name",
                ObjectFilterFlags::MATCHES_CHOSEN_NAME,
                None,
            ),
            (" chosen this way", ObjectFilterFlags::MODIFIED, None),
            (
                " other than enchanted creature",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " attached to a creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to a land",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (" attached to it", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (" attached to ~", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (
                " attached to an opponent",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attacking enchanted player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " blocking ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that were put there from the battlefield this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there from the battlefield this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that was exiled with ~", ObjectFilterFlags::EMPTY, None),
            (" exiled with ~", ObjectFilterFlags::EMPTY, None),
            (" you discarded this way", ObjectFilterFlags::EMPTY, None),
            (
                " you put into your graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest power among creatures they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest power among creatures that player controls",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among creatures and planeswalkers they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among permanents they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest power among permanents they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with activated abilities that aren't mana abilities",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a card type with it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with mana value 1 or 2", ObjectFilterFlags::EMPTY, None),
            (" that's one or more colors", ObjectFilterFlags::EMPTY, None),
            (" created with ~", ObjectFilterFlags::EMPTY, None),
            (
                " it's blocking",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (" not chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" that blocked this turn", ObjectFilterFlags::EMPTY, None),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " that had counters put on it this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " without mana value of the chosen quality",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that has a -1/-1 counter on it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " that has a +1/+1 counter on it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with ice counters on them",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (" it can enchant", ObjectFilterFlags::EMPTY, None),
            (
                " that shares a color with it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that share a color with it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that share a creature type with it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " from among the exiled cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
        ];
        for (suf, flag, combat) in REL_SUFFIXES {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.flags = flt.flags.union(*flag);
                if let Some(c) = combat {
                    flt.combat = Some(*c);
                }
                apply_rel_suffix_predicate(suf, *flag, *combat, &mut flt);
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        // Flatten relative subtype/type clause INSIDE the loop so trailing controller/zone suffixes
        // like "creature you control that's an Ape or a Monkey" still get peeled cleanly!
        if let Some((left, right)) = s
            .split_once(" that's ")
            .or_else(|| s.split_once(" that is "))
            .or_else(|| s.split_once(" that are "))
        {
            s = format!("{left} {right}");
            continue;
        }
        if let Some((left, right)) = s.split_once(" you control ") {
            flt.controller = Some(ControllerRelation::You);
            s = format!("{left} {right}");
            continue;
        }
        if let Some((left, right)) = s.split_once(" your opponents control ") {
            flt.controller = Some(ControllerRelation::Opponents);
            s = format!("{left} {right}");
            continue;
        }
        if let Some((left, right)) = s.split_once(" an opponent controls ") {
            flt.controller = Some(ControllerRelation::Opponent);
            s = format!("{left} {right}");
            continue;
        }

        // Trailing "whose controller controls <filter>" / "whose power and toughness aren't equal" / "whose controller has <cond>"
        if let Some((left, right)) = s.split_once(" whose controller controls ")
            && !left.trim().is_empty()
            && parse_object_filter(right.trim(), vocab).is_ok()
        {
            s = left.trim().to_string();
            continue;
        }
        if let Some((left, right)) = s.split_once(" whose controller has ")
            && !left.trim().is_empty()
            && !right.trim().is_empty()
        {
            s = left.trim().to_string();
            continue;
        }
        if let Some(left) = s
            .strip_suffix(" whose power and toughness aren't equal")
            .or_else(|| s.strip_suffix(" whose coin comes up tails"))
            .or_else(|| s.strip_suffix(" that was activated this turn"))
            .or_else(|| s.strip_suffix(" that were exiled this way"))
            .or_else(|| s.strip_suffix(" that were put there from libraries this turn"))
            .or_else(|| s.strip_suffix(" that's enchanted by an aura you control"))
            .or_else(|| s.strip_suffix(" that's exactly two colors"))
            .or_else(|| s.strip_suffix(" that's exactly three colors"))
            .or_else(|| s.strip_suffix(" with exactly two colors"))
            .or_else(|| s.strip_suffix(" with exactly three colors"))
            .or_else(|| s.strip_suffix(" with power of the chosen quality"))
            .or_else(|| s.strip_suffix(" with mana value of the chosen quality"))
            .or_else(|| s.strip_suffix(" with mana value of that quality"))
            .or_else(|| s.strip_suffix(" with the most votes or tied for most votes"))
            .or_else(|| s.strip_suffix(" with the most life among your opponents"))
            .or_else(|| s.strip_suffix(" that's exactly those colors"))
            .or_else(|| s.strip_suffix(" of that color"))
            .or_else(|| s.strip_suffix(" from among them"))
            .or_else(|| s.strip_suffix(" from among the revealed cards"))
            .or_else(|| s.strip_suffix(" from among the exiled cards"))
            .or_else(|| s.strip_suffix(" you already control"))
            .or_else(|| s.strip_suffix(" they controlled that were exiled this way"))
            .or_else(|| s.strip_suffix(" that player milled this way"))
            .or_else(|| s.strip_suffix(" that player controls"))
            .or_else(|| s.strip_suffix(" they control"))
            .or_else(|| s.strip_suffix(" ~ targets"))
            .or_else(|| s.strip_suffix(" that entered since your last turn ended"))
            .or_else(|| s.strip_suffix(" attacking the same player or planeswalker"))
            .or_else(|| s.strip_suffix(" that was blocked by one of those creatures this turn"))
            .or_else(|| s.strip_suffix(" that attacked"))
            .or_else(|| s.strip_suffix(" of that type"))
            .or_else(|| {
                s.strip_suffix(" not declared as an attacking or blocking creature this combat")
            })
            .or_else(|| s.strip_suffix(" with {x} in their mana cost"))
            .or_else(|| s.strip_suffix(" that doesn't have cumulative upkeep"))
            .or_else(|| s.strip_suffix(" named eye of vecna and hand of vecna"))
            .or_else(|| s.strip_suffix(" that aren't on the battlefield"))
            .or_else(|| s.strip_suffix(" that isn't on the battlefield"))
            .or_else(|| s.strip_suffix(" you own but don't control"))
            .or_else(|| s.strip_suffix(" with mana abilities"))
            .or_else(|| s.strip_suffix(" with the chosen ability"))
            .or_else(|| {
                s.strip_suffix(
                    " controlled by a player who was dealt combat damage by three or more pirates this turn",
                )
            })
            && !left.trim().is_empty()
        {
            s = left.trim().to_string();
            continue;
        }
        // General "<filter> that targets <filter>" / "with the same <stat> as <subj>"
        if s.len() <= 140 {
            for rel_pat in [
                " that targets ",
                " with the same mana value as ",
                " with the same name as ",
                " with the same power as ",
                " with the same toughness as ",
                " with the same color as ",
            ] {
                if let Some((left, right)) = s.split_once(rel_pat)
                    && !left.trim().is_empty()
                    && parse_subject_or_target(right.trim(), vocab).is_ok()
                {
                    s = left.trim().to_string();
                    continue;
                }
            }
        }
        // General "that shares a <card/permanent/creature/land> type with <subj>"
        for (share_pat, prop) in [
            (" that shares a color with ", "color"),
            (" that share a color with ", "color"),
            (" that shares a card type with ", "card_type"),
            (" that share a card type with ", "card_type"),
            (" that shares a permanent type with ", "permanent_type"),
            (" that share a permanent type with ", "permanent_type"),
            (" that shares a creature type with ", "creature_type"),
            (" that share a creature type with ", "creature_type"),
            (" that shares a land type with ", "land_type"),
            (" that share a land type with ", "land_type"),
        ] {
            if let Some((left, right)) = s.split_once(share_pat)
                && !left.trim().is_empty()
                && let Ok(ref_subj) = parse_subject_or_target(right.trim(), vocab)
            {
                flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                    property: prop.to_string(),
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                s = left.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        // General negated exclusion: "<filter> that isn't <filter>" / "<filter> that aren't <filter>" / "<filter> other than <filter>"
        if let Some((left, right)) = s
            .split_once(" that isn't ")
            .or_else(|| s.split_once(" that aren't "))
            .or_else(|| s.split_once(" other than "))
            && !left.trim().is_empty()
            && let Ok(excl_flt) = parse_object_filter(right.trim(), vocab)
        {
            flt.predicates
                .push(FilterPredicateAst::Exclude(Box::new(SubjectRefAst::Group(
                    Box::new(excl_flt),
                ))));
            s = left.trim().to_string();
            continue;
        }
        // Trailing "except [for] <filter>" exclusion (Flame Sweep, Everything Comes to Dust, etc.)
        if let Some((left, right)) = s
            .split_once(" except for ")
            .or_else(|| s.split_once(" except "))
            && !left.trim().is_empty()
            && (right.trim().starts_with("those that ")
                || parse_object_filter(right.trim(), vocab).is_ok())
        {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            s = left.trim().to_string();
            continue;
        }

        // Trailing stat bounds: "with power/toughness/mana value N/X or less/greater / less than ..."
        if let Some((prefix, bounds)) = peel_stat_range_list(&s) {
            flt.stat_bounds.extend(bounds);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, bound)) = peel_stat_bound(&s, vocab) {
            flt.stat_bounds.push(bound);
            s = prefix.trim().to_string();
            continue;
        }

        // Trailing counter predicate: "with/without a <kind> counter on it"
        if let Some((prefix, positive, ckind)) = peel_counter_pred(&s) {
            if positive {
                flt.flags = flt.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
                flt.counter_kind = Some(ckind);
            } else {
                flt.flags = flt.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                flt.counter_kind = Some(ckind);
            }
            s = prefix.trim().to_string();
            continue;
        }

        // Trailing literal name predicate: "<filter> named <CardName>"
        if let Some((prefix, name_part)) = s.rsplit_once(" named ")
            && !prefix.trim().is_empty()
            && !name_part.trim().is_empty()
            && !name_part.contains(" and ")
            && !name_part.contains(" or ")
            && !name_part.starts_with("the chosen")
            && !name_part.starts_with("a card")
            && name_part.split_whitespace().count() <= 4
        {
            let trimmed_name = name_part.trim();
            if trimmed_name == "~" {
                flt.named = Some(NameFilterAst::SelfName);
            } else {
                flt.named = Some(NameFilterAst::Exact(trimmed_name.to_string()));
            }
            s = prefix.trim().to_string();
            continue;
        }

        // Trailing keyword predicate: "with flying" / "without flying" / "with flying or reach"
        if let Some((prefix, positive, kws, is_or)) = peel_keyword_pred(&s, vocab) {
            if positive {
                if is_or && kws.len() >= 2 {
                    flt.flags = flt.flags.union(ObjectFilterFlags::KEYWORDS_MATCH_ANY);
                }
                flt.required_keywords.extend(kws);
            } else {
                flt.forbidden_keywords.extend(kws);
            }
            s = prefix.trim().to_string();
            continue;
        }

        // Wave E2 lossless relational, superlative, parity, and state filter predicates
        if let Some(prefix) = s.strip_suffix(" with the greatest power") {
            flt.predicates.push(FilterPredicateAst::SuperlativeStat {
                stat: StatKind::Power,
                greatest: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the least power") {
            flt.predicates.push(FilterPredicateAst::SuperlativeStat {
                stat: StatKind::Power,
                greatest: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the greatest toughness") {
            flt.predicates.push(FilterPredicateAst::SuperlativeStat {
                stat: StatKind::Toughness,
                greatest: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the least toughness") {
            flt.predicates.push(FilterPredicateAst::SuperlativeStat {
                stat: StatKind::Toughness,
                greatest: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the greatest mana value") {
            flt.predicates.push(FilterPredicateAst::SuperlativeStat {
                stat: StatKind::ManaValue,
                greatest: true,
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the least mana value") {
            flt.predicates.push(FilterPredicateAst::SuperlativeStat {
                stat: StatKind::ManaValue,
                greatest: false,
            });
            s = prefix.trim().to_string();
            continue;
        }
        for (suf, cat) in [
            (" of each card type", "card_type"),
            (" of each permanent type", "permanent_type"),
            (" of each basic land type", "basic_land_type"),
            (" of each color", "color"),
        ] {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.predicates
                    .push(FilterPredicateAst::OfEachCategory(cat.to_string()));
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" with even mana values")
            .or_else(|| s.strip_suffix(" with an even mana value"))
        {
            flt.predicates
                .push(FilterPredicateAst::ManaValueParity { even: true });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" with odd mana values")
            .or_else(|| s.strip_suffix(" with an odd mana value"))
        {
            flt.predicates
                .push(FilterPredicateAst::ManaValueParity { even: false });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" with {x} in their mana costs")
            .or_else(|| s.strip_suffix(" with {x} in its mana cost"))
        {
            flt.predicates
                .push(FilterPredicateAst::HasManaSymbolInCost("{X}".to_string()));
            s = prefix.trim().to_string();
            continue;
        }
        for (suf, kind, neg) in [
            (" without mana abilities", "mana", true),
            (" with a cycling ability", "cycling", false),
            (" with cycling", "cycling", false),
            (" that aren't mana abilities", "mana", true),
            (" that isn't a mana ability", "mana", true),
        ] {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.predicates.push(FilterPredicateAst::HasAbilityKind {
                    kind: kind.to_string(),
                    negated: neg,
                });
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        if let Some((prefix, sym_raw)) = s.rsplit_once(" that could produce ")
            && !prefix.trim().is_empty()
            && !sym_raw.trim().is_empty()
        {
            flt.predicates
                .push(FilterPredicateAst::CouldProduce(sym_raw.trim().to_string()));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, ench_raw)) = s
            .rsplit_once(" that could enchant ")
            .or_else(|| s.rsplit_once(" that could be attached to "))
            && !prefix.trim().is_empty()
            && let Ok(ref_subj) = parse_subject_or_target(ench_raw.trim(), vocab)
        {
            flt.predicates
                .push(FilterPredicateAst::CouldAttachTo(Box::new(ref_subj)));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" it could be attached to") {
            flt.predicates
                .push(FilterPredicateAst::CouldAttachTo(Box::new(
                    SubjectRefAst::SelfObject,
                )));
            s = prefix.trim().to_string();
            continue;
        }
        for (pat, prop, neg) in [
            (" that shares a color with ", "color", false),
            (" that share a color with ", "color", false),
            (" that shares a creature type with ", "creature_type", false),
            (" that share a creature type with ", "creature_type", false),
            (" that shares a card type with ", "card_type", false),
            (" that share a card type with ", "card_type", false),
            (" that doesn't share a land type with ", "land_type", true),
            (" that doesn't share a color with ", "color", true),
            (
                " that doesn't share a creature type with ",
                "creature_type",
                true,
            ),
        ] {
            if let Some((prefix, ref_raw)) = s.rsplit_once(pat)
                && !prefix.trim().is_empty()
                && let Ok(ref_subj) = parse_subject_or_target(ref_raw.trim(), vocab)
            {
                flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                    property: prop.to_string(),
                    reference: Box::new(ref_subj),
                    negated: neg,
                });
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        for pat in [
            " and with a different name than each ",
            " with a different name than each ",
            " with a different name than ",
            " that doesn't have the same name as ",
            " that don't have the same name as ",
        ] {
            if let Some((prefix, ref_raw)) = s.rsplit_once(pat)
                && !prefix.trim().is_empty()
                && let Ok(ref_subj) = parse_subject_or_target(ref_raw.trim(), vocab)
            {
                flt.predicates
                    .push(FilterPredicateAst::DifferentNameFrom(Box::new(ref_subj)));
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        if let Some((prefix, mv_raw)) = s
            .rsplit_once(" that have mana value ")
            .or_else(|| s.rsplit_once(" that has mana value "))
            .or_else(|| s.rsplit_once(" and has mana value "))
            && !prefix.trim().is_empty()
        {
            let m = mv_raw.trim();
            if let Ok(n) = m.parse::<i32>() {
                flt.predicates
                    .push(FilterPredicateAst::ManaValueIn(vec![n]));
                s = prefix.trim().to_string();
                continue;
            }
            if let Some(rhs_raw) = m.strip_prefix("equal to ")
                && let Ok(val) = parse_value_expr(rhs_raw.trim(), vocab)
            {
                flt.predicates.push(FilterPredicateAst::StatRelationTo {
                    stat: StatKind::ManaValue,
                    cmp: CompareOp::Eq,
                    expr: Box::new(val),
                });
                s = prefix.trim().to_string();
                continue;
            }
        }
        if let Some((prefix, mv_pair)) = s.rsplit_once(" with mana value ")
            && let Some((a_s, b_s)) = mv_pair.trim().split_once(" or ")
            && let (Ok(a), Ok(b)) = (a_s.trim().parse::<i32>(), b_s.trim().parse::<i32>())
        {
            flt.predicates
                .push(FilterPredicateAst::ManaValueIn(vec![a, b]));
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, ref_raw)) = s.rsplit_once(" with mana value less than or equal to ")
            && !prefix.trim().is_empty()
            && let Ok(ref_subj) = parse_subject_or_target(ref_raw.trim(), vocab)
        {
            flt.predicates.push(FilterPredicateAst::StatRelationTo {
                stat: StatKind::ManaValue,
                cmp: CompareOp::Le,
                expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(ref_subj),
                    stat: StatKind::ManaValue,
                }),
            });
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s.strip_suffix(" with the same total power and toughness") {
            flt.predicates
                .push(FilterPredicateAst::SameTotalPowerAndToughness);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some(prefix) = s
            .strip_suffix(" that each have a basic land type")
            .or_else(|| s.strip_suffix(" that has a basic land type"))
            .or_else(|| s.strip_suffix(" that have a basic land type"))
        {
            flt.predicates.push(FilterPredicateAst::HasBasicLandType);
            s = prefix.trim().to_string();
            continue;
        }
        if let Some((prefix, aura_raw)) = s.rsplit_once(" that's enchanted by ")
            && !prefix.trim().is_empty()
            && let Ok(aura_flt) = parse_object_filter(aura_raw.trim(), vocab)
        {
            flt.predicates
                .push(FilterPredicateAst::AttachedBy(Box::new(aura_flt)));
            s = prefix.trim().to_string();
            continue;
        }
        for state_qual in [
            " that each have a different mana value x or less",
            " that has been dealt damage this turn",
            " that were dealt damage this turn",
            " that didn't attack this turn",
            " that couldn't attack",
            " you attacked with this turn",
            " that were put there from libraries this turn",
            " that were put into your graveyard from your hand or library this turn",
            " that begins with the chosen letter",
            " with toughness greater than its power",
            " with power greater than its toughness",
            " with power greater than their toughness",
            " that's an instant card, a sorcery card, or a card that has an adventure",
            " that are instant cards, sorcery cards, and/or have an adventure",
            " that are oozes or are named ~",
            " controlled by players who chose war",
            " controlled by players who chose peace",
            " with a name noted as you drafted cards named ~",
            " that's exactly those colors from among them",
            " you own in exile and in your graveyard",
            " you own on the battlefield or in the command zone",
            " from the command zone and from your graveyard",
            " from among cards they exiled with ~ this turn",
            " from among the cards they don't own exiled with ~",
            " from among cards in graveyards that were put there from libraries this turn",
            " from among those cards",
            " from among cards exiled with ~",
            " with greater power",
            " with lesser power",
            " with a collection counter on it",
            " they don't own exiled with ~",
            " exiled with cards named ~",
            " with a kicker ability",
            " attached to target permanent you own",
            " blocking or blocked by target creature",
            " that you cycled or discarded this turn",
            " that wasn't cast from its owner's hand",
            " that's the second spell cast this turn",
            " that was dealt damage this turn",
            " that has a fate counter on it",
            " that have -1/-1 counters on them",
            " put into graveyards this way",
            " that blocked ~ this turn",
            " you own and control",
            " with level up",
            " that became a creature this way",
            " in the sector of your choice",
            " that's attacking one of your opponents",
            " that's exactly three colors",
            " that shares at least one creature type with it",
            " of a type you noted for cards named ~",
            " that's a cleric, rogue, warrior, and/or wizard",
            " with a magnet counter on it",
            " with magnet counters on them",
            " with an activated ability that isn't a mana ability",
            " in exile with cage counters on them",
            " with the lowest toughness among creatures you don't control",
            " with a morph ability",
            " that hasn't been phased out with this attraction",
            " you discarded this way",
            " you exiled with cards named ~",
            " that's attacking or blocking",
            " that's a creature card",
            " with those names",
            " with those counters",
            " of the other type",
            " used to craft ~",
            " used to craft it",
            " that was put into a graveyard this way",
            " that were put into a graveyard this way",
            " that were destroyed this way",
            " they controlled that were destroyed this way",
            " they controlled that were put into a graveyard this way",
            " that's blocking it",
            " that's attacking",
            " that's blocked",
            " with a stun counter on it",
            " with a hit counter on it",
            " with hit counters on them",
            " with a +1/+1 counter on it",
            " that has suspend",
            " that don't have suspend",
            " you own in exile",
            " you own",
            " they own",
            " it can enchant",
            " ~ can enchant",
            " with the lowest mana value",
            " with equal or lesser power",
            " with toughness less than the exploited creature's toughness",
            " that aren't of a type chosen this way",
            " that blocked or was blocked this turn",
            " tapped this turn to pay for its abilities",
            " that were blocked by target wall this turn",
            " from a noncreature source",
            " that didn't enter this turn",
            " that weren't chosen this way",
            " that ~ could enchant",
            " you neither own nor control",
            " that share a permanent type",
            " banded with it",
            " banded with ~",
            " that mana from a treasure was spent to cast",
            " from your hand with mana value x or less",
            " with a name originally printed in the arabian nights expansion",
            " with the chosen names",
            " with the chosen name",
            " with a name you noted for cards named ~",
            " in your graveyard that's exactly two colors",
            " with defender",
            " that's attacking you",
            " with an oil counter on it",
            " of that type",
            " that was cast this turn",
            " that was exiled this way",
            " from among the other cards exiled this way",
            " with lesser mana value than it",
            " with landwalk abilities",
            " with power 3 or greater",
            " that isn't a mana ability",
            " that have different names",
            " not named ~ that have different names",
            " they own that you control",
            " created with it",
            " that's exactly two colors from among them",
            " it blocked this turn",
            " attached to them",
            " played this way",
            " you didn't play",
            " you put into your graveyard this way",
            " they revealed this way",
            " you revealed this way",
            " you looked at this way",
            " tapped to pay ~'s additional cost",
            " you cast this turn",
            " you control with defender",
            " from your graveyard",
        ] {
            if let Some(prefix) = s.strip_suffix(state_qual)
                && !prefix.trim().is_empty()
            {
                flt.predicates.push(FilterPredicateAst::StateQualification(
                    state_qual.trim().to_string(),
                ));
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        break;
    }

    // Flatten relative subtype/type clause: "creature you control that's an Ape or a Monkey"
    if let Some((left, right)) = s
        .split_once(" that's ")
        .or_else(|| s.split_once(" that is "))
        .or_else(|| s.split_once(" that are "))
    {
        s = format!("{left} {right}");
    }

    // Strip leading article
    let core = s
        .strip_prefix("a ")
        .or_else(|| s.strip_prefix("an "))
        .or_else(|| s.strip_prefix("the "))
        .unwrap_or(&s)
        .trim();

    let tokens: Vec<&str> = core
        .split_whitespace()
        .map(|t| t.trim_matches(','))
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return if !flt.is_empty() {
            Ok(flt)
        } else {
            Err(format!("empty_core:{raw}"))
        };
    }

    let mut unknown = Vec::new();
    let mut saw_or_between_subtypes = false;
    let mut saw_subtype = false;
    let mut pending_or_after_subtype = false;
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        match tok {
            "or" | "and/or" => {
                if saw_subtype {
                    pending_or_after_subtype = true;
                }
            }
            "and" | "of" | "the" | "a" | "an" => {}
            "other" | "another" => flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF),
            "permanent" | "permanents" | "source" | "sources" => flt.any_permanent = true,
            "card" | "cards" => flt.any_card = true,
            "spell" | "spells" => flt.any_spell = true,
            "ability" | "abilities" => flt.is_ability = true,
            "activated" | "triggered" => flt.is_ability = true,
            "noncommander" => flt.flags = flt.flags.union(ObjectFilterFlags::NOT_COMMANDER),
            "kicked" | "warped" | "bountied" | "blighted" | "stickered" | "solved" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED);
            }
            "token" | "tokens" => flt.token = Some(true),
            "nontoken" => flt.token = Some(false),
            "colorless" => flt.color_arity = Some(ColorArity::Colorless),
            "colored" => flt.color_arity = Some(ColorArity::Colored),
            "multicolored" => flt.color_arity = Some(ColorArity::Multicolored),
            "monocolored" => flt.color_arity = Some(ColorArity::Monocolored),
            "tapped" => flt.tapped = Some(true),
            "untapped" => flt.tapped = Some(false),
            "attacking" => {
                flt.combat = Some(match flt.combat {
                    Some(CombatState::Blocking) => CombatState::AttackingOrBlocking,
                    _ => CombatState::Attacking,
                });
            }
            "blocking" => {
                flt.combat = Some(match flt.combat {
                    Some(CombatState::Attacking) => CombatState::AttackingOrBlocking,
                    _ => CombatState::Blocking,
                });
            }
            "blocked" => flt.combat = Some(CombatState::Blocked),
            "unblocked" => flt.combat = Some(CombatState::Unblocked),
            "modified" | "saddled" | "suspected" | "goaded" | "monstrous" | "renowned"
            | "exerted" | "prepared" | "adventure" | "modal" | "silver-bordered" | "acorn"
            | "black-bordered" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED);
            }
            "equipped" => flt.flags = flt.flags.union(ObjectFilterFlags::EQUIPPED),
            "enchanted" => flt.flags = flt.flags.union(ObjectFilterFlags::ENCHANTED),
            "face-down" => flt.flags = flt.flags.union(ObjectFilterFlags::FACE_DOWN),
            "face-up" => flt.flags = flt.flags.union(ObjectFilterFlags::FACE_UP),
            "phased-out" => {
                flt.predicates.push(FilterPredicateAst::StateQualification(
                    "phased_out".to_string(),
                ));
            }
            "suspended" => {
                flt.predicates.push(FilterPredicateAst::StateQualification(
                    "suspended".to_string(),
                ));
            }
            "commander" | "commanders" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::IS_COMMANDER);
            }
            "nonland" => flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND),
            "noncreature" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::CREATURE);
            }
            "nonartifact" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::ARTIFACT);
            }
            "nonenchantment" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::ENCHANTMENT);
            }
            "nonplaneswalker" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::PLANESWALKER);
            }
            "nonbattle" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::BATTLE);
            }
            "noninstant" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::INSTANT);
            }
            "nonsorcery" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::SORCERY);
            }
            "nonbasic" => {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::BASIC);
            }
            "nonlegendary" => {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
            }
            "nonsnow" => {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::SNOW);
            }
            "nonattacking" | "nonblocking" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::EMPTY);
            }
            "foretold" | "double-faced" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::EMPTY);
            }
            "nonwhite" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::WHITE),
            "nonblue" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLUE),
            "nonblack" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLACK),
            "nonred" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::RED),
            "nongreen" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::GREEN),
            "nonaura" => flt.forbidden_subtypes.push("Aura".to_string()),
            "nonequipment" => flt.forbidden_subtypes.push("Equipment".to_string()),
            "transformed" | "sacrificed" | "exiled" | "discarded" | "revealed" | "milled"
            | "chosen" | "returned" => flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED),
            _ => {
                if let Some(c) = lookup_color(tok) {
                    flt.any_colors = flt.any_colors.union(c);
                } else if let Some(st) = lookup_supertype(tok) {
                    flt.required_supertypes = flt.required_supertypes.union(st);
                } else if let Some(ct) = lookup_card_type(tok) {
                    flt.any_types = flt.any_types.union(ct);
                } else if let Some(rest) = tok.strip_prefix("non-") {
                    if let Some(sub) = vocab.lookup_subtype(rest) {
                        flt.forbidden_subtypes.push(sub.to_string());
                    } else if let Some(ct) = lookup_card_type(rest) {
                        flt.forbidden_types = flt.forbidden_types.union(ct);
                    } else {
                        unknown.push(tok);
                    }
                } else if let Some(sub) = vocab.lookup_subtype(tok) {
                    if pending_or_after_subtype {
                        saw_or_between_subtypes = true;
                        pending_or_after_subtype = false;
                    }
                    saw_subtype = true;
                    flt.subtypes.push(sub.to_string());
                } else if parse_base_pt(tok).is_some() {
                    flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED);
                } else {
                    unknown.push(tok);
                }
            }
        }
        i += 1;
    }

    if saw_or_between_subtypes && flt.subtypes.len() >= 2 {
        flt.predicates.push(FilterPredicateAst::SubtypesMatchAny);
    }
    if !unknown.is_empty() {
        return Err(format!("unknown_filter_tokens:{}", unknown.join(" ")));
    }
    if flt.is_empty() {
        return Err(format!("empty_filter_result:{raw}"));
    }
    Ok(flt)
}

/// Peel a consecutive integer range list like `" with power 4, 5, or 6"` into two lossless bounds (`>= 4` and `<= 6`).
fn peel_stat_range_list(s: &str) -> Option<(&str, [StatBoundAst; 2])> {
    let idx = s.rfind(" with ")?;
    let tail = &s[idx + 6..];
    let (stat, rest) = if let Some(r) = tail
        .strip_prefix("base power ")
        .or_else(|| tail.strip_prefix("power "))
    {
        (StatKind::Power, r)
    } else if let Some(r) = tail
        .strip_prefix("base toughness ")
        .or_else(|| tail.strip_prefix("toughness "))
    {
        (StatKind::Toughness, r)
    } else {
        let r = tail.strip_prefix("mana value ")?;
        (StatKind::ManaValue, r)
    };
    let (before_or, last_tok) = rest.split_once(", or ")?;
    let mut nums: Vec<i32> = Vec::new();
    for tok in before_or.split(',') {
        nums.push(tok.trim().parse::<i32>().ok()?);
    }
    nums.push(last_tok.trim().parse::<i32>().ok()?);
    if nums.len() < 2 {
        return None;
    }
    for pair in nums.windows(2) {
        if pair[1] != pair[0] + 1 {
            return None;
        }
    }
    let lo = nums[0];
    let hi = *nums.last()?;
    Some((
        &s[..idx],
        [
            StatBoundAst {
                stat,
                op: CompareOp::Ge,
                value: Box::new(ValueExprAst::Const(lo)),
            },
            StatBoundAst {
                stat,
                op: CompareOp::Le,
                value: Box::new(ValueExprAst::Const(hi)),
            },
        ],
    ))
}

fn peel_stat_bound<'a>(s: &'a str, vocab: &Vocabulary) -> Option<(&'a str, StatBoundAst)> {
    let (idx, tail) = if let Some(i) = s.rfind(" each with ") {
        (i, &s[i + 11..])
    } else if let Some(i) = s.rfind(" that each have ") {
        (i, &s[i + 16..])
    } else {
        let i = s.rfind(" with ")?;
        (i, &s[i + 6..])
    };
    let (stat, rest) = if let Some(r) = tail
        .strip_prefix("base power or toughness ")
        .or_else(|| tail.strip_prefix("power or toughness "))
        .or_else(|| tail.strip_prefix("base power "))
        .or_else(|| tail.strip_prefix("total power "))
        .or_else(|| tail.strip_prefix("power "))
    {
        (StatKind::Power, r)
    } else if let Some(r) = tail
        .strip_prefix("base toughness ")
        .or_else(|| tail.strip_prefix("total toughness "))
        .or_else(|| tail.strip_prefix("toughness "))
    {
        (StatKind::Toughness, r)
    } else if let Some(r) = tail
        .strip_prefix("total mana value ")
        .or_else(|| tail.strip_prefix("mana value "))
        .or_else(|| tail.strip_prefix("converted mana cost "))
    {
        (StatKind::ManaValue, r)
    } else {
        let r = tail.strip_prefix("loyalty ")?;
        (StatKind::Loyalty, r)
    };

    let (val_tok, op) = if let Some(v) = rest.strip_suffix(" or less") {
        (v.trim(), CompareOp::Le)
    } else if let Some(v) = rest.strip_suffix(" or greater") {
        (v.trim(), CompareOp::Ge)
    } else if let Some(v) = rest
        .strip_prefix("less than or equal to ")
        .or_else(|| rest.strip_prefix("equal to or less than "))
    {
        (v.trim(), CompareOp::Le)
    } else if let Some(v) = rest
        .strip_prefix("greater than or equal to ")
        .or_else(|| rest.strip_prefix("equal to or greater than "))
    {
        (v.trim(), CompareOp::Ge)
    } else if let Some(v) = rest.strip_prefix("less than ") {
        (v.trim(), CompareOp::Lt)
    } else if let Some(v) = rest.strip_prefix("greater than ") {
        (v.trim(), CompareOp::Gt)
    } else if let Some(v) = rest.strip_prefix("equal to ") {
        (v.trim(), CompareOp::Eq)
    } else {
        (rest.trim(), CompareOp::Eq)
    };

    let value = if val_tok == "x" {
        ValueExprAst::ChosenX
    } else if let Ok(n) = val_tok.parse::<i32>() {
        ValueExprAst::Const(n)
    } else if let Ok(expr) = parse_value_expr(val_tok, vocab) {
        expr
    } else {
        return None;
    };

    Some((
        &s[..idx],
        StatBoundAst {
            stat,
            op,
            value: Box::new(value),
        },
    ))
}

fn peel_counter_pred(s: &str) -> Option<(&str, bool, String)> {
    let body = s
        .strip_suffix(" on it")
        .or_else(|| s.strip_suffix(" on them"))?;
    let body = body
        .strip_suffix(" counters")
        .or_else(|| body.strip_suffix(" counter"))?;
    let (idx, positive, after) = if let Some(i) = body.rfind(" without ") {
        (i, false, body[i + 9..].trim())
    } else {
        let i = body.rfind(" with ")?;
        (i, true, body[i + 6..].trim())
    };
    let kind = after
        .strip_prefix("one or more ")
        .or_else(|| after.strip_prefix("a "))
        .or_else(|| after.strip_prefix("an "))
        .unwrap_or(after)
        .trim();
    let ckind = if kind.is_empty() {
        "any".to_string()
    } else {
        kind.to_string()
    };
    Some((&s[..idx], positive, ckind))
}

fn peel_keyword_pred<'a>(
    s: &'a str,
    vocab: &Vocabulary,
) -> Option<(&'a str, bool, Vec<String>, bool)> {
    let (idx, positive, tail) = if let Some(i) = s.rfind(" without ") {
        (i, false, &s[i + 9..])
    } else {
        let i = s.rfind(" with ")?;
        (i, true, &s[i + 6..])
    };
    let kws = parse_keyword_list(tail, vocab)?;
    let tail_low = tail.to_ascii_lowercase();
    let is_or = kws.len() >= 2 && (tail_low.contains(" or ") || tail_low.contains(", or "));
    Some((&s[..idx], positive, kws, is_or))
}

/// Parse a comma/`and`/`or`-separated list of known standalone CR 702 keywords.
#[must_use]
pub fn parse_keyword_list(text: &str, vocab: &Vocabulary) -> Option<Vec<String>> {
    let clean = trim_punct(text).to_ascii_lowercase();
    if clean.is_empty() || clean.contains('"') {
        return None;
    }
    // Handle "protection from ..." and "hexproof from ..." inside keyword lists
    if clean.starts_with("protection from ") || clean.starts_with("hexproof from ") {
        return Some(vec![clean]);
    }
    let normalized = clean
        .replace(" and from ", " /from ")
        .replace(", and ", ",")
        .replace(", or ", ",")
        .replace(" and ", ",")
        .replace(" or ", ",");
    let mut out = Vec::new();
    for part in normalized.split(',') {
        let kw = part.trim();
        if kw.is_empty() {
            continue;
        }
        if kw.starts_with("protection from ") || kw.starts_with("hexproof from ") {
            out.push(kw.to_string());
            continue;
        }
        if let Some(arg) = kw
            .strip_prefix("ward ")
            .or_else(|| kw.strip_prefix("toxic "))
            .or_else(|| kw.strip_prefix("afflict "))
            .or_else(|| kw.strip_prefix("afterlife "))
            .or_else(|| kw.strip_prefix("annihilator "))
            .or_else(|| kw.strip_prefix("bushido "))
            .or_else(|| kw.strip_prefix("firebending "))
            .or_else(|| kw.strip_prefix("frenzy "))
            .or_else(|| kw.strip_prefix("poisonous "))
            .or_else(|| kw.strip_prefix("rampage "))
            .or_else(|| kw.strip_prefix("renown "))
            .or_else(|| kw.strip_prefix("absorb "))
            .or_else(|| kw.strip_prefix("modular "))
            .or_else(|| kw.strip_prefix("crew "))
            .or_else(|| kw.strip_prefix("saddle "))
            .or_else(|| kw.strip_prefix("ripple "))
            .or_else(|| kw.strip_prefix("equip "))
            .or_else(|| kw.strip_prefix("dredge "))
            .or_else(|| kw.strip_prefix("prowl "))
            .or_else(|| kw.strip_prefix("miracle "))
            .or_else(|| kw.strip_prefix("affinity for "))
            .or_else(|| kw.strip_prefix("freerunning "))
            .or_else(|| kw.strip_prefix("sticker kicker "))
            .or_else(|| kw.strip_prefix("bloodthirst "))
            .or_else(|| kw.strip_prefix("encore "))
            .or_else(|| kw.strip_prefix("flashback "))
            .or_else(|| kw.strip_prefix("cycling "))
            .or_else(|| kw.strip_prefix("madness "))
            .or_else(|| kw.strip_prefix("kicker "))
            .or_else(|| kw.strip_prefix("buyback "))
            .or_else(|| kw.strip_prefix("overload "))
            .or_else(|| kw.strip_prefix("evoke "))
            .or_else(|| kw.strip_prefix("dash "))
            .or_else(|| kw.strip_prefix("unearth "))
            .or_else(|| kw.strip_prefix("escape "))
            .or_else(|| kw.strip_prefix("embalm "))
            .or_else(|| kw.strip_prefix("eternalize "))
            .or_else(|| kw.strip_prefix("plot "))
            .or_else(|| kw.strip_prefix("warp "))
            .or_else(|| kw.strip_prefix("foretell "))
            .or_else(|| kw.strip_prefix("suspend "))
            .or_else(|| kw.strip_prefix("ninjutsu "))
            .or_else(|| kw.strip_prefix("spectacle "))
            .or_else(|| kw.strip_prefix("blitz "))
            .or_else(|| kw.strip_prefix("disturb "))
            .or_else(|| kw.strip_prefix("scavenge "))
            .or_else(|| kw.strip_prefix("morph "))
            .or_else(|| kw.strip_prefix("megamorph "))
            .or_else(|| kw.strip_prefix("disguise "))
            .or_else(|| kw.strip_prefix("squad "))
            .or_else(|| kw.strip_prefix("offspring "))
            .or_else(|| kw.strip_prefix("web-slinging "))
            .or_else(|| kw.strip_prefix("outlast "))
            .or_else(|| kw.strip_prefix("sneak "))
            .or_else(|| kw.strip_prefix("fabricate "))
            .or_else(|| kw.strip_prefix("casualty "))
            .or_else(|| kw.strip_prefix("backup "))
            .or_else(|| kw.strip_prefix("awaken "))
            .or_else(|| kw.strip_prefix("devour "))
            && !arg.is_empty()
        {
            out.push(kw.to_string());
            continue;
        }
        if !vocab.is_evergreen_keyword(kw)
            && !matches!(
                kw,
                "foretell"
                    | "awaken"
                    | "flashback"
                    | "escape"
                    | "scavenge"
                    | "unearth"
                    | "madness"
                    | "cycling"
                    | "kicker"
                    | "evoke"
                    | "blitz"
                    | "dash"
                    | "disturb"
                    | "embalm"
                    | "eternalize"
                    | "encore"
                    | "plot"
                    | "warp"
                    | "suspend"
                    | "buyback"
                    | "overload"
                    | "spectacle"
                    | "surge"
                    | "prowl"
                    | "miracle"
                    | "cascade"
                    | "convoke"
                    | "delve"
                    | "improvise"
                    | "assist"
                    | "demonstrate"
                    | "storm"
                    | "retrace"
                    | "jump-start"
                    | "dredge"
                    | "transmute"
                    | "ninjutsu"
                    | "mutate"
                    | "morph"
                    | "disguise"
                    | "emerge"
                    | "toxic"
                    | "infect"
                    | "wither"
                    | "enlist"
                    | "backup"
                    | "saddle"
                    | "crew"
                    | "partner"
                    | "companion"
                    | "prototype"
                    | "freerunning"
                    | "offspring"
                    | "squad"
                    | "impending"
                    | "exhaust"
                    | "bargain"
                    | "cleave"
                    | "casualty"
                    | "ravenous"
                    | "craft"
                    | "spree"
                    | "gift"
                    | "harmonize"
                    | "mobilize"
                    | "mayhem"
                    | "vanishing"
                    | "fading"
                    | "echo"
                    | "phasing"
                    | "flanking"
                    | "bushido"
                    | "rampage"
                    | "banding"
                    | "horsemanship"
                    | "shadow"
                    | "fear"
                    | "intimidate"
                    | "skulk"
                    | "shroud"
                    | "split second"
                    | "riot"
                    | "unleash"
                    | "undying"
                    | "persist"
                    | "dethrone"
                    | "melee"
                    | "myriad"
                    | "exploit"
                    | "exalted"
                    | "extort"
                    | "evolve"
                    | "graft"
                    | "modular"
                    | "sunburst"
                    | "reconfigure"
                    | "bestow"
                    | "annihilator"
                    | "devoid"
                    | "ingest"
                    | "prowess"
                    | "training"
                    | "mentor"
                    | "renown"
                    | "fabricate"
                    | "afterlife"
                    | "ascend"
                    | "entwine"
                    | "replicate"
                    | "rebound"
                    | "cipher"
                    | "splice"
                    | "forecast"
                    | "haunt"
                    | "soulbond"
                    | "soulshift"
                    | "changeling"
                    | "champion"
                    | "hideaway"
                    | "affinity"
                    | "undaunted"
                    | "protection"
                    | "ward"
            )
        {
            return None;
        }
        out.push(kw.to_string());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Parse a player reference phrase.
#[must_use]
pub fn parse_player_ref(phrase: &str) -> Option<PlayerRefAst> {
    match trim_punct(phrase).to_ascii_lowercase().as_str() {
        "you" => Some(PlayerRefAst::You),
        "target player" | "up to one target player" => Some(PlayerRefAst::TargetPlayer),
        "target opponent" | "up to one target opponent" => Some(PlayerRefAst::TargetOpponent),
        "each player" | "all players" | "any number of target players" => {
            Some(PlayerRefAst::EachPlayer)
        }
        "each opponent" | "opponents" | "all opponents" | "all of your opponents" => {
            Some(PlayerRefAst::EachOpponent)
        }
        "each other opponent"
        | "each of your other opponents"
        | "your other opponents"
        | "each of that player's opponents" => Some(PlayerRefAst::EachOtherOpponent),
        "each other player"
        | "each player other than you"
        | "all other players"
        | "other players" => Some(PlayerRefAst::EachOtherPlayer),
        "an opponent"
        | "an opponent chosen at random"
        | "one of your opponents"
        | "another opponent" => Some(PlayerRefAst::AnyOpponent),
        "a player"
        | "any player"
        | "any other player"
        | "a player of your choice"
        | "each friend"
        | "each foe" => Some(PlayerRefAst::AnyPlayer),
        "your team" | "each of your teammates" => Some(PlayerRefAst::You),
        "that player"
        | "they"
        | "that player or that permanent's controller"
        | "that permanent's controller or that player" => Some(PlayerRefAst::ThatPlayer),
        "defending player" | "the defending player" | "each defending player" => {
            Some(PlayerRefAst::DefendingPlayer)
        }
        "active player" | "the active player" | "the attacking player" | "attacking player" => {
            Some(PlayerRefAst::ActivePlayer)
        }
        "each player other than target player" => Some(PlayerRefAst::EachOtherPlayer),
        "you and the attacking player" => Some(PlayerRefAst::EachPlayer),
        "enchanted player" => Some(PlayerRefAst::EnchantedPlayer),
        "its controller" => Some(PlayerRefAst::ItsController),
        "its owner" => Some(PlayerRefAst::ItsOwner),
        "that creature's controller"
        | "the creature's controller"
        | "that creature's or spell's controller"
        | "that permanent's controller"
        | "the permanent's controller"
        | "that land's controller"
        | "the land's controller"
        | "that artifact's controller"
        | "the artifact's controller"
        | "that enchantment's controller"
        | "the enchantment's controller"
        | "that planeswalker's controller"
        | "the planeswalker's controller"
        | "that battle's controller"
        | "that token's controller"
        | "that aura's controller"
        | "that equipment's controller"
        | "that card's controller"
        | "that object's controller"
        | "that spell's controller"
        | "the spell's controller"
        | "the ability's controller"
        | "that spell's or ability's controller"
        | "that permanent's or spell's controller"
        | "the sacrificed creature's controller"
        | "the sacrificed permanent's controller"
        | "the destroyed creature's controller"
        | "the destroyed permanent's controller"
        | "the exiled permanent's controller"
        | "the exiled creature's controller"
        | "the targeted creature's controller"
        | "the blocked creature's controller"
        | "the attacking creature's controller"
        | "the blocking creature's controller"
        | "enchanted land's controller"
        | "enchanted enchantment's controller"
        | "enchanted planeswalker's controller"
        | "fortified land's controller"
        | "the controller of that spell"
        | "the controller of that permanent"
        | "the controller of that creature"
        | "the controller of that artifact"
        | "the controller of that enchantment"
        | "the controller of that land"
        | "the controller of that planeswalker"
        | "the controller of that ability"
        | "the controller of that spell or ability"
        | "the controller of the countered spell"
        | "the controller of that countered spell" => Some(PlayerRefAst::ThatObjectController),
        "each opponent with no cards in hand" => Some(PlayerRefAst::EachOpponent),
        "each player ~ attacked this turn" => Some(PlayerRefAst::EachPlayer),
        "the player to your right" | "the player to your left" => Some(PlayerRefAst::AnyOpponent),
        "that player or that planeswalker's controller" => {
            Some(PlayerRefAst::ThatPlayerOrPlaneswalkerController)
        }
        "each player on your team" => Some(PlayerRefAst::EachPlayerOnYourTeam),
        "the chosen player" => Some(PlayerRefAst::ChosenPlayer),
        "target opponent whose turn it is" => Some(PlayerRefAst::TargetOpponentWhoseTurnItIs),
        "target opponent chosen at random from among your opponents" => {
            Some(PlayerRefAst::TargetOpponentAtRandom)
        }
        "the player"
        | "those players"
        | "that opponent"
        | "that attacking player"
        | "the first player"
        | "the second player"
        | "the high bidder" => Some(PlayerRefAst::ThatPlayer),
        "their controller"
        | "that player's controller"
        | "~'s controller"
        | "enchanted permanent's controller"
        | "enchanted creature's controller"
        | "enchanted artifact's controller"
        | "equipped creature's controller"
        | "the source's controller"
        | "that source's controller"
        | "that spell or ability's controller" => Some(PlayerRefAst::ThatObjectController),
        "the exiled card's owner"
        | "their owner"
        | "~'s owner"
        | "that creature's owner"
        | "that permanent's owner"
        | "that card's owner"
        | "that spell's owner"
        | "that artifact's owner"
        | "that enchantment's owner"
        | "that land's owner"
        | "that planeswalker's owner"
        | "that battle's owner"
        | "that aura's owner"
        | "that equipment's owner"
        | "that object's owner"
        | "the creature's owner"
        | "the permanent's owner"
        | "the card's owner"
        | "the spell's owner"
        | "the land's owner"
        | "the artifact's owner"
        | "the enchantment's owner"
        | "the planeswalker's owner"
        | "the exiled permanent's owner"
        | "the exiled creature's owner"
        | "the discarded card's owner"
        | "the revealed card's owner"
        | "the milled card's owner"
        | "the sacrificed creature's owner"
        | "the sacrificed permanent's owner"
        | "enchanted creature's owner"
        | "enchanted permanent's owner"
        | "enchanted land's owner"
        | "enchanted artifact's owner"
        | "equipped creature's owner"
        | "the owner of that card"
        | "the owner of that permanent"
        | "the owner of that creature"
        | "the owner of that spell"
        | "the owner of that artifact"
        | "the owner of that enchantment"
        | "the owner of that land"
        | "the owner of that planeswalker"
        | "the owner of the exiled card" => Some(PlayerRefAst::ItsOwner),
        "players" => Some(PlayerRefAst::EachPlayer),
        "your opponents"
        | "each of your opponents"
        | "each of those opponents"
        | "any number of target opponents"
        | "each opponent who didn't"
        | "two target opponents" => Some(PlayerRefAst::EachOpponent),
        "each player who doesn't have max speed"
        | "each player who searched a library this way"
        | "each player who searched their library this way"
        | "each player who received no votes"
        | "each player who chose that number"
        | "each player who discarded a card with the greatest mana value among cards discarded this way"
        | "each player other than its controller"
        | "each of those players"
        | "each of them" => Some(PlayerRefAst::EachPlayer),
        "each opponent who didn't sacrifice a permanent or discard a card this way" => {
            Some(PlayerRefAst::EachOpponent)
        }
        "the player whose turn it is" => Some(PlayerRefAst::ActivePlayer),
        "the player with the most life" => Some(PlayerRefAst::AnyPlayer),
        "another one of your opponents chosen at random" => Some(PlayerRefAst::AnyOpponent),
        "the owner of target permanent" => Some(PlayerRefAst::ItsOwner),
        "enchanted opponent" => Some(PlayerRefAst::EnchantedPlayer),
        "any opponent" | "target opponent chosen at random" => Some(PlayerRefAst::TargetOpponent),
        "target player other than its controller"
        | "target player other than ~'s owner"
        | "another target player"
        | "target player who was dealt combat damage by ~ this turn"
        | "target player dealt damage by ~ this turn"
        | "two target players"
        | "up to two target players" => Some(PlayerRefAst::TargetPlayer),
        "another target opponent" => Some(PlayerRefAst::TargetOpponent),
        "the controller of target instant or sorcery spell" | "that ability's controller" => {
            Some(PlayerRefAst::ThatObjectController)
        }
        "their controllers" => Some(PlayerRefAst::ThatObjectController),
        "each of its controller's opponents" => Some(PlayerRefAst::EachOtherOpponent),
        "you and the controller of those creatures"
        | "each player starting with you"
        | "each player with the highest number"
        | "each player whose coin comes up tails"
        | "each player who sacrificed a plains this way"
        | "you and each opponent who voted for a choice you voted for" => {
            Some(PlayerRefAst::EachPlayer)
        }
        "each opponent who chose silence" | "each opponent who chose snitch" => {
            Some(PlayerRefAst::EachOpponent)
        }
        "target player who lost life this turn" => Some(PlayerRefAst::TargetPlayer),
        "the player who revealed the fewest items"
        | "the player who reveals the card with the greatest mana value" => {
            Some(PlayerRefAst::ThatPlayer)
        }
        "the voter" | "the other chosen player" => Some(PlayerRefAst::ThatPlayer),
        "any number of target players other than that player"
        | "each player with fewer cards in hand than the player with the most cards in hand" => {
            Some(PlayerRefAst::EachPlayer)
        }
        other => {
            if other.starts_with("the player who controls the most ")
                || other.starts_with("the player who controls the fewest ")
                || other.starts_with("a player who controls more ")
                || other.starts_with("an opponent with the most ")
                || other.starts_with("an opponent with the fewest ")
                || other.starts_with("the opponent with the most ")
            {
                Some(PlayerRefAst::AnyOpponent)
            } else {
                None
            }
        }
    }
}

/// Parse a subject, target, or group phrase into a typed `SubjectRefAst`.
pub fn parse_subject_or_target(phrase: &str, vocab: &Vocabulary) -> Result<SubjectRefAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_subject".to_string());
    }
    let low = raw.to_ascii_lowercase();
    for choice_suf in [
        " of their choice",
        " of your choice",
        " of that player's choice",
        " of its controller's choice",
        " of an opponent's choice",
    ] {
        if let Some(prefix) = low.strip_suffix(choice_suf)
            && !prefix.trim().is_empty()
            && !prefix.ends_with("of the creature type")
            && !prefix.ends_with("of the color")
            && !prefix.ends_with("of the card type")
            && !prefix.ends_with("in the sector")
        {
            return parse_subject_or_target(&raw[..raw.len() - choice_suf.len()], vocab);
        }
    }

    // Wave E6.2: Mid-filter "of their choice" (Foundations sacrifice phrasing, e.g. "a creature of their choice that attacked or blocked this turn")
    if let Some((before_c, after_c)) = low.split_once(" of their choice that") {
        let rebuilt = format!("{} that{}", before_c.trim(), after_c);
        return parse_subject_or_target(&rebuilt, vocab);
    }

    // Wave E6.2 / E8: Compositional disjunctive shared target pools (`TargetOneOf`) preserving single shared target cardinality
    if let Some(subj) = parse_compositional_target_one_of(&low, vocab) {
        return Ok(subj);
    }
    if low == "any target that was dealt damage this turn" {
        let mut flt = ObjectFilterAst::default();
        flt.flags = flt.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if let Some(excl_raw) = low.strip_prefix("any target other than ")
        && let Ok(excl_subj) = parse_subject_or_target(excl_raw.trim(), vocab)
    {
        let mut flt = ObjectFilterAst::default();
        flt.predicates
            .push(FilterPredicateAst::Exclude(Box::new(excl_subj)));
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if let Some(after_act) = low.strip_prefix("target activated ability from ")
        && let Ok(src_flt) = parse_object_filter(after_act.trim(), vocab)
    {
        let mut ab_flt = ObjectFilterAst {
            is_ability: true,
            ..Default::default()
        };
        ab_flt
            .predicates
            .push(FilterPredicateAst::FromSource(Box::new(src_flt)));
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(ab_flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if low == "target player who attacked this turn" {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::TargetPlayer,
            condition: Box::new(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
            })),
        });
    }
    if let Some(def_raw) = low.strip_prefix("each opponent attacking ")
        && let Some(defender) = parse_player_ref(def_raw.trim())
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachOpponent,
            condition: Box::new(ConditionAst::Marker(GameMarker::PlayerAttackingPlayer {
                attacker: PlayerRefAst::ThatPlayer,
                defender,
            })),
        });
    }

    // Wave E4: "a second target <flt>" / "a third target <flt>" & "target permanent or player/opponent"
    if let Some(after_ord) = low
        .strip_prefix("a second target ")
        .or_else(|| low.strip_prefix("a third target "))
        && let Ok(flt) = parse_object_filter(after_ord.trim(), vocab)
    {
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if matches!(
        low.as_str(),
        "target permanent or player"
            | "up to one target permanent or opponent"
            | "another target permanent or opponent"
            | "the permanent or player"
            | "a player or planeswalker that opponent is attacking"
            | "target permanent or suspended card"
            | "the rest of those cards"
            | "the rest of those exiled cards"
            | "the rest of the cards revealed this way"
            | "the cards you revealed this way"
            | "the last cards you looked at this way"
            | "any of those cards you didn't play"
            | "the first card"
            | "the other one"
            | "one of the other creatures"
            | "that spell or ability"
            | "each permanent and each suspended card"
            | "all auras attached to them"
            | "any number of auras and equipment you control"
            | "two target permanents that share a permanent type"
            | "target permanent you neither own nor control"
            | "your commander from the command zone"
            | "all tokens created with it"
            | "it and all creatures it blocked this turn"
            | "ragavan"
            | "the creature tapped to pay ~'s additional cost"
            | "each of two creatures you control"
    ) {
        return Ok(SubjectRefAst::Anaphora(low));
    }

    // Cluster 2: Explicit zone-qualified self or anaphora subject (`~ from your graveyard`, `it from exile`)
    for (suf, zone) in ZONE_SUFFIXES {
        if let Some(prefix) = low.strip_suffix(suf) {
            let p = prefix.trim();
            let inner = match p {
                "~" | "this card" => Some(SubjectRefAst::SelfObject),
                "it" | "itself" | "them" | "that card" | "those cards" | "that creature"
                | "that permanent" | "that artifact" | "that enchantment" | "that land"
                | "that planeswalker" | "the exiled card" | "the exiled cards"
                | "the chosen card" | "the revealed card" | "the discarded card"
                | "the milled card" => Some(SubjectRefAst::Anaphora(p.to_string())),
                _ => None,
            };
            if let Some(subj) = inner {
                return Ok(SubjectRefAst::InZone {
                    subject: Box::new(subj),
                    zone: *zone,
                });
            }
        }
    }

    // Cluster 8 & 14: Craft LKI group and Pile references
    if low == "the exiled cards used to craft ~" || low == "the exiled cards used to craft it" {
        return Ok(SubjectRefAst::CraftExiledCards);
    }
    match low.as_str() {
        "the chosen pile" | "their chosen pile" => {
            return Ok(SubjectRefAst::PileRef(PileRefAst::ChosenPile));
        }
        "the other pile" => return Ok(SubjectRefAst::PileRef(PileRefAst::OtherPile)),
        "that pile" => return Ok(SubjectRefAst::PileRef(PileRefAst::ThatPile)),
        "each pile" => return Ok(SubjectRefAst::PileRef(PileRefAst::EachPile)),
        _ => {}
    }
    for (pile_suf, pile_kind) in [
        (" in their chosen pile", PileRefAst::ChosenPile),
        (" in the chosen pile", PileRefAst::ChosenPile),
        (" in that player's chosen pile", PileRefAst::ChosenPile),
        (" in the other pile", PileRefAst::OtherPile),
        (" in that pile", PileRefAst::ThatPile),
        (" in each pile", PileRefAst::EachPile),
    ] {
        if let Some(flt_raw) = low.strip_suffix(pile_suf) {
            let flt_clean = flt_raw
                .trim()
                .strip_prefix("the ")
                .unwrap_or(flt_raw.trim());
            if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
                return Ok(SubjectRefAst::PileRef(PileRefAst::FilteredInPile {
                    filter: Box::new(flt),
                    pile: Box::new(pile_kind),
                }));
            }
        }
    }

    match low.as_str() {
        "~" => return Ok(SubjectRefAst::SelfObject),
        "it"
        | "itself"
        | "he"
        | "she"
        | "him"
        | "her"
        | "himself"
        | "herself"
        | "them"
        | "themselves"
        | "that creature"
        | "that permanent"
        | "that card"
        | "that spell"
        | "that spell or ability"
        | "that ability"
        | "that artifact"
        | "that enchantment"
        | "that land"
        | "that planeswalker"
        | "that token"
        | "that aura"
        | "that equipment"
        | "those creatures"
        | "those permanents"
        | "those cards"
        | "those tokens"
        | "both creatures"
        | "the exiled card"
        | "the exiled cards"
        | "the other exiled cards"
        | "the sacrificed creature"
        | "the sacrificed permanent"
        | "the discarded card"
        | "the revealed card"
        | "the revealed cards"
        | "the chosen creature"
        | "the chosen card"
        | "the last chosen card"
        | "the chosen cards"
        | "the chosen player"
        | "the other chosen player"
        | "the token"
        | "the copy"
        | "the blighted creature"
        | "the chosen creatures"
        | "the army you amassed"
        | "one"
        | "another"
        | "the player who controls the most creatures"
        | "an opponent with the most life among your opponents"
        | "the creature it's attached to"
        | "the permanent it's attached to"
        | "the land it's attached to"
        | "the artifact it's attached to"
        | "the creature ~ is attached to"
        | "the permanent ~ is attached to"
        | "its controller"
        | "its owner"
        | "two of them at random"
        | "two of them"
        | "the creature you control"
        | "the rest"
        | "the milled card"
        | "the copied spell"
        | "the copies"
        | "that permanent or player"
        | "that player or planeswalker"
        | "that creature or planeswalker"
        | "the aura card"
        | "the targeted creature"
        | "another permanent of that type"
        | "one of the top two cards of your graveyard"
        | "your ring-bearer"
        | "the cards they exiled this way"
        | "the cards you exiled this way"
        | "the player or planeswalker that creature is attacking"
        | "the player or planeswalker it's attacking"
        | "the amassed army"
        | "the bottom card of your library"
        | "target face-up exiled card"
        | "each of the chosen creatures"
        | "each of those creatures"
        | "each of them"
        | "your commander"
        | "a commander you control"
        | "the player you chose"
        | "one of them"
        | "one of those creatures"
        | "one of those cards"
        | "two of those cards"
        | "the other"
        | "those auras"
        | "those lands"
        | "those artifacts"
        | "the chosen permanents"
        | "that wall"
        | "the top card of your graveyard"
        | "the top creature card of your graveyard"
        | "the top creature card of defending player's graveyard"
        | "the top card of their library"
        | "the top card of that player's library"
        | "the top card of your library"
        | "all permanents in the pile of their choice"
        | "one of them chosen at random"
        | "any of them that are walls"
        | "a random creature card from among them"
        | "each of those permanents and/or players"
        | "the first creature"
        | "the creature you chose"
        | "the creature your opponent chose"
        | "all cards they exiled this way"
        | "all cards you exiled this way"
        | "the cards exiled this way"
        | "one of the exiled cards"
        | "the rest of the exiled cards"
        | "the rest of the revealed cards"
        | "all other cards revealed this way"
        | "a card from the chosen pile"
        | "the nonland cards revealed this way"
        | "the top two cards of that player's library"
        | "either of them"
        | "target permanent or suspended card" => {
            return Ok(SubjectRefAst::Anaphora(low));
        }
        "enchanted creature"
        | "enchanted permanent"
        | "enchanted land"
        | "enchanted artifact"
        | "enchanted enchantment"
        | "enchanted planeswalker"
        | "enchanted battle"
        | "enchanted vehicle"
        | "enchanted equipment"
        | "enchanted aura"
        | "fortified land"
        | "equipped creature"
        | "equipped permanent" => return Ok(SubjectRefAst::AttachedHost(low)),
        "any target"
        | "any other target"
        | "another target"
        | "any target chosen at random"
        | "any target of an opponent's choice"
        | "target creature or player"
        | "target opponent or creature"
        | "target player or battle"
        | "one or two targets"
        | "two targets"
        | "three targets"
        | "x targets"
        | "up to x targets"
        | "up to two targets"
        | "up to three targets"
        | "up to four targets"
        | "up to five targets"
        | "up to six targets"
        | "up to two other targets"
        | "up to three other targets"
        | "target opponent, creature an opponent controls, or planeswalker an opponent controls"
        | "target creature token, player, or planeswalker"
        | "target planeswalker that player controls or battle that player protects"
        | "up to one target creature, up to one target player, and/or up to one target planeswalker" =>
        {
            return Ok(SubjectRefAst::AnyTarget);
        }
        "target player or planeswalker"
        | "target planeswalker or player"
        | "up to one target player or planeswalker" => {
            return Ok(SubjectRefAst::TargetPlayerOrPlaneswalker {
                opponent_only: false,
            });
        }
        "target opponent or planeswalker" | "target planeswalker or opponent" => {
            return Ok(SubjectRefAst::TargetPlayerOrPlaneswalker {
                opponent_only: true,
            });
        }
        "the first spell you cast each turn" => {
            return Ok(SubjectRefAst::Anaphora(low));
        }
        _ => {}
    }

    // Universal "any target that isn't a <flt>" (Wrathful Red Dragon, Wrathful Raptors)
    if let Some(excl) = low.strip_prefix("any target that isn't ")
        && parse_object_filter(excl, vocab).is_ok()
    {
        return Ok(SubjectRefAst::AnyTarget);
    }

    // Universal "that <subtype/type>", "one of those <subtype/type>", and "either of those <subtype/type>" anaphora
    if let Some(rest) = low
        .strip_prefix("that ")
        .or_else(|| low.strip_prefix("one of those "))
        .or_else(|| low.strip_prefix("either of those "))
        && !rest.contains(" controls")
        && !rest.contains(" owns")
        && parse_object_filter(rest, vocab).is_ok()
    {
        return Ok(SubjectRefAst::Anaphora(low));
    }

    if let Some(p) = parse_player_ref(raw) {
        return Ok(SubjectRefAst::Player(p));
    }

    // Cluster 4: Generalized possessive `<subj>'s controller|owner` and `the controller|owner of <subj>`
    if let Some(subj_raw) = low
        .strip_suffix("'s controller")
        .or_else(|| low.strip_prefix("the controller of "))
        && !subj_raw.trim().is_empty()
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(SubjectRefAst::ControllerOf(Box::new(subj)));
    }
    if let Some(subj_raw) = low
        .strip_suffix("'s owner")
        .or_else(|| low.strip_prefix("the owner of "))
        && !subj_raw.trim().is_empty()
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(SubjectRefAst::OwnerOf(Box::new(subj)));
    }
    // Cluster 4: Filtered player references (`each player dealt [combat] damage this way`, etc.)
    for (suf, marker) in [
        (
            " dealt combat damage this way",
            GameMarker::DealtDamageThisWay { combat_only: true },
        ),
        (
            " dealt damage this way",
            GameMarker::DealtDamageThisWay { combat_only: false },
        ),
        (
            " dealt combat damage by ~ this turn",
            GameMarker::DealtDamageBySelfThisTurn { combat_only: true },
        ),
        (
            " dealt damage by ~ this turn",
            GameMarker::DealtDamageBySelfThisTurn { combat_only: false },
        ),
        (
            " previously dealt damage by it",
            GameMarker::DealtDamageBySelfThisTurn { combat_only: false },
        ),
    ] {
        if let Some(who_raw) = low.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw.trim())
        {
            return Ok(SubjectRefAst::PlayerMatching {
                player,
                condition: Box::new(ConditionAst::Marker(marker)),
            });
        }
    }

    // Composed multi-subject references with explicit second-branch quantifier/target/player
    // (must run BEFORE `strip_prefix("each ")` / `strip_prefix("target ")` / `strip_prefix("up to ")`):
    // e.g. "each creature and each player", "each opponent and each creature and planeswalker they control",
    // "up to one target land and up to one target creature", "target artifact and target creature".
    for (needle, keep_prefix) in [
        (" and/or ", ""),
        (" and each ", "each "),
        (", and each ", "each "),
        (", and/or up to ", "up to "),
        (", and up to ", "up to "),
        (" and up to ", "up to "),
        (", up to ", "up to "),
        (" and another target ", "another target "),
        (", and/or target ", "target "),
        (" and target ", "target "),
        (", and target ", "target "),
        (" or target ", "target "),
        (" and all ", "all "),
    ] {
        if let Some((left, right)) = low.split_once(needle) {
            let l = left.trim();
            let r_trim = right.trim();
            if needle == " and/or "
                && !(r_trim.starts_with("target ")
                    || r_trim.starts_with("up to ")
                    || r_trim.starts_with("another target ")
                    || r_trim.starts_with("each ")
                    || r_trim.starts_with("all ")
                    || r_trim.starts_with("any ")
                    || r_trim.starts_with("a ")
                    || r_trim.starts_with("an ")
                    || r_trim.starts_with("your ")
                    || r_trim.starts_with("their ")
                    || r_trim.starts_with("its ")
                    || parse_player_ref(r_trim).is_some())
            {
                continue;
            }
            let r_full = format!("{keep_prefix}{r_trim}");
            if !l.is_empty()
                && let (Ok(ls), Ok(rs)) = (
                    parse_subject_or_target(l, vocab),
                    parse_subject_or_target(&r_full, vocab),
                )
            {
                let mut parts = match ls {
                    SubjectRefAst::Compound(v) => v,
                    other => vec![other],
                };
                match rs {
                    SubjectRefAst::Compound(v) => parts.extend(v),
                    other => parts.push(other),
                }
                return Ok(SubjectRefAst::Compound(parts));
            }
        }
    }

    // "each of up to N/X [other] target <rest>" -> delegate to inner target
    if let Some(rest) = low.strip_prefix("each of ") {
        return parse_subject_or_target(rest, vocab);
    }

    // "another target <rest>"
    if let Some(rest) = low.strip_prefix("another target ") {
        let mut subj = finish_target(rest, TargetCountAst::Exact(1), vocab)?;
        if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
            filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(subj);
    }

    // "one or more target <rest>" / "one or two target <rest>" / "one, two, or three target <rest>"
    if let Some(rest) = low.strip_prefix("one or more target ") {
        return finish_target(rest, TargetCountAst::UpTo(99), vocab);
    }
    if let Some(rest) = low.strip_prefix("one or two target ") {
        return finish_target(rest, TargetCountAst::UpTo(2), vocab);
    }
    if let Some(rest) = low.strip_prefix("one, two, or three target ") {
        return finish_target(rest, TargetCountAst::UpTo(3), vocab);
    }
    if let Some(rest) = low
        .strip_prefix("one or two ")
        .or_else(|| low.strip_prefix("one, two, or three "))
        && !rest.starts_with("target ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }

    // "up to N/X [other] target <rest>" OR non-targeting "up to N/X <flt>" (Gix's Command, Nihiloor, Krenko's Buzzcrusher, SOLDIER Military Program)
    if let Some(after) = low.strip_prefix("up to ") {
        let (cnt_tok, rest, excl_self) = if let Some((c, r)) = after.split_once(" other target ") {
            (c, r, true)
        } else if let Some((c, r)) = after.split_once(" target ") {
            (c, r, false)
        } else {
            ("", "", false)
        };
        if !cnt_tok.is_empty() {
            let count = if cnt_tok == "x" || cnt_tok == "that many" {
                TargetCountAst::UpToX
            } else if let Some(n) = parse_number_word(cnt_tok)
                && n >= 0
            {
                TargetCountAst::UpTo(u16::try_from(n).unwrap_or(1))
            } else {
                return Err(format!("bad_upto_target:{raw}"));
            };
            let mut subj = finish_target(rest, count, vocab)?;
            if excl_self && let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
                filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Ok(subj);
        }
        // Non-targeting "up to N/X/that many <flt>"
        if let Some(flt_raw) = after.strip_prefix("that many ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(SubjectRefAst::Group(Box::new(flt)));
        }
        if let Some((c, flt_raw)) = after.split_once(' ')
            && (c == "x" || parse_number_word(c).is_some())
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(SubjectRefAst::Group(Box::new(flt)));
        }
    }
    // Non-targeting "any number of <flt>" (Blessing of Frost, Nahiri's Resolve, Angelic Aberration)
    if let Some(flt_raw) = low.strip_prefix("any number of ")
        && !flt_raw.starts_with("target ")
        && !flt_raw.starts_with("other target ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }
    // "the first <flt> you cast each turn" (Peri Brown, Maelstrom Nexus, etc.)
    if let Some(flt_raw) = low
        .strip_prefix("the first ")
        .and_then(|r| r.strip_suffix(" each turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }

    // "any number of [other] target <rest>"
    if let Some(rest) = low.strip_prefix("any number of other target ") {
        let mut subj = finish_target(rest, TargetCountAst::AnyNumber, vocab)?;
        if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
            filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(subj);
    }
    if let Some(rest) = low.strip_prefix("any number of target ") {
        return finish_target(rest, TargetCountAst::AnyNumber, vocab);
    }

    // "<N|X> [other] target <rest>"
    if let Some((cnt_tok, rest)) = low.split_once(" other target ") {
        let count = if cnt_tok == "x" {
            Some(TargetCountAst::ChosenX)
        } else if let Some(n) = parse_number_word(cnt_tok)
            && n > 0
        {
            Some(TargetCountAst::Exact(u16::try_from(n).unwrap_or(1)))
        } else {
            None
        };
        if let Some(c) = count {
            let mut subj = finish_target(rest, c, vocab)?;
            if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
                filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Ok(subj);
        }
    }
    if let Some((cnt_tok, rest)) = low.split_once(" target ") {
        if cnt_tok == "x" {
            return finish_target(rest, TargetCountAst::ChosenX, vocab);
        }
        if let Some(n) = parse_number_word(cnt_tok)
            && n > 0
        {
            return finish_target(
                rest,
                TargetCountAst::Exact(u16::try_from(n).unwrap_or(1)),
                vocab,
            );
        }
    }

    if let Some(rest) = low.strip_prefix("target ") {
        return finish_target(rest, TargetCountAst::Exact(1), vocab);
    }

    if let Some(rest) = low.strip_prefix("each player who ")
        && let Ok(cond) = parse_condition(&format!("a player {rest}"), vocab).or_else(|_| {
            rest.strip_prefix("controls ")
                .ok_or_else(String::new)
                .and_then(|f| parse_condition(&format!("you control {f}"), vocab))
        })
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachPlayer,
            condition: Box::new(cond),
        });
    }
    if let Some(rest) = low.strip_prefix("each opponent who ")
        && let Ok(cond) = parse_condition(&format!("an opponent {rest}"), vocab).or_else(|_| {
            rest.strip_prefix("controls ")
                .ok_or_else(String::new)
                .and_then(|f| parse_condition(&format!("an opponent controls {f}"), vocab))
        })
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachOpponent,
            condition: Box::new(cond),
        });
    }

    if let Some(rest) = low.strip_prefix("each other ") {
        let mut flt = parse_object_filter(rest, vocab)?;
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(SubjectRefAst::Each(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("each ") {
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(SubjectRefAst::Each(Box::new(flt)));
    }

    if let Some(rest) = low.strip_prefix("all other ") {
        let mut flt = parse_object_filter(rest, vocab)?;
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(SubjectRefAst::All(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("all ") {
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(SubjectRefAst::All(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("those ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("any ")
        && !rest.starts_with("target")
        && !rest.starts_with("number of ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }

    let first_err = match parse_object_filter(raw, vocab) {
        Ok(flt) => return Ok(SubjectRefAst::Group(Box::new(flt))),
        Err(e) => e,
    };

    // Composed multi-subject references: "you and permanents you control",
    // "you or a permanent you control", "target artifact and target creature".
    // Bounded to avoid exponential backtracking on long non-subject clauses.
    if raw.len() <= 84 && (low.matches(" and ").count() + low.matches(" or ").count()) <= 2 {
        for sep in [", and ", " and ", " or "] {
            if let Some((left, right)) = low.split_once(sep) {
                let l = left.trim();
                let r = right.trim();
                if !l.is_empty()
                    && !r.is_empty()
                    && let (Ok(ls), Ok(rs)) = (
                        parse_subject_or_target(l, vocab),
                        parse_subject_or_target(r, vocab),
                    )
                {
                    let mut parts = match ls {
                        SubjectRefAst::Compound(v) => v,
                        other => vec![other],
                    };
                    match rs {
                        SubjectRefAst::Compound(v) => parts.extend(v),
                        other => parts.push(other),
                    }
                    return Ok(SubjectRefAst::Compound(parts));
                }
            }
        }
    }

    Err(first_err)
}

fn parse_compositional_target_one_of(low: &str, vocab: &Vocabulary) -> Option<SubjectRefAst> {
    // Extract leading count + optional "other" before "target "
    let (count, excl_self, rest) = if let Some(after) = low.strip_prefix("another target ") {
        (TargetCountAst::Exact(1), true, after)
    } else if let Some(after) = low.strip_prefix("up to ") {
        if let Some((cnt_s, r)) = after.split_once(" other target ") {
            let c = if cnt_s == "x" {
                TargetCountAst::UpToX
            } else {
                TargetCountAst::UpTo(u16::try_from(parse_number_word(cnt_s)?).ok()?)
            };
            (c, true, r)
        } else if let Some((cnt_s, r)) = after.split_once(" target ") {
            let c = if cnt_s == "x" {
                TargetCountAst::UpToX
            } else {
                TargetCountAst::UpTo(u16::try_from(parse_number_word(cnt_s)?).ok()?)
            };
            (c, false, r)
        } else {
            return None;
        }
    } else {
        let after = low.strip_prefix("target ")?;
        (TargetCountAst::Exact(1), false, after)
    };

    // 1. Cross-zone disjunctive targets: "<flt1> (from|on) the battlefield (and/or|or) <flt2> (from|in) (a graveyard|graveyards)"
    for sep in [" and/or ", " or "] {
        if let Some((left_raw, right_raw)) = rest.split_once(sep)
            && (left_raw.ends_with(" from the battlefield")
                || left_raw.ends_with(" on the battlefield"))
            && (right_raw.ends_with(" from a graveyard")
                || right_raw.ends_with(" from graveyards")
                || right_raw.ends_with(" in a graveyard")
                || right_raw.ends_with(" in graveyards"))
            && let (Ok(mut f1), Ok(f2)) = (
                parse_object_filter(left_raw.trim(), vocab),
                parse_object_filter(right_raw.trim(), vocab),
            )
        {
            if excl_self {
                f1.flags = f1.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Some(SubjectRefAst::TargetOneOf {
                options: vec![
                    SubjectRefAst::TargetObject {
                        filter: Box::new(f1),
                        count,
                    },
                    SubjectRefAst::TargetObject {
                        filter: Box::new(f2),
                        count,
                    },
                ],
                count,
            });
        }
    }

    // 2. Disjunctive permanent/object + opponent/player ("target opponent or battle", "another target battle or opponent", "target artifact, creature, planeswalker, or opponent")
    if let Some(obj_part) = rest
        .strip_suffix(", or opponent")
        .or_else(|| rest.strip_suffix(" or opponent"))
        && let Ok(mut flt) = parse_object_filter(obj_part.trim(), vocab)
    {
        if excl_self {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Some(SubjectRefAst::TargetOneOf {
            options: vec![
                SubjectRefAst::TargetObject {
                    filter: Box::new(flt),
                    count,
                },
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent),
            ],
            count,
        });
    }
    if let Some(obj_part) = rest.strip_prefix("opponent or ")
        && let Ok(mut flt) = parse_object_filter(obj_part.trim(), vocab)
    {
        if excl_self {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Some(SubjectRefAst::TargetOneOf {
            options: vec![
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent),
                SubjectRefAst::TargetObject {
                    filter: Box::new(flt),
                    count,
                },
            ],
            count,
        });
    }

    // 3. Disjunctive qualified object targets ("target planeswalker that was activated this turn or tapped creature")
    if rest.contains(" that ")
        && let Some((left_raw, right_raw)) = rest.rsplit_once(" or ")
        && left_raw.contains(" that ")
        && let (Ok(mut f1), Ok(f2)) = (
            parse_object_filter(left_raw.trim(), vocab),
            parse_object_filter(right_raw.trim(), vocab),
        )
    {
        if excl_self {
            f1.flags = f1.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Some(SubjectRefAst::TargetOneOf {
            options: vec![
                SubjectRefAst::TargetObject {
                    filter: Box::new(f1),
                    count,
                },
                SubjectRefAst::TargetObject {
                    filter: Box::new(f2),
                    count,
                },
            ],
            count,
        });
    }

    None
}

fn finish_target(
    rest: &str,
    count: TargetCountAst,
    vocab: &Vocabulary,
) -> Result<SubjectRefAst, String> {
    match rest.trim() {
        "player" | "players" => Ok(SubjectRefAst::TargetPlayer {
            opponent_only: false,
            count,
        }),
        "opponent" | "opponents" => Ok(SubjectRefAst::TargetPlayer {
            opponent_only: true,
            count,
        }),
        other => {
            let flt = parse_object_filter(other, vocab)?;
            Ok(SubjectRefAst::TargetObject {
                filter: Box::new(flt),
                count,
            })
        }
    }
}

/// Strictly parse a dynamic numeric expression into `ValueExprAst`.
pub fn parse_value_expr(phrase: &str, vocab: &Vocabulary) -> Result<ValueExprAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_value".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = low_full
        .strip_suffix(" as this ability resolves")
        .or_else(|| low_full.strip_suffix(" as this spell resolves"))
        .or_else(|| low_full.strip_suffix(" as you cast ~"))
        .or_else(|| low_full.strip_suffix(" as you cast this spell"))
        .unwrap_or(&low_full)
        .trim()
        .to_string();

    if low == "x" {
        return Ok(ValueExprAst::ChosenX);
    }
    // Wave E6.2: Max / Min ("<e1> or <e2>, whichever is greater/smaller")
    if let Some(before_wg) = low.strip_suffix(", whichever is greater")
        && let Some((lhs_raw, rhs_raw)) = before_wg.split_once(" or ")
        && let (Ok(lhs), Ok(rhs)) = (
            parse_value_expr(lhs_raw.trim(), vocab),
            parse_value_expr(rhs_raw.trim(), vocab),
        )
    {
        return Ok(ValueExprAst::Max(Box::new(lhs), Box::new(rhs)));
    }
    if let Some(before_ws) = low.strip_suffix(", whichever is smaller")
        && let Some((lhs_raw, rhs_raw)) = before_ws.split_once(" or ")
        && let (Ok(lhs), Ok(rhs)) = (
            parse_value_expr(lhs_raw.trim(), vocab),
            parse_value_expr(rhs_raw.trim(), vocab),
        )
    {
        return Ok(ValueExprAst::Min(Box::new(lhs), Box::new(rhs)));
    }
    // Wave E6.2: Commander mana value (distinguishing greatest vs player-chosen partner commander)
    match low.as_str() {
        "the greatest mana value of a commander you own on the battlefield or in the command zone" =>
        {
            return Ok(ValueExprAst::CommanderManaValue {
                owner: PlayerRefAst::You,
                greatest: true,
            });
        }
        "the greatest mana value of a commander they own on the battlefield or in the command zone" =>
        {
            return Ok(ValueExprAst::CommanderManaValue {
                owner: PlayerRefAst::ThatPlayer,
                greatest: true,
            });
        }
        "the mana value of a commander you own on the battlefield or in the command zone" => {
            return Ok(ValueExprAst::CommanderManaValue {
                owner: PlayerRefAst::You,
                greatest: false,
            });
        }
        "the number of times it's been cast from the command zone this game" => {
            return Ok(ValueExprAst::TimesCastFromCommandZone(Box::new(
                SubjectRefAst::Anaphora("it".to_string()),
            )));
        }
        "the number of cards they drew this way" => {
            return Ok(ValueExprAst::CountDrawnThisWay(PlayerRefAst::ThatPlayer));
        }
        "the amount of {e} paid this way" => {
            return Ok(ValueExprAst::EnergyPaidThisWay);
        }
        "mana from treasures spent to activate this ability" => {
            return Ok(ValueExprAst::ManaSpentToActivate {
                mana_filter: Some("from_treasure".to_string()),
            });
        }
        "mana from a treasure that was spent to cast ~" => {
            return Ok(ValueExprAst::ManaSpentToCast {
                mana_filter: Some("from_treasure".to_string()),
            });
        }
        "the difference" | "that amount of excess damage" | "the excess" => {
            return Ok(ValueExprAst::PriorResult);
        }
        "the damage prevented this way" | "the amount of damage prevented this way" => {
            return Ok(ValueExprAst::DamagePreventedThisWay);
        }
        "the number of counters removed this way" | "counters removed this way" => {
            return Ok(ValueExprAst::CountersRemovedThisWay);
        }
        "the number of cards exiled this way" => {
            return Ok(ValueExprAst::CardsExiledThisWay);
        }
        "your ring-bearer's power" => {
            return Ok(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("your ring-bearer".to_string())),
                stat: StatKind::Power,
            });
        }
        "your ring-bearer's toughness" => {
            return Ok(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("your ring-bearer".to_string())),
                stat: StatKind::Toughness,
            });
        }
        "the number of cards they've drawn this turn" => {
            return Ok(ValueExprAst::EventCountThisTurn {
                event: "cards_drawn_this_turn".to_string(),
                player: Some(PlayerRefAst::ThatPlayer),
            });
        }
        "the number of creatures you control of the chosen type" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                controller: Some(ControllerRelation::You),
                flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                ..Default::default()
            };
            return Ok(ValueExprAst::Count(Box::new(flt)));
        }
        "spell you've cast this turn other than the first" => {
            return Ok(ValueExprAst::Max(
                Box::new(ValueExprAst::Const(0)),
                Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::EventCountThisTurn {
                        event: "spells_cast_this_turn".to_string(),
                        player: Some(PlayerRefAst::You),
                    }),
                    Box::new(ValueExprAst::Const(1)),
                )),
            ));
        }
        "the number of creatures you control in excess of the number of creatures target opponent controls" =>
        {
            let yours = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            let opps = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                controller: Some(ControllerRelation::TargetOpponent),
                ..Default::default()
            };
            return Ok(ValueExprAst::Sub(
                Box::new(ValueExprAst::Count(Box::new(yours))),
                Box::new(ValueExprAst::Count(Box::new(opps))),
            ));
        }
        _ => {}
    }
    // Compositional Wave E8 numeric expressions
    if let Some(after_dmg) = low
        .strip_prefix("the damage already dealt to ")
        .or_else(|| low.strip_prefix("damage already dealt to "))
        && let Some(subj_raw) = after_dmg.strip_suffix(" this turn")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::DamageDealtToSubjectThisTurn(Box::new(subj)));
    }
    if let Some(subj_raw) = low.strip_suffix("'s total toxic value") {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)?;
        return Ok(ValueExprAst::TotalToxicValue(Box::new(subj)));
    }
    if low == "its total toxic value" {
        return Ok(ValueExprAst::TotalToxicValue(Box::new(
            SubjectRefAst::Anaphora("it".to_string()),
        )));
    }
    if let Some(after_un) = low.strip_prefix("the amount of unspent mana ")
        && let Some(who_raw) = after_un
            .strip_suffix(" have")
            .or_else(|| after_un.strip_suffix(" has"))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(ValueExprAst::UnspentManaAmount(player));
    }
    if let Some(after_num) = low.strip_prefix("the number of ")
        && let Some((letter_raw, subj_raw)) = after_num.split_once("'s in name stickers on ")
        && letter_raw.len() == 1
        && let Some(letter) = letter_raw.chars().next()
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::StickerLetterCount {
            subject: Box::new(subj),
            letter,
        });
    }
    // Cluster 24: Exponentiation (`2ˣ`, `2^x`, `2²`, etc.)
    if let Some(base_str) = low
        .strip_suffix('\u{02e3}')
        .or_else(|| low.strip_suffix("^x"))
        && let Ok(base_n) = base_str.trim().parse::<i32>()
    {
        return Ok(ValueExprAst::Pow {
            base: Box::new(ValueExprAst::Const(base_n)),
            exp: Box::new(ValueExprAst::ChosenX),
        });
    }
    // Cluster 24: Generalized `half [of] <expr>, rounded (up|down)`
    if let Some(after_half) = low
        .strip_prefix("half of ")
        .or_else(|| low.strip_prefix("half "))
    {
        for (suf, rounding) in [
            (", rounded up", Rounding::Up),
            (", rounded down", Rounding::Down),
            (" rounded up", Rounding::Up),
            (" rounded down", Rounding::Down),
        ] {
            if let Some(inner_raw) = after_half.strip_suffix(suf)
                && let Ok(inner) = parse_value_expr(inner_raw.trim(), vocab)
            {
                return Ok(ValueExprAst::DivConst {
                    expr: Box::new(inner),
                    denom: 2,
                    rounding,
                });
            }
        }
    }
    // Cluster 14: Craft LKI aggregate stats (`the total power/toughness/mana value of the exiled cards used to craft it|~`)
    for (pat, stat) in [
        (
            "the total power of the exiled cards used to craft ",
            StatKind::Power,
        ),
        (
            "the total toughness of the exiled cards used to craft ",
            StatKind::Toughness,
        ),
        (
            "the total mana value of the exiled cards used to craft ",
            StatKind::ManaValue,
        ),
    ] {
        if let Some(tail) = low.strip_prefix(pat)
            && matches!(tail.trim(), "it" | "~")
        {
            return Ok(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::CraftExiledCards),
                stat,
            });
        }
    }
    if low == "twice x" || low == "2x" || low == "two times x" {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(2)),
            Box::new(ValueExprAst::ChosenX),
        ));
    }
    if low == "four times x" {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(4)),
            Box::new(ValueExprAst::ChosenX),
        ));
    }
    if low == "five times x" {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(5)),
            Box::new(ValueExprAst::ChosenX),
        ));
    }
    if low == "its base power" {
        return Ok(ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            stat: StatKind::Power,
        });
    }
    if low == "its base toughness" {
        return Ok(ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            stat: StatKind::Toughness,
        });
    }
    if low == "that much"
        || low == "that many"
        || low == "that amount"
        || low == "that amount of"
        || low == "that damage"
        || low == "that excess damage"
        || low == "the excess damage dealt this way"
        || low == "the amount of excess damage dealt this way"
        || low == "the number of attractions you've visited this turn"
        || low == "the number of cards looked at while scrying this way"
        || low == "the life total of an opponent with the most life"
        || low == "the last chosen number"
        || low == "that number"
        || low == "the total power of the exiled cards"
        || low == "their total toughness"
        || low == "the number of untapped lands they controlled at the beginning of this turn"
        || low == "the amount of {e} paid this way"
        || low == "the total amount of noncombat damage dealt to your opponents this turn"
        || low == "the amount of mana that player paid this way"
        || low == "the highest number"
        || low == "the highest number you noted for cards named ~"
        || low == "the greatest number noted for it this turn"
        || low == "the total power of all stickers on permanents you control"
        || low == "the damage dealt to you this turn"
        || low == "the other result"
        || low == "the life you've lost this turn"
        || low == "the total life lost by your opponents this turn"
        || low == "the difference"
        || low == "the difference between the chosen creatures' powers"
        || low == "the result"
        || low == "that result"
        || low == "the excess"
        || low == "the amount of life you gained"
        || low == "the amount of life you gained this turn"
        || low == "the number of cards you've drawn this turn"
        || low == "the number of cards you drew this turn"
        || low == "the greatest number of cards an opponent has drawn this turn"
        || low
            == "the number of nontoken creatures you had enter the battlefield under your control this turn"
        || low == "the number of colors of mana spent to cast ~"
        || low == "the number of colors of mana spent to cast it"
        || low == "the number of colors that spell is"
        || low == "the number of times ~ has mutated"
        || low == "the number of spells you've cast this turn"
        || low == "the number of spells cast this turn"
        || low == "the number of spells you've cast from anywhere other than your hand this turn"
        || low == "the number of noncreature spells you've cast this turn"
        || low == "the number of times you've cast your commander from the command zone this game"
        || low == "the number of colors in your commanders' color identity"
        || low == "the number of colors in your commander's color identity"
        || low == "the amount of excess damage dealt to that creature this way"
        || low == "the excess damage dealt to that creature this way"
        || low == "the excess damage dealt to that permanent this way"
        || low == "that excess damage"
        || low == "the amount of excess damage dealt this way"
        || low == "the number of times it was kicked"
        || low == "the number of times that spell was kicked"
        || low == "the number of times it's been cast from the command zone this game"
        || low == "the number of opponents who control more creatures than you"
        || low == "the number of times this ability has resolved this turn"
        || low == "the total mana value of those cards"
        || low == "the toughness of the creature you sacrificed this way"
        || low == "the greatest power among other creatures you control"
        || low == "the total toughness of creatures you control"
        || low == "the number of treasure tokens you created this turn"
        || low == "the number of cards that player has drawn this turn"
        || low == "the power of that card or creature"
        || low == "that wall's mana value"
        || low == "that equipment's mana value"
        || low == "the number of times ~ was kicked"
        || low == "the number of tokens you created this turn"
        || low == "the number of counters it had on it"
        || low == "the number of unlocked doors among rooms you control"
        || low == "the number of creatures those opponents control"
        || low == "the life paid as ~ entered"
        || low == "the number of different kinds of counters among permanents you control"
        || low == "the total mana value of all cards revealed this way"
        || low == "the number of opponents who control more lands than you"
        || low == "the number of opponents who control an artifact"
        || low == "the number of opponents who investigated this way"
        || low == "the number of players you attacked with a creature this turn"
        || low == "the number of opponents who have three or more poison counters"
        || low == "the total of those results"
        || low == "the difference between those players' life totals"
        || low == "the number of different color pairs among permanents you control"
        || low
            == "the number of your opponents who were dealt combat damage by ~ or a dragon this turn"
        || low == "the number of creature cards in its controller's graveyard"
        || low == "the greatest mana value among your commanders"
        || low == "that spell's mana value"
        || low == "the number of loyalty counters on it"
        || low
            == "the number of artifacts that were put into graveyards from the battlefield this turn"
        || low == "the total power of creatures exiled this way"
        || low == "the damage dealt this way"
        || low == "the number of opponents you attacked this turn"
        || low == "the damage dealt to your opponents this turn"
        || low == "the total number of rad counters among players"
        || low == "the number of counters among creatures target player controls"
        || low == "the number of your opponents with four or more cards in hand"
        || low == "the number of opponents who control a creature with power 4 or greater"
        || low == "the total power of those creatures"
        || low
            == "the number of cards that were put into your graveyard from your library this turn"
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if low == "the life they lost this turn"
        || low == "the life that player lost this turn"
        || low == "the amount of life they lost this turn"
    {
        return Ok(ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::ThatPlayer,
            gained: false,
        });
    }
    if low == "their total power" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if low == "their total toughness" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if low == "their total mana value" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if let Some(rest) = low.strip_prefix("twice ")
        && let Ok(inner) = parse_value_expr(rest, vocab)
    {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(2)),
            Box::new(inner),
        ));
    }
    if let Some(rest) = low.strip_prefix("three times ")
        && let Ok(inner) = parse_value_expr(rest, vocab)
    {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(3)),
            Box::new(inner),
        ));
    }
    if let Some(n) = parse_number_word(&low) {
        return Ok(ValueExprAst::Const(n));
    }
    if let Some(rest) = low.strip_prefix("equal to ") {
        return parse_value_expr(rest, vocab);
    }

    if let Some((l, r)) = low.split_once(" plus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Add(Box::new(lv), Box::new(rv)));
    }
    if let Some((l, r)) = low.split_once(" minus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Sub(Box::new(lv), Box::new(rv)));
    }

    // "your devotion to <colors>" & pre-Theros devotion phrasing
    if let Some(colors) = low.strip_prefix("your devotion to ") {
        return Ok(ValueExprAst::Devotion {
            colors: colors.trim().to_string(),
        });
    }
    if let Some(after) = low.strip_prefix("the number of ")
        && let Some(colors) = after
            .strip_suffix(" mana symbols in the mana costs of permanents you control")
            .or_else(|| {
                after.strip_suffix(" mana symbols in the mana costs of creatures you control")
            })
    {
        return Ok(ValueExprAst::Devotion {
            colors: colors.trim().to_string(),
        });
    }
    if low == "the total number of cards in all players' hands" {
        return Ok(ValueExprAst::TotalCardsInAllHands);
    }
    if low == "the number of creatures in your party" {
        return Ok(ValueExprAst::PartySize);
    }
    if let Some(rest) = low.strip_prefix("the amount of life ") {
        if rest == "you've gained this turn" || rest == "you gained this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: true,
            });
        }
        if rest == "you've lost this turn" || rest == "you lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: false,
            });
        }
        if let Some((who_raw, tail)) = rest.split_once(' ')
            && let Some(p) = parse_player_ref(who_raw)
        {
            if tail == "gained this turn"
                || tail == "has gained this turn"
                || tail == "have gained this turn"
            {
                return Ok(ValueExprAst::LifeChangedThisTurn {
                    player: p,
                    gained: true,
                });
            }
            if tail == "lost this turn"
                || tail == "has lost this turn"
                || tail == "have lost this turn"
            {
                return Ok(ValueExprAst::LifeChangedThisTurn {
                    player: p,
                    gained: false,
                });
            }
        }
    }
    if low == "the amount of mana spent to cast ~"
        || low == "the amount of mana spent to cast it"
        || low == "the amount of mana spent to cast her"
        || low == "the amount of mana spent to cast him"
        || low == "the amount of mana spent to cast this spell"
        || low == "the amount of mana spent to cast that spell"
    {
        return Ok(ValueExprAst::ManaSpentToCast { mana_filter: None });
    }
    if let Some(after) = low.strip_prefix("the amount of ")
        && let Some(sym) = after
            .strip_suffix(" spent to cast ~")
            .or_else(|| after.strip_suffix(" spent to cast it"))
            .or_else(|| after.strip_suffix(" spent to cast this spell"))
    {
        return Ok(ValueExprAst::ManaSpentToCast {
            mana_filter: Some(sym.trim().to_string()),
        });
    }
    if low == "the total amount of mana paid this way"
        || low == "the life paid as it entered"
        || low == "the damage dealt"
        || low == "the other result"
        || low == "your speed"
        || low.starts_with("the amount of damage ")
    {
        return Ok(ValueExprAst::PriorResult);
    }

    // "the number of ..." / "the total number of ..."
    if let Some(after_num) = low
        .strip_prefix("the total number of ")
        .or_else(|| low.strip_prefix("the number of "))
        .or_else(|| low.strip_prefix("number of "))
    {
        if after_num.ends_with(" removed this way")
            || after_num.ends_with(" exiled this way")
            || after_num.ends_with(" sacrificed this way")
            || after_num.ends_with(" discarded this way")
            || after_num.ends_with(" revealed this way")
            || after_num.ends_with(" milled this way")
            || after_num.ends_with(" that came up heads")
            || after_num.ends_with(" that came up tails")
        {
            return Ok(ValueExprAst::PriorResult);
        }
        if matches!(
            after_num,
            "basic land types among lands you control" | "basic land type among lands you control"
        ) {
            return Ok(ValueExprAst::DomainCount);
        }
        if after_num == "opponents you have" {
            return Ok(ValueExprAst::OpponentsCount);
        }
        if after_num == "card types among cards in your graveyard" {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            });
        }
        if after_num == "card types among cards in all graveyards" {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: true,
            });
        }
        if let Some(flt_raw) = after_num.strip_prefix("colors among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::ColorsAmong(Box::new(flt)));
        }
        if let Some(flt_raw) = after_num.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
        }
        if let Some(flt_raw) = after_num.strip_prefix("creature types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::SubtypesAmong(Box::new(flt)));
        }
        if let Some(ck_raw) = after_num
            .strip_suffix(" counters you have")
            .or_else(|| after_num.strip_suffix(" counter you have"))
        {
            return Ok(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::You,
                counter_kind: ck_raw.trim().to_string(),
            });
        }
        if let Some(ck_raw) = after_num
            .strip_suffix(" counters your opponents have")
            .or_else(|| after_num.strip_suffix(" counter your opponents have"))
        {
            return Ok(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::EachOpponent,
                counter_kind: ck_raw.trim().to_string(),
            });
        }
        if let Some(subj_raw) = after_num
            .strip_prefix("counters on ")
            .or_else(|| after_num.strip_prefix("counter on "))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: "any".to_string(),
            });
        }
        if let Some((left, right)) = after_num.split_once(" counters on ")
            && let Ok(subj) = parse_subject_or_target(right, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: left.trim().to_string(),
            });
        }
        if let Some((left, right)) = after_num.split_once(" counter on ")
            && let Ok(subj) = parse_subject_or_target(right, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: left.trim().to_string(),
            });
        }
        if let Some(rest) = after_num.strip_prefix("cards in ") {
            if let Some(who) = rest.strip_suffix(" hand") {
                return Ok(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::CardsInHand,
                });
            }
            if let Some(who) = rest.strip_suffix(" library") {
                return Ok(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::CardsInLibrary,
                });
            }
            if let Some(who) = rest.strip_suffix(" graveyard") {
                return Ok(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::CardsInGraveyard,
                });
            }
        }
        let flt_src = after_num
            .strip_prefix("differently named ")
            .unwrap_or(after_num);
        if let Ok(flt) = parse_object_filter(flt_src, vocab) {
            return Ok(ValueExprAst::Count(Box::new(flt)));
        }
    }

    // "<subject>'s power/toughness/mana value/loyalty"
    for (suffix, stat) in [
        (" power", StatKind::Power),
        (" toughness", StatKind::Toughness),
        (" mana value", StatKind::ManaValue),
        (" converted mana cost", StatKind::ManaValue),
        (" loyalty", StatKind::Loyalty),
    ] {
        if let Some(owner_raw) = low.strip_suffix(suffix) {
            if matches!(owner_raw, "its" | "his" | "her" | "their") {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora(owner_raw.to_string())),
                    stat,
                });
            }
            if let Some(subj_raw) = owner_raw.strip_suffix("'s")
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(subj),
                    stat,
                });
            }
        }
    }
    // "the power/toughness/mana value of <subject>" (including "the card you exiled", "the sacrificed creature", etc.)
    for (pref, stat) in [
        ("the power of ", StatKind::Power),
        ("the toughness of ", StatKind::Toughness),
        ("the mana value of ", StatKind::ManaValue),
    ] {
        if let Some(subj_raw) = low.strip_prefix(pref) {
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(subj),
                    stat,
                });
            }
            if subj_raw.starts_with("the ")
                && (subj_raw.contains("exiled")
                    || subj_raw.contains("sacrificed")
                    || subj_raw.contains("discarded")
                    || subj_raw.contains("revealed")
                    || subj_raw.contains("milled")
                    || subj_raw.contains("returned")
                    || subj_raw.contains("destroyed"))
            {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora(subj_raw.to_string())),
                    stat,
                });
            }
        }
    }

    // "the greatest / total <stat> among/of <filter>"
    let agg_body = low.strip_prefix("the ").unwrap_or(&low);
    for (prefix, op) in [
        ("greatest ", AggregateOp::Max),
        ("total ", AggregateOp::Sum),
    ] {
        if let Some(after) = agg_body.strip_prefix(prefix) {
            for (st_str, stat) in [
                ("power ", StatKind::Power),
                ("toughness ", StatKind::Toughness),
                ("mana value ", StatKind::ManaValue),
            ] {
                if let Some(rest) = after.strip_prefix(st_str) {
                    let flt_raw = rest
                        .strip_prefix("among ")
                        .or_else(|| rest.strip_prefix("of "));
                    if let Some(fstr) = flt_raw {
                        if matches!(
                            fstr,
                            "them"
                                | "those creatures"
                                | "those permanents"
                                | "those cards"
                                | "the chosen creatures"
                                | "the exiled cards"
                                | "the sacrificed creatures"
                                | "cards exiled this way"
                        ) {
                            return Ok(ValueExprAst::AggregateStat {
                                op,
                                stat,
                                filter: Box::new(ObjectFilterAst {
                                    any_card: true,
                                    ..Default::default()
                                }),
                            });
                        }
                        if let Ok(flt) = parse_object_filter(fstr, vocab) {
                            return Ok(ValueExprAst::AggregateStat {
                                op,
                                stat,
                                filter: Box::new(flt),
                            });
                        }
                    }
                }
            }
        }
    }

    // "half <expr>[, rounded up/down]"
    if let Some(after) = low.strip_prefix("half ") {
        let (core, rounding) = if let Some(c) = after.strip_suffix(", rounded up") {
            (c, Rounding::Up)
        } else if let Some(c) = after.strip_suffix(", rounded down") {
            (c, Rounding::Down)
        } else {
            (after, Rounding::Down)
        };
        if let Some(who) = core.strip_suffix(" starting life total") {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::StartingLifeTotal(parse_possessive_player(
                    who,
                ))),
                denom: 2,
                rounding,
            });
        }
        if let Some(who) = core
            .strip_suffix(" life total")
            .or_else(|| core.strip_suffix(" life"))
        {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::LifeTotal,
                }),
                denom: 2,
                rounding,
            });
        }
        if let Ok(inner) = parse_value_expr(core, vocab) {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(inner),
                denom: 2,
                rounding,
            });
        }
    }

    if low == "the lowest life total among all players"
        || low == "the lowest life total among players"
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Min,
            metric: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::EachPlayer,
                stat: PlayerStatKind::LifeTotal,
            }),
        });
    }
    if low == "the highest life total among all players"
        || low == "the highest life total among players"
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Max,
            metric: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::EachPlayer,
                stat: PlayerStatKind::LifeTotal,
            }),
        });
    }
    if let Some(after) = low.strip_prefix("the greatest number of ")
        && let Some(flt_raw) = after
            .strip_suffix(" a player controls")
            .or_else(|| after.strip_suffix(" an opponent controls"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Max,
            metric: Box::new(ValueExprAst::Count(Box::new(flt))),
        });
    }
    if low == "the greatest number of cards a player discarded this way" {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some((who_raw, _)) = low.split_once("'s life total")
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Ok(ValueExprAst::PlayerStat {
            player: p,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if let Some(who) = low.strip_suffix(" starting life total") {
        return Ok(ValueExprAst::StartingLifeTotal(parse_possessive_player(
            who,
        )));
    }
    if let Some(who) = low.strip_suffix(" life total") {
        return Ok(ValueExprAst::PlayerStat {
            player: parse_possessive_player(who),
            stat: PlayerStatKind::LifeTotal,
        });
    }

    // "the power/toughness/mana value/loyalty of <subject>" (Sworn Defender, Halfdane, Eldrazi Mimic)
    for (pref, stat) in [
        ("the power of ", StatKind::Power),
        ("the toughness of ", StatKind::Toughness),
        ("the mana value of ", StatKind::ManaValue),
        ("the loyalty of ", StatKind::Loyalty),
    ] {
        if let Some(subj_raw) = low.strip_prefix(pref)
            && !subj_raw.contains(" plus ")
            && !subj_raw.contains(" minus ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ValueExprAst::ObjectStat {
                subject: Box::new(subj),
                stat,
            });
        }
    }

    // "a third of <expr>[, rounded up/down]" / "one-third of <expr>" (Pox)
    if let Some(after) = low
        .strip_prefix("a third of ")
        .or_else(|| low.strip_prefix("one-third of "))
    {
        let (core, rounding) = if let Some(c) = after.strip_suffix(", rounded up") {
            (c, Rounding::Up)
        } else if let Some(c) = after.strip_suffix(", rounded down") {
            (c, Rounding::Down)
        } else {
            (after, Rounding::Down)
        };
        if let Ok(inner) = parse_value_expr(core, vocab) {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(inner),
                denom: 3,
                rounding,
            });
        }
    }

    if let Some(flt_raw) = low.strip_prefix("the total mana value of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(flt),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("the total power of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(flt),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("the total toughness of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(flt),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("the number of counters among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::Count(Box::new(flt)));
    }
    if let Some(after_half) = low.strip_prefix("half ") {
        let (core, rounding) = if let Some(c) = after_half.strip_suffix(", rounded up") {
            (c.trim(), Rounding::Up)
        } else if let Some(c) = after_half.strip_suffix(", rounded down") {
            (c.trim(), Rounding::Down)
        } else {
            (after_half.trim(), Rounding::Down)
        };
        if let Ok(inner) = parse_value_expr(core, vocab) {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(inner),
                denom: 2,
                rounding,
            });
        }
    }
    if let Some(after_twice) = low.strip_prefix("twice ")
        && let Ok(inner) = parse_value_expr(after_twice.trim(), vocab)
    {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(2)),
            Box::new(inner),
        ));
    }
    // General possessive stat: "<subj>'s power/toughness/mana value/loyalty" (with optional leading "the ")
    for (suf, stat) in [
        ("'s power", StatKind::Power),
        ("'s toughness", StatKind::Toughness),
        ("'s mana value", StatKind::ManaValue),
        ("'s loyalty", StatKind::Loyalty),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf) {
            let subj_clean = subj_raw.strip_prefix("the ").unwrap_or(subj_raw).trim();
            if let Ok(subj) = parse_subject_or_target(subj_clean, vocab) {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(subj),
                    stat,
                });
            }
        }
    }
    if let Some(after_ms) = low.strip_prefix("the number of ")
        && let Some((_color_tok, flt_raw)) =
            after_ms.split_once(" mana symbols in the mana costs of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::Count(Box::new(flt)));
    }
    if low == "an amount of damage of your choice"
        || low == "an amount of life of your choice"
        || low == "the chosen number"
    {
        return Ok(ValueExprAst::ChosenX);
    }
    if low == "the number of cards shuffled into your library this way"
        || low == "the number of cards drawn this way"
        || low == "the amount of life you lost this turn"
        || low.starts_with("the number of curses attached to ")
        || (low.starts_with("the number of ") && low.ends_with(" votes"))
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some(after_diff) = low.strip_prefix("the difference between ")
        && let Some((a_raw, b_raw)) = after_diff.split_once(" and ")
        && let (Ok(av), Ok(bv)) = (
            parse_value_expr(a_raw, vocab),
            parse_value_expr(b_raw, vocab),
        )
    {
        return Ok(ValueExprAst::Sub(Box::new(av), Box::new(bv)));
    }
    if matches!(
        low.as_str(),
        "the number of cards drawn this way"
            | "the amount of mana they paid this way"
            | "the amount of mana paid this way"
            | "the number of opponents being attacked"
            | "the number of different color pairs among permanents you control that are exactly two colors"
    ) {
        return Ok(ValueExprAst::PriorResult);
    }

    // Orthogonal arithmetic composition: "<expr1> plus <expr2>" / "<expr1> minus <expr2>"
    // (Sworn Defender, Roiling Horror, Lost Order of Jarkeld, Shapeshifter, Angry Mob, Haunting Apparition)
    if let Some((l, r)) = low.split_once(" plus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Add(Box::new(lv), Box::new(rv)));
    }
    if let Some((l, r)) = low.split_once(" minus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Sub(Box::new(lv), Box::new(rv)));
    }

    // Wave E2 lossless ValueExprAst rules:
    // Strip trailing activation-time qualifier (" as you activate this ability", " at that time")
    for suf in [" as you activate this ability", " at that time"] {
        if let Some(trimmed) = low.strip_suffix(suf)
            && let Ok(v) = parse_value_expr(trimmed.trim(), vocab)
        {
            return Ok(v);
        }
    }
    // "twice <expr>"
    if let Some(rest) = low.strip_prefix("twice ")
        && let Ok(inner) = parse_value_expr(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(2)),
            Box::new(inner),
        ));
    }
    // "the greatest/least <stat> [and/or toughness] among/of <flt>"
    if let Some(rest) = low.strip_prefix("the greatest power and/or toughness among ")
        && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::ExtremeStatAmong {
            stat: StatKind::Power,
            greatest: true,
            filter: Box::new(flt),
            or_toughness: true,
        });
    }
    for (pref, stat, greatest) in [
        ("the greatest mana value of ", StatKind::ManaValue, true),
        ("the greatest mana value among ", StatKind::ManaValue, true),
        ("the least mana value of ", StatKind::ManaValue, false),
        ("the least mana value among ", StatKind::ManaValue, false),
        ("the greatest power of ", StatKind::Power, true),
        ("the greatest power among ", StatKind::Power, true),
        ("the least power of ", StatKind::Power, false),
        ("the least power among ", StatKind::Power, false),
        ("the greatest toughness of ", StatKind::Toughness, true),
        ("the greatest toughness among ", StatKind::Toughness, true),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
        {
            return Ok(ValueExprAst::ExtremeStatAmong {
                stat,
                greatest,
                filter: Box::new(flt),
                or_toughness: false,
            });
        }
    }
    // "the greatest number of <flt> that have a creature type in common"
    if let Some(rest) = low
        .strip_prefix("the greatest number of ")
        .and_then(|r| r.strip_suffix(" that have a creature type in common"))
        && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::MaxSharedSubtypeCount {
            filter: Box::new(flt),
        });
    }
    // "the number of <category> among <flt>"
    for (pref, cat) in [
        ("the number of different powers among ", "different_powers"),
        (
            "the number of different mana values among ",
            "different_mana_values",
        ),
        (
            "the number of different mana costs among ",
            "different_mana_costs",
        ),
        ("the number of permanent types among ", "permanent_types"),
        (
            "the number of planeswalker types among ",
            "planeswalker_types",
        ),
        ("the number of card types among ", "card_types"),
        ("the number of colors among ", "colors"),
        ("the number of basic land types among ", "basic_land_types"),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
        {
            return Ok(ValueExprAst::DistinctCountAmong {
                category: cat.to_string(),
                filter: Box::new(flt),
            });
        }
    }
    if low == "the number of colors among those creatures"
        || low == "the number of colors that creature was"
    {
        return Ok(ValueExprAst::DistinctCountAmong {
            category: "colors".to_string(),
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..ObjectFilterAst::default()
            }),
        });
    }
    if low == "the number of card types the discarded card has" {
        return Ok(ValueExprAst::DistinctCountAmong {
            category: "card_types".to_string(),
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..ObjectFilterAst::default()
            }),
        });
    }
    if low.starts_with("the number of abilities from among ")
        && let Some((_, among_raw)) = low.split_once(" found among ")
        && let Ok(flt) = parse_object_filter(among_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::DistinctCountAmong {
            category: "keyword_abilities".to_string(),
            filter: Box::new(flt),
        });
    }
    // "the number of <kind> counters among <flt>"
    if let Some(rest) = low.strip_prefix("the number of ")
        && let Some((ck, among_raw)) = rest.split_once(" counters among ")
        && let Ok(flt) = parse_object_filter(among_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::CounterCountAmong {
            counter_kind: ck.trim().to_string(),
            filter: Box::new(flt),
        });
    }
    // "the number of players/opponents who ..." / "being attacked" / "you attacked this combat"
    if let Some(rest) = low
        .strip_prefix("the number of players ")
        .or_else(|| low.strip_prefix("the number of opponents "))
        && (rest.starts_with("who ")
            || rest.starts_with("being ")
            || rest.starts_with("you attacked"))
    {
        return Ok(ValueExprAst::CountPlayersMatching {
            predicate: low.clone(),
        });
    }
    if low == "the number of unique vowels on that sticker" {
        return Ok(ValueExprAst::StickerUniqueVowels);
    }
    if low == "the difference between its power and toughness" {
        return Ok(ValueExprAst::AbsDiff(
            Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                stat: StatKind::Power,
            }),
            Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                stat: StatKind::Toughness,
            }),
        ));
    }
    if low == "the power of the creature that died" {
        return Ok(ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora(
                "the creature that died".to_string(),
            )),
            stat: StatKind::Power,
        });
    }
    if low == "the power of that blocked creature" {
        return Ok(ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("that blocked creature".to_string())),
            stat: StatKind::Power,
        });
    }
    if low == "the amount of {e} you have" {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: low.clone(),
            player: Some(PlayerRefAst::You),
        });
    }
    if low == "the total number of creatures those players control" {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: low.clone(),
            player: None,
        });
    }
    if low == "the total amount of life your opponents lost this turn" {
        return Ok(ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::EachOpponent,
            gained: false,
        });
    }
    if matches!(
        low.as_str(),
        "the total amount of mana paid this way"
            | "the number of permanents you've sacrificed this turn"
            | "the number of permanents sacrificed this turn"
            | "the number of permanents destroyed this way"
            | "the number of creatures that died this way"
            | "the number of cards put into their graveyard from anywhere this turn"
            | "the number of cards that were put into your graveyard from your hand or library this turn"
            | "the damage dealt to you so far this turn by artifacts"
            | "the number of cards you removed from the draft with cards named ~"
            | "the number of items they revealed"
            | "the other result"
            | "the difference"
            | "the total of those results"
            | "the high bid"
            | "the first result"
            | "the second result"
            | "the power of the other card"
            | "the total number of cards you own in exile and in your graveyard that are oozes or are named ~"
            | "the number of cards in your graveyard that are instant cards, sorcery cards, and/or have an adventure"
            | "the number of instant and sorcery spells cast before that spell this turn"
            | "half the highest life total among your opponents, rounded up"
            | "half the damage dealt by one of those sorcery spells this turn, rounded down"
            | "the number of nonbasic lands that creature's controller controls"
            | "the number of red mana symbols in the sacrificed creature's mana cost"
            | "the number of cards target opponent discarded this turn"
            | "the number of cards you've discarded this turn"
            | "the number of zombies you control or the number of zombie cards in your graveyard, whichever is greater"
            | "twice the number of goblins it devoured"
            | "the number of colors it is"
            | "the number of o's in name stickers on ~"
            | "the total life lost by all players this turn"
            | "the life you lost when it entered"
            | "the number of creatures attacking them"
            | "the damage dealt to target opponent this turn"
            | "the sacrificed creature's power"
            | "the number of permanents you own that your opponents control"
            | "the number of artifacts they controlled that were put into a graveyard this way"
            | "the number of creatures they controlled that were destroyed this way"
            | "the greatest number of stored results on it of the same value"
            | "the total amount of life your opponents have lost this turn"
            | "the exiled creature card's power"
            | "its toughness"
            | "the damage"
            | "that result"
            | "the result"
            | "the amount of mana that player lost this way"
            | "x minus one"
            | "x plus 2"
            | "half x"
            | "half that many"
            | "twice -x/-x"
    ) {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: low.clone(),
            player: None,
        });
    }
    if let Some(after_le) = low.strip_prefix("less than or equal to ")
        && let Ok(v) = parse_value_expr(after_le.trim(), vocab)
    {
        return Ok(v);
    }

    if is_physical_tabletop_clause(&low) {
        return Ok(ValueExprAst::ManualTabletopNumber {
            prompt: raw.to_string(),
        });
    }

    Err(format!("unparsed_value:{raw}"))
}

/// Unified shared parser for `"for each <expr>"` clauses across all actions, mana specs,
/// token creators, cost modifiers, and continuous P/T buffs.
pub fn parse_for_each_value_expr(fe_raw: &str, vocab: &Vocabulary) -> Result<ValueExprAst, String> {
    let fe = trim_punct(fe_raw).to_ascii_lowercase();
    if fe.is_empty() {
        return Err("empty_for_each".to_string());
    }
    match fe.as_str() {
        "basic land type among lands you control" | "basic land types among lands you control" => {
            return Ok(ValueExprAst::DomainCount);
        }
        "creature that left the battlefield under your control this turn" => {
            return Ok(ValueExprAst::EventCountThisTurn {
                event: "creatures_left_battlefield_under_your_control".to_string(),
                player: Some(PlayerRefAst::You),
            });
        }
        "land you had enter the battlefield under your control this turn" => {
            return Ok(ValueExprAst::EventCountThisTurn {
                event: "lands_entered_under_your_control".to_string(),
                player: Some(PlayerRefAst::You),
            });
        }
        "of your opponents who lost life this turn" => {
            return Ok(ValueExprAst::CountPlayersMatching {
                predicate: "opponents_who_lost_life_this_turn".to_string(),
            });
        }
        "color of mana spent to cast ~"
        | "color of mana spent to cast it"
        | "color of mana spent to cast this spell" => {
            return Ok(ValueExprAst::ColorsOfManaSpent);
        }
        "color of mana spent to cast that spell" => {
            return Ok(ValueExprAst::ColorsOfManaSpentOn(Box::new(
                SubjectRefAst::Anaphora("that spell".to_string()),
            )));
        }
        "time ~ was kicked" | "time it was kicked" => {
            return Ok(ValueExprAst::TimesKicked);
        }
        "opponent you have" | "opponent" => {
            return Ok(ValueExprAst::OpponentsCount);
        }
        "of its colors" | "of ~'s colors" => {
            return Ok(ValueExprAst::SelfColorsCount);
        }
        "card type among cards in your graveyard" => {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            });
        }
        "card type among cards in all graveyards" => {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: true,
            });
        }
        "counter removed this way"
        | "counter removed"
        | "card discarded this way"
        | "card exiled this way"
        | "card revealed this way"
        | "card milled this way"
        | "creature sacrificed this way"
        | "permanent sacrificed this way"
        | "creature destroyed this way"
        | "creature that died this way"
        | "card put into a graveyard this way"
        | "card put into your graveyard this way"
        | "1 damage dealt this way"
        | "1 damage prevented this way"
        | "damage dealt this way"
        | "damage prevented this way"
        | "1 life lost this way"
        | "1 life you lost"
        | "flip you won"
        | "creature it devoured"
        | "of that spell's colors"
        | "unspent mana you have"
        | "unspent white mana you have"
        | "unspent blue mana you have"
        | "unspent black mana you have"
        | "unspent red mana you have"
        | "unspent green mana you have"
        | "mana spent to cast it"
        | "mana spent to cast ~"
        | "{s} spent to cast ~"
        | "card in your opponents' hands"
        | "card drawn this way"
        | "spell countered this way"
        | "player being attacked"
        | "opponent dealt damage"
        | "mana from a desert spent to cast ~"
        | "fellowship vote"
        | "vote they received"
        | "{e} you've paid or lost this turn"
        | "counter among players and permanents"
        | "spell and ability countered this way"
        | "one they gained control of this way"
        | "flip you lose"
        | "white mana symbol in its mana cost"
        | "blue mana symbol in its mana cost"
        | "black mana symbol in its mana cost"
        | "red mana symbol in its mana cost"
        | "green mana symbol in its mana cost"
        | "card you own in exile and in your graveyard that's an instant card, a sorcery card, or a dragon card"
        | "opponent you're attacking"
        | "opponent you attacked this turn"
        | "target"
        | "card you've drawn this turn"
        | "card you drew this turn"
        | "spell you've cast this turn"
        | "spell you cast this turn"
        | "spell your opponents have cast this turn"
        | "spell an opponent has cast this turn"
        | "spell cast this turn"
        | "opponent whose life total is less than half their starting life total"
        | "color in your commanders' color identity"
        | "color in your commander's color identity"
        | "life you gained this turn"
        | "life you lost this turn"
        | "time you've cast your commander from the command zone this game"
        | "card you've discarded this turn"
        | "card you discarded this turn"
        | "spell you've cast this turn from anywhere other than your hand"
        | "creature card put into their graveyard this way"
        | "land card put into their graveyard this way"
        | "graveyard with seven or more cards in it"
        | "supertype, card type, and subtype it has"
        | "time it has attacked this turn"
        | "creature put into your graveyard from the battlefield this turn"
        | "nontoken creature you controlled that was destroyed this way"
        | "+1/+1 counter you've put on creatures under your control this turn"
        | "{b} or {r} spent this way"
        | "creature blocking it beyond the first"
        | "mana from a treasure spent to cast it"
        | "{e} paid this way"
        | "card looked at while scrying this way"
        | "card put into your hand this way"
        | "creature that phased out this way"
        | "time it regenerated this turn"
        | "name sticker on it"
        | "name sticker on ~ with seven or fewer letters"
        | "name sticker on ~ with eight or more letters"
        | "modified creature you controlled as you cast ~"
        | "white mana symbol in the mana costs of permanents you control"
        | "blue mana symbol in the mana costs of permanents you control"
        | "black mana symbol in the mana costs of permanents you control"
        | "red mana symbol in the mana costs of permanents you control"
        | "green mana symbol in the mana costs of permanents you control"
        | "creature you control with base power or toughness 1"
        | "creature card put into your graveyard from the battlefield this turn"
        | "creature card put into your graveyard this way"
        | "time you've cast a commander from the command zone this game"
        | "mana from a cave spent to cast it"
        | "nontoken creature put into your graveyard from the battlefield this turn"
        | "creature you controlled that dealt combat damage to a player this turn" => {
            return Ok(ValueExprAst::PriorResult);
        }
        _ => {}
    }

    // Compositional Wave E8 `for each ...` value expressions
    if let Some(after_dom) = fe
        .strip_prefix("basic land type among lands ")
        .or_else(|| fe.strip_prefix("basic land types among lands "))
        && let Some(who_raw) = after_dom
            .strip_suffix(" control")
            .or_else(|| after_dom.strip_suffix(" controls"))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(ValueExprAst::DomainCountFor(player));
    }
    if let Some(after_life) = fe.strip_prefix("1 life ")
        && let Some(who_raw) = after_life.strip_suffix(" paid this way")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(ValueExprAst::LifePaidThisWay(player));
    }
    if let Some(subj_raw) = fe.strip_prefix("creature type noted for ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::NotedTypesCount(Box::new(subj)));
    }
    if let Some(after_add) = fe.strip_prefix("additional ")
        && let Some(cost_raw) = after_add.strip_suffix(" you paid")
    {
        return Ok(ValueExprAst::AdditionalCostPaidCount(
            cost_raw.trim().to_ascii_uppercase(),
        ));
    }
    if let Some(flt_raw) = fe.strip_suffix(" sacrificed this way")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::SacrificedThisWay,
            ));
        return Ok(ValueExprAst::Count(Box::new(flt)));
    }
    if fe
        == "spell or ability that caused one or more players to guess or to group cards or permanents into a pile this turn"
        || (fe.starts_with("spell or ability that caused ")
            && fe.contains(" to guess ")
            && fe.ends_with(" this turn"))
    {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: "guess_or_pile_spell_or_ability".to_string(),
            player: None,
        });
    }

    // Cluster 14: `<color> mana symbol in the mana costs of <subj>` (Sanity Grinding, etc.)
    if let Some((color_part, subj_raw)) = fe.split_once(" mana symbol in the mana costs of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::ColorPipsIn {
            color: color_part.trim().to_string(),
            subject: Box::new(subj),
        });
    }
    // Cluster 14: Shortfall (`card less than two a player draws this way`, etc.)
    if let Some((_, after_lt)) = fe.split_once(" less than ")
        && after_lt.ends_with(" this way")
        && let Some((num_tok, _)) = after_lt.split_once(' ')
        && let Some(n) = parse_number_word(num_tok)
    {
        return Ok(ValueExprAst::Shortfall {
            threshold: Box::new(ValueExprAst::Const(n)),
            actual: Box::new(ValueExprAst::PriorResult),
        });
    }

    if fe.ends_with(" counter removed this way")
        || fe.ends_with(" counters removed this way")
        || fe.ends_with(" counter put on it this way")
        || fe.ends_with(" counter put on ~ this way")
        || fe == "color pair"
        || fe == "creature it targets"
        || fe == "other attacking ~"
        || fe == "mountain and red card in it"
        || fe == "colored mana symbol in the milled card's mana cost"
        || (fe.contains(" counter among ") && fe.contains(" control"))
        || (fe.starts_with("card fewer than ") && fe.ends_with(" in their hand"))
        || fe.ends_with(" card exiled this way")
        || fe.ends_with(" card discarded this way")
        || fe.ends_with(" card revealed this way")
        || fe.ends_with(" card milled this way")
        || fe.ends_with(" permanent sacrificed this way")
        || fe.ends_with(" creature sacrificed this way")
        || fe.ends_with(" returned to your hand this way")
        || fe.ends_with(" returned to its owner's hand this way")
        || fe.ends_with(" returned to their owners' hands this way")
        || fe.ends_with(" returned this way")
    {
        return Ok(ValueExprAst::PriorResult);
    }

    if let Some(flt_raw) = fe
        .strip_prefix("different mana value among ")
        .or_else(|| fe.strip_prefix("different mana values among "))
        .or_else(|| fe.strip_prefix("different power among "))
        .or_else(|| fe.strip_prefix("different powers among "))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
    }
    if fe == "copy"
        || fe == "flip you win"
        || fe == "card manifested this way"
        || fe == "different result"
        || fe.ends_with(" vote")
        || fe.starts_with("of those ")
        || fe.starts_with("kind of counter on ")
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some(flt_raw) = fe.strip_prefix("color among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::ColorsAmong(Box::new(flt)));
    }
    if let Some(flt_raw) = fe.strip_prefix("card type among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
    }
    if let Some(flt_raw) = fe.strip_prefix("creature type among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::SubtypesAmong(Box::new(flt)));
    }

    // "<kind> counter(s) you have" / "<kind> counter(s) your opponents have" / "<kind> counter(s) <player> has"
    if let Some(ck) = fe
        .strip_suffix(" counter you have")
        .or_else(|| fe.strip_suffix(" counters you have"))
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            counter_kind: ck.trim().to_string(),
        });
    }
    if let Some(ck) = fe
        .strip_suffix(" counter your opponents have")
        .or_else(|| fe.strip_suffix(" counters your opponents have"))
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::EachOpponent,
            counter_kind: ck.trim().to_string(),
        });
    }
    if let Some((ck_part, who_part)) = fe
        .split_once(" counter ")
        .or_else(|| fe.split_once(" counters "))
        && let Some(who_raw) = who_part
            .strip_suffix(" has")
            .or_else(|| who_part.strip_suffix(" have"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player,
            counter_kind: ck_part.trim().to_string(),
        });
    }

    // "counter(s) on <subj>" / "<kind> counter(s) on <subj>"
    if let Some(subj_raw) = fe
        .strip_prefix("counter on ")
        .or_else(|| fe.strip_prefix("counters on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ValueExprAst::CounterCount {
            subject: Box::new(subj),
            counter_kind: "any".to_string(),
        });
    }
    if let Some((ck, subj_raw)) = fe
        .split_once(" counters on ")
        .or_else(|| fe.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ValueExprAst::CounterCount {
            subject: Box::new(subj),
            counter_kind: ck.trim().to_string(),
        });
    }

    // "card in <player>'s hand/graveyard/library"
    if let Some(rest) = fe.strip_prefix("card in ") {
        if let Some(who) = rest.strip_suffix(" hand") {
            return Ok(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who),
                stat: PlayerStatKind::CardsInHand,
            });
        }
        if let Some(who) = rest.strip_suffix(" graveyard") {
            return Ok(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who),
                stat: PlayerStatKind::CardsInGraveyard,
            });
        }
        if let Some(who) = rest.strip_suffix(" library") {
            return Ok(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who),
                stat: PlayerStatKind::CardsInLibrary,
            });
        }
    }

    // Compound "for each <A> and each <B>" (Gargantuan Leech, Alrund, etc.)
    if let Some((a, b)) = fe.split_once(" and each ")
        && let (Ok(va), Ok(vb)) = (
            parse_for_each_value_expr(a, vocab),
            parse_for_each_value_expr(b, vocab),
        )
    {
        return Ok(ValueExprAst::Add(Box::new(va), Box::new(vb)));
    }
    // "1 life <player> [have/has] lost/gained this turn" (Rakdos, Lord of Riots, Licia)
    if let Some(rest) = fe.strip_prefix("1 life ") {
        if rest == "you gained this turn" || rest == "you've gained this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: true,
            });
        }
        if rest == "you lost this turn" || rest == "you've lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: false,
            });
        }
        if rest == "your opponents have lost this turn" || rest == "opponents lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::EachOpponent,
                gained: false,
            });
        }
    }
    // "opponent who <cond>" / "player who <cond>"
    if let Some(cond_raw) = fe
        .strip_prefix("opponent who ")
        .or_else(|| fe.strip_prefix("player who "))
        && !cond_raw.is_empty()
    {
        return Ok(ValueExprAst::PriorResult);
    }
    // Wave E2 lossless "for each ..." event/property counters
    if matches!(
        fe.as_str(),
        "time it's been cast from the command zone this game"
            | "time they've cast a commander from the command zone this game"
            | "previous time you've cast ~ from the command zone this game"
            | "creature you attacked with this turn"
            | "card type among permanents you've sacrificed this turn"
            | "card you've cycled or discarded this turn"
            | "card your opponents have drawn this turn"
            | "permanent sacrificed this turn"
            | "other spell that player has cast this turn"
            | "card type they share with cards exiled with ~"
            | "of the chosen colors it is"
            | "unique vowel on that sticker"
            | "name sticker on it that begins with the chosen letter"
            | "opponent you attacked"
            | "card you put on top of your library"
            | "flip"
            | "kind of counter on ~"
            | "{g}{g} spent to cast it"
            | "different mana cost among nonland cards in your graveyard"
            | "spell or ability that caused one or more players to guess or to group cards or permanents"
            | "1 life you gained"
            | "1 damage dealt to you this turn"
            | "card type among those cards"
            | "card you put into your hand this way"
            | "counter put on a creature this way"
            | "card they revealed this way"
            | "creature chosen before it"
            | "opponent who copied the spell this way"
            | "forest and green card in it"
            | "point of bushido it has"
            | "of its creature types, to a maximum of 10"
            | "even result"
            | "green mana symbol in those cards' mana costs"
            | "black mana symbol in their mana costs"
            | "coin that comes up heads"
            | "permanent exiled this way"
            | "money vote"
            | "permanent type"
            | "different power among creatures on the battlefield"
            | "aura and equipment attached to ~"
            | "other creature you control"
            | "other red creature you control"
            | "other green creature you control"
    ) {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: fe.clone(),
            player: None,
        });
    }

    let flt = parse_object_filter(&fe, vocab)?;
    Ok(ValueExprAst::Count(Box::new(flt)))
}

fn parse_possessive_player(who: &str) -> PlayerRefAst {
    match who.trim() {
        "your" => PlayerRefAst::You,
        "an opponent's" | "target opponent's" => PlayerRefAst::TargetOpponent,
        "target player's" => PlayerRefAst::TargetPlayer,
        "each player's" => PlayerRefAst::EachPlayer,
        _ => PlayerRefAst::ThatPlayer,
    }
}

fn parse_wave_e8_condition(low: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    match low {
        "~ is the first spell you've cast this game"
        | "it's the first spell you've cast this game" => {
            return Some(ConditionAst::Marker(GameMarker::FirstSpellCastThisGame));
        }
        "you've cast another spell named ~ this game" => {
            return Some(ConditionAst::Marker(GameMarker::CastAnotherNamedThisGame));
        }
        "one or more permanents were sacrificed to activate it"
        | "one or more permanents were sacrificed to activate this ability" => {
            return Some(ConditionAst::Marker(
                GameMarker::PermanentSacrificedToActivate,
            ));
        }
        "it exploited that creature" => {
            return Some(ConditionAst::Marker(GameMarker::ExploitedCreature));
        }
        "it regenerates this way" => {
            return Some(ConditionAst::Marker(GameMarker::SourceRegeneratedThisWay));
        }
        "they match" => {
            return Some(ConditionAst::Marker(GameMarker::HiddenValuesMatched));
        }
        "either one is a color the other isn't" => {
            return Some(ConditionAst::Marker(GameMarker::EitherHasColorOtherDoesnt));
        }
        "they're the monarch" | "that player is the monarch" => {
            return Some(ConditionAst::Marker(GameMarker::Monarch(
                PlayerRefAst::ThatPlayer,
            )));
        }
        "you didn't create a token this way" => {
            let tok = ObjectFilterAst {
                token: Some(true),
                ..Default::default()
            };
            return Some(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(tok),
                    action: "create".to_string(),
                },
            ))));
        }
        "you didn't draw cards this way" | "you didn't draw a card this way" => {
            return Some(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::CountDrawnThisWay(PlayerRefAst::You)),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::Const(0)),
            });
        }
        "you didn't put the revealed card onto the battlefield this way" => {
            return Some(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "battlefield".to_string(),
            }));
        }
        "a player is dealt damage this way" => {
            return Some(ConditionAst::Marker(GameMarker::DealtDamageThisWay {
                combat_only: false,
            }));
        }
        "a creature dealt damage by ~ this turn died" => {
            let mut cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            cr.flags = cr
                .flags
                .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
            return Some(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(cr),
                    action: "dies".to_string(),
                },
            ));
        }
        _ => {}
    }

    // "<subj> entered from <zone> or was cast from <zone>"
    if let Some((_, rest)) = low.split_once(" entered from ").and_then(|(l, r)| {
        r.split_once(" or was cast from ")
            .map(|(z1, z2)| ((l, z1), z2))
    }) {
        let zone = match rest.trim() {
            "exile" => Some(ZoneFilter::Exile),
            "your library" => Some(ZoneFilter::YourLibrary),
            "a library" => Some(ZoneFilter::AnyLibrary),
            "your graveyard" => Some(ZoneFilter::YourGraveyard),
            "a graveyard" => Some(ZoneFilter::AnyGraveyard),
            _ => None,
        };
        if let Some(z) = zone {
            return Some(ConditionAst::Marker(GameMarker::EnteredOrCastFromZone(z)));
        }
    }

    // "<player>'s speed is greater than each other player's speed"
    if let Some(who_raw) = low.strip_suffix("'s speed is greater than each other player's speed")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(ConditionAst::Marker(GameMarker::SpeedGreaterThanEachOther(
            player,
        )));
    }

    // "<player> reveals exactly the chosen number of cards of the chosen color"
    if let Some(who_raw) =
        low.strip_suffix(" reveals exactly the chosen number of cards of the chosen color")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(ConditionAst::Marker(
            GameMarker::RevealedExactCountAndColor { player },
        ));
    }

    // "<N> cards that share a card type were milled this way"
    if let Some(cnt_raw) = low.strip_suffix(" cards that share a card type were milled this way")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Marker(GameMarker::MilledCardsShareCardType {
            min_count: u8::try_from(n).unwrap_or(2),
        }));
    }

    // "exactly <N> colors of mana were spent to activate this ability"
    if let Some(after) = low.strip_prefix("exactly ")
        && let Some(cnt_raw) =
            after.strip_suffix(" colors of mana were spent to activate this ability")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Marker(
            GameMarker::ColorsSpentToActivateExact(u8::try_from(n).unwrap_or(3)),
        ));
    }

    // "<flt> under your control was destroyed this turn by a spell or ability an opponent controlled"
    if let Some(flt_raw) = low.strip_suffix(
        " under your control was destroyed this turn by a spell or ability an opponent controlled",
    ) && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(
            GameMarker::PermanentsDestroyedByOpponentThisTurn {
                filter: Box::new(flt),
            },
        ));
    }

    // "<subj> was attached to <flt>"
    if let Some((subj_raw, host_raw)) = low.split_once(" was attached to ")
        && let (Ok(subj), Ok(host_flt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_object_filter(host_raw.trim(), vocab),
        )
    {
        return Some(ConditionAst::Marker(GameMarker::WasAttachedTo {
            subject: Box::new(subj),
            host_filter: Box::new(host_flt),
        }));
    }

    // "the ring has tempted you <N> or more times this game"
    if let Some(after) = low.strip_prefix("the ring has tempted you ")
        && let Some(cnt_raw) = after.strip_suffix(" or more times this game")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Marker(
            GameMarker::RingTemptedMinTimesThisGame(u8::try_from(n).unwrap_or(2)),
        ));
    }

    // "<subj> is your ring-bearer"
    if let Some(subj_raw) = low.strip_suffix(" is your ring-bearer")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(GameMarker::SubjectIsRingBearer(
            Box::new(subj),
        )));
    }

    // "fewer than <N> cards were discarded this way"
    if let Some(after) = low.strip_prefix("fewer than ")
        && let Some(cnt_raw) = after.strip_suffix(" cards were discarded this way")
        && let Some(n) = parse_number_word(cnt_raw.trim())
        && n > 0
    {
        return Some(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::ZoneActionCountThisWay {
                action: PriorActionRefKind::DiscardedThisWay,
                min_count: u16::try_from(n).unwrap_or(2),
            },
        ))));
    }

    // "you chose <flt> this way"
    if let Some(after) = low.strip_prefix("you chose ")
        && let Some(flt_raw) = after.strip_suffix(" this way")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(
            GameMarker::ZoneActionHappenedThisWay {
                filter: Box::new(flt),
                action: "choose".to_string(),
            },
        ));
    }

    // "<player> discards <flt> this way"
    if let Some((who_raw, rest)) = low.split_once(" discards ")
        && let Some(flt_raw) = rest.strip_suffix(" this way")
        && parse_player_ref(who_raw.trim()).is_some()
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::Marker(
            GameMarker::ZoneActionHappenedThisWay {
                filter: Box::new(flt),
                action: "discard".to_string(),
            },
        ));
    }

    // "<flt> in your graveyard has <kw>" (Kathril, Aspect Warper)
    if let Some((flt_raw, kw_raw)) = low.split_once(" in your graveyard has ")
        && let (Ok(mut flt), Some(kws)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_keyword_list(kw_raw.trim(), vocab),
        )
    {
        flt.zone = Some(ZoneFilter::YourGraveyard);
        flt.required_keywords.extend(kws);
        return Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "you had <N> or more <flt> enter the battlefield under your control this turn" (Park Heights Pegasus)
    if let Some(after) = low.strip_prefix("you had ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) =
            rest.strip_suffix(" enter the battlefield under your control this turn")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Some(ConditionAst::EnteredUnderYourControlThisTurn {
            filter: Box::new(flt),
            min_count: u16::try_from(n.max(1)).unwrap_or(1),
        });
    }

    // "<player> own(s) <N> or more <flt>" (Etrata, the Silencer)
    if let Some((who_raw, rest)) = low.split_once(" own ").or_else(|| low.split_once(" owns "))
        && let Some((cnt_raw, flt_raw)) = rest.split_once(" or more ")
        && let (Some(_player), Some(n), Ok(flt)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(1)).unwrap_or(1),
        });
    }

    // "its mana value is equal to <val>" (Glissa Sunseeker)
    if let Some(val_raw) = low.strip_prefix("its mana value is equal to ")
        && let Ok(rhs) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                stat: StatKind::ManaValue,
            }),
            op: CompareOp::Eq,
            rhs: Box::new(rhs),
        });
    }

    // Compositional conjunction "<c1> and <c2>" when both sides are recognized conditions
    if !low.contains(" and/or ")
        && let Some((c1, c2)) = low.split_once(" and ")
        && !c1.trim().is_empty()
        && !c2.trim().is_empty()
        && let (Some(lhs), Some(rhs)) = (
            parse_wave_e8_condition_atom(c1.trim(), vocab),
            parse_wave_e8_condition_atom(c2.trim(), vocab),
        )
    {
        return Some(ConditionAst::And(Box::new(lhs), Box::new(rhs)));
    }

    None
}

fn parse_wave_e8_condition_atom(low: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    if low == "~ was cast from your hand" || low == "you cast it from your hand" {
        return Some(ConditionAst::WasCast {
            by_you: true,
            from_hand: true,
        });
    }
    if !low.contains(" and ") {
        parse_wave_e8_condition(low, vocab)
    } else {
        None
    }
}

/// Strictly parse an intervening-`if`, resolution-`if`, or `as long as` condition.
pub fn parse_condition(phrase: &str, vocab: &Vocabulary) -> Result<ConditionAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_condition".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = low_full
        .strip_suffix(" as you cast ~")
        .or_else(|| low_full.strip_suffix(" as you cast this spell"))
        .unwrap_or(&low_full)
        .to_string();

    // Wave E6.2: Compound "if <c1> and if <c2>" / "if <c1> or if <c2>" conditions
    if let Some((c1, c2)) = low.split_once(" and if ")
        && let (Ok(lhs), Ok(rhs)) = (parse_condition(c1, vocab), parse_condition(c2, vocab))
    {
        return Ok(ConditionAst::And(Box::new(lhs), Box::new(rhs)));
    }
    if let Some((c1, c2)) = low.split_once(" or if ")
        && let (Ok(lhs), Ok(rhs)) = (parse_condition(c1, vocab), parse_condition(c2, vocab))
    {
        return Ok(ConditionAst::Or(Box::new(lhs), Box::new(rhs)));
    }
    if let Some(val_raw) = low.strip_prefix("the result is equal to or less than ")
        && let Ok(rhs) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PriorResult),
            op: CompareOp::Le,
            rhs: Box::new(rhs),
        });
    }

    // Wave E8 compositional typed conditions (GameMarker and Compare)
    if let Some(cond) = parse_wave_e8_condition(&low, vocab) {
        return Ok(cond);
    }

    match low.as_str() {
        "~'s additional cost was paid" => {
            return Ok(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: None,
            }));
        }
        "you revealed a dragon card or chose a dragon" => {
            return Ok(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: Some("revealed_or_chose_dragon".to_string()),
            }));
        }
        "you've been dealt damage by two or more creatures this turn" => {
            let cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(GameMarker::PlayerDealtDamageBy {
                player: PlayerRefAst::You,
                source_filter: Box::new(cr),
                min_sources: 2,
                combat_only: false,
                this_game: false,
            }));
        }
        "an opponent had three or more cards put into their graveyard from anywhere this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::PlayerHadCardsPutIntoGraveyardThisTurn {
                    player: PlayerRefAst::AnyOpponent,
                    min_count: 3,
                },
            ));
        }
        "it's the first instant spell, the first sorcery spell, or the first otter spell other than ~ you've cast this turn" =>
        {
            let inst = ObjectFilterAst {
                any_types: CardTypeMask::INSTANT,
                any_spell: true,
                ..Default::default()
            };
            let sorc = ObjectFilterAst {
                any_types: CardTypeMask::SORCERY,
                any_spell: true,
                ..Default::default()
            };
            let otter = ObjectFilterAst {
                subtypes: vec!["Otter".to_string()],
                any_spell: true,
                flags: ObjectFilterFlags::EXCLUDE_SELF,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::IsNthSpellMatchingThisTurn {
                    nth: 1,
                    filters: vec![inst, sorc, otter],
                },
            ));
        }
        "the top card of target player's graveyard is a creature card" => {
            let cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(GameMarker::TopCardOfZoneMatches {
                zone: ZoneFilter::TargetPlayerGraveyard,
                filter: Box::new(cr),
            }));
        }
        "you discarded the card with the greatest mana value among those cards or tied for greatest" =>
        {
            return Ok(ConditionAst::Marker(GameMarker::PlayerActionExtremeStat {
                player: PlayerRefAst::You,
                action: PriorActionRefKind::DiscardedThisWay,
                stat: StatKind::ManaValue,
                greatest: true,
                allow_tie: true,
            }));
        }
        "you've cycled a card named ~ four or more times this game" => {
            return Ok(ConditionAst::Marker(
                GameMarker::CycledCardNamedTimesThisGame {
                    name: NameFilterAst::SelfName,
                    min_times: 4,
                },
            ));
        }
        "you reveal three cards with different names" => {
            return Ok(ConditionAst::Marker(GameMarker::RevealedDistinctNames {
                min_count: 3,
            }));
        }
        "seven or more {e} was paid this way" => {
            return Ok(ConditionAst::Marker(GameMarker::EnergyPaidThisWayAtLeast(
                7,
            )));
        }
        "eight or more cards were returned to your hand this way" => {
            return Ok(ConditionAst::Marker(GameMarker::ZoneActionCountThisWay {
                action: PriorActionRefKind::ReturnedToHandThisWay,
                min_count: 8,
            }));
        }
        "there were three or more card types among the sacrificed permanents" => {
            let sac_perm = ObjectFilterAst {
                any_permanent: true,
                predicates: vec![FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::SacrificedThisWay,
                )],
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(GameMarker::DistinctCardTypesAmong {
                filter: Box::new(sac_perm),
                min_count: 3,
            }));
        }
        "the two spells have the same mana value" => {
            return Ok(ConditionAst::Marker(
                GameMarker::SameManaValueAsTriggerSpell,
            ));
        }
        "a white creature dies this way" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_colors: ColorMask::WHITE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "dies".to_string(),
                },
            ));
        }
        "a card is put into exile this way" => {
            let flt = ObjectFilterAst {
                any_card: true,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "exiled".to_string(),
                },
            ));
        }
        "a creature is put into exile this way" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "exiled".to_string(),
                },
            ));
        }
        "that creature was destroyed this way" => {
            let flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "destroyed".to_string(),
                },
            ));
        }
        "target attacking creature token is the chosen type" => {
            let subj_flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                combat: Some(CombatState::Attacking),
                token: Some(true),
                ..Default::default()
            };
            let chosen_flt = ObjectFilterAst {
                flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(subj_flt),
                    count: TargetCountAst::Exact(1),
                }),
                filter: Box::new(chosen_flt),
            });
        }
        "the revealed card is the chosen type" => {
            let chosen_flt = ObjectFilterAst {
                flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("the revealed card".to_string())),
                filter: Box::new(chosen_flt),
            });
        }
        "its controller has more than four cards in hand" => {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::ItsController,
                op: CompareOp::Gt,
                value: 4,
            });
        }
        "it doesn't have a counter of that kind on it" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::CounterCompare {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "same".to_string(),
                op: CompareOp::Ge,
                value: 1,
            })));
        }
        "it has the least power or is tied for least power among creatures on the battlefield" => {
            let all_cr = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                zone: Some(ZoneFilter::Battlefield),
                ..Default::default()
            };
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::Power,
                }),
                op: CompareOp::Le,
                rhs: Box::new(ValueExprAst::ExtremeStatAmong {
                    stat: StatKind::Power,
                    greatest: false,
                    filter: Box::new(all_cr),
                    or_toughness: false,
                }),
            });
        }
        "cards with five or more different mana values are exiled with ~" => {
            let exiled_flt = ObjectFilterAst {
                any_card: true,
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            };
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::DistinctCountAmong {
                    category: "mana_values".to_string(),
                    filter: Box::new(exiled_flt),
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(5)),
            });
        }
        "that player or that planeswalker's controller has more life than you" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::ThatPlayerOrPlaneswalkerController,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
            });
        }
        "the player hasn't played the card" => {
            return Ok(ConditionAst::Marker(GameMarker::HasNotPlayedOrCast));
        }
        "it's controlled by the chosen player" => {
            let flt = ObjectFilterAst {
                controller: Some(ControllerRelation::ChosenPlayer),
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(flt),
            });
        }
        "it targets you or a permanent you control" => {
            let your_perm = ObjectFilterAst {
                any_permanent: true,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            };
            let flt = ObjectFilterAst {
                predicates: vec![FilterPredicateAst::TargetsSubject(Box::new(
                    SubjectRefAst::Compound(vec![
                        SubjectRefAst::Player(PlayerRefAst::You),
                        SubjectRefAst::Group(Box::new(your_perm)),
                    ]),
                ))],
                ..Default::default()
            };
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(flt),
            });
        }
        "it is" => {
            return Ok(ConditionAst::Marker(GameMarker::SameNameAsTarget));
        }
        "~ remains tapped and that creature's power remains less than or equal to ~'s power" => {
            return Ok(ConditionAst::And(
                Box::new(ConditionAst::Marker(GameMarker::RemainsTapped)),
                Box::new(ConditionAst::Compare {
                    lhs: Box::new(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                        stat: StatKind::Power,
                    }),
                    op: CompareOp::Le,
                    rhs: Box::new(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::SelfObject),
                        stat: StatKind::Power,
                    }),
                }),
            ));
        }
        "~ was kicked" | "it was kicked" => return Ok(ConditionAst::WasKicked),
        "you cast it" | "you cast ~" => {
            return Ok(ConditionAst::WasCast {
                by_you: true,
                from_hand: false,
            });
        }
        "~ was cast" | "it was cast" => {
            return Ok(ConditionAst::WasCast {
                by_you: false,
                from_hand: false,
            });
        }
        "you cast it from your hand" | "~ was cast from your hand" => {
            return Ok(ConditionAst::WasCast {
                by_you: true,
                from_hand: true,
            });
        }
        "no spells were cast last turn" => {
            return Ok(ConditionAst::SpellsCastLastTurn {
                any_player_single: false,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "a player cast two or more spells last turn" => {
            return Ok(ConditionAst::SpellsCastLastTurn {
                any_player_single: true,
                op: CompareOp::Ge,
                value: 2,
            });
        }
        "you search your library this way" => return Ok(ConditionAst::SearchedLibraryThisWay),
        "it's not a token"
        | "it isn't a token"
        | "that creature isn't a token"
        | "~ isn't a token" => {
            return Ok(ConditionAst::IsNotToken);
        }
        "a creature died this turn" => {
            return Ok(ConditionAst::Morbid {
                your_control_only: false,
            });
        }
        "a creature died under your control this turn" => {
            return Ok(ConditionAst::Morbid {
                your_control_only: true,
            });
        }
        "you have no cards in hand" | "you have no cards in your hand" => {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "it's your turn" | "it is your turn" => return Ok(ConditionAst::IsYourTurn),
        "it's not your turn" | "it isn't your turn" | "it's an opponent's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)));
        }
        "you gained life this turn" | "you've gained life this turn" => {
            return Ok(ConditionAst::GainedLifeThisTurn);
        }
        "an opponent lost life this turn" | "opponents lost life this turn" => {
            return Ok(ConditionAst::OpponentLostLifeThisTurn);
        }
        "you attacked this turn" | "you attacked with a creature this turn" => {
            return Ok(ConditionAst::AttackedThisTurn);
        }
        "~ entered this turn" | "~ entered the battlefield this turn" => {
            return Ok(ConditionAst::EnteredThisTurn);
        }
        "you control your commander"
        | "you control a commander"
        | "you control a commander as you cast ~" => {
            return Ok(ConditionAst::ControlsCommander);
        }
        "tribute wasn't paid" => return Ok(ConditionAst::TributeNotPaid),
        "tribute was paid" => return Ok(ConditionAst::Not(Box::new(ConditionAst::TributeNotPaid))),
        "it shares a creature type with ~" => return Ok(ConditionAst::SharesTypeWithSelf),
        "you descended this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DescendedThisTurn));
        }
        "you have the city's blessing" => {
            return Ok(ConditionAst::Marker(GameMarker::CitysBlessing));
        }
        "you're the monarch" | "you are the monarch" => {
            return Ok(ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)));
        }
        "defending player is the monarch" => {
            return Ok(ConditionAst::Marker(GameMarker::Monarch(
                PlayerRefAst::DefendingPlayer,
            )));
        }
        "there is no monarch" => return Ok(ConditionAst::Marker(GameMarker::NoMonarch)),
        "you've completed a dungeon" => {
            return Ok(ConditionAst::Marker(GameMarker::CompletedDungeon));
        }
        "it's day" => return Ok(ConditionAst::Marker(GameMarker::Day)),
        "it's night" => return Ok(ConditionAst::Marker(GameMarker::Night)),
        "it's neither day nor night" => {
            return Ok(ConditionAst::Marker(GameMarker::NeitherDayNorNight));
        }
        "you have max speed" => return Ok(ConditionAst::Marker(GameMarker::MaxSpeed)),
        "you have the initiative" => {
            return Ok(ConditionAst::Marker(GameMarker::Initiative(
                PlayerRefAst::You,
            )));
        }
        "you committed a crime this turn" | "you've committed a crime this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn));
        }
        "you gained or lost life this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::PlayerGainedOrLostLifeThisTurn {
                    player: PlayerRefAst::You,
                },
            ));
        }
        "a player lost life this turn" => {
            return Ok(ConditionAst::LostLifeThisTurn {
                player: PlayerRefAst::AnyPlayer,
                min_amount: 1,
            });
        }
        "an opponent was dealt damage this turn"
        | "an opponent has been dealt damage this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: false,
                },
            ));
        }
        "an opponent was dealt noncombat damage this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: true,
                },
            ));
        }
        "you attacked with two or more creatures this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedWithMinCreaturesThisTurn {
                    player: PlayerRefAst::You,
                    min_count: 2,
                },
            ));
        }
        "a creature entered the battlefield under your control this turn"
        | "a creature entered under your control this turn" => {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                min_count: 1,
            });
        }
        "you cast a spell this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value: 1,
                from_hand_only: false,
            }));
        }
        "you've cast two or more spells this turn" | "you cast two or more spells this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value: 2,
                from_hand_only: false,
            }));
        }
        "you haven't cast a spell from your hand this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
                from_hand_only: true,
            }));
        }
        "you've cast a historic spell this turn" => {
            return Ok(ConditionAst::CastSpellThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(ObjectFilterAst {
                    required_supertypes: SupertypeMask::HISTORIC,
                    any_spell: true,
                    ..Default::default()
                }),
            });
        }
        "this spell was cast from your graveyard"
        | "~ was cast from your graveyard"
        | "~ was cast from a graveyard"
        | "it was cast from a graveyard" => {
            return Ok(ConditionAst::Marker(GameMarker::CastFromGraveyard));
        }
        "~ was cast from exile" => return Ok(ConditionAst::Marker(GameMarker::CastFromExile)),
        "~ was foretold" | "it was foretold" => {
            return Ok(ConditionAst::Marker(GameMarker::Foretold));
        }
        "~ wasn't cast from your hand"
        | "~ was cast from anywhere other than your hand"
        | "you didn't cast it from your hand" => {
            return Ok(ConditionAst::Marker(GameMarker::CastFromOutsideHand));
        }
        "~ attacked or blocked this combat" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedOrBlockedThisCombat,
            ));
        }
        "an opponent owns a card in exile" => {
            return Ok(ConditionAst::Marker(GameMarker::OwnerHasCardInExile {
                player: PlayerRefAst::AnyOpponent,
            }));
        }
        "you own a card in exile" => {
            return Ok(ConditionAst::Marker(GameMarker::OwnerHasCardInExile {
                player: PlayerRefAst::You,
            }));
        }
        "defending player is poisoned" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
                player: PlayerRefAst::DefendingPlayer,
            }));
        }
        "its controller is poisoned" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
                player: PlayerRefAst::ItsController,
            }));
        }
        "defending player has a rad counter" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerHasRadCounter {
                player: PlayerRefAst::DefendingPlayer,
            }));
        }
        "there are no cards in your graveyard" => {
            return Ok(ConditionAst::GraveyardCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "you control neither creature" => {
            return Ok(ConditionAst::Marker(GameMarker::ControlsNeither));
        }
        "you control all three" => {
            return Ok(ConditionAst::Marker(GameMarker::ControlsAllThree));
        }
        "you control fewer creatures than an opponent" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsFewerThanOpponent {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                },
            ));
        }
        "you control more creatures than each other player"
        | "you control more creatures than that spell's controller" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                    than_each_other: true,
                },
            ));
        }
        "you control more creatures of that type than each other player" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                        ..Default::default()
                    }),
                    than_each_other: true,
                },
            ));
        }
        "you control more lands than defending player" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::LAND,
                        ..Default::default()
                    }),
                    than_each_other: false,
                },
            ));
        }
        "you bargained" | "~ was bargained" | "it was bargained" => {
            return Ok(ConditionAst::Marker(GameMarker::Bargained));
        }
        "the gift was promised" => return Ok(ConditionAst::Marker(GameMarker::GiftPromised)),
        "you're attacking" => return Ok(ConditionAst::Marker(GameMarker::YouAreAttacking)),
        "you're defending" => return Ok(ConditionAst::Marker(GameMarker::YouAreDefending)),
        "two or more players have lost the game" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayersLostGame {
                op: CompareOp::Ge,
                value: 2,
            }));
        }
        "you don't put the card into your hand"
        | "you didn't put a card into your hand this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "hand".to_string(),
            }));
        }
        "you don't put the card onto the battlefield"
        | "you didn't put a card onto the battlefield this way"
        | "you don't put it onto the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "battlefield".to_string(),
            }));
        }
        "a nonland permanent left the battlefield this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::NonlandPermanentLeftThisTurn,
            ));
        }
        "a spell was warped this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::SpellWarpedThisTurn));
        }
        "~ was cast using teamwork" | "it's cast using teamwork" => {
            return Ok(ConditionAst::Marker(GameMarker::CastUsingTeamwork));
        }
        "chaos gets more votes or the vote is tied" => {
            return Ok(ConditionAst::Marker(GameMarker::VoteResult {
                winning: "chaos_or_tie".to_string(),
            }));
        }
        "planeswalk gets more votes" => {
            return Ok(ConditionAst::Marker(GameMarker::VoteResult {
                winning: "planeswalk".to_string(),
            }));
        }
        "you have a full party" => return Ok(ConditionAst::Marker(GameMarker::FullParty)),
        "it's the first combat phase of the turn" => {
            return Ok(ConditionAst::Marker(GameMarker::FirstCombatPhase));
        }
        "it isn't a mana ability" => return Ok(ConditionAst::Marker(GameMarker::NotManaAbility)),
        "it's not their turn" | "it isn't that player's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsPlayersTurn(
                PlayerRefAst::ThatPlayer,
            ))));
        }
        "it's on the battlefield" | "it is on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                zone: ZoneFilter::Battlefield,
            }));
        }
        "~ is in the command zone or on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::InCommandZoneOrBattlefield));
        }
        "~ is suspended" => return Ok(ConditionAst::Marker(GameMarker::IsSuspended)),
        "~ is paired with another creature" => {
            return Ok(ConditionAst::Marker(GameMarker::PairedWithAnotherCreature));
        }
        "no creatures are on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::NoCreaturesOnBattlefield));
        }
        "it has a single target" => return Ok(ConditionAst::Marker(GameMarker::SingleTarget)),
        "it isn't being declared as an attacker" => {
            return Ok(ConditionAst::Marker(GameMarker::NotDeclaredAttacker));
        }
        "you chose a creature other than ~ as your ring-bearer" => {
            return Ok(ConditionAst::Marker(GameMarker::RingBearerOtherThanSelf));
        }
        "a card left your graveyard this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn));
        }
        "you have more cards in hand than each opponent" => {
            return Ok(ConditionAst::Marker(GameMarker::HandMoreThanEachOpponent));
        }
        "your life total is less than or equal to half your starting life total" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Le,
                rhs: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::You)),
                    denom: 2,
                    rounding: Rounding::Down,
                }),
            });
        }
        "an opponent's life total is less than half their starting life total" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Lt,
                rhs: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::AnyOpponent)),
                    denom: 2,
                    rounding: Rounding::Down,
                }),
            });
        }
        "it entered from your graveyard or you cast it from your graveyard" => {
            return Ok(ConditionAst::Marker(GameMarker::EnteredOrCastFromGraveyard));
        }
        "it shares a card type with the exiled card" => {
            return Ok(ConditionAst::Marker(GameMarker::SharesCardTypeWithExiled));
        }
        "it targets a commander" => {
            return Ok(ConditionAst::Marker(GameMarker::TargetsCommander));
        }
        "it has the same mana value as the discarded card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameManaValueAsDiscarded));
        }
        "it has the same name as that card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameNameAsTarget));
        }
        "its mana value is x" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::ManaValue,
                }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::ChosenX),
            });
        }
        "you control ~" => return Ok(ConditionAst::Marker(GameMarker::ControlsSelf)),
        "~ remains tapped" | "it remains tapped" => {
            return Ok(ConditionAst::Marker(GameMarker::RemainsTapped));
        }
        "~ remains on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::RemainsOnBattlefield));
        }
        "it's attacking alone" => return Ok(ConditionAst::Marker(GameMarker::AttackingAlone)),
        "damage is prevented this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DamagePreventedThisWay));
        }
        "you haven't cast it"
        | "you haven't played it"
        | "you don't cast it"
        | "you don't play it"
        | "you didn't cast it"
        | "it wasn't cast" => {
            return Ok(ConditionAst::Marker(GameMarker::HasNotPlayedOrCast));
        }
        "it's bargained" | "~ is bargained" => {
            return Ok(ConditionAst::Marker(GameMarker::Bargained));
        }
        "it's attacking the player with the most life or tied for most life" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackingPlayerWithMostLife { or_tied: true },
            ));
        }
        "it's attacking the player with the most life" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackingPlayerWithMostLife { or_tied: false },
            ));
        }
        "you have more life than an opponent" | "you have more life than each opponent" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent,
                    stat: PlayerStatKind::LifeTotal,
                }),
            });
        }
        "no creatures attacked this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::DidNotAttackWithCreatureThisTurn {
                    player: PlayerRefAst::AnyPlayer,
                },
            ));
        }
        "no mana was spent to cast that spell" | "no mana was spent to cast it" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ManaSpentToCast { mana_filter: None }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::Const(0)),
            });
        }
        "you won" | "you won the clash" | "you win the flip" | "you won the flip" => {
            return Ok(ConditionAst::WonContest);
        }
        "it didn't die" | "that creature didn't die" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::DiedThisWay,
            ))));
        }
        "the gift wasn't promised" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::GiftPromised,
            ))));
        }
        "it's paired with a creature" | "~ is paired with a creature" => {
            return Ok(ConditionAst::Marker(GameMarker::PairedWithAnotherCreature));
        }
        "a permanent's ability is countered this way" => {
            return Ok(ConditionAst::Marker(GameMarker::AbilityCounteredThisWay {
                permanent_only: true,
            }));
        }
        "~ attacked this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
            }));
        }
        "you've discarded a card this turn" | "you discarded a card this turn" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: "discard".to_string(),
            });
        }
        "you haven't added mana with this ability this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn {
                op: CompareOp::Eq,
                value: 0,
            }));
        }
        "it's not that player's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsPlayersTurn(
                PlayerRefAst::ThatPlayer,
            ))));
        }
        "that spell targets only ~" | "it targets only ~" => {
            return Ok(ConditionAst::Marker(GameMarker::SingleTarget));
        }
        "that card remains exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                any_of: false,
            }));
        }
        "those cards remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("those cards".to_string())),
                any_of: false,
            }));
        }
        "any of those cards remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("those cards".to_string())),
                any_of: true,
            }));
        }
        "it remains exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                any_of: false,
            }));
        }
        "they remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("they".to_string())),
                any_of: false,
            }));
        }
        "it attacked this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            }));
        }
        "it attacked or blocked this turn" | "that creature attacked or blocked this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedOrBlockedThisTurn {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                },
            ));
        }
        "that creature dies this way"
        | "it dies this way"
        | "it died this way"
        | "a creature died this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DiedThisWay));
        }
        "~ didn't attack or come under your control this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
                or_entered_this_turn: true,
            }));
        }
        "~ didn't attack this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
                or_entered_this_turn: false,
            }));
        }
        "one or more cards left your graveyard this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn));
        }
        "one or more of them entered from a graveyard or was cast from a graveyard" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AnyEnteredOrCastFromGraveyard,
            ));
        }
        "life was paid to activate it" => {
            return Ok(ConditionAst::Marker(GameMarker::LifePaidToActivate));
        }
        "that player has another opponent who isn't being attacked" => {
            return Ok(ConditionAst::Marker(GameMarker::HasUnattackedOpponent));
        }
        "it has greater power or toughness than ~" => {
            return Ok(ConditionAst::Marker(
                GameMarker::GreaterPowerOrToughnessThan {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    other: Box::new(SubjectRefAst::SelfObject),
                },
            ));
        }
        "this is the first time this ability has resolved this turn" | "it's the first time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(1),
            ));
        }
        "this is the second time this ability has resolved this turn" | "it's the second time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(2),
            ));
        }
        "this is the third time this ability has resolved this turn" | "it's the third time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(3),
            ));
        }
        "you cast ~ during your main phase" | "you cast it during your main phase" => {
            return Ok(ConditionAst::Marker(GameMarker::CastDuringMainPhase));
        }
        "it escaped" | "~ escaped" => {
            return Ok(ConditionAst::Marker(GameMarker::Escaped));
        }
        "it has the same mana value as the revealed card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameManaValueAsRevealed));
        }
        "evidence was collected" => {
            return Ok(ConditionAst::Marker(GameMarker::EvidenceCollected));
        }
        "you lost life last turn" => {
            return Ok(ConditionAst::Marker(GameMarker::LostLifeLastTurn {
                player: PlayerRefAst::You,
            }));
        }
        "~ attacked this combat" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisCombat {
                subject: Box::new(SubjectRefAst::SelfObject),
            }));
        }
        "the amount of mana spent to cast it was less than its mana value"
        | "the amount of mana spent to cast ~ was less than its mana value" => {
            return Ok(ConditionAst::Marker(GameMarker::ManaSpentLessThanManaValue));
        }
        "they entered or were cast from a graveyard" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AnyEnteredOrCastFromGraveyard,
            ));
        }
        "you didn't attack with a creature this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::DidNotAttackWithCreatureThisTurn {
                    player: PlayerRefAst::You,
                },
            ));
        }
        "you didn't cast a spell this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
                player: PlayerRefAst::You,
                filter: None,
            }));
        }
        "an opponent has more life than you" => {
            return Ok(ConditionAst::OpponentMoreThanYou {
                metric: "life".to_string(),
                filter: None,
            });
        }
        "an opponent has more cards in hand than you" => {
            return Ok(ConditionAst::OpponentMoreThanYou {
                metric: "cards_in_hand".to_string(),
                filter: None,
            });
        }
        "~ is on the stack" => {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: "on_stack".to_string(),
            });
        }
        "~ didn't enter the battlefield this turn" | "~ didn't enter this turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::EnteredThisTurn)));
        }
        "a creature is attacking you" => {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::EachOpponent,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            });
        }
        "a player attacked you during their last turn"
        | "that player attacked you during their last turn" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::ThatPlayer,
                action: "attacked_you_last_turn".to_string(),
            });
        }
        "an opponent was dealt noncombat damage last turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: true,
                },
            ));
        }
        "excess damage was dealt this way"
        | "it's greater"
        | "that spell is countered this way"
        | "you have an enduring story"
        | "that creature attacked this turn"
        | "it's a spell with lesser mana value"
        | "the amount of mana spent to cast that spell was less than its mana value"
        | "a card is exiled with it"
        | "a card is exiled with ~"
        | "it was cast from your graveyard"
        | "they were cast using web-slinging"
        | "~ was cast using web-slinging"
        | "you have a card in hand"
        | "it was the second spell you cast this turn"
        | "it was blocked this turn"
        | "~ was dealt damage this turn"
        | "it was dealt damage this turn"
        | "it dealt combat damage to a player this turn"
        | "an opponent lost life last turn"
        | "they aren't attacking you"
        | "that player is you"
        | "it entered under your control"
        | "that cost wasn't paid"
        | "the copy is a permanent spell"
        | "it's your first, second, or third turn of the game"
        | "an opponent is poisoned"
        | "you've activated a loyalty ability this turn"
        | "~ attacked or blocked this turn"
        | "it was attacking or blocking alone"
        | "an aura you controlled was attached to it"
        | "you didn't play a land this turn"
        | "you didn't lose life this turn"
        | "you didn't play a card from exile this turn"
        | "you put a counter on ~ this turn"
        | "a counter was put on ~ this turn"
        | "you put a counter on a creature this turn"
        | "~ dealt damage to an opponent this turn"
        | "~ dealt damage to it this turn"
        | "no creatures died this turn"
        | "you attacked with exactly one other creature this combat"
        | "it's the second creature spell you cast this turn"
        | "~ is in your graveyard or on the battlefield"
        | "you're the defending player"
        | "enchanted equipment is attached to a creature"
        | "it didn't attack this turn"
        | "an opponent searched their library this turn"
        | "three or more creatures attacked this turn"
        | "there are five colors among permanents you control"
        | "it devoured a creature"
        | "it hasn't dealt damage yet"
        | "your team gained life this turn"
        | "you've drawn more than one card this turn"
        | "one or more cards were put into exile this turn"
        | "no permanents left the battlefield this turn"
        | "a creature died under an opponent's control this turn"
        | "a creature card left your graveyard this turn"
        | "its power was different from its base power"
        | "that creature was dealt excess damage this turn"
        | "you haven't been dealt combat damage since your last turn"
        | "no other creatures are attacking that player"
        | "you were the monarch as the turn began"
        | "they attacked you and/or a planeswalker you control"
        | "it has the same name as one of the cards exiled with ~"
        | "~ is in your graveyard with a creature card directly above it"
        | "another creature died this turn"
        | "no cards are in that graveyard"
        | "you have less life than an opponent"
        | "its mana value is odd"
        | "its mana value is even"
        | "an opponent protects it"
        | "~ is your ring-bearer"
        | "enchanted creature is attacking alone"
        | "you've visited an attraction this turn"
        | "~ hasn't dealt damage yet"
        | "you cast it from anywhere other than your hand"
        | "that creature has greater power or toughness than ~"
        | "that player controls more lands than each other player"
        | "you removed two or more loyalty counters to activate it"
        | "you don't draw a card this way"
        | "that card has the chosen name"
        | "that land was nonbasic"
        | "exactly one creature is attacking"
        | "a land you controlled was put into a graveyard from the battlefield this turn"
        | "you or a player you're attacking has the initiative"
        | "two or more of those creatures are attacking you and/or planeswalkers you control"
        | "it enlisted a creature this combat"
        | "that player had another land enter the battlefield under their control this turn"
        | "excess damage was dealt to that creature this way"
        | "excess damage was dealt to a creature this way"
        | "excess damage was dealt to that permanent this way"
        | "the creature the opponent controls is dealt excess damage this way"
        | "~ attacked a battle this turn"
        | "it attacked during its controller's last turn"
        | "it attacked during your last turn"
        | "~'s power is even"
        | "~'s power is odd"
        | "that artifact is put into a graveyard this way"
        | "that creature is put into a graveyard this way"
        | "~ is still on the battlefield"
        | "they control two or more basic lands"
        | "they control a commander"
        | "an opponent has been dealt noncombat damage this turn"
        | "you've scried or surveilled this turn"
        | "~ is on the battlefield or in your graveyard"
        | "an opponent was dealt combat damage by a legendary creature this turn"
        | "it's a land or double-faced card"
        | "three or more creature cards are above ~"
        | "you've paid or lost four or more {e} this turn"
        | "red sources you controlled dealt 4 or more noncombat damage this turn"
        | "~ blocked or was blocked by a blue creature this turn"
        | "you've rolled three or more dice this turn"
        | "you've been attacked this step"
        | "you're attacking two or more opponents"
        | "at least one creature is blocking ~"
        | "ten or more lands are on the battlefield"
        | "there are twenty-five or more loyalty counters among jaces you control"
        | "mana from a treasure was spent to cast it or activate it"
        | "two or more equipment are attached to it"
        | "an enchantment was put into your graveyard from the battlefield this turn"
        | "~ regenerated this turn"
        | "it shares a card type with the card exiled this way"
        | "no player does"
        | "the player doesn't"
        | "you win"
        | "you paid one or more {e} this way"
        | "one or more +1/+1 counters are moved this way"
        | "two cards that share a card type are discarded this way"
        | "that creature was cast for its warp cost"
        | "that card is returned to its owner's hand this way"
        | "you gain control of a creature this way"
        | "a creature spell you cast this turn was countered by a spell or ability an opponent controlled"
        | "a player other than you lost life this turn"
        | "an enchantment is on the battlefield"
        | "excess damage was dealt to a permanent this way"
        | "~ is enchanted, equipped, and has a counter on it"
        | "this is the first or second time this ability has resolved this turn"
        | "they control fewer lands than you"
        | "the discovered card's mana value is less than 10"
        | "that creature shares a color with the mana that land produced"
        | "that spell targets a commander you control"
        | "you control two or more nonland, nontoken permanents with the same name as one another"
        | "your library has no cards in it"
        | "you control more creatures than attacking player"
        | "you control more lands than attacking player" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: low,
            });
        }
        "they're not a token" | "they aren't a token" => {
            return Ok(ConditionAst::IsNotToken);
        }
        "~ is in the command zone" => {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: "in_command_zone".to_string(),
            });
        }
        "it wasn't cast this way" | "you don't cast that card this way" => {
            return Ok(ConditionAst::Not(Box::new(
                ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: "cast_this_way".to_string(),
                },
            )));
        }
        "you haven't completed tomb of annihilation" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::CompletedDungeon,
            ))));
        }
        _ => {}
    }

    if let Some(after_atk) = low.strip_prefix("you attacked with ")
        && let Some(cnt_str) = after_atk.strip_suffix(" or more creatures this turn")
        && let Some(n) = parse_number_word(cnt_str.trim())
        && n > 0
    {
        return Ok(ConditionAst::Marker(
            GameMarker::AttackedWithMinCreaturesThisTurn {
                player: PlayerRefAst::You,
                min_count: u16::try_from(n).unwrap_or(2),
            },
        ));
    }
    if low.starts_with("it's blocking or blocked by ")
        || low.starts_with("at least one ") && low.ends_with(" is milled this way")
        || low.starts_with("you won ") && low.ends_with(" flips this way")
        || low.starts_with("target player has fewer than ") && low.ends_with(" counters")
        || low.starts_with("you put fewer than ") && low.ends_with(" this way")
        || low.starts_with("those creatures have total power ") && low.ends_with(" or greater")
        || low.starts_with("an opponent has at least ")
            && low.ends_with(" more cards in hand than you")
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if let Some(flt_raw) = low
        .strip_suffix(" is in your graveyard")
        .or_else(|| low.strip_suffix(" is in a graveyard"))
        && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
    {
        flt.zone = Some(if low.ends_with(" is in a graveyard") {
            ZoneFilter::AnyGraveyard
        } else {
            ZoneFilter::YourGraveyard
        });
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // Orthogonal negated subtype/status/mana-spent/keyword conditions:
    // "it's not a <flt>", "it's not <status>", "<mana> wasn't spent to cast it/~", "it didn't have <kw>"
    if let Some(flt_raw) = low
        .strip_prefix("it's not ")
        .or_else(|| low.strip_prefix("it isn't "))
    {
        if matches!(
            flt_raw,
            "suspected"
                | "tapped"
                | "untapped"
                | "monstrous"
                | "renowned"
                | "saddled"
                | "crewed"
                | "goaded"
                | "blocked"
        ) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                status: flt_raw.to_string(),
            })));
        }
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(flt),
            })));
        }
    }
    if let Some(spent) = low
        .strip_suffix(" wasn't spent to cast it")
        .or_else(|| low.strip_suffix(" wasn't spent to cast ~"))
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
            spent.trim().to_string(),
        ))));
    }
    if let Some(kw) = low.strip_prefix("it didn't have ")
        && parse_keyword_list(kw, vocab).is_some()
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            keyword: kw.to_string(),
        })));
    }
    if let Some(after) = low
        .strip_prefix("there's ")
        .or_else(|| low.strip_prefix("there is "))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        let flt_clean = flt_raw.replace(" total in all graveyards", " in all graveyards");
        if let Ok(flt) = parse_object_filter(&flt_clean, vocab) {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }
    if let Some((who_raw, flt_raw)) = low
        .split_once(" controls no ")
        .or_else(|| low.split_once(" control no "))
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some((who_raw, rest)) = low.split_once(" controls more ")
        && let Some(flt_raw) = rest.strip_suffix(" than you")
        && let (Some(_player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::OpponentMoreThanYou {
            metric: "permanents".to_string(),
            filter: Some(Box::new(flt)),
        });
    }
    if low.starts_with("you roll a ")
        || low.ends_with(" was beheld")
        || low.ends_with(" crewed it this turn")
        || low.ends_with(" attacked this combat")
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "<N> or more creatures died [under your control] this turn" / "<N> or more cards have been exiled with ~" / "<N> or more <flt> are in your graveyard"
    if let Some((cnt_raw, tail)) = low.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        if tail == "creatures died this turn"
            || tail == "creatures died under your control this turn"
        {
            let creature_flt = ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            };
            return Ok(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(creature_flt),
                from_zone: Some(ZoneFilter::Battlefield),
                to_zone: Some(ZoneFilter::AnyGraveyard),
                controller: if tail.contains("under your control") {
                    Some(PlayerRefAst::You)
                } else {
                    None
                },
                min_count: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
        if tail == "cards have been exiled with ~" {
            let exiled_flt = ObjectFilterAst {
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            };
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(exiled_flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
        if let Some(flt_raw) = tail.strip_suffix(" are in your graveyard")
            && let Ok(flt) = parse_object_filter(&format!("{flt_raw} in your graveyard"), vocab)
        {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }
    // "there are fewer than <N> <flt>" (Shadowborn Demon)
    if let Some(after) = low.strip_prefix("there are fewer than ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Lt,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    // "you sacrificed <N> or more <flt> this turn" / "<player> was dealt <N> or more [combat] damage this turn" / "you've been dealt <N> or more damage this turn" / "it has dealt <N> or more damage to that player this turn"
    if (low.starts_with("you sacrificed ") && low.ends_with(" this turn"))
        || (low.contains(" was dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("you've been dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("it has dealt ") && low.ends_with(" damage to that player this turn"))
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "you have <N> or more cards in your library" / "you have exactly <N> cards in your hand"
    // Cluster 13 & 20: Coin-flip outcomes, player/card tie comparisons, and Harnessed state
    if matches!(
        low.as_str(),
        "you win all the flips"
            | "you won all the flips"
            | "you lose a flip"
            | "you lost a flip"
            | "you lose the flip"
            | "you lost the flip"
            | "you win the flip"
            | "you won the flip"
    ) || (low.starts_with("you win ") && low.ends_with(" or more flips"))
        || (low.starts_with("you won ") && low.ends_with(" or more flips"))
    {
        return Ok(ConditionAst::CoinFlipOutcome { outcome: low });
    }
    if let Some(after_tie) = low.strip_prefix("two or more ")
        && let Some((_, metric)) = after_tie.split_once(" are tied for ")
    {
        return Ok(ConditionAst::PlayersTiedFor {
            metric: metric.trim().to_string(),
            min_tied: 2,
        });
    }
    if low
        == "each of those creatures could be blocked by all creatures that the other is blocked by"
    {
        return Ok(ConditionAst::CanLegallySwapBlockAssignments {
            subjects: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
            perspective: CombatSwapPerspective::BlockedAttackers,
        });
    }
    if low == "each of those creatures could block all creatures that the other is blocking" {
        return Ok(ConditionAst::CanLegallySwapBlockAssignments {
            subjects: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
            perspective: CombatSwapPerspective::BlockingCreatures,
        });
    }
    if low == "the chosen card is cast as a spell" {
        return Ok(ConditionAst::Marker(GameMarker::ChosenCardCastAsSpell));
    }
    if let Some(subj_raw) = low.strip_suffix(" is harnessed")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectHarnessed(
            Box::new(subj),
        )));
    }

    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(cnt_raw) = after.strip_suffix(" or more cards in your library")
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::CardsInLibrary,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some(rest) = after.strip_prefix("exactly ")
            && let Some(cnt_raw) = rest
                .strip_suffix(" cards in your hand")
                .or_else(|| rest.strip_suffix(" cards in hand"))
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }
    // "<player> controls <flt>" (e.g. "that player controls a Plains") / "you control <subj>" / "that mana is spent on <flt>" / "that target is <colors>"
    if let Some((who_raw, flt_raw)) = low.split_once(" controls ")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    // Lossless "you control <flt> named <A> [and <B> ...]" preserving exact original-case card names
    // (Scepter of Empires, Power Plant Worker, Helm of Kaldra, Festering Newt, Ruin in Their Wake, Galactus, etc.)
    if let Some(after) = low.strip_prefix("you control ")
        && let Some((flt_raw, names_low)) = after.split_once(" named ")
        && !names_low.trim().is_empty()
        && let Ok(base_flt) = parse_object_filter(flt_raw, vocab)
    {
        let raw_base = raw.get(..low.len()).unwrap_or(raw);
        let names_start = "you control ".len() + flt_raw.len() + " named ".len();
        let names_orig = raw_base.get(names_start..).unwrap_or(names_low).trim();
        let is_singular = flt_raw.starts_with("a ")
            || flt_raw.starts_with("an ")
            || flt_raw.starts_with("another ");
        let (is_disjunction, names): (bool, Vec<String>) = if names_orig.contains(" or ") {
            (
                true,
                names_orig
                    .split(" or ")
                    .map(|p| p.trim().to_string())
                    .collect(),
            )
        } else if is_singular {
            (false, vec![names_orig.to_string()])
        } else {
            (false, split_named_card_list(names_orig))
        };
        let mut iter = names.into_iter().filter(|n| !n.is_empty());
        if let Some(first_name) = iter.next() {
            let make_cond = |name: String| {
                let mut f = base_flt.clone();
                f.named = Some(if name == "~" {
                    NameFilterAst::SelfName
                } else {
                    NameFilterAst::Exact(name)
                });
                ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter: Box::new(f),
                    op: CompareOp::Ge,
                    value: 1,
                }
            };
            let mut acc = make_cond(first_name);
            for next_name in iter {
                acc = if is_disjunction {
                    ConditionAst::Or(Box::new(acc), Box::new(make_cond(next_name)))
                } else {
                    ConditionAst::And(Box::new(acc), Box::new(make_cond(next_name)))
                };
            }
            return Ok(acc);
        }
    }
    if let Some(subj_raw) = low.strip_prefix("you control ") {
        if let Ok(flt) = parse_object_filter(subj_raw, vocab) {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: 1,
            });
        }
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            match subj {
                SubjectRefAst::Group(flt) | SubjectRefAst::All(flt) | SubjectRefAst::Each(flt) => {
                    return Ok(ConditionAst::ControlsCount {
                        player: PlayerRefAst::You,
                        filter: flt,
                        op: CompareOp::Ge,
                        value: 1,
                    });
                }
                _ => {
                    return Ok(ConditionAst::PlayerActionThisTurn {
                        player: PlayerRefAst::You,
                        action: low,
                    });
                }
            }
        }
    }
    if low.starts_with("that mana is spent on ")
        || low.starts_with("any of that mana is spent on ")
        || low.starts_with("that target is ")
        || low.starts_with("~ is enchanted by ")
        || low.starts_with("they were attacked this turn by ")
        || (low.starts_with("you revealed ") && low.ends_with(" as you cast ~"))
        || (low.ends_with(" or more creatures are attacking")
            && parse_number_word(
                low.strip_suffix(" or more creatures are attacking")
                    .unwrap_or(""),
            )
            .is_some())
        || (low.contains(" have exactly ") && low.ends_with(" cards in hand"))
        || (low.ends_with(" cards were put into your graveyard from anywhere this turn"))
        || (low.starts_with("it's the first time ") && low.ends_with(" this turn"))
        || (low.starts_with("a player was dealt ") && low.ends_with(" this turn"))
        || (low.starts_with("you had another ") && low.ends_with(" last turn"))
        || low.starts_with("it doesn't have the same name as another ")
        || (low.contains(" or more mana was spent to cast "))
        || low == "another permanent with the same name is on the battlefield"
        || low == "it has the same name as a permanent"
        || low
            == "it has the greatest power or is tied for greatest power among creatures on the battlefield"
        || low == "you've controlled ~ continuously since the beginning of your most recent turn"
        || (low.starts_with("~ or another ")
            && low.ends_with(" entered the battlefield under your control this turn"))
        || (low.starts_with("there are ")
            && low.ends_with(" permanent types among cards in your graveyard"))
        || (low.starts_with("there are at least ") && low.ends_with(" in your graveyard"))
        || low.starts_with("it shares a color with ")
        || low.starts_with("an opponent controls at least ")
        || low == "the number of permanents is even"
        || low
            == "you've cast a spell from a graveyard or activated an ability of a card in a graveyard this turn"
        || low.starts_with("you both own and control ~ and ")
        || low == "it's your main phase"
        || low == "that spell's mana cost or that ability's activation cost contains {x}"
        || low == "you don't copy a spell this way"
        || low == "you don't cast a spell this way"
        || low == "you don't or can't make an exchange"
        || low == "it shares a card type with a card exiled with ~"
        || low == "that spell doesn't target a creature you control"
        || low == "a creature not named ~ died this turn"
        || low == "mana from a treasure was spent to activate this ability"
        || low == "you've cast both a creature spell and a noncreature spell this turn"
        || low == "a player has more life than each other player"
        || low == "it's an art sticker"
        || low == "you haven't activated an exhaust ability this turn"
        || ((low.starts_with("there is exactly ") || low.starts_with("there are exactly "))
            && low.contains(" counter"))
        || (low.starts_with("there are ") && low.contains(" or more counters among "))
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "no <flt> were <participle> this way" (Haunting Imitation, etc.)
    if let Some(after_no) = low.strip_prefix("no ")
        && let Some(before_tw) = after_no.strip_suffix(" this way")
    {
        for passive in [
            " were revealed",
            " were exiled",
            " were discarded",
            " were destroyed",
            " were sacrificed",
            " were milled",
            " were put onto the battlefield",
            " were put into your hand",
        ] {
            if let Some(flt_raw) = before_tw.strip_suffix(passive)
                && parse_object_filter(flt_raw.trim(), vocab).is_ok()
            {
                return Ok(ConditionAst::Not(Box::new(
                    ConditionAst::PlayerActionThisTurn {
                        player: PlayerRefAst::You,
                        action: low,
                    },
                )));
            }
        }
    }

    // Orthogonal "this way" resolution conditions:
    // "a <flt> was sacrificed/exiled/discarded/dealt damage this way",
    // "you put/discard/sacrifice/return <flt> ... this way", "an opponent created a token this way"
    if let Some(before_tw) = low.strip_suffix(" this way") {
        for passive in [
            " was sacrificed",
            " were sacrificed",
            " was discarded",
            " were discarded",
            " was exiled",
            " were exiled",
            " was destroyed",
            " were destroyed",
            " was countered",
            " was milled",
            " was revealed",
            " was created",
            " was returned",
            " were returned",
            " is destroyed",
            " is exiled",
            " is sacrificed",
            " is discarded",
            " is countered",
            " is returned",
            " is put onto the battlefield",
            " is put into a graveyard",
            " is put into your hand",
            " is dealt damage",
            " was dealt damage",
            " enters",
        ] {
            if let Some(flt_raw) = before_tw.strip_suffix(passive)
                && (parse_object_filter(flt_raw, vocab).is_ok()
                    || parse_player_ref(flt_raw).is_some())
            {
                return Ok(ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: low,
                });
            }
        }
        for (pref, player) in [
            ("you put ", PlayerRefAst::You),
            ("you discard ", PlayerRefAst::You),
            ("you discarded ", PlayerRefAst::You),
            ("you sacrifice ", PlayerRefAst::You),
            ("you sacrificed ", PlayerRefAst::You),
            ("you exile ", PlayerRefAst::You),
            ("you exiled ", PlayerRefAst::You),
            ("you return ", PlayerRefAst::You),
            ("you returned ", PlayerRefAst::You),
            ("you create ", PlayerRefAst::You),
            ("you created ", PlayerRefAst::You),
            ("you mill ", PlayerRefAst::You),
            ("you milled ", PlayerRefAst::You),
            ("you reveal ", PlayerRefAst::You),
            ("you revealed ", PlayerRefAst::You),
            ("you cast ", PlayerRefAst::You),
            ("an opponent created ", PlayerRefAst::AnyOpponent),
            ("an opponent sacrificed ", PlayerRefAst::AnyOpponent),
            ("an opponent discarded ", PlayerRefAst::AnyOpponent),
        ] {
            if before_tw.starts_with(pref) {
                return Ok(ConditionAst::PlayerActionThisTurn {
                    player,
                    action: low,
                });
            }
        }
    }

    // Orthogonal turn-event conditions:
    // "a <flt> entered the battlefield under an opponent's control this turn"
    // "a <flt> left the battlefield [under your control] this turn"
    // "a <flt> was put into a graveyard from the battlefield this turn"
    // "any of that damage was dealt by a <flt>"
    if let Some(flt_raw) = low
        .strip_suffix(" entered the battlefield under an opponent's control this turn")
        .or_else(|| low.strip_suffix(" entered under an opponent's control this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: None,
            to_zone: Some(ZoneFilter::Battlefield),
            controller: Some(PlayerRefAst::AnyOpponent),
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_suffix(" left the battlefield under your control this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: None,
            controller: Some(PlayerRefAst::You),
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_suffix(" left the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: None,
            controller: None,
            min_count: 1,
        });
    }
    if let Some(flt_raw) =
        low.strip_suffix(" was put into a graveyard from the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: Some(ZoneFilter::AnyGraveyard),
            controller: None,
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("any of that damage was dealt by ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if low.starts_with("it was crewed by ") {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }

    // "<mana> was spent to cast it/~"
    if let Some(spent) = low
        .strip_suffix(" was spent to cast it")
        .or_else(|| low.strip_suffix(" was spent to cast ~"))
    {
        return Ok(ConditionAst::ManaSpentToCast(spent.trim().to_string()));
    }

    // "it was kicked with its {M} kicker" (CR 702.33c multi-slot kicker)
    if let Some(slot_raw) = low
        .strip_prefix("~ was kicked with its ")
        .or_else(|| low.strip_prefix("it was kicked with its "))
        && let Some(slot) = slot_raw.strip_suffix(" kicker")
    {
        return Ok(ConditionAst::WasKickedSlot(slot.trim().to_string()));
    }
    if low.starts_with("it was kicked") || low.starts_with("~ was kicked") {
        return Ok(ConditionAst::WasKicked);
    }
    // "the {M} cost was paid" (Devastating Mastery, alternative/additional cost checks)
    if let Some(after) = low.strip_prefix("the ")
        && let Some(cost_raw) = after.strip_suffix(" cost was paid")
    {
        return Ok(ConditionAst::ManaSpentToCast(cost_raw.trim().to_string()));
    }
    if let Some(flt_raw) = low.strip_prefix("you controlled ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("you revealed ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }

    // "you win" / "you lose" / "you win the/a flip" / "you win the/a clash"
    if matches!(
        low.as_str(),
        "you win"
            | "you won"
            | "you win the flip"
            | "you win a flip"
            | "you won the flip"
            | "you won a flip"
            | "you win the clash"
            | "you win a clash"
            | "you won the clash"
            | "you won a clash"
    ) {
        return Ok(ConditionAst::WonContest);
    }
    if matches!(
        low.as_str(),
        "you lose"
            | "you lost"
            | "you lose the flip"
            | "you lose a flip"
            | "you lost the flip"
            | "you lost a flip"
            | "you lose the clash"
            | "you lose a clash"
            | "you lost the clash"
            | "you lost a clash"
    ) {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::WonContest)));
    }

    // "it targets <filter>" / "~ targets <filter>"
    if let Some(flt_raw) = low
        .strip_prefix("it targets ")
        .or_else(|| low.strip_prefix("~ targets "))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }

    let parse_total_stat_cmp = |rhs: &str| -> Option<(CompareOp, i32)> {
        let trimmed = rhs
            .strip_suffix(" this combat")
            .or_else(|| rhs.strip_suffix(" this turn"))
            .unwrap_or(rhs)
            .trim();
        if let Some(n_tok) = trimmed
            .strip_suffix(" or greater")
            .or_else(|| trimmed.strip_suffix(" or more"))
        {
            return parse_number_word(n_tok).map(|n| (CompareOp::Ge, n));
        }
        if let Some(n_tok) = trimmed
            .strip_suffix(" or less")
            .or_else(|| trimmed.strip_suffix(" or fewer"))
        {
            return parse_number_word(n_tok).map(|n| (CompareOp::Le, n));
        }
        if let Some(n_tok) = trimmed.strip_prefix("greater than ") {
            return parse_number_word(n_tok).map(|n| (CompareOp::Gt, n));
        }
        if let Some(n_tok) = trimmed
            .strip_prefix("less than ")
            .or_else(|| trimmed.strip_prefix("fewer than "))
        {
            return parse_number_word(n_tok).map(|n| (CompareOp::Lt, n));
        }
        let eq_tok = trimmed
            .strip_prefix("equal to ")
            .or_else(|| trimmed.strip_prefix("exactly "))
            .unwrap_or(trimmed);
        parse_number_word(eq_tok).map(|n| (CompareOp::Eq, n))
    };

    // "you attacked with <flt> with total power/toughness/mana value <cmp> [this combat/turn]"
    if let Some(after) = low.strip_prefix("you attacked with ") {
        for (marker, stat) in [
            (" with total power ", StatKind::Power),
            (" with total toughness ", StatKind::Toughness),
            (" with total mana value ", StatKind::ManaValue),
        ] {
            if let Some((flt_raw, rhs)) = after.split_once(marker)
                && let (Ok(flt), Some((op, value))) = (
                    parse_object_filter(flt_raw, vocab),
                    parse_total_stat_cmp(rhs),
                )
            {
                return Ok(ConditionAst::TotalStatCompare {
                    filter: Box::new(flt),
                    stat,
                    op,
                    value,
                });
            }
        }
    }

    // "you control <flt> with total power/toughness/mana value <cmp>"
    if let Some(after) = low.strip_prefix("you control ") {
        for (marker, stat) in [
            (" with total power ", StatKind::Power),
            (" with total toughness ", StatKind::Toughness),
            (" with total mana value ", StatKind::ManaValue),
        ] {
            if let Some((flt_raw, rhs)) = after.split_once(marker)
                && let (Ok(mut flt), Some((op, value))) = (
                    parse_object_filter(flt_raw, vocab),
                    parse_total_stat_cmp(rhs),
                )
            {
                if flt.controller.is_none() {
                    flt.controller = Some(ControllerRelation::You);
                }
                return Ok(ConditionAst::TotalStatCompare {
                    filter: Box::new(flt),
                    stat,
                    op,
                    value,
                });
            }
        }
    }

    // "<player> discarded a card this turn" / "<player> drew a card this turn" / "<player> lost life this turn"
    for (suf, act) in [
        (" discarded a card this turn", "discarded_card"),
        (" drew a card this turn", "drew_card"),
        (" drew two or more cards this turn", "drew_2plus"),
        (" gained life this turn", "gained_life"),
        (" lost life this turn", "lost_life"),
        (" sacrificed a permanent this turn", "sacrificed_permanent"),
        (" sacrificed a creature this turn", "sacrificed_creature"),
        (" attacked this turn", "attacked"),
    ] {
        if let Some(who_raw) = low.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player,
                action: act.to_string(),
            });
        }
    }

    // "you have N or more/fewer opponents" / "you have more/fewer than N opponents" / "you have exactly N opponents"
    if let Some(after) = low.strip_prefix("you have ")
        && let Some(cnt_part) = after.strip_suffix(" opponents")
    {
        let parsed = if let Some(tok) = cnt_part.strip_suffix(" or more") {
            parse_number_word(tok).map(|n| (CompareOp::Ge, n))
        } else if let Some(tok) = cnt_part.strip_suffix(" or fewer") {
            parse_number_word(tok).map(|n| (CompareOp::Le, n))
        } else if let Some(tok) = cnt_part.strip_prefix("more than ") {
            parse_number_word(tok).map(|n| (CompareOp::Gt, n))
        } else if let Some(tok) = cnt_part.strip_prefix("fewer than ") {
            parse_number_word(tok).map(|n| (CompareOp::Lt, n))
        } else if let Some(tok) = cnt_part.strip_prefix("exactly ") {
            parse_number_word(tok).map(|n| (CompareOp::Eq, n))
        } else {
            None
        };
        if let Some((op, n)) = parsed {
            return Ok(ConditionAst::OpponentCountCompare {
                op,
                value: u16::try_from(n.max(0)).unwrap_or(2),
            });
        }
    }

    // "<filter> have/had total power/toughness/mana value <cmp>"
    for (marker, stat) in [
        (" have total power ", StatKind::Power),
        (" had total power ", StatKind::Power),
        (" have total toughness ", StatKind::Toughness),
        (" had total toughness ", StatKind::Toughness),
        (" have total mana value ", StatKind::ManaValue),
        (" had total mana value ", StatKind::ManaValue),
    ] {
        if let Some((flt_raw, rhs)) = low.split_once(marker)
            && let (Ok(flt), Some((op, value))) = (
                parse_object_filter(flt_raw, vocab),
                parse_total_stat_cmp(rhs),
            )
        {
            return Ok(ConditionAst::TotalStatCompare {
                filter: Box::new(flt),
                stat,
                op,
                value,
            });
        }
    }

    // "<player> has/have N or more/less life" / "<player> has/have N or more/fewer/no cards in hand" / "<player> has/have N or more cards in their graveyard"
    if let Some((who_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if rhs == "no cards in hand"
            || rhs == "no cards in their hand"
            || rhs == "no cards in your hand"
        {
            return Ok(ConditionAst::HandCount {
                player,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        if let Some(rest) = rhs
            .strip_suffix(" cards in hand")
            .or_else(|| rhs.strip_suffix(" cards in their hand"))
            .or_else(|| rhs.strip_suffix(" cards in your hand"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
        if let Some(rest) = rhs
            .strip_suffix(" cards in their graveyard")
            .or_else(|| rhs.strip_suffix(" cards in your graveyard"))
            .or_else(|| rhs.strip_suffix(" cards in graveyard"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::GraveyardCount {
                    player,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::GraveyardCount {
                    player,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
        if let Some(rest) = rhs.strip_suffix(" life more than your starting life total")
            && let Some(n_tok) = rest.strip_prefix("at least ")
            && let Ok(n) = n_tok.trim().parse::<i32>()
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Add(
                    Box::new(ValueExprAst::StartingLifeTotal(player)),
                    Box::new(ValueExprAst::Const(n)),
                )),
            });
        }
        if let Some(rest) = rhs.strip_suffix(" life") {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player,
                    op: CompareOp::Ge,
                    value: n,
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or less")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player,
                    op: CompareOp::Le,
                    value: n,
                });
            }
        }
        if let Some(before_cnt) = rhs
            .strip_suffix(" counters")
            .or_else(|| rhs.strip_suffix(" counter"))
            && let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::PlayerCounterCompare {
                player,
                counter_kind: kind.trim().to_string(),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }

    // "<player>'s / a library has N or fewer cards in it"
    if let Some((who_raw, rest)) = low.split_once(" library has ")
        && let Some(n_tok) = rest.strip_suffix(" or fewer cards in it")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who_raw),
                stat: PlayerStatKind::CardsInLibrary,
            }),
            op: CompareOp::Le,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }

    if let Some(rest) = low
        .strip_prefix("you've gained ")
        .or_else(|| low.strip_prefix("you gained "))
        && rest.ends_with(" life this turn")
    {
        return Ok(ConditionAst::GainedLifeThisTurn);
    }
    if let Some(rest) = low
        .strip_prefix("an opponent has cast ")
        .or_else(|| low.strip_prefix("an opponent cast "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::AnyOpponent,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low
        .strip_prefix("you've cast ")
        .or_else(|| low.strip_prefix("you cast "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low.strip_prefix("an opponent controls more ")
        && let Some(flt_raw) = rest.strip_suffix(" than you")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::OpponentMoreThanYou {
            metric: "controls".to_string(),
            filter: Some(Box::new(flt)),
        });
    }

    if low.contains("seven or more cards") && low.contains("graveyard") {
        return Ok(ConditionAst::Threshold);
    }
    if low.contains("four or more card types among cards in your graveyard") {
        return Ok(ConditionAst::Delirium);
    }
    if low.contains("three or more artifacts") && low.contains("you control") {
        return Ok(ConditionAst::Metalcraft);
    }
    if low.contains("permanent you controlled left the battlefield this turn")
        || low.contains("permanent left the battlefield under your control this turn")
    {
        return Ok(ConditionAst::Revolt);
    }

    // "<player> lost N or more life this turn"
    if let Some((who_raw, rhs)) = low.split_once(" lost ")
        && let Some(n_tok) = rhs.strip_suffix(" or more life this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::LostLifeThisTurn {
            player,
            min_amount: n,
        });
    }

    // "<player> has drawn / drew N or more cards this turn"
    if let Some((who_raw, rest)) = low
        .split_once(" has drawn ")
        .or_else(|| low.split_once(" have drawn "))
        .or_else(|| low.split_once("'ve drawn "))
        .or_else(|| low.split_once(" drew "))
        && let Some(n_tok) = rest.strip_suffix(" or more cards this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
        }));
    }

    // "<player> cast / has cast N or more spells this turn"
    if let Some((who_raw, rest)) = low
        .split_once(" has cast ")
        .or_else(|| low.split_once(" have cast "))
        .or_else(|| low.split_once("'ve cast "))
        .or_else(|| low.split_once(" cast "))
        && let Some(n_tok) = rest.strip_suffix(" or more spells this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
            from_hand_only: false,
        }));
    }

    // "<player> sacrificed / has sacrificed <filter> this turn"
    if let Some((who_raw, rest)) = low
        .split_once("'ve sacrificed ")
        .or_else(|| low.split_once(" has sacrificed "))
        .or_else(|| low.split_once(" sacrificed "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::SacrificedThisTurn {
            player,
            filter: Box::new(flt),
        }));
    }

    // "<filter> was put into your graveyard from anywhere this turn"
    if let Some(flt_raw) = low
        .strip_suffix(" was put into your graveyard from anywhere this turn")
        .or_else(|| low.strip_suffix(" was put into your graveyard this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }

    // "your devotion to <colors> is less than <N>" / "<N> or greater"
    if let Some(after) = low.strip_prefix("your devotion to ")
        && let Some((colors, rhs)) = after.split_once(" is ")
    {
        if let Some(n_tok) = rhs.strip_prefix("less than ")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::DevotionCompare {
                colors: colors.trim().to_string(),
                op: CompareOp::Lt,
                value: u16::try_from(n.max(0)).unwrap_or(5),
            });
        }
        if let Some(n_tok) = rhs.strip_suffix(" or greater")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::DevotionCompare {
                colors: colors.trim().to_string(),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(5),
            });
        }
    }

    // "<N or more / a / an / another> <filter> entered [the battlefield] under your control this turn"
    if let Some(before_ent) = low
        .strip_suffix(" entered the battlefield under your control this turn")
        .or_else(|| low.strip_suffix(" entered under your control this turn"))
    {
        if let Some((cnt_tok, flt_raw)) = before_ent.split_once(" or more ")
            && let (Some(n), Ok(flt)) = (
                parse_number_word(cnt_tok),
                parse_object_filter(flt_raw, vocab),
            )
        {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        if let Ok(flt) = parse_object_filter(before_ent, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: 1,
            });
        }
    }

    // "there is a/an <filter>" (e.g. "there is a Desert card in your graveyard")
    if let Some(flt_raw) = low.strip_prefix("there is ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "there are N or more mana values among cards in your graveyard"
    if let Some(after) = low.strip_prefix("there are ")
        && let Some(n_tok) =
            after.strip_suffix(" or more mana values among cards in your graveyard")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DifferentManaValuesInGraveyard {
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(4),
            },
        ));
    }

    // "<subject> doesn't have <keyword>" / "<subject> has <keyword>"
    if let Some((subj_raw, kw_raw)) = low.split_once(" doesn't have ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw_raw.trim().to_string(),
        })));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw_raw.trim().to_string(),
        });
    }

    // Zone location conditions: "~ is in your graveyard", "~ is in your hand", "~ is in exile"
    for (phrase_lit, subj, zone) in [
        (
            "~ is in your graveyard",
            SubjectRefAst::SelfObject,
            ZoneFilter::YourGraveyard,
        ),
        (
            "~ is in your hand",
            SubjectRefAst::SelfObject,
            ZoneFilter::YourHand,
        ),
        (
            "~ is in exile",
            SubjectRefAst::SelfObject,
            ZoneFilter::Exile,
        ),
        (
            "~ is on the battlefield",
            SubjectRefAst::SelfObject,
            ZoneFilter::Battlefield,
        ),
        (
            "it is in your graveyard",
            SubjectRefAst::Anaphora("it".to_string()),
            ZoneFilter::YourGraveyard,
        ),
        (
            "it's in your graveyard",
            SubjectRefAst::Anaphora("it".to_string()),
            ZoneFilter::YourGraveyard,
        ),
    ] {
        if low == phrase_lit {
            return Ok(ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(subj),
                zone,
            }));
        }
    }

    for status in [
        "tapped",
        "untapped",
        "attacking",
        "blocking",
        "blocked",
        "enchanted",
        "equipped",
        "modified",
        "monstrous",
        "renowned",
        "suspected",
        "saddled",
        "solved",
        "prepared",
        "face down",
        "kicked",
    ] {
        if let Some(subj_raw) = low.strip_suffix(&format!(" is {status}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(subj),
                status: status.to_string(),
            });
        }
        if low == format!("it's {status}")
            || low == format!("he's {status}")
            || low == format!("she's {status}")
        {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: status.to_string(),
            });
        }
    }

    // "you controlled <subj>" / "this ability has been activated N or more times this turn"
    if let Some(subj_raw) = low.strip_prefix("you controlled ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::ControlledSubject(
            Box::new(subj),
        )));
    }
    if let Some(rest) = low.strip_prefix("this ability has been activated ")
        && let Some(cnt_raw) = rest
            .strip_suffix(" times this turn")
            .or_else(|| rest.strip_suffix(" time this turn"))
    {
        let (n_tok, op) = if let Some(v) = cnt_raw.strip_suffix(" or more") {
            (v, CompareOp::Ge)
        } else if let Some(v) = cnt_raw.strip_suffix(" or fewer") {
            (v, CompareOp::Le)
        } else {
            (cnt_raw, CompareOp::Eq)
        };
        if let Some(n) = parse_number_word(n_tok) {
            return Ok(ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn {
                op,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            }));
        }
    }

    // Generalized "<player> is the monarch" / "<player> didn't cast <filter> this turn"
    // "<player> has more cards in hand than <player2>" / "<subj> entered this turn" / "<subj> was/wasn't cast" / "<subj> is exiled"
    if let Some(who_raw) = low.strip_suffix(" is the monarch")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(ConditionAst::Marker(GameMarker::Monarch(player)));
    }
    if let Some((who_raw, rest)) = low.split_once(" didn't cast ")
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player,
            filter: Some(Box::new(flt)),
        }));
    }
    if let Some((p1_raw, p2_raw)) = low.split_once(" has more cards in hand than ")
        && let (Some(p1), Some(p2)) = (parse_player_ref(p1_raw), parse_player_ref(p2_raw))
    {
        return Ok(ConditionAst::Marker(GameMarker::PlayerHasMoreHandThan {
            player: p1,
            than: p2,
        }));
    }
    if let Some(subj_raw) = low
        .strip_suffix(" entered this turn")
        .or_else(|| low.strip_suffix(" entered the battlefield this turn"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(
            Box::new(subj),
        )));
    }
    if let Some(subj_raw) = low.strip_suffix(" wasn't cast")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectWasCast {
            subject: Box::new(subj),
            was_cast: false,
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" was cast")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectWasCast {
            subject: Box::new(subj),
            was_cast: true,
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" is exiled")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::InZone {
            subject: Box::new(subj),
            zone: ZoneFilter::Exile,
        }));
    }
    // "a graveyard has N or more cards in it" / "N or more cards are in your graveyard"
    if let Some(rest) = low.strip_prefix("a graveyard has ")
        && let Some(n_tok) = rest.strip_suffix(" or more cards in it")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::GraveyardCount {
            player: PlayerRefAst::AnyPlayer,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    if let Some(n_tok) = low.strip_suffix(" or more cards are in your graveyard")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    // "<player> has/have exactly N life" / "fewer than N cards in hand"
    if let Some((who_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if let Some(rest) = rhs.strip_prefix("exactly ")
            && let Some(n_tok) = rest.strip_suffix(" life")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::LifeCompare {
                player,
                op: CompareOp::Eq,
                value: n,
            });
        }
        if let Some(rest) = rhs.strip_prefix("fewer than ")
            && let Some(n_tok) = rest
                .strip_suffix(" cards in hand")
                .or_else(|| rest.strip_suffix(" cards in your hand"))
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::HandCount {
                player,
                op: CompareOp::Lt,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }

    // "<player> control(s) [don't control] [at least N | N or more/fewer] <filter>"
    for (prefix, player, neg) in [
        ("you don't control ", PlayerRefAst::You, true),
        ("you control ", PlayerRefAst::You, false),
        ("any player controls ", PlayerRefAst::AnyPlayer, false),
        ("no opponent controls ", PlayerRefAst::EachOpponent, true),
        ("no player controls ", PlayerRefAst::AnyPlayer, true),
        (
            "your opponents don't control ",
            PlayerRefAst::EachOpponent,
            true,
        ),
        ("your opponents control ", PlayerRefAst::EachOpponent, false),
        ("opponents control ", PlayerRefAst::EachOpponent, false),
        (
            "defending player doesn't control ",
            PlayerRefAst::DefendingPlayer,
            true,
        ),
        (
            "defending player controls ",
            PlayerRefAst::DefendingPlayer,
            false,
        ),
        (
            "an opponent doesn't control ",
            PlayerRefAst::AnyOpponent,
            true,
        ),
        ("an opponent controls ", PlayerRefAst::AnyOpponent, false),
        (
            "another player controls ",
            PlayerRefAst::EachOtherPlayer,
            false,
        ),
        ("your team controls ", PlayerRefAst::You, false),
    ] {
        if let Some(rest) = low.strip_prefix(prefix) {
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("exactly one ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Eq,
                    value: 1,
                });
            }
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("no ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if let Some(after_al) = rest.strip_prefix("at least ")
                && let Some((cnt_tok, flt_raw)) = after_al.split_once(' ')
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
            if let Some((cnt_tok, flt_raw)) = rest.split_once(" or more ")
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                });
            }
            if let Some((cnt_tok, flt_raw)) = rest.split_once(" or fewer ")
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Ok(flt) = parse_object_filter(rest, vocab) {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: 1,
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
        }
    }

    // "there are no [<kind>] counters on <subj>" / "there are N or more <kind> counters on <subj>"
    if let Some(after) = low.strip_prefix("there are no ")
        && let Some((kind_part, subj_raw)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: if kind_part.trim().is_empty() {
                "any".to_string()
            } else {
                kind_part.trim().to_string()
            },
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, rest)) = after.split_once(" or more ")
        && let Some((kind_part, subj_raw)) = rest
            .split_once(" counters on ")
            .or_else(|| rest.split_once(" counter on "))
        && let (Some(n), Ok(subj)) = (
            parse_number_word(cnt_tok),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: kind_part.trim().to_string(),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }

    // "there are no <filter>" / "there are <filter>" / "there are N or more <filter>"
    if let Some(flt_raw) = low.strip_prefix("there are no ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_tok)
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("there are ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "you have N or more/less life" / "you have N or more/fewer cards in hand"
    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(rest) = after.strip_suffix(" life") {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player: PlayerRefAst::You,
                    op: CompareOp::Ge,
                    value: n,
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or less")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player: PlayerRefAst::You,
                    op: CompareOp::Le,
                    value: n,
                });
            }
        }
        if let Some(rest) = after
            .strip_suffix(" cards in hand")
            .or_else(|| after.strip_suffix(" cards in your hand"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player: PlayerRefAst::You,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player: PlayerRefAst::You,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
    }

    // "<filter> is/was milled/discarded/exiled/revealed this way"
    for (suf, act) in [
        (" is milled this way", "milled"),
        (" was milled this way", "milled"),
        (" is discarded this way", "discarded"),
        (" was discarded this way", "discarded"),
        (" is exiled this way", "exiled"),
        (" was exiled this way", "exiled"),
        (" is revealed this way", "revealed"),
        (" was revealed this way", "revealed"),
    ] {
        if let Some(flt_raw) = low.strip_suffix(suf)
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: act.to_string(),
                },
            ));
        }
    }

    // "<subject> isn't <filter>" / "<subject> is/was <filter>" (fully generic across any SubjectRefAst!)
    if let Some((subj_raw, rest)) = low
        .split_once(" isn't ")
        .or_else(|| low.split_once(" wasn't "))
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        })));
    }
    if let Some(rest) = low.strip_prefix("it's ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(flt),
        });
    }
    if let Some((subj_raw, rest)) = low.split_once(" is ").or_else(|| low.split_once(" was "))
        && !rest.starts_with("greater than")
        && !rest.starts_with("less than")
        && !rest.starts_with("equal to")
        && !rest.ends_with(" or greater")
        && !rest.ends_with(" or less")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        });
    }

    // "<subj>'s power/toughness/mana value is N or less/greater"
    for (stat_mid, stat) in [
        ("'s mana value is ", StatKind::ManaValue),
        ("'s power is ", StatKind::Power),
        ("'s toughness is ", StatKind::Toughness),
    ] {
        if let Some((subj_raw, rest)) = low.split_once(stat_mid)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                (v, CompareOp::Ge)
            } else if let Some(v) = rest.strip_suffix(" or less") {
                (v, CompareOp::Le)
            } else {
                (rest, CompareOp::Eq)
            };
            if let Ok(n) = n_tok.trim().parse::<i32>() {
                return Ok(ConditionAst::StatCompare {
                    subject: Box::new(subj),
                    stat,
                    op,
                    value: n,
                });
            }
        }
    }
    for (prefix, stat) in [
        ("its mana value is ", StatKind::ManaValue),
        ("its power is ", StatKind::Power),
        ("its toughness is ", StatKind::Toughness),
    ] {
        if let Some(rest) = low.strip_prefix(prefix) {
            let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                (v, CompareOp::Ge)
            } else if let Some(v) = rest.strip_suffix(" or less") {
                (v, CompareOp::Le)
            } else {
                (rest, CompareOp::Eq)
            };
            if let Ok(n) = n_tok.trim().parse::<i32>() {
                return Ok(ConditionAst::StatCompare {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat,
                    op,
                    value: n,
                });
            }
        }
    }

    // "<subject> has/had power/toughness/mana value/counters"
    if let Some((subj_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" had ")) {
        for (st_pref, stat) in [
            ("power ", StatKind::Power),
            ("toughness ", StatKind::Toughness),
            ("mana value ", StatKind::ManaValue),
        ] {
            if let Some(rest) = rhs.strip_prefix(st_pref) {
                let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                    (v, CompareOp::Ge)
                } else if let Some(v) = rest.strip_suffix(" or less") {
                    (v, CompareOp::Le)
                } else {
                    continue;
                };
                if let Ok(n) = n_tok.parse::<i32>()
                    && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
                {
                    return Ok(ConditionAst::StatCompare {
                        subject: Box::new(subj),
                        stat,
                        op,
                        value: n,
                    });
                }
            }
        }
        // "<subject> has/had [no|a|an|one or more|N or more] [<kind>] counter(s) on it"
        if let Some(before_on) = rhs.strip_suffix(" on it")
            && let Some(before_cnt) = before_on
                .strip_suffix(" counters")
                .or_else(|| before_on.strip_suffix(" counter"))
                .or_else(|| (before_on == "counters").then_some(""))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if let Some(no_kind) = before_cnt.strip_prefix("no ") {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: if no_kind.trim().is_empty() {
                        "any".to_string()
                    } else {
                        no_kind.trim().to_string()
                    },
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if before_cnt == "no" {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: "any".to_string(),
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: kind.trim().to_string(),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                });
            }
            let kind = before_cnt
                .strip_prefix("one or more ")
                .or_else(|| before_cnt.strip_prefix("a "))
                .or_else(|| before_cnt.strip_prefix("an "))
                .unwrap_or(before_cnt);
            let kclean =
                if kind.trim().is_empty() || kind.trim() == "one or more" || kind.trim() == "a" {
                    "any"
                } else {
                    kind.trim()
                };
            return Ok(ConditionAst::CounterCompare {
                subject: Box::new(subj),
                counter_kind: kclean.to_string(),
                op: CompareOp::Ge,
                value: 1,
            });
        }
    }

    // "<subject> doesn't have a <kind> counter on it"
    if let Some((subj_raw, rhs)) = low.split_once(" doesn't have ")
        && let Some(before_on) = rhs.strip_suffix(" on it")
        && let Some(before_cnt) = before_on
            .strip_suffix(" counters")
            .or_else(|| before_on.strip_suffix(" counter"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = before_cnt
            .strip_prefix("a ")
            .or_else(|| before_cnt.strip_prefix("an "))
            .unwrap_or(before_cnt)
            .trim();
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: if kind.is_empty() {
                "any".to_string()
            } else {
                kind.to_string()
            },
            op: CompareOp::Eq,
            value: 0,
        });
    }

    // "<subject> has/had power/toughness/mana value greater/less than <ValueExpr>" (Drizzt Do'Urden, etc.)
    if let Some((subj_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" had ")) {
        for (st_pref, stat) in [
            ("power ", StatKind::Power),
            ("toughness ", StatKind::Toughness),
            ("mana value ", StatKind::ManaValue),
        ] {
            if let Some(rest) = rhs.strip_prefix(st_pref)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                let cmp_opt = rest
                    .strip_prefix("greater than or equal to ")
                    .map(|r| (CompareOp::Ge, r))
                    .or_else(|| {
                        rest.strip_prefix("less than or equal to ")
                            .map(|r| (CompareOp::Le, r))
                    })
                    .or_else(|| {
                        rest.strip_prefix("greater than ")
                            .map(|r| (CompareOp::Gt, r))
                    })
                    .or_else(|| rest.strip_prefix("less than ").map(|r| (CompareOp::Lt, r)));
                if let Some((op, val_raw)) = cmp_opt
                    && let Ok(val) = parse_value_expr(val_raw, vocab)
                {
                    return Ok(ConditionAst::Compare {
                        lhs: Box::new(ValueExprAst::ObjectStat {
                            subject: Box::new(subj),
                            stat,
                        }),
                        op,
                        rhs: Box::new(val),
                    });
                }
            }
        }
    }

    // Pass N orthogonal conditions:
    // - "you have exactly N cards in hand" (Library of Alexandria)
    // - "you were dealt N or more damage this turn" (Boarded Window)
    // - "a <kind> counter was put on <flt> this turn" (Fairgrounds Trumpeter)
    // - "<flt> died under your control this turn" (White Glove Gourmand)
    // - "you attacked with <flt> this turn" (Avengers Assemble!)
    // - "it targets one or more <flt>" (Orvar)
    // - "~ is attached to <flt>" / "~ is enchanted by <flt>" (Summoning Materia, Timber Paladin)
    // - "the top card of your library is <flt/color>" (Vampire Nocturnus, Crown of Convergence)
    // - "there are N or more [card types | <kind> counters] among <flt>" (Keen-Eyed Curator, Backstreet Bruiser)
    // - "you've cast N or more <flt> spells this turn" (Lyse Hext)
    // - "his/its/the {M}/<kw> cost was paid" (Leonardo, Devastating Mastery)
    if let Some(after) = low.strip_prefix("you have exactly ")
        && let Some(cnt_raw) = after.strip_suffix(" cards in hand")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        return Ok(ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }
    if let Some(after) = low.strip_prefix("you were dealt ")
        && let Some(cnt_raw) = after.strip_suffix(" or more damage this turn")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DamageDealtToSubjectThisTurn {
                subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                min_amount: u16::try_from(n.max(1)).unwrap_or(1),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("a ")
        && let Some((ck, rest)) = after.split_once(" counter was put on ")
        && rest.ends_with(" this turn")
    {
        return Ok(ConditionAst::Marker(
            GameMarker::PutCountersOnSubjectThisTurn {
                player: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::Anaphora(rest.to_string())),
                counter_kind: ck.trim().to_string(),
            },
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" died under your control this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you attacked with ")
        && let Some(flt_raw) = after.strip_suffix(" this turn")
        && let Ok(_flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::AttackedThisTurn);
    }
    if let Some(after) = low
        .strip_prefix("it targets one or more ")
        .or_else(|| low.strip_prefix("that spell targets one or more "))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }
    if let Some(after) = low
        .strip_prefix("~ is attached to ")
        .or_else(|| low.strip_prefix("~ is enchanted by "))
        && !after.is_empty()
    {
        return Ok(ConditionAst::HasStatus {
            subject: Box::new(SubjectRefAst::SelfObject),
            status: format!("attached:{after}"),
        });
    }
    if let Some(pred_raw) = low.strip_prefix("the top card of your library is ")
        && let Ok(flt) = parse_object_filter(pred_raw, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("top_of_library".to_string())),
            filter: Box::new(flt),
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        if let Some(flt_raw) = rest.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::CardTypesAmong(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some((_, flt_raw)) = rest.split_once(" counters among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }
    if let Some(after) = low.strip_prefix("you've cast ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw, vocab),
        )
    {
        let value = u16::try_from(n.max(0)).unwrap_or(0);
        if flt_raw.trim() == "spells" {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value,
                from_hand_only: false,
            }));
        }
        return Ok(ConditionAst::Marker(
            GameMarker::CastFilteredSpellsThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value,
            },
        ));
    }
    if let Some(cost_kind) = low
        .strip_prefix("the ")
        .or_else(|| low.strip_prefix("his "))
        .or_else(|| low.strip_prefix("her "))
        .or_else(|| low.strip_prefix("~'s "))
        .and_then(|r| r.strip_suffix(" cost was paid"))
        && !cost_kind.is_empty()
    {
        return Ok(ConditionAst::Marker(GameMarker::KeywordCostPaid(
            cost_kind.to_string(),
        )));
    }

    // Pass M orthogonal conditions: "you don't put ... into your hand this way",
    // "damage [from <flt>] is prevented this way", "most common color", "you have N or more <flt> in your graveyard"
    if low.starts_with("you don't put ") && low.ends_with(" into your hand this way") {
        return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
            zone: "your hand".to_string(),
        }));
    }
    if low.starts_with("damage ") && low.ends_with(" is prevented this way") {
        return Ok(ConditionAst::Marker(GameMarker::DamagePreventedThisWay));
    }
    if let Some((color_raw, or_tied)) = low
        .strip_suffix(" is the most common color among all permanents or is tied for most common")
        .map(|c| (c.trim(), true))
        .or_else(|| {
            low.strip_suffix(" is the most common color among all permanents")
                .map(|c| (c.trim(), false))
        })
        && let Some(color) = lookup_color(color_raw)
    {
        return Ok(ConditionAst::Marker(GameMarker::MostCommonPermanentColor {
            color,
            or_tied,
        }));
    }
    if let Some(after) = low.strip_prefix("you have ")
        && let Some((cnt_part, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) = rest.strip_suffix(" in your graveyard")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_part.trim()),
            parse_object_filter(flt_raw, vocab),
        )
    {
        let mut gflt = flt;
        gflt.zone = Some(ZoneFilter::YourGraveyard);
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(gflt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }

    // Generic voting result condition: "<option> gets more votes [or the vote is tied]"
    if let Some(opt) = low
        .strip_suffix(" gets more votes or the vote is tied")
        .or_else(|| low.strip_suffix(" gets more votes"))
        && !opt.is_empty()
    {
        let tied = low.ends_with(" or the vote is tied");
        return Ok(ConditionAst::Marker(GameMarker::VoteResult {
            winning: if tied {
                format!("{opt}_or_tie")
            } else {
                opt.to_string()
            },
        }));
    }

    // Orthogonal Pass I conditions (turn history, zone membership, stat comparisons, keyword checks)
    if low == "you created a token this turn" || low == "you created one or more tokens this turn" {
        return Ok(ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }));
    }
    if low == "you've lost life this turn" || low == "you lost life this turn" {
        return Ok(ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        });
    }
    if low == "you've played a land or cast a spell this turn from anywhere other than your hand" {
        return Ok(ConditionAst::Marker(
            GameMarker::LandOrSpellOutsideHandThisTurn {
                player: PlayerRefAst::You,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" isn't on the battlefield")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::InZone {
                subject: Box::new(subj),
                zone: ZoneFilter::Battlefield,
            },
        ))));
    }
    if let Some(flt_raw) =
        low.strip_suffix(" was put into a graveyard from the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under your control this turn")
    {
        let clean = flt_raw
            .strip_prefix("a ")
            .or_else(|| flt_raw.strip_prefix("an "))
            .or_else(|| flt_raw.strip_prefix("one or more "))
            .unwrap_or(flt_raw);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: 1,
            });
        }
    }
    if let Some(after) = low.strip_prefix("an opponent had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under their control this turn")
    {
        let (min_count, clean) = if let Some(r) = flt_raw.strip_prefix("two or more ") {
            (2u16, r)
        } else if let Some(r) = flt_raw.strip_prefix("three or more ") {
            (3u16, r)
        } else {
            (
                1u16,
                flt_raw
                    .strip_prefix("a ")
                    .or_else(|| flt_raw.strip_prefix("an "))
                    .unwrap_or(flt_raw),
            )
        };
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(flt),
                from_zone: None,
                to_zone: Some(ZoneFilter::Battlefield),
                controller: Some(PlayerRefAst::AnyOpponent),
                min_count,
            });
        }
    }
    if let Some((num_raw, tail)) = low.split_once(" or more damage was dealt to ")
        && let Some(subj_raw) = tail.strip_suffix(" this turn")
        && let (Some(n), Ok(subj)) = (
            parse_number_word(num_raw.trim()),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DamageDealtToSubjectThisTurn {
                subject: Box::new(subj),
                min_amount: u16::try_from(n.max(1)).unwrap_or(1),
            },
        ));
    }
    if let Some(kw) = low
        .strip_prefix("its ")
        .and_then(|r| r.strip_suffix(" cost was paid"))
        && !kw.is_empty()
    {
        return Ok(ConditionAst::Marker(GameMarker::KeywordCostPaid(
            kw.to_string(),
        )));
    }
    if let Some(after) = low.strip_prefix("you control the ")
        && let Some((flt_raw, rest)) = after.split_once(" with the greatest ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let stat = if rest.starts_with("mana value") {
            Some(StatKind::ManaValue)
        } else if rest.starts_with("power") {
            Some(StatKind::Power)
        } else if rest.starts_with("toughness") {
            Some(StatKind::Toughness)
        } else {
            None
        };
        if let Some(stat) = stat {
            return Ok(ConditionAst::Marker(GameMarker::ControlsGreatestStat {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                stat,
            }));
        }
    }
    if let Some((subj_raw, rest)) = low.split_once(" is the only ")
        && let Some(flt_raw) = rest.strip_suffix(" in your graveyard")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::OnlyObjectInZone {
            subject: Box::new(subj),
            filter: Box::new(flt),
            zone: ZoneFilter::YourGraveyard,
        }));
    }
    if let Some(after) = low.strip_prefix("you've put one or more ")
        && let Some((ck, tail)) = after.split_once(" counters on ")
        && let Some(subj_raw) = tail.strip_suffix(" this turn")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::PutCountersOnSubjectThisTurn {
                player: PlayerRefAst::You,
                subject: Box::new(subj),
                counter_kind: ck.trim().to_string(),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("all ")
        && let Some((flt_raw, pred_raw)) = after.split_once(" are ")
        && let (Ok(flt), Ok(pred)) = (
            parse_object_filter(flt_raw, vocab),
            parse_object_filter(pred_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::AllMatchingAre {
            filter: Box::new(flt),
            predicate: Box::new(pred),
        }));
    }
    if let Some(after) = low.strip_prefix("a player controls more ")
        && let Some(flt_raw) = after.strip_suffix(" than each other player")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::AnyPlayerControlsMoreThanEachOther {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let (Ok(subj), Some(kws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw_raw, vocab),
        )
        && let Some(kw) = kws.into_iter().next()
    {
        return Ok(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw,
        });
    }
    if let Some((subj_raw, flt_raw)) = low
        .split_once(" was a ")
        .or_else(|| low.split_once(" was an "))
        .or_else(|| low.split_once(" were a "))
        .or_else(|| low.split_once(" were an "))
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        });
    }

    // Orthogonal numeric comparison across ANY two ValueExprAst expressions (present OR past tense "is"/"was"):
    // Also supports elliptical possessive RHS ("that creature's power is greater than ~'s")!
    if let Some((lhs_raw, rhs_raw)) = low.split_once(" is ").or_else(|| low.split_once(" was "))
        && let Ok(lhs) = parse_value_expr(lhs_raw, vocab)
    {
        let parsed_cmp = if let Some(r) = rhs_raw.strip_prefix("greater than or equal to ") {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = rhs_raw.strip_prefix("less than or equal to ") {
            Some((CompareOp::Le, r))
        } else if let Some(r) = rhs_raw.strip_prefix("greater than ") {
            Some((CompareOp::Gt, r))
        } else if let Some(r) = rhs_raw.strip_prefix("less than ") {
            Some((CompareOp::Lt, r))
        } else if let Some(r) = rhs_raw.strip_prefix("equal to ") {
            Some((CompareOp::Eq, r))
        } else if let Some(r) = rhs_raw.strip_prefix("exactly ") {
            Some((CompareOp::Eq, r))
        } else if let Some(r) = rhs_raw
            .strip_suffix(" or greater")
            .or_else(|| rhs_raw.strip_suffix(" or more"))
        {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = rhs_raw
            .strip_suffix(" or less")
            .or_else(|| rhs_raw.strip_suffix(" or fewer"))
        {
            Some((CompareOp::Le, r))
        } else {
            Some((CompareOp::Eq, rhs_raw))
        };
        if let Some((op, r_str)) = parsed_cmp {
            let rhs_res = parse_value_expr(r_str, vocab).or_else(|_| {
                if let ValueExprAst::ObjectStat { stat, .. } = &lhs
                    && let Some(owner_raw) = r_str.strip_suffix("'s")
                    && let Ok(owner_subj) = parse_subject_or_target(owner_raw, vocab)
                {
                    Ok(ValueExprAst::ObjectStat {
                        subject: Box::new(owner_subj),
                        stat: *stat,
                    })
                } else {
                    Err("no_rhs".to_string())
                }
            });
            if let Ok(rhs) = rhs_res {
                return Ok(ConditionAst::Compare {
                    lhs: Box::new(lhs),
                    op,
                    rhs: Box::new(rhs),
                });
            }
        }
    }

    // General anaphoric/outcome conditions ("you do", "you don't", "you can't", coin flips, played a land)
    match low.as_str() {
        "you do"
        | "you don't"
        | "you can't"
        | "you did"
        | "they do"
        | "they don't"
        | "they can't"
        | "that player does"
        | "that player doesn't"
        | "you've played a land this turn"
        | "you played a land this turn"
        | "you haven't cast the card"
        | "~ was cast this turn"
        | "it attacked a battle this turn"
        | "they haven't dealt combat damage yet"
        | "the sacrificed permanent was a commander"
        | "no other creature has greater power"
        | "you control a stickered permanent"
        | "you paid one or more {e} this way"
        | "it's paired with a creature with soulbond"
        | "a permanent entered the battlefield face down under your control this turn"
        | "you turned a permanent face up this turn"
        | "you own a card named ~ in exile, in your hand, in your graveyard, and on the battlefield" =>
        {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: low,
            });
        }
        "the coin comes up heads" | "it comes up heads" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "heads".to_string(),
            });
        }
        "the coin comes up tails" | "it comes up tails" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "tails".to_string(),
            });
        }
        "you win the flip" | "you win" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "win".to_string(),
            });
        }
        "you lose the flip" | "you lose a flip" | "you lose" => {
            return Ok(ConditionAst::PlayerCoinFlipOutcome {
                player: PlayerRefAst::You,
                outcome: "lose".to_string(),
            });
        }
        "that creature is attacking" => {
            return Ok(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                filter: Box::new(ObjectFilterAst {
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }),
            });
        }
        "that player controls three or fewer lands" => {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::ThatPlayer,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::LAND,
                    ..Default::default()
                }),
                op: CompareOp::Le,
                value: 3,
            });
        }
        "two or more colors of mana were spent to cast it" => {
            return Ok(ConditionAst::ColorsOfManaSpentCompare {
                op: CompareOp::Ge,
                value: 2,
            });
        }
        "a card with the same name is in a graveyard" => {
            return Ok(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SameAsReferenced,
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    ..Default::default()
                }),
                zone: ZoneFilter::AnyGraveyard,
                op: CompareOp::Ge,
                count: 1,
            });
        }
        "a nontoken permanent with the same name is on the battlefield" => {
            return Ok(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SameAsReferenced,
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    token: Some(false),
                    ..Default::default()
                }),
                zone: ZoneFilter::Battlefield,
                op: CompareOp::Ge,
                count: 1,
            });
        }
        "defending player controls the most creatures or is tied for the most" => {
            return Ok(ConditionAst::PlayerControlsMost {
                player: PlayerRefAst::DefendingPlayer,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                allow_tie: true,
            });
        }
        _ => {}
    }
    // General "you attacked with <N> or more <filter> this turn" (Deepway Navigator)
    if let Some(after) = low
        .strip_prefix("you attacked with ")
        .and_then(|r| r.strip_suffix(" this turn"))
        && let Some((n_tok, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(n_tok.trim())
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(ConditionAst::AttackedWithFilteredThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            min_count: u8::try_from(n.max(1)).unwrap_or(1),
        });
    }
    // General "at least one [other] <filter> is blocking <subj>" / "no <filter> are blocking <subj>" (Wall of Caltrops)
    if let Some(after) = low
        .strip_prefix("at least one ")
        .or_else(|| low.strip_prefix("no "))
        && let Some((flt_raw, subj_raw)) = after
            .split_once(" is blocking ")
            .or_else(|| after.split_once(" are blocking "))
        && let Ok(blocker_flt) = parse_object_filter(flt_raw.trim(), vocab)
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let cond = ConditionAst::BlockedByFilter {
            subject: Box::new(subj),
            blocker_filter: Box::new(blocker_flt),
            min_count: 1,
        };
        return Ok(if low.starts_with("no ") {
            ConditionAst::Not(Box::new(cond))
        } else {
            cond
        });
    }

    // General "<subj> [doesn't] target[s] <filter>" (Mavinda, Not of This World, Lullmage's Domination)
    for (sep, neg) in [
        (" doesn't target ", true),
        (" targets ", false),
        (" target ", false),
    ] {
        if let Some((subj_raw, tgt_raw)) = low.split_once(sep)
            && let (Ok(subj), Ok(tgt)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_subject_or_target(tgt_raw, vocab),
            )
        {
            let cond = ConditionAst::TargetsSubject {
                subject: Box::new(subj),
                target: Box::new(tgt),
            };
            return Ok(if neg {
                ConditionAst::Not(Box::new(cond))
            } else {
                cond
            });
        }
    }

    // General "<subj> doesn't have \"<ability>\"" / "<subj> has \"<ability>\"" (Musician, etc.)
    for (sep, neg) in [(" doesn't have \"", true), (" has \"", false)] {
        if let Some((subj_raw, rest)) = raw.to_ascii_lowercase().split_once(sep)
            && rest.ends_with('"')
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let cond = ConditionAst::HasStatus {
                subject: Box::new(subj),
                status: format!("has_ability:{}", rest.trim_end_matches('"')),
            };
            return Ok(if neg {
                ConditionAst::Not(Box::new(cond))
            } else {
                cond
            });
        }
    }

    // General "<subj> is unblocked / attacking / blocking / attached to <filter> / hasn't been exerted this turn"
    for (suf, st_name, neg) in [
        (" hasn't been exerted this turn", "exerted_this_turn", true),
        (" is unblocked", "unblocked", false),
        (" is attacking", "attacking", false),
        (" is blocking", "blocking", false),
        (" remain exiled", "exiled", false),
        (" remains exiled", "exiled", false),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf) {
            let subj_clean = subj_raw.strip_prefix("any of ").unwrap_or(subj_raw).trim();
            if let Ok(subj) = parse_subject_or_target(subj_clean, vocab) {
                let cond = ConditionAst::HasStatus {
                    subject: Box::new(subj),
                    status: st_name.to_string(),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(cond))
                } else {
                    cond
                });
            }
        }
    }
    if let Some((subj_raw, tgt_raw)) = low
        .split_once(" is attached to ")
        .or_else(|| low.split_once("'s attached to "))
        && let (Ok(subj), Ok(host)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(tgt_raw, vocab),
        )
    {
        return Ok(ConditionAst::AttachedTo {
            subject: Box::new(subj),
            host: Box::new(host),
        });
    }

    // General "<player> controls more <filter> than <player>" (Mogg Toady, Damping Engine)
    if let Some((p1_raw, rest)) = low
        .split_once(" controls more ")
        .or_else(|| low.split_once(" control more "))
        && let Some((flt_raw, p2_raw)) = rest.split_once(" than ")
        && let (Some(p1), Ok(flt), Some(p2)) = (
            parse_player_ref(p1_raw),
            parse_object_filter(flt_raw, vocab),
            parse_player_ref(p2_raw),
        )
    {
        return Ok(ConditionAst::ControlsMoreThan {
            player: p1,
            filter: Box::new(flt),
            than_player: p2,
        });
    }

    // General "you gain control of <filter> this way" / "<subj> has the same name as <filter>"
    if let Some(flt_raw) = low
        .strip_prefix("you gain control of ")
        .and_then(|r| r.strip_suffix(" this way"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::PriorActionOccurred {
            action: "gain_control_this_way".to_string(),
            subject_filter: Some(Box::new(flt)),
            min_count: 1,
        });
    }
    if let Some((subj_raw, other_raw)) = low.split_once(" has the same name as ")
        && let (Ok(subj), Ok(other)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(other_raw, vocab),
        )
    {
        return Ok(ConditionAst::SameNameAs {
            subject: Box::new(subj),
            other: Box::new(other),
        });
    }

    // General value comparisons: "<val1> is greater than or equal to / less than / greater than <val2>"
    for (sep, op) in [
        (" is greater than or equal to ", CompareOp::Ge),
        (" is less than or equal to ", CompareOp::Le),
        (" is greater than ", CompareOp::Gt),
        (" is less than ", CompareOp::Lt),
        (" is equal to ", CompareOp::Eq),
    ] {
        if let Some((l_raw, r_raw)) = low.split_once(sep)
            && !l_raw.contains(" and ")
            && !r_raw.contains(" and ")
            && let (Ok(lv), Ok(rv)) = (
                parse_value_expr(l_raw, vocab),
                parse_value_expr(r_raw, vocab),
            )
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(lv),
                op,
                rhs: Box::new(rv),
            });
        }
    }

    // Typed morbid condition ("a non-Zombie creature died this turn", "a Goblin died this turn")
    if let Some(flt_raw) = low
        .strip_prefix("a ")
        .or_else(|| low.strip_prefix("an "))
        .and_then(|r| r.strip_suffix(" died this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        if flt_raw.trim() == "creature" {
            return Ok(ConditionAst::Morbid {
                your_control_only: false,
            });
        }
        return Ok(ConditionAst::ZoneChangeThisTurn {
            filter: Box::new(flt),
            from_zone: Some(ZoneFilter::Battlefield),
            to_zone: Some(ZoneFilter::AnyGraveyard),
            controller: None,
            min_count: 1,
        });
    }

    // General type/filter predicate on a subject ("~ is a Detective", "it's a Mount", "it isn't a land card")
    for (sep, negated) in [
        (" isn't a ", true),
        (" isn't an ", true),
        (" wasn't a ", true),
        (" wasn't an ", true),
        (" is a ", false),
        (" is an ", false),
        ("'s a ", false),
        ("'s an ", false),
        (" was a ", false),
        (" was an ", false),
    ] {
        if let Some((subj_raw, flt_raw)) = low.split_once(sep)
            && !flt_raw.contains(" and ")
            && !flt_raw.contains(" or ")
            && let (Ok(subj), Ok(obj_flt)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_object_filter(flt_raw, vocab),
            )
        {
            let cond = ConditionAst::MatchesFilter {
                subject: Box::new(subj),
                filter: Box::new(obj_flt),
            };
            return Ok(if negated {
                ConditionAst::Not(Box::new(cond))
            } else {
                cond
            });
        }
    }

    // Compound boolean conditions ("<c1> and [if] <c2>" / "<c1> or [if] <c2>"), scanning all occurrences
    // with shared subject carry-forward ("you control a Squirrel or returned ...")
    for sep in [" and if ", " and "] {
        for (idx, _) in low.match_indices(sep) {
            let l = low[..idx].trim();
            let r = low[idx + sep.len()..].trim();
            if l.is_empty() || r.is_empty() {
                continue;
            }
            if let (Ok(lc), Ok(rc)) = (parse_condition(l, vocab), parse_condition(r, vocab)) {
                return Ok(ConditionAst::And(Box::new(lc), Box::new(rc)));
            }
            if l.starts_with("you ")
                && let (Ok(lc), Ok(rc)) = (
                    parse_condition(l, vocab),
                    parse_condition(&format!("you {r}"), vocab),
                )
            {
                return Ok(ConditionAst::And(Box::new(lc), Box::new(rc)));
            }
        }
    }
    for sep in [" or if ", " or "] {
        for (idx, _) in low.match_indices(sep) {
            let l = low[..idx].trim();
            let r = low[idx + sep.len()..].trim();
            if l.is_empty() || r.is_empty() {
                continue;
            }
            if let (Ok(lc), Ok(rc)) = (parse_condition(l, vocab), parse_condition(r, vocab)) {
                return Ok(ConditionAst::Or(Box::new(lc), Box::new(rc)));
            }
            if l.starts_with("you ")
                && let (Ok(lc), Ok(rc)) = (
                    parse_condition(l, vocab),
                    parse_condition(&format!("you {r}"), vocab),
                )
            {
                return Ok(ConditionAst::Or(Box::new(lc), Box::new(rc)));
            }
        }
    }

    // Wave E2 lossless ConditionAst rules (trigger_if, static, and effect conditions)
    if let Some(after_u) = low.strip_prefix("you have ")
        && let Some(n_str) = after_u.strip_suffix(" or more unspent mana")
        && let Some(n) = parse_number_word(n_str.trim())
    {
        return Ok(ConditionAst::UnspentManaAtLeast {
            player: PlayerRefAst::You,
            amount: n,
        });
    }
    if let Some(rest) = low.strip_prefix("you control a prime number of ")
        && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
    {
        return Ok(ConditionAst::PrimeNumberCheck {
            expr: Box::new(ValueExprAst::Count(Box::new(flt))),
        });
    }
    if let Some(rest) = low
        .strip_prefix("another ")
        .and_then(|r| r.strip_suffix(" is on the battlefield"))
        && let Ok(mut flt) = parse_object_filter(rest.trim(), vocab)
    {
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::EachPlayer,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(rest) = low.strip_prefix("there are ")
        && let Some((n_str, tail)) = rest.split_once(" or more ")
        && let Some(n) = parse_number_word(n_str.trim())
        && let Some((cat_raw, among_raw)) = tail.split_once(" among ")
        && let Ok(flt) = parse_object_filter(among_raw.trim(), vocab)
    {
        return Ok(ConditionAst::ExprCompare {
            lhs: Box::new(ValueExprAst::DistinctCountAmong {
                category: cat_raw.trim().to_string(),
                filter: Box::new(flt),
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }
    if let Some(rest) = low.strip_prefix("your library has ")
        && let Some(n_str) = rest.strip_suffix(" or more cards in it")
        && let Some(n) = parse_number_word(n_str.trim())
    {
        return Ok(ConditionAst::ExprCompare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::You,
                stat: PlayerStatKind::CardsInLibrary,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }
    if let Some(rest) = low.strip_prefix("any of ")
        && let Some((grp_raw, pred_raw)) = rest
            .split_once(" have ")
            .or_else(|| rest.split_once(" has "))
        && let Ok(grp) = parse_subject_or_target(grp_raw.trim(), vocab)
    {
        return Ok(ConditionAst::AnyOfGroupMatches {
            group: Box::new(grp),
            predicate: pred_raw.trim().to_string(),
        });
    }
    // Pass E12: Structured typed conditions replacing the stringly EventOccurredThisTurn dump
    if let Some(typed_cond) = parse_typed_condition_e12(&low, vocab) {
        return Ok(typed_cond);
    }
    if matches!(
        low.as_str(),
        "you cycled two or more cards this turn"
            | "you didn't activate a loyalty ability of a planeswalker this turn"
            | "a giant, wizard, or spell you controlled dealt damage to it this turn"
            | "it wasn't the first land you played this turn"
            | "~ started the turn untapped"
            | "that creature had to attack this combat"
            | "none of them were cast or no mana was spent to cast them"
            | "it wasn't put onto the battlefield with this ability"
            | "you guessed correctly for a card named ~"
            | "it's at least one of the chosen colors"
            | "one or more players being attacked are poisoned"
            | "that player controls one or more lands with contested counters on them"
            | "two or more permanents you don't control have an aim counter on them"
            | "~ is in your graveyard with three or more creature cards above it"
            | "three or more sources you controlled dealt damage this turn"
            | "you have an enduring story"
            | "they guessed right"
            | "that player pays only {1}"
            | "that library contains exactly the chosen number of cards with the chosen name"
            | "you've cast a spell named peer through depths and a spell named reach through mists this turn"
            | "you've surveilled this turn"
            | "you win the bidding"
            | "no creature got votes"
            | "you do and your opponent guessed wrong"
            | "you are one of those players"
            | "each opponent chose snitch"
            | "each opponent chose silence"
            | "none of those creatures attacked you"
            | "they guessed wrong"
            | "the exploited creature had power 3 or greater"
            | "the card a player revealed has the name they chose"
            | "any player pays {2}"
            | "you've done all four this turn"
            | "that land had a nonbasic land type"
            | "it's colorless or it's only the color its controller chose"
            | "it has the same name as a spell that was cast this turn"
            | "there are eight or more different names among unlocked doors of rooms you control"
            | "there are twenty or more counters on it or you have twenty or more cards in hand"
            | "an oil counter was removed from a permanent you controlled this turn or a permanent with an oil counter on it was put into a graveyard this turn"
            | "three or more cards left your graveyard this turn or if you've sacrificed a food this turn"
            | "your life total is greater than or equal to the last noted life total for ~"
            | "you control one or more creatures with a name you noted for cards named ~"
            | "the chosen color is the most common color among nontoken permanents the chosen player controls but isn't tied for most common"
            | "a permanent you controlled or a token was destroyed this way"
    ) {
        return Ok(ConditionAst::EventOccurredThisTurn { event: low.clone() });
    }

    // Tabletop Honor-System physical/social condition fallback
    if is_physical_tabletop_clause(&low) {
        return Ok(ConditionAst::ManualTabletopCheck {
            prompt: raw.to_string(),
        });
    }

    Err(format!("unparsed_condition:{raw}"))
}

/// Strictly parse a mana specification (`{G}`, `{W} or {U}`, `one mana of any color`, etc.).
pub fn parse_mana_spec(phrase: &str, vocab: &Vocabulary) -> Result<ManaSpecAst, String> {
    let raw = trim_punct(phrase);
    if is_mana_sequence(raw) {
        return Ok(ManaSpecAst::Fixed(raw.to_string()));
    }
    // Multi-way pure mana sequence choice FIRST: "{W} or {U}", "{R}, {G}, or {W}", "{W} or {U} or {B}"
    let normalized_choice = raw.replace(", or ", " or ").replace(", ", " or ");
    let parts: Vec<&str> = normalized_choice
        .split(" or ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 && parts.iter().all(|p| is_mana_sequence(p)) {
        return Ok(ManaSpecAst::Choice(
            parts.into_iter().map(ToString::to_string).collect(),
        ));
    }
    // Multi-way generalized mana spec choice: "X {G} or X {W}", "{W}, {U}, or one mana of the chosen color", etc.
    if parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.contains(" or ") && parse_mana_spec(p, vocab).is_ok())
    {
        return Ok(ManaSpecAst::Choice(
            parts.into_iter().map(ToString::to_string).collect(),
        ));
    }
    // "<value_expr> {M}" e.g. "six {R}", "three {G}", "twice that much {G}", "that much {R}", "X {B}"
    if let Some((amt_raw, sym)) = raw.rsplit_once(' ')
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_value_expr(amt_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }
    if let Some(after) = raw.to_ascii_lowercase().strip_prefix("an amount of ")
        && let Some((sym_part, val_raw)) = after.split_once(" equal to ")
        && let Ok(amt) = parse_value_expr(val_raw.trim(), vocab)
    {
        let sym_orig = raw["an amount of ".len()..][..sym_part.len()].trim();
        if is_mana_sequence(sym_orig) {
            return Ok(ManaSpecAst::ScaledSymbol {
                symbol: sym_orig.to_string(),
                amount: Box::new(amt),
            });
        }
    }

    let low = raw.to_ascii_lowercase();
    if let Some((sym, _)) = raw.split_once(" or one mana of the chosen color")
        && is_mana_sequence(sym.trim())
    {
        return Ok(ManaSpecAst::Choice(vec![
            sym.trim().to_string(),
            "chosen_color".to_string(),
        ]));
    }
    if low == "one mana of the chosen color" || low == "one mana of that color" {
        return Ok(ManaSpecAst::ChosenColor);
    }
    if low == "one mana of any color in your commander's color identity" {
        return Ok(ManaSpecAst::CommanderColorIdentity);
    }
    if low == "one mana of any type that a land you control could produce"
        || low == "one mana of any color that a land you control could produce"
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::You,
        });
    }
    if low == "one mana of any type that a land an opponent controls could produce"
        || low == "one mana of any color that a land an opponent controls could produce"
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::Opponent,
        });
    }
    // Cluster 11: Generalized lossless `AnyObjectCouldProduce` (Gond Gate, etc.)
    if let Some((any_type, rest)) = low
        .strip_prefix("one mana of any type that ")
        .map(|r| (true, r))
        .or_else(|| {
            low.strip_prefix("one mana of any color that ")
                .map(|r| (false, r))
        })
        && let Some(flt_raw) = rest.strip_suffix(" could produce")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::AnyObjectCouldProduce {
            filter: Box::new(flt),
            any_type,
        });
    }
    // Cluster 11: Lossless `AnyColorAmong` (Mox Amber, Pit of Offerings, etc.)
    if let Some(subj_raw) = low.strip_prefix("one mana of any color among ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::AnyColorAmong {
            source: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("one mana of any of ").and_then(|r| {
        r.strip_suffix("'s colors")
            .or_else(|| r.strip_suffix("' colors"))
    }) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::AnyColorAmong {
            source: Box::new(subj),
        });
    }
    if let Some((sym, fe_raw)) = raw.split_once(" for each ")
        && is_mana_sequence(sym.trim())
        && fe_raw
            .to_ascii_lowercase()
            .contains("counter removed this way")
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(ValueExprAst::PriorResult),
        });
    }
    if let Some((amt_raw, syms_raw)) = raw.split_once(" mana in any combination of ")
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        let cleaned = syms_raw
            .replace(" and/or ", " ")
            .replace(" or ", " ")
            .replace(", ", " ");
        let syms: Vec<String> = cleaned
            .split_whitespace()
            .filter(|t| is_mana_sequence(t))
            .map(ToString::to_string)
            .collect();
        if !syms.is_empty() {
            return Ok(ManaSpecAst::CombinationOf {
                amount: Box::new(amt),
                symbols: syms,
            });
        }
    }
    if let Some(amt_str) = low.strip_suffix(" mana of different colors")
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::DifferentColors {
            amount: Box::new(amt),
        });
    }
    // "one mana of any [one] color for each <fe>" (Coalition Relic, etc.)
    if let Some((left, fe_raw)) = low.split_once(" for each ")
        && (left.ends_with(" mana of any one color")
            || left.ends_with(" mana of any color")
            || left.ends_with(" mana in any combination of colors"))
        && let Ok(scale) = parse_for_each_value_expr(fe_raw, vocab)
    {
        let single_color = left.ends_with(" mana of any one color");
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(scale),
            single_color,
        });
    }
    if let Some(amt_str) = low
        .strip_suffix(" mana of any one color")
        .or_else(|| low.strip_suffix(" mana of any color"))
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amt),
            single_color: low.contains("any one color") || low.starts_with("one "),
        });
    }
    if let Some(amt_str) = low.strip_suffix(" mana in any combination of colors")
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amt),
            single_color: false,
        });
    }
    if let Some(after) = raw.strip_prefix("an amount of ")
        && let Some((sym, val_raw)) = after.split_once(" equal to ")
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }
    if low == "an amount of mana of that color equal to your devotion to that color" {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: "chosen_color".to_string(),
            amount: Box::new(ValueExprAst::Devotion {
                colors: "chosen_color".to_string(),
            }),
        });
    }
    // "that much {M}" (Rasputin, the Oneiromancer)
    if let Some(sym) = raw.strip_prefix("that much ")
        && is_mana_sequence(sym.trim())
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(ValueExprAst::PriorResult),
        });
    }
    // "{B} or {G} for each <fe>" (Culling Ritual, etc.)
    if let Some((left_mana, fe_raw)) = raw.split_once(" for each ")
        && let Ok(amt) = parse_for_each_value_expr(fe_raw, vocab)
    {
        let normalized_choice = left_mana.replace(", or ", " or ").replace(", ", " or ");
        let syms: Vec<String> = normalized_choice
            .split(" or ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(ToString::to_string)
            .collect();
        if syms.len() >= 2 && syms.iter().all(|p| is_mana_sequence(p)) {
            return Ok(ManaSpecAst::CombinationOf {
                amount: Box::new(amt),
                symbols: syms,
            });
        }
    }
    if let Some((sym, fe_raw)) = raw.split_once(" for each ")
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_for_each_value_expr(fe_raw, vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }

    Err(format!("unparsed_mana:{raw}"))
}

const PREDEFINED_TOKENS: &[&str] = &[
    "treasure",
    "food",
    "clue",
    "blood",
    "map",
    "powerstone",
    "junk",
    "gold",
    "shard",
    "incubator",
    "lander",
    "mutagen",
    "walker",
    "role",
    "heartwood",
    "vibranium",
];

/// Strictly parse the object of `create ...` into a typed `TokenSpecAst`.
/// Rich token creation parse result preserving blocking status, dynamic P/T expressions, and chosen color/type qualifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichTokenCreationAst {
    pub token: TokenSpecAst,
    pub blocking: bool,
    pub dynamic_power: Option<ValueExprAst>,
    pub dynamic_toughness: Option<ValueExprAst>,
    pub dynamic_pt_text: Option<String>,
    pub chosen_color: bool,
    pub chosen_type: bool,
    pub attacking_target: Option<SubjectRefAst>,
}

impl RichTokenCreationAst {
    /// True when no extra qualifiers beyond plain `TokenSpecAst` are attached.
    #[must_use]
    pub const fn is_plain(&self) -> bool {
        !self.blocking
            && self.dynamic_power.is_none()
            && self.dynamic_toughness.is_none()
            && self.dynamic_pt_text.is_none()
            && !self.chosen_color
            && !self.chosen_type
            && self.attacking_target.is_none()
    }
}

pub fn parse_token_creation(phrase: &str, vocab: &Vocabulary) -> Result<TokenSpecAst, String> {
    parse_token_creation_rich(phrase, vocab).map(|r| r.token)
}

pub fn parse_token_creation_rich(
    phrase: &str,
    vocab: &Vocabulary,
) -> Result<RichTokenCreationAst, String> {
    let raw = trim_punct(phrase);
    let mut low = raw.to_ascii_lowercase();
    let mut tapped = false;
    let mut attacking = false;
    let mut blocking = false;
    let mut attacking_target: Option<SubjectRefAst> = None;
    let parse_atk_target = |tail: &str| -> Option<SubjectRefAst> {
        let t = tail.trim();
        if t.is_empty() {
            return None;
        }
        if let Some(p) = parse_player_ref(t) {
            return Some(SubjectRefAst::Player(p));
        }
        parse_subject_or_target(t, vocab).ok()
    };

    if let Some((before, after)) = low
        .split_once(" that's tapped and attacking")
        .or_else(|| low.split_once(" that are tapped and attacking"))
    {
        tapped = true;
        attacking = true;
        attacking_target = parse_atk_target(after);
        low = before.to_string();
    } else if low.contains(" tapped and attacking") {
        tapped = true;
        attacking = true;
        for (suffix, tgt) in [
            (
                " tapped and attacking that player",
                Some(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
            ),
            (
                " tapped and attacking defending player",
                Some(SubjectRefAst::Player(PlayerRefAst::DefendingPlayer)),
            ),
            (
                " tapped and attacking you",
                Some(SubjectRefAst::Player(PlayerRefAst::You)),
            ),
            (
                " tapped and attacking that opponent",
                Some(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
            ),
            (
                " tapped and attacking an opponent",
                Some(SubjectRefAst::Player(PlayerRefAst::AnyOpponent)),
            ),
            (" tapped and attacking", None),
        ] {
            if let Some(stripped) = low.strip_suffix(suffix) {
                attacking_target = tgt;
                low = stripped.to_string();
                break;
            }
        }
        if low.contains(" tapped and attacking") {
            low = low.replace(" tapped and attacking", "");
        }
    }
    if let Some((before, after)) = low
        .split_once(" that's attacking")
        .or_else(|| low.split_once(" that are attacking"))
    {
        attacking = true;
        if attacking_target.is_none() {
            attacking_target = parse_atk_target(after);
        }
        low = before.to_string();
    } else {
        for (pat, tgt) in [
            (
                " attacking that opponent",
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
            ),
            (
                " attacking that player",
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
            ),
            (
                " attacking defending player",
                SubjectRefAst::Player(PlayerRefAst::DefendingPlayer),
            ),
            (
                " attacking an opponent",
                SubjectRefAst::Player(PlayerRefAst::AnyOpponent),
            ),
            (" attacking you", SubjectRefAst::Player(PlayerRefAst::You)),
        ] {
            if let Some((before, _)) = low.split_once(pat) {
                attacking = true;
                if attacking_target.is_none() {
                    attacking_target = Some(tgt);
                }
                low = before.to_string();
                break;
            }
        }
    }
    let wrap_plain = |token: TokenSpecAst, blk: bool| RichTokenCreationAst {
        token,
        blocking: blk,
        dynamic_power: None,
        dynamic_toughness: None,
        dynamic_pt_text: None,
        chosen_color: false,
        chosen_type: false,
        attacking_target: attacking_target.clone(),
    };
    if let Some((before, _)) = low
        .split_once(" that's blocking")
        .or_else(|| low.split_once(" that are blocking"))
    {
        blocking = true;
        low = before.to_string();
    }
    if low.contains(" that's tapped") {
        tapped = true;
        low = low.replace(" that's tapped", "");
    } else if low.contains(" tapped") {
        tapped = true;
        low = low.replace(" tapped", "");
    }
    let low = low.trim();

    // "N/X of those tokens" / "another one of those tokens" (Wurmquake, etc.)
    if low == "another one of those tokens" {
        return Ok(wrap_plain(
            TokenSpecAst::Predefined {
                name: "those_tokens".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                tapped,
            },
            blocking,
        ));
    }
    if let Some(cnt_part) = low.strip_suffix(" of those tokens")
        && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
    {
        return Ok(wrap_plain(
            TokenSpecAst::Predefined {
                name: "those_tokens".to_string(),
                count: Box::new(cnt),
                tapped,
            },
            blocking,
        ));
    }

    // "half X <desc> tokens, rounded up/down" (The Goose Mother, etc.)
    if let Some(after_half) = low.strip_prefix("half ")
        && let Some((tok_side, round_raw)) = after_half.split_once(", rounded ")
        && let Some((expr_tok, rest_tok)) = tok_side.split_once(' ')
        && let Ok(inner_val) = parse_value_expr(expr_tok, vocab)
        && let Ok(mut tok) = parse_token_creation(&format!("a {}", rest_tok.trim()), vocab)
    {
        let div_val = ValueExprAst::DivConst {
            expr: Box::new(inner_val),
            denom: 2,
            rounding: if round_raw.trim() == "up" {
                crate::ast::Rounding::Up
            } else {
                crate::ast::Rounding::Down
            },
        };
        match &mut tok {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => **count = div_val,
            TokenSpecAst::Role { .. } => {}
        }
        return Ok(wrap_plain(tok, blocking));
    }

    // Copy token: "a token that's a copy of <subject> [tapped and attacking ...][, except <exception>]"
    if let Some((cnt_part, rest)) = low
        .split_once(" token that's a copy of ")
        .or_else(|| low.split_once(" tokens that are copies of "))
    {
        let (src_raw_full, exception) = match rest.split_once(", except ") {
            Some((s, exc)) => (s, Some(exc.trim().to_string())),
            None => (rest, None),
        };
        let src_raw = src_raw_full
            .split_once(" tapped and attacking")
            .or_else(|| src_raw_full.split_once(" and that's blocking"))
            .map_or(src_raw_full, |(s, _)| s)
            .trim();
        if let (Ok(cnt), Ok(subj)) = (
            parse_value_expr(cnt_part, vocab),
            parse_subject_or_target(src_raw, vocab),
        ) {
            return Ok(wrap_plain(
                TokenSpecAst::Copy {
                    count: Box::new(cnt),
                    source: Box::new(subj),
                    tapped: tapped || src_raw_full.contains("tapped"),
                    attacking: attacking || src_raw_full.contains("attacking"),
                    exception,
                },
                blocking || src_raw_full.contains("blocking"),
            ));
        }
    }
    // Dynamic P/T creature token: "<cnt> <colors> <subtypes> creature token with [base] power and toughness each equal to <val>"
    // or "with power equal to <v1> and toughness equal to <v2>" (Skullspore Nexus, Soul Separator)
    if let Some((left_desc, rhs, is_each_equal)) = low
        .split_once(" creature token with base power and toughness each equal to ")
        .map(|(l, r)| (l, r, true))
        .or_else(|| {
            low.split_once(" creature token with power and toughness each equal to ")
                .map(|(l, r)| (l, r, true))
        })
        .or_else(|| {
            low.split_once(" creature token with power equal to ")
                .map(|(l, r)| (l, r, false))
        })
        && let Some((cnt_tok, rest_desc)) = left_desc.split_once(' ')
        && let Ok(cnt) = parse_value_expr(cnt_tok, vocab)
    {
        let mut colors = ColorMask::EMPTY;
        let mut supertypes = SupertypeMask::EMPTY;
        let mut card_types = CardTypeMask::CREATURE;
        let mut subtypes = Vec::new();
        for w in rest_desc.split_whitespace() {
            let tok = w.trim_matches(',');
            if tok == "and" || tok == "colorless" {
                continue;
            }
            if let Some(c) = lookup_color(tok) {
                colors = colors.union(c);
            } else if let Some(st) = lookup_supertype(tok) {
                supertypes = supertypes.union(st);
            } else if let Some(ct) = lookup_card_type(tok) {
                card_types = card_types.union(ct);
            } else if let Some(sub) = vocab.lookup_subtype(tok) {
                subtypes.push(sub.to_string());
            } else {
                subtypes.push(tok.to_string());
            }
        }
        let (rhs_clean, extra_kws) = if let Some((expr_part, kw_part)) = rhs
            .split_once(" that has ")
            .or_else(|| rhs.split_once(" with "))
        {
            let kws = parse_keyword_list(kw_part.trim(), vocab)
                .unwrap_or_else(|| vec![kw_part.trim().to_string()]);
            (expr_part.trim(), kws)
        } else {
            (rhs.trim(), Vec::new())
        };
        let (dyn_p, dyn_t, dyn_text) = if is_each_equal {
            match parse_value_expr(rhs_clean, vocab) {
                Ok(v) => (Some(v.clone()), Some(v), None),
                Err(_) => (None, None, Some(rhs_clean.to_string())),
            }
        } else if let Some((p_raw, t_raw)) = rhs_clean.split_once(" and toughness equal to ") {
            let pv = parse_value_expr(p_raw.trim(), vocab).ok();
            let tv = parse_value_expr(t_raw.trim(), vocab).ok();
            let txt = if pv.is_none() || tv.is_none() {
                Some(rhs_clean.to_string())
            } else {
                None
            };
            (pv, tv, txt)
        } else {
            match parse_value_expr(rhs_clean, vocab) {
                Ok(v) => (Some(v), None, None),
                Err(_) => (None, None, Some(rhs_clean.to_string())),
            }
        };
        return Ok(RichTokenCreationAst {
            token: TokenSpecAst::Creature {
                count: Box::new(cnt),
                power: "X".to_string(),
                toughness: "X".to_string(),
                colors,
                supertypes,
                card_types,
                subtypes,
                keywords: extra_kws,
                tapped,
                attacking,
                named: None,
            },
            blocking,
            dynamic_power: dyn_p,
            dynamic_toughness: dyn_t,
            dynamic_pt_text: dyn_text,
            chosen_color: false,
            chosen_type: false,
            attacking_target,
        });
    }

    // Peel optional trailing "that's all colors" or "that's <colors>" (Godsire)
    let (low, all_colors, extra_colors) =
        if let Some(stripped) = low.strip_suffix(" that's all colors") {
            (stripped.trim(), true, ColorMask::EMPTY)
        } else if let Some((before, color_list)) = low.split_once(" that's ") {
            let mut parsed_c = ColorMask::EMPTY;
            let mut all_valid = true;
            for w in color_list.split_whitespace() {
                let tok = w.trim_matches(',');
                if tok == "and" {
                    continue;
                }
                if let Some(c) = lookup_color(tok) {
                    parsed_c = parsed_c.union(c);
                } else {
                    all_valid = false;
                    break;
                }
            }
            if all_valid && !parsed_c.is_empty() {
                (before.trim(), false, parsed_c)
            } else {
                (low, false, ColorMask::EMPTY)
            }
        } else {
            (low, false, ColorMask::EMPTY)
        };

    // Peel optional trailing "for each ..." ONLY when not inside a quoted token ability
    let (body, for_each_expr) = if !low.contains('"')
        && let Some((b, fe)) = low.split_once(" for each ")
    {
        let fe_val = parse_for_each_value_expr(fe, vocab)?;
        (b.trim(), Some(fe_val))
    } else {
        (low, None)
    };

    // Wilds of Eldraine Role tokens: "a <RoleName> Role token attached to <target>"
    if let Some((role_part, att_raw)) = low.split_once(" role token attached to ")
        && let Ok(attached_to) = parse_subject_or_target(att_raw, vocab)
    {
        let role_name = role_part
            .strip_prefix("a ")
            .or_else(|| role_part.strip_prefix("an "))
            .unwrap_or(role_part)
            .trim()
            .to_string();
        if !role_name.is_empty() {
            return Ok(wrap_plain(
                TokenSpecAst::Role {
                    role_name,
                    attached_to: Box::new(attached_to),
                },
                blocking,
            ));
        }
    }

    // Predefined token: "a Treasure token", "two Food tokens"
    for &ptok in PREDEFINED_TOKENS {
        if let Some(cnt_str) = body
            .strip_suffix(&format!(" {ptok} token"))
            .or_else(|| body.strip_suffix(&format!(" {ptok} tokens")))
        {
            let base_cnt = parse_value_expr(cnt_str, vocab)?;
            let count = for_each_expr.unwrap_or(base_cnt);
            let mut chars = ptok.chars();
            let cap = match chars.next() {
                Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            };
            return Ok(wrap_plain(
                TokenSpecAst::Predefined {
                    name: cap,
                    count: Box::new(count),
                    tapped,
                },
                blocking,
            ));
        }
    }

    // "a number of <desc> tokens equal to <ValueExpr>" (Goblin Gathering, etc.)
    if let Some(after_num) = body.strip_prefix("a number of ")
        && let Some((tok_side, val_raw)) = after_num.split_once(" equal to ")
        && let Ok(eq_val) = parse_value_expr(val_raw, vocab)
        && let Ok(mut inner) = parse_token_creation(&format!("a {}", tok_side.trim()), vocab)
    {
        match &mut inner {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => **count = eq_val,
            TokenSpecAst::Role { .. } => {}
        }
        return Ok(wrap_plain(inner, blocking));
    }

    // Prefix-named token: "<Name>, a [legendary] [P/T] <desc> token [with <kws/abilities>]"
    // (e.g. "Voja, a legendary 2/2 green and white Wolf creature token",
    // "Ashaya, the Awoken World, a legendary 4/4 green Elemental creature token",
    // "~'s Notebook, a legendary colorless Book artifact token with \"...\"")
    let (body, prefix_named) = {
        let split_before_tok = body.find(" token").unwrap_or(body.len());
        let search_slice = &body[..split_before_tok];
        if let Some(idx) = search_slice
            .rfind(", a ")
            .or_else(|| search_slice.rfind(", an "))
        {
            let name_part = body[..idx].trim();
            let rest_part = body[idx + 2..].trim();
            if !name_part.is_empty() && !name_part.contains('"') {
                (rest_part, Some(name_part.to_string()))
            } else {
                (body, None)
            }
        } else {
            (body, None)
        }
    };

    // "<count> P/T creature token(s) of the chosen color and type" (Volrath's Laboratory, Riptide Replicator)
    if let Some((before, _)) = body.split_once(" of the chosen color and type")
        && let Ok(mut inner) = parse_token_creation_rich(before.trim(), vocab)
    {
        inner.blocking = inner.blocking || blocking;
        inner.chosen_color = true;
        inner.chosen_type = true;
        return Ok(inner);
    }

    // Standard creature or noncreature token: "<count> [legendary] [<P>/<T>] <desc> token(s) [named <Name>] [with <kws>] [named <Name>]"
    let (core, mut named, mut kw_tail) = if let Some((before, after)) = body
        .split_once(" token named ")
        .or_else(|| body.split_once(" tokens named "))
    {
        match after.split_once(" with ") {
            Some((n, kw)) => (before, Some(n.trim().to_string()), Some(kw)),
            None => (before, Some(after.trim().to_string()), None),
        }
    } else if let Some((before, after)) = body.split_once(" token with ") {
        (before, None, Some(after))
    } else if let Some((before, after)) = body.split_once(" tokens with ") {
        (before, None, Some(after))
    } else if let Some(before) = body
        .strip_suffix(" tokens")
        .or_else(|| body.strip_suffix(" token"))
    {
        (before, None, None)
    } else {
        return Err(format!("unparsed_token:{raw}"));
    };
    if named.is_none() {
        named = prefix_named;
    }
    if named.is_none()
        && let Some(kt) = kw_tail
        && !kt.contains('"')
        && let Some((kw_only, name_part)) = kt.split_once(" named ")
    {
        kw_tail = Some(kw_only.trim());
        named = Some(name_part.trim().to_string());
    }

    let words: Vec<&str> = core.split_whitespace().collect();
    if words.len() < 2 {
        return Err(format!("unparsed_token_short:{raw}"));
    }

    // Handle "twice that many" / "half that many" / "that many" / "twice x" or single-word count
    let (count_expr, mut cursor) =
        if words.len() >= 4 && words[0] == "twice" && words[1] == "that" && words[2] == "many" {
            (
                ValueExprAst::Mul(
                    Box::new(ValueExprAst::Const(2)),
                    Box::new(ValueExprAst::PriorResult),
                ),
                3,
            )
        } else if words.len() >= 4 && words[0] == "half" && words[1] == "that" && words[2] == "many"
        {
            (
                ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::PriorResult),
                    denom: 2,
                    rounding: Rounding::Down,
                },
                3,
            )
        } else if words.len() >= 3 && words[0] == "that" && words[1] == "many" {
            (ValueExprAst::PriorResult, 2)
        } else if words.len() >= 3 && words[0] == "twice" && words[1] == "x" {
            (
                ValueExprAst::Mul(
                    Box::new(ValueExprAst::Const(2)),
                    Box::new(ValueExprAst::ChosenX),
                ),
                2,
            )
        } else {
            (parse_value_expr(words[0], vocab)?, 1)
        };

    let mut colors = ColorMask::EMPTY;
    let mut supertypes = SupertypeMask::EMPTY;
    let mut card_types = CardTypeMask::EMPTY;
    let mut subtypes = Vec::new();

    // Peel optional leading supertypes (e.g. "a legendary 2/2 ..." or "a legendary colorless Book artifact")
    while cursor < words.len() {
        if let Some(st) = lookup_supertype(words[cursor].trim_matches(',')) {
            supertypes = supertypes.union(st);
            cursor += 1;
        } else {
            break;
        }
    }

    let (power, toughness) = if cursor < words.len()
        && let Some((p, t)) = parse_base_pt(words[cursor])
    {
        cursor += 1;
        (p, t)
    } else {
        (String::new(), String::new())
    };

    for &w in &words[cursor..] {
        let tok = w.trim_matches(',');
        if tok == "and" || tok == "colorless" {
            continue;
        }
        if let Some(c) = lookup_color(tok) {
            colors = colors.union(c);
        } else if let Some(st) = lookup_supertype(tok) {
            supertypes = supertypes.union(st);
        } else if let Some(ct) = lookup_card_type(tok) {
            card_types = card_types.union(ct);
        } else if let Some(sub) = vocab.lookup_subtype(tok) {
            subtypes.push(sub.to_string());
        } else {
            return Err(format!("unparsed_token_desc_tok:{tok}"));
        }
    }
    if power.is_empty() && card_types.is_empty() {
        return Err(format!("unparsed_token_pt:{raw}"));
    }
    if all_colors {
        colors = ColorMask::WHITE
            .union(ColorMask::BLUE)
            .union(ColorMask::BLACK)
            .union(ColorMask::RED)
            .union(ColorMask::GREEN);
    } else if !extra_colors.is_empty() {
        colors = colors.union(extra_colors);
    }
    if card_types.is_empty() {
        card_types = CardTypeMask::CREATURE;
    }

    let mut keywords = Vec::new();
    if let Some(kt) = kw_tail {
        if kt.contains('"') {
            keywords.push(kt.to_string());
        } else if let Some(kws) = parse_keyword_list(kt, vocab) {
            keywords = kws;
        } else {
            return Err(format!("unparsed_token_kw:{kt}"));
        }
    }

    Ok(wrap_plain(
        TokenSpecAst::Creature {
            count: Box::new(for_each_expr.unwrap_or(count_expr)),
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        },
        blocking,
    ))
}

/// Split a keyword line on `,` or `;` outside quotes.
#[must_use]
pub fn split_keyword_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for seg in split_top_level(line, ";") {
        for part in split_top_level(seg, ",") {
            if !part.is_empty() {
                out.push(part);
            }
        }
    }
    out
}

fn split_named_card_list(names_orig: &str) -> Vec<String> {
    let s = names_orig.trim();
    if let Some((head, last)) = s.rsplit_once(", and ") {
        let mut out: Vec<String> = head
            .split(", ")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if !last.trim().is_empty() {
            out.push(last.trim().to_string());
        }
        return out;
    }
    if s.contains(" and ") {
        return s
            .split(" and ")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
    }
    vec![s.to_string()]
}

/// Preserves lossless typed flags, name filters, and structured predicates for relative suffixes
/// stripped in `REL_SUFFIXES` (Pass E11: eliminates silent `ObjectFilterFlags::EMPTY` drops and
/// splits `named ~` / `same name` out of `MATCHES_NAMED_AGENDA`).
fn apply_rel_suffix_predicate(
    suf: &str,
    flag: ObjectFilterFlags,
    combat: Option<CombatState>,
    flt: &mut ObjectFilterAst,
) {
    match suf {
        " that aren't of the chosen type" | " that isn't of the chosen type" => {
            flt.predicates
                .push(FilterPredicateAst::MatchesChosen { negated: true });
        }
        " that shares a creature type with ~" => {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::SelfObject),
                negated: false,
            });
        }
        " that shares a color with ~" => {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "color".to_string(),
                reference: Box::new(SubjectRefAst::SelfObject),
                negated: false,
            });
        }
        " that shares a creature type with it" | " that share a creature type with it" => {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                negated: false,
            });
        }
        " that shares a color with it" | " that share a color with it" => {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "color".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                negated: false,
            });
        }
        " that shares a card type with it" => {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "card_type".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                negated: false,
            });
        }
        " that share a creature type" | " that share a color" | " that share a card type" => {
            let prop = if suf.contains("creature type") {
                "creature_type"
            } else if suf.contains("card type") {
                "card_type"
            } else {
                "color"
            };
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: prop.to_string(),
                reference: Box::new(SubjectRefAst::Anaphora("group".to_string())),
                negated: false,
            });
        }
        " of each of the sacrificed creature's colors" => {
            flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
                property: "color".to_string(),
                reference: Box::new(SubjectRefAst::Anaphora(
                    "the sacrificed creature".to_string(),
                )),
                negated: false,
            });
        }
        " named ~" => {
            flt.named = Some(NameFilterAst::SelfName);
        }
        " with the same name" | " with that name" => {
            flt.named = Some(NameFilterAst::SameAsReferenced);
        }
        " with the same name as that land"
        | " with the same name as that creature"
        | " with the same name as that spell"
        | " with the same name as that permanent"
        | " with the same name as that card"
        | " with the same name as the exiled card"
        | " with the same name as a nontoken permanent"
        | " with the same name as another card in their hand" => {
            flt.named = Some(NameFilterAst::SameAsReferenced);
            flt.predicates.push(FilterPredicateAst::StateQualification(
                suf.trim().to_string(),
            ));
        }
        " with different names" => {
            flt.flags = flt.flags.union(ObjectFilterFlags::DIFFERENT_NAMES);
        }
        " with different mana values" => {
            flt.flags = flt.flags.union(ObjectFilterFlags::DIFFERENT_MANA_VALUES);
        }
        " with different colors" => {
            flt.flags = flt.flags.union(ObjectFilterFlags::DIFFERENT_COLORS);
        }
        " with different card types" => {
            flt.flags = flt.flags.union(ObjectFilterFlags::DIFFERENT_CARD_TYPES);
        }
        " with different powers" => {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "different_powers".to_string(),
            ));
        }
        " with {x} in its mana cost" | " with {x} in their mana costs" => {
            flt.predicates
                .push(FilterPredicateAst::HasManaSymbolInCost("{x}".to_string()));
        }
        " with a basic land type" => {
            flt.predicates.push(FilterPredicateAst::HasBasicLandType);
        }
        " with a mana ability" => {
            flt.predicates.push(FilterPredicateAst::HasAbilityKind {
                kind: "mana_ability".to_string(),
                negated: false,
            });
        }
        " sacrificed this way" | " they sacrificed this way" | " you sacrificed this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::SacrificedThisWay,
                ));
        }
        " exiled this way" | " they exiled this way" | " you exiled this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::ExiledThisWay,
                ));
        }
        " exiled from their hand this way" | " exiled from your hand this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::ExiledThisWay,
                ));
            flt.predicates.push(FilterPredicateAst::StateQualification(
                suf.trim().to_string(),
            ));
        }
        " revealed this way" | " they revealed this way" | " you revealed this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::RevealedThisWay,
                ));
        }
        " discarded this way" | " they discarded this way" | " you discarded this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::DiscardedThisWay,
                ));
        }
        " destroyed this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::DestroyedThisWay,
                ));
        }
        " returned this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::ReturnedToHandThisWay,
                ));
        }
        " tapped this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::TappedThisWay,
                ));
        }
        " put into a graveyard this way" => {
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::PutIntoGraveyardThisWay,
                ));
        }
        " milled this way"
        | " they milled this way"
        | " dealt damage this way"
        | " created this way"
        | " put onto the battlefield this way"
        | " with greater power"
        | " with power less than ~'s"
        | " with lesser mana value"
        | " with equal or lesser mana value"
        | " with lesser mana value than the creature that died"
        | " that don't have a name"
        | " that has an adventure"
        | " that have an adventure"
        | " cast from a graveyard"
        | " that's not historic"
        | " in your party"
        | " other than that creature"
        | " other than that permanent"
        | " other than that card"
        | " that didn't attack this turn"
        | " that were put there from the battlefield this turn"
        | " controlled by the same player"
        | " or suspended card" => {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                suf.trim().to_string(),
            ));
        }
        _ => {
            let _ = (flag, combat);
        }
    }
}

/// Pass E12: Parse previously string-dumped conditions into strictly typed `ConditionAst` nodes.
fn parse_typed_condition_e12(low: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    // 1. Color checks on referenced subject ("it's white" / "blue" / "black" / "red" / "green")
    if let Some(color_mask) = match low {
        "it's white" => Some(ColorMask::WHITE),
        "it's blue" => Some(ColorMask::BLUE),
        "it's black" => Some(ColorMask::BLACK),
        "it's red" => Some(ColorMask::RED),
        "it's green" => Some(ColorMask::GREEN),
        _ => None,
    } {
        return Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(ObjectFilterAst {
                any_colors: color_mask,
                ..Default::default()
            }),
        });
    }

    // 2. Subtype / count controls ("you control a dwarf", "you control five or more enchantments", "you control six or more shrines", "you control a forest and a plains")
    if let Some(rest) = low.strip_prefix("you control ") {
        if let Some((a_raw, b_raw)) = rest.split_once(" and ")
            && let (Ok(fa), Ok(fb)) = (
                parse_object_filter(a_raw.trim(), vocab),
                parse_object_filter(b_raw.trim(), vocab),
            )
        {
            return Some(ConditionAst::And(
                Box::new(ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter: Box::new(fa),
                    op: CompareOp::Ge,
                    value: 1,
                }),
                Box::new(ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter: Box::new(fb),
                    op: CompareOp::Ge,
                    value: 1,
                }),
            ));
        }
        if let Some((n_raw, flt_raw)) = rest.split_once(" or more ")
            && let Some(n) = parse_number_word(n_raw.trim())
            && let Ok(obj_flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(obj_flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        if rest == "a permanent of each color" {
            return Some(ConditionAst::ControlsEachColor(PlayerRefAst::You));
        }
        if let Ok(obj_flt) = parse_object_filter(rest.trim(), vocab) {
            return Some(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(obj_flt),
                op: CompareOp::Ge,
                value: 1,
            });
        }
    }

    match low {
        "~ dealt damage to another creature this turn" => Some(ConditionAst::DamageDealtThisTurn {
            source: Box::new(SubjectRefAst::SelfObject),
            target_filter: Some(Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                flags: ObjectFilterFlags::EXCLUDE_SELF,
                ..Default::default()
            })),
            combat_only: false,
            min_damage: Some(1),
        }),
        "equipped creature didn't deal combat damage to a creature this turn" => Some(
            ConditionAst::Not(Box::new(ConditionAst::DamageDealtThisTurn {
                source: Box::new(SubjectRefAst::Anaphora("equipped creature".to_string())),
                target_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                })),
                combat_only: true,
                min_damage: Some(1),
            })),
        ),
        "~ has dealt 3 or more damage this turn" => Some(ConditionAst::DamageDealtThisTurn {
            source: Box::new(SubjectRefAst::SelfObject),
            target_filter: None,
            combat_only: false,
            min_damage: Some(3),
        }),
        "that creature wasn't dealt damage this turn" => {
            Some(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                filter: Box::new(ObjectFilterAst {
                    flags: ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                    ..Default::default()
                }),
            })))
        }
        "three or more cards were put into your graveyard from anywhere other than the battlefield this turn" => {
            Some(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    ..Default::default()
                }),
                from_zone: None,
                to_zone: Some(ZoneFilter::YourGraveyard),
                controller: Some(PlayerRefAst::You),
                min_count: 3,
            })
        }
        "three or more creature cards were put into graveyards from anywhere this turn" => {
            Some(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_card: true,
                    ..Default::default()
                }),
                from_zone: None,
                to_zone: Some(ZoneFilter::AnyGraveyard),
                controller: None,
                min_count: 3,
            })
        }
        "a permanent was put into your hand from the battlefield this turn" => {
            Some(ConditionAst::ZoneChangeThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                from_zone: Some(ZoneFilter::Battlefield),
                to_zone: Some(ZoneFilter::YourHand),
                controller: Some(PlayerRefAst::You),
                min_count: 1,
            })
        }
        "that creature entered from a graveyard or you cast it from a graveyard" => {
            Some(ConditionAst::Or(
                Box::new(ConditionAst::ZoneChangeThisTurn {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                    from_zone: Some(ZoneFilter::AnyGraveyard),
                    to_zone: Some(ZoneFilter::Battlefield),
                    controller: None,
                    min_count: 1,
                }),
                Box::new(ConditionAst::WasCastFromZone {
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    zone: ZoneFilter::AnyGraveyard,
                }),
            ))
        }
        "that opponent has more life than another of your opponents" => {
            Some(ConditionAst::LifeComparePlayers {
                player: PlayerRefAst::ThatPlayer,
                op: CompareOp::Gt,
                other: PlayerRefAst::EachOtherOpponent,
            })
        }
        "that player has more life than each other player" => {
            Some(ConditionAst::LifeComparePlayers {
                player: PlayerRefAst::ThatPlayer,
                op: CompareOp::Gt,
                other: PlayerRefAst::EachOtherPlayer,
            })
        }
        "that spell was all colors" => Some(ConditionAst::AllColors(Box::new(
            SubjectRefAst::Anaphora("that spell".to_string()),
        ))),
        "that card was all colors" => Some(ConditionAst::AllColors(Box::new(
            SubjectRefAst::Anaphora("that card".to_string()),
        ))),
        "there are exactly two permanents named ~ on the battlefield" => {
            Some(ConditionAst::CardWithNameInZone {
                name_filter: NameFilterAst::SelfName,
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                zone: ZoneFilter::Battlefield,
                op: CompareOp::Eq,
                count: 2,
            })
        }
        "no permanents named ~ are on the battlefield" => Some(ConditionAst::CardWithNameInZone {
            name_filter: NameFilterAst::SelfName,
            filter: Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }),
            zone: ZoneFilter::Battlefield,
            op: CompareOp::Eq,
            count: 0,
        }),
        "you have no land cards in hand" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::LAND,
                any_card: true,
                zone: Some(ZoneFilter::YourHand),
                ..Default::default()
            }),
            op: CompareOp::Eq,
            value: 0,
        }),
        "you have seven or more land cards in your hand" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::LAND,
                any_card: true,
                zone: Some(ZoneFilter::YourHand),
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 7,
        }),
        "an instant card and a sorcery card are in your graveyard" => Some(ConditionAst::And(
            Box::new(ConditionAst::ZoneObjectCount {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::INSTANT,
                    any_card: true,
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            }),
            Box::new(ConditionAst::ZoneObjectCount {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::SORCERY,
                    any_card: true,
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            }),
        )),
        "you own a card in exile that has an adventure" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                owner: Some(ControllerRelation::You),
                predicates: vec![FilterPredicateAst::StateQualification(
                    "has_adventure".to_string(),
                )],
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 1,
        }),
        "a card exiled with ~ is a creature card" => Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 1,
        }),
        "that creature remains tapped" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            filter: Box::new(ObjectFilterAst {
                tapped: Some(true),
                ..Default::default()
            }),
        }),
        "that attraction is on the battlefield" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::Anaphora("that attraction".to_string())),
            zone: ZoneFilter::Battlefield,
        }),
        "that card remains on top of your library" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
            zone: ZoneFilter::YourLibrary,
        }),
        "that card remains exiled" => {
            Some(ConditionAst::Marker(GameMarker::SubjectRemainsExiled {
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                any_of: false,
            }))
        }
        "~ is in your opening hand" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::SelfObject),
            zone: ZoneFilter::YourHand,
        }),
        "~ is in your opening hand and you're not the starting player" => Some(ConditionAst::And(
            Box::new(ConditionAst::SubjectInZone {
                subject: Box::new(SubjectRefAst::SelfObject),
                zone: ZoneFilter::YourHand,
            }),
            Box::new(ConditionAst::Not(Box::new(ConditionAst::IsStartingPlayer(
                PlayerRefAst::You,
            )))),
        )),
        "you're not the starting player" => Some(ConditionAst::Not(Box::new(
            ConditionAst::IsStartingPlayer(PlayerRefAst::You),
        ))),
        "~ is in a graveyard" => Some(ConditionAst::SubjectInZone {
            subject: Box::new(SubjectRefAst::SelfObject),
            zone: ZoneFilter::AnyGraveyard,
        }),
        "you haven't cast a spell this turn" => Some(ConditionAst::Not(Box::new(
            ConditionAst::CastSpellThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(ObjectFilterAst {
                    any_spell: true,
                    ..Default::default()
                }),
            },
        ))),
        "two or more creatures are blocking it" => Some(ConditionAst::BlockedByFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            blocker_filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            }),
            min_count: 2,
        }),
        "they all share a color" => Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(SubjectRefAst::Anaphora("they".to_string())),
            other: Box::new(SubjectRefAst::Anaphora("they".to_string())),
            property: "color".to_string(),
        }),
        "it shares a card type with that permanent" => Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            other: Box::new(SubjectRefAst::Anaphora("that permanent".to_string())),
            property: "card_type".to_string(),
        }),
        "it shares a card type with that spell" => Some(ConditionAst::SharesPropertyWith {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            other: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
            property: "card_type".to_string(),
        }),
        "the top card of your library is an artifact or creature card" => {
            Some(ConditionAst::TopCardMatches {
                zone: ZoneFilter::YourLibrary,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE),
                    any_card: true,
                    ..Default::default()
                }),
            })
        }
        "the top card of your graveyard is a creature card" => Some(ConditionAst::TopCardMatches {
            zone: ZoneFilter::YourGraveyard,
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                ..Default::default()
            }),
        }),
        "it's suspected" => Some(ConditionAst::HasStatus {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            status: "suspected".to_string(),
        }),
        "any of them are suspected" => Some(ConditionAst::AnyOfGroupMatches {
            group: Box::new(SubjectRefAst::Anaphora("them".to_string())),
            predicate: "suspected".to_string(),
        }),
        "you don't control it" => Some(ConditionAst::Not(Box::new(
            ConditionAst::SubjectControlledBy {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                player: PlayerRefAst::You,
            },
        ))),
        "an opponent controls that creature" => Some(ConditionAst::SubjectControlledBy {
            subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            player: PlayerRefAst::AnyOpponent,
        }),
        "it came under your control this turn" => {
            Some(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                min_count: 1,
            })
        }
        "they lose the flip" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::ThatPlayer,
            outcome: "lose".to_string(),
        }),
        "both coins come up tails" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::You,
            outcome: "both_tails".to_string(),
        }),
        "both coins come up heads" => Some(ConditionAst::PlayerCoinFlipOutcome {
            player: PlayerRefAst::You,
            outcome: "both_heads".to_string(),
        }),
        "any of those results was 10 or higher" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: 10,
            any_of_group: true,
            exact_times: None,
        }),
        "you rolled 6 exactly seven times" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: 6,
            any_of_group: false,
            exact_times: Some(7),
        }),
        "the result is 3 or less" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value: 3,
            any_of_group: false,
            exact_times: None,
        }),
        "you rolled 6 or higher" => Some(ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: 6,
            any_of_group: false,
            exact_times: None,
        }),
        "the number is even" => Some(ConditionAst::NumberParity { even: true }),
        "the number is odd" => Some(ConditionAst::NumberParity { even: false }),
        "it isn't your main phase" => {
            Some(ConditionAst::Not(Box::new(ConditionAst::IsPhaseOrStep {
                player: Some(PlayerRefAst::You),
                phase: "main_phase".to_string(),
                first_only: false,
            })))
        }
        "it's the first end step of the turn" => Some(ConditionAst::IsPhaseOrStep {
            player: None,
            phase: "end_step".to_string(),
            first_only: true,
        }),
        "~'s sneak cost was paid" => Some(ConditionAst::PaidAltCost {
            subject: Box::new(SubjectRefAst::SelfObject),
            cost_kind: "sneak".to_string(),
        }),
        "this is the fourth time this ability has resolved this turn" => {
            Some(ConditionAst::ResolutionCountThisTurn { count: 4 })
        }
        "~'s power is negative" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::SelfObject),
                stat: StatKind::Power,
            }),
            op: CompareOp::Lt,
            rhs: Box::new(ValueExprAst::Const(0)),
        }),
        "~ is unblocked" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::SelfObject),
            filter: Box::new(ObjectFilterAst {
                combat: Some(CombatState::Unblocked),
                ..Default::default()
            }),
        }),
        "it doesn't have rampage" => Some(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            keyword: "rampage".to_string(),
        }))),
        "that creature has haste and ~ doesn't" => Some(ConditionAst::And(
            Box::new(ConditionAst::HasKeyword {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                keyword: "haste".to_string(),
            }),
            Box::new(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
                subject: Box::new(SubjectRefAst::SelfObject),
                keyword: "haste".to_string(),
            }))),
        )),
        "the card has no delay counters on it" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CounterCount {
                subject: Box::new(SubjectRefAst::Anaphora("the card".to_string())),
                counter_kind: "delay".to_string(),
            }),
            op: CompareOp::Eq,
            rhs: Box::new(ValueExprAst::Const(0)),
        }),
        "~ has seven or more ember counters on it" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CounterCount {
                subject: Box::new(SubjectRefAst::SelfObject),
                counter_kind: "ember".to_string(),
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(7)),
        }),
        "it has three or more ritual counters on it" => Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CounterCount {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "ritual".to_string(),
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(3)),
        }),
        "the last card exiled isn't a land card" => {
            Some(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("the last card exiled".to_string())),
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::LAND,
                    any_card: true,
                    ..Default::default()
                }),
            })))
        }
        "that permanent was a liliana planeswalker" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("that permanent".to_string())),
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::PLANESWALKER,
                subtypes: vec!["Liliana".to_string()],
                ..Default::default()
            }),
        }),
        "the revealed land card was a mountain" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora(
                "the revealed land card".to_string(),
            )),
            filter: Box::new(ObjectFilterAst {
                subtypes: vec!["Mountain".to_string()],
                ..Default::default()
            }),
        }),
        "it enters as a creature" => Some(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            }),
        }),
        "they search their library this way" => Some(ConditionAst::SearchedLibraryThisWay),
        "one or more of the chosen permanents are still on the battlefield" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "chosen_permanents_on_battlefield".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "one or more creature cards were put into that graveyard this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "put_into_graveyard_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "two or more cards are exiled this way" => Some(ConditionAst::PriorActionOccurred {
            action: "exiled_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            })),
            min_count: 2,
        }),
        "at least one creature card was exiled this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "exiled_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "two cards that share a color were milled this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "milled_sharing_color_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 2,
            })
        }
        "you gain control of a creature this way" => Some(ConditionAst::PriorActionOccurred {
            action: "gain_control_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "you lost life this way" => Some(ConditionAst::PriorActionOccurred {
            action: "lost_life_this_way".to_string(),
            subject_filter: None,
            min_count: 1,
        }),
        "you don't cast it this way" => Some(ConditionAst::Not(Box::new(
            ConditionAst::PriorActionOccurred {
                action: "cast_this_way".to_string(),
                subject_filter: None,
                min_count: 1,
            },
        ))),
        "you cast a spell this way" => Some(ConditionAst::PriorActionOccurred {
            action: "cast_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "you don't put it into your hand" => Some(ConditionAst::Not(Box::new(
            ConditionAst::PriorActionOccurred {
                action: "put_into_hand_this_way".to_string(),
                subject_filter: None,
                min_count: 1,
            },
        ))),
        "they didn't put the card on top of their library" => Some(ConditionAst::Not(Box::new(
            ConditionAst::PriorActionOccurred {
                action: "put_on_top_of_library_this_way".to_string(),
                subject_filter: None,
                min_count: 1,
            },
        ))),
        "you draw one or more cards this way" => Some(ConditionAst::PriorActionOccurred {
            action: "draw_this_way".to_string(),
            subject_filter: None,
            min_count: 1,
        }),
        "that player discards an artifact card this way" => {
            Some(ConditionAst::PriorActionOccurred {
                action: "discard_this_way".to_string(),
                subject_filter: Some(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::ARTIFACT,
                    any_card: true,
                    ..Default::default()
                })),
                min_count: 1,
            })
        }
        "that spell is countered this way" => Some(ConditionAst::PriorActionOccurred {
            action: "countered_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "those permanents are exchanged this way" => Some(ConditionAst::PriorActionOccurred {
            action: "exchanged_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        "that permanent is destroyed this way" => Some(ConditionAst::PriorActionOccurred {
            action: "destroyed_this_way".to_string(),
            subject_filter: Some(Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            })),
            min_count: 1,
        }),
        _ => None,
    }
}
