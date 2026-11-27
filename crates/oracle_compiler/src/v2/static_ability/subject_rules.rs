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
    parse_cant_rule, parse_extended_subject_predicate, parse_static_subject_tail,
    try_parse_extended_landwalk, try_parse_extended_prot_or_hexproof,
};

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
            if ts.peek_word().is_some_and(|w| {
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
                )
            }) {
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
            && !ts.peek_word().is_some_and(|w| {
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
                        | "also"
                )
            })
        {
            ts.restore(ecp);
        }
    }
    // Optional subject post-qualifiers not handled inside `parse_object_filter`
    {
        let qcp = ts.checkpoint();
        if ts.eat_phrase(&["controlled", "by", "players", "who", "chose"])
            && (ts.eat_word("war") || ts.eat_word("peace"))
        {
            // consumed `Archangel of Strife` qualifier
        } else if ts.eat_phrase(&["attacking", "the", "last", "chosen", "player"])
            || ts.eat_phrase(&["played", "by", "your", "opponents"])
            || ts.eat_phrase(&[
                "that",
                "attacked",
                "during",
                "their",
                "controller's",
                "last",
                "turn",
            ])
            || ts.eat_phrase(&["each", "player", "controls"])
            || ts.eat_phrase(&["with", "mana", "value", "of", "the", "chosen", "quality"])
            || ts.eat_phrase(&["without", "mana", "value", "of", "the", "chosen", "quality"])
            || ts.eat_phrase(&[
                "with", "mana", "value", "equal", "to", "the", "chosen", "number",
            ])
            || ts.eat_phrase(&[
                "that", "you've", "put", "one", "or", "more", "+1/+1", "counters", "on", "this",
                "turn",
            ])
        {
            // consumed qualifier
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

    // Try extended subject-headed predicates early for `gets +X/+Y`, `has all loyalty abilities`, `has protection from ...`, etc.
    if let Ok(ext) = try_parse(ts, |s| {
        parse_extended_subject_predicate(s, &subject, &condition)
    }) {
        return Ok(ext);
    }

    // `<subj> [also] gets/get [an additional] +X/+Y [instead] [for each <expr>] ...`
    let also_cp = ts.checkpoint();
    let _ = ts.eat_word("also");
    if ts.eat_word("gets") || ts.eat_word("get") {
        let _ = ts.eat_phrase(&["an", "additional"]);
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let _ = ts.eat_word("instead");
        let mut scale = opt(ts, parse_static_scale_expr).map(Box::new);
        let mut grant_keywords = Vec::new();
        let mut extra_parts = Vec::new();
        // `and +P/+T for each <expr>` (`Eidolon of Countless Battles`)
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
            if ts.eat_phrase(&["and", "have"])
                || ts.eat_phrase(&["and", "has"])
                || ts.eat_word("has")
                || ts.eat_word("have")
            {
                if let Some(quoted) = ts.next_quoted() {
                    let compiled = crate::v2::card::compile_face_v2(
                        crate::FaceInput {
                            name: "~",
                            type_line: "Permanent",
                            oracle_text: quoted,
                        },
                        ts.vocab,
                    );
                    if compiled.is_ok() {
                        extra_parts.push(StaticAbility::GrantAbility {
                            subject: subject.clone(),
                            ability: Box::new(compiled),
                            condition: condition.clone(),
                        });
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
            if ts.eat_phrase(&["and", "attacks", "each", "combat", "if", "able"])
                || ts.eat_phrase(&["and", "attack", "each", "combat", "if", "able"])
                || ts.eat_phrase(&["and", "attacks", "each", "turn", "if", "able"])
            {
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
            if ts.eat_phrase(&["and", "is", "all", "creature", "types"])
                || ts.eat_phrase(&["and", "are", "all", "creature", "types"])
                || ts.eat_phrase(&["and", "is", "every", "creature", "type"])
                || ts.eat_phrase(&["and", "are", "every", "creature", "type"])
            {
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
                if (ts.eat_word("creature") || ts.eat_word("creatures"))
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
            if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
                || ts.eat_phrase(&["and", "don't", "untap", "during"]))
                && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
                    || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
                    || ts.eat_phrase(&["your", "untap", "step"]))
            {
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
            if ts.eat_phrase(&[
                "and",
                "its",
                "activated",
                "abilities",
                "can't",
                "be",
                "activated",
            ]) || ts.eat_phrase(&[
                "and",
                "their",
                "activated",
                "abilities",
                "can't",
                "be",
                "activated",
            ]) {
                let _ = ts.eat_phrase(&["unless", "they're", "mana", "abilities"])
                    || ts.eat_phrase(&["unless", "they", "are", "mana", "abilities"]);
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
            if ts.eat_phrase(&["and", "is", "goaded"]) || ts.eat_phrase(&["and", "are", "goaded"]) {
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
            if ts.eat_phrase(&["and", "is", "a"])
                || ts.eat_phrase(&["and", "is", "an"])
                || ts.eat_phrase(&["and", "is"])
                || ts.eat_phrase(&["and", "are"])
                || (had_comma7
                    && (ts.eat_phrase(&["is", "a"])
                        || ts.eat_phrase(&["is", "an"])
                        || ts.eat_word("is")
                        || ts.eat_word("are")))
            {
                let mut colors = crate::ast::ColorMask::EMPTY;
                let mut subtypes = Vec::new();
                let mut saw_card_type = false;
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
                        saw_card_type = true;
                    } else {
                        break;
                    }
                    let sep_cp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    let next_matches = ts.peek_word().is_some_and(|nw| {
                        let nl = nw.to_ascii_lowercase();
                        crate::vocab::lookup_color(&nl).is_some()
                            || ts.vocab.lookup_subtype(&nl).is_some()
                            || crate::vocab::lookup_card_type(&nl).is_some()
                    });
                    if !next_matches {
                        ts.restore(sep_cp);
                        break;
                    }
                }
                if !subtypes.is_empty() || !colors.is_empty() || saw_card_type {
                    let keep_existing = ts.eat_phrase(&[
                        "in", "addition", "to", "its", "other", "colors", "and", "types",
                    ]) || ts.eat_phrase(&[
                        "in", "addition", "to", "their", "other", "colors", "and", "types",
                    ]) || ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                        || ts.eat_phrase(&[
                            "in", "addition", "to", "their", "other", "creature", "types",
                        ])
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
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
            if (ts.eat_phrase(&["and", "has"])
                || ts.eat_phrase(&["and", "have"])
                || (had_comma8 && (ts.eat_word("has") || ts.eat_word("have"))))
                && let Some(quoted) = ts.next_quoted()
            {
                let compiled = crate::v2::card::compile_face_v2(
                    crate::FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if compiled.is_ok() {
                    extra_parts.push(StaticAbility::GrantAbility {
                        subject: subject.clone(),
                        ability: Box::new(compiled),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(rcp8);
            let rcp9 = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "loses", "all", "abilities"])
                || ts.eat_phrase(&["and", "lose", "all", "abilities"])
            {
                extra_parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(rcp9);
            let rcp9b = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "loses"]) || ts.eat_phrase(&["and", "lose"]))
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
            {
                let compiled = crate::v2::card::compile_face_v2(
                    crate::FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if compiled.is_ok() {
                    extra_parts.push(StaticAbility::GrantAbility {
                        subject: subject.clone(),
                        ability: Box::new(compiled),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(rcp10);
            let rcp11 = ts.checkpoint();
            let had_comma11 = ts.eat_comma();
            if (ts.eat_phrase(&["and", "has"])
                || ts.eat_phrase(&["and", "have"])
                || (had_comma11 && (ts.eat_word("has") || ts.eat_word("have"))))
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
        // Chained `as long as <cond>, [and] gets/has ...` (`Tek`)
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
                if (ts.eat_word("gets") || ts.eat_word("get"))
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
                if (ts.eat_word("has") || ts.eat_word("have"))
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

    // `<subj> isn't a <type>` / `<subj> aren't <types>` (e.g. Theros Gods: `~ isn't a creature`)
    if ts.eat_word("isn't")
        || ts.eat_phrase(&["is", "not"])
        || ts.eat_word("aren't")
        || ts.eat_phrase(&["are", "not"])
    {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let mut types = crate::ast::CardTypeMask::EMPTY;
        while let Some(w) = ts.peek_word() {
            if let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase()) {
                ts.next_word();
                types = types.union(ct);
                let sep_cp = ts.checkpoint();
                if (ts.eat_word("or") || ts.eat_word("and"))
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
            let mut parts = vec![StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::RemoveTypes(types),
                condition: condition.clone(),
            }];
            parse_static_subject_tail(ts, &subject, &condition, &mut parts);
            return Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                StaticAbility::Compound(parts)
            });
        }
    }

    // `<subj> assigns combat damage equal to its toughness rather than its power`
    if ts.eat_phrase(&[
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
    ]) || ts.eat_phrase(&[
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
    ]) {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::AssignDamageByToughness,
            condition: condition.clone(),
        }];
        if ts.eat_phrase(&[
            "and", "can", "attack", "as", "though", "it", "didn't", "have", "defender",
        ]) || ts.eat_phrase(&[
            "and", "can", "attack", "as", "though", "they", "didn't", "have", "defender",
        ]) {
            parts.push(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition,
            });
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> can attack as though it/they didn't have defender` / `can attack as though it/they had haste`
    if ts.eat_phrase(&[
        "can", "attack", "as", "though", "it", "didn't", "have", "defender",
    ]) || ts.eat_phrase(&[
        "can", "attack", "as", "though", "they", "didn't", "have", "defender",
    ]) {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::CanAttackWithDefender,
            condition: condition.clone(),
        }];
        parse_static_subject_tail(ts, &subject, &condition, &mut parts);
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }
    if ts.eat_phrase(&["can", "attack", "as", "though", "it", "had", "haste"])
        || ts.eat_phrase(&["can", "attack", "as", "though", "they", "had", "haste"])
    {
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
    if ts.eat_phrase(&["is", "every", "basic", "land", "type"])
        || ts.eat_phrase(&["are", "every", "basic", "land", "type"])
        || ts.eat_phrase(&["is", "every", "nonbasic", "land", "type"])
        || ts.eat_phrase(&["are", "every", "nonbasic", "land", "type"])
        || ts.eat_phrase(&["is", "every", "land", "type"])
        || ts.eat_phrase(&["are", "every", "land", "type"])
        || ts.eat_phrase(&["has", "every", "land", "type"])
        || ts.eat_phrase(&["have", "every", "land", "type"])
    {
        let keep_existing = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
            || ts.eat_phrase(&["in", "addition", "to", "its", "other", "land", "types"])
            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "land", "types"]);
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
    if ts.eat_phrase(&["has", "every", "creature", "type"])
        || ts.eat_phrase(&["have", "every", "creature", "type"])
    {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::AllCreatureTypes,
            condition,
        });
    }

    // `<subj> is/are the chosen [creature] type in addition to its/their other [creature] types`
    {
        let mcp = ts.checkpoint();
        if (ts.eat_word("is") || ts.eat_word("are"))
            && (ts.eat_phrase(&["the", "chosen", "creature", "type", "in", "addition", "to"])
                || ts.eat_phrase(&["the", "chosen", "type", "in", "addition", "to"]))
            && (ts.eat_phrase(&["its", "other", "types"])
                || ts.eat_phrase(&["their", "other", "types"])
                || ts.eat_phrase(&["its", "other", "creature", "types"])
                || ts.eat_phrase(&["their", "other", "creature", "types"]))
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::AdditionalTypesChosen,
                condition: None,
            });
        }
        ts.restore(mcp);
        if (ts.eat_word("is") || ts.eat_word("are")) && ts.eat_phrase(&["the", "chosen", "color"]) {
            let _ = ts.eat_phrase(&["in", "addition", "to", "its", "other", "colors"])
                || ts.eat_phrase(&["in", "addition", "to", "their", "other", "colors"]);
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::IsChosenColor,
                condition: None,
            });
        }
        ts.restore(mcp);
        if (ts.eat_word("is") || ts.eat_word("are")) && ts.eat_phrase(&["the", "chosen", "type"]) {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::IsChosenType,
                condition: None,
            });
        }
        ts.restore(mcp);
    }

    // `<subj> is/are goaded` / `<subj> is/are colorless` / `<subj> is/are all colors`
    if ts.eat_phrase(&["is", "goaded"]) || ts.eat_phrase(&["are", "goaded"]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject),
            rule: RuleRestrictionKind::AreGoaded,
            condition,
        });
    }
    if ts.eat_phrase(&["is", "colorless"]) || ts.eat_phrase(&["are", "colorless"]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::SetColors {
                colors: crate::ast::ColorMask::EMPTY,
                keep_existing: false,
            },
            condition,
        });
    }
    if ts.eat_phrase(&["is", "all", "colors"]) || ts.eat_phrase(&["are", "all", "colors"]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::SetColors {
                colors: crate::ast::ColorMask(0b11111),
                keep_existing: false,
            },
            condition,
        });
    }

    // `<subj> is/are [a/an] <P>/<T> [colors] [subtypes] <types> [with <kws>] [that are still lands | in addition to ...]` (`Awaken the Ancient`, `Vastwood Zendikon`, `Nature's Revolt`)
    {
        let pcp = ts.checkpoint();
        if (ts.eat_word("is") || ts.eat_word("are"))
            && {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                true
            }
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(c) = crate::vocab::lookup_color(&low) {
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
                let keep_existing = ts.eat_phrase(&["that", "are", "still", "lands"])
                    || ts.eat_phrase(&["that's", "still", "a", "land"])
                    || ts.eat_phrase(&["that's", "still", "a", "planeswalker"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "land", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "land", "types"]);
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
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        ts.restore(pcp);
    }

    // `<subj> is a/an [colorless|colors] [subtypes] <types> [with base power and toughness P/T] [with "<quoted>"] [and has <kws>] [, and [it] loses all other abilities/types]` (`Ensoul Artifact`, `Darksteel Mutation`, `Imprisoned in the Moon`, `Minimus Containment`, `Sugar Coat`, `In Too Deep`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["is", "a"]) || ts.eat_phrase(&["is", "an"]) || ts.eat_word("are") {
            let mut is_colorless = false;
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if low == "colorless" {
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
                    // Optional `, <kws>` and `, and "<quoted>"` (`Goddric, Cloaked Reveler`)
                    loop {
                        let gcp = ts.checkpoint();
                        let had_comma = ts.eat_comma();
                        let had_and = ts.eat_word("and");
                        if !had_comma && !had_and {
                            ts.restore(gcp);
                            break;
                        }
                        if let Some(quoted) = ts.next_quoted() {
                            let compiled = crate::v2::card::compile_face_v2(
                                crate::FaceInput {
                                    name: "~",
                                    type_line: "Permanent",
                                    oracle_text: quoted.trim_end_matches(','),
                                },
                                ts.vocab,
                            );
                            if compiled.is_ok() {
                                parts.push(StaticAbility::GrantAbility {
                                    subject: subject.clone(),
                                    ability: Box::new(compiled),
                                    condition: condition.clone(),
                                });
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
                        let compiled = crate::v2::card::compile_face_v2(
                            crate::FaceInput {
                                name: "~",
                                type_line: "Permanent",
                                oracle_text: quoted,
                            },
                            ts.vocab,
                        );
                        if compiled.is_ok() {
                            has_rich_tail = true;
                            parts.push(StaticAbility::GrantAbility {
                                subject: subject.clone(),
                                ability: Box::new(compiled),
                                condition: condition.clone(),
                            });
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
                let keep_existing = ts
                    .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "land", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "land", "types"]);
                // Trailing `and has <kws>`, `, and [it] loses all other abilities[, card types, and creature types]`
                loop {
                    let tcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if (ts.eat_phrase(&["and", "has"]) || ts.eat_phrase(&["and", "have"]))
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
                    if ts.eat_phrase(&["and", "it", "loses", "all", "other"])
                        || ts.eat_phrase(&["and", "loses", "all", "other"])
                        || ts.eat_phrase(&["and", "they", "lose", "all", "other"])
                        || ts.eat_phrase(&["and", "lose", "all", "other"])
                        || ts.eat_phrase(&["and", "loses", "all"])
                        || ts.eat_phrase(&["and", "lose", "all"])
                    {
                        let mut ate_any = false;
                        while ts.eat_word("abilities")
                            || ts.eat_phrase(&["card", "types"])
                            || ts.eat_phrase(&["creature", "types"])
                            || ts.eat_word("types")
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
                    parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                    return Ok(if parts.len() == 1 {
                        parts.remove(0)
                    } else {
                        StaticAbility::Compound(parts)
                    });
                }
            }
        }
        ts.restore(ecp);
    }

    // `<subj> is/are every [basic] land type` / `is/are every creature type` (`Omo, Queen of Vesuva`)
    {
        let ev_cp = ts.checkpoint();
        if (ts.eat_word("are") || ts.eat_word("is")) && ts.eat_word("every") {
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
        if ts.eat_word("are") || ts.eat_word("is") {
            let _ = ts.eat_word("also");
            let _ = ts.eat_word("a") || ts.eat_word("an");
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            let mut saw_card_type = false;
            let mut saw_supertype = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(c) = crate::vocab::lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else if crate::vocab::lookup_supertype(&low).is_some() {
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
                        || crate::vocab::lookup_supertype(&nl).is_some()
                        || ts.vocab.lookup_subtype(&nl).is_some()
                        || crate::vocab::lookup_card_type(&nl).is_some()
                });
                if !more {
                    ts.restore(sep_cp);
                    break;
                }
            }
            if !subtypes.is_empty() || saw_card_type || !colors.is_empty() || saw_supertype {
                let keep_existing = ts
                    .eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&[
                        "in", "addition", "to", "their", "other", "creature", "types",
                    ])
                    || ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "land", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "land", "types"]);
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
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        ts.restore(st_cp);
    }

    // `<subj> lose(s) all abilities [except mana abilities] / all [land|creature] types [and abilities] / <kws> [and can't have or gain <kws>]`
    {
        let lose_cp = ts.checkpoint();
        if ts.eat_word("lose") || ts.eat_word("loses") {
            let matched_lose = if ts.eat_phrase(&["all", "abilities"]) {
                let _ = ts.eat_phrase(&["except", "mana", "abilities"]);
                true
            } else if ts.eat_phrase(&["all", "land", "types", "and", "abilities"])
                || ts.eat_phrase(&["all", "creature", "types", "and", "abilities"])
                || ts.eat_phrase(&["all", "creature", "types"])
                || ts.eat_phrase(&["all", "land", "types"])
            {
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
                    if (ts.eat_phrase(&["and", "have", "base", "power", "and", "toughness"])
                        || ts.eat_phrase(&["and", "has", "base", "power", "and", "toughness"]))
                        && let Some((power, toughness)) = ts.next_base_pt()
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
                    if ts.eat_phrase(&["and", "is", "a"])
                        || ts.eat_phrase(&["and", "is", "an"])
                        || ts.eat_phrase(&["and", "are"])
                        || ts.eat_phrase(&["and", "becomes", "a"])
                        || ts.eat_phrase(&["and", "becomes", "an"])
                    {
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
                            let keep = ts
                                .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                                || ts.eat_phrase(&[
                                    "in", "addition", "to", "their", "other", "types",
                                ]);
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
                        let _ = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                        if parts.len() > 1 {
                            continue;
                        }
                    }
                    ts.restore(lcp2);
                    let lcp3 = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
                        || ts.eat_phrase(&["and", "don't", "untap", "during"]))
                        && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
                            || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
                            || ts.eat_phrase(&["your", "untap", "step"]))
                    {
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
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        ts.restore(lose_cp);
    }

    // `<subj> has/have all activated abilities of <flt>`
    // OR `<subj> has/have base power and toughness <P>/<T> [and are <Subtypes> in addition to their other types]`
    // OR `<subj> has/have "<quoted ability>"` OR `<subj> has/have <kws>`
    if ts.eat_word("has") || ts.eat_word("have") {
        if ts.eat_phrase(&["all", "activated", "abilities", "of"])
            || ts.eat_phrase(&["all", "activated", "and", "triggered", "abilities", "of"])
        {
            let source = if ts.eat_phrase(&["that", "card"])
                || ts.eat_phrase(&["that", "creature"])
                || ts.eat_phrase(&["the", "exiled", "card"])
                || ts.eat_phrase(&["the", "last", "chosen", "card"])
            {
                crate::v2::ir::ObjectFilter {
                    any_card: true,
                    ..Default::default()
                }
            } else {
                let _ = ts.eat_word("all") || ts.eat_word("each");
                parse_object_filter(ts)?
            };
            let _ = ts.eat_phrase(&["except", "mana", "abilities"]);
            let mut parts = vec![StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::CopyActivatedAbilities(source),
                condition: condition.clone(),
            }];
            parse_static_subject_tail(ts, &subject, &condition, &mut parts);
            return Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                StaticAbility::Compound(parts)
            });
        }
        if ts.eat_phrase(&["the", "chosen", "creature", "type", "in", "addition", "to"])
            && (ts.eat_phrase(&["its", "other", "types"])
                || ts.eat_phrase(&["their", "other", "types"]))
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
                let mut parts = vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness: 0,
                }];
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
            ts.restore(bp_cp);
        }
        if ts.eat_phrase(&["base", "toughness"])
            && let Some(toughness) = ts.next_number(false)
        {
            let mut parts = vec![StaticAbility::SetBasePT {
                subject: subject.clone(),
                power: 0,
                toughness,
            }];
            parse_static_subject_tail(ts, &subject, &condition, &mut parts);
            return Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                StaticAbility::Compound(parts)
            });
        }
        if ts.eat_phrase(&["base", "power", "and", "toughness"]) {
            if ts.eat_phrase(&["each", "equal", "to"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
            {
                let mut parts = vec![StaticAbility::DynamicBasePT {
                    power: Some(val.clone()),
                    toughness: Some(val),
                }];
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
            if let Some((power, toughness)) = ts.next_base_pt() {
                let mut parts = vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness,
                }];
                if ts.eat_phrase(&["and", "are"]) || ts.eat_phrase(&["and", "is"]) {
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
                        let keep_existing = ts
                            .eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                            || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                            || ts.eat_phrase(&[
                                "in", "addition", "to", "their", "other", "creature", "types",
                            ])
                            || ts.eat_phrase(&[
                                "in", "addition", "to", "its", "other", "creature", "types",
                            ]);
                        parts.push(StaticAbility::SetSubtypes {
                            subject: subject.clone(),
                            subtypes,
                            keep_existing,
                        });
                    }
                }
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
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
                let mut parts = vec![StaticAbility::DynamicBasePT {
                    power: Some(ValueExpr::Var(crate::v2::ir::VarName::X)),
                    toughness: Some(ValueExpr::Var(crate::v2::ir::VarName::X)),
                }];
                parse_static_subject_tail(ts, &subject, &condition, &mut parts);
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        if let Some(quoted) = ts.next_quoted() {
            let compiled = crate::v2::card::compile_face_v2(
                crate::FaceInput {
                    name: "~",
                    type_line: "Permanent",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !compiled.is_ok() {
                return Err(compiled
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile static granted ability")));
            }
            let mut parts = vec![StaticAbility::GrantAbility {
                subject: subject.clone(),
                ability: Box::new(compiled),
                condition: condition.clone(),
            }];
            {
                let q2_cp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Some(q2) = ts.next_quoted()
                {
                    let c2 = crate::v2::card::compile_face_v2(
                        crate::FaceInput {
                            name: "~",
                            type_line: "Permanent",
                            oracle_text: q2,
                        },
                        ts.vocab,
                    );
                    if c2.is_ok() {
                        parts.push(StaticAbility::GrantAbility {
                            subject: subject.clone(),
                            ability: Box::new(c2),
                            condition: condition.clone(),
                        });
                    } else {
                        ts.restore(q2_cp);
                    }
                } else {
                    ts.restore(q2_cp);
                }
            }
            parse_static_subject_tail(ts, &subject, &condition, &mut parts);
            return Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                StaticAbility::Compound(parts)
            });
        }
        // Special `has` static predicates (`Spy Kit`, `Clandestine Chameleon`, `Pin Collection`, `Duplicant`, `Volrath's Shapeshifter`)
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
        ]) || ts.eat_phrase(&["the", "power"])
            && ts.eat_comma()
            && ts.eat_word("toughness")
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "and", "creature", "types", "of", "the", "last", "creature", "card", "exiled",
                "with", "it",
            ])
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
            parse_keyword_list(ts)?.0
        };
        let first_cond = if ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if") {
            let c = parse_static_condition(ts)?;
            Some(match condition.clone() {
                Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                None => c,
            })
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
                let _ = ts.eat_word("has") || ts.eat_word("have");
                if let Ok((more_kws, _)) = try_parse(ts, parse_keyword_list)
                    && (ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if"))
                    && let Ok(c) = try_parse(ts, parse_static_condition)
                {
                    let merged = match condition.clone() {
                        Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                        None => c,
                    };
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: more_kws,
                        condition: Some(merged),
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
                {
                    let compiled = crate::v2::card::compile_face_v2(
                        crate::FaceInput {
                            name: "~",
                            type_line: "Permanent",
                            oracle_text: quoted.trim_end_matches(','),
                        },
                        ts.vocab,
                    );
                    if compiled.is_ok() {
                        let _ = (ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if"))
                            && opt(ts, parse_static_condition).is_some();
                        parts.push(StaticAbility::GrantAbility {
                            subject: subject.clone(),
                            ability: Box::new(compiled),
                            condition: condition.clone(),
                        });
                        continue;
                    }
                }
                ts.restore(qcp);
            }
            ts.restore(hcp);
            let lcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "loses", "all", "other", "abilities"])
                || ts.eat_phrase(&["and", "lose", "all", "other", "abilities"])
                || ts.eat_phrase(&["and", "it", "loses", "all", "other", "abilities"])
                || ts.eat_phrase(&["and", "loses", "all", "abilities"])
                || ts.eat_phrase(&["and", "lose", "all", "abilities"])
            {
                parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(lcp);
            let lkw_cp = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "loses"]) || ts.eat_phrase(&["and", "lose"]))
                && let Ok(_kws) = try_parse(ts, parse_keyword_list)
            {
                parts.push(StaticAbility::RemoveAllAbilities {
                    subject: subject.clone(),
                });
                continue;
            }
            ts.restore(lkw_cp);
            if ts.eat_phrase(&[
                "and", "can", "attack", "as", "though", "it", "didn't", "have", "defender",
            ]) || ts.eat_phrase(&[
                "and", "can", "attack", "as", "though", "they", "didn't", "have", "defender",
            ]) {
                parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::CanAttackWithDefender,
                    condition: condition.clone(),
                });
                continue;
            }
            if ts.eat_phrase(&["and", "attack", "each", "combat", "if", "able"])
                || ts.eat_phrase(&["and", "attacks", "each", "combat", "if", "able"])
            {
                parts.push(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::MustAttack,
                    condition: condition.clone(),
                });
                continue;
            }
            break;
        }
        parse_static_subject_tail(ts, &subject, &condition, &mut parts);
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> doesn't untap during its controller's untap step [and its activated abilities can't be activated]`
    if (ts.eat_phrase(&["doesn't", "untap", "during"])
        || ts.eat_phrase(&["don't", "untap", "during"]))
        && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
            || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
            || ts.eat_phrase(&["your", "untap", "step"]))
    {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::DoesntUntap,
            condition: condition.clone(),
        }];
        let acp = ts.checkpoint();
        let _ = ts.eat_comma();
        if ts.eat_phrase(&[
            "and",
            "its",
            "activated",
            "abilities",
            "can't",
            "be",
            "activated",
        ]) || ts.eat_phrase(&[
            "and",
            "their",
            "activated",
            "abilities",
            "can't",
            "be",
            "activated",
        ]) {
            let _ = ts.eat_phrase(&["unless", "they're", "mana", "abilities"])
                || ts.eat_phrase(&["unless", "they", "are", "mana", "abilities"]);
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                condition,
            });
        } else {
            ts.restore(acp);
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> attacks or blocks each combat if able` / `<subj> blocks each combat if able` / `<subj> attacks each combat if able [unless <cond>]`
    if ts.eat_phrase(&["attacks", "or", "blocks", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attack", "or", "block", "each", "combat", "if", "able"])
    {
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
    if ts.eat_phrase(&["blocks", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["block", "each", "combat", "if", "able"])
    {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::AllMustBlock,
            condition,
        });
    }
    if ts.eat_phrase(&["attacks", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attacks", "each", "turn", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "turn", "if", "able"])
    {
        let final_cond = if ts.eat_word("unless") {
            let u = ConditionExpr::Not(Box::new(parse_static_condition(ts)?));
            Some(match condition {
                Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(u)),
                None => u,
            })
        } else {
            condition
        };
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::MustAttack,
            condition: final_cond.clone(),
        }];
        parse_static_subject_tail(ts, &subject, &final_cond, &mut parts);
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> can be blocked as though they/it didn't have <kw>`
    if (ts.eat_phrase(&[
        "can", "be", "blocked", "as", "though", "they", "didn't", "have",
    ]) || ts.eat_phrase(&[
        "can", "be", "blocked", "as", "though", "it", "didn't", "have",
    ])) && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
    {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::IgnoreKeywordForBlocking(kw),
            condition: None,
        });
    }

    // `<subj> can block creatures with <kw> as though they didn't have <kw> / as though it had <kw>` (`Aetherflame Wall`, `Heartwood Dryad`)
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["can", "block", "creatures", "with"])
            && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
            && (ts.eat_phrase(&["as", "though", "they", "didn't", "have"])
                || ts.eat_phrase(&["as", "though", "it", "had"]))
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

    // `<subj> can block an additional [<N>] creature(s) each combat` (`Watcher in the Web`, `Hundred-Handed One`)
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["can", "block", "an", "additional"]) {
            let extra = ts
                .next_number(false)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(1);
            if (ts.eat_word("creature") || ts.eat_word("creatures"))
                && ts.eat_phrase(&["each", "combat"])
            {
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

    // `<subj> can be attached only to <flt>` (`Gate Smasher`, `O-Naginata`, `Konda's Banner`)
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["can", "be", "attached", "only", "to"]) {
            let _ = ts.eat_word("a") || ts.eat_word("an");
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
    if ts.eat_phrase(&["enters", "untapped"]) || ts.eat_phrase(&["enter", "untapped"]) {
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
    if ts.eat_phrase(&["enters", "tapped"]) || ts.eat_phrase(&["enter", "tapped"]) {
        let mut parts = vec![StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::EntersTapped,
            condition: condition.clone(),
        }];
        let ucp = ts.checkpoint();
        if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
            || ts.eat_phrase(&["and", "don't", "untap", "during"]))
            && (ts.eat_phrase(&["your", "untap", "step"])
                || ts.eat_phrase(&["its", "controller's", "untap", "step"])
                || ts.eat_phrase(&["their", "controllers'", "untap", "steps"]))
        {
            parts.push(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::DoesntUntap,
                condition,
            });
        } else {
            ts.restore(ucp);
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let mut additional = ts.eat_phrase(&["an", "additional"]) || ts.eat_word("additional");
        let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else if ts.eat_word("a") || ts.eat_word("an") {
            Some(ValueExpr::Const(1))
        } else {
            Some(try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1)))
        };
        if ts.eat_word("additional") {
            additional = true;
        }
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
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
            // Optional `and as a <Subtype> in addition to its other types` (`Master Biomancer`)
            let mut extra_sub: Option<StaticAbility> = None;
            {
                let m_cp = ts.checkpoint();
                if (ts.eat_phrase(&["and", "as", "a"]) || ts.eat_phrase(&["and", "as", "an"]))
                    && let Some(sub) = ts
                        .peek_word()
                        .and_then(|w| ts.vocab.lookup_subtype(&w.to_ascii_lowercase()))
                {
                    let sub_owned = sub.to_string();
                    ts.next_word();
                    let keep = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
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
                let c = parse_static_condition(ts)?;
                Some(match condition {
                    Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                    None => c,
                })
            } else if ts.eat_word("unless") {
                let c = ConditionExpr::Not(Box::new(parse_static_condition(ts)?));
                Some(match condition {
                    Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                    None => c,
                })
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
        if (ts.eat_phrase(&["enter", "as", "a", "copy", "of"])
            || ts.eat_phrase(&["enters", "as", "a", "copy", "of"]))
            && try_parse(ts, parse_subject_ref).is_ok()
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

    // `<subj> must be blocked [by exactly one creature | by two or more creatures | by all creatures able to block it] if able`
    if ts.eat_phrase(&["must", "be", "blocked", "if", "able"])
        || ts.eat_phrase(&[
            "must", "be", "blocked", "by", "exactly", "one", "creature", "if", "able",
        ])
        || ts.eat_phrase(&[
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
        ])
        || ts.eat_phrase(&[
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
        ])
    {
        let mut parts = vec![StaticAbility::RuleRestriction {
            subject: Some(subject.clone()),
            rule: RuleRestrictionKind::MustBeBlocked,
            condition: condition.clone(),
        }];
        parse_static_subject_tail(ts, &subject, &condition, &mut parts);
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> can't attack [or block] unless its/their controller pays <cost> | you <cost> [for each <expr>]`
    // (`Brainwash`, `Oppressive Rays`, `Qal Sisma Behemoth`, `Phyrexian Marauder`, `Exalted Dragon`, `Floodtide Serpent`)
    {
        let ucp = ts.checkpoint();
        if (ts.eat_phrase(&["can't", "attack", "or", "block", "unless"])
            || ts.eat_phrase(&["can't", "attack", "unless"])
            || ts.eat_phrase(&["can't", "block", "unless"]))
            && (ts.eat_phrase(&["its", "controller", "pays"])
                || ts.eat_phrase(&["their", "controller", "pays"])
                || ts.eat_phrase(&["you", "pay"])
                || ts.eat_word("you"))
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

    // `<subj> can't attack/block <tgt> unless their/its controller pays <cost> [for each <expr>]` (Ghostly Prison / Archon of Absolution / Heat Wave / Onakke Oathkeeper)
    {
        let acp = ts.checkpoint();
        if (ts.eat_phrase(&["can't", "attack"]) || ts.eat_phrase(&["can't", "block"]))
            && let Ok(target) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("unless")
            && (ts.eat_phrase(&["their", "controller", "pays"])
                || ts.eat_phrase(&["its", "controller", "pays"]))
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
                || ts.eat_phrase(&["for", "each", "of", "those", "creatures"])
                || ts.eat_phrase(&[
                    "for",
                    "each",
                    "creature",
                    "they",
                    "control",
                    "that's",
                    "attacking",
                    "you",
                ])
                || ts.eat_phrase(&["for", "each", "blocking", "creature", "they", "control"])
                || opt(ts, parse_static_scale_expr).is_some();
            return Ok(StaticAbility::AttackTax {
                target,
                cost,
                per_creature,
            });
        }
        ts.restore(acp);
    }

    // `<subj> crews Vehicles as though its power were <N> greater` (`Prodigy's Prototype` Pilot token + 8+ cards)
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

    // `<subj> can't ... [as long as / if <cond>]` (supports compound `and can't ...` and `, and its activated abilities can't be activated`)
    if ts.eat_word("can't") {
        let first_rule = parse_cant_rule(ts)?;
        let trailing_cond = if ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if") {
            let c = parse_static_condition(ts)?;
            Some(match condition.clone() {
                Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                None => c,
            })
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
            if (ts.eat_phrase(&["and", "its", "activated", "abilities"])
                || ts.eat_phrase(&["and", "their", "activated", "abilities"]))
                && {
                    let _ = ts.eat_phrase(&["with", "{t}", "in", "their", "costs"])
                        || (ts.eat_word("with")
                            && matches!(
                                ts.peek().map(|t| &t.kind),
                                Some(crate::v2::lexer::TokenKind::Mana(
                                    crate::v2::ir::ManaSymbol::Tap
                                ))
                            )
                            && {
                                ts.next_tok();
                                ts.eat_phrase(&["in", "their", "costs"])
                            });
                    ts.eat_phrase(&["can't", "be", "activated"])
                }
            {
                let _ = ts.eat_phrase(&["unless", "they're", "mana", "abilities"])
                    || ts.eat_phrase(&["unless", "they", "are", "mana", "abilities"]);
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
            if (ts.eat_phrase(&["and", "has"]) || ts.eat_phrase(&["and", "have"]))
                && let Some(quoted) = ts.next_quoted()
            {
                let compiled = crate::v2::card::compile_face_v2(
                    crate::FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if compiled.is_ok() {
                    parts.push(StaticAbility::GrantAbility {
                        subject: subject.clone(),
                        ability: Box::new(compiled),
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(hcp);
            let kcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if (ts.eat_phrase(&["and", "has"]) || ts.eat_phrase(&["and", "have"]))
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
        parse_static_subject_tail(ts, &subject, &condition, &mut parts);
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    Err(ts.err("expected static ability predicate"))
}
