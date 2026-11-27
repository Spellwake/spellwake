//! `If <event> would ...` replacement effect parser for `v2`.

use crate::v2::clause::parse_effect_sentence;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    DamageReplacement, ObjectFilter, PlayerRef, PlayerReplacementKind, PlayerTriggerKind,
    Replacement, ReplacementRuleKind, SubjectRef, TokenReplacementKind, TriggerEvent, ValueExpr,
    ZoneKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::static_ability::cond_ext::parse_static_condition;
use crate::v2::value::parse_value_expr;

use super::{eat_period_eof, lookup_predefined_token, parse_instead_clause, wrap_while_cond};

fn eat_mana_override_cost(ts: &mut TokenStream<'_>) -> Option<crate::v2::ir::ManaCost> {
    if let Some(mc) = ts.eat_mana_cost() {
        Some(mc)
    } else if ts.eat_phrase(&["colorless", "mana"]) {
        Some(crate::v2::ir::ManaCost {
            symbols: vec![crate::v2::ir::ManaSymbol::Colorless],
        })
    } else {
        None
    }
}

fn is_anaphoric_same_target(orig: &SubjectRef, tail: &SubjectRef) -> bool {
    use crate::v2::ir::AnaphoraRef;
    if orig == tail {
        return true;
    }
    match (orig, tail) {
        (SubjectRef::Player(_), SubjectRef::Player(PlayerRef::ThatPlayer)) => true,
        (
            SubjectRef::Group(flt) | SubjectRef::All(flt) | SubjectRef::Each(flt),
            SubjectRef::Anaphora(
                AnaphoraRef::ThatPermanent
                | AnaphoraRef::ThatCreature
                | AnaphoraRef::It
                | AnaphoraRef::Them
                | AnaphoraRef::ThosePermanents
                | AnaphoraRef::ThoseCreatures,
            ),
        ) => {
            matches!(
                tail,
                SubjectRef::Anaphora(
                    AnaphoraRef::ThatPermanent
                        | AnaphoraRef::It
                        | AnaphoraRef::Them
                        | AnaphoraRef::ThosePermanents
                )
            ) || (flt.any_types.0 & crate::ast::CardTypeMask::CREATURE.0) != 0
                || (flt.all_types.0 & crate::ast::CardTypeMask::CREATURE.0) != 0
        }
        (
            SubjectRef::SelfObject | SubjectRef::Anaphora(_),
            SubjectRef::Anaphora(
                AnaphoraRef::It
                | AnaphoraRef::ThatPermanent
                | AnaphoraRef::ThatCreature
                | AnaphoraRef::SameTarget,
            ),
        ) => true,
        (SubjectRef::Compound(a), SubjectRef::Compound(b)) if a.len() == 2 && b.len() == 2 => {
            let has_anaphora = b.iter().any(|e| {
                matches!(
                    e,
                    SubjectRef::Anaphora(_) | SubjectRef::Player(PlayerRef::ThatPlayer)
                )
            });
            if !has_anaphora {
                return false;
            }
            let plain_permanent = ObjectFilter {
                any_permanent: true,
                ..Default::default()
            };
            let match_elem = |x: &SubjectRef, y: &SubjectRef| {
                is_anaphoric_same_target(x, y)
                    || matches!(
                        (x, y),
                        (
                            SubjectRef::Group(_) | SubjectRef::All(_) | SubjectRef::Each(_),
                            SubjectRef::Group(g)
                        ) if g.as_ref() == &plain_permanent
                    )
                    || matches!(
                        (x, y),
                        (
                            SubjectRef::Player(_),
                            SubjectRef::Player(PlayerRef::AnyPlayer)
                        )
                    )
            };
            (match_elem(&a[0], &b[0]) && match_elem(&a[1], &b[1]))
                || (match_elem(&a[0], &b[1]) && match_elem(&a[1], &b[0]))
        }
        _ => false,
    }
}

fn resolve_optional_to_target(
    ts: &mut TokenStream<'_>,
    existing: &Option<SubjectRef>,
    allow_or_noun: bool,
) -> PResult<Option<SubjectRef>> {
    let cp = ts.checkpoint();
    if !ts.eat_word("to") {
        return Ok(existing.clone());
    }
    let first = match try_parse(ts, parse_subject_ref) {
        Ok(s) => s,
        Err(e) => {
            ts.restore(cp);
            return Err(e);
        }
    };
    let tail = if allow_or_noun && ts.eat_word("or") {
        let second = if let Ok(s2) = try_parse(ts, parse_subject_ref) {
            s2
        } else if ts.eat_word("permanent") {
            SubjectRef::Group(Box::new(ObjectFilter {
                any_permanent: true,
                ..Default::default()
            }))
        } else if ts.eat_word("player") {
            SubjectRef::Player(PlayerRef::AnyPlayer)
        } else {
            ts.restore(cp);
            return Err(ts.err("expected target after `or` in damage replacement"));
        };
        SubjectRef::Compound(vec![first, second])
    } else {
        first
    };
    match existing {
        None => Ok(Some(tail)),
        Some(orig) if is_anaphoric_same_target(orig, &tail) => Ok(Some(orig.clone())),
        Some(_) => {
            ts.restore(cp);
            Err(ts.err("conflicting target in damage replacement"))
        }
    }
}

pub(super) fn parse_if_would_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 1a0. Opening hand battlefield start (`If ~ is in your opening hand, you may begin the game with it/~ on the battlefield.`)
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_phrase(&["is", "in", "your", "opening", "hand"])
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may", "begin", "the", "game", "with"])
            && (ts.eat_word("it") || ts.eat_tilde())
            && ts.eat_phrase(&["on", "the", "battlefield"])
            && eat_period_eof(ts)
        {
            return Ok(Replacement::Rule(
                ReplacementRuleKind::OpeningHandOnBattlefield,
            ));
        }
        ts.restore(cp);
    }

    // 1a0c2. Subject `would` replacements
    {
        let cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("would")
        {
            let wcp = ts.checkpoint();
            if ts.eat_phrase(&["be", "dealt", "damage"])
                && ts.eat_comma()
                && ts.eat_phrase(&["prevent", "that", "damage", "and"])
                && let Ok(rider) = try_parse(ts, parse_effect_sentence)
                && !rider.is_empty()
                && eat_period_eof(ts)
            {
                return Ok(Replacement::DamageRule(
                    DamageReplacement::PreventWithRider {
                        target: subj,
                        condition: None,
                        rider,
                    },
                ));
            }
            ts.restore(wcp);
            if matches!(subj, SubjectRef::SelfObject)
                && ts.eat_word("die")
                && ts.eat_comma()
                && let Ok(instead) = try_parse(ts, parse_instead_clause)
                && eat_period_eof(ts)
            {
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::Dies {
                        subject: SubjectRef::SelfObject,
                    },
                    instead,
                });
            }
        }
        ts.restore(cp);
    }

    // 1a0c3. `If you would scry / get counters / draw ...`
    {
        let ycp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would"]) {
            let ybody = ts.checkpoint();
            if ts.eat_phrase(&["scry", "a", "number", "of", "cards"]) && ts.eat_comma() {
                let s_cp = ts.checkpoint();
                if ts.eat_phrase(&["scry", "that", "many", "cards", "plus"])
                    && let Some(n) = ts.next_number(false)
                    && ts.eat_word("instead")
                {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player: PlayerRef::You,
                                action: PlayerTriggerKind::Scry,
                            },
                            instead: vec![crate::v2::ir::Effect::MutatePlayer {
                                player: PlayerRef::You,
                                mutation: crate::v2::ir::PlayerMutation::ScaledAction {
                                    kind: crate::v2::ir::ScaledPlayerAction::Scry,
                                    amount: ValueExpr::Add(
                                        Box::new(ValueExpr::PriorActionCount(
                                            crate::v2::ir::PriorActionKind::ThatMany,
                                        )),
                                        Box::new(ValueExpr::Const(n)),
                                    ),
                                },
                                duration: crate::v2::ir::Duration::Instant,
                            }],
                        });
                    }
                }
                ts.restore(s_cp);
                if let Ok(instead) = parse_instead_clause(ts) {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player: PlayerRef::You,
                                action: PlayerTriggerKind::Scry,
                            },
                            instead,
                        });
                    }
                }
            }
            ts.restore(ybody);
            if ts.eat_phrase(&["get", "one", "or", "more"]) {
                let kind_opt = if matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(crate::v2::lexer::TokenKind::Mana(
                        crate::v2::ir::ManaSymbol::Energy
                    ))
                ) {
                    ts.next_tok();
                    Some(Some(crate::v2::ir::CounterKind::Energy))
                } else if ts.eat_word("counters") {
                    Some(None)
                } else {
                    None
                };
                if let Some(kind) = kind_opt
                    && ts.eat_comma()
                    && ts.eat_phrase(&["you", "get"])
                {
                    let (add, factor) = if ts.eat_phrase(&["twice", "that", "many"]) {
                        (0i16, 2u8)
                    } else if ts.eat_phrase(&["that", "many", "plus"])
                        && let Some(n) = ts.next_number(false)
                        && n > 0
                    {
                        (i16::try_from(n).unwrap_or(1), 1u8)
                    } else {
                        (0, 0)
                    };
                    if add != 0 || factor > 1 {
                        let _ = ts
                            .eat_phrase(&["of", "each", "of", "those", "kinds", "of", "counters"])
                            || ts.eat_phrase(&["of", "those", "counters"])
                            || (matches!(
                                ts.peek().map(|t| &t.kind),
                                Some(crate::v2::lexer::TokenKind::Mana(
                                    crate::v2::ir::ManaSymbol::Energy
                                ))
                            ) && ts.next_tok().is_some());
                        if ts.eat_word("instead") {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::CounterRule {
                                    filter: ObjectFilter::default(),
                                    kind,
                                    add,
                                    factor,
                                });
                            }
                        }
                    }
                }
            }
            ts.restore(ybody);
            if ts.eat_phrase(&["draw", "one", "or", "more", "cards"])
                && ts.eat_comma()
                && {
                    let _ = ts.eat_word("you");
                    ts.eat_phrase(&["draw", "that", "many", "cards", "plus"])
                }
                && let Some(n) = ts.next_number(false)
                && ts.eat_word("instead")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player: PlayerRef::You,
                            action: PlayerTriggerKind::CardDrawn,
                        },
                        instead: vec![crate::v2::ir::Effect::ZoneOp {
                            actor: PlayerRef::You,
                            op: crate::v2::ir::ZoneOpKind::Draw,
                            subject: SubjectRef::Player(PlayerRef::You),
                            from: Vec::new(),
                            to: None,
                            modifiers: crate::v2::ir::EntryModifiers::default(),
                            except: None,
                            count: Some(crate::v2::ir::CountSpec {
                                count: ValueExpr::Add(
                                    Box::new(ValueExpr::PriorActionCount(
                                        crate::v2::ir::PriorActionKind::ThatMany,
                                    )),
                                    Box::new(ValueExpr::Const(n)),
                                ),
                                up_to: false,
                                any_number: false,
                            }),
                        }],
                    });
                }
            }
        }
        ts.restore(ycp);
    }

    // 1a0c3b. `If a source would deal damage to ~, prevent that damage. <rider>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "source", "would", "deal", "damage", "to"])
            && ts.eat_tilde()
            && ts.eat_comma()
            && ts.eat_phrase(&["prevent", "that", "damage"])
            && ts.eat_period()
            && let Ok(rider) = try_parse(ts, parse_effect_sentence)
            && !rider.is_empty()
            && eat_period_eof(ts)
        {
            return Ok(Replacement::DamageRule(
                DamageReplacement::PreventWithRider {
                    target: SubjectRef::SelfObject,
                    condition: None,
                    rider,
                },
            ));
        }
        ts.restore(cp);
    }

    // 1a0c. `If ~ would leave the battlefield, exile it instead of putting it anywhere else.`
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_phrase(&["would", "leave", "the", "battlefield"])
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "exile", "it", "instead", "of", "putting", "it", "anywhere", "else",
            ])
            && eat_period_eof(ts)
        {
            return Ok(Replacement::Rule(
                ReplacementRuleKind::LeaveBattlefieldExileInstead,
            ));
        }
        ts.restore(cp);
    }

    // 1a0d. `If an opponent would lose life [during your turn], they lose twice that much life instead.`
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "lose", "life"])
        {
            let during_your_turn = ts.eat_phrase(&["during", "your", "turn"]);
            if ts.eat_comma()
                && ts.eat_any_phrase(&[
                    &["they", "lose", "twice", "that", "much", "life", "instead"],
                    &[
                        "that", "player", "loses", "twice", "that", "much", "life", "instead",
                    ],
                ])
                && eat_period_eof(ts)
            {
                return Ok(Replacement::PlayerRule {
                    player,
                    rule: PlayerReplacementKind::LoseLifeMultiply {
                        factor: 2,
                        during_your_turn,
                    },
                });
            }
        }
        ts.restore(cp);
    }

    // 1a0d2. Life-gain-to-loss and extra-turn-skip replacements
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref) {
            if ts.eat_phrase(&["would", "gain", "life"])
                && ts.eat_comma()
                && ts.eat_any_phrase(&[
                    &["that", "player", "loses", "that", "much", "life", "instead"],
                    &["they", "lose", "that", "much", "life", "instead"],
                ])
                && eat_period_eof(ts)
            {
                return Ok(Replacement::LoseLifeInsteadOfGain(player));
            }
            ts.restore(cp);
            if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
                && ts.eat_phrase(&["would", "begin", "an", "extra", "turn"])
                && ts.eat_comma()
                && ts.eat_any_phrase(&[
                    &["that", "player", "skips", "that", "turn", "instead"],
                    &["they", "skip", "that", "turn", "instead"],
                ])
                && eat_period_eof(ts)
            {
                return Ok(Replacement::PlayerRule {
                    player,
                    rule: PlayerReplacementKind::SkipExtraTurn,
                });
            }
        }
        ts.restore(cp);
    }

    // 1a0f3. `If <player> would mill one or more cards, <player> mill(s) that many cards plus <N> instead.`
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "mill", "one", "or", "more", "cards"])
            && ts.eat_comma()
            && try_parse(ts, crate::v2::filter::parse_player_ref).is_ok()
            && (ts.eat_word("mill") || ts.eat_word("mills"))
            && ts.eat_phrase(&["that", "many", "cards", "plus"])
            && let Ok(extra) = try_parse(ts, parse_value_expr)
            && ts.eat_word("instead")
            && eat_period_eof(ts)
        {
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
        ts.restore(cp);
    }

    // 1a0f. `If a land is tapped for mana, it produces [{M} | colorless mana] instead of any other type [and amount].`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "land", "is", "tapped", "for", "mana"])
            && ts.eat_comma()
            && ts.eat_phrase(&["it", "produces"])
            && let Some(mc) = eat_mana_override_cost(ts)
            && ts.eat_phrase(&["instead", "of", "any", "other", "type"])
        {
            let _ = ts.eat_phrase(&["and", "amount"]);
            if eat_period_eof(ts) {
                return Ok(Replacement::Rule(ReplacementRuleKind::ManaOverride(mc)));
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

    // 1a. Token creation replacements
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["an", "effect", "would", "create"])
            && ts.eat_phrase(&["one", "or", "more", "tokens"])
            && ts.eat_phrase(&["under", "your", "control"])
            && ts.eat_comma()
            && ts.eat_phrase(&["it", "creates", "twice", "that", "many"])
            && ts.eat_phrase(&["of", "those", "tokens", "instead"])
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
        if ts.eat_phrase(&["one", "or", "more"]) {
            let filter = if ts.eat_word("creature") {
                Some(ObjectFilter {
                    token: Some(true),
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    ..Default::default()
                })
            } else if ts.eat_word("artifact") {
                Some(ObjectFilter {
                    token: Some(true),
                    any_types: crate::ast::CardTypeMask::ARTIFACT,
                    ..Default::default()
                })
            } else if ts.eat_word("enchantment") {
                Some(ObjectFilter {
                    token: Some(true),
                    any_types: crate::ast::CardTypeMask::ENCHANTMENT,
                    ..Default::default()
                })
            } else {
                None
            };
            if ts.eat_phrase(&["tokens", "would", "be", "created"]) {
                let controller = if ts.eat_phrase(&["under", "your", "control"]) {
                    PlayerRef::You
                } else {
                    PlayerRef::EachPlayer
                };
                if ts.eat_comma() {
                    if ts.eat_phrase(&["twice", "that", "many"])
                        && ts.eat_phrase(&["of", "those", "tokens", "are", "created", "instead"])
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(Replacement::TokenRule {
                            controller,
                            filter,
                            rule: TokenReplacementKind::Multiply(2),
                        });
                    }
                    if ts.eat_phrase(&["three", "times", "that", "many"])
                        && ts.eat_phrase(&["of", "those", "tokens", "are", "created", "instead"])
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(Replacement::TokenRule {
                            controller,
                            filter,
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
                                    filter,
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
                                        filter,
                                        rule: TokenReplacementKind::Bonus {
                                            spec: Box::new(crate::v2::ir::TokenSpec::Predefined(
                                                pt,
                                            )),
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
                                    filter,
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
                    && eat_period_eof(ts)
                {
                    return Ok(Replacement::TokenRule {
                        controller: PlayerRef::You,
                        filter,
                        rule: TokenReplacementKind::Replace(Box::new(spec1)),
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 1b. `If an effect would put one or more counters on <flt>, it puts twice that many of those counters on that permanent instead.`
    if ts.eat_phrase(&["an", "effect", "would", "put"])
        && ts.eat_phrase(&["one", "or", "more", "counters", "on"])
    {
        let flt = parse_object_filter(ts)?;
        if ts.eat_comma()
            && ts.eat_phrase(&["it", "puts", "twice", "that", "many"])
            && ts.eat_phrase(&["of", "those", "counters"])
            && ts.eat_phrase(&["on", "that", "permanent", "instead"])
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

    // 1c0b. `If <player> would gain life [this turn], that player gains no life instead.`
    {
        let ncp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_phrase(&["would", "gain", "life"])
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_comma()
                && ts.eat_phrase(&["that", "player", "gains", "no", "life", "instead"])
                && eat_period_eof(ts)
            {
                return Ok(Replacement::PlayerRule {
                    player,
                    rule: PlayerReplacementKind::ModifyLifeGain { add: 0, factor: 0 },
                });
            }
        }
        ts.restore(ncp);
    }

    // 1c. `If you would gain life [while <cond>], you gain that much life plus <N> / twice that much life instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "would", "gain", "life"]) {
            let while_cond = if ts.eat_word("while") {
                opt(ts, parse_static_condition)
            } else {
                None
            };
            if ts.eat_comma() {
                let gcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "gain"]) {
                    if ts.eat_phrase(&["twice", "that", "much", "life", "instead"]) {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(wrap_while_cond(
                            while_cond,
                            Replacement::PlayerRule {
                                player: PlayerRef::You,
                                rule: PlayerReplacementKind::ModifyLifeGain { add: 0, factor: 2 },
                            },
                        ));
                    }
                    if ts.eat_phrase(&["that", "much", "life", "plus"])
                        && let Some(n) = ts.next_number(false)
                        && n > 0
                        && ts.eat_word("instead")
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(wrap_while_cond(
                            while_cond,
                            Replacement::PlayerRule {
                                player: PlayerRef::You,
                                rule: PlayerReplacementKind::ModifyLifeGain {
                                    add: i16::try_from(n).unwrap_or(1),
                                    factor: 1,
                                },
                            },
                        ));
                    }
                }
                ts.restore(gcp);
                if let Ok(instead) = parse_instead_clause(ts)
                    && eat_period_eof(ts)
                {
                    return Ok(wrap_while_cond(
                        while_cond,
                        Replacement::EventInstead {
                            event: TriggerEvent::LifeChanged {
                                player: PlayerRef::You,
                                gained: true,
                            },
                            instead,
                        },
                    ));
                }
            }
        }
        ts.restore(cp);
    }

    // 1d. `If one or more [<kind>] counters would be put on [a/an] <flt>, that many plus/minus <N> / twice that many [of those | <kind>] counters are put on it/that permanent/that creature instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "or", "more"]) {
            let kind = opt(ts, parse_counter_kind);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["would", "be", "put", "on"])
            {
                let _ = ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("any");
                if let Ok(flt) = try_parse(ts, parse_object_filter)
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
                                kind,
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
                                // consumed optional `<kind> counter(s)` before `plus`/`minus`
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
                            && n > 0
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

    // 1e. `If <source> would deal [combat] damage [to <tgt>], <prevent | modify | instead>`
    {
        let cp = ts.checkpoint();
        let mut src_flt_opt: Option<ObjectFilter> = None;
        let mut src_is_self = false;
        {
            let scp = ts.checkpoint();
            let mut src_flt = ObjectFilter::default();
            let mut matched_src_word = false;
            let _ = ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("any");
            if ts.eat_word("another") {
                src_flt.flags = src_flt
                    .flags
                    .union(crate::v2::ir::FilterFlags::EXCLUDE_SELF);
            }
            if ts.eat_word("noncreature") {
                src_flt.forbidden_types = src_flt
                    .forbidden_types
                    .union(crate::ast::CardTypeMask::CREATURE);
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
                } else if ts.eat_phrase(&[
                    "controlled",
                    "by",
                    "one",
                    "of",
                    "the",
                    "chosen",
                    "players",
                ]) {
                    src_flt.controller = Some(crate::ast::ControllerRelation::ChosenPlayer);
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
                if ts.eat_tilde() {
                    src_is_self = true;
                    src_flt_opt = Some(ObjectFilter::default());
                } else if let Ok(mut f) = try_parse(ts, parse_object_filter) {
                    let ocp = ts.checkpoint();
                    if ts.eat_word("or")
                        && let Ok(f2) = try_parse(ts, parse_object_filter)
                    {
                        f = ObjectFilter {
                            predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![f, f2])],
                            ..Default::default()
                        };
                    } else {
                        ts.restore(ocp);
                    }
                    src_flt_opt = Some(f);
                }
            }
        }
        if let Some(src_flt) = src_flt_opt
            && ts.eat_phrase(&["would", "deal"])
        {
            let combat_only = ts.eat_word("combat");
            let min_dmg = {
                let dcp = ts.checkpoint();
                if let Some(n) = ts.next_number(false)
                    && ts.eat_phrase(&["or", "more"])
                {
                    Some(n)
                } else {
                    ts.restore(dcp);
                    None
                }
            };
            if ts.eat_word("damage") {
                let _ = ts.eat_phrase(&["this", "turn"]);
                let target = if ts.eat_word("to") {
                    if ts.eat_phrase(&["one", "of", "your", "opponents"]) {
                        Some(SubjectRef::Player(PlayerRef::AnyOpponent))
                    } else if let Some(mut first_tgt) = opt(ts, parse_subject_ref) {
                        let mut tgts = vec![first_tgt.clone()];
                        loop {
                            let tcp = ts.checkpoint();
                            let had_comma = ts.eat_comma();
                            let had_or = ts.eat_word("or") || ts.eat_word("and/or");
                            if !had_comma && !had_or {
                                ts.restore(tcp);
                                break;
                            }
                            if had_comma && !had_or {
                                if ts.eat_word("battle") {
                                    tgts.push(SubjectRef::Group(Box::new(ObjectFilter {
                                        any_types: crate::ast::CardTypeMask::BATTLE,
                                        ..Default::default()
                                    })));
                                    continue;
                                }
                                if ts.eat_word("planeswalker") {
                                    tgts.push(SubjectRef::Group(Box::new(ObjectFilter {
                                        any_types: crate::ast::CardTypeMask::PLANESWALKER,
                                        ..Default::default()
                                    })));
                                    continue;
                                }
                                ts.restore(tcp);
                                break;
                            }
                            let _ = ts.eat_phrase(&["one", "or", "more"]);
                            if ts.eat_word("opponent") {
                                tgts.push(SubjectRef::Player(PlayerRef::AnyOpponent));
                                continue;
                            }
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
                let _ = ts.eat_phrase(&["this", "turn"]);
                let while_cond = if ts.eat_word("while") {
                    opt(ts, parse_static_condition)
                } else {
                    None
                };
                let dmg_source = if src_is_self {
                    None
                } else {
                    Some(src_flt.clone())
                };
                if ts.eat_comma() {
                    // `prevent half that damage, rounded up/down`
                    if !combat_only && min_dmg.is_none() {
                        let hcp = ts.checkpoint();
                        if ts.eat_phrase(&["prevent", "half", "that", "damage"]) {
                            let rounding = if ts.eat_rounded_modifier() == Some(true) {
                                crate::ast::Rounding::Up
                            } else {
                                crate::ast::Rounding::Down
                            };
                            if eat_period_eof(ts) {
                                return Ok(wrap_while_cond(
                                    while_cond,
                                    Replacement::DamageRule(DamageReplacement::PreventFixed {
                                        source: dmg_source,
                                        target,
                                        amount: ValueExpr::Div {
                                            expr: Box::new(ValueExpr::PriorActionCount(
                                                crate::v2::ir::PriorActionKind::ThatMany,
                                            )),
                                            denom: 2,
                                            rounding,
                                        },
                                    }),
                                ));
                            }
                        }
                        ts.restore(hcp);
                    }
                    // `prevent that damage [and/, <rider>] [. <rider2>]` (only for unfiltered source with explicit target)
                    if !combat_only
                        && min_dmg.is_none()
                        && !src_is_self
                        && src_flt == ObjectFilter::default()
                        && let Some(tgt) = target.clone()
                    {
                        let pcp = ts.checkpoint();
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            let mut rider = Vec::new();
                            if ts.eat_word("and") || ts.eat_comma() {
                                let _ = ts.eat_word("and");
                                let start = ts.pos;
                                let mut end = start;
                                while end < ts.tokens.len()
                                    && !matches!(
                                        ts.tokens[end].kind,
                                        crate::v2::lexer::TokenKind::Period
                                    )
                                {
                                    end += 1;
                                }
                                let mut sub = ts.sub_stream(
                                    &ts.tokens[start..end],
                                    crate::v2::ir::ParseStage::Effect,
                                );
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
                                let mut sub = ts.sub_stream(
                                    &ts.tokens[start..end],
                                    crate::v2::ir::ParseStage::Effect,
                                );
                                if let Ok(r) = parse_effect_sentence(&mut sub)
                                    && sub.is_eof()
                                {
                                    rider.extend(r);
                                    ts.commit_substream_eof(end, &sub);
                                }
                            }
                            if eat_period_eof(ts) {
                                if rider.is_empty() {
                                    return Ok(wrap_while_cond(
                                        while_cond,
                                        Replacement::DamageRule(DamageReplacement::PreventAll {
                                            target: tgt,
                                        }),
                                    ));
                                }
                                return Ok(Replacement::DamageRule(
                                    DamageReplacement::PreventWithRider {
                                        target: tgt,
                                        condition: while_cond,
                                        rider,
                                    },
                                ));
                            }
                        }
                        ts.restore(pcp);
                    }
                    if !combat_only && let Some(threshold) = min_dmg {
                        let ccp = ts.checkpoint();
                        if ts.eat_phrase(&["it", "deals"])
                            && let Some(capped_to) = ts.next_number(false)
                            && ts.eat_word("damage")
                            && let Ok(resolved_target) =
                                resolve_optional_to_target(ts, &target, true)
                            && ts.eat_word("instead")
                            && eat_period_eof(ts)
                        {
                            return Ok(wrap_while_cond(
                                while_cond,
                                Replacement::DamageRule(DamageReplacement::Cap {
                                    source: dmg_source,
                                    target: resolved_target,
                                    threshold,
                                    capped_to,
                                }),
                            ));
                        }
                        ts.restore(ccp);
                    }
                    if !combat_only
                        && min_dmg.is_none()
                        && ts.eat_phrase(&["prevent", "all", "but"])
                        && let Some(n) = ts.next_number(false)
                        && ts.eat_phrase(&["of", "that", "damage"])
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(wrap_while_cond(
                            while_cond,
                            Replacement::DamageRule(DamageReplacement::PreventAllBut {
                                source: dmg_source,
                                target,
                                remaining: u16::try_from(n).unwrap_or(1),
                            }),
                        ));
                    }
                    if !combat_only && min_dmg.is_none() {
                        let pcp = ts.checkpoint();
                        if ts.eat_word("prevent")
                            && let Ok(amount) = try_parse(ts, parse_value_expr)
                        {
                            let resolved_target = if ts.eat_phrase(&["of", "that", "damage"]) {
                                Some(target.clone())
                            } else if ts.eat_word("damage") {
                                let _ = ts.eat_any_phrase(&[
                                    &["that", "spell", "would", "deal"],
                                    &["that", "source", "would", "deal"],
                                ]);
                                resolve_optional_to_target(ts, &target, false).ok()
                            } else {
                                None
                            };
                            if let Some(resolved_target) = resolved_target {
                                let final_amount = if ts.eat_comma()
                                    && ts.eat_phrase(&["where", "x", "is"])
                                    && let Ok(val) = try_parse(ts, parse_value_expr)
                                {
                                    val
                                } else {
                                    amount
                                };
                                if eat_period_eof(ts) {
                                    return Ok(wrap_while_cond(
                                        while_cond,
                                        Replacement::DamageRule(DamageReplacement::PreventFixed {
                                            source: dmg_source,
                                            target: resolved_target,
                                            amount: final_amount,
                                        }),
                                    ));
                                }
                            }
                        }
                        ts.restore(pcp);
                    }
                    if !combat_only && min_dmg.is_none() {
                        let deals_cp = ts.checkpoint();
                        let leading_instead = ts.eat_word("instead");
                        if ts.eat_any_phrase(&[
                            &["it", "deals"],
                            &["they", "deal"],
                            &["that", "source", "deals"],
                        ]) {
                            if ts.eat_phrase(&["half", "that", "damage"]) {
                                let prevent_rounding = if ts.eat_rounded_modifier() == Some(true) {
                                    crate::ast::Rounding::Down
                                } else {
                                    crate::ast::Rounding::Up
                                };
                                let _ = ts.eat_comma();
                                if let Ok(resolved_target) =
                                    resolve_optional_to_target(ts, &target, true)
                                    && (leading_instead || ts.eat_word("instead"))
                                    && eat_period_eof(ts)
                                {
                                    return Ok(wrap_while_cond(
                                        while_cond,
                                        Replacement::DamageRule(DamageReplacement::PreventFixed {
                                            source: dmg_source,
                                            target: resolved_target,
                                            amount: ValueExpr::Div {
                                                expr: Box::new(ValueExpr::PriorActionCount(
                                                    crate::v2::ir::PriorActionKind::ThatMany,
                                                )),
                                                denom: 2,
                                                rounding: prevent_rounding,
                                            },
                                        }),
                                    ));
                                }
                            }
                            if !src_is_self
                                && src_flt == ObjectFilter::default()
                                && let Some(from) = target.clone()
                            {
                                let rcp = ts.checkpoint();
                                if ts.eat_phrase(&["that", "damage", "to"])
                                    && let Ok(to) = try_parse(ts, parse_subject_ref)
                                    && (leading_instead || ts.eat_word("instead"))
                                    && eat_period_eof(ts)
                                {
                                    return Ok(wrap_while_cond(
                                        while_cond,
                                        Replacement::DamageRule(DamageReplacement::RedirectAll {
                                            from,
                                            to,
                                        }),
                                    ));
                                }
                                ts.restore(rcp);
                            }
                            let factor_opt = if ts.eat_any_phrase(&[
                                &["double", "that", "damage"],
                                &["twice", "that", "much", "damage"],
                            ]) {
                                Some(2u8)
                            } else if ts.eat_phrase(&["triple", "that", "damage"]) {
                                Some(3u8)
                            } else {
                                None
                            };
                            if let Some(factor) = factor_opt
                                && let Ok(resolved_target) =
                                    resolve_optional_to_target(ts, &target, true)
                                && (leading_instead || ts.eat_word("instead"))
                            {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(wrap_while_cond(
                                    while_cond,
                                    Replacement::ModifyDamage {
                                        source: dmg_source,
                                        target: resolved_target,
                                        add: 0,
                                        factor,
                                    },
                                ));
                            }
                            {
                                let dcp = ts.checkpoint();
                                if ts.eat_phrase(&["that", "much", "damage", "plus"]) {
                                    if ts.eat_word("x") {
                                        let add = if ts.eat_comma()
                                            && ts.eat_phrase(&["where", "x", "is"])
                                            && let Ok(val) = try_parse(ts, parse_value_expr)
                                        {
                                            val
                                        } else {
                                            ValueExpr::Var(crate::v2::ir::VarName::X)
                                        };
                                        if let Ok(resolved_target) =
                                            resolve_optional_to_target(ts, &target, true)
                                            && (leading_instead || ts.eat_word("instead"))
                                            && eat_period_eof(ts)
                                        {
                                            return Ok(wrap_while_cond(
                                                while_cond,
                                                Replacement::DamageRule(
                                                    DamageReplacement::ModifyDynamic {
                                                        source: dmg_source,
                                                        target: resolved_target,
                                                        add,
                                                    },
                                                ),
                                            ));
                                        }
                                    }
                                    ts.restore(dcp);
                                }
                            }
                            {
                                let dcp = ts.checkpoint();
                                if ts.eat_phrase(&["that", "much", "damage", "plus"]) {
                                    if ts.eat_phrase(&[
                                        "an", "amount", "of", "damage", "equal", "to",
                                    ]) && let Ok(add) = try_parse(ts, parse_value_expr)
                                        && let Ok(resolved_target) =
                                            resolve_optional_to_target(ts, &target, false)
                                        && (leading_instead || ts.eat_word("instead"))
                                        && eat_period_eof(ts)
                                    {
                                        return Ok(wrap_while_cond(
                                            while_cond,
                                            Replacement::DamageRule(
                                                DamageReplacement::ModifyDynamic {
                                                    source: dmg_source,
                                                    target: resolved_target,
                                                    add,
                                                },
                                            ),
                                        ));
                                    }
                                    ts.restore(dcp);
                                }
                            }
                            let sign_opt = if ts.eat_phrase(&["that", "much", "damage", "minus"]) {
                                Some(-1i16)
                            } else if ts.eat_phrase(&["that", "much", "damage", "plus"]) {
                                Some(1i16)
                            } else {
                                None
                            };
                            if let Some(sign) = sign_opt
                                && let Some(n) = ts.next_number(false)
                                && n > 0
                                && let Ok(resolved_target) =
                                    resolve_optional_to_target(ts, &target, false)
                                && (leading_instead || ts.eat_word("instead"))
                            {
                                let _ = ts.eat_period();
                                ts.expect_eof()?;
                                return Ok(wrap_while_cond(
                                    while_cond,
                                    Replacement::ModifyDamage {
                                        source: dmg_source,
                                        target: resolved_target,
                                        add: sign * i16::try_from(n).unwrap_or(1),
                                        factor: 1,
                                    },
                                ));
                            }
                        }
                        ts.restore(deals_cp);
                    }
                    // General damage replacement (`If a source you control would deal [combat] damage to ..., <instead>.`)
                    if min_dmg.is_none()
                        && let Ok(instead) = parse_instead_clause(ts)
                    {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        let ev_src = if src_is_self {
                            SubjectRef::SelfObject
                        } else {
                            SubjectRef::Group(Box::new(src_flt))
                        };
                        return Ok(wrap_while_cond(
                            while_cond,
                            Replacement::EventInstead {
                                event: TriggerEvent::DealsDamage {
                                    combat_only,
                                    source: Some(ev_src),
                                    target,
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

    // 1f-1g. Extended `If ... would ...` replacements (delegated to `if_would_ext.rs`)
    if let Ok(rep) = try_parse(ts, super::if_would_ext::parse_if_would_ext) {
        return Ok(rep);
    }

    // 1h. `If <cond>, it/~ enters with ... / enters tapped`
    {
        let cp = ts.checkpoint();
        let cond_opt = if ts.eat_phrase(&["at", "least"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["mana", "of", "the", "same", "color"])
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
        {
            Some(crate::v2::ir::ConditionExpr::ColoredManaSpent {
                color: crate::ast::ColorMask(0b11111),
                min_count: u8::try_from(n).unwrap_or(3),
            })
        } else {
            ts.restore(cp);
            try_parse(ts, parse_static_condition).ok()
        };
        if let Some(condition) = cond_opt
            && ts.eat_comma()
            && (ts.eat_word("it") || ts.eat_tilde())
        {
            if ts.eat_phrase(&["enters", "tapped"]) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ConditionalEnters {
                        condition,
                        inner: Box::new(Replacement::EntersTapped { unless: None }),
                    });
                }
            } else if ts.eat_phrase(&["enters", "with"]) {
                let mut leading_kws = None;
                {
                    let lkw_cp = ts.checkpoint();
                    if let Ok((kws, _)) = try_parse(ts, crate::v2::keywords::parse_keyword_list)
                        && ts.eat_phrase(&["and", "with"])
                    {
                        leading_kws = Some(kws);
                    } else {
                        ts.restore(lkw_cp);
                    }
                }
                if let Ok(inner) = super::parse_enters_with_counters_body(ts, leading_kws) {
                    return Ok(Replacement::ConditionalEnters {
                        condition,
                        inner: Box::new(inner),
                    });
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
