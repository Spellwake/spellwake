//! Combat, damage, P/T modification, life, counters, tokens, animation, keyword/ability grants,
//! object partitions, phasing, and blocker-swap effect parsers.

use super::{
    parse_atomic_effect, parse_comparison_condition, parse_effect_body, split_leading_player_verb,
};
use crate::ast::{
    CardTypeMask, CombatState, CombatSwapPerspective, CompareOp, ConditionAst, ControllerRelation,
    CostAtomAst, DelayedStepScope, DieRollContextAst, DurationAst, ExchangeKind,
    FilterPredicateAst, GameMarker, InstructionAst, ManaSpecAst, ObjectFilterAst,
    ObjectFilterFlags, ObjectPartitionKind, PileRefAst, PlayerRefAst, PriorActionRefKind, Rounding,
    StatKind, StaticAbilityAst, SubjectRefAst, TargetCountAst, TokenSpecAst, TurnStepAst,
    ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, peel_leading_count};
use crate::combinators::{
    anaphora, any_card_flt, any_perm, box_anaphora, cond_cmp, grp, ins_ability_restr, ins_add_mana,
    ins_animate, ins_choose_group, ins_combat_rule, ins_counters, ins_create_token, ins_delayed,
    ins_dmg, ins_draw, ins_for_each_player, ins_gain_control, ins_grant_ab, ins_grant_kw, ins_if,
    ins_life, ins_move, ins_opt_player, ins_opt1, ins_pile_action, ins_player_counters,
    ins_rm_counters, ins_sac, ins_temp_static, ins_unless, obj_type, parse_pile_visibility,
    parse_two_quoted_piles, self_obj, some1, split_once_any, strip_prefix_any, strip_suffix_any,
    val_div, val_obj_stat, vconst,
};
use crate::compile_granted_ability;
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_token_creation_rich, parse_value_expr,
};
use crate::preprocess::{parse_base_pt, parse_pt_mod};
use crate::vocab::{Vocabulary, parse_number_word};

pub(super) fn try_parse_deal_damage(s: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    let low = s.to_ascii_lowercase();
    let (idx, verb_len) = if let Some(i) = low.find(" deals ") {
        (i, 7)
    } else if let Some(i) = low.find(" each deal ") {
        (i, 11)
    } else {
        let i = low.find(" deal ")?;
        (i, 6)
    };
    let src = parse_subject_or_target(&s[..idx], vocab).ok()?;
    let after = s[idx + verb_len..].trim();
    let alow = after.to_ascii_lowercase();

    // Compound: "<amt1> damage to <t1> and <amt2> damage[, rounded up/down,] to <t2>"
    if let Some((left, right)) = alow.split_once(" and ")
        && (left.contains(" damage to ")
            || left.contains(" damage, rounded up, to ")
            || left.contains(" damage, rounded down, to "))
        && (right.contains(" damage to ")
            || right.contains(" damage, rounded up, to ")
            || right.contains(" damage, rounded down, to "))
        && let (Some(a), Some(b)) = (
            try_parse_deal_damage(&format!("~ deals {left}"), vocab),
            try_parse_deal_damage(&format!("~ deals {right}"), vocab),
        )
    {
        let mut combined = Vec::with_capacity(a.len() + b.len());
        for ins in a.into_iter().chain(b) {
            if let InstructionAst::DealDamage { target, amount, .. } = ins {
                combined.push(InstructionAst::DealDamage {
                    source: Box::new(src.clone()),
                    target,
                    amount,
                });
            }
        }
        return Some(combined);
    }

    // "damage equal to <val> divided as you choose among <targets>" (Living Inferno, Captain America, Orca)
    if let Some(rest) = alow.strip_prefix("damage equal to ")
        && let Some((val_raw, targets_desc)) = rest.split_once(" divided as you choose among ")
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
    {
        return some1(InstructionAst::DealDamageDivided {
            source: Box::new(src),
            amount: Box::new(amt),
            targets_desc: targets_desc.trim().to_string(),
            divider: None,
        });
    }
    // "damage equal to <val> to <t1> and that much damage to <t2>" (Dragonspark Reactor)
    if let Some(rest) = alow.strip_prefix("damage equal to ")
        && let Some((left_part, t2_raw)) = rest.split_once(" and that much damage to ")
        && let Some((val_raw, t1_raw)) = left_part.split_once(" to ")
        && let (Ok(amt), Ok(t1), Ok(t2)) = (
            parse_value_expr(val_raw, vocab),
            parse_subject_or_target(t1_raw, vocab),
            parse_subject_or_target(t2_raw, vocab),
        )
    {
        return Some(vec![
            ins_dmg(src.clone(), t1, amt.clone()),
            ins_dmg(src, t2, amt),
        ]);
    }

    // "damage equal to <val> to <tgt>" (try split_once first, then rsplit_once for "... attached to it to any target", Tetsuo)
    if let Some(rest) = alow.strip_prefix("damage equal to ") {
        for split in [rest.split_once(" to "), rest.rsplit_once(" to ")] {
            if let Some((val_raw, tgt_raw)) = split
                && let (Ok(amt), Ok(tgt)) = (
                    parse_value_expr(val_raw, vocab),
                    parse_subject_or_target(tgt_raw, vocab),
                )
            {
                return Some(vec![ins_dmg(src, tgt, amt)]);
            }
        }
    }

    // "damage to <tgt> equal to <val>"
    if let Some(rest) = alow.strip_prefix("damage to ")
        && let Some((tgt_raw, val_raw)) = rest.split_once(" equal to ")
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
        && let Ok(tgt) = parse_subject_or_target(tgt_raw, vocab)
    {
        return Some(vec![ins_dmg(src, tgt, amt)]);
    }

    // Divided damage: "<amt> damage divided as you choose among <targets>" or "divided evenly, rounded down, among <targets>"
    if let Some((amt_raw, targets_desc)) = alow
        .split_once(" damage divided as you choose among ")
        .or_else(|| alow.split_once(" damage divided evenly, rounded down, among "))
        .or_else(|| alow.split_once(" damage divided evenly, rounded up, among "))
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return some1(InstructionAst::DealDamageDivided {
            source: Box::new(src),
            amount: Box::new(amt),
            targets_desc: targets_desc.trim().to_string(),
            divider: None,
        });
    }

    // "<amt> damage" (when trailing " instead" was peeled by parse_atomic_effect!)
    if let Some(amt_raw) = alow.strip_suffix(" damage")
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return some1(ins_dmg(src, anaphora("previous_target"), amt));
    }

    // "<amt> damage[, rounded up/down,] to <tgt> [unless <cond>] [at end of combat] [for each <filter>]"
    if let Some((amt_raw_base, round_suf, rest)) = alow
        .split_once(" damage, rounded up, to ")
        .map(|(a, r)| (a, Some(", rounded up"), r))
        .or_else(|| {
            alow.split_once(" damage, rounded down, to ")
                .map(|(a, r)| (a, Some(", rounded down"), r))
        })
        .or_else(|| alow.split_once(" damage to ").map(|(a, r)| (a, None, r)))
    {
        let amt_owned = match round_suf {
            Some(rs) => format!("{amt_raw_base}{rs}"),
            None => amt_raw_base.to_string(),
        };
        let mut amt = parse_value_expr(&amt_owned, vocab).ok()?;
        let (rest_nounless, unless_cond) = match rest.split_once(" unless ") {
            Some((before, c_raw)) => match parse_condition(c_raw, vocab) {
                Ok(c) => (before, Some(c)),
                Err(_) => (rest, None),
            },
            None => (rest, None),
        };
        let (rest_nodelay, delayed_eoc) = match rest_nounless.strip_suffix(" at end of combat") {
            Some(c) => (c, true),
            None => (rest_nounless, false),
        };
        let (tgt_raw, fe_opt) = match rest_nodelay.split_once(" for each ") {
            Some((t, fe)) => (t, Some(fe)),
            None => (rest_nodelay, None),
        };
        if let Some(fe) = fe_opt {
            let fe_val = parse_for_each_value_expr(fe, vocab).ok()?;
            amt = ValueExprAst::Mul(Box::new(amt), Box::new(fe_val));
        }
        let tgt = parse_subject_or_target(tgt_raw, vocab).ok()?;
        let ins = ins_dmg(src, tgt, amt);
        let ins = match unless_cond {
            Some(cond) => ins_unless(vec![ins], None, None, None, Some(cond)),
            None => ins,
        };
        return Some(if delayed_eoc {
            vec![ins_delayed(
                TurnStepAst::EndOfCombat,
                DelayedStepScope::Next,
                vec![ins],
            )]
        } else {
            vec![ins]
        });
    }
    None
}

pub(super) fn try_parse_modify_pt(
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let low = s_nodur.to_ascii_lowercase();
    let (idx, vlen) = if let Some(i) = low.find(" each get an additional ") {
        (i, 24)
    } else if let Some(i) = low.find(" gets an additional ") {
        (i, 20)
    } else if let Some(i) = low.find(" get an additional ") {
        (i, 19)
    } else if let Some(i) = low.find(" each get ") {
        (i, 10)
    } else if let Some(i) = low.find(" gets ") {
        (i, 6)
    } else {
        let i = low.find(" get ")?;
        (i, 5)
    };
    let subj = parse_subject_or_target(&s_nodur[..idx], vocab).ok()?;
    let after = s_nodur[idx + vlen..].trim();
    // "<subj> gets +1/-1 or -1/+1" choice pump (Morphling / Endling)
    if let Some((a_tok, b_tok)) = after.split_once(" or ")
        && let (Some(pa), Some(pb)) = (parse_pt_mod(a_tok.trim()), parse_pt_mod(b_tok.trim()))
    {
        return some1(InstructionAst::ModifyPTChoice {
            subject: Box::new(subj),
            choices: vec![pa, pb],
            duration: dur.clone(),
        });
    }
    let (pt_tok, rest) = match after.split_once(' ') {
        Some((p, r)) => (p, r.trim()),
        None => (after, ""),
    };
    let (dx, dy) = parse_pt_mod(pt_tok)?;

    let mut out = Vec::new();
    let rlow = rest.to_ascii_lowercase();
    let mut scale = None;
    let mut kw_part = None;
    let mut ability_text = None;
    let mut extra_combat_rule = None;

    if let Some(fe) = strip_prefix_any(&rlow, &["for each ", "until end of turn for each "]) {
        let (fe_core, extra_rule, trailing_gains) =
            if let Some(c) = fe.strip_suffix(" and can't be blocked") {
                (c, Some("can't be blocked"), None)
            } else if let Some((c, g)) = split_once_any(&fe, &[" and gains ", " and gain "]) {
                (c, None, Some(g.trim().to_string()))
            } else {
                (fe, None, None)
            };
        scale = Some(Box::new(parse_for_each_value_expr(fe_core, vocab).ok()?));
        extra_combat_rule = extra_rule;
        kw_part = trailing_gains;
    } else if let Some(after_g) = strip_prefix_any(&rest, &["and gains ", "and gain "]) {
        if after_g.starts_with('"') && after_g.ends_with('"') {
            ability_text = Some(after_g.trim_matches('"').to_string());
        } else if let Some((kw_side, ab_side)) = after_g.split_once(" and \"")
            && let Some(ab_inner) = ab_side.strip_suffix('"')
        {
            kw_part = Some(kw_side.to_ascii_lowercase());
            ability_text = Some(ab_inner.to_string());
        } else if after_g.eq_ignore_ascii_case("all creature types") {
            extra_combat_rule = Some("all creature types");
        } else {
            kw_part = Some(after_g.to_ascii_lowercase());
        }
    } else if rlow == "until end of turn and can't be blocked" || rlow == "and can't be blocked" {
        extra_combat_rule = Some("can't be blocked");
    } else if !rlow.is_empty() {
        return None;
    }

    out.push(InstructionAst::ModifyPT {
        subject: Box::new(subj.clone()),
        dx,
        dy,
        duration: dur.clone(),
        scale,
    });
    if let Some(kp) = kw_part {
        let kws = parse_keyword_list(&kp, vocab)?;
        out.push(ins_grant_kw(subj.clone(), kws, dur.clone()));
    }
    if let Some(ab_txt) = ability_text {
        let compiled = compile_granted_ability(&ab_txt, vocab);
        out.push(ins_grant_ab(subj.clone(), ab_txt, compiled, dur.clone()));
    }
    if let Some(rule) = extra_combat_rule {
        out.push(ins_combat_rule(subj, rule.to_string(), dur.clone()));
    }
    Some(out)
}

pub(super) fn try_parse_modify_abilities(
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let low = s_nodur.to_ascii_lowercase();
    for (needle, is_gain) in [
        (" each gain ", true),
        (" also gains ", true),
        (" also gain ", true),
        (" gains ", true),
        (" gain ", true),
        (" each lose ", false),
        (" loses ", false),
        (" lose ", false),
    ] {
        if let Some(idx) = low.find(needle) {
            let rhs = s_nodur[idx + needle.len()..].trim();
            let rlow = rhs.to_ascii_lowercase();
            // Word-boundary check so "lifelink" is never mistaken for "life"
            if rlow == "life"
                || rlow.ends_with(" life")
                || rlow.starts_with("life ")
                || rlow.starts_with("control ")
            {
                continue;
            }
            let subj = parse_subject_or_target(&s_nodur[..idx], vocab).ok()?;
            // "<subj> gains that ability" / "<subj> loses \"<ab>\"" (Angelic Skirmisher, Glittering Lion)
            if is_gain && rlow == "that ability" {
                return Some(vec![ins_grant_ab(subj, "that ability", None, dur.clone())]);
            }
            if !is_gain && rhs.starts_with('"') && rhs.ends_with('"') {
                return some1(InstructionAst::RemoveKeywords {
                    subject: Box::new(subj),
                    keywords: vec![rhs.trim_matches('"').to_string()],
                    duration: dur.clone(),
                });
            }
            // "<subj> loses all abilities and has/have base power and toughness P/T"
            if !is_gain
                && let Some(pt_raw) = rlow
                    .strip_prefix("all abilities and has base power and toughness ")
                    .or_else(|| {
                        rlow.strip_prefix("all abilities and have base power and toughness ")
                    })
                && let Some((p, t)) = parse_base_pt(pt_raw.trim())
            {
                return Some(vec![
                    InstructionAst::RemoveAllAbilities {
                        subject: Box::new(subj.clone()),
                        duration: dur.clone(),
                    },
                    ins_animate(subj, p, t, None, Vec::new(), dur.clone()),
                ]);
            }
            // "<subj> lose(s) <kws> and can't have <kws>" (Arcane Lighthouse, Shadowspear, Bonds of Mortality)
            if !is_gain
                && let Some((lose_part, cant_part)) = rlow.split_once(" and can't have ")
                && let (Some(lkws), Some(_ckws)) = (
                    parse_keyword_list(lose_part, vocab),
                    parse_keyword_list(cant_part, vocab),
                )
            {
                return Some(vec![
                    InstructionAst::RemoveKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: lkws,
                        duration: dur.clone(),
                    },
                    ins_combat_rule(subj, format!("cant_have:{cant_part}"), dur.clone()),
                ]);
            }
            // "<subj> loses <kw1> and gains <kw2>"
            if !is_gain
                && let Some((lose_part, gain_part)) =
                    split_once_any(&rlow, &[" and gains ", " and gain "])
                && let (Some(lkws), Some(gkws)) = (
                    parse_keyword_list(lose_part, vocab),
                    parse_keyword_list(gain_part, vocab),
                )
            {
                return Some(vec![
                    InstructionAst::RemoveKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: lkws,
                        duration: dur.clone(),
                    },
                    ins_grant_kw(subj, gkws, dur.clone()),
                ]);
            }
            // "<subj> gains <kws> and \"<ability>\""
            if is_gain
                && let Some((kw_side, ab_side)) = rhs.split_once(" and \"")
                && let Some(ab_inner) = ab_side.strip_suffix('"')
                && let Some(kws) = parse_keyword_list(kw_side, vocab)
            {
                let compiled = compile_granted_ability(ab_inner, vocab);
                return Some(vec![
                    ins_grant_kw(subj.clone(), kws, dur.clone()),
                    ins_grant_ab(subj, ab_inner.to_string(), compiled, dur.clone()),
                ]);
            }
            // "<subj> gains <kws> and gets +X/+Y [for each ...]"
            if is_gain
                && let Some((kw_side, pt_side)) = split_once_any(&rhs, &[" and gets ", " and get "])
                && let Some(kws) = parse_keyword_list(kw_side, vocab)
                && let Some(mut pt_ins) =
                    try_parse_modify_pt(&format!("~ gets {pt_side}"), dur, vocab)
            {
                for ins in &mut pt_ins {
                    if let InstructionAst::ModifyPT { subject, .. } = ins {
                        **subject = subj.clone();
                    }
                }
                let mut out = vec![InstructionAst::GrantKeywords {
                    subject: Box::new(subj),
                    keywords: kws,
                    duration: dur.clone(),
                }];
                out.extend(pt_ins);
                return Some(out);
            }
            if rlow == "all abilities" && !is_gain {
                return some1(InstructionAst::RemoveAllAbilities {
                    subject: Box::new(subj),
                    duration: dur.clone(),
                });
            }
            if rlow.starts_with("protection from ") && is_gain {
                return Some(vec![ins_grant_kw(subj, vec![rlow], dur.clone())]);
            }
            let (kw_target, is_choice) = if let Some(rest_kw) =
                strip_prefix_any(&rhs, &["your choice of ", "Your choice of "])
            {
                (rest_kw, true)
            } else {
                (rhs, false)
            };
            if let Some(kws) = parse_keyword_list(kw_target, vocab) {
                if is_choice && kws.len() >= 2 {
                    let branches = kws
                        .into_iter()
                        .map(|kw| {
                            vec![if is_gain {
                                ins_grant_kw(subj.clone(), vec![kw], dur.clone())
                            } else {
                                InstructionAst::RemoveKeywords {
                                    subject: Box::new(subj.clone()),
                                    keywords: vec![kw],
                                    duration: dur.clone(),
                                }
                            }]
                        })
                        .collect();
                    return Some(vec![InstructionAst::Choice(branches)]);
                }
                return some1(if is_gain {
                    ins_grant_kw(subj, kws, dur.clone())
                } else {
                    InstructionAst::RemoveKeywords {
                        subject: Box::new(subj),
                        keywords: kws,
                        duration: dur.clone(),
                    }
                });
            }
        }
    }
    None
}

pub(super) fn try_parse_life(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // DrainLife: "each opponent loses N life and you gain N/that much life"
    if let Some((left, right)) = low.split_once(" life and you gain ")
        && let Some(gain_str) = right.strip_suffix(" life")
        && let Some((who_raw, loss_str)) = split_once_any(&left, &[" loses ", " lose "])
        && let (Some(who), Ok(loss), Ok(gain)) = (
            parse_player_ref(who_raw),
            parse_value_expr(loss_str, vocab),
            parse_value_expr(gain_str, vocab),
        )
    {
        return Some(vec![
            ins_life(who, loss, false),
            ins_life(PlayerRefAst::You, gain, true),
        ]);
    }

    for (verb_base, verb_3p, is_gain) in [("gain", "gains", true), ("lose", "loses", false)] {
        if let Some((who, rhs)) = split_leading_player_verb(low, verb_base, verb_3p) {
            if let Some(val_raw) = rhs.strip_prefix("life equal to ") {
                let amt = if val_raw == "the life lost this way"
                    || val_raw == "the damage dealt this way"
                    || val_raw == "the damage prevented this way"
                {
                    ValueExprAst::PriorResult
                } else {
                    parse_value_expr(val_raw, vocab).ok()?
                };
                return Some(vec![ins_life(who, amt, is_gain)]);
            }
            if let Some((amt_str, fe_str)) = rhs.split_once(" life for each ")
                && let (Ok(base), Ok(fe_val)) = (
                    parse_value_expr(amt_str, vocab),
                    parse_for_each_value_expr(fe_str, vocab),
                )
            {
                return some1(ins_life(
                    who,
                    ValueExprAst::Mul(Box::new(base), Box::new(fe_val)),
                    is_gain,
                ));
            }
            if let Some(amt_str) = rhs.strip_suffix(" life")
                && let Ok(amt) = parse_value_expr(amt_str, vocab)
            {
                return Some(vec![ins_life(who, amt, is_gain)]);
            }
            if rhs.starts_with("half ")
                && let Ok(amt) = parse_value_expr(rhs, vocab)
            {
                return Some(vec![ins_life(who, amt, is_gain)]);
            }
        }
    }
    None
}

pub(super) fn try_parse_counters(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    // MoveCounters: "move <cnt> <kind> counter(s) from <src> onto/to <dst>"
    if let Some(after) = low.strip_prefix("move ")
        && let Some((left, rest)) = split_once_any(&after, &[" counters from ", " counter from "])
        && let Some((src_raw, dst_raw)) = split_once_any(&rest, &[" onto ", " to "])
        && let (Ok(from), Ok(to)) = (
            parse_subject_or_target(src_raw, vocab),
            parse_subject_or_target(dst_raw, vocab),
        )
    {
        if let Some(kind) = left.strip_prefix("any number of ") {
            return Some(InstructionAst::MoveCounters {
                from: Box::new(from),
                to: Box::new(to),
                counter_kind: kind.trim().to_string(),
                all: false,
                any_number: true,
                count: None,
            });
        }
        if let Some(kind) = left.strip_prefix("all ") {
            return Some(InstructionAst::MoveCounters {
                from: Box::new(from),
                to: Box::new(to),
                counter_kind: kind.trim().to_string(),
                all: true,
                any_number: false,
                count: None,
            });
        }
        let (cnt, kind) = peel_leading_count(left, vocab);
        let counter_kind = if kind.is_empty() {
            "any".to_string()
        } else {
            kind.to_string()
        };
        return Some(InstructionAst::MoveCounters {
            from: Box::new(from),
            to: Box::new(to),
            counter_kind,
            all: false,
            any_number: false,
            count: Some(Box::new(cnt)),
        });
    }

    // PutCounters: "[<player> ]put(s) a number of <kind> counters on <subj> equal to <val>"
    // or "put a number of <kind> counters equal to <val> on <subj>" (Szarel, Jenova, Oversimplify)
    if let Some((_, after_put)) = split_leading_player_verb(low, "put", "puts")
        && let Some(after) = after_put.strip_prefix("a number of ")
    {
        if let Some((kind, rest)) = split_once_any(&after, &[" counters on ", " counter on "])
            && let Some((subj_raw, val_raw)) = rest.split_once(" equal to ")
            && let (Ok(subj), Ok(val)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_value_expr(val_raw, vocab),
            )
        {
            return Some(ins_counters(subj, kind.trim().to_string(), val));
        }
        if let Some((kind, rest)) =
            split_once_any(&after, &[" counters equal to ", " counter equal to "])
            && let Some((val_raw, subj_raw)) = rest.rsplit_once(" on ")
            && let (Ok(val), Ok(subj)) = (
                parse_value_expr(val_raw, vocab),
                parse_subject_or_target(subj_raw, vocab),
            )
        {
            return Some(ins_counters(subj, kind.trim().to_string(), val));
        }
    }

    // Put sticker: "put a [name/art/ability] sticker on <subj>"
    if let Some(after) = low.strip_prefix("put a sticker on ")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Some(InstructionAst::PutSticker {
            kind: "any".to_string(),
            target: Box::new(subj),
        });
    }
    if let Some(after) = low.strip_prefix("put a ")
        && let Some((kind, subj_raw)) = after.split_once(" sticker on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PutSticker {
            kind: kind.trim().to_string(),
            target: Box::new(subj),
        });
    }

    // PutCounters with keyword-counter choice:
    // "put [your choice of] a counter from among <kws> on <subj>"
    // "put a +1/+1 counter and a counter from among <kws> on <subj>" (Elspeth Resplendent, Aragorn)
    if let Some(after) = low.strip_prefix("put ")
        && let Some((left, rest)) = after.split_once(" a counter from among ")
        && let Some((kws_raw, subj_raw)) = rest.split_once(" on ")
        && let (Some(_kws), Ok(subj)) = (
            parse_keyword_list(kws_raw, vocab),
            parse_subject_or_target(subj_raw, vocab),
        )
        && (left.is_empty() || left == "your choice of" || left.ends_with(" counter and"))
    {
        return Some(ins_counters(
            subj,
            format!("choice:{kws_raw}"),
            ValueExprAst::Const(1),
        ));
    }

    // PutCounters: "[<player>] put(s) <cnt> <kind> counter(s) on <subj> [for each <filter>]"
    if let Some((_, after)) = split_leading_player_verb(low, "put", "puts")
        && let Some((left, rest)) = split_once_any(&after, &[" counters on ", " counter on "])
    {
        let (subj_raw, fe_opt) = match rest.split_once(" for each ") {
            Some((sr, fe)) => (sr, Some(fe)),
            None => (rest, None),
        };
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            let (mut cnt, kind) = peel_leading_count(left, vocab);
            if let Some(fe) = fe_opt {
                let scale_val = parse_for_each_value_expr(fe, vocab).ok()?;
                cnt = ValueExprAst::Mul(Box::new(cnt), Box::new(scale_val));
            }
            return Some(ins_counters(subj, kind.to_string(), cnt));
        }
    }

    // RemoveCounters (including "remove those counters" / "remove all of those counters")
    if low == "remove those counters" || low == "remove all of those counters" {
        return Some(InstructionAst::RemoveCounters {
            subject: self_obj(),
            counter_kind: "those".to_string(),
            all: true,
            count: None,
        });
    }
    if let Some((_, after)) = split_leading_player_verb(low, "remove", "removes")
        && let Some((left, subj_raw)) =
            split_once_any(&after, &[" counters from ", " counter from "])
    {
        let (subj_clean, delayed_eoc) = match subj_raw.strip_suffix(" at end of combat") {
            Some(c) => (c, true),
            None => (subj_raw, false),
        };
        if left.starts_with("up to ") && subj_clean.starts_with("among ") {
            return None;
        }
        let subj = parse_subject_or_target(
            subj_clean.strip_prefix("among ").unwrap_or(subj_clean),
            vocab,
        )
        .ok()?;
        let ins = if let Some(kind) = left.strip_prefix("all ") {
            ins_rm_counters(subj, kind.trim().to_string(), true, None)
        } else {
            let (cnt, kind) = peel_leading_count(left, vocab);
            ins_rm_counters(
                subj,
                if kind.is_empty() {
                    "any".to_string()
                } else {
                    kind.to_string()
                },
                false,
                Some(cnt),
            )
        };
        return Some(if delayed_eoc {
            ins_delayed(TurnStepAst::EndOfCombat, DelayedStepScope::Next, vec![ins])
        } else {
            ins
        });
    }
    None
}

pub(super) fn try_parse_combat_and_animate(
    s_nodur: &str,
    low: &str,
    dur: &DurationAst,
    dur_perm: &DurationAst,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    let snlow = s_nodur.to_ascii_lowercase();
    if let Some((who_raw, rest)) = snlow.split_once(" can't get ")
        && let Some(kind_raw) = rest.strip_suffix(" counters")
        && let Ok(subj) = parse_subject_or_target(who_raw.trim(), vocab)
    {
        let k = kind_raw.trim();
        let counter_kind = if k.is_empty() || k == "any" {
            None
        } else {
            Some(k.to_string())
        };
        return Some(ins_temp_static(
            StaticAbilityAst::PreventCounters {
                subject: Box::new(subj),
                counter_kind,
                condition: None,
            },
            dur.clone(),
        ));
    }

    for rule in [
        "can't be blocked",
        "can't be countered",
        "can't be equipped",
        "can't be sacrificed",
        "can't be the targets of spells or abilities",
        "can't be the target of spells or abilities",
        "can't be the target of spells",
        "can't block",
        "can't attack",
        "can't attack you or planeswalkers you control",
        "can't attack you or a planeswalker you control",
        "can't attack its owner or planeswalkers its owner controls",
        "can't be turned face up",
        "can't be enchanted",
        "can't be enchanted by other auras",
        "isn't a planeswalker",
        "isn't an artifact",
        "isn't a creature",
        "isn't a land",
        "isn't an enchantment",
        "gains all creature types",
        "gain all creature types",
        "can't phase in",
        "becomes foretold",
        "becomes unprepared",
        "can't attack or block",
        "can't attack or block, and its activated abilities can't be activated",
        "can't attack or block and its activated abilities can't be activated",
        "can't phase out",
        "can't be regenerated",
        "blocks if able",
        "blocks this turn if able",
        "block this turn if able",
        "attacks if able",
        "attack if able",
        "attacks this turn if able",
        "attack this turn if able",
        "attacks this combat if able",
        "attack this combat if able",
        "attacks each combat if able",
        "attack each combat if able",
        "attacks a player each combat if able",
        "attacks that player this combat if able",
        "must be blocked this turn if able",
        "blocks ~ this turn if able",
        "blocks ~ if able",
        "assigns no combat damage",
        "can attack as though it didn't have defender",
        "can attack this turn as though it didn't have defender",
        "can attack as though they didn't have defender",
        "can attack this turn as though they didn't have defender",
        "can block an additional creature",
        "can block an additional creature this combat",
    ] {
        if let Some(subj_raw) = snlow.strip_suffix(&format!(" {rule}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(ins_combat_rule(subj, rule.to_string(), dur.clone()));
        }
    }
    if let Some(rest) = low.strip_prefix("goad ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(ins_combat_rule(
            subj,
            "goad",
            DurationAst::UntilYourNextTurn,
        ));
    }
    // CR 509 General forced-block combat rule mod:
    // "<subj1> blocks/block <subj2> [this combat|this turn] if able"
    // "you may have <subj1> block <subj2> [this combat|this turn] if able"
    // (Impetuous Devils, Monstrous Step, Tolsimir, Grappling Hook)
    let is_optional_block = low.starts_with("you may have ");
    let forced_block_core = strip_prefix_any(&low, &["you may have ", "have "]).unwrap_or(low);
    if let Some(before_able) = forced_block_core
        .strip_suffix(" this combat if able")
        .or_else(|| forced_block_core.strip_suffix(" this turn if able"))
        .or_else(|| forced_block_core.strip_suffix(" if able"))
        && let Some((b_raw, a_raw)) = split_once_any(&before_able, &[" blocks ", " block "])
        && let (Ok(blocker), Ok(_attacker)) = (
            parse_subject_or_target(b_raw, vocab),
            parse_subject_or_target(a_raw, vocab),
        )
    {
        let mod_ins = ins_combat_rule(blocker, format!("must_block:{a_raw}"), dur.clone());
        if is_optional_block {
            return Some(ins_opt1(mod_ins));
        }
        return Some(mod_ins);
    }
    // CR 508 Attacking redirection / status assignment ("Those creatures are now attacking that player", Tahngarth)
    if let Some((subj_raw, who_raw)) = split_once_any(
        &low,
        &[
            " are now attacking ",
            " is now attacking ",
            " is attacking ",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && (parse_player_ref(who_raw).is_some() || parse_subject_or_target(who_raw, vocab).is_ok())
    {
        return Some(ins_combat_rule(
            subj,
            format!("attacking:{who_raw}"),
            DurationAst::UntilEndOfCombat,
        ));
    }

    // CR 608.2h / CR 613.4b dynamic P/T assignment (live or LKI):
    // "<subj>'s [base] power (is|becomes) <val>" / "its power is equal to <val>"
    if let Some((subj_raw, rhs)) = split_once_any(
        &snlow,
        &[
            "'s base power becomes ",
            "'s base power is ",
            "'s power becomes ",
            "'s power is ",
            " has base power ",
            " have base power ",
        ],
    )
    .or_else(|| snlow.strip_prefix("its power is ").map(|r| ("it", r)))
    .or_else(|| snlow.strip_prefix("its power becomes ").map(|r| ("it", r)))
    .or_else(|| snlow.strip_prefix("its base power is ").map(|r| ("it", r)))
    .or_else(|| {
        snlow
            .strip_prefix("its base power becomes ")
            .map(|r| ("it", r))
    }) && let (Ok(subj), Ok(val)) = (
        parse_subject_or_target(subj_raw, vocab),
        parse_value_expr(rhs, vocab),
    ) {
        return Some(InstructionAst::SetDynamicPT {
            subject: Box::new(subj),
            power: Some(Box::new(val)),
            toughness: None,
            duration: dur.clone(),
        });
    }
    if let Some((subj_raw, rhs)) = split_once_any(
        &snlow,
        &[
            "'s base toughness becomes ",
            "'s base toughness is ",
            "'s toughness becomes ",
            "'s toughness is ",
            " has base toughness ",
            " have base toughness ",
        ],
    )
    .or_else(|| snlow.strip_prefix("its toughness is ").map(|r| ("it", r)))
    .or_else(|| {
        snlow
            .strip_prefix("its toughness becomes ")
            .map(|r| ("it", r))
    })
    .or_else(|| {
        snlow
            .strip_prefix("its base toughness is ")
            .map(|r| ("it", r))
    })
    .or_else(|| {
        snlow
            .strip_prefix("its base toughness becomes ")
            .map(|r| ("it", r))
    }) && let (Ok(subj), Ok(val)) = (
        parse_subject_or_target(subj_raw, vocab),
        parse_value_expr(rhs, vocab),
    ) {
        return Some(InstructionAst::SetDynamicPT {
            subject: Box::new(subj),
            power: None,
            toughness: Some(Box::new(val)),
            duration: dur.clone(),
        });
    }
    // CR 611/613 Dynamic Animate/Become Effects:
    // - "<subj> becomes/become a/an <desc> with base power and toughness P/T[, <kws>][, and <kws>]" (Bogardan Dragonheart, Eccentric Apprentice)
    // - "<subj> becomes/become a/an <desc> with [base] power and toughness [each] equal to <val> [and gains <kws>]" (Gideon, Veiled Sentry, Fractalize, Woodcaller Automaton)
    // - "<subj> becomes/become a/an <desc> in addition to his/her/its other types and gains <kws>" (Sarkhan, Dragon Ascendant)
    // - "<subj> has/have base power and toughness P/T and becomes a <desc> in addition to its other [creature] types" (Allosaurus Shepherd)
    if let Some((subj_raw, rhs)) =
        split_once_any(&snlow, &[" becomes a ", " becomes an ", " become "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if let Some((ty_part, rest)) = rhs.split_once(" with base power and toughness ") {
            let pt_tok = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(',');
            if let Some((p, t)) = parse_base_pt(pt_tok) {
                let tail = rest[pt_tok.len()..].trim().trim_start_matches(',').trim();
                let tail_clean = tail.strip_prefix("and ").unwrap_or(tail).trim();
                let kws = if tail_clean.is_empty() {
                    Some(Vec::new())
                } else {
                    parse_keyword_list(tail_clean, vocab)
                };
                if let Some(keywords) = kws {
                    return Some(ins_animate(
                        subj,
                        p,
                        t,
                        Some(ty_part.trim().to_string()),
                        keywords,
                        dur_perm.clone(),
                    ));
                }
            }
            if rest.starts_with("each equal to ") || rest.starts_with("equal to ") {
                return Some(ins_animate(
                    subj,
                    "X",
                    "X",
                    Some(rhs.to_string()),
                    Vec::new(),
                    dur_perm.clone(),
                ));
            }
        }
        if let Some((ty_part, rest)) = rhs.split_once(" with power and toughness each equal to ") {
            let _ = ty_part;
            let _ = rest;
            return Some(ins_animate(
                subj,
                "X",
                "X",
                Some(rhs.to_string()),
                Vec::new(),
                dur_perm.clone(),
            ));
        }
        if rhs.contains(" with ") && rhs.contains("base power and toughness equal to ") {
            return Some(ins_animate(
                subj,
                "X",
                "X",
                Some(rhs.to_string()),
                Vec::new(),
                dur_perm.clone(),
            ));
        }
        if let Some((ty_part, kw_raw)) = split_once_any(
            &rhs,
            &[
                " in addition to his other types and gains ",
                " in addition to her other types and gains ",
                " in addition to its other types and gains ",
                " in addition to their other types and gain ",
            ],
        ) && let Some(keywords) = parse_keyword_list(kw_raw, vocab)
        {
            return Some(ins_animate(
                subj,
                "keep",
                "keep",
                Some(ty_part.trim().to_string()),
                keywords,
                dur_perm.clone(),
            ));
        }
    }
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" has base power and toughness ")
        .or_else(|| snlow.split_once(" have base power and toughness "))
        && let Some((pt_tok, ty_rest)) = split_once_any(&rhs, &[" and becomes a ", " and become "])
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(pt_tok.trim()),
        )
    {
        return Some(ins_animate(
            subj,
            p,
            t,
            Some(ty_rest.trim().to_string()),
            Vec::new(),
            dur.clone(),
        ));
    }

    // CR 611/613 "<subj> becomes/become a/an <desc> with \"<ab>\" [and loses all other [card types and] abilities]"
    // (Vraska Betrayal's Sting, Kitesail Larcenist, Tenth District Hero)
    if s_nodur.contains('"')
        && let Some((subj_raw, rhs)) =
            split_once_any(&snlow, &[" becomes a ", " becomes an ", " become "])
        && let Some((ty_part, after_w)) = rhs.split_once(" with \"")
        && let Some((ab_raw, tail)) = after_w.split_once('"')
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let t_clean = tail.trim().trim_start_matches(',').trim();
        let lose_all = t_clean.contains("loses all other")
            || t_clean.contains("lose all other")
            || t_clean.contains("loses all abilities")
            || t_clean.contains("lose all abilities");
        if t_clean.is_empty() || lose_all {
            let orig_ab = s_nodur
                .split_once('"')
                .and_then(|(_, r)| r.split_once('"'))
                .map_or(ab_raw, |(inner, _)| inner);
            let compiled = compile_granted_ability(orig_ab, vocab);
            return Some(ins_temp_static(
                StaticAbilityAst::Compound(vec![
                    StaticAbilityAst::ContinuousLayerEffect {
                        subject: Box::new(subj.clone()),
                        set_types: Some(ty_part.trim().to_string()),
                        additive_types: false,
                        base_pt: None,
                        set_colors: None,
                        grant_keywords: Vec::new(),
                        lose_keywords: Vec::new(),
                        lose_all_abilities: lose_all,
                        condition: None,
                    },
                    StaticAbilityAst::GrantAbilityText {
                        subject: Box::new(subj),
                        ability_text: orig_ab.to_string(),
                        compiled,
                        condition: None,
                    },
                ]),
                dur.clone(),
            ));
        }
    }
    if let Some((subj_raw, rhs)) = snlow
        .split_once("'s base power and toughness become ")
        .or_else(|| snlow.split_once("'s base power and toughness becomes "))
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(rhs.trim()),
        )
    {
        return Some(ins_animate(subj, p, t, None, Vec::new(), dur.clone()));
    }
    // "<subj> [each] have/has base power and toughness P/T [and has/have <kws>]"
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" each have base power and toughness ")
        .or_else(|| snlow.split_once(" have base power and toughness "))
        .or_else(|| snlow.split_once(" has base power and toughness "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let (pt_tok, kws) =
            if let Some((pt_side, kw_side)) = split_once_any(&rhs, &[" and has ", " and have "]) {
                (
                    pt_side.trim(),
                    parse_keyword_list(kw_side, vocab).unwrap_or_default(),
                )
            } else {
                (rhs.trim(), Vec::new())
            };
        if let Some((p, t)) = parse_base_pt(pt_tok) {
            return Some(ins_animate(subj, p, t, None, kws, dur.clone()));
        }
    }

    // "<subj> becomes <color(s) / type> [in addition to its other colors/types]"
    if let Some((subj_raw, rhs_raw)) = split_once_any(&snlow, &[" becomes ", " become "]) {
        let rhs = strip_suffix_any(
            &rhs_raw,
            &[
                " in addition to its other colors",
                " in addition to their other colors",
                " in addition to its other types",
                " in addition to their other types",
                " in addition to its other creature types",
                " in addition to their other creature types",
            ],
        )
        .unwrap_or(rhs_raw)
        .trim();
        let is_color_or_type = matches!(
            rhs,
            "white"
                | "blue"
                | "black"
                | "red"
                | "green"
                | "colorless"
                | "all colors"
                | "that type"
                | "that color"
                | "the chosen type"
                | "the chosen color"
                | "the color of your choice"
                | "the color or colors of your choice"
                | "the creature type of your choice"
        ) || {
            // Multi-color list like "white and blue", "red, green, and white"
            let normalized = rhs
                .replace(", and ", " ")
                .replace(" and ", " ")
                .replace(',', " ");
            let mut words = normalized.split_whitespace().peekable();
            words.peek().is_some()
                && words.all(|w| matches!(w, "white" | "blue" | "black" | "red" | "green"))
        };
        if is_color_or_type && let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            return Some(ins_animate(
                subj,
                "keep",
                "keep",
                Some(rhs_raw.to_string()),
                Vec::new(),
                dur.clone(),
            ));
        }
    }
    // "<subj> becomes/become an artifact creature / creature / basic land"
    if let Some((subj_raw, rhs)) =
        split_once_any(&snlow, &[" becomes an ", " becomes a ", " become "])
        && (matches!(
            rhs,
            "artifact creature"
                | "artifact creatures"
                | "creature"
                | "creatures"
                | "artifact"
                | "artifacts"
                | "plains"
                | "island"
                | "swamp"
                | "mountain"
                | "forest"
        ) || rhs.starts_with("artifact creature with power and toughness each equal to "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(ins_animate(
            subj,
            "keep",
            "keep",
            Some(rhs.to_string()),
            Vec::new(),
            dur.clone(),
        ));
    }
    // "<subj> is a <color/subtype> in addition to its other [colors and] types" (Eluge, etc.)
    if let Some((subj_raw, rhs)) = split_once_any(&snlow, &[" is a ", " is an "])
        && (rhs.ends_with(" in addition to its other colors and types")
            || rhs.ends_with(" in addition to its other types"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(ins_animate(
            subj,
            "keep",
            "keep",
            Some(rhs.to_string()),
            Vec::new(),
            dur.clone(),
        ));
    }
    // "<subj> doesn't/don't untap during its/their controller(s)' untap step(s) for as long as <cond>"
    if let Some((subj_raw, cond_raw)) = low
        .split_once(" doesn't untap during its controller's untap step for as long as ")
        .or_else(|| {
            low.split_once(" don't untap during their controllers' untap steps for as long as ")
        })
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_condition(cond_raw, vocab),
        )
    {
        return Some(ins_combat_rule(
            subj,
            "DoesntUntap",
            DurationAst::ForAsLongAs(Box::new(cond)),
        ));
    }

    if let Some((subj_raw, blocker_raw)) = snlow.split_once(" can't be blocked by ")
        && let (Ok(subj), Ok(_flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(blocker_raw, vocab),
        )
    {
        return Some(ins_combat_rule(
            subj,
            format!("CantBeBlockedBy:{blocker_raw}"),
            dur.clone(),
        ));
    }
    if let Some((blocker_raw, atk_raw)) = snlow.split_once(" can't block ")
        && let (Ok(blocker), Ok(_atk)) = (
            parse_subject_or_target(blocker_raw, vocab),
            parse_subject_or_target(atk_raw, vocab),
        )
    {
        return Some(ins_combat_rule(
            blocker,
            format!("CantBlock:{atk_raw}"),
            dur.clone(),
        ));
    }

    // "[have] <subj> becomes/become a copy of <source> [until end of turn][, except <exception>]"
    let copy_raw = low.strip_prefix("have ").unwrap_or(low);
    let (copy_core, copy_exception) = match copy_raw.split_once(", except ") {
        Some((before, exc)) => (before.trim(), Some(exc.trim().to_string())),
        None => (copy_raw, None),
    };
    let (copy_nodur, copy_dur_opt) = if let Some(c) = copy_core.strip_suffix(" until end of turn") {
        (c.trim(), Some(DurationAst::UntilEndOfTurn))
    } else if let Some(c) = copy_core.strip_suffix(" until your next turn") {
        (c.trim(), Some(DurationAst::UntilYourNextTurn))
    } else if let Some(c) = copy_core.strip_suffix(" until end of combat") {
        (c.trim(), Some(DurationAst::UntilEndOfCombat))
    } else if let Some(c) = copy_core.strip_suffix(" until ~ leaves the battlefield") {
        (c.trim(), Some(DurationAst::UntilSourceLeaves))
    } else {
        (copy_core, None)
    };
    let copy_dur = copy_dur_opt.unwrap_or_else(|| dur.clone());
    if let Some((subj_raw, src_raw)) = copy_nodur
        .split_once(" becomes a copy of ")
        .or_else(|| copy_nodur.split_once(" become a copy of "))
        .or_else(|| copy_nodur.split_once(" become copies of "))
        && let (Ok(subj), Ok(src)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(src_raw, vocab),
        )
    {
        return Some(InstructionAst::BecomeCopy {
            subject: Box::new(subj),
            source: Box::new(src),
            duration: copy_dur,
            exception: copy_exception,
        });
    }

    // Lure effect: "All <creatures> able to block <subj> [this turn] do so" (You Look Upon the Tarrasque, etc.)
    if let Some((who_part, rest)) = snlow.split_once(" able to block ")
        && let Some(subj_raw) =
            strip_suffix_any(&rest, &[" this turn do so", " this combat do so", " do so"])
        && let (Ok(blockers), Ok(subj)) = (
            parse_subject_or_target(who_part, vocab),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        if who_part.trim() == "all creatures" {
            return Some(ins_combat_rule(subj, "AllAbleBlock", dur.clone()));
        }
        return Some(InstructionAst::ForcedBlockByFilter {
            blockers: Box::new(blockers),
            target: Box::new(subj),
            duration: dur.clone(),
        });
    }
    // "<subj> becomes a/an <type> in addition to its other types" (Clavileño, Coward // Killer)
    if let Some(before_in_add) = snlow
        .strip_suffix(" in addition to its other types")
        .or_else(|| snlow.strip_suffix(" in addition to their other types"))
        && let Some((subj_raw, ty_raw)) =
            split_once_any(&before_in_add, &[" becomes a ", " becomes an ", " become "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && (ty_raw == "~"
            || vocab.lookup_subtype(ty_raw).is_some()
            || parse_object_filter(ty_raw, vocab).is_ok())
    {
        return Some(ins_temp_static(
            StaticAbilityAst::SetOrAddType {
                subject: Box::new(subj),
                type_or_subtype: ty_raw.to_string(),
                additive: true,
                condition: None,
            },
            dur.clone(),
        ));
    }

    // Strip optional trailing "in addition to its/their other [colors and] types" for animation
    let anim_clean = strip_suffix_any(
        &snlow,
        &[
            " in addition to its other colors and types",
            " in addition to their other colors and types",
            " in addition to its other types",
            " in addition to their other types",
        ],
    )
    .unwrap_or(&snlow);

    // "<subj> becomes / become / each become a copy of <src>[, except <exc>]" (Polymorphous Rush, Shuri, Taskmaster)
    if let Some((subj_raw, rest)) = split_once_any(
        &anim_clean,
        &[
            " each become a copy of ",
            " becomes a copy of ",
            " become a copy of ",
            " become copies of ",
        ],
    ) {
        let (src_raw, exception) = match rest.split_once(", except ") {
            Some((sr, exc)) => (sr.trim(), Some(exc.trim().to_string())),
            None => (rest.trim(), None),
        };
        if let (Ok(subj), Ok(src)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(src_raw, vocab),
        ) {
            return Some(InstructionAst::BecomeCopy {
                subject: Box::new(subj),
                source: Box::new(src),
                duration: dur.clone(),
                exception,
            });
        }
    }

    // "<subj> becomes a <desc> with base power and toughness P/T"
    if let Some((subj_raw, rest)) = split_once_any(
        &anim_clean,
        &[
            " each become a ",
            " each become an ",
            " each become ",
            " becomes a ",
            " becomes an ",
            " become ",
        ],
    ) && let Some((desc, pt_tok)) = rest.split_once(" with base power and toughness ")
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(pt_tok.trim()),
        )
    {
        return Some(ins_animate(
            subj,
            p,
            t,
            Some(desc.trim().to_string()),
            Vec::new(),
            dur.clone(),
        ));
    }

    // "<subj> becomes a P/T <desc> [with <kws>]"
    if let Some((subj_raw, rest)) = split_once_any(
        &anim_clean,
        &[
            " each become a ",
            " each become an ",
            " each become ",
            " becomes a ",
            " becomes an ",
            " become ",
        ],
    ) && rest.contains("creature")
        && let Some((pt_tok, desc_rest)) = rest.split_once(' ')
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(pt_tok),
        )
    {
        let (desc, kws) = match desc_rest.split_once(" with ") {
            Some((d, kw)) => (d, parse_keyword_list(kw, vocab).unwrap_or_default()),
            None => (desc_rest, Vec::new()),
        };
        return Some(ins_animate(
            subj,
            p,
            t,
            Some(desc.trim().to_string()),
            kws,
            dur.clone(),
        ));
    }

    // "<subj> becomes a/an <Subtype(s)> [with <kws>]" (Kargan Intimidator, Warden of the First Tree, Rageform)
    if let Some((subj_raw, rest)) = split_once_any(&anim_clean, &[" becomes a ", " becomes an "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if rest == "aura with enchant creature" || rest == "aura with enchant permanent" {
            return Some(ins_animate(
                subj,
                "keep",
                "keep",
                Some(rest.to_string()),
                Vec::new(),
                dur_perm.clone(),
            ));
        }
        let (type_part, kws) = match rest.split_once(" with ") {
            Some((tp, kw_raw)) => (tp.trim(), parse_keyword_list(kw_raw, vocab)),
            None => (rest.trim(), Some(Vec::new())),
        };
        if let Some(kw_list) = kws
            && !type_part.is_empty()
            && parse_object_filter(type_part, vocab).is_ok()
        {
            return Some(ins_animate(
                subj,
                "keep",
                "keep",
                Some(type_part.to_string()),
                kw_list,
                dur.clone(),
            ));
        }
    }

    if matches!(
        low,
        "it's still a land"
            | "it's still an artifact"
            | "it's still an enchantment"
            | "he's still a planeswalker"
            | "she's still a planeswalker"
            | "it's still a planeswalker"
            | "they're still lands"
            | "it's an enchantment"
            | "it's an artifact"
            | "it's a creature"
            | "it's a land"
            | "it's a treasure"
    ) {
        return Some(InstructionAst::KeepExistingTypes(low.to_string()));
    }

    None
}

// ---------------------------------------------------------------------------
// 4. STATIC & REPLACEMENT PARSER
// ---------------------------------------------------------------------------

pub(super) fn parse_source_choice_redirect(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Harm's Way / Shining Shoal:
    // "The next <N/X> damage that a source of your choice would deal to <from> this turn is dealt to <to> instead"
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((amt_raw, rest)) =
            after.split_once(" damage that a source of your choice would deal to ")
        && let Some((from_raw, to_raw)) = rest.split_once(" this turn is dealt to ")
        && let Some(to_clean) = to_raw.strip_suffix(" instead")
        && let (Ok(amt), Ok(to)) = (
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(to_clean.trim(), vocab),
        )
    {
        let from = match from_raw.trim() {
            "you and/or permanents you control" => {
                let mut p = any_perm();
                p.controller = Some(ControllerRelation::You);
                SubjectRefAst::Compound(vec![
                    SubjectRefAst::Player(PlayerRefAst::You),
                    SubjectRefAst::All(Box::new(p)),
                ])
            }
            "you and/or creatures you control" => {
                let mut c = obj_type(CardTypeMask::CREATURE);
                c.controller = Some(ControllerRelation::You);
                SubjectRefAst::Compound(vec![
                    SubjectRefAst::Player(PlayerRefAst::You),
                    SubjectRefAst::All(Box::new(c)),
                ])
            }
            other => parse_subject_or_target(other, vocab).ok()?,
        };
        return some1(InstructionAst::RedirectDamage {
            amount: Box::new(amt),
            from: Box::new(from),
            to: Box::new(to),
            source_choice: Some("source_of_your_choice".to_string()),
            duration: Some(DurationAst::UntilEndOfTurn),
        });
    }
    None
}

pub(super) fn parse_combat_and_resource_grammar(
    s: &str,
    low: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Token creation with inherited abilities ("create <token_spec> and all triggered/activated abilities of <src>") (The Book of Vile Darkness)
    if let Some(after_cr) = low.strip_prefix("create ")
        && let Some((tok_part, inh_part)) = after_cr.split_once(" and all ")
        && let Some((kind_word, src_desc)) = inh_part.split_once(" abilities of ")
        && matches!(kind_word.trim(), "triggered" | "activated")
        && let Ok(mut create_ins) =
            parse_atomic_effect(&format!("create {}", tok_part.trim()), vocab)
    {
        let mut src_flt = any_card_flt();
        if src_desc.contains("exiled") {
            src_flt.zone = Some(ZoneFilter::Exile);
            src_flt.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        }
        src_flt.predicates.push(FilterPredicateAst::HasAbilityKind {
            kind: kind_word.trim().to_string(),
            negated: false,
        });
        create_ins.push(ins_temp_static(
            StaticAbilityAst::InheritAbilities {
                subject: box_anaphora("the token"),
                source_filter: Box::new(src_flt),
                activated_only: kind_word.trim() == "activated",
                condition: None,
            },
            DurationAst::Permanent,
        ));
        return Some(create_ins);
    }

    // 2. Extra loyalty activation reset ("for each <filter>, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn") (The Chain Veil)
    if let Some(after_fe) = low.strip_prefix("for each ")
        && let Some((flt_raw, tail)) = after_fe.split_once(", you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let _ = tail;
        return some1(InstructionAst::ForEachObject {
            filter: Box::new(flt),
            body: vec![ins_temp_static(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(box_anaphora("it")),
                    rule: "extra_loyalty_activation_as_though_none_activated".to_string(),
                    filter: None,
                    condition: None,
                }, DurationAst::UntilEndOfTurn)],
        });
    }

    // 3. Per-mana alternative activation payment ("for each mana in this ability's activation cost, you may <cost> rather than pay that mana") (Heirloom Epic)
    if let Some(after_fe) =
        low.strip_prefix("for each mana in this ability's activation cost, you may ")
        && let Some(cost_raw) = after_fe.strip_suffix(" rather than pay that mana")
        && let Ok(cost_atom) = parse_cost_atom(cost_raw.trim(), vocab)
    {
        return some1(ins_temp_static(
            StaticAbilityAst::AlternativeAbilityCost {
                cost: vec![cost_atom],
                ability_kind: "this_ability_convoke".to_string(),
                scope: "per_mana_in_activation_cost".to_string(),
                condition: None,
            },
            DurationAst::Instant,
        ));
    }

    // 4. Coin flip branch with recursive flip-again decision ("if you win/lose the flip, <body...>, and <player> decides whether to flip again") (Game of Chaos)
    if let Some((outcome, after_if)) = low
        .strip_prefix("if you win the flip, ")
        .map(|r| ("win", r))
        .or_else(|| {
            low.strip_prefix("if you lose the flip, ")
                .map(|r| ("lose", r))
        })
        && let Some((body_raw, decider_raw)) = after_if
            .strip_suffix(" decides whether to flip again")
            .or_else(|| after_if.strip_suffix(" decide whether to flip again"))
            .and_then(|r| r.rsplit_once(", and "))
        && let Some(decider) = parse_player_ref(decider_raw.trim())
    {
        let (mut then_body, fails) = parse_effect_body(body_raw.trim(), vocab);
        if fails.is_empty() && !then_body.is_empty() {
            then_body.push(ins_opt_player(
                decider,
                vec![InstructionAst::RepeatProcess {
                    condition: None,
                    max_times: None,
                    any_number: false,
                }],
            ));
            return some1(ins_if(
                ConditionAst::CoinFlipOutcome {
                    outcome: outcome.to_string(),
                },
                then_body,
            ));
        }
    }

    // 5. Pairwise turn-control exchange ("the first player controls the second player during ..., and the second player controls the first player during ...") (Cruel Entertainment)
    if let Some((left_part, right_part)) = low.split_once(", and ")
        && let Some((c1, t1)) = parse_player_controls_next_turn(left_part.trim())
        && let Some((c2, t2)) = parse_player_controls_next_turn(right_part.trim())
    {
        return Some(vec![
            InstructionAst::PlayerControlsPlayerTurn {
                controller: Box::new(c1),
                controlled: Box::new(t1),
            },
            InstructionAst::PlayerControlsPlayerTurn {
                controller: Box::new(c2),
                controlled: Box::new(t2),
            },
        ]);
    }

    // 6. Ordered multi-type sacrifices ("<player> sacrifices <n1> permanent(s) of their choice of the first type, sacrifices <n2> of their choice of the second type, then sacrifices <n3> of their choice of the third type") (Rite of Ruin)
    if let Some((who_raw, rest)) = low.split_once(" sacrifices ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((p1, rest2)) = rest.split_once(", sacrifices ")
        && let Some((p2, p3)) = rest2.split_once(", then sacrifices ")
        && let (Some(n1), Some(n2), Some(n3)) = (
            parse_ordered_type_sac_count(p1, "first type"),
            parse_ordered_type_sac_count(p2, "second type"),
            parse_ordered_type_sac_count(p3, "third type"),
        )
    {
        let nth_perm = |nth: u8| {
            let mut flt = any_perm();
            flt.predicates.push(FilterPredicateAst::NthChosenType(nth));
            Box::new(grp(flt))
        };
        let sac_player = match player {
            PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent => PlayerRefAst::ThatPlayer,
            other => other,
        };
        let body = vec![
            InstructionAst::Sacrifice {
                player: sac_player,
                count: Some(vconst(n1)),
                subject: nth_perm(1),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            },
            InstructionAst::Sacrifice {
                player: sac_player,
                count: Some(vconst(n2)),
                subject: nth_perm(2),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            },
            InstructionAst::Sacrifice {
                player: sac_player,
                count: Some(vconst(n3)),
                subject: nth_perm(3),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            },
        ];
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

    // 7. Capped life gain ("you gain life equal to <base>, but not more [life] than [the amount of {<c>} spent on x, ]the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness") (Drain Life, Soul Burn)
    if let Some(after_gain) = low.strip_prefix("you gain life equal to ")
        && let Some((base_raw, cap_raw)) = split_once_any(&after_gain, &[", but not more life than ", ", but not more than "])
        && cap_raw.ends_with("the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness")
    {
        let base_val = if base_raw.trim() == "the damage dealt" {
            ValueExprAst::PriorResult
        } else {
            parse_value_expr(base_raw.trim(), vocab).ok()?
        };
        let mut delta = base_val;
        if let Some(mana_prefix) = cap_raw.strip_prefix("the amount of ")
            && let Some((mana_sym, _)) = mana_prefix.split_once(" spent on x, ")
        {
            delta = ValueExprAst::Min(
                Box::new(delta),
                Box::new(ValueExprAst::ManaSpentToCast {
                    mana_filter: Some(format!("{}_on_X", mana_sym.trim().to_ascii_uppercase())),
                }),
            );
        }
        delta = ValueExprAst::Min(Box::new(delta), Box::new(ValueExprAst::TargetPreDamageCap));
        return Some(vec![ins_life(PlayerRefAst::You, delta, true)]);
    }

    // 8. Count colors of <subj>, then search your library for <filter> that's exactly that many colors plus <n> (Evolving Door)
    if let Some(after_cnt) = low.strip_prefix("count the colors of ")
        && let Some((subj_raw, search_part)) =
            after_cnt.split_once(", then search your library for ")
        && let Some((flt_raw, plus_raw)) =
            search_part.split_once(" that's exactly that many colors plus ")
        && let (Ok(mut search_flt), Some(plus_n)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_number_word(plus_raw.trim()),
        )
    {
        let mut src_flt = obj_type(CardTypeMask::CREATURE);
        if subj_raw.contains("sacrificed") {
            src_flt
                .predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::SacrificedThisWay,
                ));
        }
        let target_colors = ValueExprAst::Add(
            Box::new(ValueExprAst::ColorsAmong(Box::new(src_flt))),
            vconst(plus_n),
        );
        search_flt
            .predicates
            .push(FilterPredicateAst::ColorCountCompare {
                op: CompareOp::Eq,
                expr: Box::new(target_colors),
            });
        return some1(InstructionAst::SearchLibrary {
            library: "your".to_string(),
            count: vconst(1),
            up_to: false,
            any_number: false,
            exact: false,
            filter: Box::new(search_flt),
            reveal: false,
            to_zone: "found_set".to_string(),
            shuffle: false,
            extra_zones: Vec::new(),
            secondary_filter: None,
            secondary_is_or: false,
            where_x: None,
            rest_to_zone: None,
            modifier: None,
            follow_up: Vec::new(),
        });
    }

    // 10. Put X <kind> counters on target creature that <blocker> blocked this turn, where X is <val> (Glyph of Delusion)
    if let Some(after_put) = low.strip_prefix("put x ")
        && let Some((kind_raw, rest)) = after_put.split_once(" counters on target creature that ")
        && let Some((blocker_raw, x_raw)) = rest.split_once(" blocked this turn, where x is ")
        && let (Ok(blocker), Ok(x_expr)) = (
            parse_subject_or_target(blocker_raw.trim(), vocab),
            parse_value_expr(x_raw.trim(), vocab),
        )
    {
        let mut blocked_cr = obj_type(CardTypeMask::CREATURE);
        blocked_cr.predicates.push(FilterPredicateAst::BlockedBy {
            blocker: Box::new(blocker),
            this_turn: true,
        });
        return some1(InstructionAst::BindX {
            x_expr: Box::new(x_expr),
            body: vec![ins_counters(
                SubjectRefAst::TargetObject {
                    filter: Box::new(blocked_cr),
                    count: TargetCountAst::Exact(1),
                },
                kind_raw.trim().to_string(),
                ValueExprAst::ChosenX,
            )],
        });
    }

    // 11. Reanimate from LKI graveyard of last controller when blocked by <blocker> (Glyph of Reincarnation)
    if let Some(after_fe) = low.strip_prefix("for each creature that died this way, put ")
        && let Some((card_raw, rest)) = after_fe.split_once(" from the graveyard of the player who controlled that creature the last time it became blocked by ")
        && let Some((blocker_raw, dest_raw)) = rest.split_once(" onto ")
        && let (Ok(mut gy_flt), Ok(blocker)) = (
            parse_object_filter(card_raw.trim(), vocab),
            parse_subject_or_target(blocker_raw.trim(), vocab),
        )
    {
        let mut died = obj_type(CardTypeMask::CREATURE);
        died.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::DestroyedThisWay,
            ));
        gy_flt.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        gy_flt.flags = ObjectFilterFlags::LKI_PAST_TENSE;
        gy_flt.predicates.push(FilterPredicateAst::BlockedBy {
            blocker: Box::new(blocker),
            this_turn: true,
        });
        return some1(InstructionAst::ForEachObject {
            filter: Box::new(died),
            body: vec![ins_move(grp(gy_flt), dest_raw.trim().to_string())],
        });
    }

    // 12. Return <subj> to its owner's hand if it shares a color with the most common color among <scope> (Barrin's Unmaking)
    if let Some(after_ret) = low.strip_prefix("return ")
        && let Some((subj_raw, rest)) = after_ret.split_once(" to its owner's hand if ")
        && let Some((_, scope_tail)) =
            rest.split_once(" shares a color with the most common color among ")
        && let (allow_tie, scope_raw) =
            match scope_tail.strip_suffix(" or a color tied for most common") {
                Some(r) => (true, r),
                None => (false, scope_tail),
            }
        && let (Ok(subj), Ok(scope_subj)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(scope_raw.trim(), vocab),
        )
    {
        let scope_flt = match scope_subj {
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => *f,
            _ => any_perm(),
        };
        return some1(ins_if(
            ConditionAst::Marker(GameMarker::SharesMostCommonColorAmong {
                subject: Box::new(subj.clone()),
                scope: Box::new(scope_flt),
                allow_tie,
            }),
            vec![InstructionAst::ReturnZone {
                subject: Box::new(subj),
                from_zone: Some("battlefield".to_string()),
                to: "its owner's hand".to_string(),
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
            }],
        ));
    }

    // 13. Temporary shroud exemption ("until end of turn, <subj> can be the target of spells and abilities controlled by <player> as though it didn't have shroud") (Autumn Willow)
    if let Some(after_u) = low.strip_prefix("until end of turn, ")
        && let Some((subj_raw, rest)) =
            after_u.split_once(" can be the target of spells and abilities controlled by ")
        && let Some(who_raw) = rest.strip_suffix(" as though it didn't have shroud")
        && let (Ok(subj), Some(who)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_player_ref(who_raw.trim()),
        )
    {
        let ctrl = match who {
            PlayerRefAst::TargetPlayer => ControllerRelation::TargetPlayer,
            PlayerRefAst::TargetOpponent => ControllerRelation::TargetOpponent,
            PlayerRefAst::You => ControllerRelation::You,
            _ => ControllerRelation::ThatPlayer,
        };
        let sp_or_ab = ObjectFilterAst {
            any_spell: true,
            is_ability: true,
            controller: Some(ctrl),
            ..Default::default()
        };
        return some1(ins_temp_static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "ignore_shroud_for_controller".to_string(),
                filter: Some(Box::new(sp_or_ab)),
                condition: None,
            },
            DurationAst::UntilEndOfTurn,
        ));
    }

    // 14. Unpreventable and unredirectable damage ("damage that would be dealt to <subj> this turn can't be prevented or dealt instead to another permanent or player") (Whippoorwill)
    if let Some(after_d) = low.strip_prefix("damage that would be dealt to ")
        && let Some(subj_raw) = after_d.strip_suffix(
            " this turn can't be prevented or dealt instead to another permanent or player",
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_temp_static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "damage_cant_be_prevented_or_redirected".to_string(),
                filter: None,
                condition: None,
            },
            DurationAst::UntilEndOfTurn,
        ));
    }

    // 15. Reveal hand then discard cards sharing name with cards spliced onto <subj> (Minamo's Meddling)
    if let Some((who_raw, rest)) = low.split_once(
        " reveals their hand, then discards each card with the same name as a card spliced onto ",
    ) && let (Some(player), Ok(spell_subj)) = (
        parse_player_ref(who_raw.trim()),
        parse_subject_or_target(rest.trim(), vocab),
    ) {
        let mut spliced_match = any_card_flt();
        spliced_match
            .predicates
            .push(FilterPredicateAst::SameNameAsSplicedOnto(Box::new(
                spell_subj,
            )));
        return Some(vec![
            InstructionAst::RevealHand { player },
            InstructionAst::Discard {
                player,
                count: None,
                filter: Some(Box::new(spliced_match)),
                anaphora: None,
                random: false,
            },
        ]);
    }

    // 16. Damage to controller of the last <filter> that dealt damage to you this turn (Suffocation)
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Some((amt_raw, flt_raw)) = rest
            .split_once(" damage to the controller of the last ")
            .and_then(|(a, r)| {
                r.strip_suffix(" that dealt damage to you this turn")
                    .map(|f| (a, f))
            })
        && let (Ok(src), Ok(amt), Ok(mut spell_flt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        spell_flt.flags = spell_flt
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
        spell_flt
            .predicates
            .push(FilterPredicateAst::LastMatchingThisTurn);
        return some1(ins_dmg(
            src,
            SubjectRefAst::ControllerOf(Box::new(grp(spell_flt))),
            amt,
        ));
    }

    let _ = s;
    None
}

fn parse_player_controls_next_turn(part: &str) -> Option<(SubjectRefAst, SubjectRefAst)> {
    let (ctrl_raw, rest) = part.split_once(" controls ")?;
    let (tgt_raw, dur_raw) = rest.split_once(" during ")?;
    if !dur_raw.ends_with("'s next turn") {
        return None;
    }
    Some((
        SubjectRefAst::Anaphora(ctrl_raw.trim().to_string()),
        SubjectRefAst::Anaphora(tgt_raw.trim().to_string()),
    ))
}

fn parse_ordered_type_sac_count(part: &str, ordinal_suffix: &str) -> Option<i32> {
    let before_ord = part
        .trim()
        .strip_suffix(&format!(" of the {ordinal_suffix}"))?;
    let before_choice = before_ord
        .strip_suffix(" of their choice")
        .unwrap_or(before_ord)
        .trim();
    let cnt_tok = strip_suffix_any(&before_choice, &[" permanents", " permanent"])
        .unwrap_or(before_choice)
        .trim();
    parse_number_word(cnt_tok)
}

pub(super) fn parse_damage_life_mana_extended(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Dual/split damage: "<src> deals <a1> damage to <t1>[,] and [an additional] <a2> [additional] damage to <t2>"
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        for sep in [", and ", " and "] {
            if let Some((part1, part2)) = rest.split_once(sep)
                && let Some((a1, t1)) = parse_dmg_amount_and_target(part1.trim(), vocab)
                && let Some((a2, t2)) = parse_dmg_amount_and_target(part2.trim(), vocab)
            {
                return Some(vec![ins_dmg(src.clone(), t1, a1), ins_dmg(src, t2, a2)]);
            }
        }
        if let Some(after_eq) = rest.strip_prefix("damage equal to ")
            && let Some((val_raw, tgt_raw)) = after_eq.rsplit_once(" to ")
            && let (Ok(amt), Ok(tgt)) = (
                parse_value_expr(val_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
            )
        {
            return Some(vec![ins_dmg(src, tgt, amt)]);
        }
        if let Some(after_to) = rest.strip_prefix("damage to ")
            && let Some((tgt_raw, val_raw)) = after_to.split_once(" equal to ")
            && let (Ok(tgt), Ok(amt)) = (
                parse_subject_or_target(tgt_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return Some(vec![ins_dmg(src, tgt, amt)]);
        }
    }

    // 2. Scaled life gain/loss with base + per ("<player> gains/loses <base> life plus [an additional] <per> life for each <expr>")
    if let Some((who_raw, gain, rest)) = low
        .split_once(" gains ")
        .map(|(w, r)| (w, true, r))
        .or_else(|| low.split_once(" gain ").map(|(w, r)| (w, true, r)))
        .or_else(|| low.split_once(" loses ").map(|(w, r)| (w, false, r)))
        .or_else(|| low.split_once(" lose ").map(|(w, r)| (w, false, r)))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if let Some((base_raw, after_plus)) =
            split_once_any(&rest, &[" life plus an additional ", " life plus "])
            && let Some((per_raw, fe_raw)) = after_plus.split_once(" life for each ")
            && let (Ok(base_v), Ok(per_v), Ok(fe_v)) = (
                parse_value_expr(base_raw.trim(), vocab),
                parse_value_expr(per_raw.trim(), vocab),
                parse_for_each_value_expr(fe_raw.trim(), vocab),
            )
        {
            return some1(ins_life(
                player,
                ValueExprAst::Add(
                    Box::new(base_v),
                    Box::new(ValueExprAst::Mul(Box::new(per_v), Box::new(fe_v))),
                ),
                gain,
            ));
        }
        if let Some((per_raw, fe_raw)) = rest.split_once(" life for each ")
            && let Ok(per_v) = parse_value_expr(per_raw.trim(), vocab)
        {
            let fe_v = if fe_raw.trim() == "1 life they gained" {
                ValueExprAst::PriorResult
            } else {
                parse_for_each_value_expr(fe_raw.trim(), vocab).ok()?
            };
            let delta = if per_v == ValueExprAst::Const(1) {
                fe_v
            } else {
                ValueExprAst::Mul(Box::new(per_v), Box::new(fe_v))
            };
            return Some(vec![ins_life(player, delta, gain)]);
        }
    }

    // 3. Paired draw cards and lose life equal to <val> (Eventide's Shadow)
    if let Some((who_raw, val_raw)) = low
        .split_once(" draw cards and lose life equal to ")
        .or_else(|| low.split_once(" draws cards and loses life equal to "))
        && let (Some(player), Ok(val)) = (
            parse_player_ref(who_raw.trim()),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        return Some(vec![
            ins_draw(player, val.clone()),
            ins_life(player, val, false),
        ]);
    }

    // 4. "<player> gets twice that many <kind> counters" (Struggle for Project Purity)
    if let Some((who_raw, rest)) =
        split_once_any(&low, &[" gets twice that many ", " get twice that many "])
        && let Some(kind_raw) = rest.strip_suffix(" counters")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return some1(ins_player_counters(
            player,
            kind_raw.trim().to_string(),
            ValueExprAst::Mul(vconst(2), Box::new(ValueExprAst::PriorResult)),
        ));
    }

    // 5. Mana production formulas (Extraplanar Lens, Open the Omenpaths, Paliano, Belbe, Radiant Lotus, Three Tree City)
    if let Some((who_raw, rest)) = low.split_once(" adds ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if rest == "one mana of any type that land produced" {
            return some1(InstructionAst::AddManaLandProduced {
                player,
                count: Some(vconst(1)),
            });
        }
        if let Some((sym_raw, fe_raw)) = rest.split_once(" for each ")
            && let Ok(fe_val) = parse_for_each_value_expr(fe_raw.trim(), vocab)
        {
            if let Some((cnt_tok, _)) = sym_raw.trim().split_once(" mana of the chosen color")
                && let Some(n) = parse_number_word(cnt_tok.trim())
            {
                let amount = if n == 1 {
                    fe_val
                } else {
                    ValueExprAst::Mul(vconst(n), Box::new(fe_val))
                };
                return some1(ins_add_mana(
                    player,
                    ManaSpecAst::ScaledSymbol {
                        symbol: "chosen_color".to_string(),
                        amount: Box::new(amount),
                    },
                ));
            }
            if sym_raw.trim().starts_with('{') && sym_raw.trim().ends_with('}') {
                return some1(ins_add_mana(
                    player,
                    ManaSpecAst::ScaledSymbol {
                        symbol: sym_raw.trim().to_ascii_uppercase(),
                        amount: Box::new(fe_val),
                    },
                ));
            }
        }
    }
    if let Some(after) = low.strip_prefix("add ") {
        if let Some((left_m, right_m)) = after.split_once(" mana of any one color and ")
            && let Some(n2_raw) = right_m.strip_suffix(" mana of any other color")
            && let (Some(n1), Some(n2)) = (
                parse_number_word(left_m.trim()),
                parse_number_word(n2_raw.trim()),
            )
        {
            return Some(vec![
                ins_add_mana(
                    PlayerRefAst::You,
                    ManaSpecAst::AnyColor {
                        amount: vconst(n1),
                        single_color: true,
                    },
                ),
                ins_add_mana(
                    PlayerRefAst::You,
                    ManaSpecAst::AnyColor {
                        amount: vconst(n2),
                        single_color: true,
                    },
                ),
            ]);
        }
        if after == "one mana of any color chosen as you drafted cards named ~" {
            return some1(ins_add_mana(PlayerRefAst::You, ManaSpecAst::ChosenColor));
        }
        if let Some(val_raw) = after
            .strip_prefix("an amount of mana of that color equal to ")
            .or_else(|| after.strip_prefix("an amount of mana of the chosen color equal to "))
            && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
        {
            return some1(ins_add_mana(
                PlayerRefAst::You,
                ManaSpecAst::ScaledSymbol {
                    symbol: "chosen_color".to_string(),
                    amount: Box::new(val),
                },
            ));
        }
    }

    None
}

fn parse_dmg_amount_and_target(
    part: &str,
    vocab: &Vocabulary,
) -> Option<(ValueExprAst, SubjectRefAst)> {
    let clean = part.strip_prefix("an additional ").unwrap_or(part).trim();
    let (amt_raw, tgt_raw) =
        split_once_any(&clean, &[" additional damage to ", " damage to "]).unwrap_or(("", ""));
    if !amt_raw.is_empty()
        && let (Ok(amt), Ok(tgt)) = (
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some((amt, tgt));
    }
    None
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_statics_and_restrictions_extended(
    s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "<subj> become(s) copies of <src> until <dur>" (Niko, Light of Hope)
    if let Some((subj_raw, rest)) = low.split_once(" become copies of ")
        && let Some((src_raw, dur_raw)) = rest.split_once(" until ")
        && let (Ok(subject), Ok(source)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(src_raw.trim(), vocab),
        )
    {
        let copy_dur = if dur_raw.trim() == "the next end step" || dur_raw.trim() == "end of turn" {
            DurationAst::UntilEndOfTurn
        } else {
            dur.clone()
        };
        return some1(InstructionAst::BecomeCopy {
            subject: Box::new(subject),
            source: Box::new(source),
            duration: copy_dur,
            exception: None,
        });
    }

    // 2. Base P/T dynamic assignment (Galion, Trench Gorger, Brine Hag)
    if let Some(after) = low.strip_prefix("its base power and toughness become equal to ")
        && let Some(src_raw) = after
            .strip_suffix("'s power and toughness until end of turn")
            .or_else(|| after.strip_suffix("'s power and toughness"))
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        return some1(InstructionAst::SetDynamicPT {
            subject: box_anaphora("it"),
            power: Some(Box::new(val_obj_stat(src.clone(), StatKind::Power))),
            toughness: Some(Box::new(val_obj_stat(src, StatKind::Toughness))),
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    if let Some((subj_raw, val_raw)) =
        low.split_once(" has base power and base toughness each equal to ")
        && let (Ok(subject), Ok(val)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        return some1(InstructionAst::SetDynamicPT {
            subject: Box::new(subject),
            power: Some(Box::new(val.clone())),
            toughness: Some(Box::new(val)),
            duration: DurationAst::Permanent,
        });
    }
    if let Some(after) = low.strip_prefix("the base power and toughness of ")
        && let Some((subj_raw, pt_raw)) = after.split_once(" become ")
        && let Some((p_s, t_s)) = pt_raw.trim().split_once('/')
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_animate(
            subject,
            p_s.trim().to_string(),
            t_s.trim().to_string(),
            None,
            Vec::new(),
            DurationAst::Permanent,
        ));
    }

    // 3. Change land type (Vision Charm)
    if low == "each land of the first chosen type becomes the second chosen type until end of turn"
    {
        return some1(InstructionAst::BecomeChosenCharacteristic {
            subject: Box::new(SubjectRefAst::Each(Box::new(obj_type(CardTypeMask::LAND)))),
            kind: "second_chosen_land_type".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        });
    }

    // 5. Assign combat damage as though unblocked (Outmaneuver, Predatory Focus)
    let (opt_unb, unb_core) = match low.strip_prefix("you may have ") {
        Some(r) => (true, r),
        None => (false, low),
    };
    if let Some(subj_raw) = unb_core
        .strip_suffix(" assign their combat damage this turn as though they weren't blocked")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let act = ins_combat_rule(
            subj,
            "assign_combat_damage_as_though_unblocked",
            DurationAst::UntilEndOfTurn,
        );
        return Some(if opt_unb {
            vec![ins_opt1(act)]
        } else {
            vec![act]
        });
    }

    if low == "you choose how those creatures block" {
        return some1(ins_combat_rule(
            anaphora("those creatures"),
            "you_choose_how_they_block",
            DurationAst::UntilEndOfTurn,
        ));
    }
    if let Some(after) = low.strip_prefix("attacking doesn't cause ")
        && let Some((subj_raw, cond_raw)) = after.split_once(" to tap this combat if ")
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return some1(ins_temp_static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "attacking_doesnt_cause_tap".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            },
            DurationAst::UntilEndOfCombat,
        ));
    }

    // 7. Temporary revealed-hand rule (Stromgald Spy)
    if let Some(after) = low.strip_prefix("you may have ")
        && let Some((who_raw, cond_raw)) =
            after.split_once(" play with their hand revealed for as long as ")
        && let (Some(_player), Ok(cond)) = (
            parse_player_ref(who_raw.trim()),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return some1(ins_opt1(ins_temp_static(
            StaticAbilityAst::LookOrRevealRule {
                rule: format!("play_with_hand_revealed:{}", who_raw.trim()),
                filter: None,
                follow_up: Vec::new(),
                condition: Some(Box::new(cond.clone())),
            },
            DurationAst::ForAsLongAs(Box::new(cond)),
        )));
    }

    // 8. Can't lose / can't win / can't gain life (Angel's Grace, Screaming Nemesis)
    if let Some((who_raw, rule_tail)) = low
        .split_once(" can't lose the game ")
        .map(|(w, d)| (w, ("cant_lose_game", d)))
        .or_else(|| {
            low.split_once(" can't win the game ")
                .map(|(w, d)| (w, ("cant_win_game", d)))
        })
        .or_else(|| {
            low.split_once(" can't gain life ")
                .map(|(w, d)| (w, ("cant_gain_life", d)))
        })
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let rule_dur = match rule_tail.1.trim() {
            "this turn" => DurationAst::UntilEndOfTurn,
            "for the rest of the game" => DurationAst::Permanent,
            _ => return None,
        };
        return some1(ins_temp_static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: rule_tail.0.to_string(),
                filter: None,
                condition: None,
            },
            rule_dur,
        ));
    }

    // 9. Permanent goad (Jon Irenicus, Shattered One)
    if let Some(subj_raw) = low
        .strip_suffix(" is goaded for the rest of the game")
        .or_else(|| low.strip_suffix("'s goaded for the rest of the game"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_combat_rule(subj, "goaded", DurationAst::Permanent));
    }

    // 11. Activation restrictions (Merseine, Capricopian, Withering Wisps)
    if low.starts_with("only ") && low.contains(" may activate this ability") {
        return Some(vec![ins_ability_restr(low.to_string())]);
    }
    if let Some(val_raw) = low.strip_prefix("activate no more times each turn than ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return some1(InstructionAst::AbilityRestriction {
            restriction: "max_activations_per_turn".to_string(),
            condition: Some(Box::new(cond_cmp(
                ValueExprAst::EventCountThisTurn {
                    event: "ability_activations_this_turn".to_string(),
                    player: None,
                },
                CompareOp::Lt,
                val,
            ))),
        });
    }

    // 13. Sacrifice unless pay reduced mana cost (Flash)
    if let Some(after) = low.strip_prefix("sacrifice ")
        && let Some((subj_raw, delta_raw)) =
            after.split_once(" unless you pay its mana cost reduced by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_unless(
            vec![InstructionAst::Sacrifice {
                player: PlayerRefAst::You,
                count: Some(vconst(1)),
                subject: Box::new(subj),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            }],
            Some(PlayerRefAst::You),
            Some(CostAtomAst::PayMana(format!(
                "its_mana_cost_minus_{}",
                delta_raw.trim().to_ascii_uppercase()
            ))),
            None,
            None,
        ));
    }

    // 14. Modal token characteristics line (Outlaws' Merriment)
    if let Some((pt_tok, rest)) = low.split_once(' ')
        && pt_tok.contains('/')
        && pt_tok.split('/').all(|p| p.parse::<u8>().is_ok())
        && rest.contains(" with ")
    {
        return some1(InstructionAst::TokenCharacteristicsChoice(s.to_string()));
    }

    // 15. "it's attacking <player>" (Midnight Crusader Shuttle)
    if let Some(who_raw) = low.strip_prefix("it's attacking ")
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return some1(InstructionAst::EntersTappedRule {
            subject: box_anaphora("it"),
            tapped: false,
            attacking: Some(who_raw.trim().to_string()),
            duration: DurationAst::UntilEndOfCombat,
        });
    }

    // 16. "<subj> gains <kws> until end of turn, or until <cond>, whichever comes first" (Drop Tower)
    if let Some((before_or, until_part)) = low.split_once(" until end of turn, or until ")
        && let Some(cond_raw) = until_part.strip_suffix(", whichever comes first")
        && let Some((subj_raw, kw_raw)) = split_once_any(&before_or, &[" gains ", " gain "])
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        let cond = parse_condition(cond_raw.trim(), vocab)
            .ok()
            .unwrap_or_else(|| ConditionAst::EventOccurredThisTurn {
                event: cond_raw.trim().to_string(),
            });
        return some1(ins_grant_kw(
            subj,
            kws,
            DurationAst::ForAsLongAs(Box::new(ConditionAst::Not(Box::new(cond)))),
        ));
    }

    // 18. "you may activate ~'s loyalty ability <N> more times this turn" (Comet, Stellar Pup)
    if let Some(after) = low.strip_prefix("you may activate ")
        && let Some((subj_raw, rest)) = after.split_once("'s loyalty ability ")
        && let Some(num_raw) =
            strip_suffix_any(&rest, &[" more times this turn", " more time this turn"])
        && let Some(n) = parse_number_word(num_raw.trim())
        && let Ok(extra_times) = u8::try_from(n)
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_temp_static(
            StaticAbilityAst::ActivationLimitOverride {
                ability_kind: "loyalty".to_string(),
                subject: Box::new(subj),
                extra_times,
                instant_speed: false,
                condition: None,
            },
            DurationAst::UntilEndOfTurn,
        ));
    }

    None
}

#[inline(never)]
pub(super) fn parse_damage_counters_tokens_extended(
    s: &str,
    low: &str,
    _s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Multi-distinct-target damage ("~ deals <N1> damage to any target, <N2> damage to another target, and <N3> damage to a third target") (Cone of Flame)
    if let Some(after) = low.strip_prefix("~ deals ")
        && let Some((first, rest)) = after.split_once(" damage to any target, ")
        && let Some((second, third_raw)) = rest.split_once(" damage to another target, and ")
        && let Some(third) = third_raw.strip_suffix(" damage to a third target")
        && let (Ok(v1), Ok(v2), Ok(v3)) = (
            parse_value_expr(first.trim(), vocab),
            parse_value_expr(second.trim(), vocab),
            parse_value_expr(third.trim(), vocab),
        )
    {
        return Some(vec![
            ins_dmg(SubjectRefAst::SelfObject, SubjectRefAst::AnyTarget, v1),
            ins_dmg(
                SubjectRefAst::SelfObject,
                SubjectRefAst::DistinctTarget {
                    ordinal: 2,
                    filter: Box::new(ObjectFilterAst::default()),
                },
                v2,
            ),
            ins_dmg(
                SubjectRefAst::SelfObject,
                SubjectRefAst::DistinctTarget {
                    ordinal: 3,
                    filter: Box::new(ObjectFilterAst::default()),
                },
                v3,
            ),
        ]);
    }

    // 2. "~ deals double that damage [instead]" (Goblin Charbelcher)
    let no_instead = low.strip_suffix(" instead").unwrap_or(low);
    if no_instead == "~ deals double that damage" {
        let ins = ins_dmg(
            SubjectRefAst::SelfObject,
            anaphora("that target"),
            ValueExprAst::Mul(vconst(2), Box::new(ValueExprAst::PriorResult)),
        );
        return Some(if no_instead.len() != low.len() {
            vec![InstructionAst::ReplaceEffect(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 3. "prevent the next <N> damage [this way] [instead]" (Elvish Healer, Pollen Remedy)
    if let Some(after) = no_instead.strip_prefix("prevent the next ")
        && let Some(amt_raw) = strip_suffix_any(&after, &[" damage this way", " damage"])
        && let Ok(amount) = parse_value_expr(amt_raw.trim(), vocab)
    {
        let ins = InstructionAst::PreventDamage {
            amount: Box::new(amount),
            target: box_anaphora("that target"),
            duration: DurationAst::UntilEndOfTurn,
        };
        return Some(if no_instead.len() != low.len() {
            vec![InstructionAst::ReplaceEffect(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 4. Fractional player counters ("you get half x <ck> counters, rounded up/down") (Contaminated Drink)
    if let Some(after) = low.strip_prefix("you get half x ")
        && let Some((ck_raw, rnd_raw)) = after.split_once(" counters, rounded ")
    {
        let rounding = if rnd_raw.trim() == "up" {
            Rounding::Up
        } else {
            Rounding::Down
        };
        return some1(ins_player_counters(
            PlayerRefAst::You,
            ck_raw.trim().to_string(),
            val_div(ValueExprAst::ChosenX, 2, rounding),
        ));
    }

    // 5. "each creature gets twice -x/-x [dur]" (Nuclear Fallout)
    if let Some((subj_raw, rest)) = low.split_once(" gets twice ")
        && let Some((pt_tok, _)) = rest.split_once(' ').or(Some((rest, "")))
        && let Some((dx, dy)) = pt_tok.split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(InstructionAst::ModifyPT {
            subject: Box::new(subj),
            dx: dx.to_string(),
            dy: dy.to_string(),
            duration: dur.clone(),
            scale: Some(vconst(2)),
        });
    }

    // 6. "the token enters with half that many <ck> counters on it, rounded down/up" (Ochre Jelly)
    if let Some(after) = low.strip_prefix("the token enters with half that many ")
        && let Some((ck_raw, rnd_raw)) = after.split_once(" counters on it, rounded ")
    {
        let rounding = if rnd_raw.trim() == "up" {
            Rounding::Up
        } else {
            Rounding::Down
        };
        return some1(ins_counters(
            anaphora("the token"),
            ck_raw.trim().to_string(),
            val_div(ValueExprAst::PriorResult, 2, rounding),
        ));
    }

    // 7. Remove counters distributed from among all permanents (Sensational Spider-Man)
    if let Some(after) = strip_prefix_any(&low, &["you may remove up to ", "remove up to "])
        && let Some((cnt_kind, flt_raw)) = after.split_once(" counters from among ")
        && let Some((cnt_raw, ck_raw)) = cnt_kind.split_once(' ')
    {
        let flt_clean = flt_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(flt_raw.trim());
        if let (Ok(count), Ok(flt)) = (
            parse_value_expr(cnt_raw.trim(), vocab),
            parse_object_filter(flt_clean, vocab),
        ) {
            let ins = InstructionAst::RemoveCounters {
                subject: Box::new(SubjectRefAst::All(Box::new(flt))),
                counter_kind: ck_raw.trim().to_string(),
                all: false,
                count: Some(Box::new(count)),
            };
            return Some(if low.starts_with("you may ") {
                vec![ins_opt1(ins)]
            } else {
                vec![ins]
            });
        }
    }

    let build_tok_ins = |controller: PlayerRefAst, rich: crate::filter::RichTokenCreationAst| {
        if rich.is_plain() {
            ins_create_token(controller, rich.token)
        } else {
            InstructionAst::CreateQualifiedToken(Box::new(crate::ast::QualifiedTokenCreationAst {
                controller,
                token: rich.token,
                blocking: rich.blocking,
                dynamic_power: rich.dynamic_power.map(Box::new),
                dynamic_toughness: rich.dynamic_toughness.map(Box::new),
                dynamic_pt_text: rich.dynamic_pt_text,
                chosen_color: rich.chosen_color,
                chosen_type: rich.chosen_type,
                attacking_target: rich.attacking_target.map(Box::new),
            }))
        }
    };
    // 8. "remove all of them from it and create that many <token>" (Lightning Coils)
    if let Some(tok_raw) = low.strip_prefix("remove all of them from it and create that many ")
        && let Ok(mut rich) = parse_token_creation_rich(&format!("1 {tok_raw}"), vocab)
    {
        match &mut rich.token {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => {
                **count = ValueExprAst::PriorResult;
            }
            TokenSpecAst::Role { .. } => {}
        }
        return Some(vec![
            InstructionAst::RemoveCounters {
                subject: box_anaphora("it"),
                counter_kind: "all".to_string(),
                all: true,
                count: None,
            },
            build_tok_ins(PlayerRefAst::You, rich),
        ]);
    }

    // 9. "<player> creates x minus <N> <token>" (Eiganjo Uprising)
    if let Some((who_raw, rest)) = low.split_once(" creates x minus ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((sub_raw, tok_raw)) = rest.split_once(' ')
        && let Some(sub_n) = parse_number_word(sub_raw.trim())
        && let Ok(mut rich) = parse_token_creation_rich(&format!("1 {tok_raw}"), vocab)
    {
        let expr = ValueExprAst::Sub(Box::new(ValueExprAst::ChosenX), vconst(sub_n));
        match &mut rich.token {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => {
                **count = expr;
            }
            TokenSpecAst::Role { .. } => {}
        }
        return Some(vec![build_tok_ins(player, rich)]);
    }

    // 10. "create that token and a <predefined> token" (Mr. House) / characteristic tokens (Genku, Outlaws' Merriment) / copy attached (Stangg)
    if let Some(tok_raw) = low.strip_prefix("create that token and ")
        && let Ok(second_tok) = parse_token_creation_rich(tok_raw.trim(), vocab)
    {
        return Some(vec![
            ins_create_token(
                PlayerRefAst::You,
                TokenSpecAst::Predefined {
                    name: "that token".to_string(),
                    count: vconst(1),
                    tapped: false,
                },
            ),
            build_tok_ins(PlayerRefAst::You, second_tok),
        ]);
    }
    if low.starts_with("create a ") && low.ends_with(" creature token with those characteristics") {
        return some1(InstructionAst::TokenCharacteristicsChoice(s.to_string()));
    }
    if let Some(after) = low.strip_prefix("create a token that's a copy of it attached to ") {
        return some1(ins_create_token(
            PlayerRefAst::You,
            TokenSpecAst::Copy {
                count: vconst(1),
                source: box_anaphora("it"),
                tapped: false,
                attacking: false,
                exception: Some(format!("attached_to:{}", after.trim())),
            },
        ));
    }

    // 11. Advantage/stored die rolls & result-scaled scry (Berserker's Frenzy, Centaur of Attention, Boing!)
    if let Some(after) = low.strip_prefix("roll ") {
        if let Some((cnt_and_die, _)) = after.split_once(" and ignore the lower roll")
            && let Some((cnt_raw, die_raw)) = cnt_and_die.split_once(' ')
            && let Some(sides_raw) = die_raw.strip_prefix('d')
            && let (Ok(count), Ok(sides)) = (
                parse_value_expr(cnt_raw.trim(), vocab),
                sides_raw.parse::<u16>(),
            )
        {
            return some1(InstructionAst::RollDiceAndKeep {
                count: Box::new(count),
                sides,
                drop_lowest: 1,
                store_on_self: false,
            });
        }
        if let Some((cnt_raw, _)) =
            after.split_once(" six-sided dice and store those results on it")
            && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
        {
            return some1(InstructionAst::RollDiceAndKeep {
                count: Box::new(count),
                sides: 6,
                drop_lowest: 0,
                store_on_self: true,
            });
        }
    }
    if low == "scry a number of cards equal to the result" {
        return some1(InstructionAst::Scry {
            player: PlayerRefAst::You,
            count: Box::new(ValueExprAst::PriorResult),
        });
    }

    // 12. Temporary type/P-T/ability grants (Soul Sculptor, Cycle of Life, Tetravus, Wild Shape, Gabriel Angelfire)
    if let Some((subj_raw, rest)) =
        low.split_once(" becomes an enchantment and loses all abilities until ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let cond = parse_condition(rest.trim(), vocab).ok().unwrap_or_else(|| {
            ConditionAst::EventOccurredThisTurn {
                event: rest.trim().to_string(),
            }
        });
        return some1(ins_temp_static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some("Enchantment".to_string()),
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: true,
                condition: None,
            },
            DurationAst::ForAsLongAs(Box::new(ConditionAst::Not(Box::new(cond)))),
        ));
    }
    if let Some((subj_raw, rest)) = low.split_once(" has base power and toughness ")
        && let Some((pt_tok, _)) = rest.split_once(" until ")
        && let Some((p, t)) = pt_tok.trim().split_once('/')
    {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        return some1(ins_animate(
            subj,
            p.to_string(),
            t.to_string(),
            None,
            Vec::new(),
            dur.clone(),
        ));
    }
    if let Some(after) = low.strip_prefix("they each have ")
        && let Some((kw_raw, ab_raw)) = after.split_once(" and \"")
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        let ab = ab_raw.trim_end_matches('"').to_string();
        return Some(vec![
            InstructionAst::GrantKeywords {
                subject: box_anaphora("they"),
                keywords: kws,
                duration: DurationAst::Permanent,
            },
            InstructionAst::GrantAbilityText {
                subject: box_anaphora("they"),
                ability_text: ab,
                compiled: None,
                duration: DurationAst::Permanent,
            },
        ]);
    }
    if let Some((subj_raw, _)) = low.split_once(
        " has that base power and toughness, becomes that creature type, and gains that ability",
    ) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![
            ins_animate(
                subj.clone(),
                "chosen",
                "chosen",
                Some("chosen creature type".to_string()),
                Vec::new(),
                dur.clone(),
            ),
            ins_grant_ab(subj, "that ability", None, dur.clone()),
        ]);
    }
    if let Some((subj_raw, _)) = low.split_once(" gains that ability")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![ins_grant_ab(subj, "that ability", None, dur.clone())]);
    }

    None
}

#[inline(never)]
pub(super) fn parse_control_piles_combat_extended(
    _s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Exchange text boxes / exchange control / redistribute life totals
    // (Exchange of Words, Cultural Exchange, Reverse the Sands)
    if low == "exchange the text boxes of those creatures" {
        return some1(InstructionAst::ExchangeTextBoxes {
            left: box_anaphora("first creature"),
            right: box_anaphora("second creature"),
        });
    }
    if low == "those players exchange control of those creatures" {
        return some1(InstructionAst::Exchange {
            kind: ExchangeKind::Control,
            left: box_anaphora("those creatures"),
            right: None,
        });
    }
    if low == "redistribute any number of players' life totals" {
        return some1(InstructionAst::RedistributeLifeTotals {
            player: PlayerRefAst::You,
        });
    }

    // 2. Gain control of chosen/owned permanents (Dack Fayden, Scrambleverse, Order of Succession, Herald of Leshrac, Expropriate)
    if let Some((who_raw, rest)) = low.split_once(" gains control of ")
        && let Some(controller) = parse_player_ref(who_raw.trim())
        && (rest == "the permanent for which they were chosen"
            || rest == "each permanent for which they were chosen"
            || rest == "the creature they chose"
            || rest == "each land they own that you control")
    {
        return some1(ins_gain_control(
            controller,
            SubjectRefAst::Anaphora(rest.to_string()),
            DurationAst::Permanent,
        ));
    }
    if low == "choose a permanent owned by the voter and gain control of it" {
        return Some(vec![
            InstructionAst::ChooseFromGroup {
                count: vconst(1),
                random: false,
                group: box_anaphora("permanent owned by the voter"),
            },
            ins_gain_control(PlayerRefAst::You, anaphora("it"), DurationAst::Permanent),
        ]);
    }

    // 3. Structured choices (Cultural Exchange, Foreboding Steamboat, Celestial Judgment, Dack Fayden, Necrotic Plague, Tragic Arrogance)
    if let Some(after) = low.strip_prefix("choose the same number of ")
        && let Some((flt_raw, _)) = after.split_once(" another target player controls")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.controller = Some(ControllerRelation::TargetPlayer);
        flt.predicates
            .push(crate::ast::FilterPredicateAst::StateQualification(
                "another_target_player_controls".to_string(),
            ));
        return some1(ins_choose_group(ValueExprAst::PriorResult, false, grp(flt)));
    }
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(chooser) = parse_player_ref(who_raw.trim())
    {
        if let Some((cnt_raw, flt_raw)) = rest.split_once(' ')
            && let (Some(n), Ok(flt)) = (
                parse_number_word(cnt_raw.trim()),
                parse_object_filter(flt_raw.trim(), vocab),
            )
        {
            if chooser == PlayerRefAst::You {
                return some1(ins_choose_group(ValueExprAst::Const(n), false, grp(flt)));
            }
            return some1(InstructionAst::PlayerChooseFromGroup {
                chooser,
                count: vconst(n),
                random: false,
                group: Box::new(grp(flt)),
            });
        }
        if let Some(flt_raw) = rest.strip_prefix("target ")
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            let group = Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(flt),
                count: TargetCountAst::Exact(1),
            });
            if chooser == PlayerRefAst::You {
                return some1(InstructionAst::ChooseFromGroup {
                    count: vconst(1),
                    random: false,
                    group,
                });
            }
            return some1(InstructionAst::PlayerChooseFromGroup {
                chooser,
                count: vconst(1),
                random: false,
                group,
            });
        }
    }
    if low == "choose a creature with that power" {
        return some1(InstructionAst::ChooseFromGroup {
            count: vconst(1),
            random: false,
            group: box_anaphora("creature with that power"),
        });
    }
    if low == "choose a different opponent" {
        return some1(ins_choose_group(
            ValueExprAst::Const(1),
            false,
            SubjectRefAst::Player(PlayerRefAst::AnyOpponent),
        ));
    }
    if let Some(after) =
        low.strip_prefix("you choose from among the permanents that player controls ")
    {
        let mut categories = Vec::new();
        for piece in after.replace(", and ", ", ").split(", ") {
            if let Ok(flt) = parse_object_filter(piece.trim(), vocab) {
                categories.push(flt);
            }
        }
        if categories.len() >= 2 {
            return some1(InstructionAst::ChoosePerFilter {
                player: PlayerRefAst::You,
                categories,
                sacrifice_rest: false,
            });
        }
    }

    // 4. Pile separation & pile actions (Make an Example, Bend or Break, Hostile Negotiations, Triumph of Saint Katherine, Whims of the Fates)
    if let Some((who_raw, rest)) = low.split_once(" separates ")
        && let Some((flt_raw, cnt_raw)) = rest
            .split_once(" into ")
            .and_then(|(f, r)| r.strip_suffix(" piles").map(|c| (f, c)))
        && let (Some(chooser), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(pile_count) = u8::try_from(n)
    {
        let subj = parse_subject_or_target(flt_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(flt_raw.trim().to_string()));
        return some1(InstructionAst::SeparateFilteredIntoPiles {
            chooser,
            subject: Box::new(subj),
            pile_count,
            face_down_piles: 0,
        });
    }
    if low == "one of their piles is chosen by one of their opponents of their choice" {
        return some1(InstructionAst::ChoosePile {
            chooser: PlayerRefAst::AnyOpponent,
        });
    }
    if low == "shuffle that pile and put it back on top of your library" {
        return some1(ins_pile_action(
            "shuffle_to_library_top",
            PileRefAst::ThatPile,
            None,
        ));
    }
    if low == "look at the cards in each pile" {
        return Some(vec![ins_pile_action("look", PileRefAst::EachPile, None)]);
    }
    if low == "turn a pile of your choice face up" {
        return some1(ins_pile_action(
            "turn_face_up",
            PileRefAst::ChosenPile,
            None,
        ));
    }
    if low == "each player chooses one of their piles at random and sacrifices those permanents" {
        return some1(ins_pile_action(
            "choose_random_and_sacrifice",
            PileRefAst::ChosenPile,
            None,
        ));
    }

    // 5. Combat actions, fights, sacrifices, goad, and blocker limits
    // (Ezuri's Predation, Tide of War, Vislor Turlough, Noctis, Stand or Fall, False Orders, Mirri, Yare)
    if low == "each of those tokens fights a different one of those creatures" {
        return some1(InstructionAst::Fight {
            a: box_anaphora("each of those tokens"),
            b: box_anaphora("different one of those creatures"),
        });
    }
    if let Some(kind_raw) = low
        .strip_prefix("each ")
        .and_then(|r| r.strip_suffix(" creature is sacrificed by its controller"))
        && matches!(kind_raw, "attacking" | "blocking" | "blocked")
        && let Ok(flt) = parse_object_filter(&format!("{kind_raw} creatures"), vocab)
    {
        return some1(ins_sac(
            PlayerRefAst::ItsController,
            SubjectRefAst::Each(Box::new(flt)),
        ));
    }
    if let Some((subj_raw, cond_raw)) = split_once_any(
        &low,
        &["'s goaded for as long as ", " is goaded for as long as "],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let cond = parse_condition(cond_raw.trim(), vocab).ok().or_else(|| {
            let (who_raw, obj_raw) = cond_raw
                .trim()
                .split_once(" control ")
                .or_else(|| cond_raw.trim().split_once(" controls "))?;
            let player = parse_player_ref(who_raw.trim())?;
            let obj = parse_subject_or_target(obj_raw.trim(), vocab).ok()?;
            Some(ConditionAst::SubjectControlledBy {
                subject: Box::new(obj),
                player,
            })
        });
        if let Some(cond) = cond {
            return some1(ins_combat_rule(
                subj,
                "goaded",
                DurationAst::ForAsLongAs(Box::new(cond)),
            ));
        }
    }
    if let Some(subj_raw) = low.strip_suffix(" can't be blocked that combat")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_combat_rule(
            subj,
            "CantBeBlocked",
            DurationAst::UntilEndOfCombat,
        ));
    }
    if low == "only creatures in the chosen piles can block this turn" {
        return some1(ins_combat_rule(
            anaphora("creatures in the chosen piles"),
            "only_chosen_piles_can_block",
            DurationAst::UntilEndOfTurn,
        ));
    }
    if low == "have it block an attacking creature of your choice" {
        return some1(ins_combat_rule(
            anaphora("it"),
            "block_chosen_attacker",
            DurationAst::UntilEndOfCombat,
        ));
    }
    if let Some((who_raw, rest)) = low.split_once(" can't block with more than ")
        && let Some((cnt_raw, _)) = rest.split_once(" creature")
        && let (Some(player), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(max_blockers) = u8::try_from(n)
    {
        return some1(InstructionAst::ChangeBlockerLimit {
            subject: Box::new(SubjectRefAst::Player(player)),
            extra_blockers: None,
            max_blockers: Some(max_blockers),
            duration: dur.clone(),
        });
    }
    if let Some((subj_raw, rest)) = low.split_once(" can block up to ")
        && let Some((cnt_raw, _)) = rest.split_once(" additional creature")
        && let (Ok(subj), Ok(extra)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(cnt_raw.trim(), vocab),
        )
    {
        return some1(InstructionAst::ChangeBlockerLimit {
            subject: Box::new(subj),
            extra_blockers: Some(Box::new(extra)),
            max_blockers: None,
            duration: dur.clone(),
        });
    }

    None
}

/// Family 1: Typed lifetime-scoped object partitions, tags, and die-roll phase-out
/// (`Space Beleren`, `Raging River`, `Camouflage`, `Ferris Wheel`).
#[inline(never)]
pub(super) fn parse_partitions_and_phasing(
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
        return some1(ins_temp_static(
            StaticAbilityAst::BlockingPartitionConstraint {
                partition: ObjectPartitionKind::Sector,
                same_partition_only: true,
            },
            eff_dur,
        ));
    }

    // 2. Raging River S1: "<chooser> divide(s) [all ]<flt> into a \"<l1>\" pile and a \"<l2>\" pile"
    if let Some((who_raw, rest)) = split_once_any(&low, &[" divides ", " divide "])
        && let Some((flt_raw, piles_raw)) = rest.split_once(" into ")
        && let Some(labels) = parse_two_quoted_piles(piles_raw.trim())
        && let Some(chooser) = parse_player_ref(who_raw.trim())
    {
        let flt_clean = flt_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(flt_raw.trim());
        if let Ok(filter) = parse_object_filter(flt_clean, vocab) {
            return some1(InstructionAst::PartitionObjects {
                chooser,
                filter: Box::new(filter),
                kind: ObjectPartitionKind::LabeledPiles(labels),
                pile_count_expr: None,
                any_number: false,
                replaces_declare_blockers: false,
                duration: DurationAst::UntilEndOfCombat,
            });
        }
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
            return some1(ins_temp_static(
                StaticAbilityAst::CantBeBlockedExceptByAny {
                    subject: Box::new(subject),
                    allowed_filters: vec![f1, f2],
                    condition: None,
                },
                eff_dur,
            ));
        }
    }

    // 5. Camouflage S2: "[this turn, ]instead of declaring blockers, <chooser> chooses any number of <flt> and divides them into a number of piles equal to the number of attacking creatures for whom that player is the defending player"
    let camo_body = low.strip_prefix("this turn, ").unwrap_or(low);
    if let Some(after_rep) = camo_body.strip_prefix("instead of declaring blockers, ")
        && let Some((who_raw, rest)) = split_once_any(
            &after_rep,
            &[" chooses any number of ", " choose any number of "],
        )
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
        return some1(InstructionAst::PartitionObjects {
            chooser,
            filter: Box::new(filter),
            kind: ObjectPartitionKind::CombatBlockerPiles,
            pile_count_expr: Some(Box::new(ValueExprAst::Count(Box::new(defended_attackers)))),
            any_number: true,
            replaces_declare_blockers: true,
            duration: DurationAst::UntilEndOfTurn,
        });
    }

    // 6. Camouflage S3: "<flt> may likewise be put into additional piles"
    if let Some(flt_raw) = low.strip_suffix(" may likewise be put into additional piles")
        && let Ok(filter) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return some1(InstructionAst::AllowExtraPartitionMembership {
            filter: Box::new(filter),
        });
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
            return some1(InstructionAst::AssignPilesToSubjects {
                targets: Box::new(targets),
                distinct,
                random,
            });
        }
    }

    // 8. Camouflage S5: "each creature in a pile that can block the creature that pile is assigned to does so"
    if low == "each creature in a pile that can block the creature that pile is assigned to does so"
    {
        return Some(vec![InstructionAst::BlockAssignedPartitionTarget]);
    }

    // 9. Ferris Wheel S2: "<subj> phase(s) out until <roller> roll(s) [a ]<N> or less/greater [while rolling to visit your attractions]"
    if let Some((subj_raw, until_raw)) =
        split_once_any(&low, &[" phases out until ", " phase out until "])
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some((who_raw, roll_rest)) = split_once_any(&until_raw, &[" roll ", " rolls "])
        && let Some(roller) = parse_player_ref(who_raw.trim())
    {
        let (roll_core, context) =
            if let Some(c) = roll_rest.strip_suffix(" while rolling to visit your attractions") {
                (c.trim(), DieRollContextAst::VisitAttractions)
            } else {
                (roll_rest.trim(), DieRollContextAst::Any)
            };
        let roll_core = strip_prefix_any(&roll_core, &["a ", "an "]).unwrap_or(roll_core);
        let (num_raw, op) = if let Some(n) = roll_core.strip_suffix(" or less") {
            (n.trim(), CompareOp::Le)
        } else if let Some(n) = strip_suffix_any(&roll_core, &[" or greater", " or higher"]) {
            (n.trim(), CompareOp::Ge)
        } else {
            (roll_core.trim(), CompareOp::Eq)
        };
        if let Some(n) = parse_number_word(num_raw)
            && let Ok(threshold) = u16::try_from(n)
        {
            return some1(InstructionAst::PhaseOutUntilDieRoll {
                subject: Box::new(subject),
                roller,
                context,
                op,
                threshold,
            });
        }
    }

    None
}

/// Family 3 & Family 4: Boolean-predicate guessing, visibility piles, and pairwise blocker swaps
/// (`The Seventh Doctor`, `The Celestial Toymaker`, `General Jarkeld`, `Sorrow's Path`).
#[inline(never)]
pub(super) fn parse_guessing_piles_and_combat_swaps(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. The Seventh Doctor: "<guesser> guess(es) whether <lhs> is greater/less than [or equal to] <rhs>"
    if let Some((who_raw, cond_raw)) =
        split_once_any(&low, &[" guesses whether ", " guess whether "])
        && let Some(guesser) = parse_player_ref(who_raw.trim())
    {
        if let Some(cond) = parse_comparison_condition(cond_raw.trim(), vocab) {
            return some1(InstructionAst::GuessCondition {
                guesser,
                condition: Box::new(cond),
            });
        }
        if let Ok(cond) = parse_condition(cond_raw.trim(), vocab) {
            return some1(InstructionAst::GuessCondition {
                guesser,
                condition: Box::new(cond),
            });
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
        return some1(InstructionAst::ExileIntoVisibilityPiles {
            chooser: PlayerRefAst::You,
            subject: Box::new(subject),
            piles: vec![v1, v2],
        });
    }

    // 3. General Jarkeld: "each creature that's blocking exactly one of those attacking creatures stops blocking it and is blocking the other attacking creature"
    if low
        == "each creature that's blocking exactly one of those attacking creatures stops blocking it and is blocking the other attacking creature"
    {
        return some1(InstructionAst::SwapBlockerAssignments {
            perspective: CombatSwapPerspective::BlockedAttackers,
            remove_from_combat_first: false,
            exclusive_blockers_only: true,
        });
    }

    // 4. Sorrow's Path S2 then-body: "remove both of them from combat"
    if low == "remove both of them from combat" {
        return some1(InstructionAst::RemoveFromCombat {
            subject: box_anaphora("both of them"),
        });
    }

    // 5. Sorrow's Path S3: "each one then blocks all creatures the other was blocking"
    if low == "each one then blocks all creatures the other was blocking"
        || low == "each one blocks all creatures the other was blocking"
    {
        return some1(InstructionAst::SwapBlockerAssignments {
            perspective: CombatSwapPerspective::BlockingCreatures,
            remove_from_combat_first: false,
            exclusive_blockers_only: false,
        });
    }

    None
}
