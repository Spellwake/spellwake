//! Subject-initiated verb frames and trailing duration clauses for `v2`.

use crate::FaceInput;
use crate::ast::CardTypeMask;
use crate::v2::card::compile_face_v2;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{
    ChoiceKind, Duration, Effect, ObjectMutation, PlayerMutation, PlayerRef, Replacement,
    RuleRestrictionKind, StaticAbility, SubjectRef, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};
use crate::vocab::{lookup_card_type, lookup_color};

/// Subject-headed verb frames (`<src> deals ...`, `<subj> gets ...`, `<subj> gains ...`,
/// `<subj> becomes ...`, `<subj> is goaded ...`, `<subj> can't ...`).
pub(super) fn parse_subject_verb_frame(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
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
    let _ = ts.eat_word("also") || ts.eat_word("each");

    // 1. `<src> deals <amt> damage ...` OR `<src> deals damage equal to <val> to <tgt>` OR `<src> deals damage to <tgt> equal to <val>`
    if ts.eat_word("deals") || ts.eat_word("deal") {
        // 1a. `deals damage equal to <val> to <tgt>` / `deals damage to <tgt> equal to <val>`
        if ts.eat_word("damage") {
            if ts.eat_phrase(&["equal", "to"]) {
                let amount = parse_value_expr(ts)?;
                if !ts.eat_word("to") {
                    return Err(ts.err("expected `to` after `damage equal to <val>`"));
                }
                let target = parse_subject_ref(ts)?;
                return Ok(vec![Effect::DealDamage {
                    source: subject,
                    target,
                    amount,
                    divided_by: None,
                }]);
            }
            if ts.eat_word("to") {
                let target = parse_subject_ref(ts)?;
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
            let amount = parse_value_expr(ts)?;
            if !ts.eat_word("damage") {
                return Err(ts.err("expected `damage` after amount"));
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
            if !ts.eat_word("to") {
                return Err(ts.err("expected `to` after `damage`"));
            }
            let first_tgt = parse_subject_ref(ts)?;
            let target = {
                let mut tgts = vec![first_tgt];
                loop {
                    let tcp = ts.checkpoint();
                    if ts.eat_word("and") {
                        // Lookahead: if next is `<amount> damage to ...`, leave for outer multi-damage loop!
                        let acp = ts.checkpoint();
                        if opt(ts, parse_value_expr).is_some() && ts.eat_word("damage") {
                            ts.restore(tcp);
                            break;
                        }
                        ts.restore(acp);
                        if let Ok(next_t) = try_parse(ts, parse_subject_ref) {
                            // Do NOT swallow a player actor for the next clause (e.g. `~ deals 2 damage to target creature and you gain 2 life`)!
                            let after_is_boundary = ts.is_eof()
                                || matches!(
                                    ts.peek().map(|t| &t.kind),
                                    Some(TokenKind::Comma | TokenKind::Period)
                                )
                                || ts.peek_word().is_some_and(|w| {
                                    matches!(
                                        w.to_ascii_lowercase().as_str(),
                                        "and" | "for" | "until" | "if" | "unless" | "this" | "at"
                                    )
                                });
                            if after_is_boundary {
                                tgts.push(next_t);
                                continue;
                            }
                        }
                    }
                    ts.restore(tcp);
                    break;
                }
                if tgts.len() == 1 {
                    tgts.remove(0)
                } else {
                    SubjectRef::Compound(tgts)
                }
            };
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
            let cp = ts.checkpoint();
            let has_sep = if ts.eat_comma() {
                let _ = ts.eat_word("and");
                true
            } else {
                ts.eat_word("and")
            };
            if has_sep && opt(ts, parse_value_expr).is_some() && ts.eat_word("damage") {
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

    // 2. `<subj> gets/get [an additional] +X/+Y [and gains <kws>] [until end of turn] [for each <expr>]`
    if ts.eat_word("gets") || ts.eat_word("get") {
        let _ = ts.eat_phrase(&["an", "additional"]);
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let mut gained_kws = None;
        if ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]) {
            let (kws, _) = parse_keyword_list(ts)?;
            gained_kws = Some(kws);
        }
        let mut lost_kws = None;
        if ts.eat_phrase(&["and", "loses"]) || ts.eat_phrase(&["and", "lose"]) {
            let (kws, _) = parse_keyword_list(ts)?;
            lost_kws = Some(kws);
        }
        let mut cant_mut = None;
        if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
            cant_mut = Some(ObjectMutation::CantBeBlocked);
        } else if ts.eat_phrase(&["and", "can't", "block"]) {
            cant_mut = Some(ObjectMutation::CantBlock);
        } else if ts.eat_phrase(&["and", "can't", "attack"]) {
            cant_mut = Some(ObjectMutation::CantAttack);
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        let scale = opt(ts, parse_for_each_expr).map(Box::new);
        let mut out = vec![Effect::MutateObject {
            subject: subject.clone(),
            mutation: ObjectMutation::ModifyPT {
                power,
                toughness,
                scale,
            },
            duration: duration.clone(),
        }];
        if let Some(kws) = gained_kws {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::GrantKeywords(kws),
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
                subject,
                mutation: cm,
                duration,
            });
        }
        return Ok(out);
    }

    // 3. `<subj> gains/gain "<quoted ability>" [duration]` OR `<subj> gains/gain [your choice of] <kws> [duration]`
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
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantAbility(Box::new(compiled)),
                duration,
            }]);
        }
        let is_choice = ts.eat_phrase(&["your", "choice", "of"]);
        let (kws, _) = parse_keyword_list(ts)?;
        let mut extra_ability = None;
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
        }
        let mut cant_mut = None;
        if ts.eat_phrase(&["and", "can't", "be", "blocked"]) {
            cant_mut = Some(ObjectMutation::CantBeBlocked);
        } else if ts.eat_phrase(&["and", "can't", "block"]) {
            cant_mut = Some(ObjectMutation::CantBlock);
        } else if ts.eat_phrase(&["and", "can't", "attack"]) {
            cant_mut = Some(ObjectMutation::CantAttack);
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
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
        if let Some(ab) = extra_ability {
            out.push(Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::GrantAbility(Box::new(ab)),
                duration: duration.clone(),
            });
        }
        if let Some(cm) = cant_mut {
            out.push(Effect::MutateObject {
                subject,
                mutation: cm,
                duration,
            });
        }
        return Ok(out);
    }

    // 4. `<subj> enters tapped` / `<subj> enters with <N> [additional] <kind> counters on it`
    if ts.eat_phrase(&["enters", "tapped"]) || ts.eat_phrase(&["enter", "tapped"]) {
        return Ok(vec![Effect::ApplyReplacement {
            replacement: Box::new(Replacement::EntersTapped { unless: None }),
            duration: Duration::Instant,
        }]);
    }
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let count = parse_value_expr(ts)?;
        let additional = ts.eat_word("additional");
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_phrase(&["on", "it"]) {
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

    // 4b. `<subj> has/have base power and toughness <P>/<T> [dur]` OR `<subj> has/have "<quoted ability>" [dur]` OR `<subj> has/have <kws> [dur]`
    if ts.eat_word("has") || ts.eat_word("have") {
        if ts.eat_phrase(&["base", "power", "and", "toughness"])
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::SetDynamicBasePT {
                    power: ValueExpr::Const(power),
                    toughness: ValueExpr::Const(toughness),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                duration,
            }]);
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
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantKeywords(kws),
                duration,
            }]);
        }
    }

    // 5. `<subj> becomes ...` (`a copy of <src>`, `the color of your choice`, `a <Subtypes/Types> [with base power and toughness P/T] [with <kws>] [dur]`)
    if ts.eat_word("becomes") || ts.eat_word("become") {
        if ts.eat_phrase(&["the", "color", "of", "your", "choice"])
            || ts.eat_phrase(&["the", "color", "of", "its", "controller's", "choice"])
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeChosenColor,
                duration,
            }]);
        }
        if ts.eat_phrase(&["a", "copy", "of"]) || ts.eat_phrase(&["copies", "of"]) {
            let source = parse_subject_ref(ts)?;
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeCopy {
                    source: Box::new(source),
                    exceptions: Vec::new(),
                },
                duration,
            }]);
        }
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let mut base_pt = ts.next_base_pt();
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
            } else if lookup_color(&low).is_some() || low == "and" {
                ts.next_word();
            } else {
                break;
            }
        }
        let keep_existing_types = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
            || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
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
        ]) {
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
        if ts.eat_word("with")
            && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
        {
            keywords = kws;
        }
        if !subtypes.is_empty() || !add_types.is_empty() || base_pt.is_some() {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::BecomeCreatureTypes {
                    subtypes,
                    add_types,
                    keep_existing_types,
                    base_pt,
                    keywords,
                },
                duration,
            }]);
        }
    }

    // 6. `<subj> is [also] goaded [duration]`
    if ts.eat_phrase(&["is", "also", "goaded"])
        || ts.eat_phrase(&["are", "also", "goaded"])
        || ts.eat_phrase(&["is", "goaded"])
        || ts.eat_phrase(&["are", "goaded"])
    {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Goad,
            duration,
        }]);
    }

    // 7. `<subj> can't attack [or block] / can't be blocked / can't be countered / can't be regenerated [duration]`
    if ts.eat_word("can't") {
        let mutation = if ts.eat_phrase(&["attack", "or", "block"]) {
            ObjectMutation::CantAttackOrBlock
        } else if ts.eat_word("attack") {
            ObjectMutation::CantAttack
        } else if ts.eat_word("block") {
            if let Ok(blk_subj) = try_parse(ts, parse_subject_ref) {
                ObjectMutation::CantBlockSubject(Box::new(blk_subj))
            } else {
                ObjectMutation::CantBlock
            }
        } else if ts.eat_phrase(&["be", "blocked"]) {
            ObjectMutation::CantBeBlocked
        } else if ts.eat_phrase(&["be", "regenerated"]) {
            ObjectMutation::CantBeRegenerated
        } else if ts.eat_phrase(&["be", "countered"]) {
            ObjectMutation::CantBeCountered
        } else {
            return Err(ts.err("expected combat restriction after `can't`"));
        };
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation,
            duration,
        }]);
    }

    // 8. `<subj> doesn't untap during its controller's next untap step`
    if (ts.eat_phrase(&["doesn't", "untap", "during"])
        || ts.eat_phrase(&["don't", "untap", "during"]))
        && (ts.eat_phrase(&["its", "controller's", "next", "untap", "step"])
            || ts.eat_phrase(&["their", "controllers'", "next", "untap", "steps"])
            || ts.eat_phrase(&["your", "next", "untap", "step"]))
    {
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::DoesntUntapNextUntap,
            duration: Duration::UntilYourNextTurn,
        }]);
    }

    // 9. `<subj> loses/lose [all abilities | [your choice of] <kws>] [duration]`
    if ts.eat_word("loses") || ts.eat_word("lose") {
        if ts.eat_phrase(&["all", "abilities"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::RemoveAllAbilities,
                duration,
            }]);
        }
        let _ = ts.eat_phrase(&["your", "choice", "of"]);
        let (kws, _) = parse_keyword_list(ts)?;
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::RemoveKeywords(kws),
            duration,
        }]);
    }

    // 10. `<subj> phases out` / `<subj> fights <tgt>` / `<subj> fight each other` / `<subj> explores` / `<subj> connives`
    if ts.eat_phrase(&["phases", "out"]) || ts.eat_phrase(&["phase", "out"]) {
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::PhaseOut,
            duration: Duration::Instant,
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
    if ts.eat_word("explores") || ts.eat_word("explore") {
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Explore,
            duration: Duration::Instant,
        }]);
    }
    if ts.eat_word("connives") || ts.eat_word("connive") {
        let amt = ts
            .next_number(false)
            .map(ValueExpr::Const)
            .unwrap_or(ValueExpr::Const(1));
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::Connive(amt),
            duration: Duration::Instant,
        }]);
    }

    Err(ts.err("expected verb predicate after subject"))
}

pub(super) fn parse_trailing_duration(ts: &mut TokenStream<'_>) -> Option<Duration> {
    if ts.eat_phrase(&["until", "end", "of", "turn"]) || ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_word("until")
            && ts.eat_tilde()
            && ts.eat_phrase(&["leaves", "the", "battlefield"])
        {
            return Some(Duration::UntilSourceLeaves);
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["until", "end", "of", "combat"])
        || ts.eat_phrase(&["at", "end", "of", "combat"])
        || ts.eat_phrase(&["this", "combat"])
        || ts.eat_phrase(&["that", "combat"])
    {
        return Some(Duration::UntilEndOfCombat);
    }
    if ts.eat_phrase(&["until", "your", "next", "turn"]) {
        return Some(Duration::UntilYourNextTurn);
    }
    if ts.eat_phrase(&["until", "the", "end", "of", "your", "next", "turn"]) {
        return Some(Duration::UntilEndOfNextTurn);
    }
    if ts.eat_phrase(&["for", "as", "long", "as", "it", "remains", "exiled"]) {
        return Some(Duration::ForAsLongAsExiled);
    }
    if ts.eat_phrase(&["for", "as", "long", "as"]) {
        let cond = parse_condition(ts).ok()?;
        return Some(Duration::ForAsLongAs(Box::new(cond)));
    }
    if ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    None
}
