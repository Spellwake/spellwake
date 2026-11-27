//! Subject-initiated verb frames and trailing duration clauses for `v2`.

use crate::ast::CardTypeMask;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{
    AnaphoraRef, CharacteristicKind, ChoiceKind, Duration, Effect, ObjectKeywordAction,
    ObjectMutation, ObjectStateAction, PlayerMutation, PlayerRef, Replacement, RuleRestrictionKind,
    ScaledPlayerAction, StaticAbility, SubjectRef, SubjectStaticRule, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};
use crate::vocab::lookup_card_type;

pub(super) use super::subject_combat::parse_trailing_duration;
use super::subject_combat::{parse_damage_target, parse_subject_cant};
use super::{
    apply_continuous, apply_replacement, compile_quoted_permanent, eat_type_color_subtype_run,
    mutate_obj, mutate_player, prompt_choice,
};

const DONT_UNTAP_STEPS: &[&[&str]] = &[
    &["your", "next", "untap", "step"],
    &["its", "controller's", "next", "untap", "step"],
    &["their", "controllers'", "next", "untap", "steps"],
];

fn try_parse_and_cant_rider(
    ts: &mut TokenStream<'_>,
    post_duration: bool,
) -> Option<ObjectMutation> {
    let mut_opt = if ts.eat_any_phrase(&[
        &[
            "and", "can't", "be", "blocked", "this", "turn", "except", "by",
        ],
        &["and", "can't", "be", "blocked", "except", "by"],
    ]) && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
    {
        Some(ObjectMutation::ApplyRule(
            RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)),
        ))
    } else if ts.eat_any_phrase(&[
        &["and", "can't", "be", "blocked", "this", "turn", "by"],
        &["and", "can't", "be", "blocked", "by"],
    ]) && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
    {
        Some(ObjectMutation::ApplyRule(
            RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)),
        ))
    } else if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
        Some(ObjectMutation::ApplyRule(
            RuleRestrictionKind::CantBeBlocked,
        ))
    } else if ts.eat_phrase(&["and", "can't", "block"]) {
        Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock))
    } else if ts.eat_phrase(&["and", "can't", "attack"]) {
        Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack))
    } else if !post_duration
        && ts.eat_any_phrase(&[
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
        ])
    {
        let _ = ts.eat_any_phrase(&[
            &["unless", "they're", "mana", "abilities"],
            &["unless", "they", "are", "mana", "abilities"],
        ]);
        return Some(ObjectMutation::ApplyRule(
            RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
        ));
    } else if post_duration && ts.eat_phrase(&["and", "can", "block", "an", "additional"]) {
        let extra = ts
            .next_number(false)
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(1);
        let _ = ts.eat_any_word(&["creature", "creatures"]);
        let _ = ts.eat_phrase(&["each", "combat"]);
        Some(ObjectMutation::CanBlockAdditional(extra))
    } else {
        None
    };
    if post_duration && mut_opt.is_some() {
        let _ = parse_trailing_duration(ts);
    }
    mut_opt
}

/// Subject-headed verb frames (`<src> deals ...`, `<subj> gets ...`, `<subj> gains ...`,
/// `<subj> becomes ...`, `<subj> is goaded ...`, `<subj> can't ...`).
pub(super) fn parse_subject_verb_frame(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    // `It's a/an [<P>/<T>] [<colors>] <Subtypes/Types> [with base power and toughness P/T] [in addition to its other [colors and] types] [dur]`
    {
        let its_cp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["it's", "a"], &["it's", "an"]])
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("copy"))
        {
            let mut base_pt = ts.next_base_pt();
            let (add_types, colors, subtypes) = eat_type_color_subtype_run(ts, true, false, true);
            if base_pt.is_none() && ts.eat_phrase(&["with", "base", "power", "and", "toughness"]) {
                base_pt = ts.next_base_pt();
            }
            if !subtypes.is_empty() || !add_types.is_empty() || base_pt.is_some() {
                let keep_existing_types = ts.eat_in_addition_to_other_types();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                if ts.is_eof() {
                    let subj = SubjectRef::Anaphora(AnaphoraRef::It);
                    let mut out = Vec::new();
                    if !colors.is_empty() {
                        out.push(mutate_obj(
                            subj.clone(),
                            ObjectMutation::BecomeColors {
                                colors,
                                keep_existing_colors: keep_existing_types,
                            },
                            duration.clone(),
                        ));
                    }
                    out.push(mutate_obj(
                        subj,
                        ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types,
                            base_pt,
                            keywords: Vec::new(),
                        },
                        duration,
                    ));
                    return Ok(out);
                }
            }
        }
        ts.restore(its_cp);
    }
    if ts.eat_phrase(&["damage", "can't", "be", "prevented"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![apply_continuous(
            StaticAbility::RuleRestriction {
                subject: None,
                rule: RuleRestrictionKind::DamageCantBePrevented,
                condition: None,
            },
            duration,
        )]);
    }
    let subject = parse_subject_ref(ts)?;
    let mut out = parse_subject_predicate(ts, subject.clone())?;
    // Compositional `, <pred2> [if <cond>], and <pred3> [if <cond>]` / `and <pred2>` chaining on the same subject
    loop {
        let ccp = ts.checkpoint();
        let had_comma = ts.eat_comma();
        let had_and = ts.eat_word("and");
        if (had_and || had_comma)
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("target"))
            && let Ok(more) = try_parse(ts, |s| parse_subject_predicate(s, subject.clone()))
        {
            let icp = ts.checkpoint();
            if ts.eat_word("if")
                && let Ok(cond) = try_parse(ts, parse_condition)
            {
                out.push(Effect::BranchCondition {
                    condition: cond,
                    then_body: more,
                    else_body: Vec::new(),
                });
            } else {
                ts.restore(icp);
                out.extend(more);
            }
            continue;
        }
        ts.restore(ccp);
        break;
    }
    Ok(out)
}

fn parse_subject_predicate(ts: &mut TokenStream<'_>, subject: SubjectRef) -> PResult<Vec<Effect>> {
    let _ = ts.eat_word("also") || ts.eat_word("each");

    // 1. `<src> deals <amt> damage ...` OR `<src> deals damage equal to <val> to <tgt>` OR `<src> deals damage to <tgt> equal to <val>`
    if ts.eat_word("deals") || ts.eat_word("deal") {
        // 1a. `deals damage equal to <val> [divided as <player> chooses among <tgt> | to <tgt>]`
        //     OR `deals damage to <tgt> equal to <val>`
        if ts.eat_word("damage") {
            if ts.eat_phrase(&["equal", "to"]) {
                let amount = parse_value_expr(ts)?;
                if ts.eat_phrase(&["divided", "as"]) {
                    let divider = parse_player_ref(ts)?;
                    if !(ts.eat_word("chooses") || ts.eat_word("choose")) || !ts.eat_word("among") {
                        return Err(ts.err("expected `chooses among` in divided damage"));
                    }
                    let target = parse_subject_ref(ts)?;
                    return Ok(vec![Effect::DealDamage {
                        source: subject,
                        target,
                        amount,
                        divided_by: Some(divider),
                    }]);
                }
                if !ts.eat_word("to") {
                    return Err(ts.err("expected `to` after `damage equal to <val>`"));
                }
                let target = parse_damage_target(ts)?;
                return Ok(vec![Effect::DealDamage {
                    source: subject,
                    target,
                    amount,
                    divided_by: None,
                }]);
            }
            if ts.eat_word("to") {
                let target = parse_damage_target(ts)?;
                if ts.eat_phrase(&["equal", "to"]) {
                    let amount = parse_value_expr(ts)?;
                    return Ok(vec![Effect::DealDamage {
                        source: subject,
                        target,
                        amount,
                        divided_by: None,
                    }]);
                }
                return Err(ts.err("expected `equal to <val>` after `deals damage to <tgt>`"));
            }
            return Err(ts.err("expected `equal to` or `to` after `deals damage`"));
        }

        let mut out = Vec::new();
        loop {
            let amount = if ts.eat_phrase(&["that", "much"]) {
                ValueExpr::PriorActionCount(crate::v2::ir::PriorActionKind::DealtDamageThisWay)
            } else {
                parse_value_expr(ts)?
            };
            let _ = ts.eat_word("of");
            if !ts.eat_word("damage") {
                return Err(ts.err("expected `damage` after amount"));
            }
            // Optional `, rounded down/up [,]` (e.g. `half X damage, rounded down, to any target`)
            if ts.eat_rounded_modifier().is_some() {
                let _ = ts.eat_comma();
            }
            if ts.eat_phrase(&["divided", "evenly"]) {
                if ts.eat_rounded_modifier().is_some() {
                    let _ = ts.eat_comma();
                }
                if !ts.eat_word("among") {
                    return Err(ts.err("expected `among` after `divided evenly`"));
                }
                let target = parse_subject_ref(ts)?;
                out.push(Effect::DealDamage {
                    source: subject.clone(),
                    target,
                    amount,
                    divided_by: Some(PlayerRef::You),
                });
                break;
            }
            if ts.eat_phrase(&["divided", "as"]) {
                let divider = parse_player_ref(ts)?;
                if !ts.eat_any_word(&["chooses", "choose"]) || !ts.eat_word("among") {
                    return Err(ts.err("expected `chooses among` in divided damage"));
                }
                let target = parse_subject_ref(ts)?;
                out.push(Effect::DealDamage {
                    source: subject.clone(),
                    target,
                    amount,
                    divided_by: Some(divider),
                });
                break;
            }
            if ts.is_eof()
                || ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("instead"))
            {
                out.push(Effect::DealDamage {
                    source: subject.clone(),
                    target: SubjectRef::Anaphora(AnaphoraRef::SameTarget),
                    amount,
                    divided_by: None,
                });
                break;
            }
            if !ts.eat_word("to") {
                return Err(ts.err("expected `to` after `damage`"));
            }
            let mut target = parse_damage_target(ts)?;
            {
                let ecp = ts.checkpoint();
                let _ = ts.eat_comma();
                if ts.eat_word("except") {
                    let _ = ts.eat_word("for");
                    if let Ok(ex_subj) = try_parse(ts, parse_subject_ref) {
                        match &mut target {
                            SubjectRef::All(f) | SubjectRef::Each(f) | SubjectRef::Group(f) => {
                                f.predicates.push(crate::v2::ir::FilterPredicate::Exclude(
                                    Box::new(ex_subj),
                                ));
                            }
                            _ => {}
                        }
                    } else {
                        ts.restore(ecp);
                    }
                } else {
                    ts.restore(ecp);
                }
            }
            let scaled_amt = if let Some(fe) = opt(ts, parse_for_each_expr) {
                if amount == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(amount), Box::new(fe))
                }
            } else {
                amount
            };
            out.push(Effect::DealDamage {
                source: subject.clone(),
                target,
                amount: scaled_amt,
                divided_by: None,
            });
            if ts.eat_any_phrase(&[
                &["and", "doesn't", "untap", "during"],
                &["and", "don't", "untap", "during"],
            ]) && ts.eat_any_phrase(DONT_UNTAP_STEPS)
            {
                out.push(mutate_obj(
                    subject.clone(),
                    ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                    Duration::UntilYourNextTurn,
                ));
            }
            let cp = ts.checkpoint();
            let has_sep = if ts.eat_comma() {
                let _ = ts.eat_word("and");
                true
            } else {
                ts.eat_word("and")
            };
            if has_sep
                && (ts.eat_phrase(&["that", "much"]) || opt(ts, parse_value_expr).is_some())
                && ts.eat_word("damage")
            {
                ts.restore(cp);
                if ts.eat_comma() {
                    let _ = ts.eat_word("and");
                } else {
                    ts.eat_word("and");
                }
                continue;
            }
            ts.restore(cp);
            break;
        }
        return Ok(out);
    }

    // 2. `<subj> gets/get [an additional | twice] +X/+Y [and gains <kws>] [until end of turn] [for each <expr>]`
    if ts.eat_any_word(&["gets", "get"]) {
        let _ = ts.eat_phrase(&["an", "additional"]);
        let is_twice = ts.eat_word("twice");
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        if ts.eat_word("or")
            && let Some((p2, t2)) = ts.next_pt_mod()
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![prompt_choice(
                PlayerRef::You,
                ChoiceKind::ChooseBranch(vec![
                    vec![mutate_obj(
                        subject.clone(),
                        ObjectMutation::ModifyPT {
                            power,
                            toughness,
                            scale: None,
                        },
                        duration.clone(),
                    )],
                    vec![mutate_obj(
                        subject,
                        ObjectMutation::ModifyPT {
                            power: p2,
                            toughness: t2,
                            scale: None,
                        },
                        duration,
                    )],
                ]),
            )]);
        }
        let mut gained_kws = None;
        let mut gained_ability = None;
        let mut gained_all_creature_types = false;
        if ts.eat_any_phrase(&[
            &["and", "gains"],
            &["and", "gain"],
            &["and", "has"],
            &["and", "have"],
        ]) {
            if ts.eat_phrase(&["all", "creature", "types"]) {
                gained_all_creature_types = true;
            } else if let Some(quoted) = ts.next_quoted() {
                gained_ability = Some(compile_quoted_permanent(ts, quoted)?);
            } else {
                let _ = ts.eat_phrase(&["your", "choice", "of"]);
                let (kws, _) = parse_keyword_list(ts)?;
                gained_kws = Some(kws);
                let qcp = ts.checkpoint();
                let _ = ts.eat_comma();
                if ts.eat_word("and")
                    && let Some(quoted) = ts.next_quoted()
                {
                    gained_ability = Some(compile_quoted_permanent(ts, quoted)?);
                } else {
                    ts.restore(qcp);
                }
            }
        }
        let mut lost_kws = None;
        let mut lost_all_abilities = false;
        let mut lost_all_creature_types = false;
        if ts.eat_any_phrase(&[&["and", "loses"], &["and", "lose"]]) {
            if ts.eat_phrase(&["all", "creature", "types"]) {
                lost_all_creature_types = true;
            } else if ts.eat_phrase(&["all", "abilities"]) {
                lost_all_abilities = true;
            } else {
                let (kws, _) = parse_keyword_list(ts)?;
                lost_kws = Some(kws);
            }
        }
        let mut cant_mut = try_parse_and_cant_rider(ts, false);
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        if cant_mut.is_none() {
            cant_mut = try_parse_and_cant_rider(ts, true);
        }
        if gained_kws.is_none()
            && ts.eat_any_phrase(&[&["and", "gains"], &["and", "gain"]])
            && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
        {
            let _ = parse_trailing_duration(ts);
            gained_kws = Some(kws);
        }
        let scale = opt(ts, parse_for_each_expr)
            .map(|fe| {
                if is_twice {
                    ValueExpr::Mul(Box::new(ValueExpr::Const(2)), Box::new(fe))
                } else {
                    fe
                }
            })
            .or_else(|| is_twice.then_some(ValueExpr::Const(2)))
            .map(Box::new);
        if cant_mut.is_none() && ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
            let _ = parse_trailing_duration(ts);
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlocked,
            ));
        }
        let mut out = vec![mutate_obj(
            subject.clone(),
            ObjectMutation::ModifyPT {
                power,
                toughness,
                scale,
            },
            duration.clone(),
        )];
        if gained_all_creature_types {
            out.push(apply_continuous(
                StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::AllCreatureTypes,
                    condition: None,
                },
                duration.clone(),
            ));
        }
        if let Some(kws) = gained_kws {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::GrantKeywords(kws),
                duration.clone(),
            ));
        }
        if let Some(ab) = gained_ability {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::GrantAbility(Box::new(ab)),
                duration.clone(),
            ));
        }
        if lost_all_creature_types {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::BecomeCreatureTypes {
                    subtypes: Vec::new(),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: false,
                    base_pt: None,
                    keywords: Vec::new(),
                },
                duration.clone(),
            ));
        }
        if lost_all_abilities {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::RemoveAllAbilities,
                duration.clone(),
            ));
        }
        if let Some(kws) = lost_kws {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::RemoveKeywords(kws),
                duration.clone(),
            ));
        }
        if let Some(cm) = cant_mut {
            out.push(mutate_obj(subject.clone(), cm, duration.clone()));
        }
        append_trailing_subject_riders(ts, &subject, &duration, &mut out);
        return Ok(out);
    }

    // 3. `<subj> gains/gain "<quoted ability>" [and "<q2>"] [duration]` OR `<subj> gains/gain [your choice of] <kws> [and gets +P/+T] [duration]`
    //    CRITICAL RULE: Unqualified `gains <kw>` defaults to `Duration::Permanent`!
    if ts.eat_any_word(&["gains", "gain"]) {
        if let Some(quoted) = ts.next_quoted() {
            let mut granted = vec![compile_quoted_permanent(ts, quoted)?];
            loop {
                let qcp = ts.checkpoint();
                let had_comma = ts.eat_comma();
                let had_and = ts.eat_word("and");
                if (had_comma || had_and)
                    && let Some(q2) = ts.next_quoted()
                {
                    granted.push(compile_quoted_permanent(ts, q2)?);
                    continue;
                }
                ts.restore(qcp);
                break;
            }
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(granted
                .into_iter()
                .map(|ab| {
                    mutate_obj(
                        subject.clone(),
                        ObjectMutation::GrantAbility(Box::new(ab)),
                        duration.clone(),
                    )
                })
                .collect());
        }
        if ts.eat_phrase(&["that", "ability"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![mutate_obj(
                subject,
                ObjectMutation::GrantKeywords(Vec::new()),
                duration,
            )]);
        }
        if ts.eat_any_phrase(&[
            &["all", "basic", "land", "types"],
            &["all", "creature", "types"],
        ]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![apply_continuous(
                StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::AllCreatureTypes,
                    condition: None,
                },
                duration,
            )]);
        }
        if ts.eat_phrase(&["all", "activated", "abilities", "of"]) {
            let flt_opt = if let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter) {
                Some(flt)
            } else if try_parse(ts, parse_subject_ref).is_ok() {
                Some(crate::v2::ir::ObjectFilter::default())
            } else {
                None
            };
            if let Some(flt) = flt_opt {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![apply_continuous(
                    StaticAbility::SubjectRule {
                        subject,
                        rule: SubjectStaticRule::CopyActivatedAbilities(flt),
                        condition: None,
                    },
                    duration,
                )]);
            }
        }
        let is_choice = ts.eat_phrase(&["your", "choice", "of"]);
        let (kws, _) = parse_keyword_list(ts)?;
        let mut extra_pt = None;
        let mut extra_pt_scale = None;
        if ts.eat_any_phrase(&[&["and", "gets"], &["and", "get"]])
            && let Some((p, t)) = ts.next_pt_mod()
        {
            extra_pt = Some((p, t));
            extra_pt_scale = opt(ts, parse_for_each_expr).map(Box::new);
        }
        let mut extra_abilities = Vec::new();
        loop {
            let qcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_and = ts.eat_word("and");
            if (had_comma || had_and)
                && let Some(quoted) = ts.next_quoted()
            {
                extra_abilities.push(compile_quoted_permanent(ts, quoted)?);
                continue;
            }
            ts.restore(qcp);
            break;
        }
        let mut cant_mut = try_parse_and_cant_rider(ts, false);
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        if cant_mut.is_none() {
            cant_mut = try_parse_and_cant_rider(ts, true);
        }
        if is_choice {
            return Ok(vec![prompt_choice(
                PlayerRef::You,
                ChoiceKind::ChooseKeywordAndGrant {
                    subject,
                    keywords: kws,
                    duration,
                },
            )]);
        }
        let mut out = vec![mutate_obj(
            subject.clone(),
            ObjectMutation::GrantKeywords(kws),
            duration.clone(),
        )];
        if let Some((power, toughness)) = extra_pt {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::ModifyPT {
                    power,
                    toughness,
                    scale: extra_pt_scale,
                },
                duration.clone(),
            ));
        }
        for ab in extra_abilities {
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::GrantAbility(Box::new(ab)),
                duration.clone(),
            ));
        }
        if let Some(cm) = cant_mut {
            out.push(mutate_obj(subject.clone(), cm, duration.clone()));
        }
        append_trailing_subject_riders(ts, &subject, &duration, &mut out);
        return Ok(out);
    }

    // 4. `<subj> enters tapped [and attacking [that player/opponent]] [dur]` / `<subj> enters with <N> [additional] <kind> counters on it`
    if ts.eat_any_phrase(&[&["enters", "tapped"], &["enter", "tapped"]]) {
        if ts.eat_phrase(&["and", "attacking"]) {
            let _ = ts.eat_any_phrase(&[
                &["that", "opponent"],
                &["that", "player"],
                &["the", "defending", "player"],
                &["defending", "player"],
            ]);
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
        return Ok(vec![apply_replacement(
            Replacement::EntersTapped { unless: None },
            duration,
        )]);
    }
    if ts.eat_any_phrase(&[&["enters", "with"], &["enter", "with"]]) {
        let mut additional = false;
        let count = if ts.eat_phrase(&["an", "additional"]) {
            additional = true;
            ValueExpr::Const(1)
        } else if ts.eat_any_word(&["a", "an"]) {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        if ts.eat_word("additional") {
            additional = true;
        }
        let kind = parse_counter_kind(ts)?;
        if ts.eat_any_word(&["counters", "counter"])
            && ts.eat_any_phrase(&[&["on", "it"], &["on", "them"]])
        {
            let _ = ts.eat_rounded_modifier();
            return Ok(vec![apply_replacement(
                Replacement::EntersWithCounters {
                    subject: Some(subject),
                    kind,
                    count,
                    additional,
                },
                Duration::UntilEndOfTurn,
            )]);
        }
    }

    // 4b. `<subj> has/have base power and toughness <P>/<T> [dur]` OR `<subj> has/have base power/toughness <N> [dur]` OR `<subj> has/have "<quoted ability>" [dur]` OR `<subj> has/have <kws> [dur]`
    if ts.eat_any_word(&["has", "have"]) {
        if ts.eat_phrase(&["base", "power", "and", "toughness"]) {
            let pt_pair = if let Some((power, toughness)) = ts.next_base_pt() {
                Some((ValueExpr::Const(power), ValueExpr::Const(toughness)))
            } else if ts.eat_word("x")
                && matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Slash))
            {
                ts.next_tok();
                if ts.eat_word("x") {
                    Some((
                        ValueExpr::Var(crate::v2::ir::VarName::X),
                        ValueExpr::Var(crate::v2::ir::VarName::X),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((power, toughness)) = pt_pair {
                let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut out = vec![mutate_obj(
                    subject.clone(),
                    ObjectMutation::SetDynamicBasePT {
                        power,
                        toughness,
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration.clone(),
                )];
                loop {
                    let tcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if ts.eat_any_phrase(&[
                        &["gains", "all", "creature", "types"],
                        &["gain", "all", "creature", "types"],
                    ]) {
                        out.push(apply_continuous(
                            StaticAbility::SubjectRule {
                                subject: subject.clone(),
                                rule: SubjectStaticRule::AllCreatureTypes,
                                condition: None,
                            },
                            duration.clone(),
                        ));
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if ts.eat_any_word(&["gains", "gain", "has", "have"])
                        && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                    {
                        let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        if gdur != duration {
                            duration = gdur.clone();
                        }
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::GrantKeywords(kws),
                            gdur,
                        ));
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if ts.eat_phrase(&["can't", "be", "blocked"]) {
                        let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlocked),
                            bdur,
                        ));
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if ts.eat_any_phrase(&[&["isn't", "a"], &["isn't", "an"], &["aren't"]])
                        && let Some(w) = ts.peek_word()
                        && ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
                    {
                        ts.next_word();
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::BecomeCreatureTypes {
                                subtypes: Vec::new(),
                                add_types: CardTypeMask::EMPTY,
                                keep_existing_types: false,
                                base_pt: None,
                                keywords: Vec::new(),
                            },
                            duration.clone(),
                        ));
                        continue;
                    }
                    ts.restore(tcp);
                    break;
                }
                if let Some(fdur) = parse_trailing_duration(ts) {
                    for eff in &mut out {
                        if let Effect::MutateObject { duration: d, .. } = eff {
                            *d = fdur.clone();
                        }
                    }
                }
                return Ok(out);
            }
        }
        {
            let bpcp = ts.checkpoint();
            if ts.eat_phrase(&["base", "power"])
                && let Some(p) = ts.next_number(false)
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::SetDynamicBasePT {
                        power: ValueExpr::Const(p),
                        toughness: ValueExpr::ObjectStat {
                            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                            stat: crate::ast::StatKind::Toughness,
                        },
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration,
                )]);
            }
            ts.restore(bpcp);
            if ts.eat_phrase(&["base", "toughness"])
                && let Some(t) = ts.next_number(false)
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::SetDynamicBasePT {
                        power: ValueExpr::ObjectStat {
                            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                            stat: crate::ast::StatKind::Power,
                        },
                        toughness: ValueExpr::Const(t),
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration,
                )]);
            }
            ts.restore(bpcp);
        }
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_quoted_permanent(ts, quoted)?;
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![mutate_obj(
                subject,
                ObjectMutation::GrantAbility(Box::new(compiled)),
                duration,
            )]);
        }
        if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
            let mut out = vec![mutate_obj(
                subject.clone(),
                ObjectMutation::GrantKeywords(kws),
                Duration::Permanent,
            )];
            let qcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and")
                && let Some(quoted) = ts.next_quoted()
            {
                if let Ok(compiled) = compile_quoted_permanent(ts, quoted) {
                    out.push(mutate_obj(
                        subject.clone(),
                        ObjectMutation::GrantAbility(Box::new(compiled)),
                        Duration::Permanent,
                    ));
                } else {
                    ts.restore(qcp);
                }
            } else {
                ts.restore(qcp);
            }
            // Support `has flying and is an Angel in addition to its other types` (`Valkyrie's Call`)
            {
                let tcp = ts.checkpoint();
                if ts.eat_any_phrase(&[&["and", "is"], &["and", "are"]]) {
                    let _ = ts.eat_any_word(&["a", "an"]);
                    let (add_types, _, subtypes) =
                        eat_type_color_subtype_run(ts, false, false, false);
                    if !subtypes.is_empty() || !add_types.is_empty() {
                        let keep_existing_types = ts.eat_in_addition_to_other_types();
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::BecomeCreatureTypes {
                                subtypes,
                                add_types,
                                keep_existing_types,
                                base_pt: None,
                                keywords: Vec::new(),
                            },
                            Duration::Permanent,
                        ));
                    } else {
                        ts.restore(tcp);
                    }
                } else {
                    ts.restore(tcp);
                }
            }
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            for eff in &mut out {
                if let Effect::MutateObject { duration: d, .. } = eff {
                    *d = duration.clone();
                }
            }
            return Ok(out);
        }
    }

    // 5. `<subj> becomes ...` (`a copy of <src>`, `the color of your choice`, `a <Subtypes/Types> [with base power and toughness P/T] [with <kws>] [dur]`)
    {
        let becomes_cp = ts.checkpoint();
        if ts.eat_any_word(&["becomes", "become"]) {
            if ts.eat_word("colorless") {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeColors {
                        colors: crate::ast::ColorMask::EMPTY,
                        keep_existing_colors: false,
                    },
                    duration,
                )]);
            }
            if ts.eat_any_phrase(&[
                &["the", "color", "or", "colors", "of", "your", "choice"],
                &["the", "color", "of", "your", "choice"],
                &["the", "color", "of", "its", "controller's", "choice"],
                &["that", "color"],
            ]) {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeChosen(CharacteristicKind::Color),
                    duration,
                )]);
            }
            if ts.eat_word("prepared") {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::Prepare),
                    Duration::Permanent,
                )]);
            }
            if ts.eat_word("plotted") {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::GrantKeywords(vec![crate::v2::ir::KeywordSpec {
                        kind: crate::v2::ir::KeywordKind::Plot,
                        param: None,
                    }]),
                    Duration::Permanent,
                )]);
            }
            if ts.eat_word("foretold") {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::GrantKeywords(vec![crate::v2::ir::KeywordSpec {
                        kind: crate::v2::ir::KeywordKind::Foretell,
                        param: None,
                    }]),
                    Duration::Permanent,
                )]);
            }
            if ts.eat_phrase(&["the", "basic", "land", "type", "of", "your", "choice"]) {
                let _ = ts.eat_in_addition_to_other_types();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeChosen(CharacteristicKind::BasicLandType),
                    duration,
                )]);
            }
            if ts.eat_any_phrase(&[
                &["that", "type"],
                &["the", "creature", "type", "of", "your", "choice"],
            ]) {
                let _ = ts.eat_in_addition_to_other_types();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeChosen(CharacteristicKind::CreatureType),
                    duration,
                )]);
            }
            if ts.eat_any_phrase(&[&["a", "copy", "of"], &["copies", "of"]]) {
                let source = parse_subject_ref(ts)?;
                let mut exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                if exceptions.is_empty() {
                    exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                    if let Some(d2) = parse_trailing_duration(ts) {
                        duration = d2;
                    }
                }
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeCopy {
                        source: Box::new(source),
                        exceptions,
                    },
                    duration,
                )]);
            }
            let _ = ts.eat_any_word(&["a", "an"]);
            let is_xx = {
                let xcp = ts.checkpoint();
                if ts.eat_word("x")
                    && matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Slash))
                    && ts.peek_n(1).is_some_and(
                        |t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x")),
                    )
                {
                    ts.next_tok();
                    ts.next_tok();
                    true
                } else {
                    ts.restore(xcp);
                    false
                }
            };
            let mut base_pt = if is_xx { None } else { ts.next_base_pt() };
            let (add_types, colors, subtypes) = eat_type_color_subtype_run(ts, true, true, true);
            let mut keep_existing_types = ts.eat_in_addition_to_other_types();
            if is_xx {
                let mut kws = Vec::new();
                if ts.eat_word("with")
                    && let Ok((k, _)) = try_parse(ts, parse_keyword_list)
                {
                    kws = k;
                }
                if ts.eat_in_addition_to_other_types() {
                    keep_existing_types = true;
                }
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut out = vec![mutate_obj(
                    subject.clone(),
                    ObjectMutation::SetDynamicBasePT {
                        power: ValueExpr::Var(crate::v2::ir::VarName::X),
                        toughness: ValueExpr::Var(crate::v2::ir::VarName::X),
                        add_types,
                        keep_existing_types,
                    },
                    duration.clone(),
                )];
                if !subtypes.is_empty() || !kws.is_empty() {
                    out.push(mutate_obj(
                        subject,
                        ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types,
                            base_pt: None,
                            keywords: kws,
                        },
                        duration,
                    ));
                }
                return Ok(out);
            }
            let keep_existing_colors = ts.eat_any_phrase(&[
                &["in", "addition", "to", "its", "other", "colors"],
                &["in", "addition", "to", "their", "other", "colors"],
            ]);
            if subtypes.is_empty()
                && add_types.is_empty()
                && base_pt.is_none()
                && !colors.is_empty()
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut out = vec![mutate_obj(
                    subject.clone(),
                    ObjectMutation::BecomeColors {
                        colors,
                        keep_existing_colors: keep_existing_colors || keep_existing_types,
                    },
                    duration.clone(),
                )];
                if ts.eat_phrase(&["and", "attacks", "this", "turn", "if", "able"]) {
                    out.push(mutate_obj(
                        subject,
                        ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                        Duration::UntilEndOfTurn,
                    ));
                }
                return Ok(out);
            }
            if ts.eat_any_phrase(&[
                &[
                    "and",
                    "has",
                    "base",
                    "power",
                    "and",
                    "base",
                    "toughness",
                    "each",
                    "equal",
                    "to",
                ],
                &[
                    "and",
                    "has",
                    "base",
                    "power",
                    "and",
                    "toughness",
                    "each",
                    "equal",
                    "to",
                ],
                &[
                    "with",
                    "base",
                    "power",
                    "and",
                    "toughness",
                    "each",
                    "equal",
                    "to",
                ],
                &["with", "power", "and", "toughness", "each", "equal", "to"],
            ]) {
                let val = parse_value_expr(ts)?;
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::SetDynamicBasePT {
                        power: val.clone(),
                        toughness: val,
                        add_types,
                        keep_existing_types,
                    },
                    duration,
                )]);
            }
            let mut keywords = Vec::new();
            if ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                && let Some(pt) = ts.next_base_pt()
            {
                base_pt = Some(pt);
            }
            let mut extra_ability = None;
            let mut gained_all_creature_types = false;
            if ts.eat_word("with") {
                if ts.eat_phrase(&["all", "creature", "types"]) {
                    gained_all_creature_types = true;
                } else if let Some(quoted) = ts.next_quoted() {
                    if let Ok(compiled) = compile_quoted_permanent(ts, quoted) {
                        extra_ability = Some(compiled);
                    }
                } else if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                    keywords = kws;
                    if ts.eat_phrase(&["and", "all", "creature", "types"]) {
                        gained_all_creature_types = true;
                    } else if ts.eat_word("and")
                        && let Some(quoted) = ts.next_quoted()
                        && let Ok(compiled) = compile_quoted_permanent(ts, quoted)
                    {
                        extra_ability = Some(compiled);
                    }
                }
            }
            if ts.eat_any_phrase(&[&["and", "gains"], &["and", "gain"]])
                && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            {
                keywords.extend(kws);
            }
            let mut retained_type = None;
            if ts.eat_any_phrase(&[
                &["that's", "still", "a"],
                &["that's", "still", "an"],
                &["that", "are", "still"],
            ]) && let Some(w) = ts.peek_word()
                && let Some(ct) = lookup_card_type(&w.to_ascii_lowercase())
            {
                ts.next_word();
                retained_type = Some(ct);
            }
            if !subtypes.is_empty() || !add_types.is_empty() || base_pt.is_some() {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                if ts.eat_in_addition_to_other_types() {
                    keep_existing_types = true;
                }
                let _ = ts.eat_any_phrase(&[
                    &["that's", "no", "longer", "an", "equipment"],
                    &["that's", "no", "longer", "a", "creature"],
                ]);
                let duration = parse_trailing_duration(ts).unwrap_or(duration);
                let mut loses_abilities = ts.eat_any_phrase(&[
                    &["and", "has", "no", "abilities"],
                    &["and", "loses", "all", "abilities"],
                    &["and", "loses", "all", "other", "abilities"],
                    &["and", "lose", "all", "other", "abilities"],
                ]);
                if !loses_abilities {
                    let lcp = ts.checkpoint();
                    if ts.eat_comma() && ts.eat_phrase(&["loses", "all", "abilities"]) {
                        loses_abilities = true;
                        let _ = ts.eat_comma();
                    } else {
                        ts.restore(lcp);
                    }
                }
                let mut out = vec![mutate_obj(
                    subject.clone(),
                    ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types,
                        base_pt,
                        keywords,
                    },
                    duration.clone(),
                )];
                if loses_abilities {
                    out.push(mutate_obj(
                        subject.clone(),
                        ObjectMutation::RemoveAllAbilities,
                        duration.clone(),
                    ));
                }
                if let Some(ab) = extra_ability {
                    out.push(mutate_obj(
                        subject.clone(),
                        ObjectMutation::GrantAbility(Box::new(ab)),
                        duration.clone(),
                    ));
                }
                if let Some(ct) = retained_type {
                    out.push(mutate_obj(
                        subject.clone(),
                        ObjectMutation::RetainTypes(ct),
                        duration.clone(),
                    ));
                }
                if gained_all_creature_types {
                    out.push(apply_continuous(
                        StaticAbility::SubjectRule {
                            subject: subject.clone(),
                            rule: SubjectStaticRule::AllCreatureTypes,
                            condition: None,
                        },
                        duration.clone(),
                    ));
                }
                {
                    let bcp = ts.checkpoint();
                    if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
                        let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlocked),
                            bdur,
                        ));
                    } else {
                        ts.restore(bcp);
                    }
                }
                {
                    let gcp = ts.checkpoint();
                    if ts.eat_any_phrase(&[&["and", "gains"], &["and", "gain"]])
                        && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                    {
                        let gdur = parse_trailing_duration(ts).unwrap_or(duration);
                        out.push(mutate_obj(
                            subject,
                            ObjectMutation::GrantKeywords(kws),
                            gdur,
                        ));
                    } else {
                        ts.restore(gcp);
                    }
                }
                return Ok(out);
            }
        }
        ts.restore(becomes_cp);
    }

    // 5a. `<subj> becomes prepared`
    if ts.eat_any_phrase(&[&["becomes", "prepared"], &["become", "prepared"]]) {
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::StateAction(ObjectStateAction::Prepare),
            Duration::Permanent,
        )]);
    }

    // 5b. `<subj> assigns no combat damage this turn`
    if ts.eat_any_phrase(&[
        &["assigns", "no", "combat", "damage"],
        &["assign", "no", "combat", "damage"],
    ]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::StateAction(ObjectStateAction::AssignsNoCombatDamage),
            duration,
        )]);
    }

    // 5c. `<subj> can attack [this turn] as though it/they didn't have defender`
    if ts.eat_any_phrase(&[
        &[
            "can", "attack", "this", "turn", "as", "though", "they", "didn't", "have", "defender",
        ],
        &[
            "can", "attack", "this", "turn", "as", "though", "it", "didn't", "have", "defender",
        ],
        &[
            "can", "attack", "as", "though", "they", "didn't", "have", "defender",
        ],
        &[
            "can", "attack", "as", "though", "it", "didn't", "have", "defender",
        ],
    ]) {
        return Ok(vec![apply_continuous(
            StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition: None,
            },
            Duration::UntilEndOfTurn,
        )]);
    }

    // 5c2. `<subj> can block an additional creature / <N> additional creatures [dur]`
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["can", "block"]) {
            let extra = if ts.eat_phrase(&["an", "additional", "creature"]) {
                Some(1u8)
            } else if let Some(n) = ts.next_number(false).and_then(|v| u8::try_from(v).ok())
                && ts.eat_any_phrase(&[&["additional", "creatures"], &["additional", "creature"]])
            {
                Some(n)
            } else {
                None
            };
            if let Some(n) = extra {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::CanBlockAdditional(n),
                    duration,
                )]);
            }
        }
        ts.restore(bcp);
    }
    // 5c3. `<subj> [this turn] cost(s) {M} less/more to cast [for each <expr>] [if <cond>]`
    {
        let ccp = ts.checkpoint();
        let _ = ts.eat_phrase(&["this", "turn"]);
        if ts.eat_any_word(&["cost", "costs"])
            && let Some(delta) = ts.eat_mana_cost()
        {
            let less_opt = if ts.eat_phrase(&["less", "to", "cast"]) {
                Some(true)
            } else if ts.eat_phrase(&["more", "to", "cast"]) {
                Some(false)
            } else {
                None
            };
            if let Some(less) = less_opt {
                let scale = opt(ts, parse_for_each_expr);
                let cond = if ts.eat_word("if") {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let flt = match subject {
                    SubjectRef::Group(f) | SubjectRef::All(f) | SubjectRef::Each(f) => *f,
                    _ => crate::v2::ir::ObjectFilter {
                        any_spell: true,
                        ..Default::default()
                    },
                };
                return Ok(vec![apply_continuous(
                    StaticAbility::CostModifier {
                        subject: flt,
                        delta,
                        less,
                        scale,
                        min_mana: None,
                        condition: cond,
                    },
                    duration,
                )]);
            }
        }
        ts.restore(ccp);
    }

    // 5d. `<subj> attacks [<player>] this turn/combat if able` / `<subj> blocks [<tgt>] this turn if able` / `<subj> must be blocked this turn if able`
    if ts.eat_any_phrase(&[
        &["attacks", "this", "turn", "if", "able"],
        &["attack", "this", "turn", "if", "able"],
        &["attacks", "this", "combat", "if", "able"],
        &["attack", "this", "combat", "if", "able"],
    ]) {
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
            Duration::UntilEndOfTurn,
        )]);
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_any_word(&["attacks", "attack"])
            && try_parse(ts, parse_subject_ref).is_ok()
            && ts.eat_any_phrase(&[
                &["this", "combat", "if", "able"],
                &["this", "turn", "if", "able"],
            ])
        {
            return Ok(vec![mutate_obj(
                subject,
                ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                Duration::UntilEndOfCombat,
            )]);
        }
        ts.restore(acp);
    }
    if ts.eat_phrase(&["can", "block", "any", "number", "of", "creatures"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![apply_continuous(
            StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CanBlockAnyNumber,
                condition: None,
            },
            duration,
        )]);
    }
    if ts.eat_phrase(&["must", "be", "blocked", "this", "turn", "if", "able"]) {
        return Ok(vec![apply_continuous(
            StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::MustBeBlocked,
                condition: None,
            },
            Duration::UntilEndOfTurn,
        )]);
    }
    {
        let bcp = ts.checkpoint();
        if ts.eat_any_word(&["blocks", "block"]) {
            let blocked_tgt = if ts.peek_phrase(&["this", "turn", "if", "able"])
                || ts.peek_phrase(&["this", "combat", "if", "able"])
            {
                None
            } else {
                opt(ts, parse_subject_ref).map(Box::new)
            };
            if ts.eat_any_phrase(&[
                &["this", "turn", "if", "able"],
                &["this", "combat", "if", "able"],
            ]) {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::MustBlockThisTurn(blocked_tgt),
                    Duration::UntilEndOfTurn,
                )]);
            }
        }
        ts.restore(bcp);
    }

    // 6. `<subj> is [also] goaded [duration]` OR `<subj> is a/an [<color>] <Subtypes> [in addition to its other [colors and] types] [dur]`
    {
        let icp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["is", "a"], &["is", "an"]])
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("copy"))
        {
            let (add_types, colors, subtypes) = eat_type_color_subtype_run(ts, true, false, false);
            if !subtypes.is_empty() || !add_types.is_empty() {
                let keep_existing_types = ts.eat_in_addition_to_other_types();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                let mut out = Vec::new();
                if !colors.is_empty() {
                    out.push(mutate_obj(
                        subject.clone(),
                        ObjectMutation::BecomeColors {
                            colors,
                            keep_existing_colors: keep_existing_types,
                        },
                        duration.clone(),
                    ));
                }
                out.push(mutate_obj(
                    subject,
                    ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types,
                        base_pt: None,
                        keywords: Vec::new(),
                    },
                    duration,
                ));
                return Ok(out);
            }
        }
        ts.restore(icp);
    }
    if ts.eat_any_phrase(&[
        &["is", "also", "goaded"],
        &["are", "also", "goaded"],
        &["is", "goaded"],
        &["are", "goaded"],
    ]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::StateAction(ObjectStateAction::Goad),
            duration,
        )]);
    }

    // 7. `<subj> can't ... [duration]` (delegated to `subject_combat.rs`)
    if ts.eat_word("can't") {
        return parse_subject_cant(ts, subject);
    }

    // 8. `<subj> doesn't untap during its controller's [next] untap step [for as long as <cond>]`
    {
        let ucp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["doesn't", "untap", "during"],
            &["don't", "untap", "during"],
        ]) && ts.eat_any_phrase(&[
            &["its", "controller's", "next", "untap", "step"],
            &["their", "controller's", "next", "untap", "step"],
            &["their", "controllers'", "next", "untap", "steps"],
            &["that", "player's", "next", "untap", "step"],
            &["your", "next", "untap", "step"],
            &["its", "controller's", "untap", "step"],
            &["their", "controller's", "untap", "step"],
            &["their", "controllers'", "untap", "steps"],
            &["that", "player's", "untap", "step"],
            &["your", "untap", "step"],
        ]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
            return Ok(vec![mutate_obj(
                subject,
                ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                duration,
            )]);
        }
        ts.restore(ucp);
    }

    // 9. `<subj> loses/lose [all abilities [and becomes ...] | [your choice of] <kws>] [duration]`
    {
        let loses_cp = ts.checkpoint();
        if ts.eat_any_word(&["loses", "lose"]) {
            if ts.eat_phrase(&["all", "creature", "types"]) {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeCreatureTypes {
                        subtypes: Vec::new(),
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: false,
                        base_pt: None,
                        keywords: Vec::new(),
                    },
                    duration,
                )]);
            }
            if ts.eat_phrase(&["all", "abilities"]) {
                let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut out = vec![mutate_obj(
                    subject.clone(),
                    ObjectMutation::RemoveAllAbilities,
                    duration.clone(),
                )];
                {
                    let ptcp = ts.checkpoint();
                    if ts.eat_any_phrase(&[
                        &["and", "has", "base", "power", "and", "toughness"],
                        &["and", "have", "base", "power", "and", "toughness"],
                    ]) && let Some((power, toughness)) = ts.next_base_pt()
                    {
                        let pdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        duration = pdur.clone();
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::SetDynamicBasePT {
                                power: ValueExpr::Const(power),
                                toughness: ValueExpr::Const(toughness),
                                add_types: CardTypeMask::EMPTY,
                                keep_existing_types: true,
                            },
                            pdur,
                        ));
                    } else {
                        ts.restore(ptcp);
                    }
                }
                {
                    let bcp = ts.checkpoint();
                    if ts.eat_any_phrase(&[&["and", "becomes"], &["and", "become"]]) {
                        let _ = ts.eat_any_word(&["a", "an"]);
                        let mut base_pt = ts.next_base_pt();
                        let (add_types, colors, subtypes) =
                            eat_type_color_subtype_run(ts, true, false, true);
                        if base_pt.is_none()
                            && ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                        {
                            base_pt = ts.next_base_pt();
                        }
                        let keep_existing_types = ts.eat_in_addition_to_other_types();
                        if !add_types.is_empty()
                            || !subtypes.is_empty()
                            || !colors.is_empty()
                            || base_pt.is_some()
                        {
                            let bdur =
                                parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                            if bdur != duration {
                                duration = bdur.clone();
                                out[0] = mutate_obj(
                                    subject.clone(),
                                    ObjectMutation::RemoveAllAbilities,
                                    duration.clone(),
                                );
                            }
                            if !colors.is_empty() {
                                out.push(mutate_obj(
                                    subject.clone(),
                                    ObjectMutation::BecomeColors {
                                        colors,
                                        keep_existing_colors: false,
                                    },
                                    bdur.clone(),
                                ));
                            }
                            out.push(mutate_obj(
                                subject,
                                ObjectMutation::BecomeCreatureTypes {
                                    subtypes,
                                    add_types,
                                    keep_existing_types,
                                    base_pt,
                                    keywords: Vec::new(),
                                },
                                bdur,
                            ));
                        } else {
                            ts.restore(bcp);
                        }
                    } else {
                        ts.restore(bcp);
                    }
                }
                return Ok(out);
            }
            let _ = ts.eat_phrase(&["your", "choice", "of"]);
            if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut out = vec![mutate_obj(
                    subject.clone(),
                    ObjectMutation::RemoveKeywords(kws),
                    duration.clone(),
                )];
                let gcp = ts.checkpoint();
                if ts.eat_any_phrase(&[&["and", "gains"], &["and", "gain"]])
                    && let Ok((gkws, _)) = try_parse(ts, parse_keyword_list)
                {
                    let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                    if gdur != duration {
                        duration = gdur.clone();
                        if let Effect::MutateObject { duration: d, .. } = &mut out[0] {
                            *d = duration.clone();
                        }
                    }
                    out.push(mutate_obj(
                        subject,
                        ObjectMutation::GrantKeywords(gkws),
                        gdur,
                    ));
                } else {
                    ts.restore(gcp);
                }
                return Ok(out);
            }
        }
        ts.restore(loses_cp);
    }

    // 10. `<subj> phases out [until ...]` / `<subj> fights <tgt>` / `<subj> fight each other` / `<subj> explores` / `<subj> connives`
    if ts.eat_any_phrase(&[&["phases", "out"], &["phase", "out"]]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::StateAction(ObjectStateAction::PhaseOut),
            duration,
        )]);
    }
    if ts.eat_any_word(&["fights", "fight"]) {
        let target = if ts.eat_phrase(&["each", "other"]) {
            SubjectRef::Anaphora(AnaphoraRef::Them)
        } else {
            parse_subject_ref(ts)?
        };
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::Fight(Box::new(target)),
            Duration::Instant,
        )]);
    }
    if ts.eat_any_word(&["endures", "endure"]) {
        let amount = parse_value_expr(ts)?;
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Endure,
                amount,
            },
            Duration::Instant,
        )]);
    }
    if ts.eat_any_word(&["explores", "explore"]) {
        let _ = ts.eat_word("again");
        let ecp = ts.checkpoint();
        let amount = if let Ok(times) = try_parse(ts, parse_value_expr)
            && ts.eat_word("times")
        {
            times
        } else {
            ts.restore(ecp);
            ValueExpr::Const(1)
        };
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Explore,
                amount,
            },
            Duration::Instant,
        )]);
    }
    if ts.eat_any_word(&["connives", "connive"]) {
        let amount = opt(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1));
        return Ok(vec![mutate_player(
            PlayerRef::You,
            PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Connive,
                amount,
            },
            Duration::Instant,
        )]);
    }

    if ts.eat_any_word(&["targets", "target"]) {
        let new_tgt = parse_subject_ref(ts)?;
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::RetargetTo(Box::new(new_tgt)),
            Duration::Instant,
        )]);
    }

    super::subject_combat::parse_extra_subject_predicates(ts, subject)
}

fn append_trailing_subject_riders(
    ts: &mut TokenStream<'_>,
    subject: &SubjectRef,
    duration: &Duration,
    out: &mut Vec<Effect>,
) {
    if ts.eat_any_phrase(&[
        &[
            "and", "can", "attack", "this", "turn", "as", "though", "it", "didn't", "have",
            "defender",
        ],
        &[
            "and", "can", "attack", "this", "turn", "as", "though", "they", "didn't", "have",
            "defender",
        ],
        &[
            "and", "can", "attack", "as", "though", "it", "didn't", "have", "defender",
        ],
        &[
            "and", "can", "attack", "as", "though", "they", "didn't", "have", "defender",
        ],
    ]) {
        out.push(apply_continuous(
            StaticAbility::SubjectRule {
                subject: subject.clone(),
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition: None,
            },
            Duration::UntilEndOfTurn,
        ));
    }
    if ts.eat_any_phrase(&[
        &["and", "doesn't", "untap", "during"],
        &["and", "don't", "untap", "during"],
    ]) && ts.eat_any_phrase(DONT_UNTAP_STEPS)
    {
        out.push(mutate_obj(
            subject.clone(),
            ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
            Duration::UntilYourNextTurn,
        ));
    }
    if ts.eat_phrase(&["and", "must", "be", "blocked", "this", "turn", "if", "able"]) {
        out.push(apply_continuous(
            StaticAbility::RuleRestriction {
                subject: Some(subject.clone()),
                rule: RuleRestrictionKind::MustBeBlocked,
                condition: None,
            },
            Duration::UntilEndOfTurn,
        ));
    }
    let gcp = ts.checkpoint();
    if ts.eat_any_phrase(&[&["and", "gains"], &["and", "gain"]])
        && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
    {
        let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
        out.push(mutate_obj(
            subject.clone(),
            ObjectMutation::GrantKeywords(kws),
            gdur,
        ));
    } else {
        ts.restore(gcp);
    }
    let bcp = ts.checkpoint();
    if ts.eat_any_phrase(&[&["and", "becomes"], &["and", "become"]]) {
        let _ = ts.eat_any_word(&["a", "an"]);
        let (add_types, _, subtypes) = eat_type_color_subtype_run(ts, false, false, false);
        if !add_types.is_empty() || !subtypes.is_empty() {
            let keep_existing_types = ts.eat_in_addition_to_other_types();
            let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
            out.push(mutate_obj(
                subject.clone(),
                ObjectMutation::BecomeCreatureTypes {
                    subtypes,
                    add_types,
                    keep_existing_types,
                    base_pt: None,
                    keywords: Vec::new(),
                },
                bdur,
            ));
        } else {
            ts.restore(bcp);
        }
    } else {
        ts.restore(bcp);
    }
    let dcp = ts.checkpoint();
    if ts.eat_phrase(&["and", "deals"])
        && let Ok(dmg) = try_parse(ts, parse_value_expr)
        && ts.eat_phrase(&["damage", "to"])
        && let Ok(tgt) = try_parse(ts, parse_subject_ref)
    {
        out.push(Effect::DealDamage {
            source: subject.clone(),
            target: tgt,
            amount: dmg,
            divided_by: None,
        });
    } else {
        ts.restore(dcp);
    }
}
