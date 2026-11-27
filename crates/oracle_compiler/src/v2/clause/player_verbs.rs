//! Player-initiated verb frames, mana additions, damage prevention, and keyword action verbs for `v2`.

use crate::FaceInput;
use crate::v2::card::compile_face_v2;
use crate::v2::clause::subject_verbs::parse_trailing_duration;
use crate::v2::clause::tokens::parse_token_creation;
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_dest,
    parse_zone_ref,
};
use crate::v2::ir::{
    CharacteristicKind, ChoiceKind, Cost, CountSpec, CounterKind, Duration, Effect, EntryModifiers,
    ForEachDomain, LifeOp, ManaSpec, ManaSymbol, ObjectFilter, ObjectMutation, PlayerMutation,
    PlayerRef, PlayerRestrictionKind, StaticAbility, SubjectRef, ValueExpr, ZoneDest, ZoneKind,
    ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Verb frames with a known player actor (`actor`).
pub(super) fn parse_player_verb_frame(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    // 1. `Add <mana> [for each <expr>]` (exact, choice `{W} or {U}` / `{B}, {R}, or {G}`, any color, chosen color)
    if ts.eat_word("add") || ts.eat_word("adds") {
        let base = parse_mana_addition(ts)?;
        let spec = if let Some(scale) = opt(ts, parse_for_each_expr) {
            ManaSpec::Scaled {
                base: Box::new(base),
                scale,
            }
        } else {
            base
        };
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::AddMana(spec),
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 2. `get(s) {E}...` / `get(s) an emblem with "..."`
    if ts.eat_word("get") || ts.eat_word("gets") {
        if ts.eat_phrase(&["an", "emblem", "with"]) {
            let quoted = ts
                .next_quoted()
                .ok_or_else(|| ts.err("expected quoted emblem text"))?;
            let emblem_face = compile_face_v2(
                FaceInput {
                    name: "Emblem",
                    type_line: "Emblem",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !emblem_face.is_ok() {
                return Err(emblem_face
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile emblem text")));
            }
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::CreateEmblem(Box::new(emblem_face)),
                    duration: Duration::Permanent,
                }],
                (),
            ));
        }
        // Energy symbols `{E}{E}{E}`
        let mut energy_count = 0i32;
        while matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(ManaSymbol::Energy))
        ) {
            ts.next_tok();
            energy_count += 1;
        }
        if energy_count > 0 {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::GainCounters {
                        kind: CounterKind::Energy,
                        count: ValueExpr::Const(energy_count),
                    },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        return Err(ts.err("expected emblem or energy after `get(s)`"));
    }

    // 3. `play(s) ...` (both `play lands and cast spells from <zone> [dur]`, `plays with their hand revealed`, and `play that card [without paying its mana cost] [dur]`)
    if ts.eat_word("play") || ts.eat_word("plays") {
        if ts.eat_phrase(&["with", "their", "hand", "revealed"])
            || ts.eat_phrase(&["with", "your", "hand", "revealed"])
        {
            return Ok((
                vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::PlayerRestriction {
                        player: actor,
                        restriction: PlayerRestrictionKind::PlaysWithHandRevealed,
                        condition: None,
                    }),
                    duration: Duration::UntilEndOfTurn,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["lands", "and", "cast", "spells", "from"]) {
            let top_only = ts.eat_phrase(&["the", "top", "of"]);
            let zone = parse_zone_ref(ts)?;
            let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::PlayFromZone {
                        player: actor,
                        zone,
                        top_only,
                        lands: true,
                        spells: true,
                    }),
                    duration: dur,
                }],
                (),
            ));
        }
        let subj = parse_subject_ref(ts)?;
        let free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
            || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
        let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::GrantPlayPermission {
                    cast_only: false,
                    subject: Box::new(subj),
                    free,
                },
                duration: dur,
            }],
            (),
        ));
    }

    // 3b. `can't cast spells [or activate abilities]`
    if ts.eat_phrase(&["can't", "cast", "spells"]) {
        let mut out = vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::PlayerRestriction {
                player: actor,
                restriction: PlayerRestrictionKind::CantCastSpells,
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        }];
        if ts.eat_phrase(&["or", "activate", "abilities"]) {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player: actor,
                    restriction: PlayerRestrictionKind::CantActivateAbilities,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            });
        }
        return Ok((out, ()));
    }

    // 3c. `cast(s) <flt> as though they had flash` OR `cast(s) <subj> [from <zone>] [without paying its mana cost] [dur]`
    if ts.eat_word("cast") || ts.eat_word("casts") {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["as", "though", "they", "had", "flash"])
        {
            return Ok((
                vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::CastAsThoughFlash {
                        player: actor,
                        filter,
                    }),
                    duration: Duration::UntilEndOfTurn,
                }],
                (),
            ));
        }
        ts.restore(cp);
        let cp2 = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            let _ = if ts.eat_word("from") {
                opt(ts, parse_zone_ref)
            } else {
                None
            };
            let free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
            let dur = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::GrantPlayPermission {
                        cast_only: true,
                        subject: Box::new(subj),
                        free,
                    },
                    duration: dur,
                }],
                (),
            ));
        }
        ts.restore(cp2);
    }

    // 3d. `have <subj> deal <amt> damage to <tgt>` (e.g. `You may have ~ deal 2 damage to any target`)
    if ts.eat_word("have") || ts.eat_word("has") {
        let cp = ts.checkpoint();
        if let Ok(source) = try_parse(ts, parse_subject_ref)
            && (ts.eat_word("deal") || ts.eat_word("deals"))
        {
            let amount = if ts.eat_phrase(&["damage", "equal", "to"]) {
                Some(parse_value_expr(ts)?)
            } else if let Ok(amt) = try_parse(ts, parse_value_expr)
                && ts.eat_word("damage")
            {
                Some(amt)
            } else {
                None
            };
            if let Some(amount) = amount
                && ts.eat_word("to")
                && let Ok(target) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![Effect::DealDamage {
                        source,
                        target,
                        amount,
                        divided_by: None,
                    }],
                    (),
                ));
            }
        }
        ts.restore(cp);
    }

    // 3e. `roll a d20` / `roll a d6` / `roll a six-sided die` / `roll <K> d<N>`
    if ts.eat_word("roll") || ts.eat_word("rolls") {
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            1u8
        } else {
            ts.next_number(false)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(1)
        };
        let sides = if ts.eat_phrase(&["six-sided", "die"]) || ts.eat_phrase(&["six-sided", "dice"])
        {
            Some(6u8)
        } else if let Some(w) = ts.peek_word()
            && let Some(rest) = w.strip_prefix('d').or_else(|| w.strip_prefix('D'))
            && let Ok(s) = rest.parse::<u8>()
        {
            ts.next_word();
            Some(s)
        } else {
            None
        };
        if let Some(sides) = sides {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::RollDie { sides, count },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        return Err(ts.err("expected die specification after `roll`"));
    }

    // 3f. `copy(ies) <subj>` (`Copy target instant or sorcery spell`, `Copy target activated or triggered ability`)
    if ts.eat_word("copy") || ts.eat_word("copies") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::CopyObject {
                actor,
                subject,
                count: ValueExpr::Const(1),
                modifiers: EntryModifiers::default(),
            }],
            (),
        ));
    }

    // 3g. `flip(s) a coin`
    if (ts.eat_word("flip") || ts.eat_word("flips")) && ts.eat_phrase(&["a", "coin"]) {
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::FlipCoin,
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 3h. `become(s) the monarch` / `take(s) an extra turn [after this one]`
    if (ts.eat_word("become") || ts.eat_word("becomes")) && ts.eat_phrase(&["the", "monarch"]) {
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::BecomeMonarch,
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if (ts.eat_word("take") || ts.eat_word("takes")) && ts.eat_phrase(&["an", "extra", "turn"]) {
        let _ = ts.eat_phrase(&["after", "this", "one"]);
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::ExtraTurn,
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 3i. `double(s) the number of [<kind>] counters on <subj>`
    if (ts.eat_word("double") || ts.eat_word("doubles")) && ts.eat_phrase(&["the", "number", "of"])
    {
        let kind = if ts.eat_word("counters") || ts.eat_word("counter") {
            None
        } else {
            let ck = parse_counter_kind(ts)?;
            if !(ts.eat_word("counters") || ts.eat_word("counter")) {
                return Err(ts.err("expected `counters` in double counters"));
            }
            Some(ck)
        };
        if !ts.eat_word("on") {
            return Err(ts.err("expected `on` in double counters"));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::DoubleCounters { kind },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 4. `pay(s) <cost>`
    if ts.eat_word("pay") || ts.eat_word("pays") {
        let cost = if let Some(mc) = ts.eat_mana_cost() {
            Cost::Mana(mc)
        } else {
            let amt = parse_value_expr(ts)?;
            if ts.eat_word("life") {
                Cost::PayLife(amt)
            } else {
                return Err(ts.err("expected `life` or mana after `pay`"));
            }
        };
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::PayCost(cost),
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 5. `gain(s) control of <subj> [dur]`
    if ts.eat_phrase(&["gain", "control", "of"]) || ts.eat_phrase(&["gains", "control", "of"]) {
        let subject = parse_subject_ref(ts)?;
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GainControl { controller: actor },
                duration,
            }],
            (),
        ));
    }

    // 6. `gain(s) / lose(s) life ...` (including `loses life and discards cards equal to <val>`)
    if let Some(op) = if ts.eat_word("gain") || ts.eat_word("gains") {
        Some(LifeOp::Gain)
    } else if ts.eat_word("lose") || ts.eat_word("loses") {
        Some(LifeOp::Lose)
    } else {
        None
    } {
        // Paired `loses life and discards cards equal to <val>`
        if op == LifeOp::Lose
            && (ts.eat_phrase(&["life", "and", "discards", "cards", "equal", "to"])
                || ts.eat_phrase(&["life", "and", "discard", "cards", "equal", "to"]))
        {
            let val = parse_value_expr(ts)?;
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::Player(actor),
                    body: vec![
                        Effect::AdjustLife {
                            player: PlayerRef::ThatPlayer,
                            amount: val.clone(),
                            op: LifeOp::Lose,
                        },
                        Effect::ZoneOp {
                            actor: PlayerRef::ThatPlayer,
                            op: ZoneOpKind::Discard {
                                hand: false,
                                random: false,
                            },
                            subject: SubjectRef::Group(Box::new(ObjectFilter {
                                any_card: true,
                                ..Default::default()
                            })),
                            from: vec![ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: ZoneOwner::ThatPlayer,
                            }],
                            to: None,
                            modifiers: EntryModifiers::default(),
                            except: None,
                            count: Some(CountSpec {
                                count: val,
                                up_to: false,
                                any_number: false,
                            }),
                        },
                    ],
                }],
                (),
            ));
        }

        if ts.eat_phrase(&["life", "equal", "to"]) {
            let amount = parse_value_expr(ts)?;
            return Ok((
                vec![Effect::AdjustLife {
                    player: actor,
                    amount,
                    op,
                }],
                (),
            ));
        }
        let base = parse_value_expr(ts)?;
        if ts.eat_word("life") {
            let amount = if let Some(fe) = opt(ts, parse_for_each_expr) {
                if base == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(base), Box::new(fe))
                }
            } else {
                base
            };
            return Ok((
                vec![Effect::AdjustLife {
                    player: actor,
                    amount,
                    op,
                }],
                (),
            ));
        }
        return Err(ts.err("expected `life` after gain/lose"));
    }

    // 7. `draw(s) ...`
    if ts.eat_word("draw") || ts.eat_word("draws") {
        let count = if ts.eat_phrase(&["a", "number", "of", "cards", "equal", "to"])
            || ts.eat_phrase(&["cards", "equal", "to"])
        {
            parse_value_expr(ts)?
        } else {
            let base = if ts.eat_word("a") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let _ = ts.eat_word("additional");
            if !(ts.eat_word("card") || ts.eat_word("cards")) {
                return Err(ts.err("expected `card(s)` after `draw`"));
            }
            let adjusted = if ts.eat_word("plus") {
                ValueExpr::Add(Box::new(base), Box::new(parse_value_expr(ts)?))
            } else if ts.eat_word("minus") {
                ValueExpr::Sub(Box::new(base), Box::new(parse_value_expr(ts)?))
            } else {
                base
            };
            if let Some(fe) = opt(ts, parse_for_each_expr) {
                if adjusted == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(adjusted), Box::new(fe))
                }
            } else {
                adjusted
            }
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Draw,
                subject: SubjectRef::Player(actor),
                from: vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                to: Some(ZoneDest {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::Default,
                    library_position: None,
                    library_order: None,
                    under_control: None,
                }),
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 8. `discard(s) ...`
    if ts.eat_word("discard") || ts.eat_word("discards") {
        if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Discard {
                        hand: true,
                        random: false,
                    },
                    subject: SubjectRef::Player(actor),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
                        owner: ZoneOwner::Default,
                    }],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        let up_to = ts.eat_phrase(&["up", "to"]);
        // Targeted/filtered discard (`discards that card`, `discards a creature card`, `discards a nonland card`)
        {
            let dcp = ts.checkpoint();
            if !up_to && let Ok(subj) = try_parse(ts, parse_subject_ref) {
                let random = ts.eat_phrase(&["at", "random"]);
                if !matches!(subj, SubjectRef::Counted { .. }) {
                    return Ok((
                        vec![Effect::ZoneOp {
                            actor,
                            op: ZoneOpKind::Discard {
                                hand: false,
                                random,
                            },
                            subject: subj,
                            from: vec![ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: ZoneOwner::Default,
                            }],
                            to: None,
                            modifiers: EntryModifiers::default(),
                            except: None,
                            count: None,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(dcp);
        }
        let cnt = if ts.eat_word("a") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        if !(ts.eat_word("card") || ts.eat_word("cards")) {
            return Err(ts.err("expected `card(s)` after `discard`"));
        }
        let random = ts.eat_phrase(&["at", "random"]);
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Discard {
                    hand: false,
                    random,
                },
                subject: SubjectRef::Player(actor),
                from: vec![ZoneRef {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::Default,
                }],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count: cnt,
                    up_to,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 9. `sacrifice(s) ...`
    if ts.eat_word("sacrifice") || ts.eat_word("sacrifices") {
        // Handle anaphoric / `~` / `all <flt>` subjects (`Sacrifice it`, `Sacrifice ~`, `sacrifices all permanents in the pile of their choice`)
        let is_explicit_subject = matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Tilde))
            || ts.peek_word().is_some_and(|w| {
                w.eq_ignore_ascii_case("it")
                    || w.eq_ignore_ascii_case("them")
                    || w.eq_ignore_ascii_case("all")
                    || w.eq_ignore_ascii_case("that")
                    || w.eq_ignore_ascii_case("those")
            });
        if is_explicit_subject {
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Sacrifice,
                    subject,
                    from: vec![ZoneRef {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::Default,
                    }],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let _ = ts.eat_phrase(&["of", "those"]);
        let flt = parse_object_filter(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Sacrifice,
                subject: SubjectRef::Group(Box::new(flt)),
                from: vec![ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                }],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 10. `create(s) <token>`
    if ts.eat_word("create") || ts.eat_word("creates") {
        let (count, spec, modifiers) = parse_token_creation(ts)?;
        return Ok((
            vec![Effect::CreateToken {
                controller: actor,
                count,
                spec,
                modifiers,
            }],
            (),
        ));
    }

    // 11. `destroy(s) <subj>`
    if ts.eat_word("destroy") || ts.eat_word("destroys") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Destroy {
                    can_regenerate: true,
                },
                subject,
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 12. `exile(s) ...`
    if ts.eat_word("exile") || ts.eat_word("exiles") {
        // `exile their hand` / `exile all cards from their hand [face down]`
        if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]) {
            let face_down = ts.eat_phrase(&["face", "down"]);
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down,
                        counters: None,
                    },
                    subject: SubjectRef::All(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![ZoneRef {
                            zone: ZoneKind::Hand,
                            owner: ZoneOwner::ThatPlayer,
                        }],
                        ..Default::default()
                    })),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
                        owner: ZoneOwner::ThatPlayer,
                    }],
                    to: None,
                    modifiers: EntryModifiers {
                        face_down,
                        ..EntryModifiers::default()
                    },
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        // `exile <zone>` (`exile target player's graveyard`, `exile all graveyards`, `exile each opponent's graveyard`)
        {
            let zcp = ts.checkpoint();
            if let Ok(zref) = try_parse(ts, parse_zone_ref)
                && zref.zone == ZoneKind::Graveyard
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Exile {
                            face_down: false,
                            counters: None,
                        },
                        subject: SubjectRef::All(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zref],
                            ..Default::default()
                        })),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(zcp);
        }
        // `exile cards from the top of <lib> until <player> reveal(s)/exile(s) <flt>`
        {
            let ecp = ts.checkpoint();
            if ts.eat_phrase(&["cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("until")
                && opt(ts, parse_player_ref).is_some()
                && (ts.eat_word("reveal")
                    || ts.eat_word("reveals")
                    || ts.eat_word("exile")
                    || ts.eat_word("exiles"))
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::ExileUntilReveal {
                            filter: Box::new(flt),
                        },
                        subject: SubjectRef::Player(actor),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(ecp);
        }
        // `exile a number of cards from the top of <lib> equal to <val>`
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of", "cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Exile {
                            face_down: false,
                            counters: None,
                        },
                        subject: SubjectRef::Group(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zref],
                            ..Default::default()
                        })),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: Some(CountSpec {
                            count,
                            up_to: false,
                            any_number: false,
                        }),
                    }],
                    (),
                ));
            }
            ts.restore(ncp);
        }
        // `exile the top [N] card(s) of <owner>'s library`
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("card") || ts.eat_word("cards")) {
                    return Err(ts.err("expected `card(s)` after `the top <N>`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of` after `the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            let face_down = ts.eat_phrase(&["face", "down"]);
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down,
                        counters: None,
                    },
                    subject: SubjectRef::Group(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![zref],
                        ..Default::default()
                    })),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers {
                        face_down,
                        ..EntryModifiers::default()
                    },
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            ));
        }

        // Compound 3-way exile (`Exile all enchantments from the battlefield, all instant cards from all graveyards, and all artifact cards from all hands`)
        let first = parse_subject_ref(ts)?;
        let mut subjects = vec![first];
        loop {
            let cp = ts.checkpoint();
            if ts.eat_comma() {
                let _ = ts.eat_word("and");
                if ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("all"))
                    && let Ok(next_s) = try_parse(ts, parse_subject_ref)
                {
                    subjects.push(next_s);
                    continue;
                }
            }
            ts.restore(cp);
            break;
        }
        let subject = if subjects.len() == 1 {
            subjects.remove(0)
        } else {
            SubjectRef::Compound(subjects)
        };
        let mut from = Vec::new();
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let face_down = ts.eat_phrase(&["face", "down"]);
        // Optional `with <N> <kind> counters on it` / `with a number of <kind> counters on it equal to <val>`
        let counters = if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let wcp = ts.checkpoint();
            ts.eat_word("with");
            if ts.eat_phrase(&["a", "number", "of"]) {
                let ck = parse_counter_kind(ts)?;
                if (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["on", "it", "equal", "to"])
                {
                    let val = parse_value_expr(ts)?;
                    Some((ck, val))
                } else {
                    ts.restore(wcp);
                    None
                }
            } else if let Ok(val) = try_parse(ts, parse_value_expr)
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
            {
                let _ = ts.eat_phrase(&["on", "it"]);
                Some((ck, val))
            } else {
                ts.restore(wcp);
                None
            }
        } else {
            None
        };
        let ex_dur = parse_trailing_duration(ts);
        let _ = ts.eat_phrase(&[
            "instead",
            "of",
            "putting",
            "it",
            "into",
            "its",
            "owner's",
            "graveyard",
        ]) || ts.eat_phrase(&[
            "instead",
            "of",
            "putting",
            "them",
            "into",
            "their",
            "owners'",
            "graveyards",
        ]);
        let eff = Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Exile {
                face_down,
                counters,
            },
            subject,
            from,
            to: None,
            modifiers: EntryModifiers {
                face_down,
                ..EntryModifiers::default()
            },
            except: None,
            count: None,
        };
        let eff = match ex_dur {
            Some(duration) => Effect::ScopedDuration {
                duration,
                body: vec![eff],
            },
            None => eff,
        };
        return Ok((vec![eff], ()));
    }

    // 13. `return(s) ...` (both `return <subj> [from <zone>] [except <exc>] to <dest> [from <zone>]` AND `returns to their hand <subj>`)
    if ts.eat_word("return") || ts.eat_word("returns") {
        // Inverted destination-first: `returns to their hand each card they exiled this way`
        if ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("to")) {
            let (dest, modifiers) = parse_zone_dest(ts)?;
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        let mut from = Vec::new();
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let except = if ts.eat_word("except") {
            Some(Box::new(parse_subject_ref(ts)?))
        } else {
            None
        };
        let (dest, modifiers) = parse_zone_dest(ts)?;
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject,
                from,
                to: Some(dest),
                modifiers,
                except,
                count: None,
            }],
            (),
        ));
    }

    // 14. `put(s) ...` (BOTH `put <cnt> <kind> counter(s) on <subj>` AND `put <subj> into/onto/on <dest>`)
    if ts.eat_word("put") || ts.eat_word("puts") {
        // 14a. `put a number of <kind> counters on <subj> equal to <val>`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
            {
                let subject = parse_subject_ref(ts)?;
                if ts.eat_phrase(&["equal", "to"]) {
                    let count = parse_value_expr(ts)?;
                    return Ok((
                        vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::PutCounters { kind, count },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(cp);
        }
        // 14b. `put <cnt> <kind> counter(s) [and <cnt2> <kind2> counter(s)] on <subj> [for each/every ...]`
        {
            let cp = ts.checkpoint();
            let base_cnt = if ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("another") {
                Some(ValueExpr::Const(1))
            } else {
                opt(ts, parse_value_expr)
            };
            if let Some(base) = base_cnt
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                let mut pairs = vec![(base, kind)];
                loop {
                    let acp = ts.checkpoint();
                    if ts.eat_word("and") {
                        let c2 = if ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("another")
                        {
                            Some(ValueExpr::Const(1))
                        } else {
                            opt(ts, parse_value_expr)
                        };
                        if let Some(c2) = c2
                            && let Ok(k2) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counter") || ts.eat_word("counters"))
                        {
                            pairs.push((c2, k2));
                            continue;
                        }
                    }
                    ts.restore(acp);
                    break;
                }
                if ts.eat_word("on") {
                    let subject = parse_subject_ref(ts)?;
                    let fe = opt(ts, parse_for_each_expr);
                    let mut out = Vec::with_capacity(pairs.len());
                    for (b, k) in pairs {
                        let count = if let Some(ref f) = fe {
                            if b == ValueExpr::Const(1) {
                                f.clone()
                            } else {
                                ValueExpr::Mul(Box::new(b), Box::new(f.clone()))
                            }
                        } else {
                            b
                        };
                        out.push(Effect::MutateObject {
                            subject: subject.clone(),
                            mutation: ObjectMutation::PutCounters { kind: k, count },
                            duration: Duration::Instant,
                        });
                    }
                    return Ok((out, ()));
                }
            }
            ts.restore(cp);
        }
        // 14c. `put them back in any order` / `put <subj> on top` / `put <subj> [from <zone>] into/onto/on <dest> [from <zone>]`
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&["back", "in", "any", "order"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(ZoneDest {
                        zone: ZoneKind::Library,
                        owner: ZoneOwner::Default,
                        library_position: Some(crate::v2::ir::LibraryPosition::Top),
                        library_order: Some(crate::v2::ir::LibraryOrder::AnyOrder),
                        under_control: None,
                    }),
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        {
            let t_cp = ts.checkpoint();
            if ts.eat_phrase(&["on", "top"])
                && !ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("of"))
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject,
                        from: Vec::new(),
                        to: Some(ZoneDest {
                            zone: ZoneKind::Library,
                            owner: ZoneOwner::Default,
                            library_position: Some(crate::v2::ir::LibraryPosition::Top),
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(t_cp);
        }
        let mut from = Vec::new();
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let (dest, modifiers) = parse_zone_dest(ts)?;
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let mut out = vec![Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Move,
            subject,
            from,
            to: Some(dest),
            modifiers,
            except: None,
            count: None,
        }];
        let cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(s2) = try_parse(ts, parse_subject_ref)
            && let Ok((d2, m2)) = try_parse(ts, parse_zone_dest)
        {
            out.push(Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: s2,
                from: Vec::new(),
                to: Some(d2),
                modifiers: m2,
                except: None,
                count: None,
            });
        } else {
            ts.restore(cp);
        }
        return Ok((out, ()));
    }

    // 15. `move(s) <cnt> [<kind>] counter(s) from <src> onto/to <dst>` (NEVER stores `"a"`!)
    if ts.eat_word("move") || ts.eat_word("moves") {
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        let kind = if ts.eat_word("counter") || ts.eat_word("counters") {
            CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !(ts.eat_word("counter") || ts.eat_word("counters")) {
                return Err(ts.err("expected `counter(s)` after counter kind"));
            }
            ck
        };
        if !ts.eat_word("from") {
            return Err(ts.err("expected `from` in move counters"));
        }
        let from = parse_subject_ref(ts)?;
        if !(ts.eat_word("onto") || ts.eat_word("to")) {
            return Err(ts.err("expected `onto` or `to` in move counters"));
        }
        let to = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject: from.clone(),
                mutation: ObjectMutation::MoveCounters {
                    from: Box::new(from),
                    to: Box::new(to),
                    kind,
                    count,
                },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 16. `distribute a number of <kind> counters equal to <val> among <subj>`
    if ts.eat_word("distribute") || ts.eat_word("distributes") {
        let count_first = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else {
            Some(parse_value_expr(ts)?)
        };
        let kind = parse_counter_kind(ts)?;
        if !(ts.eat_word("counters") || ts.eat_word("counter")) {
            return Err(ts.err("expected `counters` in distribute"));
        }
        let count = match count_first {
            Some(c) => c,
            None => {
                if !ts.eat_phrase(&["equal", "to"]) {
                    return Err(ts.err("expected `equal to` in distribute"));
                }
                parse_value_expr(ts)?
            }
        };
        if !ts.eat_word("among") {
            return Err(ts.err("expected `among` in distribute"));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::DistributeCounters { kind, count },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 17. `scry(ies) <N>` / `surveil(s) <N>` / `shuffle`
    if ts.eat_word("scry") || ts.eat_word("scries") {
        let amt = parse_value_expr(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Scry(amt),
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("surveil") || ts.eat_word("surveils") {
        let amt = parse_value_expr(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Surveil(amt),
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("shuffle") || ts.eat_word("shuffles") {
        // `shuffles their hand and graveyard into their library` / `shuffle <subj> into <lib>`
        {
            let scp = ts.checkpoint();
            if (ts.eat_phrase(&["their", "hand", "and", "graveyard"])
                || ts.eat_phrase(&["your", "hand", "and", "graveyard"]))
                && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject: SubjectRef::HandOf(actor),
                        from: vec![
                            ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: zref.owner,
                            },
                            ZoneRef {
                                zone: ZoneKind::Graveyard,
                                owner: zref.owner,
                            },
                        ],
                        to: Some(ZoneDest {
                            zone: ZoneKind::Library,
                            owner: zref.owner,
                            library_position: Some(crate::v2::ir::LibraryPosition::Shuffled),
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(scp);
        }
        // `shuffle <zone> into <lib>` (`shuffle your graveyard into your library`)
        {
            let scp = ts.checkpoint();
            if let Ok(from_z) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("into")
                && let Ok(to_z) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject: SubjectRef::All(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![from_z],
                            ..Default::default()
                        })),
                        from: vec![from_z],
                        to: Some(ZoneDest {
                            zone: to_z.zone,
                            owner: to_z.owner,
                            library_position: Some(crate::v2::ir::LibraryPosition::Shuffled),
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(scp);
        }
        {
            let scp = ts.checkpoint();
            if let Ok(subject) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("into")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Shuffle,
                        subject,
                        from: Vec::new(),
                        to: Some(ZoneDest {
                            zone: zref.zone,
                            owner: zref.owner,
                            library_position: Some(crate::v2::ir::LibraryPosition::Shuffled),
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(scp);
        }
        let _ = ts.eat_phrase(&["your", "library"]) || ts.eat_phrase(&["their", "library"]);
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Shuffle,
                subject: SubjectRef::Player(actor),
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 18. `search your library [and/or graveyard] for [any number of | up to | exactly] <N|X> <flt>`
    if ts.eat_word("search") || ts.eat_word("searches") {
        let mut libs = vec![parse_zone_ref(ts)?];
        if (ts.eat_word("and/or") || ts.eat_word("and") || ts.eat_word("or"))
            && let Ok(z2) = try_parse(ts, parse_zone_ref)
        {
            libs.push(z2);
        }
        if !ts.eat_word("for") {
            return Err(ts.err("expected `for` in search library"));
        }
        let any_number = ts.eat_phrase(&["any", "number", "of"]);
        let up_to = !any_number && ts.eat_phrase(&["up", "to"]);
        let exact = !any_number && !up_to && ts.eat_word("exactly");
        let count = if any_number {
            ValueExpr::Const(0)
        } else if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let flt = parse_object_filter(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::SearchLibrary {
                    reveal: false,
                    shuffle: false,
                    exact,
                },
                subject: SubjectRef::Group(Box::new(flt)),
                from: libs,
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: up_to || (!exact && !any_number),
                    any_number,
                }),
            }],
            (),
        ));
    }

    // 19. `reveal(s) [cards from the top of <lib> until ... | the top <N> cards of <lib> | <subj>]` / `look(s) at ...`
    if ts.eat_word("reveal") || ts.eat_word("reveals") {
        {
            let rcp = ts.checkpoint();
            if ts.eat_phrase(&["cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("until")
                && opt(ts, parse_player_ref).is_some()
                && (ts.eat_word("reveal") || ts.eat_word("reveals"))
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::ExileUntilReveal {
                            filter: Box::new(flt),
                        },
                        subject: SubjectRef::Player(actor),
                        from: vec![zref],
                        to: None,
                        modifiers: EntryModifiers::default(),
                        except: None,
                        count: None,
                    }],
                    (),
                ));
            }
            ts.restore(rcp);
        }
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("cards") || ts.eat_word("card")) {
                    return Err(ts.err("expected `card(s)` after `reveal the top`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of` after `reveal the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: true },
                    subject: SubjectRef::Player(actor),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Reveal,
                subject,
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }
    if ts.eat_phrase(&["look", "at"]) || ts.eat_phrase(&["looks", "at"]) {
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("cards") || ts.eat_word("card")) {
                    return Err(ts.err("expected `card(s)` after `look at the top`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of <library>` after `look at the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: false },
                    subject: SubjectRef::Player(actor),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            ));
        }
        // `look at <val> cards from the top of <lib>` (e.g. `Look at that many cards from the top of your library`)
        let lcp = ts.checkpoint();
        if let Ok(count) = try_parse(ts, parse_value_expr)
            && (ts.eat_word("cards") || ts.eat_word("card"))
            && ts.eat_phrase(&["from", "the", "top", "of"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: false },
                    subject: SubjectRef::Player(actor),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            ));
        }
        ts.restore(lcp);
        // `look at <subj>` (`Look at target opponent's hand`, `Look at a card at random in target player's hand`)
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookAt,
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        return Err(ts.err("expected target after `look at`"));
    }

    // 20. `separate(s) them into a face-down pile and a face-up pile`
    if ts.eat_word("separate") || ts.eat_word("separates") {
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&[
            "into",
            "a",
            "face-down",
            "pile",
            "and",
            "a",
            "face-up",
            "pile",
        ]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 1,
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["into", "two", "piles"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 0,
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        return Err(ts.err("expected pile specification after `separate`"));
    }

    // 21. `choose(s) <new targets for <subj> | characteristic | target>`
    if ts.eat_word("choose") || ts.eat_word("chooses") {
        if ts.eat_phrase(&["new", "targets", "for"]) {
            let subj = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::ChooseNewTargets(Box::new(subj)),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "creature", "type"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CreatureType),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "card", "type"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CardType),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "color"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Color),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "card", "name"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CardName),
                }],
                (),
            ));
        }
        // Optional leading count (`choose two cards in your hand drawn this turn`)
        let _ = opt(ts, parse_value_expr);
        let subj = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::PromptChoice {
                chooser: actor,
                kind: ChoiceKind::ChooseObject(subj),
            }],
            (),
        ));
    }

    // 22. `tap or untap <subj>` / `untap(s) <subj>` / `tap(s) <subj>`
    if ts.eat_phrase(&["tap", "or", "untap"]) || ts.eat_phrase(&["taps", "or", "untaps"]) {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::PromptChoice {
                chooser: actor,
                kind: ChoiceKind::ChooseBranch(vec![
                    vec![Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::Tap,
                        duration: Duration::Instant,
                    }],
                    vec![Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::Untap,
                        duration: Duration::Instant,
                    }],
                ]),
            }],
            (),
        ));
    }
    if ts.eat_word("untap") || ts.eat_word("untaps") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::Untap,
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("tap") || ts.eat_word("taps") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::Tap,
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 23. `turn(s) face up <subj>` / `turn(s) <subj> face up`
    if ts.eat_word("turn") || ts.eat_word("turns") {
        if ts.eat_phrase(&["face", "up"]) {
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::TurnFaceUp,
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&["face", "up"]) {
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::TurnFaceUp,
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["face", "down"]) {
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::TurnFaceDown,
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
    }

    // 24. `counter <subj> [unless its controller pays <cost>]`
    if ts.eat_word("counter") {
        let subject = parse_subject_ref(ts)?;
        let unless_pay = if ts.eat_phrase(&["unless", "its", "controller", "pays"]) {
            Some(Box::new(parse_cost_atom(ts)?))
        } else {
            None
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::CounterSpell { unless_pay },
                subject,
                from: vec![ZoneRef {
                    zone: ZoneKind::Stack,
                    owner: ZoneOwner::Default,
                }],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 25. `mill(s) <N> card(s)`
    if ts.eat_word("mill") || ts.eat_word("mills") {
        let count = if ts.eat_word("a") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        if !(ts.eat_word("card") || ts.eat_word("cards")) {
            return Err(ts.err("expected `card(s)` after `mill`"));
        }
        let final_cnt = if let Some(fe) = opt(ts, parse_for_each_expr) {
            if count == ValueExpr::Const(1) {
                fe
            } else {
                ValueExpr::Mul(Box::new(count), Box::new(fe))
            }
        } else {
            count
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Mill,
                subject: SubjectRef::Player(actor),
                from: vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                to: Some(ZoneDest {
                    zone: ZoneKind::Graveyard,
                    owner: ZoneOwner::Default,
                    library_position: None,
                    library_order: None,
                    under_control: None,
                }),
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count: final_cnt,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 26. `regenerate(s) <subj>` / `transform(s) <subj>` / `attach(es) <subj> to <host>` / `remove(s) <N> <kind> counter(s) from <subj>`
    if ts.eat_word("regenerate") || ts.eat_word("regenerates") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::Regenerate,
                duration: Duration::UntilEndOfTurn,
            }],
            (),
        ));
    }
    if ts.eat_word("transform") || ts.eat_word("transforms") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::Transform,
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("attach") || ts.eat_word("attaches") {
        let subject = parse_subject_ref(ts)?;
        if !ts.eat_word("to") {
            return Err(ts.err("expected `to` in attach clause"));
        }
        let host = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::AttachTo(Box::new(host)),
                duration: Duration::Permanent,
            }],
            (),
        ));
    }
    if ts.eat_word("remove") || ts.eat_word("removes") {
        let (all, count) = if ts.eat_word("all") {
            (true, None)
        } else if ts.eat_word("a") || ts.eat_word("an") {
            (false, Some(ValueExpr::Const(1)))
        } else {
            (false, Some(parse_value_expr(ts)?))
        };
        let kind = if ts.eat_word("counter") || ts.eat_word("counters") {
            CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !(ts.eat_word("counter") || ts.eat_word("counters")) {
                return Err(ts.err("expected `counter(s)` after counter kind in remove"));
            }
            ck
        };
        if !ts.eat_word("from") {
            return Err(ts.err("expected `from` in remove counters"));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::RemoveCounters { kind, count, all },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 27. `prevent ... damage ...`
    if ts.eat_word("prevent") || ts.eat_word("prevents") {
        return Ok((vec![parse_prevent_damage(ts)?], ()));
    }

    // 28. Standalone keyword action verbs (`proliferate`, `investigate`, `populate`, `amass`, `adapt`, `goad`, etc.)
    if let Some(eff) = opt(ts, |s| parse_keyword_action_verb(s, actor)) {
        return Ok((vec![eff], ()));
    }

    Err(ts.err("expected player verb frame"))
}

fn parse_mana_addition(ts: &mut TokenStream<'_>) -> PResult<ManaSpec> {
    let _ = ts.eat_phrase(&["an", "additional"]);
    if ts.eat_phrase(&["one", "mana", "of", "the", "chosen", "color"]) {
        return Ok(ManaSpec::ChosenColor(ValueExpr::Const(1)));
    }
    if ts.eat_phrase(&["one", "mana", "of", "any", "color"]) {
        return Ok(ManaSpec::AnyColor {
            amount: ValueExpr::Const(1),
            single_color: true,
        });
    }
    {
        let cp = ts.checkpoint();
        if let Ok(amount) = try_parse(ts, parse_value_expr)
            && ts.eat_word("mana")
        {
            if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
                return Ok(ManaSpec::ChosenColor(amount));
            }
            if ts.eat_phrase(&["of", "any", "one", "color"])
                || ts.eat_phrase(&["of", "any", "color"])
            {
                return Ok(ManaSpec::AnyColor {
                    amount,
                    single_color: true,
                });
            }
            if ts.eat_phrase(&["in", "any", "combination", "of", "colors"]) {
                return Ok(ManaSpec::AnyColor {
                    amount,
                    single_color: false,
                });
            }
            if ts.eat_phrase(&["of", "different", "colors"]) {
                return Ok(ManaSpec::DifferentColors(amount));
            }
        }
        ts.restore(cp);
    }
    if let Some(first) = ts.eat_mana_cost() {
        let mut choices = vec![first];
        loop {
            let cp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_or = ts.eat_word("or");
            if (had_comma || had_or)
                && let Some(next_mc) = ts.eat_mana_cost()
            {
                choices.push(next_mc);
                continue;
            }
            ts.restore(cp);
            break;
        }
        return Ok(if choices.len() == 1 {
            ManaSpec::Exact(choices.remove(0))
        } else {
            ManaSpec::Choice(choices)
        });
    }
    Err(ts.err("expected mana spec after `add`"))
}

fn parse_prevent_damage(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let (amount, combat_only) = if ts.eat_phrase(&["all", "combat", "damage"]) {
        (None, true)
    } else if ts.eat_phrase(&["all", "damage"]) {
        (None, false)
    } else if ts.eat_phrase(&["the", "next"]) {
        let amt = parse_value_expr(ts)?;
        let combat = ts.eat_word("combat");
        if !ts.eat_word("damage") {
            return Err(ts.err("expected `damage` after `prevent the next <N>`"));
        }
        (Some(amt), combat)
    } else {
        return Err(ts.err("expected `all [combat] damage` or `the next <N> damage`"));
    };

    let _ = ts.eat_phrase(&["that", "would", "be", "dealt"]);
    let mut source = None;
    let mut target = None;
    let mut duration = Duration::UntilEndOfTurn;

    loop {
        if ts.eat_word("to") {
            target = Some(parse_subject_ref(ts)?);
            continue;
        }
        if ts.eat_word("by") {
            if ts.eat_phrase(&["a", "source", "of", "your", "choice"])
                || ts.eat_phrase(&["a", "source", "of", "their", "choice"])
            {
                source = Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatObject));
            } else {
                source = Some(parse_subject_ref(ts)?);
            }
            continue;
        }
        if let Some(d) = parse_trailing_duration(ts) {
            duration = d;
            continue;
        }
        break;
    }

    Ok(Effect::PreventDamage {
        amount,
        combat_only,
        source,
        target,
        duration,
    })
}

fn parse_keyword_action_verb(ts: &mut TokenStream<'_>, actor: PlayerRef) -> PResult<Effect> {
    if ts.eat_word("proliferate") || ts.eat_word("proliferates") {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Proliferate,
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("investigate") || ts.eat_word("investigates") {
        let amt = if ts.eat_word("twice") {
            ValueExpr::Const(2)
        } else {
            ValueExpr::Const(1)
        };
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Investigate(amt),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("populate") || ts.eat_word("populates") {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Populate,
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("forage") || ts.eat_word("forages") {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Forage,
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("adapt") || ts.eat_word("adapts") {
        let n = ts
            .next_number(false)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| ts.err("expected number after `adapt`"))?;
        return Ok(Effect::MutateObject {
            subject: SubjectRef::SelfObject,
            mutation: ObjectMutation::Adapt(n),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("amass") || ts.eat_word("amasses") {
        let subtype = if let Some(w) = ts.peek_word()
            && let Some(sub) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase())
        {
            ts.next_word();
            sub.to_string()
        } else {
            "Army".to_string()
        };
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Amass { subtype, amount },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("incubate") || ts.eat_word("incubates") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Incubate(amount),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("discover") || ts.eat_word("discovers") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Discover(amount),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("bolster") || ts.eat_word("bolsters") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Bolster(amount),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("support") || ts.eat_word("supports") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Support(amount),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("fateseal") || ts.eat_word("fateseals") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Fateseal(amount),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["manifest", "dread"]) || ts.eat_phrase(&["manifests", "dread"]) {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Manifest { dread: true },
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["venture", "into", "the", "dungeon"])
        || ts.eat_phrase(&["ventures", "into", "the", "dungeon"])
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::VentureIntoDungeon,
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["the", "ring", "tempts", "you"]) {
        return Ok(Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::RingTemptsYou,
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["collect", "evidence"]) {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::CollectEvidence(amount),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("goad") || ts.eat_word("goads") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Goad,
            duration: Duration::UntilYourNextTurn,
        });
    }
    if ts.eat_word("detain") || ts.eat_word("detains") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Detain,
            duration: Duration::UntilYourNextTurn,
        });
    }
    if ts.eat_word("suspect") || ts.eat_word("suspects") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Suspect,
            duration: Duration::Permanent,
        });
    }
    if ts.eat_word("exert") || ts.eat_word("exerts") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Exert,
            duration: Duration::Instant,
        });
    }
    Err(ts.err("not a keyword action verb"))
}
