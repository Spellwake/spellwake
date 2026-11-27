//! Zone manipulation, exile, sacrifice, discard, library search/ops, pile separation,
//! and exchange effect parsers.

use super::{
    parse_atomic_effect, parse_effect_body, parse_sub_effect_extended, split_leading_player_verb,
};
use crate::ast::{
    CardTypeMask, ConditionAst, ControllerRelation, CountLimitAst, DelayedStepScope, DurationAst,
    ExchangeKind, FilterPredicateAst, InstructionAst, NameFilterAst, ObjectFilterAst,
    ObjectFilterFlags, PileRefAst, PlayerRefAst, Rounding, SearchModifierAst, StatKind,
    StaticAbilityAst, SubjectRefAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, peel_leading_count};
use crate::clauses::statics::is_orthogonal_battlefield_dest;
use crate::combinators::{
    anaphora, any_card_flt, box_anaphora, grp, ins_choose_group, ins_delayed, ins_discard,
    ins_draw, ins_exchange, ins_exile, ins_exile_top, ins_fe_subj, ins_for_each_player,
    ins_gain_control, ins_grant_cast, ins_if, ins_look_top, ins_manifest, ins_move, ins_move_cnt,
    ins_opt_player, ins_opt1, ins_pile_action, ins_return_simple, ins_sac, ins_shuffle_into,
    ins_temp_static, ins_unless, obj_type, some1, split_once_any, strip_prefix_any,
    strip_suffix_any, val_div, val_player_stat, vconst,
};
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_value_expr,
};
use crate::preprocess::parse_base_pt;
use crate::vocab::{Vocabulary, parse_number_word};

pub(super) fn try_parse_exile(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    if let Some(after) = strip_prefix_any(&low, &["exile ", "exiles "]) {
        if !after.contains(" from ")
            && !after.contains(" in ")
            && let Some(whose) = strip_suffix_any(&after, &[" graveyards", " graveyard"])
        {
            return Some(InstructionAst::ExileGraveyard {
                whose: whose
                    .trim_end_matches("'s")
                    .trim_end_matches('\'')
                    .to_string(),
            });
        }
        if let Some(subj_raw) = after.strip_suffix(" until ~ leaves the battlefield")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::ExileUntilLeaves {
                subject: Box::new(subj),
            });
        }
        if let Some((subj_raw, cnt_rest)) = after.split_once(" with ")
            && let Some(left) = strip_suffix_any(&cnt_rest, &[" counters on it", " counter on it"])
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (cnt, kind) = peel_leading_count(left, vocab);
            return Some(InstructionAst::ExileWithCounters {
                subject: Box::new(subj),
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
                face_down: false,
            });
        }
    }

    if let Some((who, after)) = split_leading_player_verb(low, "exile", "exiles") {
        let actor = if who == PlayerRefAst::You && !low.starts_with("you ") {
            None
        } else {
            Some(who)
        };
        if let Some(whose) = strip_suffix_any(&after, &[" graveyards", " graveyard"])
            && !whose.contains(" from ")
            && !whose.contains(" in ")
        {
            return Some(InstructionAst::ExileGraveyard {
                whose: whose.trim_end_matches("'s").to_string(),
            });
        }
        let (after_fd, face_down) = match after.strip_suffix(" face down") {
            Some(c) => (c, true),
            None => (after, false),
        };
        let (after_fd, random) =
            match strip_suffix_any(&after_fd, &[" chosen at random", " at random"]) {
                Some(c) => (c.trim(), true),
                None => (after_fd, false),
            };
        if let Some(whose_lib) = after_fd.strip_prefix("the bottom card of ")
            && let Some(whose) = strip_suffix_any(&whose_lib, &[" library", " libraries"])
        {
            return Some(ins_exile_top(
                actor,
                format!("{}_bottom", whose.trim_end_matches("'s")),
                ValueExprAst::Const(1),
                face_down,
            ));
        }
        if let Some((subj_raw, rest_c)) = after_fd.split_once(" with a number of ")
            && let Some((kind_raw, eq_raw)) = rest_c.split_once(" counters on it equal to ")
            && let (Ok(subj), Ok(cnt)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_value_expr(eq_raw.trim(), vocab),
            )
        {
            return Some(InstructionAst::ExileWithCounters {
                subject: Box::new(subj),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(cnt),
                face_down,
            });
        }
        if after_fd == "one of them from your graveyard" {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::InZone {
                    subject: box_anaphora("one of them"),
                    zone: ZoneFilter::YourGraveyard,
                }),
                face_down,
                random,
                into_pile: false,
            });
        }
        if let Some((ex_head, types_tail)) =
            after_fd.split_once(" with four or more card types among them")
            && types_tail.trim().is_empty()
            && let Some(flt_raw) = ex_head.strip_prefix("any number of ")
            && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "four_or_more_card_types_among_them".to_string(),
            ));
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(grp(flt)),
                face_down,
                random,
                into_pile: false,
            });
        }
        if let Some((cnt_part, lib_part)) = after_fd.split_once(" cards from the bottom of ")
            && let Some(whose) = lib_part.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
        {
            return Some(ins_exile_top(
                actor,
                format!("{}_bottom", whose.trim_end_matches("'s")),
                cnt,
                face_down,
            ));
        }
        if let Some(rest) = after_fd.strip_prefix("the top ")
            && let Some((cnt_part, whose_lib)) = split_once_any(&rest, &[" cards of ", " card of "])
            && let Some(whose) = whose_lib.strip_suffix(" library")
        {
            let cnt = if cnt_part.is_empty() {
                ValueExprAst::Const(1)
            } else {
                parse_value_expr(cnt_part, vocab).ok()?
            };
            return Some(ins_exile_top(
                actor,
                whose.trim_end_matches("'s").to_string(),
                cnt,
                face_down,
            ));
        }
        if let Some(whose_lib) = after_fd.strip_prefix("the top card of ")
            && let Some(whose) = strip_suffix_any(&whose_lib, &[" library", " libraries"])
        {
            return Some(ins_exile_top(
                actor,
                whose.trim_end_matches("'s").to_string(),
                ValueExprAst::Const(1),
                face_down,
            ));
        }
        if after_fd == "the top card" {
            return Some(ins_exile_top(
                actor,
                "your",
                ValueExprAst::Const(1),
                face_down,
            ));
        }
        if let Some(cnt_raw) = after_fd
            .strip_prefix("the top ")
            .and_then(|r| r.strip_suffix(" cards"))
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            return Some(ins_exile_top(actor, "your", cnt, face_down));
        }
        if after_fd == "your hand" || after_fd == "their hand" {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_fd.to_string())),
                random: false,
                face_down,
                into_pile: false,
            });
        }
        if let Some(subj_raw) = after_fd.strip_suffix(" from it")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(subj),
                random: false,
                face_down,
                into_pile: false,
            });
        }
        if let Some(who_hand) = after_fd.strip_prefix("all cards from ")
            && who_hand.ends_with("'s hand")
        {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(who_hand.to_string())),
                random: false,
                face_down,
                into_pile: false,
            });
        }
        if after_fd.starts_with("all but the bottom card of ") && after_fd.ends_with("'s library") {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_fd.to_string())),
                random: false,
                face_down,
                into_pile: false,
            });
        }
        // "exile <count> cards from the top of <whose> library" / "exile a number of cards from the top of <whose> library equal to <val>"
        if let Some((cnt_part, lib_part)) = split_once_any(
            &after_fd,
            &[" cards from the top of ", " card from the top of "],
        ) {
            if let Some((whose_lib, eq_val)) = lib_part.split_once(" library equal to ")
                && cnt_part == "a number of"
                && let Ok(cnt) = parse_value_expr(eq_val, vocab)
            {
                return Some(ins_exile_top(
                    actor,
                    whose_lib.trim_end_matches("'s").to_string(),
                    cnt,
                    face_down,
                ));
            }
            if let Some(whose) = lib_part.strip_suffix(" library")
                && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
            {
                return Some(ins_exile_top(
                    actor,
                    whose.trim_end_matches("'s").to_string(),
                    cnt,
                    face_down,
                ));
            }
        }
        // "<player> exiles cards from the top of <whose> library until <player> exile(s) <filter>"
        if let Some(rest) = after_fd.strip_prefix("cards from the top of ")
            && let Some((lib_raw, until_rest)) = rest
                .split_once(" library until you ")
                .or_else(|| rest.split_once(" library until they "))
                .or_else(|| rest.split_once(" library until that player "))
        {
            let flt_raw =
                strip_prefix_any(&until_rest, &["exile ", "exiles "]).unwrap_or(until_rest);
            if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                return Some(InstructionAst::ExileOrRevealTopUntil {
                    exile: true,
                    library: lib_raw.trim_end_matches("'s").to_string(),
                    filter: Box::new(flt),
                });
            }
        }
        let (core, delayed) = if let Some(c) =
            after_fd.strip_suffix(" at the beginning of the next end step")
        {
            (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
        } else if let Some(c) = after_fd.strip_suffix(" at the beginning of your next end step") {
            (c, Some((TurnStepAst::EndStep, DelayedStepScope::YourNext)))
        } else if let Some(c) = after_fd.strip_suffix(" at end of combat") {
            (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
        } else {
            (after_fd, None)
        };
        let (cnt, core_nocnt) = peel_leading_count(core, vocab);
        let subj_res = parse_subject_or_target(core, vocab).or_else(|err| {
            if matches!(cnt, ValueExprAst::Const(1)) {
                parse_subject_or_target(core_nocnt, vocab)
            } else {
                Err(err)
            }
        });
        if let Ok(subj) = subj_res {
            let ins = InstructionAst::Exile {
                actor,
                subject: Box::new(subj),
                random,
                face_down,
                into_pile: false,
            };
            return Some(match delayed {
                Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                None => ins,
            });
        }
        if !random
            && !matches!(cnt, ValueExprAst::Const(1))
            && let Ok(subj) = parse_subject_or_target(core_nocnt, vocab)
        {
            let ins = InstructionAst::MoveCounted {
                actor,
                count: Some(Box::new(cnt)),
                up_to: false,
                any_number: false,
                subject: Box::new(subj),
                to: "exile".to_string(),
            };
            return Some(match delayed {
                Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                None => ins,
            });
        }
    }
    None
}

pub(super) fn try_parse_sacrifice(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    if let Some(idx) = low.find(" unless ")
        && let Some(subj_raw) = low[..idx].strip_prefix("sacrifice ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let rhs = &low[idx + 8..];
        if let Some(spent_raw) = rhs.strip_suffix(" was spent to cast it") {
            return Some(ins_if(
                ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
                    spent_raw.trim().to_string(),
                ))),
                vec![ins_sac(PlayerRefAst::You, subj)],
            ));
        }
        if let Some(pay_raw) = rhs.strip_prefix("you pay ")
            && let Ok(cost) = parse_cost_atom(&s[idx + 8 + 8..], vocab)
        {
            let _ = pay_raw;
            return Some(InstructionAst::SacrificeUnlessPay {
                subject: Box::new(subj),
                cost,
            });
        }
        if rhs.starts_with("you ")
            && let Ok(cost) = parse_cost_atom(&s[idx + 8 + 4..], vocab)
        {
            return Some(InstructionAst::SacrificeUnlessPay {
                subject: Box::new(subj),
                cost,
            });
        }
        if let Ok(cond) = parse_condition(rhs, vocab) {
            return Some(ins_if(
                ConditionAst::Not(Box::new(cond)),
                vec![ins_sac(PlayerRefAst::You, subj)],
            ));
        }
    }
    // "<subj>'s controller sacrifices it" (Arcum Dagsson, Reality Acid, Soul Ransom)
    if let Some(subj_raw) = low.strip_suffix("'s controller sacrifices it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(ins_sac(PlayerRefAst::ThatObjectController, subj));
    }
    // "<player> chooses and sacrifices <subj>" (Barrin's Spite, Retribution)
    if let Some((who, after)) =
        split_leading_player_verb(low, "choose and sacrifice", "chooses and sacrifices")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Some(ins_sac(who, subj));
    }
    if let Some((who, after)) = split_leading_player_verb(low, "sacrifice", "sacrifices") {
        let (core, delayed_step) =
            if let Some(c) = after.strip_suffix(" at the beginning of the next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
            } else if let Some(c) = after.strip_suffix(" at the beginning of your next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::YourNext)))
            } else if let Some(c) = after.strip_suffix(" at end of combat") {
                (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
            } else {
                (after, None)
            };
        if let Some(any_rest) = core.strip_prefix("any number of ")
            && let Ok(subj) = parse_subject_or_target(any_rest, vocab)
        {
            let ins = InstructionAst::SacrificeAnyNumber {
                player: who,
                subject: Box::new(subj),
            };
            return Some(match delayed_step {
                Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                None => ins,
            });
        }
        let (core, rounding) = if let Some(c) = core.strip_suffix(", rounded up") {
            (c.trim(), Some(Rounding::Up))
        } else if let Some(c) = core.strip_suffix(", rounded down") {
            (c.trim(), Some(Rounding::Down))
        } else {
            (core, None)
        };
        let (core, chooser) = if let Some(c) = core.strip_suffix(" of an opponent's choice") {
            (c.trim(), Some(PlayerRefAst::AnyOpponent))
        } else if core.ends_with(" in the pile of their choice")
            || core.ends_with(" in the pile of his or her choice")
        {
            (core, None)
        } else {
            (
                strip_suffix_any(&core, &[" of their choice", " of his or her choice"])
                    .unwrap_or(core),
                None,
            )
        };
        let (core, except) =
            if let Some((l, r)) = split_once_any(&core, &[", except for ", " except for "]) {
                match parse_subject_or_target(r.trim(), vocab) {
                    Ok(exc) => (l.trim(), Some(Box::new(exc))),
                    Err(_) => (core, None),
                }
            } else {
                (core, None)
            };
        if core.contains(" except for ") {
            return None;
        }
        let (core, half) = if let Some(c) = strip_prefix_any(&core, &["half of the ", "half the "])
        {
            (c.trim(), true)
        } else {
            (core, false)
        };
        let (parsed_subj, parsed_count) = if let Some(pile_inner) = core
            .strip_suffix(" in the pile of their choice")
            .or_else(|| core.strip_suffix(" in the pile of his or her choice"))
            && let Ok(flt) = parse_object_filter(
                pile_inner
                    .trim()
                    .strip_prefix("all ")
                    .or_else(|| pile_inner.trim().strip_prefix("the "))
                    .unwrap_or(pile_inner.trim()),
                vocab,
            ) {
            (
                Some(SubjectRefAst::PileRef(PileRefAst::FilteredInPile {
                    filter: Box::new(flt),
                    pile: Box::new(PileRefAst::ChosenPile),
                })),
                None,
            )
        } else if let Ok(subj) = parse_subject_or_target(core, vocab) {
            (Some(subj), None)
        } else {
            let (cnt, core_nocnt) = peel_leading_count(core, vocab);
            let core_subj = core_nocnt.strip_prefix("of ").unwrap_or(core_nocnt);
            match parse_subject_or_target(core_subj, vocab) {
                Ok(subj) => (
                    Some(subj),
                    if matches!(cnt, ValueExprAst::Const(1)) {
                        None
                    } else {
                        Some(Box::new(cnt))
                    },
                ),
                Err(_) => (None, None),
            }
        };
        if let Some(subj) = parsed_subj {
            let ins = InstructionAst::Sacrifice {
                player: who,
                count: parsed_count,
                subject: Box::new(subj),
                except,
                half,
                rounding,
                chooser,
            };
            return Some(match delayed_step {
                Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                None => ins,
            });
        }
    }
    None
}

pub(super) fn try_parse_discard(s: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    let low = s.to_ascii_lowercase();
    let (who, after) = split_leading_player_verb(&low, "discard", "discards")?;
    if let Some((left_part, unless_raw)) = after.split_once(" unless you discard ")
        && let Ok(flt) = parse_object_filter(unless_raw, vocab)
    {
        let (cnt, _) = peel_leading_count(left_part, vocab);
        return Some(InstructionAst::DiscardUnless {
            player: who,
            count: Box::new(cnt),
            unless_filter: Box::new(flt),
        });
    }
    if let Some((left_part, unless_raw)) = after.split_once(" unless you ")
        && let Ok(cost) = parse_cost_atom(unless_raw, vocab)
    {
        let (cnt, _) = peel_leading_count(left_part, vocab);
        return Some(ins_unless(
            vec![ins_discard(who, Some(cnt))],
            Some(PlayerRefAst::You),
            Some(cost),
            None,
            None,
        ));
    }
    if let Some(fe) = strip_prefix_any(&after, &["a card for each ", "one card for each "])
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return Some(ins_discard(who, Some(cnt)));
    }
    if matches!(after, "your hand" | "their hand" | "his or her hand") {
        return Some(InstructionAst::DiscardHand { player: who });
    }
    if let Some(after_half) = after.strip_prefix("half the cards in ")
        && (after_half.starts_with("their hand") || after_half.starts_with("your hand"))
    {
        let rounding = if after_half.contains("rounded up") {
            crate::ast::Rounding::Up
        } else {
            crate::ast::Rounding::Down
        };
        return Some(ins_discard(
            who,
            Some(val_div(
                val_player_stat(who, crate::ast::PlayerStatKind::CardsInHand),
                2,
                rounding,
            )),
        ));
    }
    let (core, random) = match after.strip_suffix(" at random") {
        Some(c) => (c, true),
        None => (after, false),
    };
    if core.starts_with("that ")
        || core.starts_with("those ")
        || matches!(
            core,
            "it" | "them"
                | "one of them"
                | "two of them"
                | "one of those cards"
                | "two of those cards"
        )
    {
        return Some(InstructionAst::Discard {
            player: who,
            count: None,
            filter: None,
            anaphora: Some(core.to_string()),
            random,
        });
    }
    if let Some(val_raw) = core.strip_prefix("a number of cards equal to ")
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::Discard {
            player: who,
            count: Some(Box::new(cnt)),
            filter: None,
            anaphora: None,
            random,
        });
    }
    let (core, is_up_to, is_any_num, is_all) = if let Some(c) = core.strip_prefix("up to ") {
        (c, true, false, false)
    } else if let Some(c) = core.strip_prefix("any number of ") {
        (c, false, true, false)
    } else if let Some(c) = core.strip_prefix("all ") {
        (c, false, false, true)
    } else {
        (core, false, false, false)
    };
    let (cnt, flt_raw) = peel_leading_count(core, vocab);
    let flt = parse_object_filter(flt_raw, vocab).ok()?;
    if is_up_to || is_any_num || is_all {
        return Some(InstructionAst::DiscardUpTo {
            player: who,
            max_count: Box::new(cnt),
            filter: Some(Box::new(flt)),
            any_number: is_any_num,
            all: is_all,
            random,
        });
    }
    Some(InstructionAst::Discard {
        player: who,
        count: Some(Box::new(cnt)),
        filter: Some(Box::new(flt)),
        anaphora: None,
        random,
    })
}

pub(super) fn try_parse_zone_moves(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    // ReturnZone ("return ..." or "<player> return(s) ..." or inverted "return to <dest> <subj>")
    if let Some((_, after)) = split_leading_player_verb(low, "return", "returns") {
        let (core, delayed) =
            if let Some(c) = after.strip_suffix(" at the beginning of the next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
            } else if let Some(c) = after.strip_suffix(" at the beginning of your next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::YourNext)))
            } else if let Some(c) = after
                .strip_suffix(" at the beginning of that player's next end step")
                .or_else(|| after.strip_suffix(" at the beginning of their next end step"))
            {
                (
                    c,
                    Some((TurnStepAst::EndStep, DelayedStepScope::ThatPlayersNext)),
                )
            } else if let Some(c) = after.strip_suffix(" at the beginning of the next upkeep") {
                (c, Some((TurnStepAst::Upkeep, DelayedStepScope::Next)))
            } else if let Some(c) = after.strip_suffix(" at the beginning of your next upkeep") {
                (c, Some((TurnStepAst::Upkeep, DelayedStepScope::YourNext)))
            } else if let Some(c) = after
                .strip_suffix(" at the beginning of their next upkeep")
                .or_else(|| after.strip_suffix(" at the beginning of that player's next upkeep"))
            {
                (
                    c,
                    Some((TurnStepAst::Upkeep, DelayedStepScope::ThatPlayersNext)),
                )
            } else if let Some(c) = after.strip_suffix(" at end of combat") {
                (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
            } else {
                (after, None)
            };
        let (core, except) =
            if let Some((l, r)) = split_once_any(&core, &[", except for ", " except for "]) {
                match parse_subject_or_target(r.trim(), vocab) {
                    Ok(exc) => (l.trim(), Some(Box::new(exc))),
                    Err(_) => (core, None),
                }
            } else {
                (core, None)
            };
        if core.contains(" except for ") {
            return None;
        }
        // Inverted order (longest destination prefix first):
        // "return to the battlefield [tapped] [under ... control] <subj>" / "return to their/your/its owner's hand <subj>"
        for (inv_pref, dest, tapped) in [
            (
                "to the battlefield tapped under your control ",
                "the battlefield tapped under your control",
                true,
            ),
            (
                "to the battlefield tapped under its owner's control ",
                "the battlefield tapped under its owner's control",
                true,
            ),
            (
                "to the battlefield tapped under their owners' control ",
                "the battlefield tapped under their owners' control",
                true,
            ),
            (
                "to the battlefield tapped under their owner's control ",
                "the battlefield tapped under their owner's control",
                true,
            ),
            (
                "to the battlefield under your control tapped ",
                "the battlefield tapped under your control",
                true,
            ),
            (
                "to the battlefield under its owner's control tapped ",
                "the battlefield tapped under its owner's control",
                true,
            ),
            (
                "to the battlefield under your control ",
                "the battlefield under your control",
                false,
            ),
            (
                "to the battlefield under its owner's control ",
                "the battlefield under its owner's control",
                false,
            ),
            (
                "to the battlefield under their owners' control ",
                "the battlefield under their owners' control",
                false,
            ),
            (
                "to the battlefield under their owner's control ",
                "the battlefield under their owner's control",
                false,
            ),
            ("to the battlefield tapped ", "the battlefield tapped", true),
            ("to the battlefield ", "the battlefield", false),
            ("to its owner's hand ", "its owner's hand", false),
            ("to their owners' hands ", "their owners' hands", false),
            ("to their owner's hand ", "their owner's hand", false),
            ("to their hand ", "their hand", false),
            ("to your hand ", "your hand", false),
            ("to that player's hand ", "that player's hand", false),
        ] {
            if let Some(subj_raw) = core.strip_prefix(inv_pref) {
                let (subj_raw, random) = match subj_raw.strip_suffix(" at random") {
                    Some(c) => (c.trim(), true),
                    None => (subj_raw, false),
                };
                let (subj_clean, any_number, up_to, count_opt) =
                    if let Some(r) = subj_raw.strip_prefix("any number of ") {
                        if r.starts_with("target ") || r.starts_with("other target ") {
                            (subj_raw, false, false, None)
                        } else {
                            (r, true, false, None)
                        }
                    } else if let Some(r) = subj_raw.strip_prefix("up to ") {
                        if r.contains(" target ") {
                            (subj_raw, false, false, None)
                        } else {
                            let (c, rem) = peel_leading_count(r, vocab);
                            (rem, false, true, Some(Box::new(c)))
                        }
                    } else {
                        (subj_raw, false, false, None)
                    };
                let parsed = if let Ok(s) = parse_subject_or_target(subj_clean, vocab) {
                    Some((s, None))
                } else if let Some((sr, fz)) = subj_clean.split_once(" from ")
                    && let Ok(s) = parse_subject_or_target(sr, vocab)
                {
                    Some((s, Some(fz.to_string())))
                } else {
                    None
                };
                if let Some((subj, from_zone)) = parsed {
                    let ins = InstructionAst::ReturnZone {
                        subject: Box::new(subj),
                        from_zone,
                        to: dest.to_string(),
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        random,
                        except,
                        count: count_opt,
                        up_to,
                        any_number,
                        actor: None,
                    };
                    return Some(match delayed {
                        Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                        None => ins,
                    });
                }
            }
        }
        let (core_noatt, attached_to) = if let Some((before, att_raw)) =
            core.rsplit_once(" attached to ")
            && let Ok(att) = parse_subject_or_target(att_raw, vocab)
        {
            (before, Some(Box::new(att)))
        } else {
            (core, None)
        };
        let (core_nocnt, with_counter) = if let Some((before, after_w)) =
            core_noatt.rsplit_once(" with ")
            && let Some(kind) = strip_suffix_any(&after_w, &[" counter on it", " counters on it"])
        {
            let kind_clean = strip_prefix_any(kind.trim(), &["a ", "an ", "one "])
                .unwrap_or(kind.trim())
                .trim();
            (
                before,
                (!kind_clean.is_empty()).then(|| kind_clean.to_string()),
            )
        } else {
            (core_noatt, None)
        };
        // "return <subj> under your control [with a finality counter on it]" (Meathook Massacre II)
        if let Some(subj_raw) = core_nocnt.strip_suffix(" under your control")
            && !subj_raw.contains(" to ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let ins = InstructionAst::ReturnZone {
                subject: Box::new(subj),
                from_zone: None,
                to: "the battlefield under your control".to_string(),
                tapped: false,
                transformed: false,
                with_counter,
                attached_to,
                random: false,
                except,
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            };
            return Some(match delayed {
                Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                None => ins,
            });
        }
        if let Some((left, dest)) = core_nocnt.rsplit_once(" to ") {
            const VALID_DESTS: &[&str] = &[
                "its owner's hand",
                "his owner's hand",
                "her owner's hand",
                "their owner's hand",
                "their owners' hands",
                "their hand",
                "your hand",
                "that player's hand",
                "their owner's graveyard",
                "their owners' graveyards",
                "its owner's graveyard",
                "the command zone",
                "the top of its owner's library",
                "the bottom of its owner's library",
            ];
            let valid_bf = is_orthogonal_battlefield_dest(dest);
            if valid_bf || VALID_DESTS.contains(&dest) {
                let (left_norand, rand_before_to) = match left.strip_suffix(" at random") {
                    Some(c) => (c.trim(), true),
                    None => (left, false),
                };
                let (subj_raw, from_zone) = match left_norand.split_once(" from ") {
                    Some((sr, fz)) => (sr, Some(fz.to_string())),
                    None => (left_norand, None),
                };
                let (subj_raw, except_inline) = if except.is_none()
                    && let Some((sr, ex_raw)) = subj_raw.split_once(" except ")
                    && let Ok(ex_subj) = parse_subject_or_target(ex_raw.trim(), vocab)
                {
                    (sr.trim(), Some(Box::new(ex_subj)))
                } else {
                    (subj_raw, None)
                };
                let final_except = except.or(except_inline);
                let (subj_raw, rand_in_subj) = match subj_raw.strip_suffix(" at random") {
                    Some(c) => (c.trim(), true),
                    None => (subj_raw, false),
                };
                let random = rand_before_to || rand_in_subj;
                let (subj_clean, any_number, up_to, explicit_cnt) =
                    if let Some(r) = subj_raw.strip_prefix("any number of ") {
                        if r.starts_with("target ") || r.starts_with("other target ") {
                            (subj_raw, false, false, None)
                        } else {
                            (r, true, false, None)
                        }
                    } else if let Some(r) = subj_raw.strip_prefix("up to ") {
                        if r.contains(" target ") {
                            (subj_raw, false, false, None)
                        } else {
                            let (c, rem) = peel_leading_count(r, vocab);
                            (rem, false, true, Some(Box::new(c)))
                        }
                    } else {
                        (subj_raw, false, false, None)
                    };
                let (peeled_cnt, subj_nocnt) = peel_leading_count(subj_clean, vocab);
                let (subj_res, final_cnt) =
                    if let Ok(s) = parse_subject_or_target(subj_clean, vocab) {
                        (Ok(s), explicit_cnt)
                    } else if let Ok(s) = parse_subject_or_target(subj_nocnt, vocab) {
                        let c = if explicit_cnt.is_some() {
                            explicit_cnt
                        } else if matches!(peeled_cnt, ValueExprAst::Const(1)) {
                            None
                        } else {
                            Some(Box::new(peeled_cnt))
                        };
                        (Ok(s), c)
                    } else {
                        (Err(String::new()), explicit_cnt)
                    };
                if let Ok(subj) = subj_res {
                    let ins = InstructionAst::ReturnZone {
                        subject: Box::new(subj),
                        from_zone,
                        to: dest.to_string(),
                        tapped: dest.contains("tapped"),
                        transformed: dest.contains("transformed") || dest.contains("converted"),
                        with_counter,
                        attached_to,
                        random,
                        except: final_except,
                        count: final_cnt,
                        up_to,
                        any_number,
                        actor: None,
                    };
                    return Some(match delayed {
                        Some((step, scope)) => ins_delayed(step, scope, vec![ins]),
                        None => ins,
                    });
                }
            }
        }
    }

    // ChooseAndKeep ("... and the rest ..." / "... and the other ..." including "put one back and the rest into ...")
    if let Some((_, after)) = split_leading_player_verb(low, "put", "puts")
        && let Some((keep_part, rest_part)) =
            split_once_any(&after, &[" and the rest ", " and the other "])
    {
        let (up_to_pref, keep_clean) = if let Some(r) = keep_part.strip_prefix("up to ") {
            ("up to ", r)
        } else {
            ("", keep_part)
        };
        for pick in [
            "one of them",
            "two of them",
            "three of them",
            "one of those cards",
            "two of those cards",
            "three of those cards",
            "one pile",
            "that pile",
            "one",
            "that card",
            "a card from among them",
        ] {
            if let Some(after_pick) = keep_clean.strip_prefix(pick) {
                let ap = after_pick.trim();
                let keep_to = if ap == "back" {
                    "top of library"
                } else if let Some(b) = ap.strip_prefix("back on ") {
                    b
                } else {
                    strip_prefix_any(&ap, &["into ", "onto ", "on "]).unwrap_or("")
                };
                let rest_to =
                    strip_prefix_any(&rest_part, &["into ", "on ", "onto "]).unwrap_or("");
                if !keep_to.is_empty() && !rest_to.is_empty() {
                    return Some(InstructionAst::ChooseAndKeep {
                        pick: format!("{up_to_pref}{pick}"),
                        keep_to: keep_to.to_string(),
                        rest_to: rest_to.to_string(),
                    });
                }
            }
        }
        // Mulch-style: "put all <filter> revealed this way into <dest1> and the rest into <dest2>"
        if let Some((flt_part, keep_to)) = split_once_any(&keep_part, &[" into ", " onto "])
            && let Some(rest_to) = strip_prefix_any(&rest_part, &["into ", "on "])
            && parse_subject_or_target(flt_part, vocab).is_ok()
        {
            return Some(InstructionAst::ChooseAndKeep {
                pick: flt_part.to_string(),
                keep_to: keep_to.to_string(),
                rest_to: rest_to.to_string(),
            });
        }
    }

    // RevealFromLooked
    if let Some(after) = low.strip_prefix("reveal ")
        && let Some((flt_raw, dest_part)) = after.split_once(" from among them and put ")
    {
        let trimmed = flt_raw.trim();
        let (pick_prefix, rest_after_upto) = if let Some(r) = trimmed.strip_prefix("up to ") {
            ("up to ", r)
        } else {
            ("", trimmed)
        };
        let (cnt_ast, clean_flt) = peel_leading_count(rest_after_upto, vocab);
        let count_tag = if trimmed.starts_with("any number ") {
            "any number ".to_string()
        } else if trimmed.starts_with("all ") {
            "all ".to_string()
        } else if let ValueExprAst::Const(n) = cnt_ast
            && n >= 2
        {
            format!("{pick_prefix}{n} ")
        } else {
            String::new()
        };
        if let Ok(flt) = parse_object_filter(clean_flt, vocab) {
            return Some(InstructionAst::RevealFromLooked {
                filter: Box::new(flt),
                to: format!("{count_tag}{dest_part}"),
            });
        }
    }

    // "<subj>'s owner puts it on ..." / "Its owner puts it on ..." / "The owner of <subj> puts it on/into ..."
    if low == "its owner puts it on their choice of the top or bottom of their library" {
        return Some(ins_move(
            anaphora("it"),
            "owner_choice_top_or_bottom_of_library",
        ));
    }
    if let Some((subj_raw, rest)) = low.split_once("'s owner puts it on ")
        && rest == "their choice of the top or bottom of their library"
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(ins_move(subj, "owner_choice_top_or_bottom_of_library"));
    }
    if let Some(after) = low.strip_prefix("the owner of ")
        && let Some((subj_raw, rest)) = split_once_any(&after, &[" puts it on ", " puts it into "])
        && (rest == "their choice of the top or bottom of their library"
            || rest == "their library second from the top or on the bottom")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(ins_move(subj, rest.to_string()));
    }

    // "shuffle and put <subj> on top [of your library]" (Mwonvuli Beast Tracker)
    if let Some(after) = low.strip_prefix("shuffle and put ")
        && let Some(subj_raw) = strip_suffix_any(&after, &[" on top", " on top of your library"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(ins_move(subj, "top_of_library_after_shuffle"));
    }

    // MoveZone: "[<player>] put(s) <subj> onto/into/on <dest>" (or inverted "put onto/into <dest> <subj>")
    if let Some((_, after)) = split_leading_player_verb(low, "put", "puts") {
        let after_norm = after
            .strip_suffix(" from the command zone")
            .unwrap_or(after)
            .replace(" back on top of ", " on top of ");
        for (inv_pref, dest) in [
            (
                "onto the battlefield under your control ",
                "the battlefield under your control",
            ),
            ("onto the battlefield tapped ", "the battlefield tapped"),
            ("onto the battlefield ", "the battlefield"),
            ("into your hand ", "your hand"),
            ("into your graveyard ", "your graveyard"),
        ] {
            if let Some(subj_raw) = after_norm.strip_prefix(inv_pref)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                return Some(ins_move(subj, dest.to_string()));
            }
        }
        for sep in [" onto ", " into ", " on "] {
            if let Some((obj_raw, dest)) = after_norm.rsplit_once(sep) {
                const MOVE_DESTS: &[&str] = &[
                    "the battlefield",
                    "the battlefield tapped",
                    "the battlefield tapped and attacking",
                    "the battlefield tapped and attacking that player",
                    "the battlefield under your control",
                    "the battlefield tapped under your control",
                    "the battlefield under the control of that card's owner",
                    "the battlefield face down under your control",
                    "the battlefield face up",
                    "the battlefield face up or face down",
                    "the battlefield or into your hand",
                    "your hand",
                    "its owner's hand",
                    "their owner's hand",
                    "their owners' hands",
                    "that player's hand",
                    "their hand",
                    "top",
                    "the top",
                    "the bottom",
                    "the top of your library",
                    "top of your library",
                    "top of your library in any order",
                    "the top of your library in any order",
                    "the top of their library",
                    "top of their library",
                    "top of their library in any order",
                    "the top of their library in any order",
                    "the top of that player's library",
                    "top of that player's library",
                    "top of that player's library in any order",
                    "the top of that player's library in any order",
                    "the top of that library",
                    "top of that library",
                    "top of that library in any order",
                    "the bottom of your library",
                    "bottom of your library",
                    "the bottom of your library in a random order",
                    "the bottom of your library in any order",
                    "the bottom of their library",
                    "bottom of their library",
                    "the bottom of their library in a random order",
                    "the bottom of their library in any order",
                    "the bottom of that library",
                    "the bottom of that library in a random order",
                    "the bottom of that library in any order",
                    "the bottom in a random order",
                    "the bottom in any order",
                    "the bottom of that player's library",
                    "the bottom of that player's library in a random order",
                    "the bottom of that player's library in any order",
                    "the top of its owner's library",
                    "top of its owner's library",
                    "the top of his owner's library",
                    "top of his owner's library",
                    "the top of her owner's library",
                    "top of her owner's library",
                    "the top of their owners' libraries",
                    "top of their owners' libraries",
                    "top of their owners' libraries in any order",
                    "the bottom of its owner's library",
                    "bottom of its owner's library",
                    "the bottom of his owner's library",
                    "bottom of his owner's library",
                    "the bottom of her owner's library",
                    "bottom of her owner's library",
                    "the bottom of their owners' libraries",
                    "bottom of their owners' libraries",
                    "the bottom of their owners' libraries in any order",
                    "the bottom of their owners' libraries in a random order",
                    "its owner's library second from the top",
                    "its owner's library third from the top",
                    "his owner's library second from the top",
                    "his owner's library third from the top",
                    "her owner's library second from the top",
                    "her owner's library third from the top",
                    "your library second from the top",
                    "your library third from the top",
                    "your graveyard",
                    "their graveyard",
                    "their graveyards",
                    "their owners' graveyards",
                    "their hands",
                    "its owner's graveyard",
                    "that player's graveyard",
                    "their owner's graveyard",
                    "your choice of the top or bottom of its owner's library",
                    "their choice of the top or bottom of its owner's library",
                ];
                let is_lib_depth_dest = dest.ends_with(" from the top")
                    || dest.starts_with("its owner's library just beneath the top ")
                    || dest.starts_with("their library just beneath the top ")
                    || dest.starts_with("your library just beneath the top ");
                // Compound "put <pick> onto/into <dest1> and the rest <dest2>" (Genesis Ultimatum, Green Sun's Twilight)
                if let Some((dest1_raw, rest_dest)) = dest.split_once(" and the rest ") {
                    let dest1 = dest1_raw.trim();
                    if (is_orthogonal_battlefield_dest(dest1) || MOVE_DESTS.contains(&dest1))
                        && (rest_dest.starts_with("into ")
                            || rest_dest.starts_with("on ")
                            || rest_dest.starts_with("onto "))
                    {
                        return Some(InstructionAst::ChooseAndKeep {
                            pick: obj_raw.trim().to_string(),
                            keep_to: dest1.to_string(),
                            rest_to: rest_dest.trim().to_string(),
                        });
                    }
                }
                let (dest_clean, _att_opt) = match dest.rsplit_once(" attached to ") {
                    Some((d_before, att_raw))
                        if parse_subject_or_target(att_raw, vocab).is_ok() =>
                    {
                        (d_before.trim(), Some(att_raw))
                    }
                    _ => (dest, None),
                };
                if is_orthogonal_battlefield_dest(dest_clean)
                    || MOVE_DESTS.contains(&dest_clean)
                    || is_lib_depth_dest
                {
                    let obj_clean = obj_raw.trim();
                    // Multi-zone source ("put <subj> from <z1> (and|or) from <z2> onto/into <dest>")
                    // Guard strictly so intermediate destinations/clauses ("from gy onto bf, then put ... from hand")
                    // are NEVER swallowed into `from_zone`.
                    if let Some((subj_part, from_zones)) = obj_clean.split_once(" from ")
                        && from_zones.contains(" from ")
                        && is_clean_multi_zone_source(from_zones)
                        && let Ok(subj) = parse_subject_or_target(subj_part.trim(), vocab)
                    {
                        return Some(InstructionAst::ReturnZone {
                            subject: Box::new(subj),
                            from_zone: Some(from_zones.trim().to_string()),
                            to: dest.to_string(),
                            tapped: dest.contains("tapped"),
                            transformed: dest.contains("transformed") || dest.contains("converted"),
                            with_counter: None,
                            attached_to: None,
                            random: false,
                            except: None,
                            count: None,
                            up_to: false,
                            any_number: false,
                            actor: None,
                        });
                    }
                    if let Some(any_rest) = obj_clean.strip_prefix("any number of ") {
                        if let Ok(t @ SubjectRefAst::TargetObject { .. }) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(ins_move(t, dest.to_string()));
                        }
                        if let Ok(subj) = parse_subject_or_target(any_rest, vocab) {
                            return Some(ins_move_cnt(
                                None,
                                None,
                                false,
                                true,
                                subj,
                                dest.to_string(),
                            ));
                        }
                        return None;
                    }
                    if let Some(up_rest) = obj_clean.strip_prefix("up to ") {
                        if let Ok(t @ SubjectRefAst::TargetObject { .. }) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(ins_move(t, dest.to_string()));
                        }
                        let (cnt, up_nocnt) = peel_leading_count(up_rest, vocab);
                        if let Ok(subj) = parse_subject_or_target(up_nocnt, vocab) {
                            return Some(ins_move_cnt(
                                None,
                                Some(cnt),
                                true,
                                false,
                                subj,
                                dest.to_string(),
                            ));
                        }
                        return None;
                    }
                    if obj_clean.starts_with("one of ") {
                        if let Ok(a @ SubjectRefAst::Anaphora(_)) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(ins_move(a, dest.to_string()));
                        }
                        return None;
                    }
                    let (cnt, obj_nocnt) = peel_leading_count(obj_clean, vocab);
                    let subj = if obj_clean == "the rest" || obj_clean == "the rest of the cards" {
                        anaphora("the_rest")
                    } else if let Ok(s) = parse_subject_or_target(obj_clean, vocab) {
                        s
                    } else if matches!(cnt, ValueExprAst::Const(1)) {
                        parse_subject_or_target(obj_nocnt, vocab).ok()?
                    } else if let Ok(s) = parse_subject_or_target(obj_nocnt, vocab) {
                        return Some(ins_move_cnt(
                            None,
                            Some(cnt),
                            false,
                            false,
                            s,
                            dest.to_string(),
                        ));
                    } else {
                        return None;
                    };
                    return Some(ins_move(subj, dest.to_string()));
                }
            }
        }
    }

    None
}

/// Extract embedded base power/toughness and keyword lists from named or quoted
/// animation descriptions such as "legendary 0/0 Elemental creature with haste named Vitu-Ghazi".
pub(super) fn parse_named_or_quoted_animation_fields(
    rest: &str,
    vocab: &Vocabulary,
) -> (String, String, Vec<String>) {
    let non_quoted = rest.split_once('"').map_or(rest, |(head, _)| head);
    let low = non_quoted.to_ascii_lowercase();
    let (before_named, after_named) = match low.split_once(" named ") {
        Some((head, tail)) => (head.trim(), Some(tail.trim())),
        None => (low.trim(), None),
    };
    let mut pt = before_named
        .split_whitespace()
        .find_map(|tok| parse_base_pt(tok.trim_end_matches(',')));
    if pt.is_none()
        && let Some(tail) = after_named
        && let Some((_, after_bpt)) = tail.split_once("base power and toughness ")
        && let Some(tok) = after_bpt.split_whitespace().next()
    {
        pt = parse_base_pt(tok.trim_end_matches(','));
    }
    let keywords = before_named
        .split_once(" with ")
        .and_then(|(_, kw_part)| parse_keyword_list(kw_part.trim(), vocab))
        .unwrap_or_default();
    match pt {
        Some((power, toughness)) => (power, toughness, keywords),
        None => (String::new(), String::new(), keywords),
    }
}

/// Parse a single search-card clause, handling both "<type> named <name>" and
/// regular object filters while refusing compound search phrases inside a single name.
fn parse_search_single_filter(flt_raw: &str, vocab: &Vocabulary) -> Option<ObjectFilterAst> {
    if let Some((type_part, name_raw)) = flt_raw.split_once(" named ")
        && !type_part.trim_end().ends_with(" not")
        && type_part.trim() != "not"
        && !type_part.trim_end().ends_with(" for cards")
        && !type_part.trim_end().ends_with(" drafted cards")
    {
        let n = name_raw.trim();
        if n.is_empty()
            || n.contains(" and/or a ")
            || n.contains(" and/or an ")
            || n.contains(" or a ")
            || n.contains(" or an ")
            || n.contains(" and a ")
            || n.contains(" and an ")
        {
            return None;
        }
        let mut base_flt = if matches!(type_part.trim(), "card" | "cards") {
            any_card_flt()
        } else {
            parse_object_filter(type_part.trim(), vocab).ok()?
        };
        let nf = if n == "~" {
            NameFilterAst::SelfName
        } else {
            NameFilterAst::Exact(n.to_string())
        };
        base_flt.named = Some(nf);
        Some(base_flt)
    } else {
        parse_object_filter(flt_raw, vocab).ok()
    }
}

pub(super) fn try_parse_search_library(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    let low_owned = if low.contains(" which have ") {
        low.replace(" which have ", " that have ")
    } else {
        low.to_string()
    };
    let low = low_owned
        .strip_prefix("then ")
        .unwrap_or(low_owned.as_str());
    let (opt_player, leading_subject_player, after) = if let Some(a) = low.strip_prefix("search ") {
        (None, None, a)
    } else if let Some((who_raw, a)) = split_once_any(&low, &[" may each search ", " may search "])
    {
        if let Some(p) = parse_player_ref(who_raw) {
            (Some(p), None, a)
        } else if who_raw.starts_with("each player who ") || who_raw.starts_with("any number of ") {
            (
                Some(PlayerRefAst::EachPlayer),
                Some(SubjectRefAst::Anaphora(who_raw.trim().to_string())),
                a,
            )
        } else {
            return None;
        }
    } else if let Some((who_raw, a)) = split_once_any(&low, &[" searches ", " search "]) {
        let _ = parse_player_ref(who_raw)?;
        (None, None, a)
    } else {
        return None;
    };

    // Cluster 7 + Wave E2: Support single-zone, multi-zone (with or without Oxford comma), and pile searches
    let (lib_who, extra_zones, rest_raw) = if let Some((lw, r)) = after
        .split_once(" library, graveyard, and/or outside the game for ")
        .or_else(|| after.split_once(" library, graveyard and/or outside the game for "))
    {
        (lw, vec![ZoneFilter::YourGraveyard, ZoneFilter::Exile], r)
    } else if let Some((lw, r)) = after
        .split_once(" library, graveyard, and/or hand for ")
        .or_else(|| after.split_once(" library, graveyard and/or hand for "))
    {
        (lw, vec![ZoneFilter::YourGraveyard, ZoneFilter::YourHand], r)
    } else if let Some((lw, r)) = after
        .split_once(" library, hand, and/or graveyard for ")
        .or_else(|| after.split_once(" library, hand and/or graveyard for "))
    {
        (lw, vec![ZoneFilter::YourHand, ZoneFilter::YourGraveyard], r)
    } else if let Some((lw, r)) = after
        .split_once(" graveyard, hand, and/or library for ")
        .or_else(|| after.split_once(" graveyard, hand and/or library for "))
    {
        (lw, vec![ZoneFilter::YourGraveyard, ZoneFilter::YourHand], r)
    } else if let Some((lw, r)) = after
        .split_once(" graveyard, hand, and library for ")
        .or_else(|| after.split_once(" graveyard, hand and library for "))
    {
        (lw, vec![ZoneFilter::YourGraveyard, ZoneFilter::YourHand], r)
    } else if let Some((lw, r)) = after.split_once(" library and/or graveyard for ") {
        (lw, vec![ZoneFilter::YourGraveyard], r)
    } else if let Some((lw, r)) = after.split_once(" library and graveyard for ") {
        (lw, vec![ZoneFilter::YourGraveyard], r)
    } else if let Some((lw, r)) = after.split_once(" library or graveyard for ") {
        (lw, vec![ZoneFilter::YourGraveyard], r)
    } else if let Some((lw, r)) = after.split_once(" graveyard and/or library for ") {
        (lw, vec![ZoneFilter::YourGraveyard], r)
    } else if let Some((lw, r)) = after.split_once(" library and/or hand for ") {
        (lw, vec![ZoneFilter::YourHand], r)
    } else if let Some((lw, r)) = after.split_once(" library and hand for ") {
        (lw, vec![ZoneFilter::YourHand], r)
    } else if let Some((lw, r)) = after.split_once(" hand and/or library for ") {
        (lw, vec![ZoneFilter::YourHand], r)
    } else if let Some((lw, r)) = after.split_once(" library for ") {
        (lw, Vec::new(), r)
    } else {
        let r = after.strip_prefix("the other pile for ")?;
        ("the other pile", Vec::new(), r)
    };

    // Wave E2: Extract inline `, where x is <expr>, ` inside search clauses
    // (Collective Voyage, Jaheira's Respite, Sylvan Primordial, Surveyor's Scope, Oreskos Explorer, Rampant Rejuvenator, Harvest Season, Boundless Realms, Celebrate the Harvest)
    let mut where_x: Option<Box<ValueExprAst>> = None;
    let rest_owned: String;
    let rest: &str = if let Some(wx_idx) = rest_raw.find(", where x is ") {
        let after_wx = &rest_raw[wx_idx + ", where x is ".len()..];
        let mut wx_end: Option<usize> = None;
        for (c_idx, _) in after_wx.match_indices(", ") {
            let candidate = after_wx[..c_idx].trim();
            if let Ok(val) = parse_value_expr(candidate, vocab) {
                where_x = Some(Box::new(val));
                wx_end = Some(c_idx);
                break;
            }
        }
        if where_x.is_none()
            && let Ok(val) = parse_value_expr(after_wx.trim(), vocab)
        {
            where_x = Some(Box::new(val));
            wx_end = Some(after_wx.len());
        }
        if let Some(end_off) = wx_end {
            let tail_after = &after_wx[end_off..];
            rest_owned = format!("{}{}", &rest_raw[..wx_idx], tail_after);
            &rest_owned
        } else {
            rest_raw
        }
    } else {
        rest_raw
    };

    const COMMA_TAIL_STARTERS: &[&str] = &[
        "reveal ",
        "reveals ",
        "put ",
        "puts ",
        "exile ",
        "exiles ",
        "then ",
        "and put ",
        "and puts ",
        "and exile ",
        "and exiles ",
        "and reveal ",
        "and reveals ",
        "and shuffle",
        "and shuffles",
        "shuffle",
        "shuffles",
    ];
    let mut best_split: Option<(usize, usize)> = None;
    for (idx, _) in rest.match_indices(", ") {
        let after_comma = &rest[idx + 2..];
        if COMMA_TAIL_STARTERS
            .iter()
            .any(|st| after_comma.starts_with(st))
        {
            best_split = Some((idx, 2));
            break;
        }
    }
    for (needle, skip) in [
        (" and put ", 5usize),
        (" and puts ", 5usize),
        (" and exile ", 5usize),
        (" and exiles ", 5usize),
        (" and reveal ", 5usize),
        (" and reveals ", 5usize),
        (" and shuffle", 5usize),
        (" and shuffles", 5usize),
        (" then shuffle", 1usize),
        (" then shuffles", 1usize),
    ] {
        if let Some(idx) = rest.find(needle) {
            let take = match best_split {
                Some((cur_idx, _)) => idx < cur_idx,
                None => true,
            };
            if take {
                best_split = Some((idx, skip));
            }
        }
    }
    let (flt_part_raw, tail) = match best_split {
        Some((idx, skip)) => (rest[..idx].trim(), rest[idx + skip..].trim()),
        None => (rest.trim(), ""),
    };

    let (flt_part_raw, modifier) = if let Some(f) = flt_part_raw.strip_suffix(" instead of one") {
        (f.trim(), Some(SearchModifierAst::InsteadOfOne))
    } else if let Some(f) = flt_part_raw.strip_prefix("an additional ") {
        (f.trim(), Some(SearchModifierAst::Additional))
    } else {
        (flt_part_raw, None)
    };

    // Split optional secondary destination ("and the other into ...", "and the rest into ...")
    let (tail_main, rest_to_zone) =
        if let Some((l, r)) = split_once_any(&tail, &[" and the other ", " and the rest "]) {
            let rz = split_once_any(&r, &[", then shuffle", ", then shuffles", ", shuffle"])
                .map_or(r, |(d, _)| d)
                .trim()
                .to_string();
            (l.trim(), Some(rz))
        } else {
            (tail, None)
        };

    let mut follow_up: Vec<InstructionAst> = Vec::new();
    // Reject complex unparsed tail clauses unless we parse them into follow_up cleanly
    if tail_main.contains("with a ")
        || tail_main.contains("with an ")
        || tail_main.contains("unless ")
        || tail_main.contains("if you do")
        || tail_main.contains("if it's")
        || tail_main.contains("for each ")
        || tail_main.contains("lose ")
        || tail_main.contains("loses ")
        || tail_main.contains("gain ")
        || tail_main.contains("gains ")
        || tail_main.contains("draw ")
        || tail_main.contains("draws ")
        || tail_main.contains("discard ")
        || tail_main.contains("discards ")
        || tail_main.contains("sacrifice ")
        || tail_main.contains("sacrifices ")
        || tail_main.contains("pay ")
        || tail_main.contains("pays ")
        || tail_main.contains("cast ")
        || tail_main.contains("casts ")
        || tail_main.contains(" play ")
        || tail_main.contains(" plays ")
        || tail_main.contains("create ")
        || tail_main.contains("creates ")
        || tail_main.contains("destroy ")
        || tail_main.contains("destroys ")
        || tail_main.contains("damage")
        || tail_main.contains("counter")
    {
        // Try strictly parsing the extra tail clause(s) as follow-up instructions!
        let (parsed_tail, tail_fails) = parse_effect_body(tail_main, vocab);
        if tail_fails.is_empty() && !parsed_tail.is_empty() {
            follow_up = parsed_tail;
        } else {
            return None;
        }
    }

    let shuffle = tail.contains("shuffle");
    let reveal = tail_main.starts_with("reveal ")
        || tail_main.starts_with("reveals ")
        || tail_main.contains(" reveal ")
        || tail_main.contains(" reveals ");

    let any_number = flt_part_raw.starts_with("any number of ") || flt_part_raw.starts_with("all ");
    let up_to = flt_part_raw.starts_with("up to ");
    let exact = flt_part_raw.starts_with("exactly ");
    let flt_clean = strip_prefix_any(
        &flt_part_raw,
        &["any number of ", "all ", "up to ", "exactly "],
    )
    .unwrap_or(flt_part_raw);

    // Optional compound search target: "<flt1> and/or a/an <flt2>", "<flt1> and a/an <flt2>",
    // or "<flt1> or a/an <flt2>". The right side must parse as a valid search filter (named or typed),
    // which supports both "a card named A and/or a card named B" and "a card named A or an Equipment card"
    // while keeping single card names such as "Sword of Feast and Famine" intact.
    let (primary_raw, secondary_filter, secondary_is_or) = if let Some((left_f, right_f)) =
        split_once_any(&flt_clean, &[" and/or a ", " and/or an "])
        && let Some(sf) = parse_search_single_filter(right_f.trim(), vocab)
    {
        (left_f.trim(), Some(Box::new(sf)), false)
    } else if let Some((left_f, right_f)) = split_once_any(&flt_clean, &[" and a ", " and an "])
        && let Some(sf) = parse_search_single_filter(right_f.trim(), vocab)
    {
        (left_f.trim(), Some(Box::new(sf)), false)
    } else if let Some((left_f, right_f)) = split_once_any(&flt_clean, &[" or a ", " or an "])
        && let Some(sf) = parse_search_single_filter(right_f.trim(), vocab)
    {
        (left_f.trim(), Some(Box::new(sf)), true)
    } else {
        (flt_clean, None, false)
    };

    let (cnt, flt_raw) = if any_number {
        (ValueExprAst::Const(0), primary_raw)
    } else if let Some(after_num) = strip_prefix_any(&primary_raw, &["a number of ", "number of "])
        && let Some((inner_flt, eq_val)) = after_num.split_once(" equal to ")
        && let Ok(parsed_cnt) = parse_value_expr(eq_val.trim(), vocab)
    {
        (parsed_cnt, inner_flt.trim())
    } else {
        peel_leading_count(primary_raw, vocab)
    };
    let flt = parse_search_single_filter(flt_raw, vocab)?;

    let has_exile = tail_main.contains("exile it")
        || tail_main.contains("exiles it")
        || tail_main.contains("exile that card")
        || tail_main.contains("exiles that card")
        || tail_main.contains("exile them")
        || tail_main.contains("exiles them")
        || tail_main.contains("exile those cards")
        || tail_main.contains("exiles those cards");
    let to_zone = if !follow_up.is_empty() {
        "follow_up"
    } else if tail_main.contains("the battlefield tapped and attacking") {
        "the battlefield tapped and attacking"
    } else if tail_main.contains("the battlefield tapped under your control") {
        "the battlefield tapped under your control"
    } else if tail_main.contains("the battlefield tapped") {
        "the battlefield tapped"
    } else if tail_main.contains("the battlefield under your control") {
        "the battlefield under your control"
    } else if tail_main.contains("the battlefield") {
        "the battlefield"
    } else if tail_main.contains("your hand") {
        "your hand"
    } else if tail_main.contains("their hand") {
        "their hand"
    } else if tail_main.contains("that player's hand") {
        "that player's hand"
    } else if tail_main.contains("its owner's hand") {
        "its owner's hand"
    } else if tail_main.contains("your graveyard") {
        "your graveyard"
    } else if tail_main.contains("their graveyard") {
        "their graveyard"
    } else if tail_main.contains("that player's graveyard") {
        "that player's graveyard"
    } else if tail_main.contains("top of your library in any order") {
        "top of your library in any order"
    } else if tail_main.contains("top of their library in any order") {
        "top of their library in any order"
    } else if tail_main.contains("top of their library") {
        "top of their library"
    } else if tail_main.contains("top of your library")
        || tail_main.contains("on top")
        || tail_main.contains("top of it")
    {
        "top of your library"
    } else if tail_main.contains("bottom of your library") {
        "bottom of your library"
    } else if tail_main.contains("bottom of their library") {
        "bottom of their library"
    } else if has_exile && tail_main.contains("face down") {
        "exile face down"
    } else if has_exile {
        "exile"
    } else if tail_main.contains("put ")
        || tail_main.contains("puts ")
        || tail_main.contains("exile")
    {
        return None;
    } else {
        "found_set"
    };

    let search_ins = InstructionAst::SearchLibrary {
        library: lib_who.trim_end_matches("'s").to_string(),
        count: Box::new(cnt),
        up_to,
        any_number,
        filter: Box::new(flt),
        reveal,
        to_zone: to_zone.to_string(),
        shuffle,
        extra_zones,
        secondary_filter,
        secondary_is_or,
        where_x,
        rest_to_zone,
        modifier,
        follow_up,
        exact,
    };
    let wrapped = match opt_player {
        Some(PlayerRefAst::You) => ins_opt1(search_ins),
        Some(player) => ins_opt_player(player, vec![search_ins]),
        None => search_ins,
    };
    Some(match leading_subject_player {
        Some(subj_p) => ins_fe_subj(subj_p, vec![wrapped]),
        None => wrapped,
    })
}

/// Verify that a multi-zone `from <z1> (and|or) from <z2>` tail contains only clean zone
/// phrases and never swallows an intermediate destination or chained instruction clause.
fn is_clean_multi_zone_source(from_zones: &str) -> bool {
    let low = from_zones.to_ascii_lowercase();
    for bad in [
        ", put ",
        " onto ",
        " into ",
        " under ",
        " then ",
        " to the ",
        " to your ",
        " to its ",
        " tapped",
        "exile it",
        "return ",
    ] {
        if low.contains(bad) {
            return false;
        }
    }
    let norm = low
        .replace(" and/or from ", " | ")
        .replace(" and from ", " | ")
        .replace(" or from ", " | ")
        .replace(", from ", " | ");
    let parts: Vec<&str> = norm.split(" | ").map(str::trim).collect();
    if parts.len() < 2 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && matches!(
                *part,
                "exile"
                    | "your graveyard"
                    | "a graveyard"
                    | "any graveyard"
                    | "graveyards"
                    | "all graveyards"
                    | "that player's graveyard"
                    | "its owner's graveyard"
                    | "your hand"
                    | "their hand"
                    | "that player's hand"
                    | "the command zone"
                    | "your library"
                    | "their library"
                    | "that player's library"
                    | "the battlefield"
                    | "outside the game"
            )
    })
}

pub(super) fn try_parse_library_ops(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    if let Some((who, amt_raw)) = split_leading_player_verb(low, "scry", "scries")
        && let Ok(cnt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(InstructionAst::Scry {
            player: who,
            count: Box::new(cnt),
        });
    }
    if let Some((who, amt_raw)) = split_leading_player_verb(low, "surveil", "surveils")
        && let Ok(cnt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(InstructionAst::Surveil {
            player: who,
            count: Box::new(cnt),
        });
    }
    if let Some((who, after)) = split_leading_player_verb(low, "mill", "mills") {
        let cnt_opt = if let Some(after_half) = after.strip_prefix("half ")
            && (after_half.starts_with("their library") || after_half.starts_with("your library"))
        {
            let rounding = if after_half.contains("rounded up") {
                crate::ast::Rounding::Up
            } else {
                crate::ast::Rounding::Down
            };
            Some(val_div(
                val_player_stat(who, crate::ast::PlayerStatKind::CardsInLibrary),
                2,
                rounding,
            ))
        } else if let Some(val_raw) =
            strip_prefix_any(&after, &["cards equal to ", "a number of cards equal to "])
        {
            parse_value_expr(val_raw, vocab).ok()
        } else if let Some(cnt_raw) = strip_suffix_any(&after, &[" cards", " card"]) {
            parse_value_expr(cnt_raw, vocab).ok()
        } else {
            None
        };
        if let Some(cnt) = cnt_opt {
            return Some(InstructionAst::Mill {
                player: who,
                count: Box::new(cnt),
            });
        }
    }
    if let Some((who, rev_rest)) = split_leading_player_verb(low, "reveal", "reveals")
        && matches!(
            rev_rest,
            "it" | "them"
                | "that card"
                | "those cards"
                | "the chosen card"
                | "the chosen cards"
                | "the exiled card"
                | "the exiled cards"
                | "the top card"
        )
    {
        return Some(InstructionAst::RevealByPlayer {
            player: who,
            subject: Box::new(SubjectRefAst::Anaphora(rev_rest.to_string())),
        });
    }
    if let Some(after) = low.strip_prefix("reveal ") {
        if let Some(flt_part) = after.strip_suffix(" from among them") {
            let trimmed = flt_part.trim();
            let clean = trimmed.strip_prefix("up to ").unwrap_or(trimmed);
            let (cnt_ast, flt_raw) = peel_leading_count(clean, vocab);
            let to_tag = if trimmed.starts_with("any number ") {
                "revealed:any_number".to_string()
            } else if trimmed.starts_with("all ") {
                "revealed:all".to_string()
            } else if let ValueExprAst::Const(n) = cnt_ast
                && n >= 2
            {
                format!("revealed:{n}")
            } else {
                "revealed".to_string()
            };
            if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                return Some(InstructionAst::RevealFromLooked {
                    filter: Box::new(flt),
                    to: to_tag,
                });
            }
        }
        if after.starts_with("target ") && parse_subject_or_target(after, vocab).is_ok() {
            return Some(ins_look_top(true, "target", ValueExprAst::Const(1)));
        }
    }
    if let Some((who, rest)) = split_leading_player_verb(low, "reveal", "reveals")
        && let Some(whose_lib) = rest.strip_prefix("the top card of ")
        && (whose_lib.ends_with(" library") || whose_lib.ends_with(" libraries"))
    {
        let lib_str = whose_lib
            .trim_end_matches(" library")
            .trim_end_matches(" libraries")
            .trim_end_matches("'s")
            .trim()
            .to_string();
        let library = if lib_str.is_empty() {
            "your".to_string()
        } else {
            lib_str
        };
        if who == PlayerRefAst::You {
            return Some(ins_look_top(true, library, ValueExprAst::Const(1)));
        }
        return Some(InstructionAst::PlayerLookTopLibrary {
            player: who,
            library,
            count: vconst(1),
            reveal: true,
        });
    }
    if let Some(after) = low.strip_prefix("look at ")
        && let Some((cnt_raw, lib_rest)) = after.split_once(" cards from the top of ")
        && let Some(whose) = lib_rest.strip_suffix(" library")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(ins_look_top(
            false,
            whose.trim_end_matches("'s").to_string(),
            cnt,
        ));
    }
    if let Some((who_raw, rest)) = split_once_any(&low, &[" reveals ", " reveal "])
        && let Some(who) = parse_player_ref(who_raw)
    {
        if let Some(val_raw) = rest.strip_prefix("a number of cards from their hand equal to ")
            && let Ok(cnt) = parse_value_expr(val_raw, vocab)
        {
            if who == PlayerRefAst::You {
                return Some(ins_look_top(true, "hand", cnt));
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library: "hand".to_string(),
                count: Box::new(cnt),
                reveal: true,
            });
        }
        if let Some(after_top) = rest.strip_prefix("the top ")
            && let Some((cnt_raw, lib_tail)) =
                split_once_any(&after_top, &[" cards of ", " card of "])
            && let Some(whose) = lib_tail.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            let library = whose.trim_end_matches("'s").to_string();
            if who == PlayerRefAst::You {
                return Some(ins_look_top(true, library, cnt));
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library,
                count: Box::new(cnt),
                reveal: true,
            });
        }
        if let Some(cnt_raw) = rest
            .strip_suffix(" cards from their hand")
            .or_else(|| rest.strip_suffix(" card from their hand"))
            .or_else(|| rest.strip_suffix(" cards from your hand"))
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            if who == PlayerRefAst::You {
                return Some(ins_look_top(true, "hand", cnt));
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library: "hand".to_string(),
                count: Box::new(cnt),
                reveal: true,
            });
        }
    }
    if let Some((who_raw, rest)) = split_once_any(
        &low,
        &[
            " reveals a card at random from ",
            " reveal a card at random from ",
        ],
    ) && rest.ends_with(" hand")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::RevealRandomFromHand { player });
    }
    if let Some((who, after_look)) = split_leading_player_verb(low, "look at", "looks at")
        && let Some(rest) = after_look.strip_prefix("the top ")
    {
        if let Some(whose_lib) = rest.strip_prefix("card of ")
            && let Some(whose) = whose_lib.strip_suffix(" library")
        {
            let library = whose.trim_end_matches("'s").to_string();
            if who == PlayerRefAst::You {
                return Some(ins_look_top(false, library, ValueExprAst::Const(1)));
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library,
                count: vconst(1),
                reveal: false,
            });
        }
        if let Some((cnt_str, whose_lib)) = rest.split_once(" cards of ")
            && let Some(whose) = whose_lib.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_str, vocab)
        {
            let library = whose.trim_end_matches("'s").to_string();
            if who == PlayerRefAst::You {
                return Some(ins_look_top(false, library, cnt));
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library,
                count: Box::new(cnt),
                reveal: false,
            });
        }
    }
    for (pref, reveal) in [("look at the top ", false), ("reveal the top ", true)] {
        if let Some(rest) = low.strip_prefix(pref) {
            if let Some(whose_lib) = rest.strip_prefix("card of ")
                && let Some(whose) = whose_lib.strip_suffix(" library")
            {
                return Some(ins_look_top(
                    reveal,
                    whose.trim_end_matches("'s").to_string(),
                    ValueExprAst::Const(1),
                ));
            }
            if let Some((cnt_str, whose_lib)) = rest.split_once(" cards of ")
                && let Some(whose) = whose_lib.strip_suffix(" library")
                && let Ok(cnt) = parse_value_expr(cnt_str, vocab)
            {
                return Some(ins_look_top(
                    reveal,
                    whose.trim_end_matches("'s").to_string(),
                    cnt,
                ));
            }
        }
    }
    // Exile/Reveal from top of library until ...
    for (pref, exile) in [
        ("exile cards from the top of ", true),
        ("reveal cards from the top of ", false),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Some((lib_raw, until_rest)) = rest.split_once(" library until you ")
        {
            let flt_raw =
                strip_prefix_any(&until_rest, &["exile ", "reveal "]).unwrap_or(until_rest);
            let (_, flt_nocnt) = peel_leading_count(flt_raw, vocab);
            if let Ok(flt) = parse_object_filter(flt_raw, vocab)
                .or_else(|_| parse_object_filter(flt_nocnt, vocab))
            {
                return Some(InstructionAst::ExileOrRevealTopUntil {
                    exile,
                    library: lib_raw.trim_end_matches("'s").to_string(),
                    filter: Box::new(flt),
                });
            }
        }
    }

    // Look at <player>'s hand
    if let Some(rest) = low.strip_prefix("look at ")
        && let Some(owner_raw) = rest.strip_suffix("'s hand")
        && let Some(p) = parse_player_ref(owner_raw)
    {
        return Some(InstructionAst::LookAtHand { player: p });
    }

    // Imperative "shuffle your graveyard into your library" / "shuffle ~ and your graveyard into their owner's library" (Campfire, Elixir of Immortality)
    if low == "shuffle your graveyard into your library"
        || low == "shuffle ~ and your graveyard into their owner's library"
    {
        return Some(InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        });
    }
    // "<player> shuffles their graveyard into their library"
    if let Some((who_raw, rest)) = split_once_any(&low, &[" shuffles ", " shuffle "])
        && matches!(
            rest,
            "their graveyard into their library"
                | "your graveyard into your library"
                | "his or her graveyard into his or her library"
        )
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::ShuffleGraveyardIntoLibrary { player: p });
    }

    if low == "reveal your hand" {
        return Some(InstructionAst::RevealHand {
            player: PlayerRefAst::You,
        });
    }
    if let Some((who_raw, hand_part)) = split_once_any(&low, &[" reveals ", " reveal "])
        && matches!(hand_part, "their hand" | "his or her hand" | "your hand")
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::RevealHand { player: p });
    }
    None
}

pub(super) fn parse_pile_separation_and_visibility(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Atris, Fortune's Favor, Sauron's Ransom:
    // "<player> look(s) at the top <N> cards of your library and separate(s) them into a face-down pile and a face-up pile"
    if let Some((who_raw, rest)) =
        split_once_any(&low, &[" looks at the top ", " look at the top "])
        && let Some(chooser) = parse_player_ref(who_raw.trim())
        && let Some((cnt_raw, tail)) = rest.split_once(" cards of your library and separate")
        && let Ok(cnt) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        let face_down_piles = if tail.contains("two face-down piles") {
            2
        } else if tail.contains("a face-down pile and a face-up pile")
            || tail.contains("a face-up pile and a face-down pile")
        {
            1
        } else {
            return None;
        };
        return Some(vec![
            ins_look_top(false, "your", cnt),
            InstructionAst::SeparateIntoPiles {
                chooser,
                pile_count: 2,
                face_down_piles,
            },
        ]);
    }

    // Compositional cast-up-to-N spells from among a filtered group without paying mana costs
    // (e.g. "<player> may cast [up to <N> ]spell[s] from among <filter> [without paying ...]")
    if let Some((who_raw, rest)) = low.split_once(" may cast ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_part, group_tail)) =
            split_once_any(&rest, &[" spells from among ", " spell from among "])
    {
        let (flt_raw, free) = if let Some(p) = group_tail
            .strip_suffix(" without paying their mana costs")
            .or_else(|| group_tail.strip_suffix(" without paying its mana cost"))
        {
            (p.trim(), true)
        } else {
            (group_tail.trim(), false)
        };
        let count_limit = if let Some(n_raw) = cnt_part.trim().strip_prefix("up to ") {
            parse_value_expr(n_raw.trim(), vocab)
                .ok()
                .map(|v| CountLimitAst::UpTo(Box::new(v)))
        } else if matches!(cnt_part.trim(), "a" | "an" | "") {
            Some(CountLimitAst::UpTo(vconst(1)))
        } else {
            parse_value_expr(cnt_part.trim(), vocab)
                .ok()
                .map(|v| CountLimitAst::UpTo(Box::new(v)))
        };
        if let Ok(mut flt) = parse_object_filter(flt_raw, vocab) {
            // "other cards exiled with ~" excludes the just-exiled card (not self, which is on the battlefield).
            if flt_raw.starts_with("other ")
                && (flt.flags.0 & ObjectFilterFlags::EXILED_WITH_SELF.0) != 0
            {
                flt.flags = ObjectFilterFlags(flt.flags.0 & !ObjectFilterFlags::EXCLUDE_SELF.0);
                flt.predicates
                    .push(FilterPredicateAst::Exclude(Box::new(anaphora(
                        "that exiled card",
                    ))));
            }
            if flt_raw.contains("your opponents own") {
                flt.owner = Some(ControllerRelation::Opponents);
            } else if flt_raw.contains("you own") {
                flt.owner = Some(ControllerRelation::You);
            }
            if flt_raw.contains("from exile") || flt_raw.contains("in exile") {
                flt.zone = Some(ZoneFilter::Exile);
            }
            let inner = InstructionAst::GrantCastPermission {
                player,
                cast_only: true,
                subject: Box::new(grp(flt)),
                free,
                duration: "now".to_string(),
                count_limit,
                alt_cost: None,
            };
            return some1(if player == PlayerRefAst::You {
                ins_opt1(inner)
            } else {
                ins_opt_player(player, vec![inner])
            });
        }
    }

    // Compositional: "exile <subj> [face down ]with <N> <kind> counter(s) on it, then <follow_up>"
    if let Some(after_ex) = low.strip_prefix("exile ")
        && let Some((head, tail)) = after_ex.split_once(", then ")
        && let Some((subj_and_vis, cnt_and_kind)) = head.split_once(" with ")
        && let Some(ck_raw) =
            strip_suffix_any(&cnt_and_kind, &[" counters on it", " counter on it"])
    {
        let (subj_raw, face_down) = match subj_and_vis.trim().strip_suffix(" face down") {
            Some(p) => (p.trim(), true),
            None => (subj_and_vis.trim(), false),
        };
        let (cnt, kind_raw) = peel_leading_count(ck_raw.trim(), vocab);
        if let (Ok(subj), Ok(mut follow)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_atomic_effect(tail.trim(), vocab),
        ) {
            let mut out = vec![InstructionAst::ExileWithCounters {
                subject: Box::new(subj),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(cnt),
                face_down,
            }];
            out.append(&mut follow);
            return Some(out);
        }
    }

    // Compositional: "<player> turns face up <subj>, then <follow_up>"
    if let Some((who_raw, rest)) = split_once_any(&low, &[" turns face up ", " turn face up "])
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((subj_raw, tail)) = rest.split_once(", then ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let tail_for_actor = if let Some(rest_put) = tail
            .trim()
            .strip_prefix("puts ")
            .or_else(|| tail.trim().strip_prefix("put "))
        {
            format!("put {rest_put}")
        } else {
            tail.trim().to_string()
        };
        let follow = parse_atomic_effect(tail.trim(), vocab)
            .or_else(|_| parse_atomic_effect(&tail_for_actor, vocab));
        if let Ok(mut follow_ins) = follow {
            let actor = if matches!(
                player,
                PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent
            ) {
                PlayerRefAst::ThatPlayer
            } else {
                player
            };
            let mut body = vec![InstructionAst::TurnFaceUp {
                player: actor,
                subject: Box::new(subj),
            }];
            body.append(&mut follow_ins);
            return Some(
                if matches!(
                    player,
                    PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent
                ) {
                    vec![ins_for_each_player(player, body)]
                } else {
                    body
                },
            );
        }
    }

    // Compositional: "exile <subj> in a face-down pile, shuffle that pile, then (manifest|cloak) <anaphora>"
    if let Some(after_ex) = low.strip_prefix("exile ")
        && let Some((subj_raw, rest)) =
            after_ex.split_once(" in a face-down pile, shuffle that pile, then ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let last_ins = if let Some(m_subj) = rest.strip_prefix("manifest ") {
            Some(ins_manifest(
                ValueExprAst::Const(0),
                false,
                Some(SubjectRefAst::Anaphora(m_subj.trim().to_string())),
            ))
        } else {
            rest.strip_prefix("cloak ")
                .map(|c_subj| InstructionAst::Cloak {
                    count: vconst(0),
                    subject: Some(Box::new(SubjectRefAst::Anaphora(c_subj.trim().to_string()))),
                })
        };
        if let Some(final_op) = last_ins {
            return Some(vec![
                InstructionAst::Exile {
                    actor: Some(PlayerRefAst::You),
                    subject: Box::new(subj),
                    random: false,
                    face_down: true,
                    into_pile: true,
                },
                InstructionAst::ShuffleIntoLibrary {
                    subject: box_anaphora("that pile"),
                },
                final_op,
            ]);
        }
    }

    None
}

pub(super) fn parse_cumulative_exile_until(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Dream Harvest / Tasha's Hideous Laughter:
    // "<player> exile(s) cards from the top of their library until <they have / that player has> exiled cards with total mana value <N> or greater [this way]"
    if let Some((who_raw, rest)) = low
        .split_once(" exiles cards from the top of their library until ")
        .or_else(|| low.split_once(" exile cards from the top of their library until "))
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((_, mv_tail)) = rest.split_once("exiled cards with total mana value ")
        && let Some(n_raw) = strip_suffix_any(&mv_tail, &[" or greater this way", " or greater"])
        && let Ok(threshold) = parse_value_expr(n_raw.trim(), vocab)
    {
        return some1(InstructionAst::ExileOrRevealTopUntilTotalStat {
            exile: true,
            player,
            stat: StatKind::ManaValue,
            threshold: Box::new(threshold),
        });
    }
    None
}

pub(super) fn parse_zone_choice_exchange_extended(
    _s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 3. Exile from each graveyard ("[you may] exile <flt> from each graveyard") (King Narfi's Betrayal, Summon: Esper Valigarmanda)
    let (opt_ex, ex_core) = match low.strip_prefix("you may exile ") {
        Some(r) => (true, r),
        None => match low.strip_prefix("exile ") {
            Some(r) => (false, r),
            None => (false, ""),
        },
    };
    if let Some(flt_raw) = ex_core.strip_suffix(" from each graveyard")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.zone = Some(ZoneFilter::AllGraveyards);
        let ins = ins_for_each_player(
            PlayerRefAst::EachPlayer,
            vec![ins_exile(Some(PlayerRefAst::You), grp(flt))],
        );
        return Some(if opt_ex {
            vec![ins_opt1(ins)]
        } else {
            vec![ins]
        });
    }

    // 4. Exile top card of each player's library with a counter (Evelyn, the Covetous)
    if let Some(after_ex) = low.strip_prefix("exile the top card of each player's library with a ")
        && let Some(kind_raw) = after_ex.strip_suffix(" counter on it")
    {
        return some1(ins_for_each_player(
            PlayerRefAst::EachPlayer,
            vec![InstructionAst::ExileWithCounters {
                subject: box_anaphora("the top card of their library"),
                counter_kind: kind_raw.trim().to_string(),
                count: vconst(1),
                face_down: false,
            }],
        ));
    }

    // 5. Exile card from hand until ~ leaves the battlefield (Intellect Devourer)
    if let Some((who_raw, rest)) = split_once_any(&low, &[" exiles ", " exile "])
        && let Some(subj_raw) = rest.strip_suffix(" until ~ leaves the battlefield")
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return some1(ins_for_each_player(
            player,
            vec![InstructionAst::ExileUntilLeaves {
                subject: Box::new(subj),
            }],
        ));
    }

    // 6. Exile card from hand and may play for as long as it remains exiled (Lightstall Inquisitor)
    if let Some((who_raw, rest)) = split_once_any(&low, &[" exiles ", " exile "])
        && let Some((subj_raw, play_tail)) = rest.split_once(" and may play ")
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return some1(ins_for_each_player(
            player,
            vec![
                ins_exile(Some(PlayerRefAst::ThatPlayer), subj),
                ins_grant_cast(
                    PlayerRefAst::ThatPlayer,
                    false,
                    anaphora("that card"),
                    false,
                    play_tail.trim().to_string(),
                ),
            ],
        ));
    }

    // 8. Exile N of them at random (Orcish Librarian)
    if let Some(after) = low.strip_prefix("exile ")
        && let Some(cnt_raw) = after.strip_suffix(" of them at random")
        && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return some1(InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(count)),
            up_to: false,
            any_number: false,
            subject: box_anaphora("them"),
            to: "exile_random".to_string(),
        });
    }

    // 9. Put that many cards from top of library into hand / Put bottom card into graveyard (Scroll Rack, Cellar Door)
    if let Some(after) = low.strip_prefix("put that many cards from the top of your library into ")
    {
        return some1(InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(ValueExprAst::PriorResult)),
            up_to: false,
            any_number: false,
            subject: box_anaphora("the top cards of your library"),
            to: after.trim().to_string(),
        });
    }
    if let Some((who_raw, rest)) = low.split_once(" puts the bottom card of their library into ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return some1(InstructionAst::MoveCounted {
            actor: Some(player),
            count: Some(vconst(1)),
            up_to: false,
            any_number: false,
            subject: box_anaphora("the bottom card of their library"),
            to: rest.trim().to_string(),
        });
    }

    // 10. Search library, then shuffle and put that card <ordinal> from the top (Long-Term Plans)
    if let Some(after) = low.strip_prefix("search your library for ")
        && let Some((flt_raw, ord_tail)) = after.split_once(", then shuffle and put that card ")
        && let Some(ord_word) = ord_tail.strip_suffix(" from the top")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let from_top = match ord_word.trim() {
            "first" => 1,
            "second" => 2,
            "third" => 3,
            "fourth" => 4,
            "fifth" => 5,
            "sixth" => 6,
            "seventh" => 7,
            _ => return None,
        };
        return Some(vec![
            InstructionAst::SearchLibrary {
                library: "your".to_string(),
                count: vconst(1),
                up_to: false,
                any_number: false,
                exact: false,
                filter: Box::new(flt),
                reveal: false,
                to_zone: "found_set".to_string(),
                shuffle: true,
                extra_zones: Vec::new(),
                secondary_filter: None,
                secondary_is_or: false,
                where_x: None,
                rest_to_zone: None,
                modifier: None,
                follow_up: Vec::new(),
            },
            InstructionAst::PutInLibraryAtPosition {
                player: PlayerRefAst::You,
                subject: box_anaphora("that card"),
                from_top,
            },
        ]);
    }

    // 11. Manifest a number of cards equal to <val> / Manifest one of those cards (Omarthis, Write into Being)
    if let Some(val_raw) =
        low.strip_prefix("manifest a number of cards from the top of your library equal to ")
        && let Ok(count) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![ins_manifest(count, false, None)]);
    }
    if let Some(subj_raw) = low.strip_prefix("manifest ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![ins_manifest(
            ValueExprAst::Const(1),
            false,
            Some(subj),
        )]);
    }

    // 12. The owners of those cards shuffle them into their libraries (Turn the Earth)
    if let Some(after) = low.strip_prefix("the owners of ")
        && let Some((subj_raw, tail)) = after.split_once(" shuffle ")
        && tail.ends_with(" into their libraries")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![ins_shuffle_into(subj)]);
    }

    // 13. Return to your hand <s1>, <s2>, and <s3> (Remove Enchantments)
    if let Some(after) = low.strip_prefix("return to your hand ")
        && let Some((s1_raw, rest)) = after.split_once(", ")
        && let Some((s2_raw, s3_raw)) = rest.split_once(", and ")
        && let (Ok(s1), Ok(s2), Ok(s3)) = (
            parse_subject_or_target(s1_raw.trim(), vocab),
            parse_subject_or_target(s2_raw.trim(), vocab),
            parse_subject_or_target(s3_raw.trim(), vocab),
        )
    {
        return some1(InstructionAst::ReturnZone {
            subject: Box::new(SubjectRefAst::Compound(vec![s1, s2, s3])),
            from_zone: Some("battlefield".to_string()),
            to: "your hand".to_string(),
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
        });
    }

    // 15. Return ~ to the battlefield under your control as a <type> (Takklemaggot)
    if let Some(after) = low.strip_prefix("return ")
        && let Some((subj_raw, type_raw)) =
            after.split_once(" to the battlefield under your control as ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![
            ins_return_simple(subj.clone(), "battlefield under your control", false),
            ins_temp_static(
                StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj),
                    type_or_subtype: type_raw.trim().to_string(),
                    additive: false,
                    condition: None,
                },
                DurationAst::Permanent,
            ),
        ]);
    }

    // 16. Sacrifice a tenth of ... rounded up (Legate Lanius)
    if let Some((who_raw, rest)) = low.split_once(" sacrifices a tenth of ")
        && let Some(flt_raw) = rest.strip_suffix(", rounded up")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        let count = ValueExprAst::DivConst {
            expr: Box::new(ValueExprAst::Count(Box::new(flt.clone()))),
            denom: 10,
            rounding: Rounding::Up,
        };
        return some1(InstructionAst::Sacrifice {
            player,
            count: Some(Box::new(count)),
            subject: Box::new(grp(flt)),
            except: None,
            half: false,
            rounding: Some(Rounding::Up),
            chooser: Some(PlayerRefAst::ThatPlayer),
        });
    }

    // 17. Choose-per-filter / sacrifice-rest (Global Ruin, Stick Together, Guided Passage, Distended Mindbender)
    if let Some((who_raw, rest)) = split_once_any(&low, &[" chooses from ", " choose from "])
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (sac_rest, core) = match rest.strip_suffix(", then sacrifices the rest") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Some(after_src) = core.strip_prefix("the lands they control ")
            && let Ok(flt) = parse_object_filter(after_src.trim(), vocab)
        {
            return some1(InstructionAst::ChoosePerFilter {
                player,
                categories: vec![flt],
                sacrifice_rest: sac_rest,
            });
        }
        if let Some(after_among) = strip_prefix_any(&core, &["among them ", "it "]) {
            let parts: Vec<&str> = if let Some((a, bc)) = after_among.split_once(", ")
                && let Some((b, c)) = bc.split_once(", and ")
            {
                vec![a.trim(), b.trim(), c.trim()]
            } else if let Some((a, b)) = after_among.split_once(" and ") {
                vec![a.trim(), b.trim()]
            } else {
                Vec::new()
            };
            if !parts.is_empty() {
                let mut cats = Vec::with_capacity(parts.len());
                for p in parts {
                    cats.push(parse_object_filter(p, vocab).ok()?);
                }
                return some1(InstructionAst::ChoosePerFilter {
                    player,
                    categories: cats,
                    sacrifice_rest: sac_rest,
                });
            }
        }
    }
    if let Some((who_raw, rest)) = low.split_once(" chooses a party from among ")
        && let Some(flt_raw) = rest.strip_suffix(", then sacrifices the rest")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return some1(InstructionAst::ChoosePerFilter {
            player,
            categories: vec![flt],
            sacrifice_rest: true,
        });
    }

    // 19. Exile the pile of an opponent's choice and return the other to <zone> (Death or Glory)
    if let Some(dest) =
        low.strip_prefix("exile the pile of an opponent's choice and return the other to ")
    {
        return Some(vec![
            ins_pile_action("exile", PileRefAst::ChosenPile, None),
            ins_return_simple(
                SubjectRefAst::PileRef(PileRefAst::OtherPile),
                dest.trim().to_string(),
                false,
            ),
        ]);
    }

    // 21. Player chooses a group / random choice (Phyrexian Grimoire, Summon: Valefor, Confusion in the Ranks, Tyrant of Discord)
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (random, core) = match rest.strip_suffix(" at random") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Ok(group) = parse_subject_or_target(core.trim(), vocab) {
            let act = InstructionAst::ChooseFromGroup {
                count: vconst(1),
                random,
                group: Box::new(group),
            };
            return Some(
                if matches!(
                    player,
                    PlayerRefAst::EachOpponent | PlayerRefAst::EachPlayer
                ) {
                    vec![ins_for_each_player(player, vec![act])]
                } else {
                    vec![act]
                },
            );
        }
    }

    // 22. Exchange control of the creature you each control with the greatest mana value (Juxtapose)
    if let Some(after) = low.strip_prefix("you and ")
        && let Some((other_raw, obj_raw)) = after.split_once(" exchange control of ")
        && let Some(_other) = parse_player_ref(other_raw.trim())
        && let Some(stat_part) =
            obj_raw.strip_prefix("the creature you each control with the greatest ")
    {
        let stat = match stat_part.trim() {
            "mana value" => StatKind::ManaValue,
            "power" => StatKind::Power,
            "toughness" => StatKind::Toughness,
            _ => return None,
        };
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.predicates.push(FilterPredicateAst::SuperlativeStat {
            stat,
            greatest: true,
        });
        return Some(vec![ins_exchange(ExchangeKind::Control, grp(flt), None)]);
    }

    // 23. For any number of opponents, <eff> (Windgrace's Judgment)
    if let Some(body_low) = low.strip_prefix("for any number of opponents, ")
        && let Some(body) = parse_sub_effect_extended(body_low, body_low, vocab)
    {
        return Some(vec![ins_for_each_player(PlayerRefAst::EachOpponent, body)]);
    }

    // 27. Leave <subj> in <zone> (Deliver Unto Evil)
    if let Some(after) = low.strip_prefix("leave ")
        && let Some((subj_raw, zone_raw)) = after.split_once(" in ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![ins_move(subj, format!("stay_in:{}", zone_raw.trim()))]);
    }

    // 29. "<player> destroys <N> <flt>" (Burning of Xinye)
    if let Some((who_raw, rest)) = split_once_any(&low, &[" destroys ", " destroy "])
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_tok, flt_raw)) = rest.trim().split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_tok.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return some1(ins_for_each_player(
            player,
            vec![
                ins_choose_group(ValueExprAst::Const(n), false, grp(flt)),
                InstructionAst::Destroy {
                    subject: box_anaphora("those permanents"),
                    random: false,
                    except: None,
                },
            ],
        ));
    }

    // 30. "<player> draws as many cards as they discarded this way" (Forget)
    if let Some(after) = low.strip_prefix("draws as many cards as ")
        && after.ends_with(" discarded this way")
    {
        return some1(ins_draw(
            PlayerRefAst::ThatPlayer,
            ValueExprAst::PriorResult,
        ));
    }

    // 31. "<player> each discard their hands" (Wheel and Deal)
    if let Some(who_raw) = low.strip_suffix(" each discard their hands")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return some1(ins_for_each_player(
            player,
            vec![InstructionAst::DiscardHand {
                player: PlayerRefAst::ThatPlayer,
            }],
        ));
    }

    // 32. "another player of your choice may gain control of <subj>" (Stardew Valley)
    if let Some(subj_raw) = low.strip_prefix("another player of your choice may gain control of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_opt_player(
            PlayerRefAst::EachOtherPlayer,
            vec![ins_gain_control(
                PlayerRefAst::ThatPlayer,
                subj,
                dur.clone(),
            )],
        ));
    }

    None
}
