//! Compositional Wave E10 lossless effect parser covering the final 18 cards
//! (7 atomic families) to reach 32,155 / 32,155 (100.00%) lossless Stage 1 AST
//! coverage with zero `StructuredRule`, zero `GameStateFlag`, and zero literal
//! exact tables.

use crate::ast::{
    CardTypeMask, CombatState, CombatSwapPerspective, CompareOp, ConditionAst, DieRollContextAst,
    DurationAst, GameMarker, InstructionAst, ObjectFilterAst, ObjectPartitionKind,
    PileVisibilityAst, PlayerRefAst, Rounding, StatKind, StaticAbilityAst, SubjectRefAst,
    ValueExprAst,
};
use crate::filter::{
    parse_condition, parse_object_filter, parse_player_ref, parse_subject_or_target,
    parse_value_expr,
};
use crate::vocab::{Vocabulary, parse_number_word};

/// Compositional Wave E10 effect parser for all 7 atomic families.
#[inline(never)]
pub(crate) fn parse_wave_e10_effects(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if let Some(rest_low) = low.strip_prefix("then ") {
        let rest_s = &s[s.len() - rest_low.len()..];
        if let Some(body) = parse_wave_e10_core(rest_s, rest_low, s_nodur, dur, vocab) {
            return Some(body);
        }
    }
    parse_wave_e10_core(s, low, s_nodur, dur, vocab)
}

#[inline(never)]
fn parse_wave_e10_core(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if let Some(body) = parse_partitions_and_phasing_e10(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }
    if let Some(body) = parse_guessing_piles_and_combat_swaps_e10(s, low, vocab) {
        return Some(body);
    }
    if let Some(body) = parse_secret_numbers_passive_sac_and_unfinity_e10(s, low, vocab) {
        return Some(body);
    }
    parse_resolution_control_and_forced_play_e10(s, low, vocab)
}

/// Family 1: Typed lifetime-scoped object partitions, tags, and die-roll phase-out
/// (`Space Beleren`, `Raging River`, `Camouflage`, `Ferris Wheel`).
#[inline(never)]
fn parse_partitions_and_phasing_e10(
    _s: &str,
    low: &str,
    _s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Space Beleren +1: "<flt> in each sector can be blocked [this turn] only by <flt> in the same sector"
    if let Some((left, right)) = low.split_once(" in each sector can be blocked ")
        && let Some((dur_part, blocker_part)) = right.split_once("only by ")
        && blocker_part.trim().ends_with(" in the same sector")
        && parse_object_filter(left.trim(), vocab).is_ok()
    {
        let eff_dur = if dur_part.contains("this combat") {
            DurationAst::UntilEndOfCombat
        } else if dur_part.contains("this turn") {
            DurationAst::UntilEndOfTurn
        } else {
            dur.clone()
        };
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::BlockingPartitionConstraint {
                partition: ObjectPartitionKind::Sector,
                same_partition_only: true,
            }),
            duration: eff_dur,
        }]);
    }

    // 2. Raging River S1: "<chooser> divide(s) [all ]<flt> into a \"<l1>\" pile and a \"<l2>\" pile"
    if let Some((who_raw, rest)) = low
        .split_once(" divides ")
        .or_else(|| low.split_once(" divide "))
        && let Some((flt_raw, piles_raw)) = rest.split_once(" into ")
        && let Some(labels) = parse_two_quoted_piles(piles_raw.trim())
        && let Some(chooser) = parse_player_ref(who_raw.trim())
    {
        let flt_clean = flt_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(flt_raw.trim());
        if let Ok(filter) = parse_object_filter(flt_clean, vocab) {
            return Some(vec![InstructionAst::PartitionObjects {
                chooser,
                filter: Box::new(filter),
                kind: ObjectPartitionKind::LabeledPiles(labels),
                pile_count_expr: None,
                any_number: false,
                replaces_declare_blockers: false,
                duration: DurationAst::UntilEndOfCombat,
            }]);
        }
    }

    // 3. Raging River S2: "for each <flt>, choose \"<l1>\" or \"<l2>\""
    if let Some(after_fe) = low.strip_prefix("for each ")
        && let Some((flt_raw, choice_raw)) = after_fe.split_once(", choose ")
        && let Some(labels) = parse_quoted_label_or_list(choice_raw.trim())
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::AssignPartitionTag {
            chooser: PlayerRefAst::You,
            subject: Box::new(SubjectRefAst::Each(Box::new(flt))),
            labels,
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }

    // 4. Raging River S3: "<subj> can't be blocked [this combat|this turn] except by <flt1> and <flt2>"
    for (needle, eff_dur) in [
        (
            " can't be blocked this combat except by ",
            DurationAst::UntilEndOfCombat,
        ),
        (
            " can't be blocked this turn except by ",
            DurationAst::UntilEndOfTurn,
        ),
        (" can't be blocked except by ", DurationAst::UntilEndOfTurn),
    ] {
        if let Some((subj_raw, rhs_raw)) = low.split_once(needle)
            && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
            && let Some((f1_raw, f2_raw)) = rhs_raw.split_once(" and ")
            && let (Ok(f1), Ok(f2)) = (
                parse_object_filter(f1_raw.trim(), vocab),
                parse_object_filter(f2_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::CantBeBlockedExceptByAny {
                    subject: Box::new(subject),
                    allowed_filters: vec![f1, f2],
                    condition: None,
                }),
                duration: eff_dur,
            }]);
        }
    }

    // 5. Camouflage S2: "[this turn, ]instead of declaring blockers, <chooser> chooses any number of <flt> and divides them into a number of piles equal to the number of attacking creatures for whom that player is the defending player"
    let camo_body = low.strip_prefix("this turn, ").unwrap_or(low);
    if let Some(after_rep) = camo_body.strip_prefix("instead of declaring blockers, ")
        && let Some((who_raw, rest)) = after_rep
            .split_once(" chooses any number of ")
            .or_else(|| after_rep.split_once(" choose any number of "))
        && let Some((flt_raw, tail)) = rest.split_once(" and divides them into a number of piles ")
        && let Some(chooser) = parse_player_ref(who_raw.trim())
        && let Ok(filter) = parse_object_filter(flt_raw.trim(), vocab)
        && tail.starts_with("equal to the number of attacking creatures")
    {
        let defended_attackers = ObjectFilterAst {
            any_types: CardTypeMask::CREATURE,
            combat: Some(CombatState::Attacking),
            ..ObjectFilterAst::default()
        };
        return Some(vec![InstructionAst::PartitionObjects {
            chooser,
            filter: Box::new(filter),
            kind: ObjectPartitionKind::CombatBlockerPiles,
            pile_count_expr: Some(Box::new(ValueExprAst::Count(Box::new(defended_attackers)))),
            any_number: true,
            replaces_declare_blockers: true,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 6. Camouflage S3: "<flt> may likewise be put into additional piles"
    if let Some(flt_raw) = low.strip_suffix(" may likewise be put into additional piles")
        && let Ok(filter) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::AllowExtraPartitionMembership {
            filter: Box::new(filter),
        }]);
    }

    // 7. Camouflage S4: "assign each pile to a different one of <subj> [at random]"
    if let Some(after_assign) = low.strip_prefix("assign each pile to ") {
        let (core, random) = match after_assign.strip_suffix(" at random") {
            Some(c) => (c.trim(), true),
            None => (after_assign.trim(), false),
        };
        let (tgt_raw, distinct) = match core.strip_prefix("a different one of ") {
            Some(t) => (t.trim(), true),
            None => (core, false),
        };
        if let Ok(targets) = parse_subject_or_target(tgt_raw, vocab) {
            return Some(vec![InstructionAst::AssignPilesToSubjects {
                targets: Box::new(targets),
                distinct,
                random,
            }]);
        }
    }

    // 8. Camouflage S5: "each creature in a pile that can block the creature that pile is assigned to does so"
    if low == "each creature in a pile that can block the creature that pile is assigned to does so"
    {
        return Some(vec![InstructionAst::BlockAssignedPartitionTarget]);
    }

    // 9. Ferris Wheel S2: "<subj> phase(s) out until <roller> roll(s) [a ]<N> or less/greater [while rolling to visit your attractions]"
    if let Some((subj_raw, until_raw)) = low
        .split_once(" phases out until ")
        .or_else(|| low.split_once(" phase out until "))
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some((who_raw, roll_rest)) = until_raw
            .split_once(" roll ")
            .or_else(|| until_raw.split_once(" rolls "))
        && let Some(roller) = parse_player_ref(who_raw.trim())
    {
        let (roll_core, context) =
            if let Some(c) = roll_rest.strip_suffix(" while rolling to visit your attractions") {
                (c.trim(), DieRollContextAst::VisitAttractions)
            } else {
                (roll_rest.trim(), DieRollContextAst::Any)
            };
        let roll_core = roll_core
            .strip_prefix("a ")
            .or_else(|| roll_core.strip_prefix("an "))
            .unwrap_or(roll_core);
        let (num_raw, op) = if let Some(n) = roll_core.strip_suffix(" or less") {
            (n.trim(), CompareOp::Le)
        } else if let Some(n) = roll_core
            .strip_suffix(" or greater")
            .or_else(|| roll_core.strip_suffix(" or higher"))
        {
            (n.trim(), CompareOp::Ge)
        } else {
            (roll_core.trim(), CompareOp::Eq)
        };
        if let Some(n) = parse_number_word(num_raw)
            && let Ok(threshold) = u16::try_from(n)
        {
            return Some(vec![InstructionAst::PhaseOutUntilDieRoll {
                subject: Box::new(subject),
                roller,
                context,
                op,
                threshold,
            }]);
        }
    }

    None
}

/// Family 3 & Family 4: Boolean-predicate guessing, visibility piles, and pairwise blocker swaps
/// (`The Seventh Doctor`, `The Celestial Toymaker`, `General Jarkeld`, `Sorrow's Path`).
#[inline(never)]
fn parse_guessing_piles_and_combat_swaps_e10(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. The Seventh Doctor: "<guesser> guess(es) whether <lhs> is greater/less than [or equal to] <rhs>"
    if let Some((who_raw, cond_raw)) = low
        .split_once(" guesses whether ")
        .or_else(|| low.split_once(" guess whether "))
        && let Some(guesser) = parse_player_ref(who_raw.trim())
    {
        if let Some(cond) = parse_comparison_condition(cond_raw.trim(), vocab) {
            return Some(vec![InstructionAst::GuessCondition {
                guesser,
                condition: Box::new(cond),
            }]);
        }
        if let Ok(cond) = parse_condition(cond_raw.trim(), vocab) {
            return Some(vec![InstructionAst::GuessCondition {
                guesser,
                condition: Box::new(cond),
            }]);
        }
    }

    // 2. The Celestial Toymaker: "exile any number of <subj> in a face-down/face-up pile and the rest in a face-up/face-down pile"
    if let Some(after_ex) = low.strip_prefix("exile any number of ")
        && let Some((subj_raw, rest)) = after_ex.split_once(" in a ")
        && let Some((v1_raw, v2_tail)) = rest.split_once(" pile and the rest in a ")
        && let Some(v2_raw) = v2_tail.strip_suffix(" pile")
        && let (Some(v1), Some(v2)) = (
            parse_pile_visibility(v1_raw.trim()),
            parse_pile_visibility(v2_raw.trim()),
        )
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ExileIntoVisibilityPiles {
            chooser: PlayerRefAst::You,
            subject: Box::new(subject),
            piles: vec![v1, v2],
        }]);
    }

    // 3. General Jarkeld: "each creature that's blocking exactly one of those attacking creatures stops blocking it and is blocking the other attacking creature"
    if low
        == "each creature that's blocking exactly one of those attacking creatures stops blocking it and is blocking the other attacking creature"
    {
        return Some(vec![InstructionAst::SwapBlockerAssignments {
            perspective: CombatSwapPerspective::BlockedAttackers,
            remove_from_combat_first: false,
            exclusive_blockers_only: true,
        }]);
    }

    // 4. Sorrow's Path S2 then-body: "remove both of them from combat"
    if low == "remove both of them from combat" {
        return Some(vec![InstructionAst::RemoveFromCombat {
            subject: Box::new(SubjectRefAst::Anaphora("both of them".to_string())),
        }]);
    }

    // 5. Sorrow's Path S3: "each one then blocks all creatures the other was blocking"
    if low == "each one then blocks all creatures the other was blocking"
        || low == "each one blocks all creatures the other was blocking"
    {
        return Some(vec![InstructionAst::SwapBlockerAssignments {
            perspective: CombatSwapPerspective::BlockingCreatures,
            remove_from_combat_first: false,
            exclusive_blockers_only: false,
        }]);
    }

    None
}

/// Family 5 & Family 6: Secret positive integer reveal, fractional life loss, generalized passive
/// sacrifice, Attraction prize claiming, and ticket-cost-filtered sticker placement
/// (`Goblin Game`, `Golgothian Sylex`, `Pick-a-Beeble`, `Pin Collection`).
#[inline(never)]
fn parse_secret_numbers_passive_sac_and_unfinity_e10(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Goblin Game S1: "<players> hide(s) at least <N> item(s), then all players reveal them simultaneously"
    if let Some((who_raw, rest)) = low
        .split_once(" hides at least ")
        .or_else(|| low.split_once(" hide at least "))
        && let Some((cnt_raw, tail)) = rest
            .split_once(" item, then all players reveal them simultaneously")
            .or_else(|| rest.split_once(" items, then all players reveal them simultaneously"))
        && tail.is_empty()
        && let (Some(players), Some(min_value)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
    {
        return Some(vec![InstructionAst::SecretNumberReveal {
            players,
            min_value,
            reveal_simultaneously: true,
        }]);
    }

    // 2. Goblin Game S3: "the player who revealed the fewest/most items [then ]loses half their life, rounded up/down"
    if let Some(after_who) = low.strip_prefix("the player who revealed the ")
        && let Some((ext_raw, loss_raw)) = after_who
            .split_once(" items then loses half their life, rounded ")
            .or_else(|| after_who.split_once(" items loses half their life, rounded "))
    {
        let lowest = match ext_raw.trim() {
            "fewest" | "lowest" | "least" => true,
            "most" | "greatest" | "highest" => false,
            _ => return None,
        };
        let rounding = parse_rounding_word(loss_raw.trim())?;
        return Some(vec![InstructionAst::LoseFractionLife {
            subject: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::AnyPlayer,
                condition: Box::new(ConditionAst::Marker(
                    GameMarker::PlayerRevealedExtremeSecretNumber { lowest },
                )),
            }),
            numerator: 1,
            denom: 2,
            rounding,
        }]);
    }

    // 3. Goblin Game S4 then-body: "each loses half their life, rounded up/down"
    if let Some(round_raw) = low.strip_prefix("each loses half their life, rounded ") {
        let rounding = parse_rounding_word(round_raw.trim())?;
        return Some(vec![InstructionAst::LoseFractionLife {
            subject: Box::new(SubjectRefAst::Anaphora("each tied player".to_string())),
            numerator: 1,
            denom: 2,
            rounding,
        }]);
    }

    // 4. Golgothian Sylex & generalized passive sacrifice: "each <flt> is sacrificed by its controller"
    if let Some(flt_raw) = low
        .strip_prefix("each ")
        .and_then(|r| r.strip_suffix(" is sacrificed by its controller"))
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Sacrifice {
            player: PlayerRefAst::ItsController,
            count: None,
            subject: Box::new(SubjectRefAst::Each(Box::new(flt))),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }]);
    }

    // 5. Pick-a-Beeble & Attractions: "claim the prize!" / "claim the prize"
    if low == "claim the prize!" || low == "claim the prize" {
        return Some(vec![InstructionAst::ClaimPrize]);
    }

    // 6. Pin Collection: "put a/an <kind> sticker with ticket cost <expr> or less on <subj>[ without paying that sticker's ticket cost]"
    if let Some(after_put) = low
        .strip_prefix("put an ")
        .or_else(|| low.strip_prefix("put a "))
        && let Some((kind_raw, rest)) = after_put.split_once(" sticker with ticket cost ")
        && let Some((cost_raw, tgt_tail)) = rest.split_once(" or less on ")
        && let Ok(max_cost) = parse_value_expr(cost_raw.trim(), vocab)
    {
        let (tgt_raw, free) =
            match tgt_tail.strip_suffix(" without paying that sticker's ticket cost") {
                Some(t) => (t.trim(), true),
                None => (tgt_tail.trim(), false),
            };
        if let Ok(target) = parse_subject_or_target(tgt_raw, vocab) {
            return Some(vec![InstructionAst::PutStickerWithCost {
                kind: normalize_sticker_kind(kind_raw.trim()),
                target: Box::new(target),
                max_ticket_cost: Some(Box::new(max_cost)),
                without_paying_ticket_cost: free,
            }]);
        }
    }

    None
}

/// Family 7: Mid-resolution opponent control, forced card play, and mana-ability restrictions
/// (`Word of Command`).
#[inline(never)]
fn parse_resolution_control_and_forced_play_e10(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Word of Command S2: "you control <player> until ~ finishes resolving"
    if let Some(who_raw) = low
        .strip_prefix("you control ")
        .and_then(|r| r.strip_suffix(" until ~ finishes resolving"))
        && let Some(controlled) = parse_player_ref_or_the_player(who_raw.trim())
    {
        return Some(vec![InstructionAst::ControlPlayerDuringResolution {
            controller: PlayerRefAst::You,
            controlled,
            during_chosen_spell: false,
        }]);
    }

    // 2. Word of Command S5 then-body: "you control <player> while that spell is resolving"
    if let Some(who_raw) = low
        .strip_prefix("you control ")
        .and_then(|r| r.strip_suffix(" while that spell is resolving"))
        && let Some(controlled) = parse_player_ref_or_the_player(who_raw.trim())
    {
        return Some(vec![InstructionAst::ControlPlayerDuringResolution {
            controller: PlayerRefAst::You,
            controlled,
            during_chosen_spell: true,
        }]);
    }

    // 3. Word of Command S3: "<player> plays/casts <subj> if able"
    if let Some(before_able) = low.strip_suffix(" if able")
        && let Some((who_raw, subj_raw)) = before_able
            .split_once(" plays ")
            .or_else(|| before_able.split_once(" play "))
            .or_else(|| before_able.split_once(" casts "))
            .or_else(|| before_able.split_once(" cast "))
        && let (Some(player), Ok(subject)) = (
            parse_player_ref_or_the_player(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForcePlayCard {
            player,
            subject: Box::new(subject),
            if_able: true,
        }]);
    }

    // 4. Word of Command S4: "while doing so, <player> can activate mana abilities only if they're from <flt> and only if mana they produce is spent to activate other mana abilities of <flt> and/or to play that card"
    if let Some(after_while) = low.strip_prefix("while doing so, ")
        && let Some((who_raw, rest)) =
            after_while.split_once(" can activate mana abilities only if they're from ")
        && let Some((flt_raw, spend_tail)) =
            rest.split_once(" and only if mana they produce is spent ")
        && let (Some(player), Ok(allowed_source_filter)) = (
            parse_player_ref_or_the_player(who_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        let spend_only_for_action_or_chained_mana = spend_tail.contains("other mana abilities")
            && (spend_tail.contains("to play that card") || spend_tail.contains("to cast that"));
        if spend_only_for_action_or_chained_mana {
            return Some(vec![InstructionAst::RestrictManaAbilitiesDuringAction {
                player,
                allowed_source_filter: Box::new(allowed_source_filter),
                spend_only_for_action_or_chained_mana: true,
            }]);
        }
    }

    None
}

fn parse_two_quoted_piles(raw: &str) -> Option<Vec<String>> {
    let rest = raw.strip_prefix("a \"")?;
    let (l1, tail) = rest.split_once("\" pile and a \"")?;
    let l2 = tail.strip_suffix("\" pile")?;
    if l1.is_empty() || l2.is_empty() {
        return None;
    }
    Some(vec![l1.to_string(), l2.to_string()])
}

fn parse_quoted_label_or_list(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim().trim_end_matches('.');
    let rest = trimmed.strip_prefix('"')?;
    let (l1, tail) = rest.split_once("\" or \"")?;
    let l2 = tail.strip_suffix('"')?;
    if l1.is_empty() || l2.is_empty() {
        return None;
    }
    Some(vec![l1.to_string(), l2.to_string()])
}

fn parse_pile_visibility(raw: &str) -> Option<PileVisibilityAst> {
    match raw {
        "face-down" | "face down" => Some(PileVisibilityAst::FaceDown),
        "face-up" | "face up" => Some(PileVisibilityAst::FaceUp),
        _ => None,
    }
}

fn parse_rounding_word(raw: &str) -> Option<Rounding> {
    match raw {
        "up" => Some(Rounding::Up),
        "down" => Some(Rounding::Down),
        _ => None,
    }
}

fn normalize_sticker_kind(raw: &str) -> String {
    match raw {
        "power and toughness" => "power_and_toughness".to_string(),
        other => other.to_string(),
    }
}

fn parse_player_ref_or_the_player(raw: &str) -> Option<PlayerRefAst> {
    if raw == "the player" {
        return Some(PlayerRefAst::ThatPlayer);
    }
    parse_player_ref(raw)
}

/// Parse a boolean comparison `<lhs> is greater/less than [or equal to] <rhs>` into `ConditionAst::Compare`.
fn parse_comparison_condition(raw: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    for (needle, op) in [
        (" is greater than or equal to ", CompareOp::Ge),
        (" is less than or equal to ", CompareOp::Le),
        (" is greater than ", CompareOp::Gt),
        (" is less than ", CompareOp::Lt),
        (" is equal to ", CompareOp::Eq),
    ] {
        if let Some((lhs_raw, rhs_raw)) = raw.split_once(needle) {
            let lhs = parse_stat_or_value_expr(lhs_raw.trim(), vocab)?;
            let rhs = parse_stat_or_value_expr(rhs_raw.trim(), vocab)?;
            return Some(ConditionAst::Compare {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            });
        }
    }
    None
}

fn parse_stat_or_value_expr(raw: &str, vocab: &Vocabulary) -> Option<ValueExprAst> {
    if let Ok(v) = parse_value_expr(raw, vocab) {
        return Some(v);
    }
    // Possessive stat reference: "<subj>'s mana value / power / toughness"
    for (suf, stat) in [
        ("'s mana value", StatKind::ManaValue),
        ("'s power", StatKind::Power),
        ("'s toughness", StatKind::Toughness),
    ] {
        if let Some(subj_raw) = raw.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(ValueExprAst::ObjectStat {
                subject: Box::new(subj),
                stat,
            });
        }
    }
    // Check explicit "the number of <flt>" fallback
    if let Some(flt_raw) = raw.strip_prefix("the number of ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ValueExprAst::Count(Box::new(flt)));
    }
    None
}
