//! Subject-headed static abilities (`<subj> gets +X/+Y`, `<subj> has <kws>`, `<subj> can't ...`, etc.) for `v2`.

use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_subject_ref,
};
use crate::v2::ir::{
    ConditionExpr, RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectStaticRule, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

use super::cond_ext::{parse_static_condition, parse_static_scale_expr};
use super::subject_rules_ext::{
    eat_doesnt_untap_during_untap_step, finish_subject_static, parse_cant_rule,
    parse_extended_subject_predicate, parse_static_subject_tail, try_compile_quoted_grant,
    try_parse_extended_landwalk, try_parse_extended_prot_or_hexproof,
};
use super::{and_cond, wrap_compound};

fn is_subject_predicate_verb(w: &str, allow_also: bool) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "enter"
            | "enters"
            | "get"
            | "gets"
            | "have"
            | "has"
            | "can't"
            | "can"
            | "don't"
            | "doesn't"
            | "are"
            | "is"
            | "lose"
            | "loses"
            | "attack"
            | "attacks"
            | "must"
    ) || (allow_also && w.eq_ignore_ascii_case("also"))
}

fn eat_colors_subtypes_types_comma_and(
    ts: &mut TokenStream<'_>,
    allow_supertype: bool,
) -> (crate::ast::ColorMask, Vec<String>, bool, bool) {
    let mut colors = crate::ast::ColorMask::EMPTY;
    let mut subtypes = Vec::new();
    let mut saw_card_type = false;
    let mut saw_supertype = false;
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if let Some(c) = crate::vocab::lookup_color(&low) {
            ts.next_word();
            colors = colors.union(c);
        } else if allow_supertype && crate::vocab::lookup_supertype(&low).is_some() {
            ts.next_word();
            saw_supertype = true;
        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
            ts.next_word();
            subtypes.push(sub.to_string());
        } else if crate::vocab::lookup_card_type(&low).is_some() {
            ts.next_word();
            saw_card_type = true;
        } else {
            break;
        }
        let sep_cp = ts.checkpoint();
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        let more = ts.peek_word().is_some_and(|nw| {
            let nl = nw.to_ascii_lowercase();
            crate::vocab::lookup_color(&nl).is_some()
                || (allow_supertype && crate::vocab::lookup_supertype(&nl).is_some())
                || ts.vocab.lookup_subtype(&nl).is_some()
                || crate::vocab::lookup_card_type(&nl).is_some()
        });
        if !more {
            ts.restore(sep_cp);
            break;
        }
    }
    (colors, subtypes, saw_card_type, saw_supertype)
}

fn eat_colors_subtypes_types_and(
    ts: &mut TokenStream<'_>,
    allow_colorless: bool,
) -> (bool, crate::ast::ColorMask, Vec<String>, bool) {
    let mut is_colorless = false;
    let mut colors = crate::ast::ColorMask::EMPTY;
    let mut subtypes = Vec::new();
    let mut saw_type = false;
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if allow_colorless && low == "colorless" {
            ts.next_word();
            is_colorless = true;
        } else if let Some(c) = crate::vocab::lookup_color(&low) {
            ts.next_word();
            colors = colors.union(c);
        } else if low == "and" {
            let acp = ts.checkpoint();
            ts.next_word();
            if ts.peek_word().is_some_and(|nw| {
                let nl = nw.to_ascii_lowercase();
                crate::vocab::lookup_color(&nl).is_some()
                    || crate::vocab::lookup_card_type(&nl).is_some()
                    || ts.vocab.lookup_subtype(&nl).is_some()
            }) {
                continue;
            }
            ts.restore(acp);
            break;
        } else if crate::vocab::lookup_card_type(&low).is_some() {
            ts.next_word();
            saw_type = true;
        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
            ts.next_word();
            subtypes.push(sub.to_string());
        } else {
            break;
        }
    }
    (is_colorless, colors, subtypes, saw_type)
}

pub(super) fn parse_subject_static_ability(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 3. Subject-headed static abilities (`<subj> gets/get +X/+Y [and has/have <kws>]`,
    //    `<subj> can't be the targets of <flt>`, `<subj> can't attack`,
    //    `<flt1> and <flt2> enter tapped`, etc.)
    let mut subject = parse_subject_ref(ts)?;
    {
        let conj_cp = ts.checkpoint();
        let mut items = vec![subject.clone()];
        let mut chained = false;
        loop {
            let step_cp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_and = ts.eat_word("and");
            if !had_comma && !had_and {
                ts.restore(step_cp);
                break;
            }
            if had_and {
                let _ = ts.eat_word("all");
            }
            if let Ok(next_subj) = try_parse(ts, parse_subject_ref) {
                items.push(next_subj);
                chained = true;
                if had_and {
                    break;
                }
                continue;
            }
            ts.restore(step_cp);
            break;
        }
        if chained {
            let _ = ts.eat_word("each");
            if ts
                .peek_word()
                .is_some_and(|w| is_subject_predicate_verb(w, false))
            {
                subject = crate::v2::ir::SubjectRef::Compound(items);
            } else {
                ts.restore(conj_cp);
            }
        } else {
            ts.restore(conj_cp);
        }
    }
    // Optional `each` after a compound subject (e.g. `~ and enchanted creature each get ...`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("each")
            && !ts
                .peek_word()
                .is_some_and(|w| is_subject_predicate_verb(w, true))
        {
            ts.restore(ecp);
        }
    }
    // Optional subject post-qualifiers not handled inside `parse_object_filter`
    {
        let qcp = ts.checkpoint();
        let _ = ts.eat_phrase(&["controlled", "by", "players"]);
        if ts.eat_phrase(&["who", "chose"]) && ts.eat_any_word(&["war", "peace"]) {
            // consumed faction choice qualifier
        } else {
            ts.restore(qcp);
            if ts.eat_any_phrase(&[
                &["attacking", "the", "last", "chosen", "player"],
                &["played", "by", "your", "opponents"],
                &[
                    "that",
                    "attacked",
                    "during",
                    "their",
                    "controller's",
                    "last",
                    "turn",
                ],
                &["each", "player", "controls"],
                &["with", "mana", "value", "of", "the", "chosen", "quality"],
                &["without", "mana", "value", "of", "the", "chosen", "quality"],
                &[
                    "with", "mana", "value", "equal", "to", "the", "chosen", "number",
                ],
            ]) || (ts.eat_phrase(&["of", "a", "type", "you", "noted", "for", "cards", "named"])
                && ts.eat_tilde())
                || (ts.eat_phrase(&["that", "you've", "put", "one", "or", "more"])
                    && ts.next_pt_mod().is_some()
                    && ts.eat_phrase(&["counters", "on", "this", "turn"]))
            {
                // consumed qualifier
            } else if ts.eat_word("tokens")
                && ts.peek_word().is_some_and(|w| {
                    matches!(
                        w.to_ascii_lowercase().as_str(),
                        "get" | "gets" | "have" | "has"
                    )
                })
            {
                // consumed trailing `and/or tokens` noun after subtype filter
            } else if ts.eat_phrase(&["that", "are"])
                && ts
                    .peek_word()
                    .is_some_and(|w| ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some())
            {
                ts.next_word();
                if !ts.eat_phrase(&["and/or", "tokens"]) {
                    ts.restore(qcp);
                }
            } else {
                ts.restore(qcp);
            }
        }
    }

    // Try extended subject-headed predicates early
    if let Ok(ext) = try_parse(ts, |s| {
        parse_extended_subject_predicate(s, &subject, &condition)
    }) {
        return Ok(ext);
    }

    // `<subj> [also] gets/get [an additional] +X/+Y [instead] [for each <expr>] ...`
    let also_cp = ts.checkpoint();
    let _ = ts.eat_word("also");
    if ts.eat_any_word(&["gets", "get"]) {
        let _ = ts.eat_phrase(&["an", "additional"]);
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let _ = ts.eat_word("instead");
        let mut scale = opt(ts, parse_static_scale_expr).map(Box::new);
        let mut grant_keywords = Vec::new();
        let mut extra_parts = Vec::new();
        // `and +P/+T for each <expr>`
        {
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Some((p2, t2)) = ts.next_pt_mod()
            {
                let sc2 = opt(ts, parse_static_scale_expr).map(Box::new);
                extra_parts.push(StaticAbility::ContinuousModifyPT {
                    subject: subject.clone(),
                    power: p2,
                    toughness: t2,
                    grant_keywords: Vec::new(),
                    scale: sc2,
                    condition: condition.clone(),
                });
            } else {
                ts.restore(acp);
            }
        }
        {
            let kcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[&["and", "have"], &["and", "has"]])
                || ts.eat_any_word(&["has", "have"])
            {
                if let Some(quoted) = ts.next_quoted() {
                    if let Some(st) =
                        try_compile_quoted_grant(ts, quoted, &subject, &condition, false)
                    {
                        extra_parts.push(st);
                    } else {
                        ts.restore(kcp);
                    }
                } else if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                    grant_keywords = kws;
                } else {
                    ts.restore(kcp);
                }
            } else {
                ts.restore(kcp);
            }
        }
        if scale.is_none() {
            scale = opt(ts, parse_static_scale_expr).map(Box::new);
        }
        loop {
            let rcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "can't"])
                && let Ok(rule) = try_parse(ts, parse_cant_rule)
            {
                extra_parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp);
            let rcp2 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["and", "attacks", "each", "combat", "if", "able"],
                &["and", "attack", "each", "combat", "if", "able"],
                &["and", "attacks", "each", "turn", "if", "able"],
            ]) {
                extra_parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::MustAttack,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp2);
            let rcp3 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["and", "is", "all", "creature", "types"],
                &["and", "are", "all", "creature", "types"],
                &["and", "is", "every", "creature", "type"],
                &["and", "are", "every", "creature", "type"],
            ]) {
                extra_parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::AllCreatureTypes,
                    condition: None,
                });
                continue;
            }
            ts.restore(rcp3);
            let rcp3b = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "can", "block", "an", "additional"]) {
                let extra = ts
                    .next_number(false)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(1);
                if ts.eat_any_word(&["creature", "creatures"])
                    && (ts.eat_phrase(&["each", "combat"]) || ts.is_eof())
                {
                    extra_parts.push(StaticAbility::SubjectRule {
                        subject: subject.clone(),
                        rule: SubjectStaticRule::CanBlockAdditional(extra),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(rcp3b);
            let rcp4 = ts.checkpoint();
            let _ = ts.eat_comma();
            if eat_doesnt_untap_during_untap_step(ts, true) {
                extra_parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::DoesntUntap,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp4);
            let rcp5 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &[
                    "and",
                    "its",
                    "activated",
                    "abilities",
                    "can't",
                    "be",
                    "activated",
                ],
                &[
                    "and",
                    "their",
                    "activated",
                    "abilities",
                    "can't",
                    "be",
                    "activated",
                ],
            ]) {
                let _ = ts.eat_any_phrase(&[
                    &["unless", "they're", "mana", "abilities"],
                    &["unless", "they", "are", "mana", "abilities"],
                ]);
                extra_parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp5);
            let rcp6 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[&["and", "is", "goaded"], &["and", "are", "goaded"]]) {
                extra_parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::AreGoaded,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(rcp6);
            let rcp7 = ts.checkpoint();
            let had_comma7 = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["and", "is", "a"],
                &["and", "is", "an"],
                &["and", "is"],
                &["and", "are"],
            ]) || (had_comma7
                && (ts.eat_any_phrase(&[&["is", "a"], &["is", "an"]])
                    || ts.eat_any_word(&["is", "are"])))
            {
                let (colors, subtypes, saw_card_type, _) =
                    eat_colors_subtypes_types_comma_and(ts, false);
                if !subtypes.is_empty() || !colors.is_empty() || saw_card_type {
                    let keep_existing = ts.eat_in_addition_to_other_types()
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "colors"]);
                    if !colors.is_empty() {
                        extra_parts.push(StaticAbility::SubjectRule {
                            subject: subject.clone(),
                            rule: SubjectStaticRule::SetColors {
                                colors,
                                keep_existing,
                            },
                            condition: None,
                        });
                    }
                    if !subtypes.is_empty() || saw_card_type {
                        extra_parts.push(StaticAbility::SetSubtypes {
                            subject: subject.clone(),
                            subtypes,
                            keep_existing,
                        });
                    }
                    continue;
                }
            }
            ts.restore(rcp7);
            let rcp8 = ts.checkpoint();
            let had_comma8 = ts.eat_comma();
            if (ts.eat_any_phrase(&[&["and", "has"], &["and", "have"]])
                || (had_comma8 && ts.eat_any_word(&["has", "have"])))
                && let Some(quoted) = ts.next_quoted()
                && let Some(st) = try_compile_quoted_grant(ts, quoted, &subject, &condition, false)
            {
                extra_parts.push(st);
                continue;
            }
            ts.restore(rcp8);
            let rcp9 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["and", "loses", "all", "abilities"],
                &["and", "lose", "all", "abilities"],
            ]) {
                extra_parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(rcp9);
            let rcp9b = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[&["and", "loses"], &["and", "lose"]])
                && let Ok(_kws) = try_parse(ts, parse_keyword_list)
            {
                extra_parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(rcp9b);
            let rcp10 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and")
                && let Some(quoted) = ts.next_quoted()
                && let Some(st) = try_compile_quoted_grant(ts, quoted, &subject, &condition, false)
            {
                extra_parts.push(st);
                continue;
            }
            ts.restore(rcp10);
            let rcp11 = ts.checkpoint();
            let had_comma11 = ts.eat_comma();
            if (ts.eat_any_phrase(&[&["and", "has"], &["and", "have"]])
                || (had_comma11 && ts.eat_any_word(&["has", "have"])))
                && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            {
                grant_keywords.extend(kws);
                continue;
            }
            ts.restore(rcp11);
            break;
        }
        if scale.is_none() {
            scale = opt(ts, parse_static_scale_expr).map(Box::new);
        }
        parse_static_subject_tail(ts, &subject, &condition, &mut extra_parts);
        let base = StaticAbility::ContinuousModifyPT {
            subject: subject.clone(),
            power,
            toughness,
            grant_keywords,
            scale,
            condition: condition.clone(),
        };
        // Chained `as long as <cond>, [and] gets/has ...`
        let ala_cp = ts.checkpoint();
        if ts.eat_phrase(&["as", "long", "as"])
            && let Ok(c1) = try_parse(ts, parse_static_condition)
            && ts.eat_comma()
        {
            let mut chain = vec![StaticAbility::ContinuousModifyPT {
                subject: subject.clone(),
                power,
                toughness,
                grant_keywords: Vec::new(),
                scale: None,
                condition: Some(c1),
            }];
            loop {
                let item_cp = ts.checkpoint();
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
                if ts.eat_any_word(&["gets", "get"])
                    && let Some((p2, t2)) = ts.next_pt_mod()
                    && ts.eat_phrase(&["as", "long", "as"])
                    && let Ok(c2) = try_parse(ts, parse_static_condition)
                {
                    chain.push(StaticAbility::ContinuousModifyPT {
                        subject: subject.clone(),
                        power: p2,
                        toughness: t2,
                        grant_keywords: Vec::new(),
                        scale: None,
                        condition: Some(c2),
                    });
                    continue;
                }
                ts.restore(item_cp);
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
                if ts.eat_any_word(&["has", "have"])
                    && let Ok((k2, _)) = try_parse(ts, parse_keyword_list)
                    && ts.eat_phrase(&["as", "long", "as"])
                    && let Ok(c2) = try_parse(ts, parse_static_condition)
                {
                    chain.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: k2,
                        condition: Some(c2),
                    });
                    continue;
                }
                ts.restore(item_cp);
                break;
            }
            if chain.len() > 1 {
                return Ok(StaticAbility::Compound(chain));
            }
        }
        ts.restore(ala_cp);
        if extra_parts.is_empty() {
            return Ok(base);
        }
        let mut all = vec![base];
        all.extend(extra_parts);
        return Ok(StaticAbility::Compound(all));
    }
    ts.restore(also_cp);

    // `<subj> isn't a <type>` / `<subj> aren't <types>`
    if ts.eat_any_word(&["isn't", "aren't"])
        || ts.eat_any_phrase(&[&["is", "not"], &["are", "not"]])
    {
        let _ = ts.eat_any_word(&["a", "an"]);
        let mut types = crate::ast::CardTypeMask::EMPTY;
        while let Some(w) = ts.peek_word() {
            if let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase()) {
                ts.next_word();
                types = types.union(ct);
                let sep_cp = ts.checkpoint();
                if ts.eat_any_word(&["or", "and"])
                    && ts.peek_word().is_some_and(|nw| {
                        crate::vocab::lookup_card_type(&nw.to_ascii_lowercase()).is_some()
                    })
                {
                    continue;
                }
                ts.restore(sep_cp);
                break;
            } else {
                break;
            }
        }
        if !types.is_empty() {
            return finish_subject_static(
                ts,
                &subject,
                &condition,
                vec![StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::RemoveTypes(types),
                    condition: condition.clone(),
                }],
            );
        }
    }

    // `<subj> assigns combat damage equal to its toughness rather than its power`
    if ts.eat_any_phrase(&[
        &[
            "assigns",
            "combat",
            "damage",
            "equal",
            "to",
            "its",
            "toughness",
            "rather",
            "than",
            "its",
            "power",
        ],
        &[
            "assign",
            "combat",
            "damage",
            "equal",
            "to",
            "their",
            "toughness",
            "rather",
            "than",
            "their",
            "power",
        ],
    ]) {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::AssignDamageByToughness,
            condition: condition.clone(),
        }];
        if ts.eat_any_phrase(&[
            &[
                "and", "can", "attack", "as", "though", "it", "didn't", "have", "defender",
            ],
            &[
                "and", "can", "attack", "as", "though", "they", "didn't", "have", "defender",
            ],
        ]) {
            parts.push(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition,
            });
        }
        return Ok(wrap_compound(parts));
    }

    // `<subj> can attack as though it/they didn't have defender` / `can attack as though it/they had haste`
    if ts.eat_any_phrase(&[
        &[
            "can", "attack", "as", "though", "it", "didn't", "have", "defender",
        ],
        &[
            "can", "attack", "as", "though", "they", "didn't", "have", "defender",
        ],
    ]) {
        return finish_subject_static(
            ts,
            &subject,
            &condition,
            vec![StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition: condition.clone(),
            }],
        );
    }
    if ts.eat_any_phrase(&[
        &["can", "attack", "as", "though", "it", "had", "haste"],
        &["can", "attack", "as", "though", "they", "had", "haste"],
    ]) {
        return Ok(StaticAbility::GrantKeywords {
            subject,
            keywords: vec![crate::v2::ir::KeywordSpec {
                kind: crate::v2::ir::KeywordKind::Haste,
                param: None,
            }],
            condition,
        });
    }

    // `<subj> is/are/has/have every [basic | nonbasic] land type [in addition to its/their other types]`
    if ts.eat_any_phrase(&[
        &["is", "every", "basic", "land", "type"],
        &["are", "every", "basic", "land", "type"],
        &["is", "every", "nonbasic", "land", "type"],
        &["are", "every", "nonbasic", "land", "type"],
        &["is", "every", "land", "type"],
        &["are", "every", "land", "type"],
        &["has", "every", "land", "type"],
        &["have", "every", "land", "type"],
    ]) {
        let keep_existing = ts.eat_in_addition_to_other_types();
        return Ok(StaticAbility::SetSubtypes {
            subject,
            subtypes: vec![
                "Plains".to_string(),
                "Island".to_string(),
                "Swamp".to_string(),
                "Mountain".to_string(),
                "Forest".to_string(),
            ],
            keep_existing,
        });
    }
    if ts.eat_any_phrase(&[
        &["has", "every", "creature", "type"],
        &["have", "every", "creature", "type"],
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::AllCreatureTypes,
            condition,
        });
    }

    // `<subj> is/are the chosen [creature] type in addition to its/their other [creature] types`
    {
        let mcp = ts.checkpoint();
        if ts.eat_any_word(&["is", "are"])
            && ts.eat_any_phrase(&[
                &["the", "chosen", "creature", "type"],
                &["the", "chosen", "type"],
            ])
            && ts.eat_in_addition_to_other_types()
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::AdditionalTypesChosen,
                condition: None,
            });
        }
        ts.restore(mcp);
        if ts.eat_any_word(&["is", "are"]) && ts.eat_phrase(&["the", "chosen", "color"]) {
            let _ = ts.eat_any_phrase(&[
                &["in", "addition", "to", "its", "other", "colors"],
                &["in", "addition", "to", "their", "other", "colors"],
            ]);
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::IsChosenColor,
                condition: None,
            });
        }
        ts.restore(mcp);
        if ts.eat_any_word(&["is", "are"]) && ts.eat_phrase(&["the", "chosen", "type"]) {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::IsChosenType,
                condition: None,
            });
        }
        ts.restore(mcp);
    }

    // `<subj> is/are goaded` / `<subj> is/are colorless` / `<subj> is/are all colors`
    if ts.eat_any_phrase(&[&["is", "goaded"], &["are", "goaded"]]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject),
            rule: RuleRestrictionKind::AreGoaded,
            condition,
        });
    }
    if ts.eat_any_phrase(&[&["is", "colorless"], &["are", "colorless"]]) {
        let _ = ts.eat_phrase(&["sources", "of", "damage"]);
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::SetColors {
                colors: crate::ast::ColorMask::EMPTY,
                keep_existing: false,
            },
            condition,
        });
    }
    if ts.eat_any_phrase(&[&["is", "all", "colors"], &["are", "all", "colors"]]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::SetColors {
                colors: crate::ast::ColorMask(0b11111),
                keep_existing: false,
            },
            condition,
        });
    }

    // `<subj> is/are [a/an] <P>/<T> [colors] [subtypes] <types> [with <kws>] [that are still lands | in addition to ...]`
    {
        let pcp = ts.checkpoint();
        if ts.eat_any_word(&["is", "are"])
            && {
                let _ = ts.eat_any_word(&["a", "an"]);
                true
            }
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let (_, colors, subtypes, saw_type) = eat_colors_subtypes_types_and(ts, false);
            if saw_type || !subtypes.is_empty() {
                let mut parts = vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness,
                }];
                let mut kws = Vec::new();
                if ts.eat_word("with")
                    && let Ok((k, _)) = try_parse(ts, parse_keyword_list)
                {
                    kws = k;
                }
                let keep_existing = ts.eat_any_phrase(&[
                    &["that", "are", "still", "lands"],
                    &["that's", "still", "a", "land"],
                    &["that's", "still", "a", "planeswalker"],
                ]) || ts.eat_in_addition_to_other_types();
                if !colors.is_empty() {
                    parts.push(StaticAbility::SubjectRule {
                        subject: subject.clone(),
                        rule: SubjectStaticRule::SetColors {
                            colors,
                            keep_existing: false,
                        },
                        condition: condition.clone(),
                    });
                }
                if !subtypes.is_empty() || keep_existing {
                    parts.push(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes,
                        keep_existing,
                    });
                }
                if !kws.is_empty() {
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: kws,
                        condition: condition.clone(),
                    });
                }
                return finish_subject_static(ts, &subject, &condition, parts);
            }
        }
        ts.restore(pcp);
    }

    // `<subj> is a/an [colorless|colors] [subtypes] <types> [with base power and toughness P/T] [with "<quoted>"] ...`
    {
        let ecp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["is", "a"], &["is", "an"]]) || ts.eat_word("are") {
            let (is_colorless, colors, subtypes, saw_type) =
                eat_colors_subtypes_types_and(ts, true);
            if saw_type || !subtypes.is_empty() {
                let mut parts = Vec::new();
                if is_colorless || !colors.is_empty() {
                    parts.push(StaticAbility::SubjectRule {
                        subject: subject.clone(),
                        rule: SubjectStaticRule::SetColors {
                            colors,
                            keep_existing: false,
                        },
                        condition: condition.clone(),
                    });
                }
                let mut has_rich_tail = is_colorless || !colors.is_empty();
                if ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                    && let Some((power, toughness)) = ts.next_base_pt()
                {
                    has_rich_tail = true;
                    parts.push(StaticAbility::SetBasePT {
                        subject: subject.clone(),
                        power,
                        toughness,
                    });
                    // Optional `, <kws>` and `, and "<quoted>"`
                    loop {
                        let gcp = ts.checkpoint();
                        let had_comma = ts.eat_comma();
                        let had_and = ts.eat_word("and");
                        if !had_comma && !had_and {
                            ts.restore(gcp);
                            break;
                        }
                        if let Some(quoted) = ts.next_quoted() {
                            if let Some(st) =
                                try_compile_quoted_grant(ts, quoted, &subject, &condition, true)
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
                        ts.restore(gcp);
                        break;
                    }
                }
                if ts.eat_phrase(&["with", "power", "and", "toughness", "each", "equal", "to"])
                    && let Ok(val) = try_parse(ts, parse_value_expr)
                {
                    has_rich_tail = true;
                    parts.push(StaticAbility::DynamicBasePT {
                        power: Some(val.clone()),
                        toughness: Some(val),
                    });
                }
                if ts.eat_word("with") {
                    if let Some(quoted) = ts.next_quoted() {
                        if let Some(st) =
                            try_compile_quoted_grant(ts, quoted, &subject, &condition, false)
                        {
                            has_rich_tail = true;
                            parts.push(st);
                        }
                    } else if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                        has_rich_tail = true;
                        parts.push(StaticAbility::GrantKeywords {
                            subject: subject.clone(),
                            keywords: kws,
                            condition: condition.clone(),
                        });
                    }
                }
                let keep_existing = ts.eat_in_addition_to_other_types();
                // Trailing `and has <kws>`, `, and [it] loses all other abilities[, card types, and creature types]`
                loop {
                    let tcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if ts.eat_any_phrase(&[&["and", "has"], &["and", "have"]])
                        && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                    {
                        has_rich_tail = true;
                        parts.push(StaticAbility::GrantKeywords {
                            subject: subject.clone(),
                            keywords: kws,
                            condition: condition.clone(),
                        });
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    if ts.eat_any_phrase(&[
                        &["and", "it", "loses", "all", "other"],
                        &["and", "loses", "all", "other"],
                        &["and", "they", "lose", "all", "other"],
                        &["and", "lose", "all", "other"],
                        &["and", "loses", "all"],
                        &["and", "lose", "all"],
                    ]) {
                        let mut ate_any = false;
                        while ts.eat_any_word(&["abilities", "types"])
                            || ts.eat_any_phrase(&[&["card", "types"], &["creature", "types"]])
                        {
                            ate_any = true;
                            let _ = ts.eat_comma();
                            let _ = ts.eat_word("and");
                        }
                        if ate_any {
                            has_rich_tail = true;
                            parts.push(StaticAbility::RemoveAllAbilities {
                                subject: subject.clone(),
                            });
                            continue;
                        }
                    }
                    ts.restore(tcp);
                    break;
                }
                if has_rich_tail {
                    if !subtypes.is_empty() || keep_existing || saw_type {
                        parts.push(StaticAbility::SetSubtypes {
                            subject: subject.clone(),
                            subtypes,
                            keep_existing,
                        });
                    }
                    return finish_subject_static(ts, &subject, &condition, parts);
                }
            }
        }
        ts.restore(ecp);
    }

    // `<subj> is/are every [basic] land type` / `is/are every creature type`
    {
        let ev_cp = ts.checkpoint();
        if ts.eat_any_word(&["are", "is"]) && ts.eat_word("every") {
            let _ = ts.eat_word("basic");
            if ts.eat_phrase(&["land", "type"]) {
                return Ok(StaticAbility::SetSubtypes {
                    subject,
                    subtypes: vec![
                        "Plains".to_string(),
                        "Island".to_string(),
                        "Swamp".to_string(),
                        "Mountain".to_string(),
                        "Forest".to_string(),
                    ],
                    keep_existing: true,
                });
            }
            if ts.eat_phrase(&["creature", "type"]) {
                return Ok(StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::AllCreatureTypes,
                    condition,
                });
            }
        }
        ts.restore(ev_cp);
    }

    // `<subj> is/are [also] [a/an] <Colors/Supertypes/Subtypes/Types> [in addition to its/their other types]`
    {
        let st_cp = ts.checkpoint();
        if ts.eat_any_word(&["are", "is"]) {
            let _ = ts.eat_word("also");
            let _ = ts.eat_any_word(&["a", "an"]);
            let (colors, subtypes, saw_card_type, saw_supertype) =
                eat_colors_subtypes_types_comma_and(ts, true);
            if !subtypes.is_empty() || saw_card_type || !colors.is_empty() || saw_supertype {
                let keep_existing = ts.eat_in_addition_to_other_types();
                let mut parts = Vec::new();
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
                if !subtypes.is_empty() || saw_card_type || saw_supertype {
                    parts.push(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes,
                        keep_existing,
                    });
                }
                return finish_subject_static(ts, &subject, &condition, parts);
            }
        }
        ts.restore(st_cp);
    }

    // `<subj> lose(s) all abilities [except mana abilities] / all [land|creature] types [and abilities] / <kws> [and can't have or gain <kws>]`
    {
        let lose_cp = ts.checkpoint();
        if ts.eat_any_word(&["lose", "loses"]) {
            let matched_lose = if ts.eat_phrase(&["all", "abilities"]) {
                let _ = ts.eat_phrase(&["except", "mana", "abilities"]);
                true
            } else if ts.eat_any_phrase(&[
                &["all", "land", "types", "and", "abilities"],
                &["all", "creature", "types", "and", "abilities"],
                &["all", "creature", "types"],
                &["all", "land", "types"],
            ]) {
                true
            } else if try_parse(ts, parse_keyword_list).is_ok() {
                if ts.eat_phrase(&["and", "can't", "have", "or", "gain"]) {
                    let _ = try_parse(ts, parse_keyword_list);
                }
                true
            } else {
                false
            };
            if matched_lose {
                let mut parts = vec![StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                }];
                loop {
                    let lcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if ts.eat_any_phrase(&[
                        &["and", "have", "base", "power", "and", "toughness"],
                        &["and", "has", "base", "power", "and", "toughness"],
                    ]) && let Some((power, toughness)) = ts.next_base_pt()
                    {
                        parts.push(StaticAbility::SetBasePT {
                            subject: subject.clone(),
                            power,
                            toughness,
                        });
                        continue;
                    }
                    ts.restore(lcp);
                    let lcp2 = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if ts.eat_any_phrase(&[
                        &["and", "is", "a"],
                        &["and", "is", "an"],
                        &["and", "are"],
                        &["and", "becomes", "a"],
                        &["and", "becomes", "an"],
                    ]) {
                        let mut subtypes = Vec::new();
                        while let Some(w) = ts.peek_word() {
                            let low = w.to_ascii_lowercase();
                            if crate::vocab::lookup_color(&low).is_some()
                                || crate::vocab::lookup_card_type(&low).is_some()
                                || low == "and"
                                || low == "colorless"
                            {
                                ts.next_word();
                            } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                                ts.next_word();
                                subtypes.push(sub.to_string());
                            } else {
                                break;
                            }
                        }
                        if !subtypes.is_empty() {
                            let keep = ts.eat_in_addition_to_other_types();
                            parts.push(StaticAbility::SetSubtypes {
                                subject: subject.clone(),
                                subtypes,
                                keep_existing: keep,
                            });
                        }
                        if ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                            && let Some((power, toughness)) = ts.next_base_pt()
                        {
                            parts.push(StaticAbility::SetBasePT {
                                subject: subject.clone(),
                                power,
                                toughness,
                            });
                        } else if ts.eat_phrase(&[
                            "with",
                            "power",
                            "and",
                            "toughness",
                            "each",
                            "equal",
                            "to",
                        ]) && let Ok(val) = try_parse(ts, parse_value_expr)
                        {
                            parts.push(StaticAbility::DynamicBasePT {
                                power: Some(val.clone()),
                                toughness: Some(val),
                            });
                        }
                        let _ = ts.eat_in_addition_to_other_types();
                        if parts.len() > 1 {
                            continue;
                        }
                    }
                    ts.restore(lcp2);
                    let lcp3 = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if eat_doesnt_untap_during_untap_step(ts, true) {
                        parts.push(StaticAbility::SubjectRule {
                            subject: subject.clone(),
                            rule: SubjectStaticRule::DoesntUntap,
                            condition: condition.clone(),
                        });
                        continue;
                    }
                    ts.restore(lcp3);
                    break;
                }
                return finish_subject_static(ts, &subject, &condition, parts);
            }
        }
        ts.restore(lose_cp);
    }

    // `<subj> has/have ...`
    if ts.eat_any_word(&["has", "have"]) {
        if ts.eat_any_phrase(&[
            &["all", "activated", "abilities", "of"],
            &["all", "activated", "and", "triggered", "abilities", "of"],
        ]) {
            let source = if ts.eat_any_phrase(&[
                &["that", "card"],
                &["that", "creature"],
                &["the", "exiled", "card"],
                &["the", "last", "chosen", "card"],
            ]) {
                crate::v2::ir::ObjectFilter {
                    any_card: true,
                    ..Default::default()
                }
            } else {
                let _ = ts.eat_any_word(&["all", "each"]);
                parse_object_filter(ts)?
            };
            let _ = ts.eat_phrase(&["except", "mana", "abilities"]);
            return finish_subject_static(
                ts,
                &subject,
                &condition,
                vec![StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::CopyActivatedAbilities(source),
                    condition: condition.clone(),
                }],
            );
        }
        if ts.eat_phrase(&["the", "chosen", "creature", "type"])
            && ts.eat_in_addition_to_other_types()
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::AdditionalTypesChosen,
                condition,
            });
        }
        {
            let bp_cp = ts.checkpoint();
            if ts.eat_phrase(&["base", "power"])
                && !ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("and"))
                && let Some(power) = ts.next_number(false)
            {
                return finish_subject_static(
                    ts,
                    &subject,
                    &condition,
                    vec![StaticAbility::SetBasePT {
                        subject: subject.clone(),
                        power,
                        toughness: 0,
                    }],
                );
            }
            ts.restore(bp_cp);
        }
        if ts.eat_phrase(&["base", "toughness"])
            && let Some(toughness) = ts.next_number(false)
        {
            return finish_subject_static(
                ts,
                &subject,
                &condition,
                vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power: 0,
                    toughness,
                }],
            );
        }
        if ts.eat_phrase(&["base", "power", "and", "toughness"]) {
            if ts.eat_phrase(&["each", "equal", "to"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
            {
                return finish_subject_static(
                    ts,
                    &subject,
                    &condition,
                    vec![StaticAbility::DynamicBasePT {
                        power: Some(val.clone()),
                        toughness: Some(val),
                    }],
                );
            }
            if let Some((power, toughness)) = ts.next_base_pt() {
                let mut parts = vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness,
                }];
                if ts.eat_any_phrase(&[&["and", "are"], &["and", "is"]]) {
                    let mut subtypes = Vec::new();
                    while let Some(w) = ts.peek_word() {
                        if let Some(sub) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase()) {
                            ts.next_word();
                            subtypes.push(sub.to_string());
                            let _ = ts.eat_comma();
                            let _ = ts.eat_word("and");
                        } else {
                            break;
                        }
                    }
                    if !subtypes.is_empty() {
                        let keep_existing = ts.eat_in_addition_to_other_types();
                        parts.push(StaticAbility::SetSubtypes {
                            subject: subject.clone(),
                            subtypes,
                            keep_existing,
                        });
                    }
                }
                return finish_subject_static(ts, &subject, &condition, parts);
            }
            if ts.eat_word("x/x")
                || (ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("x"))
                    && matches!(
                        ts.peek_n(1).map(|t| &t.kind),
                        Some(crate::v2::lexer::TokenKind::Slash)
                    )
                    && matches!(
                        ts.peek_n(2).map(|t| &t.kind),
                        Some(crate::v2::lexer::TokenKind::Word(w)) if w.eq_ignore_ascii_case("x")
                    ))
            {
                if !ts.peek_word().is_none_or(|w| !w.eq_ignore_ascii_case("x")) {
                    ts.next_tok();
                    ts.next_tok();
                    ts.next_tok();
                }
                return finish_subject_static(
                    ts,
                    &subject,
                    &condition,
                    vec![StaticAbility::DynamicBasePT {
                        power: Some(ValueExpr::Var(crate::v2::ir::VarName::X)),
                        toughness: Some(ValueExpr::Var(crate::v2::ir::VarName::X)),
                    }],
                );
            }
        }
        if let Some(quoted) = ts.next_quoted() {
            let compiled = ts.compile_quoted_permanent_ability(quoted)?;
            let mut parts = vec![StaticAbility::GrantAbility {
                subject: subject.clone(),
                ability: Box::new(compiled),
                condition: condition.clone(),
            }];
            {
                let q2_cp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Some(q2) = ts.next_quoted()
                    && let Some(st) = try_compile_quoted_grant(ts, q2, &subject, &condition, false)
                {
                    parts.push(st);
                } else {
                    ts.restore(q2_cp);
                }
            }
            return finish_subject_static(ts, &subject, &condition, parts);
        }
        // Special `has` static predicates
        if ts.eat_phrase(&["all", "abilities", "of", "ability", "stickers", "on"]) {
            let _ = opt(ts, parse_subject_ref);
            if ts.eat_word("and") {
                let _ = opt(ts, parse_object_filter);
            }
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
        }
        if ts.eat_phrase(&[
            "all",
            "names",
            "of",
            "nonlegendary",
            "creature",
            "cards",
            "in",
            "addition",
            "to",
            "its",
            "name",
        ]) || (ts.eat_phrase(&["the", "power"])
            && ts.eat_comma()
            && ts.eat_word("toughness")
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "and", "creature", "types", "of", "the", "last", "creature", "card", "exiled",
                "with", "it",
            ]))
            || (ts.eat_phrase(&[
                "the", "full", "text", "of", "that", "card", "and", "has", "the", "text",
            ]) && ts.next_quoted().is_some())
        {
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
        }
        let keywords = if let Some(kw) =
            try_parse_extended_prot_or_hexproof(ts).or_else(|| try_parse_extended_landwalk(ts))
        {
            vec![kw]
        } else {
            // Avoid greedily consuming `and <kw> abilities ...` as a keyword
            let kw_limit = (ts.pos..ts.tokens.len().saturating_sub(2)).find(|&idx| {
                matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("and"))
                    && matches!(ts.tokens[idx + 2].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("abilities"))
            });
            if let Some(lim) = kw_limit
                && lim > ts.pos
            {
                let mut sub = ts.sub_stream(&ts.tokens[ts.pos..lim], ts.stage);
                let kws = parse_keyword_list(&mut sub)?.0;
                ts.pos += sub.pos;
                kws
            } else {
                parse_keyword_list(ts)?.0
            }
        };
        let first_cond = if ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if") {
            Some(and_cond(condition.clone(), parse_static_condition(ts)?))
        } else {
            condition.clone()
        };
        let mut parts = vec![StaticAbility::GrantKeywords {
            subject: subject.clone(),
            keywords,
            condition: first_cond,
        }];
        loop {
            let hcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "can't"])
                && let Ok(rule) = try_parse(ts, parse_cant_rule)
            {
                parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(hcp);
            {
                let kcp = ts.checkpoint();
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
                let _ = ts.eat_any_word(&["has", "have"]);
                if let Ok((more_kws, _)) = try_parse(ts, parse_keyword_list)
                    && (ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if"))
                    && let Ok(c) = try_parse(ts, parse_static_condition)
                {
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: more_kws,
                        condition: Some(and_cond(condition.clone(), c)),
                    });
                    continue;
                }
                ts.restore(kcp);
            }
            {
                let qcp = ts.checkpoint();
                let had_comma = ts.eat_comma();
                let had_and = ts.eat_word("and");
                if (had_comma || had_and)
                    && let Some(quoted) = ts.next_quoted()
                    && let Some(st) =
                        try_compile_quoted_grant(ts, quoted, &subject, &condition, true)
                {
                    let _ = (ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if"))
                        && opt(ts, parse_static_condition).is_some();
                    parts.push(st);
                    continue;
                }
                ts.restore(qcp);
            }
            ts.restore(hcp);
            let lcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["and", "loses", "all", "other", "abilities"],
                &["and", "lose", "all", "other", "abilities"],
                &["and", "it", "loses", "all", "other", "abilities"],
                &["and", "loses", "all", "abilities"],
                &["and", "lose", "all", "abilities"],
            ]) {
                parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(lcp);
            let lkw_cp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[&["and", "loses"], &["and", "lose"]])
                && let Ok(_kws) = try_parse(ts, parse_keyword_list)
            {
                parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(lkw_cp);
            if ts.eat_any_phrase(&[
                &[
                    "and", "can", "attack", "as", "though", "it", "didn't", "have", "defender",
                ],
                &[
                    "and", "can", "attack", "as", "though", "they", "didn't", "have", "defender",
                ],
            ]) {
                parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::CanAttackWithDefender,
                    condition: condition.clone(),
                });
                continue;
            }
            if ts.eat_any_phrase(&[
                &["and", "attack", "each", "combat", "if", "able"],
                &["and", "attacks", "each", "combat", "if", "able"],
            ]) {
                parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::MustAttack,
                    condition: condition.clone(),
                });
                continue;
            }
            break;
        }
        return finish_subject_static(ts, &subject, &condition, parts);
    }

    // `<subj> doesn't untap during its controller's untap step [and its activated abilities can't be activated]`
    if eat_doesnt_untap_during_untap_step(ts, false) {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::DoesntUntap,
            condition: condition.clone(),
        }];
        let acp = ts.checkpoint();
        let _ = ts.eat_comma();
        if ts.eat_any_phrase(&[
            &[
                "and",
                "its",
                "activated",
                "abilities",
                "can't",
                "be",
                "activated",
            ],
            &[
                "and",
                "their",
                "activated",
                "abilities",
                "can't",
                "be",
                "activated",
            ],
        ]) {
            let _ = ts.eat_any_phrase(&[
                &["unless", "they're", "mana", "abilities"],
                &["unless", "they", "are", "mana", "abilities"],
            ]);
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                condition,
            });
        } else {
            ts.restore(acp);
        }
        return Ok(wrap_compound(parts));
    }

    // `<subj> attacks or blocks / blocks / attacks each combat if able`
    if ts.eat_any_phrase(&[
        &["attacks", "or", "blocks", "each", "combat", "if", "able"],
        &["attack", "or", "block", "each", "combat", "if", "able"],
    ]) {
        return Ok(StaticAbility::Compound(vec![
            StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::MustAttack,
                condition: condition.clone(),
            },
            StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::AllMustBlock,
                condition,
            },
        ]));
    }
    if ts.eat_any_phrase(&[
        &["blocks", "each", "combat", "if", "able"],
        &["block", "each", "combat", "if", "able"],
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::AllMustBlock,
            condition,
        });
    }
    if ts.eat_any_phrase(&[
        &["attacks", "each", "combat", "if", "able"],
        &["attack", "each", "combat", "if", "able"],
        &["attacks", "each", "turn", "if", "able"],
        &["attack", "each", "turn", "if", "able"],
    ]) {
        let final_cond = if ts.eat_word("unless") {
            let u = ConditionExpr::Not(Box::new(parse_static_condition(ts)?));
            Some(and_cond(condition, u))
        } else {
            condition
        };
        return finish_subject_static(
            ts,
            &subject,
            &final_cond,
            vec![StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::MustAttack,
                condition: final_cond.clone(),
            }],
        );
    }

    // `<subj> can be blocked as though they/it didn't have <kw>`
    if ts.eat_any_phrase(&[
        &[
            "can", "be", "blocked", "as", "though", "they", "didn't", "have",
        ],
        &[
            "can", "be", "blocked", "as", "though", "it", "didn't", "have",
        ],
    ]) && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
    {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::IgnoreKeywordForBlocking(kw),
            condition: None,
        });
    }

    // `<subj> can block creatures with <kw> as though they didn't have <kw> / as though it had <kw>`
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["can", "block", "creatures", "with"])
            && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
            && ts.eat_any_phrase(&[
                &["as", "though", "they", "didn't", "have"],
                &["as", "though", "it", "had"],
            ])
            && try_parse(ts, crate::v2::filter::parse_keyword_kind).is_ok()
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::IgnoreKeywordForBlocking(kw),
                condition,
            });
        }
        ts.restore(bcp);
    }

    // `<subj> can block only <flt>`
    if ts.eat_phrase(&["can", "block", "only"]) {
        let flt = parse_object_filter(ts)?;
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject),
            rule: RuleRestrictionKind::CanBlockOnly(Box::new(flt)),
            condition,
        });
    }

    // `<subj> can block any number of creatures`
    if ts.eat_phrase(&["can", "block", "any", "number", "of", "creatures"]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::CanBlockAnyNumber,
            condition: None,
        });
    }

    // `<subj> can block an additional [<N>] creature(s) each combat`
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["can", "block", "an", "additional"]) {
            let extra = ts
                .next_number(false)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(1);
            if ts.eat_any_word(&["creature", "creatures"]) && ts.eat_phrase(&["each", "combat"]) {
                let _ = opt(ts, parse_static_scale_expr);
                return Ok(StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::CanBlockAdditional(extra),
                    condition,
                });
            }
        }
        ts.restore(bcp);
    }

    // `<subj> can be attached only to <flt>`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["can", "be", "attached", "only", "to"]) {
            let _ = ts.eat_any_word(&["a", "an"]);
            if let Ok(flt) = try_parse(ts, parse_object_filter) {
                return Ok(StaticAbility::FilterRule {
                    filter: flt,
                    kind: crate::v2::ir::FilterStaticKind::AttachmentRestriction,
                });
            }
        }
        ts.restore(acp);
    }

    // `<subj> enter(s) tapped / untapped / under the control of an opponent`
    if ts.eat_any_phrase(&[&["enters", "untapped"], &["enter", "untapped"]]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::EntersUntapped,
            condition,
        });
    }
    if ts.eat_phrase(&[
        "enters", "under", "the", "control", "of", "an", "opponent", "of", "your", "choice",
    ]) {
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::EntersUnderOpponentControl,
        ));
    }
    if ts.eat_any_phrase(&[&["enters", "tapped"], &["enter", "tapped"]]) {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::EntersTapped,
            condition: condition.clone(),
        }];
        if eat_doesnt_untap_during_untap_step(ts, true) {
            parts.push(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::DoesntUntap,
                condition,
            });
        }
        return Ok(wrap_compound(parts));
    }
    if ts.eat_any_phrase(&[&["enters", "with"], &["enter", "with"]]) {
        let mut additional = ts.eat_phrase(&["an", "additional"]) || ts.eat_word("additional");
        let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else if ts.eat_any_word(&["a", "an"]) {
            Some(ValueExpr::Const(1))
        } else {
            Some(try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1)))
        };
        if ts.eat_word("additional") {
            additional = true;
        }
        let kind = parse_counter_kind(ts)?;
        if ts.eat_any_word(&["counter", "counters"])
            && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
        {
            let count = if let Some(c) = count_opt {
                if let Some(scale) = opt(ts, parse_static_scale_expr) {
                    ValueExpr::Mul(Box::new(c), Box::new(scale))
                } else {
                    c
                }
            } else if ts.eat_phrase(&["equal", "to"]) {
                super::cond_ext::parse_static_value_expr(ts)?
            } else {
                return Err(ts.err("expected `equal to <val>` after `a number of counters`"));
            };
            // Optional `and as a <Subtype> in addition to its other types`
            let mut extra_sub: Option<StaticAbility> = None;
            {
                let m_cp = ts.checkpoint();
                if ts.eat_any_phrase(&[&["and", "as", "a"], &["and", "as", "an"]])
                    && let Some(sub) = ts
                        .peek_word()
                        .and_then(|w| ts.vocab.lookup_subtype(&w.to_ascii_lowercase()))
                {
                    let sub_owned = sub.to_string();
                    ts.next_word();
                    let keep = ts.eat_in_addition_to_other_types();
                    extra_sub = Some(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes: vec![sub_owned],
                        keep_existing: keep,
                    });
                } else {
                    ts.restore(m_cp);
                }
            }
            let final_cond = if ts.eat_word("if") {
                Some(and_cond(condition, parse_static_condition(ts)?))
            } else if ts.eat_word("unless") {
                Some(and_cond(
                    condition,
                    ConditionExpr::Not(Box::new(parse_static_condition(ts)?)),
                ))
            } else {
                condition
            };
            let base = StaticAbility::EntersWithCountersStatic {
                subject,
                kind,
                count,
                additional,
                condition: final_cond,
            };
            return Ok(if let Some(sub_st) = extra_sub {
                StaticAbility::Compound(vec![base, sub_st])
            } else {
                base
            });
        }
    }

    // `<subj> enter(s) as a copy of <subj2>`
    {
        let ccp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["enter", "as", "a", "copy", "of"],
            &["enters", "as", "a", "copy", "of"],
        ]) && try_parse(ts, parse_subject_ref).is_ok()
        {
            let filter = match subject {
                crate::v2::ir::SubjectRef::Group(f) | crate::v2::ir::SubjectRef::All(f) => *f,
                _ => crate::v2::ir::ObjectFilter::default(),
            };
            return Ok(StaticAbility::ConditionalReplacement {
                condition: condition.unwrap_or(ConditionExpr::Flag(
                    crate::v2::ir::ConditionFlag::ControlsSelf,
                )),
                replacement: Box::new(crate::v2::ir::Replacement::EnterAsCopy {
                    optional: false,
                    filter,
                    exceptions: Vec::new(),
                }),
            });
        }
        ts.restore(ccp);
    }

    // `<subj> must be blocked [by ...] if able`
    if ts.eat_any_phrase(&[
        &["must", "be", "blocked", "if", "able"],
        &[
            "must", "be", "blocked", "by", "exactly", "one", "creature", "if", "able",
        ],
        &[
            "must",
            "be",
            "blocked",
            "by",
            "two",
            "or",
            "more",
            "creatures",
            "if",
            "able",
        ],
        &[
            "must",
            "be",
            "blocked",
            "by",
            "all",
            "creatures",
            "able",
            "to",
            "block",
            "it",
        ],
    ]) {
        return finish_subject_static(
            ts,
            &subject,
            &condition,
            vec![StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: RuleRestrictionKind::MustBeBlocked,
                condition: condition.clone(),
            }],
        );
    }

    // `<subj> can't attack [or block] unless its/their controller pays <cost> | you <cost> [for each <expr>]`
    {
        let ucp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["can't", "attack", "or", "block", "unless"],
            &["can't", "attack", "unless"],
            &["can't", "block", "unless"],
        ]) && (ts.eat_any_phrase(&[
            &["its", "controller", "pays"],
            &["their", "controller", "pays"],
            &["you", "pay"],
        ]) || ts.eat_word("you"))
            && let Ok(cost) = try_parse(ts, crate::v2::cost::parse_cost_atom)
        {
            let per_creature = crate::v2::value::parse_for_each_expr(ts).is_ok();
            return Ok(StaticAbility::AttackTax {
                target: subject,
                cost,
                per_creature,
            });
        }
        ts.restore(ucp);
    }

    // `<subj> can't attack/block <tgt> unless their/its controller pays <cost> [for each <expr>]`
    {
        let acp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["can't", "attack"], &["can't", "block"]])
            && let Ok(target) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("unless")
            && ts.eat_any_phrase(&[
                &["their", "controller", "pays"],
                &["its", "controller", "pays"],
            ])
            && let Ok(cost) = try_parse(ts, crate::v2::cost::parse_cost_atom)
        {
            let (cost, scaled_per) = match cost {
                crate::v2::ir::Cost::ScaledMana {
                    base,
                    scale:
                        crate::v2::ir::ValueExpr::PriorActionCount(
                            crate::v2::ir::PriorActionKind::ThatMany,
                        ),
                } => (crate::v2::ir::Cost::Mana(base), true),
                other => (other, false),
            };
            let per_creature = scaled_per
                || ts.eat_any_phrase(&[
                    &["for", "each", "of", "those", "creatures"],
                    &[
                        "for",
                        "each",
                        "creature",
                        "they",
                        "control",
                        "that's",
                        "attacking",
                        "you",
                    ],
                    &["for", "each", "blocking", "creature", "they", "control"],
                ])
                || opt(ts, parse_static_scale_expr).is_some();
            return Ok(StaticAbility::AttackTax {
                target,
                cost,
                per_creature,
            });
        }
        ts.restore(acp);
    }

    // `<subj> crews Vehicles as though its power were <N> greater`
    {
        let vcp = ts.checkpoint();
        if ts.eat_phrase(&["crews", "vehicles", "as", "though", "its", "power", "were"])
            && let Some(n) = ts.next_number(false).and_then(|v| u8::try_from(v).ok())
            && ts.eat_word("greater")
        {
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CrewPowerBonus(
                i32::from(n),
            )));
        }
        ts.restore(vcp);
    }

    // `<subj> can't ... [as long as / if <cond>]`
    if ts.eat_word("can't") {
        let first_rule = parse_cant_rule(ts)?;
        let trailing_cond = if ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if") {
            Some(and_cond(condition.clone(), parse_static_condition(ts)?))
        } else {
            condition.clone()
        };
        let mut parts = vec![StaticAbility::RuleRestriction {
            subject: Some(subject.clone()),
            rule: first_rule,
            condition: trailing_cond,
        }];
        loop {
            if ts.eat_phrase(&["and", "can't"]) {
                let next_rule = parse_cant_rule(ts)?;
                parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: next_rule,
                    condition: condition.clone(),
                });
                continue;
            }
            let acp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["and", "its", "activated", "abilities"],
                &["and", "their", "activated", "abilities"],
            ]) && {
                let _ = ts.eat_word("with")
                    && matches!(
                        ts.peek().map(|t| &t.kind),
                        Some(crate::v2::lexer::TokenKind::Mana(
                            crate::v2::ir::ManaSymbol::Tap
                        ))
                    )
                    && {
                        ts.next_tok();
                        ts.eat_phrase(&["in", "their", "costs"])
                    };
                ts.eat_phrase(&["can't", "be", "activated"])
            } {
                let _ = ts.eat_any_phrase(&[
                    &["unless", "they're", "mana", "abilities"],
                    &["unless", "they", "are", "mana", "abilities"],
                ]);
                parts.push(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(acp);
            let hcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[&["and", "has"], &["and", "have"]])
                && let Some(quoted) = ts.next_quoted()
                && let Some(st) = try_compile_quoted_grant(ts, quoted, &subject, &condition, false)
            {
                parts.push(st);
                continue;
            }
            ts.restore(hcp);
            let kcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[&["and", "has"], &["and", "have"]])
                && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
            {
                parts.push(StaticAbility::GrantKeywords {
                    subject: subject.clone(),
                    keywords,
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(kcp);
            break;
        }
        return finish_subject_static(ts, &subject, &condition, parts);
    }

    Err(ts.err("expected static ability predicate"))
}
