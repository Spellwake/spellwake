//! Static ability (`StaticAbilityAst`) and replacement effect (`ReplacementEffectAst`) parser.

mod rules;

use crate::ast::{
    ConditionAst, ControllerRelation, CostAtomAst, DurationAst, EntersAsCopyModifiedAst,
    FilterPredicateAst, GameMarker, InstructionAst, ObjectFilterAst, PlayerRefAst,
    ReplacementEffectAst, StaticAbilityAst, SubjectRefAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, parse_cost_list, peel_leading_count};
use crate::clauses::effects::{
    attach_where_x, extract_leading_subject_prefix, find_top_level_pattern, parse_atomic_effect,
    parse_effect_body, parse_instead_body, substitute_chosen_x,
};
use crate::combinators::{
    DepthGuard, anaphora, any_card_flt, any_perm, box_anaphora, extract_quoted_spans, grp, ins_if,
    ins_temp_rep, self_obj, split_once_any, strip_prefix_any, strip_suffix_any, vconst,
};
use crate::compile_granted_ability;
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_value_expr,
};
use crate::preprocess::{
    is_mana_sequence, is_physical_tabletop_clause, parse_base_pt, parse_pt_mod, split_top_level,
};
use crate::vocab::{Vocabulary, parse_number_word};

/// Either a compiled static ability or a compiled replacement effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticOrReplacement {
    Static(StaticAbilityAst),
    Replacement(ReplacementEffectAst),
}

/// Resolve independent conditional statements before attaching any leading or trailing
/// gate. Otherwise a suffix on the final statement can leak into earlier statements.
fn parse_independent_conditional_statics(s: &str, vocab: &Vocabulary) -> Option<StaticAbilityAst> {
    let sentences = split_top_level(s, ". ");
    if sentences.len() > 1
        && sentences.iter().any(|sentence| {
            let low = sentence.to_ascii_lowercase();
            low.contains("as long as ") || low.starts_with("if ")
        })
    {
        let mut items = Vec::new();
        for sentence in sentences {
            let StaticOrReplacement::Static(item) =
                parse_static_or_replacement(sentence.trim(), vocab).ok()?
            else {
                return None;
            };
            items.push(item);
        }
        return Some(StaticAbilityAst::Compound(items));
    }

    // Shared-subject lists such as "~ gets ... as long as A, has ... as long as B".
    // Every continuation must introduce its own effect verb and its own gate.
    let clauses = split_top_level(s, ", ");
    if clauses.len() < 2 || !clauses.iter().all(|part| part.contains(" as long as ")) {
        return None;
    }
    let first = clauses[0];
    let subject_end = [" gets ", " get ", " has ", " have "]
        .iter()
        .filter_map(|verb| first.find(verb))
        .min()?;
    let subject = &first[..subject_end];
    parse_subject_or_target(subject, vocab).ok()?;
    let mut items = Vec::new();
    for (index, clause) in clauses.iter().enumerate() {
        let clause = clause.trim().strip_prefix("and ").unwrap_or(clause.trim());
        let statement = if index == 0 {
            clause.to_string()
        } else {
            if !["gets ", "get ", "has ", "have "]
                .iter()
                .any(|verb| clause.starts_with(verb))
            {
                return None;
            }
            format!("{subject} {clause}")
        };
        let StaticOrReplacement::Static(item) =
            parse_static_or_replacement(&statement, vocab).ok()?
        else {
            return None;
        };
        items.push(item);
    }
    Some(StaticAbilityAst::Compound(items))
}

/// Strictly parse a permanent static/replacement paragraph or spell static/cost modifier.
pub fn parse_static_or_replacement(
    para: &str,
    vocab: &Vocabulary,
) -> Result<StaticOrReplacement, String> {
    let Some(_depth_guard) = DepthGuard::enter() else {
        return Err("max_parse_depth".to_string());
    };
    let s = para.trim().trim_end_matches('.').trim();
    let low = s.to_ascii_lowercase();

    if let Some(compound) = parse_independent_conditional_statics(s, vocab) {
        return Ok(st_s(compound));
    }

    // 0. Global trailing ", where X is <ValueExpr>" in static/keyword-X clauses (quote-aware)
    if let Some(idx) = find_top_level_pattern(s, ", where x is ")
        && let Ok(x_val) = parse_value_expr(&s[idx + ", where x is ".len()..], vocab)
    {
        let head = s[..idx].trim();
        if let Ok(mut sr) = parse_static_or_replacement(head, vocab)
            && attach_where_x(&mut sr, x_val)
        {
            return Ok(sr);
        }
        let hlow = head.to_ascii_lowercase();
        if let Some(kw_name) = hlow.strip_suffix(" x")
            && crate::keywords::parse_keyword_token(head, vocab).is_some()
        {
            return Ok(st_s(StaticAbilityAst::GrantKeywordWithCost {
                subject: self_obj(),
                keyword: kw_name.trim().to_string(),
                cost_rule: s[idx + 2..].trim().to_string(),
                condition: None,
            }));
        }
    }

    // 0b. "You can't cast ~ if/unless <cond>" (Rock Jockey, etc.)
    if let Some(after_cc) =
        strip_prefix_any(&low, &["you can't cast ~ ", "you can't cast this spell "])
        && let Some(cond_raw) = strip_prefix_any(&after_cc, &["if ", "unless ", "as long as "])
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(st_s(StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(self_obj()),
            rule: "cant_cast_self".to_string(),
            filter: None,
            condition: Some(Box::new(cond)),
        }));
    }

    // 0c. "You may cast ~ from your graveyard if/as long as <cond>" (Undead Sprinter, Eelectrocute, Risen Executioner)
    if let Some(after_gy) = low
        .strip_prefix("you may cast ~ from your graveyard ")
        .or_else(|| low.strip_prefix("you may cast this card from your graveyard "))
        && let Some(cond_raw) = strip_prefix_any(&after_gy, &["if ", "as long as "])
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(st_s(StaticAbilityAst::PlayOrCastPermission {
            cast_only: true,
            filter: Box::new(crate::ast::ObjectFilterAst::default()),
            permission: "cast_self_from_graveyard".to_string(),
            condition: Some(Box::new(cond)),
        }));
    }

    // 1. Enchant <filter/player>
    if let Some(rest) = low.strip_prefix("enchant ") {
        if rest == "player" || rest == "opponent" {
            return Ok(st_s(StaticAbilityAst::AttachmentRestriction {
                filter: None,
                target_player: Some(rest.to_string()),
            }));
        }
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(st_s(StaticAbilityAst::AttachmentRestriction {
            filter: Some(Box::new(flt)),
            target_player: None,
        }));
    }

    // 2. Replacement: Enters tapped / Enters prepared / Enters as a copy
    if low == "~ enters prepared" || low == "~ enters the battlefield prepared" {
        return Ok(st_r(ReplacementEffectAst::EntersPrepared));
    }
    if let Some((rest, enters_tapped)) = low
        .strip_prefix("you may have ~ enter as a copy of ")
        .map(|r| (r, false))
        .or_else(|| {
            low.strip_prefix("you may have ~ enter the battlefield as a copy of ")
                .map(|r| (r, false))
        })
        .or_else(|| {
            low.strip_prefix("you may have ~ enter tapped as a copy of ")
                .map(|r| (r, true))
        })
        .or_else(|| {
            low.strip_prefix("you may have ~ enter the battlefield tapped as a copy of ")
                .map(|r| (r, true))
        })
        .or_else(|| {
            low.strip_prefix("as ~ enters, you may have it become a copy of ")
                .map(|r| (r, false))
        })
        .or_else(|| {
            low.strip_prefix("as ~ enters, you may have ~ become a copy of ")
                .map(|r| (r, false))
        })
    {
        let (flt_raw, exception) = match rest.split_once(", except ") {
            Some((f, e)) => (f, Some(e.trim().to_string())),
            None => (rest, None),
        };
        let (flt_nodur, duration) = match flt_raw.strip_suffix(" until end of turn") {
            Some(c) => (c.trim(), DurationAst::UntilEndOfTurn),
            None => (flt_raw.trim(), DurationAst::Permanent),
        };
        let flt_clean = flt_nodur.strip_prefix("any ").unwrap_or(flt_nodur);
        if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
            if enters_tapped || duration != DurationAst::Permanent {
                return Ok(st_r(ReplacementEffectAst::EntersAsCopyModified(Box::new(
                    EntersAsCopyModifiedAst {
                        optional: true,
                        filter: Box::new(flt),
                        exception,
                        enters_tapped,
                        duration,
                    },
                ))));
            }
            return Ok(st_r(ReplacementEffectAst::EntersAsCopy {
                optional: true,
                filter: Box::new(flt),
                exception,
            }));
        }
    }
    if low == "~ enters tapped" || low == "~ enters the battlefield tapped" {
        return Ok(st_r(ReplacementEffectAst::EntersTapped { unless: None }));
    }
    if low == "~ enters tapped and doesn't untap during your untap step"
        || low == "~ enters the battlefield tapped and doesn't untap during your untap step"
    {
        return Ok(st_s(StaticAbilityAst::Compound(vec![
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(self_obj()),
                rule: "EntersTappedAndDoesntUntap".to_string(),
                filter: None,
                condition: None,
            },
        ])));
    }
    // "~ enters tapped and with <N> <kind> counters on it" (Noble's Purse, Sphere of the Suns)
    if let Some(after) = low
        .strip_prefix("~ enters tapped and with ")
        .or_else(|| low.strip_prefix("~ enters the battlefield tapped and with "))
        && let Some((cnt_kind, _)) = split_once_any(&after, &[" counters on it", " counter on it"])
    {
        let (cnt, kind) = peel_leading_count(cnt_kind, vocab);
        return Ok(st_r(ReplacementEffectAst::EntersTappedAndWithCounters {
            counter_kind: kind.to_string(),
            count: Box::new(cnt),
        }));
    }
    // "As ~ enters, look at an opponent's hand, then choose <choice>" (Anointed Peacekeeper, Sorcerous Spyglass, Arachne)
    if let Some(choice) = low.strip_prefix("as ~ enters, look at an opponent's hand, then choose ")
    {
        return Ok(st_r(ReplacementEffectAst::AsEntersLookAndChoose {
            target_player: PlayerRefAst::AnyOpponent,
            zone: ZoneFilter::OpponentHand,
            choice: choice.trim().to_string(),
        }));
    }
    // "As ~ enters, you may behold <flt>. If you don't, ~ enters tapped" (Theorist's Sanctum)
    if let Some(after) = low.strip_prefix("as ~ enters, you may behold ")
        && let Some((flt_raw, _)) = after.split_once(". if you don't, ~ enters tapped")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let or_control = low.contains(" this way or you control ");
        return Ok(st_r(ReplacementEffectAst::EntersTappedUnlessReveal {
            filter: Box::new(flt),
            or_control,
        }));
    }
    // "A deck can have up to <N> cards named ~" (Nazgul, Seven Dwarves)
    if let Some(after) = low.strip_prefix("a deck can have up to ")
        && let Some(cnt_tok) = after.strip_suffix(" cards named ~")
        && parse_number_word(cnt_tok).is_some()
    {
        return Ok(st_s(StaticAbilityAst::DeckConstructionRule(low)));
    }
    if low.starts_with("as ~ enters")
        && (low.ends_with("from your hand. if you don't, ~ enters tapped")
            || low.ends_with("from your hand. if you don't, it enters tapped")
            || (low.contains(". ~ enters tapped unless you revealed ")
                && (low.ends_with(" this way") || low.contains(" this way or you control "))))
        && let Some((_, after_rev)) = low.split_once("you may reveal ")
        && let Some((flt_raw, _)) = after_rev.split_once(" from your hand")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let or_control = low.contains(" this way or you control ");
        return Ok(st_r(ReplacementEffectAst::EntersTappedUnlessReveal {
            filter: Box::new(flt),
            or_control,
        }));
    }
    if low == "if it's neither day nor night, it becomes day as ~ enters" {
        return Ok(st_rule("BecomesDayOnEnterIfNeither"));
    }
    if low.starts_with("as ~ enters")
        && (low.ends_with("life. if you don't, it enters tapped")
            || low.ends_with("life. if you don't, ~ enters tapped")
            || low.ends_with("life. if you don't, it enters the battlefield tapped")
            || low.ends_with("life. if you don't, ~ enters the battlefield tapped"))
        && let Some((_, after_pay)) = low.split_once("you may pay ")
        && let Some((life_str, _)) = after_pay.split_once(" life")
        && let Ok(life) = life_str.parse::<u16>()
    {
        return Ok(st_r(ReplacementEffectAst::EntersTappedUnlessPayLife {
            life,
        }));
    }
    if let Some(cond_raw) = low
        .strip_prefix("~ enters tapped unless ")
        .or_else(|| low.strip_prefix("~ enters the battlefield tapped unless "))
    {
        let cond = parse_condition(cond_raw, vocab)?;
        return Ok(st_r(ReplacementEffectAst::EntersTapped {
            unless: Some(Box::new(cond)),
        }));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some(cond_raw) = strip_suffix_any(
            &after_if,
            &[", ~ enters tapped", ", ~ enters the battlefield tapped"],
        )
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(st_r(ReplacementEffectAst::EntersTapped {
            unless: Some(Box::new(ConditionAst::Not(Box::new(cond)))),
        }));
    }

    // 3. Replacement: Enters tapped with counters / Escapes with counters / Enters with counters
    if let Some(after) = strip_prefix_any(
        &low,
        &[
            "~ enters tapped with ",
            "~ enters the battlefield tapped with ",
        ],
    ) && let Some(left) = strip_suffix_any(&after, &[" counters on it", " counter on it"])
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Ok(st_r(ReplacementEffectAst::EntersTappedWithCounters {
            counter_kind: kind.to_string(),
            count: Box::new(cnt),
        }));
    }
    if let Some(after) = strip_prefix_any(&low, &["~ escapes with ", "it escapes with "])
        && let Some(left) = strip_suffix_any(
            &after,
            &[
                " counters on it instead",
                " counter on it instead",
                " counters on it",
                " counter on it",
            ],
        )
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Ok(st_r(ReplacementEffectAst::EscapesWithCounters {
            counter_kind: kind.to_string(),
            count: Box::new(cnt),
        }));
    }
    // CR 702.33c Multi-slot / "and/or" Kicker enters-with replacement:
    // "If ~ was kicked with its {B} kicker, it enters with a +1/+1 counter on it and with \"Pay 3 life: Regenerate ~.\""
    let multi_kicker_pair = low
        .strip_prefix("if ~ was kicked with its ")
        .and_then(|r| r.split_once(" kicker, it enters with "));
    if let Some((slot_raw, after_mk)) = multi_kicker_pair
        && let Some((left, rest)) =
            split_once_any(&after_mk, &[" counters on it", " counter on it"])
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        let cond = Some(Box::new(ConditionAst::WasKickedSlot(
            slot_raw.trim().to_string(),
        )));
        let valid_tail = rest.is_empty()
            || rest.strip_prefix(" and with ").is_some_and(|kw_or_ab| {
                parse_keyword_list(kw_or_ab, vocab).is_some()
                    || (kw_or_ab.starts_with('"') && kw_or_ab.ends_with('"'))
            });
        if valid_tail {
            return Ok(st_r(ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
                per: None,
                x_expr: None,
                condition: cond,
            }));
        }
    }
    if let Some(after) = strip_prefix_any(
        &low,
        &[
            "~ enters with ",
            "~ enters the battlefield with ",
            "it enters with ",
            "it enters the battlefield with ",
            "if ~ was kicked, it enters with ",
            "if ~ was kicked, it enters the battlefield with ",
        ],
    ) {
        let kicked_cond = if low.starts_with("if ~ was kicked") {
            Some(Box::new(ConditionAst::WasKicked))
        } else {
            None
        };
        if let Some((left, rest)) = split_once_any(&after, &[" counters on it", " counter on it"]) {
            let (mut cnt, kind) = peel_leading_count(left, vocab);
            let mut per = None;
            let mut x_expr = None;
            let mut cond = kicked_cond;
            if let Some(wx) = rest.strip_prefix(", where x is ") {
                x_expr = Some(Box::new(parse_value_expr(wx, vocab)?));
            } else if let Some(eq_raw) = rest.strip_prefix(" equal to ") {
                if eq_raw == "the amount of mana spent to cast it"
                    || eq_raw == "the amount of mana spent to cast ~"
                {
                    x_expr = Some(Box::new(ValueExprAst::PriorResult));
                } else {
                    x_expr = Some(Box::new(parse_value_expr(eq_raw, vocab)?));
                }
            } else if matches!(
                rest,
                " for each color of mana spent to cast it"
                    | " for each color of mana spent to cast ~"
            ) {
                x_expr = Some(Box::new(ValueExprAst::ColorsOfManaSpent));
            } else if matches!(
                rest,
                " for each time it was kicked" | " for each time ~ was kicked"
            ) {
                x_expr = Some(Box::new(ValueExprAst::TimesKicked));
            } else if let Some(fe) = rest.strip_prefix(" for each ") {
                if let Some((fe1, second_part)) = fe.split_once(" and ")
                    && let Some((sec_left, fe2)) = split_once_any(
                        &second_part,
                        &[" counters on it for each ", " counter on it for each "],
                    )
                    && let Ok(flt1) = parse_object_filter(fe1.trim(), vocab)
                    && let Ok(flt2) = parse_object_filter(fe2.trim(), vocab)
                {
                    let (cnt2, kind2) = peel_leading_count(sec_left.trim(), vocab);
                    return Ok(st_r(ReplacementEffectAst::Compound(vec![
                        ReplacementEffectAst::EntersWithCounters {
                            counter_kind: kind.to_string(),
                            count: Box::new(cnt),
                            per: Some(Box::new(flt1)),
                            x_expr: None,
                            condition: cond.clone(),
                        },
                        ReplacementEffectAst::EntersWithCounters {
                            counter_kind: kind2.to_string(),
                            count: Box::new(cnt2),
                            per: Some(Box::new(flt2)),
                            x_expr: None,
                            condition: cond,
                        },
                    ])));
                }
                if let Ok(flt) = parse_object_filter(fe, vocab) {
                    per = Some(Box::new(flt));
                } else {
                    x_expr = Some(Box::new(parse_for_each_value_expr(fe, vocab)?));
                }
            } else if let Some(kw_raw) = rest.strip_prefix(" and with ")
                && (parse_keyword_list(kw_raw, vocab).is_some()
                    || (kw_raw.starts_with('"') && kw_raw.ends_with('"')))
            {
                // e.g. "If ~ was kicked, it enters with two +1/+1 counters on it and with flying"
            } else if let Some(c_raw) = rest.strip_prefix(" if ") {
                cond = Some(Box::new(parse_condition(c_raw, vocab)?));
            } else if let Some(c_raw) = rest.strip_prefix(" unless ") {
                cond = Some(Box::new(ConditionAst::Not(Box::new(parse_condition(
                    c_raw, vocab,
                )?))));
            } else if let Some(fe) = rest
                .strip_prefix(" plus an additional +1/+1 counter on it for each ")
                .or_else(|| rest.strip_prefix(" and a +1/+1 counter on it for each "))
            {
                let extra = parse_for_each_value_expr(fe, vocab)?;
                cnt = ValueExprAst::Add(Box::new(cnt), Box::new(extra));
            } else if rest.starts_with(" from among ") {
                // Grimdancer / Crystalline Giant: "~ enters with your choice of two different counters on it from among ..."
            } else if !rest.is_empty() {
                return Err(format!("etb_cnt_tail:{rest}"));
            }
            return Ok(st_r(ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
                per,
                x_expr,
                condition: cond,
            }));
        }
    }

    // 3b. Others enter with [additional] counters ("Each other creature you control enters with an additional +1/+1 counter on it", Bloodspore Thrinax, Grumgully, Good-Fortune Unicorn)
    if let Some((subj_raw, rest)) = split_once_any(
        &low,
        &[
            " enters with ",
            " enter with ",
            " enters the battlefield with ",
            " enter the battlefield with ",
        ],
    ) && subj_raw != "~"
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let (rest_noadd, additional) = if let Some(r) = rest.strip_prefix("an additional ") {
            (r, true)
        } else {
            (rest, false)
        };
        if let Some((left, tail)) = split_once_any(
            &rest_noadd,
            &[
                " counters on it",
                " counter on it",
                " counters on them",
                " counter on them",
            ],
        ) {
            let x_val_opt = if tail.is_empty() {
                Some(None)
            } else if let Some(wx) = tail.strip_prefix(", where x is ") {
                parse_value_expr(wx, vocab).ok().map(Some)
            } else {
                None
            };
            if let Some(x_opt) = x_val_opt {
                let (mut cnt, kind) = peel_leading_count(left, vocab);
                let mut per = None;
                if let Some(xv) = x_opt
                    && !substitute_chosen_x(&mut cnt, &xv)
                {
                    per = Some(Box::new(xv));
                }
                return Ok(st_r(ReplacementEffectAst::OthersEnterWithCounters {
                    subject: Box::new(subj),
                    counter_kind: kind.to_string(),
                    count: Box::new(cnt),
                    additional,
                    per,
                }));
            }
        }
    }

    // 4. Replacement: TriggerMultiplier (Panharmonicon/Elesh Norn/Yarok/Isshin/Katara) + Knight of the Holy Nimbus + Ali from Cairo + EventInstead
    if let Some(after) = low.strip_prefix("if a triggered ability of ")
        && let Some((src_part, tail)) = after.split_once(" triggers, ")
        && tail.contains("triggers an additional time")
    {
        return Ok(st_s(StaticAbilityAst::TriggerMultiplier {
            event_desc: src_part.trim().to_string(),
            multiplier: 2,
            condition: None,
        }));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((ev_part, tail)) = after.split_once(" causes a triggered ability of ")
        && (tail.ends_with("triggers an additional time")
            || tail.ends_with("triggers an additional time."))
    {
        if let Some((src_raw, _)) = tail.split_once(" to trigger")
            && let Ok(src_flt) = parse_object_filter(src_raw.trim(), vocab)
        {
            return Ok(st_s(StaticAbilityAst::ScopedTriggerMultiplier {
                event_desc: ev_part.trim().to_string(),
                source_filter: Box::new(src_flt),
                additional_triggers: 1,
                condition: None,
            }));
        }
        return Ok(st_s(StaticAbilityAst::TriggerMultiplier {
            event_desc: ev_part.trim().to_string(),
            multiplier: 2,
            condition: None,
        }));
    }
    if low == "if ~ would be destroyed, regenerate it" {
        return Ok(st_instead(
            "~",
            "be destroyed",
            "regenerate it",
            Some(vec![InstructionAst::Regenerate {
                subject: self_obj(),
            }]),
        ));
    }
    if let Some(after_fl) =
        low.strip_prefix("damage that would reduce your life total to less than ")
        && low.ends_with(" instead")
    {
        let floor_tok = after_fl
            .split_once(" reduces")
            .map_or(after_fl, |(f, _)| f)
            .trim();
        let instead_body = parse_value_expr(floor_tok, vocab).ok().map(|val| {
            vec![InstructionAst::SetLifeTotal {
                player: PlayerRefAst::You,
                value: Box::new(val),
            }]
        });
        return Ok(st_instead(
            "damage",
            "reduce_life_below_floor",
            s.to_string(),
            instead_body,
        ));
    }
    if low.starts_with("if a spell or ability an opponent controls causes you to discard ~, put it onto the battlefield")
        || (low.starts_with("if ")
            && (low.contains(" would deal damage to ")
                || low.contains(" would deal combat damage to ")
                || low.contains(" damage would be dealt to "))
            && low.contains(", prevent that damage"))
    {
        let instead_part = s.split_once(", ").map_or(s, |(_, r)| r);
        let instead_body = parse_instead_body(instead_part, vocab);
        return Ok(st_r(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "~".to_string(),
                would: "special_replacement".to_string(),
                instead: s.to_string(),
                instead_body,
            },
        ));
    }
    if let Some(after_dbl) = low.strip_prefix("double all damage that ")
        && after_dbl.ends_with(" would deal")
    {
        return Ok(st_instead(
            after_dbl
                .strip_suffix(" would deal")
                .unwrap_or(after_dbl)
                .trim()
                .to_string(),
            "deal_damage",
            "double_damage",
            Some(vec![InstructionAst::DoubleStatOrCounters {
                subject: box_anaphora("damage"),
                kind: "damage".to_string(),
                duration: DurationAst::Instant,
            }]),
        ));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_ev, rest)) = after.split_once(" would ")
        && let Some((would, instead_raw)) = split_would_instead_clause(rest)
        && let Some(instead) = instead_raw
            .strip_suffix(" instead")
            .or_else(|| instead_raw.strip_prefix("instead "))
            .or_else(|| instead_raw.strip_prefix("you may instead "))
            .or_else(|| instead_raw.strip_prefix("instead, "))
            .or_else(|| {
                if instead_raw.contains(" instead ") {
                    Some(instead_raw)
                } else {
                    None
                }
            })
    {
        let instead_body = parse_instead_body(instead, vocab);
        return Ok(st_instead(
            subj_ev.trim().to_string(),
            would.trim().to_string(),
            instead.trim().to_string(),
            instead_body,
        ));
    }

    // CR 614 "each <flt> that would <would> ... instead" (Crafty Cutpurse, etc.)
    if let Some((subj_part, rest)) = low.split_once(" that would ")
        && let Some(instead) = rest.strip_suffix(" instead")
    {
        let instead_body = parse_instead_body(instead, vocab);
        return Ok(st_instead(
            subj_part.trim().to_string(),
            "would_event",
            instead.trim().to_string(),
            instead_body,
        ));
    }

    // 5. Replacement: As ~ enters, [secretly] choose <choice> / enters under opponent's control
    if low == "~ enters under the control of an opponent of your choice" {
        return Ok(st_r(ReplacementEffectAst::AsEntersChoose {
            choice: "opponent_controller".to_string(),
        }));
    }
    if let Some(choice) = strip_prefix_any(
        &low,
        &[
            "as ~ enters, choose ",
            "as ~ enters the battlefield, choose ",
            "as ~ enters, secretly choose ",
            "as it enters, choose ",
            "as it enters the battlefield, choose ",
        ],
    ) && !choice.is_empty()
        && !choice.contains('.')
    {
        return Ok(st_r(ReplacementEffectAst::AsEntersChoose {
            choice: choice.to_string(),
        }));
    }
    // General "As ~ enters [or is turned face up], <imperative action>"
    // (Phylactery Lich, Wood Elemental, Phyrexian Processor, Crowd-Control Warden, Arachne, etc.)
    for as_pref in [
        "as ~ enters or is turned face up, ",
        "as ~ enters the battlefield, ",
        "as ~ enters, ",
        "as it enters, ",
    ] {
        if low.starts_with(as_pref) {
            let body_str = &s[as_pref.len()..];
            let (ins, fails) = parse_effect_body(body_str, vocab);
            if fails.is_empty() && !ins.is_empty() {
                return Ok(st_r(ReplacementEffectAst::AsEntersAction(ins)));
            }
        }
    }

    // Opening hand Leyline / Chancellor reveal / Exert-as-attacks / ". The same is true for ..." multi-sentence statics
    if low == "if ~ is in your opening hand, you may begin the game with it on the battlefield" {
        return Ok(st_rule("OpeningHandBeginOnBattlefield"));
    }
    if let Some(after_oh) = low.strip_prefix("you may reveal ~ from your opening hand. if you do, ")
    {
        let body_slice = &s[s.len() - after_oh.len()..];
        let (ins, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !ins.is_empty() {
            return Ok(st_s(StaticAbilityAst::OpeningHandRevealAction {
                body: ins,
            }));
        }
    }
    if let Some(after_ex) = low.strip_prefix("you may exert ~ as it attacks. when you do, ") {
        let body_slice = &s[s.len() - after_ex.len()..];
        let (ins, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !ins.is_empty() {
            return Ok(st_s(StaticAbilityAst::ExertOnAttackAction { body: ins }));
        }
    }
    if let Some((first_sent, same_tail)) = low.split_once(". the same is true for ")
        && let Ok(StaticOrReplacement::Static(first_st)) =
            parse_static_or_replacement(&s[..first_sent.len()], vocab)
    {
        return Ok(st_s(StaticAbilityAst::Compound(vec![
            first_st,
            st_rule_ast(None, format!("same_is_true_for:{same_tail}"), None),
        ])));
    }
    if low.starts_with("during your end step, ")
        && let Ok(mut inner) =
            parse_static_or_replacement(&s["during your end step, ".len()..], vocab)
    {
        attach_condition(
            &mut inner,
            ConditionAst::Marker(GameMarker::DuringStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::EndStep,
            }),
        );
        return Ok(inner);
    }

    // "Once during each of your turns, <static>" / "Once each turn, <static>"
    if low.starts_with("once during each of your turns, ")
        && let Ok(StaticOrReplacement::Static(inner)) =
            parse_static_or_replacement(&s["once during each of your turns, ".len()..], vocab)
    {
        return Ok(st_s(StaticAbilityAst::OnceDuringYourTurn(Box::new(inner))));
    }
    if low.starts_with("once each turn, ")
        && let Ok(StaticOrReplacement::Static(inner)) =
            parse_static_or_replacement(&s["once each turn, ".len()..], vocab)
    {
        return Ok(st_s(StaticAbilityAst::OncePerTurn(Box::new(inner))));
    }

    // "During your turn, <static>" / "During each opponent's turn, <static>" / "During turns other than yours, <static>"
    if low.starts_with("during your turn, ")
        && let Ok(mut inner) = parse_static_or_replacement(&s["during your turn, ".len()..], vocab)
    {
        attach_condition(&mut inner, ConditionAst::IsYourTurn);
        return Ok(inner);
    }
    if low.starts_with("during turns other than yours, ")
        && let Ok(mut inner) =
            parse_static_or_replacement(&s["during turns other than yours, ".len()..], vocab)
    {
        attach_condition(
            &mut inner,
            ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)),
        );
        return Ok(inner);
    }
    // "Otherwise, <static>" (Serra's Boon, etc.)
    if low.starts_with("otherwise, ")
        && let Ok(StaticOrReplacement::Static(inner)) =
            parse_static_or_replacement(&s["otherwise, ".len()..], vocab)
    {
        return Ok(st_s(StaticAbilityAst::Otherwise(Box::new(inner))));
    }

    // "As long as <cond>, <static>" (scan all ", " split points for comma-containing conditions like Kosei)
    if low.starts_with("as long as ") {
        let after_ala = &s["as long as ".len()..];
        for (idx, _) in after_ala.match_indices(", ") {
            let cond_raw = &after_ala[..idx];
            let rest = &after_ala[idx + 2..];
            if let Ok(cond) = parse_condition(cond_raw, vocab)
                && let Ok(mut inner) = parse_static_or_replacement(rest, vocab)
            {
                attach_condition(&mut inner, cond);
                return Ok(inner);
            }
        }
    }

    // Leading "If <cond>, <static>" (Avatar of Will, Chrome Replicator, etc.)
    if low.starts_with("if ") && !low.contains(" would ") {
        let after_if = &s[3..];
        for (idx, _) in after_if.match_indices(", ") {
            let cond_raw = &after_if[..idx];
            let rest = &after_if[idx + 2..];
            if let Ok(cond) = parse_condition(cond_raw, vocab)
                && let Ok(mut inner) = parse_static_or_replacement(rest, vocab)
            {
                attach_condition(&mut inner, cond);
                return Ok(inner);
            }
        }
    }

    // Trailing "<static> if <cond>" (Shadow the Hedgehog: "Each spell you cast has split second if ...")
    if !low.starts_with("if ")
        && !low.starts_with("cast ~ only if ")
        && let Some(idx) = low.rfind(" if ")
        && let Ok(cond) = parse_condition(&s[idx + 4..], vocab)
        && let Ok(mut inner) = parse_static_or_replacement(s[..idx].trim(), vocab)
    {
        attach_condition(&mut inner, cond);
        return Ok(inner);
    }

    // Free alternative casting cost: "You may cast ~ without paying its mana cost" (Deepwood Legate, Kyren Legate, Land Grant)
    if low == "you may cast ~ without paying its mana cost"
        || low == "you may cast this spell without paying its mana cost"
    {
        return Ok(st_alt_cast(Vec::new(), None));
    }

    // Trailing "<static> as long as <cond>" (Threshold, Metalcraft, Delirium, conditional pump, So Tiny "instead as long as")
    if let Some(idx) = low.rfind(" as long as ")
        && let Ok(cond) = parse_condition(&s[idx + " as long as ".len()..], vocab)
    {
        let left_raw = s[..idx].trim();
        let left_clean = strip_suffix_any(&left_raw, &[" instead", " Instead"])
            .unwrap_or(left_raw)
            .trim();
        if let Ok(mut inner) = parse_static_or_replacement(left_clean, vocab) {
            attach_condition(&mut inner, cond);
            return Ok(inner);
        }
    }

    // Trailing "<static> unless <cond>" (Reckless Cohort, Pipsqueak: "~ can't attack alone unless it has a +1/+1 counter on it")
    // Preserve Pillowfort "unless their/its controller pays ..." for the AttackTax rule below.
    if let Some(idx) = low.rfind(" unless ") {
        let rhs_u = &low[idx + " unless ".len()..];
        if !rhs_u.starts_with("their controller pays ")
            && !rhs_u.starts_with("its controller pays ")
            && let Ok(cond) = parse_condition(&s[idx + " unless ".len()..], vocab)
            && let Ok(mut inner) = parse_static_or_replacement(s[..idx].trim(), vocab)
        {
            attach_condition(&mut inner, ConditionAst::Not(Box::new(cond)));
            return Ok(inner);
        }
    }

    // 6. ContinuousModifyPT + optional keywords / quoted ability / combat rule / for-each
    if let Some((idx, vlen)) = low
        .find(" gets ")
        .map(|i| (i, 6))
        .or_else(|| low.find(" get ").map(|i| (i, 5)))
        && let Ok(subj) = parse_subject_or_target(&s[..idx], vocab)
    {
        let after = s[idx + vlen..].trim();
        let after =
            strip_prefix_any(&after, &["an additional ", "An additional "]).unwrap_or(after);
        let (pt_tok_raw, rest) = match after.split_once(' ') {
            Some((p, r)) => (p, r.trim()),
            None => (after, ""),
        };
        let pt_tok = pt_tok_raw.trim_end_matches(',');
        if let Some((dx, dy)) = parse_pt_mod(pt_tok) {
            let mut grant_keywords = Vec::new();
            let mut grant_ability_text = None;
            let mut scale = None;
            let mut paired_rule = None;
            let mut valid = true;

            // Peel optional trailing ", and is a <types> in addition to its other [colors and] types" / " and is a <types>..."
            let (rest, trailing_is_type) = if let Some((before, after_is)) = split_once_any(
                &rest,
                &[", and is a ", ", and is an ", " and is a ", " and is an "],
            ) {
                (before.trim(), Some(after_is.trim()))
            } else {
                (rest, None)
            };
            if let Some(ty) = trailing_is_type {
                paired_rule = Some(Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj.clone()),
                    type_or_subtype: ty.to_ascii_lowercase(),
                    additive: ty.to_ascii_lowercase().contains("in addition"),
                    condition: None,
                }));
            }

            // Peel optional leading "has ..." after comma ("+1/+0, has flying" or "+1/+0, has \"...\"")
            let rest_clean = if let Some(after_h) = strip_prefix_any(&rest, &["has ", "have "]) {
                if let Some((kw_side, ab_side)) = after_h.split_once(" and \"")
                    && let Some(ab_inner) = ab_side.trim_end_matches(',').strip_suffix('"')
                {
                    if let Some(kws) = parse_keyword_list(kw_side, vocab) {
                        grant_keywords = kws;
                        grant_ability_text = Some(ab_inner.to_string());
                        ""
                    } else {
                        rest
                    }
                } else if after_h.starts_with('"') && after_h.trim_end_matches(',').ends_with('"') {
                    grant_ability_text =
                        Some(after_h.trim_end_matches(',').trim_matches('"').to_string());
                    ""
                } else if let Some(kws) = parse_keyword_list(after_h, vocab) {
                    grant_keywords = kws;
                    ""
                } else {
                    rest
                }
            } else {
                rest
            };

            // Split optional trailing "and has/have <kws>" BEFORE "for each ..." so
            // "Equipped creature gets +1/+0 for each Gate you control and has vigilance and menace" works!
            let (base_tail, tail_has) =
                if let Some((b, h)) = split_once_any(&rest_clean, &[" and has ", " and have "]) {
                    (b.trim(), Some(h.trim()))
                } else {
                    (rest_clean, None)
                };
            let blow_tail = base_tail.to_ascii_lowercase();

            if let Some(after_has) = tail_has {
                if after_has.starts_with('"') && after_has.ends_with('"') {
                    grant_ability_text = Some(after_has.trim_matches('"').to_string());
                } else if let Some((kw_side, ab_side)) =
                    split_once_any(&after_has, &[", and \"", " and \""])
                    && let Some(ab_inner) = ab_side.strip_suffix('"')
                    && let Some(kws) = parse_keyword_list(kw_side, vocab)
                {
                    grant_keywords = kws;
                    grant_ability_text = Some(ab_inner.to_string());
                } else if let Some(kws) = parse_keyword_list(after_has, vocab) {
                    grant_keywords = kws;
                } else {
                    valid = false;
                }
            }

            if let Some(wx) = blow_tail.strip_prefix("where x is ") {
                if let Ok(val) = parse_value_expr(wx, vocab) {
                    scale = Some(Box::new(val));
                } else {
                    valid = false;
                }
            } else if let Some(fe) = blow_tail.strip_prefix("for each ") {
                if let Ok(fe_val) = parse_for_each_value_expr(fe, vocab) {
                    scale = Some(Box::new(fe_val));
                } else {
                    valid = false;
                }
            } else if let Some(after_has) = strip_prefix_any(&base_tail, &["and has ", "and have "])
            {
                if after_has.starts_with('"') && after_has.ends_with('"') {
                    grant_ability_text = Some(after_has.trim_matches('"').to_string());
                } else if let Some((kw_side, ab_side)) =
                    split_once_any(&after_has, &[", and \"", " and \""])
                    && let Some(ab_inner) = ab_side.strip_suffix('"')
                    && let Some(kws) = parse_keyword_list(kw_side, vocab)
                {
                    grant_keywords = kws;
                    grant_ability_text = Some(ab_inner.to_string());
                } else if let Some(kws) = parse_keyword_list(after_has, vocab) {
                    grant_keywords = kws;
                } else {
                    valid = false;
                }
            } else if blow_tail == "and loses all abilities"
                || blow_tail == "and lose all abilities"
            {
                paired_rule = Some(Box::new(st_rule_ast(
                    Some(subj.clone()),
                    "LosesAllAbilities",
                    None,
                )));
            } else if let Some(lost_kw) = strip_prefix_any(&blow_tail, &["and loses ", "and lose "])
                && parse_keyword_list(lost_kw, vocab).is_some()
            {
                paired_rule = Some(Box::new(st_rule_ast(
                    Some(subj.clone()),
                    format!("Loses:{lost_kw}"),
                    None,
                )));
            } else if let Some(rule_tail) = base_tail.strip_prefix("and ") {
                if let Ok(StaticOrReplacement::Static(r_ast)) =
                    parse_static_or_replacement(&format!("~ {rule_tail}"), vocab)
                {
                    paired_rule = Some(Box::new(r_ast));
                } else {
                    valid = false;
                }
            } else if !blow_tail.is_empty() {
                valid = false;
            }

            if valid {
                return Ok(st_s(StaticAbilityAst::ContinuousModifyPT {
                    subject: Box::new(subj),
                    dx,
                    dy,
                    grant_keywords,
                    grant_ability_text,
                    scale,
                    condition: None,
                    paired_rule,
                }));
            }
        }
    }

    // 7. GrantAbilityText / GrantKeywords (+ optional "and \"<ab>\"" / "and loses <kw>" / "and <rule>")
    if s.contains('"')
        && let Some((left, right)) = s.split_once(" has ").or_else(|| s.split_once(" have "))
        && let Some((kw_side, ab_side)) = right.split_once(" and \"")
        && let Some(ab_text) = ab_side.strip_suffix('"')
        && let (Ok(subj), Some(kws)) = (
            parse_subject_or_target(left, vocab),
            parse_keyword_list(kw_side, vocab),
        )
    {
        let compiled = compile_granted_ability(ab_text, vocab);
        return Ok(st_s(StaticAbilityAst::GrantKeywords {
            subject: Box::new(subj.clone()),
            keywords: kws,
            condition: None,
            paired_rule: Some(Box::new(StaticAbilityAst::GrantAbilityText {
                subject: Box::new(subj),
                ability_text: ab_text.to_string(),
                compiled,
                condition: None,
            })),
        }));
    }
    if s.contains('"')
        && let Some((left, spans, trailing)) = extract_quoted_spans(s)
        && trailing.trim().is_empty()
        && let Some(subj_raw) = left
            .trim_end()
            .strip_suffix(" has")
            .or_else(|| left.trim_end().strip_suffix(" have"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        if spans.len() == 1 {
            let ab_text = spans[0];
            let compiled = compile_granted_ability(ab_text, vocab);
            return Ok(st_grant_ab(subj, ab_text.to_string(), compiled));
        }
        let mut grants = Vec::with_capacity(spans.len());
        for ab_text in spans {
            let compiled = compile_granted_ability(ab_text, vocab);
            grants.push(StaticAbilityAst::GrantAbilityText {
                subject: Box::new(subj.clone()),
                ability_text: ab_text.to_string(),
                compiled,
                condition: None,
            });
        }
        return Ok(st_s(StaticAbilityAst::Compound(grants)));
    }
    if !s.contains('"')
        && let Some((idx, vlen)) = low
            .find(" also has ")
            .map(|i| (i, 10))
            .or_else(|| low.find(" also have ").map(|i| (i, 11)))
            .or_else(|| low.find(" has ").map(|i| (i, 5)))
            .or_else(|| low.find(" have ").map(|i| (i, 6)))
        && let Ok(subj) = parse_subject_or_target(&s[..idx], vocab)
    {
        let rhs = &s[idx + vlen..];
        let rlow = rhs.to_ascii_lowercase();
        if let Some((kw_side, lose_side)) = split_once_any(&rlow, &[" and loses ", " and lose "])
            && let (Some(gkws), Some(lkws)) = (
                parse_keyword_list(kw_side, vocab),
                parse_keyword_list(lose_side, vocab),
            )
        {
            return Ok(st_cont_layer(
                subj, None, false, None, None, gkws, lkws, false,
            ));
        }
        if let Some(kws) = parse_keyword_list(rhs, vocab) {
            return Ok(st_kw(subj, kws));
        }
        if let Some((kw_part, rule_part)) = rhs.split_once(" and ")
            && rule_part.to_ascii_lowercase().starts_with("can't ")
            && let Some(kws) = parse_keyword_list(kw_part, vocab)
            && let Ok(StaticOrReplacement::Static(r_ast)) =
                parse_static_or_replacement(&format!("~ {rule_part}"), vocab)
        {
            return Ok(st_s(StaticAbilityAst::GrantKeywords {
                subject: Box::new(subj),
                keywords: kws,
                condition: None,
                paired_rule: Some(Box::new(r_ast)),
            }));
        }
    }

    // 7b. Continuous ability inheritance: "<subj> has/have all [activated|loyalty] abilities of [all] <filter>"
    // (Necrotic Ooze, Trazyn, Mirran Safehouse, Steward of the Harvest, Marvin, Kasmina)
    if let Some((subj_raw, src_raw)) = split_once_any(
        &low,
        &[
            " has the loyalty abilities of ",
            " have the loyalty abilities of ",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && parse_subject_or_target(src_raw, vocab).is_ok()
    {
        return Ok(st_s(StaticAbilityAst::InheritAbilities {
            subject: Box::new(subj),
            source_filter: Box::new(any_perm()),
            activated_only: true,
            condition: None,
        }));
    }
    for (needle, activated_only) in [
        (" has all activated and triggered abilities of ", false),
        (" have all activated and triggered abilities of ", false),
        (" has all activated abilities of ", true),
        (" have all activated abilities of ", true),
        (" has all loyalty abilities of ", true),
        (" have all loyalty abilities of ", true),
        (" has all abilities of ", false),
        (" have all abilities of ", false),
    ] {
        if let Some((subj_raw, src_raw)) = low.split_once(needle) {
            let src_clean = src_raw.strip_prefix("all ").unwrap_or(src_raw);
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
                && (parse_object_filter(src_clean, vocab).is_ok()
                    || src_clean == "the last chosen card"
                    || src_clean.starts_with("cards exiled with "))
            {
                let source_filter = parse_object_filter(src_clean, vocab).unwrap_or(any_card_flt());
                return Ok(st_s(StaticAbilityAst::InheritAbilities {
                    subject: Box::new(subj),
                    source_filter: Box::new(source_filter),
                    activated_only,
                    condition: None,
                }));
            }
        }
    }

    // Pass N iconic Aura/Equipment transformation statics:
    // "<subj> is a/an <desc> with \"<ab>\" [and [it] loses all other [card types and] abilities]"
    // (Imprisoned in the Moon, Minimus Containment, Sugar Coat, In Too Deep)
    if s.contains('"')
        && let Some((subj_raw, rhs)) = split_once_any(&low, &[" is a ", " is an "])
        && let Some((ty_part, after_w)) = rhs.split_once(" with \"")
        && let Some((ab_raw, tail)) = after_w.split_once('"')
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let t_clean = tail.trim().trim_start_matches(',').trim();
        let lose_all =
            t_clean.contains("loses all other") || t_clean.contains("loses all abilities");
        if t_clean.is_empty() || lose_all {
            let orig_ab = s
                .split_once('"')
                .and_then(|(_, r)| r.split_once('"'))
                .map_or(ab_raw, |(inner, _)| inner);
            let compiled = compile_granted_ability(orig_ab, vocab);
            return Ok(st_s(StaticAbilityAst::Compound(vec![
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
            ])));
        }
    }
    // "<subj> is snow" / "<subj> is legendary" / "<subj> are snow/legendary/no longer snow/the chosen color" / "<subj> is a copy of <src>"
    if let Some((subj_raw, rhs)) = low.split_once(" is ").or_else(|| low.split_once(" are "))
        && matches!(
            rhs,
            "snow" | "legendary" | "no longer snow" | "the chosen color"
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_cont_layer(
            subj,
            Some(rhs.to_string()),
            rhs != "no longer snow" && rhs != "the chosen color",
            None,
            if rhs == "the chosen color" {
                Some(rhs.to_string())
            } else {
                None
            },
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    // CR 613.4b "<subj> has/have base power and toughness each equal to <val>" (Opalescence, Starfield of Nyx, March of the Machines)
    if let Some((subj_raw, val_raw)) = low
        .split_once(" has base power and toughness each equal to ")
        .or_else(|| low.split_once(" have base power and toughness each equal to "))
        && let (Ok(subj), Ok(val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(val_raw, vocab),
        )
    {
        return Ok(st_s(StaticAbilityAst::DynamicBasePT {
            subject: Box::new(subj),
            power: Box::new(val.clone()),
            toughness: Box::new(val),
            condition: None,
        }));
    }
    // "<subj> have/has base power/toughness <N>" (Maha, Its Feathers Night)
    if let Some((subj_raw, rhs)) = split_once_any(
        &low,
        &[
            " have base toughness ",
            " has base toughness ",
            " have base power ",
            " has base power ",
        ],
    ) && let (Ok(subj), Ok(_val)) = (
        parse_subject_or_target(subj_raw, vocab),
        parse_value_expr(rhs, vocab),
    ) {
        let is_power = low.contains(" base power ");
        return Ok(st_cont_layer(
            subj,
            None,
            false,
            Some(if is_power {
                (rhs.trim().to_string(), "keep".to_string())
            } else {
                ("keep".to_string(), rhs.trim().to_string())
            }),
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    // "<subj> lose(s) <kw> and can't have or gain <kw>" (Archetype cycle)
    if let Some((subj_raw, rest)) = split_once_any(&low, &[" lose ", " loses "])
        && let Some((kw1, _kw2)) = rest.split_once(" and can't have or gain ")
        && let (Ok(subj), Some(lkws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw1, vocab),
        )
    {
        return Ok(st_cont_layer(
            subj,
            None,
            false,
            None,
            None,
            Vec::new(),
            lkws,
            false,
        ));
    }
    if let Some((subj_raw, src_raw)) = low.split_once(" is a copy of ")
        && let (Ok(subj), Ok(_src)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(src_raw, vocab),
        )
    {
        return Ok(st_cont_layer(
            subj,
            Some(format!("copy_of:{src_raw}")),
            false,
            None,
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }

    // 7c. Unified ContinuousLayerEffect (Layers 4/5/6/7b):
    // "<subj> is a/an P/T <desc> creature [with <kws>] [in addition to its other types]"
    // (Awaken the Ancient, Vastwood Zendikon, Nissa's Zendikon, Grist, Grand Master of Flowers)
    if let Some((subj_raw, rhs)) =
        split_once_any(&low, &[" is a ", " is an ", "'s a ", "'s an ", " are "])
        && let Some((pt_tok, desc_rest)) = rhs.split_once(' ')
        && let Some((p, t)) = parse_base_pt(pt_tok)
        && desc_rest.contains("creature")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let additive_types = desc_rest.contains("in addition to its other")
            || desc_rest.contains("in addition to their other");
        let desc_clean = desc_rest
            .split_once(" in addition to ")
            .map_or(desc_rest, |(a, _)| a)
            .trim();
        let (ty_part, grant_keywords) = match desc_clean.split_once(" with ") {
            Some((d, kw_raw)) => (
                d.trim(),
                parse_keyword_list(kw_raw, vocab).unwrap_or_default(),
            ),
            None => (desc_clean, Vec::new()),
        };
        return Ok(st_cont_layer(
            subj,
            Some(ty_part.to_string()),
            additive_types,
            Some((p, t)),
            None,
            grant_keywords,
            Vec::new(),
            false,
        ));
    }
    // "<subj> is a/an <desc> and loses all other card types" (One with the Stars)
    if let Some((subj_raw, rhs)) = split_once_any(&low, &[" is a ", " is an ", " are "])
        && let Some(ty_part) = rhs
            .strip_suffix(" and loses all other card types")
            .or_else(|| rhs.strip_suffix(" and lose all other card types"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_cont_layer(
            subj,
            Some(ty_part.trim().to_string()),
            false,
            None,
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    // "<subj> is a/an <desc> with base power and toughness P/T [and has <kws>] [and loses all other abilities] [in addition to its other types]"
    // "<subj> are/is colorless / white / blue / black / red / green"
    if let Some((subj_raw, rhs)) = split_once_any(&low, &[" is a ", " is an ", " are "])
        && let Some((ty_part, rest)) = rhs.split_once(" with base power and toughness ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let additive_types = rest.contains("in addition to its other")
            || rest.contains("in addition to their other");
        let lose_all = rest.contains("loses all other abilities")
            || rest.contains("lose all other abilities")
            || rest.contains("loses all abilities");
        let pt_tok = rest.split_whitespace().next().unwrap_or("");
        if let Some((p, t)) = parse_base_pt(pt_tok.trim_end_matches(',')) {
            let mut grant_keywords = Vec::new();
            if let Some((_, after_has)) = split_once_any(&rest, &[" and has ", " and have "]) {
                let kw_clean = after_has
                    .split_once(" and loses ")
                    .map_or(after_has, |(k, _)| k);
                if let Some(kws) = parse_keyword_list(kw_clean, vocab) {
                    grant_keywords = kws;
                }
            }
            return Ok(st_cont_layer(
                subj,
                Some(ty_part.trim().to_string()),
                additive_types,
                Some((p, t)),
                None,
                grant_keywords,
                Vec::new(),
                lose_all,
            ));
        }
    }
    if let Some((subj_raw, col_raw)) = low.split_once(" are ").or_else(|| low.split_once(" is "))
        && matches!(
            col_raw,
            "colorless" | "white" | "blue" | "black" | "red" | "green" | "all colors"
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_cont_layer(
            subj,
            None,
            false,
            None,
            Some(col_raw.to_string()),
            Vec::new(),
            Vec::new(),
            false,
        ));
    }

    // "<subj> [loses all abilities and] is/becomes an artifact creature with power and toughness each equal to its mana value" (Animate Artifact, March of the Machines, Titania's Song)
    if let Some((subj_raw, rhs)) = low
        .split_once(" is an artifact creature with power and toughness each equal to ")
        .or_else(|| {
            low.split_once("'s an artifact creature with power and toughness each equal to ")
        })
        .or_else(|| {
            low.split_once(
                " loses all abilities and becomes an artifact creature with power and toughness each equal to ",
            )
        })
        .or_else(|| {
            low.split_once(
                " becomes an artifact creature with power and toughness each equal to ",
            )
        })
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_s(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some("artifact creature".to_string()),
                additive_types: false,
                base_pt: Some((rhs.trim().to_string(), rhs.trim().to_string())),
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: low.contains("loses all abilities"),
                condition: None,
            },
        ));
    }

    // "<subj> has/have base power <p> or base toughness <t>" (Vhati il-Dal)
    if let Some((subj_raw, rhs)) = split_once_any(&low, &[" has base power ", " have base power "])
        && let Some((p_raw, t_raw)) = rhs.split_once(" or base toughness ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_cont_layer(
            subj,
            None,
            false,
            Some((p_raw.trim().to_string(), t_raw.trim().to_string())),
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }

    // "<subj> have/has base power and toughness P/T [and are/is <types> in addition to their/its other types]" (Kudo, King Among Bears)
    if let Some((subj_raw, rhs)) = split_once_any(
        &low,
        &[
            " have base power and toughness ",
            " has base power and toughness ",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let pt_tok = rhs.split_whitespace().next().unwrap_or("");
        if let Some((p, t)) = parse_base_pt(pt_tok.trim_end_matches(',')) {
            let rest = rhs[pt_tok.len()..].trim();
            let (set_types, additive_types) =
                if let Some(after_are) = strip_prefix_any(&rest, &["and are ", "and is "]) {
                    let ty = after_are
                        .split_once(" in addition to ")
                        .map_or(after_are, |(a, _)| a);
                    (
                        Some(ty.trim().to_string()),
                        after_are.contains("in addition to"),
                    )
                } else {
                    (None, false)
                };
            let grant_keywords = if set_types.is_none() && !rest.is_empty() {
                let kw_cand = strip_prefix_any(&rest, &["and have ", "and has ", "and ", ", "])
                    .unwrap_or(rest)
                    .trim();
                parse_keyword_list(kw_cand, vocab).unwrap_or_default()
            } else {
                Vec::new()
            };
            let where_x_ok = rest
                .strip_prefix("where x is ")
                .is_some_and(|wx| parse_value_expr(wx, vocab).is_ok());
            if rest.is_empty() || set_types.is_some() || !grant_keywords.is_empty() || where_x_ok {
                return Ok(st_cont_layer(
                    subj,
                    set_types,
                    additive_types,
                    Some((p, t)),
                    None,
                    grant_keywords,
                    Vec::new(),
                    false,
                ));
            }
        }
    }

    // 7d. Keyword grant with derived cost ("Each <filter> has <kw>. Its/The <kw> cost is equal to <rule>.")
    // and single-sentence "Each <filter> has <kw> {X}, where X is <val>" (Sliver Gravemother)
    if let Some((before_wx, wx_raw)) = low.split_once(", where x is ")
        && let Some((subj_raw, kw_part)) = split_once_any(&before_wx, &[" has ", " have "])
        && let Some(kw_name) = kw_part.strip_suffix(" {x}")
        && !kw_name.contains(' ')
        && let (Ok(subj), Ok(_val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(wx_raw, vocab),
        )
    {
        return Ok(st_s(StaticAbilityAst::GrantKeywordWithCost {
            subject: Box::new(subj),
            keyword: kw_name.to_string(),
            cost_rule: wx_raw.to_string(),
            condition: None,
        }));
    }
    // (Dream Devourer, The Cave of Skulls, Varolz, Underworld Breach)
    if let Some((first_sent, second_sent)) = low.split_once(". ")
        && (second_sent.starts_with("its ") || second_sent.starts_with("the "))
        && let Some((_, cost_rule)) = second_sent.split_once(" cost is equal to ")
        && let Some((subj_raw, kw_raw)) = split_once_any(&first_sent, &[" has ", " have "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kw = kw_raw.trim();
        if !kw.is_empty() && !kw.contains(' ') {
            return Ok(st_s(StaticAbilityAst::GrantKeywordWithCost {
                subject: Box::new(subj),
                keyword: kw.to_string(),
                cost_rule: cost_rule.trim().to_string(),
                condition: None,
            }));
        }
    }

    // 7e. Standalone "<subj> loses/lose all abilities", "<subj> are/is every basic land/creature type ...",
    // "<subj> are/is <types> in addition to their/its other [colors and] types",
    // "<subj> doesn't/don't untap during its/their controller(s)' untap step(s)",
    // "its/their activated abilities can't be activated"
    if let Some(subj_raw) = strip_suffix_any(&low, &[" loses all abilities", " lose all abilities"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_cont_layer(
            subj,
            None,
            false,
            None,
            None,
            Vec::new(),
            Vec::new(),
            true,
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" are ").or_else(|| low.split_once(" is "))
        && (rhs.ends_with(" in addition to their other types")
            || rhs.ends_with(" in addition to its other types")
            || rhs.ends_with(" in addition to their other colors and types")
            || rhs.ends_with(" in addition to its other colors and types")
            || rhs == "every basic land type"
            || rhs == "every creature type")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let ty_clean = rhs
            .split_once(" in addition to ")
            .map_or(rhs, |(t, _)| t)
            .trim();
        return Ok(st_cont_layer(
            subj,
            Some(ty_clean.to_string()),
            rhs.contains("in addition to"),
            None,
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    if let Some(subj_raw) = low
        .strip_suffix(" doesn't untap during its controller's untap step")
        .or_else(|| low.strip_suffix(" don't untap during their controllers' untap steps"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "DoesntUntap"));
    }
    if low == "its activated abilities can't be activated"
        || low == "their activated abilities can't be activated"
        || low == "its activated abilities can't be activated unless they're mana abilities"
        || low == "their activated abilities can't be activated unless they're mana abilities"
        || low == "you may activate ~'s loyalty abilities any time you could cast an instant"
    {
        return Ok(st_rule_subj(
            anaphora("it"),
            format!("AbilityTimingOrLock:{low}"),
        ));
    }

    // 8. CDA P/T (including Tarmogoyf / Nethergoyf / Souls of the Lost "power is X and toughness is that number plus N")
    if let Some(rest) = low.strip_prefix("~'s power is equal to ")
        && let Some((val_raw, plus_raw)) =
            rest.split_once(" and its toughness is equal to that number plus ")
        && let (Ok(val), Some(n)) = (
            parse_value_expr(val_raw, vocab),
            parse_number_word(plus_raw.trim()),
        )
    {
        return Ok(st_s(StaticAbilityAst::CdaPowerToughness {
            power: Some(Box::new(val.clone())),
            toughness: Some(Box::new(ValueExprAst::Add(Box::new(val), vconst(n)))),
        }));
    }
    // General asymmetric CDA: "~'s power is equal to <v1> and its toughness is equal to <v2>" (Shapeshifter, Sutured Ghoul)
    if let Some(rest) = low.strip_prefix("~'s power is equal to ")
        && let Some((p_raw, t_raw)) = rest.split_once(" and its toughness is equal to ")
        && let (Ok(pv), Ok(tv)) = (
            parse_value_expr(p_raw, vocab),
            parse_value_expr(t_raw, vocab),
        )
    {
        return Ok(st_s(StaticAbilityAst::CdaPowerToughness {
            power: Some(Box::new(pv)),
            toughness: Some(Box::new(tv)),
        }));
    }
    if let Some(val_raw) = low.strip_prefix("~'s power and toughness are each equal to ") {
        let val = parse_value_expr(val_raw, vocab)?;
        return Ok(st_s(StaticAbilityAst::CdaPowerToughness {
            power: Some(Box::new(val.clone())),
            toughness: Some(Box::new(val)),
        }));
    }
    if let Some(val_raw) = low.strip_prefix("~'s power is equal to ") {
        let val = parse_value_expr(val_raw, vocab)?;
        return Ok(st_s(StaticAbilityAst::CdaPowerToughness {
            power: Some(Box::new(val)),
            toughness: None,
        }));
    }
    if let Some(val_raw) = low.strip_prefix("~'s toughness is equal to ") {
        let val = parse_value_expr(val_raw, vocab)?;
        return Ok(st_s(StaticAbilityAst::CdaPowerToughness {
            power: None,
            toughness: Some(Box::new(val)),
        }));
    }

    // 9. Cost modifiers + Strive + Alternative cost + Flash-sac + Flashback reduction + Additional cost + Cast permissions
    if low.starts_with("you may cast ~ as though it had flash. if you cast it any time a sorcery couldn't have been cast,") {
        return Ok(st_rule_subj(SubjectRefAst::SelfObject, "FlashSacrificeAtCleanup"));
    }
    if let Some(after_fb) = s.strip_prefix("Flashback ")
        && let Some((fb_cost, rest)) = after_fb.split_once(". ~ costs ")
        && let Some((delta_tok, _)) = rest.split_once(" less to cast this way")
        && is_mana_sequence(fb_cost.trim())
        && is_mana_sequence(delta_tok.trim())
    {
        return Ok(st_s(StaticAbilityAst::FlashbackCostModifier {
            cost: fb_cost.trim().to_string(),
            delta: delta_tok.trim().to_string(),
        }));
    }
    if let Some(after) = strip_prefix_any(&s, &["You may ", "you may "]) {
        if let Some((cost_raw, spells_raw)) =
            after.split_once(" rather than pay the mana cost for ")
        {
            let cost_clean = strip_prefix_any(&cost_raw, &["pay ", "Pay "]).unwrap_or(cost_raw);
            if let (Ok(cost), Ok(spells_flt)) = (
                parse_cost_list(cost_clean, vocab),
                parse_object_filter(spells_raw.trim(), vocab),
            ) {
                return Ok(st_alt_cast(cost, Some(spells_flt)));
            }
        } else if let Some((cost_raw, _)) = after
            .split_once(" rather than pay ~'s mana cost")
            .or_else(|| after.split_once(" rather than paying ~'s mana cost"))
        {
            let cost_clean = strip_prefix_any(&cost_raw, &["pay ", "Pay "]).unwrap_or(cost_raw);
            if let Ok(cost) = parse_cost_list(cost_clean, vocab) {
                return Ok(st_alt_cast(cost, None));
            }
        }
    }
    // Activated ability & keyword-cost modifier:
    // - "Abilities ... cost {M} more/less to activate [for each ... | if ... | , where X is ...]"
    // - "Flashback/Buyback/Dash/Equip/Blitz costs [you/your opponents pay] cost {M} less/more [for each ...]"
    if let Some((idx, vlen)) = low
        .find(" cost ")
        .map(|i| (i, 6))
        .or_else(|| low.find(" costs ").map(|i| (i, 7)))
        && let Some((delta_tok, tail)) = s[idx + vlen..].trim().split_once(' ')
        && is_mana_sequence(delta_tok)
    {
        let lhs_low = low[..idx].trim();
        let tlow = tail.to_ascii_lowercase();
        let is_ability_cost = (lhs_low.contains("abilities") || lhs_low.contains("ability"))
            && (tlow.starts_with("more to activate") || tlow.starts_with("less to activate"));
        let is_kw_cost = (lhs_low.ends_with(" costs")
            || lhs_low.ends_with(" costs you pay")
            || lhs_low.ends_with(" costs your opponents pay")
            || lhs_low.ends_with(" costs opponents pay"))
            && (tlow == "less"
                || tlow == "more"
                || tlow.starts_with("less ")
                || tlow.starts_with("more "));
        if is_ability_cost || is_kw_cost {
            let less = tlow.starts_with("less");
            // Preserve CR 118.7a minimum-mana floor sentence rather than swallowing it:
            // "This effect can't reduce the mana in that [ability's activation] cost to less than one mana"
            let (tlow_core, min_mana_floor) = strip_min_mana_floor_sentence(&tlow);
            let scale_expr = tlow_core
                .split_once(" for each ")
                .and_then(|(_, fe)| parse_for_each_value_expr(fe.trim(), vocab).ok())
                .or_else(|| {
                    tlow_core
                        .split_once(", where x is ")
                        .and_then(|(_, xe)| parse_value_expr(xe.trim(), vocab).ok())
                })
                .map(Box::new);
            let condition = tlow_core
                .split_once(" if ")
                .and_then(|(_, c)| parse_condition(c.trim(), vocab).ok())
                .map(Box::new);
            let subject = Box::new(SubjectRefAst::Anaphora(s[..idx].trim().to_string()));
            if let Some(min_mana) = min_mana_floor {
                return Ok(st_s(StaticAbilityAst::CostModifierWithFloor {
                    subject,
                    delta: delta_tok.to_string(),
                    less,
                    next_only: false,
                    per: None,
                    scale_expr,
                    min_mana,
                    condition,
                }));
            }
            return Ok(st_s(StaticAbilityAst::CostModifier {
                subject,
                delta: delta_tok.to_string(),
                less,
                next_only: false,
                per: None,
                scale_expr,
                condition,
            }));
        }
    }
    // Grant parameterized keyword to cards in a zone (Homing Sliver: "Each Sliver card in each player's hand has slivercycling {3}")
    if let Some((subj_raw, kw_raw)) = s.split_once(" has ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some((kw, _)) = crate::keywords::parse_keyword_token(kw_raw.trim(), vocab)
    {
        let kw_full = match kw.parameter {
            Some(p) => format!("{} {p}", kw.name),
            None => kw.name,
        };
        return Ok(st_kw(subj, vec![kw_full]));
    }
    // Abandoned Sarcophagus: "You may cast spells that have a cycling ability from your graveyard"
    if low == "you may cast spells that have a cycling ability from your graveyard" {
        let flt = ObjectFilterAst {
            any_spell: true,
            zone: Some(ZoneFilter::YourGraveyard),
            predicates: vec![FilterPredicateAst::HasAbilityKind {
                kind: "cycling".to_string(),
                negated: false,
            }],
            ..Default::default()
        };
        return Ok(st_perm(true, flt, "from your graveyard"));
    }
    if let Some(after) = s
        .strip_prefix("As an additional cost to cast ~, you may ")
        .or_else(|| s.strip_prefix("as an additional cost to cast ~, you may "))
        && let Some((cost_part, rest)) = after.split_once(". ~ costs ")
        && (rest.to_ascii_lowercase().contains("less to cast for each ")
            || rest.to_ascii_lowercase().contains("less to cast if "))
        && let Ok(ca) = parse_cost_atom(cost_part, vocab)
    {
        return Ok(st_s(StaticAbilityAst::AdditionalCastCost { cost: ca }));
    }
    if let Some(after) = s.strip_prefix("~ costs ")
        && let Some(delta) = after.strip_suffix(" more to cast for each target beyond the first")
        && is_mana_sequence(delta.trim())
    {
        return Ok(st_s(StaticAbilityAst::StrivePerExtraTarget {
            delta: delta.trim().to_string(),
        }));
    }
    if let Some((idx, vlen)) = low
        .find(" costs ")
        .map(|i| (i, 7))
        .or_else(|| low.find(" cost ").map(|i| (i, 6)))
    {
        let after = s[idx + vlen..].trim();
        if let Some((delta_tok, tail)) = after.split_once(' ')
            && is_mana_sequence(delta_tok)
        {
            let tlow = tail.to_ascii_lowercase();
            let (less, fe_opt, cond_opt, turn_cond) =
                if let Some(fe) = tlow.strip_prefix("less to cast for each ") {
                    (Some(true), Some(fe), None, None)
                } else if let Some(fe) = tlow.strip_prefix("more to cast for each ") {
                    (Some(false), Some(fe), None, None)
                } else if let Some(c) = tlow.strip_prefix("less to cast if ") {
                    (Some(true), None, Some(c), None)
                } else if let Some(c) = tlow.strip_prefix("more to cast if ") {
                    (Some(false), None, Some(c), None)
                } else if tlow == "less to cast during your turn" {
                    (Some(true), None, None, Some(ConditionAst::IsYourTurn))
                } else if tlow == "less to cast during your end step" {
                    (
                        Some(true),
                        None,
                        None,
                        Some(ConditionAst::Marker(GameMarker::DuringStep {
                            player: PlayerRefAst::You,
                            step: TurnStepAst::EndStep,
                        })),
                    )
                } else if tlow == "more to cast during your turn" {
                    (Some(false), None, None, Some(ConditionAst::IsYourTurn))
                } else if tlow == "less to cast during opponents' turns"
                    || tlow == "less to cast during an opponent's turn"
                {
                    (
                        Some(true),
                        None,
                        None,
                        Some(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn))),
                    )
                } else if tlow == "more to cast except during its controller's turn" {
                    (
                        Some(false),
                        None,
                        None,
                        Some(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn))),
                    )
                } else if let Some(wx) = tlow.strip_prefix("less to cast, where x is ")
                    && let Ok(x_val) = parse_value_expr(wx, vocab)
                {
                    let raw_left = s[..idx].trim();
                    let subj_clean = raw_left
                        .replace(" you cast from anywhere other than your hand", "")
                        .replace(" you cast but don't own", "")
                        .replace(" your opponents cast", "")
                        .replace(" opponents cast", "")
                        .replace(" enchanted player casts", "")
                        .replace(" you cast", "")
                        .replace(" players cast", "");
                    let subj_trimmed = subj_clean.trim();
                    let had_you_cast = raw_left.contains(" you cast")
                        && !raw_left.contains(" you cast from anywhere other than your hand")
                        && !raw_left.contains(" you cast but don't own");
                    let had_opp_cast = raw_left.contains(" your opponents cast")
                        || raw_left.contains(" opponents cast");
                    let subj_opt = if subj_trimmed == "~" {
                        Some(SubjectRefAst::SelfObject)
                    } else if subj_trimmed.eq_ignore_ascii_case("it") {
                        Some(anaphora("it"))
                    } else {
                        parse_object_filter(subj_trimmed, vocab).ok().map(|mut f| {
                            if f.controller.is_none() {
                                if had_you_cast {
                                    f.controller = Some(ControllerRelation::You);
                                } else if had_opp_cast {
                                    f.controller = Some(ControllerRelation::Opponents);
                                }
                            }
                            grp(f)
                        })
                    };
                    if let Some(subj) = subj_opt {
                        return Ok(st_cost_mod(
                            subj,
                            delta_tok.to_string(),
                            true,
                            false,
                            None,
                            Some(x_val),
                            None,
                        ));
                    }
                    (None, None, None, None)
                } else if tlow == "less to cast" {
                    (Some(true), None, None, None)
                } else if tlow == "more to cast" {
                    (Some(false), None, None, None)
                } else {
                    (None, None, None, None)
                };
            if let Some(is_less) = less {
                let raw_left = s[..idx].trim();
                let (subj_part, target_clause) = match raw_left.split_once(" that target ") {
                    Some((l, t)) => (l.trim(), Some(t.trim())),
                    None => (raw_left, None),
                };
                let sp_low = subj_part.to_ascii_lowercase();
                let cast_outside_hand =
                    sp_low.contains(" you cast from anywhere other than your hand");
                let cast_not_owned = sp_low.contains(" you cast but don't own");
                let cast_by_opps =
                    sp_low.contains(" your opponents cast") || sp_low.contains(" opponents cast");
                let cast_by_opp = sp_low.contains(" an opponent casts");
                let cast_by_ench = sp_low.contains(" enchanted player casts");
                let cast_by_you =
                    !cast_outside_hand && !cast_not_owned && sp_low.contains(" you cast");
                let subj_raw = subj_part
                    .replace(" you cast from anywhere other than your hand", "")
                    .replace(" you cast but don't own", "")
                    .replace(" your opponents cast", "")
                    .replace(" opponents cast", "")
                    .replace(" an opponent casts", "")
                    .replace(" enchanted player casts", "")
                    .replace(" you cast", "")
                    .replace(" players cast", "");
                let subj_trimmed = subj_raw.trim().trim_end_matches(" each turn").trim();
                let nth_spell = if subj_trimmed.to_ascii_lowercase().starts_with("the first ") {
                    Some(1u8)
                } else if subj_trimmed.to_ascii_lowercase().starts_with("the second ") {
                    Some(2u8)
                } else {
                    None
                };
                let subj_clean = strip_prefix_any(
                    &subj_trimmed,
                    &[
                        "The first ",
                        "the first ",
                        "The second ",
                        "the second ",
                        "Each ",
                        "each ",
                    ],
                )
                .unwrap_or(subj_trimmed);
                let parsed_target_ok = match target_clause {
                    Some(t) => parse_subject_or_target(t, vocab).ok().map(Some),
                    None => Some(None),
                };
                let subj_opt = if let Some(parsed_target) = parsed_target_ok {
                    if subj_clean == "~" {
                        Some(SubjectRefAst::SelfObject)
                    } else if subj_clean.eq_ignore_ascii_case("it")
                        || subj_clean.eq_ignore_ascii_case("it also")
                    {
                        Some(anaphora("it"))
                    } else {
                        parse_object_filter(subj_clean, vocab).ok().map(|mut f| {
                            if f.controller.is_none() {
                                if cast_by_you || cast_outside_hand || cast_not_owned {
                                    f.controller = Some(ControllerRelation::You);
                                } else if cast_by_opps {
                                    f.controller = Some(ControllerRelation::Opponents);
                                } else if cast_by_opp {
                                    f.controller = Some(ControllerRelation::Opponent);
                                } else if cast_by_ench {
                                    f.controller = Some(ControllerRelation::ThatPlayer);
                                }
                            }
                            if cast_not_owned {
                                f.predicates.push(FilterPredicateAst::NotOwnedByController);
                            }
                            if cast_outside_hand {
                                f.predicates.push(FilterPredicateAst::StateQualification(
                                    "cast from outside hand".to_string(),
                                ));
                            }
                            if let Some(n) = nth_spell {
                                f.predicates
                                    .push(FilterPredicateAst::NthSpellCastThisTurn(n));
                            }
                            if let Some(t_subj) = parsed_target {
                                f.predicates
                                    .push(FilterPredicateAst::TargetsSubject(Box::new(t_subj)));
                            }
                            grp(f)
                        })
                    }
                } else {
                    None
                };
                let per_scale_opt = match fe_opt {
                    Some("basic land type among lands you control") => {
                        Some((None, Some(Box::new(ValueExprAst::DomainCount))))
                    }
                    Some(fe) => {
                        if let Ok(flt) = parse_object_filter(fe, vocab) {
                            Some((Some(Box::new(flt)), None))
                        } else {
                            parse_for_each_value_expr(fe, vocab)
                                .ok()
                                .filter(|v| !matches!(v, ValueExprAst::PriorResult))
                                .map(|val| (None, Some(Box::new(val))))
                        }
                    }
                    None => Some((None, None)),
                };
                let cond_res = match (cond_opt, turn_cond) {
                    (Some(c), _) => parse_condition(c, vocab)
                        .ok()
                        .map(|cond| Some(Box::new(cond))),
                    (None, Some(tc)) => Some(Some(Box::new(tc))),
                    (None, None) => Some(None),
                };
                if let (Some(subj), Some((per, scale_expr)), Some(condition)) =
                    (subj_opt, per_scale_opt, cond_res)
                {
                    return Ok(st_s(StaticAbilityAst::CostModifier {
                        subject: Box::new(subj),
                        delta: delta_tok.to_string(),
                        less: is_less,
                        next_only: false,
                        per,
                        scale_expr,
                        condition,
                    }));
                }
            }
        }
    }

    if let Some(after) = strip_prefix_any(
        &s,
        &[
            "As an additional cost to cast ",
            "as an additional cost to cast ",
        ],
    ) && let Some((spell_part, cost_raw)) = after.split_once(", ")
        && (spell_part == "~" || parse_object_filter(spell_part, vocab).is_ok())
    {
        let cost_clean = strip_prefix_any(&cost_raw, &["you may ", "You may "]).unwrap_or(cost_raw);
        let cost_first_sent = cost_clean.split_once(". ").map_or(cost_clean, |(a, _)| a);
        if let Ok(ca) = parse_cost_atom(cost_first_sent, vocab) {
            return Ok(st_s(StaticAbilityAst::AdditionalCastCost { cost: ca }));
        }
        if let Ok(list) = parse_cost_list(cost_first_sent, vocab)
            && let Some(first) = list.into_iter().next()
        {
            return Ok(st_s(StaticAbilityAst::AdditionalCastCost { cost: first }));
        }
        if cost_first_sent.to_ascii_lowercase().starts_with("choose ")
            && parse_atomic_effect(cost_first_sent, vocab).is_ok()
        {
            return Ok(st_s(StaticAbilityAst::AdditionalCastCost {
                cost: CostAtomAst::Choose(Box::new(SubjectRefAst::Anaphora(
                    cost_first_sent.to_string(),
                ))),
            }));
        }
    }

    for (pref, cast_only) in [("you may cast ", true), ("you may play ", false)] {
        if let Some(rest) = low.strip_prefix(pref) {
            if rest == "~ as though it had flash" {
                return Ok(st_perm(cast_only, any_card_flt(), "as though it had flash"));
            }
            // Conditional flash / graveyard cast: "You may cast ~ as though it had flash if <cond>"
            if let Some(cond_raw) = rest.strip_prefix("~ as though it had flash if ") {
                let cond = if cond_raw.starts_with("you pay ") {
                    let pay_rest = cond_raw.strip_prefix("you ").unwrap_or(cond_raw);
                    let cost_trimmed =
                        strip_suffix_any(&pay_rest, &[" more to cast it", " more to cast ~"])
                            .unwrap_or(pay_rest)
                            .trim();
                    parse_cost_atom(cost_trimmed, vocab)
                        .map(|c| ConditionAst::Marker(GameMarker::PayCostCondition(Box::new(c))))
                } else {
                    parse_condition(cond_raw, vocab)
                };
                if let Ok(c) = cond {
                    return Ok(st_s(StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter: Box::new(any_card_flt()),
                        permission: "as though it had flash".to_string(),
                        condition: Some(Box::new(c)),
                    }));
                }
            }
            if let Some(cond_raw) = rest.strip_prefix("~ from your graveyard if ")
                && let Ok(c) = parse_condition(cond_raw, vocab)
            {
                return Ok(st_s(StaticAbilityAst::PlayOrCastPermission {
                    cast_only,
                    filter: Box::new(any_card_flt()),
                    permission: "from your graveyard".to_string(),
                    condition: Some(Box::new(c)),
                }));
            }
            if let Some(after_gy) = rest.strip_prefix("~ from your graveyard ")
                && (after_gy.starts_with("by ") || after_gy.starts_with("using its "))
            {
                return Ok(st_perm(
                    cast_only,
                    any_card_flt(),
                    format!("from your graveyard {after_gy}"),
                ));
            }
            // Broad play/cast from zone permissions:
            // "you may play the top card of your library"
            // "you may play lands and cast spells from <zone>"
            // "you may play a <flt> land or cast a <flt> spell from <zone> [without paying ...]"
            // "you may cast <flt> from <zone> [by <cost> | without paying its mana cost | if <cond>]"
            if rest == "the top card of your library" {
                return Ok(st_perm(
                    cast_only,
                    any_card_flt(),
                    "from the top of your library",
                ));
            }
            if rest.contains("exiled with ")
                && let Ok(flt) = parse_object_filter(rest, vocab)
            {
                return Ok(st_perm(cast_only, flt, "from exile"));
            }
            if let Some(zone_part) = strip_prefix_any(
                &rest,
                &[
                    "lands and cast spells from ",
                    "a land or cast a spell from ",
                ],
            ) {
                return Ok(st_perm(false, any_card_flt(), format!("from {zone_part}")));
            }
            if let Some((left_land, right_cast)) = rest.split_once(" land or cast ")
                && let Some((flt_raw, zone_rest)) = right_cast.split_once(" from ")
                && let (Ok(land_flt), Ok(spell_flt)) = (
                    parse_object_filter(&format!("{} land", left_land.trim()), vocab),
                    parse_object_filter(flt_raw, vocab),
                )
            {
                let perm = format!("from {zone_rest}");
                return Ok(st_s(StaticAbilityAst::Compound(vec![
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only: false,
                        filter: Box::new(land_flt),
                        permission: perm.clone(),
                        condition: None,
                    },
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only: true,
                        filter: Box::new(spell_flt),
                        permission: perm,
                        condition: None,
                    },
                ])));
            }
            if let Some((flt_raw, zone_rest)) = rest
                .strip_prefix("lands and cast ")
                .and_then(|r| r.split_once(" from "))
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(st_perm(false, flt, format!("from {zone_rest}")));
            }
            if let Some((flt_raw, zone_rest)) = rest.split_once(" from ") {
                let (zone_part, cond_opt) = match zone_rest.split_once(" if ") {
                    Some((z, c)) => (z.trim(), parse_condition(c, vocab).ok()),
                    None => (zone_rest.trim(), None),
                };
                if zone_part.starts_with("your graveyard")
                    || zone_part.starts_with("the top of your graveyard")
                    || zone_part.starts_with("the top of your library")
                    || zone_part.starts_with("your hand")
                    || zone_part.starts_with("exile")
                    || zone_part.starts_with("among ")
                {
                    let flt_opt = if flt_raw == "~" {
                        Some(any_card_flt())
                    } else {
                        parse_object_filter(flt_raw, vocab).ok()
                    };
                    if let Some(flt) = flt_opt
                        && (!zone_rest.contains(" if ") || cond_opt.is_some())
                    {
                        return Ok(st_s(StaticAbilityAst::PlayOrCastPermission {
                            cast_only,
                            filter: Box::new(flt),
                            permission: format!("from {zone_part}"),
                            condition: cond_opt.map(Box::new),
                        }));
                    }
                }
            }
            for perm in [
                "as though they had flash",
                "from your graveyard",
                "from the top of your library",
                "from exile",
                "from among cards exiled with ~",
                "from your hand without paying their mana costs",
                "without paying their mana costs",
            ] {
                if let Some(flt_raw) = rest.strip_suffix(&format!(" {perm}")) {
                    let flt = if flt_raw == "~" {
                        any_card_flt()
                    } else if let Ok(f) = parse_object_filter(flt_raw, vocab) {
                        f
                    } else {
                        continue;
                    };
                    return Ok(st_perm(cast_only, flt, perm.to_string()));
                }
            }
        }
    }

    // 10. Global rules, deck construction, visibility, combat restrictions, damage prevention, type change
    if low.starts_with("a deck can have any number of cards named ")
        || low.starts_with("companion \u{2014} ")
        || low == "draft ~ face up"
    {
        return Ok(st_s(StaticAbilityAst::DeckConstructionRule(s.to_string())));
    }
    const SKIP_STEPS: &[(&str, PlayerRefAst, TurnStepAst)] = &[
        (
            "skip your draw step",
            PlayerRefAst::You,
            TurnStepAst::DrawStep,
        ),
        (
            "skip your untap step",
            PlayerRefAst::You,
            TurnStepAst::UntapStep,
        ),
        (
            "players skip their untap steps",
            PlayerRefAst::EachPlayer,
            TurnStepAst::UntapStep,
        ),
        (
            "players skip their upkeep steps",
            PlayerRefAst::EachPlayer,
            TurnStepAst::Upkeep,
        ),
    ];
    if let Some(&(_, player, step)) = SKIP_STEPS.iter().find(|&&(k, _, _)| k == low) {
        return Ok(st_s(StaticAbilityAst::SkipStep {
            player,
            step,
            condition: None,
        }));
    }
    if let Some(noun) = low.strip_prefix("you control enchanted ")
        && matches!(
            noun,
            "creature"
                | "permanent"
                | "artifact"
                | "artifact creature"
                | "land"
                | "equipment"
                | "enchantment"
        )
    {
        return Ok(st_s(StaticAbilityAst::ControlAttachedHost));
    }
    match low.as_str() {
        "~ can't be countered" => return Ok(st_rule("CantBeCountered")),
        "you may look at the top card of your library any time" => {
            return Ok(st_rule("look_top_library_any_time"));
        }
        "play with the top card of your library revealed"
        | "players play with the top card of their libraries revealed" => {
            let rule = if low.starts_with("players ") {
                "all_players_reveal_library_top"
            } else {
                "play_with_top_revealed"
            };
            return Ok(st_s(StaticAbilityAst::LookOrRevealRule {
                rule: rule.to_string(),
                filter: None,
                follow_up: Vec::new(),
                condition: None,
            }));
        }
        "you may have ~ assign its combat damage as though it weren't blocked" => {
            return Ok(st_rule_subj(
                SubjectRefAst::SelfObject,
                "assign_combat_damage_unblocked",
            ));
        }
        "untap all permanents you control during each opponent's untap step" => {
            return Ok(st_rule_subj(
                SubjectRefAst::All(Box::new(ObjectFilterAst {
                    any_permanent: true,
                    controller: Some(ControllerRelation::You),
                    ..Default::default()
                })),
                "UntapEachOtherUntapStep",
            ));
        }
        "you have no maximum hand size"
        | "players have no maximum hand size"
        | "you may play an additional land on each of your turns"
        | "you may play lands and cast spells from the top of your library"
        | "you may choose not to untap ~ during your untap step"
        | "~ can be your commander"
        | "you may look at the top card of their library any time"
        | "players can't gain life"
        | "your opponents can't gain life"
        | "opponents can't gain life"
        | "damage can't be prevented"
        | "creatures can't be regenerated"
        | "your opponents can't cast spells during your turn"
        | "players can't cast spells during your turn"
        | "you may spend mana as though it were mana of any color"
        | "you may spend mana as though it were mana of any type"
        | "each player can't cast more than one spell each turn"
        | "players can't cast more than one spell each turn"
        | "your opponents can't cast more than one spell each turn"
        | "you can't cast more than one spell each turn"
        | "each player can't draw more than one card each turn"
        | "players can't draw more than one card each turn"
        | "players can't untap more than two permanents during their untap steps"
        | "players can't untap more than one permanent during their untap steps"
        | "players can't untap more than one land during their untap steps"
        | "no more than one creature can attack each combat"
        | "no more than one creature can block each combat"
        | "players play with their hands revealed"
        | "your opponents play with their hands revealed"
        | "while voting, you may vote an additional time"
        | "you don't lose the game for having 0 or less life"
        | "you can't lose the game and your opponents can't win the game"
        | "you may exert ~ as it attacks"
        | "~ is all colors"
        | "this ability doesn't affect its color identity"
        | "activated abilities of sources with the chosen name can't be activated unless they're mana abilities"
        | "players can't cycle cards"
        | "your life total can't change"
        | "players can't search libraries"
        | "your opponents can't search libraries"
        | "you can't spend mana to cast ~"
        | "enchanted player can't gain life"
        | "that player can't gain life for the rest of the game"
        | "combat damage can't be prevented"
        | "damage that would be dealt by ~ can't be prevented"
        | "lands can't enter the battlefield"
        | "~ can't attack during extra turns"
        | "~ can't attack if it attacked during your last turn"
        | "players can't cast spells during combat"
        | "your opponents can't cast spells during combat"
        | "you can't play lands or cast spells from your hand"
        | "enchanted creature can't attack, block, or transform"
        | "each opponent can't draw more than one card each turn"
        | "while voting, you get an additional vote"
        | "you can't lose the game"
        | "~ is the chosen type"
        | "enchanted land is the chosen type"
        | "~ is every creature type"
        | "~ is every nonbasic land type"
        | "~ isn't legendary if it's a token"
        | "you may look at cards exiled with ~"
        | "all walls able to block ~ do so"
        | "all morph costs cost {2} more"
        | "unlock costs you pay cost {1} less"
        | "players can't draw cards or gain life"
        | "damage isn't removed from ~ during cleanup steps"
        | "all lands lose all abilities except mana abilities"
        | "you gain life rather than lose life from radiation"
        | "players can cast spells only during their own turns"
        | "all damage is dealt as though its source had wither"
        | "any player may cast spells as though they had flash"
        | "you may discard ~ any time you could cast an instant"
        | "enchanted creature can attack as though it had haste"
        | "no more than one creature can attack you each combat"
        | "the chosen player reveals each card they draw"
        | "the chosen player's maximum hand size is four"
        | "enchanted creature has landwalk of the chosen type"
        | "you may spend white mana as though it were red mana"
        | "creatures played by your opponents enter tapped"
        | "all creatures with flying able to block ~ do so"
        | "double all damage equipped creature would deal"
        | "humans you control have each of the chosen abilities"
        | "skip your upkeep step if you have no cards in hand"
        | "players can't activate planeswalkers' loyalty abilities"
        | "enchanted creature can't attack, block, or crew vehicles"
        | "~ can't attack a player it has already attacked this turn"
        | "damage doesn't cause you to lose life"
        | "it must be blocked if able"
        | "mana abilities of ~ cost an additional 1 life to activate"
        | "counters remain on ~ as it moves to any zone other than a player's hand or library"
        | "you can cast spells only during your turn and you can cast no more than two spells each turn"
        | "all creatures become black and all lands become swamps"
        | "you may tap lands you don't control for mana"
        | "you may activate the loyalty abilities of ~ twice each turn rather than only once"
        | "you may activate equip abilities any time you could cast an instant"
        | "you may spend mana as though it were mana of any color to pay the activation costs of ~'s abilities"
        | "you may look at face-down creatures you don't control any time"
        | "you may look at the top card of your library and at face-down creatures you don't control any time"
        | "you may play cards exiled with ~, and you may spend mana as though it were mana of any color to cast those spells"
        | "you may have ~ assign his combat damage as though he weren't blocked"
        | "you may have ~ assign her combat damage as though she weren't blocked"
        | "if a creature you control attacks, ~ also attacks if able"
        | "each player may play an additional land during each of their turns"
        | "each player may play an additional land on each of their turns"
        | "each player may attack only the nearest opponent in the chosen direction and planeswalkers controlled by that opponent"
        | "each player may attack only the nearest opponent in the last chosen direction and planeswalkers controlled by that opponent"
        | "you can't win the game and your opponents can't lose the game"
        | "you may cast the exiled card, and mana of any type can be spent to cast that spell"
        | "you may activate exhaust abilities as though they haven't been activated"
        | "its activated abilities can't be activated unless they're mana abilities"
        | "plotting cards from your hand costs {2} less"
        | "you may pay {0} rather than pay the equip cost of the first equip ability you activate during each of your turns"
        | "~ and enchanted creature each get +1/+1 for each creature you control and +1/+1 for each aura you control"
        | "each opponent can cast spells only any time they could cast a sorcery"
        | "you can't cast ~ during your first, second, or third turns of the game"
        | "you can't choose an untapped creature as ~'s target as you cast it"
        | "if ~ is your commander, choose a color before the game begins. ~ is the chosen color" => {
            return Ok(st_rule(low));
        }
        _ => {}
    }

    // "You / Each player may play <N> additional lands on/during each of your/their turns"
    if let Some(cnt_raw) = low.strip_prefix("you may play ").and_then(|r| {
        r.strip_suffix(" additional lands on each of your turns")
            .or_else(|| r.strip_suffix(" additional lands during each of your turns"))
    }) && parse_number_word(cnt_raw).is_some()
    {
        return Ok(st_rule_subj(SubjectRefAst::Player(PlayerRefAst::You), low));
    }
    // "You can/may spend mana [as though it were mana] of any type/color to cast <flt>" (Vizier of the Menagerie, Case File Auditor)
    if let Some(flt_raw) = strip_prefix_any(
        &low,
        &[
            "you can spend mana of any type to cast ",
            "you may spend mana of any type to cast ",
            "you can spend mana of any color to cast ",
            "you may spend mana of any color to cast ",
        ],
    )
    .or_else(|| low.strip_prefix("you may spend mana as though it were mana of any color to cast "))
    .or_else(|| low.strip_prefix("you may spend mana as though it were mana of any type to cast "))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_subj_flt(
            SubjectRefAst::Player(PlayerRefAst::You),
            "SpendManaAnyTypeForFilter",
            flt,
        ));
    }
    // "<player> can't cast more than <N> spell(s) each turn" / "<player> can't untap more than <N> <flt> during ..."
    if let Some((who_raw, rest)) = low.split_once(" can't cast more than ")
        && (rest.ends_with(" spell each turn") || rest.ends_with(" spells each turn"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(st_rule_subj(SubjectRefAst::Player(player), low));
    }
    if let Some((who_raw, rest)) = low.split_once(" can't untap more than ")
        && rest.contains(" during ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(st_rule_subj(SubjectRefAst::Player(player), low));
    }
    // "you may pay {M} to cast ~" (Asmoranomardicadaistinaculdacar)
    if let Some(cost_raw) = strip_prefix_any(&s, &["you may pay ", "You may pay "])
        && let Some(mana_str) = cost_raw.strip_suffix(" to cast ~")
        && is_mana_sequence(mana_str.trim())
    {
        return Ok(st_alt_cast(
            vec![CostAtomAst::PayMana(mana_str.trim().to_string())],
            None,
        ));
    }

    // "Spend only mana produced by <flt> to cast ~" (Myr Superion, Imperiosaur)
    if let Some(flt_raw) = low
        .strip_prefix("spend only mana produced by ")
        .and_then(|r| r.strip_suffix(" to cast ~"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_s(StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(self_obj()),
            rule: "SpendOnlyManaFrom".to_string(),
            filter: Some(Box::new(flt)),
            condition: None,
        }));
    }
    // "<subj> [saddles Mounts and] crews Vehicles / saddles Mounts as though its/their power were <val> greater"
    // (Hotshot Mechanic, Deathless Pilot, Pilot tokens, and future crew/saddle power bonus cards)
    for (phrase, crews_vehicles, saddles_mounts) in [
        (" saddles mounts and crews vehicles as though ", true, true),
        (" crews vehicles and saddles mounts as though ", true, true),
        (" crews vehicles as though ", true, false),
        (" saddles mounts as though ", false, true),
    ] {
        if let Some((subj_raw, rest)) = low.split_once(phrase)
            && let Some(val_raw) =
                strip_prefix_any(&rest, &["its power were ", "their power were "])
                    .and_then(|r| r.strip_suffix(" greater"))
            && let (Ok(subj), Ok(bonus)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return Ok(st_s(StaticAbilityAst::CrewOrSaddlePowerBonus {
                subject: Box::new(subj),
                crews_vehicles,
                saddles_mounts,
                bonus: Box::new(bonus),
            }));
        }
    }
    // "As ~ transforms into ~, <effect>" (Zenos, Curse of Leeches)
    if let Some(after) = low.strip_prefix("as ~ transforms into ~, ")
        && let Ok(body) = parse_atomic_effect(after, vocab)
    {
        return Ok(st_r(ReplacementEffectAst::AsTransformsAction(body)));
    }

    // "<subj> can't attack you" / "<subj> can't transform" / "<subj> can't enter the battlefield"
    for (suf, rule_name) in [
        (" can't attack you", "CantAttackYou"),
        (" can't transform", "CantTransform"),
        (" can't enter the battlefield", "CantEnterBattlefield"),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(st_rule_subj(subj, rule_name.to_string()));
        }
    }
    // "Activated abilities of <flt> can't be activated unless they're mana abilities"
    if let Some(flt_raw) = low
        .strip_prefix("activated abilities of ")
        .and_then(|r| r.strip_suffix(" can't be activated unless they're mana abilities"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "NonManaActivatedAbilitiesCantBeActivated"));
    }
    // "<subj> has/have <kws> during your turn"
    if let Some(before_turn) = strip_suffix_any(&s, &[" during your turn", " during your turn."])
        && let Ok(mut inner) = parse_static_or_replacement(before_turn, vocab)
    {
        attach_condition(&mut inner, ConditionAst::IsYourTurn);
        return Ok(inner);
    }

    // "<filter> enter(s) untapped" (Spelunking, Archelos, etc.)
    if let Some(subj_raw) = low
        .strip_suffix(" enter untapped")
        .or_else(|| low.strip_suffix(" enters untapped"))
        .or_else(|| low.strip_suffix(" enter the battlefield untapped"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "EntersUntapped"));
    }
    // "<filter> can't be cast" (Nevermore, Meddling Mage, Void Winnower, etc.)
    if let Some(flt_raw) = low.strip_suffix(" can't be cast")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "CantBeCast"));
    }
    // "You don't lose unspent [<color>] mana as steps and phases end" (Omnath, Kruphix, Electro, Leyline Tyrant)
    if low.starts_with("you don't lose unspent ") && low.ends_with(" mana as steps and phases end")
    {
        return Ok(st_rule_subj(SubjectRefAst::Player(PlayerRefAst::You), low));
    }

    // Orthogonal global static rules: Collector Ouphe, Torpor Orb, Illusory Angel, Grand Melee, Dragon Hunter, Storm Windrider, Pipsqueak
    if let Some(flt_raw) = low
        .strip_prefix("activated abilities of ")
        .and_then(|r| r.strip_suffix(" can't be activated"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "ActivatedAbilitiesCantBeActivated"));
    }
    if let Some(flt_raw) = low
        .strip_suffix(" entering don't cause abilities to trigger")
        .or_else(|| low.strip_suffix(" entering the battlefield don't cause abilities to trigger"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "EnterTriggersSuppressed"));
    }
    if let Some(cond_raw) = low.strip_prefix("cast ~ only if ")
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(st_s(StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(self_obj()),
            rule: "CastOnlyIf".to_string(),
            filter: None,
            condition: Some(Box::new(cond)),
        }));
    }
    if let Some(subj_raw) = strip_suffix_any(
        &low,
        &[
            " attacks each combat if able",
            " attack each combat if able",
            " blocks each combat if able",
            " block each combat if able",
            " attacks or blocks each combat if able",
            " attack or block each combat if able",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let rule = if low.contains("attacks or blocks") || low.contains("attack or block") {
            "AttacksOrBlocksEachCombatIfAble"
        } else if low.contains("block") {
            "BlocksEachCombatIfAble"
        } else {
            "AttacksEachCombatIfAble"
        };
        return Ok(st_rule_subj(subj, rule.to_string()));
    }
    // "<subj> can't block or be blocked by <filter>" (Sneaky Homunculus, etc.)
    if let Some((subj_raw, flt_raw)) = low.split_once(" can't block or be blocked by ")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(st_rule_subj_flt(subj, "CantBlockOrBeBlockedBy", flt));
    }
    // "You may activate abilities of <filter> as though those creatures had haste" (Thousand-Year Elixir)
    if let Some(rest) = low.strip_prefix("you may activate abilities of ")
        && let Some((flt_raw, _)) = rest.split_once(" as though ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "ActivateAbilitiesAsThoughHaste"));
    }
    if let Some((subj_raw, rest)) = low.split_once(" can block ")
        && let Some(flt_raw) = rest.strip_suffix(" as though it had reach")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(st_rule_subj_flt(subj, "CanBlockAsThoughReach", flt));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't attack you or block creatures you control")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "CantAttackYouOrBlockYourCreatures"));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't attack alone")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "CantAttackAlone"));
    }

    // "<player> can't cast <filter/~>" / "<player> can't play <filter>" (Gideon's Intervention, Rock Jockey, etc.)
    if let Some((who_raw, flt_raw, is_cast_only)) = low
        .split_once(" can't cast ")
        .map(|(w, f)| (w, f, true))
        .or_else(|| low.split_once(" can't play ").map(|(w, f)| (w, f, false)))
        && (flt_raw == "~" || parse_object_filter(flt_raw, vocab).is_ok())
    {
        let flt = parse_object_filter(flt_raw, vocab).unwrap_or_default();
        let who_norm = match who_raw.trim() {
            "your opponents" | "opponents" => Some(PlayerRefAst::EachOpponent),
            "players" => Some(PlayerRefAst::EachPlayer),
            other => parse_player_ref(other),
        };
        if let Some(player) = who_norm {
            let rule = if flt_raw == "~" {
                if is_cast_only {
                    "CantCastSelf".to_string()
                } else {
                    "CantPlaySelf".to_string()
                }
            } else if is_cast_only {
                "CantCast:filter".to_string()
            } else {
                "CantPlay".to_string()
            };
            return Ok(st_s(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule,
                filter: if flt_raw == "~" {
                    None
                } else {
                    Some(Box::new(flt))
                },
                condition: None,
            }));
        }
    }
    // "<subj>'s controller may have it assign its combat damage as though it weren't blocked" (Indomitable Might)
    if let Some(subj_raw) = low.strip_suffix(
        "'s controller may have it assign its combat damage as though it weren't blocked",
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "AssignDamageAsThoughUnblocked"));
    }

    if let Some((subj_raw, blocker_raw)) = low.split_once(" can't be blocked by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if blocker_raw.starts_with("more than one creature") {
            return Ok(st_rule_subj(subj, "AtMostOneBlocker"));
        }
        if let Some(disjuncts) = split_disjunctive_object_filters(blocker_raw, vocab) {
            // De Morgan: "can't be blocked by (A or B)" == "can't be blocked by A" AND "can't be blocked by B"
            let items = disjuncts
                .into_iter()
                .map(|flt| st_rule_ast(Some(subj.clone()), "CantBeBlockedBy", Some(flt)))
                .collect();
            return Ok(st_s(StaticAbilityAst::Compound(items)));
        }
        if let Ok(flt) = parse_object_filter(blocker_raw, vocab) {
            return Ok(st_rule_subj_flt(subj, "CantBeBlockedBy", flt));
        }
    }
    if let Some((blocker_raw, atk_raw)) = low.split_once(" can't block ") {
        if let (Ok(blocker), Ok(atk_flt)) = (
            parse_subject_or_target(blocker_raw, vocab),
            parse_object_filter(atk_raw, vocab),
        ) {
            return Ok(st_rule_subj_flt(blocker, "CantBlock", atk_flt));
        }
        if let (Ok(blocker_flt), Ok(atk)) = (
            parse_object_filter(blocker_raw, vocab),
            parse_subject_or_target(atk_raw, vocab),
        ) {
            return Ok(st_rule_subj_flt(atk, "CantBeBlockedBy", blocker_flt));
        }
    }
    if let Some(rest) = low.strip_prefix("all creatures able to block ")
        && let Some(subj_raw) = rest.strip_suffix(" do so")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "AllAbleBlock"));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can't be blocked except by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if rhs.ends_with(" or more creatures") {
            return Ok(st_rule_subj(subj, format!("MinBlockers:{rhs}")));
        }
        if let Some(allowed_filters) = split_disjunctive_object_filters(rhs, vocab) {
            return Ok(st_s(StaticAbilityAst::CantBeBlockedExceptByAny {
                subject: Box::new(subj),
                allowed_filters,
                condition: None,
            }));
        }
        if let Ok(flt) = parse_object_filter(rhs, vocab) {
            return Ok(st_rule_subj_flt(subj, "CantBeBlockedExceptBy", flt));
        }
    }
    if let Some((subj_raw, atk_raw)) = low.split_once(" can block only ")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(atk_raw, vocab),
        )
    {
        return Ok(st_rule_subj_flt(subj, "CanBlockOnly", flt));
    }
    if low.starts_with("creatures can't attack you")
        && low.contains(" unless their controller pays ")
    {
        return Ok(st_rule(s.to_string()));
    }
    if let Some(subj_raw) = low
        .strip_prefix("untap ")
        .and_then(|r| r.strip_suffix(" during each other player's untap step"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "UntapEachOtherUntapStep"));
    }
    // CR 508/509/614 General static permissions, combat caps, and mana-production replacements:
    // - "Any player may cast <flt> [without paying their mana costs and] as though they had flash" (Vernal Equinox, Quick Sliver, Aluren)
    // - "You may cast the first <flt> you cast each turn as though it had flash" (The Blue Spirit)
    // - "You may look at the top card of your library any time, and you may play lands [and cast <flt>] from the top of your library" (Radha, Case of the Locked Hothouse)
    // - "No more than <N> creatures can attack [you/~] each combat" (Crawlspace, The Eternal Wanderer, Caverns of Despair)
    // - "<subj> can block an additional <N> creatures each combat" / "... for each <fe>" (Watcher in the Web, Hundred-Handed One, Kemba's Legion)
    // - "If you tap a <flt> for mana, it produces <N> times as much of that mana instead" (Nyxbloom Ancient, Virtue of Strength)
    // - "If [a player taps] a <flt> [is tapped] for mana, it produces <mana> instead of any other type [and amount]" (Contamination, Hall of Gemstone, Harvest Mage, Pale Moon)
    if let Some(after_any) = low.strip_prefix("any player may cast ")
        && let Some(flt_raw) = after_any
            .strip_suffix(" without paying their mana costs and as though they had flash")
            .or_else(|| after_any.strip_suffix(" as though they had flash"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_perm(
            true,
            flt,
            if after_any.contains("without paying") {
                "any_player_free_flash".to_string()
            } else {
                "any_player_flash".to_string()
            },
        ));
    }
    if let Some(after_first) = low.strip_prefix("you may cast the first ")
        && let Some(flt_raw) =
            after_first.strip_suffix(" you cast each turn as though it had flash")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_perm(true, flt, "first_each_turn_flash"));
    }
    if let Some(after_look) =
        low.strip_prefix("you may look at the top card of your library any time, and ")
        && let Ok(StaticOrReplacement::Static(inner)) =
            parse_static_or_replacement(after_look, vocab)
    {
        return Ok(st_s(StaticAbilityAst::Compound(vec![
            st_rule_ast(None, "look_top_library_any_time", None),
            inner,
        ])));
    }
    if low.starts_with("no more than ")
        && low.ends_with(" each combat")
        && low.contains(" can attack")
    {
        return Ok(st_rule(low));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can block an additional ")
        && rhs.contains(" each combat")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let rule = if rhs.trim() == "creature each combat" || rhs.trim() == "creature" {
            "additional_block".to_string()
        } else {
            format!("block_additional:{rhs}")
        };
        return Ok(st_rule_subj(subj, rule));
    }
    if (low.starts_with("if you tap ")
        || low.starts_with("if a player taps ")
        || low.starts_with("if a ")
        || low.starts_with("lands tapped for mana produce "))
        && low.contains("for mana")
        && (low.contains(" produces ") || low.contains(" produce "))
        && low.contains(" instead")
    {
        return Ok(st_instead(
            "tap_for_mana",
            "produce_mana",
            s.to_string(),
            None,
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can't block ")
        && let Some((flt_raw, cost_raw)) = rhs.split_once(" unless you pay ")
        && let (Ok(subj), Ok(flt), Ok(_cost)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
            parse_cost_atom(&format!("pay {cost_raw}"), vocab),
        )
    {
        return Ok(st_rule_subj_flt(
            subj,
            format!("CantBlockUnlessPay:{cost_raw}"),
            flt,
        ));
    }
    // CR 611/614/509 Hatebear, zone-cast lock, payment lock, and block-as-though statics:
    // - "Players can't cast [<flt>] spells from <z1> or <z2>" (Grafdigger's Cage, Weathered Runestone, Soulless Jailer)
    // - "Players can't pay life or sacrifice <flt> to cast spells or activate abilities" (Angel of Jubilation, Yasharn)
    // - "Spells and abilities your opponents control can't cause ..." (Tamiyo, Ashiok, Tajuru Preserver, Sigarda)
    // - "Players can't play lands as long as <cond>" (Limited Resources)
    // - "<subj> can block creatures with <kw> as though they didn't have <kw>" (Aetherflame Wall)
    // - "You may cast ~ from your graveyard if <cond>. If you do, <effect>" (Undead Sprinter)
    // - "As ~ is turned face up, <effect>" (Gift of Doom)
    // - "If a source would deal damage to a player, you may prevent X of that damage, where X is <val>" (Battletide Alchemist)
    if (low.starts_with("players can't cast ") && low.contains(" from "))
        || (low.starts_with("players can't pay life or sacrifice ")
            && low.ends_with(" to cast spells or activate abilities"))
        || low.starts_with("spells and abilities your opponents control can't cause ")
    {
        return Ok(st_rule(low));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can block creatures with ")
        && rhs.contains(" as though ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, format!("can_block_with:{rhs}")));
    }
    if low.starts_with("as ~ is turned face up, ") {
        let body_orig = &s["as ~ is turned face up, ".len()..];
        if let Ok(ins) = parse_atomic_effect(body_orig, vocab) {
            return Ok(st_r(ReplacementEffectAst::AsTurnedFaceUpAction(ins)));
        }
    }
    if low.starts_with("you may cast ~ from your graveyard if ")
        && let Some((first_s, second_s)) = s.split_once(". If you do, ")
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(first_s, vocab)
        && let Ok(ins) = parse_atomic_effect(second_s, vocab)
    {
        return Ok(st_s(StaticAbilityAst::Compound(vec![
            st,
            StaticAbilityAst::FollowUp(ins),
        ])));
    }
    if low.starts_with(
        "if a source would deal damage to a player, you may prevent x of that damage, where x is ",
    ) && let Some((_, wx)) = low.split_once(", where x is ")
        && let Ok(val) = parse_value_expr(wx, vocab)
    {
        return Ok(st_instead(
            "damage_source",
            "deal_damage",
            s.to_string(),
            Some(vec![InstructionAst::PreventDamage {
                amount: Box::new(val),
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::AnyPlayer)),
                duration: DurationAst::Instant,
            }]),
        ));
    }

    if let Some(flt_raw) =
        low.strip_prefix("your opponents can't cast spells or activate abilities of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "opponents_cant_cast_or_activate"));
    }
    if low.starts_with("triggered abilities you control can't cause you to ")
        || low.starts_with(
            "while an opponent is choosing targets as part of casting a spell they control",
        )
    {
        return Ok(st_rule(low));
    }
    for (suf, tag) in [
        (
            " can't attack or block, and its activated abilities with {t} in their costs can't be activated",
            "cant_attack_block_tap_activate",
        ),
        (
            " can't attack or block, and their activated abilities with {t} in their costs can't be activated",
            "cant_attack_block_tap_activate",
        ),
        (
            " can't attack, block, or crew vehicles, and its activated abilities can't be activated unless they're mana abilities",
            "cant_attack_block_crew_nonmana_activate",
        ),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(st_rule_subj(subj, tag.to_string()));
        }
    }
    if let Some((subj_raw, rhs)) = low.split_once(" is a ")
        && let Some((ty_kw, _)) = rhs.split_once(" and it loses all other card types")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_cont_layer(
            subj,
            Some(ty_kw.trim().to_string()),
            false,
            None,
            None,
            Vec::new(),
            Vec::new(),
            false,
        ));
    }
    if s.contains('"')
        && let Some((subj_raw, rhs)) = low.split_once(" has \"")
        && let Some((ab_inner, tail)) = rhs.split_once('"')
        && let Some(ty_raw) = tail
            .trim()
            .strip_prefix("and is a ")
            .or_else(|| tail.trim().strip_prefix("and is an "))
        && ty_raw.ends_with(" in addition to its other types")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let orig_ab = s
            .split_once('"')
            .and_then(|(_, r)| r.split_once('"'))
            .map_or(ab_inner, |(inner, _)| inner);
        let compiled = compile_granted_ability(orig_ab, vocab);
        return Ok(st_s(StaticAbilityAst::Compound(vec![
            StaticAbilityAst::GrantAbilityText {
                subject: Box::new(subj.clone()),
                ability_text: orig_ab.to_string(),
                compiled,
                condition: None,
            },
            StaticAbilityAst::SetOrAddType {
                subject: Box::new(subj),
                type_or_subtype: ty_raw
                    .strip_suffix(" in addition to its other types")
                    .unwrap_or(ty_raw)
                    .trim()
                    .to_string(),
                additive: true,
                condition: None,
            },
        ])));
    }
    if let Some((first_s, second_s)) = s.split_once(". ")
        && !first_s.contains('"')
        && !second_s.contains(". ")
        && let Ok(StaticOrReplacement::Static(st1)) = parse_static_or_replacement(first_s, vocab)
    {
        if let Ok(StaticOrReplacement::Static(st2)) = parse_static_or_replacement(second_s, vocab) {
            return Ok(st_s(StaticAbilityAst::Compound(vec![st1, st2])));
        }
        let s2_low = second_s.to_ascii_lowercase();
        if let Some(after_if) = s2_low.strip_prefix("if ")
            && let Some((cond_raw, eff_raw)) = after_if.split_once(", ")
            && let Ok(cond) = parse_condition(cond_raw, vocab)
            && let Ok(mut sr2) = parse_static_or_replacement(
                eff_raw.strip_suffix(" instead").unwrap_or(eff_raw),
                vocab,
            )
        {
            attach_condition(&mut sr2, cond);
            if let StaticOrReplacement::Static(st2) = sr2 {
                return Ok(st_s(StaticAbilityAst::Compound(vec![st1, st2])));
            }
        }
    }

    if let Some(after_sp) =
        low.strip_prefix("if a spell would deal damage to a permanent or player, prevent ")
    {
        let instead_body = after_sp
            .strip_suffix(" of that damage")
            .and_then(|amt_raw| parse_value_expr(amt_raw.trim(), vocab).ok())
            .map(|amount| {
                vec![InstructionAst::PreventDamage {
                    amount: Box::new(amount),
                    target: Box::new(SubjectRefAst::AnyTarget),
                    duration: DurationAst::Instant,
                }]
            });
        return Ok(st_instead(
            "spell_damage",
            "deal_damage",
            s.to_string(),
            instead_body,
        ));
    }

    if low.starts_with("if ")
        && (low.contains(" would deal damage to ") || low.contains(" would deal combat damage to "))
        && low.contains(", prevent ")
        && (low.ends_with(" of that damage") || low.ends_with(" prevent that damage"))
    {
        let instead_part = s.split_once(", ").map_or(s, |(_, r)| r);
        let instead_body = parse_instead_body(instead_part, vocab);
        return Ok(st_instead(
            "damage_source",
            "deal_damage",
            s.to_string(),
            instead_body,
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" is also a ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_set_type(subj, rhs.to_string(), true));
    }
    for (mid, rule_name) in [
        (" can't attack or block unless ", "CantAttackOrBlockUnless"),
        (" can't attack unless ", "CantAttackUnless"),
        (" can't block unless ", "CantBlockUnless"),
    ] {
        if let Some((subj_raw, cond_raw)) = low.split_once(mid)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if let Ok(cond) = parse_condition(cond_raw, vocab) {
                return Ok(st_rule_full(
                    Some(subj),
                    rule_name.to_string(),
                    None,
                    Some(cond),
                ));
            }
            if let Some(cost_raw) = cond_raw.strip_prefix("you ")
                && parse_cost_atom(cost_raw, vocab).is_ok()
            {
                return Ok(st_rule_subj(subj, format!("{rule_name}:cost:{cost_raw}")));
            }
        }
    }

    // Pass M orthogonal global statics:
    // - Legend rule exemption (`Mirror Box`, `Mirror Gallery`, `The Master, Multiplied`, `Sakashima`)
    // - Cost reduction riders (`Ragemonger`, `Training Grounds`, `Morophon`, `Heartstone`, `Valiant Changeling`)
    // - Cast restrictions (`You can't cast ~`, `Your opponents can't cast <flt>`, `Players can't cast <flt>`)
    // - Static damage redirection (`Pariah`: `All damage that would be dealt to <from> is dealt to <to> instead`)
    if low.starts_with("the \"legend rule\" doesn't apply") {
        let flt = low
            .strip_prefix("the \"legend rule\" doesn't apply to ")
            .and_then(|f| parse_object_filter(f, vocab).ok())
            .map(Box::new);
        return Ok(st_s(StaticAbilityAst::RuleOrCombatRestriction {
            subject: None,
            rule: "LegendRuleExempt".to_string(),
            filter: flt,
            condition: None,
        }));
    }
    if low == "this effect reduces only the amount of colored mana you pay"
        || low == "this effect can't reduce the mana in that cost to less than one mana"
        || low.starts_with("this effect can't reduce the amount of mana ")
        || low.starts_with("this effect can't reduce the mana in that cost to less than ")
    {
        return Ok(st_rule(format!("CostReductionRider:{low}")));
    }

    if let Some(flt_raw) = low
        .strip_suffix(" entering or dying don't cause abilities to trigger")
        .or_else(|| low.strip_suffix(" entering doesn't cause abilities to trigger"))
        .or_else(|| low.strip_suffix(" dying doesn't cause abilities to trigger"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(st_rule_flt(flt, "SuppressTriggers"));
    }
    // "<subj> lose(s) <keywords>" (Mystic Decree: "All creatures lose flying and islandwalk")
    if !s.contains('"')
        && let Some((subj_raw, kw_raw)) = split_once_any(&low, &[" loses ", " lose "])
        && let (Ok(subj), Some(lkws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw_raw, vocab),
        )
    {
        return Ok(st_cont_layer(
            subj,
            None,
            false,
            None,
            None,
            Vec::new(),
            lkws,
            false,
        ));
    }

    // Maximum hand size modifiers ("Your maximum hand size is <N/val>" / "equal to <val>" / "reduced by N" / "increased by N")
    if let Some(after) = low
        .strip_prefix("your maximum hand size is ")
        .or_else(|| low.strip_prefix("each opponent's maximum hand size is "))
        .or_else(|| low.strip_prefix("each player's maximum hand size is "))
    {
        let val_raw = strip_prefix_any(&after, &["equal to ", "reduced by ", "increased by "])
            .unwrap_or(after);
        if parse_value_expr(val_raw, vocab).is_ok() {
            return Ok(st_rule(format!("MaxHandSize:{low}")));
        }
    }
    if matches!(
        low.as_str(),
        "players can't draw cards"
            | "your opponents can't draw cards"
            | "each opponent can't draw cards"
            | "opponents can't draw cards"
    ) {
        return Ok(st_rule(format!("CantDraw:{low}")));
    }

    // Pillowfort / attack-or-block tax ("Creatures can't attack ... unless their controller pays ...",
    // "<subj> can't attack or block unless its controller pays ...")
    if let Some((left, right)) = low.split_once(" unless ")
        && (left.contains("can't attack") || left.contains("can't block"))
        && (right.starts_with("their controller pays ")
            || right.starts_with("its controller pays "))
    {
        return Ok(st_rule(format!("AttackTax:{low}")));
    }

    // "<subj> can't be the target of <filter>" / "<subj> can't be blocked if <cond>"
    if let Some((subj_raw, src_raw)) = split_once_any(
        &low,
        &[" can't be the target of ", " can't be the targets of "],
    ) {
        let subj_opt = if subj_raw.trim() == "players and permanents" {
            Some(SubjectRefAst::Compound(vec![
                SubjectRefAst::Player(PlayerRefAst::EachPlayer),
                SubjectRefAst::All(Box::new(any_perm())),
            ]))
        } else {
            parse_subject_or_target(subj_raw, vocab).ok()
        };
        if let Some(subj) = subj_opt {
            let flt = parse_object_filter(src_raw, vocab).ok().map(Box::new);
            return Ok(st_s(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("CantBeTargetOf:{src_raw}"),
                filter: flt,
                condition: None,
            }));
        }
    }

    // Cluster 12: Typed counter prevention statics (`<subj> can't have [<kind>] counters put on it/them`, `<player> can't get [<kind>] counters`)
    if let Some((subj_raw, rest)) = low.split_once(" can't have ")
        && let Some(kind_raw) = strip_suffix_any(
            &rest,
            &[
                " counters put on it",
                " counters placed on it",
                " counters put on them",
                " counters placed on them",
                " counter put on it",
                " counter put on them",
            ],
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let k = kind_raw.trim();
        let counter_kind = if k.is_empty() || k == "any" {
            None
        } else {
            Some(k.to_string())
        };
        return Ok(st_s(StaticAbilityAst::PreventCounters {
            subject: Box::new(subj),
            counter_kind,
            condition: None,
        }));
    }
    if let Some((who_raw, rest)) = low.split_once(" can't get ")
        && let Some(kind_raw) = rest.strip_suffix(" counters")
        && let Ok(subj) = parse_subject_or_target(who_raw.trim(), vocab)
    {
        let k = kind_raw.trim();
        let counter_kind = if k.is_empty() || k == "any" {
            None
        } else {
            Some(k.to_string())
        };
        return Ok(st_s(StaticAbilityAst::PreventCounters {
            subject: Box::new(subj),
            counter_kind,
            condition: None,
        }));
    }
    // Cluster 16: Damage-as-though keyword proxy static (`all damage is dealt to you as though its source had infect`)
    if let Some((dmg_part, rest)) = low.split_once(" is dealt to ")
        && dmg_part.contains("damage")
        && let Some((tgt_raw, kw_raw)) = rest.split_once(" as though its source had ")
        && let Ok(target) = parse_subject_or_target(tgt_raw.trim(), vocab)
    {
        return Ok(st_s(StaticAbilityAst::DamageDealtAsThough {
            damage_filter: dmg_part.trim().to_string(),
            target: Box::new(target),
            source_keyword: kw_raw.trim().to_string(),
            condition: None,
        }));
    }

    for rule in [
        "can't be blocked",
        "can't be countered",
        "can't be equipped",
        "can't be sacrificed",
        "can't become untapped",
        "can't have counters put on it",
        "can't have counters placed on it",
        "can't have counters put on them",
        "can't be the targets of spells or abilities",
        "can't be the target of spells or abilities",
        "can't be the target of spells",
        "can't block",
        "can't block and can't be blocked",
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
        "can attack as though it didn't have defender",
        "can attack as though they didn't have defender",
        "isn't a creature",
        "aren't creatures",
        "isn't an artifact",
        "isn't a human",
        "is the chosen type in addition to its other types",
        "is all creature types",
        "is every creature type",
        "is goaded",
        "is also goaded",
        "loses all other abilities",
        "lose all other abilities",
        "assigns combat damage equal to its toughness rather than its power",
        "can't attack or block alone",
        "can't attack alone",
        "can't block alone",
        "attacks each combat if able",
        "attacks or blocks each combat if able",
        "must be blocked if able",
        "can block an additional creature each combat",
        "can block any number of creatures",
        "doesn't untap during your untap step",
        "doesn't untap during its controller's untap step",
        "don't untap during their controllers' untap steps",
    ] {
        if let Some(subj_raw) = low.strip_suffix(&format!(" {rule}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if rule == "can't block and can't be blocked" {
                return Ok(st_s(StaticAbilityAst::Compound(vec![
                    st_rule_ast(Some(subj.clone()), "cant_block", None),
                    st_rule_ast(Some(subj), "cant_be_blocked", None),
                ])));
            }
            if rule == "can't attack or block alone" {
                return Ok(st_s(StaticAbilityAst::Compound(vec![
                    st_rule_ast(Some(subj.clone()), "cant_attack_alone", None),
                    st_rule_ast(Some(subj), "cant_block_alone", None),
                ])));
            }
            let canonical_rule = match rule {
                "can't be blocked" => "cant_be_blocked",
                "can't be countered" => "CantBeCountered",
                "can't block" => "cant_block",
                "can't attack" => "cant_attack",
                "can't attack or block" => "cant_attack_or_block",
                "can attack as though it didn't have defender"
                | "can attack as though they didn't have defender" => {
                    "can_attack_as_though_no_defender"
                }
                "is goaded" | "is also goaded" => "permanents_are_goaded",
                "can't attack alone" => "cant_attack_alone",
                "can't block alone" => "cant_block_alone",
                "attacks each combat if able" => "AttacksEachCombatIfAble",
                "attacks or blocks each combat if able" => "AttacksOrBlocksEachCombatIfAble",
                "must be blocked if able" => "must_be_blocked_if_able",
                "can block an additional creature each combat" => "additional_block",
                "can block any number of creatures" => "can_block_any_number",
                "doesn't untap during its controller's untap step"
                | "don't untap during their controllers' untap steps" => "DoesntUntap",
                other => other,
            };
            return Ok(st_rule_subj(subj, canonical_rule.to_string()));
        }
    }
    // "<subj>'s activated abilities can't be activated" / "<player> can't activate abilities that aren't ..."
    if let Some(subj_raw) = low.strip_suffix("'s activated abilities can't be activated")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, "CantActivateAbilities"));
    }
    if let Some((who_raw, rest)) = low.split_once(" can't activate abilities that aren't ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(st_rule_subj(
            SubjectRefAst::Player(player),
            format!("cant_activate_non:{rest}"),
        ));
    }
    // General CR 508/509 "as though" combat permission statics (Quagmire, Undertow, Weathered Sentinels, Chaos Lord, Glaring Spotlight)
    if let Some((subj_raw, rhs)) = low.split_once(" can be blocked as though ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, format!("block_as_though:{rhs}")));
    }
    if let Some((subj_raw, rhs)) =
        split_once_any(&low, &[" can attack ", " can be the targets of "])
        && rhs.contains("as though ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_rule_subj(subj, format!("as_though:{rhs}")));
    }

    // Static damage prevention ("to <subj>", "to and dealt by <subj>", "by <subj>")
    if low.starts_with("prevent all ") {
        let combat_only = low.starts_with("prevent all combat ");
        let noncombat_only = low.starts_with("prevent all noncombat ");
        if let Some((_, subj_raw)) = low.split_once("damage that would be dealt to and dealt by ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(st_s(StaticAbilityAst::PreventAllDamageStatic {
                combat_only,
                noncombat_only,
                both_ways: true,
                subject: Box::new(subj.clone()),
                by: Some(Box::new(subj)),
            }));
        }
        if let Some((_, rest)) = low.split_once("damage that would be dealt to ") {
            let (subj_raw, by_opt) = match rest.split_once(" by ") {
                Some((sr, b)) => (sr, parse_subject_or_target(b, vocab).ok().map(Box::new)),
                None => (rest, None),
            };
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(st_s(StaticAbilityAst::PreventAllDamageStatic {
                    combat_only,
                    noncombat_only,
                    both_ways: false,
                    subject: Box::new(subj),
                    by: by_opt,
                }));
            }
        }
        if let Some((_, by_raw)) = low.split_once("damage that would be dealt by ")
            && let Ok(by_subj) = parse_subject_or_target(by_raw, vocab)
        {
            return Ok(st_s(StaticAbilityAst::PreventAllDamageStatic {
                combat_only,
                noncombat_only,
                both_ways: false,
                subject: Box::new(SubjectRefAst::AnyTarget),
                by: Some(Box::new(by_subj)),
            }));
        }
        // "Prevent all [combat] damage that <src> would deal to <tgt>" (Goblin Furrier, etc.)
        if let Some((_, rest)) = low.split_once("damage that ")
            && let Some((src_raw, tgt_raw)) = rest.split_once(" would deal to ")
            && let (Ok(by_subj), Ok(tgt_subj)) = (
                parse_subject_or_target(src_raw, vocab),
                parse_subject_or_target(tgt_raw, vocab),
            )
        {
            return Ok(st_s(StaticAbilityAst::PreventAllDamageStatic {
                combat_only,
                noncombat_only,
                both_ways: false,
                subject: Box::new(tgt_subj),
                by: Some(Box::new(by_subj)),
            }));
        }
    }

    // Type/subtype change: "Enchanted land is a Swamp" / "it's an artifact creature" / "They're Zombies in addition to their other types"
    if let Some((subj_raw, rhs)) = split_once_any(
        &low,
        &[" is a ", " is an ", "'s a ", "'s an ", "'re ", " are "],
    ) {
        let (core_t, additive) = match strip_suffix_any(
            &rhs,
            &[
                " in addition to its other types",
                " in addition to their other types",
                " in addition to its other land types",
                " in addition to their other land types",
                " in addition to its other creature types",
                " in addition to their other creature types",
            ],
        ) {
            Some(c) => (c, true),
            None => (rhs, false),
        };
        if (matches!(
            core_t,
            "plains"
                | "island"
                | "swamp"
                | "mountain"
                | "forest"
                | "islands"
                | "swamps"
                | "mountains"
                | "forests"
                | "artifact"
                | "artifacts"
                | "creature"
                | "creatures"
                | "artifact creature"
                | "enchantment"
                | "enchantments"
                | "land"
                | "lands"
                | "the chosen type"
        ) || vocab.lookup_subtype(core_t).is_some()
            || (core_t.contains(' ') && parse_object_filter(core_t, vocab).is_ok()))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(st_set_type(subj, core_t.to_string(), additive));
        }
    }

    // Others enter tapped
    if let Some(subj_raw) = strip_suffix_any(
        &low,
        &[
            " enters the battlefield tapped",
            " enter the battlefield tapped",
            " enters tapped",
            " enter tapped",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(st_r(ReplacementEffectAst::OthersEnterTapped {
            subject: Box::new(subj),
        }));
    }

    // Compound static conjunction combinator ("<static1> and <static2>", or "<static1>, <static2>, and <static3>",
    // quote-aware via split_top_level, with optional shared-subject carry-forward):
    // e.g. "Enchanted creature can't attack or block and has \"...\""
    // or "~ gets +1/+1, is black, and has \"...\""
    if !low.starts_with("if ") && !low.starts_with("as ") {
        let and_parts = split_top_level(s, " and ");
        if and_parts.len() >= 2 {
            for split_idx in 1..and_parts.len() {
                let left_owned = and_parts[..split_idx].join(" and ");
                let right_owned = and_parts[split_idx..].join(" and ");
                let left_trim = left_owned.trim();
                let right_trim = right_owned.trim();
                if let Ok(StaticOrReplacement::Static(ls)) =
                    parse_static_or_replacement(left_trim, vocab)
                    && let Some(rs) = parse_static_with_optional_subject(
                        right_trim,
                        extract_leading_subject_prefix(left_trim),
                        vocab,
                    )
                {
                    return Ok(st_s(StaticAbilityAst::Compound(vec![ls, rs])));
                }
            }
        }
        let comma_parts = split_top_level(s, ", ");
        if comma_parts.len() >= 2 && comma_parts.len() <= 4 {
            let first = comma_parts[0].trim();
            let subj_prefix = extract_leading_subject_prefix(first);
            if let Ok(StaticOrReplacement::Static(first_st)) =
                parse_static_or_replacement(first, vocab)
            {
                let mut items = vec![first_st];
                let mut ok = true;
                for part in &comma_parts[1..] {
                    let p = part.trim();
                    let p_clean = p.strip_prefix("and ").unwrap_or(p).trim();
                    if let Some(st) =
                        parse_static_with_optional_subject(p_clean, subj_prefix, vocab)
                    {
                        items.push(st);
                    } else {
                        ok = false;
                        break;
                    }
                }
                if ok && items.len() >= 2 {
                    return Ok(st_s(StaticAbilityAst::Compound(items)));
                }
            }
        }
    }

    // Compositional Wave 16: General static rules
    // 1. "<filter> enters/enter with an additional <N> <kind> counter(s) on it/them [for each <over>]" (Giada, Kalain, Bioengineered Future)
    // Preserves subject + count + per losslessly via OthersEnterWithCounters!
    if let Some((subj_raw, rest)) = split_once_any(
        &low,
        &[
            " enters with an additional ",
            " enter with an additional ",
            " enters with a ",
            " enter with a ",
        ],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && (rest.contains(" counter on it")
            || rest.contains(" counter on them")
            || rest.contains(" counters on it")
            || rest.contains(" counters on them"))
    {
        let additional = low.contains(" with an additional ");
        let (cnt_part, per_opt) = match rest.split_once(" for each ") {
            Some((l, r)) => (l.trim(), parse_for_each_value_expr(r.trim(), vocab).ok()),
            None => (rest.trim(), None),
        };
        let (cnt, kind) = if let Some((left_c, _)) = cnt_part.split_once(" counter") {
            let (c, k) = peel_leading_count(left_c.trim(), vocab);
            (c, if k.is_empty() { left_c.trim() } else { k })
        } else {
            (ValueExprAst::Const(1), "counter")
        };
        if matches!(subj, SubjectRefAst::SelfObject) {
            return Ok(st_r(ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
                per: None,
                x_expr: per_opt.map(Box::new),
                condition: None,
            }));
        }
        return Ok(st_r(ReplacementEffectAst::OthersEnterWithCounters {
            subject: Box::new(subj),
            counter_kind: kind.to_string(),
            count: Box::new(cnt),
            additional,
            per: per_opt.map(Box::new),
        }));
    }
    // 2. "<filter> enter(s) as a copy of <subj>" (Essence of the Wild, Infinite Reflection)
    if let Some((subj_raw, copy_raw)) =
        split_once_any(&low, &[" enter as a copy of ", " enters as a copy of "])
        && let Ok(affected) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        if matches!(affected, SubjectRefAst::SelfObject)
            && let Ok(flt) = parse_object_filter(copy_raw.trim(), vocab)
        {
            return Ok(st_r(ReplacementEffectAst::EntersAsCopy {
                optional: false,
                filter: Box::new(flt),
                exception: None,
            }));
        }
        if let Ok(copy_of) = parse_subject_or_target(copy_raw.trim(), vocab) {
            return Ok(st_r(ReplacementEffectAst::OthersEnterAsCopy {
                affected: Box::new(affected),
                copy_of: Box::new(copy_of),
                exception: None,
            }));
        }
    }
    // 3. "<filter> are goaded" / "<filter> are basic" (Baeloth Barrityl, Rootpath Purifier)
    for (suf, rule_name) in [
        (" are goaded", "permanents_are_goaded"),
        (" is goaded", "permanents_are_goaded"),
        (" are basic", "lands_are_basic"),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Ok(st_rule_subj(subj, rule_name.to_string()));
        }
    }
    // 4. "<subj> can't attack or block unless <cost/cond>" / "<subj> can't be enchanted" / "other players can't gain control of <subj>" / "players don't lose unspent mana ..."
    if low == "players don't lose unspent mana as steps and phases end" {
        return Ok(st_rule("players_keep_unspent_mana"));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't be attacked")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(st_rule_subj(subj, "cant_be_attacked"));
    }
    if let Some(subj_raw) = low.strip_prefix("other players can't gain control of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(st_rule_subj(subj, "others_cant_gain_control"));
    }
    // 5. "The first <spell> <player> casts [during each of their turns / each turn] may be cast without paying its mana cost" (Weftwalking)
    if let Some(after_first) = low.strip_prefix("the first ")
        && let Some(before_may) = after_first
            .strip_suffix(" may be cast without paying its mana cost")
            .or_else(|| {
                after_first
                    .strip_suffix(" costs {2} less to cast and can be cast as though it had flash")
            })
    {
        let spell_part = split_once_any(&before_may, &[" you cast", " each player casts"])
            .map_or(before_may, |(a, _)| a);
        if let Ok(flt) = parse_object_filter(spell_part.trim(), vocab) {
            return Ok(st_perm(true, flt, low));
        }
    }
    // 6. Compound cost modifiers: "~ costs {1} less to cast if <c1> and {1} less to cast if <c2>" (Assassin's Ink)
    //    and "~ costs <life> more to cast for each <over>" (Phyrexian Purge)
    if let Some(after_c) = low.strip_prefix("~ costs ")
        && let Some((left_c, right_c)) = after_c.split_once(" and ")
        && let (Ok(StaticOrReplacement::Static(s1)), Ok(StaticOrReplacement::Static(s2))) = (
            parse_static_or_replacement(&format!("~ costs {left_c}"), vocab),
            parse_static_or_replacement(&format!("~ costs {right_c}"), vocab),
        )
    {
        return Ok(st_s(StaticAbilityAst::Compound(vec![s1, s2])));
    }
    if let Some(after_c) = low.strip_prefix("~ costs ")
        && let Some((life_tok, fe_raw)) = after_c.split_once(" life more to cast for each ")
        && parse_number_word(life_tok.trim()).is_some()
        && let Ok(scale) = parse_for_each_value_expr(fe_raw.trim(), vocab)
    {
        return Ok(st_s(StaticAbilityAst::CostModifier {
            subject: self_obj(),
            delta: format!("{} life", life_tok.trim()),
            less: false,
            next_only: false,
            per: None,
            scale_expr: Some(Box::new(scale)),
            condition: None,
        }));
    }

    // Compositional Wave 17: Out-of-line static combinators
    if let Some(w17) = rules::parse_extended_statics(s, &low, vocab) {
        return Ok(w17);
    }

    // Compositional Wave 15: "~ and enchanted creature each get +X/+X" / "Enchanted creature gets -X/-Y"
    if let Some((subj_raw, pt_raw)) = split_once_any(&low, &[" each get ", " gets "])
        && let Some((dx, dy)) = parse_pt_mod(pt_raw.trim())
        && (subj_raw == "~ and enchanted creature"
            || parse_subject_or_target(subj_raw, vocab).is_ok())
    {
        let subject = if subj_raw == "~ and enchanted creature" {
            SubjectRefAst::Compound(vec![
                SubjectRefAst::SelfObject,
                SubjectRefAst::AttachedHost("enchanted creature".to_string()),
            ])
        } else {
            parse_subject_or_target(subj_raw, vocab).unwrap_or(SubjectRefAst::SelfObject)
        };
        return Ok(st_mod_pt(subject, dx, dy, None));
    }

    // Compositional Wave 14/15: General multi-sentence static paragraph (2..=4 sentences outside quotes),
    // allowing each sentence to be a Static, Replacement, "If <cond>, <replacement>", or FollowUp(effect)
    // (Undead Sprinter, Dragon's Disciple, Mariposa Military Base, Storage Matrix, Scheming Fence, Molten Sentry)
    {
        let sent_parts = split_top_level(s, ". ");
        if sent_parts.len() >= 2 && sent_parts.len() <= 4 {
            let mut items = Vec::new();
            let mut ok = true;
            for sp in &sent_parts {
                let st_raw = sp.trim().trim_end_matches('.').trim();
                if st_raw.is_empty() {
                    continue;
                }
                if let Ok(sr) = parse_static_or_replacement(st_raw, vocab) {
                    match sr {
                        StaticOrReplacement::Static(st) => items.push(st),
                        StaticOrReplacement::Replacement(rep) => {
                            items.push(StaticAbilityAst::FollowUp(vec![ins_temp_rep(
                                rep,
                                DurationAst::Instant,
                            )]));
                        }
                    }
                } else if let Ok(eff) = parse_atomic_effect(st_raw, vocab)
                    && !eff.is_empty()
                {
                    items.push(StaticAbilityAst::FollowUp(eff));
                } else if let Some(after_if) = st_raw.to_ascii_lowercase().strip_prefix("if ")
                    && let Some((cond_raw, rep_raw)) = after_if.split_once(", ")
                    && let Ok(cond) = parse_condition(cond_raw, vocab)
                    && let Ok(StaticOrReplacement::Replacement(rep)) =
                        parse_static_or_replacement(rep_raw, vocab)
                {
                    items.push(StaticAbilityAst::FollowUp(vec![ins_if(
                        cond,
                        vec![ins_temp_rep(rep, DurationAst::Instant)],
                    )]));
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && items.len() >= 2 {
                return Ok(st_s(StaticAbilityAst::Compound(items)));
            }
        }
    }

    // Tabletop Honor-System physical/social static rule
    if is_physical_tabletop_clause(&low) {
        return Ok(st_s(StaticAbilityAst::ManualTabletopStatic {
            prompt: s.to_string(),
        }));
    }

    Err(format!("unparsed_static:{s}"))
}

/// Convenience wrapper for `StaticOrReplacement::Static`.
#[inline]
pub(super) fn st_s(s: StaticAbilityAst) -> StaticOrReplacement {
    StaticOrReplacement::Static(s)
}

/// Convenience wrapper for `StaticOrReplacement::Replacement`.
#[inline]
pub(super) fn st_r(r: ReplacementEffectAst) -> StaticOrReplacement {
    StaticOrReplacement::Replacement(r)
}

/// Convenience constructor for an unconditioned `RuleOrCombatRestriction` static ability.
#[inline]
pub(super) fn st_rule(rule: impl Into<String>) -> StaticOrReplacement {
    st_s(StaticAbilityAst::RuleOrCombatRestriction {
        subject: None,
        rule: rule.into(),
        filter: None,
        condition: None,
    })
}

/// Convenience constructor for a subject-scoped `RuleOrCombatRestriction` static ability.
#[inline]
pub(super) fn st_rule_subj(subject: SubjectRefAst, rule: impl Into<String>) -> StaticOrReplacement {
    st_s(StaticAbilityAst::RuleOrCombatRestriction {
        subject: Some(Box::new(subject)),
        rule: rule.into(),
        filter: None,
        condition: None,
    })
}

/// Convenience constructor for a filter-scoped `RuleOrCombatRestriction` static ability.
#[inline]
pub(super) fn st_rule_flt(filter: ObjectFilterAst, rule: impl Into<String>) -> StaticOrReplacement {
    st_s(StaticAbilityAst::RuleOrCombatRestriction {
        subject: None,
        rule: rule.into(),
        filter: Some(Box::new(filter)),
        condition: None,
    })
}

/// Convenience constructor for a `PlayOrCastPermission` static ability.
#[inline]
pub(super) fn st_perm(
    cast_only: bool,
    filter: ObjectFilterAst,
    permission: impl Into<String>,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::PlayOrCastPermission {
        cast_only,
        filter: Box::new(filter),
        permission: permission.into(),
        condition: None,
    })
}

fn starts_with_static_predicate_verb(clause: &str) -> bool {
    let low = clause.trim_start().to_ascii_lowercase();
    [
        "has ", "have ", "gets ", "get ", "gains ", "gain ", "loses ", "lose ", "is ", "are ",
        "isn't ", "aren't ", "can't ", "can ", "doesn't ", "don't ", "must ", "attacks ",
        "attack ", "blocks ", "block ", "enters ", "enter ", "assigns ", "assign ", "deals ",
        "deal ", "becomes ", "become ", "costs ", "cost ",
    ]
    .iter()
    .any(|prefix| low.starts_with(prefix))
}

fn parse_static_with_optional_subject(
    clause: &str,
    subj_prefix: Option<&str>,
    vocab: &Vocabulary,
) -> Option<StaticAbilityAst> {
    let parse_one = |text: &str| match parse_static_or_replacement(text, vocab) {
        Ok(StaticOrReplacement::Static(st)) => Some(st),
        _ => None,
    };
    if starts_with_static_predicate_verb(clause)
        && let Some(sp) = subj_prefix
    {
        return parse_one(&format!("{sp} {clause}")).or_else(|| parse_one(clause));
    }
    parse_one(clause).or_else(|| subj_prefix.and_then(|sp| parse_one(&format!("{sp} {clause}"))))
}

/// Convenience constructor for a `GrantKeywords` static ability.
#[inline]
pub(super) fn st_kw(subject: SubjectRefAst, keywords: Vec<String>) -> StaticOrReplacement {
    st_s(StaticAbilityAst::GrantKeywords {
        subject: Box::new(subject),
        keywords,
        condition: None,
        paired_rule: None,
    })
}

/// Convenience constructor for a `RuleOrCombatRestriction` with both subject and filter.
#[inline]
pub(super) fn st_rule_subj_flt(
    subject: SubjectRefAst,
    rule: impl Into<String>,
    filter: ObjectFilterAst,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::RuleOrCombatRestriction {
        subject: Some(Box::new(subject)),
        rule: rule.into(),
        filter: Some(Box::new(filter)),
        condition: None,
    })
}

/// Convenience constructor for an `EventInstead` replacement effect.
#[inline]
pub(super) fn st_instead(
    subject_or_event: impl Into<String>,
    would: impl Into<String>,
    instead: impl Into<String>,
    instead_body: Option<Vec<InstructionAst>>,
) -> StaticOrReplacement {
    st_r(ReplacementEffectAst::EventInstead {
        subject_or_event: subject_or_event.into(),
        would: would.into(),
        instead: instead.into(),
        instead_body,
    })
}

/// Convenience constructor for a `GrantAbilityText` static ability.
#[inline]
pub(super) fn st_grant_ab(
    subject: SubjectRefAst,
    ability_text: impl Into<String>,
    compiled: Option<Box<crate::ast::FaceAst>>,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::GrantAbilityText {
        subject: Box::new(subject),
        ability_text: ability_text.into(),
        compiled,
        condition: None,
    })
}

/// Convenience constructor for a `CostModifier` static ability.
#[inline]
pub(super) fn st_cost_mod(
    subject: SubjectRefAst,
    delta: impl Into<String>,
    less: bool,
    next_only: bool,
    per: Option<ObjectFilterAst>,
    scale_expr: Option<ValueExprAst>,
    condition: Option<ConditionAst>,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::CostModifier {
        subject: Box::new(subject),
        delta: delta.into(),
        less,
        next_only,
        per: per.map(Box::new),
        scale_expr: scale_expr.map(Box::new),
        condition: condition.map(Box::new),
    })
}

/// Convenience constructor for a `ContinuousLayerEffect` static ability.
#[inline]
#[allow(clippy::too_many_arguments)]
pub(super) fn st_cont_layer(
    subject: SubjectRefAst,
    set_types: Option<String>,
    additive_types: bool,
    base_pt: Option<(String, String)>,
    set_colors: Option<String>,
    grant_keywords: Vec<String>,
    lose_keywords: Vec<String>,
    lose_all_abilities: bool,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::ContinuousLayerEffect {
        subject: Box::new(subject),
        set_types,
        additive_types,
        base_pt,
        set_colors,
        grant_keywords,
        lose_keywords,
        lose_all_abilities,
        condition: None,
    })
}

/// Convenience constructor for a `ContinuousModifyPT` static ability.
#[inline]
pub(super) fn st_mod_pt(
    subject: SubjectRefAst,
    dx: impl Into<String>,
    dy: impl Into<String>,
    scale: Option<ValueExprAst>,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::ContinuousModifyPT {
        subject: Box::new(subject),
        dx: dx.into(),
        dy: dy.into(),
        grant_keywords: Vec::new(),
        grant_ability_text: None,
        scale: scale.map(Box::new),
        condition: None,
        paired_rule: None,
    })
}

/// Convenience constructor for a `SetOrAddType` static ability.
#[inline]
pub(super) fn st_set_type(
    subject: SubjectRefAst,
    type_or_subtype: impl Into<String>,
    additive: bool,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::SetOrAddType {
        subject: Box::new(subject),
        type_or_subtype: type_or_subtype.into(),
        additive,
        condition: None,
    })
}

/// Convenience constructor for a `RuleOrCombatRestriction` with optional subject, filter, and condition.
#[inline]
pub(super) fn st_rule_full(
    subject: Option<SubjectRefAst>,
    rule: impl Into<String>,
    filter: Option<ObjectFilterAst>,
    condition: Option<ConditionAst>,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::RuleOrCombatRestriction {
        subject: subject.map(Box::new),
        rule: rule.into(),
        filter: filter.map(Box::new),
        condition: condition.map(Box::new),
    })
}

/// Convenience constructor for a bare `GrantKeywords` `StaticAbilityAst`.
#[inline]
pub(super) fn st_kw_ast(subject: SubjectRefAst, keywords: Vec<String>) -> StaticAbilityAst {
    StaticAbilityAst::GrantKeywords {
        subject: Box::new(subject),
        keywords,
        condition: None,
        paired_rule: None,
    }
}

/// Convenience constructor for a bare `RuleOrCombatRestriction` `StaticAbilityAst`.
#[inline]
pub(super) fn st_rule_ast(
    subject: Option<SubjectRefAst>,
    rule: impl Into<String>,
    filter: Option<ObjectFilterAst>,
) -> StaticAbilityAst {
    StaticAbilityAst::RuleOrCombatRestriction {
        subject: subject.map(Box::new),
        rule: rule.into(),
        filter: filter.map(Box::new),
        condition: None,
    }
}

/// Convenience constructor for a simple `AlternativeCastCost` static ability.
#[inline]
pub(super) fn st_alt_cast(
    cost: Vec<CostAtomAst>,
    for_spells: Option<ObjectFilterAst>,
) -> StaticOrReplacement {
    st_s(StaticAbilityAst::AlternativeCastCost {
        cost,
        condition: None,
        for_spells: for_spells.map(Box::new),
        next_only: false,
    })
}

/// True when `dest` is `"the battlefield"` followed strictly by orthogonal entry modifiers
/// (`tapped`, `attacking`, `face down`, `transformed`, `converted`, `under <controller>'s control`).
pub(crate) fn is_orthogonal_battlefield_dest(dest: &str) -> bool {
    let Some(mut rest) = dest.strip_prefix("the battlefield") else {
        return false;
    };
    rest = rest.trim();
    loop {
        if rest.is_empty() {
            return true;
        }
        let mut matched = false;
        for mod_tok in [
            "tapped and attacking defending player or a planeswalker they control",
            "tapped and attacking that player or a planeswalker they control",
            "tapped and attacking that player or a planeswalker",
            "tapped and attacking that opponent",
            "tapped and attacking that player",
            "tapped and attacking defending player",
            "tapped and attacking",
            "tapped and blocking that creature",
            "blocking that creature",
            "tapped and transformed",
            "tapped and converted",
            "tapped",
            "attacking",
            "face down",
            "face up",
            "flipped",
            "transformed",
            "converted",
            "under your control",
            "under their control",
            "under its owner's control",
            "under their owner's control",
            "under their owners' control",
            "under that player's control",
            "under an opponent's control",
            "under the control of that creature's owner",
            "under the control of that card's owner",
            "under the control of that permanent's owner",
            "under the control of its owner",
            "under the control of their owners",
            "and",
        ] {
            if let Some(after) = rest.strip_prefix(mod_tok)
                && (after.is_empty() || after.starts_with(' '))
            {
                rest = after.trim();
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }
}

pub(crate) fn player_ref_to_controller_relation(
    player: PlayerRefAst,
) -> Option<ControllerRelation> {
    match player {
        PlayerRefAst::You => Some(ControllerRelation::You),
        PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
            Some(ControllerRelation::Opponent)
        }
        PlayerRefAst::TargetOpponent => Some(ControllerRelation::TargetOpponent),
        PlayerRefAst::TargetPlayer => Some(ControllerRelation::TargetPlayer),
        PlayerRefAst::ThatPlayer => Some(ControllerRelation::ThatPlayer),
        PlayerRefAst::DefendingPlayer => Some(ControllerRelation::DefendingPlayer),
        PlayerRefAst::ActivePlayer => Some(ControllerRelation::ActivePlayer),
        PlayerRefAst::ChosenPlayer => Some(ControllerRelation::ChosenPlayer),
        PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController => {
            Some(ControllerRelation::ItsController)
        }
        _ => None,
    }
}

fn strip_min_mana_floor_sentence(tlow: &str) -> (&str, Option<u8>) {
    let Some((head, tail)) = tlow.split_once(". this effect can't reduce the ") else {
        return (tlow, None);
    };
    let tail_clean = tail.trim().trim_end_matches('.').trim();
    if let Some(after_lt) = tail_clean
        .strip_prefix("mana in that cost to less than ")
        .or_else(|| tail_clean.strip_prefix("mana in that ability's activation cost to less than "))
        && let Some(num_tok) = after_lt.strip_suffix(" mana")
        && let Some(n) = parse_number_word(num_tok.trim())
        && let Ok(floor) = u8::try_from(n)
    {
        return (head.trim(), Some(floor));
    }
    (tlow, None)
}

pub(super) fn attach_condition(target: &mut StaticOrReplacement, cond: ConditionAst) {
    let set_or_and = |slot: &mut Option<Box<ConditionAst>>, cond: ConditionAst| {
        *slot = Some(match slot.take() {
            Some(existing) => Box::new(ConditionAst::And(existing, Box::new(cond))),
            None => Box::new(cond),
        });
    };
    match target {
        StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousModifyPT { condition, .. }
            | StaticAbilityAst::GrantKeywords { condition, .. }
            | StaticAbilityAst::GrantAbilityText { condition, .. }
            | StaticAbilityAst::InheritAbilities { condition, .. }
            | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
            | StaticAbilityAst::TriggerMultiplier { condition, .. }
            | StaticAbilityAst::ScopedTriggerMultiplier { condition, .. }
            | StaticAbilityAst::GrantKeywordWithCost { condition, .. }
            | StaticAbilityAst::SetOrAddType { condition, .. }
            | StaticAbilityAst::CostModifier { condition, .. }
            | StaticAbilityAst::CostModifierWithFloor { condition, .. }
            | StaticAbilityAst::PlayOrCastPermission { condition, .. }
            | StaticAbilityAst::DynamicBasePT { condition, .. }
            | StaticAbilityAst::DynamicModifyPT { condition, .. }
            | StaticAbilityAst::PreventCounters { condition, .. }
            | StaticAbilityAst::DamageDealtAsThough { condition, .. }
            | StaticAbilityAst::AlternativeCastCost { condition, .. }
            | StaticAbilityAst::ManaSpendRule { condition, .. }
            | StaticAbilityAst::LookOrRevealRule { condition, .. }
            | StaticAbilityAst::AlternativeAbilityCost { condition, .. }
            | StaticAbilityAst::DoubleAllDamage { condition, .. }
            | StaticAbilityAst::PreventFractionDamage { condition, .. }
            | StaticAbilityAst::PreventThresholdDamage { condition, .. }
            | StaticAbilityAst::PreventAndReplaceDamage { condition, .. }
            | StaticAbilityAst::CounterModificationReplacement { condition, .. }
            | StaticAbilityAst::SkipStep { condition, .. }
            | StaticAbilityAst::ControlChange { condition, .. }
            | StaticAbilityAst::AdditionalActivationCost { condition, .. }
            | StaticAbilityAst::ActivationLimitOverride { condition, .. }
            | StaticAbilityAst::CombatDamageAssignmentRule { condition, .. }
            | StaticAbilityAst::ModifyMaximumHandSize { condition, .. }
            | StaticAbilityAst::CantBeBlockedExceptByAny { condition, .. }
            | StaticAbilityAst::RuleOrCombatRestriction { condition, .. },
        )
        | StaticOrReplacement::Replacement(ReplacementEffectAst::EntersWithCounters {
            condition,
            ..
        }) => set_or_and(condition, cond),
        StaticOrReplacement::Static(
            StaticAbilityAst::OncePerTurn(inner)
            | StaticAbilityAst::OnceDuringYourTurn(inner)
            | StaticAbilityAst::Otherwise(inner)
            | StaticAbilityAst::ScaledBy { inner, .. },
        ) => {
            let mut wrapped = StaticOrReplacement::Static((**inner).clone());
            attach_condition(&mut wrapped, cond);
            if let StaticOrReplacement::Static(updated) = wrapped {
                **inner = updated;
            }
        }
        StaticOrReplacement::Static(StaticAbilityAst::Compound(items)) => {
            for item in items {
                let mut wrapped = StaticOrReplacement::Static(item.clone());
                attach_condition(&mut wrapped, cond.clone());
                if let StaticOrReplacement::Static(updated) = wrapped {
                    *item = updated;
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn split_disjunctive_object_filters(
    raw: &str,
    vocab: &Vocabulary,
) -> Option<Vec<ObjectFilterAst>> {
    let trimmed = raw.trim();
    // 1. Explicit " and/or " disjunction (checked before " or " so "power 2 or less and/or walls" stays intact)
    if trimmed.contains(" and/or ") {
        let mut out = Vec::new();
        for seg in trimmed.split(" and/or ") {
            let flt = parse_object_filter(seg.trim(), vocab).ok()?;
            out.push(flt);
        }
        if out.len() >= 2 {
            return Some(out);
        }
    }
    // 2. Oxford comma list ("krakens, leviathans, octopuses, and serpents" / "a, b, or c")
    for tail_sep in [", and/or ", ", or ", ", and "] {
        if let Some((head, last)) = trimmed.rsplit_once(tail_sep) {
            let mut out = Vec::new();
            let mut ok = true;
            for seg in head.split(", ") {
                match parse_object_filter(seg.trim(), vocab) {
                    Ok(flt) => out.push(flt),
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && let Ok(last_flt) = parse_object_filter(last.trim(), vocab) {
                out.push(last_flt);
                if out.len() >= 2 {
                    return Some(out);
                }
            }
        }
    }
    // 3. Plain " or " disjunction ("knights or walls", "vampires or zombies", "enchanted creatures or enchantment creatures")
    if trimmed.contains(" or ") {
        let mut out = Vec::new();
        let mut ok = true;
        for seg in trimmed.split(" or ") {
            match parse_object_filter(seg.trim(), vocab) {
                Ok(flt) => out.push(flt),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && out.len() >= 2 {
            return Some(out);
        }
    }
    None
}

/// Splits the post-`" would "` portion of an `If <subj> would <would>, <instead>` replacement
/// without breaking on Oxford commas inside `<would>` (e.g. `"a graveyard, hand, or library, exile it instead"`).
fn split_would_instead_clause(rest: &str) -> Option<(&str, &str)> {
    let mut search_from = 0;
    while let Some(rel_idx) = rest[search_from..].find(", ") {
        let split_at = search_from + rel_idx;
        let after = &rest[split_at + 2..];
        let after_trim = after.trim_start();
        // Skip Oxford-comma list continuations when a later ", " introduces the replacement clause.
        let looks_like_list_item = after_trim.starts_with("or ")
            || after_trim.starts_with("and ")
            || (!after_trim.starts_with("instead")
                && !after_trim.starts_with("you may instead")
                && after_trim.contains(", ")
                && rest[..split_at].contains(" into ")
                && !after_trim
                    .split_once(", ")
                    .is_some_and(|(seg, _)| seg.contains(" instead")));
        if looks_like_list_item && after_trim.contains(", ") {
            search_from = split_at + 2;
            continue;
        }
        return Some((&rest[..split_at], after));
    }
    None
}
