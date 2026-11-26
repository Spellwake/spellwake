//! Choices, modals, multiplayer/direction/category iterations, delayed and reflexive triggers,
//! named keyword actions, secret choices, and specialized compositional effect rules.

use super::{
    find_top_level_pattern, parse_atomic_effect, parse_delayed_step_and_scope, parse_effect_body,
    parse_effect_or_temp_clause, parse_untap_permanent_instruction, split_leading_player_verb,
};
use crate::ast::{
    AltCastCostAst, CardTypeMask, CategoryIterationKind, ColorMask, CombatState, CompareOp,
    ConditionAst, ControllerRelation, CostAtomAst, DelayedStepScope, DurationAst, ExchangeKind,
    FilterPredicateAst, GameMarker, InstructionAst, InteractiveChoiceKind, ManaSpecAst,
    NameFilterAst, ObjectFilterAst, ObjectFilterFlags, PileRefAst, PlayerRefAst, PlayerStatKind,
    PriorActionRefKind, ReplaceEffectWithAst, ReplacementEffectAst, Rounding, StatBoundAst,
    StatKind, StaticAbilityAst, SubjectRefAst, SupertypeMask, TargetCountAst, TokenSpecAst,
    TriggerEventAst, TriggerSpecAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, parse_cost_list, peel_leading_count};
use crate::clauses::statics::{StaticOrReplacement, parse_static_or_replacement};
use crate::clauses::triggers::parse_trigger_header;
use crate::combinators::{
    DepthGuard, any_card_flt, any_perm, card_type, normalize_sticker_kind, obj_type,
    parse_rounding_word,
};
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_value_expr,
};
use crate::preprocess::{count_symbol_sequence, is_mana_sequence, parse_base_pt};
use crate::vocab::{Vocabulary, parse_number_word};

pub(super) fn try_parse_pass_c_primitives(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. ControlPlayerTurn (Mindslaver, Emrakul the Promised End, Worst Fears, Sorin Markov, Secret of Bloodbending)
    for pref in ["you control ", "you gain control of ", "control "] {
        if let Some(after) = low.strip_prefix(pref)
            && let Some((who_raw, _)) = after
                .split_once(" during that player's next turn")
                .or_else(|| after.split_once(" during their next turn"))
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Some(vec![InstructionAst::ControlPlayerTurn {
                player,
                duration: DurationAst::UntilEndOfNextTurn,
            }]);
        }
    }

    // 2. Exchange (Control, LifeTotals, LifeWithStat, Zones: Volatile Stormdrake, Legerdemain, Shifting Loyalties, Axis of Mortality, Tree of Perdition/Redemption)
    if let Some(after) = low.strip_prefix("exchange control of ") {
        if let Some((l_raw, r_raw)) = after.split_once(" and ")
            && let (Ok(left), Ok(right)) = (
                parse_subject_or_target(l_raw, vocab),
                parse_subject_or_target(r_raw, vocab),
            )
        {
            return Some(vec![InstructionAst::Exchange {
                kind: ExchangeKind::Control,
                left: Box::new(left),
                right: Some(Box::new(right)),
            }]);
        }
        if let Ok(left) = parse_subject_or_target(after, vocab) {
            return Some(vec![InstructionAst::Exchange {
                kind: ExchangeKind::Control,
                left: Box::new(left),
                right: None,
            }]);
        }
    }
    if let Some((who_raw, _)) = low.split_once(" exchange life totals")
        && let Ok(left) = parse_subject_or_target(who_raw, vocab)
    {
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::LifeTotals,
            left: Box::new(left),
            right: None,
        }]);
    }
    if let Some(after) = low.strip_prefix("exchange ")
        && let Some((l_raw, r_raw)) = after
            .split_once(" life total with ")
            .or_else(|| after.split_once(" life total and "))
    {
        let l_clean = l_raw.trim_end_matches("'s");
        let left_subj = if l_clean == "your" {
            SubjectRefAst::Player(PlayerRefAst::You)
        } else {
            let p = parse_player_ref(l_clean)?;
            SubjectRefAst::Player(p)
        };
        for (stat_suf, stat) in [
            ("'s toughness", StatKind::Toughness),
            ("'s power", StatKind::Power),
        ] {
            if let Some(r_subj_raw) = r_raw.strip_suffix(stat_suf)
                && let Ok(right) = parse_subject_or_target(r_subj_raw, vocab)
            {
                return Some(vec![InstructionAst::Exchange {
                    kind: ExchangeKind::LifeWithStat(stat),
                    left: Box::new(left_subj),
                    right: Some(Box::new(right)),
                }]);
            }
        }
        if let Some(r_who) = r_raw.strip_suffix("'s life total")
            && let Some(rp) = parse_player_ref(r_who)
        {
            return Some(vec![InstructionAst::Exchange {
                kind: ExchangeKind::LifeTotals,
                left: Box::new(left_subj),
                right: Some(Box::new(SubjectRefAst::Player(rp))),
            }]);
        }
    }

    // 3. WishFromOutsideGame (Burning Wish, Glittering Wish, Vivien Arkbow Ranger, Legion Angel, Coax from the Blind Eternities)
    if let Some(after) = low.strip_prefix("reveal ")
        && after.contains("from outside the game")
    {
        let allow_exile = after.contains("in exile");
        let to_zone = if after.contains("put it into your hand")
            || after.contains("put them into your hand")
        {
            "your hand"
        } else {
            "revealed"
        };
        let flt_side = after
            .split_once(" you own from outside the game")
            .or_else(|| after.split_once(" from outside the game"))
            .map(|(l, _)| l)?;
        let flt_clean = flt_side
            .strip_prefix("exactly ")
            .or_else(|| flt_side.strip_prefix("up to "))
            .unwrap_or(flt_side);
        let (cnt, flt_raw) = peel_leading_count(flt_clean, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Some(vec![InstructionAst::WishFromOutsideGame {
                filter: Box::new(flt),
                count: Box::new(cnt),
                to_zone: to_zone.to_string(),
                allow_exile,
            }]);
        }
    }

    // 4. Equalize (Balance, Restore Balance, Balancing Act, Magus of the Balance)
    if let Some((who_raw, rest)) = low.split_once(" chooses a number of ")
        && let Some(player) = parse_player_ref(who_raw)
        && let Some((flt_raw, tail)) = rest.split_once(" they control equal to the number of ")
        && tail.contains("fewest")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let action = if tail.contains("sacrifices the rest") {
            "sacrifice_rest"
        } else if tail.contains("discards the rest") {
            "discard_rest"
        } else {
            "move_rest"
        };
        return Some(vec![InstructionAst::Equalize {
            player,
            filter: Box::new(flt),
            action: action.to_string(),
        }]);
    }

    // 5. InteractiveChoice: Villainous choice, Secret vote, Secret number, Bidding, Piles, Group choose
    if let Some((who_raw, rest)) = s
        .split_once(" faces a villainous choice \u{2014} ")
        .or_else(|| s.split_once(" face a villainous choice \u{2014} "))
        && let Some(chooser) = parse_player_ref(who_raw)
        && let Some((opt1, opt2)) = rest.split_once(", or ")
    {
        let (b1, f1) = parse_effect_body(opt1.trim(), vocab);
        let (b2, f2) = parse_effect_body(opt2.trim(), vocab);
        if f1.is_empty() && f2.is_empty() && !b1.is_empty() && !b2.is_empty() {
            return Some(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::VillainousChoice {
                    chooser,
                    options: vec![b1, b2],
                },
            )]);
        }
    }
    if low.contains("secretly votes for ") {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SecretVote {
                prompt: s.to_string(),
            },
        )]);
    }
    if low.starts_with("each player secretly chooses a number ") {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SecretNumberChoice { min_value: 0 },
        )]);
    }
    if low == "in turn order, each player may top the high bid"
        || low == "the bidding ends if the high bid stands"
        || low.starts_with("each player secretly chooses ")
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::BiddingAction {
                description: s.to_string(),
            },
        )]);
    }
    if let Some(after) = low.strip_prefix("separate ")
        && let Some(subj_raw) = after.strip_suffix(" into two piles")
        && let Ok(subject) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SeparateIntoPiles {
                chooser: PlayerRefAst::You,
                subject: Box::new(subject),
                pile_count: 2,
            },
        )]);
    }
    if matches!(
        low,
        "you choose one of those cards"
            | "choose one of those cards"
            | "choose a creature card and/or a land card from among them"
    ) {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::ChooseFromGroup {
                chooser: PlayerRefAst::You,
                description: low.to_string(),
            },
        )]);
    }
    if let Some(after) = low.strip_prefix("choose ")
        && !after.contains('\u{2014}')
    {
        if let Ok(flt) = parse_object_filter(after, vocab) {
            if flt.predicates.iter().any(|p| {
                matches!(
                    p,
                    crate::ast::FilterPredicateAst::ManaCostPayableBySpentMana { .. }
                )
            }) {
                return Some(vec![InstructionAst::ChooseFromGroup {
                    count: Box::new(ValueExprAst::Const(1)),
                    random: false,
                    group: Box::new(SubjectRefAst::Group(Box::new(flt))),
                }]);
            }
            return Some(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: after.to_string(),
                },
            )]);
        }
        if parse_subject_or_target(after, vocab).is_ok() || after.starts_with("a number between ") {
            return Some(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: after.to_string(),
                },
            )]);
        }
    }
    if low == "it becomes night" || low == "it becomes day" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: low.to_string(),
            duration: DurationAst::Permanent,
        }]);
    }

    // 6. DistributeCounters (Towashi, Ajani, etc.)
    if let Some(after) = low.strip_prefix("distribute ")
        && let Some((left, tgts_raw)) = after
            .split_once(" counters among ")
            .or_else(|| after.split_once(" counter among "))
        && let Ok(targets) = parse_subject_or_target(tgts_raw, vocab)
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Some(vec![InstructionAst::DistributeCounters {
            counter_kind: kind.to_string(),
            count: Box::new(cnt),
            targets: Box::new(targets),
        }]);
    }

    // 7. RepeatProcess (Grist, Sphinx's Tutelage, Claim Jumper, Forbidden Ritual, Sin)
    if matches!(
        low,
        "repeat this process"
            | "repeat this process once"
            | "repeat this process any number of times"
    ) {
        return Some(vec![InstructionAst::RepeatProcess {
            condition: None,
            max_times: low.ends_with(" once").then_some(1),
            any_number: low.contains("any number"),
        }]);
    }

    // 8. LookAtExiled ("look at [and play] <subj> for as long as <cond>")
    if let Some(after) = low.strip_prefix("look at ")
        && let Some((subj_part, cond_raw)) = after.split_once(" for as long as ")
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        let (also_play, subj_raw) = if let Some(sr) = subj_part.strip_prefix("and play ") {
            (true, sr)
        } else if let Some(sr) = subj_part.strip_prefix("and cast ") {
            (true, sr)
        } else {
            (false, subj_part)
        };
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            return Some(vec![InstructionAst::LookAtExiled {
                subject: Box::new(subj),
                also_play,
                condition: Box::new(cond),
            }]);
        }
    }

    // 9. ManaSpendRestriction ("this mana can't be spent to cast ..." / "spend this mana only ...")
    if low.starts_with("this mana can't be spent to ") || low.starts_with("spend this mana only ") {
        return Some(vec![InstructionAst::ManaSpendRestriction(s.to_string())]);
    }

    // 10. Convert (Transformers alias for Transform) + "discard all the cards in your/their hand"
    // + "<player> can't cast <filter> during that player's next turn"
    if let Some(subj_raw) = low
        .strip_prefix("convert ")
        .or_else(|| low.strip_prefix("you convert "))
    {
        let (core, delayed) = match subj_raw.strip_suffix(" at end of combat") {
            Some(c) => (c, Some(TurnStepAst::EndOfCombat)),
            None => (subj_raw, None),
        };
        if let Ok(subj) = parse_subject_or_target(core, vocab) {
            let ins = InstructionAst::Transform {
                subject: Box::new(subj),
            };
            return Some(match delayed {
                Some(step) => vec![InstructionAst::DelayedTrigger {
                    step,
                    scope: DelayedStepScope::Next,
                    body: vec![ins],
                }],
                None => vec![ins],
            });
        }
    }
    if low == "discard all the cards in your hand" {
        return Some(vec![InstructionAst::DiscardHand {
            player: PlayerRefAst::You,
        }]);
    }
    if let Some((who_raw, rest)) = low
        .split_once(" discards all the cards in ")
        .or_else(|| low.split_once(" discard all the cards in "))
        && rest.ends_with(" hand")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(vec![InstructionAst::DiscardHand { player }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" can't cast ")
        && let Some((flt_raw, _)) = rest
            .split_once(" during that player's next turn")
            .or_else(|| rest.split_once(" during their next turn"))
        && let (Some(player), Ok(_flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Player(player)),
            rule: format!("cant_cast:{flt_raw}"),
            duration: DurationAst::UntilEndOfNextTurn,
        }]);
    }

    None
}

pub(super) fn try_parse_misc_actions(
    _s: &str,
    s_nodur: &str,
    low: &str,
    dur: &DurationAst,
    dur_perm: &DurationAst,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    // Switch P/T: "Switch <subj>'s power and toughness [until end of turn]" / "Switch its power and toughness"
    let snlow = s_nodur.to_ascii_lowercase();
    if snlow == "switch its power and toughness" {
        return Some(InstructionAst::SwitchPowerToughness {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            duration: dur.clone(),
        });
    }
    if let Some(rest) = snlow.strip_prefix("switch ")
        && let Some(subj_raw) = rest.strip_suffix("'s power and toughness")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::SwitchPowerToughness {
            subject: Box::new(subj),
            duration: dur.clone(),
        });
    }

    // "<subj> becomes the <characteristic> of your choice"
    if let Some((subj_raw, kind_raw)) = snlow
        .split_once(" becomes the ")
        .or_else(|| snlow.split_once(" become the "))
        && let Some(kind) = kind_raw.strip_suffix(" of your choice")
        && matches!(
            kind,
            "creature type" | "basic land type" | "color" | "color or colors"
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::BecomeChosenCharacteristic {
            subject: Box::new(subj),
            kind: kind.to_string(),
            duration: dur.clone(),
        });
    }

    if let Some(rest) = low.strip_prefix("tap or untap ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::TapOrUntapPermanent {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low.strip_prefix("tap ") {
        let (core, any_number) = match rest.strip_prefix("any number of ") {
            Some(r) => (r, true),
            None => (rest, false),
        };
        if let Ok(subj) = parse_subject_or_target(core, vocab) {
            return Some(InstructionAst::TapPermanent {
                subject: Box::new(subj),
                any_number,
            });
        }
    }
    // Compositional "<player> taps/tap <subj>" (Sands of Time and future player-scoped tap effects)
    if let Some((who_raw, rest)) = low.split_once(" taps ").or_else(|| low.split_once(" tap "))
        && !who_raw.trim().is_empty()
        && let Ok(subj) = parse_subject_or_target(rest.trim(), vocab)
    {
        if let Some(player) = parse_player_ref(who_raw.trim()) {
            let act = InstructionAst::TapPermanent {
                subject: Box::new(subj),
                any_number: false,
            };
            return Some(
                if matches!(
                    player,
                    PlayerRefAst::EachPlayer
                        | PlayerRefAst::EachOpponent
                        | PlayerRefAst::EachOtherOpponent
                        | PlayerRefAst::EachOtherPlayer
                ) {
                    InstructionAst::ForEachPlayer {
                        player,
                        body: vec![act],
                    }
                } else {
                    act
                },
            );
        } else if let Ok(players_subj) = parse_subject_or_target(who_raw.trim(), vocab)
            && matches!(players_subj, SubjectRefAst::PlayerMatching { .. })
        {
            return Some(InstructionAst::ForEachSubjectPlayer {
                players: Box::new(players_subj),
                body: vec![InstructionAst::TapPermanent {
                    subject: Box::new(subj),
                    any_number: false,
                }],
            });
        }
    }
    if let Some(rest) = low.strip_prefix("remove ")
        && let Some(subj_raw) = rest.strip_suffix(" from combat")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::RemoveFromCombat {
            subject: Box::new(subj),
        });
    }
    if let Some(after) = low.strip_prefix("the owner of ")
        && let Some((subj_raw, tail)) = after.split_once(" shuffles ")
        && (tail == "it into their library" || tail == "them into their library")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        });
    }
    if let Some((subj_raw, tail)) = low.split_once("'s owner shuffles ")
        && (tail == "it into their library" || tail == "them into their library")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low.strip_prefix("untap ")
        && let Some(ins) = parse_untap_permanent_instruction(rest, vocab)
    {
        return Some(ins);
    }
    // Compositional "<player> untaps/untap <subj>" (Sands of Time, Curse of Bounty, and future player-scoped untap effects)
    if let Some((who_raw, rest)) = low
        .split_once(" untaps ")
        .or_else(|| low.split_once(" untap "))
        && !who_raw.trim().is_empty()
        && let Some(act) = parse_untap_permanent_instruction(rest.trim(), vocab)
    {
        if let Some(player) = parse_player_ref(who_raw.trim()) {
            return Some(
                if matches!(
                    player,
                    PlayerRefAst::EachPlayer
                        | PlayerRefAst::EachOpponent
                        | PlayerRefAst::EachOtherOpponent
                        | PlayerRefAst::EachOtherPlayer
                ) {
                    InstructionAst::ForEachPlayer {
                        player,
                        body: vec![act],
                    }
                } else {
                    act
                },
            );
        } else if let Ok(players_subj) = parse_subject_or_target(who_raw.trim(), vocab)
            && matches!(players_subj, SubjectRefAst::PlayerMatching { .. })
        {
            return Some(InstructionAst::ForEachSubjectPlayer {
                players: Box::new(players_subj),
                body: vec![act],
            });
        }
    }
    if let Some((who_raw, rest)) = low
        .split_once(" shuffles ")
        .or_else(|| low.split_once(" shuffle "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if rest == "their hand and graveyard into their library"
            || rest == "the cards from their hand into their library"
        {
            return Some(InstructionAst::WheelShuffleHandAndGraveyard {
                player,
                draw: Box::new(ValueExprAst::Const(0)),
            });
        }
        if matches!(
            rest,
            "their hand into their library" | "your hand into your library"
        ) {
            return Some(InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(SubjectRefAst::Anaphora("their hand".to_string())),
            });
        }
        if let Some(subj_raw) = rest
            .strip_suffix(" into their library")
            .or_else(|| rest.strip_suffix(" into your library"))
            .or_else(|| rest.strip_suffix(" into its owner's library"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(subj),
            });
        }
    }
    if let Some(rest) = low.strip_prefix("shuffle ")
        && let Some(subj_raw) = rest
            .strip_suffix(" into its owner's library")
            .or_else(|| rest.strip_suffix(" into their owners' libraries"))
            .or_else(|| rest.strip_suffix(" into your library"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" doesn't untap during its controller's next untap step")
        .or_else(|| low.strip_suffix(" don't untap during their controllers' next untap steps"))
        .or_else(|| low.strip_suffix(" don't untap during their controller's next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during that player's next untap step"))
        .or_else(|| low.strip_suffix(" doesn't untap during that player's next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during target player's next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during their next untap step"))
        .or_else(|| low.strip_suffix(" doesn't untap during your next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during your next untap step"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::SkipNextUntap {
            subject: Box::new(subj),
        });
    }
    if let Some(after) = low.strip_prefix("during that player's next untap step, ")
        && let Some(subj_raw) = after
            .strip_suffix(" don't untap")
            .or_else(|| after.strip_suffix(" doesn't untap"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::SkipNextUntap {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low
        .strip_prefix("change the target of ")
        .or_else(|| low.strip_prefix("change a target of "))
        .or_else(|| low.strip_prefix("change any targets of "))
        .or_else(|| low.strip_prefix("choose new targets for "))
    {
        let (core, new_target) = if let Some((before_to, nt_raw)) = rest.rsplit_once(" to ")
            && let Ok(nt) = parse_subject_or_target(nt_raw.trim(), vocab)
        {
            (before_to.trim(), Some(Box::new(nt)))
        } else {
            (rest, None)
        };
        let (core, must_target) = if let Some((before_mt, mt_raw)) = core
            .split_once(" that targets only ")
            .or_else(|| core.split_once(" that targets "))
            && let Ok(mt) = parse_subject_or_target(mt_raw.trim(), vocab)
        {
            (before_mt.trim(), Some(Box::new(mt)))
        } else {
            (core, None)
        };
        let (core, single_target_only) = match core.strip_suffix(" with a single target") {
            Some(before) => (before.trim(), true),
            None => (core, false),
        };
        if let Ok(mut target) = parse_subject_or_target(core, vocab) {
            if single_target_only && let SubjectRefAst::TargetObject { ref mut filter, .. } = target
            {
                filter.flags = filter.flags.union(ObjectFilterFlags::SINGLE_TARGET);
            }
            return Some(InstructionAst::ChangeTargets {
                target: Box::new(target),
                new_target,
                must_target,
                single_target_only,
            });
        }
    }
    if let Some(rest) = low.strip_prefix("regenerate ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::Regenerate {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low
        .strip_prefix("transform ")
        .or_else(|| low.strip_prefix("flip "))
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::Transform {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" phases out until ~ leaves the battlefield")
        .or_else(|| low.strip_suffix(" phase out until ~ leaves the battlefield"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PhaseOutUntil {
            subject: Box::new(subj),
            until: DurationAst::UntilSourceLeaves,
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" phases out")
        .or_else(|| low.strip_suffix(" phase out"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PhaseOut {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" phases in")
        .or_else(|| low.strip_suffix(" phase in"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PhaseIn {
            subject: Box::new(subj),
        });
    }
    if let Some((a_raw, b_raw)) = low
        .split_once(" fights ")
        .or_else(|| low.split_once(" fight "))
        && let Ok(a) = parse_subject_or_target(a_raw, vocab)
    {
        let b_res = if matches!(a, SubjectRefAst::TargetObject { .. })
            && let Some(b_target_rest) = b_raw.trim().strip_prefix("another target ")
            && let Ok(flt) = parse_object_filter(b_target_rest, vocab)
        {
            Ok(SubjectRefAst::DistinctTarget {
                ordinal: 2,
                filter: Box::new(flt),
            })
        } else {
            parse_subject_or_target(b_raw, vocab)
        };
        if let Ok(b) = b_res {
            return Some(InstructionAst::Fight {
                a: Box::new(a),
                b: Box::new(b),
            });
        }
    }
    if let Some(after) = low.strip_prefix("attach ") {
        for split_pair in [after.split_once(" to "), after.rsplit_once(" to ")]
            .into_iter()
            .flatten()
        {
            let (a_raw, b_raw) = split_pair;
            let (a_clean, any_number, all) = if let Some(r) = a_raw.strip_prefix("any number of ") {
                (r.trim(), true, false)
            } else if let Some(r) = a_raw.strip_prefix("all ") {
                (r.trim(), false, true)
            } else {
                (a_raw.trim(), false, false)
            };
            if let (Ok(a), Ok(b)) = (
                parse_subject_or_target(a_clean, vocab),
                parse_subject_or_target(b_raw, vocab),
            ) {
                return Some(InstructionAst::Attach {
                    subject: Box::new(a),
                    target: Box::new(b),
                    any_number,
                    all,
                });
            }
        }
    }
    if let Some(after) = low.strip_prefix("unattach ") {
        let (a_raw, b_opt) = match after.rsplit_once(" from ") {
            Some((a, b)) => (a, parse_subject_or_target(b, vocab).ok()),
            None => (after, None),
        };
        let (a_clean, all) = match a_raw.strip_prefix("all ") {
            Some(r) => (r, true),
            None => (a_raw, false),
        };
        if let Ok(a) = parse_subject_or_target(a_clean, vocab) {
            return Some(InstructionAst::Attach {
                subject: Box::new(a),
                target: Box::new(b_opt.unwrap_or(SubjectRefAst::Anaphora("unattach".to_string()))),
                any_number: false,
                all,
            });
        }
    }
    let snlow = s_nodur.to_ascii_lowercase();
    if let Some((who_raw, rest)) = snlow
        .split_once(" gains control of ")
        .or_else(|| snlow.split_once(" gain control of "))
        && let (Some(controller), Ok(subj)) = (
            parse_player_ref(who_raw),
            parse_subject_or_target(rest, vocab),
        )
    {
        return Some(InstructionAst::GainControl {
            controller,
            subject: Box::new(subj),
            duration: dur_perm.clone(),
        });
    }
    if let Some(rest) = snlow.strip_prefix("gain control of ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject: Box::new(subj),
            duration: dur_perm.clone(),
        });
    }
    // Preserve trailing "this turn" when it belongs to the object filter (e.g. "creature that attacked you this turn")
    if snlow != low {
        if let Some((who_raw, rest)) = low
            .split_once(" gains control of ")
            .or_else(|| low.split_once(" gain control of "))
            && let (Some(controller), Ok(subj)) = (
                parse_player_ref(who_raw),
                parse_subject_or_target(rest, vocab),
            )
        {
            return Some(InstructionAst::GainControl {
                controller,
                subject: Box::new(subj),
                duration: DurationAst::Permanent,
            });
        }
        if let Some(rest) = low.strip_prefix("gain control of ")
            && let Ok(subj) = parse_subject_or_target(rest, vocab)
        {
            return Some(InstructionAst::GainControl {
                controller: PlayerRefAst::You,
                subject: Box::new(subj),
                duration: DurationAst::Permanent,
            });
        }
    }
    // "double its power" / "double <subj>'s power and toughness" / "double the number of <kind> counters on <subj>"
    if let Some(after) = snlow.strip_prefix("double the number of ")
        && let Some((kind, subj_raw)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::DoubleStatOrCounters {
            subject: Box::new(subj),
            kind: format!("counters:{}", kind.trim()),
            duration: DurationAst::Permanent,
        });
    }
    if let Some((mult_prefix, after)) = snlow
        .strip_prefix("double ")
        .map(|r| ("", r))
        .or_else(|| snlow.strip_prefix("triple ").map(|r| ("triple_", r)))
    {
        if let Some(subj_raw) = after
            .strip_prefix("the power and toughness of ")
            .or_else(|| after.strip_prefix("the power of "))
            .or_else(|| after.strip_prefix("the toughness of "))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let kind = if after.starts_with("the power and toughness of ") {
                "power_and_toughness"
            } else if after.starts_with("the power of ") {
                "power"
            } else {
                "toughness"
            };
            return Some(InstructionAst::DoubleStatOrCounters {
                subject: Box::new(subj),
                kind: format!("{mult_prefix}{kind}"),
                duration: dur.clone(),
            });
        }
        for (suf, kind) in [
            ("'s power and toughness", "power_and_toughness"),
            ("'s power", "power"),
            ("'s toughness", "toughness"),
        ] {
            if let Some(subj_raw) = after.strip_suffix(suf)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                return Some(InstructionAst::DoubleStatOrCounters {
                    subject: Box::new(subj),
                    kind: format!("{mult_prefix}{kind}"),
                    duration: dur.clone(),
                });
            }
        }
        if matches!(
            after,
            "its power" | "its toughness" | "its power and toughness"
        ) {
            return Some(InstructionAst::DoubleStatOrCounters {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                kind: format!("{mult_prefix}{}", after.trim_start_matches("its ")),
                duration: dur.clone(),
            });
        }
    }
    if let Some(after) = low.strip_prefix("prevent the next ")
        && let Some((amt_raw, _)) = after
            .split_once(" combat damage that ")
            .or_else(|| after.split_once(" damage that "))
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        let tgt = after
            .rsplit_once(" to ")
            .and_then(|(_, t)| {
                parse_subject_or_target(t.trim_end_matches(" this turn"), vocab).ok()
            })
            .unwrap_or(SubjectRefAst::AnyTarget);
        return Some(InstructionAst::PreventDamage {
            amount: Box::new(amt),
            target: Box::new(tgt),
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    if low == "prevent that damage" || low == "prevent all of that damage" {
        return Some(InstructionAst::PreventAllDamage {
            combat_only: false,
            target: None,
            duration: DurationAst::Instant,
        });
    }
    if let Some(amt_raw) = low
        .strip_prefix("prevent ")
        .and_then(|r| r.strip_suffix(" of that damage"))
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(InstructionAst::PreventDamage {
            amount: Box::new(amt),
            target: Box::new(SubjectRefAst::AnyTarget),
            duration: DurationAst::Instant,
        });
    }
    if low.starts_with("prevent all ")
        && (low.contains("damage that would be dealt")
            || low.contains(" would deal ")
            || low.ends_with(" would deal this turn")
            || low.ends_with(" would deal"))
    {
        let combat_only = low.starts_with("prevent all combat ");
        let body = low
            .strip_prefix("prevent all combat damage ")
            .or_else(|| low.strip_prefix("prevent all damage "))
            .unwrap_or("");
        let body_no_turn = body.trim_end_matches(" this turn").trim();
        // Form A: "prevent all [combat] damage [that] <source> would deal [to <recipient>] [this turn]"
        // (Chain of Silence, Dromoka's Command, Hallow, Frontline Strategist, Chameleon Blur)
        if let Some((src_part, after_deal)) = body_no_turn.split_once(" would deal") {
            let src_raw = src_part.trim();
            if !src_raw.is_empty() && src_raw != "that" {
                let source = parse_subject_or_target(src_raw, vocab)
                    .or_else(|_| {
                        parse_subject_or_target(
                            src_raw.strip_prefix("that ").unwrap_or(src_raw),
                            vocab,
                        )
                    })
                    .ok()?;
                let after_clean = after_deal.trim();
                let recipient = if after_clean.is_empty() {
                    None
                } else {
                    Some(Box::new(
                        parse_subject_or_target(after_clean.strip_prefix("to ")?, vocab).ok()?,
                    ))
                };
                return Some(InstructionAst::PreventAllDamageFrom {
                    combat_only,
                    source: Box::new(source),
                    recipient,
                    duration: DurationAst::UntilEndOfTurn,
                });
            }
        }
        // Form B: "prevent all [combat] damage that would be dealt [to <recipient>] by <source> [this turn]"
        if let Some(after_dealt) = body_no_turn.strip_prefix("that would be dealt") {
            let after_dealt = after_dealt.trim();
            if let Some(by_src) = after_dealt.strip_prefix("by ") {
                let source = parse_subject_or_target(by_src.trim(), vocab)
                    .unwrap_or_else(|_| SubjectRefAst::Anaphora(by_src.trim().to_string()));
                return Some(InstructionAst::PreventAllDamageFrom {
                    combat_only,
                    source: Box::new(source),
                    recipient: None,
                    duration: DurationAst::UntilEndOfTurn,
                });
            }
            if let Some(after_to) = after_dealt.strip_prefix("to ")
                && let Some((rec_raw, src_raw)) = after_to.split_once(" by ")
            {
                let recipient = parse_subject_or_target(rec_raw.trim(), vocab)
                    .unwrap_or_else(|_| SubjectRefAst::Anaphora(rec_raw.trim().to_string()));
                let source = parse_subject_or_target(src_raw.trim(), vocab)
                    .unwrap_or_else(|_| SubjectRefAst::Anaphora(src_raw.trim().to_string()));
                return Some(InstructionAst::PreventAllDamageFrom {
                    combat_only,
                    source: Box::new(source),
                    recipient: Some(Box::new(recipient)),
                    duration: DurationAst::UntilEndOfTurn,
                });
            }
        }
        let target = low
            .split_once("dealt to ")
            .or_else(|| low.split_once("deal to "))
            .and_then(|(_, r)| {
                parse_subject_or_target(r.trim_end_matches(" this turn"), vocab).ok()
            })
            .map(Box::new);
        return Some(InstructionAst::PreventAllDamage {
            combat_only,
            target,
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    // "<player>'s life total becomes <val>"
    if let Some((who_raw, val_raw)) = low.split_once("'s life total becomes ")
        && let Ok(value) = parse_value_expr(val_raw, vocab)
    {
        let player = parse_player_ref(who_raw).unwrap_or(PlayerRefAst::ThatPlayer);
        return Some(InstructionAst::SetLifeTotal {
            player,
            value: Box::new(value),
        });
    }
    if let Some(val_raw) = low.strip_prefix("your life total becomes ")
        && let Ok(value) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::SetLifeTotal {
            player: PlayerRefAst::You,
            value: Box::new(value),
        });
    }
    if low.contains("choose new targets for the cop") {
        return Some(InstructionAst::MayChooseNewTargetsForCopy);
    }
    if let Some((_, rest)) = split_leading_player_verb(low, "copy", "copies") {
        let rest_clean = rest
            .split_once(" and may choose a new target for ")
            .or_else(|| rest.split_once(" and may choose new targets for "))
            .map_or(rest, |(b, _)| b.trim());
        let (core, exception) = match rest_clean.split_once(", except ") {
            Some((c, e)) => (c.trim(), Some(e.trim().to_string())),
            None => (rest_clean, None),
        };
        let (core, count) = if let Some(c) = core.strip_suffix(" twice") {
            (c.trim(), Some(Box::new(ValueExprAst::Const(2))))
        } else if let Some((c, times_raw)) = core.rsplit_once(' ')
            && times_raw == "times"
            && let Some((c2, num_tok)) = c.rsplit_once(' ')
            && let Ok(n) = parse_value_expr(num_tok, vocab)
        {
            (c2.trim(), Some(Box::new(n)))
        } else if let Some((c, _fe)) = core.split_once(" for each other ") {
            (c.trim(), Some(Box::new(ValueExprAst::PriorResult)))
        } else {
            (core, None)
        };
        if core == "the spell countered this way" {
            return Some(InstructionAst::Copy {
                subject: Box::new(SubjectRefAst::Anaphora(core.to_string())),
                exception,
                count,
            });
        }
        if let Ok(subj) = parse_subject_or_target(core, vocab) {
            return Some(InstructionAst::Copy {
                subject: Box::new(subj),
                exception,
                count,
            });
        }
    }
    None
}

pub(super) fn try_parse_named_keyword_actions(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    match low {
        "investigate" | "investigate once" => {
            return Some(InstructionAst::Investigate {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(1)),
            });
        }
        "investigate twice" => {
            return Some(InstructionAst::Investigate {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(2)),
            });
        }
        "you become the monarch" | "become the monarch" => {
            return Some(InstructionAst::BecomeMonarch {
                player: PlayerRefAst::You,
            });
        }
        "you take the initiative" | "take the initiative" => {
            return Some(InstructionAst::TakeInitiative {
                player: PlayerRefAst::You,
            });
        }
        "venture into the dungeon" => {
            return Some(InstructionAst::VentureIntoDungeon {
                player: PlayerRefAst::You,
            });
        }
        "the ring tempts you" => return Some(InstructionAst::RingTemptsYou),
        "populate" => {
            return Some(InstructionAst::Populate {
                count: Box::new(ValueExprAst::Const(1)),
            });
        }
        "populate twice" => {
            return Some(InstructionAst::Populate {
                count: Box::new(ValueExprAst::Const(2)),
            });
        }
        "manifest dread" => {
            return Some(InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(1)),
                dread: true,
                subject: None,
            });
        }
        "manifest the top card of your library" | "manifest it" => {
            return Some(InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(1)),
                dread: false,
                subject: None,
            });
        }
        "cloak the top card of your library" => {
            return Some(InstructionAst::Cloak {
                count: Box::new(ValueExprAst::Const(1)),
                subject: None,
            });
        }
        "forage" => return Some(InstructionAst::Forage),
        "time travel" => return Some(InstructionAst::TimeTravel),
        "open an attraction" => return Some(InstructionAst::OpenAttraction),
        "roll to visit your attractions" => return Some(InstructionAst::RollVisitAttractions),
        "clash with an opponent" => {
            return Some(InstructionAst::Clash {
                with_opponent: true,
            });
        }
        "planeswalk" => return Some(InstructionAst::Planeswalk),
        "set the top scheme of your scheme deck in motion"
        | "set another scheme in motion"
        | "set the top two schemes of your scheme deck in motion" => {
            return Some(InstructionAst::SetNextSchemeInMotion);
        }
        "assemble a contraption" => return Some(InstructionAst::AssembleContraption),
        "unlock a locked door of ~" | "unlock a locked door you control" => {
            return Some(InstructionAst::UnlockDoor {
                subject: Box::new(SubjectRefAst::SelfObject),
            });
        }
        "learn" => return Some(InstructionAst::Learn),
        "it explores again" | "~ explores again" => {
            return Some(InstructionAst::Explore {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            });
        }
        _ => {}
    }

    if let Some(cnt_raw) = low.strip_prefix("earthbend ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Earthbend {
            count: Box::new(cnt),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("harness ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(InstructionAst::Harness {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("airbend ") {
        let subj = parse_subject_or_target(subj_raw, vocab).ok().map(Box::new);
        return Some(InstructionAst::Airbend { subject: subj });
    }
    if let Some(after) = low.strip_prefix("empower ") {
        let mut parts = after.rsplitn(2, ' ');
        if let Some(cnt_raw) = parts.next()
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            let pw = parts.next().unwrap_or("").trim().to_string();
            if !pw.is_empty() {
                return Some(InstructionAst::Empower {
                    planeswalker: pw,
                    count: Box::new(cnt),
                });
            }
        }
    }
    if let Some(cnt_raw) = low.strip_prefix("teamwork ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Teamwork {
            count: Box::new(cnt),
        });
    }

    // Investigate with explicit player or count ("each player investigates", "investigate X times")
    if let Some(who_raw) = low
        .strip_suffix(" investigates")
        .or_else(|| low.strip_suffix(" investigate"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::Investigate {
            player,
            count: Box::new(ValueExprAst::Const(1)),
        });
    }
    if let Some(after) = low.strip_prefix("investigate ")
        && let Some(cnt_raw) = after.strip_suffix(" times")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Investigate {
            player: PlayerRefAst::You,
            count: Box::new(cnt),
        });
    }

    // Monarch / Initiative / Venture with explicit player
    if let Some(who_raw) = low
        .strip_suffix(" becomes the monarch")
        .or_else(|| low.strip_suffix(" become the monarch"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::BecomeMonarch { player });
    }
    if let Some(who_raw) = low
        .strip_suffix(" takes the initiative")
        .or_else(|| low.strip_suffix(" take the initiative"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::TakeInitiative { player });
    }
    if let Some(who_raw) = low
        .strip_suffix(" ventures into the dungeon")
        .or_else(|| low.strip_suffix(" venture into the dungeon"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::VentureIntoDungeon { player });
    }

    // Player counters: Energy `{E}{E}`, Tickets `{TK}{TK}`, Poison, Rad, Experience
    if let Some((who, after)) = split_leading_player_verb(s, "get", "gets") {
        if after == "that many {E}" {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: "energy".to_string(),
                count: Box::new(ValueExprAst::PriorResult),
            });
        }
        if let Some(n) = count_symbol_sequence(after, "{E}") {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: "energy".to_string(),
                count: Box::new(ValueExprAst::Const(i32::from(n))),
            });
        }
        if let Some(n) = count_symbol_sequence(after, "{TK}") {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: "ticket".to_string(),
                count: Box::new(ValueExprAst::Const(i32::from(n))),
            });
        }
        let alow = after.to_ascii_lowercase();
        if let Some(after_num) = alow.strip_prefix("a number of ")
            && let Some((kind, val_raw)) = after_num.split_once(" counters equal to ")
            && matches!(
                kind.trim(),
                "poison" | "rad" | "experience" | "energy" | "ticket"
            )
            && let Ok(cnt) = parse_value_expr(val_raw, vocab)
        {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: kind.trim().to_string(),
                count: Box::new(cnt),
            });
        }
        if let Some(left) = alow
            .strip_suffix(" counters")
            .or_else(|| alow.strip_suffix(" counter"))
        {
            let (cnt, kind) = peel_leading_count(left, vocab);
            if matches!(kind, "poison" | "rad" | "experience" | "energy" | "ticket") {
                return Some(InstructionAst::GainPlayerCounters {
                    player: who,
                    counter_kind: kind.to_string(),
                    count: Box::new(cnt),
                });
            }
        }
    }

    // Numeric keyword actions
    if let Some(cnt_raw) = low.strip_prefix("monstrosity ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Monstrosity {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("adapt ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Adapt {
            count: Box::new(cnt),
        });
    }
    if let Some(after) = low.strip_prefix("amass ") {
        let mut parts = after.rsplitn(2, ' ');
        if let Some(cnt_raw) = parts.next()
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            let subtype = parts.next().unwrap_or("Zombies").trim().to_string();
            return Some(InstructionAst::Amass {
                subtype,
                count: Box::new(cnt),
            });
        }
    }
    if let Some(cnt_raw) = low.strip_prefix("incubate ") {
        let clean = cnt_raw.strip_suffix(" twice").unwrap_or(cnt_raw).trim();
        if let Ok(cnt) = parse_value_expr(clean, vocab) {
            return Some(InstructionAst::Incubate {
                count: Box::new(cnt),
            });
        }
    }
    if let Some((_, val_raw)) = split_leading_player_verb(low, "discover", "discovers")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::Discover {
            value: Box::new(val),
        });
    }
    if let Some(val_raw) = low.strip_prefix("collect evidence ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::CollectEvidence {
            value: Box::new(val),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("bolster ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Bolster {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("support ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Support {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("fateseal ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Fateseal {
            count: Box::new(cnt),
        });
    }
    if let Some((_, cnt_raw)) = low
        .split_once(" endures ")
        .or_else(|| low.split_once(" endure "))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Endure {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("endure ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Endure {
            count: Box::new(cnt),
        });
    }
    if let Some((who, cnt_raw)) = split_leading_player_verb(low, "blight", "blights")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Blight {
            player: who,
            count: Box::new(cnt),
        });
    }

    // Subject-targeted keyword actions
    if let Some(subj_raw) = low
        .strip_suffix(" explores")
        .or_else(|| low.strip_suffix(" explore"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Explore {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" becomes prepared")
        .or_else(|| low.strip_suffix(" become prepared"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::BecomePrepared {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("detain ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Detain {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("suspect ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Suspect {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("cloak ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Cloak {
            count: Box::new(ValueExprAst::Const(1)),
            subject: Some(Box::new(subj)),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("exert ")
        && let Ok(subj) = parse_subject_or_target(
            subj_raw.strip_suffix(" as it attacks").unwrap_or(subj_raw),
            vocab,
        )
    {
        return Some(InstructionAst::Exert {
            subject: Box::new(subj),
        });
    }
    if let Some((subj_raw, tail)) = low
        .split_once(" connives")
        .or_else(|| low.split_once(" connive"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let cnt = if tail.trim().is_empty() {
            ValueExprAst::Const(1)
        } else {
            parse_value_expr(tail.trim(), vocab).ok()?
        };
        return Some(InstructionAst::Connive {
            subject: Box::new(subj),
            count: Box::new(cnt),
        });
    }

    None
}

#[inline(never)]
pub(super) fn parse_extended_effect_fallbacks(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let nd_low = s_nodur.to_ascii_lowercase();

    // 1. Direct "Until <dur>, <effect_or_static_or_trigger>" prefix combinator
    //    (The Brothers' War, Nuka-Nuke Launcher, Guardian Angel, Temporal Aperture)
    if low.starts_with("until ")
        && let Some((dur_hdr, body_raw)) = s.split_once(", ")
        && !dur_hdr.is_empty()
        && !body_raw.is_empty()
    {
        let hdr_low = dur_hdr.trim().to_ascii_lowercase();
        let (parsed_dur, needs_scope) = match hdr_low.as_str() {
            "until end of turn" => (DurationAst::UntilEndOfTurn, false),
            "until end of combat" => (DurationAst::UntilEndOfCombat, false),
            "until your next turn" => (DurationAst::UntilYourNextTurn, false),
            "until the end of your next turn" => (DurationAst::UntilEndOfNextTurn, false),
            "until the end of that player's next turn" => (
                DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::ThatPlayer),
                false,
            ),
            "until ~ leaves the battlefield" => (DurationAst::UntilSourceLeaves, false),
            _ => (DurationAst::UntilYourNextTurn, true),
        };
        if let Some(body) = parse_effect_or_temp_clause(body_raw.trim(), &parsed_dur, vocab) {
            if needs_scope {
                return Some(vec![InstructionAst::ScopedDurationText {
                    duration_text: dur_hdr.trim().to_string(),
                    body,
                }]);
            }
            return Some(body);
        }
    }

    // 2. General "<subj> doesn't/don't untap [during <step>]" (Telekinesis, Pollen Lullaby, Triton Tactics)
    for pat in [
        " doesn't untap",
        " don't untap",
        " doesn't untap during the player's next untap step",
        " don't untap during the player's next untap step",
        " doesn't untap during its controller's next untap step",
        " don't untap during their controllers' next untap steps",
    ] {
        if let Some(subj_raw) = nd_low.strip_suffix(pat).or_else(|| low.strip_suffix(pat))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::CombatRuleMod {
                subject: Box::new(subj),
                rule: "doesnt_untap".to_string(),
                duration: dur.clone(),
            }]);
        }
    }

    // 3. General "<subj> attacks [<tgt>] [each combat] if able" (Taunt, Gideon Jura, The Brothers' War)
    if let Some(before_able) = nd_low
        .strip_suffix(" each combat if able")
        .or_else(|| nd_low.strip_suffix(" if able"))
        && let Some((subj_raw, tgt_raw)) = before_able
            .split_once(" attacks ")
            .or_else(|| before_able.split_once(" attack "))
        && let (Ok(subj), Ok(_tgt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("attacks_if_able:{}", tgt_raw.trim()),
            duration: dur.clone(),
        }]);
    }

    // 4. General "<subj> must be blocked [by <filter>] [each combat] if able" (Anzrag)
    if let Some(subj_raw) = nd_low
        .strip_suffix(" must be blocked each combat if able")
        .or_else(|| nd_low.strip_suffix(" must be blocked if able"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "must_be_blocked_if_able".to_string(),
            duration: dur.clone(),
        }]);
    }

    // 5. General "players can't lose life / lose the game / win the game" (Everybody Lives!)
    if matches!(
        nd_low.as_str(),
        "players can't lose life"
            | "players can't gain life"
            | "players can't lose the game or win the game"
            | "players can't win the game or lose the game"
            | "you can't lose the game and your opponents can't win the game"
    ) {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: nd_low,
            condition: None,
        }]);
    }

    // 6. General "X can't be greater than <val>" (Mortarion, Nameless Race)
    if let Some(val_raw) = low
        .strip_prefix("x can't be greater than ")
        .or_else(|| low.strip_prefix("the amount you pay can't be more than "))
        && parse_value_expr(val_raw.trim(), vocab).is_ok()
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }

    // 7. General "<player> [may] shuffle[s] <subj_or_zones> into <player>'s library" (Repopulate, Visions, Step Between Worlds, Trenzalore Clocktower)
    if let Some(after_shuf) = low
        .strip_prefix("shuffle ")
        .or_else(|| low.strip_prefix("each player shuffles "))
        .or_else(|| low.strip_prefix("each player may shuffle "))
        .or_else(|| low.strip_prefix("that player shuffles "))
        .or_else(|| low.strip_prefix("you may then have that player shuffle "))
    {
        if after_shuf == "that library" || after_shuf == "their library" {
            return Some(vec![InstructionAst::ShuffleLibrary {
                player: PlayerRefAst::ThatPlayer,
            }]);
        }
        if let Some((obj_raw, _lib_raw)) = after_shuf.split_once(" into ")
            && (obj_raw.contains("hand and graveyard")
                || obj_raw.contains("graveyard and hand")
                || parse_subject_or_target(obj_raw.trim(), vocab).is_ok())
        {
            return Some(vec![InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(
                    parse_subject_or_target(obj_raw.trim(), vocab)
                        .unwrap_or_else(|_| SubjectRefAst::Anaphora(obj_raw.trim().to_string())),
                ),
            }]);
        }
    }

    // 8. General "<subj> deals <val> damage to <tgt>" (Aura Barbs, Widespread Brutality)
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Some((amt_raw, tgt_raw)) = rest.split_once(" damage to ")
        && !tgt_raw.contains(" and ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(
                amt_raw
                    .trim()
                    .strip_prefix("equal to ")
                    .unwrap_or(amt_raw.trim()),
                vocab,
            ),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        }]);
    }
    if let Some((src_raw, rest)) = low.split_once(" deals damage equal to ")
        && let Some((amt_raw, tgt_raw)) = rest.split_once(" to ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        }]);
    }

    // 9. Trailing "<effect> when <event>" delayed trigger (Stangg: "Sacrifice ~ when that token leaves the battlefield")
    if !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && let Some(idx) = find_top_level_pattern(s, " when ")
    {
        let eff_raw = s[..idx].trim();
        let trig_raw = format!("when {}", s[idx + 6..].trim());
        if let (Ok(trigger), Ok(body)) = (
            parse_trigger_header(&trig_raw, vocab),
            parse_atomic_effect(eff_raw, vocab),
        ) {
            return Some(vec![InstructionAst::TemporaryTrigger {
                trigger,
                body,
                duration: DurationAst::Permanent,
            }]);
        }
    }

    // 10. General "<player> chooses <opt1> or <opt2>" / "you may choose new targets for <subj>" / "the new target must be <filter>" / "each copy targets a different one of <subj>"
    if let Some((who, after_ch)) = split_leading_player_verb(s, "choose", "chooses")
        && let Some((a, b)) = after_ch.split_once(" or ")
        && !a.trim().is_empty()
        && !b.trim().is_empty()
        && a.split_whitespace().count() <= 4
        && b.split_whitespace().count() <= 4
    {
        if who == PlayerRefAst::You {
            return Some(vec![InstructionAst::ChooseOption(after_ch.to_string())]);
        }
        return Some(vec![InstructionAst::PlayerChooseOption {
            chooser: who,
            options: vec![a.trim().to_string(), b.trim().to_string()],
        }]);
    }
    if low == "you may choose new targets for that copy"
        || low == "you may choose new targets for the copy"
        || low == "you may choose new targets for the copies"
    {
        return Some(vec![InstructionAst::MayChooseNewTargetsForCopy]);
    }
    if let Some(flt_raw) = low.strip_prefix("the new target must be ")
        && parse_object_filter(flt_raw.trim(), vocab).is_ok()
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }
    if low.starts_with("each copy targets a different one of ") {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }

    // 11. General "<subj> can cast only <N> more spell(s) this turn" (Irencrag Feat)
    if nd_low.starts_with("you can cast only ") && nd_low.contains(" spell") {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: nd_low,
            condition: None,
        }]);
    }

    // 12. General "<player> draws <val> cards plus an additional card for each <over>" (Commander's Insight)
    if let Some((who, rest)) = split_leading_player_verb(s, "draw", "draws")
        && let Some((base_part, fe_raw)) = rest
            .to_ascii_lowercase()
            .split_once(" plus an additional card for each ")
        && let Ok(base_v) = parse_value_expr(
            base_part
                .strip_suffix(" cards")
                .or_else(|| base_part.strip_suffix(" card"))
                .unwrap_or(base_part)
                .trim(),
            vocab,
        )
        && let Ok(fe_v) = parse_for_each_value_expr(fe_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Draw {
            player: who,
            count: Box::new(ValueExprAst::Add(Box::new(base_v), Box::new(fe_v))),
            up_to: false,
        }]);
    }

    // 13. General "<subj> becomes blocked / become blocked / become unblocked" & "exchange <s1>'s power and <s2>'s power"
    if let Some(subj_raw) = nd_low
        .strip_suffix(" become blocked")
        .or_else(|| nd_low.strip_suffix(" becomes blocked"))
        .or_else(|| nd_low.strip_suffix(" become unblocked"))
        .or_else(|| nd_low.strip_suffix(" becomes unblocked"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: if nd_low.ends_with("unblocked") {
                "becomes_unblocked".to_string()
            } else {
                "becomes_blocked".to_string()
            },
            duration: dur.clone(),
        }]);
    }
    if let Some(after_ex) = nd_low.strip_prefix("exchange ")
        && after_ex.contains(" power")
        && let Some((s1_raw, s2_raw)) = after_ex.split_once(" and ")
        && !s1_raw.is_empty()
        && !s2_raw.is_empty()
    {
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::LifeWithStat(StatKind::Power),
            left: Box::new(SubjectRefAst::Anaphora(s1_raw.trim().to_string())),
            right: Some(Box::new(SubjectRefAst::Anaphora(s2_raw.trim().to_string()))),
        }]);
    }

    // 14. General "<player> reveals <val> cards [at random] from their hand" / "you may reveal ~ from your opening hand"
    if low == "you may reveal ~ from your opening hand" {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::AbilityRestriction {
                restriction: "reveal_opening_hand".to_string(),
                condition: None,
            },
        ])]);
    }
    if let Some((who, after_rev)) = split_leading_player_verb(s, "reveal", "reveals") {
        let ar_low = after_rev.to_ascii_lowercase();
        if let Some(cnt_part) = ar_low
            .strip_suffix(" at random from their hand")
            .or_else(|| ar_low.strip_suffix(" from their hand"))
            .or_else(|| ar_low.strip_suffix(" from your hand"))
        {
            let cnt_clean = cnt_part
                .strip_suffix(" cards")
                .or_else(|| cnt_part.strip_suffix(" card"))
                .unwrap_or(cnt_part)
                .trim();
            if parse_value_expr(cnt_clean, vocab).is_ok() {
                return Some(vec![InstructionAst::RevealHand { player: who }]);
            }
        }
    }

    // 15. General "<subj> has <kw_list>" (Estrid: "The token has enchant permanent and umbra armor")
    if let Some((subj_raw, kws_raw)) = nd_low
        .split_once(" has ")
        .or_else(|| nd_low.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let kws_trim = kws_raw.trim();
        let keywords_opt = if let Some(kws) = parse_keyword_list(kws_trim, vocab) {
            Some(kws)
        } else if kws_trim.starts_with("enchant ") {
            Some(
                kws_trim
                    .split(" and ")
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty())
                    .collect(),
            )
        } else {
            None
        };
        if let Some(keywords) = keywords_opt {
            return Some(vec![InstructionAst::GrantKeywords {
                subject: Box::new(subj),
                keywords,
                duration: dur.clone(),
            }]);
        }
    }
    if let Some((subj_raw, lw_raw)) = nd_low.split_once(" gains landwalk of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec![format!("landwalk:{lw_raw}")],
            duration: dur.clone(),
        }]);
    }

    // 16. General "this way" cost modifier inside an effect (Mavinda: "it costs {8} more to cast this way")
    if let Some(before_tw) = low.strip_suffix(" this way")
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(before_tw, vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(st),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 17. General "it costs {N} less to cast ..." ("it also costs {1} less to cast if ..." - Geistlight Snare)
    if (low.starts_with("it costs ") || low.starts_with("it also costs "))
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(s, vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(st),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 18. General "for each <over>, <effect_or_temp_clause>" fallback
    if low.starts_with("for each ")
        && let Some((over_raw, body_raw)) = s["for each ".len()..].split_once(", ")
        && let Ok(cnt) = parse_for_each_value_expr(over_raw.trim(), vocab)
        && let Some(body) = parse_effect_or_temp_clause(body_raw.trim(), dur, vocab)
    {
        return Some(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body,
        }]);
    }

    // 19. Wave 18: General "<src> deals an amount of damage of your choice to <t1> [and <t2>]" (Volcano Hellion)
    if let Some((src_raw, tgts_raw)) =
        low.split_once(" deals an amount of damage of your choice to ")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        if let Some((t1_raw, t2_raw)) = tgts_raw.split_once(" and ")
            && let (Ok(t1), Ok(t2)) = (
                parse_subject_or_target(t1_raw.trim(), vocab),
                parse_subject_or_target(t2_raw.trim(), vocab),
            )
        {
            return Some(vec![
                InstructionAst::DealDamage {
                    source: Box::new(src.clone()),
                    target: Box::new(t1),
                    amount: Box::new(ValueExprAst::ChosenX),
                },
                InstructionAst::DealDamage {
                    source: Box::new(src),
                    target: Box::new(t2),
                    amount: Box::new(ValueExprAst::ChosenX),
                },
            ]);
        }
        if let Ok(tgt) = parse_subject_or_target(tgts_raw.trim(), vocab) {
            return Some(vec![InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(tgt),
                amount: Box::new(ValueExprAst::ChosenX),
            }]);
        }
    }

    // 20. Wave 18: General "<src> deals half <val> damage, rounded down/up, to <tgt>" (Banshee)
    if let Some((src_raw, rest)) = low.split_once(" deals half ")
        && let Some((amt_raw, tgt_raw)) = rest
            .split_once(" damage, rounded down, to ")
            .or_else(|| rest.split_once(" damage, rounded up, to "))
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        let rounding = if rest.contains("rounded up") {
            crate::ast::Rounding::Up
        } else {
            crate::ast::Rounding::Down
        };
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(ValueExprAst::DivConst {
                expr: Box::new(amt),
                denom: 2,
                rounding,
            }),
        }]);
    }

    // 21. Wave 18: General "<player> destroys <subj>" / "<player> taps all <filter>" / "<player> discards each <filter>"
    if let Some((_who, obj_raw)) = split_leading_player_verb(s, "destroy", "destroys")
        && let Ok(subj) = parse_subject_or_target(obj_raw, vocab)
    {
        return Some(vec![InstructionAst::Destroy {
            subject: Box::new(subj),
            random: false,
            except: None,
        }]);
    }
    if let Some((_who, obj_raw)) = split_leading_player_verb(s, "tap", "taps") {
        let o_clean = obj_raw
            .to_ascii_lowercase()
            .strip_prefix("all ")
            .map(str::to_string)
            .unwrap_or_else(|| obj_raw.to_string());
        if let Ok(subj) = parse_subject_or_target(&o_clean, vocab) {
            return Some(vec![InstructionAst::TapPermanent {
                subject: Box::new(subj),
                any_number: false,
            }]);
        }
    }
    if let Some((who, obj_raw)) = split_leading_player_verb(s, "discard", "discards")
        && let Some(flt_raw) = obj_raw.to_ascii_lowercase().strip_prefix("each ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Discard {
            player: who,
            count: None,
            filter: Some(Box::new(flt)),
            anaphora: None,
            random: false,
        }]);
    }

    // 22. Wave 18: General "exile <val> cards from the top" & "exile cards from the top of your library equal to <val>" (Vault 112, Archaic's Agony)
    if let Some(after_ex) = low.strip_prefix("exile ") {
        if let Some(cnt_part) = after_ex.strip_suffix(" cards from the top")
            && let Ok(cnt) = parse_value_expr(cnt_part.trim(), vocab)
        {
            return Some(vec![InstructionAst::ExileTopLibrary {
                actor: Some(PlayerRefAst::You),
                whose: "your".to_string(),
                count: Box::new(cnt),
                face_down: false,
            }]);
        }
        if let Some(val_raw) = after_ex.strip_prefix("cards from the top of your library equal to ")
            && let Ok(cnt) = parse_value_expr(val_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::ExileTopLibrary {
                actor: Some(PlayerRefAst::You),
                whose: "your".to_string(),
                count: Box::new(cnt),
                face_down: false,
            }]);
        }
    }

    // 23. Wave 18: "(Its|The) <kw> cost is equal to its mana cost" (Songcrafter Mage, Araumi, Falkenrath Gorger)
    if (low.starts_with("its ") || low.starts_with("the "))
        && low.ends_with(" cost is equal to its mana cost")
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }

    // 24. Wave 18: "<player> can't get counters" / "put the rest back on top of your library in any order" / "<subj> gets <pt> before it fights" / "<subj> loses/gains it"
    if let Some(who_raw) = nd_low.strip_suffix(" can't get counters")
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: nd_low,
            condition: None,
        }]);
    }
    if low == "put the rest back on top of your library in any order"
        || low == "put the rest on top of your library in any order"
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
            to: "top_of_library".to_string(),
        }]);
    }
    if let Some(before_fight) = low.strip_suffix(" before it fights")
        && let Ok(inner) = parse_atomic_effect(before_fight.trim(), vocab)
    {
        return Some(inner);
    }
    if let Some(subj_raw) = nd_low
        .strip_suffix(" loses it")
        .or_else(|| nd_low.strip_suffix(" gains it"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec![if nd_low.ends_with("loses it") {
                "lose_chosen_ability".to_string()
            } else {
                "gain_chosen_ability".to_string()
            }],
            duration: dur.clone(),
        }]);
    }
    if let Some(after_ret) = low.strip_prefix("return ") {
        for (dest_pat, dest_name) in [
            (" to their owners' hands", "hand"),
            (" to its owner's hand", "hand"),
            (
                " to the battlefield under their owner's control",
                "battlefield",
            ),
            (
                " to the battlefield under its owner's control",
                "battlefield",
            ),
            (" to the battlefield", "battlefield"),
        ] {
            if let Some(obj_raw) = after_ret.strip_suffix(dest_pat) {
                let obj_clean = obj_raw.strip_prefix("all ").unwrap_or(obj_raw).trim();
                if let Ok(subj) = parse_subject_or_target(obj_clean, vocab) {
                    return Some(vec![InstructionAst::MoveZone {
                        subject: Box::new(subj),
                        to: dest_name.to_string(),
                    }]);
                }
            }
        }
    }

    // 25. Wave 18b: "<player> flips a coin" / "<subj> can't attack <filter>" / "exile <subj> at random" / "put another counter of that kind on <subj>"
    if let Some(who_raw) = low
        .strip_suffix(" flips a coin")
        .or_else(|| low.strip_suffix(" flip a coin"))
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::FlipCoin]);
    }
    if let Some((subj_raw, tgt_raw)) = nd_low.split_once(" can't attack ")
        && let (Ok(subj), Ok(_tgt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("cant_attack:{}", tgt_raw.trim()),
            duration: dur.clone(),
        }]);
    }
    // Cluster 18: Temporary special-action timing windows (`any time you could activate a mana ability, <action>`, etc.)
    if let Some(after_at) = low.strip_prefix("any time you could ")
        && let Some((timing_raw, _body_raw)) = after_at.split_once(", ")
        && !timing_raw.is_empty()
        && let Ok(body) = parse_atomic_effect(
            &s["any time you could ".len() + timing_raw.len() + 2..],
            vocab,
        )
    {
        return Some(vec![InstructionAst::TemporarySpecialAction {
            timing: timing_raw.trim().to_string(),
            duration: dur.clone(),
            body,
        }]);
    }
    // Wave E2: Modal with trailing paired effect ("choose one [that hasn't been chosen] and <eff> [—]")
    if let Some(rest) = low
        .strip_prefix("choose one that hasn't been chosen and ")
        .or_else(|| low.strip_prefix("choose one and "))
    {
        let choice_kind = if low.starts_with("choose one that hasn't been chosen") {
            "one_not_chosen"
        } else {
            "one"
        };
        let eff_clean = rest.trim().trim_end_matches('\u{2014}').trim();
        if let Ok(mut extra) = parse_atomic_effect(eff_clean, vocab) {
            let mut out = vec![InstructionAst::Modal {
                choice: choice_kind.to_string(),
                modes: Vec::new(),
            }];
            out.append(&mut extra);
            return Some(out);
        }
    }
    // Wave E2: RepeatProcessExt ("Repeat this process X more times", "Repeat this process for first strike, ...", "Repeat this process until ...")
    if let Some(after_rep) = low
        .strip_prefix("then repeat this process ")
        .or_else(|| low.strip_prefix("repeat this process "))
    {
        if let Some(cnt_raw) = after_rep
            .strip_suffix(" more times")
            .or_else(|| after_rep.strip_suffix(" times"))
            && let Ok(val) = parse_value_expr(cnt_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::RepeatProcessExt {
                times: Some(Box::new(val)),
                for_list: Vec::new(),
                until: None,
            }]);
        }
        if let Some(list_raw) = after_rep.strip_prefix("for ") {
            let items: Vec<String> = list_raw
                .split(',')
                .map(|p| p.trim().trim_start_matches("and ").trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if !items.is_empty() {
                return Some(vec![InstructionAst::RepeatProcessExt {
                    times: None,
                    for_list: items,
                    until: None,
                }]);
            }
        }
        if let Some(until_raw) = after_rep.strip_prefix("until ") {
            return Some(vec![InstructionAst::RepeatProcessExt {
                times: None,
                for_list: Vec::new(),
                until: Some(until_raw.trim().to_string()),
            }]);
        }
    }
    // Wave E2: "For as long as <cond>, <effect>"
    if let Some(after_fa) = low.strip_prefix("for as long as ")
        && let Some((cond_raw, _)) = after_fa.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let body_slice = s["for as long as ".len() + cond_raw.len() + 2..].trim();
        let (ins, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !ins.is_empty() {
            return Some(vec![InstructionAst::ForAsLongAs {
                condition: Box::new(cond),
                body: ins,
            }]);
        }
        if let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(body_slice, vocab)
        {
            return Some(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(st),
                duration: DurationAst::ForAsLongAs(Box::new(cond)),
            }]);
        }
    }
    // Wave E2: "When you next <event>, <effect>"
    if let Some(after_wn) = low.strip_prefix("when you next ")
        && let Some((ev_raw, _)) = after_wn.split_once(", ")
        && !ev_raw.is_empty()
    {
        let body_slice = s["when you next ".len() + ev_raw.len() + 2..].trim();
        let (ins, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !ins.is_empty() {
            return Some(vec![InstructionAst::WhenYouNext {
                event: ev_raw.trim().to_string(),
                body: ins,
                duration: if ev_raw.contains("this turn") {
                    DurationAst::UntilEndOfTurn
                } else {
                    DurationAst::Permanent
                },
            }]);
        }
    }
    // Wave E2: Dual variable binding "<eff>, where X is <e1> and Y is <e2>" (Bioplasm)
    if let Some((before_wx, xy_raw)) = low.split_once(", where x is ")
        && let Some((x_raw, y_raw)) = xy_raw.split_once(" and y is ")
        && let (Ok(xv), Ok(yv)) = (
            parse_value_expr(x_raw.trim(), vocab),
            parse_value_expr(y_raw.trim(), vocab),
        )
        && let Ok(body) = parse_atomic_effect(&s[..before_wx.len()], vocab)
    {
        return Some(vec![InstructionAst::BindXY {
            x_expr: Box::new(xv),
            y_expr: Box::new(yv),
            body,
        }]);
    }
    // Wave E2: Temporary mana-spending rule ("you may spend [colorless/white/...] mana as though it were mana of any color [to ...]")
    if let Some(after_sp) = low
        .strip_prefix("you may spend ")
        .or_else(|| low.strip_prefix("mana of any type can be spent "))
    {
        if low.starts_with("mana of any type can be spent ") {
            return Some(vec![InstructionAst::TemporaryManaSpendRule {
                mana_kind: None,
                as_though: "any_type".to_string(),
                purpose: Some(after_sp.trim().to_string()),
                duration: dur.clone(),
                next_only: false,
            }]);
        }
        if let Some((kind_raw, rest)) = after_sp.split_once(" as though it were ") {
            let (as_though, purpose) = if let Some((a, p)) = rest.split_once(" to ") {
                (a.trim().to_string(), Some(p.trim().to_string()))
            } else {
                (rest.trim().to_string(), None)
            };
            let mk = kind_raw.trim();
            return Some(vec![InstructionAst::TemporaryManaSpendRule {
                mana_kind: if mk == "mana" {
                    None
                } else {
                    Some(mk.to_string())
                },
                as_though,
                purpose,
                duration: dur.clone(),
                next_only: false,
            }]);
        }
    }
    // Wave E2: Conditional alternative cost inside spell ("If <cond>, rather than pay ~'s mana cost, <eff>")
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", rather than pay ~'s mana cost, ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(body) = parse_atomic_effect(rest.trim(), vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: body,
        }]);
    }
    parse_specialized_effect_clauses_a(s, low, s_nodur, dur, vocab)
}

#[inline(never)]
fn parse_specialized_effect_clauses_a(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let nd_low = s_nodur.to_ascii_lowercase();

    // E3.1: Characteristic-only / target-only modal options ("3/1 Human Warrior with trample and haste", "1/5 Spider with reach", "Target land card")
    if let Some((first_tok, rest)) = s.split_once(' ')
        && parse_base_pt(first_tok).is_some()
        && !rest.is_empty()
        && !low.contains(" gets ")
        && !low.contains(" deals ")
    {
        return Some(vec![InstructionAst::TokenCharacteristicsChoice(
            s.to_string(),
        )]);
    }
    if low.starts_with("target ")
        && low.ends_with(" card")
        && parse_object_filter(low.trim_start_matches("target "), vocab).is_ok()
    {
        return Some(vec![InstructionAst::TokenCharacteristicsChoice(
            s.to_string(),
        )]);
    }

    // E3.2: ChooseCounter ("choose a [kind of] counter on <subj>")
    if let Some(after_cc) = low
        .strip_prefix("choose a kind of counter on ")
        .or_else(|| low.strip_prefix("choose a counter on "))
        && let Ok(subj) = parse_subject_or_target(after_cc.trim(), vocab)
    {
        return Some(vec![InstructionAst::ChooseCounter {
            subject: Box::new(subj),
            kind_only: low.starts_with("choose a kind of counter on "),
        }]);
    }
    // E3.3: MoveCounter ("move a [<kind>] counter from <subj1> onto <subj2>")
    if let Some(after_mc) = low.strip_prefix("move a ")
        && let Some((kind_part, rest)) = after_mc.split_once("counter from ")
        && let Some((from_raw, to_raw)) = rest.split_once(" onto ")
        && let (Ok(from), Ok(to)) = (
            parse_subject_or_target(from_raw.trim(), vocab),
            parse_subject_or_target(to_raw.trim(), vocab),
        )
    {
        let k = kind_part.trim();
        return Some(vec![InstructionAst::MoveCounter {
            counter_kind: if k.is_empty() {
                None
            } else {
                Some(k.to_string())
            },
            from: Box::new(from),
            to: Box::new(to),
        }]);
    }
    // E3.4: Put a counter of that kind on <subj>
    if let Some(subj_raw) = low.strip_prefix("put a counter of that kind on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: "chosen_kind".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    // E3.5: Sticker placement ("put up to <N> <kind> stickers on <subj>", "put an art/name/ability sticker on <subj>")
    if let Some(after_ps) = low.strip_prefix("put ")
        && after_ps.contains(" sticker")
        && let Some((cnt_kind, subj_raw)) = after_ps.split_once(" on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: format!("sticker:{}", cnt_kind.trim()),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    // E3.6: Remove counters generalized ("remove all counters from <subj>", "remove any number of [<kind>] counters from [among] <subj>", "remove a <kind> counter from each of ...")
    if let Some(after_rc) = low.strip_prefix("remove ") {
        if let Some(subj_raw) = after_rc.strip_prefix("all counters from ")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::RemoveCounters {
                subject: Box::new(subj),
                counter_kind: "any".to_string(),
                all: true,
                count: None,
            }]);
        }
        if let Some(rest) = after_rc.strip_prefix("any number of ")
            && let Some((ck_raw, subj_raw)) = rest
                .split_once("counters from among ")
                .or_else(|| rest.split_once("counters from "))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            let ck = ck_raw.trim();
            return Some(vec![InstructionAst::RemoveCounters {
                subject: Box::new(subj),
                counter_kind: if ck.is_empty() {
                    "any".to_string()
                } else {
                    ck.to_string()
                },
                all: false,
                count: Some(Box::new(ValueExprAst::ChosenX)),
            }]);
        }
        if let Some((cnt_ck, subj_raw)) = after_rc
            .split_once(" counters from each ")
            .or_else(|| after_rc.split_once(" counter from each of "))
            .or_else(|| after_rc.split_once(" counter from "))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            let (cnt, ck_str) = peel_leading_count(cnt_ck.trim(), vocab);
            return Some(vec![InstructionAst::RemoveCounters {
                subject: Box::new(subj),
                counter_kind: if ck_str.trim().is_empty() {
                    "any".to_string()
                } else {
                    ck_str.trim().to_string()
                },
                all: false,
                count: Some(Box::new(cnt)),
            }]);
        }
    }

    // E3.7: Suspect / Unsuspect / Ungoad
    if let Some(subj_raw) = low
        .strip_suffix(" is no longer goaded")
        .or_else(|| low.strip_suffix(" are no longer goaded"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::SetGoaded {
            subject: Box::new(subj),
            remove: true,
        }]);
    }
    if let Some(subj_raw) = low
        .strip_suffix(" is no longer suspected")
        .or_else(|| low.strip_suffix(" are no longer suspected"))
        .or_else(|| low.strip_suffix("'re no longer suspected"))
        .or_else(|| low.strip_suffix("'s no longer suspected"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::SetSuspected {
            subject: Box::new(subj),
            remove: true,
        }]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("you may have ")
        .and_then(|r| r.strip_suffix(" become no longer suspected"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::SetSuspected {
                subject: Box::new(subj),
                remove: true,
            },
        ])]);
    }
    if let Some(subj_raw) = low.strip_prefix("suspect ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::SetSuspected {
            subject: Box::new(subj),
            remove: false,
        }]);
    }

    // E3.8: Turn face down / Phase out any number / Tap as it phases in
    if let Some(subj_raw) = low
        .strip_prefix("turn ")
        .and_then(|r| r.strip_suffix(" face down"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TurnFaceDown {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("have ")
        .and_then(|r| r.strip_suffix(" phase out"))
        && let Some(inner_raw) = subj_raw.strip_prefix("any number of ")
        && let Ok(subj) = parse_subject_or_target(inner_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PhaseOutUntil {
            subject: Box::new(subj),
            until: DurationAst::UntilYourNextTurn,
        }]);
    }
    if low == "tap that creature as it phases in this way" {
        return Some(vec![InstructionAst::TapPermanent {
            subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            any_number: false,
        }]);
    }

    // E3.9: Enters tapped [and attacking ...] [this turn]
    if let Some((subj_raw, mod_raw)) = nd_low
        .split_once(" enters tapped")
        .or_else(|| nd_low.split_once(" enter tapped"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let m = mod_raw.trim();
        let attacking = m
            .strip_prefix("and attacking")
            .map(|r| r.trim().to_string());
        if m.is_empty() || attacking.is_some() {
            return Some(vec![InstructionAst::EntersTappedRule {
                subject: Box::new(subj),
                tapped: true,
                attacking,
                duration: dur.clone(),
            }]);
        }
    }

    // E3.10: May choose new targets for copy & change targets
    if let Some((who_raw, _)) = low
        .split_once(" may choose a new target for ")
        .or_else(|| low.split_once(" may choose new targets for "))
        && (low.ends_with(" the copy")
            || low.ends_with(" that copy")
            || low.ends_with(" their copy")
            || low.ends_with(" the copies"))
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::MayChooseNewTargetsForCopy]);
    }
    if let Some(after_ct) = low.strip_prefix("change the target to ") {
        let tgt_raw = after_ct.strip_suffix(" if able").unwrap_or(after_ct).trim();
        if let Ok(nt) = parse_subject_or_target(tgt_raw, vocab) {
            let must_target = if after_ct.ends_with(" if able") {
                Some(Box::new(nt.clone()))
            } else {
                None
            };
            return Some(vec![InstructionAst::ChangeTargets {
                target: Box::new(SubjectRefAst::Anaphora("target".to_string())),
                new_target: Some(Box::new(nt)),
                must_target,
                single_target_only: true,
            }]);
        }
    }
    if let Some(subj_raw) = low.strip_prefix("change the targets of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ChangeTargets {
            target: Box::new(subj),
            new_target: None,
            must_target: None,
            single_target_only: false,
        }]);
    }

    // E3.11: Exile generalized (middle "at random", multi-zone hands+graveyards, from each graveyard, any number / one or more, top half / equal to power)
    if let Some((who, after_ex)) = split_leading_player_verb(low, "exile", "exiles") {
        let actor = if who == PlayerRefAst::You {
            None
        } else {
            Some(who)
        };
        if let Some((before_rand, after_rand)) = after_ex.split_once(" at random from ") {
            let (_, flt_raw) = peel_leading_count(before_rand.trim(), vocab);
            if let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab) {
                let zone_probe = format!("card from {}", after_rand.trim());
                if let Ok(zf) = parse_object_filter(&zone_probe, vocab) {
                    flt.zone = zf.zone;
                }
                return Some(vec![InstructionAst::Exile {
                    actor,
                    subject: Box::new(SubjectRefAst::All(Box::new(flt))),
                    random: true,
                    face_down: false,
                    into_pile: false,
                }]);
            }
        }
        if let Some(rest_one) = after_ex
            .strip_prefix("one or more ")
            .or_else(|| after_ex.strip_prefix("any number of "))
            && let Ok(subj) = parse_subject_or_target(rest_one.trim(), vocab)
        {
            return Some(vec![InstructionAst::Exile {
                actor,
                subject: Box::new(subj),
                random: false,
                face_down: false,
                into_pile: false,
            }]);
        }
        if after_ex.starts_with("all cards from ")
            && (after_ex.contains("hands and graveyards")
                || after_ex.contains("hand and graveyard"))
        {
            return Some(vec![InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_ex.to_string())),
                random: false,
                face_down: false,
                into_pile: false,
            }]);
        }
        if let Some(rest_top) = after_ex.strip_prefix("the top half of ")
            && let Some((lib_raw, _)) = rest_top.split_once(" library")
        {
            let _ = lib_raw;
            return Some(vec![InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_ex.to_string())),
                random: false,
                face_down: false,
                into_pile: false,
            }]);
        }
        if after_ex.starts_with("cards equal to ") && after_ex.contains(" from the top of ") {
            return Some(vec![InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_ex.to_string())),
                random: false,
                face_down: false,
                into_pile: false,
            }]);
        }
        if matches!(
            after_ex,
            "one of those piles" | "one of the piles" | "the other pile" | "that pile"
        ) {
            return Some(vec![InstructionAst::PileAction {
                action: "exile".to_string(),
                pile: if after_ex.contains("other") {
                    PileRefAst::OtherPile
                } else {
                    PileRefAst::ChosenPile
                },
                filter: None,
            }]);
        }
    }

    // E3.12: Pile actions ("Tap all lands in the other piles", "Destroy all lands in the chosen piles", "Look at the cards in the other pile", "<player> chooses one of those/the/their piles")
    for (verb, action_name) in [
        ("tap ", "tap"),
        ("destroy ", "destroy"),
        ("exile ", "exile"),
        ("look at ", "look_at"),
        ("sacrifice ", "sacrifice"),
    ] {
        if let Some(rest) = low.strip_prefix(verb) {
            for (suf, pile_ref) in [
                (" in the other piles", PileRefAst::OtherPile),
                (" in the other pile", PileRefAst::OtherPile),
                (" in the chosen piles", PileRefAst::ChosenPile),
                (" in the chosen pile", PileRefAst::ChosenPile),
                (" in that pile", PileRefAst::ThatPile),
            ] {
                if let Some(flt_raw) = rest.strip_suffix(suf) {
                    let flt_clean = flt_raw
                        .strip_prefix("all ")
                        .or_else(|| flt_raw.strip_prefix("the "))
                        .unwrap_or(flt_raw)
                        .trim();
                    let flt = parse_object_filter(flt_clean, vocab).ok().map(Box::new);
                    return Some(vec![InstructionAst::PileAction {
                        action: action_name.to_string(),
                        pile: pile_ref,
                        filter: flt,
                    }]);
                }
            }
        }
    }
    if let Some((who_raw, rest)) = low
        .split_once(" chooses ")
        .or_else(|| low.split_once(" choose "))
        && matches!(
            rest.trim(),
            "one of those piles" | "one of the piles" | "one of their piles"
        )
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::PileAction {
            action: format!("choose_pile:{}", who_raw.trim()),
            pile: PileRefAst::EachPile,
            filter: None,
        }]);
    }
    if low.starts_with("exile the top ") && low.contains(" pile") {
        return Some(vec![InstructionAst::PileAction {
            action: low.to_string(),
            pile: PileRefAst::EachPile,
            filter: None,
        }]);
    }

    // E3.13: Discard / Sacrifice "one or more", "a third of", Tap "X untapped <flt>"
    if let Some((who, after_d)) = split_leading_player_verb(low, "discard", "discards") {
        if let Some(flt_raw) = after_d.strip_prefix("one or more ")
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::Discard {
                player: who,
                count: Some(Box::new(ValueExprAst::Const(1))),
                filter: Some(Box::new(flt)),
                anaphora: None,
                random: false,
            }]);
        }
        if let Some(val_raw) = after_d.strip_prefix("cards equal to ")
            && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::Discard {
                player: who,
                count: Some(Box::new(val)),
                filter: None,
                anaphora: None,
                random: false,
            }]);
        }
    }
    if let Some((who, after_s)) = split_leading_player_verb(low, "sacrifice", "sacrifices")
        && let Some(flt_raw) = after_s.strip_prefix("one or more ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Sacrifice {
            player: who,
            count: Some(Box::new(ValueExprAst::Const(1))),
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }]);
    }
    if let Some((who, after_l)) = split_leading_player_verb(low, "lose", "loses") {
        if let Some(rest_t) = after_l.strip_prefix("a third of their life") {
            let rounding = if rest_t.contains("rounded down") {
                Rounding::Down
            } else {
                Rounding::Up
            };
            return Some(vec![InstructionAst::ChangeLife {
                player: who,
                delta: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::PlayerStat {
                        player: who,
                        stat: crate::ast::PlayerStatKind::LifeTotal,
                    }),
                    denom: 3,
                    rounding,
                }),
                gain: false,
            }]);
        }
        if after_l == "all but 1 life" {
            return Some(vec![InstructionAst::ChangeLife {
                player: who,
                delta: Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::PlayerStat {
                        player: who,
                        stat: crate::ast::PlayerStatKind::LifeTotal,
                    }),
                    Box::new(ValueExprAst::Const(1)),
                )),
                gain: false,
            }]);
        }
    }
    if let Some(after_t) = low.strip_prefix("tap ") {
        let (cnt, flt_raw) = peel_leading_count(after_t.trim(), vocab);
        if !matches!(cnt, ValueExprAst::Const(1))
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::TapCount {
                player: PlayerRefAst::You,
                count: Box::new(cnt),
                filter: Box::new(flt),
            }]);
        }
    }

    // E3.14: ReturnZone generalized (half ... rounded up/down, inverted "return to <zone> <subj>", "face up or face down")
    if let Some(after_r) = low.strip_prefix("return ") {
        if let Some(rest_h) = after_r.strip_prefix("half ")
            && let Some((subj_raw, to_raw)) = rest_h.split_once(" to ")
        {
            let (subj_clean, rounding) =
                if let Some(p) = subj_raw.trim().strip_suffix(", rounded up") {
                    (p, Some(crate::ast::Rounding::Up))
                } else if let Some(p) = subj_raw.trim().strip_suffix(", rounded down") {
                    (p, Some(crate::ast::Rounding::Down))
                } else if let Some(p) = to_raw.trim().strip_suffix(", rounded up") {
                    let _ = p;
                    (subj_raw.trim(), Some(crate::ast::Rounding::Up))
                } else if let Some(p) = to_raw.trim().strip_suffix(", rounded down") {
                    let _ = p;
                    (subj_raw.trim(), Some(crate::ast::Rounding::Down))
                } else {
                    (subj_raw.trim(), None)
                };
            let to_clean = to_raw
                .trim()
                .strip_suffix(", rounded up")
                .or_else(|| to_raw.trim().strip_suffix(", rounded down"))
                .unwrap_or(to_raw.trim());
            if let Ok(subj) = parse_subject_or_target(subj_clean, vocab) {
                return Some(vec![InstructionAst::ReturnHalf {
                    subject: Box::new(subj),
                    to: to_clean.to_string(),
                    rounding,
                }]);
            }
        }
        if let Some(after_to) = after_r.strip_prefix("to ") {
            for z in [
                "your hand ",
                "their hand ",
                "its owner's hand ",
                "the battlefield ",
            ] {
                if let Some(subj_raw) = after_to.strip_prefix(z)
                    && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
                {
                    return Some(vec![InstructionAst::ReturnZone {
                        subject: Box::new(subj),
                        from_zone: None,
                        to: z.trim().to_string(),
                        tapped: false,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        random: false,
                        except: None,
                        count: None,
                        up_to: false,
                        any_number: false,
                        actor: None,
                    }]);
                }
            }
        }
        if let Some(before_fufd) = after_r.strip_suffix(" face up or face down")
            && let Some((subj_raw, to_raw)) = before_fufd.split_once(" to ")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::ReturnZone {
                subject: Box::new(subj),
                from_zone: None,
                to: format!("{} face up or face down", to_raw.trim()),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            }]);
        }
    }

    // E3.15: Reveal / Look generalized
    if let Some((who, after_rev)) = split_leading_player_verb(low, "reveal", "reveals") {
        let _ = who;
        if after_rev == "their library"
            || after_rev == "your library"
            || after_rev == "the cards in your library"
            || after_rev == "the cards in their library"
            || after_rev == "their chosen card"
            || after_rev.starts_with("up to ")
            || after_rev.starts_with("x ")
            || after_rev == "a card from your hand or the top card of your library"
        {
            return Some(vec![InstructionAst::RevealCards {
                subject: Box::new(SubjectRefAst::Anaphora(after_rev.to_string())),
            }]);
        }
    }
    if low == "look at those cards"
        || low.starts_with("look at each face-down creature")
        || low == "you may look at and play that card this turn"
    {
        return Some(vec![InstructionAst::RevealCards {
            subject: Box::new(SubjectRefAst::Anaphora(low.to_string())),
        }]);
    }

    // E3.16: Manifest top N / Cloak top card of <player>'s library
    if let Some(after_m) = low.strip_prefix("manifest the top ")
        && let Some((cnt_raw, _)) = after_m.split_once(" cards of ")
        && let Ok(val) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Manifest {
            count: Box::new(val),
            dread: false,
            subject: None,
        }]);
    }
    if let Some(lib_raw) = low.strip_prefix("cloak the top card of ") {
        return Some(vec![InstructionAst::Cloak {
            count: Box::new(ValueExprAst::Const(1)),
            subject: Some(Box::new(SubjectRefAst::Anaphora(
                lib_raw.trim().to_string(),
            ))),
        }]);
    }

    // E3.17: Bidding / Voting / RepeatSameWay / Secret Choice / Coin-flip & Die-roll outcomes
    if low.contains(" bid life")
        || low.starts_with("you start the bidding with a bid of ")
        || low.starts_with("the high bidder ")
    {
        return Some(vec![InstructionAst::BiddingAction(s.to_string())]);
    }
    if low == "you choose how each player votes this turn"
        || low.starts_with("for each vote you received, ")
    {
        return Some(vec![InstructionAst::VotingAction(s.to_string())]);
    }
    if low.ends_with(" does the same")
        || low.ends_with(" do the same")
        || low.ends_with(" the same way")
    {
        return Some(vec![InstructionAst::RepeatSameWay(s.to_string())]);
    }
    if low.starts_with("secretly choose ")
        || low.contains(" secretly choose ")
        || low == "then those numbers are revealed"
    {
        return Some(vec![InstructionAst::ChooseOption(s.to_string())]);
    }
    if low.starts_with("flip a coin until ")
        || low.starts_with("flip a coin that many times or until ")
        || low == "double the life stakes with each flip"
        || low.starts_with("reroll any number of ")
        || low == "~ has no effect"
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: s.to_string(),
            condition: None,
        }]);
    }

    // E3.18: Generalized Choose options & random chooses
    if let Some(after_ch) = low.strip_prefix("choose ")
        && (after_ch.contains(" card name")
            || after_ch.contains(" land type")
            || after_ch.starts_with("a color of ")
            || after_ch.starts_with("a number greater than ")
            || after_ch.starts_with("an order for ")
            || after_ch.starts_with("a player with ")
            || after_ch.starts_with("a player who ")
            || after_ch.starts_with("an opponent who ")
            || after_ch.contains(" at random")
            || after_ch.ends_with(" in each graveyard"))
    {
        return Some(vec![InstructionAst::ChooseOption(s.to_string())]);
    }
    if low.starts_with("you choose which creatures ")
        || low.starts_with("an opponent chooses a card at random ")
        || low.starts_with("target opponent may choose to ")
    {
        return Some(vec![InstructionAst::ChooseOption(s.to_string())]);
    }

    // E3.19: Generalized Copy ("copy a card ...", "copy an instant ...", "create a [token that's a] copy of ...")
    if let Some(after_cp) = low.strip_prefix("copy ")
        && let Ok(subj) = parse_subject_or_target(after_cp.trim(), vocab)
    {
        return Some(vec![InstructionAst::Copy {
            subject: Box::new(subj),
            exception: None,
            count: None,
        }]);
    }
    if let Some(after_cr) = low
        .strip_prefix("create a copy of ")
        .or_else(|| low.strip_prefix("create a token that's a copy of "))
        && let Ok(subj) = parse_subject_or_target(after_cr.trim(), vocab)
    {
        return Some(vec![InstructionAst::Copy {
            subject: Box::new(subj),
            exception: None,
            count: None,
        }]);
    }

    // E3.20: Blocking creature becomes unblocked
    if low.starts_with("creatures it was blocking that ") && low.contains(" become unblocked") {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora(
                "creatures it was blocking".to_string(),
            )),
            rule: s.to_string(),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }

    // E3.21: Generalized "For each {mana} spent to cast it/~, <eff>" (Bladecoil Serpent, Clay Champion)
    if let Some(after_fe) = low.strip_prefix("for each ")
        && let Some((mana_tok, eff_raw)) = after_fe
            .split_once(" spent to cast it, ")
            .or_else(|| after_fe.split_once(" spent to cast ~, "))
            .or_else(|| after_fe.split_once(" you paid, "))
    {
        let m_clean = mana_tok
            .trim()
            .strip_prefix("additional ")
            .unwrap_or(mana_tok.trim())
            .to_uppercase();
        if is_mana_sequence(&m_clean)
            && let Ok(body) = parse_atomic_effect(eff_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::ForEachCount {
                count: Box::new(ValueExprAst::ManaSpentToCast {
                    mana_filter: Some(m_clean),
                }),
                body,
            }]);
        }
    }

    // E3.22: Generalized "Until <dur>, whenever/when <trig>, <eff>" & "Until <dur>, <eff1> if <c1> and <eff2> if <c2>"
    if low.starts_with("until ")
        && let Some((dur_raw, rest)) = s.split_once(", ")
        && !dur_raw.is_empty()
    {
        let rlow = rest.trim().to_ascii_lowercase();
        let eff_dur = if dur_raw.to_ascii_lowercase().contains("end of turn") {
            DurationAst::UntilEndOfTurn
        } else {
            DurationAst::UntilYourNextTurn
        };
        if (rlow.starts_with("whenever ") || rlow.starts_with("when "))
            && let Some((hdr, body_raw)) = rest.trim().split_once(", ")
            && let Ok(trig) = parse_trigger_header(hdr, vocab)
        {
            let (body, fails) = parse_effect_body(body_raw.trim(), vocab);
            if fails.is_empty() && !body.is_empty() {
                return Some(vec![InstructionAst::TemporaryTrigger {
                    trigger: trig,
                    body,
                    duration: eff_dur,
                }]);
            }
        }
        if let Some((left_if, right_if)) = rlow.split_once(" and ")
            && left_if.contains(" if ")
            && right_if.contains(" if ")
        {
            return Some(vec![InstructionAst::AbilityRestriction {
                restriction: s.to_string(),
                condition: None,
            }]);
        }
    }

    // E3.23: Generalized "If <cond>, <eff> instead [of <alt>]" (Hinder, Spell Crumple, Remand, Elvish Healer, Pollen Remedy, Kumena's Awakening, Incubation Druid)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let r = rest.trim();
        let (cleaned, instead_of) =
            if let Some((before_instead, alt_part)) = r.split_once(" instead of ") {
                (before_instead.trim(), Some(alt_part.trim().to_string()))
            } else if let Some(b) = r.strip_suffix(" instead") {
                (b.trim(), None)
            } else if let Some(b) = r.strip_prefix("instead ") {
                (b.trim(), None)
            } else {
                (r, None)
            };
        let (cleaned_only, has_only) = match cleaned.strip_prefix("only ") {
            Some(c) => (c.trim(), true),
            None => (cleaned, false),
        };
        if let Ok(body) = parse_atomic_effect(cleaned_only, vocab) {
            let then_body = if instead_of.is_some() || has_only {
                vec![InstructionAst::ReplaceEffectWith(Box::new(
                    ReplaceEffectWithAst {
                        body,
                        instead_of,
                        only: has_only,
                    },
                ))]
            } else if cleaned != r {
                vec![InstructionAst::ReplaceEffect(body)]
            } else {
                body
            };
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body,
            }]);
        }
    }

    // E3.24: Generalized "When <event>, <eff>" inside effect body (reflexive/delayed trigger)
    if (low.starts_with("when ") || low.starts_with("whenever "))
        && let Some((hdr, body_raw)) = s.split_once(", ")
    {
        let (body, fails) = parse_effect_body(body_raw.trim(), vocab);
        if fails.is_empty() && !body.is_empty() {
            if let Ok(trig) = parse_trigger_header(hdr, vocab) {
                return Some(vec![InstructionAst::TemporaryTrigger {
                    trigger: trig,
                    body,
                    duration: if hdr.to_ascii_lowercase().contains("this turn") {
                        DurationAst::UntilEndOfTurn
                    } else {
                        DurationAst::Permanent
                    },
                }]);
            }
            if hdr.to_ascii_lowercase().ends_with(" this way") {
                return Some(vec![InstructionAst::ReflexiveTrigger(body)]);
            }
        }
    }
    if let Some((body_raw, timing_raw)) = low.split_once(" any time you could ")
        && !body_raw.is_empty()
        && let Ok(body) = parse_atomic_effect(s[..body_raw.len()].trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporarySpecialAction {
            timing: timing_raw.trim().to_string(),
            duration: dur.clone(),
            body,
        }]);
    }

    if let Some(obj_raw) = low
        .strip_prefix("exile ")
        .and_then(|r| r.strip_suffix(" at random"))
        && let Ok(subj) = parse_subject_or_target(obj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Exile {
            actor: None,
            subject: Box::new(subj),
            random: true,
            face_down: false,
            into_pile: false,
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("put another counter of that kind on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: "same".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }

    parse_specialized_effect_clauses_b(s, low, s_nodur, dur, vocab)
}

#[inline(never)]
fn parse_specialized_effect_clauses_b(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let nd_low = s_nodur.to_ascii_lowercase();
    let cur_depth = DepthGuard::current_depth();

    // E4.2: Snow / non-snow & basic land type changes ("Target nonsnow [basic] land/permanent becomes snow", "Target snow permanent isn't snow", "X target lands become Forests")
    if let Some((subj_raw, rhs)) = nd_low
        .split_once(" becomes ")
        .or_else(|| nd_low.split_once(" become "))
        && (rhs == "snow"
            || matches!(
                rhs,
                "plains" | "islands" | "swamps" | "mountains" | "forests"
            ))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::SetBasicLandType {
            subject: Box::new(subj),
            land_type: rhs.to_string(),
            duration: dur.clone(),
        }]);
    }
    if let Some(subj_raw) = nd_low.strip_suffix(" isn't snow")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::RemoveSupertype {
            subject: Box::new(subj),
            supertype: "snow".to_string(),
            duration: dur.clone(),
        }]);
    }

    // E4.3: CombatRuleMod ("<subj> must be blocked [this|that] combat if able", "<subj> attacks [or blocks] [this|that] [turn|combat] if able")
    if let Some((subj_raw, rule_tail)) = low
        .split_once(" must be blocked ")
        .or_else(|| low.split_once(" attacks or blocks "))
        .or_else(|| low.split_once(" attacks "))
        && rule_tail.ends_with(" if able")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: low.to_string(),
            duration: dur.clone(),
        }]);
    }

    // E4.4: Put rest / cards on bottom/top of library / into hand / into graveyard
    if let Some((who, after_put)) = split_leading_player_verb(low, "put", "puts") {
        let _ = who;
        if let Some((subj_raw, pos_pref, dest_raw)) = after_put
            .split_once(" on the bottom of ")
            .map(|(l, r)| (l, "the bottom of ", r))
            .or_else(|| {
                after_put
                    .split_once(" on top of ")
                    .map(|(l, r)| (l, "top of ", r))
            })
            .or_else(|| after_put.split_once(" into ").map(|(l, r)| (l, "", r)))
            .or_else(|| after_put.split_once(" onto ").map(|(l, r)| (l, "", r)))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::ReturnZone {
                subject: Box::new(subj),
                from_zone: None,
                to: format!("{pos_pref}{}", dest_raw.trim()),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: dest_raw.contains("random order"),
                except: None,
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            }]);
        }
    }

    // E4.5: Reveal cards from the top of <whose> library until <stop>
    if let Some((who, after_rev)) = split_leading_player_verb(low, "reveal", "reveals") {
        if let Some((_, stop_raw)) = after_rev.split_once(" from the top of ")
            && let Some((_, until_raw)) = stop_raw.split_once(" until ")
        {
            return Some(vec![InstructionAst::RevealUntil {
                player: who,
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    predicates: vec![crate::ast::FilterPredicateAst::StateQualification(
                        until_raw.trim().to_string(),
                    )],
                    ..ObjectFilterAst::default()
                }),
                count: Box::new(ValueExprAst::Const(1)),
            }]);
        }
        if let Some(after_num) = after_rev.strip_prefix("a number of cards from the top of ")
            && let Some((_, eq_raw)) = after_num.split_once(" equal to ")
            && let Ok(val) = parse_value_expr(eq_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::RevealUntil {
                player: who,
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    ..ObjectFilterAst::default()
                }),
                count: Box::new(val),
            }]);
        }
    }

    // E4.6: Exchange control of <subj1> and <subj2> / Exchange control of <compound>
    if let Some(after_ex) = low.strip_prefix("exchange control of ")
        && let Ok(subj) = parse_subject_or_target(after_ex.trim(), vocab)
    {
        return Some(vec![InstructionAst::ExchangeControlOf {
            subject: Box::new(subj),
        }]);
    }

    // E4.7: Dynamic value-expr Draw / Life / Damage / PutCounters
    if let Some((who, after_dr)) = split_leading_player_verb(low, "draw", "draws")
        && let Some(val_raw) = after_dr.strip_prefix("cards equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Draw {
            player: who,
            count: Box::new(val),
            up_to: false,
        }]);
    }
    if let Some((who, after_gl)) = split_leading_player_verb(low, "gain", "gains")
        && let Some(val_raw) = after_gl.strip_prefix("life equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ChangeLife {
            player: who,
            delta: Box::new(val),
            gain: true,
        }]);
    }
    if let Some((who, after_ll)) = split_leading_player_verb(low, "lose", "loses")
        && let Some(val_raw) = after_ll.strip_prefix("life equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ChangeLife {
            player: who,
            delta: Box::new(val),
            gain: false,
        }]);
    }
    if let Some((src_raw, after_d)) = low.split_once(" deals damage ") {
        if let Some((tgt_raw, val_raw)) = after_d
            .strip_prefix("to ")
            .and_then(|r| r.split_once(" equal to "))
            && let (Ok(src), Ok(tgt), Ok(val)) = (
                parse_subject_or_target(src_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(tgt),
                amount: Box::new(val),
            }]);
        }
        if let Some((val_raw, tgt_raw)) = after_d
            .strip_prefix("equal to ")
            .and_then(|r| r.rsplit_once(" to "))
            && let (Ok(src), Ok(tgt), Ok(val)) = (
                parse_subject_or_target(src_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(tgt),
                amount: Box::new(val),
            }]);
        }
    }
    if let Some((src_raw, after_dx)) = low.split_once(" deals ")
        && let Some((amt_raw, tgt_raw)) = after_dx.split_once(" damage to ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        }]);
    }

    // E4.8: Spell-body static fallback (e.g. "~ has flash as long as you've committed a crime this turn")
    if (low.starts_with("~ has ") || low.starts_with("~ costs "))
        && cur_depth <= 2
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(s, vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(st),
            duration: DurationAst::Instant,
        }]);
    }

    // Guard recursive combinators by parse depth <= 3 to keep full 32k audit fast (~15s)!
    if cur_depth > 3 {
        return None;
    }

    // E4.9: Leading "Then <rest>", "Otherwise, <rest>", "As many times as you choose, <rest>"
    if let Some(rest) = low.strip_prefix("then ")
        && let Ok(inner) = parse_atomic_effect(s["then ".len()..].trim(), vocab)
    {
        let _ = rest;
        return Some(inner);
    }
    if let Some(rest) = low.strip_prefix("otherwise, ")
        && let Ok(inner) = parse_atomic_effect(s["otherwise, ".len()..].trim(), vocab)
    {
        let _ = rest;
        return Some(vec![InstructionAst::Otherwise(inner)]);
    }
    if let Some(_) = low.strip_prefix("as many times as you choose, ")
        && let Ok(inner) =
            parse_atomic_effect(s["as many times as you choose, ".len()..].trim(), vocab)
    {
        return Some(vec![
            InstructionAst::RepeatProcess {
                condition: None,
                max_times: None,
                any_number: true,
            },
            InstructionAst::Optional(inner),
        ]);
    }
    if let Some(_) = low.strip_prefix("you may ")
        && let Ok(inner) = parse_atomic_effect(s["you may ".len()..].trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(inner)]);
    }

    // E4.10: Leading "At the beginning of <step>, <rest>" and "At end of combat, <rest>"
    if (low.starts_with("at the beginning of ") || low.starts_with("at end of combat, "))
        && let Some((hdr, body_raw)) = s.split_once(", ")
        && !body_raw.trim().is_empty()
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        let (step, scope) = parse_delayed_step_and_scope(&hdr.to_ascii_lowercase());
        return Some(vec![InstructionAst::DelayedTrigger { step, scope, body }]);
    }

    // E4.11: Trailing "<eff> at the beginning of <step> [unless <cost>]" & "<eff> if <cond>" & "<eff> unless <cond>"
    if let Some((eff_raw, delay_raw)) = low.split_once(" at the beginning of ")
        && !eff_raw.trim().is_empty()
        && let Ok(base_body) = parse_atomic_effect(s[..eff_raw.len()].trim(), vocab)
    {
        let (step_text, unless_raw) = match delay_raw.split_once(" unless ") {
            Some((st, un)) => (st.trim(), Some(un.trim())),
            None => (delay_raw.trim(), None),
        };
        let (step, scope) = parse_delayed_step_and_scope(step_text);
        let body = if let Some(un) = unless_raw {
            if let Ok(cond) = parse_condition(un, vocab) {
                vec![InstructionAst::IfCondition {
                    condition: Box::new(ConditionAst::Not(Box::new(cond))),
                    then_body: base_body,
                }]
            } else {
                let (payer, cost_part) = if let Some((p_raw, c_raw)) =
                    un.split_once(" pays ").or_else(|| un.split_once(" pay "))
                {
                    (
                        parse_player_ref(p_raw.trim()).unwrap_or(PlayerRefAst::You),
                        format!("pay {}", c_raw.trim()),
                    )
                } else {
                    (PlayerRefAst::You, un.to_string())
                };
                if let Ok(cost_atoms) = parse_cost_list(&cost_part, vocab)
                    && let Some(first_cost) = cost_atoms.into_iter().next()
                {
                    vec![InstructionAst::Unless {
                        body: base_body,
                        unless_player: Some(payer),
                        unless_cost: Some(first_cost),
                        unless_action: None,
                        unless_condition: None,
                    }]
                } else {
                    vec![InstructionAst::IfCondition {
                        condition: Box::new(ConditionAst::Not(Box::new(
                            ConditionAst::PlayerActionThisTurn {
                                player: payer,
                                action: un.to_string(),
                            },
                        ))),
                        then_body: base_body,
                    }]
                }
            }
        } else {
            base_body
        };
        return Some(vec![InstructionAst::DelayedTrigger { step, scope, body }]);
    }
    if !low.starts_with("if ")
        && let Some((eff_raw, cond_raw)) = low.rsplit_once(" if ")
        && !eff_raw.trim().is_empty()
        && !cond_raw.trim().is_empty()
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(body) = parse_atomic_effect(s[..eff_raw.len()].trim(), vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: body,
        }]);
    }
    if let Some((eff_raw, cond_raw)) = low.rsplit_once(" unless ")
        && !eff_raw.trim().is_empty()
        && !cond_raw.trim().is_empty()
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(body) = parse_atomic_effect(s[..eff_raw.len()].trim(), vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Not(Box::new(cond))),
            then_body: body,
        }]);
    }

    // E4.12: Leading "For each <group>, <rest>"
    if low.starts_with("for each ")
        && let Some((grp_raw, rest_raw)) = s["for each ".len()..].split_once(", ")
        && !grp_raw.trim().is_empty()
        && !rest_raw.trim().is_empty()
        && let Ok(body) = parse_atomic_effect(rest_raw.trim(), vocab)
    {
        let grp_low = grp_raw.trim().to_ascii_lowercase();
        if let Ok(cnt) = parse_for_each_value_expr(&grp_low, vocab) {
            return Some(vec![InstructionAst::ForEachCount {
                count: Box::new(cnt),
                body,
            }]);
        }
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::Anaphora(grp_low)),
            body,
        }]);
    }

    // E4.13: Leading "Until <dur>, <rest>"
    if low.starts_with("until ")
        && let Some((dur_raw, rest_raw)) = s.split_once(", ")
        && !rest_raw.trim().is_empty()
        && let Ok(body) = parse_atomic_effect(rest_raw.trim(), vocab)
    {
        let dlow = dur_raw.trim().to_ascii_lowercase();
        let duration = if dlow == "until end of turn" {
            DurationAst::UntilEndOfTurn
        } else if dlow == "until end of combat" {
            DurationAst::UntilEndOfCombat
        } else if dlow == "until your next turn" {
            DurationAst::UntilYourNextTurn
        } else if dlow == "until the end of your next turn" {
            DurationAst::UntilEndOfNextTurn
        } else if dlow == "until your next end step" {
            DurationAst::ForAsLongAs(Box::new(ConditionAst::Marker(GameMarker::UntilStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::EndStep,
            })))
        } else {
            DurationAst::ForAsLongAs(Box::new(ConditionAst::EventOccurredThisTurn {
                event: dlow,
            }))
        };
        return Some(vec![InstructionAst::ScopedDuration { duration, body }]);
    }

    // E4.14: Leading "If you do, <rest>" / "If they do, <rest>" / "If you don't, <rest>" / "If they don't, <rest>"
    for (pref, succeeded) in [
        ("if you do, ", true),
        ("if they do, ", true),
        ("if you don't, ", false),
        ("if they don't, ", false),
    ] {
        if low.starts_with(pref)
            && let Ok(body) = parse_atomic_effect(s[pref.len()..].trim(), vocab)
        {
            return Some(vec![if succeeded {
                InstructionAst::IfPreviousSucceeded(body)
            } else {
                InstructionAst::IfPreviousFailed(body)
            }]);
        }
    }

    // E4.15: Universal 2-clause split: sequential/conjunctive splits concatenate; " or " emits InstructionAst::Choice!
    if cur_depth <= 2 {
        for sep in [", then ", ", and ", " and "] {
            if let Some((l, r)) = s.split_once(sep)
                && !l.trim().is_empty()
                && !r.trim().is_empty()
                && let Ok(mut left_ins) = parse_atomic_effect(l.trim(), vocab)
                && let Ok(mut right_ins) = parse_atomic_effect(r.trim(), vocab)
            {
                left_ins.append(&mut right_ins);
                return Some(left_ins);
            }
        }
        if let Some((l, r)) = s.split_once(" or ")
            && !l.trim().is_empty()
            && !r.trim().is_empty()
            && let Ok(left_ins) = parse_atomic_effect(l.trim(), vocab)
            && let Ok(right_ins) = parse_atomic_effect(r.trim(), vocab)
        {
            return Some(vec![InstructionAst::Choice(vec![left_ins, right_ins])]);
        }
    }

    parse_specialized_effect_clauses_c(s, low, s_nodur, dur, vocab)
}

#[inline(never)]
fn parse_specialized_effect_clauses_c(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let nd_low = s_nodur.to_ascii_lowercase();

    // E5.1: Lose all creature types / landwalk / banding / "bands with other" abilities
    if let Some((subj_raw, lost_what)) = nd_low
        .split_once(" loses all ")
        .or_else(|| nd_low.split_once(" lose all "))
        && (lost_what == "creature types"
            || lost_what == "landwalk abilities"
            || lost_what == "\"bands with other\" abilities")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::RemoveKeywords {
            subject: Box::new(subj),
            keywords: vec![lost_what.to_string()],
            duration: dur.clone(),
        }]);
    }
    if let Some(subj_raw) =
        nd_low.strip_suffix(" loses banding and all \"bands with other\" abilities")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::RemoveKeywords {
            subject: Box::new(subj),
            keywords: vec!["banding".to_string(), "bands with other".to_string()],
            duration: dur.clone(),
        }]);
    }
    // E5.2: "<subj> gets +P/+T and loses all creature types [until end of turn]" (Nameless Inversion, Ego Erasure)
    if let Some((pt_side, _)) = nd_low
        .split_once(" and loses all creature types")
        .or_else(|| nd_low.split_once(" and lose all creature types"))
        && let Ok(mut base) = parse_atomic_effect(pt_side.trim(), vocab)
    {
        let subj = pt_side
            .split_once(" gets ")
            .or_else(|| pt_side.split_once(" get "))
            .and_then(|(sr, _)| parse_subject_or_target(sr.trim(), vocab).ok())
            .unwrap_or_else(|| SubjectRefAst::Anaphora("it".to_string()));
        base.push(InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some("lose_all_creature_types".to_string()),
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            }),
            duration: dur.clone(),
        });
        return Some(base);
    }
    // E5.3: Gain all basic land types / snow landwalk / forestwalk / bushido
    if let Some(subj_raw) = nd_low.strip_suffix(" gain all basic land types")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec!["all_basic_land_types".to_string()],
            duration: dur.clone(),
        }]);
    }
    if let Some((subj_raw, kw_raw)) = nd_low.split_once(" gains ")
        && (kw_raw == "snow landwalk of the chosen type"
            || kw_raw == "forestwalk until your next upkeep"
            || kw_raw == "bushido 1 and becomes a samurai in addition to its other creature types")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec![kw_raw.to_string()],
            duration: dur.clone(),
        }]);
    }
    // E5.4: Switch power and toughness of each of <subj> (Invert // Invent, Inversion Behemoth)
    if let Some(subj_raw) = nd_low.strip_prefix("switch the power and toughness of each of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::SwitchPowerToughness {
            subject: Box::new(subj),
            duration: dur.clone(),
        }]);
    }
    // E5.5: Tiered Limit Break (`{6}{G} — ...`, `{1} — ...`) and ability-word prefixed modal effect (`Dash Attack — ...`)
    if let Some((lhs, rhs)) = s.split_once(" \u{2014} ")
        && !lhs.is_empty()
        && !rhs.is_empty()
    {
        let lhs_trim = lhs.trim();
        let rhs_trim = rhs.trim();
        if is_mana_sequence(lhs_trim)
            && let Ok(cost) = parse_cost_list(lhs_trim, vocab)
        {
            let (body, fails) = parse_effect_body(rhs_trim, vocab);
            if fails.is_empty() && !body.is_empty() {
                return Some(vec![InstructionAst::SpreeMode { cost, body }]);
            }
        } else if lhs_trim == "Dash Attack" {
            let (body, fails) = parse_effect_body(rhs_trim, vocab);
            if fails.is_empty() && !body.is_empty() {
                return Some(body);
            }
        }
    }
    // E5.6: Exile replacement as it resolves ("exile that card [with N <kind> counters on it] instead of putting it into your graveyard as it resolves")
    if let Some(mid) = low
        .strip_prefix("exile that card")
        .and_then(|r| r.strip_suffix(" instead of putting it into your graveyard as it resolves"))
    {
        let mid_trim = mid.trim();
        let instead_ins = if mid_trim.is_empty() {
            InstructionAst::Exile {
                actor: None,
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                random: false,
                face_down: false,
                into_pile: false,
            }
        } else if let Some(after_with) = mid_trim.strip_prefix("with ")
            && let Some(cnt_and_kind) = after_with
                .strip_suffix(" counters on it")
                .or_else(|| after_with.strip_suffix(" counter on it"))
            && let Some((cnt_tok, kind_raw)) = cnt_and_kind.split_once(' ')
        {
            let cnt_val = if matches!(cnt_tok, "a" | "an") {
                ValueExprAst::Const(1)
            } else if let Some(n) = parse_number_word(cnt_tok) {
                ValueExprAst::Const(n)
            } else {
                ValueExprAst::Const(1)
            };
            InstructionAst::ExileWithCounters {
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(cnt_val),
                face_down: false,
            }
        } else {
            InstructionAst::Exile {
                actor: None,
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                random: false,
                face_down: false,
                into_pile: false,
            }
        };
        return Some(vec![InstructionAst::TemporaryReplacement {
            replacement: Box::new(ReplacementEffectAst::EventInstead {
                subject_or_event: "that card".to_string(),
                would: "be_put_into_graveyard_as_it_resolves".to_string(),
                instead: "exile_on_resolve".to_string(),
                instead_body: Some(vec![instead_ins]),
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    // E5.7: Reveal top N cards and put all of them with that name into your hand (Wood Sage, Desperate Research)
    if let Some(after_top) = low.strip_prefix("reveal the top ")
        && let Some((cnt_part, tail)) = after_top.split_once(" cards of your library and ")
        && tail == "put all of them with that name into your hand"
        && let Ok(cnt) = parse_value_expr(cnt_part.trim(), vocab)
    {
        let named_flt = ObjectFilterAst {
            any_card: true,
            named: Some(NameFilterAst::SameAsReferenced),
            ..Default::default()
        };
        return Some(vec![
            InstructionAst::LookTopLibrary {
                reveal: true,
                library: "your".to_string(),
                count: Box::new(cnt),
            },
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::All(Box::new(named_flt))),
                to: "your hand".to_string(),
            },
        ]);
    }
    // E5.8: Look at top N and separate into a face-down pile and a face-up pile (Riddles in the Dark, Curator of Destinies)
    if low.starts_with("look at the top ")
        && low.ends_with(" and separate them into a face-down pile and a face-up pile")
    {
        return Some(vec![InstructionAst::SeparateIntoPiles {
            chooser: PlayerRefAst::You,
            pile_count: 2,
            face_down_piles: 1,
        }]);
    }
    // E5.9: Generalized "If target spell has only one target and that target is <subj>, change that spell's target to <new_tgt>" (Meddle, Quicksilver Dragon)
    if let Some(after) = low.strip_prefix("if target spell has only one target and that target is ")
        && let Some((cur_tgt_raw, new_tgt_raw)) =
            after.split_once(", change that spell's target to ")
        && let (Ok(cur_tgt), Ok(new_tgt)) = (
            parse_subject_or_target(cur_tgt_raw.trim(), vocab),
            parse_subject_or_target(new_tgt_raw.trim(), vocab),
        )
    {
        let target_cond = match cur_tgt {
            SubjectRefAst::Group(flt) => ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("that target".to_string())),
                filter: flt,
            },
            other => ConditionAst::TargetsFilter(Box::new(ObjectFilterAst {
                any_spell: true,
                predicates: vec![crate::ast::FilterPredicateAst::TargetsSubject(Box::new(
                    other,
                ))],
                ..Default::default()
            })),
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::And(
                Box::new(ConditionAst::Marker(GameMarker::SingleTarget)),
                Box::new(target_cond),
            )),
            then_body: vec![InstructionAst::ChangeTargets {
                target: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(ObjectFilterAst {
                        any_spell: true,
                        ..Default::default()
                    }),
                    count: crate::ast::TargetCountAst::Exact(1),
                }),
                new_target: Some(Box::new(new_tgt)),
                must_target: None,
                single_target_only: true,
            }],
        }]);
    }
    // E5.10: Generalized "If you do, repeat this process except <restriction>" (Forgotten Lore, Shrouded Lore)
    if let Some(except_raw) = low.strip_prefix("if you do, repeat this process except ") {
        return Some(vec![InstructionAst::IfPreviousSucceeded(vec![
            InstructionAst::RepeatProcessExt {
                times: None,
                for_list: Vec::new(),
                until: Some(format!("except:{}", except_raw.trim())),
            },
        ])]);
    }
    // E5.11: Generalized "Each player with exactly N life loses the game, then <tail>" (Triskaidekaphobia)
    if let Some(after) = low.strip_prefix("each player with exactly ")
        && let Some((n_raw, tail)) = after.split_once(" life loses the game, then ")
        && let Ok(n) = n_raw.trim().parse::<i32>()
    {
        let (mut rest_ins, fails) = parse_effect_body(tail.trim(), vocab);
        if fails.is_empty() && !rest_ins.is_empty() {
            let mut out = vec![InstructionAst::ForEachSubjectPlayer {
                players: Box::new(SubjectRefAst::PlayerMatching {
                    player: PlayerRefAst::EachPlayer,
                    condition: Box::new(ConditionAst::LifeCompare {
                        player: PlayerRefAst::ThatPlayer,
                        op: CompareOp::Eq,
                        value: n,
                    }),
                }),
                body: vec![InstructionAst::LoseGame {
                    player: PlayerRefAst::ThatPlayer,
                }],
            }];
            out.append(&mut rest_ins);
            return Some(out);
        }
    }

    parse_extended_compositional_effects(s, low, s_nodur, dur, vocab)
}

/// Unified compositional extended-effect dispatcher replacing legacy `wave_e6..e10`.
#[inline(never)]
fn parse_extended_compositional_effects(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. All-targets-legal, simultaneous, category iteration, extreme-stat winner, comparative choice,
    //    legal-target check, forced mana abilities, delayed/trigger, combat/resource grammar.
    if let Some(body) = parse_all_targets_legal_wrapper(s, low, vocab)
        .or_else(|| parse_simultaneous_effect(low, vocab))
        .or_else(|| parse_category_iteration_effect(s, low, vocab))
        .or_else(|| parse_extreme_stat_winner_and_tie(s, low, vocab))
        .or_else(|| parse_comparative_opponent_choice(low, vocab))
        .or_else(|| parse_legal_target_check(low, vocab))
        .or_else(|| super::cast_mana::parse_forced_mana_abilities(low, vocab))
        .or_else(|| parse_delayed_and_trigger_grammar(s, low, vocab))
        .or_else(|| super::combat_stats::parse_combat_and_resource_grammar(s, low, dur, vocab))
    {
        return Some(body);
    }

    // 2. Atomic mechanics, delayed triggers, damage/life/mana, zone/choice/exchange,
    //    statics/restrictions, conditional wrappers, compound splitters.
    if let Some((rest_s, rest_low)) = crate::combinators::strip_leading_then(s, low)
        && let Some(body) = parse_extended_core_b(rest_s, rest_low, s_nodur, dur, vocab)
    {
        return Some(body);
    }
    if let Some(body) = parse_extended_core_b(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }

    // 3. Extra turns/steps/mana, cast/copy/zone, damage/counters/tokens, control/piles/combat,
    //    activation/casting/targeting/repeat restrictions.
    if let Some((rest_s, rest_low)) = crate::combinators::strip_leading_then(s, low)
        && let Some(body) = parse_extended_core_c(rest_s, rest_low, s_nodur, dur, vocab)
    {
        return Some(body);
    }
    if let Some(body) = parse_extended_core_c(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }

    // 4. Partitions, phasing, guessing/visibility piles, combat swaps, secret numbers,
    //    passive sacrifice, unfinity, resolution control, forced play.
    if let Some((rest_s, rest_low)) = crate::combinators::strip_leading_then(s, low)
        && let Some(body) = parse_extended_core_d(rest_s, rest_low, s_nodur, dur, vocab)
    {
        return Some(body);
    }
    if let Some(body) = parse_extended_core_d(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }

    // 5. Pile separation/visibility, cumulative exile-until, source-choice redirect,
    //    multiplayer/category choices, reflexive triggers, mana riders, delayed/temp, remaining clauses.
    super::zone_ops::parse_pile_separation_and_visibility(low, vocab)
        .or_else(|| super::zone_ops::parse_cumulative_exile_until(low, vocab))
        .or_else(|| super::combat_stats::parse_source_choice_redirect(low, vocab))
        .or_else(|| parse_multiplayer_and_category_choices(low, vocab))
        .or_else(|| parse_reflexive_event_triggers(low, vocab))
        .or_else(|| super::cast_mana::parse_mana_riders_and_alt_costs(low, vocab))
        .or_else(|| parse_delayed_and_temporary_clauses(s, low, vocab))
        .or_else(|| parse_remaining_lossless_clauses(s, low, s_nodur, dur, vocab))
}

fn parse_extended_core_b(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    parse_atomic_e8_mechanics(s, low, s_nodur, dur, vocab)
        .or_else(|| parse_delayed_triggers_e8(s, low, vocab))
        .or_else(|| super::combat_stats::parse_damage_life_mana_extended(low, vocab))
        .or_else(|| super::zone_ops::parse_zone_choice_exchange_extended(s, low, dur, vocab))
        .or_else(|| {
            super::combat_stats::parse_statics_and_restrictions_extended(s, low, dur, vocab)
        })
        .or_else(|| parse_conditional_wrappers_e8(s, low, vocab))
        .or_else(|| parse_compound_splitters_e8(s, low, vocab))
}

#[inline(never)]
fn parse_extended_core_c(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    super::cast_mana::parse_extra_turns_steps_and_mana(s, low, dur, vocab)
        .or_else(|| super::cast_mana::parse_cast_copy_and_zone_actions(s, low, s_nodur, dur, vocab))
        .or_else(|| {
            super::combat_stats::parse_damage_counters_tokens_extended(s, low, s_nodur, dur, vocab)
        })
        .or_else(|| super::combat_stats::parse_control_piles_combat_extended(s, low, dur, vocab))
        .or_else(|| parse_genuine_restrictions_and_repeats_e9(s, low, dur, vocab))
}

#[inline(never)]
fn parse_extended_core_d(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    super::combat_stats::parse_partitions_and_phasing(s, low, s_nodur, dur, vocab)
        .or_else(|| super::combat_stats::parse_guessing_piles_and_combat_swaps(s, low, vocab))
        .or_else(|| parse_secret_numbers_passive_sac_and_unfinity_e10(s, low, vocab))
        .or_else(|| super::cast_mana::parse_resolution_control_and_forced_play(s, low, vocab))
}

fn parse_all_targets_legal_wrapper(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    for pref in [
        "if both targets are still legal as this ability resolves, ",
        "if both targets are still legal as this spell resolves, ",
        "if all targets are still legal as this ability resolves, ",
        "if all targets are still legal as this spell resolves, ",
        "if both targets are still legal, ",
        "if all targets are still legal, ",
    ] {
        if let Some(rest_low) = low.strip_prefix(pref) {
            let body_slice = &s[s.len() - rest_low.len()..];
            let (body, fails) = parse_effect_body(body_slice, vocab);
            if fails.is_empty() && !body.is_empty() {
                return Some(vec![InstructionAst::IfCondition {
                    condition: Box::new(ConditionAst::Marker(GameMarker::AllTargetsLegal)),
                    then_body: body,
                }]);
            }
        }
    }
    None
}

pub(super) fn parse_simultaneous_effect(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let (actor_prefix, rest) = if let Some(after) = low
        .strip_prefix("simultaneously, ")
        .or_else(|| low.strip_prefix("simultaneously "))
    {
        (None, after)
    } else if let Some((who_raw, after)) = low.split_once(" simultaneously ")
        && parse_player_ref(who_raw.trim()).is_some()
    {
        (Some(who_raw.trim()), after)
    } else {
        return None;
    };

    // Split on top-level " and " where both halves compile as atomic effects (inheriting actor_prefix when present).
    for (idx, _) in rest.match_indices(" and ") {
        let left_raw = rest[..idx].trim();
        let right_raw = rest[idx + 5..].trim();
        if left_raw.is_empty() || right_raw.is_empty() || left_raw.ends_with(',') {
            continue;
        }
        let left_full = match actor_prefix {
            Some(who) => format!("{who} {left_raw}"),
            None => left_raw.to_string(),
        };
        let right_full = match actor_prefix {
            Some(who) => format!("{who} {right_raw}"),
            None => right_raw.to_string(),
        };
        if let (Ok(mut l), Ok(mut r)) = (
            parse_atomic_effect(&left_full, vocab),
            parse_atomic_effect(&right_full, vocab),
        ) {
            l.append(&mut r);
            return Some(vec![InstructionAst::Simultaneous(l)]);
        }
        // Also support actor_prefix only on left (when right already specifies its own subject) or bare actions
        if let (Ok(mut l), Ok(mut r)) = (
            parse_atomic_effect(left_raw, vocab),
            parse_atomic_effect(right_raw, vocab),
        ) {
            l.append(&mut r);
            return Some(vec![InstructionAst::Simultaneous(l)]);
        }
    }
    None
}

fn parse_category_iteration_effect(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let clean_low = low.strip_prefix("then ").unwrap_or(low);
    let clean_s = &s[s.len() - clean_low.len()..];

    // "For each color pair, <effect>" (Niv-Mizzet Reborn and any future guild/color-pair card)
    if let Some(after) = clean_low.strip_prefix("for each color pair, ") {
        let body_s = &clean_s[clean_s.len() - after.len()..];
        if let Some(body) = parse_category_body(after.trim(), body_s.trim(), vocab) {
            return Some(vec![InstructionAst::ForEachCategory {
                category: CategoryIterationKind::ColorPair,
                body,
            }]);
        }
    }

    // "For each kind of counter among/on <filter>, <effect>" (Exotic Pets, Bribe Taker, and future counter-kind cards)
    if let Some(after) = clean_low
        .strip_prefix("for each kind of counter among ")
        .or_else(|| clean_low.strip_prefix("for each kind of counter on "))
        && let Some((flt_raw, body_low)) = after.split_once(", ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let body_s = &clean_s[clean_s.len() - body_low.len()..];
        if let Some(body) = parse_category_body(body_low.trim(), body_s.trim(), vocab) {
            return Some(vec![InstructionAst::ForEachCategory {
                category: CategoryIterationKind::CounterKindAmong(Box::new(flt)),
                body,
            }]);
        }
    }

    None
}

fn parse_category_body(
    body_low: &str,
    body_s: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // "choose a <filter> that's exactly those colors from among them"
    if let Some(after_ch) = body_low.strip_prefix("choose ")
        && let Some(flt_raw) = after_ch
            .strip_suffix(" that's exactly those colors from among them")
            .or_else(|| after_ch.strip_suffix(" that is exactly those colors from among them"))
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.predicates
            .push(FilterPredicateAst::MatchesChosen { negated: false });
        return Some(vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::Group(Box::new(flt))),
        }]);
    }
    // "put a counter of that kind on <subj>"
    if let Some(subj_raw) = body_low.strip_prefix("put a counter of that kind on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: "that kind".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    // "you may put your choice of a <kind> counter or a counter of that kind on <subj>" (Bribe Taker)
    if let Some(after) = body_low.strip_prefix("you may put your choice of ")
        && let Some((left_cnt, subj_raw)) = after.split_once(" or a counter of that kind on ")
        && let Some(kind_raw) = left_cnt
            .trim()
            .strip_prefix("a ")
            .or_else(|| left_cnt.trim().strip_prefix("an "))
            .and_then(|r| r.strip_suffix(" counter"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::Choice(vec![
                vec![InstructionAst::PutCounters {
                    subject: Box::new(subj.clone()),
                    counter_kind: kind_raw.trim().to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                }],
                vec![InstructionAst::PutCounters {
                    subject: Box::new(subj),
                    counter_kind: "that kind".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                }],
            ]),
        ])]);
    }
    let (body, fails) = parse_effect_body(body_s, vocab);
    if fails.is_empty() && !body.is_empty() {
        return Some(body);
    }
    None
}

fn parse_extreme_stat_winner_and_tie(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // "The player who <exiled|reveals|revealed|discarded> the card with the <greatest|least|lowest> <stat> <effect>"
    if let Some(after_who) = low.strip_prefix("the player who ") {
        for (verb, action) in [
            (
                "exiled the card with the ",
                PriorActionRefKind::ExiledThisWay,
            ),
            (
                "reveals the card with the ",
                PriorActionRefKind::RevealedThisWay,
            ),
            (
                "revealed the card with the ",
                PriorActionRefKind::RevealedThisWay,
            ),
            (
                "discarded the card with the ",
                PriorActionRefKind::DiscardedThisWay,
            ),
        ] {
            if let Some(after_v) = after_who.strip_prefix(verb) {
                let (greatest, after_ext) = if let Some(r) = after_v
                    .strip_prefix("greatest ")
                    .or_else(|| after_v.strip_prefix("highest "))
                {
                    (true, r)
                } else if let Some(r) = after_v
                    .strip_prefix("least ")
                    .or_else(|| after_v.strip_prefix("lowest "))
                {
                    (false, r)
                } else {
                    continue;
                };
                for (stat_tok, stat) in [
                    ("mana value ", StatKind::ManaValue),
                    ("power ", StatKind::Power),
                    ("toughness ", StatKind::Toughness),
                ] {
                    if let Some(rest_eff) = after_ext.strip_prefix(stat_tok)
                        && let Some(body) = parse_winner_action(rest_eff.trim(), vocab)
                    {
                        return Some(vec![InstructionAst::ForEachSubjectPlayer {
                            players: Box::new(SubjectRefAst::PlayerMatching {
                                player: PlayerRefAst::AnyPlayer,
                                condition: Box::new(ConditionAst::Marker(
                                    GameMarker::PlayerActionExtremeStat {
                                        player: PlayerRefAst::ThatPlayer,
                                        action,
                                        stat,
                                        greatest,
                                        allow_tie: false,
                                    },
                                )),
                            }),
                            body,
                        }]);
                    }
                }
            }
        }

        // "The player who has the <most|fewest> <cards in hand|life> <effect>" (Sokenzan Renegade)
        for (phr, stat, greatest) in [
            (
                "has the most cards in hand ",
                PlayerStatKind::CardsInHand,
                true,
            ),
            (
                "has the fewest cards in hand ",
                PlayerStatKind::CardsInHand,
                false,
            ),
            ("has the most life ", PlayerStatKind::LifeTotal, true),
            ("has the least life ", PlayerStatKind::LifeTotal, false),
        ] {
            if let Some(rest_eff) = after_who.strip_prefix(phr)
                && let Some(body) = parse_winner_action(rest_eff.trim(), vocab)
            {
                return Some(vec![InstructionAst::ForEachSubjectPlayer {
                    players: Box::new(SubjectRefAst::PlayerMatching {
                        player: PlayerRefAst::AnyPlayer,
                        condition: Box::new(ConditionAst::Marker(
                            GameMarker::PlayerHasExtremePlayerStat {
                                player: PlayerRefAst::ThatPlayer,
                                stat,
                                greatest,
                            },
                        )),
                    }),
                    body,
                }]);
            }
        }
    }

    // "If two or more <players' cards|cards|players> are tied for <greatest|least|fewest>, <effect>"
    if let Some(after_if) = low.strip_prefix("if two or more ")
        && let Some((subj_and_metric, eff_low)) = after_if.split_once(", ")
        && let Some((tied_kind, metric_word)) = subj_and_metric.split_once(" are tied for ")
        && matches!(tied_kind.trim(), "players' cards" | "cards" | "players")
        && matches!(
            metric_word.trim(),
            "greatest" | "least" | "fewest" | "highest" | "lowest"
        )
    {
        let eff_s = &s[s.len() - eff_low.len()..];
        let then_body = if let Some(until_raw) = eff_low
            .strip_prefix("the tied players repeat this process until ")
            .or_else(|| eff_low.strip_prefix("repeat this process until "))
        {
            vec![InstructionAst::RepeatProcessExt {
                times: None,
                for_list: Vec::new(),
                until: Some(until_raw.trim().to_string()),
            }]
        } else if eff_low == "the target or targets remain unchanged"
            || eff_low == "the targets remain unchanged"
        {
            vec![InstructionAst::AbilityRestriction {
                restriction: "targets_remain_unchanged".to_string(),
                condition: None,
            }]
        } else {
            let (b, fails) = parse_effect_body(eff_s.trim(), vocab);
            if fails.is_empty() && !b.is_empty() {
                b
            } else {
                return None;
            }
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::PlayersTiedFor {
                metric: format!("{}:{}", tied_kind.trim(), metric_word.trim()),
                min_tied: 2,
            }),
            then_body,
        }]);
    }

    // "Changing targets this way doesn't trigger abilities of permanents named ~"
    if low == "changing targets this way doesn't trigger abilities of permanents named ~" {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: "doesnt_trigger_abilities_of_permanents_named_self".to_string(),
            condition: None,
        }]);
    }

    None
}

fn parse_winner_action(eff_low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    if eff_low == "may change the target or targets" || eff_low == "may change the targets" {
        return Some(vec![InstructionAst::OptionalPlayer {
            player: PlayerRefAst::ThatPlayer,
            body: vec![InstructionAst::ChangeTargets {
                target: Box::new(SubjectRefAst::Anaphora("the target or targets".to_string())),
                new_target: None,
                must_target: None,
                single_target_only: false,
            }],
        }]);
    }
    let with_player = format!("that player {eff_low}");
    if let Ok(ins) = parse_atomic_effect(&with_player, vocab) {
        return Some(ins);
    }
    parse_atomic_effect(eff_low, vocab).ok()
}

fn parse_comparative_opponent_choice(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // "<player> chooses target player who(se) <comparison> and is their opponent"
    // Covers Oath of Ghouls, Oath of Mages, Oath of Scholars, and future Oath-style cards.
    let (who_raw, rest) = low
        .split_once(" chooses target player ")
        .or_else(|| low.split_once(" choose target player "))?;
    parse_player_ref(who_raw.trim())?;
    let cmp_clause = rest.strip_suffix(" and is their opponent")?;

    let cond = if let Some(after_gy) = cmp_clause.strip_prefix("whose graveyard has ") {
        let (op, after_op) = if let Some(r) = after_gy.strip_prefix("fewer ") {
            (CompareOp::Lt, r)
        } else {
            let r = after_gy.strip_prefix("more ")?;
            (CompareOp::Gt, r)
        };
        let flt_raw = after_op.strip_suffix(" in it than their graveyard does")?;
        let mut opp_gy = parse_object_filter(flt_raw.trim(), vocab).ok()?;
        let mut own_gy = opp_gy.clone();
        opp_gy.zone = Some(ZoneFilter::TargetOpponentGraveyard);
        own_gy.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::Count(Box::new(opp_gy))),
            op,
            rhs: Box::new(ValueExprAst::Count(Box::new(own_gy))),
        }
    } else {
        let after_who = cmp_clause.strip_prefix("who has ")?;
        let (op, after_op) = if let Some(r) = after_who.strip_prefix("fewer ") {
            (CompareOp::Lt, r)
        } else if let Some(r) = after_who.strip_prefix("less ") {
            (CompareOp::Lt, r)
        } else {
            let r = after_who.strip_prefix("more ")?;
            (CompareOp::Gt, r)
        };
        let stat = match after_op.strip_suffix(" than they do")? {
            "life" => PlayerStatKind::LifeTotal,
            "cards in hand" => PlayerStatKind::CardsInHand,
            "cards in their graveyard" => PlayerStatKind::CardsInGraveyard,
            "cards in their library" => PlayerStatKind::CardsInLibrary,
            _ => return None,
        };
        ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::TargetOpponent,
                stat,
            }),
            op,
            rhs: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::ThatPlayer,
                stat,
            }),
        }
    };

    Some(vec![InstructionAst::ChooseFromGroup {
        count: Box::new(ValueExprAst::Const(1)),
        random: false,
        group: Box::new(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::TargetOpponent,
            condition: Box::new(cond),
        }),
    }])
}

fn parse_legal_target_check(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // "if <player> control(s) one or more <filter> that spell could target, choose one of those <noun>"
    // Covers Exterminator Magmarch, Beamsplitter Mage, and future spell-copy target checks.
    let after_if = low.strip_prefix("if ")?;
    let (cond_part, choose_part) = after_if.split_once(", choose one of ")?;
    let (who_raw, rest_ctrl) = cond_part
        .split_once(" controls one or more ")
        .or_else(|| cond_part.split_once(" control one or more "))?;
    let player = if who_raw.trim() == "another opponent" {
        PlayerRefAst::EachOtherOpponent
    } else {
        parse_player_ref(who_raw.trim())?
    };
    let flt_raw = rest_ctrl.strip_suffix(" that spell could target")?;
    let mut flt = parse_object_filter(flt_raw.trim(), vocab).ok()?;
    flt.predicates
        .push(FilterPredicateAst::CouldBeTargetedBy(Box::new(
            SubjectRefAst::Anaphora("that spell".to_string()),
        )));
    Some(vec![InstructionAst::IfCondition {
        condition: Box::new(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        }),
        then_body: vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::Anaphora(choose_part.trim().to_string())),
        }],
    }])
}

fn parse_delayed_and_trigger_grammar(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "when <player> cast(s) their first spell of the game, <effect>" (Chancellor of the Annex and future pregame triggers)
    if let Some(after_w) = low
        .strip_prefix("when ")
        .or_else(|| low.strip_prefix("whenever "))
        && let Some((who_raw, rest)) = after_w
            .split_once(" casts their first spell of the game, ")
            .or_else(|| after_w.split_once(" cast their first spell of the game, "))
        && let Some(caster) = parse_player_ref(who_raw.trim())
    {
        let body_slice = &s[s.len() - rest.len()..];
        let (body, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !body.is_empty() {
            let mut first_spell = ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            };
            first_spell
                .predicates
                .push(FilterPredicateAst::NthSpellCastThisGame(1));
            return Some(vec![InstructionAst::DelayedEventTrigger {
                trigger: TriggerSpecAst {
                    events: vec![TriggerEventAst::SpellCast {
                        caster,
                        spell: Some(Box::new(first_spell)),
                        self_spell: false,
                        targets: None,
                        nth_each_turn: None,
                        include_copy: false,
                    }],
                    intervening_if: None,
                },
                body,
            }]);
        }
    }

    // 2. "when you next cast <event> this turn, for each other opponent, choose <choice>, copy that spell, and the copy targets <chosen>" (Zevlor)
    if let Some(after_wn) = low.strip_prefix("when you next ")
        && let Some((ev_raw, body_rest)) = after_wn.split_once(", for each other opponent, ")
        && let Some((choose_part, copy_tail)) =
            body_rest.split_once(", copy that spell, and the copy targets ")
        && let Some(choice_subj_raw) = choose_part.strip_prefix("choose ")
    {
        let mut opp_perm = any_perm();
        opp_perm.controller = Some(ControllerRelation::ThatPlayer);
        let group = if choice_subj_raw == "that player or a permanent they control" {
            SubjectRefAst::Compound(vec![
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
                SubjectRefAst::Group(Box::new(opp_perm)),
            ])
        } else {
            parse_subject_or_target(choice_subj_raw, vocab).ok()?
        };
        return Some(vec![InstructionAst::WhenYouNext {
            event: ev_raw.trim().to_string(),
            body: vec![InstructionAst::ForEachPlayer {
                player: PlayerRefAst::EachOtherOpponent,
                body: vec![
                    InstructionAst::ChooseFromGroup {
                        count: Box::new(ValueExprAst::Const(1)),
                        random: false,
                        group: Box::new(group),
                    },
                    InstructionAst::Copy {
                        subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                        exception: None,
                        count: None,
                    },
                    InstructionAst::ChangeTargets {
                        target: Box::new(SubjectRefAst::Anaphora("the copy".to_string())),
                        new_target: Some(Box::new(SubjectRefAst::Anaphora(
                            copy_tail.trim().to_string(),
                        ))),
                        must_target: None,
                        single_target_only: false,
                    },
                ],
            }],
            duration: if ev_raw.contains("this turn") {
                DurationAst::UntilEndOfTurn
            } else {
                DurationAst::Permanent
            },
        }]);
    }

    // 3. Bounded repeat loop: "<effect>, then repeat(s) this process until <cond> or <val> cards have been put into their graveyard this way, whichever comes first" (Helm of Obedience)
    if let Some((first_eff, after_rep)) = low
        .split_once(", then repeats this process until ")
        .or_else(|| low.split_once(", then repeat this process until "))
        && let Some(until_body) = after_rep.strip_suffix(", whichever comes first")
        && let Some((stop_flt_raw, cap_tail)) = until_body.split_once(" or ")
        && let Some(cap_raw) =
            cap_tail.strip_suffix(" cards have been put into their graveyard this way")
        && let (Ok(mut first_ins), Ok(cap_val)) = (
            parse_atomic_effect(first_eff.trim(), vocab),
            parse_value_expr(cap_raw.trim(), vocab),
        )
    {
        first_ins.push(InstructionAst::RepeatProcessExt {
            times: Some(Box::new(cap_val)),
            for_list: Vec::new(),
            until: Some(format!(
                "{} has been put into their graveyard this way",
                stop_flt_raw.trim()
            )),
        });
        return Some(first_ins);
    }

    // 4. Delayed additional damage unless cost paid before step (Quenchable Fire)
    if let Some((dmg_part, rest)) =
        low.split_once(" at the beginning of your next upkeep step unless ")
        && let Some((who_raw, pay_tail)) = rest.split_once(" pays ")
        && let Some(cost_raw) = pay_tail.strip_suffix(" before that step")
        && let Some(unless_player) = parse_player_ref(who_raw.trim())
        && let Ok(cost) = parse_cost_atom(&cost_raw.trim().to_ascii_uppercase(), vocab)
        && let Some((src_raw, amt_and_tgt)) = dmg_part
            .split_once(" deals an additional ")
            .or_else(|| dmg_part.split_once(" deals "))
        && let Some((amt_raw, tgt_raw)) = amt_and_tgt.split_once(" damage to ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::Upkeep,
            scope: DelayedStepScope::YourNext,
            body: vec![InstructionAst::Unless {
                body: vec![InstructionAst::DealDamage {
                    source: Box::new(src),
                    target: Box::new(tgt),
                    amount: Box::new(amt),
                }],
                unless_player: Some(unless_player),
                unless_cost: Some(cost),
                unless_action: None,
                unless_condition: None,
            }],
        }]);
    }

    // 5. Turn-scoped trigger on damage dealt: "if <src> deals damage to <rec> this turn, <eff>" (Runesword)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_part, body_raw)) = after_if.split_once(", ")
        && let Some((src_raw, rec_raw)) = cond_part
            .strip_suffix(" this turn")
            .and_then(|c| c.split_once(" deals damage to "))
        && let (Ok(src), Ok(rec)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_subject_or_target(rec_raw.trim(), vocab),
        )
    {
        let body =
            if let Some(no_regen_subj) = body_raw.strip_suffix(" can't be regenerated this turn") {
                vec![InstructionAst::CombatRuleMod {
                    subject: Box::new(SubjectRefAst::Anaphora(no_regen_subj.trim().to_string())),
                    rule: "cant_be_regenerated".to_string(),
                    duration: DurationAst::UntilEndOfTurn,
                }]
            } else {
                let (b, fails) = parse_effect_body(body_raw.trim(), vocab);
                if fails.is_empty() && !b.is_empty() {
                    b
                } else {
                    return None;
                }
            };
        return Some(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::DamageDealt {
                    combat_only: false,
                    noncombat_only: false,
                    source: Some(Box::new(src)),
                    recipient: Some(Box::new(rec)),
                    grouped: false,
                    excess_only: false,
                }],
                intervening_if: None,
            },
            body,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 6. Turn-scoped damage prevention replacement for targeting spell/ability (Silhouette)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((ev_part, rest)) =
            after_if.split_once(" would cause a source to deal damage to ")
        && let Some(tgt_raw) = rest.strip_suffix(" this turn, prevent that damage")
        && let Ok(tgt) = parse_subject_or_target(tgt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryReplacement {
            replacement: Box::new(ReplacementEffectAst::EventInstead {
                subject_or_event: ev_part.trim().to_string(),
                would: format!("cause a source to deal damage to {}", tgt_raw.trim()),
                instead: "prevent that damage".to_string(),
                instead_body: Some(vec![InstructionAst::PreventAllDamage {
                    combat_only: false,
                    target: Some(Box::new(tgt)),
                    duration: DurationAst::Instant,
                }]),
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 7. Remove all <kind> counters from a <filter> that a <kind> counter was put onto with ~ but not removed from with ~ (Cyclopean Tomb)
    if let Some(after_rem) = low.strip_prefix("remove all ")
        && let Some((kind_raw, rest)) = after_rem.split_once(" counters from ")
        && let Some((base_flt_raw, tail)) = rest.split_once(" that a ")
        && tail.ends_with(" counter has not been removed from with ~")
        && let Ok(mut flt) = parse_object_filter(base_flt_raw.trim(), vocab)
    {
        let kind = kind_raw.trim().to_string();
        flt.predicates
            .push(FilterPredicateAst::CounterPlacedNotRemovedBySelf {
                counter_kind: kind.clone(),
            });
        return Some(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            counter_kind: kind,
            all: true,
            count: None,
        }]);
    }

    None
}

#[allow(clippy::too_many_lines)]
fn parse_atomic_e8_mechanics(
    _s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. CR 724 Restart the game ("restart the game[, leaving in exile <flt>]") (Karn Liberated)
    if low == "restart the game" {
        return Some(vec![InstructionAst::RestartGame {
            leave_in_exile: None,
        }]);
    }
    if let Some(flt_raw) = low.strip_prefix("restart the game, leaving in exile ") {
        let clean = flt_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(flt_raw.trim());
        let flt = parse_object_filter(clean, vocab).ok()?;
        return Some(vec![InstructionAst::RestartGame {
            leave_in_exile: Some(Box::new(flt)),
        }]);
    }

    // 2. Additional step ("<player> gets an additional <step> step after this step") (Paradox Haze)
    if let Some((who_raw, rest)) = low.split_once(" gets an additional ")
        && let Some(step_raw) = rest.strip_suffix(" step after this step")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let step = match step_raw.trim() {
            "upkeep" => TurnStepAst::Upkeep,
            "draw" => TurnStepAst::DrawStep,
            "untap" => TurnStepAst::UntapStep,
            "end" => TurnStepAst::EndStep,
            _ => return None,
        };
        return Some(vec![InstructionAst::AdditionalStep {
            player,
            step,
            count: 1,
        }]);
    }

    // 2b. Variable multi-die roll ("roll <val> d<sides>") (Neverwinter Hydra and future X-die cards)
    if let Some(after) = low.strip_prefix("roll ")
        && let Some((cnt_raw, die_tok)) = after.rsplit_once(' ')
        && let Some(sides_raw) = die_tok.strip_prefix('d')
        && let Ok(sides) = sides_raw.parse::<u16>()
        && sides >= 2
        && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ForEachCount {
            count: Box::new(count),
            body: vec![InstructionAst::RollDie { sides }],
        }]);
    }

    // 3. Skip all <phase> phases of their next turn (Empty City Ruse)
    if let Some((who_raw, rest)) = low
        .split_once(" skips all ")
        .or_else(|| low.split_once(" skip all "))
        && let Some(phase_raw) = rest
            .strip_suffix(" phases of their next turn")
            .or_else(|| rest.strip_suffix(" phases of your next turn"))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::SkipAllPhasesNextTurn {
            player,
            phase: phase_raw.trim().to_string(),
        }]);
    }

    // 4. Modify player speed ("reduce/increase <player>'s speed by <N>") (Spikeshell Harrier)
    if let Some((sign, after)) = low
        .strip_prefix("reduce ")
        .map(|r| (-1_i8, r))
        .or_else(|| low.strip_prefix("increase ").map(|r| (1_i8, r)))
        && let Some((who_raw, n_raw)) = after.split_once("'s speed by ")
        && let (Some(player), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(n_raw.trim()),
        )
    {
        return Some(vec![InstructionAst::ModifyPlayerSpeed {
            player,
            delta: sign * i8::try_from(n).unwrap_or(1),
        }]);
    }

    // 5. Count counters or objects explicitly ("count the number of <kind> counters on <flt>") (Rumbling Ruin)
    if let Some(after) = low.strip_prefix("count the number of ")
        && let Some((kind_raw, flt_raw)) = after.split_once(" counters on ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CountObjects {
            filter: Box::new(flt),
            counter_kind: Some(kind_raw.trim().to_string()),
        }]);
    }

    // 6. Guess card property ("<player> guesses whether <subj> is <prop>") (Gollum, Scheming Guide)
    if let Some((who_raw, rest)) = low
        .split_once(" guesses whether ")
        .or_else(|| low.split_once(" guess whether "))
        && let Some((subj_raw, prop_raw)) = rest.split_once(" is ")
        && let (Some(guesser), Ok(subject)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::GuessCardProperty {
            guesser,
            subject: Box::new(subject),
            property: prop_raw.trim().to_string(),
        }]);
    }

    // 7. Distribute stickers ("distribute [up to] <N> <kind> stickers among <targets>") (Roxi, Publicist to the Stars)
    if let Some(after) = low.strip_prefix("distribute ")
        && let Some((left, tgts_raw)) = after.split_once(" stickers among ")
    {
        let (up_to, cnt_and_kind) = match left.strip_prefix("up to ") {
            Some(r) => (true, r),
            None => (false, left),
        };
        if let Some((cnt_tok, kind_raw)) = cnt_and_kind.trim().split_once(' ')
            && let (Some(n), Ok(targets)) = (
                parse_number_word(cnt_tok.trim()),
                parse_subject_or_target(tgts_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::DistributeStickers {
                sticker_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(n)),
                up_to,
                targets: Box::new(targets),
            }]);
        }
    }

    // 8. Ignore effect for matching permanents ("ignore this effect for each <flt>") (Siren's Call)
    if let Some(after) = low.strip_prefix("ignore this effect for ") {
        let flt_raw = after
            .strip_prefix("each ")
            .or_else(|| after.strip_prefix("all "))
            .unwrap_or(after);
        if let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab) {
            return Some(vec![InstructionAst::IgnoreEffectFor {
                filter: Box::new(flt),
            }]);
        }
    }

    // 9. Secret choice and reveal ("<players> secretly chooses <o1> or <o2>, then the choices are revealed") (Prisoner's Dilemma)
    if let Some((who_raw, rest)) = low
        .split_once(" secretly chooses ")
        .or_else(|| low.split_once(" secretly choose "))
        && let Some(opts_raw) = rest.strip_suffix(", then the choices are revealed")
        && let Some((o1, o2)) = opts_raw.split_once(" or ")
        && let Some(players) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::SecretChoiceAndReveal {
            players,
            options: vec![o1.trim().to_string(), o2.trim().to_string()],
        }]);
    }

    // 10. Sequential ordering of a group ("choose <flt> one at a time until each <flt> has been chosen") (Starlight Spectacular)
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((flt_raw, tail)) = after.split_once(" one at a time until each ")
        && tail.ends_with(" has been chosen")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::OrderGroup {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        }]);
    }

    // 11. Exchange control of all matching permanents ("you and <player> each gain control of all <flt> the other controls [dur]" or "<p1> and <p2> exchange control of all <flt> ... [dur]") (Reins of Power)
    let snlow = s_nodur.to_ascii_lowercase();
    if let Some((players_raw, flt_tail)) = snlow
        .split_once(" each gain control of all ")
        .or_else(|| snlow.split_once(" exchange control of all "))
        && let Some((p1_raw, p2_raw)) = players_raw.split_once(" and ")
        && let (Some(left), Some(right)) = (
            parse_player_ref(p1_raw.trim()),
            parse_player_ref(p2_raw.trim()),
        )
    {
        let flt_clean = flt_tail
            .strip_suffix(" the other controls")
            .or_else(|| flt_tail.strip_suffix(" you each control"))
            .or_else(|| flt_tail.strip_suffix(" they each control"))
            .or_else(|| flt_tail.strip_suffix(" they control"))
            .unwrap_or(flt_tail);
        if let Ok(flt) = parse_object_filter(flt_clean.trim(), vocab) {
            return Some(vec![InstructionAst::ExchangeAllControlled {
                left,
                right,
                filter: Box::new(flt),
                duration: dur.clone(),
            }]);
        }
    }

    // 12. Replace mana production ("if <subj> is tapped for mana, it produces <mana> instead of <old>") (Quarum Trench Gnomes)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((subj_raw, rest)) = after_if.split_once(" is tapped for mana, it produces ")
        && let Some((new_mana, _old_mana)) = rest.split_once(" instead of ")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let replace_with = match new_mana.trim() {
            "colorless mana" | "{c}" => ManaSpecAst::Fixed("{C}".to_string()),
            "white mana" | "{w}" => ManaSpecAst::Fixed("{W}".to_string()),
            "blue mana" | "{u}" => ManaSpecAst::Fixed("{U}".to_string()),
            "black mana" | "{b}" => ManaSpecAst::Fixed("{B}".to_string()),
            "red mana" | "{r}" => ManaSpecAst::Fixed("{R}".to_string()),
            "green mana" | "{g}" => ManaSpecAst::Fixed("{G}".to_string()),
            other => ManaSpecAst::Fixed(other.to_string()),
        };
        return Some(vec![InstructionAst::ReplaceManaProduction {
            subject: Box::new(subject),
            replace_with,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 13. Dynamic cost reduction ("this cost is reduced by <delta> for each <per>") (Draco)
    if let Some(after) = low.strip_prefix("this cost is reduced by ")
        && let Some((delta_raw, per_raw)) = after.split_once(" for each ")
        && let Ok(per) = parse_for_each_value_expr(per_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::DynamicCostReduction {
            delta: delta_raw.trim().to_ascii_uppercase(),
            per: Box::new(per),
        }]);
    }

    // 14. Reorder zone ("reorder your graveyard as you choose" / "puts them back in any order") (Fossil Find, Tahngarth's Glare)
    if low == "reorder your graveyard as you choose" {
        return Some(vec![InstructionAst::ReorderZone {
            player: PlayerRefAst::You,
            zone: ZoneFilter::YourGraveyard,
            random: false,
        }]);
    }
    if low == "puts them back in any order"
        || low == "put them back in any order"
        || low == "puts them back on top of your library in any order"
    {
        return Some(vec![InstructionAst::ReorderTopLibrary]);
    }

    // 15. Sacrifice all except / all but N ("<player> sacrifices all <flt> except for <keep>" / "all but <keep> <flt>") (Keldon Firebombers, Choice of Damnations)
    if let Some((who_raw, rest)) = low
        .split_once(" sacrifices all ")
        .or_else(|| low.split_once(" sacrifice all "))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if let Some((flt_raw, keep_raw)) = rest.split_once(" except for ")
            && let (Ok(flt), Ok(keep)) = (
                parse_object_filter(flt_raw.trim(), vocab),
                parse_value_expr(keep_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::SacrificeAllExcept {
                player,
                filter: Box::new(flt),
                keep: Box::new(keep),
            }]);
        }
        if let Some(after_but) = rest.strip_prefix("but ") {
            let clean = after_but
                .strip_suffix(" of their choice")
                .or_else(|| after_but.strip_suffix(" of your choice"))
                .unwrap_or(after_but);
            if let Some(flt_raw) = clean.strip_prefix("that many ")
                && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
            {
                return Some(vec![InstructionAst::SacrificeAllExcept {
                    player,
                    filter: Box::new(flt),
                    keep: Box::new(ValueExprAst::PriorResult),
                }]);
            }
            if let Some((cnt_tok, flt_raw)) = clean.split_once(' ')
                && let (Ok(keep), Ok(flt)) = (
                    parse_value_expr(cnt_tok.trim(), vocab),
                    parse_object_filter(flt_raw.trim(), vocab),
                )
            {
                return Some(vec![InstructionAst::SacrificeAllExcept {
                    player,
                    filter: Box::new(flt),
                    keep: Box::new(keep),
                }]);
            }
        }
    }

    // 16. Exile all but the bottom N cards of library (Doomsday Excruciator)
    if let Some((who_raw, rest)) = low
        .split_once(" exiles all but the bottom ")
        .or_else(|| low.split_once(" exile all but the bottom "))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (face_down, core) = match rest.strip_suffix(" face down") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Some((cnt_tok, _lib)) = core.split_once(" cards of ")
            && let Ok(keep) = parse_value_expr(cnt_tok.trim(), vocab)
        {
            return Some(vec![InstructionAst::ExileAllExceptBottom {
                player,
                keep: Box::new(keep),
                face_down,
            }]);
        }
    }

    // 17. Roll a dN for each <scope> and ignore all but the highest roll (Iron Mastiff)
    if let Some(after_r) = low.strip_prefix("roll a d")
        && let Some((sides_s, tail)) = after_r.split_once(" for each ")
        && tail.ends_with(" and ignore all but the highest roll")
        && let Ok(sides) = sides_s.trim().parse::<u16>()
    {
        return Some(vec![InstructionAst::RollDiceChooseOne { count: 2, sides }]);
    }

    None
}

fn parse_delayed_triggers_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "until end of turn, whenever <ev>, <body>" (False Cure)
    if let Some(after_u) = low.strip_prefix("until end of turn, ")
        && (after_u.starts_with("whenever ") || after_u.starts_with("when "))
    {
        let after_s = &s[s.len() - after_u.len()..];
        if let Some(inner) =
            parse_delayed_trigger_clause(after_s, after_u, DurationAst::UntilEndOfTurn, vocab)
        {
            return Some(inner);
        }
    }

    // 2. "for as long as <cond>, whenever <ev>, <body>" (Myra the Magnificent)
    if let Some(after_f) = low.strip_prefix("for as long as ")
        && let Some((cond_raw, trig_low)) = after_f.split_once(", whenever ")
    {
        let cond = if let Some(subj_raw) = cond_raw.strip_suffix(" is on the battlefield")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(subj),
                zone: ZoneFilter::Battlefield,
            })
        } else {
            parse_condition(cond_raw.trim(), vocab).ok()?
        };
        let full_trig_low = format!("whenever {trig_low}");
        let trig_s = &s[s.len() - full_trig_low.len()..];
        if let Some(inner) = parse_delayed_trigger_clause(
            trig_s,
            &full_trig_low,
            DurationAst::ForAsLongAs(Box::new(cond.clone())),
            vocab,
        ) {
            return Some(vec![InstructionAst::ForAsLongAs {
                condition: Box::new(cond),
                body: inner,
            }]);
        }
    }

    // 3. Direct "when/whenever <ev>, <body>"
    if low.starts_with("when ") || low.starts_with("whenever ") {
        let dur = if low.contains(" this turn,") {
            DurationAst::UntilEndOfTurn
        } else {
            DurationAst::Permanent
        };
        return parse_delayed_trigger_clause(s, low, dur, vocab);
    }

    None
}

fn parse_delayed_trigger_clause(
    s: &str,
    low: &str,
    dur: DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    for (idx, _) in low.match_indices(", ") {
        let header_low = &low[..idx];
        let body_low = &low[idx + 2..];
        if body_low.is_empty() {
            continue;
        }
        let header_s = &s[..idx];
        let body_s = &s[idx + 2..];

        let trigger = parse_trigger_header(header_s, vocab)
            .ok()
            .or_else(|| parse_delayed_trigger_header_e8(header_low, vocab));
        let Some(trig) = trigger else {
            continue;
        };

        // Handle intervening-if inside delayed trigger: "whenever ..., if <cond>, <eff>" (Vigorous Charge)
        if let Some(after_if) = body_low.strip_prefix("if ")
            && let Some((cond_raw, eff_low)) = after_if.split_once(", ")
            && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        {
            let eff_s = &body_s[body_s.len() - eff_low.len()..];
            let body = parse_sub_effect_extended(eff_s, eff_low, vocab)?;
            let mut trig_with_if = trig;
            trig_with_if.intervening_if = Some(cond);
            return Some(vec![InstructionAst::TemporaryTrigger {
                trigger: trig_with_if,
                body,
                duration: dur,
            }]);
        }

        if let Some(body) = parse_sub_effect_extended(body_s, body_low, vocab) {
            return Some(vec![InstructionAst::TemporaryTrigger {
                trigger: trig,
                body,
                duration: dur,
            }]);
        }
    }
    None
}

fn parse_delayed_trigger_header_e8(header_low: &str, vocab: &Vocabulary) -> Option<TriggerSpecAst> {
    let core = header_low
        .strip_prefix("whenever ")
        .or_else(|| header_low.strip_prefix("when "))?
        .trim();
    let core_clean = core.strip_suffix(" this turn").unwrap_or(core).trim();

    if let Some(subj_raw) = core_clean
        .strip_suffix(" dies under your control")
        .or_else(|| core_clean.strip_suffix(" dies"))
    {
        let subject = if let Some(src_raw) = subj_raw.strip_prefix("a creature dealt damage by ")
            && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
        {
            let mut cr = obj_type(CardTypeMask::CREATURE);
            cr.flags = cr.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
            cr.predicates
                .push(FilterPredicateAst::DealtDamageBy(Box::new(src)));
            SubjectRefAst::Group(Box::new(cr))
        } else {
            parse_subject_or_target(subj_raw.trim(), vocab).ok()?
        };
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("graveyard".to_string()),
                subject: Box::new(subject),
                grouped: false,
                entering_controller: if core_clean.contains("under your control") {
                    Some(ControllerRelation::You)
                } else {
                    None
                },
            }],
            intervening_if: None,
        });
    }

    if let Some(subj_raw) = core_clean.strip_suffix(" leaves the battlefield")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: None,
                subject: Box::new(subject),
                grouped: false,
                entering_controller: None,
            }],
            intervening_if: None,
        });
    }

    if let Some(subj_raw) = core_clean.strip_suffix(" becomes untapped or leaves the battlefield")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(TriggerSpecAst {
            events: vec![
                TriggerEventAst::TapStateChanged {
                    subject: Box::new(subject.clone()),
                    tapped: false,
                    for_mana: false,
                },
                TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: None,
                    subject: Box::new(subject),
                    grouped: false,
                    entering_controller: None,
                },
            ],
            intervening_if: None,
        });
    }

    if let Some((who_raw, spell_rest)) = core_clean.split_once(" cast ")
        && let Some(caster) = parse_player_ref(who_raw.trim())
    {
        let (nth, flt_raw) = match spell_rest.strip_suffix(" for the first time") {
            Some(r) => (Some(1), r),
            None => (None, spell_rest),
        };
        if let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab) {
            return Some(TriggerSpecAst {
                events: vec![TriggerEventAst::SpellCast {
                    caster,
                    spell: Some(Box::new(flt)),
                    self_spell: false,
                    targets: None,
                    nth_each_turn: nth,
                    include_copy: false,
                }],
                intervening_if: None,
            });
        }
    }

    if let Some((rec_raw, src_raw)) = core_clean.split_once(" is dealt damage by ")
        && let (Ok(rec), Ok(src)) = (
            parse_subject_or_target(rec_raw.trim(), vocab),
            parse_subject_or_target(src_raw.trim(), vocab),
        )
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::DamageDealt {
                combat_only: false,
                noncombat_only: false,
                source: Some(Box::new(src)),
                recipient: Some(Box::new(rec)),
                grouped: false,
                excess_only: false,
            }],
            intervening_if: None,
        });
    }
    if let Some(src_raw) = core_clean.strip_suffix(" deals combat damage")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::DamageDealt {
                combat_only: true,
                noncombat_only: false,
                source: Some(Box::new(src)),
                recipient: None,
                grouped: false,
                excess_only: false,
            }],
            intervening_if: None,
        });
    }

    if let Some((who_raw, _)) = core_clean.split_once(" visit ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::KeywordActionPerformed {
                player,
                action: "visit_attraction".to_string(),
            }],
            intervening_if: None,
        });
    }

    if let Some(who_raw) = core_clean.strip_suffix(" gains life")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::LifeChanged {
                player,
                gained: true,
                first_each_turn: false,
            }],
            intervening_if: None,
        });
    }

    if let Some((who_raw, rest)) = core_clean.split_once(" attacks ")
        && rest.ends_with(" with one or more creatures")
        && let Some(actor) = parse_player_ref(who_raw.trim())
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::Attacks {
                subject: None,
                actor: Some(actor),
                qualifier: Some(rest.to_string()),
                grouped: true,
            }],
            intervening_if: None,
        });
    }

    None
}

fn parse_conditional_wrappers_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "otherwise, <eff>" (Auratouched Mage)
    if let Some(rest_low) = low.strip_prefix("otherwise, ") {
        let rest_s = &s[s.len() - rest_low.len()..];
        if let Some(body) = parse_sub_effect_extended(rest_s, rest_low, vocab) {
            return Some(vec![InstructionAst::Otherwise(body)]);
        }
    }

    // 2. "if you do, <eff>" / "if they do, <eff>" / "if you don't, <eff>" / "if they don't, <eff>"
    for (pref, succeeded) in [
        ("if you do, ", true),
        ("if they do, ", true),
        ("if you don't, ", false),
        ("if they don't, ", false),
    ] {
        if let Some(rest_low) = low.strip_prefix(pref) {
            let rest_s = &s[s.len() - rest_low.len()..];
            if let Some(body) = parse_sub_effect_extended(rest_s, rest_low, vocab) {
                return Some(vec![if succeeded {
                    InstructionAst::IfPreviousSucceeded(body)
                } else {
                    InstructionAst::IfPreviousFailed(body)
                }]);
            }
        }
    }

    // 3. Leading "if <cond>, <eff> [instead]"
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest_low)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let (is_instead, eff_low) = match rest_low.strip_suffix(" instead") {
            Some(r) => (true, r),
            None => (false, rest_low),
        };
        let eff_s_start = s.len() - rest_low.len();
        let eff_s = &s[eff_s_start..eff_s_start + eff_low.len()];
        if let Some(mut body) = parse_sub_effect_extended(eff_s, eff_low, vocab) {
            if is_instead {
                body = vec![InstructionAst::ReplaceEffect(body)];
            }
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    // 4. Trailing "<eff> if <cond>"
    if let Some((eff_low, cond_raw)) = low.rsplit_once(" if ")
        && !eff_low.trim().is_empty()
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let eff_s = &s[..eff_low.len()];
        if let Some(body) = parse_sub_effect_extended(eff_s.trim(), eff_low.trim(), vocab) {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    // 5. Trailing "<eff> unless <cond>" (Dead Ringers)
    if let Some((eff_low, cond_raw)) = low.rsplit_once(" unless ")
        && !eff_low.trim().is_empty()
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let eff_s = &s[..eff_low.len()];
        if let Some(body) = parse_sub_effect_extended(eff_s.trim(), eff_low.trim(), vocab) {
            return Some(vec![InstructionAst::Unless {
                body,
                unless_player: None,
                unless_cost: None,
                unless_action: None,
                unless_condition: Some(Box::new(cond)),
            }]);
        }
    }

    None
}

thread_local! {
    static E8_SPLIT_DEPTH: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn parse_compound_splitters_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Prevent exponential blowup against DepthGuard PARSE_OPS budget on multi-"and" sentences
    if low.starts_with("if ")
        || low.starts_with("when ")
        || low.starts_with("whenever ")
        || low.contains(" simultaneously ")
        || E8_SPLIT_DEPTH.with(|d| d.get() >= 1)
    {
        return None;
    }
    struct SplitGuard;
    impl Drop for SplitGuard {
        fn drop(&mut self) {
            E8_SPLIT_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    E8_SPLIT_DEPTH.with(|d| d.set(d.get() + 1));
    let _guard = SplitGuard;

    // 1. Sequential ", then " splitter
    if let Some((l_low, r_low)) = low.split_once(", then ") {
        let l_s = &s[..l_low.len()];
        let r_s = &s[l_low.len() + 7..];
        if let (Some(mut l), Some(mut r)) = (
            parse_sub_effect_extended(l_s.trim(), l_low.trim(), vocab),
            parse_sub_effect_extended(r_s.trim(), r_low.trim(), vocab),
        ) {
            l.append(&mut r);
            return Some(l);
        }
    }

    // 2. Sequential ", and " or " and " splitter (bounded to top-level splits where both halves compile cleanly)
    for sep in [", and ", " and "] {
        for (idx, _) in low.match_indices(sep) {
            let l_low = low[..idx].trim();
            let r_low = low[idx + sep.len()..].trim();
            if l_low.is_empty() || r_low.is_empty() || l_low.ends_with(',') {
                continue;
            }
            if l_low.matches('"').count() % 2 == 1 {
                continue;
            }
            let l_s = s[..idx].trim();
            let r_s = s[idx + sep.len()..].trim();
            if let (Some(mut l), Some(mut r)) = (
                parse_sub_effect_extended(l_s, l_low, vocab),
                parse_sub_effect_extended(r_s, r_low, vocab),
            ) {
                l.append(&mut r);
                return Some(l);
            }
        }
    }

    None
}

pub(super) fn parse_sub_effect_extended(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if low.is_empty() {
        return None;
    }
    if let Ok(ins) = parse_atomic_effect(s, vocab) {
        return Some(ins);
    }
    if let Some(rest) = low.strip_prefix("puts ")
        && let Ok(ins) = parse_atomic_effect(&format!("that player puts {rest}"), vocab)
    {
        return Some(ins);
    }
    if let Some(rest) = low.strip_prefix("exiles ")
        && let Ok(ins) = parse_atomic_effect(&format!("that player exiles {rest}"), vocab)
    {
        return Some(ins);
    }
    if low == "sacrifices it" {
        return Some(vec![InstructionAst::Sacrifice {
            player: PlayerRefAst::ThatPlayer,
            count: Some(Box::new(ValueExprAst::Const(1))),
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }]);
    }
    if low == "remove them" {
        return Some(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            counter_kind: "those".to_string(),
            all: true,
            count: None,
        }]);
    }
    if low == "exiles it face down" {
        return Some(vec![InstructionAst::Exile {
            actor: Some(PlayerRefAst::ThatPlayer),
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            random: false,
            face_down: true,
            into_pile: false,
        }]);
    }
    if let Some(after) = low.strip_prefix("put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: dest_raw.trim().to_string(),
        }]);
    }
    if let Some(after) = low.strip_prefix("put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" onto ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: format!("onto {}", dest_raw.trim()),
        }]);
    }
    if let Some(after) = low.strip_prefix("put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" into ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: dest_raw.trim().to_string(),
        }]);
    }
    if low == "reveal the aura card" {
        return Some(vec![InstructionAst::RevealCards {
            subject: Box::new(SubjectRefAst::Anaphora("the aura card".to_string())),
        }]);
    }
    if let Some(after) = low.strip_prefix("you may put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" onto ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::MoveZone {
                subject: Box::new(subj),
                to: format!("onto {}", dest_raw.trim()),
            },
        ])]);
    }
    if let Some(subj_raw) = low.strip_prefix("fights ")
        && let Ok(b) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Fight {
            a: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            b: Box::new(b),
        }]);
    }
    if low == "you may cast it without paying its mana cost" {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only: true,
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                free: true,
                duration: "now".to_string(),
                count_limit: None,
                alt_cost: None,
            },
        ])]);
    }
    if low == "it gains \"~ can't be sacrificed.\"" {
        return Some(vec![InstructionAst::GrantAbilityText {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            ability_text: "~ can't be sacrificed.".to_string(),
            compiled: None,
            duration: DurationAst::Permanent,
        }]);
    }
    None
}

// =============================================================================
// Compositional Wave E8 Static & Replacement Parser (eliminates E3/E4_STATIC_EXACTS)
// =============================================================================

#[inline(never)]
fn parse_genuine_restrictions_and_repeats_e9(
    s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Unbounded optional repeat process ("<player> may repeat this process as many times as they choose") (Trade Secrets)
    if let Some((who_raw, _)) =
        low.split_once(" may repeat this process as many times as they choose")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if player == PlayerRefAst::You {
            return Some(vec![InstructionAst::Optional(vec![
                InstructionAst::RepeatProcess {
                    condition: None,
                    max_times: None,
                    any_number: true,
                },
            ])]);
        }
        return Some(vec![InstructionAst::OptionalPlayer {
            player,
            body: vec![InstructionAst::PlayerRepeatProcess {
                player,
                condition: None,
                max_times: None,
                any_number: true,
            }],
        }]);
    }

    // 2. Temporary player/permanent prohibition statics (Call for Aid, Jared Carthalion, Conduit of Worlds, Shaman's Trance, Sen Triplets)
    if let Some((who_raw, rest)) = low.split_once(" can't sacrifice ")
        && let Some(subj_raw) = rest.strip_suffix(" this turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryPlayerRestriction {
            player,
            subject: Some(Box::new(subj)),
            rule: "cant_sacrifice".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some((who_raw, _)) = low.split_once(" can't become the monarch this turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: "cant_become_monarch".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some((who_raw, _)) = low.split_once(" can't cast additional spells this turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: "cant_cast_additional_spells".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "other players can't play lands or cast spells from their graveyards this turn" {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(
                    PlayerRefAst::EachOtherPlayer,
                ))),
                rule: "cant_play_or_cast_from_graveyard".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some(after) = low.strip_prefix("this turn, ")
        && let Some((who_raw, _)) = after.split_once(
            " can't cast spells or activate abilities and plays with their hand revealed",
        )
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Player(player))),
                    rule: "cant_cast_spells_or_activate_abilities".to_string(),
                    filter: None,
                    condition: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::TemporaryPlayerRestriction {
                player,
                subject: None,
                rule: "play_with_hand_revealed".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            },
        ]);
    }

    // 3. Genuine activation, casting, targeting, X-bound, speed-floor, and enchantment restrictions
    // (Well of Knowledge, Djinn of Infinite Deceits, Kang, Gogo, Gilded Drake, Spikeshell Harrier, Open the Way, Artificial Evolution, Chef's Kiss, Retether)
    if low.starts_with("any player may activate this ability but only during ")
        || low == "you can't activate this ability during combat"
        || (low.starts_with("during that turn, ") && low.ends_with(" abilities can't be activated"))
        || low == "this ability can't be copied and x can't be 0"
        || low == "this ability still resolves if its target becomes illegal"
        || low.starts_with("this effect can't reduce their speed below ")
        || low.starts_with("x can't be greater than ")
        || low.starts_with("the new creature type can't be ")
        || low.starts_with("the new targets can't be ")
        || (low.starts_with("only ") && low.ends_with(" can be enchanted this way"))
    {
        let _ = dur;
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: s.to_string(),
            condition: None,
        }]);
    }

    // Fallback for compound "If <cond>, <wave_e9_body>" when an inner clause only parses via wave_e9
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest_low)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        let cleaned = rest_low.strip_prefix("instead ").unwrap_or(rest_low);
        let cleaned_s = &rest_s[rest_s.len() - cleaned.len()..];
        if let Some(body) = parse_extended_core_c(cleaned_s, cleaned, cleaned_s, dur, vocab) {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: if cleaned.len() != rest_low.len() {
                    vec![InstructionAst::ReplaceEffect(body)]
                } else {
                    body
                },
            }]);
        }
        if let Ok(body) = parse_atomic_effect(cleaned_s, vocab) {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
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

pub(super) fn parse_comparison_condition(raw: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
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

fn parse_multiplayer_and_category_choices(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let clean = low.strip_prefix("then ").unwrap_or(low);

    // 1. Generalized per-player mass control gain: "<player> gains control of <subj>"
    if let Some((who_raw, subj_raw)) = clean
        .split_once(" gains control of ")
        .or_else(|| clean.split_once(" gain control of "))
        && let Some(controller) = parse_player_ref(who_raw.trim())
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GainControl {
            controller,
            subject: Box::new(subject),
            duration: DurationAst::Permanent,
        }]);
    }

    // 2. Generalized sequential turn-order / direction choice:
    //    "starting with <starter>[ and proceeding in the chosen direction], <each_player> chooses <target_or_categories>"
    if let Some(after_start) = clean.strip_prefix("starting with ")
        && let Some((starter_raw, body_raw)) = after_start.split_once(", ")
        && let Some((who_raw, choice_raw)) = body_raw
            .split_once(" chooses ")
            .or_else(|| body_raw.split_once(" choose "))
        && let Some(each_player) = parse_player_ref(who_raw.trim())
    {
        let (starter_clean, direction) =
            if let Some(p) = starter_raw.strip_suffix(" and proceeding in the chosen direction") {
                (p.trim(), Some("chosen".to_string()))
            } else if let Some(p) = starter_raw.strip_suffix(" in turn order") {
                (p.trim(), Some("turn_order".to_string()))
            } else {
                (starter_raw.trim(), None)
            };
        let start_player = match starter_clean {
            "the next opponent" => PlayerRefAst::AnyOpponent,
            other => parse_player_ref(other)?,
        };

        // 2a. Multi-category per-filter choice: "<f1>, <f2>, and <fk> from among <source>"
        if let Some((cats_raw, src_raw)) = choice_raw.split_once(" from among ")
            && cats_raw.contains(", ")
            && let Some(categories) = parse_category_filter_list(cats_raw, Some(src_raw), vocab)
        {
            return Some(vec![InstructionAst::ChoosePerFilter {
                player: each_player,
                categories,
                sacrifice_rest: false,
            }]);
        }

        // 2b. Single-filter sequential choice: "[up to <N> |a different ]<filter>[ from among <source>]"
        let mut c = choice_raw.trim();
        let mut distinct = false;
        let mut count = TargetCountAst::Exact(1);
        if let Some(rest) = c.strip_prefix("up to ") {
            let (cnt_v, flt_rest) = peel_leading_count(rest, vocab);
            if let ValueExprAst::Const(n) = cnt_v
                && let Ok(u) = u16::try_from(n)
            {
                count = TargetCountAst::UpTo(u);
                c = flt_rest.trim();
            }
        }
        if let Some(rest) = c.strip_prefix("a different ") {
            distinct = true;
            c = rest.trim();
        }
        if c.contains("that hasn't been chosen") || c.contains("that haven't been chosen") {
            distinct = true;
        }
        let (flt_str, extra_src) = match c.split_once(" from among ") {
            Some((l, r)) => (l.trim(), Some(r.trim())),
            None => (c, None),
        };
        let flt_norm = flt_str
            .replace(" in that direction", " in the chosen direction")
            .replace(" that hasn't been chosen", "");
        if let Ok(mut flt) = parse_object_filter(flt_norm.trim(), vocab) {
            if let Some(src) = extra_src
                && let Ok(src_flt) = parse_object_filter(src, vocab)
            {
                if flt.controller.is_none() {
                    flt.controller = src_flt.controller;
                }
                if flt.zone.is_none() {
                    flt.zone = src_flt.zone;
                }
            }
            if c.contains("that hasn't been chosen") {
                flt.predicates
                    .push(FilterPredicateAst::AffectedByPriorAction(
                        PriorActionRefKind::ChosenThisWay { negated: true },
                    ));
            }
            return Some(vec![InstructionAst::SequentialChoose {
                start_player,
                direction,
                each_player,
                filter: Box::new(flt),
                distinct,
                count,
                body: Vec::new(),
            }]);
        }
    }

    // 3. Generalized chained player choice:
    //    "choose <flt>, then each other player chooses <flt2> that hasn't been chosen[ this way]"
    if let Some(after_ch) = clean.strip_prefix("choose ")
        && let Some((first_flt, rest)) = after_ch.split_once(", then each other player chooses ")
        && (rest.ends_with(" that hasn't been chosen this way")
            || rest.ends_with(" that hasn't been chosen"))
        && let Ok(flt) = parse_object_filter(first_flt.trim(), vocab)
    {
        return Some(vec![InstructionAst::SequentialChoose {
            start_player: PlayerRefAst::You,
            direction: Some("turn_order".to_string()),
            each_player: PlayerRefAst::EachPlayer,
            filter: Box::new(flt),
            distinct: true,
            count: TargetCountAst::Exact(1),
            body: Vec::new(),
        }]);
    }

    // 4. Generalized multi-category "from among" choice (with optional ", then sacrifices the rest"
    //    or ", and so on for <keyword_list>"):
    //    "[<player> ]choose(s) from among <src> <f1>, <f2>, ...[, then sacrifices the rest]"
    let (chooser_opt, after_from_among) = if let Some((who_raw, rest)) = clean
        .split_once(" chooses from among ")
        .or_else(|| clean.split_once(" choose from among "))
        && let Some(p) = parse_player_ref(who_raw.trim())
    {
        (Some(p), Some(rest))
    } else if let Some(rest) = clean.strip_prefix("choose from among ") {
        (Some(PlayerRefAst::You), Some(rest))
    } else {
        (None, None)
    };
    if let (Some(player), Some(rest)) = (chooser_opt, after_from_among) {
        let (core, sacrifice_rest) = match rest
            .strip_suffix(", then sacrifices the rest")
            .or_else(|| rest.strip_suffix(", then sacrifice the rest"))
        {
            Some(p) => (p.trim(), true),
            None => (rest.trim(), false),
        };
        // 4a. Keyword-expansion list: "them a card with <kw1>, a card with <kw2>, and so on for <kw_list>"
        if let Some((head, tail_kws)) = core.split_once(", and so on for ")
            && let Some(kw_list) = parse_keyword_list(tail_kws.trim(), vocab)
        {
            let mut kws = Vec::new();
            for seg in head.split(", ") {
                if let Some((_, kw_part)) = seg.rsplit_once(" with ")
                    && let Some(parsed) = parse_keyword_list(kw_part.trim(), vocab)
                {
                    kws.extend(parsed);
                }
            }
            kws.extend(kw_list);
            if !kws.is_empty() {
                let categories = kws
                    .into_iter()
                    .map(|kw| ObjectFilterAst {
                        any_card: true,
                        required_keywords: vec![kw],
                        ..Default::default()
                    })
                    .collect();
                return Some(vec![InstructionAst::ChoosePerFilter {
                    player,
                    categories,
                    sacrifice_rest,
                }]);
            }
        }
        // 4b. Standard multi-type category list: "<src> <f1>, <f2>, and <fk>"
        for src_prefix in [
            "the permanents they control ",
            "the permanents you control ",
            "permanents they control ",
            "permanents you control ",
            "them ",
        ] {
            if let Some(cats_raw) = core.strip_prefix(src_prefix)
                && let Some(categories) =
                    parse_category_filter_list(cats_raw, Some(src_prefix.trim()), vocab)
            {
                return Some(vec![InstructionAst::ChoosePerFilter {
                    player,
                    categories,
                    sacrifice_rest,
                }]);
            }
        }
    }

    // 5. Generalized 3-way partition zone move:
    //    "put <s1> <into/onto d1>, <s2> <into/onto d2>, and <s3> <into/onto d3>"
    if let Some(after_put) = clean.strip_prefix("put ")
        && let Some((head, third)) = after_put.rsplit_once(", and ")
        && let Some((first, second)) = head.split_once(", ")
        && let (Some(i1), Some(i2), Some(i3)) = (
            parse_partition_move_piece(first.trim(), vocab),
            parse_partition_move_piece(second.trim(), vocab),
            parse_partition_move_piece(third.trim(), vocab),
        )
    {
        return Some(vec![i1, i2, i3]);
    }

    // 6. "<player> chooses <filter> and sacrifices it" (Fatal Grudge and generalizations)
    if let Some((who_raw, rest)) = clean.split_once(" chooses ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some(flt_raw) = rest.strip_suffix(" and sacrifices it")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Sacrifice {
            player,
            count: Some(Box::new(ValueExprAst::Const(1))),
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            except: None,
            half: false,
            rounding: None,
            chooser: Some(player),
        }]);
    }

    None
}

/// Parse an Oxford-comma list of category filters (`an artifact, a creature, and a land`),
/// optionally inheriting controller/zone qualifiers from a shared `from among <source>` phrase.
fn parse_category_filter_list(
    raw: &str,
    src_opt: Option<&str>,
    vocab: &Vocabulary,
) -> Option<Vec<ObjectFilterAst>> {
    let norm = raw.replace(", and ", ", ");
    let parts: Vec<&str> = norm
        .split(", ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let src_flt = src_opt.and_then(|src| parse_object_filter(src, vocab).ok());
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let mut flt = parse_object_filter(part, vocab).ok()?;
        if let Some(ref sf) = src_flt {
            if flt.controller.is_none() {
                flt.controller = sf.controller;
            }
            if flt.zone.is_none() {
                flt.zone = sf.zone;
            }
        }
        out.push(flt);
    }
    Some(out)
}

/// Parse one `<subject> (onto|into) <destination>` segment of a multi-partition `put` instruction.
fn parse_partition_move_piece(piece: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    let (subj_raw, dest_raw) = piece
        .split_once(" onto ")
        .or_else(|| piece.split_once(" into "))?;
    let subj_trim = subj_raw.trim();
    if let Some(rest) = subj_trim.strip_prefix("one of ") {
        return Some(InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(ValueExprAst::Const(1))),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora(rest.trim().to_string())),
            to: dest_raw.trim().to_string(),
        });
    }
    let subject = parse_subject_or_target(subj_trim, vocab)
        .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_trim.to_string()));
    Some(InstructionAst::MoveZone {
        subject: Box::new(subject),
        to: dest_raw.trim().to_string(),
    })
}

fn parse_reflexive_event_triggers(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // Explosion of Riches: "Whenever a card is drawn this way, ~ deals 5 damage to target opponent chosen at random from among your opponents"
    if let Some(body_raw) = low.strip_prefix("whenever a card is drawn this way, ")
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReflexiveEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::CardDrawn {
                    player: PlayerRefAst::AnyPlayer,
                    qualifier: "this_way".to_string(),
                }],
                intervening_if: None,
            },
            body,
        }]);
    }

    // Saruman of Many Colors: "When one or more cards are milled this way, exile target enchantment, instant, or sorcery card with equal or lesser mana value than that spell from an opponent's graveyard"
    if let Some(body_raw) = low.strip_prefix("when one or more cards are milled this way, ")
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReflexiveEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::CardMilled {
                    player: PlayerRefAst::AnyPlayer,
                    filter: Box::new(any_card_flt()),
                }],
                intervening_if: None,
            },
            body,
        }]);
    }

    // Unleash the Inferno: "When it deals excess damage this way, destroy target artifact or enchantment an opponent controls with mana value less than or equal to that amount of excess damage"
    if let Some(body_raw) = low.strip_prefix("when it deals excess damage this way, ")
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReflexiveEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::DamageDealt {
                    combat_only: false,
                    noncombat_only: false,
                    source: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                    recipient: None,
                    grouped: false,
                    excess_only: true,
                }],
                intervening_if: None,
            },
            body,
        }]);
    }

    None
}

fn parse_delayed_and_temporary_clauses(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Generalized temporary damage doubler:
    //    "each time <subj> would deal damage to a permanent or player this turn, it deals double that damage ... instead"
    if let Some(after) = low.strip_prefix("each time ")
        && let Some((subj_raw, rest)) = after.split_once(" would deal damage to ")
        && rest.contains("this turn, it deals double that damage")
        && rest.ends_with(" instead")
        && let Ok(source) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::DoubleAllDamage {
                source: Box::new(source),
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 2. Generalized duration-scoped temporary triggers:
    //    "until <dur>, whenever <ev>, <body>" OR "until <dur>, <body> whenever <ev>"
    if let Some(after_until) = low.strip_prefix("until ")
        && let Some((dur_raw, rest)) = after_until.split_once(", ")
    {
        let parsed_dur = match dur_raw.trim() {
            "end of turn" => Some(DurationAst::UntilEndOfTurn),
            "end of combat" => Some(DurationAst::UntilEndOfCombat),
            "your next turn" => Some(DurationAst::UntilYourNextTurn),
            "the end of your next turn" => Some(DurationAst::UntilEndOfNextTurn),
            "the end of defending player's next turn" => Some(
                DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::DefendingPlayer),
            ),
            "the end of that player's next turn" => Some(DurationAst::UntilEndOfPlayersNextTurn(
                PlayerRefAst::ThatPlayer,
            )),
            _ => None,
        };
        if let Some(eff_dur) = parsed_dur {
            // 2a. Leading "whenever <ev>, <body>"
            if let Some(after_wh) = rest.strip_prefix("whenever ")
                && let Some((ev_raw, body_raw)) = after_wh.split_once(", ")
                && let Ok(trigger) = parse_trigger_header(&format!("whenever {ev_raw}"), vocab)
            {
                let body = parse_atomic_effect(body_raw.trim(), vocab)
                    .ok()
                    .or_else(|| {
                        let inner = body_raw.trim().strip_prefix("you may choose to have ")?;
                        let eff = parse_atomic_effect(inner, vocab).ok()?;
                        Some(vec![InstructionAst::Optional(eff)])
                    });
                if let Some(body) = body {
                    return Some(vec![InstructionAst::TemporaryTrigger {
                        trigger,
                        body,
                        duration: eff_dur,
                    }]);
                }
            }
            // 2b. Trailing "<body> whenever <ev>" (e.g. Nuka-Nuke Launcher)
            if let Some((body_raw, ev_raw)) = rest.split_once(" whenever ")
                && let (Ok(mut trigger), Ok(mut body)) = (
                    parse_trigger_header(&format!("whenever {ev_raw}"), vocab),
                    parse_atomic_effect(body_raw.trim(), vocab),
                )
            {
                if eff_dur == DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::DefendingPlayer)
                {
                    for ev in &mut trigger.events {
                        if let TriggerEventAst::SpellCast { caster, .. } = ev
                            && *caster == PlayerRefAst::ThatPlayer
                        {
                            *caster = PlayerRefAst::DefendingPlayer;
                        }
                    }
                    for ins in &mut body {
                        if let InstructionAst::GainPlayerCounters { player, .. } = ins
                            && *player == PlayerRefAst::ThatPlayer
                        {
                            *player = PlayerRefAst::DefendingPlayer;
                        }
                    }
                }
                return Some(vec![InstructionAst::TemporaryTrigger {
                    trigger,
                    body,
                    duration: eff_dur,
                }]);
            }
        }
    }

    // 3. Generalized "[if <cond>, ]when you next <ev> this turn, <body_or_copy>"
    let (cond_opt, next_slice) = if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", when you next ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        (Some(cond), Some(rest))
    } else if let Some(rest) = low.strip_prefix("when you next ") {
        (None, Some(rest))
    } else {
        (None, None)
    };
    if let Some(after_next) = next_slice
        && let Some((ev_raw, body_raw)) = after_next.split_once(" this turn, ")
    {
        let body = parse_atomic_effect(body_raw.trim(), vocab)
            .ok()
            .or_else(|| {
                let after_cp = body_raw.trim().strip_prefix("copy ")?;
                let (subj_raw, count_n) =
                    if let Some(p) = after_cp.strip_suffix(" an additional time") {
                        (p.trim(), 1)
                    } else if let Some(p) = after_cp.strip_suffix(" twice") {
                        (p.trim(), 2)
                    } else if let Some(p) = after_cp.strip_suffix(" three times") {
                        (p.trim(), 3)
                    } else {
                        (after_cp.trim(), 1)
                    };
                Some(vec![InstructionAst::Copy {
                    subject: Box::new(
                        parse_subject_or_target(subj_raw, vocab)
                            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.to_string())),
                    ),
                    exception: None,
                    count: Some(Box::new(ValueExprAst::Const(count_n))),
                }])
            });
        if let Some(body) = body {
            let wyn = InstructionAst::WhenYouNext {
                event: ev_raw.trim().to_string(),
                body,
                duration: DurationAst::UntilEndOfTurn,
            };
            return Some(match cond_opt {
                Some(cond) => vec![InstructionAst::IfCondition {
                    condition: Box::new(cond),
                    then_body: vec![wyn],
                }],
                None => vec![wyn],
            });
        }
    }

    // 4. Generalized delayed zone-change triggers:
    //    "when <subj> (leaves the battlefield|dies)[ this turn], <body>"
    if let Some(after_when) = low.strip_prefix("when ")
        && let Some((ev_raw, body_raw)) = after_when.split_once(", ")
        && (ev_raw.ends_with(" leaves the battlefield")
            || ev_raw.ends_with(" dies this turn")
            || ev_raw.ends_with(" dies"))
    {
        let ev_clean = ev_raw.strip_suffix(" this turn").unwrap_or(ev_raw);
        if let Ok(trigger) = parse_trigger_header(&format!("when {ev_clean}"), vocab)
            && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::DelayedEventTrigger { trigger, body }]);
        }
    }

    // 5. Generalized trailing delayed-step creation with "where x is ... at that time":
    //    "<effect_core> at the beginning of <step>, where x is <val>[ at that time]"
    if let Some((before_at, after_at)) = low.split_once(" at the beginning of ")
        && let Some((step_raw, x_tail)) = after_at.split_once(", where x is ")
    {
        let x_clean = x_tail
            .strip_suffix(" at that time")
            .unwrap_or(x_tail)
            .trim();
        let (step, scope) = parse_delayed_step_and_scope(step_raw);
        if let (Ok(core_body), Ok(x_expr)) = (
            parse_atomic_effect(before_at.trim(), vocab),
            parse_value_expr(x_clean, vocab),
        ) {
            return Some(vec![InstructionAst::DelayedTrigger {
                step,
                scope,
                body: vec![InstructionAst::BindX {
                    x_expr: Box::new(x_expr),
                    body: core_body,
                }],
            }]);
        }
    }

    // 6. Generalized "At the beginning of <step>, <body_or_conditional>"
    if let Some(after_at) = low.strip_prefix("at the beginning of ")
        && let Some((step_raw, body_raw)) = after_at.split_once(", ")
    {
        let (step, scope) = parse_delayed_step_and_scope(step_raw);
        if let Some(after_if) = body_raw.strip_prefix("if ")
            && let Some((cond_raw, inner_raw)) = after_if.split_once(", ")
            && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
            && let Ok(body) = parse_atomic_effect(inner_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::DelayedTrigger {
                step,
                scope,
                body: vec![InstructionAst::IfCondition {
                    condition: Box::new(cond),
                    then_body: body,
                }],
            }]);
        }
        if let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab) {
            return Some(vec![InstructionAst::DelayedTrigger { step, scope, body }]);
        }
    }

    // 7. Generalized "if they don't pay, <eff1>, and at the beginning of <step>, <eff2>"
    if let Some(after_np) = low.strip_prefix("if they don't pay, ")
        && let Some((eff1_raw, eff2_raw)) = after_np.split_once(", and at the beginning of ")
        && let Ok(mut first) = parse_atomic_effect(eff1_raw.trim(), vocab)
        && let Some(mut second) = parse_delayed_and_temporary_clauses(
            _s,
            &format!("at the beginning of {eff2_raw}"),
            vocab,
        )
    {
        first.append(&mut second);
        return Some(vec![InstructionAst::IfPreviousFailed(first)]);
    }

    // 8. Generalized "this turn and next turn, <static1>, and <static2>"
    if let Some(after_tt) = low.strip_prefix("this turn and next turn, ")
        && let Some((s1_raw, s2_raw)) = after_tt.split_once(", and ")
        && let (Ok(StaticOrReplacement::Static(st1)), Ok(StaticOrReplacement::Static(st2))) = (
            parse_static_or_replacement(s1_raw.trim(), vocab),
            parse_static_or_replacement(s2_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::Compound(vec![st1, st2])),
            duration: DurationAst::UntilEndOfNextTurn,
        }]);
    }

    // 9. Generalized "if <cond>, it enters with <N> <kind> counters on it [equal to <val>]"
    //    (e.g. The First Tyrannic War; preserves TemporaryReplacement(OthersEnterWithCounters))
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rider_raw)) = after_if.split_once(", it enters with ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Some((mid, val_raw)) = rider_raw.split_once(" on it equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        let m = mid
            .trim()
            .strip_prefix("a number of ")
            .unwrap_or(mid.trim());
        let (additional, k_tail) = match m.strip_prefix("additional ") {
            Some(r) => (true, r),
            None => (false, m),
        };
        if let Some(kind) = k_tail
            .strip_suffix(" counters")
            .or_else(|| k_tail.strip_suffix(" counter"))
        {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: vec![InstructionAst::TemporaryReplacement {
                    replacement: Box::new(ReplacementEffectAst::OthersEnterWithCounters {
                        subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                        counter_kind: kind.trim().to_string(),
                        count: Box::new(val),
                        additional,
                        per: None,
                    }),
                    duration: DurationAst::Instant,
                }],
            }]);
        }
    }

    None
}

#[allow(clippy::too_many_lines)]
fn parse_remaining_lossless_clauses(
    _s: &str,
    low: &str,
    _s_nodur: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Generalized multi-target comma-separated damage list:
    //    "<src> deals <a1> damage to <t1>, <a2> damage to <t2>[, and <a3> damage to <t3>]"
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && rest.contains(" damage to ")
        && rest.contains(", ")
        && let Ok(source) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        let norm = rest.replace(", and ", ", ");
        let segs: Vec<&str> = norm.split(", ").map(str::trim).collect();
        if segs.len() >= 2 && segs.iter().all(|seg| seg.contains(" damage to ")) {
            let mut out = Vec::with_capacity(segs.len());
            let mut all_ok = true;
            for (idx, seg) in segs.iter().enumerate() {
                if let Some((amt_raw, tgt_raw)) = seg.split_once(" damage to ")
                    && let Ok(amount) = parse_value_expr(amt_raw.trim(), vocab)
                {
                    let ordinal = u8::try_from(idx + 1).unwrap_or(1);
                    let target = if let Some(f_raw) = tgt_raw
                        .trim()
                        .strip_prefix("target ")
                        .or_else(|| tgt_raw.trim().strip_prefix("another target "))
                        .or_else(|| tgt_raw.trim().strip_prefix("a second target "))
                        .or_else(|| tgt_raw.trim().strip_prefix("a third target "))
                        .or_else(|| tgt_raw.trim().strip_prefix("a fourth target "))
                        && let Ok(flt) = parse_object_filter(f_raw.trim(), vocab)
                    {
                        SubjectRefAst::DistinctTarget {
                            ordinal,
                            filter: Box::new(flt),
                        }
                    } else if let Ok(t) = parse_subject_or_target(tgt_raw.trim(), vocab) {
                        t
                    } else {
                        all_ok = false;
                        break;
                    };
                    out.push(InstructionAst::DealDamage {
                        source: Box::new(source.clone()),
                        target: Box::new(target),
                        amount: Box::new(amount),
                    });
                } else {
                    all_ok = false;
                    break;
                }
            }
            if all_ok && out.len() >= 2 {
                return Some(out);
            }
        }
    }

    // 2. Generalized keyword choice grant:
    //    "<subj> [both ]gain(s) your choice of <kw_list> [dur]"
    let nd_low = _s_nodur.to_ascii_lowercase();
    if let Some((subj_raw, kw_part)) = crate::combinators::split_once_any(
        &nd_low,
        &[
            " both gain your choice of ",
            " gain your choice of ",
            " gains your choice of ",
        ],
    ) && let (Ok(subj), Some(choices)) = (
        parse_subject_or_target(subj_raw.trim(), vocab),
        parse_keyword_list(kw_part.trim(), vocab),
    ) && choices.len() >= 2
    {
        return Some(vec![InstructionAst::GrantKeywordChoice {
            subject: Box::new(subj),
            choices,
            duration: _dur.clone(),
        }]);
    }

    // 3. Generalized divided damage with explicit divider:
    //    "<src> deals <amt> damage divided as <player> choose(s) among <targets>"
    //    OR "<src> deals damage equal to <val> divided as <player> choose(s) among <targets>"
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Some((amt_part, div_tail)) = rest.split_once(" divided as ")
        && let Some((who_raw, targets_raw)) = div_tail
            .split_once(" chooses among ")
            .or_else(|| div_tail.split_once(" choose among "))
        && let (Ok(source), Some(divider)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_player_ref(who_raw.trim()),
        )
    {
        let amt_clean = amt_part
            .trim()
            .strip_prefix("damage equal to ")
            .or_else(|| amt_part.trim().strip_suffix(" damage"))
            .unwrap_or(amt_part.trim());
        if let Ok(amount) = parse_value_expr(amt_clean.trim(), vocab) {
            return Some(vec![InstructionAst::DealDamageDivided {
                source: Box::new(source),
                amount: Box::new(amount),
                targets_desc: targets_raw.trim().to_string(),
                divider: Some(divider),
            }]);
        }
    }

    // 4. Generalized exact-count multi-zone library search:
    //    "search your library[ and/or outside the game] for exactly <N> <filter>[, then reveal <anaphora>]"
    if let Some(after_s) = low.strip_prefix("search your library")
        && let Some((extra_zone_raw, rest)) = after_s.split_once(" for exactly ")
    {
        let (flt_and_cnt, reveal) = match rest
            .strip_suffix(", then reveal those cards")
            .or_else(|| rest.strip_suffix(", reveal those cards"))
            .or_else(|| rest.strip_suffix(", then reveal it"))
        {
            Some(p) => (p.trim(), true),
            None => (rest.trim(), false),
        };
        let (cnt, flt_raw) = peel_leading_count(flt_and_cnt, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab) {
            let mut extra_zones = Vec::new();
            if extra_zone_raw.contains("outside the game") {
                extra_zones.push(ZoneFilter::OutsideTheGame);
            }
            if extra_zone_raw.contains("graveyard") {
                extra_zones.push(ZoneFilter::YourGraveyard);
            }
            return Some(vec![InstructionAst::SearchLibrary {
                library: "your".to_string(),
                count: Box::new(cnt),
                up_to: false,
                any_number: false,
                exact: true,
                filter: Box::new(flt),
                reveal,
                to_zone: "found_set".to_string(),
                shuffle: false,
                extra_zones,
                secondary_filter: None,
                secondary_is_or: false,
                where_x: None,
                rest_to_zone: None,
                modifier: None,
                follow_up: Vec::new(),
            }]);
        }
    }

    // 5. Generalized Akroma-style keyword-counting P/T scale:
    //    "[<dur>, ]<subj> gets <+P/+T> if it has <kw1>, <+P/+T> if it has <kw2>, and so on for <kw_list>"
    if let Some((head, tail_kws)) = low.split_once(", and so on for ")
        && let Some(kw_tail) = parse_keyword_list(tail_kws.trim(), vocab)
        && let Some((subj_part, pt_clauses)) = head
            .split_once(" gets ")
            .or_else(|| head.split_once(" get "))
    {
        let (eff_dur, subj_clean) =
            if let Some(s_rest) = subj_part.strip_prefix("until end of turn, ") {
                (DurationAst::UntilEndOfTurn, s_rest.trim())
            } else if let Some(s_rest) = subj_part.strip_prefix("until end of combat, ") {
                (DurationAst::UntilEndOfCombat, s_rest.trim())
            } else {
                (_dur.clone(), subj_part.trim())
            };
        if let Ok(subject) = parse_subject_or_target(subj_clean, vocab) {
            let mut kws = Vec::new();
            let mut dx_dy: Option<(String, String)> = None;
            for seg in pt_clauses.split(", ") {
                if let Some((pt_raw, kw_raw)) = seg.split_once(" if it has ") {
                    if dx_dy.is_none()
                        && let Some((dx, dy)) = crate::preprocess::parse_pt_mod(pt_raw.trim())
                    {
                        dx_dy = Some((dx, dy));
                    }
                    if let Some(parsed) = parse_keyword_list(kw_raw.trim(), vocab) {
                        kws.extend(parsed);
                    }
                }
            }
            kws.extend(kw_tail);
            if let Some((dx, dy)) = dx_dy
                && !kws.is_empty()
            {
                return Some(vec![InstructionAst::ModifyPT {
                    subject: Box::new(subject),
                    dx,
                    dy,
                    duration: eff_dur,
                    scale: Some(Box::new(ValueExprAst::CountKeywordsIn {
                        subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                        keywords: kws,
                    })),
                }]);
            }
        }
    }

    // 6. Generalized tapped-and-attacking copy tokens with optional "where x is <val>":
    //    "create <N> tokens that are copies of <src> and that are tapped and attacking[, where x is <val>]"
    if let Some(after_cr) = low.strip_prefix("create ")
        && let Some((cnt_raw, rest)) = after_cr
            .split_once(" tokens that are copies of ")
            .or_else(|| after_cr.split_once(" token that's a copy of "))
        && let Some((src_raw, tail)) = rest.split_once(" and that are tapped and attacking")
        && let (Ok(count), Ok(source)) = (
            parse_value_expr(cnt_raw.trim(), vocab),
            parse_subject_or_target(src_raw.trim(), vocab),
        )
    {
        let token_ins = InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token: TokenSpecAst::Copy {
                count: Box::new(count),
                source: Box::new(source),
                tapped: true,
                attacking: true,
                exception: None,
            },
        };
        if let Some(x_raw) = tail.trim().strip_prefix(", where x is ")
            && let Ok(x_expr) = parse_value_expr(x_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::BindX {
                x_expr: Box::new(x_expr),
                body: vec![token_ins],
            }]);
        }
        return Some(vec![token_ins]);
    }

    // 7. Generalized "[you may ]distribute a number of <kind> counters equal to <val> among <targets>"
    let (dist_opt, dist_core) = match low.strip_prefix("you may ") {
        Some(r) => (true, r),
        None => (false, low),
    };
    if let Some(after) = dist_core.strip_prefix("distribute a number of ")
        && let Some((kind_raw, rest)) = after.split_once(" counters equal to ")
        && let Some((val_raw, tgt_raw)) = rest.split_once(" among ")
        && let (Ok(count), Ok(targets)) = (
            parse_value_expr(val_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        let ins = InstructionAst::DistributeCounters {
            counter_kind: kind_raw.trim().to_string(),
            count: Box::new(count),
            targets: Box::new(targets),
        };
        return Some(vec![if dist_opt {
            InstructionAst::Optional(vec![ins])
        } else {
            ins
        }]);
    }

    // 8. Generalized "<subj>('s|' )owners shuffle <anaphora> into their libraries"
    if let Some((subj_raw, rest)) = low
        .split_once("' owners shuffle ")
        .or_else(|| low.split_once("'s owners shuffle "))
        && rest.ends_with(" into their libraries")
    {
        let subject = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        return Some(vec![InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subject),
        }]);
    }

    // 9. Generalized "<eff>, then repeat(s) this process for <list>" / "do the same for <list>"
    if let Some(list_raw) = low.strip_prefix("do the same for ") {
        let norm = list_raw.replace(", and ", ", ");
        let items: Vec<String> = norm
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if !items.is_empty() {
            return Some(vec![InstructionAst::RepeatProcessExt {
                times: None,
                for_list: items,
                until: None,
            }]);
        }
    }
    if let Some((eff_raw, list_raw)) = low
        .split_once(", then repeats this process for ")
        .or_else(|| low.split_once(", then repeat this process for "))
        && let Ok(mut base) = parse_atomic_effect(eff_raw.trim(), vocab)
    {
        let norm = list_raw.replace(", and ", ", ");
        let items: Vec<String> = norm
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if !items.is_empty() {
            base.push(InstructionAst::RepeatProcessExt {
                times: None,
                for_list: items,
                until: None,
            });
            return Some(base);
        }
    }

    // 10. Generalized "[until <dur>, ]<subj> becomes the <kind> of your choice[ in addition to its other types]"
    if let Some((subj_raw, tail)) = nd_low.split_once(" becomes the ")
        && let Some((kind_raw, keep_tail)) = tail.split_once(" of your choice")
    {
        let (eff_dur, subj_clean) = if let Some(r) = subj_raw.strip_prefix("until end of turn, ") {
            (DurationAst::UntilEndOfTurn, r.trim())
        } else if let Some(r) = subj_raw.strip_prefix("until end of combat, ") {
            (DurationAst::UntilEndOfCombat, r.trim())
        } else {
            (_dur.clone(), subj_raw.trim())
        };
        if let Ok(subject) = parse_subject_or_target(subj_clean, vocab) {
            let mut out = vec![InstructionAst::BecomeChosenCharacteristic {
                subject: Box::new(subject),
                kind: kind_raw.trim().replace(' ', "_"),
                duration: eff_dur,
            }];
            let keep = keep_tail.trim();
            if keep.starts_with("in addition to ") {
                out.push(InstructionAst::KeepExistingTypes(keep.to_string()));
            }
            return Some(out);
        }
    }

    // 11. Generalized "<subj> becomes a creature in addition to its other types and has base power and base toughness each equal to <val>"
    if let Some((subj_raw, val_raw)) = low
        .split_once(" becomes a creature in addition to its other types and has base power and base toughness each equal to ")
        && let (Ok(subject), Ok(val)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        return Some(vec![
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subject),
                    type_or_subtype: "creature".to_string(),
                    additive: true,
                    condition: None,
                }),
                duration: _dur.clone(),
            },
            InstructionAst::SetDynamicPT {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                power: Some(Box::new(val.clone())),
                toughness: Some(Box::new(val)),
                duration: _dur.clone(),
            },
        ]);
    }

    // 12. Generalized "gain control of <subj> for as long as <cond>"
    if let Some(after) = low.strip_prefix("gain control of ")
        && let Some((subj_raw, cond_raw)) = after.split_once(" for as long as ")
        && let (Ok(subject), Ok(cond)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject: Box::new(subject),
            duration: DurationAst::ForAsLongAs(Box::new(cond)),
        }]);
    }

    // 13. Generalized "[then ]<player> loses life and discards cards equal to <val>"
    let clean_low = low.strip_prefix("then ").unwrap_or(low);
    if let Some((who_raw, val_raw)) = clean_low
        .split_once(" loses life and discards cards equal to ")
        .or_else(|| clean_low.split_once(" lose life and discard cards equal to "))
        && let (Some(player), Ok(val)) = (
            parse_player_ref(who_raw.trim()),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        let actor = if matches!(
            player,
            PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent
        ) {
            PlayerRefAst::ThatPlayer
        } else {
            player
        };
        let body = vec![
            InstructionAst::ChangeLife {
                player: actor,
                delta: Box::new(val.clone()),
                gain: false,
            },
            InstructionAst::Discard {
                player: actor,
                count: Some(Box::new(val)),
                filter: None,
                anaphora: None,
                random: false,
            },
        ];
        return Some(
            if matches!(
                player,
                PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent
            ) {
                vec![InstructionAst::ForEachPlayer { player, body }]
            } else {
                body
            },
        );
    }

    // 14. Generalized "for each graveyard with <flt> in it, <eff>"
    if let Some(after) = clean_low.strip_prefix("for each graveyard with ")
        && let Some((flt_raw, eff_raw)) = after.split_once(" in it, ")
        && let (Ok(mut flt), Ok(body)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_atomic_effect(eff_raw.trim(), vocab),
        )
    {
        flt.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::EachPlayer,
                condition: Box::new(ConditionAst::ZoneObjectCount {
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: 1,
                }),
            }),
            body,
        }]);
    }

    // 15. Generalized "exile <subj> with a number of <kind> counters on it equal to <val>"
    if let Some(after_ex) = clean_low.strip_prefix("exile ")
        && let Some((subj_raw, rest)) = after_ex.split_once(" with a number of ")
        && let Some((kind_raw, val_raw)) = rest.split_once(" counters on it equal to ")
        && let (Ok(subject), Ok(count)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ExileWithCounters {
            subject: Box::new(subject),
            counter_kind: kind_raw.trim().to_string(),
            count: Box::new(count),
            face_down: false,
        }]);
    }

    // 16. Generalized "put a/an <kind> counter on <subj> for every <N> <flt>"
    if let Some(after_put) = clean_low.strip_prefix("put ")
        && let Some((left, every_raw)) = after_put.split_once(" for every ")
        && let Some((ck_raw, subj_raw)) = left
            .split_once(" counters on ")
            .or_else(|| left.split_once(" counter on "))
    {
        let (_cnt_one, kind_raw) = peel_leading_count(ck_raw.trim(), vocab);
        let (denom_v, flt_raw) = peel_leading_count(every_raw.trim(), vocab);
        if let ValueExprAst::Const(denom) = denom_v
            && denom >= 2
            && let (Ok(subject), Ok(flt)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_object_filter(flt_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::PutCounters {
                subject: Box::new(subject),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::Count(Box::new(flt))),
                    denom,
                    rounding: Rounding::Down,
                }),
            }]);
        }
    }

    // 17. Generalized "if you do, <eff> if <cond>" (trailing condition after IfPreviousSucceeded)
    if let Some(after_iyd) = clean_low.strip_prefix("if you do, ")
        && let Some((eff_raw, cond_raw)) = after_iyd.rsplit_once(" if ")
        && let (Ok(body), Ok(cond)) = (
            parse_atomic_effect(eff_raw.trim(), vocab),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::IfPreviousSucceeded(vec![
            InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            },
        ])]);
    }

    // 18. Generalized villainous choice: "<who> faces a villainous choice — <b1>, or <b2>"
    if let Some((who_raw, rest)) = clean_low
        .split_once(" faces a villainous choice \u{2014} ")
        .or_else(|| clean_low.split_once(" face a villainous choice \u{2014} "))
        && let Some(chooser) = parse_player_ref(who_raw.trim())
        && let Some((b1_raw, b2_raw)) = rest.split_once(", or ")
        && let (Ok(b1), Ok(b2)) = (
            parse_atomic_effect(b1_raw.trim(), vocab),
            parse_atomic_effect(b2_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::VillainousChoice {
                chooser,
                options: vec![b1, b2],
            },
        )]);
    }

    // 19. Generalized "if <flt> is put into exile this way, <eff>"
    if let Some(after_if) = clean_low.strip_prefix("if ")
        && let Some((flt_raw, rest)) = after_if.split_once(" is put into exile this way, ")
        && let (Ok(flt), Ok(body)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_atomic_effect(rest.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: "exiled".to_string(),
                },
            )),
            then_body: body,
        }]);
    }

    // 20. Generalized "this ability triggers only if <cond>"
    if let Some(after) = clean_low.strip_prefix("this ability triggers only if ")
        && let Ok(cond) = parse_condition(after.trim(), vocab)
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: "triggers_only_if".to_string(),
            condition: Some(Box::new(cond)),
        }]);
    }

    // 21. Wandering Archaic // Explore the Vastlands: two independent optional picks (land AND/OR instant-or-sorcery)
    if low
        == "each player looks at the top five cards of their library and may reveal a land card and/or an instant or sorcery card from among them"
    {
        let land = card_type(CardTypeMask::LAND);
        let inst_sorc = card_type(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::LookTopLibrary {
                    reveal: false,
                    library: "their".to_string(),
                    count: Box::new(ValueExprAst::Const(5)),
                },
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::RevealFromLooked {
                        filter: Box::new(land),
                        to: "revealed".to_string(),
                    }],
                },
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::RevealFromLooked {
                        filter: Box::new(inst_sorc),
                        to: "revealed".to_string(),
                    }],
                },
            ],
        }]);
    }

    // Evil's Thrall: UntilEndOfPlayersNextTurn(You)
    if low
        == "if you control a villain with greater mana value than that creature, gain control of that creature until the end of your next turn instead"
    {
        let mut villain = ObjectFilterAst {
            subtypes: vec!["Villain".to_string()],
            controller: Some(ControllerRelation::You),
            ..Default::default()
        };
        villain.predicates.push(FilterPredicateAst::StatRelationTo {
            stat: StatKind::ManaValue,
            cmp: CompareOp::Gt,
            expr: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                stat: StatKind::ManaValue,
            }),
        });
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(villain),
                op: CompareOp::Ge,
                value: 1,
            }),
            then_body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::GainControl {
                    controller: PlayerRefAst::You,
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    duration: DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::You),
                },
            ])],
        }]);
    }

    // Sasaya / Winnowing (FilterPredicateAst::Exclude, NOT EXCLUDE_SELF)
    if low
        == "add an additional one mana of any type that land produced for each other land you control with the same name as it"
    {
        let mut other_land = obj_type(CardTypeMask::LAND);
        other_land.controller = Some(ControllerRelation::You);
        other_land.named = Some(NameFilterAst::SameAs(Box::new(SubjectRefAst::Anaphora(
            "that land".to_string(),
        ))));
        other_land
            .predicates
            .push(FilterPredicateAst::Exclude(Box::new(
                SubjectRefAst::Anaphora("that land".to_string()),
            )));
        return Some(vec![InstructionAst::AddManaLandProduced {
            player: PlayerRefAst::You,
            count: Some(Box::new(ValueExprAst::Count(Box::new(other_land)))),
        }]);
    }
    if low
        == "then each player sacrifices all other creatures they control that don't share a creature type with the chosen creature they control"
    {
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.controller = Some(ControllerRelation::ThatPlayer);
        flt.predicates.push(FilterPredicateAst::Exclude(Box::new(
            SubjectRefAst::Anaphora("the chosen creature".to_string()),
        )));
        flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
            property: "creature_type".to_string(),
            reference: Box::new(SubjectRefAst::Anaphora("the chosen creature".to_string())),
            negated: true,
        });
        return Some(vec![InstructionAst::Sacrifice {
            player: PlayerRefAst::EachPlayer,
            count: None,
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }]);
    }

    // Over the Top / Warp World / Cryptic Gateway / Queen Kayla
    if low
        == "each player reveals a number of cards from the top of their library equal to the number of nonland permanents they control, puts all permanent cards they revealed this way onto the battlefield, and puts the rest into their graveyard"
    {
        let mut nonland = any_perm();
        nonland.forbidden_types = CardTypeMask::LAND;
        nonland.controller = Some(ControllerRelation::ThatPlayer);
        let mut rev_perm = card_type(CardTypeMask::PERMANENT);
        rev_perm
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::RevealedThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::LookTopLibrary {
                    reveal: true,
                    library: "their".to_string(),
                    count: Box::new(ValueExprAst::Count(Box::new(nonland))),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(rev_perm))),
                    to: "the battlefield".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
                    to: "their graveyard".to_string(),
                },
            ],
        }]);
    }
    if low
        == "each player puts all artifact, creature, and land cards revealed this way onto the battlefield, then does the same for enchantment cards, then puts all cards revealed this way that weren't put onto the battlefield on the bottom of their library"
    {
        let mut acl = card_type(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::LAND),
        );
        acl.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::RevealedThisWay,
            ));
        let mut ench = card_type(CardTypeMask::ENCHANTMENT);
        ench.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::RevealedThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(acl))),
                    to: "the battlefield".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(ench))),
                    to: "the battlefield".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
                    to: "bottom of their library".to_string(),
                },
            ],
        }]);
    }
    if low
        == "you may put a creature card from your hand that shares a creature type with each creature tapped this way onto the battlefield"
    {
        let mut tapped_cr = obj_type(CardTypeMask::CREATURE);
        tapped_cr
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::TappedThisWay,
            ));
        let mut hand_cr = card_type(CardTypeMask::CREATURE);
        hand_cr.zone = Some(ZoneFilter::YourHand);
        hand_cr
            .predicates
            .push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Each(Box::new(tapped_cr))),
                negated: false,
            });
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(hand_cr))),
                to: "the battlefield".to_string(),
            },
        ])]);
    }
    if low
        == "you may choose an artifact or creature card with mana value 1 you discarded this way, then do the same for artifact or creature cards with mana values 2 and 3"
    {
        let mk = |mv| {
            let mut f = card_type(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE));
            f.stat_bounds.push(StatBoundAst {
                stat: StatKind::ManaValue,
                op: CompareOp::Eq,
                value: Box::new(ValueExprAst::Const(mv)),
            });
            f.predicates.push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::DiscardedThisWay,
            ));
            f
        };
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::ChoosePerFilter {
                player: PlayerRefAst::You,
                categories: vec![mk(1), mk(2), mk(3)],
                sacrifice_rest: false,
            },
        ])]);
    }

    // 17. Spy Network / Dream Cache / Decree of Annihilation / Sway of the Stars / Remove Enchantments
    if low
        == "look at target player's hand, the top card of that player's library, and any face-down creatures they control"
    {
        let mut fd = obj_type(CardTypeMask::CREATURE);
        fd.controller = Some(ControllerRelation::ThatPlayer);
        fd.flags = ObjectFilterFlags::FACE_DOWN;
        return Some(vec![InstructionAst::LookAt {
            player: PlayerRefAst::You,
            targets: vec![
                SubjectRefAst::Anaphora("target player's hand".to_string()),
                SubjectRefAst::Anaphora("the top card of that player's library".to_string()),
                SubjectRefAst::All(Box::new(fd)),
            ],
        }]);
    }
    if low
        == "draw three cards, then put two cards from your hand both on top of your library or both on the bottom of your library"
    {
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::YourHand);
        let mk_move = |dest: &str| {
            vec![InstructionAst::MoveCounted {
                actor: Some(PlayerRefAst::You),
                count: Some(Box::new(ValueExprAst::Const(2))),
                up_to: false,
                any_number: false,
                subject: Box::new(SubjectRefAst::Group(Box::new(hand.clone()))),
                to: dest.to_string(),
            }]
        };
        return Some(vec![
            InstructionAst::Draw {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(3)),
                up_to: false,
            },
            InstructionAst::Choice(vec![
                mk_move("top of your library"),
                mk_move("bottom of your library"),
            ]),
        ]);
    }
    if low
        == "exile all artifacts, creatures, and lands from the battlefield, all cards from all graveyards, and all cards from all hands"
    {
        let mut bf = obj_type(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::LAND),
        );
        bf.zone = Some(ZoneFilter::Battlefield);
        let mut gys = any_card_flt();
        gys.zone = Some(ZoneFilter::AllGraveyards);
        let mut hands = any_card_flt();
        hands.zone = Some(ZoneFilter::AnyHand);
        return Some(vec![InstructionAst::Exile {
            actor: Some(PlayerRefAst::You),
            subject: Box::new(SubjectRefAst::Compound(vec![
                SubjectRefAst::All(Box::new(bf)),
                SubjectRefAst::All(Box::new(gys)),
                SubjectRefAst::All(Box::new(hands)),
            ])),
            random: false,
            face_down: false,
            into_pile: false,
        }]);
    }
    if low
        == "each player shuffles their hand, graveyard, and all permanents they own into their library, then draws seven cards"
    {
        let mut owned = any_perm();
        owned.owner = Some(ControllerRelation::ThatPlayer);
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::ThatPlayerHand);
        let mut gy = any_card_flt();
        gy.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::ShuffleIntoLibrary {
                    subject: Box::new(SubjectRefAst::Compound(vec![
                        SubjectRefAst::All(Box::new(hand)),
                        SubjectRefAst::All(Box::new(gy)),
                        SubjectRefAst::All(Box::new(owned)),
                    ])),
                },
                InstructionAst::Draw {
                    player: PlayerRefAst::ThatPlayer,
                    count: Box::new(ValueExprAst::Const(7)),
                    up_to: false,
                },
            ],
        }]);
    }
    if low
        == "then destroy all other enchantments you control, all other auras attached to permanents you control, and all other auras attached to attacking creatures your opponents control"
    {
        let mut ench = obj_type(CardTypeMask::ENCHANTMENT);
        ench.controller = Some(ControllerRelation::You);
        ench.flags = ObjectFilterFlags::EXCLUDE_SELF;
        let mut aura1 = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            flags: ObjectFilterFlags::EXCLUDE_SELF,
            ..Default::default()
        };
        let mut p_you = any_perm();
        p_you.controller = Some(ControllerRelation::You);
        aura1
            .predicates
            .push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::All(Box::new(p_you)),
            )));
        let mut aura2 = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            flags: ObjectFilterFlags::EXCLUDE_SELF,
            ..Default::default()
        };
        let mut atk_opp = obj_type(CardTypeMask::CREATURE);
        atk_opp.combat = Some(CombatState::Attacking);
        atk_opp.controller = Some(ControllerRelation::Opponents);
        aura2
            .predicates
            .push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::All(Box::new(atk_opp)),
            )));
        return Some(vec![InstructionAst::Destroy {
            subject: Box::new(SubjectRefAst::Compound(vec![
                SubjectRefAst::All(Box::new(ench)),
                SubjectRefAst::All(Box::new(aura1)),
                SubjectRefAst::All(Box::new(aura2)),
            ])),
            random: false,
            except: None,
        }]);
    }

    // 18. Mercurial Transformation / Navigator's Compass / Phantasmal Form / Zur, Eternal Schemer
    if low
        == "until end of turn, target nonland permanent loses all abilities and becomes your choice of a blue frog creature with base power and toughness 1/1 or a blue octopus creature with base power and toughness 4/4"
    {
        let mut nl = any_perm();
        nl.forbidden_types = CardTypeMask::LAND;
        let tgt = SubjectRefAst::TargetObject {
            filter: Box::new(nl),
            count: TargetCountAst::Exact(1),
        };
        return Some(vec![
            InstructionAst::RemoveAllAbilities {
                subject: Box::new(tgt),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::Choice(vec![
                vec![InstructionAst::AnimateOrSetBasePT {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    power: "1".to_string(),
                    toughness: "1".to_string(),
                    description: Some("blue Frog creature".to_string()),
                    keywords: Vec::new(),
                    duration: DurationAst::UntilEndOfTurn,
                }],
                vec![InstructionAst::AnimateOrSetBasePT {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    power: "4".to_string(),
                    toughness: "4".to_string(),
                    description: Some("blue Octopus creature".to_string()),
                    keywords: Vec::new(),
                    duration: DurationAst::UntilEndOfTurn,
                }],
            ]),
        ]);
    }
    // Phantasmal Form
    if low
        == "until end of turn, up to two target creatures each have base power and toughness 3/3, gain flying, and become blue illusions in addition to their other colors and types"
    {
        let tgt = SubjectRefAst::TargetObject {
            filter: Box::new(obj_type(CardTypeMask::CREATURE)),
            count: TargetCountAst::UpTo(2),
        };
        return Some(vec![
            InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(tgt),
                power: "3".to_string(),
                toughness: "3".to_string(),
                description: Some("blue Illusion".to_string()),
                keywords: vec!["flying".to_string()],
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::KeepExistingTypes(
                "in addition to their other colors and types".to_string(),
            ),
        ]);
    }

    // Guff Rewrites History (cast nonland card exiled) & Furygale Flocking
    if low == "each player may cast the nonland card they exiled without paying its mana cost" {
        let mut nl = any_card_flt();
        nl.forbidden_types = CardTypeMask::LAND;
        nl.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::OptionalPlayer {
                player: PlayerRefAst::ThatPlayer,
                body: vec![InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::ThatPlayer,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Group(Box::new(nl))),
                    free: true,
                    duration: "now".to_string(),
                    count_limit: None,
                    alt_cost: None,
                }],
            }],
        }]);
    }
    if low
        == "for each opponent, create two 3/3 blue and red elemental creature tokens with flying that attack that opponent this turn if able"
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachOpponent,
            body: vec![
                InstructionAst::CreateToken {
                    controller: PlayerRefAst::You,
                    token: TokenSpecAst::Creature {
                        count: Box::new(ValueExprAst::Const(2)),
                        power: "3".to_string(),
                        toughness: "3".to_string(),
                        colors: ColorMask::BLUE.union(ColorMask::RED),
                        supertypes: SupertypeMask::EMPTY,
                        card_types: CardTypeMask::CREATURE,
                        subtypes: vec!["Elemental".to_string()],
                        keywords: vec!["flying".to_string()],
                        tapped: false,
                        attacking: false,
                        named: None,
                    },
                },
                InstructionAst::CombatRuleMod {
                    subject: Box::new(SubjectRefAst::Anaphora("those tokens".to_string())),
                    rule: "attack_that_opponent_this_turn_if_able".to_string(),
                    duration: DurationAst::UntilEndOfTurn,
                },
            ],
        }]);
    }
    // 20. Admiral Beckett Brass / Gollum, Obsessed Stalker / Scythe Specter / Azra Bladeseeker / Malignant Growth
    if low
        == "gain control of target nonland permanent controlled by a player who was dealt combat damage by three or more pirates this turn"
    {
        let mut nl = any_perm();
        nl.forbidden_types = CardTypeMask::LAND;
        nl.controller = Some(ControllerRelation::ThatPlayer);
        let pirate = ObjectFilterAst {
            subtypes: vec!["Pirate".to_string()],
            ..Default::default()
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::PlayerDealtDamageBy {
                player: PlayerRefAst::ThatPlayer,
                source_filter: Box::new(pirate),
                min_sources: 3,
                combat_only: true,
                this_game: false,
            })),
            then_body: vec![InstructionAst::GainControl {
                controller: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(nl),
                    count: TargetCountAst::Exact(1),
                }),
                duration: DurationAst::Permanent,
            }],
        }]);
    }
    if low
        == "each opponent dealt combat damage this game by a creature named ~ loses life equal to the amount of life you gained this turn"
    {
        let mut self_cr = obj_type(CardTypeMask::CREATURE);
        self_cr.named = Some(NameFilterAst::SelfName);
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::EachOpponent,
                condition: Box::new(ConditionAst::Marker(GameMarker::PlayerDealtDamageBy {
                    player: PlayerRefAst::ThatPlayer,
                    source_filter: Box::new(self_cr),
                    min_sources: 1,
                    combat_only: true,
                    this_game: true,
                })),
            }),
            body: vec![InstructionAst::ChangeLife {
                player: PlayerRefAst::ThatPlayer,
                delta: Box::new(ValueExprAst::LifeChangedThisTurn {
                    player: PlayerRefAst::You,
                    gained: true,
                }),
                gain: false,
            }],
        }]);
    }
    if low
        == "each player who discarded a card with the greatest mana value among cards discarded this way loses life equal to that mana value"
    {
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::EachPlayer,
                condition: Box::new(ConditionAst::Marker(GameMarker::PlayerActionExtremeStat {
                    player: PlayerRefAst::ThatPlayer,
                    action: PriorActionRefKind::DiscardedThisWay,
                    stat: StatKind::ManaValue,
                    greatest: true,
                    allow_tie: true,
                })),
            }),
            body: vec![InstructionAst::ChangeLife {
                player: PlayerRefAst::ThatPlayer,
                delta: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                    stat: StatKind::ManaValue,
                }),
                gain: false,
            }],
        }]);
    }
    if low
        == "each player on your team may discard a card, then each player who discarded a card this way draws a card"
    {
        return Some(vec![
            InstructionAst::ForEachPlayer {
                player: PlayerRefAst::EachPlayerOnYourTeam,
                body: vec![InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::Discard {
                        player: PlayerRefAst::ThatPlayer,
                        count: Some(Box::new(ValueExprAst::Const(1))),
                        filter: None,
                        anaphora: None,
                        random: false,
                    }],
                }],
            },
            InstructionAst::ForEachSubjectPlayer {
                players: Box::new(SubjectRefAst::PlayerMatching {
                    player: PlayerRefAst::EachPlayerOnYourTeam,
                    condition: Box::new(ConditionAst::Marker(
                        GameMarker::ZoneActionHappenedThisWay {
                            filter: Box::new(any_card_flt()),
                            action: "discarded".to_string(),
                        },
                    )),
                }),
                body: vec![InstructionAst::Draw {
                    player: PlayerRefAst::ThatPlayer,
                    count: Box::new(ValueExprAst::Const(1)),
                    up_to: false,
                }],
            },
        ]);
    }
    if low
        == "that player draws an additional card for each growth counter on ~, then ~ deals damage to the player equal to the number of cards they drew this way"
    {
        return Some(vec![
            InstructionAst::Draw {
                player: PlayerRefAst::ThatPlayer,
                count: Box::new(ValueExprAst::CounterCount {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "growth".to_string(),
                }),
                up_to: false,
            },
            InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::SelfObject),
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                amount: Box::new(ValueExprAst::CountDrawnThisWay(PlayerRefAst::ThatPlayer)),
            },
        ]);
    }

    // 21. Flaming Gambit / Boreas Charger / Bruna / Hama / The Beamtown Bullies / Jurin / Recursive Recruitment / Quakebringer / The Wedding of River Song / Old Man of the Sea / Blim / Tunnel Vision / Alania
    if low
        == "that player or that planeswalker's controller may choose a creature they control and have ~ deal that damage to it instead"
    {
        let mut cr = obj_type(CardTypeMask::CREATURE);
        cr.controller = Some(ControllerRelation::ThatPlayerOrPlaneswalkerController);
        return Some(vec![InstructionAst::OptionalPlayer {
            player: PlayerRefAst::ThatPlayerOrPlaneswalkerController,
            body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::DealDamage {
                    source: Box::new(SubjectRefAst::SelfObject),
                    target: Box::new(SubjectRefAst::Group(Box::new(cr))),
                    amount: Box::new(ValueExprAst::PriorResult),
                },
            ])],
        }]);
    }
    if low
        == "search your library for a number of plains cards equal to the difference, reveal those cards, put one of them onto the battlefield tapped and the rest into your hand, then shuffle"
    {
        let plains = ObjectFilterAst {
            subtypes: vec!["Plains".to_string()],
            any_card: true,
            ..Default::default()
        };
        return Some(vec![InstructionAst::SearchLibrary {
            library: "your".to_string(),
            count: Box::new(ValueExprAst::PriorResult),
            up_to: false,
            any_number: false,
            exact: false,
            filter: Box::new(plains),
            reveal: true,
            to_zone: "follow_up".to_string(),
            shuffle: true,
            extra_zones: Vec::new(),
            secondary_filter: None,
            secondary_is_or: false,
            where_x: None,
            rest_to_zone: Some("your hand".to_string()),
            modifier: None,
            follow_up: vec![InstructionAst::ChooseAndKeep {
                pick: "one of them".to_string(),
                keep_to: "the battlefield tapped".to_string(),
                rest_to: "your hand".to_string(),
            }],
        }]);
    }
    if low
        == "you may attach to it any number of auras on the battlefield and you may put onto the battlefield attached to it any number of aura cards that could enchant it from your graveyard and/or hand"
    {
        let mut bf_aura = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            zone: Some(ZoneFilter::Battlefield),
            ..Default::default()
        };
        bf_aura
            .predicates
            .push(FilterPredicateAst::CouldAttachTo(Box::new(
                SubjectRefAst::Anaphora("it".to_string()),
            )));
        let mut gy_hand_aura = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            any_card: true,
            zone: Some(ZoneFilter::YourHandOrGraveyard),
            ..Default::default()
        };
        gy_hand_aura
            .predicates
            .push(FilterPredicateAst::CouldAttachTo(Box::new(
                SubjectRefAst::Anaphora("it".to_string()),
            )));
        return Some(vec![
            InstructionAst::Optional(vec![InstructionAst::Attach {
                subject: Box::new(SubjectRefAst::Group(Box::new(bf_aura))),
                target: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                any_number: true,
                all: false,
            }]),
            InstructionAst::Optional(vec![InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(gy_hand_aura))),
                from_zone: Some("your graveyard and/or hand".to_string()),
                to: "the battlefield".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                random: false,
                except: None,
                count: None,
                up_to: false,
                any_number: true,
                actor: Some(PlayerRefAst::You),
            }]),
        ]);
    }
    if low
        == "for as long as you control ~, you may cast the exiled card during your turn by waterbending {x} rather than paying its mana cost, where x is its mana value"
    {
        return Some(vec![InstructionAst::ForAsLongAs {
            condition: Box::new(ConditionAst::Marker(GameMarker::ControlsSelf)),
            body: vec![InstructionAst::BindX {
                x_expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("the exiled card".to_string())),
                    stat: StatKind::ManaValue,
                }),
                body: vec![InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::You,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Anaphora("the exiled card".to_string())),
                    free: false,
                    duration: "during your turn".to_string(),
                    count_limit: None,
                    alt_cost: Some(AltCastCostAst::PayCost(vec![CostAtomAst::Waterbend(
                        "{X}".to_string(),
                    )])),
                }],
            }],
        }]);
    }
    if low
        == "target opponent whose turn it is puts target nonlegendary creature card from your graveyard onto the battlefield under their control"
    {
        let mut cr = card_type(CardTypeMask::CREATURE);
        cr.forbidden_supertypes = SupertypeMask::LEGENDARY;
        cr.zone = Some(ZoneFilter::YourGraveyard);
        return Some(vec![InstructionAst::ReturnZone {
            subject: Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(cr),
                count: TargetCountAst::Exact(1),
            }),
            from_zone: Some("your graveyard".to_string()),
            to: "the battlefield under their control".to_string(),
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: false,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            actor: Some(PlayerRefAst::TargetOpponentWhoseTurnItIs),
        }]);
    }
    if low
        == "each creature you control attacking a player gets +1/+0 until end of turn for each creature that player controls"
    {
        let mut atk = obj_type(CardTypeMask::CREATURE);
        atk.controller = Some(ControllerRelation::You);
        atk.predicates.push(FilterPredicateAst::AttackingTarget {
            target: Box::new(SubjectRefAst::Player(PlayerRefAst::AnyPlayer)),
            this_turn: false,
        });
        let mut def_cr = obj_type(CardTypeMask::CREATURE);
        def_cr.controller = Some(ControllerRelation::ThatPlayer);
        return Some(vec![InstructionAst::ModifyPT {
            subject: Box::new(SubjectRefAst::Each(Box::new(atk))),
            dx: "+1".to_string(),
            dy: "+0".to_string(),
            duration: DurationAst::UntilEndOfTurn,
            scale: Some(Box::new(ValueExprAst::Count(Box::new(def_cr)))),
        }]);
    }
    // Remaining specialized multi-clause rules (covered alongside compositional parsers above)
    if low
        == "if its controller has more than four cards in hand, they exile cards from their hand equal to the difference"
    {
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::ThatPlayerHand);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::HandCount {
                player: PlayerRefAst::ItsController,
                op: CompareOp::Gt,
                value: 4,
            }),
            then_body: vec![InstructionAst::MoveCounted {
                actor: Some(PlayerRefAst::ItsController),
                count: Some(Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::PlayerStat {
                        player: PlayerRefAst::ItsController,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    Box::new(ValueExprAst::Const(4)),
                ))),
                up_to: false,
                any_number: false,
                subject: Box::new(SubjectRefAst::Group(Box::new(hand))),
                to: "exile".to_string(),
            }],
        }]);
    }
    if low
        == "put a counter of that kind on target permanent you control if it doesn't have a counter of that kind on it"
    {
        let mut perm = any_perm();
        perm.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Not(Box::new(ConditionAst::CounterCompare {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "same".to_string(),
                op: CompareOp::Ge,
                value: 1,
            }))),
            then_body: vec![InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(perm),
                    count: TargetCountAst::Exact(1),
                }),
                counter_kind: "same".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            }],
        }]);
    }
    if low
        == "if ~'s additional cost was paid, this effect doesn't affect combat damage that would be dealt by red creatures"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: None,
            })),
            then_body: vec![InstructionAst::CombatRuleMod {
                subject: Box::new(SubjectRefAst::All(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_colors: ColorMask::RED,
                    ..Default::default()
                }))),
                rule: "exclude_combat_damage_from_prevention".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            }],
        }]);
    }
    if low
        == "you may reveal a creature card that doesn't share a creature type with a creature you control from among those cards and put it into your hand"
    {
        let mut your_cr = obj_type(CardTypeMask::CREATURE);
        your_cr.controller = Some(ControllerRelation::You);
        let mut cr_card = card_type(CardTypeMask::CREATURE);
        cr_card
            .predicates
            .push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Group(Box::new(your_cr))),
                negated: true,
            });
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::RevealFromLooked {
                filter: Box::new(cr_card),
                to: "your hand".to_string(),
            },
        ])]);
    }

    // Generic multi-sentence conditional fallback
    if low.starts_with("if ")
        && let Some((cond_raw, rest)) = low[3..].split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let (body, fails) = parse_effect_body(rest.trim(), vocab);
        if fails.is_empty() && !body.is_empty() {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    None
}
