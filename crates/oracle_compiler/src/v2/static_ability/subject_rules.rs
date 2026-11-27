//! Subject-headed static abilities (`<subj> gets +X/+Y`, `<subj> has <kws>`, `<subj> can't ...`, etc.) for `v2`.

use crate::v2::condition::parse_condition;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_subject_ref,
};
use crate::v2::ir::{
    ConditionExpr, RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectStaticRule, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

pub(super) fn parse_subject_static_ability(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 3. Subject-headed static abilities (`<subj> gets/get +X/+Y [and has/have <kws>]`,
    //    `<subj> can't be the targets of <flt>`, `<subj> can't attack`, etc.)
    let subject = parse_subject_ref(ts)?;

    // `<subj> gets/get [an additional] +X/+Y [for each <expr>] [, has/have <kws>] [and has/have <kws>] [and can't <rule>] [for each <expr>]`
    if ts.eat_word("gets") || ts.eat_word("get") {
        let _ = ts.eat_phrase(&["an", "additional"]);
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let mut scale = opt(ts, parse_for_each_expr).map(Box::new);
        let mut grant_keywords = Vec::new();
        let mut extra_parts = Vec::new();
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
            scale = opt(ts, parse_for_each_expr).map(Box::new);
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
            scale = opt(ts, parse_for_each_expr).map(Box::new);
        }
        let base = StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale,
            condition,
        };
        if extra_parts.is_empty() {
            return Ok(base);
        }
        let mut all = vec![base];
        all.extend(extra_parts);
        return Ok(StaticAbility::Compound(all));
    }

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
                let _ = ts.eat_word("or") || ts.eat_word("and");
            } else {
                break;
            }
        }
        if !types.is_empty() {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::RemoveTypes(types),
                condition,
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
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::AssignDamageByToughness,
            condition,
        });
    }

    // `<subj> can attack as though it/they didn't have defender`
    if ts.eat_phrase(&[
        "can", "attack", "as", "though", "it", "didn't", "have", "defender",
    ]) || ts.eat_phrase(&[
        "can", "attack", "as", "though", "they", "didn't", "have", "defender",
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::CanAttackWithDefender,
            condition,
        });
    }

    // `<subj> is/are the chosen type in addition to its/their other types` (`Metallic Mimic`)
    {
        let mcp = ts.checkpoint();
        if (ts.eat_word("is") || ts.eat_word("are"))
            && ts.eat_phrase(&["the", "chosen", "type", "in", "addition", "to"])
            && (ts.eat_phrase(&["its", "other", "types"])
                || ts.eat_phrase(&["their", "other", "types"]))
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::AdditionalTypesChosen,
                condition: None,
            });
        }
        ts.restore(mcp);
        if (ts.eat_word("is") || ts.eat_word("are")) && ts.eat_phrase(&["the", "chosen", "color"]) {
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

    // `<subj> is a [color] [subtypes] creature [with base power and toughness P/T] [in addition to its other types]` (`Ensoul Artifact`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["is", "a"]) || ts.eat_phrase(&["is", "an"]) || ts.eat_word("are") {
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if crate::vocab::lookup_color(&low).is_some() || low == "and" {
                    ts.next_word();
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
            if saw_type
                && ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                && let Some((power, toughness)) = ts.next_base_pt()
            {
                let _ = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                let mut parts = vec![StaticAbility::SetBasePT {
                    subject: subject.clone(),
                    power,
                    toughness,
                }];
                if !subtypes.is_empty() {
                    parts.push(StaticAbility::SetSubtypes {
                        subject,
                        subtypes,
                        keep_existing: true,
                    });
                }
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        ts.restore(ecp);
    }

    // `<subj> are <Subtypes>` (e.g. `Nonbasic lands are Mountains.`)
    if ts.eat_word("are") || ts.eat_word("is") {
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
            let keep_existing = ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
            return Ok(StaticAbility::SetSubtypes {
                subject,
                subtypes,
                keep_existing,
            });
        }
    }

    // `<subj> lose all abilities [and have base power and toughness <P>/<T> | and is a <color> <subtype> creature with base power and toughness <P>/<T> | and doesn't untap ...]`
    if ts.eat_phrase(&["lose", "all", "abilities"]) || ts.eat_phrase(&["loses", "all", "abilities"])
    {
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
            {
                let mut subtypes = Vec::new();
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if crate::vocab::lookup_color(&low).is_some()
                        || crate::vocab::lookup_card_type(&low).is_some()
                        || low == "and"
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
                    parts.push(StaticAbility::SetSubtypes {
                        subject: subject.clone(),
                        subtypes,
                        keep_existing: false,
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
                }
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
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> has/have all activated abilities of <flt>`
    // OR `<subj> has/have base power and toughness <P>/<T> [and are <Subtypes> in addition to their other types]`
    // OR `<subj> has/have "<quoted ability>"` OR `<subj> has/have <kws>`
    if ts.eat_word("has") || ts.eat_word("have") {
        if ts.eat_phrase(&["all", "activated", "abilities", "of"]) {
            let source = parse_object_filter(ts)?;
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CopyActivatedAbilities(source),
                condition: None,
            });
        }
        if ts.eat_phrase(&["base", "power", "and", "toughness"])
            && let Some((power, toughness)) = ts.next_base_pt()
        {
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
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
                    parts.push(StaticAbility::SetSubtypes {
                        subject,
                        subtypes,
                        keep_existing,
                    });
                }
            }
            return Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                StaticAbility::Compound(parts)
            });
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
            let first = StaticAbility::GrantAbility {
                subject: subject.clone(),
                ability: Box::new(compiled),
                condition: condition.clone(),
            };
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
                    return Ok(StaticAbility::Compound(vec![
                        first,
                        StaticAbility::GrantAbility {
                            subject,
                            ability: Box::new(c2),
                            condition,
                        },
                    ]));
                }
            }
            return Ok(first);
        }
        let (keywords, _) = parse_keyword_list(ts)?;
        let first_cond = if ts.eat_phrase(&["as", "long", "as"]) {
            let c = parse_condition(ts)?;
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
                    && ts.eat_phrase(&["as", "long", "as"])
                    && let Ok(c) = try_parse(ts, parse_condition)
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
                if !compiled.is_ok() {
                    return Err(compiled
                        .errors
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| ts.err("failed to compile static granted ability")));
                }
                parts.push(StaticAbility::GrantAbility {
                    subject: subject.clone(),
                    ability: Box::new(compiled),
                    condition: condition.clone(),
                });
                continue;
            }
            ts.restore(hcp);
            break;
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    // `<subj> doesn't untap during its controller's untap step`
    if (ts.eat_phrase(&["doesn't", "untap", "during"])
        || ts.eat_phrase(&["don't", "untap", "during"]))
        && (ts.eat_phrase(&["its", "controller's", "untap", "step"])
            || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
            || ts.eat_phrase(&["your", "untap", "step"]))
    {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::DoesntUntap,
            condition,
        });
    }

    // `<subj> attacks each combat if able [unless <cond>]`
    if ts.eat_phrase(&["attacks", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "combat", "if", "able"])
        || ts.eat_phrase(&["attacks", "each", "turn", "if", "able"])
        || ts.eat_phrase(&["attack", "each", "turn", "if", "able"])
    {
        let final_cond = if ts.eat_word("unless") {
            let u = ConditionExpr::Not(Box::new(parse_condition(ts)?));
            Some(match condition {
                Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(u)),
                None => u,
            })
        } else {
            condition
        };
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::MustAttack,
            condition: final_cond,
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

    // `<subj> can block an additional creature each combat`
    if ts.eat_phrase(&[
        "can",
        "block",
        "an",
        "additional",
        "creature",
        "each",
        "combat",
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::CanBlockAdditional(1),
            condition,
        });
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
        return Ok(StaticAbility::SubjectRule {
            subject,
            rule: SubjectStaticRule::EntersTapped,
            condition,
        });
    }
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else if ts.eat_word("a") || ts.eat_word("an") {
            Some(ValueExpr::Const(1))
        } else {
            Some(try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1)))
        };
        let additional = ts.eat_word("additional");
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counter") || ts.eat_word("counters"))
            && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
        {
            let count = if let Some(c) = count_opt {
                if let Some(scale) = opt(ts, parse_for_each_expr) {
                    ValueExpr::Mul(Box::new(c), Box::new(scale))
                } else {
                    c
                }
            } else if ts.eat_phrase(&["equal", "to"]) {
                parse_value_expr(ts)?
            } else {
                return Err(ts.err("expected `equal to <val>` after `a number of counters`"));
            };
            let final_cond = if ts.eat_word("if") {
                let c = parse_condition(ts)?;
                Some(match condition {
                    Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                    None => c,
                })
            } else {
                condition
            };
            return Ok(StaticAbility::EntersWithCountersStatic {
                subject,
                kind,
                count,
                additional,
                condition: final_cond,
            });
        }
    }

    // `<subj> must be blocked [if able | by all creatures able to block it]`
    if ts.eat_phrase(&["must", "be", "blocked", "if", "able"])
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
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(subject),
            rule: RuleRestrictionKind::MustBeBlocked,
            condition,
        });
    }

    // `<subj> can't attack you [or planeswalkers you control] unless their controller pays {M} [for each of those creatures]` (Ghostly Prison / Archon of Absolution)
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["can't", "attack"])
            && let Ok(target) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("unless")
            && (ts.eat_phrase(&["their", "controller", "pays"])
                || ts.eat_phrase(&["its", "controller", "pays"]))
            && let Some(mc) = ts.eat_mana_cost()
        {
            let per_creature = ts.eat_phrase(&["for", "each", "of", "those", "creatures"])
                || ts.eat_phrase(&[
                    "for",
                    "each",
                    "creature",
                    "they",
                    "control",
                    "that's",
                    "attacking",
                    "you",
                ]);
            return Ok(StaticAbility::AttackTax {
                target,
                cost: crate::v2::ir::Cost::Mana(mc),
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

    // `<subj> can't ... [as long as <cond>]` (supports compound `and can't ...` and `, and its activated abilities can't be activated`)
    if ts.eat_word("can't") {
        let first_rule = parse_cant_rule(ts)?;
        let trailing_cond = if ts.eat_phrase(&["as", "long", "as"]) {
            let c = parse_condition(ts)?;
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
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }

    Err(ts.err("expected static ability predicate"))
}

pub(super) fn parse_cant_rule(ts: &mut TokenStream<'_>) -> PResult<RuleRestrictionKind> {
    if ts.eat_phrase(&["become", "untapped"]) {
        return Ok(RuleRestrictionKind::CantBecomeUntapped);
    }
    if ts.eat_phrase(&["be", "enchanted", "by"]) {
        let flt = parse_object_filter(ts)?;
        return Ok(RuleRestrictionKind::CantBeEnchantedBy(Box::new(flt)));
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
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackOrBlockUnless(Box::new(cond)));
        }
        return Ok(RuleRestrictionKind::CantAttackOrBlock);
    }
    if ts.eat_word("attack") {
        if ts.eat_comma()
            && ts.eat_word("block")
            && ts.eat_comma()
            && ts.eat_phrase(&["or", "crew", "vehicles"])
        {
            return Ok(RuleRestrictionKind::CantAttackOrBlock);
        }
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
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantAttackUnless(Box::new(cond)));
        }
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("you"))
            && let Ok(tgt) = try_parse(ts, parse_subject_ref)
        {
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
            let cond = parse_condition(ts)?;
            return Ok(RuleRestrictionKind::CantBlockUnless(Box::new(cond)));
        }
        if let Ok(blocked_subj) = try_parse(ts, parse_subject_ref) {
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
