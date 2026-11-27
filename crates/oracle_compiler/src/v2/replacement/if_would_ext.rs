//! Extended `If ... would ...` replacement effect parser for `v2` (extracted from `if_would.rs`).

use crate::v2::clause::parse_effect_sentence;
use crate::v2::filter::{parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    DamageReplacement, ObjectFilter, PlayerRef, PlayerReplacementKind, PlayerTriggerKind,
    Replacement, ReplacementRuleKind, SubjectRef, TriggerEvent, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::static_ability::cond_ext::parse_static_condition;
use crate::v2::value::parse_value_expr;

use super::{eat_period_eof, parse_effect_sentences_commit, parse_instead_clause, wrap_while_cond};

/// Parse extended `If ... would ...` replacement effects (damage-to, destroyed, draw-step skip,
/// forced discard, draw, prevent-fixed damage, mill doubling, lose-game, explore-again, etc.).
pub(super) fn parse_if_would_ext(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 1f. `If [combat] damage would be dealt to <subj> [while <cond>], prevent that damage [and/, <eff>] | <instead>.`
    {
        let cp = ts.checkpoint();
        let combat_only = ts.eat_word("combat");
        if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
            && let Ok(target) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            let cond = if ts.eat_word("while") {
                opt(ts, parse_static_condition)
            } else {
                None
            };
            if ts.eat_comma() {
                if !combat_only && ts.eat_phrase(&["prevent", "that", "damage"]) {
                    let mut rider = Vec::new();
                    if ts.eat_word("and") || ts.eat_comma() {
                        let _ = ts.eat_word("and");
                        let start = ts.pos;
                        let mut end = start;
                        while end < ts.tokens.len()
                            && !matches!(ts.tokens[end].kind, crate::v2::lexer::TokenKind::Period)
                        {
                            end += 1;
                        }
                        let mut sub = ts
                            .sub_stream(&ts.tokens[start..end], crate::v2::ir::ParseStage::Effect);
                        if let Ok(r) = parse_effect_sentence(&mut sub)
                            && sub.is_eof()
                        {
                            rider.extend(r);
                            ts.commit_substream_eof(end, &sub);
                        }
                    }
                    if ts.eat_period() && !ts.is_eof() {
                        let start = ts.pos;
                        let mut end = ts.tokens.len();
                        if end > start
                            && matches!(
                                ts.tokens[end - 1].kind,
                                crate::v2::lexer::TokenKind::Period
                            )
                        {
                            end -= 1;
                        }
                        let mut sub = ts
                            .sub_stream(&ts.tokens[start..end], crate::v2::ir::ParseStage::Effect);
                        if let Ok(r2) = parse_effect_sentence(&mut sub)
                            && sub.is_eof()
                        {
                            rider.extend(r2);
                            ts.commit_substream_eof(end, &sub);
                        }
                    }
                    if eat_period_eof(ts) {
                        if rider.is_empty() {
                            return Ok(wrap_while_cond(
                                cond,
                                Replacement::DamageRule(DamageReplacement::PreventAll { target }),
                            ));
                        }
                        return Ok(Replacement::DamageRule(
                            DamageReplacement::PreventWithRider {
                                target,
                                condition: cond,
                                rider,
                            },
                        ));
                    }
                } else {
                    // Single or multi-sentence instead clause
                    let m_cp = ts.checkpoint();
                    if let Some(rep) = try_parse_multi_sentence_instead(
                        ts,
                        TriggerEvent::DealtDamage {
                            subject: target.clone(),
                            combat_only,
                        },
                    ) {
                        return Ok(wrap_while_cond(cond, rep));
                    }
                    ts.restore(m_cp);
                    if let Ok(instead) = parse_instead_clause(ts)
                        && eat_period_eof(ts)
                    {
                        return Ok(wrap_while_cond(
                            cond,
                            Replacement::EventInstead {
                                event: TriggerEvent::DealtDamage {
                                    subject: target,
                                    combat_only,
                                },
                                instead,
                            },
                        ));
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 1f1. `If <subj> would be destroyed, regenerate it / <instead>.`
    {
        let dcp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["would", "be", "destroyed"])
            && ts.eat_comma()
        {
            if ts.eat_word("regenerate")
                && (ts.eat_word("it") || ts.eat_word("them") || ts.eat_tilde())
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::Dies {
                            subject: subject.clone(),
                        },
                        instead: vec![crate::v2::ir::Effect::MutateObject {
                            subject,
                            mutation: crate::v2::ir::ObjectMutation::StateAction(
                                crate::v2::ir::ObjectStateAction::Regenerate,
                            ),
                            duration: crate::v2::ir::Duration::Instant,
                        }],
                    });
                }
            } else if let Ok(instead) = parse_instead_clause(ts) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::Dies { subject },
                        instead,
                    });
                }
            }
        }
        ts.restore(dcp);
    }

    // 1f2. `If you would begin your draw step, [you may] skip that step instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "begin", "your", "draw", "step"]) && ts.eat_comma() {
            let optional = ts.eat_phrase(&["you", "may"]);
            if ts.eat_phrase(&["skip", "that", "step", "instead"]) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::PlayerRule {
                        player: PlayerRef::You,
                        rule: PlayerReplacementKind::SkipStep {
                            step: crate::ast::TurnStepAst::DrawStep,
                            optional,
                        },
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1f3. Forced discard replacements (`If a spell or ability an opponent controls causes you to discard ~ / a card, <instead>.`)
    {
        let cp = ts.checkpoint();
        if let Ok(cause) = try_parse(ts, parse_object_filter)
            && cause.any_spell
            && cause.is_ability
            && cause.controller == Some(crate::ast::ControllerRelation::Opponent)
            && ts.eat_phrase(&["causes", "you", "to", "discard"])
        {
            let disc_subj = if ts.eat_tilde() {
                Some(SubjectRef::SelfObject)
            } else if ts.eat_phrase(&["a", "card"]) {
                Some(SubjectRef::Group(Box::new(ObjectFilter {
                    any_card: true,
                    ..Default::default()
                })))
            } else {
                None
            };
            if let Some(subject) = disc_subj
                && ts.eat_comma()
                && let Ok(instead) = parse_instead_clause(ts)
                && eat_period_eof(ts)
            {
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::ForcedDiscard {
                        player: PlayerRef::You,
                        subject,
                    },
                    instead,
                });
            }
        }
        ts.restore(cp);
    }

    // 1g. `If <player> would draw a card [while <cond>], <instead>.`
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "draw", "a", "card"])
        {
            let while_cond =
                if ts.eat_phrase(&["while", "your", "library", "has", "no", "cards", "in", "it"]) {
                    Some(crate::v2::ir::ConditionExpr::Compare {
                        lhs: Box::new(ValueExpr::PlayerStat {
                            player: PlayerRef::You,
                            stat: crate::ast::PlayerStatKind::CardsInLibrary,
                        }),
                        op: crate::ast::CompareOp::Eq,
                        rhs: Box::new(ValueExpr::Const(0)),
                    })
                } else if ts.eat_word("while") {
                    opt(ts, parse_static_condition)
                } else {
                    None
                };
            if ts.eat_comma() {
                let dcp = ts.checkpoint();
                // `you may instead <effect sentences>`
                if ts.eat_phrase(&["you", "may", "instead"])
                    && let Some(effs) = parse_effect_sentences_commit(ts)
                {
                    return Ok(wrap_while_cond(
                        while_cond,
                        Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player,
                                action: PlayerTriggerKind::CardDrawn,
                            },
                            instead: vec![crate::v2::ir::Effect::Optional {
                                actor: PlayerRef::You,
                                body: effs,
                            }],
                        },
                    ));
                }
                ts.restore(dcp);
                if let Ok(instead) = parse_instead_clause(ts)
                    && eat_period_eof(ts)
                {
                    return Ok(wrap_while_cond(
                        while_cond,
                        Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player,
                                action: PlayerTriggerKind::CardDrawn,
                            },
                            instead,
                        },
                    ));
                }
                ts.restore(dcp);
                if let Some(rep) = try_parse_multi_sentence_instead(
                    ts,
                    TriggerEvent::PlayerAction {
                        player,
                        action: PlayerTriggerKind::CardDrawn,
                    },
                ) {
                    return Ok(wrap_while_cond(while_cond, rep));
                }
            }
        }
        ts.restore(cp);
    }

    // 1g1. `If <flt> would deal damage [to <tgt>], prevent <N> of that damage [, where X is <val>].`
    {
        let cp = ts.checkpoint();
        if let Ok(src_flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["would", "deal", "damage"])
        {
            let target = if ts.eat_word("to") {
                if let Some(mut first_tgt) = opt(ts, parse_subject_ref) {
                    let mut tgts = vec![first_tgt.clone()];
                    loop {
                        let tcp = ts.checkpoint();
                        let _ = ts.eat_comma();
                        if !(ts.eat_word("or") || ts.eat_word("and/or")) {
                            ts.restore(tcp);
                            break;
                        }
                        let _ = ts.eat_phrase(&["one", "or", "more"]);
                        if let Ok(next_tgt) = try_parse(ts, parse_subject_ref) {
                            tgts.push(next_tgt);
                            continue;
                        }
                        ts.restore(tcp);
                        break;
                    }
                    if tgts.len() > 1 {
                        first_tgt = SubjectRef::Compound(tgts);
                    }
                    Some(first_tgt)
                } else {
                    None
                }
            } else {
                None
            };
            if ts.eat_comma()
                && ts.eat_word("prevent")
                && let Ok(mut amount) = try_parse(ts, parse_value_expr)
                && ts.eat_phrase(&["of", "that", "damage"])
            {
                let wcp = ts.checkpoint();
                if ts.eat_comma()
                    && ts.eat_phrase(&["where", "x", "is"])
                    && let Ok(val) = try_parse(ts, parse_value_expr)
                {
                    amount = val;
                } else {
                    ts.restore(wcp);
                }
                if eat_period_eof(ts) {
                    return Ok(Replacement::DamageRule(DamageReplacement::PreventFixed {
                        source: Some(src_flt),
                        target,
                        amount,
                    }));
                }
            }
        }
        ts.restore(cp);
    }

    // 1g2. Mill doubling (`If <player> would mill one or more cards, they mill twice that many cards instead.`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "mill", "one", "or", "more", "cards"])
            && ts.eat_comma()
            && ts.eat_phrase(&["they", "mill", "twice", "that", "many", "cards", "instead"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::PlayerRule {
                    player,
                    rule: PlayerReplacementKind::MultiplyMill { factor: 2 },
                });
            }
        }
        ts.restore(cp);
    }

    // 1g5. Lose-the-game replacement (`If you would lose the game, <instead>.`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "lose", "the", "game"])
            && ts.eat_comma()
            && let Ok(instead) = parse_instead_clause(ts)
            && eat_period_eof(ts)
        {
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::PlayerAction {
                    player: PlayerRef::You,
                    action: PlayerTriggerKind::LosesGame,
                },
                instead,
            });
        }
        ts.restore(cp);
    }

    // 1g4. Explore-again replacement (`If <flt> would explore, instead it explores, then it explores again.`)
    {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["would", "explore"])
            && ts.eat_comma()
            && ts.eat_phrase(&["instead", "it", "explores"])
            && ts.eat_comma()
            && ts.eat_phrase(&["then", "it", "explores", "again"])
            && eat_period_eof(ts)
        {
            return Ok(Replacement::Rule(ReplacementRuleKind::ExploreAgain(filter)));
        }
        ts.restore(cp);
    }

    // 1g6. `If a creature you control that's enchanted would die, <instead>.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "creature", "you", "control", "that's", "enchanted"])
            && ts.eat_phrase(&["would", "die"])
            && ts.eat_comma()
            && let Ok(instead) = parse_instead_clause(ts)
            && eat_period_eof(ts)
        {
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::Dies {
                    subject: SubjectRef::Group(Box::new(ObjectFilter {
                        any_types: crate::ast::CardTypeMask::CREATURE,
                        controller: Some(crate::ast::ControllerRelation::You),
                        flags: crate::v2::ir::FilterFlags::ENCHANTED,
                        ..Default::default()
                    })),
                },
                instead,
            });
        }
        ts.restore(cp);
    }

    Err(ts.err("not an extended if-would replacement"))
}

fn try_parse_multi_sentence_instead(
    ts: &mut TokenStream<'_>,
    event: TriggerEvent,
) -> Option<Replacement> {
    let cp = ts.checkpoint();
    let first_dot = (ts.pos..ts.tokens.len())
        .find(|&idx| matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Period))?;
    if first_dot + 1 >= ts.tokens.len() {
        return None;
    }
    let mut head_ts = ts.sub_stream(&ts.tokens[ts.pos..first_dot], ts.stage);
    let mut instead = parse_instead_clause(&mut head_ts).ok()?;
    if !head_ts.is_eof() {
        ts.restore(cp);
        return None;
    }
    ts.commit_substream_eof(first_dot, &head_ts);
    let _ = ts.eat_period();
    if let Some(more) = parse_effect_sentences_commit(ts) {
        instead.extend(more);
        return Some(Replacement::EventInstead { event, instead });
    }
    ts.restore(cp);
    None
}
