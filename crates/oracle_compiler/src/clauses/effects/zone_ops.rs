//! Zone manipulation, exile, sacrifice, discard, library search/ops, pile separation,
//! and exchange effect parsers.

use super::{
    parse_atomic_effect, parse_effect_body, parse_sub_effect_extended, split_leading_player_verb,
};
use crate::ast::{
    CardTypeMask, CompareOp, ConditionAst, ControllerRelation, CountLimitAst, DelayedStepScope,
    DurationAst, ExchangeKind, FilterPredicateAst, InstructionAst, InteractiveChoiceKind,
    NameFilterAst, ObjectFilterAst, ObjectFilterFlags, PileRefAst, PlayerRefAst, Rounding,
    SearchModifierAst, StatBoundAst, StatKind, StaticAbilityAst, SubjectRefAst, TokenSpecAst,
    TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, peel_leading_count};
use crate::clauses::statics::is_orthogonal_battlefield_dest;
use crate::combinators::obj_type;
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_value_expr,
};
use crate::preprocess::parse_base_pt;
use crate::vocab::{Vocabulary, parse_number_word};

pub(super) fn try_parse_exile(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    if let Some(after) = low
        .strip_prefix("exile ")
        .or_else(|| low.strip_prefix("exiles "))
    {
        if !after.contains(" from ")
            && !after.contains(" in ")
            && let Some(whose) = after
                .strip_suffix(" graveyards")
                .or_else(|| after.strip_suffix(" graveyard"))
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
            && let Some(left) = cnt_rest
                .strip_suffix(" counters on it")
                .or_else(|| cnt_rest.strip_suffix(" counter on it"))
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
        if let Some(whose) = after
            .strip_suffix(" graveyards")
            .or_else(|| after.strip_suffix(" graveyard"))
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
        let (after_fd, random) = match after_fd
            .strip_suffix(" chosen at random")
            .or_else(|| after_fd.strip_suffix(" at random"))
        {
            Some(c) => (c.trim(), true),
            None => (after_fd, false),
        };
        if let Some(whose_lib) = after_fd.strip_prefix("the bottom card of ")
            && let Some(whose) = whose_lib
                .strip_suffix(" library")
                .or_else(|| whose_lib.strip_suffix(" libraries"))
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: format!("{}_bottom", whose.trim_end_matches("'s")),
                count: Box::new(ValueExprAst::Const(1)),
                face_down,
            });
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
                    subject: Box::new(SubjectRefAst::Anaphora("one of them".to_string())),
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
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                face_down,
                random,
                into_pile: false,
            });
        }
        if let Some((cnt_part, lib_part)) = after_fd.split_once(" cards from the bottom of ")
            && let Some(whose) = lib_part.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: format!("{}_bottom", whose.trim_end_matches("'s")),
                count: Box::new(cnt),
                face_down,
            });
        }
        if let Some(rest) = after_fd.strip_prefix("the top ")
            && let Some((cnt_part, whose_lib)) = rest
                .split_once(" cards of ")
                .or_else(|| rest.split_once(" card of "))
            && let Some(whose) = whose_lib.strip_suffix(" library")
        {
            let cnt = if cnt_part.is_empty() {
                ValueExprAst::Const(1)
            } else {
                parse_value_expr(cnt_part, vocab).ok()?
            };
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: whose.trim_end_matches("'s").to_string(),
                count: Box::new(cnt),
                face_down,
            });
        }
        if let Some(whose_lib) = after_fd.strip_prefix("the top card of ")
            && let Some(whose) = whose_lib
                .strip_suffix(" library")
                .or_else(|| whose_lib.strip_suffix(" libraries"))
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: whose.trim_end_matches("'s").to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down,
            });
        }
        if after_fd == "the top card" {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: "your".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down,
            });
        }
        if let Some(cnt_raw) = after_fd
            .strip_prefix("the top ")
            .and_then(|r| r.strip_suffix(" cards"))
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: "your".to_string(),
                count: Box::new(cnt),
                face_down,
            });
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
        if let Some((cnt_part, lib_part)) = after_fd
            .split_once(" cards from the top of ")
            .or_else(|| after_fd.split_once(" card from the top of "))
        {
            if let Some((whose_lib, eq_val)) = lib_part.split_once(" library equal to ")
                && cnt_part == "a number of"
                && let Ok(cnt) = parse_value_expr(eq_val, vocab)
            {
                return Some(InstructionAst::ExileTopLibrary {
                    actor,
                    whose: whose_lib.trim_end_matches("'s").to_string(),
                    count: Box::new(cnt),
                    face_down,
                });
            }
            if let Some(whose) = lib_part.strip_suffix(" library")
                && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
            {
                return Some(InstructionAst::ExileTopLibrary {
                    actor,
                    whose: whose.trim_end_matches("'s").to_string(),
                    count: Box::new(cnt),
                    face_down,
                });
            }
        }
        // "<player> exiles cards from the top of <whose> library until <player> exile(s) <filter>"
        if let Some(rest) = after_fd.strip_prefix("cards from the top of ")
            && let Some((lib_raw, until_rest)) = rest
                .split_once(" library until you ")
                .or_else(|| rest.split_once(" library until they "))
                .or_else(|| rest.split_once(" library until that player "))
        {
            let flt_raw = until_rest
                .strip_prefix("exile ")
                .or_else(|| until_rest.strip_prefix("exiles "))
                .unwrap_or(until_rest);
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
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
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
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
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
            return Some(InstructionAst::IfCondition {
                condition: Box::new(ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
                    spent_raw.trim().to_string(),
                )))),
                then_body: vec![InstructionAst::Sacrifice {
                    player: PlayerRefAst::You,
                    count: None,
                    subject: Box::new(subj),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: None,
                }],
            });
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
            return Some(InstructionAst::IfCondition {
                condition: Box::new(ConditionAst::Not(Box::new(cond))),
                then_body: vec![InstructionAst::Sacrifice {
                    player: PlayerRefAst::You,
                    count: None,
                    subject: Box::new(subj),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: None,
                }],
            });
        }
    }
    // "<subj>'s controller sacrifices it" (Arcum Dagsson, Reality Acid, Soul Ransom)
    if let Some(subj_raw) = low.strip_suffix("'s controller sacrifices it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Sacrifice {
            player: PlayerRefAst::ThatObjectController,
            count: None,
            subject: Box::new(subj),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        });
    }
    // "<player> chooses and sacrifices <subj>" (Barrin's Spite, Retribution)
    if let Some((who, after)) =
        split_leading_player_verb(low, "choose and sacrifice", "chooses and sacrifices")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Some(InstructionAst::Sacrifice {
            player: who,
            count: None,
            subject: Box::new(subj),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        });
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
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
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
                core.strip_suffix(" of their choice")
                    .or_else(|| core.strip_suffix(" of his or her choice"))
                    .unwrap_or(core),
                None,
            )
        };
        let (core, except) = if let Some((l, r)) = core
            .split_once(", except for ")
            .or_else(|| core.split_once(" except for "))
        {
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
        let (core, half) = if let Some(c) = core
            .strip_prefix("half of the ")
            .or_else(|| core.strip_prefix("half the "))
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
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
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
        return Some(InstructionAst::Unless {
            body: vec![InstructionAst::Discard {
                player: who,
                count: Some(Box::new(cnt)),
                filter: None,
                anaphora: None,
                random: false,
            }],
            unless_player: Some(PlayerRefAst::You),
            unless_cost: Some(cost),
            unless_action: None,
            unless_condition: None,
        });
    }
    if let Some(fe) = after
        .strip_prefix("a card for each ")
        .or_else(|| after.strip_prefix("one card for each "))
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return Some(InstructionAst::Discard {
            player: who,
            count: Some(Box::new(cnt)),
            filter: None,
            anaphora: None,
            random: false,
        });
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
        return Some(InstructionAst::Discard {
            player: who,
            count: Some(Box::new(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PlayerStat {
                    player: who,
                    stat: crate::ast::PlayerStatKind::CardsInHand,
                }),
                denom: 2,
                rounding,
            })),
            filter: None,
            anaphora: None,
            random: false,
        });
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
        let (core, except) = if let Some((l, r)) = core
            .split_once(", except for ")
            .or_else(|| core.split_once(" except for "))
        {
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
                        Some((step, scope)) => InstructionAst::DelayedTrigger {
                            step,
                            scope,
                            body: vec![ins],
                        },
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
            && let Some(kind) = after_w
                .strip_suffix(" counter on it")
                .or_else(|| after_w.strip_suffix(" counters on it"))
        {
            (before, Some(kind.trim().to_string()))
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
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
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
                        Some((step, scope)) => InstructionAst::DelayedTrigger {
                            step,
                            scope,
                            body: vec![ins],
                        },
                        None => ins,
                    });
                }
            }
        }
    }

    // ChooseAndKeep ("... and the rest ..." / "... and the other ..." including "put one back and the rest into ...")
    if let Some((_, after)) = split_leading_player_verb(low, "put", "puts")
        && let Some((keep_part, rest_part)) = after
            .split_once(" and the rest ")
            .or_else(|| after.split_once(" and the other "))
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
                    ap.strip_prefix("into ")
                        .or_else(|| ap.strip_prefix("onto "))
                        .or_else(|| ap.strip_prefix("on "))
                        .unwrap_or("")
                };
                let rest_to = rest_part
                    .strip_prefix("into ")
                    .or_else(|| rest_part.strip_prefix("on "))
                    .or_else(|| rest_part.strip_prefix("onto "))
                    .unwrap_or("");
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
        if let Some((flt_part, keep_to)) = keep_part
            .split_once(" into ")
            .or_else(|| keep_part.split_once(" onto "))
            && let Some(rest_to) = rest_part
                .strip_prefix("into ")
                .or_else(|| rest_part.strip_prefix("on "))
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
        return Some(InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            to: "owner_choice_top_or_bottom_of_library".to_string(),
        });
    }
    if let Some((subj_raw, rest)) = low.split_once("'s owner puts it on ")
        && rest == "their choice of the top or bottom of their library"
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: "owner_choice_top_or_bottom_of_library".to_string(),
        });
    }
    if let Some(after) = low.strip_prefix("the owner of ")
        && let Some((subj_raw, rest)) = after
            .split_once(" puts it on ")
            .or_else(|| after.split_once(" puts it into "))
        && (rest == "their choice of the top or bottom of their library"
            || rest == "their library second from the top or on the bottom")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: rest.to_string(),
        });
    }

    // "shuffle and put <subj> on top [of your library]" (Mwonvuli Beast Tracker)
    if let Some(after) = low.strip_prefix("shuffle and put ")
        && let Some(subj_raw) = after
            .strip_suffix(" on top")
            .or_else(|| after.strip_suffix(" on top of your library"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: "top_of_library_after_shuffle".to_string(),
        });
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
                return Some(InstructionAst::MoveZone {
                    subject: Box::new(subj),
                    to: dest.to_string(),
                });
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
                    if let Some((subj_part, from_zones)) = obj_clean.split_once(" from ")
                        && from_zones.contains(" from ")
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
                            return Some(InstructionAst::MoveZone {
                                subject: Box::new(t),
                                to: dest.to_string(),
                            });
                        }
                        if let Ok(subj) = parse_subject_or_target(any_rest, vocab) {
                            return Some(InstructionAst::MoveCounted {
                                actor: None,
                                count: None,
                                up_to: false,
                                any_number: true,
                                subject: Box::new(subj),
                                to: dest.to_string(),
                            });
                        }
                        return None;
                    }
                    if let Some(up_rest) = obj_clean.strip_prefix("up to ") {
                        if let Ok(t @ SubjectRefAst::TargetObject { .. }) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(InstructionAst::MoveZone {
                                subject: Box::new(t),
                                to: dest.to_string(),
                            });
                        }
                        let (cnt, up_nocnt) = peel_leading_count(up_rest, vocab);
                        if let Ok(subj) = parse_subject_or_target(up_nocnt, vocab) {
                            return Some(InstructionAst::MoveCounted {
                                actor: None,
                                count: Some(Box::new(cnt)),
                                up_to: true,
                                any_number: false,
                                subject: Box::new(subj),
                                to: dest.to_string(),
                            });
                        }
                        return None;
                    }
                    if obj_clean.starts_with("one of ") {
                        if let Ok(a @ SubjectRefAst::Anaphora(_)) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(InstructionAst::MoveZone {
                                subject: Box::new(a),
                                to: dest.to_string(),
                            });
                        }
                        return None;
                    }
                    let (cnt, obj_nocnt) = peel_leading_count(obj_clean, vocab);
                    let subj = if obj_clean == "the rest" || obj_clean == "the rest of the cards" {
                        SubjectRefAst::Anaphora("the_rest".to_string())
                    } else if let Ok(s) = parse_subject_or_target(obj_clean, vocab) {
                        s
                    } else if matches!(cnt, ValueExprAst::Const(1)) {
                        parse_subject_or_target(obj_nocnt, vocab).ok()?
                    } else if let Ok(s) = parse_subject_or_target(obj_nocnt, vocab) {
                        return Some(InstructionAst::MoveCounted {
                            actor: None,
                            count: Some(Box::new(cnt)),
                            up_to: false,
                            any_number: false,
                            subject: Box::new(s),
                            to: dest.to_string(),
                        });
                    } else {
                        return None;
                    };
                    return Some(InstructionAst::MoveZone {
                        subject: Box::new(subj),
                        to: dest.to_string(),
                    });
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
            ObjectFilterAst {
                any_card: true,
                ..ObjectFilterAst::default()
            }
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
    } else if let Some((who_raw, a)) = low
        .split_once(" may each search ")
        .or_else(|| low.split_once(" may search "))
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
    } else if let Some((who_raw, a)) = low
        .split_once(" searches ")
        .or_else(|| low.split_once(" search "))
    {
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
    let (tail_main, rest_to_zone) = if let Some((l, r)) = tail
        .split_once(" and the other ")
        .or_else(|| tail.split_once(" and the rest "))
    {
        let rz = r
            .split_once(", then shuffle")
            .or_else(|| r.split_once(", then shuffles"))
            .or_else(|| r.split_once(", shuffle"))
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
    let flt_clean = flt_part_raw
        .strip_prefix("any number of ")
        .or_else(|| flt_part_raw.strip_prefix("all "))
        .or_else(|| flt_part_raw.strip_prefix("up to "))
        .or_else(|| flt_part_raw.strip_prefix("exactly "))
        .unwrap_or(flt_part_raw);

    // Optional compound search target: "<flt1> and/or a/an <flt2>", "<flt1> and a/an <flt2>",
    // or "<flt1> or a/an <flt2>". The right side must parse as a valid search filter (named or typed),
    // which supports both "a card named A and/or a card named B" and "a card named A or an Equipment card"
    // while keeping single card names such as "Sword of Feast and Famine" intact.
    let (primary_raw, secondary_filter, secondary_is_or) = if let Some((left_f, right_f)) =
        flt_clean
            .split_once(" and/or a ")
            .or_else(|| flt_clean.split_once(" and/or an "))
        && let Some(sf) = parse_search_single_filter(right_f.trim(), vocab)
    {
        (left_f.trim(), Some(Box::new(sf)), false)
    } else if let Some((left_f, right_f)) = flt_clean
        .split_once(" and a ")
        .or_else(|| flt_clean.split_once(" and an "))
        && let Some(sf) = parse_search_single_filter(right_f.trim(), vocab)
    {
        (left_f.trim(), Some(Box::new(sf)), false)
    } else if let Some((left_f, right_f)) = flt_clean
        .split_once(" or a ")
        .or_else(|| flt_clean.split_once(" or an "))
        && let Some(sf) = parse_search_single_filter(right_f.trim(), vocab)
    {
        (left_f.trim(), Some(Box::new(sf)), true)
    } else {
        (flt_clean, None, false)
    };

    let (cnt, flt_raw) = if any_number {
        (ValueExprAst::Const(0), primary_raw)
    } else if let Some(after_num) = primary_raw
        .strip_prefix("a number of ")
        .or_else(|| primary_raw.strip_prefix("number of "))
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
        exact: false,
    };
    let wrapped = match opt_player {
        Some(PlayerRefAst::You) => InstructionAst::Optional(vec![search_ins]),
        Some(player) => InstructionAst::OptionalPlayer {
            player,
            body: vec![search_ins],
        },
        None => search_ins,
    };
    Some(match leading_subject_player {
        Some(subj_p) => InstructionAst::ForEachSubjectPlayer {
            players: Box::new(subj_p),
            body: vec![wrapped],
        },
        None => wrapped,
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
            Some(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PlayerStat {
                    player: who,
                    stat: crate::ast::PlayerStatKind::CardsInLibrary,
                }),
                denom: 2,
                rounding,
            })
        } else if let Some(val_raw) = after
            .strip_prefix("cards equal to ")
            .or_else(|| after.strip_prefix("a number of cards equal to "))
        {
            parse_value_expr(val_raw, vocab).ok()
        } else if let Some(cnt_raw) = after
            .strip_suffix(" cards")
            .or_else(|| after.strip_suffix(" card"))
        {
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
            return Some(InstructionAst::LookTopLibrary {
                reveal: true,
                library: "target".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            });
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
            return Some(InstructionAst::LookTopLibrary {
                reveal: true,
                library,
                count: Box::new(ValueExprAst::Const(1)),
            });
        }
        return Some(InstructionAst::PlayerLookTopLibrary {
            player: who,
            library,
            count: Box::new(ValueExprAst::Const(1)),
            reveal: true,
        });
    }
    if let Some(after) = low.strip_prefix("look at ")
        && let Some((cnt_raw, lib_rest)) = after.split_once(" cards from the top of ")
        && let Some(whose) = lib_rest.strip_suffix(" library")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::LookTopLibrary {
            reveal: false,
            library: whose.trim_end_matches("'s").to_string(),
            count: Box::new(cnt),
        });
    }
    if let Some((who_raw, rest)) = low
        .split_once(" reveals ")
        .or_else(|| low.split_once(" reveal "))
        && let Some(who) = parse_player_ref(who_raw)
    {
        if let Some(val_raw) = rest.strip_prefix("a number of cards from their hand equal to ")
            && let Ok(cnt) = parse_value_expr(val_raw, vocab)
        {
            if who == PlayerRefAst::You {
                return Some(InstructionAst::LookTopLibrary {
                    reveal: true,
                    library: "hand".to_string(),
                    count: Box::new(cnt),
                });
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library: "hand".to_string(),
                count: Box::new(cnt),
                reveal: true,
            });
        }
        if let Some(after_top) = rest.strip_prefix("the top ")
            && let Some((cnt_raw, lib_tail)) = after_top
                .split_once(" cards of ")
                .or_else(|| after_top.split_once(" card of "))
            && let Some(whose) = lib_tail.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            let library = whose.trim_end_matches("'s").to_string();
            if who == PlayerRefAst::You {
                return Some(InstructionAst::LookTopLibrary {
                    reveal: true,
                    library,
                    count: Box::new(cnt),
                });
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
                return Some(InstructionAst::LookTopLibrary {
                    reveal: true,
                    library: "hand".to_string(),
                    count: Box::new(cnt),
                });
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library: "hand".to_string(),
                count: Box::new(cnt),
                reveal: true,
            });
        }
    }
    if let Some((who_raw, rest)) = low
        .split_once(" reveals a card at random from ")
        .or_else(|| low.split_once(" reveal a card at random from "))
        && rest.ends_with(" hand")
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
                return Some(InstructionAst::LookTopLibrary {
                    reveal: false,
                    library,
                    count: Box::new(ValueExprAst::Const(1)),
                });
            }
            return Some(InstructionAst::PlayerLookTopLibrary {
                player: who,
                library,
                count: Box::new(ValueExprAst::Const(1)),
                reveal: false,
            });
        }
        if let Some((cnt_str, whose_lib)) = rest.split_once(" cards of ")
            && let Some(whose) = whose_lib.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_str, vocab)
        {
            let library = whose.trim_end_matches("'s").to_string();
            if who == PlayerRefAst::You {
                return Some(InstructionAst::LookTopLibrary {
                    reveal: false,
                    library,
                    count: Box::new(cnt),
                });
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
                return Some(InstructionAst::LookTopLibrary {
                    reveal,
                    library: whose.trim_end_matches("'s").to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                });
            }
            if let Some((cnt_str, whose_lib)) = rest.split_once(" cards of ")
                && let Some(whose) = whose_lib.strip_suffix(" library")
                && let Ok(cnt) = parse_value_expr(cnt_str, vocab)
            {
                return Some(InstructionAst::LookTopLibrary {
                    reveal,
                    library: whose.trim_end_matches("'s").to_string(),
                    count: Box::new(cnt),
                });
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
            let flt_raw = until_rest
                .strip_prefix("exile ")
                .or_else(|| until_rest.strip_prefix("reveal "))
                .unwrap_or(until_rest);
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

    // Wheel: "each player shuffles their hand and graveyard into their library, then draws N cards"
    if let Some((who_raw, rest)) = low
        .split_once(" shuffles their hand and graveyard into their library, then draws ")
        .or_else(|| {
            low.split_once(" shuffle their hand and graveyard into their library, then draw ")
        })
        && let Some(cnt_raw) = rest.strip_suffix(" cards")
        && let (Some(player), Ok(draw)) =
            (parse_player_ref(who_raw), parse_value_expr(cnt_raw, vocab))
    {
        return Some(InstructionAst::WheelShuffleHandAndGraveyard {
            player,
            draw: Box::new(draw),
        });
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
    if let Some((who_raw, rest)) = low
        .split_once(" shuffles ")
        .or_else(|| low.split_once(" shuffle "))
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
    if let Some((who_raw, hand_part)) = low
        .split_once(" reveals ")
        .or_else(|| low.split_once(" reveal "))
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
    if let Some((who_raw, rest)) = low
        .split_once(" looks at the top ")
        .or_else(|| low.split_once(" look at the top "))
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
            InstructionAst::LookTopLibrary {
                reveal: false,
                library: "your".to_string(),
                count: Box::new(cnt),
            },
            InstructionAst::SeparateIntoPiles {
                chooser,
                pile_count: 2,
                face_down_piles,
            },
        ]);
    }

    // Conditional wrapper around pile separation ("if <cond>, <pile_sep>")
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Some(inner) = parse_pile_separation_and_visibility(rest.trim(), vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: inner,
        }]);
    }

    // Compositional cast-up-to-N spells from among a filtered group without paying mana costs
    // (e.g. "<player> may cast [up to <N> ]spell[s] from among <filter> [without paying ...]")
    if let Some((who_raw, rest)) = low.split_once(" may cast ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_part, group_tail)) = rest
            .split_once(" spells from among ")
            .or_else(|| rest.split_once(" spell from among "))
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
            Some(CountLimitAst::UpTo(Box::new(ValueExprAst::Const(1))))
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
                flt.predicates.push(FilterPredicateAst::Exclude(Box::new(
                    SubjectRefAst::Anaphora("that exiled card".to_string()),
                )));
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
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                free,
                duration: "now".to_string(),
                count_limit,
                alt_cost: None,
            };
            return Some(vec![if player == PlayerRefAst::You {
                InstructionAst::Optional(vec![inner])
            } else {
                InstructionAst::OptionalPlayer {
                    player,
                    body: vec![inner],
                }
            }]);
        }
    }

    // Compositional: "exile <subj> [face down ]with <N> <kind> counter(s) on it, then <follow_up>"
    if let Some(after_ex) = low.strip_prefix("exile ")
        && let Some((head, tail)) = after_ex.split_once(", then ")
        && let Some((subj_and_vis, cnt_and_kind)) = head.split_once(" with ")
        && let Some(ck_raw) = cnt_and_kind
            .strip_suffix(" counters on it")
            .or_else(|| cnt_and_kind.strip_suffix(" counter on it"))
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
    if let Some((who_raw, rest)) = low
        .split_once(" turns face up ")
        .or_else(|| low.split_once(" turn face up "))
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
                    vec![InstructionAst::ForEachPlayer { player, body }]
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
            Some(InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(0)),
                dread: false,
                subject: Some(Box::new(SubjectRefAst::Anaphora(m_subj.trim().to_string()))),
            })
        } else {
            rest.strip_prefix("cloak ")
                .map(|c_subj| InstructionAst::Cloak {
                    count: Box::new(ValueExprAst::Const(0)),
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
                    subject: Box::new(SubjectRefAst::Anaphora("that pile".to_string())),
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
        && let Some(n_raw) = mv_tail
            .strip_suffix(" or greater this way")
            .or_else(|| mv_tail.strip_suffix(" or greater"))
        && let Ok(threshold) = parse_value_expr(n_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ExileOrRevealTopUntilTotalStat {
            exile: true,
            player,
            stat: StatKind::ManaValue,
            threshold: Box::new(threshold),
        }]);
    }
    None
}

pub(super) fn parse_zone_choice_exchange_extended(
    _s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Copy card in exile/zone ("copy <flt>" / "you may copy <flt>") (Arcane Savant, Reversal of Fortune)
    let (opt_copy, copy_core) = match low.strip_prefix("you may copy ") {
        Some(r) => (true, r),
        None => match low.strip_prefix("copy ") {
            Some(r) => (false, r),
            None => (false, ""),
        },
    };
    if !copy_core.is_empty()
        && let Ok(flt) = parse_object_filter(copy_core.trim(), vocab)
    {
        let ins = InstructionAst::Copy {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            exception: None,
            count: None,
        };
        return Some(if opt_copy {
            vec![InstructionAst::Optional(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 2. Create token copy of a filtered card ("create a token that's a copy of <flt>") (Caller of the Untamed)
    if let Some(src_raw) = low.strip_prefix("create a token that's a copy of ")
        && let Ok(flt) = parse_object_filter(src_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token: TokenSpecAst::Copy {
                count: Box::new(ValueExprAst::Const(1)),
                source: Box::new(SubjectRefAst::Group(Box::new(flt))),
                tapped: false,
                attacking: false,
                exception: None,
            },
        }]);
    }

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
        let ins = InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::Exile {
                actor: Some(PlayerRefAst::You),
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                random: false,
                face_down: false,
                into_pile: false,
            }],
        };
        return Some(if opt_ex {
            vec![InstructionAst::Optional(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 4. Exile top card of each player's library with a counter (Evelyn, the Covetous)
    if let Some(after_ex) = low.strip_prefix("exile the top card of each player's library with a ")
        && let Some(kind_raw) = after_ex.strip_suffix(" counter on it")
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::ExileWithCounters {
                subject: Box::new(SubjectRefAst::Anaphora(
                    "the top card of their library".to_string(),
                )),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down: false,
            }],
        }]);
    }

    // 5. Exile card from hand until ~ leaves the battlefield (Intellect Devourer)
    if let Some((who_raw, rest)) = low
        .split_once(" exiles ")
        .or_else(|| low.split_once(" exile "))
        && let Some(subj_raw) = rest.strip_suffix(" until ~ leaves the battlefield")
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![InstructionAst::ExileUntilLeaves {
                subject: Box::new(subj),
            }],
        }]);
    }

    // 6. Exile card from hand and may play for as long as it remains exiled (Lightstall Inquisitor)
    if let Some((who_raw, rest)) = low
        .split_once(" exiles ")
        .or_else(|| low.split_once(" exile "))
        && let Some((subj_raw, play_tail)) = rest.split_once(" and may play ")
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![
                InstructionAst::Exile {
                    actor: Some(PlayerRefAst::ThatPlayer),
                    subject: Box::new(subj),
                    random: false,
                    face_down: false,
                    into_pile: false,
                },
                InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::ThatPlayer,
                    cast_only: false,
                    subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                    free: false,
                    duration: play_tail.trim().to_string(),
                    count_limit: None,
                    alt_cost: None,
                },
            ],
        }]);
    }

    // 7. Exile cards from top of library equal to <val> (Bone Mask)
    if let Some(val_raw) = low.strip_prefix("exile cards from the top of your library equal to ")
        && let Ok(count) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ExileTopLibrary {
            actor: Some(PlayerRefAst::You),
            whose: "your".to_string(),
            count: Box::new(count),
            face_down: false,
        }]);
    }

    // 8. Exile N of them at random (Orcish Librarian)
    if let Some(after) = low.strip_prefix("exile ")
        && let Some(cnt_raw) = after.strip_suffix(" of them at random")
        && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(count)),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora("them".to_string())),
            to: "exile_random".to_string(),
        }]);
    }

    // 9. Put that many cards from top of library into hand / Put bottom card into graveyard (Scroll Rack, Cellar Door)
    if let Some(after) = low.strip_prefix("put that many cards from the top of your library into ")
    {
        return Some(vec![InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(ValueExprAst::PriorResult)),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora(
                "the top cards of your library".to_string(),
            )),
            to: after.trim().to_string(),
        }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" puts the bottom card of their library into ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::MoveCounted {
            actor: Some(player),
            count: Some(Box::new(ValueExprAst::Const(1))),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora(
                "the bottom card of their library".to_string(),
            )),
            to: rest.trim().to_string(),
        }]);
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
                count: Box::new(ValueExprAst::Const(1)),
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
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                from_top,
            },
        ]);
    }

    // 11. Manifest a number of cards equal to <val> / Manifest one of those cards (Omarthis, Write into Being)
    if let Some(val_raw) =
        low.strip_prefix("manifest a number of cards from the top of your library equal to ")
        && let Ok(count) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Manifest {
            count: Box::new(count),
            dread: false,
            subject: None,
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("manifest ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Manifest {
            count: Box::new(ValueExprAst::Const(1)),
            dread: false,
            subject: Some(Box::new(subj)),
        }]);
    }

    // 12. The owners of those cards shuffle them into their libraries (Turn the Earth)
    if let Some(after) = low.strip_prefix("the owners of ")
        && let Some((subj_raw, tail)) = after.split_once(" shuffle ")
        && tail.ends_with(" into their libraries")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        }]);
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
        return Some(vec![InstructionAst::ReturnZone {
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
        }]);
    }

    // 14. Return a card at random from your graveyard to your hand (Fossil Find)
    if let Some(after) = low.strip_prefix("return ")
        && let Some((subj_raw, rest)) = after.split_once(" at random from ")
        && let Some((zone_raw, to_raw)) = rest.split_once(" to ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReturnZone {
            subject: Box::new(subj),
            from_zone: Some(zone_raw.trim().to_string()),
            to: to_raw.trim().to_string(),
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: true,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            actor: None,
        }]);
    }

    // 15. Return ~ to the battlefield under your control as a <type> (Takklemaggot)
    if let Some(after) = low.strip_prefix("return ")
        && let Some((subj_raw, type_raw)) =
            after.split_once(" to the battlefield under your control as ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![
            InstructionAst::ReturnZone {
                subject: Box::new(subj.clone()),
                from_zone: None,
                to: "battlefield under your control".to_string(),
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
            },
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj),
                    type_or_subtype: type_raw.trim().to_string(),
                    additive: false,
                    condition: None,
                }),
                duration: DurationAst::Permanent,
            },
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
        return Some(vec![InstructionAst::Sacrifice {
            player,
            count: Some(Box::new(count)),
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            except: None,
            half: false,
            rounding: Some(Rounding::Up),
            chooser: Some(PlayerRefAst::ThatPlayer),
        }]);
    }

    // 17. Choose-per-filter / sacrifice-rest (Global Ruin, Stick Together, Guided Passage, Distended Mindbender)
    if let Some((who_raw, rest)) = low
        .split_once(" chooses from ")
        .or_else(|| low.split_once(" choose from "))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (sac_rest, core) = match rest.strip_suffix(", then sacrifices the rest") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Some(after_src) = core.strip_prefix("the lands they control ")
            && let Ok(flt) = parse_object_filter(after_src.trim(), vocab)
        {
            return Some(vec![InstructionAst::ChoosePerFilter {
                player,
                categories: vec![flt],
                sacrifice_rest: sac_rest,
            }]);
        }
        if let Some(after_among) = core
            .strip_prefix("among them ")
            .or_else(|| core.strip_prefix("it "))
        {
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
                return Some(vec![InstructionAst::ChoosePerFilter {
                    player,
                    categories: cats,
                    sacrifice_rest: sac_rest,
                }]);
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
        return Some(vec![InstructionAst::ChoosePerFilter {
            player,
            categories: vec![flt],
            sacrifice_rest: true,
        }]);
    }

    // 18. Starting with you, each player separates <subj> into <N> piles (Whims of the Fates)
    if let Some(after) = low.strip_prefix("starting with you, ")
        && let Some((who_raw, rest)) = after.split_once(" separates ")
        && let Some((subj_raw, pile_raw)) = rest.split_once(" into ")
        && let Some(cnt_tok) = pile_raw.strip_suffix(" piles")
        && let (Some(chooser), Ok(subject), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_number_word(cnt_tok.trim()),
        )
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SeparateIntoPiles {
                chooser,
                subject: Box::new(subject),
                pile_count: u8::try_from(n).unwrap_or(3),
            },
        )]);
    }

    // 19. Exile the pile of an opponent's choice and return the other to <zone> (Death or Glory)
    if let Some(dest) =
        low.strip_prefix("exile the pile of an opponent's choice and return the other to ")
    {
        return Some(vec![
            InstructionAst::PileAction {
                action: "exile".to_string(),
                pile: PileRefAst::ChosenPile,
                filter: None,
            },
            InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::PileRef(PileRefAst::OtherPile)),
                from_zone: None,
                to: dest.trim().to_string(),
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
            },
        ]);
    }

    // 20. Choose <flt> with mana value 1 ..., then do the same for <flt> with mana value 2 and 3
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((first_raw, rest)) = after.split_once(", then do the same for ")
        && let Some((base_raw, nums_raw)) = rest.split_once(" with mana value ")
        && let Some((n2_s, n3_s)) = nums_raw.split_once(" and ")
        && let (Ok(f1), Ok(base_flt), Ok(n2), Ok(n3)) = (
            parse_object_filter(first_raw.trim(), vocab),
            parse_object_filter(base_raw.trim(), vocab),
            n2_s.trim().parse::<i32>(),
            n3_s.trim().parse::<i32>(),
        )
    {
        let mut f2 = base_flt.clone();
        f2.zone = f1.zone;
        f2.stat_bounds.push(StatBoundAst {
            stat: StatKind::ManaValue,
            op: CompareOp::Eq,
            value: Box::new(ValueExprAst::Const(n2)),
        });
        let mut f3 = base_flt;
        f3.zone = f1.zone;
        f3.stat_bounds.push(StatBoundAst {
            stat: StatKind::ManaValue,
            op: CompareOp::Eq,
            value: Box::new(ValueExprAst::Const(n3)),
        });
        return Some(vec![
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(f1))),
            },
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(f2))),
            },
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(f3))),
            },
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
                count: Box::new(ValueExprAst::Const(1)),
                random,
                group: Box::new(group),
            };
            return Some(
                if matches!(
                    player,
                    PlayerRefAst::EachOpponent | PlayerRefAst::EachPlayer
                ) {
                    vec![InstructionAst::ForEachPlayer {
                        player,
                        body: vec![act],
                    }]
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
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::Control,
            left: Box::new(SubjectRefAst::Group(Box::new(flt))),
            right: None,
        }]);
    }

    // 23. For any number of opponents, <eff> (Windgrace's Judgment)
    if let Some(body_low) = low.strip_prefix("for any number of opponents, ")
        && let Some(body) = parse_sub_effect_extended(body_low, body_low, vocab)
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachOpponent,
            body,
        }]);
    }

    // 24. Distribute counters among one, two, or three <flt> (Invasion of Alara)
    if let Some(after) = low.strip_prefix("distribute ")
        && let Some((cnt_and_kind, tgts_raw)) = after.split_once(" counters among ")
        && let Some((cnt_tok, kind_raw)) = cnt_and_kind.trim().split_once(' ')
        && let Some(n) = parse_number_word(cnt_tok.trim())
    {
        let clean_tgt = tgts_raw
            .trim()
            .strip_prefix("one, two, or three ")
            .or_else(|| tgts_raw.trim().strip_prefix("one or two "))
            .unwrap_or(tgts_raw.trim());
        if let Ok(flt) = parse_object_filter(clean_tgt, vocab) {
            return Some(vec![InstructionAst::DistributeCounters {
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(n)),
                targets: Box::new(SubjectRefAst::Group(Box::new(flt))),
            }]);
        }
    }

    // 25. Gain control for as long as <cond> (Garland, Royal Kidnapper)
    if let Some(after) = low.strip_prefix("gain control of ")
        && let Some((subj_raw, cond_raw)) = after.split_once(" for as long as ")
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject: Box::new(subj),
            duration: DurationAst::ForAsLongAs(Box::new(cond)),
        }]);
    }

    // 26. Tap up to X target <flt>, where X is <val> (Make a _____ Splash)
    if let Some((action_raw, x_raw)) = low.split_once(", where x is ")
        && let (Ok(body), Ok(x_expr)) = (
            parse_atomic_effect(action_raw.trim(), vocab),
            parse_value_expr(x_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::BindX {
            x_expr: Box::new(x_expr),
            body,
        }]);
    }

    // 27. Leave <subj> in <zone> (Deliver Unto Evil)
    if let Some(after) = low.strip_prefix("leave ")
        && let Some((subj_raw, zone_raw)) = after.split_once(" in ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: format!("stay_in:{}", zone_raw.trim()),
        }]);
    }

    // 28. Look at the top card(s) of <player>'s library (Tahngarth's Glare, look_and_exile_face_down)
    if let Some((who_raw, rest)) = low.split_once(" looks at the top ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_part, lib_part)) = rest.split_once(" of ")
        && let Some(whose_raw) = lib_part.strip_suffix(" library")
    {
        let count = if cnt_part.trim() == "card" {
            ValueExprAst::Const(1)
        } else {
            let num_s = cnt_part
                .trim()
                .strip_suffix(" cards")
                .unwrap_or(cnt_part.trim());
            ValueExprAst::Const(parse_number_word(num_s.trim())?)
        };
        return Some(vec![InstructionAst::PlayerLookTopLibrary {
            player,
            library: whose_raw.trim().to_string(),
            count: Box::new(count),
            reveal: false,
        }]);
    }

    // 29. "<player> destroys <N> <flt>" (Burning of Xinye)
    if let Some((who_raw, rest)) = low
        .split_once(" destroys ")
        .or_else(|| low.split_once(" destroy "))
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_tok, flt_raw)) = rest.trim().split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_tok.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![
                InstructionAst::ChooseFromGroup {
                    count: Box::new(ValueExprAst::Const(n)),
                    random: false,
                    group: Box::new(SubjectRefAst::Group(Box::new(flt))),
                },
                InstructionAst::Destroy {
                    subject: Box::new(SubjectRefAst::Anaphora("those permanents".to_string())),
                    random: false,
                    except: None,
                },
            ],
        }]);
    }

    // 30. "<player> draws as many cards as they discarded this way" (Forget)
    if let Some(after) = low.strip_prefix("draws as many cards as ")
        && after.ends_with(" discarded this way")
    {
        return Some(vec![InstructionAst::Draw {
            player: PlayerRefAst::ThatPlayer,
            count: Box::new(ValueExprAst::PriorResult),
            up_to: false,
        }]);
    }

    // 31. "<player> each discard their hands" (Wheel and Deal)
    if let Some(who_raw) = low.strip_suffix(" each discard their hands")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![InstructionAst::DiscardHand {
                player: PlayerRefAst::ThatPlayer,
            }],
        }]);
    }

    // 32. "another player of your choice may gain control of <subj>" (Stardew Valley)
    if let Some(subj_raw) = low.strip_prefix("another player of your choice may gain control of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::OptionalPlayer {
            player: PlayerRefAst::EachOtherPlayer,
            body: vec![InstructionAst::GainControl {
                controller: PlayerRefAst::ThatPlayer,
                subject: Box::new(subj),
                duration: dur.clone(),
            }],
        }]);
    }

    None
}
