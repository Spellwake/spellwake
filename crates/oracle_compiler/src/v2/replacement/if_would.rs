//! `If <event> would ...` replacement effect parser for `v2`.

use crate::v2::clause::parse_effect_sentence;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    DamageReplacement, ObjectFilter, PlayerActionKind, PlayerRef, PlayerReplacementKind,
    PlayerTriggerKind, Replacement, ReplacementRuleKind, SubjectRef, TokenReplacementKind,
    TriggerEvent, ValueExpr, ZoneKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

use super::{lookup_predefined_token, parse_instead_clause};

pub(super) fn parse_if_would_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 1a0. Leyline opening hand (`If ~ is in your opening hand, you may begin the game with it on the battlefield.`)
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_phrase(&["is", "in", "your", "opening", "hand"])
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "you",
                "may",
                "begin",
                "the",
                "game",
                "with",
                "it",
                "on",
                "the",
                "battlefield",
            ])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(
                    ReplacementRuleKind::OpeningHandOnBattlefield,
                ));
            }
        }
        ts.restore(cp);
    }

    // 1a0c2. `If ~ would enter and it wasn't cast or no mana was spent to cast it, exile it instead.` (`Primeval Spawn`)
    //        `If ~ would be dealt damage, prevent that damage and remove a +1/+1 counter from it.` (`Unbreathing Horde`)
    //        `If ~ would deal combat damage to a player, prevent that damage. <rider>` (`Sokrates, Athenian Teacher`)
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_word("would") {
            let wcp = ts.checkpoint();
            if ts.eat_phrase(&[
                "enter", "and", "it", "wasn't", "cast", "or", "no", "mana", "was", "spent", "to",
                "cast", "it",
            ]) && ts.eat_comma()
                && ts.eat_phrase(&["exile", "it", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ConditionalEnters {
                        condition: crate::v2::ir::ConditionExpr::SubjectCheck {
                            subject: Box::new(SubjectRef::SelfObject),
                            kind: crate::v2::ir::SubjectConditionKind::NotCastOrNoManaSpent,
                        },
                        inner: Box::new(Replacement::Rule(
                            ReplacementRuleKind::LeaveBattlefieldExileInstead,
                        )),
                    });
                }
            }
            ts.restore(wcp);
            if ts.eat_phrase(&["be", "dealt", "damage"])
                && ts.eat_comma()
                && ts.eat_phrase(&["prevent", "that", "damage", "and"])
                && let Ok(rider) = try_parse(ts, parse_effect_sentence)
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::DamageRule(
                        DamageReplacement::PreventWithRider {
                            target: SubjectRef::SelfObject,
                            condition: None,
                            rider,
                        },
                    ));
                }
            }
            ts.restore(wcp);
            if ts.eat_phrase(&["deal", "combat", "damage", "to"])
                && let Ok(target) = try_parse(ts, parse_subject_ref)
                && ts.eat_comma()
                && ts.eat_phrase(&["prevent", "that", "damage"])
                && ts.eat_period()
                && let Ok(rider) = try_parse(ts, parse_effect_sentence)
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::DamageRule(
                        DamageReplacement::PreventWithRider {
                            target,
                            condition: None,
                            rider,
                        },
                    ));
                }
            }
        }
        ts.restore(cp);
    }

    // 1a0c3. `If a source would deal damage to ~, prevent that damage. <rider>` (`Swans of Bryn Argoll`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "source", "would", "deal", "damage", "to"])
            && ts.eat_tilde()
            && ts.eat_comma()
            && ts.eat_phrase(&["prevent", "that", "damage"])
            && ts.eat_period()
            && let Ok(rider) = try_parse(ts, parse_effect_sentence)
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::DamageRule(
                    DamageReplacement::PreventWithRider {
                        target: SubjectRef::SelfObject,
                        condition: None,
                        rider,
                    },
                ));
            }
        }
        ts.restore(cp);
    }

    // 1a0c. `If ~ would leave the battlefield, exile it instead of putting it anywhere else.` (`Llanowar Greenwidow` + Unearth/Flashback/Disturb/Escape)
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_phrase(&["would", "leave", "the", "battlefield"])
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "exile", "it", "instead", "of", "putting", "it", "anywhere", "else",
            ])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(
                    ReplacementRuleKind::LeaveBattlefieldExileInstead,
                ));
            }
        }
        ts.restore(cp);
    }

    // 1a0d. `If an opponent would lose life [during your turn], they lose twice that much life instead.` (`Bloodletter of Aclazotz`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "lose", "life"])
        {
            let during_your_turn = ts.eat_phrase(&["during", "your", "turn"]);
            if ts.eat_comma()
                && (ts.eat_phrase(&["they", "lose", "twice", "that", "much", "life", "instead"])
                    || ts.eat_phrase(&[
                        "that", "player", "loses", "twice", "that", "much", "life", "instead",
                    ]))
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::PlayerRule {
                        player,
                        rule: PlayerReplacementKind::LoseLifeMultiply {
                            factor: 2,
                            during_your_turn,
                        },
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1a0d2. `If an opponent / a player would gain life, that player loses that much life instead.` (`Tainted Remedy`, `Plague Drone`)
    //        `If an opponent / a player would begin an extra turn, that player skips that turn instead.` (`Stranglehold`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref) {
            if ts.eat_phrase(&["would", "gain", "life"])
                && ts.eat_comma()
                && (ts.eat_phrase(&["that", "player", "loses", "that", "much", "life", "instead"])
                    || ts.eat_phrase(&["they", "lose", "that", "much", "life", "instead"]))
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::LoseLifeInsteadOfGain(player));
                }
            }
            ts.restore(cp);
            if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
                && ts.eat_phrase(&["would", "begin", "an", "extra", "turn"])
                && ts.eat_comma()
                && (ts.eat_phrase(&["that", "player", "skips", "that", "turn", "instead"])
                    || ts.eat_phrase(&["they", "skip", "that", "turn", "instead"]))
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::PlayerRule {
                        player,
                        rule: PlayerReplacementKind::SkipExtraTurn,
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1a0f2. `If you tap <flt> for mana, it produces twice / three times as much of that mana instead.` (`Nyxbloom Ancient`, `Mana Reflection`, `Virtue of Strength`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && (ts.eat_word("tap") || ts.eat_word("taps"))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["for", "mana"])
            && ts.eat_comma()
            && ts.eat_phrase(&["it", "produces"])
        {
            let mult = if ts.eat_word("twice") {
                Some(2i32)
            } else if ts.eat_phrase(&["three", "times"]) {
                Some(3i32)
            } else {
                None
            };
            if let Some(m) = mult
                && ts.eat_phrase(&["as", "much", "of", "that", "mana", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerObjectEvent {
                            player,
                            kind: crate::v2::ir::PlayerObjectTriggerKind::TappedForMana,
                            filter: Some(flt),
                        },
                        instead: vec![crate::v2::ir::Effect::MutatePlayer {
                            player,
                            mutation: crate::v2::ir::PlayerMutation::AddMana(
                                crate::v2::ir::ManaSpec::AnyColor {
                                    amount: ValueExpr::Const(m),
                                    single_color: false,
                                },
                            ),
                            duration: crate::v2::ir::Duration::Instant,
                        }],
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1a0f3. `If <player> would mill one or more cards, <player> mill(s) that many cards plus <N> instead.` (`The Water Crystal`)
    //        `If <player> would search a library, <player> searches the top <N> cards of that library instead.` (`Aven Mindcensor`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_word("would")
        {
            let m_cp = ts.checkpoint();
            if ts.eat_phrase(&["mill", "one", "or", "more", "cards"])
                && ts.eat_comma()
                && try_parse(ts, crate::v2::filter::parse_player_ref).is_ok()
                && (ts.eat_word("mill") || ts.eat_word("mills"))
                && ts.eat_phrase(&["that", "many", "cards", "plus"])
                && let Ok(extra) = try_parse(ts, parse_value_expr)
                && ts.eat_word("instead")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerObjectEvent {
                            player,
                            kind: crate::v2::ir::PlayerObjectTriggerKind::MillsCard,
                            filter: None,
                        },
                        instead: vec![crate::v2::ir::Effect::ZoneOp {
                            actor: player,
                            op: crate::v2::ir::ZoneOpKind::Mill,
                            subject: SubjectRef::Player(player),
                            from: vec![crate::v2::ir::ZoneRef {
                                zone: ZoneKind::Library,
                                owner: crate::v2::ir::ZoneOwner::ThatPlayer,
                            }],
                            to: None,
                            modifiers: crate::v2::ir::EntryModifiers::default(),
                            except: None,
                            count: Some(crate::v2::ir::CountSpec {
                                count: ValueExpr::Add(
                                    Box::new(ValueExpr::PriorActionCount(
                                        crate::v2::ir::PriorActionKind::ThatMany,
                                    )),
                                    Box::new(extra),
                                ),
                                up_to: false,
                                any_number: false,
                            }),
                        }],
                    });
                }
            }
            ts.restore(m_cp);
            if ts.eat_phrase(&["search", "a", "library"])
                && ts.eat_comma()
                && try_parse(ts, crate::v2::filter::parse_player_ref).is_ok()
                && (ts.eat_word("search") || ts.eat_word("searches"))
                && ts.eat_phrase(&["the", "top"])
                && let Ok(depth) = try_parse(ts, parse_value_expr)
                && (ts.eat_word("cards") || ts.eat_word("card"))
                && ts.eat_phrase(&["of", "that", "library", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player,
                            action: PlayerTriggerKind::SearchesLibrary,
                        },
                        instead: vec![crate::v2::ir::Effect::ZoneOp {
                            actor: player,
                            op: crate::v2::ir::ZoneOpKind::LookTop { reveal: false },
                            subject: SubjectRef::Player(player),
                            from: vec![crate::v2::ir::ZoneRef {
                                zone: ZoneKind::Library,
                                owner: crate::v2::ir::ZoneOwner::ThatPlayer,
                            }],
                            to: None,
                            modifiers: crate::v2::ir::EntryModifiers::default(),
                            except: None,
                            count: Some(crate::v2::ir::CountSpec {
                                count: depth,
                                up_to: false,
                                any_number: false,
                            }),
                        }],
                    });
                }
            }
            ts.restore(m_cp);
            if ts.eat_phrase(&["roll", "one", "or", "more"])
                && {
                    let _ = ts.eat_word("planar");
                    ts.eat_word("dice")
                }
                && ts.eat_comma()
                && ts.eat_phrase(&["instead", "roll", "that", "many"])
                && {
                    let _ = ts.eat_word("planar");
                    ts.eat_phrase(&["dice", "plus"])
                }
                && let Ok(extra) = try_parse(ts, parse_value_expr)
                && ts.eat_phrase(&["and", "ignore"])
                && (ts.eat_phrase(&["the", "lowest", "roll"]) || ts.eat_word("one"))
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::RolledDie { player, value: 0 },
                        instead: vec![crate::v2::ir::Effect::MutatePlayer {
                            player,
                            mutation: crate::v2::ir::PlayerMutation::RollDie {
                                sides: 20,
                                count: match extra {
                                    ValueExpr::Const(n) => u8::try_from(n.max(1) + 1).unwrap_or(2),
                                    _ => 2,
                                },
                            },
                            duration: crate::v2::ir::Duration::Instant,
                        }],
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1a0f. `If a land is tapped for mana, it produces {M} instead of any other type and amount.` (`Contamination`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "land", "is", "tapped", "for", "mana"])
            && ts.eat_comma()
            && ts.eat_phrase(&["it", "produces"])
            && let Some(mc) = ts.eat_mana_cost()
            && ts.eat_phrase(&["instead", "of", "any", "other", "type", "and", "amount"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(ReplacementRuleKind::ManaOverride(mc)));
            }
        }
        ts.restore(cp);
    }

    // 1a0e. `If a player would draw a card, that player skips that draw instead.` (`Possessed Portal`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "draw", "a", "card"])
        {
            let _ = ts.eat_phrase(&["while", "your", "library", "has", "no", "cards", "in", "it"]);
            if ts.eat_comma()
                && (ts.eat_phrase(&["that", "player", "skips", "that", "draw", "instead"])
                    || ts.eat_phrase(&["skip", "that", "draw", "instead"]))
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::PlayerRule {
                        player,
                        rule: PlayerReplacementKind::SkipStep {
                            step: crate::ast::TurnStepAst::DrawStep,
                            optional: false,
                        },
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1a0b. Daybound enter (`If it's neither day nor night, it becomes day as ~ enters.`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["it's", "neither", "day", "nor", "night"])
            && ts.eat_comma()
            && ts.eat_phrase(&["it", "becomes", "day", "as"])
            && ts.eat_tilde()
            && ts.eat_word("enters")
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(ReplacementRuleKind::BecomesDayOnEnter));
            }
        }
        ts.restore(cp);
    }

    // 1a. `If an effect would create one or more tokens under your control, it creates twice that many of those tokens instead.`
    //     OR `If one or more tokens would be created under your control, twice that many of those tokens are created instead.`
    //     OR `If one or more tokens would be created under your control, those tokens plus [an additional | a] <Predefined> token are created instead.`
    //     OR `If you would create one or more <Predefined> tokens, instead create those tokens plus an additional <Predefined> token.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "an", "effect", "would", "create", "one", "or", "more", "tokens", "under", "your",
            "control",
        ]) && ts.eat_comma()
            && ts.eat_phrase(&[
                "it", "creates", "twice", "that", "many", "of", "those", "tokens", "instead",
            ])
        {
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::TokenRule {
                controller: PlayerRef::You,
                filter: None,
                rule: TokenReplacementKind::Multiply(2),
            });
        }
        ts.restore(cp);
        if ts.eat_phrase(&["one", "or", "more"]) && {
            let _ =
                ts.eat_word("creature") || ts.eat_word("artifact") || ts.eat_word("enchantment");
            ts.eat_phrase(&["tokens", "would", "be", "created"])
        } {
            let controller = if ts.eat_phrase(&["under", "your", "control"]) {
                PlayerRef::You
            } else {
                PlayerRef::EachPlayer
            };
            if ts.eat_comma() {
                if ts.eat_phrase(&[
                    "twice", "that", "many", "of", "those", "tokens", "are", "created", "instead",
                ]) {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::TokenRule {
                        controller,
                        filter: None,
                        rule: TokenReplacementKind::Multiply(2),
                    });
                }
                if ts.eat_phrase(&[
                    "three", "times", "that", "many", "of", "those", "tokens", "are", "created",
                    "instead",
                ]) {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::TokenRule {
                        controller,
                        filter: None,
                        rule: TokenReplacementKind::Multiply(3),
                    });
                }
                {
                    let dcp = ts.checkpoint();
                    if ts.peek_phrase(&["that", "many"])
                        && let Ok((_, spec, _)) =
                            try_parse(ts, crate::v2::clause::tokens::parse_token_creation)
                        && ts.eat_phrase(&["are", "created", "instead"])
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::TokenRule {
                                controller,
                                filter: None,
                                rule: TokenReplacementKind::Replace(Box::new(spec)),
                            });
                        }
                    }
                    ts.restore(dcp);
                }
                if ts.eat_phrase(&["those", "tokens", "plus"]) {
                    let t_cp = ts.checkpoint();
                    let _ = ts.eat_phrase(&["an", "additional"])
                        || ts.eat_word("a")
                        || ts.eat_word("an");
                    if let Some(w) = ts.peek_word()
                        && let Some(pt) = lookup_predefined_token(&w.to_ascii_lowercase())
                    {
                        ts.next_word();
                        if (ts.eat_word("token") || ts.eat_word("tokens"))
                            && ts.eat_phrase(&["are", "created", "instead"])
                        {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::TokenRule {
                                    controller,
                                    filter: None,
                                    rule: TokenReplacementKind::Bonus {
                                        spec: Box::new(crate::v2::ir::TokenSpec::Predefined(pt)),
                                        count: ValueExpr::Const(1),
                                    },
                                });
                            }
                        }
                    }
                    ts.restore(t_cp);
                    if let Ok((bonus_count, spec, _)) =
                        try_parse(ts, crate::v2::clause::tokens::parse_token_creation)
                        && ts.eat_phrase(&["are", "created", "instead"])
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::TokenRule {
                                controller,
                                filter: None,
                                rule: TokenReplacementKind::Bonus {
                                    spec: Box::new(spec),
                                    count: bonus_count,
                                },
                            });
                        }
                    }
                }
            }
        }
        ts.restore(cp);
        if ts.eat_phrase(&["you", "would", "create", "one", "or", "more"]) {
            let mut filter: Option<ObjectFilter> = None;
            if let Some(w) = ts.peek_word()
                && let Some(pt) = lookup_predefined_token(&w.to_ascii_lowercase())
            {
                ts.next_word();
                filter = Some(ObjectFilter {
                    token: Some(true),
                    subtypes: vec![format!("{pt:?}")],
                    ..Default::default()
                });
            } else if ts.eat_word("artifact") {
                filter = Some(ObjectFilter {
                    token: Some(true),
                    any_types: crate::ast::CardTypeMask::ARTIFACT,
                    ..Default::default()
                });
            } else if ts.eat_word("creature") {
                filter = Some(ObjectFilter {
                    token: Some(true),
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    ..Default::default()
                });
            } else if ts.eat_word("enchantment") {
                filter = Some(ObjectFilter {
                    token: Some(true),
                    any_types: crate::ast::CardTypeMask::ENCHANTMENT,
                    ..Default::default()
                });
            }
            if (ts.eat_word("tokens") || ts.eat_word("token")) && ts.eat_comma() {
                let _ = ts.eat_phrase(&["you", "may"]);
                if ts.eat_phrase(&["instead", "create", "those", "tokens", "plus"]) {
                    if let Ok((bonus_count, spec, _)) =
                        try_parse(ts, crate::v2::clause::tokens::parse_token_creation)
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::TokenRule {
                                controller: PlayerRef::You,
                                filter,
                                rule: TokenReplacementKind::Bonus {
                                    spec: Box::new(spec),
                                    count: bonus_count,
                                },
                            });
                        }
                    }
                } else if ts.eat_phrase(&["instead", "create"])
                    && let Ok((_, spec1, _)) =
                        try_parse(ts, crate::v2::clause::tokens::parse_token_creation)
                {
                    let mut rules = vec![Replacement::TokenRule {
                        controller: PlayerRef::You,
                        filter: filter.clone(),
                        rule: TokenReplacementKind::Replace(Box::new(spec1)),
                    }];
                    if ts.eat_word("or")
                        && let Ok((_, spec2, _)) =
                            try_parse(ts, crate::v2::clause::tokens::parse_token_creation)
                    {
                        rules.push(Replacement::TokenRule {
                            controller: PlayerRef::You,
                            filter,
                            rule: TokenReplacementKind::Replace(Box::new(spec2)),
                        });
                    }
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(if rules.len() == 1 {
                            rules.remove(0)
                        } else {
                            Replacement::Compound(rules)
                        });
                    }
                }
            }
        }
        ts.restore(cp);
    }
    // 1b0. `If you would put one or more counters on <flt> [or on yourself | or player], put [that many plus <N> | twice that many] of each of those kinds of counters on that permanent [or player] instead.`
    // (`Doc Samson`, `Innkeeper's Talent`, `Lae'zel, Vlaakith's Champion`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "put", "one", "or", "more", "counters", "on"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            let _ = ts.eat_phrase(&["or", "on", "yourself"]) || ts.eat_phrase(&["or", "player"]);
            if ts.eat_comma() && ts.eat_word("put") {
                let (add, factor) = if ts.eat_phrase(&["twice", "that", "many"]) {
                    (0i16, 2u8)
                } else if ts.eat_phrase(&["that", "many", "plus"])
                    && let Some(n) = ts.next_number(false)
                {
                    (i16::try_from(n).unwrap_or(1), 1u8)
                } else {
                    ts.restore(cp);
                    (0, 0)
                };
                if (add != 0 || factor != 0)
                    && (ts.eat_phrase(&["of", "each", "of", "those", "kinds", "of", "counters"])
                        || ts.eat_phrase(&["of", "those", "counters"]))
                    && ts.eat_phrase(&["on", "that", "permanent"])
                {
                    let _ = ts.eat_phrase(&["or", "player"]);
                    if ts.eat_word("instead") {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::CounterRule {
                                filter: flt,
                                kind: None,
                                add,
                                factor,
                            });
                        }
                    }
                }
            }
        }
        ts.restore(cp);
    }
    // 1b. `If an effect would put one or more counters on <flt>, it puts twice that many of those counters on that permanent instead.`
    if ts.eat_phrase(&[
        "an", "effect", "would", "put", "one", "or", "more", "counters", "on",
    ]) {
        let flt = parse_object_filter(ts)?;
        if ts.eat_comma()
            && ts.eat_phrase(&[
                "it",
                "puts",
                "twice",
                "that",
                "many",
                "of",
                "those",
                "counters",
                "on",
                "that",
                "permanent",
                "instead",
            ])
        {
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::CounterRule {
                filter: flt,
                kind: None,
                add: 0,
                factor: 2,
            });
        }
        return Err(ts.err("unsupported counter-doubling replacement clause"));
    }

    // 1c0. `If ~ is in your opening hand, you may begin the game with ~ on the battlefield` (`Quicksilver, Brash Blur`)
    {
        let ocp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_phrase(&["is", "in", "your", "opening", "hand"])
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may", "begin", "the", "game", "with"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["on", "the", "battlefield"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(
                    crate::v2::ir::ReplacementRuleKind::OpeningHandOnBattlefield,
                ));
            }
        }
        ts.restore(ocp);
    }
    // 1c0b. `If [a player | that player [or that planeswalker's controller]] would gain life [this turn], that player gains no life instead.` (`Sulfuric Vortex`, `Flames of the Blood Hand`)
    {
        let ncp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "gain", "life"])
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_comma()
                && ts.eat_phrase(&["that", "player", "gains", "no", "life", "instead"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::PlayerRule {
                        player,
                        rule: PlayerReplacementKind::ModifyLifeGain { add: 0, factor: 0 },
                    });
                }
            }
        }
        ts.restore(ncp);
    }
    // 1c. `If you would gain life [while <cond>], you gain that much life plus <N> / twice that much life instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "gain", "life"]) {
            let while_cond = if ts.eat_word("while") {
                opt(ts, parse_condition)
            } else {
                None
            };
            if ts.eat_comma() && ts.eat_phrase(&["you", "gain"]) {
                if ts.eat_phrase(&["twice", "that", "much", "life", "instead"]) {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    let rep = Replacement::PlayerRule {
                        player: PlayerRef::You,
                        rule: PlayerReplacementKind::ModifyLifeGain { add: 0, factor: 2 },
                    };
                    return Ok(if let Some(condition) = while_cond {
                        Replacement::ConditionalEnters {
                            condition,
                            inner: Box::new(rep),
                        }
                    } else {
                        rep
                    });
                }
                if ts.eat_phrase(&["that", "much", "life", "plus"])
                    && let Some(n) = ts.next_number(false)
                    && ts.eat_word("instead")
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    let rep = Replacement::PlayerRule {
                        player: PlayerRef::You,
                        rule: PlayerReplacementKind::ModifyLifeGain {
                            add: i16::try_from(n).unwrap_or(1),
                            factor: 1,
                        },
                    };
                    return Ok(if let Some(condition) = while_cond {
                        Replacement::ConditionalEnters {
                            condition,
                            inner: Box::new(rep),
                        }
                    } else {
                        rep
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1d. `If one or more [<kind>] counters would be put on [a/an] <flt>, that many plus <N> / twice that many [of those | <kind>] counters are put on it/that permanent/that creature instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"]) {
            let kind = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["would", "be", "put", "on"])
            {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                let flt_opt = if ts.eat_tilde() {
                    Some(ObjectFilter::default())
                } else {
                    try_parse(ts, parse_object_filter).ok()
                };
                if let Some(flt) = flt_opt
                    && ts.eat_comma()
                {
                    if ts.eat_phrase(&["twice", "that", "many"]) {
                        let _ = ts
                            .eat_phrase(&["of", "each", "of", "those", "kinds", "of", "counters"])
                            || ts.eat_phrase(&["of", "those", "counters"])
                            || (opt(ts, parse_counter_kind).is_some()
                                && (ts.eat_word("counters") || ts.eat_word("counter")));
                        if ts.eat_phrase(&["are", "put", "on", "it", "instead"])
                            || ts.eat_phrase(&["are", "put", "on", "that", "permanent", "instead"])
                            || ts.eat_phrase(&["are", "put", "on", "that", "creature", "instead"])
                        {
                            let _ = ts.eat_period();
                            ts.expect_eof()?;
                            return Ok(Replacement::CounterRule {
                                filter: flt,
                                kind: None,
                                add: 0,
                                factor: 2,
                            });
                        }
                    } else if ts.eat_phrase(&["that", "many"]) {
                        {
                            let kcp = ts.checkpoint();
                            if !ts.peek_word().is_some_and(|w| {
                                w.eq_ignore_ascii_case("plus") || w.eq_ignore_ascii_case("minus")
                            }) && opt(ts, parse_counter_kind).is_some()
                                && (ts.eat_word("counters") || ts.eat_word("counter"))
                            {
                                // consumed optional `<kind> counter(s)` before `minus <N>` (`Vizier of Remedies`)
                            } else {
                                ts.restore(kcp);
                            }
                        }
                        let sign: i16 = if ts.eat_word("plus") {
                            1
                        } else if ts.eat_word("minus") {
                            -1
                        } else {
                            0
                        };
                        if sign != 0
                            && let Some(n) = ts.next_number(false)
                        {
                            if !ts.eat_phrase(&[
                                "of", "each", "of", "those", "kinds", "of", "counters",
                            ]) && !ts.eat_phrase(&["of", "those", "counters"])
                            {
                                let k2cp = ts.checkpoint();
                                if !(opt(ts, parse_counter_kind).is_some()
                                    && (ts.eat_word("counters") || ts.eat_word("counter")))
                                {
                                    ts.restore(k2cp);
                                }
                            }
                            if ts.eat_phrase(&["are", "put", "on", "it", "instead"])
                                || ts.eat_phrase(&[
                                    "are",
                                    "put",
                                    "on",
                                    "that",
                                    "permanent",
                                    "instead",
                                ])
                                || ts.eat_phrase(&[
                                    "are", "put", "on", "that", "creature", "instead",
                                ])
                            {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(Replacement::CounterRule {
                                    filter: flt,
                                    kind,
                                    add: sign * i16::try_from(n).unwrap_or(1),
                                    factor: 1,
                                });
                            }
                        }
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 1e. `If <source> would deal [combat|noncombat] damage [to <tgt>], <prevent | modify | instead>`
    //     Supports `a [another] [color] source [you control]`, `enchanted creature`, `~`, or any `ObjectFilter`!
    {
        let cp = ts.checkpoint();
        let mut src_flt_opt: Option<ObjectFilter> = None;
        {
            let scp = ts.checkpoint();
            let mut src_flt = ObjectFilter::default();
            let mut matched_src_word = false;
            let _ = ts.eat_word("a") || ts.eat_word("an");
            if ts.eat_word("another") {
                src_flt.flags = src_flt
                    .flags
                    .union(crate::v2::ir::FilterFlags::EXCLUDE_SELF);
            }
            if let Some(w) = ts.peek_word()
                && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                src_flt.any_colors = src_flt.any_colors.union(c);
            }
            if ts.eat_word("source") {
                matched_src_word = true;
                if ts.eat_phrase(&["you", "control"]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::You);
                } else if ts.eat_phrase(&["an", "opponent", "controls"]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::Opponent);
                }
            } else if ts.eat_phrase(&["instant", "or", "sorcery", "source"]) {
                matched_src_word = true;
                src_flt.any_types =
                    crate::ast::CardTypeMask::INSTANT.union(crate::ast::CardTypeMask::SORCERY);
                if ts.eat_phrase(&["you", "control"]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::You);
                } else if ts.eat_phrase(&["an", "opponent", "controls"]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::Opponent);
                }
            }
            if matched_src_word {
                src_flt_opt = Some(src_flt);
            } else {
                ts.restore(scp);
                if ts.eat_tilde() || ts.eat_phrase(&["enchanted", "creature"]) {
                    src_flt_opt = Some(ObjectFilter::default());
                } else if let Ok(f) = try_parse(ts, parse_object_filter) {
                    src_flt_opt = Some(f);
                }
            }
        }
        if let Some(src_flt) = src_flt_opt
            && ts.eat_phrase(&["would", "deal"])
        {
            let combat_only = ts.eat_word("combat");
            let _ = ts.eat_word("noncombat");
            let mut max_dmg_prevent: Option<i32> = None;
            let min_dmg = {
                let dcp = ts.checkpoint();
                if let Some(n) = ts.next_number(false) {
                    if ts.eat_phrase(&["or", "more"]) {
                        Some(n)
                    } else if ts.eat_phrase(&["or", "less"]) {
                        max_dmg_prevent = Some(n);
                        None
                    } else {
                        ts.restore(dcp);
                        None
                    }
                } else {
                    ts.restore(dcp);
                    None
                }
            };
            if ts.eat_word("damage") {
                let _ = ts.eat_phrase(&["this", "turn"]);
                let target = if ts.eat_word("to") {
                    opt(ts, parse_subject_ref)
                } else {
                    None
                };
                let _ = ts.eat_phrase(&["this", "turn"]);
                let while_cond = if ts.eat_word("while") {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                if ts.eat_comma() {
                    // `prevent that damage [. <rider>]` (`Callous Giant`, `Swans of Bryn Argoll`)
                    {
                        let pcp = ts.checkpoint();
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            let mut rider = Vec::new();
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
                                let mut sub = ts.sub_stream(
                                    &ts.tokens[start..end],
                                    crate::v2::ir::ParseStage::Effect,
                                );
                                if let Ok(r) = parse_effect_sentence(&mut sub)
                                    && sub.is_eof()
                                {
                                    rider.extend(r);
                                    ts.pos = ts.tokens.len();
                                }
                            }
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                if let Some(cap) = max_dmg_prevent {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::PreventFixed {
                                            source: Some(src_flt),
                                            target,
                                            amount: ValueExpr::Const(cap),
                                        },
                                    ));
                                }
                                return Ok(Replacement::DamageRule(
                                    DamageReplacement::PreventWithRider {
                                        target: target.unwrap_or(SubjectRef::SelfObject),
                                        condition: while_cond,
                                        rider,
                                    },
                                ));
                            }
                        }
                        ts.restore(pcp);
                    }
                    if let Some(threshold) = min_dmg {
                        let ccp = ts.checkpoint();
                        if ts.eat_phrase(&["it", "deals"])
                            && let Some(capped_to) = ts.next_number(false)
                            && ts.eat_word("damage")
                        {
                            if ts.eat_word("to") {
                                let _ = opt(ts, parse_subject_ref);
                            }
                            if ts.eat_word("instead") {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(DamageReplacement::Cap {
                                        source: Some(src_flt),
                                        target,
                                        threshold,
                                        capped_to,
                                    }));
                                }
                            }
                        }
                        ts.restore(ccp);
                    }
                    if ts.eat_phrase(&["prevent", "all", "but"])
                        && let Some(n) = ts.next_number(false)
                        && ts.eat_phrase(&["of", "that", "damage"])
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(Replacement::DamageRule(DamageReplacement::PreventAllBut {
                            source: Some(src_flt),
                            target,
                            remaining: u16::try_from(n).unwrap_or(1),
                        }));
                    }
                    {
                        let pcp = ts.checkpoint();
                        let _ = ts.eat_phrase(&["you", "may"]);
                        if ts.eat_word("prevent")
                            && let Ok(amount) = try_parse(ts, parse_value_expr)
                            && (ts.eat_phrase(&["of", "that", "damage"])
                                || (ts.eat_word("damage") && {
                                    let _ = ts.eat_phrase(&["that", "spell", "would", "deal"])
                                        || ts.eat_phrase(&["that", "source", "would", "deal"]);
                                    if ts.eat_word("to") {
                                        let _ = opt(ts, parse_subject_ref);
                                    }
                                    true
                                }))
                        {
                            let final_amount = if ts.eat_comma()
                                && ts.eat_phrase(&["where", "x", "is"])
                                && let Ok(val) = try_parse(ts, parse_value_expr)
                            {
                                val
                            } else {
                                amount
                            };
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::DamageRule(
                                    DamageReplacement::PreventFixed {
                                        source: Some(src_flt),
                                        target,
                                        amount: final_amount,
                                    },
                                ));
                            }
                        }
                        ts.restore(pcp);
                    }
                    if ts.eat_phrase(&["prevent", "that", "damage"]) {
                        let rider = if ts.eat_word("and")
                            || ts.eat_comma()
                            || (ts.eat_period() && !ts.is_eof())
                        {
                            let _ = ts.eat_word("and");
                            parse_effect_sentence(ts)?
                        } else {
                            Vec::new()
                        };
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::DamageRule(
                                DamageReplacement::PreventWithRider {
                                    target: target.unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                    condition: None,
                                    rider,
                                },
                            ));
                        }
                    }
                    let deals_cp = ts.checkpoint();
                    let leading_instead = ts.eat_word("instead");
                    if ts.eat_phrase(&["it", "deals"]) || ts.eat_phrase(&["they", "deal"]) {
                        {
                            let rcp = ts.checkpoint();
                            if ts.eat_phrase(&["that", "damage", "to"])
                                && let Ok(to) = try_parse(ts, parse_subject_ref)
                                && (leading_instead || ts.eat_word("instead"))
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::RedirectAll {
                                            from: target
                                                .unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                            to,
                                        },
                                    ));
                                }
                            }
                            ts.restore(rcp);
                        }
                        if ts.eat_phrase(&["double", "that", "damage"])
                            || ts.eat_phrase(&["twice", "that", "much", "damage"])
                        {
                            if ts.eat_word("to") {
                                let _ = opt(ts, parse_subject_ref);
                                if ts.eat_word("or") && opt(ts, parse_subject_ref).is_none() {
                                    let _ = ts.eat_word("permanent") || ts.eat_word("player");
                                }
                            }
                            if leading_instead || ts.eat_word("instead") {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                let rep = Replacement::ModifyDamage {
                                    source: Some(src_flt),
                                    target,
                                    add: 0,
                                    factor: 2,
                                };
                                return Ok(if let Some(condition) = while_cond {
                                    Replacement::ConditionalEnters {
                                        condition,
                                        inner: Box::new(rep),
                                    }
                                } else {
                                    rep
                                });
                            }
                        }
                        if ts.eat_phrase(&["triple", "that", "damage"]) {
                            if ts.eat_word("to") {
                                let _ = opt(ts, parse_subject_ref);
                                if ts.eat_word("or") && opt(ts, parse_subject_ref).is_none() {
                                    let _ = ts.eat_word("permanent") || ts.eat_word("player");
                                }
                            }
                            if leading_instead || ts.eat_word("instead") {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                let rep = Replacement::ModifyDamage {
                                    source: Some(src_flt),
                                    target,
                                    add: 0,
                                    factor: 3,
                                };
                                return Ok(if let Some(condition) = while_cond {
                                    Replacement::ConditionalEnters {
                                        condition,
                                        inner: Box::new(rep),
                                    }
                                } else {
                                    rep
                                });
                            }
                        }
                        {
                            let dcp = ts.checkpoint();
                            if ts.eat_phrase(&["that", "much", "damage", "plus"]) {
                                if ts.eat_word("x")
                                    && ts.eat_comma()
                                    && ts.eat_phrase(&["where", "x", "is"])
                                    && let Ok(add) = try_parse(ts, parse_value_expr)
                                {
                                    if ts.eat_word("to") {
                                        let _ = opt(ts, parse_subject_ref);
                                    }
                                    if leading_instead || ts.eat_word("instead") {
                                        let _ = ts.eat_period();
                                        if ts.is_eof() {
                                            return Ok(Replacement::DamageRule(
                                                DamageReplacement::ModifyDynamic {
                                                    source: Some(src_flt),
                                                    target,
                                                    add,
                                                },
                                            ));
                                        }
                                    }
                                }
                                ts.restore(dcp);
                            }
                        }
                        {
                            let dcp = ts.checkpoint();
                            if ts.eat_phrase(&["that", "much", "damage", "plus"]) {
                                if ts.eat_phrase(&["an", "amount", "of", "damage", "equal", "to"])
                                    && let Ok(add) = try_parse(ts, parse_value_expr)
                                {
                                    if ts.eat_word("to") {
                                        let _ = opt(ts, parse_subject_ref);
                                    }
                                    if leading_instead || ts.eat_word("instead") {
                                        let _ = ts.eat_period();
                                        if ts.is_eof() {
                                            return Ok(Replacement::DamageRule(
                                                DamageReplacement::ModifyDynamic {
                                                    source: Some(src_flt),
                                                    target,
                                                    add,
                                                },
                                            ));
                                        }
                                    }
                                }
                                ts.restore(dcp);
                            }
                        }
                        if ts.eat_phrase(&["that", "much", "damage", "minus"])
                            && let Some(n) = ts.next_number(false)
                        {
                            if ts.eat_word("to") {
                                let _ = opt(ts, parse_subject_ref);
                            }
                            if leading_instead || ts.eat_word("instead") {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(Replacement::ModifyDamage {
                                    source: Some(src_flt),
                                    target,
                                    add: -i16::try_from(n).unwrap_or(1),
                                    factor: 1,
                                });
                            }
                        }
                        if ts.eat_phrase(&["that", "much", "damage", "plus"])
                            && let Some(n) = ts.next_number(false)
                        {
                            if ts.eat_word("to") {
                                let _ = opt(ts, parse_subject_ref);
                            }
                            if leading_instead || ts.eat_word("instead") {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(Replacement::ModifyDamage {
                                    source: Some(src_flt),
                                    target,
                                    add: i16::try_from(n).unwrap_or(1),
                                    factor: 1,
                                });
                            }
                        }
                    }
                    ts.restore(deals_cp);
                    // General damage replacement (`If a source you control would deal noncombat damage to ..., put that many -1/-1 counters on that creature instead.`)
                    if let Ok(instead) = parse_instead_clause(ts) {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::DealsDamage {
                                combat_only,
                                source: Some(SubjectRef::Group(Box::new(src_flt))),
                                target,
                            },
                            instead,
                        });
                    }
                }
            }
        }
        ts.restore(cp);
    }

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
                opt(ts, parse_condition)
            } else {
                None
            };
            if ts.eat_comma() {
                if ts.eat_phrase(&["prevent", "that", "damage"]) {
                    let mut rider = Vec::new();
                    if ts.eat_word("and") || ts.eat_comma() {
                        let _ = ts.eat_word("and");
                        // Parse up to the first period (or EOF)
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
                            ts.pos = ts.tokens.len();
                        }
                    }
                    let _ = ts.eat_period();
                    if ts.is_eof() {
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
                } else if let Ok(instead) = parse_instead_clause(ts) {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
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

    // 1f1. `If <subj> would be destroyed, regenerate it.` (`Knight of the Holy Nimbus`)
    {
        let dcp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["would", "be", "destroyed"])
            && ts.eat_comma()
            && ts.eat_word("regenerate")
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

    // 1f3. `If a spell or ability an opponent controls causes you to discard ~, put it onto the battlefield instead of putting it into your graveyard.` (`Loxodon Smiter`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "a", "spell", "or", "ability", "an", "opponent", "controls", "causes", "you", "to",
            "discard",
        ]) && ts.eat_tilde()
            && ts.eat_comma()
        {
            let remaining = &ts.tokens[ts.pos..];
            let mut end = remaining.len();
            if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
                end -= 1;
            }
            let mut sub = ts.sub_stream(&remaining[..end], crate::v2::ir::ParseStage::Effect);
            if let Ok(instead) = parse_effect_sentence(&mut sub)
                && sub.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::ForcedDiscard {
                        player: PlayerRef::You,
                        subject: SubjectRef::SelfObject,
                    },
                    instead,
                });
            }
        }
        ts.restore(cp);
    }

    // 1f4. `If an opponent / you would draw a card except the first one they/you draw in each of their/your draw steps, <instead>` (`Teferi's Ageless Insight`, `Hullbreacher`)
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
        } else {
            None
        };
        if let Some(player) = draw_player
            && ts.eat_comma()
            && let Ok(instead) = parse_instead_clause(ts)
        {
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

    // 1g. `If you would draw a card [while <cond>], <instead>.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "draw", "a", "card"]) {
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
                    opt(ts, parse_condition)
                } else {
                    None
                };
            if ts.eat_comma()
                && let Ok(instead) = parse_instead_clause(ts)
            {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                let rep = Replacement::EventInstead {
                    event: TriggerEvent::PlayerAction {
                        player: PlayerRef::You,
                        action: PlayerTriggerKind::CardDrawn,
                    },
                    instead,
                };
                return Ok(if let Some(condition) = while_cond {
                    Replacement::ConditionalEnters {
                        condition,
                        inner: Box::new(rep),
                    }
                } else {
                    rep
                });
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
                    opt(ts, parse_subject_ref)
                } else {
                    None
                };
                if ts.eat_comma()
                    && ts.eat_word("prevent")
                    && let Ok(amount) = try_parse(ts, parse_value_expr)
                    && ts.eat_phrase(&["of", "that", "damage"])
                {
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

    // 1g5. Lose-the-game replacement (`If you would lose the game, instead ...`) (`Exquisite Archangel`, `The Golden Throne`, `Lich's Mirror`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "lose", "the", "game"])
            && ts.eat_comma()
            && let Ok(instead) = parse_instead_clause(ts)
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
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

    // 1g6. Unspent mana becomes colorless (`If you would lose unspent mana, that mana becomes colorless instead.`) (`Horizon Stone`, `Kruphix`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "lose", "unspent", "mana"])
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "mana", "becomes", "colorless", "instead"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(ReplacementRuleKind::ManaOverride(
                    crate::v2::ir::ManaCost {
                        symbols: vec![crate::v2::ir::ManaSymbol::Colorless],
                    },
                )));
            }
        }
        ts.restore(cp);
    }

    // 1g4. Explore doubling (`If <flt> would explore, instead it explores, then it explores again.`)
    {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["would", "explore"])
            && ts.eat_comma()
            && ts.eat_phrase(&["instead", "it", "explores"])
            && ts.eat_comma()
            && ts.eat_phrase(&["then", "it", "explores", "again"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(ReplacementRuleKind::ExploreAgain(filter)));
            }
        }
        ts.restore(cp);
    }

    // 1h. `If <cond>, it/~ enters with <N> <kind> counters on it / enters tapped`
    {
        let cp = ts.checkpoint();
        if let Ok(condition) = try_parse(ts, parse_condition) {
            let _ = ts.eat_phrase(&["with", "its"])
                && ts.eat_mana_cost().is_some()
                && ts.eat_word("kicker");
            if ts.eat_comma() && (ts.eat_word("it") || ts.eat_tilde()) {
                if ts.eat_phrase(&["enters", "tapped"]) {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::ConditionalEnters {
                            condition,
                            inner: Box::new(Replacement::EntersTapped { unless: None }),
                        });
                    }
                } else if ts.eat_phrase(&["enters", "with"]) {
                    let count = if ts.eat_word("a") || ts.eat_word("an") {
                        ValueExpr::Const(1)
                    } else {
                        try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                    };
                    let additional = ts.eat_word("additional");
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && (ts.eat_word("counters") || ts.eat_word("counter"))
                        && ts.eat_phrase(&["on", "it"])
                    {
                        let extra_kws = {
                            let kwcp = ts.checkpoint();
                            if ts.eat_phrase(&["and", "with"])
                                && let Ok((kws, _)) =
                                    try_parse(ts, crate::v2::keywords::parse_keyword_list)
                            {
                                Some(kws)
                            } else {
                                ts.restore(kwcp);
                                None
                            }
                        };
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            let base_counters = Replacement::EntersWithCounters {
                                subject: None,
                                kind,
                                count,
                                additional,
                            };
                            let inner = if let Some(kws) = extra_kws {
                                Replacement::Compound(vec![
                                    base_counters,
                                    Replacement::Rule(ReplacementRuleKind::EntersWithKeywords(kws)),
                                ])
                            } else {
                                base_counters
                            };
                            return Ok(Replacement::ConditionalEnters {
                                condition,
                                inner: Box::new(inner),
                            });
                        }
                    }
                }
            }
        }
        ts.restore(cp);
    }

    let subj = parse_subject_ref(ts)?;
    if (ts.eat_phrase(&["would", "enter", "the", "battlefield"])
        || ts.eat_phrase(&["would", "enter"]))
        && {
            let _ = ts.eat_phrase(&["this", "turn"]);
            ts.eat_comma()
        }
    {
        let instead = parse_instead_clause(ts)?;
        let _ = ts.eat_period();
        ts.expect_eof()?;
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::ZoneChange {
                from: None,
                to: Some(crate::v2::ir::ZoneKind::Battlefield),
                subject: subj,
            },
            instead,
        });
    }
    if ts.eat_phrase(&["would", "die"]) && {
        let _ = ts.eat_phrase(&["this", "turn"]);
        ts.eat_comma()
    } {
        let instead = parse_instead_clause(ts)?;
        let _ = ts.eat_period();
        ts.expect_eof()?;
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::Dies { subject: subj },
            instead,
        });
    }
    if ts.eat_phrase(&["would", "leave", "the", "battlefield"]) && {
        let _ = ts.eat_phrase(&["this", "turn"]);
        ts.eat_comma()
    } {
        let instead = parse_instead_clause(ts)?;
        let _ = ts.eat_period();
        ts.expect_eof()?;
        return Ok(Replacement::EventInstead {
            event: TriggerEvent::ZoneChange {
                from: Some(crate::v2::ir::ZoneKind::Battlefield),
                to: None,
                subject: subj,
            },
            instead,
        });
    }
    if ts.eat_phrase(&["would", "be", "put", "into"]) {
        let z = crate::v2::filter::parse_zone_ref(ts)?;
        if z.zone == crate::v2::ir::ZoneKind::Graveyard {
            let from = if ts.eat_phrase(&["from", "the", "battlefield"]) {
                Some(crate::v2::ir::ZoneKind::Battlefield)
            } else {
                let _ = ts.eat_phrase(&["from", "anywhere"]);
                None
            };
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_comma() {
                let instead = parse_instead_clause(ts)?;
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::ZoneChange {
                        from,
                        to: Some(ZoneKind::Graveyard),
                        subject: subj,
                    },
                    instead,
                });
            }
        }
    }
    Err(ts.err("unsupported `If ... would ...` replacement event"))
}
