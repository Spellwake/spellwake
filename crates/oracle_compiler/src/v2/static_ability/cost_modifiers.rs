//! Static spell cost-modifier and next-spell-modifier parsers for `v2`.

use crate::ast::{ControllerRelation, TurnStepAst};
use crate::v2::filter::{parse_keyword_list, parse_object_filter};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, ConditionFlag, EventCheckKind, FilterPredicate,
    ObjectFilter, PlayerRef, RuleRestrictionKind, StaticAbility, SubjectRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

use super::cond_ext::{parse_static_condition, parse_static_scale_expr};

/// Parse `The next/first <flt> you/they cast this/each turn can't be countered / can be cast ... / has <kws> / costs {M} less to cast`.
pub(super) fn parse_next_spell_modifier(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();
    if ts.eat_any_phrase(&[&["the", "next"], &["the", "first"]])
        && let Ok(mut flt) = try_parse(ts, parse_object_filter)
    {
        let caster = if ts.eat_any_phrase(&[
            &["you", "cast", "this", "turn"],
            &["you", "cast", "each", "turn"],
            &["you", "cast", "during", "each", "of", "your", "turns"],
        ]) {
            Some((PlayerRef::You, ControllerRelation::You))
        } else if ts.eat_any_phrase(&[
            &["they", "cast", "this", "turn"],
            &["they", "cast", "each", "turn"],
        ]) {
            Some((PlayerRef::ThatPlayer, ControllerRelation::ThatPlayer))
        } else if flt.controller.is_some()
            && ts.eat_any_phrase(&[
                &["this", "turn"],
                &["each", "turn"],
                &["during", "each", "of", "your", "turns"],
            ])
        {
            let p = match flt.controller {
                Some(ControllerRelation::ThatPlayer) => PlayerRef::ThatPlayer,
                _ => PlayerRef::You,
            };
            Some((p, flt.controller.unwrap_or(ControllerRelation::You)))
        } else {
            None
        };
        if let Some((player, ctrl)) = caster {
            flt.controller = Some(ctrl);
            if ts.eat_phrase(&["can't", "be", "countered"]) {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt,
                    effect: Box::new(StaticAbility::RuleRestriction {
                        subject: None,
                        rule: RuleRestrictionKind::CantBeCountered,
                        condition,
                    }),
                });
            }
            if ts.eat_phrase(&["can", "be", "cast", "as", "though", "it", "had", "flash"]) {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt.clone(),
                    effect: Box::new(StaticAbility::ZoneCastPermission {
                        player,
                        filter: Some(flt),
                        subject: None,
                        zone: None,
                        mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                        condition,
                    }),
                });
            }
            if ts.eat_phrase(&[
                "can", "be", "cast", "without", "paying", "its", "mana", "cost",
            ]) {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt.clone(),
                    effect: Box::new(StaticAbility::ZoneCastPermission {
                        player,
                        filter: Some(flt),
                        subject: None,
                        zone: None,
                        mode: CastPermissionMode::WithoutPaying,
                        condition,
                    }),
                });
            }
            if ts.eat_word("has")
                && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
            {
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt.clone(),
                    effect: Box::new(StaticAbility::GrantKeywords {
                        subject: SubjectRef::Group(Box::new(flt)),
                        keywords,
                        condition,
                    }),
                });
            }
            if ts.eat_word("costs")
                && let Some(delta) = ts.eat_mana_cost()
                && (ts.eat_phrase(&["less", "to", "cast"]) || ts.eat_word("less"))
            {
                let scale = opt(ts, parse_static_scale_expr);
                let cost_mod = StaticAbility::CostModifier {
                    subject: flt.clone(),
                    delta,
                    less: true,
                    scale,
                    min_mana: None,
                    condition: condition.clone(),
                };
                let effect = if ts.eat_phrase(&[
                    "and", "can", "be", "cast", "as", "though", "it", "had", "flash",
                ]) {
                    StaticAbility::Compound(vec![
                        cost_mod,
                        StaticAbility::ZoneCastPermission {
                            player,
                            filter: Some(flt.clone()),
                            subject: None,
                            zone: None,
                            mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                            condition,
                        },
                    ])
                } else {
                    cost_mod
                };
                return Ok(StaticAbility::NextSpellModifier {
                    filter: flt,
                    effect: Box::new(effect),
                });
            }
        }
    }
    ts.restore(cp);
    Err(ts.err("not a next-spell modifier"))
}

/// Parse `[The first ]<filter> [<caster> cast(s)] [each turn] cost(s) {M} more/less to cast [for each <expr>]`.
pub(super) fn parse_cost_modifier(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let _ = ts.eat_word("each");
    let mut nth_spell: Option<u8> = None;
    if ts.eat_phrase(&["the", "first"]) {
        nth_spell = Some(1);
    } else if ts.eat_phrase(&["the", "second"]) {
        nth_spell = Some(2);
    }

    let mut flt: ObjectFilter = if ts.eat_tilde()
        || ts.eat_phrase(&["this", "spell"])
        || ts.eat_phrase(&["those", "spells"])
        || ts.eat_word("it")
    {
        ObjectFilter {
            any_spell: true,
            ..Default::default()
        }
    } else if ts.eat_phrase(&["your", "commander"]) {
        ObjectFilter {
            any_spell: true,
            controller: Some(ControllerRelation::You),
            flags: crate::v2::ir::FilterFlags::IS_COMMANDER,
            ..Default::default()
        }
    } else {
        let first = parse_object_filter(ts)?;
        // Support `<flt1> and <flt2> [you cast] cost ...`
        let acp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(second) = try_parse(ts, parse_object_filter)
        {
            ObjectFilter {
                predicates: vec![FilterPredicate::AnyOf(vec![first, second])],
                ..Default::default()
            }
        } else {
            ts.restore(acp);
            first
        }
    };

    // Preserve caster relation explicitly!
    if ts.eat_phrase(&["you", "cast"]) {
        flt.controller = Some(ControllerRelation::You);
    } else if ts.eat_phrase(&["your", "opponents", "cast"]) || ts.eat_phrase(&["opponents", "cast"])
    {
        flt.controller = Some(ControllerRelation::Opponents);
    } else if ts.eat_phrase(&["an", "opponent", "casts"]) {
        flt.controller = Some(ControllerRelation::Opponent);
    } else if ts.eat_phrase(&["enchanted", "player", "casts"]) {
        flt.controller = Some(ControllerRelation::ThatPlayer);
    } else if ts.eat_phrase(&["players", "cast"]) {
        // Any player
    }
    let _ = ts.eat_phrase(&["each", "turn"]);

    let mut inline_cond: Option<ConditionExpr> = None;
    if ts.eat_phrase(&["during", "your", "turn"]) {
        inline_cond = Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn));
    }

    if let Some(n) = nth_spell {
        flt.predicates
            .push(FilterPredicate::NthSpellCastThisTurn(n));
    }

    if !(ts.eat_word("cost") || ts.eat_word("costs")) {
        return Err(ts.err("expected `cost` or `costs` in cost modifier"));
    }

    let delta = ts
        .eat_mana_cost()
        .ok_or_else(|| ts.err("expected mana delta in cost modifier"))?;

    let less = if ts.eat_phrase(&["less", "to", "cast"]) {
        true
    } else if ts.eat_phrase(&["more", "to", "cast"]) {
        false
    } else {
        return Err(ts.err("expected `less to cast` or `more to cast`"));
    };

    let scale = parse_cost_scale_expr(ts);
    let extra_cond = if ts.eat_word("if") {
        Some(parse_static_condition(ts)?)
    } else if ts.eat_phrase(&["during", "your", "end", "step"]) {
        Some(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
            TurnStepAst::EndStep,
        )))
    } else if ts.eat_phrase(&["during", "your", "turn"]) {
        Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn))
    } else if ts.eat_phrase(&["except", "during", "its", "controller's", "turn"])
        || ts.eat_phrase(&["except", "during", "their", "controllers'", "turns"])
    {
        Some(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::IsYourTurn,
        ))))
    } else {
        None
    };
    let mut final_cond = condition;
    for c in [inline_cond, extra_cond].into_iter().flatten() {
        final_cond = Some(match final_cond {
            Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
            None => c,
        });
    }

    let first_mod = StaticAbility::CostModifier {
        subject: flt.clone(),
        delta,
        less,
        scale,
        min_mana: None,
        condition: final_cond.clone(),
    };
    // Optional second cost modifier conjunct (`and {M} less/more to cast [for each ...]`)
    let and_cp = ts.checkpoint();
    if ts.eat_word("and")
        && let Some(delta2) = ts.eat_mana_cost()
    {
        let less2 = if ts.eat_phrase(&["less", "to", "cast"]) {
            Some(true)
        } else if ts.eat_phrase(&["more", "to", "cast"]) {
            Some(false)
        } else {
            None
        };
        if let Some(less2) = less2 {
            let scale2 = parse_cost_scale_expr(ts);
            return Ok(StaticAbility::Compound(vec![
                first_mod,
                StaticAbility::CostModifier {
                    subject: flt,
                    delta: delta2,
                    less: less2,
                    scale: scale2,
                    min_mana: None,
                    condition: final_cond,
                },
            ]));
        }
    }
    ts.restore(and_cp);

    Ok(first_mod)
}

fn parse_cost_scale_expr(ts: &mut TokenStream<'_>) -> Option<crate::v2::ir::ValueExpr> {
    use crate::v2::ir::{PriorActionKind, TurnEventKind, ValueExpr};
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["for", "each"])
        && let Ok(val) = try_parse(ts, crate::v2::value::parse_for_each_expr)
        && matches!(val, ValueExpr::CommanderCastCount(_))
    {
        return Some(val);
    }
    ts.restore(cp);
    if ts.eat_phrase(&["for", "each", "other", "spell", "cast", "this", "turn"]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::SpellCast,
            player: None,
            exclude_first: true,
        });
    }
    if ts.eat_phrase(&["for", "each", "permanent", "sacrificed", "this", "way"]) {
        return Some(ValueExpr::PriorActionCount(
            PriorActionKind::SacrificedThisWay,
        ));
    }
    if ts.eat_phrase(&["for", "each", "counter", "removed", "this", "way"]) {
        return Some(ValueExpr::PriorActionCount(
            PriorActionKind::CountersRemovedThisWay,
        ));
    }
    if ts.eat_phrase(&[
        "for",
        "each",
        "card",
        "you've",
        "cycled",
        "or",
        "discarded",
        "this",
        "turn",
    ]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::CardDiscarded,
            player: Some(PlayerRef::You),
            exclude_first: false,
        });
    }
    if ts.eat_phrase(&[
        "for",
        "each",
        "card",
        "your",
        "opponents",
        "have",
        "drawn",
        "this",
        "turn",
    ]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::CardDrawn,
            player: Some(PlayerRef::EachOpponent),
            exclude_first: false,
        });
    }
    if ts.eat_phrase(&[
        "for", "each", "creature", "you", "attacked", "with", "this", "turn",
    ]) {
        return Some(ValueExpr::Count(Box::new(ObjectFilter {
            any_types: crate::ast::CardTypeMask::CREATURE,
            controller: Some(ControllerRelation::You),
            flags: crate::v2::ir::FilterFlags::ATTACKED_THIS_TURN,
            ..Default::default()
        })));
    }
    ts.restore(cp);
    opt(ts, parse_static_scale_expr)
}
