//! Extended subject-headed static ability predicates, tail combinators, and `can't` restrictions.

use crate::ast::ColorMask;
use crate::v2::filter::{
    parse_keyword_kind, parse_keyword_list, parse_object_filter, parse_subject_ref,
};
use crate::v2::ir::{
    ConditionExpr, KeywordKind, KeywordSpec, ObjectFilter, ObjectStateKind, RuleRestrictionKind,
    StaticAbility, SubjectRef, SubjectStaticRule,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

use super::cond_ext::{parse_static_condition, parse_static_scale_expr};
use super::wrap_compound;

pub(super) fn finish_subject_static(
    ts: &mut TokenStream<'_>,
    subject: &SubjectRef,
    condition: &Option<ConditionExpr>,
    mut parts: Vec<StaticAbility>,
) -> PResult<StaticAbility> {
    parse_static_subject_tail(ts, subject, condition, &mut parts);
    Ok(wrap_compound(parts))
}

pub(super) fn try_compile_quoted_grant(
    ts: &TokenStream<'_>,
    quoted: &str,
    subject: &SubjectRef,
    condition: &Option<ConditionExpr>,
    trim_comma: bool,
) -> Option<StaticAbility> {
    let text = if trim_comma {
        quoted.trim_end_matches(',')
    } else {
        quoted
    };
    ts.compile_quoted_permanent_ability(text)
        .ok()
        .map(|ability| StaticAbility::GrantAbility {
            subject: subject.clone(),
            ability: Box::new(ability),
            condition: condition.clone(),
        })
}

pub(super) fn eat_doesnt_untap_during_untap_step(ts: &mut TokenStream<'_>, with_and: bool) -> bool {
    let cp = ts.checkpoint();
    let head = if with_and {
        ts.eat_any_phrase(&[
            &["and", "doesn't", "untap", "during"],
            &["and", "don't", "untap", "during"],
        ])
    } else {
        ts.eat_any_phrase(&[
            &["doesn't", "untap", "during"],
            &["don't", "untap", "during"],
        ])
    };
    if head
        && ts.eat_any_phrase(&[
            &["its", "controller's", "untap", "step"],
            &["their", "controllers'", "untap", "steps"],
            &["your", "untap", "step"],
        ])
    {
        return true;
    }
    ts.restore(cp);
    false
}

/// Parse extended subject-headed static predicates that have exact IR representations.
pub(super) fn parse_extended_subject_predicate(
    ts: &mut TokenStream<'_>,
    subject: &SubjectRef,
    condition: &Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();

    // 1. `<subj> can only attack alone`
    if ts.eat_phrase(&["can", "only", "attack", "alone"]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject.clone()),
            rule: RuleRestrictionKind::CantAttackUnless(Box::new(ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::SelfObject),
                state: ObjectStateKind::AttackingAlone,
            })),
            condition: condition.clone(),
        });
    }

    // 2. `<subj> has/have base power and toughness P/T, <kws> [, and "<quoted>"]`
    if ts.eat_any_word(&["has", "have"]) {
        let bp_cp = ts.checkpoint();
        if ts.eat_phrase(&["base", "power", "and", "toughness"])
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let mut parts = vec![StaticAbility::SetBasePT {
                subject: subject.clone(),
                power,
                toughness,
            }];
            loop {
                let t_cp = ts.checkpoint();
                let had_comma = ts.eat_comma();
                let had_and = ts.eat_word("and");
                if !had_comma && !had_and {
                    ts.restore(t_cp);
                    break;
                }
                if let Some(quoted) = ts.next_quoted() {
                    if let Some(st) = try_compile_quoted_grant(ts, quoted, subject, condition, true)
                    {
                        parts.push(st);
                        continue;
                    }
                } else if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: kws,
                        condition: condition.clone(),
                    });
                    continue;
                }
                ts.restore(t_cp);
                break;
            }
            if parts.len() > 1 {
                return finish_subject_static(ts, subject, condition, parts);
            }
        }
        ts.restore(bp_cp);
    }
    ts.restore(cp);

    // 3. `<subj> gain(s) <kw_spec> as you cast them`
    if ts.eat_any_word(&["gain", "gains"])
        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
        && ts.eat_phrase(&["as", "you", "cast", "them"])
    {
        return Ok(StaticAbility::GrantKeywords {
            subject: subject.clone(),
            keywords,
            condition: condition.clone(),
        });
    }
    ts.restore(cp);

    Err(ts.err("not an extended subject predicate"))
}

/// Shared tail combinator for subject-headed static abilities:
/// chains `[, ] [and] has/have "<quoted>"`, `[, ] [and] has/have <kws>`,
/// `[, ] [and] can't <rule>`, `[, ] [and] [it] loses all [other] abilities`,
/// and `[, ] [and] is/are [a/an] <colors/subtypes/types> [in addition to ...]` onto `parts`.
pub(super) fn parse_static_subject_tail(
    ts: &mut TokenStream<'_>,
    subject: &SubjectRef,
    condition: &Option<ConditionExpr>,
    parts: &mut Vec<StaticAbility>,
) {
    loop {
        let cp = ts.checkpoint();
        let prev_quoted = ts
            .pos
            .checked_sub(1)
            .and_then(|idx| ts.tokens.get(idx))
            .and_then(|t| match t.kind {
                crate::v2::lexer::TokenKind::Quoted(q) => Some(q.trim_end()),
                _ => None,
            });
        // Sentence-ending quoted ability followed by `It's a/an <Sub> in addition to its other types.`
        if prev_quoted.is_some_and(|q| q.ends_with('.'))
            && ts.eat_any_phrase(&[&["it's", "a"], &["it's", "an"]])
            && let Some(w) = ts.peek_word()
            && let Some(sub) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase())
        {
            let sub = sub.to_string();
            ts.next_word();
            if ts.eat_in_addition_to_other_types() {
                parts.push(StaticAbility::SetSubtypes {
                    subject: subject.clone(),
                    subtypes: vec![sub],
                    keep_existing: true,
                });
                continue;
            }
            ts.restore(cp);
        }
        let had_comma = ts.eat_comma() || prev_quoted.is_some_and(|q| q.ends_with(','));
        let had_and = ts.eat_word("and");
        if !had_comma && !had_and {
            ts.restore(cp);
            break;
        }
        // `[it/they] gain(s) "<quoted>"`
        if (ts.eat_any_phrase(&[&["they", "gain"], &["it", "gains"]])
            || ts.eat_any_word(&["gain", "gains"]))
            && let Some(quoted) = ts.next_quoted()
            && let Some(st) = try_compile_quoted_grant(ts, quoted, subject, condition, true)
        {
            parts.push(st);
            continue;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        // `[it/they] has/have ...`
        {
            let has_cp = ts.checkpoint();
            let _ = ts.eat_any_word(&["it", "they"]);
            if ts.eat_any_word(&["has", "have"]) {
                let bp_cp = ts.checkpoint();
                if ts.eat_phrase(&["base", "power", "and"]) {
                    let _ = ts.eat_word("base");
                    if ts.eat_phrase(&["toughness", "each", "equal", "to"])
                        && let Ok(val) = try_parse(ts, parse_value_expr)
                    {
                        parts.push(StaticAbility::DynamicBasePT {
                            power: Some(val.clone()),
                            toughness: Some(val),
                        });
                        continue;
                    }
                }
                ts.restore(bp_cp);
                if let Some(quoted) = ts.next_quoted() {
                    if let Some(st) = try_compile_quoted_grant(ts, quoted, subject, condition, true)
                    {
                        parts.push(st);
                        continue;
                    }
                } else if let Ok((keywords, _)) = try_parse(ts, parse_keyword_list) {
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords,
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(has_cp);
        }
        // Bare quoted ability after `,` or `and`
        if let Some(quoted) = ts.next_quoted()
            && let Some(st) = try_compile_quoted_grant(ts, quoted, subject, condition, true)
        {
            parts.push(st);
            continue;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        // `can only attack alone`
        if ts.eat_phrase(&["can", "only", "attack", "alone"]) {
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: RuleRestrictionKind::CantAttackUnless(Box::new(
                    ConditionExpr::SubjectState {
                        subject: Box::new(SubjectRef::SelfObject),
                        state: ObjectStateKind::AttackingAlone,
                    },
                )),
                condition: condition.clone(),
            });
            continue;
        }
        // `can attack as though it/they didn't have defender`
        if ts.eat_phrase(&[
            "can", "attack", "as", "though", "it", "didn't", "have", "defender",
        ]) || ts.eat_phrase(&[
            "can", "attack", "as", "though", "they", "didn't", "have", "defender",
        ]) {
            parts.push(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition: condition.clone(),
            });
            continue;
        }
        // `can block creatures with <kw> [abilities] as though they didn't have <kw | those abilities>`
        if ts.eat_phrase(&["can", "block", "creatures", "with"])
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
        {
            let _ = ts.eat_word("abilities");
            if ts.eat_phrase(&["as", "though", "they", "didn't", "have"])
                && (ts.eat_phrase(&["those", "abilities"])
                    || try_parse(ts, parse_keyword_kind).is_ok_and(|k2| k2 == kw))
            {
                parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::IgnoreKeywordForBlocking(kw),
                    condition: condition.clone(),
                });
                continue;
            }
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        // Trailing `[, ] [and] equip [<subtype|type>] {M}` after `"<quoted>"`
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("equip"))
        {
            if let Ok((keywords, _)) = try_parse(ts, parse_keyword_list) {
                parts.push(StaticAbility::GrantKeywords {
                    subject: subject.clone(),
                    keywords,
                    condition: condition.clone(),
                });
                continue;
            }
            let eq_cp = ts.checkpoint();
            if ts.eat_word("equip") {
                let eq_filter = if let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                        let owned = sub.to_string();
                        ts.next_word();
                        Some(ObjectFilter {
                            subtypes: vec![owned],
                            ..Default::default()
                        })
                    } else if let Some(ct) = crate::vocab::lookup_card_type(&low) {
                        ts.next_word();
                        Some(ObjectFilter {
                            all_types: ct,
                            ..Default::default()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(mc) = ts.eat_mana_cost() {
                    let cost = vec![crate::v2::ir::Cost::Mana(mc)];
                    let param = match eq_filter {
                        Some(flt) => {
                            crate::v2::ir::KeywordParam::FilterAndCost(Box::new(flt), cost)
                        }
                        None => crate::v2::ir::KeywordParam::Cost(cost),
                    };
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: vec![KeywordSpec {
                            kind: KeywordKind::Equip,
                            param: Some(param),
                        }],
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(eq_cp);
        }
        // `assigns combat damage equal to its toughness rather than its power`
        if ts.eat_any_word(&["assigns", "assign"])
            && ts.eat_phrase(&["combat", "damage", "equal", "to"])
            && let Ok(crate::v2::ir::ValueExpr::ObjectStat {
                stat: crate::ast::StatKind::Toughness,
                ..
            }) = try_parse(ts, crate::v2::value::parse_value_expr)
            && ts.eat_phrase(&["rather", "than"])
            && let Ok(crate::v2::ir::ValueExpr::ObjectStat {
                stat: crate::ast::StatKind::Power,
                ..
            }) = try_parse(ts, crate::v2::value::parse_value_expr)
        {
            parts.push(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::AssignDamageByToughness,
                condition: condition.clone(),
            });
            continue;
        }
        // `attacks/blocks each combat if able` / `is/are goaded` / `can block an additional <N> creature(s)`
        if ts.eat_phrase(&["attacks", "each", "combat", "if", "able"])
            || ts.eat_phrase(&["attack", "each", "combat", "if", "able"])
        {
            parts.push(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::MustAttack,
                condition: condition.clone(),
            });
            continue;
        }
        if ts.eat_phrase(&["blocks", "each", "combat", "if", "able"])
            || ts.eat_phrase(&["block", "each", "combat", "if", "able"])
        {
            parts.push(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::AllMustBlock,
                condition: condition.clone(),
            });
            continue;
        }
        if ts.eat_phrase(&["is", "goaded"]) || ts.eat_phrase(&["are", "goaded"]) {
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: RuleRestrictionKind::AreGoaded,
                condition: condition.clone(),
            });
            continue;
        }
        {
            let bcp = ts.checkpoint();
            if ts.eat_phrase(&["can", "block", "an", "additional"]) {
                let extra = ts
                    .next_number(false)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(1);
                if (ts.eat_word("creature") || ts.eat_word("creatures"))
                    && (ts.eat_phrase(&["each", "combat"]) || ts.is_eof())
                {
                    parts.push(StaticAbility::SubjectRule {
                        subject: subject.clone(),
                        rule: SubjectStaticRule::CanBlockAdditional(extra),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(bcp);
        }
        // `[it/they] can't <rule>`
        {
            let ccp = ts.checkpoint();
            let _ = ts.eat_word("it") || ts.eat_word("they");
            if ts.eat_word("can't")
                && let Ok(rule) = try_parse(ts, parse_cant_rule)
            {
                parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(ccp);
        }
        // `[it/they] gets/get +P/+T [for each ...]`
        {
            let gcp = ts.checkpoint();
            let _ = ts.eat_word("it") || ts.eat_word("they");
            if (ts.eat_word("gets") || ts.eat_word("get"))
                && let Some((power, toughness)) = ts.next_pt_mod()
            {
                let scale = opt(ts, parse_static_scale_expr).map(Box::new);
                parts.push(StaticAbility::ContinuousModifyPT {
                    subject: subject.clone(),
                    power,
                    toughness,
                    grant_keywords: Vec::new(),
                    scale,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(gcp);
        }
        // `must be blocked if able`
        if ts.eat_phrase(&["must", "be", "blocked", "if", "able"]) {
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: RuleRestrictionKind::MustBeBlocked,
                condition: condition.clone(),
            });
            continue;
        }
        // `[it/they] loses/lose all [other] abilities`
        {
            let lcp = ts.checkpoint();
            let _ = ts.eat_word("it") || ts.eat_word("they");
            if (ts.eat_phrase(&["loses", "all"]) || ts.eat_phrase(&["lose", "all"])) && {
                let _ = ts.eat_word("other");
                ts.eat_word("abilities")
            } {
                parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(lcp);
        }
        ts.restore(cp);
        let cp2 = ts.checkpoint();
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        // `is/are [a/an] <colors/subtypes/types> [in addition to ...]`
        if ts.eat_word("is") || ts.eat_word("are") {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            let mut colors = ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(c) = crate::vocab::lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else if crate::vocab::lookup_card_type(&low).is_some() {
                    ts.next_word();
                    saw_type = true;
                } else {
                    break;
                }
            }
            if !subtypes.is_empty() || !colors.is_empty() || saw_type {
                let keep_existing = ts.eat_in_addition_to_other_types();
                if !colors.is_empty() {
                    parts.push(StaticAbility::SubjectRule {
                        subject: subject.clone(),
                        rule: SubjectStaticRule::SetColors {
                            colors,
                            keep_existing,
                        },
                        condition: condition.clone(),
                    });
                }
                if !subtypes.is_empty() || saw_type {
                    parts.push(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes,
                        keep_existing,
                    });
                }
                continue;
            }
        }
        ts.restore(cp2);
        break;
    }
}

/// Parse the strictly-typed restriction rule following `can't`.
/// Only exact 1-to-1 [`RuleRestrictionKind`] variants are accepted; everything else fails closed.
pub(crate) fn parse_cant_rule(ts: &mut TokenStream<'_>) -> PResult<RuleRestrictionKind> {
    if ts.eat_phrase(&["become", "untapped"]) {
        return Ok(RuleRestrictionKind::CantBecomeUntapped);
    }
    if ts.eat_phrase(&["phase", "out"]) {
        return Ok(RuleRestrictionKind::CantPhaseOut);
    }
    if ts.eat_phrase(&["be", "enchanted"]) {
        let flt = if ts.eat_word("by") {
            parse_object_filter(ts)?
        } else {
            ObjectFilter::default()
        };
        return Ok(RuleRestrictionKind::CantBeEnchantedBy(Box::new(flt)));
    }
    if ts.eat_phrase(&["be", "attacked"]) {
        return Ok(RuleRestrictionKind::CantAttackTarget(Box::new(
            SubjectRef::SelfObject,
        )));
    }
    if ts.eat_phrase(&["be", "the", "targets", "of"])
        || ts.eat_phrase(&["be", "the", "target", "of"])
    {
        let src = parse_object_filter(ts)?;
        return Ok(RuleRestrictionKind::CantBeTargetedBy(Box::new(src)));
    }
    if ts.eat_phrase(&["attack", "or", "block"]) {
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantAttackOrBlockAlone);
        }
        if ts.eat_word("unless") {
            let cond = parse_static_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackOrBlockUnless(Box::new(cond)));
        }
        return Ok(RuleRestrictionKind::CantAttackOrBlock);
    }
    if ts.eat_word("attack") {
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantAttackAlone);
        }
        if ts.eat_phrase(&["unless", "defending", "player", "controls"]) {
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantAttackUnlessDefendingControls(
                Box::new(flt),
            ));
        }
        if ts.eat_word("unless") {
            let cond = parse_static_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackUnless(Box::new(cond)));
        }
        if !ts.peek_word().is_some_and(|w| {
            w.eq_ignore_ascii_case("unless")
                || w.eq_ignore_ascii_case("alone")
                || w.eq_ignore_ascii_case("or")
                || w.eq_ignore_ascii_case("and")
                || w.eq_ignore_ascii_case("during")
                || w.eq_ignore_ascii_case("as")
                || w.eq_ignore_ascii_case("if")
                || w.eq_ignore_ascii_case("with")
        }) && let Ok(mut tgt) = try_parse(ts, parse_subject_ref)
        {
            let or_cp = ts.checkpoint();
            if ts.eat_word("or") {
                if let Ok(tgt2) = try_parse(ts, parse_subject_ref) {
                    tgt = SubjectRef::Compound(vec![tgt, tgt2]);
                } else {
                    ts.restore(or_cp);
                }
            }
            return Ok(RuleRestrictionKind::CantAttackTarget(Box::new(tgt)));
        }
        return Ok(RuleRestrictionKind::CantAttack);
    }
    if ts.eat_word("block") {
        if ts.eat_phrase(&["or", "be", "blocked", "by"]) {
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantBlockOrBeBlockedBy(Box::new(flt)));
        }
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantBlockAlone);
        }
        if ts.eat_word("unless") {
            let cond = parse_static_condition(ts)?;
            return Ok(RuleRestrictionKind::CantBlockUnless(Box::new(cond)));
        }
        if !ts.peek_word().is_some_and(|w| {
            w.eq_ignore_ascii_case("unless")
                || w.eq_ignore_ascii_case("alone")
                || w.eq_ignore_ascii_case("or")
                || w.eq_ignore_ascii_case("and")
                || w.eq_ignore_ascii_case("during")
                || w.eq_ignore_ascii_case("as")
                || w.eq_ignore_ascii_case("if")
                || w.eq_ignore_ascii_case("with")
        }) && let Ok(blocked_subj) = try_parse(ts, parse_subject_ref)
        {
            return Ok(RuleRestrictionKind::CantBlockSubject(Box::new(
                blocked_subj,
            )));
        }
        return Ok(RuleRestrictionKind::CantBlock);
    }
    if ts.eat_phrase(&["be", "blocked"]) {
        if ts.eat_phrase(&["by", "more", "than", "one", "creature"]) {
            return Ok(RuleRestrictionKind::CantBeBlockedByMoreThan(1));
        }
        if ts.eat_phrase(&["except", "by"]) {
            let ncp = ts.checkpoint();
            if let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&["or", "more", "creatures"])
            {
                return Ok(RuleRestrictionKind::CantBeBlockedExceptByCount(
                    u8::try_from(n).unwrap_or(2),
                ));
            }
            ts.restore(ncp);
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)));
        }
        if ts.eat_word("by") {
            let flt = parse_object_filter(ts)?;
            return Ok(RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)));
        }
        return Ok(RuleRestrictionKind::CantBeBlocked);
    }
    if ts.eat_phrase(&["be", "countered"]) {
        return Ok(RuleRestrictionKind::CantBeCountered);
    }
    if ts.eat_phrase(&["be", "cast"]) {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    Err(ts.err("expected restriction after `can't`"))
}
