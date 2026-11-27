//! Combat restrictions, `can't` predicates, compound damage targets, and trailing duration
//! helpers for subject-headed effect clauses in `v2`.

use crate::ast::CardTypeMask;
use crate::v2::condition::parse_condition;
use crate::v2::filter::parse_subject_ref;
use crate::v2::ir::{
    Duration, Effect, ObjectMutation, PlayerRef, RuleRestrictionKind, StaticAbility, SubjectRef,
    ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

use super::subject_verbs::parse_subject_verb_frame;

/// Parse `<subj> can't ... [duration]` (handles both player-level and object-level restrictions).
pub(super) fn parse_subject_cant(
    ts: &mut TokenStream<'_>,
    subject: SubjectRef,
) -> PResult<Vec<Effect>> {
    // `<subj> can't attack/block [<tgt>] unless their/its controller pays <cost> [for each <expr>]` (`War Cadence`, `Forbidding Spirit`, `Sivitri`, `War Tax`)
    {
        let tax_cp = ts.checkpoint();
        if ts.eat_word("attack") || ts.eat_word("block") {
            let target = if !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("unless"))
            {
                opt(ts, parse_subject_ref).unwrap_or_else(|| subject.clone())
            } else {
                subject.clone()
            };
            if ts.eat_word("unless")
                && (ts.eat_phrase(&["their", "controller", "pays"])
                    || ts.eat_phrase(&["its", "controller", "pays"]))
                && let Ok(cost) = try_parse(ts, crate::v2::cost::parse_cost_atom)
            {
                let per_creature = ts.eat_phrase(&["for", "each", "of", "those", "creatures"])
                    || opt(ts, parse_for_each_expr).is_some();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::AttackTax {
                        target,
                        cost,
                        per_creature,
                    }),
                    duration,
                }]);
            }
        }
        ts.restore(tax_cp);
    }
    let player_opt = match subject {
        SubjectRef::Player(p) => Some(p),
        SubjectRef::TargetPlayer { opponent_only, .. } => Some(if opponent_only {
            PlayerRef::TargetOpponent
        } else {
            PlayerRef::TargetPlayer
        }),
        _ => None,
    };
    if let Some(player) = player_opt {
        if ts.eat_phrase(&["gain", "life"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: crate::v2::ir::PlayerRestrictionKind::CantGainLife,
                    condition: None,
                }),
                duration,
            }]);
        }
        if ts.eat_phrase(&[
            "activate",
            "abilities",
            "that",
            "aren't",
            "mana",
            "abilities",
        ]) || ts.eat_phrase(&["activate", "abilities"])
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: crate::v2::ir::PlayerRestrictionKind::CantActivateAbilities,
                    condition: None,
                }),
                duration,
            }]);
        }
        if ts.eat_phrase(&["search", "libraries"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: crate::v2::ir::PlayerRestrictionKind::CantSearchLibraries,
                    condition: None,
                }),
                duration,
            }]);
        }
        let is_win = ts.eat_phrase(&["win", "the", "game"]);
        if is_win || ts.eat_phrase(&["lose", "the", "game"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            let mut out = vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: if is_win {
                        crate::v2::ir::PlayerRestrictionKind::CantWinGame
                    } else {
                        crate::v2::ir::PlayerRestrictionKind::CantLoseGame
                    },
                    condition: None,
                }),
                duration,
            }];
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(more) = try_parse(ts, parse_subject_verb_frame)
            {
                out.extend(more);
            } else {
                ts.restore(acp);
            }
            return Ok(out);
        }
        if ts.eat_word("cast")
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            let duration = if ts.eat_phrase(&["during", "that", "player's", "next", "turn"])
                || ts.eat_phrase(&["during", "their", "next", "turn"])
            {
                Duration::UntilEndOfNextTurn
            } else {
                parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn)
            };
            let mut out = vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::RuleRestriction {
                    subject: Some(SubjectRef::Group(Box::new(flt))),
                    rule: RuleRestrictionKind::CantBeCast,
                    condition: None,
                }),
                duration,
            }];
            let acp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and")
                && let Ok(more) = try_parse(ts, parse_subject_verb_frame)
            {
                out.extend(more);
            } else {
                ts.restore(acp);
            }
            return Ok(out);
        }
    }
    let mutation = if ts.eat_phrase(&["become", "untapped"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBecomeUntapped)
    } else if ts.eat_phrase(&["be", "the", "targets", "of"])
        || ts.eat_phrase(&["be", "the", "target", "of"])
    {
        let flt = crate::v2::filter::parse_object_filter(ts)?;
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeTargetedBy(Box::new(flt)))
    } else if ts.eat_phrase(&["attack", "or", "block"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttackOrBlock)
    } else if ts.eat_word("attack") {
        let tcp = ts.checkpoint();
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("you"))
            && let Ok(tgt) = try_parse(ts, parse_subject_ref)
        {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttackTarget(Box::new(tgt)))
        } else {
            ts.restore(tcp);
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack)
        }
    } else if ts.eat_word("block") {
        if let Ok(blk_subj) = try_parse(ts, parse_subject_ref) {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlockSubject(Box::new(blk_subj)))
        } else {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock)
        }
    } else if ts.eat_phrase(&["be", "blocked"]) {
        let _ = ts.eat_phrase(&["this", "turn"]);
        if ts.eat_phrase(&["except", "by"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)))
        } else if ts.eat_word("by")
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)))
        } else {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlocked)
        }
    } else if ts.eat_phrase(&["be", "regenerated"]) {
        ObjectMutation::CantBeRegenerated
    } else if ts.eat_phrase(&["be", "countered"]) {
        // Optional `by <flt>` (`Autumn's Veil`: `can't be countered by blue or black spells this turn`)
        if ts.eat_word("by") {
            let _ = try_parse(ts, crate::v2::filter::parse_object_filter);
        }
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeCountered)
    } else if ts.eat_phrase(&["phase", "out"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantPhaseOut)
    } else {
        return Err(ts.err("expected combat restriction after `can't`"));
    };
    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
    let mut out = vec![Effect::MutateObject {
        subject: subject.clone(),
        mutation,
        duration,
    }];
    // Optional `and has base power and toughness P/T [dur]` (`Atomic Microsizer`)
    {
        let hcp = ts.checkpoint();
        if (ts.eat_phrase(&["and", "has", "base", "power", "and", "toughness"])
            || ts.eat_phrase(&["and", "have", "base", "power", "and", "toughness"]))
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let d2 = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            out.push(Effect::MutateObject {
                subject,
                mutation: ObjectMutation::SetDynamicBasePT {
                    power: ValueExpr::Const(power),
                    toughness: ValueExpr::Const(toughness),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                duration: d2,
            });
        } else {
            ts.restore(hcp);
        }
    }
    Ok(out)
}

pub(super) fn parse_damage_target(ts: &mut TokenStream<'_>) -> PResult<SubjectRef> {
    let first_tgt = parse_subject_ref(ts)?;
    let mut tgts = vec![first_tgt];
    loop {
        let tcp = ts.checkpoint();
        if ts.eat_word("and") {
            let acp = ts.checkpoint();
            if opt(ts, parse_value_expr).is_some() && ts.eat_word("damage") {
                ts.restore(tcp);
                break;
            }
            ts.restore(acp);
            if let Ok(next_t) = try_parse(ts, parse_subject_ref) {
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
    Ok(if tgts.len() == 1 {
        tgts.remove(0)
    } else {
        SubjectRef::Compound(tgts)
    })
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
        if ts.eat_word("until") && ts.eat_tilde() && ts.eat_phrase(&["is", "cast", "from", "exile"])
        {
            return Some(Duration::ForAsLongAsExiled);
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
    if ts.eat_phrase(&["until", "your", "next", "end", "step"]) {
        return Some(Duration::UntilYourNextEndStep);
    }
    if ts.eat_phrase(&["until", "your", "next", "upkeep"]) {
        return Some(Duration::UntilYourNextUpkeep);
    }
    if ts.eat_phrase(&["until", "your", "next", "turn"]) {
        return Some(Duration::UntilYourNextTurn);
    }
    if ts.eat_phrase(&["until", "the", "end", "of", "your", "next", "turn"]) {
        return Some(Duration::UntilEndOfNextTurn);
    }
    if ts.eat_phrase(&["for", "the", "rest", "of", "the", "game"]) {
        return Some(Duration::ForTheRestOfTheGame);
    }
    if ts.eat_phrase(&["until", "an", "opponent", "becomes", "the", "monarch"]) {
        return Some(Duration::ForAsLongAs(Box::new(
            crate::v2::ir::ConditionExpr::Not(Box::new(
                crate::v2::ir::ConditionExpr::PlayerCheck {
                    player: PlayerRef::AnyOpponent,
                    kind: crate::v2::ir::PlayerConditionKind::IsMonarch,
                },
            )),
        )));
    }
    if ts.eat_phrase(&["for", "as", "long", "as", "it", "remains", "exiled"])
        || ts.eat_phrase(&["for", "as", "long", "as", "they", "remain", "exiled"])
    {
        return Some(Duration::ForAsLongAsExiled);
    }
    {
        let uecp = ts.checkpoint();
        if ts.eat_phrase(&["until", "you", "exile", "another", "card", "with"]) && ts.eat_tilde() {
            return Some(Duration::ForAsLongAsExiled);
        }
        ts.restore(uecp);
    }
    if ts.eat_phrase(&["for", "as", "long", "as"]) || ts.eat_phrase(&["as", "long", "as"]) {
        let cond = parse_condition(ts).ok()?;
        return Some(Duration::ForAsLongAs(Box::new(cond)));
    }
    if ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    None
}
