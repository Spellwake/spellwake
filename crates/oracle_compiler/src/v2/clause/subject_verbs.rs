//! Subject-initiated verb frames and trailing duration clauses for `v2`.

use crate::FaceInput;
use crate::ast::CardTypeMask;
use crate::v2::card::compile_face_v2;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{
    CharacteristicKind, ChoiceKind, Duration, Effect, ObjectKeywordAction, ObjectMutation,
    ObjectStateAction, PlayerMutation, PlayerRef, Replacement, RuleRestrictionKind,
    ScaledPlayerAction, StaticAbility, SubjectRef, SubjectStaticRule, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};
use crate::vocab::{lookup_card_type, lookup_color};

pub(super) use super::subject_combat::parse_trailing_duration;
use super::subject_combat::{parse_damage_target, parse_subject_cant};

/// Subject-headed verb frames (`<src> deals ...`, `<subj> gets ...`, `<subj> gains ...`,
/// `<subj> becomes ...`, `<subj> is goaded ...`, `<subj> can't ...`).
pub(super) fn parse_subject_verb_frame(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    // `It's a/an [<P>/<T>] [<colors>] <Subtypes/Types> [with base power and toughness P/T] [in addition to its other [colors and] types] [dur]`
    // (`Eluge, the Shoreless Sea`, `Cyber Conversion`, `Necromantic Selection`, `Sauron, Dino Devotee`, `Draconic Destiny`)
    {
        let its_cp = ts.checkpoint();
        if (ts.eat_phrase(&["it's", "a"]) || ts.eat_phrase(&["it's", "an"]))
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("copy"))
        {
            let mut base_pt = ts.next_base_pt();
            let mut add_types = CardTypeMask::EMPTY;
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(ct) = lookup_card_type(&low) {
                    ts.next_word();
                    add_types = add_types.union(ct);
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else if let Some(c) = lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else if low == "and"
                    && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if {
                            let nl = nw.to_ascii_lowercase();
                            lookup_color(&nl).is_some()
                                || lookup_card_type(&nl).is_some()
                                || ts.vocab.lookup_subtype(&nl).is_some()
                        })
                    })
                {
                    ts.next_word();
                } else {
                    break;
                }
            }
            if base_pt.is_none() && ts.eat_phrase(&["with", "base", "power", "and", "toughness"]) {
                base_pt = ts.next_base_pt();
            }
            if !subtypes.is_empty() || !add_types.is_empty() || base_pt.is_some() {
                let keep_existing_types = ts.eat_phrase(&[
                    "in", "addition", "to", "its", "other", "colors", "and", "types",
                ]) || ts
                    .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"]);
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                if ts.is_eof() {
                    let subj = SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It);
                    let mut out = Vec::new();
                    if !colors.is_empty() {
                        out.push(Effect::MutateObject {
                            subject: subj.clone(),
                            mutation: ObjectMutation::BecomeColors {
                                colors,
                                keep_existing_colors: keep_existing_types,
                            },
                            duration: duration.clone(),
                        });
                    }
                    out.push(Effect::MutateObject {
                        subject: subj,
                        mutation: ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types,
                            base_pt,
                            keywords: Vec::new(),
                        },
                        duration,
                    });
                    return Ok(out);
                }
            }
        }
        ts.restore(its_cp);
    }
    if ts.eat_phrase(&["damage", "can't", "be", "prevented"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::RuleRestriction {
                subject: None,
                rule: RuleRestrictionKind::DamageCantBePrevented,
                condition: None,
            }),
            duration,
        }]);
    }
    let subject = parse_subject_ref(ts)?;
    let mut out = parse_subject_predicate(ts, subject.clone())?;
    // Compositional `, <pred2>, and <pred3>` / `and <pred2>` chaining on the same subject
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
            out.extend(more);
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
            {
                let rcp = ts.checkpoint();
                if ts.eat_comma()
                    && (ts.eat_phrase(&["rounded", "down"]) || ts.eat_phrase(&["rounded", "up"]))
                {
                    let _ = ts.eat_comma();
                } else {
                    ts.restore(rcp);
                }
            }
            if ts.eat_phrase(&["divided", "evenly"]) {
                let rcp = ts.checkpoint();
                if ts.eat_comma()
                    && (ts.eat_phrase(&["rounded", "down"]) || ts.eat_phrase(&["rounded", "up"]))
                {
                    let _ = ts.eat_comma();
                } else {
                    ts.restore(rcp);
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
                if !(ts.eat_word("chooses") || ts.eat_word("choose")) || !ts.eat_word("among") {
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
                // Replacement damage amount without repeated `to <tgt>` (`If that land is a Mountain, ~ deals 2 damage instead.`)
                out.push(Effect::DealDamage {
                    source: subject.clone(),
                    target: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::SameTarget),
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
            if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
                || ts.eat_phrase(&["and", "don't", "untap", "during"]))
                && (ts.eat_phrase(&["your", "next", "untap", "step"])
                    || ts.eat_phrase(&["its", "controller's", "next", "untap", "step"])
                    || ts.eat_phrase(&["their", "controllers'", "next", "untap", "steps"]))
            {
                out.push(Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                    duration: Duration::UntilYourNextTurn,
                });
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
    if ts.eat_word("gets") || ts.eat_word("get") {
        let _ = ts.eat_phrase(&["an", "additional"]);
        let is_twice = ts.eat_word("twice");
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        if ts.eat_word("or")
            && let Some((p2, t2)) = ts.next_pt_mod()
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::PromptChoice {
                chooser: PlayerRef::You,
                kind: ChoiceKind::ChooseBranch(vec![
                    vec![Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::ModifyPT {
                            power,
                            toughness,
                            scale: None,
                        },
                        duration: duration.clone(),
                    }],
                    vec![Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::ModifyPT {
                            power: p2,
                            toughness: t2,
                            scale: None,
                        },
                        duration,
                    }],
                ]),
            }]);
        }
        let mut gained_kws = None;
        let mut gained_ability = None;
        let mut gained_all_creature_types = false;
        if ts.eat_phrase(&["and", "gains"])
            || ts.eat_phrase(&["and", "gain"])
            || ts.eat_phrase(&["and", "has"])
            || ts.eat_phrase(&["and", "have"])
        {
            if ts.eat_phrase(&["all", "creature", "types"]) {
                gained_all_creature_types = true;
            } else if let Some(quoted) = ts.next_quoted() {
                let compiled = compile_face_v2(
                    FaceInput {
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
                        .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
                }
                gained_ability = Some(compiled);
            } else {
                let _ = ts.eat_phrase(&["your", "choice", "of"]);
                let (kws, _) = parse_keyword_list(ts)?;
                gained_kws = Some(kws);
                let qcp = ts.checkpoint();
                let _ = ts.eat_comma();
                if ts.eat_word("and")
                    && let Some(quoted) = ts.next_quoted()
                {
                    let compiled = compile_face_v2(
                        FaceInput {
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
                            .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
                    }
                    gained_ability = Some(compiled);
                } else {
                    ts.restore(qcp);
                }
            }
        }
        let mut lost_kws = None;
        let mut lost_all_abilities = false;
        if ts.eat_phrase(&["and", "loses"]) || ts.eat_phrase(&["and", "lose"]) {
            if ts.eat_phrase(&["all", "abilities"]) {
                lost_all_abilities = true;
            } else {
                let (kws, _) = parse_keyword_list(ts)?;
                lost_kws = Some(kws);
            }
        }
        let mut cant_mut = None;
        if ts.eat_phrase(&["and", "can't", "be", "blocked", "except", "by"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)),
            ));
        } else if ts.eat_phrase(&["and", "can't", "be", "blocked", "by"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)),
            ));
        } else if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlocked,
            ));
        } else if ts.eat_phrase(&["and", "can't", "block"]) {
            cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock));
        } else if ts.eat_phrase(&["and", "can't", "attack"]) {
            cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack));
        } else if ts.eat_phrase(&[
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
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
            ));
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        // Support trailing `and can't be blocked [except by <flt>] [this turn]` / `and gains <kws> [dur]` AFTER duration!
        if cant_mut.is_none() {
            if ts.eat_phrase(&["and", "can't", "be", "blocked", "except", "by"])
                && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
            {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(
                    RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)),
                ));
            } else if ts.eat_phrase(&["and", "can't", "be", "blocked", "by"])
                && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
            {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(
                    RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)),
                ));
            } else if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(
                    RuleRestrictionKind::CantBeBlocked,
                ));
            } else if ts.eat_phrase(&["and", "can't", "block"]) {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock));
            } else if ts.eat_phrase(&["and", "can't", "attack"]) {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack));
            } else if ts.eat_phrase(&["and", "can", "block", "an", "additional"]) {
                let extra = ts
                    .next_number(false)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_word("creature") || ts.eat_word("creatures");
                let _ = ts.eat_phrase(&["each", "combat"]);
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::CanBlockAdditional(extra));
            }
        }
        if gained_kws.is_none()
            && (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
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
        let mut out = vec![Effect::MutateObject {
            subject: subject.clone(),
            mutation: ObjectMutation::ModifyPT {
                power,
                toughness,
                scale,
            },
            duration: duration.clone(),
        }];
        if gained_all_creature_types {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::AllCreatureTypes,
                    condition: None,
                }),
                duration: duration.clone(),
            });
        }
        if let Some(kws) = gained_kws {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::GrantKeywords(kws),
                duration: duration.clone(),
            });
        }
        if let Some(ab) = gained_ability {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::GrantAbility(Box::new(ab)),
                duration: duration.clone(),
            });
        }
        if lost_all_abilities {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::RemoveAllAbilities,
                duration: duration.clone(),
            });
        }
        if let Some(kws) = lost_kws {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::RemoveKeywords(kws),
                duration: duration.clone(),
            });
        }
        if let Some(cm) = cant_mut {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: cm,
                duration: duration.clone(),
            });
        }
        // Optional trailing `and can attack this turn as though it/they didn't have defender` (`Mobile Fort`)
        if ts.eat_phrase(&[
            "and", "can", "attack", "this", "turn", "as", "though", "it", "didn't", "have",
            "defender",
        ]) || ts.eat_phrase(&[
            "and", "can", "attack", "this", "turn", "as", "though", "they", "didn't", "have",
            "defender",
        ]) {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::CanAttackWithDefender,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            });
        }
        // Optional trailing `and doesn't/don't untap during your/its controller's next untap step` (`Homarid Warrior`)
        if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
            || ts.eat_phrase(&["and", "don't", "untap", "during"]))
            && (ts.eat_phrase(&["your", "next", "untap", "step"])
                || ts.eat_phrase(&["its", "controller's", "next", "untap", "step"])
                || ts.eat_phrase(&["their", "controllers'", "next", "untap", "steps"]))
        {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                duration: Duration::UntilYourNextTurn,
            });
        }
        // Optional trailing `and must be blocked this turn if able` / `and attacks this turn if able`
        if ts.eat_phrase(&["and", "must", "be", "blocked", "this", "turn", "if", "able"]) {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::MustBeBlocked,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            });
        }
        // Optional trailing `and gains <kws> [until end of turn]` after duration (`Chorus of Might`, `Battlegate Mimic`, `Flayer of Loyalties`)
        {
            let gcp = ts.checkpoint();
            if (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
                && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            {
                let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                out.push(Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::GrantKeywords(kws),
                    duration: gdur,
                });
            } else {
                ts.restore(gcp);
            }
        }
        // Optional trailing `and becomes a/an <sub/type> [until end of turn]` (`Wishful Merfolk` + 10+ cards)
        {
            let bcp = ts.checkpoint();
            if (ts.eat_phrase(&["and", "becomes"]) || ts.eat_phrase(&["and", "become"])) && {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                true
            } {
                let mut add_types = CardTypeMask::EMPTY;
                let mut subtypes = Vec::new();
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if let Some(ct) = lookup_card_type(&low) {
                        ts.next_word();
                        add_types = add_types.union(ct);
                    } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                        ts.next_word();
                        subtypes.push(sub.to_string());
                    } else {
                        break;
                    }
                }
                if !add_types.is_empty() || !subtypes.is_empty() {
                    let keep_existing_types = ts.eat_phrase(&[
                        "in", "addition", "to", "its", "other", "colors", "and", "types",
                    ]) || ts.eat_phrase(&[
                        "in", "addition", "to", "their", "other", "colors", "and", "types",
                    ]) || ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                        || ts.eat_phrase(&[
                            "in", "addition", "to", "their", "other", "creature", "types",
                        ])
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                    let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types,
                            base_pt: None,
                            keywords: Vec::new(),
                        },
                        duration: bdur,
                    });
                } else {
                    ts.restore(bcp);
                }
            } else {
                ts.restore(bcp);
            }
        }
        // Optional trailing `and deals <amt> damage to <tgt>` (`Wormwood Dryad`)
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["and", "deals"])
                && let Ok(dmg) = try_parse(ts, parse_value_expr)
                && ts.eat_phrase(&["damage", "to"])
                && let Ok(tgt) = try_parse(ts, parse_subject_ref)
            {
                out.push(Effect::DealDamage {
                    source: subject,
                    target: tgt,
                    amount: dmg,
                    divided_by: None,
                });
            } else {
                ts.restore(dcp);
            }
        }
        return Ok(out);
    }

    // 3. `<subj> gains/gain "<quoted ability>" [and "<q2>"] [duration]` OR `<subj> gains/gain [your choice of] <kws> [and gets +P/+T] [duration]`
    //    CRITICAL RULE: Unqualified `gains <kw>` defaults to `Duration::Permanent`!
    if ts.eat_word("gains") || ts.eat_word("gain") {
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_face_v2(
                FaceInput {
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
                    .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
            }
            let mut granted = vec![compiled];
            while ts.eat_word("and")
                && let Some(q2) = ts.next_quoted()
            {
                let c2 = compile_face_v2(
                    FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: q2,
                    },
                    ts.vocab,
                );
                if !c2.is_ok() {
                    return Err(c2
                        .errors
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
                }
                granted.push(c2);
            }
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(granted
                .into_iter()
                .map(|ab| Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::GrantAbility(Box::new(ab)),
                    duration: duration.clone(),
                })
                .collect());
        }
        if ts.eat_phrase(&["all", "creature", "types"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::AllCreatureTypes,
                    condition: None,
                }),
                duration,
            }]);
        }
        if ts.eat_phrase(&["all", "activated", "abilities", "of"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::CopyActivatedAbilities(flt),
                    condition: None,
                }),
                duration,
            }]);
        }
        let is_choice = ts.eat_phrase(&["your", "choice", "of"]);
        let (kws, _) = parse_keyword_list(ts)?;
        let mut extra_pt = None;
        let mut extra_pt_scale = None;
        if (ts.eat_phrase(&["and", "gets"]) || ts.eat_phrase(&["and", "get"]))
            && let Some((p, t)) = ts.next_pt_mod()
        {
            extra_pt = Some((p, t));
            extra_pt_scale = opt(ts, parse_for_each_expr).map(Box::new);
        }
        let mut extra_ability = None;
        {
            let qcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and")
                && let Some(quoted) = ts.next_quoted()
            {
                let compiled = compile_face_v2(
                    FaceInput {
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
                        .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
                }
                extra_ability = Some(compiled);
            } else {
                ts.restore(qcp);
            }
        }
        let mut cant_mut = None;
        if ts.eat_phrase(&["and", "can't", "be", "blocked", "except", "by"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)),
            ));
        } else if ts.eat_phrase(&["and", "can't", "be", "blocked", "by"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)),
            ));
        } else if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::CantBeBlocked,
            ));
        } else if ts.eat_phrase(&["and", "can't", "block"]) {
            cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock));
        } else if ts.eat_phrase(&["and", "can't", "attack"]) {
            cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack));
        } else if ts.eat_phrase(&[
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
            cant_mut = Some(ObjectMutation::ApplyRule(
                RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
            ));
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        if cant_mut.is_none() {
            if ts.eat_phrase(&["and", "can't", "be", "blocked", "except", "by"])
                && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
            {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(
                    RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)),
                ));
            } else if ts.eat_phrase(&["and", "can't", "be", "blocked", "by"])
                && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
            {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(
                    RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)),
                ));
            } else if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(
                    RuleRestrictionKind::CantBeBlocked,
                ));
            } else if ts.eat_phrase(&["and", "can't", "block"]) {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock));
            } else if ts.eat_phrase(&["and", "can't", "attack"]) {
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack));
            } else if ts.eat_phrase(&["and", "can", "block", "an", "additional"]) {
                let extra = ts
                    .next_number(false)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_word("creature") || ts.eat_word("creatures");
                let _ = ts.eat_phrase(&["each", "combat"]);
                let _ = parse_trailing_duration(ts);
                cant_mut = Some(ObjectMutation::CanBlockAdditional(extra));
            }
        }
        if is_choice {
            return Ok(vec![Effect::PromptChoice {
                chooser: PlayerRef::You,
                kind: ChoiceKind::ChooseKeywordAndGrant {
                    subject,
                    keywords: kws,
                    duration,
                },
            }]);
        }
        let mut out = vec![Effect::MutateObject {
            subject: subject.clone(),
            mutation: ObjectMutation::GrantKeywords(kws),
            duration: duration.clone(),
        }];
        if let Some((power, toughness)) = extra_pt {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::ModifyPT {
                    power,
                    toughness,
                    scale: extra_pt_scale,
                },
                duration: duration.clone(),
            });
        }
        if let Some(ab) = extra_ability {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::GrantAbility(Box::new(ab)),
                duration: duration.clone(),
            });
        }
        if let Some(cm) = cant_mut {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: cm,
                duration: duration.clone(),
            });
        }
        // Optional trailing `and can attack this turn as though it/they didn't have defender` (`Mobile Fort`)
        if ts.eat_phrase(&[
            "and", "can", "attack", "this", "turn", "as", "though", "it", "didn't", "have",
            "defender",
        ]) || ts.eat_phrase(&[
            "and", "can", "attack", "this", "turn", "as", "though", "they", "didn't", "have",
            "defender",
        ]) {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::SubjectRule {
                    subject: subject.clone(),
                    rule: SubjectStaticRule::CanAttackWithDefender,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            });
        }
        // Optional trailing `and doesn't/don't untap during your/its controller's next untap step` (`Homarid Warrior`)
        if (ts.eat_phrase(&["and", "doesn't", "untap", "during"])
            || ts.eat_phrase(&["and", "don't", "untap", "during"]))
            && (ts.eat_phrase(&["your", "next", "untap", "step"])
                || ts.eat_phrase(&["its", "controller's", "next", "untap", "step"])
                || ts.eat_phrase(&["their", "controllers'", "next", "untap", "steps"]))
        {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                duration: Duration::UntilYourNextTurn,
            });
        }
        // Optional trailing `and must be blocked this turn if able` / `and attacks this turn if able`
        if ts.eat_phrase(&["and", "must", "be", "blocked", "this", "turn", "if", "able"]) {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::RuleRestriction {
                    subject: Some(subject.clone()),
                    rule: RuleRestrictionKind::MustBeBlocked,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            });
        }
        // Optional trailing `and gains <kws> [until end of turn]` after duration (`Chorus of Might`, `Battlegate Mimic`, `Flayer of Loyalties`)
        {
            let gcp = ts.checkpoint();
            if (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
                && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            {
                let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                out.push(Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::GrantKeywords(kws),
                    duration: gdur,
                });
            } else {
                ts.restore(gcp);
            }
        }
        // Optional trailing `and becomes a/an <sub/type> [until end of turn]` (`Wishful Merfolk` + 10+ cards)
        {
            let bcp = ts.checkpoint();
            if (ts.eat_phrase(&["and", "becomes"]) || ts.eat_phrase(&["and", "become"])) && {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                true
            } {
                let mut add_types = CardTypeMask::EMPTY;
                let mut subtypes = Vec::new();
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if let Some(ct) = lookup_card_type(&low) {
                        ts.next_word();
                        add_types = add_types.union(ct);
                    } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                        ts.next_word();
                        subtypes.push(sub.to_string());
                    } else {
                        break;
                    }
                }
                if !add_types.is_empty() || !subtypes.is_empty() {
                    let keep_existing_types = ts.eat_phrase(&[
                        "in", "addition", "to", "its", "other", "colors", "and", "types",
                    ]) || ts.eat_phrase(&[
                        "in", "addition", "to", "their", "other", "colors", "and", "types",
                    ]) || ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                        || ts.eat_phrase(&[
                            "in", "addition", "to", "their", "other", "creature", "types",
                        ])
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                    let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types,
                            base_pt: None,
                            keywords: Vec::new(),
                        },
                        duration: bdur,
                    });
                } else {
                    ts.restore(bcp);
                }
            } else {
                ts.restore(bcp);
            }
        }
        // Optional trailing `and deals <amt> damage to <tgt>` (`Wormwood Dryad`)
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["and", "deals"])
                && let Ok(dmg) = try_parse(ts, parse_value_expr)
                && ts.eat_phrase(&["damage", "to"])
                && let Ok(tgt) = try_parse(ts, parse_subject_ref)
            {
                out.push(Effect::DealDamage {
                    source: subject,
                    target: tgt,
                    amount: dmg,
                    divided_by: None,
                });
            } else {
                ts.restore(dcp);
            }
        }
        return Ok(out);
    }

    // 4. `<subj> enters tapped [and attacking [that player/opponent]] [dur]` / `<subj> enters with <N> [additional] <kind> counters on it`
    if ts.eat_phrase(&["enters", "tapped"]) || ts.eat_phrase(&["enter", "tapped"]) {
        if ts.eat_phrase(&["and", "attacking"]) {
            let _ = ts.eat_phrase(&["that", "opponent"])
                || ts.eat_phrase(&["that", "player"])
                || ts.eat_phrase(&["the", "defending", "player"])
                || ts.eat_phrase(&["defending", "player"]);
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
        return Ok(vec![Effect::ApplyReplacement {
            replacement: Box::new(Replacement::EntersTapped { unless: None }),
            duration,
        }]);
    }
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let mut additional = false;
        let count = if ts.eat_phrase(&["an", "additional"]) {
            additional = true;
            ValueExpr::Const(1)
        } else if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        if ts.eat_word("additional") {
            additional = true;
        }
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counters") || ts.eat_word("counter"))
            && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
        {
            let rcp = ts.checkpoint();
            if ts.eat_comma()
                && (ts.eat_phrase(&["rounded", "down"]) || ts.eat_phrase(&["rounded", "up"]))
            {
                // Rounding modifier already captured on `ValueExpr::Div` when present
            } else {
                ts.restore(rcp);
            }
            return Ok(vec![Effect::ApplyReplacement {
                replacement: Box::new(Replacement::EntersWithCounters {
                    subject: Some(subject),
                    kind,
                    count,
                    additional,
                }),
                duration: Duration::UntilEndOfTurn,
            }]);
        }
    }

    // 4b. `<subj> has/have base power and toughness <P>/<T> [dur]` OR `<subj> has/have base power/toughness <N> [dur]` OR `<subj> has/have "<quoted ability>" [dur]` OR `<subj> has/have <kws> [dur]`
    if ts.eat_word("has") || ts.eat_word("have") {
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
                let mut out = vec![Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::SetDynamicBasePT {
                        power,
                        toughness,
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration: duration.clone(),
                }];
                loop {
                    let tcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if ts.eat_phrase(&["gains", "all", "creature", "types"])
                        || ts.eat_phrase(&["gain", "all", "creature", "types"])
                    {
                        out.push(Effect::ApplyContinuous {
                            effect: Box::new(StaticAbility::SubjectRule {
                                subject: subject.clone(),
                                rule: SubjectStaticRule::AllCreatureTypes,
                                condition: None,
                            }),
                            duration: duration.clone(),
                        });
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if (ts.eat_word("gains")
                        || ts.eat_word("gain")
                        || ts.eat_word("has")
                        || ts.eat_word("have"))
                        && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                    {
                        let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        if gdur != duration {
                            duration = gdur.clone();
                        }
                        out.push(Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::GrantKeywords(kws),
                            duration: gdur,
                        });
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if ts.eat_phrase(&["can't", "be", "blocked"]) {
                        let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        out.push(Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlocked),
                            duration: bdur,
                        });
                        continue;
                    }
                    ts.restore(tcp);
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                    if (ts.eat_phrase(&["isn't", "a"])
                        || ts.eat_phrase(&["isn't", "an"])
                        || ts.eat_word("aren't"))
                        && let Some(w) = ts.peek_word()
                        && ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
                    {
                        ts.next_word();
                        out.push(Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::BecomeCreatureTypes {
                                subtypes: Vec::new(),
                                add_types: CardTypeMask::EMPTY,
                                keep_existing_types: false,
                                base_pt: None,
                                keywords: Vec::new(),
                            },
                            duration: duration.clone(),
                        });
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
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::SetDynamicBasePT {
                        power: ValueExpr::Const(p),
                        toughness: ValueExpr::ObjectStat {
                            subject: Box::new(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It)),
                            stat: crate::ast::StatKind::Toughness,
                        },
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration,
                }]);
            }
            ts.restore(bpcp);
            if ts.eat_phrase(&["base", "toughness"])
                && let Some(t) = ts.next_number(false)
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::SetDynamicBasePT {
                        power: ValueExpr::ObjectStat {
                            subject: Box::new(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It)),
                            stat: crate::ast::StatKind::Power,
                        },
                        toughness: ValueExpr::Const(t),
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration,
                }]);
            }
            ts.restore(bpcp);
        }
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_face_v2(
                FaceInput {
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
                    .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
            }
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantAbility(Box::new(compiled)),
                duration,
            }]);
        }
        if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
            let mut out = vec![Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::GrantKeywords(kws),
                duration: Duration::Permanent,
            }];
            let qcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and")
                && let Some(quoted) = ts.next_quoted()
            {
                let compiled = compile_face_v2(
                    FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if compiled.is_ok() {
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::GrantAbility(Box::new(compiled)),
                        duration: Duration::Permanent,
                    });
                } else {
                    ts.restore(qcp);
                }
            } else {
                ts.restore(qcp);
            }
            // Support `has flying and is an Angel in addition to its other types` (`Valkyrie's Call`)
            {
                let tcp = ts.checkpoint();
                if ts.eat_phrase(&["and", "is"]) || ts.eat_phrase(&["and", "are"]) {
                    let _ = ts.eat_word("a") || ts.eat_word("an");
                    let mut subtypes = Vec::new();
                    let mut add_types = CardTypeMask::EMPTY;
                    while let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            subtypes.push(sub.to_string());
                        } else if let Some(ct) = lookup_card_type(&low) {
                            ts.next_word();
                            add_types = add_types.union(ct);
                        } else {
                            break;
                        }
                    }
                    if !subtypes.is_empty() || !add_types.is_empty() {
                        let keep_existing_types = ts
                            .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                        out.push(Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::BecomeCreatureTypes {
                                subtypes,
                                add_types,
                                keep_existing_types,
                                base_pt: None,
                                keywords: Vec::new(),
                            },
                            duration: Duration::Permanent,
                        });
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
    if ts.eat_word("becomes") || ts.eat_word("become") {
        if ts.eat_word("colorless") {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeColors {
                    colors: crate::ast::ColorMask::EMPTY,
                    keep_existing_colors: false,
                },
                duration,
            }]);
        }
        if ts.eat_phrase(&["the", "color", "or", "colors", "of", "your", "choice"])
            || ts.eat_phrase(&["the", "color", "of", "your", "choice"])
            || ts.eat_phrase(&["the", "color", "of", "its", "controller's", "choice"])
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeChosen(CharacteristicKind::Color),
                duration,
            }]);
        }
        if ts.eat_word("prepared") {
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::StateAction(ObjectStateAction::Prepare),
                duration: Duration::Permanent,
            }]);
        }
        if ts.eat_word("plotted") {
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantKeywords(vec![crate::v2::ir::KeywordSpec {
                    kind: crate::v2::ir::KeywordKind::Plot,
                    param: None,
                }]),
                duration: Duration::Permanent,
            }]);
        }
        if ts.eat_word("foretold") {
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantKeywords(vec![crate::v2::ir::KeywordSpec {
                    kind: crate::v2::ir::KeywordKind::Foretell,
                    param: None,
                }]),
                duration: Duration::Permanent,
            }]);
        }
        if ts.eat_phrase(&["the", "basic", "land", "type", "of", "your", "choice"]) {
            let _ = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "land", "types"]);
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeChosen(
                    crate::v2::ir::CharacteristicKind::BasicLandType,
                ),
                duration,
            }]);
        }
        if ts.eat_phrase(&["that", "color"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeChosen(CharacteristicKind::Color),
                duration,
            }]);
        }
        if ts.eat_phrase(&["that", "type"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeChosen(
                    crate::v2::ir::CharacteristicKind::CreatureType,
                ),
                duration,
            }]);
        }
        if ts.eat_phrase(&["the", "creature", "type", "of", "your", "choice"]) {
            let _ = ts.eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                || ts.eat_phrase(&[
                    "in", "addition", "to", "their", "other", "creature", "types",
                ])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeChosen(
                    crate::v2::ir::CharacteristicKind::CreatureType,
                ),
                duration,
            }]);
        }
        if ts.eat_phrase(&["a", "copy", "of"]) || ts.eat_phrase(&["copies", "of"]) {
            let source = parse_subject_ref(ts)?;
            let mut exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
            let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            if exceptions.is_empty() {
                exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                if let Some(d2) = parse_trailing_duration(ts) {
                    duration = d2;
                }
            }
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeCopy {
                    source: Box::new(source),
                    exceptions,
                },
                duration,
            }]);
        }
        let _ = ts.eat_word("a") || ts.eat_word("an");
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
        let mut add_types = CardTypeMask::EMPTY;
        let mut colors = crate::ast::ColorMask::EMPTY;
        let mut subtypes = Vec::new();
        while let Some(w) = ts.peek_word() {
            let low = w.to_ascii_lowercase();
            if let Some(ct) = lookup_card_type(&low) {
                ts.next_word();
                add_types = add_types.union(ct);
            } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                ts.next_word();
                subtypes.push(sub.to_string());
            } else if let Some(c) = lookup_color(&low) {
                ts.next_word();
                colors = colors.union(c);
            } else if low == "colorless"
                || (low == "and"
                    && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if {
                            let nl = nw.to_ascii_lowercase();
                            lookup_color(&nl).is_some()
                                || lookup_card_type(&nl).is_some()
                                || ts.vocab.lookup_subtype(&nl).is_some()
                        })
                    }))
            {
                ts.next_word();
            } else {
                break;
            }
        }
        let mut keep_existing_types = ts.eat_phrase(&[
            "in", "addition", "to", "its", "other", "colors", "and", "types",
        ]) || ts.eat_phrase(&[
            "in", "addition", "to", "their", "other", "colors", "and", "types",
        ]) || ts
            .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
            || ts.eat_phrase(&[
                "in", "addition", "to", "their", "other", "creature", "types",
            ])
            || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
        if is_xx {
            let mut kws = Vec::new();
            if ts.eat_word("with")
                && let Ok((k, _)) = try_parse(ts, parse_keyword_list)
            {
                kws = k;
            }
            if ts.eat_phrase(&[
                "in", "addition", "to", "its", "other", "colors", "and", "types",
            ]) || ts.eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
            {
                keep_existing_types = true;
            }
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            let mut out = vec![Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::SetDynamicBasePT {
                    power: ValueExpr::Var(crate::v2::ir::VarName::X),
                    toughness: ValueExpr::Var(crate::v2::ir::VarName::X),
                    add_types,
                    keep_existing_types,
                },
                duration: duration.clone(),
            }];
            if !subtypes.is_empty() || !kws.is_empty() {
                out.push(Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types,
                        base_pt: None,
                        keywords: kws,
                    },
                    duration,
                });
            }
            return Ok(out);
        }
        let keep_existing_colors = ts
            .eat_phrase(&["in", "addition", "to", "its", "other", "colors"])
            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "colors"]);
        if subtypes.is_empty() && add_types.is_empty() && base_pt.is_none() && !colors.is_empty() {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            let mut out = vec![Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::BecomeColors {
                    colors,
                    keep_existing_colors: keep_existing_colors || keep_existing_types,
                },
                duration: duration.clone(),
            }];
            if ts.eat_phrase(&["and", "attacks", "this", "turn", "if", "able"]) {
                out.push(Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                    duration: Duration::UntilEndOfTurn,
                });
            }
            return Ok(out);
        }
        if ts.eat_phrase(&[
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
        ]) || ts.eat_phrase(&[
            "and",
            "has",
            "base",
            "power",
            "and",
            "toughness",
            "each",
            "equal",
            "to",
        ]) || ts.eat_phrase(&[
            "with",
            "base",
            "power",
            "and",
            "toughness",
            "each",
            "equal",
            "to",
        ]) || ts.eat_phrase(&["with", "power", "and", "toughness", "each", "equal", "to"])
        {
            let val = parse_value_expr(ts)?;
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::SetDynamicBasePT {
                    power: val.clone(),
                    toughness: val,
                    add_types,
                    keep_existing_types,
                },
                duration,
            }]);
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
                let compiled = compile_face_v2(
                    FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if compiled.is_ok() {
                    extra_ability = Some(compiled);
                }
            } else if let Ok((kws, _)) = try_parse(ts, parse_keyword_list) {
                keywords = kws;
                if ts.eat_phrase(&["and", "all", "creature", "types"]) {
                    gained_all_creature_types = true;
                } else if ts.eat_word("and")
                    && let Some(quoted) = ts.next_quoted()
                {
                    let compiled = compile_face_v2(
                        FaceInput {
                            name: "~",
                            type_line: "Permanent",
                            oracle_text: quoted,
                        },
                        ts.vocab,
                    );
                    if compiled.is_ok() {
                        extra_ability = Some(compiled);
                    }
                }
            }
        }
        if (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
            && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
        {
            keywords.extend(kws);
        }
        let mut retained_type = None;
        if (ts.eat_phrase(&["that's", "still", "a"])
            || ts.eat_phrase(&["that's", "still", "an"])
            || ts.eat_phrase(&["that", "are", "still"]))
            && let Some(w) = ts.peek_word()
            && let Some(ct) = lookup_card_type(&w.to_ascii_lowercase())
        {
            ts.next_word();
            retained_type = Some(ct);
        }
        if !subtypes.is_empty() || !add_types.is_empty() || base_pt.is_some() {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            if ts.eat_phrase(&[
                "in", "addition", "to", "its", "other", "colors", "and", "types",
            ]) || ts.eat_phrase(&[
                "in", "addition", "to", "their", "other", "colors", "and", "types",
            ]) || ts.eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                || ts.eat_phrase(&[
                    "in", "addition", "to", "their", "other", "creature", "types",
                ])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
            {
                keep_existing_types = true;
            }
            let _ = ts.eat_phrase(&["that's", "no", "longer", "an", "equipment"])
                || ts.eat_phrase(&["that's", "no", "longer", "a", "creature"]);
            let duration = parse_trailing_duration(ts).unwrap_or(duration);
            let mut loses_abilities = ts.eat_phrase(&["and", "has", "no", "abilities"])
                || ts.eat_phrase(&["and", "loses", "all", "abilities"])
                || ts.eat_phrase(&["and", "loses", "all", "other", "abilities"])
                || ts.eat_phrase(&["and", "lose", "all", "other", "abilities"]);
            if !loses_abilities {
                let lcp = ts.checkpoint();
                if ts.eat_comma() && ts.eat_phrase(&["loses", "all", "abilities"]) {
                    loses_abilities = true;
                    let _ = ts.eat_comma();
                } else {
                    ts.restore(lcp);
                }
            }
            let mut out = vec![Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::BecomeCreatureTypes {
                    subtypes,
                    add_types,
                    keep_existing_types,
                    base_pt,
                    keywords,
                },
                duration: duration.clone(),
            }];
            if loses_abilities {
                out.push(Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::RemoveAllAbilities,
                    duration: duration.clone(),
                });
            }
            if let Some(ab) = extra_ability {
                out.push(Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::GrantAbility(Box::new(ab)),
                    duration: duration.clone(),
                });
            }
            if let Some(ct) = retained_type {
                out.push(Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::RetainTypes(ct),
                    duration: duration.clone(),
                });
            }
            if gained_all_creature_types {
                out.push(Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::SubjectRule {
                        subject: subject.clone(),
                        rule: SubjectStaticRule::AllCreatureTypes,
                        condition: None,
                    }),
                    duration: duration.clone(),
                });
            }
            // Optional post-duration `and can't be blocked [dur]` (`Dimir Keyrune`)
            {
                let bcp = ts.checkpoint();
                if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
                    let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlocked),
                        duration: bdur,
                    });
                } else {
                    ts.restore(bcp);
                }
            }
            // Optional post-duration `and gains <kws> [until end of turn]` (`Gideon`, `Water Wings`, `March from Velis Vel`)
            {
                let gcp = ts.checkpoint();
                if (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
                    && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                {
                    let gdur = parse_trailing_duration(ts).unwrap_or(duration);
                    out.push(Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::GrantKeywords(kws),
                        duration: gdur,
                    });
                } else {
                    ts.restore(gcp);
                }
            }
            return Ok(out);
        }
    }

    // 5a. `<subj> becomes prepared`
    if ts.eat_phrase(&["becomes", "prepared"]) || ts.eat_phrase(&["become", "prepared"]) {
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Prepare),
            duration: Duration::Permanent,
        }]);
    }

    // 5b. `<subj> assigns no combat damage this turn`
    if ts.eat_phrase(&["assigns", "no", "combat", "damage"])
        || ts.eat_phrase(&["assign", "no", "combat", "damage"])
    {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::AssignsNoCombatDamage),
            duration,
        }]);
    }

    // 5c. `<subj> can attack this turn as though it/they didn't have defender`
    if ts.eat_phrase(&[
        "can", "attack", "this", "turn", "as", "though", "they", "didn't", "have", "defender",
    ]) || ts.eat_phrase(&[
        "can", "attack", "this", "turn", "as", "though", "it", "didn't", "have", "defender",
    ]) {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CanAttackWithDefender,
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        }]);
    }

    // 5c2. `<subj> can block an additional creature / <N> additional creatures [dur]` (`Luminous Guardian`, `Mounted Archers`)
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["can", "block"]) {
            let extra = if ts.eat_phrase(&["an", "additional", "creature"]) {
                Some(1u8)
            } else if let Some(n) = ts.next_number(false).and_then(|v| u8::try_from(v).ok())
                && (ts.eat_phrase(&["additional", "creatures"])
                    || ts.eat_phrase(&["additional", "creature"]))
            {
                Some(n)
            } else {
                None
            };
            if let Some(n) = extra {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::CanBlockAdditional(n),
                    duration,
                }]);
            }
        }
        ts.restore(bcp);
    }
    // 5c3. `<subj> [this turn] cost(s) {M} less/more to cast [for each <expr>] [if <cond>]` (`Armor Wars`, `Spellbinding Soprano`)
    {
        let ccp = ts.checkpoint();
        let _ = ts.eat_phrase(&["this", "turn"]);
        if (ts.eat_word("cost") || ts.eat_word("costs"))
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
                return Ok(vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::CostModifier {
                        subject: flt,
                        delta,
                        less,
                        scale,
                        min_mana: None,
                        condition: cond,
                    }),
                    duration,
                }]);
            }
        }
        ts.restore(ccp);
    }

    // 5d. `<subj> attacks [<player>] this turn/combat if able` / `<subj> blocks [<tgt>] this turn if able` / `<subj> must be blocked this turn if able`
    if ts.eat_phrase(&["attacks", "this", "turn", "if", "able"])
        || ts.eat_phrase(&["attack", "this", "turn", "if", "able"])
        || ts.eat_phrase(&["attacks", "this", "combat", "if", "able"])
        || ts.eat_phrase(&["attack", "this", "combat", "if", "able"])
    {
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
            duration: Duration::UntilEndOfTurn,
        }]);
    }
    {
        let acp = ts.checkpoint();
        if (ts.eat_word("attacks") || ts.eat_word("attack"))
            && try_parse(ts, parse_subject_ref).is_ok()
            && (ts.eat_phrase(&["this", "combat", "if", "able"])
                || ts.eat_phrase(&["this", "turn", "if", "able"]))
        {
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                duration: Duration::UntilEndOfCombat,
            }]);
        }
        ts.restore(acp);
    }
    if ts.eat_phrase(&["can", "block", "any", "number", "of", "creatures"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::CanBlockAnyNumber,
                condition: None,
            }),
            duration,
        }]);
    }
    if ts.eat_phrase(&["must", "be", "blocked", "this", "turn", "if", "able"]) {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(crate::v2::ir::StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: crate::v2::ir::RuleRestrictionKind::MustBeBlocked,
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        }]);
    }
    {
        let bcp = ts.checkpoint();
        if ts.eat_word("blocks") || ts.eat_word("block") {
            let blocked_tgt = if ts.peek_phrase(&["this", "turn", "if", "able"])
                || ts.peek_phrase(&["this", "combat", "if", "able"])
            {
                None
            } else {
                opt(ts, parse_subject_ref).map(Box::new)
            };
            if ts.eat_phrase(&["this", "turn", "if", "able"])
                || ts.eat_phrase(&["this", "combat", "if", "able"])
            {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::MustBlockThisTurn(blocked_tgt),
                    duration: Duration::UntilEndOfTurn,
                }]);
            }
        }
        ts.restore(bcp);
    }

    // 6. `<subj> is [also] goaded [duration]` OR `<subj> is a/an [<color>] <Subtypes> [in addition to its other [colors and] types] [dur]` (`Rise from the Grave`, `Quicksilver Fountain`)
    {
        let icp = ts.checkpoint();
        if (ts.eat_phrase(&["is", "a"]) || ts.eat_phrase(&["is", "an"]))
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("copy"))
        {
            let mut add_types = CardTypeMask::EMPTY;
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(ct) = lookup_card_type(&low) {
                    ts.next_word();
                    add_types = add_types.union(ct);
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else if let Some(c) = lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else {
                    break;
                }
            }
            if !subtypes.is_empty() || !add_types.is_empty() {
                let keep_existing_types = ts.eat_phrase(&[
                    "in", "addition", "to", "its", "other", "colors", "and", "types",
                ]) || ts
                    .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"]);
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                let mut out = Vec::new();
                if !colors.is_empty() {
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::BecomeColors {
                            colors,
                            keep_existing_colors: keep_existing_types,
                        },
                        duration: duration.clone(),
                    });
                }
                out.push(Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types,
                        base_pt: None,
                        keywords: Vec::new(),
                    },
                    duration,
                });
                return Ok(out);
            }
        }
        ts.restore(icp);
    }
    if ts.eat_phrase(&["is", "also", "goaded"])
        || ts.eat_phrase(&["are", "also", "goaded"])
        || ts.eat_phrase(&["is", "goaded"])
        || ts.eat_phrase(&["are", "goaded"])
    {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Goad),
            duration,
        }]);
    }

    // 7. `<subj> can't ... [duration]` (delegated to `subject_combat.rs`)
    if ts.eat_word("can't") {
        return parse_subject_cant(ts, subject);
    }

    // 8. `<subj> doesn't untap during its controller's [next] untap step [for as long as <cond>]`
    if (ts.eat_phrase(&["doesn't", "untap", "during"])
        || ts.eat_phrase(&["don't", "untap", "during"]))
        && (ts.eat_phrase(&["its", "controller's", "next", "untap", "step"])
            || ts.eat_phrase(&["their", "controller's", "next", "untap", "step"])
            || ts.eat_phrase(&["their", "controllers'", "next", "untap", "steps"])
            || ts.eat_phrase(&["that", "player's", "next", "untap", "step"])
            || ts.eat_phrase(&["your", "next", "untap", "step"])
            || ts.eat_phrase(&["its", "controller's", "untap", "step"])
            || ts.eat_phrase(&["their", "controller's", "untap", "step"])
            || ts.eat_phrase(&["their", "controllers'", "untap", "steps"])
            || ts.eat_phrase(&["that", "player's", "untap", "step"])
            || ts.eat_phrase(&["your", "untap", "step"]))
    {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
            duration,
        }]);
    }

    // 9. `<subj> loses/lose [all abilities [and becomes ...] | [your choice of] <kws>] [duration]`
    if ts.eat_word("loses") || ts.eat_word("lose") {
        if ts.eat_phrase(&["all", "creature", "types"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeCreatureTypes {
                    subtypes: Vec::new(),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: false,
                    base_pt: None,
                    keywords: Vec::new(),
                },
                duration,
            }]);
        }
        if ts.eat_phrase(&["all", "abilities"]) {
            let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            let mut out = vec![Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::RemoveAllAbilities,
                duration: duration.clone(),
            }];
            {
                let ptcp = ts.checkpoint();
                if (ts.eat_phrase(&["and", "has", "base", "power", "and", "toughness"])
                    || ts.eat_phrase(&["and", "have", "base", "power", "and", "toughness"]))
                    && let Some((power, toughness)) = ts.next_base_pt()
                {
                    let pdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                    duration = pdur.clone();
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::SetDynamicBasePT {
                            power: ValueExpr::Const(power),
                            toughness: ValueExpr::Const(toughness),
                            add_types: CardTypeMask::EMPTY,
                            keep_existing_types: true,
                        },
                        duration: pdur,
                    });
                } else {
                    ts.restore(ptcp);
                }
            }
            {
                let bcp = ts.checkpoint();
                if ts.eat_phrase(&["and", "becomes"]) || ts.eat_phrase(&["and", "become"]) {
                    let _ = ts.eat_word("a") || ts.eat_word("an");
                    let mut base_pt = ts.next_base_pt();
                    let mut add_types = CardTypeMask::EMPTY;
                    let mut colors = crate::ast::ColorMask::EMPTY;
                    let mut subtypes = Vec::new();
                    while let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(ct) = lookup_card_type(&low) {
                            ts.next_word();
                            add_types = add_types.union(ct);
                        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            subtypes.push(sub.to_string());
                        } else if let Some(c) = lookup_color(&low) {
                            ts.next_word();
                            colors = colors.union(c);
                        } else if low == "and"
                            && ts.peek_n(1).is_some_and(|t| {
                                matches!(t.kind, TokenKind::Word(nw) if {
                                    let nl = nw.to_ascii_lowercase();
                                    lookup_color(&nl).is_some()
                                        || lookup_card_type(&nl).is_some()
                                        || ts.vocab.lookup_subtype(&nl).is_some()
                                })
                            })
                        {
                            ts.next_word();
                        } else {
                            break;
                        }
                    }
                    if base_pt.is_none()
                        && ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                    {
                        base_pt = ts.next_base_pt();
                    }
                    let keep_existing_types = ts
                        .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                        || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                    if !add_types.is_empty()
                        || !subtypes.is_empty()
                        || !colors.is_empty()
                        || base_pt.is_some()
                    {
                        let bdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        if bdur != duration {
                            duration = bdur.clone();
                            out[0] = Effect::MutateObject {
                                subject: subject.clone(),
                                mutation: ObjectMutation::RemoveAllAbilities,
                                duration: duration.clone(),
                            };
                        }
                        if !colors.is_empty() {
                            out.push(Effect::MutateObject {
                                subject: subject.clone(),
                                mutation: ObjectMutation::BecomeColors {
                                    colors,
                                    keep_existing_colors: false,
                                },
                                duration: bdur.clone(),
                            });
                        }
                        out.push(Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::BecomeCreatureTypes {
                                subtypes,
                                add_types,
                                keep_existing_types,
                                base_pt,
                                keywords: Vec::new(),
                            },
                            duration: bdur,
                        });
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
        let (kws, _) = parse_keyword_list(ts)?;
        let mut duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        let mut out = vec![Effect::MutateObject {
            subject: subject.clone(),
            mutation: ObjectMutation::RemoveKeywords(kws),
            duration: duration.clone(),
        }];
        let gcp = ts.checkpoint();
        if (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
            && let Ok((gkws, _)) = try_parse(ts, parse_keyword_list)
        {
            let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
            if gdur != duration {
                duration = gdur.clone();
                if let Effect::MutateObject { duration: d, .. } = &mut out[0] {
                    *d = duration.clone();
                }
            }
            out.push(Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantKeywords(gkws),
                duration: gdur,
            });
        } else {
            ts.restore(gcp);
        }
        return Ok(out);
    }

    // 10. `<subj> phases out [until ...]` / `<subj> fights <tgt>` / `<subj> fight each other` / `<subj> explores` / `<subj> connives`
    if ts.eat_phrase(&["phases", "out"]) || ts.eat_phrase(&["phase", "out"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::PhaseOut),
            duration,
        }]);
    }
    if ts.eat_word("fights") || ts.eat_word("fight") {
        let target = if ts.eat_phrase(&["each", "other"]) {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::Them)
        } else {
            parse_subject_ref(ts)?
        };
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Fight(Box::new(target)),
            duration: Duration::Instant,
        }]);
    }
    if ts.eat_word("endures") || ts.eat_word("endure") {
        let amount = parse_value_expr(ts)?;
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Endure,
                amount,
            },
            duration: Duration::Instant,
        }]);
    }
    if ts.eat_word("explores") || ts.eat_word("explore") {
        let _ = ts.eat_word("again");
        let ecp = ts.checkpoint();
        if let Ok(times) = try_parse(ts, parse_value_expr)
            && ts.eat_word("times")
        {
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::KeywordAction {
                    kind: ObjectKeywordAction::Explore,
                    amount: times,
                },
                duration: Duration::Instant,
            }]);
        }
        ts.restore(ecp);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Explore,
                amount: ValueExpr::Const(1),
            },
            duration: Duration::Instant,
        }]);
    }
    if ts.eat_word("connives") || ts.eat_word("connive") {
        let amt = opt(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1));
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Connive,
                amount: amt,
            },
            duration: Duration::Instant,
        }]);
    }

    if ts.eat_word("targets") || ts.eat_word("target") {
        let new_tgt = parse_subject_ref(ts)?;
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::RetargetTo(Box::new(new_tgt)),
            duration: Duration::Instant,
        }]);
    }

    super::subject_combat::parse_extra_subject_predicates(ts, subject)
}
