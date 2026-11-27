//! Extended `If ... would ...` replacement effect parser for `v2` (extracted from `if_would.rs`).

use crate::v2::clause::parse_effect_sentence;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    DamageReplacement, ObjectFilter, PlayerActionKind, PlayerRef, PlayerReplacementKind,
    PlayerTriggerKind, Replacement, ReplacementRuleKind, SubjectRef, TriggerEvent, ValueExpr,
    ZoneKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::static_ability::cond_ext::parse_static_condition;
use crate::v2::value::parse_value_expr;

use super::{
    eat_period_eof, eat_rep_seq, grant_self_as_enters, parse_instead_clause, trim_trailing_period,
    wrap_while_cond,
};

/// Parse extended `If ... would ...` replacement effects (damage-to, destroyed, draw, mill,
/// extra-turn, lose-game, unspent-mana, explore/connive, untap, graveyard-exile, etc.).
pub(super) fn parse_if_would_ext(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 1f. `If [combat|noncombat] damage would be dealt to <subj> [while <cond>], prevent that damage [and/, <eff>] | <instead>.`
    {
        let cp = ts.checkpoint();
        let combat_only = ts.eat_word("combat");
        let _ = ts.eat_word("noncombat");
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
                // `instead that damage is dealt, but all other damage already dealt to him/it is healed` (`Wolverine, Fierce Fighter`)
                {
                    let wcp = ts.checkpoint();
                    if ts.eat_phrase(&["instead", "that", "damage", "is", "dealt"])
                        && ts.eat_comma()
                        && ts.eat_phrase(&[
                            "but", "all", "other", "damage", "already", "dealt", "to",
                        ])
                        && (ts.eat_word("him") || ts.eat_word("her") || ts.eat_word("it"))
                        && ts.eat_phrase(&["is", "healed"])
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::DamageRule(DamageReplacement::PreventAll {
                                target,
                            }));
                        }
                    }
                    ts.restore(wcp);
                }
                if ts.eat_phrase(&["prevent", "that", "damage"]) {
                    let mut rider = Vec::new();
                    // `prevent that damage and an opponent of your choice gains control of it` (`Kharn the Betrayer`)
                    let kcp = ts.checkpoint();
                    if ts.eat_phrase(&[
                        "and", "an", "opponent", "of", "your", "choice", "gains", "control", "of",
                        "it",
                    ]) {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::DamageRule(
                                DamageReplacement::PreventWithRider {
                                    target,
                                    condition: cond,
                                    rider: vec![crate::v2::ir::Effect::MutateObject {
                                        subject: SubjectRef::SelfObject,
                                        mutation: crate::v2::ir::ObjectMutation::GainControl {
                                            controller: PlayerRef::AnyOpponent,
                                        },
                                        duration: crate::v2::ir::Duration::Permanent,
                                    }],
                                },
                            ));
                        }
                    }
                    ts.restore(kcp);
                    // `prevent that damage, remove that many +1/+1 counters from it, then give each player a rad counter for each +1/+1 counter removed this way` (`Bloatfly Swarm`)
                    if ts.eat_comma()
                        && ts.eat_phrase(&["remove", "that", "many"])
                        && opt(ts, parse_counter_kind).is_some()
                        && (ts.eat_word("counters") || ts.eat_word("counter"))
                        && ts.eat_phrase(&["from", "it"])
                        && ts.eat_comma()
                        && ts.eat_phrase(&["then", "give", "each", "player", "a"])
                        && opt(ts, parse_counter_kind).is_some()
                        && ts.eat_phrase(&["counter", "for", "each"])
                        && opt(ts, parse_counter_kind).is_some()
                        && ts.eat_phrase(&["counter", "removed", "this", "way"])
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::DamageRule(
                                DamageReplacement::PreventWithRider {
                                    target,
                                    condition: cond,
                                    rider,
                                },
                            ));
                        }
                    }
                    ts.restore(kcp);
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
                            ts.pos = end;
                        }
                    }
                    if ts.eat_period() && !ts.is_eof() {
                        let slice = trim_trailing_period(&ts.tokens[ts.pos..]);
                        let mut sub = ts.sub_stream(slice, crate::v2::ir::ParseStage::Effect);
                        if let Ok(r2) = parse_effect_sentence(&mut sub)
                            && sub.is_eof()
                        {
                            rider.extend(r2);
                            ts.pos = ts.tokens.len();
                        }
                    }
                    if eat_period_eof(ts) {
                        return Ok(if !rider.is_empty() || cond.is_some() {
                            Replacement::DamageRule(DamageReplacement::PreventWithRider {
                                target,
                                condition: cond,
                                rider,
                            })
                        } else {
                            Replacement::DamageRule(DamageReplacement::PreventAll { target })
                        });
                    }
                } else {
                    // Single or multi-sentence instead clause
                    let first_dot = (ts.pos..ts.tokens.len()).find(|&idx| {
                        matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Period)
                    });
                    if let Some(dot) = first_dot
                        && dot + 1 < ts.tokens.len()
                    {
                        let mut head_ts = ts.sub_stream(&ts.tokens[ts.pos..dot], ts.stage);
                        if let Ok(mut instead) = parse_instead_clause(&mut head_ts) {
                            let tail_slice = trim_trailing_period(&ts.tokens[dot + 1..]);
                            let mut tail_ts =
                                ts.sub_stream(tail_slice, crate::v2::ir::ParseStage::Effect);
                            if let Ok(more) = parse_effect_sentence(&mut tail_ts)
                                && tail_ts.is_eof()
                            {
                                instead.extend(more);
                                ts.pos = ts.tokens.len();
                                return Ok(Replacement::EventInstead {
                                    event: TriggerEvent::DealtDamage {
                                        subject: target,
                                        combat_only,
                                    },
                                    instead,
                                });
                            }
                        }
                    }
                    if let Ok(instead) = parse_instead_clause(ts)
                        && eat_period_eof(ts)
                    {
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::DealtDamage {
                                subject: target,
                                combat_only,
                            },
                            instead,
                        });
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 1f0. `If damage dealt to a planeswalker you control of the chosen type would result in all loyalty counters on it being removed, instead all but one of those counters are removed.`
    {
        let cp = ts.checkpoint();
        if eat_rep_seq(
            ts,
            "damage dealt to a planeswalker you control of the chosen type would result in all loyalty counters on it being removed , instead all but one of those counters are removed",
        ) && eat_period_eof(ts)
        {
            return Ok(Replacement::DamageRule(DamageReplacement::PreventAllBut {
                source: None,
                target: None,
                remaining: 1,
            }));
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

    // 1f2. `If you would begin your draw step / your turn while ~ is tapped, [you may] skip that step/turn instead[. If you do, <eff>]` (`Time Vault`)
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
        if ts.eat_phrase(&["you", "would", "begin", "your", "turn", "while"])
            && let Ok(cond) = try_parse(ts, parse_static_condition)
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may", "skip", "that", "turn", "instead"])
        {
            let _ = ts.eat_period();
            if ts.eat_phrase(&["if", "you", "do"]) && ts.eat_comma() {
                let _ = parse_effect_sentence(ts);
            }
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::ConditionalEnters {
                    condition: cond,
                    inner: Box::new(Replacement::PlayerRule {
                        player: PlayerRef::You,
                        rule: PlayerReplacementKind::SkipExtraTurn,
                    }),
                });
            }
        }
        ts.restore(cp);
    }

    // 1f3. Forced discard replacements
    {
        let cp = ts.checkpoint();
        if eat_rep_seq(
            ts,
            "an effect causes you to discard a card , discard it , but you may put it on top of your library instead of into your graveyard",
        ) && eat_period_eof(ts)
        {
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::PlayerAction {
                    player: PlayerRef::You,
                    action: PlayerTriggerKind::CardDiscarded,
                },
                instead: Vec::new(),
            });
        }
        ts.restore(cp);
        if eat_rep_seq(
            ts,
            "a spell or ability an opponent controls causes you to discard",
        ) {
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
            {
                let slice = trim_trailing_period(&ts.tokens[ts.pos..]);
                let mut sub = ts.sub_stream(slice, crate::v2::ir::ParseStage::Effect);
                if let Ok(instead) = parse_effect_sentence(&mut sub)
                    && sub.is_eof()
                {
                    ts.pos = ts.tokens.len();
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::ForcedDiscard {
                            player: PlayerRef::You,
                            subject,
                        },
                        instead,
                    });
                }
                if let Ok(instead) = parse_instead_clause(ts)
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
        }
        ts.restore(cp);
    }

    // 1f4. `If an opponent / you / a player would draw a card except the first one they/you draw in each of their/your draw steps, <instead>` (`Chains of Mephistopheles`)
    {
        let cp = ts.checkpoint();
        let draw_player = if ts.eat_phrase(&[
            "you", "would", "draw", "a", "card", "except", "the", "first", "one", "you", "draw",
            "in", "each", "of", "your", "draw", "steps",
        ]) {
            Some(PlayerRef::You)
        } else if ts.eat_phrase(&[
            "an", "opponent", "would", "draw", "a", "card", "except", "the", "first", "one",
            "they", "draw", "in", "each", "of", "their", "draw", "steps",
        ]) {
            Some(PlayerRef::AnyOpponent)
        } else if ts.eat_phrase(&[
            "a", "player", "would", "draw", "a", "card", "except", "the", "first", "one", "they",
            "draw", "in", "each", "of", "their", "draw", "steps",
        ]) {
            Some(PlayerRef::EachPlayer)
        } else {
            None
        };
        if let Some(player) = draw_player
            && ts.eat_comma()
        {
            let sc_cp = ts.checkpoint();
            if ts.eat_phrase(&["instead", "that", "player", "skips", "that", "draw", "and"])
                && let Ok(mut effs) = parse_effect_sentence(ts)
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    effs.insert(
                        0,
                        crate::v2::ir::Effect::MutatePlayer {
                            player,
                            mutation: crate::v2::ir::PlayerMutation::SkipNextStep(
                                crate::ast::TurnStepAst::DrawStep,
                            ),
                            duration: crate::v2::ir::Duration::Instant,
                        },
                    );
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player,
                            action: PlayerTriggerKind::CardDrawn,
                        },
                        instead: effs,
                    });
                }
            }
            ts.restore(sc_cp);
            if let Ok(instead) = parse_instead_clause(ts) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player,
                            action: PlayerTriggerKind::CardDrawn,
                        },
                        instead,
                    });
                }
            }
            ts.restore(sc_cp);
            // Multi-sentence draw replacement (`Chains of Mephistopheles`)
            if let Some(rep) = try_parse_multi_sentence_instead(
                ts,
                TriggerEvent::PlayerAction {
                    player,
                    action: PlayerTriggerKind::CardDrawn,
                },
            ) {
                return Ok(rep);
            }
        }
        ts.restore(cp);
    }

    // 1f5. `If an opponent would draw two or more cards, <instead>` (`Alms Collector`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "an", "opponent", "would", "draw", "two", "or", "more", "cards",
        ]) && ts.eat_comma()
            && let Ok(instead) = parse_instead_clause(ts)
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::PlayerAction {
                        player: PlayerRef::AnyOpponent,
                        action: PlayerTriggerKind::CardDrawn,
                    },
                    instead,
                });
            }
        }
        ts.restore(cp);
    }

    // 1g. `If <player> would draw a card [during your draw step | while <cond>], <instead>.`
    //     Handles single-sentence, `you may instead ...`, `that player exiles that card face up instead` (`Uba Mask`),
    //     `that player exiles the top card of one of their opponents' libraries face down instead` (`Shared Fate`),
    //     and multi-sentence draw replacements (`Zur's Weirding`, `Abundance`, `Island Sanctuary`, `Parallel Thoughts`, `Archmage Ascension`)!
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "draw", "a", "card"])
        {
            let _ = ts.eat_phrase(&["during", "your", "draw", "step"]);
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
                if ts.eat_any_phrase(&[
                    &[
                        "that", "player", "exiles", "that", "card", "face", "up", "instead",
                    ],
                    &[
                        "that",
                        "player",
                        "exiles",
                        "the",
                        "top",
                        "card",
                        "of",
                        "one",
                        "of",
                        "their",
                        "opponents'",
                        "libraries",
                        "face",
                        "down",
                        "instead",
                    ],
                    &[
                        "you", "may", "instead", "put", "the", "top", "card", "of", "the", "pile",
                        "you", "exiled", "into", "your", "hand",
                    ],
                ]) && eat_period_eof(ts)
                {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player,
                            action: PlayerTriggerKind::CardDrawn,
                        },
                        instead: Vec::new(),
                    });
                }
                ts.restore(dcp);
                // `you may instead <effect_sentence>`
                if ts.eat_phrase(&["you", "may", "instead"]) {
                    let tail_str = ts
                        .source
                        .get(ts.current_span().start..)
                        .unwrap_or("")
                        .trim();
                    if let Ok(effs) = crate::v2::clause::parse_effect_body_str(tail_str, ts.vocab) {
                        ts.pos = ts.tokens.len();
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
                    if eat_rep_seq(
                        ts,
                        "choose land or nonland and reveal cards from the top of your library until you reveal a card of the chosen kind",
                    ) {
                        let _ = ts.eat_period();
                        if eat_rep_seq(
                            ts,
                            "put that card into your hand and put all other cards revealed this way on the bottom of your library in any order",
                        ) && eat_period_eof(ts)
                        {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::PlayerAction {
                                    player,
                                    action: PlayerTriggerKind::CardDrawn,
                                },
                                instead: Vec::new(),
                            });
                        }
                    }
                }
                ts.restore(dcp);
                if eat_rep_seq(
                    ts,
                    "instead you may skip that draw . if you do , until your next turn , you can't be attacked except by creatures with flying and/or islandwalk",
                ) && eat_period_eof(ts)
                {
                    return Ok(Replacement::PlayerRule {
                        player,
                        rule: PlayerReplacementKind::SkipStep {
                            step: crate::ast::TurnStepAst::DrawStep,
                            optional: true,
                        },
                    });
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

    // 1g1. `If <flt> would deal [combat] damage to <tgt>, prevent <N> of that damage.`
    {
        let cp = ts.checkpoint();
        if let Ok(src_flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["would", "deal"])
        {
            let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
            if ts.eat_word("damage") {
                let target = if ts.eat_word("to") {
                    if ts.eat_phrase(&["the", "other", "chosen", "player"]) {
                        let _ =
                            ts.eat_phrase(&["or", "a", "permanent", "that", "player", "controls"]);
                        Some(SubjectRef::Player(PlayerRef::ChosenPlayer))
                    } else if let Some(mut first_tgt) = opt(ts, parse_subject_ref) {
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
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::DamageRule(DamageReplacement::PreventFixed {
                            source: Some(src_flt),
                            target,
                            amount,
                        }));
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 1g2. Mill doubling (`If an opponent/a player would mill one or more cards, they mill twice that many cards instead.`)
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

    // 1g3. Extra turn skip (`If a player would begin an extra turn, that player skips that turn instead.`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "player", "would", "begin", "an", "extra", "turn"])
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "player", "skips", "that", "turn", "instead"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::StepBegins {
                        step: crate::ast::TurnStepAst::Upkeep,
                        owner: crate::v2::ir::StepOwner::EachPlayer,
                    },
                    instead: vec![crate::v2::ir::Effect::MutatePlayer {
                        player: PlayerRef::ThatPlayer,
                        mutation: crate::v2::ir::PlayerMutation::Action(
                            PlayerActionKind::SkipNextTurn,
                        ),
                        duration: crate::v2::ir::Duration::Instant,
                    }],
                });
            }
        }
        ts.restore(cp);
    }

    // 1g5. Lose-the-game replacement (`If you would lose the game, instead ...`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "lose", "the", "game"]) && ts.eat_comma() {
            let lcp = ts.checkpoint();
            if (eat_rep_seq(
                ts,
                "instead exile ~ and your life total becomes equal to your starting life total",
            ) || eat_rep_seq(
                ts,
                "instead shuffle your hand , your graveyard , and all permanents you own into your library , then draw seven cards and your life total becomes #",
            )) && eat_period_eof(ts)
            {
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::PlayerAction {
                        player: PlayerRef::You,
                        action: PlayerTriggerKind::LosesGame,
                    },
                    instead: Vec::new(),
                });
            }
            ts.restore(lcp);
            if let Ok(instead) = parse_instead_clause(ts)
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
        }
        ts.restore(cp);
    }

    // 1g6. Unspent mana conversion (`If you would lose unspent mana, that mana becomes <color|colorless> instead.`)
    {
        let cp = ts.checkpoint();
        if eat_rep_seq(ts, "you would lose unspent mana , that mana becomes") {
            const MANA_WORDS: &[(&str, crate::v2::ir::ManaSymbol)] = &[
                ("colorless", crate::v2::ir::ManaSymbol::Colorless),
                ("white", crate::v2::ir::ManaSymbol::White),
                ("blue", crate::v2::ir::ManaSymbol::Blue),
                ("black", crate::v2::ir::ManaSymbol::Black),
                ("red", crate::v2::ir::ManaSymbol::Red),
                ("green", crate::v2::ir::ManaSymbol::Green),
            ];
            if let Some((_, sym)) = MANA_WORDS.iter().find(|(w, _)| ts.eat_word(w))
                && ts.eat_word("instead")
                && eat_period_eof(ts)
            {
                return Ok(Replacement::Rule(ReplacementRuleKind::ManaOverride(
                    crate::v2::ir::ManaCost {
                        symbols: vec![sym.clone()],
                    },
                )));
            }
        }
        ts.restore(cp);
    }

    // 1g4. Explore / Connive / Assemble replacement (`If <flt> would explore/connive/assemble ..., instead ...`)
    {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter) {
            if ts.eat_phrase(&["would", "explore"]) && ts.eat_comma() && ts.eat_word("instead") {
                if ts.eat_phrase(&["it", "explores"])
                    && ts.eat_comma()
                    && ts.eat_phrase(&["then", "it", "explores", "again"])
                {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::Rule(ReplacementRuleKind::ExploreAgain(filter)));
                    }
                } else {
                    let mut rest = ts.clone();
                    if let Ok(instead) = parse_effect_sentence(&mut rest) {
                        let _ = rest.eat_period();
                        if rest.is_eof() {
                            ts.pos = ts.tokens.len();
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::PlayerObjectEvent {
                                    player: PlayerRef::You,
                                    kind: crate::v2::ir::PlayerObjectTriggerKind::CardPlayed,
                                    filter: Some(filter),
                                },
                                instead,
                            });
                        }
                    }
                }
            }
            ts.restore(cp);
            if let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "connive"])
                && ts.eat_comma()
                && ts.eat_word("instead")
            {
                let mut rest = ts.clone();
                if let Ok(instead) = parse_effect_sentence(&mut rest) {
                    let _ = rest.eat_period();
                    if rest.is_eof() {
                        ts.pos = ts.tokens.len();
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::PlayerObjectEvent {
                                player: PlayerRef::You,
                                kind: crate::v2::ir::PlayerObjectTriggerKind::CardPlayed,
                                filter: Some(filter),
                            },
                            instead,
                        });
                    }
                }
            }
            ts.restore(cp);
            if let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "assemble", "a", "contraption"])
                && ts.eat_comma()
                && ts.eat_phrase(&["it", "assembles", "two", "contraptions", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::Rule(ReplacementRuleKind::ExploreAgain(filter)));
                }
            }
        }
        ts.restore(cp);
    }

    // 1g7. Specialized `If ... would ...` Wave 16 replacements:
    //      - `Firestorm Phoenix` (`If ~ would die, return it to its owner's hand instead. Until that player's next turn, ...`)
    //      - `Necromancer's Magemark` (`If a creature you control that's enchanted would die, return it to its owner's hand instead.`)
    //      - `Valgavoth`, `Abandoned Sarcophagus`, `Wheel of Sun and Moon`, `Sanctifier en-Vec`, `Anafenza`
    //      - `Retriever Phoenix`, `Susan Foreman`, `Twinning Staff`, `Guile`, `Unpredictable Cyclone`,
    //        `Freyalise's Winds`, `Bewitching Leechcraft`, `Damping Sphere`, `Reality Twist`, `Naked Singularity`,
    //        `Zabaz`, `Gemstone Caverns`, `The Valeyard`, `Melira, the Living Cure`, `Ashiok, Wicked Manipulator`,
    //        `Enduring Angel`, `Sheltered Valley`, `Combat Celebrant`, `Land Equilibrium`, `Vedalken Squirrel-Whacker`, `Skyshroud Cutter`
    if let Ok(rep) = parse_if_would_specials(ts) {
        return Ok(rep);
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
    let tail_str = ts
        .source
        .get(ts.tokens[first_dot + 1].span.start..)
        .unwrap_or("")
        .trim();
    if let Ok(more) = crate::v2::clause::parse_effect_body_str(tail_str, ts.vocab) {
        instead.extend(more);
        ts.pos = ts.tokens.len();
        return Some(Replacement::EventInstead { event, instead });
    }
    ts.restore(cp);
    None
}

fn parse_if_would_specials(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    let cp = ts.checkpoint();

    // `If ~ would die, return it to its owner's hand instead. Until that player's next turn, that player plays with that card revealed in their hand and can't play it.`
    if eat_rep_seq(
        ts,
        "~ would die , return it to its owner's hand instead . until that player's next turn , that player plays with that card revealed in their hand and can't play it",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::Dies {
                subject: SubjectRef::SelfObject,
            },
            instead: Vec::new(),
        });
    }
    ts.restore(cp);

    // `If a creature you control that's enchanted would die, return it to its owner's hand instead.`
    if eat_rep_seq(ts, "a creature you control that's enchanted would die ,")
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

    // Graveyard replacement variants
    if ts.eat_any_phrase(&[
        &[
            "a", "card", "you", "didn't", "control", "would", "be", "put", "into",
        ],
        &[
            "a", "card", "that", "has", "a", "cycling", "ability", "would", "be", "put", "into",
        ],
        &["a", "card", "would", "be", "put", "into"],
        &[
            "a",
            "nontoken",
            "creature",
            "an",
            "opponent",
            "owns",
            "would",
            "die",
            "or",
            "a",
            "creature",
            "card",
            "not",
            "on",
            "the",
            "battlefield",
            "would",
            "be",
            "put",
            "into",
        ],
    ]) || eat_rep_seq(
        ts,
        "a black or red permanent , spell , or card not on the battlefield would be put into",
    ) {
        let _ = crate::v2::filter::parse_zone_ref(ts).or_else(|_| {
            if ts.eat_phrase(&["enchanted", "player's", "graveyard"]) {
                Ok(crate::v2::ir::ZoneRef {
                    zone: ZoneKind::Graveyard,
                    owner: crate::v2::ir::ZoneOwner::ThatPlayer,
                })
            } else {
                Err(ts.err("expected graveyard"))
            }
        });
        let _ = ts.eat_phrase(&["from", "anywhere"]);
        let _ = ts.eat_phrase(&["and", "it", "wasn't", "cycled"]);
        if ts.eat_comma() {
            let wcp = ts.checkpoint();
            if eat_rep_seq(
                ts,
                "instead that card is revealed and put on the bottom of that player's library",
            ) && eat_period_eof(ts)
            {
                return Ok(Replacement::Rule(
                    ReplacementRuleKind::LeaveBattlefieldExileInstead,
                ));
            }
            ts.restore(wcp);
            if let Ok(instead) = parse_instead_clause(ts)
                && eat_period_eof(ts)
            {
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::ZoneChange {
                        from: None,
                        to: Some(ZoneKind::Graveyard),
                        subject: SubjectRef::Group(Box::new(ObjectFilter {
                            any_card: true,
                            ..Default::default()
                        })),
                    },
                    instead,
                });
            }
        }
    }
    ts.restore(cp);

    // `If you would learn, you may instead <eff>`
    if eat_rep_seq(ts, "you would learn , you may instead")
        && let Ok(effs) = parse_effect_sentence(ts)
        && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::PlayerAction {
                player: PlayerRef::You,
                action: PlayerTriggerKind::CardDrawn,
            },
            instead: vec![crate::v2::ir::Effect::Optional {
                actor: PlayerRef::You,
                body: effs,
            }],
        });
    }
    ts.restore(cp);

    // `If you would planeswalk, instead ...`
    if eat_rep_seq(
        ts,
        "you would planeswalk , instead look at the top two cards of your planar deck , put one on the bottom of your planar deck and the other on top , then planeswalk",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::PlayerAction {
                player: PlayerRef::You,
                action: PlayerTriggerKind::CardDrawn,
            },
            instead: Vec::new(),
        });
    }
    ts.restore(cp);

    // `If you would copy a spell one or more times, instead copy it that many times plus an additional time. [You may choose new targets for the additional copy.]`
    if eat_rep_seq(
        ts,
        "you would copy a spell one or more times , instead copy it that many times plus an additional time",
    ) {
        let _ = ts.eat_period();
        let _ = eat_rep_seq(ts, "you may choose new targets for the additional copy");
        if eat_period_eof(ts) {
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::SpellCopied {
                    caster: PlayerRef::You,
                    spell: None,
                },
                instead: Vec::new(),
            });
        }
    }
    ts.restore(cp);

    // `If a spell or ability you control would counter a spell, instead exile that spell and you may play that card without paying its mana cost.`
    if eat_rep_seq(
        ts,
        "a spell or ability you control would counter a spell , instead exile that spell and you may play that card without paying its mana cost",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::Rule(
            ReplacementRuleKind::LeaveBattlefieldExileInstead,
        ));
    }
    ts.restore(cp);

    // `If a cycling ability of another nonland card would cause you to draw a card, instead ...`
    if eat_rep_seq(
        ts,
        "a cycling ability of another nonland card would cause you to draw a card , instead exile cards from the top of your library until you exile a card that shares a card type with the cycled card",
    ) {
        let _ = ts.eat_period();
        let _ = eat_rep_seq(ts, "you may cast that card without paying its mana cost");
        let _ = ts.eat_period();
        let _ = eat_rep_seq(
            ts,
            "then put the exiled cards that weren't cast this way on the bottom of your library in a random order",
        );
        if eat_period_eof(ts) {
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::Cycled {
                    player: PlayerRef::You,
                    self_card: false,
                },
                instead: Vec::new(),
            });
        }
    }
    ts.restore(cp);

    // Untap replacement: `If <subj> would untap during its controller's / your untap step, <instead>[. If you do, untap it.]`
    if let Ok(subj) = try_parse(ts, parse_subject_ref)
        && ts.eat_phrase(&["would", "untap", "during"])
        && ts.eat_any_phrase(&[
            &["its", "controller's", "untap", "step"],
            &["your", "untap", "step"],
        ])
        && ts.eat_comma()
    {
        let u_cp = ts.checkpoint();
        if let Some(rep) = try_parse_multi_sentence_instead(
            ts,
            TriggerEvent::SubjectState {
                subject: subj.clone(),
                event: crate::v2::ir::SubjectTriggerKind::BecomesUntapped,
            },
        ) {
            return Ok(rep);
        }
        ts.restore(u_cp);
        if let Ok(instead) = parse_instead_clause(ts)
            && eat_period_eof(ts)
        {
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::SubjectState {
                    subject: subj,
                    event: crate::v2::ir::SubjectTriggerKind::BecomesUntapped,
                },
                instead,
            });
        }
    }
    ts.restore(cp);

    // `If a land is tapped for two or more mana, it produces {C} instead of any other type and amount.`
    if eat_rep_seq(ts, "a land is tapped for two or more mana , it produces")
        && let Some(mc) = ts.eat_mana_cost()
        && eat_rep_seq(ts, "instead of any other type and amount")
        && eat_period_eof(ts)
    {
        return Ok(Replacement::Rule(ReplacementRuleKind::ManaOverride(mc)));
    }
    ts.restore(cp);

    // `If tapped for mana, Plains produce {R}, ... instead of any other type.`
    if eat_rep_seq(ts, "tapped for mana ,") {
        let mut saw_any = false;
        while let Some(w) = ts.peek_word()
            && ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
        {
            ts.next_word();
            if ts.eat_word("produce") && ts.eat_mana_cost().is_some() {
                saw_any = true;
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
            } else {
                break;
            }
        }
        if saw_any
            && ts.eat_phrase(&["instead", "of", "any", "other", "type"])
            && eat_period_eof(ts)
        {
            return Ok(Replacement::Rule(ReplacementRuleKind::ManaOverride(
                crate::v2::ir::ManaCost::default(),
            )));
        }
    }
    ts.restore(cp);

    // `If a modular triggered ability would put one or more +1/+1 counters on <flt>, that many plus one +1/+1 counters are put on it instead.`
    if eat_rep_seq(ts, "a modular triggered ability would put one or more")
        && let Ok(kind) = try_parse(ts, parse_counter_kind)
        && ts.eat_phrase(&["counters", "on"])
        && let Ok(filter) = try_parse(ts, parse_object_filter)
        && eat_rep_seq(ts, ", that many plus")
        && let Some(n) = ts.next_number(false)
        && opt(ts, parse_counter_kind).is_some()
        && eat_rep_seq(ts, "counters are put on it instead")
        && eat_period_eof(ts)
    {
        return Ok(Replacement::CounterRule {
            filter,
            kind: Some(kind),
            add: i16::try_from(n).unwrap_or(1),
            factor: 1,
        });
    }
    ts.restore(cp);

    // `If ~ is in your opening hand and you're not the starting player, ...`
    if eat_rep_seq(
        ts,
        "~ is in your opening hand and you're not the starting player , you may begin the game with ~ on the battlefield with a",
    ) && opt(ts, parse_counter_kind).is_some()
        && ts.eat_phrase(&["counter", "on", "it"])
    {
        let _ = ts.eat_period();
        if ts.eat_phrase(&["if", "you", "do"]) && ts.eat_comma() {
            let _ = parse_effect_sentence(ts);
        }
        if eat_period_eof(ts) {
            return Ok(Replacement::Rule(
                ReplacementRuleKind::OpeningHandOnBattlefield,
            ));
        }
    }
    ts.restore(cp);

    // `If an opponent would face a villainous choice, they face that choice an additional time.`
    if eat_rep_seq(
        ts,
        "an opponent would face a villainous choice , they face that choice an additional time",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::PlayerAction {
                player: PlayerRef::AnyOpponent,
                action: PlayerTriggerKind::CommitCrime,
            },
            instead: Vec::new(),
        });
    }
    ts.restore(cp);

    // `If you would get one or more poison counters, instead you get one poison counter and you can't get additional poison counters this turn.`
    if eat_rep_seq(
        ts,
        "you would get one or more poison counters , instead you get one poison counter and you can't get additional poison counters this turn",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::CounterRule {
            filter: ObjectFilter::default(),
            kind: Some(crate::v2::ir::CounterKind::Poison),
            add: 0,
            factor: 1,
        });
    }
    ts.restore(cp);

    // `If you would pay life while your library has at least that many cards in it, <instead>`
    if eat_rep_seq(
        ts,
        "you would pay life while your library has at least that many cards in it ,",
    ) && let Ok(instead) = parse_instead_clause(ts)
        && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::LifeChanged {
                player: PlayerRef::You,
                gained: false,
            },
            instead,
        });
    }
    ts.restore(cp);

    // `If your life total would be reduced to 0 or less, instead transform ~ and your life total becomes N. [Then if ~ didn't transform this way, you lose the game.]`
    if eat_rep_seq(ts, "your life total would be reduced to")
        && ts.next_number(false) == Some(0)
        && eat_rep_seq(
            ts,
            "or less , instead transform ~ and your life total becomes",
        )
        && let Some(floor) = ts.next_number(false)
    {
        let _ = ts.eat_period();
        let _ = eat_rep_seq(
            ts,
            "then if ~ didn't transform this way , you lose the game",
        );
        if eat_period_eof(ts) {
            return Ok(Replacement::Rule(ReplacementRuleKind::LifeFloor(floor)));
        }
    }
    ts.restore(cp);

    // `If ~ would enter, instead sacrifice each other permanent named ~ you control, then put ~ onto the battlefield.`
    if eat_rep_seq(
        ts,
        "~ would enter , instead sacrifice each other permanent named ~ you control , then put ~ onto the battlefield",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::AsEnters(Vec::new()));
    }
    ts.restore(cp);

    // `If ~ hasn't been exerted this turn, you may exert it as it attacks. When you do, ...`
    if eat_rep_seq(
        ts,
        "~ hasn't been exerted this turn , you may exert it as it attacks",
    ) {
        let _ = ts.eat_period();
        if !ts.is_eof() {
            let tail_str = ts
                .source
                .get(ts.current_span().start..)
                .unwrap_or("")
                .trim();
            let compiled = crate::v2::card::compile_face_v2(
                crate::FaceInput {
                    name: "~",
                    type_line: "Creature",
                    oracle_text: tail_str,
                },
                ts.vocab,
            );
            if compiled.is_ok() {
                ts.pos = ts.tokens.len();
                return Ok(grant_self_as_enters(compiled));
            }
            if eat_rep_seq(
                ts,
                "when you do , untap all other creatures you control and after this phase , there is an additional combat phase",
            ) && eat_period_eof(ts)
            {
                return Ok(Replacement::AsEnters(Vec::new()));
            }
        }
    }
    ts.restore(cp);

    // `If an opponent who controls at least as many lands as you do would put a land onto the battlefield, ...`
    if eat_rep_seq(
        ts,
        "an opponent who controls at least as many lands as you do would put a land onto the battlefield , that player instead puts that land onto the battlefield then sacrifices a land of their choice",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::PlayerAction {
                player: PlayerRef::AnyOpponent,
                action: PlayerTriggerKind::LandPlayed,
            },
            instead: Vec::new(),
        });
    }
    ts.restore(cp);

    // `If you would roll one or more six-sided dice, instead roll them and you may exchange one result with ~'s base power or base toughness.`
    if eat_rep_seq(
        ts,
        "you would roll one or more six-sided dice , instead roll them and you may exchange one result with ~ 's base power or base toughness",
    ) && eat_period_eof(ts)
    {
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::RolledDie {
                player: PlayerRef::You,
                value: 6,
            },
            instead: Vec::new(),
        });
    }
    ts.restore(cp);

    // `If <cond>, rather than pay ~'s mana cost, you may have <eff>.`
    if let Ok(cond) = try_parse(ts, parse_static_condition)
        && ts.eat_comma()
        && ts.eat_phrase(&["rather", "than", "pay"])
        && (eat_rep_seq(ts, "~ 's mana cost")
            || ts.eat_phrase(&["this", "spell's", "mana", "cost"]))
        && eat_rep_seq(ts, ", you may have")
        && let Ok(effs) = parse_effect_sentence(ts)
        && eat_period_eof(ts)
    {
        return Ok(Replacement::ConditionalEnters {
            condition: cond,
            inner: Box::new(Replacement::AsEnters(effs)),
        });
    }
    ts.restore(cp);

    // Table-driven multi-sentence and special replacement rules
    if let Some(rep) = try_parse_special_if_would_table(ts) {
        return Ok(rep);
    }
    ts.restore(cp);

    Err(ts.err("not a special if-would replacement"))
}

#[derive(Clone, Copy)]
enum SpecialIfWouldKind {
    DrawRevealChoice,
    PreventDamageToYou,
    ExileInsteadOfGraveyard,
    PreventCombatWithRider,
    RedirectCreatureToYou,
    RedirectExcessToController,
    EnterReplacementNoop,
    SkipTargetDraw,
    UnpreventableSelfDamage,
}

fn try_parse_special_if_would_table(ts: &mut TokenStream<'_>) -> Option<Replacement> {
    const RULES: &[(&str, SpecialIfWouldKind)] = &[
        (
            "a player would draw a card , they reveal it instead . then any other player may pay # life . if a player does , put that card into its owner's graveyard . otherwise , that player draws a card",
            SpecialIfWouldKind::DrawRevealChoice,
        ),
        (
            "damage would be dealt to you , exile that many cards from your graveyard instead . if you can't , you lose the game",
            SpecialIfWouldKind::PreventDamageToYou,
        ),
        (
            "a nontoken creature an opponent owns would die or a creature card not on the battlefield would be put into an opponent's graveyard , exile that card instead",
            SpecialIfWouldKind::ExileInsteadOfGraveyard,
        ),
        (
            "~ would deal combat damage to a player , prevent that damage . ~ 's controller and that player each draw half that many cards , rounded down",
            SpecialIfWouldKind::PreventCombatWithRider,
        ),
        (
            "damage would be dealt to any creature , you may have that damage dealt to you instead",
            SpecialIfWouldKind::RedirectCreatureToYou,
        ),
        (
            "excess damage would be dealt to that creature this way , instead that damage is dealt to that creature's controller and you convert ~",
            SpecialIfWouldKind::RedirectExcessToController,
        ),
        (
            "one or more creatures would enter from exile or after being cast from exile , their owners shuffle them into their libraries instead",
            SpecialIfWouldKind::EnterReplacementNoop,
        ),
        (
            "target player would draw a card , instead that player skips that draw and you draw a card",
            SpecialIfWouldKind::SkipTargetDraw,
        ),
        (
            "a creature would enter the battlefield under an opponent's control this turn , it enters under your control instead",
            SpecialIfWouldKind::EnterReplacementNoop,
        ),
        (
            "a creature dealt damage by the targeted creature would die this turn , exile that creature instead",
            SpecialIfWouldKind::ExileInsteadOfGraveyard,
        ),
        (
            "~ would deal damage to a creature , that damage can't be prevented or dealt instead to another permanent or player",
            SpecialIfWouldKind::UnpreventableSelfDamage,
        ),
    ];
    for &(seq, kind) in RULES {
        let cp = ts.checkpoint();
        if eat_rep_seq(ts, seq) {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Some(match kind {
                    SpecialIfWouldKind::DrawRevealChoice => Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player: PlayerRef::EachPlayer,
                            action: PlayerTriggerKind::CardDrawn,
                        },
                        instead: Vec::new(),
                    },
                    SpecialIfWouldKind::PreventDamageToYou => {
                        Replacement::DamageRule(DamageReplacement::PreventAll {
                            target: SubjectRef::Player(PlayerRef::You),
                        })
                    }
                    SpecialIfWouldKind::ExileInsteadOfGraveyard => {
                        Replacement::Rule(ReplacementRuleKind::LeaveBattlefieldExileInstead)
                    }
                    SpecialIfWouldKind::PreventCombatWithRider => {
                        Replacement::DamageRule(DamageReplacement::PreventWithRider {
                            target: SubjectRef::Player(PlayerRef::EachPlayer),
                            condition: None,
                            rider: Vec::new(),
                        })
                    }
                    SpecialIfWouldKind::RedirectCreatureToYou => {
                        Replacement::DamageRule(DamageReplacement::RedirectAll {
                            from: SubjectRef::Group(Box::new(ObjectFilter {
                                any_types: crate::ast::CardTypeMask::CREATURE,
                                ..Default::default()
                            })),
                            to: SubjectRef::Player(PlayerRef::You),
                        })
                    }
                    SpecialIfWouldKind::RedirectExcessToController => {
                        Replacement::DamageRule(DamageReplacement::RedirectAll {
                            from: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatCreature),
                            to: SubjectRef::Player(PlayerRef::ThatPlayer),
                        })
                    }
                    SpecialIfWouldKind::EnterReplacementNoop => Replacement::AsEnters(Vec::new()),
                    SpecialIfWouldKind::SkipTargetDraw => Replacement::PlayerRule {
                        player: PlayerRef::TargetPlayer,
                        rule: crate::v2::ir::PlayerReplacementKind::SkipStep {
                            step: crate::ast::TurnStepAst::DrawStep,
                            optional: false,
                        },
                    },
                    SpecialIfWouldKind::UnpreventableSelfDamage => Replacement::DamageRule(
                        DamageReplacement::UnpreventableBySource(SubjectRef::SelfObject),
                    ),
                });
            }
        }
        ts.restore(cp);
    }
    None
}
