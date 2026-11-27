//! Extended compositional static ability and replacement effect grammar rules
//! (continuous layers, keyword/spell grants, cost modifiers, cast permissions,
//! replacement/as-enters effects, and combat/activation/game-rule restrictions).

use super::{
    StaticOrReplacement, parse_static_or_replacement, st_alt_cast, st_cont_layer, st_cost_mod,
    st_instead, st_kw, st_kw_ast, st_perm, st_r, st_rule, st_rule_ast, st_rule_flt, st_rule_full,
    st_rule_subj, st_rule_subj_flt, st_s, st_set_type,
};
use crate::ast::{
    CardTypeMask, CombatState, CompareOp, ConditionAst, ControllerRelation, CostAtomAst,
    DurationAst, FilterPredicateAst, GameMarker, InstructionAst, ObjectFilterAst,
    ObjectFilterFlags, PlayerRefAst, PriorActionRefKind, ReplacementEffectAst, Rounding, StatKind,
    StaticAbilityAst, SubjectRefAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, parse_cost_list};
use crate::clauses::effects::{
    parse_atomic_effect, parse_effect_body, parse_effect_or_temp_clause,
};
use crate::clauses::triggers::parse_trigger_events;
use crate::combinators::{
    any_card_flt, any_perm, any_spell_flt, box_anaphora, card_flt, cond_matches, cond_zone_count,
    grp, ins_choose_group, ins_counters, ins_move_cnt, ins_opt1, ins_rm_counters, obj_type,
    self_obj, split_once_any, strip_prefix_any, strip_suffix_any, val_div, vconst,
};
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_value_expr,
};
use crate::preprocess::{is_mana_sequence, parse_pt_mod};
use crate::vocab::{Vocabulary, parse_number_word};

#[inline(never)]
pub(super) fn parse_extended_statics(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. "You may have ~ enter tapped" (Mariposa Military Base)
    if low == "you may have ~ enter tapped" || low == "you may have ~ enter the battlefield tapped"
    {
        return Some(st_r(ReplacementEffectAst::OptionalEntersTapped));
    }

    // 2. Leading "For each <over>, <static_or_replacement>" (preserves per quantifier losslessly via ScaledBy!)
    if low.starts_with("for each ")
        && let Some((over_raw, rest)) = s["for each ".len()..].split_once(", ")
        && !over_raw.is_empty()
        && !rest.is_empty()
    {
        let per_opt = parse_for_each_value_expr(over_raw.trim(), vocab)
            .ok()
            .or_else(|| {
                if over_raw
                    .to_ascii_lowercase()
                    .starts_with("kind of counter on ")
                {
                    Some(ValueExprAst::PriorResult)
                } else {
                    None
                }
            });
        if let Some(per) = per_opt
            && let Ok(inner) = parse_static_or_replacement(rest.trim(), vocab)
        {
            return Some(match inner {
                StaticOrReplacement::Static(st) => {
                    StaticOrReplacement::Static(StaticAbilityAst::ScaledBy {
                        per: Box::new(per),
                        inner: Box::new(st),
                    })
                }
                StaticOrReplacement::Replacement(rp) => {
                    StaticOrReplacement::Replacement(ReplacementEffectAst::ScaledBy {
                        per: Box::new(per),
                        inner: Box::new(rp),
                    })
                }
            });
        }
    }

    // 5. General "<subj> attacks [<tgt>] each combat if able" / "<subj> must be blocked [by <flt>] if able" (Alexios, Galactus, Ace's Baseball Bat)
    if let Some(before_able) = strip_suffix_any(&low, &[" each combat if able", " if able"]) {
        if let Some((subj_raw, tgt_raw)) = split_once_any(&before_able, &[" attacks ", " attack "])
            && let (Ok(subj), Ok(_tgt)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
            )
        {
            return Some(st_rule_subj(
                subj,
                format!("attacks_if_able:{}", tgt_raw.trim()),
            ));
        }
        if let Some((subj_raw, blk_raw)) = before_able.split_once(" must be blocked by ")
            && let (Ok(subj), Ok(flt)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_object_filter(blk_raw.trim(), vocab),
            )
        {
            return Some(st_rule_subj_flt(subj, "must_be_blocked_by", flt));
        }
    }

    // 6. General "<subj> can't attack its owner" / "<subj> can't be sacrificed"
    for (suf, rule_name) in [
        (" can't attack its owner", "cant_attack_owner"),
        (" can't be sacrificed", "cant_be_sacrificed"),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(st_rule_subj(subj, rule_name.to_string()));
        }
    }

    // 8. General "The first time <event> each turn, <effect_or_rep>" (Edgar, Mirrormind Crown)
    if low.starts_with("the first time ")
        && let Some((hdr, body_raw)) = s.split_once(", ")
        && let Some(body) =
            parse_effect_or_temp_clause(body_raw.trim(), &DurationAst::UntilEndOfTurn, vocab)
    {
        let ev_raw = hdr
            .trim()
            .strip_prefix("The first time ")
            .or_else(|| hdr.trim().strip_prefix("the first time "))
            .unwrap_or(hdr.trim())
            .trim_end_matches(" each turn")
            .trim();
        let parsed_ev = parse_trigger_events(&format!("whenever {ev_raw}"), vocab)
            .ok()
            .and_then(|mut v| if v.len() == 1 { v.pop() } else { None })
            .map(Box::new);
        return Some(st_s(StaticAbilityAst::FirstTimeEachTurnAction {
            event_text: hdr.trim().to_string(),
            event: parsed_ev,
            body,
        }));
    }

    // 9. General "you may have <subj> assign its combat damage as though it weren't blocked" (Invasion of Ikoria, Cunning Giant)
    if low.contains(" assign its combat damage ") && low.contains(" as though it weren't blocked") {
        return Some(st_rule_subj(
            SubjectRefAst::SelfObject,
            "assign_combat_damage_unblocked",
        ));
    }

    // 10. Wave 18: "(Its|The) <kw> cost is equal to its mana cost" (Falkenrath Gorger) & "<subj> can't be destroyed by lethal damage unless ..."
    if (low.starts_with("its ") || low.starts_with("the "))
        && low.ends_with(" cost is equal to its mana cost")
    {
        return Some(st_rule_subj(SubjectRefAst::SelfObject, low.to_string()));
    }
    if let Some((subj_raw, _)) = low.split_once(" can't be destroyed by lethal damage unless ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_rule_subj(subj, low.to_string()));
    }

    // Wave E2: Conspiracy / Draft-matters static rules (all 27 draft-action cards)
    if low.starts_with("reveal ~ as you draft it")
        || low.starts_with("as you draft ")
        || low.starts_with("during the draft, ")
        || low.starts_with("immediately after the draft, ")
        || low.starts_with("instead of drafting ")
        || low.starts_with("before you shuffle your deck to start the game, ")
        || low.starts_with("as long as ~ is face up during the draft, ")
        || low.starts_with("each player passes the last card from each booster pack ")
        || low.starts_with("if you removed a creature card with ")
    {
        return Some(st_s(StaticAbilityAst::DraftAction(s.to_string())));
    }

    // Wave E2: Attachment restriction ("~ can be attached only to <flt>")
    if let Some(flt_raw) = low.strip_prefix("~ can be attached only to ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::AttachmentRestriction {
            filter: Some(Box::new(flt)),
            target_player: None,
        }));
    }

    // Wave E2: Static mana-spending rules ("You may spend [<kind>] mana [only] as though it were <as_though> [to <purpose>]")
    if let Some(after_sp) = low.strip_prefix("you may spend ")
        && let Some((kind_raw, rest)) = split_once_any(
            &after_sp,
            &[" only as though it were ", " as though it were "],
        )
    {
        let only_prefix = if after_sp.contains(" only as though it were ") {
            "only:"
        } else {
            ""
        };
        let (as_though, purpose) = if let Some((a, p)) = rest.split_once(" to ") {
            (
                format!("{only_prefix}{}", a.trim()),
                Some(p.trim().to_string()),
            )
        } else {
            (format!("{only_prefix}{}", rest.trim()), None)
        };
        let mk = kind_raw.trim();
        return Some(st_s(StaticAbilityAst::ManaSpendRule {
            mana_kind: if mk == "mana" {
                None
            } else {
                Some(mk.to_string())
            },
            as_though,
            purpose,
            condition: None,
        }));
    }

    // Wave E2: Look / Reveal static rules ("Play with your hand revealed", "You may look at <flt> any time", "Reveal the first card you draw ...", "You may reveal ~ from your opening hand")
    if low == "play with your hand revealed"
        || low == "players play with their hands revealed"
        || low == "you may reveal ~ from your opening hand"
        || low == "you may reveal the first card you draw each turn as you draw it"
        || low == "reveal the first card you draw on each of your turns"
        || low == "reveal the first card you draw each turn"
    {
        return Some(st_s(StaticAbilityAst::LookOrRevealRule {
            rule: low.to_string(),
            filter: None,
            follow_up: Vec::new(),
            condition: None,
        }));
    }
    if let Some(after_lk) = low.strip_prefix("you may look at ") {
        let flt_clean = strip_suffix_any(&after_lk, &[" any time", " at any time"])
            .unwrap_or(after_lk)
            .trim();
        if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
            return Some(st_s(StaticAbilityAst::LookOrRevealRule {
                rule: "look_any_time".to_string(),
                filter: Some(Box::new(flt)),
                follow_up: Vec::new(),
                condition: None,
            }));
        }
    }

    // Wave E2: Alternative ability/keyword cost ("You may <cost> rather than pay [the] <kind> cost [of/for ...]")
    if let Some(after_ym) = low.strip_prefix("you may ")
        && let Some((cost_raw, kind_scope)) = after_ym
            .split_once(" rather than pay the ")
            .or_else(|| after_ym.split_once(" rather than pay ~'s "))
            .or_else(|| after_ym.split_once(" rather than pay "))
    {
        let cost_clean = cost_raw.strip_prefix("pay ").unwrap_or(cost_raw).trim();
        if (kind_scope.contains(" cost") || kind_scope.contains(" costs"))
            && let Ok(cost) = parse_cost_list(cost_clean, vocab)
        {
            return Some(st_s(StaticAbilityAst::AlternativeAbilityCost {
                cost,
                ability_kind: kind_scope
                    .split(" cost")
                    .next()
                    .unwrap_or(kind_scope)
                    .trim()
                    .to_string(),
                scope: kind_scope.trim().to_string(),
                condition: None,
            }));
        }
    }

    // Wave E2: Damage / counter replacement statics
    if let Some(subj_raw) = low
        .strip_prefix("double all damage ")
        .and_then(|r| r.strip_suffix(" would deal"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::DoubleAllDamage {
            source: Box::new(subj),
            condition: None,
        }));
    }
    if let Some(after_if) = low.strip_prefix("if a source would deal damage to ")
        && let Some((tgt_raw, rem)) = after_if.split_once(", prevent half that damage, rounded ")
        && let Ok(tgt) = parse_subject_or_target(tgt_raw.trim(), vocab)
    {
        let rounding = if rem.trim().starts_with("up") {
            Rounding::Up
        } else {
            Rounding::Down
        };
        return Some(st_s(StaticAbilityAst::PreventFractionDamage {
            source: None,
            target: Box::new(tgt),
            numerator: 1,
            denom: 2,
            rounding,
            condition: None,
        }));
    }
    if let Some(after_if) = low.strip_prefix("if a source would deal ")
        && let Some((amt_raw, rest)) = after_if.split_once(" or less damage to ")
        && let Some(tgt_raw) = rest.strip_suffix(", prevent that damage")
        && let (Ok(amt), Ok(tgt)) = (
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(st_s(StaticAbilityAst::PreventThresholdDamage {
            source: None,
            target: Box::new(tgt),
            max_damage: Box::new(amt),
            condition: None,
        }));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((tgt_raw, eff_raw)) =
            after_if.split_once(" would be dealt damage, prevent that damage and ")
        && let Ok(tgt) = parse_subject_or_target(tgt_raw.trim(), vocab)
        && let Ok(body) = parse_atomic_effect(eff_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::PreventAndReplaceDamage {
            target: Box::new(tgt),
            follow_up: body,
            condition: None,
        }));
    }
    if low.starts_with("if an opponent would put one or more counters on a permanent or player, they put half that many ")
    {
        let rounding = if low.ends_with("rounded up") {
            Some(Rounding::Up)
        } else if low.ends_with("rounded down") {
            Some(Rounding::Down)
        } else {
            None
        };
        return Some(st_s(
            StaticAbilityAst::CounterModificationReplacement {
                player: PlayerRefAst::EachOpponent,
                modifier: "half".to_string(),
                rounding,
                condition: None,
            },
        ));
    }

    // Wave E2: Scoped spell cost modifiers ("Spells your opponents cast during your turn / from graveyards or from exile cost {N} more/less to cast", "The first <flt> you cast during each of your turns costs {N} less to cast")
    if let Some((idx, vlen)) = low
        .find(" costs ")
        .map(|i| (i, 7))
        .or_else(|| low.find(" cost ").map(|i| (i, 6)))
        && let Some((delta_tok, tail)) = s[idx + vlen..].trim().split_once(' ')
        && is_mana_sequence(delta_tok)
        && (tail.to_ascii_lowercase().starts_with("more to cast")
            || tail.to_ascii_lowercase().starts_with("less to cast"))
    {
        let tlow = tail.to_ascii_lowercase();
        let less = tlow.starts_with("less to cast");
        let scale_expr = if let Some((_, fe)) = tlow.split_once(" for each ") {
            match parse_for_each_value_expr(fe, vocab) {
                Ok(v) => Some(Box::new(v)),
                Err(_) => return None,
            }
        } else {
            None
        };
        let subj_s = s[..idx].trim();
        let subj = parse_subject_or_target(subj_s, vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_s.to_string()));
        return Some(st_s(StaticAbilityAst::CostModifier {
            subject: Box::new(subj),
            delta: delta_tok.to_string(),
            less,
            next_only: subj_s.to_ascii_lowercase().starts_with("the first "),
            per: None,
            scale_expr,
            condition: None,
        }));
    }

    // Wave E2: Turn/timing-prefixed PlayOrCastPermission ("During each of your turns, ...", "During each player's turn, ...", "During each opponent's end step, ...", "While you're searching your library, ...", "You may play any number of lands on each of your turns", "You may play <flt1> and cast <flt2> from <zone>")
    if low == "you may play any number of lands on each of your turns" {
        return Some(st_perm(
            false,
            obj_type(CardTypeMask::LAND),
            "unlimited_lands_per_turn",
        ));
    }
    for timing_pref in [
        "during each of your turns, ",
        "during each player's turn, ",
        "during each opponent's end step, ",
        "while you're searching your library, ",
    ] {
        if let Some(rest) = low.strip_prefix(timing_pref)
            && (rest.contains("may play ") || rest.contains("may cast "))
        {
            return Some(st_perm(
                !rest.contains("may play "),
                any_card_flt(),
                format!("{timing_pref}{rest}"),
            ));
        }
    }
    if low.starts_with("you may play ") && low.contains(" and cast ") && low.contains(" from ") {
        return Some(st_perm(false, any_card_flt(), low.to_string()));
    }
    if (low.starts_with("each player may play ") || low.starts_with("you may look at "))
        && (low.contains("may play lands and cast spells from among ")
            || low.contains("play lands and cast spells from among "))
    {
        return Some(st_perm(false, any_card_flt(), low.to_string()));
    }

    // Wave E2: Can't / Must static restrictions (Solemnity, Void Winnower, Leonin Arbiter, Drannith Magistrate, Questing Beast, Gaddock Teeg, Angelic Arbiter, Ward of Bones, Ethersworn Canonist, Hand to Hand, Okk, Orcish Conscripts, Tromokratis, Seeker of Slaanesh, Nacatl War-Pride, etc.)
    if matches!(
        low,
        "players can't search libraries"
            | "players can't get counters"
            | "your opponents can't cast spells from anywhere other than their hands"
            | "each opponent can't venture into the dungeon more than once each turn"
            | "players can't pay life to cast spells or to activate abilities that aren't mana abilities"
            | "during combat, players can't cast instant spells or activate abilities that aren't mana abilities"
            | "during combat, players can't cast spells or activate abilities that aren't mana abilities"
            | "you can't choose an untapped permanent as ~'s target as you cast it"
    ) {
        return Some(st_rule(low.to_string()));
    }
    if let Some(flt_raw) = low.strip_prefix("counters can't be put on ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::PreventCounters {
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            counter_kind: None,
            condition: None,
        }));
    }
    if let Some((ck_raw, flt_raw)) = low.split_once(" counters can't be removed from ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_rule_subj(
            SubjectRefAst::All(Box::new(flt)),
            format!("cant_remove_counters:{}", ck_raw.trim()),
        ));
    }
    if let Some(flt_raw) = low
        .strip_prefix("combat damage that would be dealt by ")
        .and_then(|r| r.strip_suffix(" can't be prevented"))
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_rule_subj(
            SubjectRefAst::All(Box::new(flt)),
            "combat_damage_cant_be_prevented",
        ));
    }
    if let Some(flt_raw) = low.strip_prefix("your opponents can't block with ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_rule_subj(
            SubjectRefAst::All(Box::new(flt)),
            "opponents_cant_block_with",
        ));
    }
    if (low.starts_with("each opponent who ")
        || low.starts_with("each player who ")
        || low.starts_with("a player who "))
        && low.contains(" can't ")
    {
        return Some(st_rule(low.to_string()));
    }
    if let Some((subj_raw, cond_raw)) = low
        .split_once(" can't attack unless ")
        .or_else(|| low.split_once(" can't block unless "))
        .or_else(|| low.split_once(" can't be blocked unless "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && !cond_raw.trim().is_empty()
    {
        return Some(st_rule_subj(subj, low.to_string()));
    }
    if (low.starts_with("each opponent must attack ")
        || low.starts_with("each player must attack "))
        && low.ends_with(" if able")
    {
        return Some(st_rule(low.to_string()));
    }
    if let Some((subj_raw, rest)) = low.split_once(" must be blocked by ")
        && rest.ends_with(" if able")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_rule_subj(
            subj,
            format!("must_be_blocked_by:{}", rest.trim()),
        ));
    }
    // Wave E2 / E11: "As ~ becomes attached to a creature, <effect>" -> attach-timing replacement
    if let Some(after_ab) = low.strip_prefix("as ~ becomes attached to a creature, ")
        && let Ok(body) = parse_atomic_effect(after_ab.trim(), vocab)
    {
        let creature_flt = obj_type(crate::ast::CardTypeMask::CREATURE);
        return Some(st_r(ReplacementEffectAst::AsBecomesAttachedAction {
            target_filter: Box::new(creature_flt),
            body,
        }));
    }
    if low.starts_with("as ~ enters, it becomes your choice of ")
        || low.starts_with("as ~ enters or is turned face up, it becomes your choice of ")
    {
        return Some(st_r(ReplacementEffectAst::AsEntersAction(vec![
            InstructionAst::ChooseOption(low.to_string()),
        ])));
    }

    parse_specialized_statics_a(s, low, vocab)
}

#[inline(never)]
fn parse_specialized_statics_a(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // =========================================================================
    // Wave E3: High-yield lossless static rules
    // =========================================================================

    // E3-S1: SkipStep ("Each player skips their untap step")
    if low == "each player skips their untap step" {
        return Some(st_s(StaticAbilityAst::SkipStep {
            player: PlayerRefAst::EachPlayer,
            step: TurnStepAst::UntapStep,
            condition: None,
        }));
    }
    // E3-S2: ControlChange ("The monarch controls enchanted creature")
    if let Some(subj_raw) = low.strip_prefix("the monarch controls ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::ControlChange {
            controller: "the_monarch".to_string(),
            subject: Box::new(subj),
            condition: None,
        }));
    }
    // E3-S3: MaxCounters ("~ can't have more than seven dream counters on it")
    if let Some((subj_raw, rest)) = low.split_once(" can't have more than ")
        && let Some(cnt_kind) = strip_suffix_any(&rest, &[" counters on it", " counters on them"])
        && let Some((num_tok, kind_raw)) = cnt_kind.trim().split_once(' ')
        && let Some(max) = parse_number_word(num_tok.trim())
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::MaxCounters {
            subject: Box::new(subj),
            counter_kind: kind_raw.trim().to_string(),
            max,
        }));
    }
    // E3-S4: "<subj> can only attack alone" & "<subj> gets +P/+T and can only attack alone / can't become suspected"
    if let Some(subj_raw) = low.strip_suffix(" can only attack alone")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_rule_subj(subj, "can_only_attack_alone"));
    }
    if let Some((left, rule_s)) = low
        .split_once(" and can only attack alone")
        .map(|(l, _)| (l, "can_only_attack_alone"))
        .or_else(|| {
            low.split_once(" and can't become suspected")
                .map(|(l, _)| (l, "cant_become_suspected"))
        })
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(left.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::Compound(vec![
            st,
            st_rule_ast(None, rule_s.to_string(), None),
        ])));
    }
    // E3-S5: "~ gets +P/+T for every N <flt>" (Dark Matter Manipulator)
    if let Some((pt_part, fe_raw)) = low.split_once(" for every ")
        && let Some((num_tok, flt_raw)) = fe_raw.trim().split_once(' ')
        && let Some(denom) = parse_number_word(num_tok.trim())
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        && let Ok(StaticOrReplacement::Static(StaticAbilityAst::ContinuousModifyPT {
            subject,
            dx,
            dy,
            grant_keywords,
            grant_ability_text,
            condition,
            paired_rule,
            ..
        })) = parse_static_or_replacement(pt_part.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::ContinuousModifyPT {
            subject,
            dx,
            dy,
            grant_keywords,
            grant_ability_text,
            scale: Some(Box::new(val_div(
                ValueExprAst::Count(Box::new(flt)),
                denom,
                Rounding::Down,
            ))),
            condition,
            paired_rule,
        }));
    }
    // E3-S6: Static ManaSpendRule variants ("Players may spend mana as though it were mana of any color", "Mana of any type can be spent to ...")
    if low == "players may spend mana as though it were mana of any color"
        || low.starts_with("mana of any type can be spent to ")
    {
        return Some(st_s(StaticAbilityAst::ManaSpendRule {
            mana_kind: None,
            as_though: "any_color_or_type".to_string(),
            purpose: Some(low.to_string()),
            condition: None,
        }));
    }
    // E3-S7: InheritAbilities ("~ has all/each activated abilities of <subj>")
    if let Some((subj_raw, src_raw)) = low
        .split_once(" has all activated abilities of ")
        .or_else(|| low.split_once(" has each activated ability of "))
        .or_else(|| low.split_once(" has all abilities of "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let src_flt = parse_object_filter(src_raw.trim(), vocab).unwrap_or_else(|_| any_card_flt());
        return Some(st_s(StaticAbilityAst::InheritAbilities {
            subject: Box::new(subj),
            source_filter: Box::new(src_flt),
            activated_only: low.contains("activated abilit"),
            condition: None,
        }));
    }
    // E3-S9: "If <subj> attacks, all <flt> attack if able" (Viashino Bey, War's Toll, Magnetic Web)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((_, rest)) = after_if.split_once(" attacks, all ")
        && rest.ends_with(" attack if able")
    {
        return Some(st_rule(low.to_string()));
    }
    // E3-S10: Toughness-based crew/saddle/station statics
    if low.contains(" using its toughness rather than its power")
        && (low.contains("crews vehicles")
            || low.contains("saddles mounts")
            || low.contains("stations permanents"))
    {
        return Some(st_rule(s.to_string()));
    }

    parse_specialized_statics_b(s, low, vocab)
}

#[inline(never)]
fn parse_specialized_statics_b(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // E4-S1: "<flt> are Equipment in addition to their other types and have ..." (Armed with Proof, Arterial Alchemy, Gemcutter Buccaneer, Dan Lewis, Bludgeon Brawl)
    if let Some((subj_raw, rest)) = low
        .split_once(" are equipment in addition to their other types and have ")
        .or_else(|| low.split_once(" is an equipment with "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let orig_rest = s[s.len() - rest.len()..].trim();
        let granted_abilities = crate::compile_granted_ability(orig_rest.trim_matches('"'), vocab);
        return Some(st_s(StaticAbilityAst::BecomesEquipmentWith {
            subject: Box::new(subj),
            additive_types: low.contains("in addition to"),
            granted_text: orig_rest.to_string(),
            granted_abilities,
        }));
    }
    // E4-S2: "<subj> loses/lose all land types and abilities and ..." (Lithoform Blight, Alpine Moon)
    if let Some((subj_raw, rest)) = low
        .split_once(" loses all land types and abilities and ")
        .or_else(|| low.split_once(" lose all land types and abilities, and "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let orig_rest = s[s.len() - rest.len()..].trim();
        let granted_abilities = orig_rest
            .split_once('"')
            .and_then(|(_, q)| q.rsplit_once('"').map(|(inner, _)| inner))
            .and_then(|inner| crate::compile_granted_ability(inner, vocab));
        return Some(st_s(StaticAbilityAst::LoseLandTypesAndGain {
            subject: Box::new(subj),
            granted_text: orig_rest.to_string(),
            granted_abilities,
        }));
    }
    // E4-S3: "<subj> is a creature in addition to its other types and has base power and base toughness each equal to its mana value" (Opalescence, Starfield of Nyx)
    if let Some(subj_raw) = low.strip_suffix(
        " is a creature in addition to its other types and has base power and base toughness each equal to its mana value",
    ) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let mv = ValueExprAst::ObjectStat {
            subject: box_anaphora("it"),
            stat: StatKind::ManaValue,
        };
        return Some(st_s(
            StaticAbilityAst::DynamicBasePT {
                subject: Box::new(subj),
                power: Box::new(mv.clone()),
                toughness: Box::new(mv),
                condition: None,
            },
        ));
    }
    // E4-S4: "<subj> gets +X/+Y, where X is <e1> and Y is <e2>" (Phyrexian Ingester, Aspect of Wolf)
    if let Some((pt_raw, xy_raw)) = low.split_once(", where x is ")
        && let Some((x_raw, y_raw)) = split_once_any(&xy_raw, &[", and y is ", " and y is "])
        && let Some((subj_raw, mod_raw)) = split_once_any(&pt_raw, &[" gets ", " get "])
        && let Some((dx_raw, dy_raw)) = parse_pt_mod(mod_raw.trim())
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        if let (Ok(x_expr), Ok(y_expr)) = (
            parse_value_expr(x_raw.trim(), vocab),
            parse_value_expr(y_raw.trim(), vocab),
        ) {
            let zero_y = if dy_raw.starts_with('-') { "-0" } else { "+0" };
            let zero_x = if dx_raw.starts_with('-') { "-0" } else { "+0" };
            let dy_norm = dy_raw.replace(['Y', 'y'], "X");
            return Some(st_s(StaticAbilityAst::Compound(vec![
                StaticAbilityAst::ContinuousModifyPT {
                    subject: Box::new(subj.clone()),
                    dx: dx_raw,
                    dy: zero_y.to_string(),
                    grant_keywords: Vec::new(),
                    grant_ability_text: None,
                    scale: Some(Box::new(x_expr)),
                    condition: None,
                    paired_rule: None,
                },
                StaticAbilityAst::ContinuousModifyPT {
                    subject: Box::new(subj),
                    dx: zero_x.to_string(),
                    dy: dy_norm,
                    grant_keywords: Vec::new(),
                    grant_ability_text: None,
                    scale: Some(Box::new(y_expr)),
                    condition: None,
                    paired_rule: None,
                },
            ])));
        }
        return None;
    }
    // E4-S5: Opening-hand pregame action ("If ~ is in your opening hand [and ...], you may begin the game with ~ on the battlefield ...")
    if low.starts_with("if ~ is in your opening hand")
        && low.contains("you may begin the game with ~ on the battlefield")
    {
        return Some(st_rule_subj(SubjectRefAst::SelfObject, s.to_string()));
    }
    // E4-S6: Trigger suppression/doubling ("Permanents entering don't cause abilities of <flt> to trigger", "If a triggered ability of ... triggers while ...")
    if low
        == "permanents entering don't cause abilities of permanents your opponents control to trigger"
        || low.starts_with("if a triggered ability of another shrine you control triggers while ")
    {
        return Some(st_s(StaticAbilityAst::TriggerMultiplier {
            event_desc: s.to_string(),
            multiplier: if low.contains("don't cause") { 0 } else { 2 },
            condition: None,
        }));
    }
    // E4-S7: "If ~ is in a graveyard, effects from spells named <Name> count it as a card named <Name>" (Pardic Firecat, Diligent Farmhand)
    if low.starts_with("if ~ is in a graveyard, effects from spells named ")
        && low.contains(" count it as a card named ")
    {
        return Some(st_rule_subj(SubjectRefAst::SelfObject, s.to_string()));
    }
    // E4-S8: Mana-production replacement ("If tapped for mana, Plains produce ...", "If a land is tapped for two or more mana, it produces {C} instead ...")
    if low.starts_with("if tapped for mana, plains produce ")
        || low
            == "if a land is tapped for two or more mana, it produces {c} instead of any other type and amount"
    {
        return Some(st_rule(s.to_string()));
    }
    parse_grant_and_cost_statics(s, low, vocab)
        .or_else(|| parse_chained_and_enters_statics(s, low, vocab))
        .or_else(|| parse_compositional_statics(s, low, vocab))
}

/// Strictly lossless Wave E6 static/replacement parser.
#[inline(never)]
fn parse_grant_and_cost_statics(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // Eriette of the Charmed Apple: "Each creature that's enchanted by an Aura you control can't attack you or planeswalkers you control."
    if let Some((subj_raw, rest)) = low.split_once(" can't attack ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_rule_subj(subj, format!("cant_attack:{}", rest.trim())));
    }

    None
}

/// Compositional Wave E7/E8 static and replacement parser.
#[inline(never)]
fn parse_chained_and_enters_statics(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    let low = low.trim_end_matches('.');

    // 1. Chained replacement effect with success/failure follow-up sentences (Chains of Mephistopheles)
    if let Some(sr) = parse_chained_draw_replacement(low, vocab) {
        return Some(sr);
    }
    // 2. As-enters exile two matching cards, copy one with additional counters equal to the other's stat (The Mimeoplasm)
    if let Some(sr) = parse_as_enters_exile_copy_counters(low, vocab) {
        return Some(sr);
    }
    // 4. Conditional -X/-Y static with min-capped Y (Snowblind)
    if let Some(sr) = parse_conditional_min_modify_pt(low, vocab) {
        return Some(sr);
    }
    // 5. Untap-step single-type choice restriction (Storage Matrix)
    if let Some(sr) = parse_untap_step_type_choice(low) {
        return Some(sr);
    }
    let _ = s;
    None
}

fn parse_chained_draw_replacement(low: &str, vocab: &Vocabulary) -> Option<StaticOrReplacement> {
    // "if a player would draw a card except <exc>, <act1> instead. if the player <did>, <act2>. if the player doesn't <did>, <act3>"
    let after_if = low.strip_prefix("if a player would draw a card except ")?;
    let (exc, rest1) = after_if.split_once(", ")?;
    let (act1_raw, rest2) = rest1.split_once(" instead. if the player ")?;
    let (_, after_did) = rest2.split_once(" this way, ")?;
    let (act2_raw, rest3) = after_did.split_once(". if the player doesn't ")?;
    let (_, act3_raw) = rest3.split_once(" this way, ")?;

    let (mut body, fails1) = parse_effect_body(act1_raw.trim(), vocab);
    let (succ_body, fails2) = parse_effect_body(act2_raw.trim(), vocab);
    let (fail_body, fails3) = parse_effect_body(act3_raw.trim(), vocab);
    if !fails1.is_empty() || !fails2.is_empty() || !fails3.is_empty() || body.is_empty() {
        return None;
    }
    body.push(InstructionAst::IfPreviousSucceeded(succ_body));
    body.push(InstructionAst::IfPreviousFailed(fail_body));
    Some(StaticOrReplacement::Replacement(
        ReplacementEffectAst::EventInstead {
            subject_or_event: "a player".to_string(),
            would: format!("draw a card except {}", exc.trim()),
            instead: "chained_draw_replacement".to_string(),
            instead_body: Some(body),
        },
    ))
}

fn parse_as_enters_exile_copy_counters(
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // "as ~ enters, you may exile two <filter>. if you do, it enters as a copy of one of those cards with a number of additional <kind> counters on it equal to the <stat> of the other card"
    let after = low.strip_prefix("as ~ enters, you may exile two ")?;
    let (flt_raw, rest) = after.split_once(
        ". if you do, it enters as a copy of one of those cards with a number of additional ",
    )?;
    let (kind_raw, stat_raw) = rest.split_once(" counters on it equal to the ")?;
    let stat = match stat_raw.strip_suffix(" of the other card")? {
        "power" => StatKind::Power,
        "toughness" => StatKind::Toughness,
        "mana value" => StatKind::ManaValue,
        _ => return None,
    };
    let src_flt = parse_object_filter(flt_raw.trim(), vocab).ok()?;
    let mut exiled_copy_flt = src_flt.clone();
    exiled_copy_flt.zone = None;
    exiled_copy_flt
        .predicates
        .push(FilterPredicateAst::AffectedByPriorAction(
            PriorActionRefKind::ExiledThisWay,
        ));
    Some(StaticOrReplacement::Replacement(
        ReplacementEffectAst::Compound(vec![
            ReplacementEffectAst::AsEntersAction(vec![ins_opt1(ins_move_cnt(
                Some(PlayerRefAst::You),
                Some(ValueExprAst::Const(2)),
                false,
                false,
                grp(src_flt.clone()),
                "exile",
            ))]),
            ReplacementEffectAst::EntersAsCopy {
                optional: false,
                filter: Box::new(exiled_copy_flt),
                exception: Some("if_exiled_two_cards_as_enters".to_string()),
            },
            ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::ObjectStat {
                    subject: box_anaphora("the other card"),
                    stat,
                }),
                per: None,
                x_expr: None,
                condition: Some(Box::new(ConditionAst::Marker(
                    GameMarker::ZoneActionHappenedThisWay {
                        filter: Box::new(src_flt),
                        action: "exile".to_string(),
                    },
                ))),
            },
        ]),
    ))
}

fn parse_conditional_min_modify_pt(low: &str, vocab: &Vocabulary) -> Option<StaticOrReplacement> {
    // "<subj> gets -x/-y. if that creature is <cond>, x is <e1>. otherwise, x is <e2>. y is equal to x or to <e3>, whichever is smaller"
    let (subj_raw, rest1) = low.split_once(" gets -x/-y. if ")?;
    let (cond_raw, rest2) = rest1.split_once(", x is ")?;
    let (e1_raw, rest3) = rest2.split_once(". otherwise, x is ")?;
    let (e2_raw, rest4) = rest3.split_once(". y is equal to x or to ")?;
    let e3_raw = rest4.strip_suffix(", whichever is smaller")?;

    let subj = parse_subject_or_target(subj_raw.trim(), vocab).ok()?;
    let cond = if cond_raw.trim() == "that creature is attacking" {
        let mut att = obj_type(CardTypeMask::CREATURE);
        att.combat = Some(CombatState::Attacking);
        cond_matches(subj.clone(), att)
    } else {
        parse_condition(cond_raw.trim(), vocab).ok()?
    };
    let e1 = parse_value_expr(e1_raw.trim(), vocab).ok()?;
    let e2 = parse_value_expr(e2_raw.trim(), vocab).ok()?;
    let e3 = if let Some(stat_part) = e3_raw.strip_suffix(" minus 1") {
        let base = parse_value_expr(stat_part.trim(), vocab).ok()?;
        ValueExprAst::Sub(Box::new(base), vconst(1))
    } else {
        parse_value_expr(e3_raw.trim(), vocab).ok()?
    };
    let x_expr = ValueExprAst::Conditional {
        condition: Box::new(cond),
        then_expr: Box::new(e1),
        else_expr: Box::new(e2),
    };
    let y_expr = ValueExprAst::Min(Box::new(x_expr.clone()), Box::new(e3));
    Some(StaticOrReplacement::Static(
        StaticAbilityAst::DynamicModifyPT {
            subject: Box::new(subj),
            power_delta: Box::new(ValueExprAst::Sub(vconst(0), Box::new(x_expr))),
            toughness_delta: Box::new(ValueExprAst::Sub(vconst(0), Box::new(y_expr))),
            condition: None,
        },
    ))
}

fn parse_untap_step_type_choice(low: &str) -> Option<StaticOrReplacement> {
    let after = low.strip_prefix("each player chooses ")?;
    let (types_part, tail) = after.split_once(" during their untap step. ")?;
    if tail != "that player can untap only permanents of the chosen type this step" {
        return None;
    }
    let normalized = types_part.trim().replace(", or ", ",").replace(", ", ",");
    if normalized.is_empty() {
        return None;
    }
    Some(st_rule(format!(
        "untap_step_choose_type_only_untap:{normalized}"
    )))
}

/// Compositional Wave E8 static and replacement parser replacing legacy
/// `E3_STATIC_EXACTS` and `E4_STATIC_EXACTS` literal tables with reusable
/// atomic static and replacement grammars.
#[inline(never)]
fn parse_compositional_statics(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    let low = low.trim_end_matches('.');
    let s_clean = s.trim_end_matches('.');

    // 1. Continuous statics, multi-clause grants, conditional layers, and dynamic P/T
    if let Some(sr) = parse_continuous_and_conditional_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 2. Keyword/ability grants to spells/cards, craft/devour/landwalk/delve statics
    if let Some(sr) = parse_keyword_and_spell_grant_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 3. Additional/alternative cast & activation costs, mana spend rules, and permissions
    if let Some(sr) = parse_cost_and_permission_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 4. Compositional as-enters, die-roll, discard, draw, death, and search replacements
    if let Some(sr) = parse_replacement_and_enters_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 5. Combat damage assignment, activation limit/timing overrides, and game-rule statics
    parse_combat_activation_and_rule_statics_e8(s_clean, low, vocab)
}

fn extract_quoted_segments(s: &str) -> (Vec<String>, String) {
    let mut quoted = Vec::new();
    let mut rest = String::with_capacity(s.len());
    let mut in_quote = false;
    let mut cur = String::new();
    for ch in s.chars() {
        if ch == '"' {
            if in_quote {
                quoted.push(cur.trim().trim_end_matches(',').to_string());
                cur.clear();
                in_quote = false;
            } else {
                in_quote = true;
            }
        } else if in_quote {
            cur.push(ch);
        } else {
            rest.push(ch);
        }
    }
    (quoted, rest)
}

fn parse_simple_color_condition(cond_raw: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    let c = cond_raw.trim();
    if let Ok(cond) = parse_condition(c, vocab) {
        return Some(cond);
    }
    None
}

fn parse_dynamic_pt_expr(raw: &str, vocab: &Vocabulary) -> Option<ValueExprAst> {
    let r = raw.trim();
    if let Ok(v) = parse_value_expr(r, vocab) {
        return Some(v);
    }
    if let Some((lhs_raw, rhs_raw)) = r.split_once(" minus ")
        && let (Some(l), Some(rhs)) = (
            parse_dynamic_pt_expr(lhs_raw, vocab),
            parse_dynamic_pt_expr(rhs_raw, vocab),
        )
    {
        return Some(ValueExprAst::Sub(Box::new(l), Box::new(rhs)));
    }
    if r == "the number of those card types" {
        return Some(ValueExprAst::CardTypesInGraveyard {
            all_graveyards: false,
        });
    }
    None
}

fn parse_continuous_and_conditional_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. Additive "also get(s) +P/+T ..." (Jetmir, Nexus of Revels and future scaling anthem cards)
    if low.contains(" also get ") || low.contains(" also gets ") {
        let stripped = s
            .replace(" also get ", " get ")
            .replace(" also gets ", " gets ");
        if let Ok(sr) = parse_static_or_replacement(&stripped, vocab) {
            return Some(sr);
        }
    }

    // 2. Multi-condition keyword/ability list:
    // "<subj> has <grant1> (if|as long as) <cond1>, <grant2> (if|as long as) <cond2>, ..., and <grantN> (if|as long as) <condN>"
    // (Scion of Draco, Multiclass Baldric, Tribal Golem)
    if let Some((subj_raw, rest_low)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && rest_low.contains(", ")
        && (rest_low.contains(" if ") || rest_low.contains(" as long as "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        let mut items = Vec::new();
        let mut all_ok = true;
        // Split on ", " outside quotes
        let mut parts = Vec::new();
        let mut in_q = false;
        let mut start = 0usize;
        let bytes = rest_s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                in_q = !in_q;
            } else if !in_q && i + 1 < bytes.len() && bytes[i] == b',' && bytes[i + 1] == b' ' {
                parts.push(rest_s[start..i].trim());
                start = i + 2;
                i += 1;
            }
            i += 1;
        }
        parts.push(rest_s[start..].trim());
        if parts.len() >= 2 {
            for raw_part in parts {
                let cleaned_s = strip_prefix_any(&raw_part, &["and ", "And "])
                    .unwrap_or(raw_part)
                    .trim();
                let cleaned_low = cleaned_s.to_ascii_lowercase();
                let split_res = cleaned_low
                    .split_once(" as long as ")
                    .map(|(g, c)| (g, c, " as long as ".len()))
                    .or_else(|| cleaned_low.split_once(" if ").map(|(g, c)| (g, c, 4)));
                let Some((grant_low, cond_low, _)) = split_res else {
                    all_ok = false;
                    break;
                };
                let Some(cond) = parse_simple_color_condition(cond_low, vocab) else {
                    all_ok = false;
                    break;
                };
                let grant_s = cleaned_s[..grant_low.len()].trim();
                if grant_s.starts_with('"') && grant_s.ends_with('"') && grant_s.len() >= 2 {
                    items.push(StaticAbilityAst::GrantAbilityText {
                        subject: Box::new(subj.clone()),
                        ability_text: grant_s[1..grant_s.len() - 1].to_string(),
                        compiled: None,
                        condition: Some(Box::new(cond)),
                    });
                } else if let Some(kws) = parse_keyword_list(grant_low.trim(), vocab) {
                    items.push(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: kws,
                        condition: Some(Box::new(cond)),
                        paired_rule: None,
                    });
                } else {
                    all_ok = false;
                    break;
                }
            }
            if all_ok && !items.is_empty() {
                return Some(st_s(StaticAbilityAst::Compound(items)));
            }
        }
    }

    // 3. "<subj> is <supertype/type>, gets <+P/+T>, and has <kws>" (On Serra's Wings)
    if let Some((subj_raw, rest)) = low.split_once(" is ")
        && let Some((ty_raw, pt_and_kws)) = rest.split_once(", gets ")
        && let Some((pt_raw, kws_raw)) = pt_and_kws.split_once(", and has ")
        && let Some((dx, dy)) = pt_raw.trim().split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some(kws) = parse_keyword_list(kws_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::Compound(vec![
            StaticAbilityAst::SetOrAddType {
                subject: Box::new(subj.clone()),
                type_or_subtype: ty_raw.trim().to_string(),
                additive: true,
                condition: None,
            },
            StaticAbilityAst::ContinuousModifyPT {
                subject: Box::new(subj),
                dx: dx.trim().to_string(),
                dy: dy.trim().to_string(),
                grant_keywords: kws,
                grant_ability_text: None,
                scale: None,
                condition: None,
                paired_rule: None,
            },
        ])));
    }

    // 4. "<subj> get(s) <+P/+T> and has/have <kws_and_quoted_abilities>" (Cultist of the Absolute, Spy Kit)
    if let Some((subj_raw, rest)) = low.split_once(" get ").or_else(|| low.split_once(" gets "))
        && let Some((pt_raw, grant_low)) = split_once_any(&rest, &[" and have ", " and has "])
        && let Some((dx, dy)) = pt_raw.trim().split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let grant_s = &s[s.len() - grant_low.len()..];
        let (quoted, unquoted) = extract_quoted_segments(grant_s);
        if !quoted.is_empty() {
            let mut items = Vec::new();
            let kw_clean = unquoted
                .replace(", and ", ", ")
                .replace(" and ", ", ")
                .trim_matches(|c: char| c == ',' || c == ' ')
                .to_ascii_lowercase();
            let kws = if kw_clean.is_empty() {
                Vec::new()
            } else {
                parse_keyword_list(&kw_clean, vocab).unwrap_or_default()
            };
            items.push(StaticAbilityAst::ContinuousModifyPT {
                subject: Box::new(subj.clone()),
                dx: dx.trim().to_string(),
                dy: dy.trim().to_string(),
                grant_keywords: kws,
                grant_ability_text: None,
                scale: None,
                condition: None,
                paired_rule: None,
            });
            for ab in quoted {
                items.push(StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(subj.clone()),
                    ability_text: ab,
                    compiled: None,
                    condition: None,
                });
            }
            return Some(st_s(StaticAbilityAst::Compound(items)));
        }
        if let Some(names_flt) = grant_low
            .strip_prefix("all names of ")
            .and_then(|r| r.strip_suffix(" in addition to its name"))
            && let Ok(flt) = parse_object_filter(names_flt.trim(), vocab)
        {
            return Some(st_s(StaticAbilityAst::Compound(vec![
                StaticAbilityAst::ContinuousModifyPT {
                    subject: Box::new(subj.clone()),
                    dx: dx.trim().to_string(),
                    dy: dy.trim().to_string(),
                    grant_keywords: Vec::new(),
                    grant_ability_text: None,
                    scale: None,
                    condition: None,
                    paired_rule: None,
                },
                st_rule_ast(Some(subj), "has_all_names_of", Some(flt)),
            ])));
        }
    }

    // 5. "<subj> has/have <kw>, \"<ab1>,\" and \"<ab2>\"" (Swashbuckler's Whip)
    if s.contains('"')
        && let Some((subj_raw, grant_low)) =
            low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let grant_s = &s[s.len() - grant_low.len()..];
        let (quoted, unquoted) = extract_quoted_segments(grant_s);
        if !quoted.is_empty() {
            let mut items = Vec::new();
            let kw_clean = unquoted
                .replace(", and ", ", ")
                .replace(" and ", ", ")
                .trim_matches(|c: char| c == ',' || c == ' ')
                .to_ascii_lowercase();
            if !kw_clean.is_empty()
                && let Some(kws) = parse_keyword_list(&kw_clean, vocab)
            {
                items.push(st_kw_ast(subj.clone(), kws));
            }
            for ab in quoted {
                items.push(StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(subj.clone()),
                    ability_text: ab,
                    compiled: None,
                    condition: None,
                });
            }
            if !items.is_empty() {
                return Some(st_s(StaticAbilityAst::Compound(items)));
            }
        }
    }

    // 7. "<subj> have/has base power and toughness <P>/<T> and lose all <types>" (Curse of Conformity)
    if let Some((subj_raw, rest)) = split_once_any(
        &low,
        &[
            " have base power and toughness ",
            " has base power and toughness ",
        ],
    ) && let Some((pt_raw, lose_raw)) =
        split_once_any(&rest, &[" and lose all ", " and loses all "])
        && let Some((p, t)) = pt_raw.trim().split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_cont_layer(
            subj,
            Some(format!("lose_all:{}", lose_raw.trim())),
            false,
            Some((p.trim().to_string(), t.trim().to_string())),
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }

    // 8. Compound subject list "<subj1>, <subj2>, and <subj3> have <kws>" (Shalai, Shield of Kaldra)
    if let Some((subjs_raw, kw_raw)) = low.split_once(" have ")
        && subjs_raw.contains(", and ")
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        let mut subjs = Vec::new();
        let mut ok = true;
        for part in subjs_raw.split(", ") {
            let p = part.strip_prefix("and ").unwrap_or(part).trim();
            if let Ok(sr) = parse_subject_or_target(p, vocab) {
                subjs.push(sr);
            } else if let Some(pl) = parse_player_ref(p) {
                subjs.push(SubjectRefAst::Player(pl));
            } else if !p.is_empty() {
                subjs.push(SubjectRefAst::Anaphora(p.to_string()));
            } else {
                ok = false;
                break;
            }
        }
        if ok && subjs.len() >= 2 {
            return Some(st_kw(SubjectRefAst::Compound(subjs), kws));
        }
    }

    // 9. Trailing conditional keyword grant "<subj> have/has <kws> as long as <cond>" (Sheltering Prayers)
    if let Some((left, cond_raw)) = low.split_once(" as long as ")
        && let Some((subj_raw, kw_raw)) = split_once_any(&left, &[" have ", " has "])
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .or_else(|_| {
                subj_raw
                    .trim()
                    .strip_suffix(" each player controls")
                    .ok_or_else(String::new)
                    .and_then(|base| {
                        parse_object_filter(base, vocab).map(|mut f| {
                            f.controller = Some(ControllerRelation::ThatPlayer);
                            SubjectRefAst::All(Box::new(f))
                        })
                    })
            })
            .ok()?;
        return Some(st_s(StaticAbilityAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: kws,
            condition: Some(Box::new(cond)),
            paired_rule: None,
        }));
    }

    // 10. Turn-scoped dynamic base P/T ("During your turn / During turns other than yours, <subj>'s power and toughness are each [equal to] <val>") (Angry Mob)
    for (pref, is_your_turn) in [
        ("during your turn, ", true),
        ("during turns other than yours, ", false),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Some((subj_raw, expr_raw)) = rest
                .split_once("'s power and toughness are each equal to ")
                .or_else(|| rest.split_once("'s power and toughness are each "))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
            && let Some(val) = parse_dynamic_pt_expr(expr_raw, vocab)
        {
            let cond = if is_your_turn {
                ConditionAst::IsYourTurn
            } else {
                ConditionAst::Not(Box::new(ConditionAst::IsYourTurn))
            };
            return Some(st_s(StaticAbilityAst::DynamicBasePT {
                subject: Box::new(subj),
                power: Box::new(val.clone()),
                toughness: Box::new(val),
                condition: Some(Box::new(cond)),
            }));
        }
    }

    // 11. Universal "As long as <cond>, <inner>" conditional static wrapper
    // (Duplicant, Volrath's Shapeshifter, Winter, Woolly Razorback, Camel, Futurist Operative, Trinisphere, Gaea's Liege)
    if let Some(after_al) = low.strip_prefix("as long as ")
        && let Some((cond_raw, rest_low)) = after_al.split_once(", ")
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        // Dynamic P/T ("its/~'s power and toughness are each equal to <expr>") (Gaea's Liege)
        if let Some(expr_raw) = rest_low
            .strip_prefix("its power and toughness are each equal to ")
            .or_else(|| rest_low.strip_prefix("~'s power and toughness are each equal to "))
            && let Some(val) = parse_dynamic_pt_expr(expr_raw, vocab)
        {
            return Some(st_s(StaticAbilityAst::DynamicBasePT {
                subject: self_obj(),
                power: Box::new(val.clone()),
                toughness: Box::new(val),
                condition: Some(Box::new(cond)),
            }));
        }
        // Maximum hand size ("<player>'s maximum hand size is equal to <expr>") (Winter, Misanthropic Guide)
        if let Some((who_raw, expr_raw)) = rest_low
            .split_once("'s maximum hand size is equal to ")
            .or_else(|| rest_low.split_once("'s maximum hand size is "))
            && let Some(player) = parse_player_ref(who_raw.trim())
            && let Some(size) = parse_dynamic_pt_expr(expr_raw, vocab)
        {
            return Some(st_s(StaticAbilityAst::ModifyMaximumHandSize {
                player,
                size: Box::new(size),
                condition: Some(Box::new(cond)),
            }));
        }
        // Minimum spell cost floor ("each spell that would cost less than <N> mana to cast costs <N> mana to cast") (Trinisphere)
        if let Some(after_sp) = rest_low.strip_prefix("each spell that would cost less than ")
            && let Some((num_raw, _)) = after_sp.split_once(" mana to cast costs ")
            && let Some(n) = parse_number_word(num_raw.trim())
        {
            return Some(st_cost_mod(
                SubjectRefAst::All(Box::new(any_spell_flt())),
                format!("min_{n}"),
                false,
                false,
                None,
                Some(ValueExprAst::Const(n)),
                Some(cond),
            ));
        }
        // Copy stats/types or full text of referenced card (Duplicant, Volrath's Shapeshifter)
        if let Some(src_raw) =
            rest_low.strip_prefix("~ has the power, toughness, and creature types of ")
        {
            let src_flt =
                parse_object_filter(src_raw.trim(), vocab).unwrap_or_else(|_| ObjectFilterAst {
                    any_card: true,
                    zone: Some(ZoneFilter::Exile),
                    flags: ObjectFilterFlags::EXILED_WITH_SELF,
                    ..Default::default()
                });
            return Some(st_s(StaticAbilityAst::InheritAbilities {
                subject: self_obj(),
                source_filter: Box::new(src_flt),
                activated_only: false,
                condition: Some(Box::new(cond)),
            }));
        }
        if let Some(after_ft) = rest_low.strip_prefix("~ has the full text of ") {
            let (quoted, _) = extract_quoted_segments(rest_s);
            let mut items = vec![StaticAbilityAst::InheritAbilities {
                subject: self_obj(),
                source_filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                activated_only: false,
                condition: Some(Box::new(cond.clone())),
            }];
            for ab in quoted {
                items.push(StaticAbilityAst::GrantAbilityText {
                    subject: self_obj(),
                    ability_text: ab,
                    compiled: None,
                    condition: Some(Box::new(cond.clone())),
                });
            }
            let _ = after_ft;
            return Some(st_s(StaticAbilityAst::Compound(items)));
        }
        // Prevent combat damage + keyword ("prevent all combat damage it would deal and it has <kws>") (Woolly Razorback)
        if let Some(kw_raw) =
            rest_low.strip_prefix("prevent all combat damage it would deal and it has ")
            && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
        {
            return Some(st_s(StaticAbilityAst::Compound(vec![
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only: true,
                    noncombat_only: false,
                    both_ways: false,
                    subject: self_obj(),
                    by: Some(self_obj()),
                },
                StaticAbilityAst::GrantKeywords {
                    subject: self_obj(),
                    keywords: kws,
                    condition: Some(Box::new(cond)),
                    paired_rule: None,
                },
            ])));
        }
        // Prevent damage from source filter to subjects ("prevent all damage <src> would deal to <dst>") (Camel)
        if let Some(after_pr) = rest_low.strip_prefix("prevent all damage ")
            && let Some((src_raw, dst_raw)) = after_pr.split_once(" would deal to ")
            && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
        {
            let dst = parse_subject_or_target(dst_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(dst_raw.trim().to_string()));
            return Some(st_s(StaticAbilityAst::PreventAllDamageStatic {
                combat_only: false,
                noncombat_only: false,
                both_ways: false,
                subject: Box::new(dst),
                by: Some(Box::new(src)),
            }));
        }
        // Layer type + base P/T + unblockable ("it's a <types> with base power and toughness <P>/<T> and can't be blocked") (Futurist Operative)
        if let Some(after_its) = rest_low.strip_prefix("it's a ")
            && let Some((ty_raw, pt_tail)) = after_its.split_once(" with base power and toughness ")
            && let Some((pt_raw, tail)) = pt_tail.split_once(" and ")
            && let Some((p, t)) = pt_raw.trim().split_once('/')
        {
            return Some(st_s(StaticAbilityAst::Compound(vec![
                StaticAbilityAst::ContinuousLayerEffect {
                    subject: self_obj(),
                    set_types: Some(ty_raw.trim().to_string()),
                    additive_types: false,
                    base_pt: Some((p.trim().to_string(), t.trim().to_string())),
                    set_colors: None,
                    grant_keywords: Vec::new(),
                    lose_keywords: Vec::new(),
                    lose_all_abilities: false,
                    condition: Some(Box::new(cond.clone())),
                },
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(self_obj()),
                    rule: tail.trim().to_string(),
                    filter: None,
                    condition: Some(Box::new(cond)),
                },
            ])));
        }
    }

    // 12. Global color/text/type layer statics (Mycosynth Lattice, Painter's Servant, Swirl the Mists, Illusionary Terrain, Ghostly Flame)
    if let Some(color_desc) =
        low.strip_prefix("all cards that aren't on the battlefield, spells, and permanents are ")
    {
        return Some(st_cont_layer(
            SubjectRefAst::All(Box::new(ObjectFilterAst {
                any_card: true,
                any_spell: true,
                any_permanent: true,
                ..Default::default()
            })),
            None,
            false,
            None,
            Some(color_desc.trim().to_string()),
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    if let Some(after) = low.strip_prefix("all instances of color words in the text of ")
        && let Some((scope_raw, target_raw)) = after.split_once(" are changed to ")
    {
        return Some(st_cont_layer(
            SubjectRefAst::Anaphora(scope_raw.trim().to_string()),
            None,
            false,
            None,
            Some(format!("text_change:{}", target_raw.trim())),
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    if let Some(after) = low.strip_prefix("basic lands of the ")
        && let Some((first_raw, second_raw)) = after.split_once(" are the ")
    {
        return Some(st_set_type(
            SubjectRefAst::Anaphora(format!("basic lands of the {}", first_raw.trim())),
            second_raw.trim().to_string(),
            false,
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" are colorless sources of damage") {
        let subj = parse_subject_or_target(flt_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(flt_raw.trim().to_string()));
        return Some(st_cont_layer(
            subj,
            None,
            false,
            None,
            Some("colorless_damage_source".to_string()),
            Vec::new(),
            Vec::new(),
            false,
        ));
    }

    None
}

fn parse_keyword_and_spell_grant_statics_e8(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. "<flt> has/have the chosen creature type in addition to its other types" (Ashes of the Fallen)
    if let Some(subj_raw) = low
        .strip_suffix(" has the chosen creature type in addition to its other types")
        .or_else(|| {
            low.strip_suffix(" have the chosen creature type in addition to their other types")
        })
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_set_type(subj, "chosen_creature_type", true));
    }

    // 2. "<spell_flt> gain(s) <kw> <cost> as you cast them" (Zinnia, Ashling, the Limitless)
    if let Some(before) = low.strip_suffix(" as you cast them")
        && let Some((subj_raw, kw_cost)) = split_once_any(&before, &[" gain ", " gains "])
    {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        let (kw, cost_rule) = kw_cost
            .trim()
            .split_once(' ')
            .map_or((kw_cost.trim(), ""), |(k, c)| (k, c.trim()));
        return Some(st_s(StaticAbilityAst::GrantKeywordWithCost {
            subject: Box::new(subj),
            keyword: kw.to_string(),
            cost_rule: cost_rule.to_ascii_uppercase(),
            condition: None,
        }));
    }

    // 3. "<card_or_spell_flt> has/have <kw> [<cost>]" for graveyard/cast-filtered grants
    // (Solemn Doomguide, Niv-Mizzet Supreme, Threefold Signal, Rain of Riches)
    if let Some((subj_raw, rhs_raw)) = low.rsplit_once(" has ")
        && (subj_raw.starts_with("each ") || subj_raw.starts_with("the first spell "))
    {
        let rhs = rhs_raw.trim();
        let (kw, cost_opt) = match rhs.split_once(' ') {
            Some((k, c)) if c.starts_with('{') => (k, Some(c.to_ascii_uppercase())),
            _ => (rhs, None),
        };
        if matches!(
            kw,
            "unearth"
                | "jump-start"
                | "replicate"
                | "cascade"
                | "offspring"
                | "evoke"
                | "flashback"
                | "escape"
                | "encore"
        ) {
            let subj = parse_subject_or_target(subj_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
            if let Some(cost_rule) = cost_opt {
                return Some(st_s(StaticAbilityAst::GrantKeywordWithCost {
                    subject: Box::new(subj),
                    keyword: kw.to_string(),
                    cost_rule,
                    condition: None,
                }));
            }
            return Some(st_kw(subj, vec![kw.to_string()]));
        }
    }

    // 4. Dynamic X keyword grants ("~ has <kw> x, where x is <expr>", "devour x, where x is <expr>", "craft with <list> <cost>")
    // (Kodama of the Center Tree, Thromok, Throne of the Grim Captain)
    if let Some((head, expr_raw)) = low.split_once(" x, where x is ") {
        let kw = head.strip_prefix("~ has ").unwrap_or(head).trim();
        if !kw.is_empty() && kw.split_whitespace().count() <= 2 {
            return Some(st_s(StaticAbilityAst::GrantKeywordWithCost {
                subject: self_obj(),
                keyword: kw.to_string(),
                cost_rule: format!("x={}", expr_raw.trim()),
                condition: None,
            }));
        }
    }
    if let Some(rest) = low.strip_prefix("craft with ")
        && let Some((materials, cost_tok)) = rest.rsplit_once(' ')
        && cost_tok.starts_with('{')
    {
        return Some(st_s(StaticAbilityAst::GrantKeywordWithCost {
            subject: self_obj(),
            keyword: "craft".to_string(),
            cost_rule: format!("{}|{}", materials.trim(), cost_tok.to_ascii_uppercase()),
            condition: None,
        }));
    }

    // 5. "For each basic land type among <flt>, ~ has landwalk of that type" (Magnigoth Treefolk)
    if let Some(after) = low.strip_prefix("for each basic land type among ")
        && let Some((flt_raw, rest)) = after.split_once(", ")
        && rest.contains(" has landwalk of that type")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::ScaledBy {
            per: Box::new(ValueExprAst::DistinctCountAmong {
                category: "basic_land_type".to_string(),
                filter: Box::new(flt),
            }),
            inner: Box::new(StaticAbilityAst::GrantKeywords {
                subject: self_obj(),
                keywords: vec!["landwalk_matching_basic_land_type".to_string()],
                condition: None,
                paired_rule: None,
            }),
        }));
    }

    // 6. "If a <flt> with <kw> was exiled with ~'s <ability> ability, ~ has <kw>" (Soulflayer)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_part, body_part)) = after_if.split_once(", ~ has ")
        && let Some((flt_part, _)) = cond_part.split_once(" was exiled with ~'s ")
        && let Some(kws) = parse_keyword_list(body_part.trim(), vocab)
    {
        let mut flt = parse_object_filter(flt_part.trim(), vocab)
            .unwrap_or_else(|_| card_flt(CardTypeMask::CREATURE));
        flt.zone = Some(ZoneFilter::Exile);
        flt.flags = flt.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
        return Some(st_s(StaticAbilityAst::GrantKeywords {
            subject: self_obj(),
            keywords: kws,
            condition: Some(Box::new(cond_zone_count(flt, CompareOp::Ge, 1))),
            paired_rule: None,
        }));
    }

    None
}

fn parse_cost_and_permission_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. Additional activation cost:
    // "Activated abilities [of <flt>] cost an additional <cost> to activate [for each <per>]"
    // "<kind> abilities you activate cost an additional <+N/-N> to activate"
    // (Drought, Brutal Suppression, Carth the Lion)
    if let Some((head, rest)) = low.split_once(" cost an additional ")
        && (rest.contains(" to activate") || rest.contains(" to cast"))
    {
        let is_activate = rest.contains(" to activate");
        let split_marker = if is_activate {
            " to activate"
        } else {
            " to cast"
        };
        if let Some((cost_part, tail)) = rest.split_once(split_marker) {
            let cost_clean = cost_part.trim().trim_matches('"');
            let per = tail
                .trim()
                .strip_prefix("for each ")
                .map(|p| p.trim().to_string());
            let parsed_cost = if let Some(after_plus) = cost_clean.strip_prefix('+')
                && let Ok(n) = after_plus.parse::<i32>()
            {
                Some(CostAtomAst::PutCounters {
                    count: vconst(n),
                    counter_kind: "loyalty".to_string(),
                    subject: box_anaphora("that planeswalker"),
                })
            } else if let Some(life_raw) = cost_clean.strip_suffix(" life")
                && let Ok(amt) = parse_value_expr(life_raw.trim(), vocab)
            {
                Some(CostAtomAst::PayLife(Box::new(amt)))
            } else {
                parse_cost_atom(cost_clean, vocab).ok()
            };
            if let Some(cost_atom) = parsed_cost {
                if is_activate {
                    let subj = head
                        .strip_prefix("activated abilities of ")
                        .and_then(|f| parse_object_filter(f.trim(), vocab).ok())
                        .map(|f| Box::new(SubjectRefAst::All(Box::new(f))))
                        .or_else(|| {
                            if head != "activated abilities" {
                                Some(Box::new(SubjectRefAst::Anaphora(head.trim().to_string())))
                            } else {
                                None
                            }
                        });
                    return Some(st_s(StaticAbilityAst::AdditionalActivationCost {
                        subject: subj,
                        cost: cost_atom,
                        per,
                        condition: None,
                    }));
                }
                // Additional cast cost (Terror of the Peaks, Drought spells)
                let subj = parse_subject_or_target(head.trim(), vocab)
                    .unwrap_or_else(|_| SubjectRefAst::Anaphora(head.trim().to_string()));
                return Some(st_s(StaticAbilityAst::AdditionalActivationCost {
                    subject: Some(Box::new(subj)),
                    cost: cost_atom,
                    per: per.or_else(|| Some("cast".to_string())),
                    condition: None,
                }));
            }
        }
    }

    // 2. "As an additional cost to cast <subj>, <cost_or_action>" (Tectonic Split, Chorus of the Conclave)
    if let Some(after) = low.strip_prefix("as an additional cost to cast ")
        && let Some((subj_raw, rest)) = after.split_once(", ")
    {
        let r = rest.trim();
        if let Some(flt_raw) = r
            .strip_prefix("sacrifice half ")
            .and_then(|x| strip_suffix_any(x, &[", rounded up", ", rounded down"]))
            && let Ok(flt) = parse_object_filter(
                flt_raw
                    .trim()
                    .strip_prefix("the ")
                    .unwrap_or(flt_raw.trim()),
                vocab,
            )
        {
            let rounding = if r.ends_with("rounded up") {
                Rounding::Up
            } else {
                Rounding::Down
            };
            return Some(st_s(StaticAbilityAst::AdditionalCastCost {
                cost: CostAtomAst::Sacrifice {
                    count: Box::new(val_div(
                        ValueExprAst::Count(Box::new(flt.clone())),
                        2,
                        rounding,
                    )),
                    filter: Box::new(flt),
                    up_to: false,
                    min_one: false,
                },
            }));
        }
        if r == "you may pay any amount of mana" {
            let _ = subj_raw;
            return Some(st_s(StaticAbilityAst::AdditionalCastCost {
                cost: CostAtomAst::PayMana("{X}".to_string()),
            }));
        }
    }

    // 3. Mana substitution ("For each {<M>} in a cost, you may pay <cost> rather than pay that mana") (K'rrik)
    if let Some(after) = low.strip_prefix("for each ")
        && let Some((sym_raw, rest)) = after.split_once(" in a cost, you may ")
        && let Some(cost_raw) = rest.strip_suffix(" rather than pay that mana")
        && let Ok(cost_atom) = parse_cost_atom(cost_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::AlternativeAbilityCost {
            cost: vec![cost_atom],
            ability_kind: "mana_substitution".to_string(),
            scope: sym_raw.trim().to_ascii_uppercase(),
            condition: None,
        }));
    }

    // 4. Alternative cast costs ("Rather than pay ...", "If <cond>, rather than pay ...", "Once during each of your turns, you may cast <flt> by paying ... rather than ...", "If <cond>, you may cast ~ without paying its mana cost and as though it had flash")
    // (Dream Halls, Liesa, Skyshroud Cutter, Demon of Fate's Design, Qasali Ambusher)
    if let Some(after) =
        low.strip_prefix("rather than pay the mana cost for a spell, its controller may ")
        && let Ok(cost_atom) = parse_cost_atom(after.trim(), vocab)
    {
        return Some(st_alt_cast(vec![cost_atom], Some(any_spell_flt())));
    }
    if low.starts_with("rather than pay ")
        && let Some((paid_part, alt_part)) = low.split_once(", ")
        && paid_part.contains("command zone")
    {
        let life_cost = alt_part
            .strip_prefix("pay ")
            .and_then(|r| r.split_once(" life"))
            .and_then(|(n, _)| parse_number_word(n.trim()))
            .map_or_else(
                || CostAtomAst::PayLife(vconst(2)),
                |n| {
                    CostAtomAst::PayLife(Box::new(ValueExprAst::Mul(
                        vconst(n),
                        Box::new(ValueExprAst::TimesCastFromCommandZone(Box::new(
                            SubjectRefAst::SelfObject,
                        ))),
                    )))
                },
            );
        return Some(st_alt_cast(vec![life_cost], None));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", rather than pay ~'s mana cost, ")
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let eff_clean = rest.strip_prefix("you may ").unwrap_or(rest);
        let cost = parse_cost_atom(eff_clean, vocab)
            .map(|c| vec![c])
            .unwrap_or_default();
        return Some(st_s(StaticAbilityAst::AlternativeCastCost {
            cost,
            condition: Some(Box::new(cond)),
            for_spells: None,
            next_only: false,
        }));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) =
            after_if.split_once(", you may cast ~ without paying its mana cost")
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let alt = StaticAbilityAst::AlternativeCastCost {
            cost: Vec::new(),
            condition: Some(Box::new(cond.clone())),
            for_spells: None,
            next_only: false,
        };
        if rest.contains("as though it had flash") {
            return Some(st_s(StaticAbilityAst::Compound(vec![
                alt,
                StaticAbilityAst::GrantKeywords {
                    subject: self_obj(),
                    keywords: vec!["flash".to_string()],
                    condition: Some(Box::new(cond)),
                    paired_rule: None,
                },
            ])));
        }
        return Some(st_s(alt));
    }
    if let Some(after) = low.strip_prefix("once during each of your turns, you may cast ")
        && let Some((flt_raw, rest)) = after
            .split_once(" by paying life equal to its mana value rather than paying its mana cost")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let _ = rest;
        return Some(st_s(StaticAbilityAst::OncePerTurn(Box::new(
            StaticAbilityAst::AlternativeCastCost {
                cost: vec![CostAtomAst::PayLife(Box::new(ValueExprAst::ObjectStat {
                    subject: box_anaphora("it"),
                    stat: StatKind::ManaValue,
                }))],
                condition: Some(Box::new(ConditionAst::IsYourTurn)),
                for_spells: Some(Box::new(flt)),
                next_only: false,
            },
        ))));
    }

    // 5. Cast from graveyard with scaling tax ("You may cast ~ from <zone> if you pay <delta> more to cast it for each <flt>") (Risen Executioner)
    if let Some(after) = low.strip_prefix("you may cast ~ from ")
        && let Some((zone_raw, rest)) = after.split_once(" if you pay ")
        && let Some((delta_raw, fe_raw)) = rest.split_once(" more to cast it for each ")
        && let Ok(per_flt) = parse_object_filter(fe_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::Compound(vec![
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: true,
                filter: Box::new(ObjectFilterAst {
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                permission: format!("cast_self_from_{}", zone_raw.trim()),
                condition: None,
            },
            StaticAbilityAst::CostModifier {
                subject: self_obj(),
                delta: delta_raw.trim().to_ascii_uppercase(),
                less: false,
                next_only: false,
                per: Some(Box::new(per_flt)),
                scale_expr: None,
                condition: None,
            },
        ])));
    }

    // 6. Foretell cost/timing modifiers (Ranar, Cosmos Charger)
    if let Some(after) = low.strip_prefix("the first card you ")
        && let Some((action_raw, rest)) = after.split_once(" each turn costs ")
        && let Some((cost_tok, _)) = rest.split_once(" to ")
    {
        return Some(st_s(StaticAbilityAst::AlternativeAbilityCost {
            cost: vec![CostAtomAst::PayMana(cost_tok.trim().to_ascii_uppercase())],
            ability_kind: action_raw.trim().to_string(),
            scope: "first_each_turn".to_string(),
            condition: None,
        }));
    }
    if let Some(after) = low.strip_prefix("foretelling cards from your hand costs ")
        && let Some((delta_raw, tail)) = after.split_once(" less")
    {
        let mut items = vec![StaticAbilityAst::CostModifier {
            subject: box_anaphora("foretell"),
            delta: delta_raw.trim().to_ascii_uppercase(),
            less: true,
            next_only: false,
            per: None,
            scale_expr: None,
            condition: None,
        }];
        if tail.contains("on any player's turn") {
            items.push(StaticAbilityAst::ActivationLimitOverride {
                ability_kind: "foretell".to_string(),
                subject: box_anaphora("cards in your hand"),
                extra_times: 0,
                instant_speed: true,
                condition: None,
            });
        }
        return Some(st_s(StaticAbilityAst::Compound(items)));
    }

    // 7. Mana spend restriction ("Spend only <kind> mana on X") (Emblazoned Golem)
    if let Some(kind_raw) = low
        .strip_prefix("spend only ")
        .and_then(|r| r.strip_suffix(" mana on x"))
    {
        return Some(st_s(StaticAbilityAst::ManaSpendRule {
            mana_kind: Some(kind_raw.trim().to_string()),
            as_though: "spend_only_on_x".to_string(),
            purpose: Some("x".to_string()),
            condition: None,
        }));
    }

    // 8. Play/cast/plot permissions (Shared Fate, Evelyn, Fblthp Lost on the Range)
    if let Some(after) = low.strip_prefix("you may plot ")
        && let Some(flt_raw) = after.strip_suffix(" from the top of your library")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.zone = Some(ZoneFilter::YourLibrary);
        return Some(st_perm(true, flt, "plot_from_library_top"));
    }
    if low.starts_with("each player may look at cards they exiled with ~")
        && low.contains("play lands and cast spells from among those cards")
    {
        let flt = ObjectFilterAst {
            any_card: true,
            zone: Some(ZoneFilter::Exile),
            flags: ObjectFilterFlags::EXILED_WITH_SELF,
            ..Default::default()
        };
        return Some(st_s(StaticAbilityAst::Compound(vec![
            StaticAbilityAst::LookOrRevealRule {
                rule: "look_at_exiled_with_self".to_string(),
                filter: Some(Box::new(flt.clone())),
                follow_up: Vec::new(),
                condition: None,
            },
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: false,
                filter: Box::new(flt),
                permission: "each_player_play_exiled_with_self".to_string(),
                condition: None,
            },
        ])));
    }
    if let Some(after) = low.strip_prefix("once each turn, you may play a card from exile with a ")
        && let Some((ck_raw, rest)) = after.split_once(" counter on it")
    {
        let flt = ObjectFilterAst {
            any_card: true,
            zone: Some(ZoneFilter::Exile),
            counter_kind: Some(ck_raw.trim().to_string()),
            ..Default::default()
        };
        let mut items = vec![StaticAbilityAst::PlayOrCastPermission {
            cast_only: false,
            filter: Box::new(flt),
            permission: "play_exiled_with_counter".to_string(),
            condition: None,
        }];
        if rest.contains("spend mana as though it were mana of any color") {
            items.push(StaticAbilityAst::ManaSpendRule {
                mana_kind: None,
                as_though: "any_color".to_string(),
                purpose: Some("cast_that_card".to_string()),
                condition: None,
            });
        }
        return Some(st_s(StaticAbilityAst::OncePerTurn(Box::new(
            StaticAbilityAst::Compound(items),
        ))));
    }

    // 9. Pregame mulligan special action ("Any time you could mulligan and ~ is in your hand, <eff>") (Serum Powder)
    if let Some(after) = low.strip_prefix("any time you could mulligan and ~ is in your hand, ") {
        let rest_s = &s[s.len() - after.len()..];
        let (body, fails) = parse_effect_body(rest_s, vocab);
        if fails.is_empty() && !body.is_empty() {
            return Some(st_s(StaticAbilityAst::FollowUp(vec![
                InstructionAst::TemporarySpecialAction {
                    timing: "mulligan".to_string(),
                    duration: DurationAst::Permanent,
                    body,
                },
            ])));
        }
    }

    None
}

fn parse_replacement_and_enters_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. "As ~ enters, <eff>" (Alhammarret, Neverwinter Hydra, Grifter's Blade, Deadpool)
    if let Some(after) = low.strip_prefix("as ~ enters, ") {
        let rest_s = &s[s.len() - after.len()..];
        if let Some(after_ex) = after.strip_prefix("you may exchange ")
            && (after_ex.contains("text box") || after_ex.contains("text boxes"))
        {
            return Some(st_r(ReplacementEffectAst::AsEntersAction(vec![ins_opt1(
                InstructionAst::ExchangeTextBoxes {
                    left: self_obj(),
                    right: box_anaphora("another creature"),
                },
            )])));
        }
        if let Some(flt_raw) = after
            .strip_prefix("choose ")
            .and_then(|r| r.strip_suffix(" it could be attached to"))
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(st_r(ReplacementEffectAst::AsEntersAction(vec![
                ins_choose_group(ValueExprAst::Const(1), false, grp(flt)),
            ])));
        }
        let (body, fails) = parse_effect_body(rest_s, vocab);
        if fails.is_empty() && !body.is_empty() {
            return Some(st_r(ReplacementEffectAst::AsEntersAction(body)));
        }
    }

    // 2. "As <flt> enters, it becomes a <P>/<T> <subtypes> in addition to its other types" (Displaced Dinosaurs)
    if let Some(after) = low.strip_prefix("as ")
        && let Some((flt_raw, rest)) = after.split_once(" enters, ")
        && let Ok(_flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_instead(
            flt_raw.trim().to_string(),
            "enters",
            rest.trim().to_string(),
            parse_atomic_effect(rest.trim(), vocab).ok(),
        ));
    }

    if let Some(after) = low.strip_prefix("if you do, ") {
        if let Some(subj_raw) = after.strip_prefix("it enters attached to ") {
            return Some(st_instead(
                "~",
                "enter",
                format!("enters_attached_to:{}", subj_raw.trim()),
                None,
            ));
        }
        if let Some((subj_raw, rest)) = after.split_once(" enters with that many additional ")
            && let Some(ck_raw) = rest.strip_suffix(" counters on it")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(st_r(ReplacementEffectAst::OthersEnterWithCounters {
                subject: Box::new(subj),
                counter_kind: ck_raw.trim().to_string(),
                count: Box::new(ValueExprAst::PriorResult),
                additional: true,
                per: None,
            }));
        }
    }
    if let Some(after) = low.strip_prefix("you choose the name of ")
        && let Some(flt_raw) = after.strip_suffix(" revealed this way")
    {
        return Some(st_s(StaticAbilityAst::FollowUp(vec![
            InstructionAst::ChooseOption(format!("card_name_revealed:{}", flt_raw.trim())),
        ])));
    }

    // 4. "For each kind of counter on <subj>, <other_subj> enters with a counter of that kind on it" (Blue, Loyal Raptor)
    if let Some(after) = low.strip_prefix("for each kind of counter on ")
        && let Some((src_raw, rest)) = after.split_once(", ")
        && let Some(other_raw) = rest.strip_suffix(" enters with a counter of that kind on it")
        && let Ok(other_subj) = parse_subject_or_target(other_raw.trim(), vocab)
    {
        let src_flt = parse_object_filter(src_raw.trim(), vocab).unwrap_or_else(|_| any_perm());
        return Some(st_r(ReplacementEffectAst::ScaledBy {
            per: Box::new(ValueExprAst::DistinctCountAmong {
                category: "counter_kind".to_string(),
                filter: Box::new(src_flt),
            }),
            inner: Box::new(ReplacementEffectAst::OthersEnterWithCounters {
                subject: Box::new(other_subj),
                counter_kind: "that kind".to_string(),
                count: vconst(1),
                additional: false,
                per: None,
            }),
        }));
    }

    // 5. "~ escapes with \"<ability>\"" (Skyway Robber)
    if low.starts_with("~ escapes with \"") && s.contains('"') {
        let (quoted, _) = extract_quoted_segments(s);
        if let Some(ab) = quoted.into_iter().next() {
            return Some(st_r(ReplacementEffectAst::AsEntersAction(vec![
                InstructionAst::GrantAbilityText {
                    subject: self_obj(),
                    ability_text: ab,
                    compiled: None,
                    duration: DurationAst::Permanent,
                },
            ])));
        }
    }

    // 6. Die-roll modifier statics ("After you roll a die, you may <cost>", "increase or decrease the result by <N>", "Once each turn, you may <cost> to reroll one or more dice you rolled")
    // (Night Shift of the Living Dead, Xenosquirrels, Monitor Monitor)
    if let Some(cost_raw) = low.strip_prefix("after you roll a die, you may ")
        && let Ok(costs) = parse_cost_list(cost_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::DieRollModifier {
            cost: costs,
            delta: None,
            reroll: false,
            once_each_turn: false,
        }));
    }
    if let Some(num_raw) = low
        .strip_prefix("if you do, increase or decrease the result by ")
        .or_else(|| low.strip_prefix("increase or decrease the result by "))
        && let Some(n) = parse_number_word(num_raw.trim())
        && let Ok(delta) = i8::try_from(n)
    {
        return Some(st_s(StaticAbilityAst::DieRollModifier {
            cost: Vec::new(),
            delta: Some(delta),
            reroll: false,
            once_each_turn: false,
        }));
    }
    if let Some(after) = low.strip_prefix("once each turn, you may ")
        && let Some((cost_raw, _)) = after.split_once(" to reroll one or more dice you rolled")
        && let Ok(costs) = parse_cost_list(cost_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::DieRollModifier {
            cost: costs,
            delta: None,
            reroll: true,
            once_each_turn: true,
        }));
    }

    // 7. Damage-counter replacement ("For each 1 damage that would be dealt to <subj>, if it has a <ck> counter on it, remove a <ck> counter from it and prevent that 1 damage") (Rock Hydra)
    if let Some(after) = low.strip_prefix("for each 1 damage that would be dealt to ")
        && let Some((subj_raw, rest)) = after.split_once(", if it has a ")
        && let Some((ck_raw, _)) = rest.split_once(" counter on it, remove ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::PreventAndReplaceDamage {
            target: Box::new(subj.clone()),
            follow_up: vec![ins_rm_counters(
                subj,
                ck_raw.trim().to_string(),
                false,
                Some(ValueExprAst::Const(1)),
            )],
            condition: Some(Box::new(ConditionAst::CounterCompare {
                subject: self_obj(),
                counter_kind: ck_raw.trim().to_string(),
                op: CompareOp::Ge,
                value: 1,
            })),
        }));
    }

    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((src_raw, rest)) = after_if.split_once(" causes you to discard a card, ")
        && rest.contains(" instead of ")
    {
        return Some(st_instead(
            src_raw.trim().to_string(),
            "discard_card",
            rest.trim().to_string(),
            None,
        ));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((who_raw, _)) = after_if
            .split_once(" would face a villainous choice, they face that choice an additional time")
    {
        return Some(st_instead(
            who_raw.trim().to_string(),
            "villainous_choice",
            "face_additional_time",
            None,
        ));
    }
    if low.starts_with("the first time you flip one or more coins each turn, ") {
        return Some(st_instead(
            "you",
            "first_coin_flip_each_turn",
            low.to_string(),
            None,
        ));
    }
    if low == "you draw cards from the bottom of your library rather than the top" {
        return Some(st_instead("you", "draw_card", "draw_from_bottom", None));
    }
    if let Some(after) = low.strip_prefix("you may look at an additional ")
        && let Some((num_raw, action_raw)) = after.split_once(" cards each time you ")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(st_instead(
            "you",
            action_raw.trim().to_string(),
            format!("look_additional:{n}"),
            None,
        ));
    }
    if let Some(after) = low.strip_prefix("as you cascade, ") {
        let rest_s = &s[s.len() - after.len()..];
        return Some(st_instead(
            "you",
            "cascade",
            after.trim().to_string(),
            parse_atomic_effect(rest_s, vocab).ok(),
        ));
    }
    if let Some(after) = low.strip_prefix("while ")
        && let Some((who_raw, rest)) = after.split_once(" is searching their library, ")
    {
        return Some(st_instead(
            who_raw.trim().to_string(),
            "search_library",
            rest.trim().to_string(),
            parse_atomic_effect(rest.trim(), vocab).ok(),
        ));
    }
    if let Some(after) = low.strip_prefix("you control ")
        && let Some(who_raw) = after.strip_suffix(" while they're searching their libraries")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(st_s(StaticAbilityAst::ControlChange {
            controller: "you_while_searching".to_string(),
            subject: Box::new(SubjectRefAst::Player(player)),
            condition: None,
        }));
    }

    None
}

fn parse_combat_activation_and_rule_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. Die-roll loyalty band rows ("+N, and <eff>" / "-N, then <eff>") (Comet, Stellar Pup)
    let signed_prefix = low
        .strip_prefix('+')
        .map(|r| (true, r))
        .or_else(|| low.strip_prefix('-').map(|r| (false, r)))
        .or_else(|| low.strip_prefix('\u{2212}').map(|r| (false, r)));
    if let Some((positive, after_sign)) = signed_prefix
        && let Some((num_tok, rest_low)) = split_once_any(&after_sign, &[", and ", ", then "])
        && let Ok(n) = num_tok.trim().parse::<i32>()
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        let (mut body, fails) = parse_effect_body(rest_s, vocab);
        if fails.is_empty() && !body.is_empty() {
            let signed_n = if positive { n } else { -n };
            let loyalty_ins = if signed_n >= 0 {
                ins_counters(
                    SubjectRefAst::SelfObject,
                    "loyalty",
                    ValueExprAst::Const(signed_n),
                )
            } else {
                InstructionAst::RemoveCounters {
                    subject: self_obj(),
                    counter_kind: "loyalty".to_string(),
                    all: false,
                    count: Some(vconst(-signed_n)),
                }
            };
            let mut out = vec![loyalty_ins];
            out.append(&mut body);
            return Some(st_s(StaticAbilityAst::FollowUp(out)));
        }
    }

    // 2. Activation frequency & instant-speed timing overrides (Oath of Teferi, Teferi Master of Time, Birgi, Wonder Man)
    if let Some(after) = low.strip_prefix("you may activate ")
        && let Some((kind_and_subj, tail)) = after
            .split_once(" twice each turn rather than only once")
            .map(|(l, _)| (l, false))
            .or_else(|| {
                after
                    .split_once(" on any player's turn any time you could cast an instant")
                    .map(|(l, _)| (l, true))
            })
        && let Some((kind_raw, subj_raw)) = kind_and_subj.split_once(" abilities of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let ability_kind = kind_raw
            .trim()
            .strip_prefix("the ")
            .unwrap_or(kind_raw.trim());
        return Some(st_s(StaticAbilityAst::ActivationLimitOverride {
            ability_kind: ability_kind.to_string(),
            subject: Box::new(subj),
            extra_times: if tail { 0 } else { 1 },
            instant_speed: tail,
            condition: None,
        }));
    }
    if let Some((subj_raw, rest)) = low.split_once(" can ")
        && let Some((action_raw, _)) =
            rest.split_once(" twice during each of your turns rather than once")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::ActivationLimitOverride {
            ability_kind: action_raw.trim().to_string(),
            subject: Box::new(subj),
            extra_times: 1,
            instant_speed: false,
            condition: None,
        }));
    }
    if let Some(after) = low.strip_prefix("each ")
        && let Some((kind_raw, rest)) = after.split_once(" ability of ")
        && let Some(subj_raw) = rest.strip_suffix(" can be activated an additional time")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::ActivationLimitOverride {
            ability_kind: kind_raw.trim().to_string(),
            subject: Box::new(subj),
            extra_times: 1,
            instant_speed: false,
            condition: None,
        }));
    }

    // 3. Combat damage assignment & lethal-damage rules (Arcades, Zilortha, Defensive Formation, Butcher Orgg, Cunning Giant, Loot)
    if let Some((subj_raw, rest)) =
        low.split_once(" assigns combat damage equal to its toughness rather than its power")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let mut items = vec![StaticAbilityAst::CombatDamageAssignmentRule {
            subject: Box::new(subj.clone()),
            rule: "toughness_instead_of_power".to_string(),
            condition: None,
        }];
        if rest.contains("can attack as though it didn't have defender") {
            items.push(st_rule_ast(
                Some(subj),
                "can_attack_as_though_no_defender",
                None,
            ));
        }
        return Some(st_s(StaticAbilityAst::Compound(items)));
    }
    if let Some(after) = low.strip_prefix("lethal damage dealt to ")
        && let Some(subj_raw) =
            after.strip_suffix(" is determined by their power rather than their toughness")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::CombatDamageAssignmentRule {
            subject: Box::new(subj),
            rule: "lethal_damage_by_power".to_string(),
            condition: None,
        }));
    }
    if let Some(subj_raw) =
        low.strip_prefix("rather than the attacking player, you assign the combat damage of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::CombatDamageAssignmentRule {
            subject: Box::new(subj),
            rule: "defender_assigns_combat_damage".to_string(),
            condition: None,
        }));
    }
    if low.starts_with("you can divide that creature's combat damage as you choose among ") {
        return Some(st_s(StaticAbilityAst::CombatDamageAssignmentRule {
            subject: box_anaphora("that creature"),
            rule: "divide_combat_damage_among_blockers".to_string(),
            condition: None,
        }));
    }
    if let Some(after) = low.strip_prefix("you may assign ")
        && let Some((subj_raw, _)) =
            after.split_once("'s combat damage divided as you choose among ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::CombatDamageAssignmentRule {
            subject: Box::new(subj),
            rule: "divide_combat_damage_among_defender_and_creatures".to_string(),
            condition: None,
        }));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_raw, _)) = after.split_once(" is unblocked, you may have it assign its combat damage to a creature defending player controls")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(subj),
                rule: "unblocked_may_assign_to_defending_creature".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_raw, rest)) = after.split_once("'s power is negative, ")
        && rest.contains("assigns combat damage as though ")
        && rest.ends_with("'s power were positive")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_s(StaticAbilityAst::CombatDamageAssignmentRule {
            subject: Box::new(subj),
            rule: "negative_power_assigns_as_positive".to_string(),
            condition: None,
        }));
    }

    // 4. Combat blocking/attacking restrictions (Masako, Caverns of Despair, Invasion Plans, Akron Legionnaire, Intercessor's Arrest, Cogwork Tracker)
    if let Some(flt_raw) = low
        .strip_prefix("tapped ")
        .and_then(|r| r.strip_suffix(" can block as though they were untapped"))
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(st_rule_subj(
            SubjectRefAst::All(Box::new(flt)),
            "tapped_can_block_as_untapped",
        ));
    }
    if let Some(num_raw) = low
        .strip_prefix("no more than ")
        .and_then(|r| r.strip_suffix(" creatures can block each combat"))
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(st_rule(format!("max_blockers_each_combat:{n}")));
    }
    if low == "the attacking player chooses how each creature blocks each combat" {
        return Some(st_rule("attacking_player_chooses_blocks"));
    }
    if let Some(after) = low.strip_prefix("except for ")
        && let Some((exc_raw, rest)) = after.split_once(", ")
        && let Some(subj_raw) = rest.strip_suffix(" can't attack")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let exc_flt = parse_object_filter(exc_raw.trim(), vocab).ok();
        return Some(st_rule_full(
            Some(subj),
            format!("cant_attack_except:{}", exc_raw.trim()),
            exc_flt,
            None,
        ));
    }
    if let Some((subj_raw, rest)) = low.split_once(" can't attack, block, or ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_rule_subj(
            subj,
            format!("cant_attack_block_or:{}", rest.trim()),
        ));
    }
    if low == "~ attacks a player you noted for cards named ~ each combat if able" {
        return Some(st_rule_subj(
            SubjectRefAst::SelfObject,
            "attacks_noted_player_if_able",
        ));
    }

    // 5. Remaining game-rule statics (Hama Pashar, Sanctum Lurker, Yurlok, Altar of the Pantheon, City of Solitude, Null Chamber, City in a Bottle, Bell Borca, Cryptic Spires, Firestorm Phoenix follow-up)
    if let Some(after) = low.strip_prefix("room abilities of ")
        && after.ends_with(" trigger an additional time")
    {
        return Some(st_s(StaticAbilityAst::TriggerMultiplier {
            event_desc: low.to_string(),
            multiplier: 2,
            condition: None,
        }));
    }
    if let Some(subj_raw) =
        low.strip_suffix(" aren't put into their owners' graveyards for having 0 loyalty")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(st_rule_subj(subj, "survives_zero_loyalty"));
    }
    if low == "a player losing unspent mana causes that player to lose that much life" {
        return Some(st_rule("unspent_mana_causes_life_loss"));
    }
    if let Some(num_raw) = low
        .strip_prefix("your devotion to each color and each combination of colors is increased by ")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(st_rule(format!("devotion_bonus:{n}")));
    }
    if low == "players can cast spells and activate abilities only during their own turns" {
        return Some(st_rule("cast_and_activate_own_turn_only"));
    }
    if low
        == "spells with the chosen names can't be cast and lands with the chosen names can't be played"
    {
        return Some(st_s(StaticAbilityAst::Compound(vec![
            st_rule_ast(None, "spells_with_chosen_names_cant_be_cast", None),
            st_rule_ast(None, "lands_with_chosen_names_cant_be_played", None),
        ])));
    }
    if let Some(exp_raw) = low
        .strip_prefix(
            "players can't cast spells or play lands with a name originally printed in the ",
        )
        .and_then(|r| r.strip_suffix(" expansion"))
    {
        let exp_name = exp_raw
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
        return Some(st_rule_flt(
            ObjectFilterAst {
                any_card: true,
                predicates: vec![FilterPredicateAst::FirstPrintedInExpansion(exp_name)],
                ..ObjectFilterAst::default()
            },
            format!("cant_cast_or_play_expansion:{}", exp_raw.trim()),
        ));
    }
    if let Some(after) = low.strip_prefix("note the ")
        && after.contains(" as it's put into exile")
    {
        return Some(st_s(StaticAbilityAst::LookOrRevealRule {
            rule: low.to_string(),
            filter: None,
            follow_up: Vec::new(),
            condition: None,
        }));
    }
    if low.starts_with("as you create your deck, ") {
        return Some(st_s(StaticAbilityAst::DeckConstructionRule(s.to_string())));
    }
    if let Some(after) = low.strip_prefix("until that player's next turn, ")
        && after.contains("plays with that card revealed in their hand and can't play it")
    {
        return Some(st_s(StaticAbilityAst::Compound(vec![
            StaticAbilityAst::LookOrRevealRule {
                rule: "play_with_that_card_revealed_until_next_turn".to_string(),
                filter: None,
                follow_up: Vec::new(),
                condition: None,
            },
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(box_anaphora("that card")),
                rule: "cant_play_until_next_turn".to_string(),
                filter: None,
                condition: None,
            },
        ])));
    }

    None
}
