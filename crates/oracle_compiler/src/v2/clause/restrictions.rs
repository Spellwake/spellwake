//! Activation/cast restriction, additional-combat, and temporary-static clause helpers for `v2`.

use crate::v2::clause::find_phrase_index;
use crate::v2::condition::parse_condition;
use crate::v2::filter::parse_object_filter;
use crate::v2::ir::{
    ActivationRestriction, Duration, Effect, ObjectFilter, ParseStage, PlayerMutation, PlayerRef,
    RuleRestrictionKind, StaticAbility, SubjectRef,
};
use crate::v2::parser::{PResult, TokenStream};
use crate::v2::static_ability::parse_static_ability;

pub(super) fn parse_additional_combat(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let _ = ts.eat_phrase(&["after", "this", "phase"])
        || ts.eat_phrase(&["after", "this", "main", "phase"])
        || ts.eat_phrase(&["after", "this", "combat", "phase"]);
    let _ = ts.eat_comma();
    if ts.eat_phrase(&["there", "is", "an", "additional", "combat", "phase"])
        || ts.eat_phrase(&["there's", "an", "additional", "combat", "phase"])
    {
        let _ = ts.eat_phrase(&["after", "this", "phase"]);
        let followed_by_main =
            ts.eat_phrase(&["followed", "by", "an", "additional", "main", "phase"]);
        return Ok(Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::AdditionalCombat { followed_by_main },
            duration: Duration::UntilEndOfTurn,
        });
    }
    Err(ts.err("expected additional combat phase clause"))
}

pub(super) fn parse_temporary_static_clause(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    // `players and permanents can't be the targets of spells or activated abilities`
    if ts.eat_phrase(&[
        "players",
        "and",
        "permanents",
        "can't",
        "be",
        "the",
        "targets",
        "of",
    ]) {
        let src = parse_object_filter(ts)?;
        return Ok(Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::Compound(vec![
                    SubjectRef::Player(PlayerRef::EachPlayer),
                    SubjectRef::All(Box::new(ObjectFilter {
                        any_permanent: true,
                        ..Default::default()
                    })),
                ])),
                rule: RuleRestrictionKind::CantBeTargetedBy(Box::new(src)),
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        });
    }
    // `The next enchantment spell you cast this turn can't be countered`
    if ts.peek_phrase(&["the", "next"]) {
        let st = parse_static_ability(ts)?;
        return Ok(Effect::ApplyContinuous {
            effect: Box::new(st),
            duration: Duration::UntilEndOfTurn,
        });
    }
    Err(ts.err("not a temporary static clause"))
}

pub(super) fn parse_activation_restrictions(
    ts: &mut TokenStream<'_>,
) -> PResult<Vec<ActivationRestriction>> {
    // `Activate no more than [twice | <N> times] each turn` (`Manaforge Cinder`, `Pit Imp`, `Phyrexian Battleflies`)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["activate", "no", "more", "than"])
            && (ts.eat_phrase(&["twice", "each", "turn"])
                || (ts.next_number(false).is_some() && ts.eat_phrase(&["times", "each", "turn"])))
        {
            return Ok(vec![ActivationRestriction::OnceEachTurn]);
        }
        ts.restore(ncp);
    }
    let valid_head = ts.eat_phrase(&["activate", "only"])
        || ts.eat_phrase(&["cast", "this", "spell", "only"])
        || {
            let cp = ts.checkpoint();
            if ts.eat_word("cast") && ts.eat_tilde() && ts.eat_word("only") {
                true
            } else {
                ts.restore(cp);
                false
            }
        };
    if !valid_head {
        return Err(ts.err("expected `Activate only` or `Cast ... only`"));
    }
    let mut out = Vec::new();
    loop {
        if ts.eat_phrase(&["as", "a", "sorcery"]) {
            out.push(ActivationRestriction::AsSorcery);
        } else if ts.eat_phrase(&["as", "an", "instant"]) {
            out.push(ActivationRestriction::AsInstant);
        } else if ts.eat_phrase(&["during", "your", "upkeep"]) {
            out.push(ActivationRestriction::DuringYourUpkeep);
        } else if ts.eat_phrase(&["during", "an", "opponent's", "upkeep"])
            || ts.eat_phrase(&["during", "each", "opponent's", "upkeep"])
        {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::And(
                    Box::new(crate::v2::ir::ConditionExpr::Not(Box::new(
                        crate::v2::ir::ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::IsYourTurn,
                        ),
                    ))),
                    Box::new(crate::v2::ir::ConditionExpr::EventCheck(
                        crate::v2::ir::EventCheckKind::DuringStep(crate::ast::TurnStepAst::Upkeep),
                    )),
                ),
            )));
        } else if ts.eat_phrase(&["during", "your", "turn"])
            || ts.eat_phrase(&["on", "your", "turn"])
        {
            out.push(ActivationRestriction::DuringYourTurn);
            if ts.eat_comma() && ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
                out.push(ActivationRestriction::BeforeAttackers);
            }
        } else if ts.eat_phrase(&["during", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["on", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "opponents'", "turns"])
        {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::Not(Box::new(crate::v2::ir::ConditionExpr::Flag(
                    crate::v2::ir::ConditionFlag::IsYourTurn,
                ))),
            )));
        } else if ts.eat_phrase(&["during", "combat", "on", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "combat", "on", "opponents'", "turns"])
        {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::Not(Box::new(crate::v2::ir::ConditionExpr::Flag(
                    crate::v2::ir::ConditionFlag::IsYourTurn,
                ))),
            )));
        } else if ts.eat_phrase(&["during", "combat", "on", "your", "turn"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringYourTurn);
        } else if ts.eat_phrase(&["during", "the", "declare", "attackers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "the", "declare", "blockers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["during", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::CombatDamage,
            ));
        } else if ts.eat_phrase(&["during", "the", "end", "of", "combat", "step"]) {
            out.push(ActivationRestriction::DuringEndOfCombat);
        } else if ts.eat_phrase(&["during", "combat", "before", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "combat", "after", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["before", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["after", "combat"]) {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::EventCheck(
                    crate::v2::ir::EventCheckKind::DuringStep(
                        crate::ast::TurnStepAst::PostcombatMain,
                    ),
                ),
            )));
        } else if ts.eat_phrase(&["during", "combat"]) {
            out.push(ActivationRestriction::DuringCombat);
        } else if ts.eat_phrase(&["before", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
            out.push(ActivationRestriction::BeforeAttackers);
        } else if ts.eat_phrase(&["once", "each", "turn"]) {
            out.push(ActivationRestriction::OnceEachTurn);
        } else if ts.eat_word("once") {
            out.push(ActivationRestriction::Once);
        } else if ts.eat_phrase(&["if", "you've", "controlled"])
            && ts.eat_tilde()
            && ts.eat_phrase(&[
                "continuously",
                "since",
                "the",
                "beginning",
                "of",
                "your",
                "most",
                "recent",
                "turn",
            ])
        {
            out.push(ActivationRestriction::ControlledContinuously);
        } else if ts.eat_word("if") {
            // Split before trailing `and only ...` (`Activate only if <cond> and only once each turn`)
            if let Some(ao_idx) = find_phrase_index(ts.tokens, ts.pos, &["and", "only"])
                && ao_idx > ts.pos
            {
                let cond_slice = &ts.tokens[ts.pos..ao_idx];
                let mut cond_ts = ts.sub_stream(cond_slice, ParseStage::Effect);
                let cond = parse_condition(&mut cond_ts)?;
                cond_ts.expect_eof()?;
                ts.pos = ao_idx;
                out.push(ActivationRestriction::OnlyIf(Box::new(cond)));
            } else {
                let mut cond = parse_condition(ts)?;
                if ts.eat_phrase(&["or", "if"]) {
                    let c2 = parse_condition(ts)?;
                    cond = crate::v2::ir::ConditionExpr::Or(Box::new(cond), Box::new(c2));
                }
                out.push(ActivationRestriction::OnlyIf(Box::new(cond)));
            }
        } else {
            return Err(ts.err("expected timing restriction after `Activate only`"));
        }
        if ts.eat_phrase(&["and", "only"]) || ts.eat_word("and") {
            continue;
        }
        break;
    }
    Ok(out)
}
