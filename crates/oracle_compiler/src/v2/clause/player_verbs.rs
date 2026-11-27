//! Player-initiated verb frames, mana additions, damage prevention, and keyword action verbs for `v2`.

use crate::FaceInput;
use crate::v2::card::compile_face_v2;
use crate::v2::clause::player_actions::{
    parse_keyword_action_verb, parse_mana_addition, parse_prevent_damage,
};
use crate::v2::clause::subject_verbs::parse_trailing_duration;
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    CastPermissionMode, CharacteristicKind, ChoiceKind, ConditionFlag, Cost, CountSpec,
    CounterKind, Duration, Effect, EntryModifiers, ForEachDomain, LifeOp, ManaSpec, ManaSymbol,
    ObjectFilter, ObjectMutation, ObjectStateAction, PlayerActionKind, PlayerMutation, PlayerRef,
    PlayerRestrictionKind, StaticAbility, SubjectRef, ValueExpr, ZoneDest, ZoneKind, ZoneOpKind,
    ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Verb frames with a known player actor (`actor`).
pub(super) fn parse_player_verb_frame(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    // 1. `double the power/toughness/number of counters ...` (falls through to section 3i for `double <subj>'s power [and toughness] [<val> times]`)
    let outer_dcp = ts.checkpoint();
    if ts.eat_word("double") || ts.eat_word("doubles") {
        if let Some(res) = super::player_verbs_ext::try_parse_double_ext(ts, actor) {
            return res;
        }
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "power", "and", "toughness", "of"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::DoubleStat {
                        power: true,
                        toughness: true,
                    },
                    duration,
                }],
                (),
            ));
        }
        ts.restore(dcp);
        if ts.eat_phrase(&["the", "power", "of"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::DoubleStat {
                        power: true,
                        toughness: false,
                    },
                    duration,
                }],
                (),
            ));
        }
        ts.restore(dcp);
        if ts.eat_phrase(&["the", "toughness", "of"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::DoubleStat {
                        power: false,
                        toughness: true,
                    },
                    duration,
                }],
                (),
            ));
        }
        ts.restore(dcp);
        if ts.eat_phrase(&["the", "number", "of"]) {
            let kind = if ts.eat_phrase(&["each", "kind", "of", "counter", "on"])
                || ts.eat_phrase(&["each", "kind", "of", "counters", "on"])
            {
                Some(None)
            } else if let Ok(k) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
            {
                Some(Some(k))
            } else if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_word("on") {
                Some(None)
            } else {
                None
            };
            if let Some(kind) = kind
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::DoubleCounters { kind },
                        duration: Duration::Instant,
                    }],
                    (),
                ));
            }
        }
        if ts.eat_phrase(&["its", "power", "and", "toughness"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                    mutation: ObjectMutation::DoubleStat {
                        power: true,
                        toughness: true,
                    },
                    duration,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["its", "power"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                    mutation: ObjectMutation::DoubleStat {
                        power: true,
                        toughness: false,
                    },
                    duration,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["its", "toughness"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                    mutation: ObjectMutation::DoubleStat {
                        power: false,
                        toughness: true,
                    },
                    duration,
                }],
                (),
            ));
        }
        ts.restore(outer_dcp);
    }

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
        if let Some(res) = super::player_verbs_ext::try_parse_get_ext(ts, actor) {
            return res;
        }
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
        // `an amount of {E} equal to <val>` (`Peema Aether-Seer`, `Robobrain War Mind`, `Electrosiphon`)
        {
            let ecp = ts.checkpoint();
            if ts.eat_phrase(&["an", "amount", "of"])
                && matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Mana(ManaSymbol::Energy))
                )
            {
                ts.next_tok();
                if ts.eat_phrase(&["equal", "to"])
                    && let Ok(count) = try_parse(ts, parse_value_expr)
                {
                    return Ok((
                        vec![Effect::MutatePlayer {
                            player: actor,
                            mutation: PlayerMutation::GainCounters {
                                kind: CounterKind::Energy,
                                count,
                            },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(ecp);
        }
        // `<val> {E}` (`you get that many {E}`, `you get X {E}`) or `<val> {TK}` (`Demonic Tourist Laser`)
        {
            let vecp = ts.checkpoint();
            if let Ok(count) = try_parse(ts, parse_value_expr) {
                if matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Mana(ManaSymbol::Energy))
                ) {
                    ts.next_tok();
                    return Ok((
                        vec![Effect::MutatePlayer {
                            player: actor,
                            mutation: PlayerMutation::GainCounters {
                                kind: CounterKind::Energy,
                                count,
                            },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
                if matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Mana(ManaSymbol::Ticket))
                ) {
                    ts.next_tok();
                    return Ok((
                        vec![Effect::MutatePlayer {
                            player: actor,
                            mutation: PlayerMutation::GainCounters {
                                kind: CounterKind::Named("ticket".to_string()),
                                count,
                            },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(vecp);
        }
        // `a number of <kind> counters equal to <val>` (`Feral Ghoul`)
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
            {
                return Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::GainCounters { kind, count },
                        duration: Duration::Instant,
                    }],
                    (),
                ));
            }
            ts.restore(ncp);
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
        // Ticket symbols `{TK}{TK}` (Unfinity)
        let mut ticket_count = 0i32;
        while matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(ManaSymbol::Ticket))
        ) {
            ts.next_tok();
            ticket_count += 1;
        }
        if ticket_count > 0 {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::GainCounters {
                        kind: CounterKind::Named("ticket".to_string()),
                        count: ValueExpr::Const(ticket_count),
                    },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        // `get(s) [a/an/another/an additional/<val>] <kind> counter(s)`
        {
            let gcp = ts.checkpoint();
            let count = if ts.eat_phrase(&["an", "additional"])
                || ts.eat_word("another")
                || ts.eat_word("a")
                || ts.eat_word("an")
            {
                Some(ValueExpr::Const(1))
            } else {
                try_parse(ts, parse_value_expr).ok()
            };
            if let Some(count) = count
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                return Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::GainCounters { kind, count },
                        duration: Duration::Instant,
                    }],
                    (),
                ));
            }
            ts.restore(gcp);
        }
        return Err(ts.err("expected emblem, energy, or counter after `get(s)`"));
    }

    // 3. `play(s) ...` (both `play lands and cast spells from <zone> [dur]`, `plays with their hand revealed`, and `play that card [without paying its mana cost] [dur]`)
    if ts.eat_word("play") || ts.eat_word("plays") {
        if ts.eat_phrase(&["an", "additional", "land"]) {
            let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::AdditionalLandPlay(1),
                    duration: dur,
                }],
                (),
            ));
        }
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
                    effect: Box::new(StaticAbility::ZoneCastPermission {
                        player: actor,
                        filter: None,
                        subject: None,
                        zone: Some(zone),
                        mode: CastPermissionMode::PlayFromZone {
                            top_only,
                            lands: true,
                            spells: true,
                        },
                        condition: None,
                    }),
                    duration: dur,
                }],
                (),
            ));
        }
        let subj = parse_subject_ref(ts)?;
        if ts.eat_word("from") {
            let fcp = ts.checkpoint();
            if super::player_verbs_ext::eat_extended_cast_source(ts) {
                // `play <subj> from among cards exiled this way / from the top of <lib>`
            } else if ts.eat_word("among") && try_parse(ts, parse_subject_ref).is_ok() {
                // `play <subj> from among <cards>`
            } else {
                ts.restore(fcp);
                let _ = opt(ts, parse_zone_ref);
            }
        }
        let mut free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
            || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
        let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        if !free {
            free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
        }
        let mut out = vec![Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::GrantPlayPermission {
                cast_only: false,
                subject: Box::new(subj),
                free,
            },
            duration: dur.clone(),
        }];
        let m_cp = ts.checkpoint();
        let _ = ts.eat_comma();
        if (ts.eat_phrase(&[
            "and", "mana", "of", "any", "type", "can", "be", "spent", "to", "cast",
        ]) || ts.eat_phrase(&[
            "and", "you", "may", "spend", "mana", "as", "though", "it", "were", "mana", "of",
            "any", "color", "to", "cast",
        ]) || ts.eat_phrase(&[
            "and", "you", "may", "spend", "mana", "as", "though", "it", "were", "mana", "of",
            "any", "type", "to", "cast",
        ])) && (ts.eat_phrase(&["those", "spells"])
            || ts.eat_phrase(&["that", "spell"])
            || ts.eat_word("it")
            || ts.eat_word("them"))
        {
            out.push(Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::SpendManaAsAnyType),
                duration: dur,
            });
        } else {
            ts.restore(m_cp);
            super::player_verbs_ext::parse_trailing_play_mana_rider(ts, actor, dur, &mut out);
        }
        return Ok((out, ()));
    }

    // 3b0. `can't play cards from their/your hand` (`Memory Vessel`)
    if ts.eat_phrase(&["can't", "play", "cards", "from", "their", "hand"])
        || ts.eat_phrase(&["can't", "play", "cards", "from", "your", "hand"])
    {
        return Ok((
            vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player: actor,
                    restriction: PlayerRestrictionKind::CantPlayFromHand,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            }],
            (),
        ));
    }

    // 3b. `can't cast spells [or activate abilities] [dur]` (`Xantid Swarm`, `Silence`, `Orim's Chant`)
    if ts.eat_phrase(&["can't", "cast", "spells"]) {
        let mut has_activate = false;
        if ts.eat_phrase(&["or", "activate", "abilities"]) {
            if ts.eat_word("of") {
                let _ = opt(ts, parse_object_filter);
            }
            has_activate = true;
        }
        let duration = crate::v2::clause::subject_verbs::parse_trailing_duration(ts)
            .unwrap_or(Duration::UntilEndOfTurn);
        let mut out = vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::PlayerRestriction {
                player: actor,
                restriction: PlayerRestrictionKind::CantCastSpells,
                condition: None,
            }),
            duration: duration.clone(),
        }];
        if has_activate {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player: actor,
                    restriction: PlayerRestrictionKind::CantActivateAbilities,
                    condition: None,
                }),
                duration,
            });
        }
        return Ok((out, ()));
    }

    // 3c. `cast(s) <flt> [this turn] as though they had flash` OR `cast(s) <subj> [from <zone>] [without paying its mana cost] [dur]`
    if ts.eat_word("cast") || ts.eat_word("casts") {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter) {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_phrase(&["as", "though", "they", "had", "flash"]) {
                let _ = ts.eat_phrase(&["this", "turn"]);
                return Ok((
                    vec![Effect::ApplyContinuous {
                        effect: Box::new(StaticAbility::ZoneCastPermission {
                            player: actor,
                            filter: Some(filter),
                            subject: None,
                            zone: None,
                            mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                            condition: None,
                        }),
                        duration: Duration::UntilEndOfTurn,
                    }],
                    (),
                ));
            }
        }
        ts.restore(cp);
        // `cast (any number of | up to <val> of) [the] copies [without paying their mana costs]` (`Arcane Bombardment`, `Wizard's Spellbook`, `Baron Helmut Zemo`)
        {
            let acp = ts.checkpoint();
            if (ts.eat_phrase(&["any", "number", "of"])
                || (ts.eat_phrase(&["up", "to"])
                    && opt(ts, parse_value_expr).is_some()
                    && ts.eat_word("of")))
                && (ts.eat_phrase(&["the", "copies"]) || ts.eat_word("copies"))
            {
                let free = ts.eat_phrase(&["without", "paying", "their", "mana", "costs"])
                    || ts.eat_phrase(&["without", "paying", "its", "mana", "cost"]);
                return Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::GrantPlayPermission {
                            cast_only: true,
                            subject: Box::new(SubjectRef::Anaphora(
                                crate::v2::ir::AnaphoraRef::ThatSpell,
                            )),
                            free,
                        },
                        duration: Duration::UntilEndOfTurn,
                    }],
                    (),
                ));
            }
            ts.restore(acp);
        }
        let cp2 = ts.checkpoint();
        let _ = ts.eat_phrase(&["any", "number", "of"])
            || (ts.eat_phrase(&["up", "to"]) && opt(ts, parse_value_expr).is_some());
        let subj_opt = if ts.eat_phrase(&["the", "other", "cards"])
            || ts.eat_phrase(&["the", "other", "card"])
        {
            Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::OtherPile))
        } else {
            try_parse(ts, parse_subject_ref).ok()
        };
        if let Some(subj) = subj_opt {
            if ts.eat_word("from") {
                loop {
                    let f1_cp = ts.checkpoint();
                    if super::player_verbs_ext::eat_extended_cast_source(ts) {
                        // `from among cards exiled this way / one of those piles / the top of <zone>`
                    } else if ts.eat_word("among") {
                        let acp = ts.checkpoint();
                        if (ts.eat_phrase(&["cards", "you", "own", "in"])
                            || ts.eat_phrase(&["cards", "in"]))
                            && opt(ts, parse_zone_ref).is_some()
                        {
                            // `from among cards [you own] in <zone>`
                        } else {
                            ts.restore(acp);
                            if try_parse(ts, parse_subject_ref).is_ok() {
                                if ts.eat_word("from") {
                                    let _ = opt(ts, parse_zone_ref);
                                }
                            } else {
                                ts.restore(f1_cp);
                                let _ = opt(ts, parse_zone_ref);
                            }
                        }
                    } else {
                        let _ = opt(ts, parse_zone_ref);
                    }
                    let fcp = ts.checkpoint();
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("or") || ts.eat_word("and");
                    if ts.eat_word("from") {
                        continue;
                    }
                    ts.restore(fcp);
                    break;
                }
            }
            let _ = ts.eat_phrase(&["if", "able"]);
            let mut free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
            let alt_costs = if !free && ts.eat_word("by") {
                let costs = crate::v2::cost::parse_cost_list(ts).unwrap_or_default();
                let _ = ts.eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"])
                    || ts.eat_phrase(&["rather", "than", "paying", "their", "mana", "costs"])
                    || ts.eat_phrase(&["in", "addition", "to", "paying", "its", "other", "costs"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "costs"]);
                costs
            } else {
                Vec::new()
            };
            let dur = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
            if !free {
                free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                    || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
            }
            let mut out = vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::GrantPlayPermission {
                    cast_only: true,
                    subject: Box::new(subj),
                    free,
                },
                duration: dur.clone(),
            }];
            if !alt_costs.is_empty() {
                out.push(Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::AlternativeCost(alt_costs)),
                    duration: dur.clone(),
                });
            }
            let m_cp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&[
                "and", "mana", "of", "any", "type", "can", "be", "spent", "to", "cast",
            ]) && (ts.eat_phrase(&["those", "spells"])
                || ts.eat_phrase(&["that", "spell"])
                || ts.eat_word("it")
                || ts.eat_word("them"))
            {
                out.push(Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::Action(PlayerActionKind::SpendManaAsAnyType),
                    duration: dur,
                });
            } else {
                ts.restore(m_cp);
                super::player_verbs_ext::parse_trailing_play_mana_rider(ts, actor, dur, &mut out);
            }
            return Ok((out, ()));
        }
        ts.restore(cp2);
    }

    // 3d0. `switch(es) <subj>'s power and toughness [until end of turn]`
    if ts.eat_word("switch") || ts.eat_word("switches") {
        let subj = if ts.eat_phrase(&["target", "creature's"]) {
            SubjectRef::TargetObject {
                filter: Box::new(ObjectFilter {
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                count: crate::v2::ir::TargetCount::Exact(1),
                another: false,
            }
        } else if ts.eat_phrase(&["each", "creature's"]) {
            SubjectRef::Each(Box::new(ObjectFilter {
                any_types: crate::ast::CardTypeMask::CREATURE,
                ..Default::default()
            }))
        } else if ts.eat_phrase(&["that", "creature's"]) {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatCreature)
        } else if ts.eat_phrase(&["enchanted", "creature's"]) {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::EnchantedCreature)
        } else if ts.eat_phrase(&["equipped", "creature's"]) {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::EquippedCreature)
        } else if ts.eat_word("its") {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It)
        } else {
            let s = parse_subject_ref(ts)?;
            let _ = ts.eat_word("'s");
            s
        };
        if ts.eat_phrase(&["power", "and", "toughness"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok((
                vec![Effect::MutateObject {
                    subject: subj,
                    mutation: ObjectMutation::StateAction(
                        ObjectStateAction::SwitchPowerAndToughness,
                    ),
                    duration,
                }],
                (),
            ));
        }
        return Err(ts.err("expected `power and toughness` after `switch`"));
    }

    // 3d1. `remove(s) <subj> from combat`
    {
        let rcp = ts.checkpoint();
        if (ts.eat_word("remove") || ts.eat_word("removes"))
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["from", "combat"])
        {
            return Ok((
                vec![Effect::MutateObject {
                    subject: subj,
                    mutation: ObjectMutation::StateAction(ObjectStateAction::RemoveFromCombat),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        ts.restore(rcp);
    }

    // 3d2. `take(s) the initiative` (placed BEFORE `take an extra turn` so `take` never shadows it!)
    if ts.eat_phrase(&["take", "the", "initiative"])
        || ts.eat_phrase(&["takes", "the", "initiative"])
    {
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::TakeInitiative),
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 3d. `have no maximum hand size [dur]` OR `have <subj> deal/fight/become/get ...`
    if ts.eat_word("have") || ts.eat_word("has") {
        if let Some(res) = super::player_verbs_ext::try_parse_have_ext(ts, actor) {
            return res;
        }
        if ts.eat_phrase(&["no", "maximum", "hand", "size"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::Action(PlayerActionKind::NoMaxHandSize),
                    duration,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["your", "life", "total", "become"]) {
            let _ = ts.eat_phrase(&["equal", "to"]);
            let amount = parse_value_expr(ts)?;
            return Ok((
                vec![Effect::AdjustLife {
                    player: actor,
                    amount,
                    op: LifeOp::Set,
                }],
                (),
            ));
        }
        // `have <subj>'s base power and toughness become <P>/<T> [or <P2>/<T2>] [dur]` (`Mirkwood Meditator`, `Master of Winds`)
        {
            let bpt_cp = ts.checkpoint();
            if let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["'s", "base", "power", "and", "toughness", "become"])
                && let Some((p1, t1)) = ts.next_base_pt()
            {
                let alt_pt = if ts.eat_word("or") {
                    ts.next_base_pt()
                } else {
                    None
                };
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let eff1 = Effect::MutateObject {
                    subject: subj.clone(),
                    mutation: ObjectMutation::SetDynamicBasePT {
                        power: ValueExpr::Const(p1),
                        toughness: ValueExpr::Const(t1),
                        add_types: crate::ast::CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration: duration.clone(),
                };
                if let Some((p2, t2)) = alt_pt {
                    let eff2 = Effect::MutateObject {
                        subject: subj,
                        mutation: ObjectMutation::SetDynamicBasePT {
                            power: ValueExpr::Const(p2),
                            toughness: ValueExpr::Const(t2),
                            add_types: crate::ast::CardTypeMask::EMPTY,
                            keep_existing_types: true,
                        },
                        duration,
                    };
                    return Ok((
                        vec![Effect::PromptChoice {
                            chooser: actor,
                            kind: ChoiceKind::ChooseBranch(vec![vec![eff1], vec![eff2]]),
                        }],
                        (),
                    ));
                }
                return Ok((vec![eff1], ()));
            }
            ts.restore(bpt_cp);
        }
        // `have the base power [and toughness] of <subj> become [equal to] <val> [dur]` (`Sita Varma`, `Exuberant Wolfbear`)
        {
            let bp_cp = ts.checkpoint();
            if (ts.eat_phrase(&["the", "base", "power", "and", "toughness", "of"])
                || ts.eat_phrase(&["the", "base", "power", "of"]))
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && (ts.eat_word("become") || ts.eat_word("becomes"))
            {
                let _ = ts.eat_phrase(&["equal", "to"]);
                if let Ok(val) = try_parse(ts, parse_value_expr) {
                    let _ = ts.eat_phrase(&["and", "toughness"]);
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok((
                        vec![Effect::MutateObject {
                            subject: subj,
                            mutation: ObjectMutation::SetDynamicBasePT {
                                power: val.clone(),
                                toughness: val,
                                add_types: crate::ast::CardTypeMask::EMPTY,
                                keep_existing_types: true,
                            },
                            duration,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(bp_cp);
        }
        // `have <subj>'s base power [and toughness] become [equal to] <val> [dur]` (`Belligerent Yearling`)
        {
            let sp_cp = ts.checkpoint();
            if let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("'s")
                && (ts.eat_phrase(&["base", "power", "and", "toughness"])
                    || ts.eat_phrase(&["base", "power"])
                    || ts.eat_phrase(&["base", "toughness"]))
                && (ts.eat_word("become") || ts.eat_word("becomes"))
            {
                let _ = ts.eat_phrase(&["equal", "to"]);
                if let Ok(val) = try_parse(ts, parse_value_expr) {
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok((
                        vec![Effect::MutateObject {
                            subject: subj,
                            mutation: ObjectMutation::SetDynamicBasePT {
                                power: val.clone(),
                                toughness: val,
                                add_types: crate::ast::CardTypeMask::EMPTY,
                                keep_existing_types: true,
                            },
                            duration,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(sp_cp);
        }
        // Compositional delegation: `have <player> <player_verb_frame>` (`have target player lose 3 life`, `have target opponent discard a card`)
        // OR `have <subj> <any subject verb frame>` (`have it deal damage equal to its power to ...`, `have ~ become a 3/3 ...`)
        {
            let pv_cp = ts.checkpoint();
            if let Ok(sub_actor) = try_parse(ts, parse_player_ref)
                && let Ok((effs, ())) = try_parse(ts, |s| parse_player_verb_frame(s, sub_actor))
            {
                return Ok((effs, ()));
            }
            ts.restore(pv_cp);
        }
        {
            let sv_cp = ts.checkpoint();
            if let Ok(effs) = try_parse(
                ts,
                crate::v2::clause::subject_verbs::parse_subject_verb_frame,
            ) {
                return Ok((effs, ()));
            }
            ts.restore(sv_cp);
        }
        let cp = ts.checkpoint();
        if let Ok(source) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("deal") || ts.eat_word("deals") {
                let dcp = ts.checkpoint();
                if ts.eat_phrase(&["damage", "to"])
                    && let Ok(target) = try_parse(ts, parse_subject_ref)
                    && ts.eat_phrase(&["equal", "to"])
                    && let Ok(amount) = try_parse(ts, parse_value_expr)
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
                ts.restore(dcp);
                if let Ok(amount) = try_parse(ts, parse_value_expr)
                    && ts.eat_phrase(&["damage", "to"])
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
                ts.restore(dcp);
            }
            if (ts.eat_word("fight") || ts.eat_word("fights"))
                && let Ok(target) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![Effect::MutateObject {
                        subject: source,
                        mutation: ObjectMutation::Fight(Box::new(target)),
                        duration: Duration::Instant,
                    }],
                    (),
                ));
            }
            if (ts.eat_word("get") || ts.eat_word("gets"))
                && let Some((power, toughness)) = ts.next_pt_mod()
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok((
                    vec![Effect::MutateObject {
                        subject: source,
                        mutation: ObjectMutation::ModifyPT {
                            power,
                            toughness,
                            scale: None,
                        },
                        duration,
                    }],
                    (),
                ));
            }
            if (ts.eat_phrase(&["become", "a", "copy", "of"])
                || ts.eat_phrase(&["becomes", "a", "copy", "of"]))
                && let Ok(copy_src) = try_parse(ts, parse_subject_ref)
            {
                let exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                return Ok((
                    vec![Effect::MutateObject {
                        subject: source,
                        mutation: ObjectMutation::BecomeCopy {
                            source: Box::new(copy_src),
                            exceptions,
                        },
                        duration,
                    }],
                    (),
                ));
            }
            if ts.eat_word("deal") || ts.eat_word("deals") {
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
        }
        ts.restore(cp);
    }

    // 3e. `roll a d20` / `roll a d6` / `roll a six-sided die` / `roll <K> d<N>`
    if ts.eat_word("roll") || ts.eat_word("rolls") {
        if let Some(res) = super::player_verbs_ext::try_parse_roll_ext(ts, actor) {
            return res;
        }
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

    // 3f. `copy(ies) <subj> [twice | <N> times] [for each <expr>] [, except ...]` (`Copy target instant or sorcery spell`, `Sea Gate Stormcaller`, `Jaya`, `Donal`, `Tawnos`, `Fork`, `The Sixth Doctor`)
    if ts.eat_word("copy") || ts.eat_word("copies") {
        // `copy the next <flt> [you cast] this turn when you cast it` / `copy the next loyalty ability you activate this turn when you activate it`
        // (`Sea Gate Stormcaller`, `Flamehold Grappler`, `Tzaangor Shaman`, `Jaya's Phoenix`)
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["the", "next"]) {
                if ts.eat_phrase(&[
                    "loyalty", "ability", "you", "activate", "this", "turn", "when", "you",
                    "activate", "it",
                ]) {
                    return Ok((
                        vec![Effect::DelayedTrigger {
                            trigger: crate::v2::ir::DelayedTriggerKind::Event(
                                crate::v2::ir::TriggerSpec {
                                    events: vec![crate::v2::ir::TriggerEvent::AbilityActivated {
                                        player: actor,
                                        non_mana_only: true,
                                    }],
                                    intervening_if: None,
                                },
                            ),
                            body: vec![Effect::CopyObject {
                                actor,
                                subject: SubjectRef::Anaphora(
                                    crate::v2::ir::AnaphoraRef::ThatSpell,
                                ),
                                count: ValueExpr::Const(1),
                                modifiers: EntryModifiers::default(),
                                exceptions: Vec::new(),
                            }],
                        }],
                        (),
                    ));
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter)
                    && (ts
                        .eat_phrase(&["you", "cast", "this", "turn", "when", "you", "cast", "it"])
                        || ts.eat_phrase(&["this", "turn", "when", "you", "cast", "it"]))
                {
                    return Ok((
                        vec![Effect::DelayedTrigger {
                            trigger: crate::v2::ir::DelayedTriggerKind::Event(
                                crate::v2::ir::TriggerSpec {
                                    events: vec![crate::v2::ir::TriggerEvent::SpellCast {
                                        caster: actor,
                                        spell: Some(flt),
                                        from_outside_hand: false,
                                    }],
                                    intervening_if: None,
                                },
                            ),
                            body: vec![Effect::CopyObject {
                                actor,
                                subject: SubjectRef::Anaphora(
                                    crate::v2::ir::AnaphoraRef::ThatSpell,
                                ),
                                count: ValueExpr::Const(1),
                                modifiers: EntryModifiers::default(),
                                exceptions: Vec::new(),
                            }],
                        }],
                        (),
                    ));
                }
            }
            ts.restore(ncp);
        }
        let subject = if ts.eat_phrase(&["that", "spell", "or", "ability"]) {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatSpell)
        } else {
            parse_subject_ref(ts)?
        };
        let count = if ts.eat_word("twice") {
            ValueExpr::Const(2)
        } else {
            let ncp = ts.checkpoint();
            if let Some(n) = ts.next_number(false)
                && ts.eat_word("times")
            {
                ValueExpr::Const(n)
            } else {
                ts.restore(ncp);
                opt(ts, parse_for_each_expr).unwrap_or(ValueExpr::Const(1))
            }
        };
        let exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
        let mut out = vec![Effect::CopyObject {
            actor,
            subject,
            count,
            modifiers: EntryModifiers::default(),
            exceptions,
        }];
        // Optional trailing `[,] and [you] may choose new targets for the copy/copies` (`Bonus Round`, `Shiko and Narset`)
        {
            let rcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and") {
                let _ = ts.eat_word("you");
                if ts.eat_phrase(&["may", "choose", "new", "targets", "for", "the", "copy"])
                    || ts.eat_phrase(&["may", "choose", "new", "targets", "for", "the", "copies"])
                {
                    out.push(Effect::Optional {
                        actor,
                        body: vec![Effect::MutatePlayer {
                            player: actor,
                            mutation: PlayerMutation::Retarget {
                                subject: Box::new(SubjectRef::Anaphora(
                                    crate::v2::ir::AnaphoraRef::ThatSpell,
                                )),
                                new_target: None,
                            },
                            duration: Duration::Instant,
                        }],
                    });
                } else {
                    ts.restore(rcp);
                }
            } else {
                ts.restore(rcp);
            }
        }
        return Ok((out, ()));
    }

    // 3f3. `change the text of <subj> by replacing all instances of [one color word / one basic land type / one creature type] with another [of your choice] [dur]` (`Sleight of Mind`, `New Blood`, `Magical Hack`, `Mind Bend`, 13+ cards)
    if ts.eat_phrase(&["change", "the", "text", "of"])
        || ts.eat_phrase(&["changes", "the", "text", "of"])
    {
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&["by", "replacing", "all", "instances", "of"]) {
            let kind = if ts.eat_phrase(&[
                "one", "color", "word", "with", "another", "or", "one", "basic", "land", "type",
                "with", "another",
            ]) || ts.eat_phrase(&["one", "color", "word", "with", "another"])
            {
                CharacteristicKind::Color
            } else if ts.eat_phrase(&["one", "basic", "land", "type", "with", "another"]) {
                CharacteristicKind::BasicLandType
            } else if ts.eat_phrase(&["one", "creature", "type", "with", "another"])
                || ts.eat_phrase(&["one", "creature", "type", "with"])
            {
                let _ = opt(ts, |t| {
                    if let Some(w) = t.peek_word()
                        && t.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
                    {
                        t.next_word();
                        Ok(())
                    } else {
                        Err(t.err("not subtype"))
                    }
                });
                CharacteristicKind::CreatureType
            } else {
                return Err(ts.err("expected text-change characteristic kind"));
            };
            let _ = ts.eat_phrase(&["of", "your", "choice"]);
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::ChangeText(kind),
                    duration,
                }],
                (),
            ));
        }
    }

    // 3f2. `change(s) [the | a | any] target(s) of <subj> [to <new_tgt>]` (`Muck Drubb`, `Willbender`, `Mizzium Meddler`, `Sideswipe`)
    if ts.eat_phrase(&["change", "the", "target", "of"])
        || ts.eat_phrase(&["changes", "the", "target", "of"])
        || ts.eat_phrase(&["change", "the", "targets", "of"])
        || ts.eat_phrase(&["change", "a", "target", "of"])
        || ts.eat_phrase(&["change", "any", "targets", "of"])
    {
        let subject = parse_subject_ref(ts)?;
        let new_target = if ts.eat_word("to") {
            Some(Box::new(parse_subject_ref(ts)?))
        } else {
            None
        };
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Retarget {
                    subject: Box::new(subject),
                    new_target,
                },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 3g. `flip(s) a coin [until you lose a flip]` (checkpointed so `flip ~` / `flip it` falls through cleanly!)
    {
        let fcp = ts.checkpoint();
        if (ts.eat_word("flip") || ts.eat_word("flips")) && ts.eat_phrase(&["a", "coin"]) {
            let flip = Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::FlipCoin),
                duration: Duration::Instant,
            };
            if ts.eat_phrase(&["until", "you", "lose", "a", "flip"])
                || ts.eat_phrase(&["until", "they", "lose", "a", "flip"])
            {
                return Ok((
                    vec![Effect::Unless {
                        body: vec![flip],
                        payer: None,
                        cost: None,
                        condition: Some(crate::v2::ir::ConditionExpr::Not(Box::new(
                            crate::v2::ir::ConditionExpr::Flag(ConditionFlag::WonCoinFlip),
                        ))),
                    }],
                    (),
                ));
            }
            return Ok((vec![flip], ()));
        }
        ts.restore(fcp);
    }

    // 3h. `become(s) the monarch` / `take(s) an extra turn [after this one]`
    if (ts.eat_word("become") || ts.eat_word("becomes")) && ts.eat_phrase(&["the", "monarch"]) {
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::BecomeMonarch),
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
                mutation: PlayerMutation::Action(PlayerActionKind::ExtraTurn),
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 3i. `double(s) the number of [<kind>] counters on <subj>` OR `double(s) <subj>'s power [and toughness] [<val> times] [until end of turn]`
    if ts.eat_word("double") || ts.eat_word("doubles") {
        if ts.eat_phrase(&["the", "number", "of"]) {
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
        let dcp = ts.checkpoint();
        let subj = if ts.eat_phrase(&["target", "creature's"]) {
            Some(SubjectRef::TargetObject {
                filter: Box::new(ObjectFilter {
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                count: crate::v2::ir::TargetCount::Exact(1),
                another: false,
            })
        } else if ts.eat_phrase(&["that", "creature's"]) {
            Some(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::ThatCreature,
            ))
        } else if ts.eat_phrase(&["equipped", "creature's"]) {
            Some(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::EquippedCreature,
            ))
        } else if ts.eat_phrase(&["enchanted", "creature's"]) {
            Some(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::EnchantedCreature,
            ))
        } else if ts.eat_word("its") {
            Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It))
        } else if ts.eat_tilde() && ts.eat_word("'s") {
            Some(SubjectRef::SelfObject)
        } else {
            None
        };
        if let Some(subject) = subj {
            let both = ts.eat_phrase(&["power", "and", "toughness"]);
            let only_p = !both && ts.eat_word("power");
            let only_t = !both && !only_p && ts.eat_word("toughness");
            if both || only_p || only_t {
                let times = {
                    let tcp = ts.checkpoint();
                    if let Ok(v) = try_parse(ts, parse_value_expr)
                        && (ts.eat_word("times") || ts.eat_word("time"))
                    {
                        Some(v)
                    } else {
                        ts.restore(tcp);
                        None
                    }
                };
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut eff = Effect::MutateObject {
                    subject: subject.clone(),
                    mutation: ObjectMutation::ModifyPT {
                        power: if both || only_p {
                            crate::v2::ir::PtDelta::PlusX
                        } else {
                            crate::v2::ir::PtDelta::Fixed(0)
                        },
                        toughness: if both || only_t {
                            crate::v2::ir::PtDelta::PlusX
                        } else {
                            crate::v2::ir::PtDelta::Fixed(0)
                        },
                        scale: Some(Box::new(ValueExpr::ObjectStat {
                            subject: Box::new(subject),
                            stat: if only_t {
                                crate::ast::StatKind::Toughness
                            } else {
                                crate::ast::StatKind::Power
                            },
                        })),
                    },
                    duration,
                };
                if let Some(t) = times {
                    eff = Effect::ForEach {
                        over: ForEachDomain::Count(t),
                        body: vec![eff],
                    };
                }
                return Ok((vec![eff], ()));
            }
        }
        ts.restore(dcp);
        return Err(ts.err("expected counters or power/toughness after `double`"));
    }

    // 3j. `exchange ...` (control, life totals, life total with stat, zones)
    if ts.eat_word("exchange") || ts.eat_word("exchanges") {
        // `exchange control of <subj1> [and <subj2>]`
        if ts.eat_phrase(&["control", "of"])
            && let Ok(first) = try_parse(ts, parse_subject_ref)
        {
            let acp = ts.checkpoint();
            let subject = if ts.eat_word("and")
                && let Ok(second) = try_parse(ts, parse_subject_ref)
            {
                SubjectRef::Compound(vec![first, second])
            } else {
                ts.restore(acp);
                first
            };
            return Ok((vec![Effect::ExchangeControl { subject }], ()));
        }
        // `exchange life totals [with <player>]`
        if ts.eat_phrase(&["life", "totals"]) {
            let other = if ts.eat_word("with") {
                parse_player_ref(ts)?
            } else {
                PlayerRef::TargetPlayer
            };
            return Ok((
                vec![
                    Effect::AdjustLife {
                        player: actor,
                        amount: ValueExpr::PlayerStat {
                            player: other,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        },
                        op: LifeOp::Set,
                    },
                    Effect::AdjustLife {
                        player: other,
                        amount: ValueExpr::PlayerStat {
                            player: actor,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        },
                        op: LifeOp::Set,
                    },
                ],
                (),
            ));
        }
        // `exchange <player>'s life total with <subj>'s power/toughness`
        {
            let lcp = ts.checkpoint();
            let life_p = if ts.eat_phrase(&["your", "life", "total"]) {
                Some(PlayerRef::You)
            } else if ts.eat_phrase(&["target", "opponent's", "life", "total"]) {
                Some(PlayerRef::TargetOpponent)
            } else if ts.eat_phrase(&["target", "player's", "life", "total"]) {
                Some(PlayerRef::TargetPlayer)
            } else if ts.eat_phrase(&["their", "life", "total"])
                || ts.eat_phrase(&["that", "player's", "life", "total"])
            {
                Some(PlayerRef::ThatPlayer)
            } else {
                None
            };
            if let Some(lp) = life_p
                && ts.eat_word("with")
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("'s")
            {
                let stat_opt = if ts.eat_word("toughness") {
                    Some(crate::ast::StatKind::Toughness)
                } else if ts.eat_word("power") {
                    Some(crate::ast::StatKind::Power)
                } else {
                    None
                };
                if let Some(stat) = stat_opt {
                    let stat_val = ValueExpr::ObjectStat {
                        subject: Box::new(subj.clone()),
                        stat,
                    };
                    let life_val = ValueExpr::PlayerStat {
                        player: lp,
                        stat: crate::ast::PlayerStatKind::LifeTotal,
                    };
                    let (p_expr, t_expr) = if stat == crate::ast::StatKind::Toughness {
                        (
                            ValueExpr::ObjectStat {
                                subject: Box::new(subj.clone()),
                                stat: crate::ast::StatKind::Power,
                            },
                            life_val,
                        )
                    } else {
                        (
                            life_val,
                            ValueExpr::ObjectStat {
                                subject: Box::new(subj.clone()),
                                stat: crate::ast::StatKind::Toughness,
                            },
                        )
                    };
                    return Ok((
                        vec![
                            Effect::AdjustLife {
                                player: lp,
                                amount: stat_val,
                                op: LifeOp::Set,
                            },
                            Effect::MutateObject {
                                subject: subj,
                                mutation: ObjectMutation::SetDynamicBasePT {
                                    power: p_expr,
                                    toughness: t_expr,
                                    add_types: crate::ast::CardTypeMask::EMPTY,
                                    keep_existing_types: true,
                                },
                                duration: Duration::Permanent,
                            },
                        ],
                        (),
                    ));
                }
            }
            ts.restore(lcp);
        }
        // `exchange your/their <zone1> and <zone2>` (e.g. `your hand and library`, `your graveyard and library`, `your hand and graveyard`)
        {
            let zcp = ts.checkpoint();
            if let Ok(z1) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("and")
            {
                let z2_opt = if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                    Some(z2)
                } else if ts.eat_word("library") {
                    Some(ZoneRef {
                        zone: ZoneKind::Library,
                        owner: z1.owner,
                    })
                } else if ts.eat_word("graveyard") {
                    Some(ZoneRef {
                        zone: ZoneKind::Graveyard,
                        owner: z1.owner,
                    })
                } else if ts.eat_word("hand") {
                    Some(ZoneRef {
                        zone: ZoneKind::Hand,
                        owner: z1.owner,
                    })
                } else {
                    None
                };
                if let Some(z2) = z2_opt {
                    return Ok((
                        vec![
                            Effect::ZoneOp {
                                actor,
                                op: ZoneOpKind::Move,
                                subject: SubjectRef::All(Box::new(ObjectFilter {
                                    any_card: true,
                                    zones: vec![z1],
                                    ..Default::default()
                                })),
                                from: vec![z1],
                                to: Some(ZoneDest {
                                    zone: z2.zone,
                                    owner: z2.owner,
                                    library_position: None,
                                    library_order: None,
                                    under_control: None,
                                }),
                                modifiers: EntryModifiers::default(),
                                except: None,
                                count: None,
                            },
                            Effect::ZoneOp {
                                actor,
                                op: ZoneOpKind::Move,
                                subject: SubjectRef::All(Box::new(ObjectFilter {
                                    any_card: true,
                                    zones: vec![z2],
                                    ..Default::default()
                                })),
                                from: vec![z2],
                                to: Some(ZoneDest {
                                    zone: z1.zone,
                                    owner: z1.owner,
                                    library_position: None,
                                    library_order: None,
                                    under_control: None,
                                }),
                                modifiers: EntryModifiers::default(),
                                except: None,
                                count: None,
                            },
                        ],
                        (),
                    ));
                }
            }
            ts.restore(zcp);
        }
        return Err(ts.err("expected target after `exchange`"));
    }

    // 3k. `control(s) <player> during that player's/their next turn`
    {
        let ccp = ts.checkpoint();
        if (ts.eat_word("control") || ts.eat_word("controls"))
            && let Ok(target_p) = try_parse(ts, parse_player_ref)
            && (ts.eat_phrase(&["during", "that", "player's", "next", "turn"])
                || ts.eat_phrase(&["during", "their", "next", "turn"]))
        {
            return Ok((
                vec![Effect::MutateObject {
                    subject: SubjectRef::Player(target_p),
                    mutation: ObjectMutation::GainControl { controller: actor },
                    duration: Duration::UntilEndOfNextTurn,
                }],
                (),
            ));
        }
        ts.restore(ccp);
    }

    // 4. `pay(s) <cost>`
    if ts.eat_word("pay") || ts.eat_word("pays") {
        let cost = if ts.eat_phrase(&["any", "amount", "of", "mana"]) {
            Cost::PayAnyAmountMana
        } else if ts.eat_phrase(&["any", "amount", "of", "life"]) {
            Cost::PayAnyAmountLife
        } else if ts.eat_phrase(&["any", "amount", "of"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            Cost::PayEnergy(ValueExpr::Var(crate::v2::ir::VarName::X))
        } else if ts.eat_phrase(&["life", "equal", "to"]) {
            Cost::PayLife(parse_value_expr(ts)?)
        } else if ts.eat_phrase(&["an", "amount", "of"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            if !ts.eat_phrase(&["equal", "to"]) {
                return Err(ts.err("expected `equal to` after `an amount of {E}`"));
            }
            let amt = parse_value_expr(ts)?;
            Cost::PayEnergy(amt)
        } else if ts.eat_phrase(&["one", "or", "more"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            Cost::PayEnergy(ValueExpr::Var(crate::v2::ir::VarName::X))
        } else if let Some(mc) = ts.eat_mana_cost() {
            Cost::Mana(mc)
        } else {
            let amt = parse_value_expr(ts)?;
            if ts.eat_word("life") {
                Cost::PayLife(amt)
            } else if matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            ) {
                ts.next_tok();
                Cost::PayEnergy(amt)
            } else {
                return Err(ts.err("expected `life`, energy, or mana after `pay`"));
            }
        };
        let _ = ts.eat_phrase(&["any", "number", "of", "times"]);
        let mut out = vec![Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::PayCost(cost),
            duration: Duration::Instant,
        }];
        // Support compound `pay {M} and <N> life` (`Ripples of Undeath`, `Phyrexian`-style pay costs)
        {
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(amt) = try_parse(ts, parse_value_expr)
                && ts.eat_word("life")
            {
                out.push(Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::PayCost(Cost::PayLife(amt)),
                    duration: Duration::Instant,
                });
            } else {
                ts.restore(acp);
            }
        }
        return Ok((out, ()));
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
        if op == LifeOp::Lose && ts.eat_phrase(&["the", "game"]) {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::Action(PlayerActionKind::LoseGame),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if op == LifeOp::Lose
            && (ts.eat_phrase(&["half", "their", "life"])
                || ts.eat_phrase(&["half", "your", "life"])
                || ts.eat_phrase(&["half", "their", "life", "total"])
                || ts.eat_phrase(&["half", "your", "life", "total"]))
        {
            let _ = ts.eat_comma();
            let rounding = if ts.eat_phrase(&["rounded", "up"]) {
                crate::ast::Rounding::Up
            } else {
                let _ = ts.eat_phrase(&["rounded", "down"]);
                crate::ast::Rounding::Down
            };
            return Ok((
                vec![Effect::AdjustLife {
                    player: actor,
                    amount: ValueExpr::Div {
                        expr: Box::new(ValueExpr::PlayerStat {
                            player: actor,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        }),
                        denom: 2,
                        rounding,
                    },
                    op: LifeOp::Lose,
                }],
                (),
            ));
        }
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

    if let Some(res) = super::zone_verbs::try_parse_zone_verb_frame(ts, actor) {
        return res;
    }

    // 21. `choose(s) <new targets for <subj> | characteristic | target>`
    if ts.eat_word("choose") || ts.eat_word("chooses") {
        if let Some(res) = super::player_verbs_ext::try_parse_choose_ext(ts, actor) {
            return res;
        }
        if ts.eat_phrase(&["new", "targets", "for"])
            || ts.eat_phrase(&["a", "new", "target", "for"])
        {
            let subj = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::Retarget {
                        subject: Box::new(subj),
                        new_target: None,
                    },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["odd", "or", "even"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::OddOrEven),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["khans", "or", "dragons"])
            || ts.eat_phrase(&["dragons", "or", "khans"])
            || ts.eat_phrase(&["abzan", "or", "mardu"])
            || ts.eat_phrase(&["temur", "or", "sultai"])
            || ts.eat_phrase(&["jeskai", "or", "abzan"])
            || ts.eat_phrase(&["mardu", "or", "temur"])
            || ts.eat_phrase(&["sultai", "or", "jeskai"])
            || ts.eat_phrase(&["sultai", "or", "abzan"])
            || ts.eat_phrase(&["abzan", "or", "sultai"])
            || ts.eat_phrase(&["war", "or", "peace"])
            || ts.eat_phrase(&["left", "or", "right"])
            || ts.eat_phrase(&["friend", "or", "foe"])
            || ts.eat_phrase(&["friends", "or", "foes"])
        {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BinaryMode),
                }],
                (),
            ));
        }
        {
            let bcp = ts.checkpoint();
            if let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&["basic", "land", "types"])
            {
                return Ok((
                    vec![Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BasicLandTypes(
                            u8::try_from(n.max(1)).unwrap_or(2),
                        )),
                    }],
                    (),
                ));
            }
            ts.restore(bcp);
        }
        if ts.eat_phrase(&["a", "creature", "type"]) {
            if ts.eat_phrase(&["other", "than"])
                && let Some(w) = ts.peek_word()
                && ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
            {
                ts.next_word();
            }
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CreatureType),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "card", "type"]) {
            // Optional `other than <types>` (e.g. `choose a card type other than creature or land`)
            if ts.eat_phrase(&["other", "than"]) {
                while let Some(w) = ts.peek_word() {
                    if crate::vocab::lookup_card_type(&w.to_ascii_lowercase()).is_some() {
                        ts.next_word();
                        let _ = ts.eat_comma();
                        let _ = ts.eat_word("or") || ts.eat_word("and");
                    } else {
                        break;
                    }
                }
            }
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CardType),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "color"]) {
            if ts.eat_phrase(&["other", "than"]) {
                while let Some(w) = ts.peek_word() {
                    if crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some() {
                        ts.next_word();
                        let _ = ts.eat_comma();
                        let _ = ts.eat_word("or") || ts.eat_word("and");
                    } else {
                        break;
                    }
                }
            }
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Color),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "nonland", "card", "name"])
            || (ts.eat_phrase(&["a", "noncreature"]) && {
                let _ = ts.eat_comma();
                ts.eat_phrase(&["nonland", "card", "name"])
            })
        {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::NonlandCardName),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "card", "name"])
            || ts.eat_phrase(&["any", "card", "name"])
            || (ts.eat_phrase(&["the", "name", "of"]) && try_parse(ts, parse_object_filter).is_ok())
        {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CardName),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "basic", "land", "type"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BasicLandType),
                }],
                (),
            ));
        }
        // Try `parse_subject_ref` BEFORE optional leading count so `choose one of them` doesn't eat `one`!
        let subj = if let Ok(s) = try_parse(ts, parse_subject_ref) {
            s
        } else {
            let _ = opt(ts, parse_value_expr);
            parse_subject_ref(ts)?
        };
        let _ = ts.eat_phrase(&["at", "random"]);
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
        let first = parse_subject_ref(ts)?;
        let subject = {
            let ucp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(second) = try_parse(ts, parse_subject_ref)
                && (ts.is_eof()
                    || matches!(
                        ts.peek().map(|t| &t.kind),
                        Some(TokenKind::Period | TokenKind::Comma)
                    ))
            {
                SubjectRef::Compound(vec![first, second])
            } else {
                ts.restore(ucp);
                first
            }
        };
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
                    mutation: ObjectMutation::StateAction(ObjectStateAction::TurnFaceUp),
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
                    mutation: ObjectMutation::StateAction(ObjectStateAction::TurnFaceUp),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["face", "down"]) {
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(ObjectStateAction::TurnFaceDown),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
    }

    // 24. `counter <subj> [or ability] [unless its controller pays <cost> [for each <expr>] | unless its controller <effect>]`
    if ts.eat_word("counter") {
        let subject = parse_subject_ref(ts)?;
        let _ = ts.eat_phrase(&["or", "ability"]);
        {
            let ucp = ts.checkpoint();
            if ts.eat_word("unless") {
                if ts.eat_phrase(&["its", "controller", "pays"]) {
                    let final_cost = if ts.eat_phrase(&["mana", "equal", "to"]) {
                        let scale = parse_value_expr(ts)?;
                        Cost::ScaledMana {
                            base: crate::v2::ir::ManaCost {
                                symbols: vec![ManaSymbol::Generic(1)],
                            },
                            scale,
                        }
                    } else {
                        let base_cost = parse_cost_atom(ts)?;
                        match base_cost {
                            Cost::Mana(mc) => {
                                if let Some(scale) = opt(ts, parse_for_each_expr) {
                                    Cost::ScaledMana { base: mc, scale }
                                } else {
                                    Cost::Mana(mc)
                                }
                            }
                            other => other,
                        }
                    };
                    return Ok((
                        vec![Effect::ZoneOp {
                            actor,
                            op: ZoneOpKind::CounterSpell {
                                unless_pay: Some(Box::new(final_cost)),
                            },
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
                if let Ok(payer) = try_parse(ts, parse_player_ref)
                    && let Ok((alt_body, _)) = try_parse(ts, |t| parse_player_verb_frame(t, payer))
                {
                    return Ok((
                        vec![Effect::Unless {
                            body: vec![Effect::ZoneOp {
                                actor,
                                op: ZoneOpKind::CounterSpell { unless_pay: None },
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
                            payer: Some(payer),
                            cost: None,
                            condition: if alt_body.is_empty() {
                                None
                            } else {
                                Some(crate::v2::ir::ConditionExpr::Flag(
                                    ConditionFlag::AdditionalCostPaid,
                                ))
                            },
                        }],
                        (),
                    ));
                }
            }
            ts.restore(ucp);
        }
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::CounterSpell { unless_pay: None },
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

    // 25. `mill(s) [half their/your library, rounded up/down | cards equal to <val> | <N> card(s)]`
    if ts.eat_word("mill") || ts.eat_word("mills") {
        let final_cnt = if ts.eat_phrase(&["half", "their", "library"])
            || ts.eat_phrase(&["half", "your", "library"])
            || ts.eat_phrase(&["half", "that", "player's", "library"])
        {
            let _ = ts.eat_comma();
            let rounding = if ts.eat_phrase(&["rounded", "up"]) {
                crate::ast::Rounding::Up
            } else {
                let _ = ts.eat_phrase(&["rounded", "down"]);
                crate::ast::Rounding::Down
            };
            ValueExpr::Div {
                expr: Box::new(ValueExpr::PlayerStat {
                    player: actor,
                    stat: crate::ast::PlayerStatKind::CardsInLibrary,
                }),
                denom: 2,
                rounding,
            }
        } else if ts.eat_phrase(&["cards", "equal", "to"]) {
            parse_value_expr(ts)?
        } else {
            let count = if ts.eat_word("a") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            if !(ts.eat_word("card") || ts.eat_word("cards")) {
                return Err(ts.err("expected `card(s)` after `mill`"));
            }
            if let Some(fe) = opt(ts, parse_for_each_expr) {
                if count == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(count), Box::new(fe))
                }
            } else {
                count
            }
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
                mutation: ObjectMutation::StateAction(ObjectStateAction::Regenerate),
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
                mutation: ObjectMutation::StateAction(ObjectStateAction::Transform),
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
        if ts.eat_phrase(&["those", "counters"])
            || ts.eat_phrase(&["those", "counter"])
            || ts.eat_phrase(&["all", "of", "them"])
            || ts.eat_phrase(&["them", "all"])
            || ts.eat_word("them")
        {
            let subject = if ts.eat_word("from") {
                parse_subject_ref(ts)?
            } else {
                SubjectRef::SelfObject
            };
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::RemoveCounters {
                        kind: CounterKind::Any,
                        count: None,
                        all: true,
                    },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        let _ = ts.eat_phrase(&["up", "to"]);
        let (all, count) = if ts.eat_phrase(&["all", "but"]) {
            let keep = parse_value_expr(ts)?;
            (false, Some(keep))
        } else if ts.eat_phrase(&["any", "number", "of"]) {
            (false, Some(ValueExpr::Const(0)))
        } else if ts.eat_word("all") {
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
        let _ = ts.eat_word("among");
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

    // 26b. `give <subj> another counter of that kind` (`Powerful Broker`, `Maulfist Revolutionary`, `Skyship Plunderer`)
    {
        let gcp = ts.checkpoint();
        if (ts.eat_word("give") || ts.eat_word("gives"))
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && (ts.eat_phrase(&["another", "counter", "of", "that", "kind"])
                || ts.eat_phrase(&["a", "counter", "of", "that", "kind"]))
        {
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::Const(1),
                    },
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        ts.restore(gcp);
    }

    // 27. `prevent ... damage ...`
    if ts.eat_word("prevent") || ts.eat_word("prevents") {
        return Ok((vec![parse_prevent_damage(ts)?], ()));
    }

    // 28. Extended player verbs (voting, meld, convert, unattach, reselect, manifest, planeswalk, repeat, etc.)
    if let Some(res) = super::player_verbs_ext::try_parse_player_verb_ext(ts, actor) {
        return res;
    }

    // 29. Standalone keyword action verbs (`proliferate`, `investigate`, `populate`, `amass`, `adapt`, `goad`, etc.)
    if let Some(eff) = opt(ts, |s| parse_keyword_action_verb(s, actor)) {
        return Ok((vec![eff], ()));
    }

    Err(ts.err("expected player verb frame"))
}
