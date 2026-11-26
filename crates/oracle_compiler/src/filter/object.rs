//! Compositional `ObjectFilterAst` noun-phrase parser, stat/counter/keyword peelers, and
//! relative-clause predicate attachment.

use super::subject::{
    is_characteristic_choice_prefix, parse_keyword_list, parse_subject_or_target,
};
use super::value::parse_value_expr;
use super::zones::{peel_controller_suffix, peel_zone_suffix};
use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation,
    DraftLedgerQueryKind, FilterPredicateAst, NameFilterAst, ObjectFilterAst, ObjectFilterFlags,
    ObjectPartitionKind, PartitionMembershipAst, PlayerRefAst, PriorActionRefKind, StatBoundAst,
    StatKind, SubjectRefAst, SupertypeMask, ValueExprAst, ZoneFilter,
};
use crate::combinators::trim_punct;
use crate::preprocess::parse_base_pt;
use crate::vocab::{Vocabulary, lookup_card_type, lookup_color, lookup_supertype};

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
    if s == "a ~ planeswalker" || s == "~ planeswalker" {
        flt.any_types = CardTypeMask::PLANESWALKER;
        flt.predicates.push(FilterPredicateAst::StateQualification(
            "same_planeswalker_type_as_self".to_string(),
        ));
        return Ok(flt);
    }

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
        if let Some((prefix, same_as_raw)) = s
            .rsplit_once(" with the same name as ")
            .or_else(|| s.rsplit_once(" that have the same name as "))
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
            .or_else(|| s.strip_suffix(" among them"))
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
        if let Some((prefix, z)) = peel_zone_suffix(&s) {
            flt.zone = Some(z);
            s = prefix.to_string();
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
            (" of an opponent's choice ", PlayerRefAst::AnyOpponent),
            (" of their choice ", PlayerRefAst::ThatPlayer),
            (" of your choice ", PlayerRefAst::You),
        ] {
            if let Some((before_ch, after_ch)) = s.split_once(ch_sep)
                && !is_characteristic_choice_prefix(before_ch)
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
        for (ch_suf, ch_player) in [
            (" of that player's choice", PlayerRefAst::ThatPlayer),
            (" of an opponent's choice", PlayerRefAst::AnyOpponent),
            (" of their choice", PlayerRefAst::ThatPlayer),
            (" of your choice", PlayerRefAst::You),
        ] {
            if let Some(before_ch) = s.strip_suffix(ch_suf)
                && !is_characteristic_choice_prefix(before_ch)
            {
                flt.predicates
                    .push(FilterPredicateAst::ChosenByPlayer(ch_player));
                s = before_ch.trim().to_string();
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
        // Negated prior-action chosen-this-way clauses (positive prior-action verbs are peeled compositionally below)
        for pa_suf in [
            " that hasn't been chosen this way",
            " not chosen by any player this way",
            " that weren't chosen this way",
            " that hasn't been chosen",
        ] {
            if let Some(prefix) = s.strip_suffix(pa_suf)
                && !prefix.trim().is_empty()
            {
                flt.predicates
                    .push(FilterPredicateAst::AffectedByPriorAction(
                        PriorActionRefKind::ChosenThisWay { negated: true },
                    ));
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

        if let Some(m) = peel_controller_suffix(&s) {
            flt.controller = Some(m.relation);
            if m.is_past_tense {
                flt.flags = flt.flags.union(ObjectFilterFlags::LKI_PAST_TENSE);
            }
            s = m.prefix.to_string();
            continue;
        }

        // Compositional prior-action relative clauses:
        // " [that (was|were) |they |you |that player ](exiled|discarded|sacrificed|destroyed|revealed|returned|tapped|milled|created|goaded|dealt damage|put into a graveyard|put into your graveyard|put onto the battlefield) this way"
        if let Some(before_tw) = s.strip_suffix(" this way")
            && !before_tw.trim().is_empty()
        {
            const PRIOR_VERBS: &[(&str, Option<PriorActionRefKind>, Option<&str>)] = &[
                (
                    " put onto the battlefield",
                    None,
                    Some("put_onto_battlefield_this_way"),
                ),
                (
                    " put into your graveyard",
                    Some(PriorActionRefKind::PutIntoGraveyardThisWay),
                    None,
                ),
                (
                    " put into a graveyard",
                    Some(PriorActionRefKind::PutIntoGraveyardThisWay),
                    None,
                ),
                (" dealt damage", None, Some("dealt_damage_this_way")),
                (" exiled", Some(PriorActionRefKind::ExiledThisWay), None),
                (
                    " discarded",
                    Some(PriorActionRefKind::DiscardedThisWay),
                    None,
                ),
                (
                    " sacrificed",
                    Some(PriorActionRefKind::SacrificedThisWay),
                    None,
                ),
                (
                    " destroyed",
                    Some(PriorActionRefKind::DestroyedThisWay),
                    None,
                ),
                (" revealed", Some(PriorActionRefKind::RevealedThisWay), None),
                (
                    " returned",
                    Some(PriorActionRefKind::ReturnedToHandThisWay),
                    None,
                ),
                (" tapped", Some(PriorActionRefKind::TappedThisWay), None),
                (" milled", None, Some("milled_this_way")),
                (" created", None, Some("created_this_way")),
                (" goaded", None, Some("goaded_this_way")),
            ];
            let mut matched_prior = false;
            for (verb, kind_opt, zone_ref_opt) in PRIOR_VERBS {
                if let Some(before_verb) = before_tw.strip_suffix(verb) {
                    let prefix = before_verb
                        .strip_suffix(" that were")
                        .or_else(|| before_verb.strip_suffix(" that was"))
                        .or_else(|| before_verb.strip_suffix(" that player"))
                        .or_else(|| before_verb.strip_suffix(" they"))
                        .or_else(|| before_verb.strip_suffix(" you"))
                        .unwrap_or(before_verb);
                    if !prefix.trim().is_empty() {
                        if let Some(k) = kind_opt {
                            flt.predicates
                                .push(FilterPredicateAst::AffectedByPriorAction(k.clone()));
                        } else if let Some(zr) = zone_ref_opt {
                            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                                kind: (*zr).to_string(),
                            });
                        }
                        s = prefix.trim().to_string();
                        matched_prior = true;
                        break;
                    }
                }
            }
            if matched_prior {
                continue;
            }
        }
        // " that had a/an <kind> counter put on it this way"
        if let Some((prefix, rest)) = s.rsplit_once(" that had ")
            && let Some(kind_raw) = rest
                .strip_suffix(" counter put on it this way")
                .or_else(|| rest.strip_suffix(" counters put on it this way"))
            && !prefix.trim().is_empty()
        {
            let kind = kind_raw
                .trim()
                .strip_prefix("a ")
                .or_else(|| kind_raw.trim().strip_prefix("an "))
                .or_else(|| kind_raw.trim().strip_prefix("one or more "))
                .unwrap_or(kind_raw.trim());
            flt.predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::ReceivedCounterThisWay(kind.to_string()),
                ));
            s = prefix.trim().to_string();
            continue;
        }
        // Compositional " with power or toughness <N> or less/greater"
        if let Some((prefix, rest)) = s.rsplit_once(" with power or toughness ")
            && !prefix.trim().is_empty()
            && (rest.ends_with(" or less") || rest.ends_with(" or greater"))
        {
            flt.predicates
                .push(FilterPredicateAst::StateQualification(format!(
                    "with power or toughness {rest}"
                )));
            s = prefix.trim().to_string();
            continue;
        }
        // Compositional " with base power and toughness <P>/<T>"
        if let Some((prefix, pt_raw)) = s.rsplit_once(" with base power and toughness ")
            && !prefix.trim().is_empty()
            && parse_base_pt(pt_raw.trim()).is_some()
        {
            flt.predicates
                .push(FilterPredicateAst::StateQualification(format!(
                    "with base power and toughness {}",
                    pt_raw.trim()
                )));
            s = prefix.trim().to_string();
            continue;
        }

        // Compositional " [that (was|were|'s) ]attached to <subj>" peeler
        if !s.ends_with(" with another aura attached to it")
            && let Some((prefix, host_raw)) = s
                .rsplit_once(" that were attached to ")
                .or_else(|| s.rsplit_once(" that was attached to "))
                .or_else(|| s.rsplit_once(" that's attached to "))
                .or_else(|| s.rsplit_once(" attached to "))
            && !prefix.trim().is_empty()
            && !host_raw.trim().is_empty()
        {
            let h = host_raw.trim();
            if matches!(h, "~" | "it" | "them") {
                flt.flags = flt.flags.union(ObjectFilterFlags::ATTACHED_TO_SELF);
                s = prefix.trim().to_string();
                continue;
            } else if let Ok(host_subj) = parse_subject_or_target(h, vocab) {
                flt.predicates
                    .push(FilterPredicateAst::AttachedTo(Box::new(host_subj)));
                s = prefix.trim().to_string();
                continue;
            }
        }

        // Remaining non-decomposable idiomatic relative suffixes (sorted longest-first at call sites)
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
                " that weren't chosen this way by any player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in the pile", ObjectFilterFlags::EMPTY, None),
            (
                " that are instant cards, are sorcery cards, and/or have an adventure",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you controlled as you cast ~",
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
            (" other than ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (" with the same mana value", ObjectFilterFlags::EMPTY, None),
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
                ObjectFilterFlags::WITH_ANY_COUNTER,
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
                " attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" not named ~", ObjectFilterFlags::EXCLUDE_SELF, None),
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
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
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
            (" with a sticker on it", ObjectFilterFlags::EMPTY, None),
            (" that convoked ~", ObjectFilterFlags::EMPTY, None),
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
                " dealt damage by enchanted creature this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
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
                " previously dealt damage by it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " it has dealt damage to this game",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that aren't all colors", ObjectFilterFlags::EMPTY, None),
            (" that isn't all colors", ObjectFilterFlags::EMPTY, None),
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
            (
                " with the chosen name",
                ObjectFilterFlags::MATCHES_CHOSEN_NAME,
                None,
            ),
            (" named ~", ObjectFilterFlags::EMPTY, None),
            (" with different powers", ObjectFilterFlags::EMPTY, None),
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
            (" with {x} in its mana cost", ObjectFilterFlags::EMPTY, None),
            (" with a basic land type", ObjectFilterFlags::EMPTY, None),
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
                " of that type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE,
                None,
            ),
            (" with that name", ObjectFilterFlags::EMPTY, None),
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
            (" other than it", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " other than the chosen creature",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " each with power greater than its base power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with power greater than its base power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with power greater than that creature's base power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that weren't cast", ObjectFilterFlags::EMPTY, None),
            (" that convoked it", ObjectFilterFlags::EMPTY, None),
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
            (" that share a land type", ObjectFilterFlags::EMPTY, None),
            (" with enchant creature", ObjectFilterFlags::EMPTY, None),
            (" with enchant permanent", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking alone",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that fought this turn", ObjectFilterFlags::EMPTY, None),
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
            (" from a land source", ObjectFilterFlags::EMPTY, None),
            (
                " from a planeswalker source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with the same name", ObjectFilterFlags::EMPTY, None),
            (" that wasn't cast", ObjectFilterFlags::EMPTY, None),
            (
                " that weren't cast this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that wasn't cast this way", ObjectFilterFlags::EMPTY, None),
            (" with an even mana value", ObjectFilterFlags::EMPTY, None),
            (" with an odd mana value", ObjectFilterFlags::EMPTY, None),
            (" chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " other than enchanted creature",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " attacking enchanted player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that was put there from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (" that was exiled with ~", ObjectFilterFlags::EMPTY, None),
            (
                " with activated abilities that aren't mana abilities",
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
            (
                " dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" that blocked this turn", ObjectFilterFlags::EMPTY, None),
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
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (
                " that has a +1/+1 counter on it",
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (
                " with ice counters on them",
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (" it can enchant", ObjectFilterFlags::EMPTY, None),
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
            " that couldn't attack",
            " you attacked with this turn",
            " that were put into your graveyard from your hand or library this turn",
            " that begins with the chosen letter",
            " with toughness greater than its power",
            " with power greater than its toughness",
            " with power greater than their toughness",
            " that are instant cards, sorcery cards, and/or have an adventure",
            " that are oozes or are named ~",
            " controlled by players who chose war",
            " controlled by players who chose peace",
            " that's exactly those colors from among them",
            " you own on the battlefield or in the command zone",
            " from among cards they exiled with ~ this turn",
            " from among the cards they don't own exiled with ~",
            " from among cards in graveyards that were put there from libraries this turn",
            " they don't own exiled with ~",
            " exiled with cards named ~",
            " with a kicker ability",
            " blocking or blocked by target creature",
            " that you cycled or discarded this turn",
            " that wasn't cast from its owner's hand",
            " that have -1/-1 counters on them",
            " put into graveyards this way",
            " that blocked ~ this turn",
            " you own and control",
            " with level up",
            " that became a creature this way",
            " that shares at least one creature type with it",
            " that's a cleric, rogue, warrior, and/or wizard",
            " in exile with cage counters on them",
            " with a morph ability",
            " that's attacking or blocking",
            " with those names",
            " with those counters",
            " of the other type",
            " used to craft ~",
            " used to craft it",
            " that has suspend",
            " that don't have suspend",
            " ~ can enchant",
            " with the lowest mana value",
            " with equal or lesser power",
            " with toughness less than the exploited creature's toughness",
            " that blocked or was blocked this turn",
            " tapped this turn to pay for its abilities",
            " that were blocked by target wall this turn",
            " from a noncreature source",
            " that didn't enter this turn",
            " that ~ could enchant",
            " you neither own nor control",
            " that share a permanent type",
            " banded with it",
            " banded with ~",
            " that mana from a treasure was spent to cast",
            " from your hand with mana value x or less",
            " with the chosen names",
            " in your graveyard that's exactly two colors",
            " that was cast this turn",
            " from among the other cards exiled this way",
            " with lesser mana value than it",
            " with landwalk abilities",
            " not named ~ that have different names",
            " they own that you control",
            " created with it",
            " that's exactly two colors from among them",
            " it blocked this turn",
            " played this way",
            " you didn't play",
            " you looked at this way",
            " tapped to pay ~'s additional cost",
            " you cast this turn",
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
            "and" => {
                // Disjunctive plural/collective subtype lists such as "Mounts and Vehicles",
                // "Skeletons, Vampires, and Zombies", "Other Wolves and Werewolves", and
                // "Treefolk spells and Shaman spells" use "and" without a following indefinite
                // article. Guard against two-object phrases ("a Swamp and a Forest") where
                // "and" precedes "a", "an", or "another".
                let next_is_indef = tokens
                    .get(i + 1)
                    .is_some_and(|next| matches!(*next, "a" | "an" | "another"));
                if saw_subtype && !next_is_indef {
                    pending_or_after_subtype = true;
                }
            }
            "of" | "the" | "a" | "an" => {}
            "other" | "another" => flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF),
            "permanent" | "permanents" | "source" | "sources" => flt.any_permanent = true,
            "card" | "cards" => flt.any_card = true,
            "spell" | "spells" => flt.any_spell = true,
            "ability" | "abilities" => flt.is_ability = true,
            "activated" | "triggered" => flt.is_ability = true,
            "noncommander" => flt.flags = flt.flags.union(ObjectFilterFlags::NOT_COMMANDER),
            "kicked" | "warped" | "bountied" | "blighted" | "stickered" | "solved" => {
                flt.predicates
                    .push(FilterPredicateAst::StateQualification(tok.to_string()));
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
            "modified" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED);
            }
            "saddled" | "suspected" | "goaded" | "monstrous" | "renowned" | "exerted"
            | "prepared" | "adventure" | "modal" | "silver-bordered" | "acorn"
            | "black-bordered" => {
                flt.predicates
                    .push(FilterPredicateAst::StateQualification(tok.to_string()));
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
            "nonattacking" => {
                flt.combat = Some(CombatState::NotAttacking);
            }
            "nonblocking" | "foretold" | "double-faced" => {
                flt.predicates
                    .push(FilterPredicateAst::StateQualification(tok.to_string()));
            }
            "nonwhite" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::WHITE),
            "nonblue" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLUE),
            "nonblack" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLACK),
            "nonred" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::RED),
            "nongreen" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::GREEN),
            "nonaura" => flt.forbidden_subtypes.push("Aura".to_string()),
            "nonequipment" => flt.forbidden_subtypes.push("Equipment".to_string()),
            "transformed" | "sacrificed" | "exiled" | "discarded" | "revealed" | "milled"
            | "chosen" | "returned" => {
                flt.predicates
                    .push(FilterPredicateAst::StateQualification(tok.to_string()));
            }
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
                    flt.predicates
                        .push(FilterPredicateAst::StateQualification(format!("pt:{tok}")));
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
    } else if let Some(i) = body.rfind(" that has ") {
        (i, true, body[i + 10..].trim())
    } else if let Some(i) = body.rfind(" that have ") {
        (i, true, body[i + 11..].trim())
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
        " with mana cost {0} or {1}" => {
            flt.predicates
                .push(FilterPredicateAst::ExactManaCostOneOf(vec![
                    "{0}".to_string(),
                    "{1}".to_string(),
                ]));
        }
        " that has a -1/-1 counter on it" => {
            flt.counter_kind = Some("-1/-1".to_string());
        }
        " that has a +1/+1 counter on it" => {
            flt.counter_kind = Some("+1/+1".to_string());
        }
        " with ice counters on them" => {
            flt.counter_kind = Some("ice".to_string());
        }
        " chosen this way" => {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "chosen_this_way".to_string(),
            ));
        }
        " that crewed it this turn"
        | " that convoked it"
        | " that fought this turn"
        | " each with power greater than its base power"
        | " with power greater than its base power"
        | " with power greater than that creature's base power" => {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                suf.trim().to_string(),
            ));
        }
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
        " milled this way" | " they milled this way" => {
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "milled_this_way".to_string(),
            });
        }
        " dealt damage this way" => {
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "dealt_damage_this_way".to_string(),
            });
        }
        " created this way" => {
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "created_this_way".to_string(),
            });
        }
        " put onto the battlefield this way" => {
            flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                kind: "put_onto_battlefield_this_way".to_string(),
            });
        }
        " that has an adventure" | " that have an adventure" => {
            flt.predicates.push(FilterPredicateAst::HasAbilityKind {
                kind: "adventure".to_string(),
                negated: false,
            });
        }
        " that didn't attack this turn" => {
            flt.flags = flt.flags.union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN);
        }
        " in your party" => {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "in_your_party".to_string(),
            ));
        }
        " cast from a graveyard" => {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "cast_from_graveyard".to_string(),
            ));
        }
        " with greater power"
        | " with power less than ~'s"
        | " with lesser mana value"
        | " with equal or lesser mana value"
        | " with lesser mana value than the creature that died"
        | " that don't have a name"
        | " that's not historic"
        | " other than that creature"
        | " other than that permanent"
        | " other than that card"
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
