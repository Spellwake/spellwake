//! Extended subject-headed static ability predicates, tail combinators, and `can't` restrictions.

use crate::ast::{CardTypeMask, ColorMask};
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_keyword_list, parse_object_filter,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    ConditionExpr, ConditionFlag, KeywordKind, KeywordSpec, ObjectFilter, ObjectStateKind,
    PlayerRef, RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectRef, SubjectStaticRule,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

use super::cond_ext::{parse_static_condition, parse_static_scale_expr};
use super::{and_cond, wrap_compound};

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

/// Parse extended subject-headed static predicates (protection/hexproof variants, landwalk variants,
/// loyalty/activated ability copying, combat-role overrides, copy-of, toughness-crewing, etc.).
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

    // 2. `<subj> can be the targets of spells and abilities [you control] as though they didn't have hexproof`
    if ts.eat_phrase(&[
        "can",
        "be",
        "the",
        "targets",
        "of",
        "spells",
        "and",
        "abilities",
    ]) {
        let _ = ts.eat_phrase(&["you", "control"]);
        if ts.eat_phrase(&["as", "though", "they", "didn't", "have", "hexproof"]) {
            return Ok(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::IgnoreKeywordForBlocking(KeywordKind::Hexproof),
                condition: condition.clone(),
            });
        }
    }
    ts.restore(cp);

    // 3. `<subj> can ...` extended rules
    if ts.eat_word("can") {
        if ts.eat_phrase(&["block", "as", "though", "they", "were", "untapped"]) {
            return Ok(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::CanBlockAdditional(1),
                condition: condition.clone(),
            });
        }
        if ts.eat_word("block")
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_any_phrase(&[
                &["as", "though", "it", "had"],
                &["as", "though", "they", "had"],
            ])
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
        {
            return Ok(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::IgnoreKeywordForBlocking(kw),
                condition: condition.clone(),
            });
        }
        ts.restore(cp);
        let _ = ts.eat_word("can");
        if ts.eat_word("attack") {
            if ts.eat_phrase(&[
                "players", "who", "attacked", "you", "during", "their", "last", "turn", "as",
                "though", "it", "didn't", "have", "defender",
            ]) {
                return Ok(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::CanAttackWithDefender,
                    condition: condition.clone(),
                });
            }
            if try_parse(ts, parse_subject_ref).is_ok() {
                if ts.eat_word("and") {
                    let _ = opt(ts, parse_subject_ref);
                }
                if ts.eat_phrase(&["as", "though", "those", "creatures", "had", "haste"]) {
                    return Ok(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: vec![KeywordSpec {
                            kind: KeywordKind::Haste,
                            param: None,
                        }],
                        condition: condition.clone(),
                    });
                }
            }
        }
        ts.restore(cp);
        let _ = ts.eat_word("can");
        if ts.eat_phrase(&[
            "be",
            "blocked",
            "as",
            "though",
            "they",
            "didn't",
            "have",
            "those",
            "abilities",
        ]) {
            return Ok(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::IgnoreKeywordForBlocking(KeywordKind::Landwalk),
                condition: condition.clone(),
            });
        }
        if ts.eat_phrase(&[
            "boast", "twice", "during", "each", "of", "your", "turns", "rather", "than", "once",
        ]) {
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
        }
    }
    ts.restore(cp);

    // 4. `<subj> is a copy of <subj2>`
    if ts.eat_phrase(&["is", "a", "copy", "of"])
        && (ts.eat_any_phrase(&[
            &["the", "last", "chosen", "card"],
            &["the", "chosen", "creature"],
        ]) || try_parse(ts, parse_subject_ref).is_ok())
    {
        return Ok(StaticAbility::ConditionalReplacement {
            condition: condition
                .clone()
                .unwrap_or(ConditionExpr::Flag(ConditionFlag::ControlsSelf)),
            replacement: Box::new(crate::v2::ir::Replacement::EnterAsCopy {
                optional: false,
                filter: ObjectFilter::default(),
                exceptions: Vec::new(),
            }),
        });
    }
    ts.restore(cp);

    // 5. `<subj> isn't legendary [if <cond>]` / `<subj> are no longer snow`
    if ts.eat_any_phrase(&[&["isn't", "legendary"], &["aren't", "legendary"]]) {
        let final_cond = if ts.eat_word("if") {
            Some(and_cond(condition.clone(), parse_static_condition(ts)?))
        } else {
            condition.clone()
        };
        return Ok(StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::RemoveTypes(CardTypeMask::EMPTY),
            condition: final_cond,
        });
    }
    if ts.eat_any_phrase(&[
        &["are", "no", "longer", "snow"],
        &["is", "no", "longer", "snow"],
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::RemoveTypes(CardTypeMask::EMPTY),
            condition: condition.clone(),
        });
    }
    ts.restore(cp);

    // 6. `<subj> [saddles Mounts and] crews Vehicles / stations permanents using its toughness rather than its power`
    if ts.eat_any_phrase(&[
        &[
            "saddles",
            "mounts",
            "and",
            "crews",
            "vehicles",
            "using",
            "its",
            "toughness",
            "rather",
            "than",
            "its",
            "power",
        ],
        &[
            "crews",
            "vehicles",
            "using",
            "its",
            "toughness",
            "rather",
            "than",
            "its",
            "power",
        ],
        &[
            "stations",
            "permanents",
            "using",
            "its",
            "toughness",
            "rather",
            "than",
            "its",
            "power",
        ],
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::AssignDamageByToughness,
            condition: condition.clone(),
        });
    }
    ts.restore(cp);

    // 7. `<subj> attack(s) <target_desc> each combat if able [unless <cond>]`
    if ts.eat_any_word(&["attack", "attacks"])
        && (ts.eat_phrase(&[
            "an",
            "opponent",
            "with",
            "the",
            "most",
            "life",
            "among",
            "your",
            "opponents",
        ]) || (ts.eat_phrase(&["a", "player", "you", "noted", "for", "cards", "named"])
            && ts.eat_tilde())
            || try_parse(ts, parse_subject_ref).is_ok())
        && ts.eat_phrase(&["each", "combat", "if", "able"])
    {
        let final_cond = if ts.eat_word("unless") {
            let u = ConditionExpr::Not(Box::new(parse_static_condition(ts)?));
            Some(and_cond(condition.clone(), u))
        } else {
            condition.clone()
        };
        return Ok(StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::MustAttack,
            condition: final_cond,
        });
    }
    ts.restore(cp);
    // `<subj> assigns combat damage as though ~'s/its power were positive`
    if ts.eat_phrase(&["assigns", "combat", "damage", "as", "though"])
        && ((ts.eat_tilde() && ts.eat_word("'s")) || ts.eat_word("its"))
        && ts.eat_phrase(&["power", "were", "positive"])
    {
        return Ok(StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::AssignDamageByToughness,
            condition: condition.clone(),
        });
    }
    ts.restore(cp);
    if ts.eat_phrase(&["must", "attack"]) {
        if !ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let _ = opt(ts, parse_subject_ref);
            if ts.eat_word("or") {
                let _ = opt(ts, parse_subject_ref);
            }
        }
        if ts.eat_phrase(&[
            "with", "at", "least", "one", "creature", "each", "combat", "if", "able",
        ]) {
            return Ok(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::MustAttack,
                condition: condition.clone(),
            });
        }
    }
    ts.restore(cp);

    // 8. `<subj> has/have ...` extended variants
    if ts.eat_any_word(&["has", "have"]) {
        // `the/all loyalty abilities of <subj|flt>` / `each/all activated abilities of <flt>`
        {
            let acp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["the", "loyalty", "abilities", "of"],
                &["all", "loyalty", "abilities", "of"],
                &["each", "activated", "ability", "of"],
                &["all", "activated", "abilities", "of"],
            ]) {
                let src = if ts.eat_tilde() {
                    Some(ObjectFilter::default())
                } else if ts.eat_phrase(&["the", "chosen", "permanent"]) {
                    Some(ObjectFilter {
                        any_permanent: true,
                        ..Default::default()
                    })
                } else if ts.eat_phrase(&["the", "exiled", "cards", "used", "to", "craft", "it"]) {
                    Some(ObjectFilter {
                        any_card: true,
                        ..Default::default()
                    })
                } else {
                    let _ = ts.eat_any_word(&["all", "each"]);
                    try_parse(ts, parse_object_filter).ok()
                };
                if let Some(source) = src {
                    let _ = ts.eat_any_phrase(&[
                        &["except", "for", "loyalty", "abilities"],
                        &["except", "mana", "abilities"],
                    ]);
                    return finish_subject_static(
                        ts,
                        subject,
                        condition,
                        vec![StaticAbility::SubjectRule {
                            subject: subject.clone(),
                            rule: SubjectStaticRule::CopyActivatedAbilities(source),
                            condition: condition.clone(),
                        }],
                    );
                }
            }
            ts.restore(acp);
        }
        if ts.eat_phrase(&["each", "of", "the", "chosen", "abilities"]) {
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
        }
        if ts.eat_phrase(&["sticker", "kicker"])
            && let Some(mc) = ts.eat_mana_cost()
        {
            return Ok(StaticAbility::GrantKeywords {
                subject: subject.clone(),
                keywords: vec![KeywordSpec {
                    kind: KeywordKind::Kicker,
                    param: Some(crate::v2::ir::KeywordParam::Cost(vec![
                        crate::v2::ir::Cost::Mana(mc),
                    ])),
                }],
                condition: condition.clone(),
            });
        }
        if let Some(kw) =
            try_parse_extended_prot_or_hexproof(ts).or_else(|| try_parse_extended_landwalk(ts))
        {
            return finish_subject_static(
                ts,
                subject,
                condition,
                vec![StaticAbility::GrantKeywords {
                    subject: subject.clone(),
                    keywords: vec![kw],
                    condition: condition.clone(),
                }],
            );
        }
        // `base power and toughness P/T, <kws> [, and "<quoted>"]`
        {
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
                        if let Some(st) =
                            try_compile_quoted_grant(ts, quoted, subject, condition, true)
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
    }
    ts.restore(cp);

    // 9. `<subj> gain(s) <kw_spec> as you cast them`
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

    // 10. `<subj> aren't put into their owners' graveyards for having 0 loyalty`
    if ts.eat_phrase(&[
        "aren't",
        "put",
        "into",
        "their",
        "owners'",
        "graveyards",
        "for",
        "having",
    ]) && ts.next_number(false) == Some(0)
        && ts.eat_word("loyalty")
    {
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::DontLoseForZeroLife,
        ));
    }
    ts.restore(cp);

    // 11. `<player> reveals each card they draw`
    if ts.eat_phrase(&["reveals", "each", "card", "they", "draw"]) {
        return Ok(StaticAbility::VisibilityRule {
            player: PlayerRef::ChosenPlayer,
            kind: crate::v2::ir::VisibilityKind::PlayWithTopRevealed,
        });
    }

    // 12. `<subj> of the first chosen type are the second chosen type`
    if ts.eat_phrase(&[
        "of", "the", "first", "chosen", "type", "are", "the", "second", "chosen", "type",
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject: subject.clone(),
            rule: SubjectStaticRule::IsChosenType,
            condition: condition.clone(),
        });
    }
    ts.restore(cp);

    // 13. `<subj> gets +X/+Y or -X/-Y` with `where X is ... and Y is ...` or multi-sentence X/Y definitions
    if ts.eat_any_word(&["gets", "get"])
        && let Some(is_neg) = eat_xy_pt_mod(ts)
    {
        let delta = if is_neg {
            crate::v2::ir::PtDelta::MinusX
        } else {
            crate::v2::ir::PtDelta::PlusX
        };
        let x_val_opt = if ts.eat_comma() && ts.eat_phrase(&["where", "x", "is"]) {
            let _ = ts.eat_word("half");
            let x_val = if ts.eat_phrase(&["the", "exiled", "creature", "card's", "power"]) {
                Some(crate::v2::ir::ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(
                        crate::v2::ir::AnaphoraRef::TheExiledCard,
                    )),
                    stat: crate::ast::StatKind::Power,
                })
            } else {
                try_parse(ts, parse_value_expr).ok()
            };
            let _ = ts.eat_rounded_modifier();
            let _ = ts.eat_comma();
            if x_val.is_some() && ts.eat_phrase(&["and", "y", "is"]) && {
                let _ = ts.eat_word("half");
                ts.eat_phrase(&["its", "toughness"]) || try_parse(ts, parse_value_expr).is_ok()
            } {
                let _ = ts.eat_rounded_modifier();
                x_val
            } else {
                None
            }
        } else if ts.eat_period()
            && ts.eat_phrase(&["if", "that", "creature", "is", "attacking"])
            && ts.eat_comma()
            && ts.eat_phrase(&["x", "is"])
            && let Ok(x_val) = try_parse(ts, parse_value_expr)
            && ts.eat_period()
            && ts.eat_word("otherwise")
            && ts.eat_comma()
            && ts.eat_phrase(&["x", "is"])
            && try_parse(ts, parse_value_expr).is_ok()
            && ts.eat_period()
            && ts.eat_phrase(&[
                "y",
                "is",
                "equal",
                "to",
                "x",
                "or",
                "to",
                "enchanted",
                "creature's",
                "toughness",
                "minus",
            ])
            && ts.next_number(false).is_some()
            && ts.eat_comma()
            && ts.eat_phrase(&["whichever", "is", "smaller"])
        {
            Some(x_val)
        } else {
            None
        };
        if let Some(expr) = x_val_opt {
            return Ok(StaticAbility::BindVariable {
                var: crate::v2::ir::VarName::X,
                expr,
                inner: Box::new(StaticAbility::ContinuousModifyPT {
                    subject: subject.clone(),
                    power: delta,
                    toughness: delta,
                    grant_keywords: Vec::new(),
                    scale: None,
                    condition: condition.clone(),
                }),
            });
        }
    }
    ts.restore(cp);

    Err(ts.err("not an extended subject predicate"))
}

fn eat_xy_pt_mod(ts: &mut TokenStream<'_>) -> Option<bool> {
    use crate::v2::lexer::TokenKind;
    let cp = ts.checkpoint();
    let is_neg = match ts.peek().map(|t| &t.kind) {
        Some(TokenKind::Plus) => false,
        Some(TokenKind::Minus) => true,
        _ => return None,
    };
    ts.next_tok();
    if ts.eat_word("x")
        && matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Slash))
        && ts.next_tok().is_some()
        && matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Plus | TokenKind::Minus)
        )
        && ts.next_tok().is_some()
        && ts.eat_word("y")
    {
        return Some(is_neg);
    }
    ts.restore(cp);
    None
}

/// Try to parse extended `protection from ...` / `hexproof from ...` clauses.
pub(super) fn try_parse_extended_prot_or_hexproof(ts: &mut TokenStream<'_>) -> Option<KeywordSpec> {
    let cp = ts.checkpoint();
    let kind = if ts.eat_phrase(&["protection", "from"]) {
        KeywordKind::Protection
    } else if ts.eat_phrase(&["hexproof", "from"]) {
        KeywordKind::Hexproof
    } else {
        return None;
    };
    let matched = ts.eat_any_phrase(&[
        &["each", "of", "its", "colors"],
        &["each", "of", "your", "opponents"],
        &["the", "chosen", "card", "type"],
        &["the", "last", "chosen", "color"],
        &["each", "of", "the", "exiled", "card's", "card", "types"],
        &["ring-bearers"],
        &[
            "each", "mana", "value", "other", "than", "the", "chosen", "number",
        ],
        &["each", "mana", "value", "of", "the", "chosen", "quality"],
        &[
            "each",
            "color",
            "that's",
            "not",
            "in",
            "your",
            "commander's",
            "color",
            "identity",
        ],
    ]) || (ts.eat_any_phrase(&[
        &["each", "mana", "value", "among"],
        &["each", "color", "among"],
    ]) && try_parse(ts, parse_object_filter).is_ok())
        || (try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["and", "from", "each", "color"]));
    if matched {
        return Some(KeywordSpec { kind, param: None });
    }
    ts.restore(cp);
    None
}

/// Try to parse qualified landwalk (`nonbasic landwalk`, `artifact landwalk`, `landwalk of the chosen type`).
pub(super) fn try_parse_extended_landwalk(ts: &mut TokenStream<'_>) -> Option<KeywordSpec> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["landwalk", "of", "the", "chosen", "type"])
        || (ts.eat_any_word(&["nonbasic", "legendary", "snow", "artifact"])
            && ts.eat_any_word(&[
                "landwalk",
                "forestwalk",
                "swampwalk",
                "plainswalk",
                "islandwalk",
                "mountainwalk",
            ]))
    {
        return Some(KeywordSpec {
            kind: KeywordKind::Landwalk,
            param: None,
        });
    }
    ts.restore(cp);
    None
}

/// Shared tail combinator for subject-headed static abilities:
/// chains `[, ] [and] has/have "<quoted>"`, `[, ] [and] has/have <kws>`,
/// `[, ] [and] can't <rule>`, `[, ] [and] [it] loses all other abilities`,
/// and `[, ] [and] is/are [a/an] <colors/subtypes> [in addition to ...]` onto `parts`.
pub(super) fn parse_static_subject_tail(
    ts: &mut TokenStream<'_>,
    subject: &SubjectRef,
    condition: &Option<ConditionExpr>,
    parts: &mut Vec<StaticAbility>,
) {
    loop {
        let cp = ts.checkpoint();
        // Trailing `named <Proper Name>` (e.g. `Witness Protection`, `Honest Work`)
        if ts.eat_word("named") {
            let mut count = 0usize;
            while count < 4
                && let Some(w) = ts.peek_word()
                && w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            {
                ts.next_word();
                count += 1;
            }
            if count > 0 {
                continue;
            }
            ts.restore(cp);
        }
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
        // `and other players can't gain control of it/them`
        if ts.eat_phrase(&["other", "players", "can't", "gain", "control", "of"])
            && ts.eat_any_word(&["them", "it"])
        {
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: RuleRestrictionKind::CantBeCast,
                condition: condition.clone(),
            });
            continue;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        // `its name and creature type are the last chosen name and creature type`
        if ts.eat_phrase(&[
            "its", "name", "and", "creature", "type", "are", "the", "last", "chosen", "name",
            "and", "creature", "type",
        ]) {
            parts.push(StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::IsChosenType,
                condition: condition.clone(),
            });
            continue;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
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
                ]) || (ts.eat_phrase(&["all", "abilities", "of", "ability", "stickers", "on"])
                    && ts.eat_tilde())
                {
                    parts.push(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
                    continue;
                }
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
                if let Some(kw) = try_parse_extended_prot_or_hexproof(ts)
                    .or_else(|| try_parse_extended_landwalk(ts))
                {
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: vec![kw],
                        condition: condition.clone(),
                    });
                    continue;
                }
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
        // `can only attack alone` (`Errantry`)
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
            if (ts.eat_phrase(&["as", "though", "they", "didn't", "have"])
                || ts.eat_phrase(&["as", "though", "it", "had"]))
                && (ts.eat_phrase(&["those", "abilities"])
                    || try_parse(ts, parse_keyword_kind).is_ok())
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
        let had_and = ts.eat_word("and");
        // Trailing `[, ] [and] equip [<subtype>] {M}` after `"<quoted>"`
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
                if ts.peek_word().is_some_and(|w| {
                    ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
                        || crate::vocab::lookup_card_type(&w.to_ascii_lowercase()).is_some()
                }) {
                    ts.next_word();
                }
                if let Some(mc) = ts.eat_mana_cost() {
                    parts.push(StaticAbility::GrantKeywords {
                        subject: subject.clone(),
                        keywords: vec![KeywordSpec {
                            kind: KeywordKind::Equip,
                            param: Some(crate::v2::ir::KeywordParam::Cost(vec![
                                crate::v2::ir::Cost::Mana(mc),
                            ])),
                        }],
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(eq_cp);
        }
        let _ = had_and;
        // `assigns combat damage equal to its toughness rather than its power`
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
                    let _ = opt(ts, parse_static_scale_expr);
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
        // `must be blocked [by <flt>] if able`
        {
            let mb_cp = ts.checkpoint();
            if ts.eat_phrase(&["must", "be", "blocked"]) {
                if ts.eat_word("by") {
                    let _ = try_parse(ts, parse_object_filter);
                }
                if ts.eat_phrase(&["if", "able"]) {
                    parts.push(StaticAbility::RuleRestriction {
                        subject: Some(subject.clone()),
                        rule: RuleRestrictionKind::MustBeBlocked,
                        condition: condition.clone(),
                    });
                    continue;
                }
            }
            ts.restore(mb_cp);
        }
        // `[it/they] loses/lose all [other] abilities[, card types, and creature types]`
        {
            let lcp = ts.checkpoint();
            let _ = ts.eat_word("it") || ts.eat_word("they");
            if ts.eat_phrase(&["loses", "all"]) || ts.eat_phrase(&["lose", "all"]) {
                let _ = ts.eat_word("other");
                let mut ate = false;
                while ts.eat_word("abilities")
                    || ts.eat_phrase(&["card", "types"])
                    || ts.eat_phrase(&["creature", "types"])
                    || ts.eat_word("types")
                {
                    ate = true;
                    let sep_cp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    let more = ts.peek_word().is_some_and(|w| {
                        let l = w.to_ascii_lowercase();
                        l == "abilities" || l == "card" || l == "creature" || l == "types"
                    });
                    if !more {
                        ts.restore(sep_cp);
                        break;
                    }
                }
                if ate {
                    parts.push(StaticAbility::RemoveAllAbilities {
                        subject: subject.clone(),
                    });
                    continue;
                }
            }
            ts.restore(lcp);
        }
        ts.restore(cp);
        let cp2 = ts.checkpoint();
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        // `is/are [a/an] [legendary] <colors/subtypes/types> [in addition to ...]`
        if ts.eat_word("is") || ts.eat_word("are") {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            let _ = ts.eat_word("legendary");
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
pub(crate) fn parse_cant_rule(ts: &mut TokenStream<'_>) -> PResult<RuleRestrictionKind> {
    if ts.eat_any_phrase(&[&["become", "untapped"], &["become", "suspected"]]) {
        return Ok(RuleRestrictionKind::CantBecomeUntapped);
    }
    if ts.eat_word("enter") {
        let _ = ts.eat_phrase(&["the", "battlefield"]);
        if ts.eat_word("from") {
            let _ = opt(ts, parse_zone_ref);
        }
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_phrase(&["be", "sacrificed"]) {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_phrase(&["be", "equipped"]) {
        return Ok(RuleRestrictionKind::CantBeEnchantedBy(Box::default()));
    }
    if ts.eat_phrase(&["be", "regenerated"]) {
        let _ = ts.eat_phrase(&["this", "turn"]);
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_phrase(&["be", "turned", "face", "up"]) {
        let _ = ts.eat_phrase(&["during", "your", "turn"]);
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_phrase(&[
        "be",
        "destroyed",
        "by",
        "lethal",
        "damage",
        "unless",
        "lethal",
        "damage",
        "dealt",
        "by",
        "a",
        "single",
        "source",
        "is",
        "marked",
        "on",
        "it",
    ]) {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_word("transform") || ts.eat_any_phrase(&[&["phase", "in"], &["phase", "out"]]) {
        return Ok(RuleRestrictionKind::CantPhaseOut);
    }
    // `have [<kind>] counters put on it/them` / `have more than <N> <kind> counters on it`
    {
        let hcp = ts.checkpoint();
        if ts.eat_word("have") {
            if ts.eat_phrase(&["more", "than"])
                && ts.next_number(false).is_some()
                && try_parse(ts, parse_counter_kind).is_ok()
                && ts.eat_any_word(&["counters", "counter"])
                && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
            {
                return Ok(RuleRestrictionKind::CantBeCast);
            }
            ts.restore(hcp);
            let _ = ts.eat_word("have");
            let _ = opt(ts, parse_counter_kind);
            if ts.eat_any_word(&["counters", "counter"])
                && ts.eat_phrase(&["put", "on"])
                && ts.eat_any_word(&["it", "them"])
            {
                return Ok(RuleRestrictionKind::CantBeCast);
            }
        }
        ts.restore(hcp);
    }
    // `cause <player> to <sacrifice/exile/discard/search ...> [or <...>]`
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("cause")
            && (ts.eat_any_word(&["you", "players"])
                || ts.eat_any_phrase(&[
                    &["their", "controller"],
                    &["its", "controller"],
                    &["a", "player"],
                ]))
            && ts.eat_word("to")
        {
            let mut matched_any = false;
            loop {
                let acp = ts.checkpoint();
                let matched_action = ((ts.eat_phrase(&["sacrifice", "or", "exile"])
                    || ts.eat_any_word(&["sacrifice", "exile", "discard"]))
                    && try_parse(ts, parse_object_filter).is_ok())
                    || (ts.eat_word("search")
                        && (ts.eat_any_phrase(&[
                            &["their", "library"],
                            &["their", "libraries"],
                            &["a", "library"],
                        ]) || ts.eat_word("libraries")));
                if matched_action {
                    matched_any = true;
                } else {
                    ts.restore(acp);
                    break;
                }
                if !ts.eat_word("or") {
                    break;
                }
            }
            if matched_any {
                return Ok(RuleRestrictionKind::CantBeCast);
            }
        }
        ts.restore(ccp);
    }
    // Player-like restrictions when subject was a filtered player (`Angelic Arbiter`, `Ward of Bones`, `Damping Engine`, `Ethersworn Canonist`, `Void Winnower`, `Maralen`, `Melira`, `Angel of Jubilation`, `Yasharn`, `Karn's Sylex`, `Hogaak`, `Stabilizer`, `Keen-Eared Sentry`, `Enthralling Hold`, `Dream Leash`)
    if ts.eat_phrase(&["draw", "cards"]) {
        let _ = ts.eat_phrase(&["or", "gain", "life"]);
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_phrase(&["cycle", "cards"]) {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_word("get") {
        let gcp = ts.checkpoint();
        let _ = opt(ts, parse_counter_kind);
        if ts.eat_word("counters") || ts.eat_word("counter") {
            return Ok(RuleRestrictionKind::CantBeCast);
        }
        ts.restore(gcp);
    }
    if ts.eat_phrase(&["spend", "mana", "to", "cast"]) && ts.eat_tilde() {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    if ts.eat_phrase(&[
        "venture", "into", "the", "dungeon", "more", "than", "once", "each", "turn",
    ]) {
        return Ok(RuleRestrictionKind::CantBeCast);
    }
    {
        let ch_cp = ts.checkpoint();
        if ts.eat_word("choose")
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_word("as")
            && ts.eat_tilde()
            && ts.eat_phrase(&["'s", "target", "as", "you", "cast", "it"])
        {
            return Ok(RuleRestrictionKind::CantBeCast);
        }
        ts.restore(ch_cp);
    }
    {
        let pl_cp = ts.checkpoint();
        if ts.eat_phrase(&["pay", "life"]) {
            if ts.eat_phrase(&["or", "sacrifice"]) {
                let _ = opt(ts, parse_object_filter);
            }
            if ts.eat_phrase(&["to", "cast", "spells", "or"]) {
                let _ = ts.eat_word("to");
                if ts.eat_phrase(&["activate", "abilities"]) {
                    let _ = ts.eat_phrase(&["that", "aren't", "mana", "abilities"]);
                    return Ok(RuleRestrictionKind::CantBeCast);
                }
            }
        }
        ts.restore(pl_cp);
    }
    {
        let ply_cp = ts.checkpoint();
        if ts.eat_phrase(&["play", "lands"]) {
            if ts.eat_phrase(&["or", "cast"]) {
                let _ = opt(ts, parse_object_filter);
                while ts.eat_comma() || ts.eat_word("or") {
                    let _ = ts.eat_word("or");
                    if try_parse(ts, parse_object_filter).is_err() {
                        break;
                    }
                }
            }
            return Ok(RuleRestrictionKind::CantBeCast);
        }
        ts.restore(ply_cp);
    }
    {
        let cst_cp = ts.checkpoint();
        if ts.eat_word("cast") {
            let _ = ts.eat_word("additional");
            if try_parse(ts, parse_object_filter).is_ok() {
                let _ = ts.eat_phrase(&["with", "even", "mana", "values"])
                    || ts.eat_phrase(&["with", "odd", "mana", "values"]);
                return Ok(RuleRestrictionKind::CantBeCast);
            }
        }
        ts.restore(cst_cp);
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
        // Optional `that can target only <flt>` / `or [of] abilities [from <flt> | that can target only <flt>]`
        let _ = ts.eat_phrase(&["that", "can", "target", "only"])
            && opt(ts, parse_object_filter).is_some();
        let ocp = ts.checkpoint();
        if ts.eat_word("or") {
            let _ = ts.eat_word("of");
            if ts.eat_word("abilities") {
                if ts.eat_word("from") || ts.eat_phrase(&["that", "can", "target", "only"]) {
                    let _ = opt(ts, parse_object_filter);
                }
            } else {
                ts.restore(ocp);
            }
        }
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
        if ts.eat_comma()
            && ts.eat_word("block")
            && ts.eat_comma()
            && (ts.eat_phrase(&["or", "crew", "vehicles"]) || ts.eat_phrase(&["or", "transform"]))
        {
            return Ok(RuleRestrictionKind::CantAttackOrBlock);
        }
        if ts.eat_word("alone") {
            return Ok(RuleRestrictionKind::CantAttackAlone);
        }
        if ts.eat_phrase(&["during", "extra", "turns"]) {
            return Ok(RuleRestrictionKind::CantAttack);
        }
        if ts.eat_word("with") && try_parse(ts, parse_object_filter).is_ok() {
            return Ok(RuleRestrictionKind::CantAttack);
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
        }) && let Ok(mut tgt) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_phrase(&["it", "has", "already", "attacked", "this", "turn"]);
            let _ = ts.eat_phrase(&["its", "owner", "controls"]);
            let ob_cp = ts.checkpoint();
            if ts.eat_phrase(&["or", "block"]) && try_parse(ts, parse_subject_ref).is_ok() {
                return Ok(RuleRestrictionKind::CantAttackOrBlock);
            }
            ts.restore(ob_cp);
            let or_cp = ts.checkpoint();
            if ts.eat_word("or") {
                if let Ok(tgt2) = try_parse(ts, parse_subject_ref) {
                    let _ = ts.eat_phrase(&["its", "owner", "controls"]);
                    tgt = SubjectRef::Compound(vec![tgt, tgt2]);
                } else {
                    ts.restore(or_cp);
                }
            }
            if ts.eat_word("unless") {
                let cond = parse_static_condition(ts)?;
                return Ok(RuleRestrictionKind::CantAttackUnless(Box::new(cond)));
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
        if ts.eat_word("with") && try_parse(ts, parse_object_filter).is_ok() {
            let _ = ts.eat_phrase(&["with", "even", "mana", "values"])
                || ts.eat_phrase(&["with", "odd", "mana", "values"]);
            return Ok(RuleRestrictionKind::CantBlock);
        }
        if ts.eat_word("unless") {
            let cond = parse_static_condition(ts)?;
            return Ok(RuleRestrictionKind::CantBlockUnless(Box::new(cond)));
        }
        // `block creatures with power equal to or greater than the enchanted creature's toughness`
        if ts.eat_phrase(&[
            "creatures",
            "with",
            "power",
            "equal",
            "to",
            "or",
            "greater",
            "than",
            "the",
            "enchanted",
            "creature's",
            "toughness",
        ]) {
            return Ok(RuleRestrictionKind::CantBlock);
        }
        if let Ok(blocked_subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("unless") {
                let ucp = ts.checkpoint();
                if (ts.eat_phrase(&["you", "pay"])
                    || ts.eat_phrase(&["their", "controller", "pays"])
                    || ts.eat_phrase(&["its", "controller", "pays"]))
                    && ((ts.next_number(false).is_some() && ts.eat_word("life"))
                        || ts.eat_mana_cost().is_some()
                        || try_parse(ts, crate::v2::cost::parse_cost_atom).is_ok())
                {
                    let _ = opt(ts, parse_static_scale_expr);
                    let _ =
                        ts.eat_phrase(&["for", "each", "blocking", "creature", "they", "control"]);
                    return Ok(RuleRestrictionKind::CantBlockSubject(Box::new(
                        blocked_subj,
                    )));
                }
                ts.restore(ucp);
                let cond = parse_static_condition(ts)?;
                return Ok(RuleRestrictionKind::CantBlockUnless(Box::new(cond)));
            }
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
            let _ = ts.eat_phrase(&["the", "monarch", "controls"])
                || ts.eat_phrase(&["that", "don't", "have", "a", "name"]);
            return Ok(RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)));
        }
        if ts.eat_word("unless") {
            let _ = parse_static_condition(ts)?;
            return Ok(RuleRestrictionKind::CantBeBlocked);
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
