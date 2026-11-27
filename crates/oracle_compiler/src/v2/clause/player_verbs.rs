//! Player-initiated verb frames, mana additions, damage prevention, and keyword action verbs for `v2`.

use crate::v2::clause::player_actions::{
    parse_keyword_action_verb, parse_mana_addition, parse_prevent_damage,
};
use crate::v2::clause::subject_verbs::parse_trailing_duration;
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    CastPermissionMode, CharacteristicKind, ChoiceKind, Cost, CounterKind, Duration, Effect,
    EntryModifiers, ForEachDomain, LifeOp, ManaSpec, ManaSymbol, ObjectFilter, ObjectMutation,
    ObjectStateAction, PlayerActionKind, PlayerMutation, PlayerRef, PlayerRestrictionKind,
    StaticAbility, SubjectRef, ValueExpr, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

use super::{
    all_zone_cards_subject, apply_continuous, compile_quoted_face, counted_zone_op, eat_rounding,
    exact_count, mutate_obj, mutate_player, player_action, prompt_choice, simple_zone_dest,
    simple_zone_op,
};

fn attach_zones_to_subject(subj: &mut SubjectRef, zones: &[ZoneRef]) -> bool {
    let flt = match subj {
        SubjectRef::Group(f)
        | SubjectRef::All(f)
        | SubjectRef::Each(f)
        | SubjectRef::TargetObject { filter: f, .. }
        | SubjectRef::Counted { filter: f, .. } => f,
        _ => return false,
    };
    for &z in zones {
        if !flt.zones.contains(&z) {
            flt.zones.push(z);
        }
    }
    true
}

fn build_play_permission_effect(
    actor: PlayerRef,
    mut subj: SubjectRef,
    cast_only: bool,
    free: bool,
    from_zones: &[ZoneRef],
    top_only: bool,
    dur: Duration,
) -> Effect {
    if !top_only && (from_zones.is_empty() || attach_zones_to_subject(&mut subj, from_zones)) {
        return mutate_player(
            actor,
            PlayerMutation::GrantPlayPermission {
                cast_only,
                subject: Box::new(subj),
                free,
            },
            dur,
        );
    }
    let mode = if free {
        CastPermissionMode::WithoutPaying
    } else if cast_only {
        CastPermissionMode::CastFromZone
    } else {
        CastPermissionMode::PlayFromZone {
            top_only,
            lands: true,
            spells: true,
        }
    };
    apply_continuous(
        StaticAbility::ZoneCastPermission {
            player: actor,
            filter: None,
            subject: Some(subj),
            zone: from_zones.first().copied(),
            mode,
            condition: None,
        },
        dur,
    )
}

/// Verb frames with a known player actor (`actor`).
pub(super) fn parse_player_verb_frame(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    // 1. `double the power/toughness/number of counters ...` (falls through to section 3i for `double <subj>'s power [and toughness] [<val> times]`)
    let outer_dcp = ts.checkpoint();
    if ts.eat_any_word(&["double", "doubles"]) {
        if let Some(res) = super::player_verbs_ext::try_parse_double_ext(ts, actor) {
            return res;
        }
        const DOUBLE_OF: &[(&[&str], bool, bool)] = &[
            (&["the", "power", "and", "toughness", "of"], true, true),
            (&["the", "power", "of"], true, false),
            (&["the", "toughness", "of"], false, true),
        ];
        for &(phrase, power, toughness) in DOUBLE_OF {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(phrase)
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok((
                    vec![mutate_obj(
                        subject,
                        ObjectMutation::DoubleStat { power, toughness },
                        duration,
                    )],
                    (),
                ));
            }
            ts.restore(dcp);
        }
        if ts.eat_phrase(&["the", "number", "of"]) {
            let kind = if ts.eat_any_phrase(&[
                &["each", "kind", "of", "counter", "on"],
                &["each", "kind", "of", "counters", "on"],
            ]) {
                Some(None)
            } else if let Ok(k) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counters", "counter"])
                && ts.eat_word("on")
            {
                Some(Some(k))
            } else if ts.eat_any_word(&["counters", "counter"]) && ts.eat_word("on") {
                Some(None)
            } else {
                None
            };
            if let Some(kind) = kind
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![mutate_obj(
                        subject,
                        ObjectMutation::DoubleCounters { kind },
                        Duration::Instant,
                    )],
                    (),
                ));
            }
        }
        const DOUBLE_ITS: &[(&[&str], bool, bool)] = &[
            (&["its", "power", "and", "toughness"], true, true),
            (&["its", "power"], true, false),
            (&["its", "toughness"], false, true),
        ];
        for &(phrase, power, toughness) in DOUBLE_ITS {
            if ts.eat_phrase(phrase) {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok((
                    vec![mutate_obj(
                        SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                        ObjectMutation::DoubleStat { power, toughness },
                        duration,
                    )],
                    (),
                ));
            }
        }
        ts.restore(outer_dcp);
    }

    if ts.eat_any_word(&["add", "adds"]) {
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
            vec![mutate_player(
                actor,
                PlayerMutation::AddMana(spec),
                Duration::Instant,
            )],
            (),
        ));
    }

    // 2. `get(s) {E}...` / `get(s) an emblem with "..."`
    if ts.eat_any_word(&["get", "gets"]) {
        if let Some(res) = super::player_verbs_ext::try_parse_get_ext(ts, actor) {
            return res;
        }
        if ts.eat_phrase(&["an", "emblem", "with"]) {
            let quoted = ts
                .next_quoted()
                .ok_or_else(|| ts.err("expected quoted emblem text"))?;
            let emblem_face = compile_quoted_face(ts, "Emblem", "Emblem", quoted)?;
            return Ok((
                vec![mutate_player(
                    actor,
                    PlayerMutation::CreateEmblem(Box::new(emblem_face)),
                    Duration::Permanent,
                )],
                (),
            ));
        }
        let gain_counters = |kind, count| {
            mutate_player(
                actor,
                PlayerMutation::GainCounters { kind, count },
                Duration::Instant,
            )
        };
        // `an amount of {E} equal to <val>`
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
                    return Ok((vec![gain_counters(CounterKind::Energy, count)], ()));
                }
            }
            ts.restore(ecp);
        }
        // `<val> {E}` or `<val> {TK}`
        {
            let vecp = ts.checkpoint();
            if let Ok(count) = try_parse(ts, parse_value_expr) {
                if matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Mana(ManaSymbol::Energy))
                ) {
                    ts.next_tok();
                    return Ok((vec![gain_counters(CounterKind::Energy, count)], ()));
                }
                if matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Mana(ManaSymbol::Ticket))
                ) {
                    ts.next_tok();
                    return Ok((
                        vec![gain_counters(
                            CounterKind::Named("ticket".to_string()),
                            count,
                        )],
                        (),
                    ));
                }
            }
            ts.restore(vecp);
        }
        // `a number of <kind> counters equal to <val>`
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counters", "counter"])
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
            {
                return Ok((vec![gain_counters(kind, count)], ()));
            }
            ts.restore(ncp);
        }
        // Energy symbols `{E}{E}{E}` / Ticket symbols `{TK}{TK}`
        for (sym, kind) in [
            (ManaSymbol::Energy, CounterKind::Energy),
            (ManaSymbol::Ticket, CounterKind::Named("ticket".to_string())),
        ] {
            let mut count = 0i32;
            while matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Mana(s)) if *s == sym) {
                ts.next_tok();
                count += 1;
            }
            if count > 0 {
                return Ok((vec![gain_counters(kind, ValueExpr::Const(count))], ()));
            }
        }
        // `get(s) [a/an/another/an additional/<val>] <kind> counter(s)`
        {
            let gcp = ts.checkpoint();
            let count = if ts.eat_phrase(&["an", "additional"])
                || ts.eat_any_word(&["another", "a", "an"])
            {
                Some(ValueExpr::Const(1))
            } else {
                try_parse(ts, parse_value_expr).ok()
            };
            if let Some(count) = count
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counter", "counters"])
            {
                return Ok((vec![gain_counters(kind, count)], ()));
            }
            ts.restore(gcp);
        }
        return Err(ts.err("expected emblem, energy, or counter after `get(s)`"));
    }

    // 3. `play(s) ...` (additional land plays, zone permissions, hand revealed, and play permissions)
    if ts.eat_any_word(&["play", "plays"]) {
        if let Some(res) = super::player_verbs_ext::try_parse_play_ext(ts, actor) {
            return res;
        }
        if ts.eat_any_phrase(&[
            &["with", "their", "hand", "revealed"],
            &["with", "your", "hand", "revealed"],
        ]) {
            return Ok((
                vec![apply_continuous(
                    StaticAbility::PlayerRestriction {
                        player: actor,
                        restriction: PlayerRestrictionKind::PlaysWithHandRevealed,
                        condition: None,
                    },
                    Duration::UntilEndOfTurn,
                )],
                (),
            ));
        }
        let subj = parse_subject_ref(ts)?;
        let mut from_zones = Vec::new();
        let mut top_only = false;
        {
            let f_outer = ts.checkpoint();
            if ts.eat_word("from") {
                if let Some((z_opt, ext_top)) =
                    super::player_verbs_ext::eat_extended_cast_source(ts)
                {
                    top_only |= ext_top;
                    if let Some(z) = z_opt {
                        from_zones.push(z);
                    }
                } else if ts.eat_word("among") && try_parse(ts, parse_subject_ref).is_ok() {
                    // `from among <cards>`
                } else if let Ok(z) = try_parse(ts, parse_zone_ref) {
                    from_zones.push(z);
                } else {
                    ts.restore(f_outer);
                }
            }
        }
        let eat_free = |s: &mut TokenStream<'_>| {
            s.eat_any_phrase(&[
                &["without", "paying", "its", "mana", "cost"],
                &["without", "paying", "their", "mana", "costs"],
            ])
        };
        let mut free = eat_free(ts);
        let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        if !free {
            free = eat_free(ts);
        }
        let mut out = vec![build_play_permission_effect(
            actor,
            subj,
            false,
            free,
            &from_zones,
            top_only,
            dur.clone(),
        )];
        let m_cp = ts.checkpoint();
        let _ = ts.eat_comma();
        if ts.eat_any_phrase(&[
            &[
                "and", "mana", "of", "any", "type", "can", "be", "spent", "to", "cast",
            ],
            &[
                "and", "you", "may", "spend", "mana", "as", "though", "it", "were", "mana", "of",
                "any", "color", "to", "cast",
            ],
            &[
                "and", "you", "may", "spend", "mana", "as", "though", "it", "were", "mana", "of",
                "any", "type", "to", "cast",
            ],
        ]) && ts.eat_any_phrase(&[&["those", "spells"], &["that", "spell"], &["it"], &["them"]])
        {
            out.push(player_action(
                actor,
                PlayerActionKind::SpendManaAsAnyType,
                dur,
            ));
        } else {
            ts.restore(m_cp);
            super::player_verbs_ext::parse_trailing_play_mana_rider(ts, actor, dur, &mut out);
        }
        return Ok((out, ()));
    }

    // 3b0. `can't play cards from their/your hand`
    if ts.eat_any_phrase(&[
        &["can't", "play", "cards", "from", "their", "hand"],
        &["can't", "play", "cards", "from", "your", "hand"],
    ]) {
        return Ok((
            vec![apply_continuous(
                StaticAbility::PlayerRestriction {
                    player: actor,
                    restriction: PlayerRestrictionKind::CantPlayFromHand,
                    condition: None,
                },
                Duration::UntilEndOfTurn,
            )],
            (),
        ));
    }

    // 3b. `can't cast spells [or activate abilities] [dur]`
    if ts.eat_phrase(&["can't", "cast", "spells"]) {
        let mut has_activate = false;
        if ts.eat_phrase(&["or", "activate", "abilities"]) {
            if ts.eat_word("of") {
                let _ = opt(ts, parse_object_filter);
            }
            has_activate = true;
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        let mut out = vec![apply_continuous(
            StaticAbility::PlayerRestriction {
                player: actor,
                restriction: PlayerRestrictionKind::CantCastSpells,
                condition: None,
            },
            duration.clone(),
        )];
        if has_activate {
            out.push(apply_continuous(
                StaticAbility::PlayerRestriction {
                    player: actor,
                    restriction: PlayerRestrictionKind::CantActivateAbilities,
                    condition: None,
                },
                duration,
            ));
        }
        return Ok((out, ()));
    }

    // 3c. `cast(s) <flt> [this turn] as though they had flash` OR `cast(s) <subj> [from <zone>] [without paying its mana cost] [dur]`
    if ts.eat_any_word(&["cast", "casts"]) {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter) {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_phrase(&["as", "though", "they", "had", "flash"]) {
                let _ = ts.eat_phrase(&["this", "turn"]);
                return Ok((
                    vec![apply_continuous(
                        StaticAbility::ZoneCastPermission {
                            player: actor,
                            filter: Some(filter),
                            subject: None,
                            zone: None,
                            mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                            condition: None,
                        },
                        Duration::UntilEndOfTurn,
                    )],
                    (),
                ));
            }
        }
        ts.restore(cp);
        // `cast (any number of | up to <val> of) [the] copies [without paying their mana costs]`
        {
            let acp = ts.checkpoint();
            if (ts.eat_phrase(&["any", "number", "of"])
                || (ts.eat_phrase(&["up", "to"])
                    && opt(ts, parse_value_expr).is_some()
                    && ts.eat_word("of")))
                && ts.eat_any_phrase(&[&["the", "copies"], &["copies"]])
            {
                let free = ts.eat_any_phrase(&[
                    &["without", "paying", "their", "mana", "costs"],
                    &["without", "paying", "its", "mana", "cost"],
                ]);
                return Ok((
                    vec![mutate_player(
                        actor,
                        PlayerMutation::GrantPlayPermission {
                            cast_only: true,
                            subject: Box::new(SubjectRef::Anaphora(
                                crate::v2::ir::AnaphoraRef::ThatSpell,
                            )),
                            free,
                        },
                        Duration::UntilEndOfTurn,
                    )],
                    (),
                ));
            }
            ts.restore(acp);
        }
        let cp2 = ts.checkpoint();
        let _ = ts.eat_phrase(&["any", "number", "of"])
            || (ts.eat_phrase(&["up", "to"]) && opt(ts, parse_value_expr).is_some());
        let subj_opt =
            if ts.eat_any_phrase(&[&["the", "other", "cards"], &["the", "other", "card"]]) {
                Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::OtherPile))
            } else {
                try_parse(ts, parse_subject_ref).ok()
            };
        if let Some(subj) = subj_opt {
            let mut from_zones = Vec::new();
            let mut top_only = false;
            {
                let f_outer = ts.checkpoint();
                if ts.eat_word("from") {
                    let mut matched_any = false;
                    loop {
                        let f1_cp = ts.checkpoint();
                        if let Some((z_opt, ext_top)) =
                            super::player_verbs_ext::eat_extended_cast_source(ts)
                        {
                            matched_any = true;
                            top_only |= ext_top;
                            if let Some(z) = z_opt {
                                from_zones.push(z);
                            }
                        } else if ts.eat_word("among") {
                            let acp = ts.checkpoint();
                            if ts
                                .eat_any_phrase(&[&["cards", "you", "own", "in"], &["cards", "in"]])
                                && let Ok(z) = try_parse(ts, parse_zone_ref)
                            {
                                matched_any = true;
                                from_zones.push(z);
                            } else {
                                ts.restore(acp);
                                if try_parse(ts, parse_subject_ref).is_ok() {
                                    matched_any = true;
                                    let fz_cp = ts.checkpoint();
                                    if ts.eat_word("from") {
                                        if let Ok(z) = try_parse(ts, parse_zone_ref) {
                                            from_zones.push(z);
                                        } else {
                                            ts.restore(fz_cp);
                                        }
                                    }
                                } else {
                                    ts.restore(f1_cp);
                                    break;
                                }
                            }
                        } else if let Ok(z) = try_parse(ts, parse_zone_ref) {
                            matched_any = true;
                            from_zones.push(z);
                        } else {
                            ts.restore(f1_cp);
                            break;
                        }
                        let fcp = ts.checkpoint();
                        let _ = ts.eat_comma();
                        let _ = ts.eat_any_word(&["or", "and"]);
                        if ts.eat_word("from") {
                            continue;
                        }
                        ts.restore(fcp);
                        break;
                    }
                    if !matched_any {
                        ts.restore(f_outer);
                    }
                }
            }
            let _ = ts.eat_phrase(&["if", "able"]);
            let eat_free = |s: &mut TokenStream<'_>| {
                s.eat_any_phrase(&[
                    &["without", "paying", "its", "mana", "cost"],
                    &["without", "paying", "their", "mana", "costs"],
                ])
            };
            let mut free = eat_free(ts);
            let alt_costs = if !free && ts.eat_word("by") {
                let costs = crate::v2::cost::parse_cost_list(ts).unwrap_or_default();
                let _ = ts.eat_any_phrase(&[
                    &["rather", "than", "paying", "its", "mana", "cost"],
                    &["rather", "than", "paying", "their", "mana", "costs"],
                ]) || ts.eat_in_addition_to_other_costs();
                costs
            } else {
                Vec::new()
            };
            let dur = parse_trailing_duration(ts).unwrap_or(Duration::Instant);
            if !free {
                free = eat_free(ts);
            }
            let mut out = vec![build_play_permission_effect(
                actor,
                subj,
                true,
                free,
                &from_zones,
                top_only,
                dur.clone(),
            )];
            if !alt_costs.is_empty() {
                out.push(apply_continuous(
                    StaticAbility::AlternativeCost(alt_costs),
                    dur.clone(),
                ));
            }
            let m_cp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&[
                "and", "mana", "of", "any", "type", "can", "be", "spent", "to", "cast",
            ]) && ts.eat_any_phrase(&[
                &["those", "spells"],
                &["that", "spell"],
                &["it"],
                &["them"],
            ]) {
                out.push(player_action(
                    actor,
                    PlayerActionKind::SpendManaAsAnyType,
                    dur,
                ));
            } else {
                ts.restore(m_cp);
                super::player_verbs_ext::parse_trailing_play_mana_rider(ts, actor, dur, &mut out);
            }
            return Ok((out, ()));
        }
        ts.restore(cp2);
    }

    // 3d0. `switch(es) <subj>'s power and toughness [until end of turn]`
    if ts.eat_any_word(&["switch", "switches"]) {
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
                vec![mutate_obj(
                    subj,
                    ObjectMutation::StateAction(ObjectStateAction::SwitchPowerAndToughness),
                    duration,
                )],
                (),
            ));
        }
        return Err(ts.err("expected `power and toughness` after `switch`"));
    }

    // 3d1. `remove(s) <subj> from combat`
    {
        let rcp = ts.checkpoint();
        if ts.eat_any_word(&["remove", "removes"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["from", "combat"])
        {
            return Ok((
                vec![mutate_obj(
                    subj,
                    ObjectMutation::StateAction(ObjectStateAction::RemoveFromCombat),
                    Duration::Instant,
                )],
                (),
            ));
        }
        ts.restore(rcp);
    }

    // 3d2. `take(s) the initiative` (placed BEFORE `take an extra turn` so `take` never shadows it!)
    if ts.eat_any_phrase(&[
        &["take", "the", "initiative"],
        &["takes", "the", "initiative"],
    ]) {
        return Ok((
            vec![player_action(
                actor,
                PlayerActionKind::TakeInitiative,
                Duration::Instant,
            )],
            (),
        ));
    }

    // 3d. `have no maximum hand size [dur]` OR `have <subj> deal/fight/become/get ...`
    if ts.eat_any_word(&["have", "has"]) {
        if let Some(res) = super::player_verbs_ext::try_parse_have_ext(ts, actor) {
            return res;
        }
        if ts.eat_phrase(&["no", "maximum", "hand", "size"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
            return Ok((
                vec![player_action(
                    actor,
                    PlayerActionKind::NoMaxHandSize,
                    duration,
                )],
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
        let set_base_pt = |subject, power, toughness, duration| {
            mutate_obj(
                subject,
                ObjectMutation::SetDynamicBasePT {
                    power,
                    toughness,
                    add_types: crate::ast::CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                duration,
            )
        };
        // `have <subj>'s base power and toughness become <P>/<T> [or <P2>/<T2>] [dur]`
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
                let eff1 = set_base_pt(
                    subj.clone(),
                    ValueExpr::Const(p1),
                    ValueExpr::Const(t1),
                    duration.clone(),
                );
                if let Some((p2, t2)) = alt_pt {
                    let eff2 =
                        set_base_pt(subj, ValueExpr::Const(p2), ValueExpr::Const(t2), duration);
                    return Ok((
                        vec![prompt_choice(
                            actor,
                            ChoiceKind::ChooseBranch(vec![vec![eff1], vec![eff2]]),
                        )],
                        (),
                    ));
                }
                return Ok((vec![eff1], ()));
            }
            ts.restore(bpt_cp);
        }
        // `have the base power [and toughness] of <subj> become [equal to] <val> [dur]`
        {
            let bp_cp = ts.checkpoint();
            if ts.eat_any_phrase(&[
                &["the", "base", "power", "and", "toughness", "of"],
                &["the", "base", "power", "of"],
            ]) && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_any_word(&["become", "becomes"])
            {
                let _ = ts.eat_phrase(&["equal", "to"]);
                if let Ok(val) = try_parse(ts, parse_value_expr) {
                    let _ = ts.eat_phrase(&["and", "toughness"]);
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok((vec![set_base_pt(subj, val.clone(), val, duration)], ()));
                }
            }
            ts.restore(bp_cp);
        }
        // `have <subj>'s base power [and toughness] become [equal to] <val> [dur]`
        {
            let sp_cp = ts.checkpoint();
            if let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("'s")
                && ts.eat_any_phrase(&[
                    &["base", "power", "and", "toughness"],
                    &["base", "power"],
                    &["base", "toughness"],
                ])
                && ts.eat_any_word(&["become", "becomes"])
            {
                let _ = ts.eat_phrase(&["equal", "to"]);
                if let Ok(val) = try_parse(ts, parse_value_expr) {
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok((vec![set_base_pt(subj, val.clone(), val, duration)], ()));
                }
            }
            ts.restore(sp_cp);
        }
        // Compositional delegation: `have <player> <player_verb_frame>` OR `have <subj> <subject_verb_frame>`
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
            if ts.eat_any_word(&["deal", "deals"]) {
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
            if ts.eat_any_word(&["fight", "fights"])
                && let Ok(target) = try_parse(ts, parse_subject_ref)
            {
                return Ok((
                    vec![mutate_obj(
                        source,
                        ObjectMutation::Fight(Box::new(target)),
                        Duration::Instant,
                    )],
                    (),
                ));
            }
            if ts.eat_any_word(&["get", "gets"])
                && let Some((power, toughness)) = ts.next_pt_mod()
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok((
                    vec![mutate_obj(
                        source,
                        ObjectMutation::ModifyPT {
                            power,
                            toughness,
                            scale: None,
                        },
                        duration,
                    )],
                    (),
                ));
            }
            if ts.eat_any_phrase(&[
                &["become", "a", "copy", "of"],
                &["becomes", "a", "copy", "of"],
            ]) && let Ok(copy_src) = try_parse(ts, parse_subject_ref)
            {
                let exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                return Ok((
                    vec![mutate_obj(
                        source,
                        ObjectMutation::BecomeCopy {
                            source: Box::new(copy_src),
                            exceptions,
                        },
                        duration,
                    )],
                    (),
                ));
            }
            if ts.eat_any_word(&["deal", "deals"]) {
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
    if ts.eat_any_word(&["roll", "rolls"]) {
        if let Some(res) = super::player_verbs_ext::try_parse_roll_ext(ts, actor) {
            return res;
        }
        let count = if ts.eat_any_word(&["a", "an"]) {
            1u8
        } else {
            ts.next_number(false)
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(1)
        };
        let sides = if ts.eat_any_phrase(&[&["six-sided", "die"], &["six-sided", "dice"]]) {
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
                vec![mutate_player(
                    actor,
                    PlayerMutation::RollDie { sides, count },
                    Duration::Instant,
                )],
                (),
            ));
        }
        return Err(ts.err("expected die specification after `roll`"));
    }

    // 3f. `copy(ies) <subj> [twice | <N> times] [for each <expr>] [, except ...]`
    if ts.eat_any_word(&["copy", "copies"]) {
        // `copy the next <flt> [you cast] this turn when you cast it` / `copy the next loyalty ability ...`
        {
            let ncp = ts.checkpoint();
            if ts.eat_phrase(&["the", "next"]) {
                let copy_that_spell = || {
                    vec![Effect::CopyObject {
                        actor,
                        subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatSpell),
                        count: ValueExpr::Const(1),
                        modifiers: EntryModifiers::default(),
                        exceptions: Vec::new(),
                    }]
                };
                let trig_ev = if ts.eat_phrase(&[
                    "loyalty", "ability", "you", "activate", "this", "turn", "when", "you",
                    "activate", "it",
                ]) {
                    Some(crate::v2::ir::TriggerEvent::AbilityActivated {
                        player: actor,
                        non_mana_only: true,
                    })
                } else if let Ok(flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_any_phrase(&[
                        &["you", "cast", "this", "turn", "when", "you", "cast", "it"],
                        &["this", "turn", "when", "you", "cast", "it"],
                    ])
                {
                    Some(crate::v2::ir::TriggerEvent::SpellCast {
                        caster: actor,
                        spell: Some(flt),
                        from_outside_hand: false,
                    })
                } else {
                    None
                };
                if let Some(ev) = trig_ev {
                    return Ok((
                        vec![Effect::DelayedTrigger {
                            trigger: crate::v2::ir::DelayedTriggerKind::Event(
                                crate::v2::ir::TriggerSpec {
                                    events: vec![ev],
                                    intervening_if: None,
                                },
                            ),
                            body: copy_that_spell(),
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
            let v_opt = if let Some(n) = ts.next_number(false) {
                Some(ValueExpr::Const(n))
            } else if ts.eat_word("x") {
                Some(ValueExpr::Var(crate::v2::ir::VarName::X))
            } else {
                None
            };
            if let Some(v) = v_opt
                && ts.eat_word("times")
            {
                v
            } else {
                ts.restore(ncp);
                super::player_verbs_ext::try_eat_copy_for_each_target(ts)
                    .or_else(|| opt(ts, parse_for_each_expr))
                    .unwrap_or(ValueExpr::Const(1))
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
        // Optional trailing `[,] and [you] may choose new targets for the copy/copies`
        {
            let rcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and") {
                let _ = ts.eat_word("you");
                if ts.eat_any_phrase(&[
                    &["may", "choose", "new", "targets", "for", "the", "copy"],
                    &["may", "choose", "new", "targets", "for", "the", "copies"],
                ]) {
                    out.push(Effect::Optional {
                        actor,
                        body: vec![mutate_player(
                            actor,
                            PlayerMutation::Retarget {
                                subject: Box::new(SubjectRef::Anaphora(
                                    crate::v2::ir::AnaphoraRef::ThatSpell,
                                )),
                                new_target: None,
                            },
                            Duration::Instant,
                        )],
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

    // 3f3. `change the text of <subj> by replacing all instances of ...`
    if ts.eat_any_phrase(&[
        &["change", "the", "text", "of"],
        &["changes", "the", "text", "of"],
    ]) {
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&["by", "replacing", "all", "instances", "of"]) {
            let kind = if ts.eat_any_phrase(&[
                &[
                    "one", "color", "word", "with", "another", "or", "one", "basic", "land",
                    "type", "with", "another",
                ],
                &["one", "color", "word", "with", "another"],
            ]) {
                CharacteristicKind::Color
            } else if ts.eat_phrase(&["one", "basic", "land", "type", "with", "another"]) {
                CharacteristicKind::BasicLandType
            } else if ts.eat_any_phrase(&[
                &["one", "creature", "type", "with", "another"],
                &["one", "creature", "type", "with"],
            ]) {
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
                vec![mutate_obj(
                    subject,
                    ObjectMutation::ChangeText(kind),
                    duration,
                )],
                (),
            ));
        }
    }

    // 3f2. `change(s) [the | a | any] target(s) of <subj> [to <new_tgt>]`
    if ts.eat_any_phrase(&[
        &["change", "the", "target", "of"],
        &["changes", "the", "target", "of"],
        &["change", "the", "targets", "of"],
        &["change", "a", "target", "of"],
        &["change", "any", "targets", "of"],
    ]) {
        let subject = parse_subject_ref(ts)?;
        let new_target = if ts.eat_word("to") {
            Some(Box::new(parse_subject_ref(ts)?))
        } else {
            None
        };
        return Ok((
            vec![mutate_player(
                actor,
                PlayerMutation::Retarget {
                    subject: Box::new(subject),
                    new_target,
                },
                Duration::Instant,
            )],
            (),
        ));
    }

    // 3g. `flip(s) a coin / <val> coins ...`
    if let Some(res) = super::player_verbs_ext::try_parse_flip_ext(ts, actor) {
        return res;
    }

    // 3h. `become(s) the monarch` / `take(s) an extra turn [after this one]`
    if ts.eat_any_word(&["become", "becomes"]) && ts.eat_phrase(&["the", "monarch"]) {
        return Ok((
            vec![player_action(
                actor,
                PlayerActionKind::BecomeMonarch,
                Duration::Instant,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["take", "takes"]) && ts.eat_phrase(&["an", "extra", "turn"]) {
        let _ = ts.eat_phrase(&["after", "this", "one"]);
        return Ok((
            vec![player_action(
                actor,
                PlayerActionKind::ExtraTurn,
                Duration::Instant,
            )],
            (),
        ));
    }

    // 3i. `double(s) the number of [<kind>] counters on <subj>` OR `double(s) <subj>'s power [and toughness] [<val> times] [until end of turn]`
    if ts.eat_any_word(&["double", "doubles"]) {
        if ts.eat_phrase(&["the", "number", "of"]) {
            let kind = if ts.eat_any_word(&["counters", "counter"]) {
                None
            } else {
                let ck = parse_counter_kind(ts)?;
                if !ts.eat_any_word(&["counters", "counter"]) {
                    return Err(ts.err("expected `counters` in double counters"));
                }
                Some(ck)
            };
            if !ts.eat_word("on") {
                return Err(ts.err("expected `on` in double counters"));
            }
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::DoubleCounters { kind },
                    Duration::Instant,
                )],
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
                        && ts.eat_any_word(&["times", "time"])
                    {
                        Some(v)
                    } else {
                        ts.restore(tcp);
                        None
                    }
                };
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                let mut eff = mutate_obj(
                    subject.clone(),
                    ObjectMutation::ModifyPT {
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
                );
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
    if ts.eat_any_word(&["exchange", "exchanges"]) {
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
            let set_to = |player, src| Effect::AdjustLife {
                player,
                amount: ValueExpr::PlayerStat {
                    player: src,
                    stat: crate::ast::PlayerStatKind::LifeTotal,
                },
                op: LifeOp::Set,
            };
            return Ok((vec![set_to(actor, other), set_to(other, actor)], ()));
        }
        // `exchange <player>'s life total with <subj>'s power/toughness`
        {
            let lcp = ts.checkpoint();
            const LIFE_TOTAL_OWNER: &[(&[&str], PlayerRef)] = &[
                (&["your", "life", "total"], PlayerRef::You),
                (
                    &["target", "opponent's", "life", "total"],
                    PlayerRef::TargetOpponent,
                ),
                (
                    &["target", "player's", "life", "total"],
                    PlayerRef::TargetPlayer,
                ),
                (&["their", "life", "total"], PlayerRef::ThatPlayer),
                (
                    &["that", "player's", "life", "total"],
                    PlayerRef::ThatPlayer,
                ),
            ];
            let life_p = LIFE_TOTAL_OWNER
                .iter()
                .find_map(|&(p, who)| ts.eat_phrase(p).then_some(who));
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
                            mutate_obj(
                                subj,
                                ObjectMutation::SetDynamicBasePT {
                                    power: p_expr,
                                    toughness: t_expr,
                                    add_types: crate::ast::CardTypeMask::EMPTY,
                                    keep_existing_types: true,
                                },
                                Duration::Permanent,
                            ),
                        ],
                        (),
                    ));
                }
            }
            ts.restore(lcp);
        }
        // `exchange your/their <zone1> and <zone2>`
        {
            let zcp = ts.checkpoint();
            if let Ok(z1) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("and")
            {
                let z2_opt = if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                    Some(z2)
                } else {
                    [
                        ("library", ZoneKind::Library),
                        ("graveyard", ZoneKind::Graveyard),
                        ("hand", ZoneKind::Hand),
                    ]
                    .into_iter()
                    .find_map(|(w, zone)| {
                        ts.eat_word(w).then_some(ZoneRef {
                            zone,
                            owner: z1.owner,
                        })
                    })
                };
                if let Some(z2) = z2_opt {
                    let swap = |from_z: ZoneRef, to_z: ZoneRef| {
                        simple_zone_op(
                            actor,
                            ZoneOpKind::Move,
                            all_zone_cards_subject(from_z),
                            vec![from_z],
                            Some(simple_zone_dest(to_z.zone, to_z.owner)),
                        )
                    };
                    return Ok((vec![swap(z1, z2), swap(z2, z1)], ()));
                }
            }
            ts.restore(zcp);
        }
        return Err(ts.err("expected target after `exchange`"));
    }

    // 3k. `control(s) <player> during that player's/their next turn`
    {
        let ccp = ts.checkpoint();
        if ts.eat_any_word(&["control", "controls"])
            && let Ok(target_p) = try_parse(ts, parse_player_ref)
            && ts.eat_any_phrase(&[
                &["during", "that", "player's", "next", "turn"],
                &["during", "their", "next", "turn"],
            ])
        {
            return Ok((
                vec![mutate_obj(
                    SubjectRef::Player(target_p),
                    ObjectMutation::GainControl { controller: actor },
                    Duration::UntilEndOfNextTurn,
                )],
                (),
            ));
        }
        ts.restore(ccp);
    }

    // 4. `pay(s) <cost>`
    if ts.eat_any_word(&["pay", "pays"]) {
        let is_energy = |s: &TokenStream<'_>| {
            matches!(
                s.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        };
        let cost = if ts.eat_phrase(&["any", "amount", "of", "mana"]) {
            Cost::PayAnyAmountMana
        } else if ts.eat_phrase(&["any", "amount", "of", "life"]) {
            Cost::PayAnyAmountLife
        } else if ts.eat_any_phrase(&[&["any", "amount", "of"], &["one", "or", "more"]])
            && is_energy(ts)
        {
            ts.next_tok();
            Cost::PayEnergy(ValueExpr::Var(crate::v2::ir::VarName::X))
        } else if ts.eat_phrase(&["life", "equal", "to"]) {
            Cost::PayLife(parse_value_expr(ts)?)
        } else if ts.eat_phrase(&["an", "amount", "of"]) && is_energy(ts) {
            ts.next_tok();
            if !ts.eat_phrase(&["equal", "to"]) {
                return Err(ts.err("expected `equal to` after `an amount of {E}`"));
            }
            Cost::PayEnergy(parse_value_expr(ts)?)
        } else if let Some(mc) = ts.eat_mana_cost() {
            Cost::Mana(mc)
        } else {
            let amt = parse_value_expr(ts)?;
            if ts.eat_word("life") {
                Cost::PayLife(amt)
            } else if is_energy(ts) {
                ts.next_tok();
                Cost::PayEnergy(amt)
            } else {
                return Err(ts.err("expected `life`, energy, or mana after `pay`"));
            }
        };
        let _ = ts.eat_phrase(&["any", "number", "of", "times"]);
        let mut out = vec![mutate_player(
            actor,
            PlayerMutation::PayCost(cost),
            Duration::Instant,
        )];
        // Support compound `pay {M} and <N> life`
        {
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(amt) = try_parse(ts, parse_value_expr)
                && ts.eat_word("life")
            {
                out.push(mutate_player(
                    actor,
                    PlayerMutation::PayCost(Cost::PayLife(amt)),
                    Duration::Instant,
                ));
            } else {
                ts.restore(acp);
            }
        }
        return Ok((out, ()));
    }

    // 5. `gain(s) control of <subj> [dur]`
    if ts.eat_any_phrase(&[&["gain", "control", "of"], &["gains", "control", "of"]]) {
        let subject = parse_subject_ref(ts)?;
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        return Ok((
            vec![mutate_obj(
                subject,
                ObjectMutation::GainControl { controller: actor },
                duration,
            )],
            (),
        ));
    }

    // 6. `gain(s) / lose(s) life ...`
    if let Some(op) = if ts.eat_any_word(&["gain", "gains"]) {
        Some(LifeOp::Gain)
    } else if ts.eat_any_word(&["lose", "loses"]) {
        Some(LifeOp::Lose)
    } else {
        None
    } {
        if op == LifeOp::Lose && ts.eat_phrase(&["the", "game"]) {
            return Ok((
                vec![player_action(
                    actor,
                    PlayerActionKind::LoseGame,
                    Duration::Instant,
                )],
                (),
            ));
        }
        if op == LifeOp::Lose
            && ts.eat_any_phrase(&[
                &["half", "their", "life", "total"],
                &["half", "your", "life", "total"],
                &["half", "their", "life"],
                &["half", "your", "life"],
            ])
        {
            let rounding = eat_rounding(ts, crate::ast::Rounding::Down);
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
        if op == LifeOp::Gain
            && ts.eat_any_phrase(&[
                &["life", "and", "draw", "cards", "equal", "to"],
                &["life", "and", "draws", "cards", "equal", "to"],
            ])
        {
            return super::player_verbs_ext::parse_gain_life_and_draw_tail(ts, actor);
        }
        // Paired `loses life and discards cards equal to <val>`
        if op == LifeOp::Lose
            && ts.eat_any_phrase(&[
                &["life", "and", "discards", "cards", "equal", "to"],
                &["life", "and", "discard", "cards", "equal", "to"],
            ])
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
                        counted_zone_op(
                            PlayerRef::ThatPlayer,
                            ZoneOpKind::Discard {
                                hand: false,
                                random: false,
                            },
                            SubjectRef::Group(Box::new(ObjectFilter {
                                any_card: true,
                                ..Default::default()
                            })),
                            vec![ZoneRef {
                                zone: ZoneKind::Hand,
                                owner: ZoneOwner::ThatPlayer,
                            }],
                            None,
                            Some(exact_count(val)),
                        ),
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
    if ts.eat_any_word(&["choose", "chooses"]) {
        if let Some(res) = super::player_verbs_ext::try_parse_choose_ext(ts, actor) {
            return res;
        }
        if ts.eat_any_phrase(&[&["new", "targets", "for"], &["a", "new", "target", "for"]]) {
            let subj = parse_subject_ref(ts)?;
            return Ok((
                vec![mutate_player(
                    actor,
                    PlayerMutation::Retarget {
                        subject: Box::new(subj),
                        new_target: None,
                    },
                    Duration::Instant,
                )],
                (),
            ));
        }
        let choose_char = |kind| {
            Ok((
                vec![prompt_choice(actor, ChoiceKind::ChooseCharacteristic(kind))],
                (),
            ))
        };
        if ts.eat_phrase(&["odd", "or", "even"]) {
            return choose_char(CharacteristicKind::OddOrEven);
        }
        if ts.eat_any_phrase(&[
            &["khans", "or", "dragons"],
            &["dragons", "or", "khans"],
            &["abzan", "or", "mardu"],
            &["temur", "or", "sultai"],
            &["jeskai", "or", "abzan"],
            &["mardu", "or", "temur"],
            &["sultai", "or", "jeskai"],
            &["sultai", "or", "abzan"],
            &["abzan", "or", "sultai"],
            &["war", "or", "peace"],
            &["left", "or", "right"],
            &["friend", "or", "foe"],
            &["friends", "or", "foes"],
        ]) {
            return choose_char(CharacteristicKind::BinaryMode);
        }
        {
            let bcp = ts.checkpoint();
            if let Some(n) = ts.next_number(false)
                && ts.eat_phrase(&["basic", "land", "types"])
            {
                return choose_char(CharacteristicKind::BasicLandTypes(
                    u8::try_from(n.max(1)).unwrap_or(2),
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
            return choose_char(CharacteristicKind::CreatureType);
        }
        if ts.eat_phrase(&["a", "card", "type"]) {
            if ts.eat_phrase(&["other", "than"]) {
                while let Some(w) = ts.peek_word() {
                    if crate::vocab::lookup_card_type(&w.to_ascii_lowercase()).is_some() {
                        ts.next_word();
                        let _ = ts.eat_comma();
                        let _ = ts.eat_any_word(&["or", "and"]);
                    } else {
                        break;
                    }
                }
            }
            return choose_char(CharacteristicKind::CardType);
        }
        if ts.eat_phrase(&["a", "color"]) {
            if ts.eat_phrase(&["other", "than"]) {
                while let Some(w) = ts.peek_word() {
                    if crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some() {
                        ts.next_word();
                        let _ = ts.eat_comma();
                        let _ = ts.eat_any_word(&["or", "and"]);
                    } else {
                        break;
                    }
                }
            }
            return choose_char(CharacteristicKind::Color);
        }
        if ts.eat_phrase(&["a", "nonland", "card", "name"])
            || (ts.eat_phrase(&["a", "noncreature"]) && {
                let _ = ts.eat_comma();
                ts.eat_phrase(&["nonland", "card", "name"])
            })
        {
            return choose_char(CharacteristicKind::NonlandCardName);
        }
        if ts.eat_any_phrase(&[&["a", "card", "name"], &["any", "card", "name"]])
            || (ts.eat_phrase(&["the", "name", "of"]) && try_parse(ts, parse_object_filter).is_ok())
        {
            return choose_char(CharacteristicKind::CardName);
        }
        if ts.eat_phrase(&["a", "basic", "land", "type"]) {
            return choose_char(CharacteristicKind::BasicLandType);
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
            vec![prompt_choice(actor, ChoiceKind::ChooseObject(subj))],
            (),
        ));
    }

    // 22. `tap or untap <subj>` / `untap(s) <subj>` / `tap(s) <subj>`
    if ts.eat_any_phrase(&[&["tap", "or", "untap"], &["taps", "or", "untaps"]]) {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![prompt_choice(
                actor,
                ChoiceKind::ChooseBranch(vec![
                    vec![mutate_obj(
                        subject.clone(),
                        ObjectMutation::Tap,
                        Duration::Instant,
                    )],
                    vec![mutate_obj(
                        subject,
                        ObjectMutation::Untap,
                        Duration::Instant,
                    )],
                ]),
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["untap", "untaps"]) {
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
            vec![mutate_obj(
                subject,
                ObjectMutation::Untap,
                Duration::Instant,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["tap", "taps"]) {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![mutate_obj(subject, ObjectMutation::Tap, Duration::Instant)],
            (),
        ));
    }

    // 23. `turn(s) face up <subj>` / `turn(s) <subj> face up|down`
    if ts.eat_any_word(&["turn", "turns"]) {
        if ts.eat_phrase(&["face", "up"]) {
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::TurnFaceUp),
                    Duration::Instant,
                )],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&["face", "up"]) {
            return Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::TurnFaceUp),
                    Duration::Instant,
                )],
                (),
            ));
        }
        if ts.eat_phrase(&["face", "down"]) {
            return Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::TurnFaceDown),
                    Duration::Instant,
                )],
                (),
            ));
        }
    }

    // 24. `counter <subj> [or ability] [unless ...]`
    if ts.eat_word("counter") {
        let subject = if ts.eat_phrase(&["the", "chosen", "spell"]) {
            SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatSpell)
        } else {
            parse_subject_ref(ts)?
        };
        let _ = ts.eat_phrase(&["or", "ability"]);
        let counter_op = |subj, unless_pay| {
            simple_zone_op(
                actor,
                ZoneOpKind::CounterSpell { unless_pay },
                subj,
                vec![ZoneRef {
                    zone: ZoneKind::Stack,
                    owner: ZoneOwner::Default,
                }],
                None,
            )
        };
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
                    return Ok((vec![counter_op(subject, Some(Box::new(final_cost)))], ()));
                }
                if let Ok(payer) = try_parse(ts, parse_player_ref)
                    && let Ok((alt_body, _)) = try_parse(ts, |t| parse_player_verb_frame(t, payer))
                    && !alt_body.is_empty()
                {
                    return Ok((
                        vec![Effect::UnlessEffect {
                            body: vec![counter_op(subject, None)],
                            unless: alt_body,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(ucp);
        }
        return Ok((vec![counter_op(subject, None)], ()));
    }

    // 25. `mill(s) [half their/your library, rounded up/down | cards equal to <val> | <N> card(s)]`
    if ts.eat_any_word(&["mill", "mills"]) {
        let final_cnt = if ts.eat_any_phrase(&[
            &["half", "their", "library"],
            &["half", "your", "library"],
            &["half", "that", "player's", "library"],
        ]) {
            let rounding = eat_rounding(ts, crate::ast::Rounding::Down);
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
            if !ts.eat_any_word(&["card", "cards"]) {
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
            vec![counted_zone_op(
                actor,
                ZoneOpKind::Mill,
                SubjectRef::Player(actor),
                vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                Some(simple_zone_dest(ZoneKind::Graveyard, ZoneOwner::Default)),
                Some(exact_count(final_cnt)),
            )],
            (),
        ));
    }

    // 26. `regenerate(s) <subj>` / `transform(s) <subj>` / `attach(es) <subj> to <host>` / `remove(s) ...`
    if ts.eat_any_word(&["regenerate", "regenerates"]) {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![mutate_obj(
                subject,
                ObjectMutation::StateAction(ObjectStateAction::Regenerate),
                Duration::UntilEndOfTurn,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["transform", "transforms"]) {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![mutate_obj(
                subject,
                ObjectMutation::StateAction(ObjectStateAction::Transform),
                Duration::Instant,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["attach", "attaches"]) {
        let subject = parse_subject_ref(ts)?;
        if !ts.eat_word("to") {
            return Err(ts.err("expected `to` in attach clause"));
        }
        let host = parse_subject_ref(ts)?;
        return Ok((
            vec![mutate_obj(
                subject,
                ObjectMutation::AttachTo(Box::new(host)),
                Duration::Permanent,
            )],
            (),
        ));
    }
    if ts.eat_any_word(&["remove", "removes"]) {
        if ts.eat_any_phrase(&[
            &["those", "counters"],
            &["those", "counter"],
            &["all", "of", "them"],
            &["them", "all"],
            &["them"],
        ]) {
            let subject = if ts.eat_word("from") {
                parse_subject_ref(ts)?
            } else {
                SubjectRef::SelfObject
            };
            return Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::RemoveCounters {
                        kind: CounterKind::Any,
                        count: None,
                        all: true,
                    },
                    Duration::Instant,
                )],
                (),
            ));
        }
        let _ = ts.eat_phrase(&["up", "to"]);
        let (all, count) = if ts.eat_phrase(&["all", "but"]) {
            (false, Some(parse_value_expr(ts)?))
        } else if ts.eat_phrase(&["any", "number", "of"]) {
            (false, Some(ValueExpr::Const(0)))
        } else if ts.eat_word("all") {
            (true, None)
        } else if ts.eat_any_word(&["a", "an"]) {
            (false, Some(ValueExpr::Const(1)))
        } else {
            (false, Some(parse_value_expr(ts)?))
        };
        let kind = if ts.eat_any_word(&["counter", "counters"]) {
            CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !ts.eat_any_word(&["counter", "counters"]) {
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
            vec![mutate_obj(
                subject,
                ObjectMutation::RemoveCounters { kind, count, all },
                Duration::Instant,
            )],
            (),
        ));
    }

    // 26b. `give <subj> another counter of that kind`
    {
        let gcp = ts.checkpoint();
        if ts.eat_any_word(&["give", "gives"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_any_phrase(&[
                &["another", "counter", "of", "that", "kind"],
                &["a", "counter", "of", "that", "kind"],
            ])
        {
            return Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::PutCounters {
                        kind: CounterKind::Any,
                        count: ValueExpr::Const(1),
                    },
                    Duration::Instant,
                )],
                (),
            ));
        }
        ts.restore(gcp);
    }

    // 27. `prevent ... damage ...`
    if ts.eat_any_word(&["prevent", "prevents"]) {
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
