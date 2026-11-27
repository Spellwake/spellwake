//! Extended compositional [`ConditionExpr`] grammar families for `v2`.

use crate::ast::{
    CardTypeMask, CompareOp, ControllerRelation, PlayerStatKind, StatKind, TurnStepAst,
};
use crate::v2::filter::{
    parse_keyword_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AggregateOp, AnaphoraRef, ConditionExpr, ConditionFlag, EventCheckKind, FilterConditionKind,
    FilterFlags, FilterPredicate, ManaSymbol, ObjectFilter, PlayerConditionKind, PlayerRef,
    PriorActionKind, SharedProperty, SubjectConditionKind, SubjectRef, TurnEventKind, ValueExpr,
    ZoneKind, ZoneMetricKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;

use super::{
    cmp_cond, eat_optional_counter_kind, eat_prefix_cmp_op, eat_stat_kind, eat_suffix_cmp_op,
    eat_table, eat_was_cast_target, not_cond, obj_stat_val, player_check, player_stat_val,
    prior_event, to_min_u16,
};

/// Parse `there is / there are ...` existential and threshold conditions.
pub(super) fn parse_there_are_condition(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["there", "is", "no", "monarch"]) {
        return Some(not_cond(player_check(
            PlayerRef::EachPlayer,
            PlayerConditionKind::IsMonarch,
        )));
    }
    if ts.eat_phrase(&[
        "there", "is", "an", "instant", "card", "and", "a", "sorcery", "card", "in",
    ]) && let Ok(zone) = try_parse(ts, parse_zone_ref)
    {
        return Some(ConditionExpr::And(
            Box::new(ConditionExpr::ZoneHasCount {
                filter: Box::new(ObjectFilter {
                    any_types: CardTypeMask::INSTANT,
                    any_card: true,
                    ..Default::default()
                }),
                zone,
                min_count: 1,
            }),
            Box::new(ConditionExpr::ZoneHasCount {
                filter: Box::new(ObjectFilter {
                    any_types: CardTypeMask::SORCERY,
                    any_card: true,
                    ..Default::default()
                }),
                zone,
                min_count: 1,
            }),
        ));
    }
    ts.restore(cp);
    if ts.eat_any_phrase(&[&["there", "is", "exactly"], &["there", "are", "exactly"]]) {
        if let Some(n) = ts.next_number(false) {
            let kcp = ts.checkpoint();
            if let Some(kind) = eat_optional_counter_kind(ts)
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Some(cmp_cond(
                    ValueExpr::CounterCount {
                        subject: Box::new(subject),
                        kind,
                    },
                    CompareOp::Eq,
                    ValueExpr::Const(n),
                ));
            }
            ts.restore(kcp);
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                if flt.zones.is_empty()
                    && ts.eat_word("in")
                    && let Ok(zone) = try_parse(ts, parse_zone_ref)
                {
                    flt.zones.push(zone);
                }
                return Some(cmp_cond(
                    ValueExpr::Count(Box::new(flt)),
                    CompareOp::Eq,
                    ValueExpr::Const(n),
                ));
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["there", "are", "fewer", "than"])
        && let Some(n) = ts.next_number(false)
        && let Ok(mut flt) = try_parse(ts, parse_object_filter)
    {
        if flt.zones.is_empty()
            && ts.eat_word("in")
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            flt.zones.push(zone);
        }
        return Some(cmp_cond(
            ValueExpr::Count(Box::new(flt)),
            CompareOp::Lt,
            ValueExpr::Const(n),
        ));
    }
    ts.restore(cp);
    if ts.eat_any_phrase(&[&["there", "are"], &["there", "were"]]) {
        let _ = ts.eat_phrase(&["at", "least"]);
        let min_count = ts
            .next_number(false)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(1);
        let _ = ts.eat_phrase(&["or", "more"]);
        if ts.eat_any_phrase(&[
            &["card", "types", "among", "the", "sacrificed", "permanents"],
            &["card", "types", "among", "the", "sacrificed", "creatures"],
        ]) {
            return Some(cmp_cond(
                ValueExpr::DistinctCardTypesAmong(Box::new(ObjectFilter {
                    any_permanent: true,
                    predicates: vec![FilterPredicate::AffectedByPriorAction(
                        PriorActionKind::SacrificedThisWay,
                    )],
                    ..Default::default()
                })),
                CompareOp::Ge,
                ValueExpr::Const(i32::from(min_count)),
            ));
        }
        if ts.eat_any_phrase(&[
            &["card", "types", "among", "cards", "exiled", "with"],
            &["permanent", "types", "among", "cards", "exiled", "with"],
        ]) && (ts.eat_tilde() || ts.eat_word("it"))
        {
            return Some(cmp_cond(
                ValueExpr::DistinctCardTypesAmong(Box::new(ObjectFilter {
                    any_card: true,
                    flags: FilterFlags::EXILED_WITH_SELF,
                    ..Default::default()
                })),
                CompareOp::Ge,
                ValueExpr::Const(i32::from(min_count)),
            ));
        }
        if ts.eat_any_phrase(&[
            &["card", "types", "among", "cards", "in"],
            &["permanent", "types", "among", "cards", "in"],
        ]) && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Some(ConditionExpr::ZoneMetric {
                zone,
                kind: ZoneMetricKind::DistinctCardTypes,
                min_count: u16::from(u8::try_from(min_count).unwrap_or(4)),
            });
        }
        if ts.eat_phrase(&["basic", "land", "types", "among", "lands"])
            && let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_any_word(&["control", "controls"])
        {
            return Some(cmp_cond(
                ValueExpr::DomainCount(player),
                CompareOp::Ge,
                ValueExpr::Const(i32::from(min_count)),
            ));
        }
        if ts.eat_phrase(&["mana", "values", "among", "cards", "in"])
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Some(cmp_cond(
                ValueExpr::DistinctManaValues(zone),
                CompareOp::Ge,
                ValueExpr::Const(i32::from(min_count)),
            ));
        }
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["different", "kinds", "of", "counters", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Some(cmp_cond(
                    ValueExpr::DistinctCounterKinds(Box::new(flt)),
                    CompareOp::Ge,
                    ValueExpr::Const(i32::from(min_count)),
                ));
            }
            ts.restore(dcp);
        }
        // `there are <N> or more [<kind>] counters (on <subj> | among <flt>)`
        {
            let ccp = ts.checkpoint();
            if let Some(kind) = eat_optional_counter_kind(ts) {
                if ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Some(cmp_cond(
                        ValueExpr::CounterCount {
                            subject: Box::new(subject),
                            kind,
                        },
                        CompareOp::Ge,
                        ValueExpr::Const(i32::from(min_count)),
                    ));
                }
                if ts.eat_word("among")
                    && let Ok(flt) = try_parse(ts, parse_object_filter)
                {
                    return Some(cmp_cond(
                        ValueExpr::CounterCount {
                            subject: Box::new(SubjectRef::All(Box::new(flt))),
                            kind,
                        },
                        CompareOp::Ge,
                        ValueExpr::Const(i32::from(min_count)),
                    ));
                }
            }
            ts.restore(ccp);
        }
        if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
            let _ = ts.eat_word("total");
            let zone = if let Some(z) = flt.zones.pop() {
                Some(z)
            } else if ts.eat_word("in") {
                try_parse(ts, parse_zone_ref).ok()
            } else {
                None
            };
            if let Some(zone) = zone {
                return Some(ConditionExpr::ZoneHasCount {
                    filter: Box::new(flt),
                    zone,
                    min_count,
                });
            }
            return Some(cmp_cond(
                ValueExpr::Count(Box::new(flt)),
                CompareOp::Ge,
                ValueExpr::Const(i32::from(min_count)),
            ));
        }
    }
    ts.restore(cp);
    None
}

/// Parse extended compositional condition families (called as fallback after core/player/subject rules).
pub(super) fn parse_extended_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // Turn / phase / step checks
    if ts.eat_any_phrase(&[
        &["it", "isn't", "your", "turn"],
        &["it's", "an", "opponent's", "turn"],
    ]) {
        return Ok(not_cond(ConditionExpr::Flag(ConditionFlag::IsYourTurn)));
    }
    const STEP_CHECKS: &[(&[&str], TurnStepAst, bool)] = &[
        (
            &["it's", "your", "main", "phase"],
            TurnStepAst::MainPhase,
            false,
        ),
        (
            &["it", "isn't", "your", "main", "phase"],
            TurnStepAst::MainPhase,
            true,
        ),
        (
            &["it's", "not", "your", "main", "phase"],
            TurnStepAst::MainPhase,
            true,
        ),
    ];
    for &(phrase, step, neg) in STEP_CHECKS {
        if ts.eat_phrase(phrase) {
            let base = ConditionExpr::EventCheck(EventCheckKind::DuringStep(step));
            return Ok(if neg { not_cond(base) } else { base });
        }
    }

    // Meld ownership + control check: `you both own and control ~ and <flt>`
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "both", "own", "and", "control"])
            && ts.eat_tilde()
            && ts.eat_word("and")
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            // Consume optional lowercase-prefix apostrophe title fragment (e.g., `x'Y...`)
            if let Some(w) = ts.peek_word()
                && let Some((_, tail)) = w.split_once('\'')
                && tail.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            {
                ts.next_word();
            }
            flt.predicates
                .push(FilterPredicate::OwnedAndControlledByYou);
            return Ok(ConditionExpr::And(
                Box::new(ConditionExpr::Flag(ConditionFlag::ControlsSelf)),
                Box::new(ConditionExpr::Controls {
                    player: PlayerRef::You,
                    filter: Box::new(flt),
                    min_count: 1,
                }),
            ));
        }
        ts.restore(mcp);
    }

    // Cards exiled with source: `<N> or more cards have been exiled with ~`
    {
        let ecp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "have", "been", "exiled", "with"])
            && (ts.eat_tilde() || ts.eat_word("it"))
        {
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(ObjectFilter {
                    any_card: true,
                    flags: FilterFlags::EXILED_WITH_SELF,
                    ..Default::default()
                })),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(ecp);
    }

    // Life total vs starting life total & Monarch checks
    const LIFE_MONARCH_CHECKS: &[(&[&str], PlayerRef, PlayerConditionKind, bool)] = &[
        (
            &[
                "your", "life", "total", "is", "less", "than", "your", "starting", "life", "total",
            ],
            PlayerRef::You,
            PlayerConditionKind::LifeAboveStarting { margin: 0 },
            true,
        ),
        (
            &[
                "your", "life", "total", "is", "greater", "than", "or", "equal", "to", "your",
                "starting", "life", "total",
            ],
            PlayerRef::You,
            PlayerConditionKind::LifeAboveStarting { margin: 0 },
            false,
        ),
        (
            &["you're", "not", "the", "monarch"],
            PlayerRef::You,
            PlayerConditionKind::IsMonarch,
            true,
        ),
        (
            &["you", "aren't", "the", "monarch"],
            PlayerRef::You,
            PlayerConditionKind::IsMonarch,
            true,
        ),
    ];
    for &(phrase, player, kind, neg) in LIFE_MONARCH_CHECKS {
        if ts.eat_phrase(phrase) {
            let base = player_check(player, kind);
            return Ok(if neg { not_cond(base) } else { base });
        }
    }
    {
        let pcp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_any_phrase(&[&["is", "the", "monarch"], &["are", "the", "monarch"]]) {
                return Ok(player_check(player, PlayerConditionKind::IsMonarch));
            }
            if ts.eat_any_phrase(&[&["is", "poisoned"], &["are", "poisoned"]]) {
                return Ok(player_check(player, PlayerConditionKind::Poisoned));
            }
        }
        ts.restore(pcp);
    }

    // Mana spent comparisons
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "amount", "of", "mana", "spent", "to", "cast"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["was", "less", "than", "its", "mana", "value"])
        {
            return Ok(cmp_cond(
                ValueExpr::ManaPaidThisWay,
                CompareOp::Lt,
                obj_stat_val(subj, StatKind::ManaValue),
            ));
        }
        ts.restore(mcp);
        if ts.eat_phrase(&["no", "mana", "was", "spent", "to", "cast"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(subj),
                kind: SubjectConditionKind::NotCastOrNoManaSpent,
            });
        }
        ts.restore(mcp);
        if ts.eat_phrase(&[
            "none", "of", "them", "were", "cast", "or", "no", "mana", "was", "spent", "to", "cast",
            "them",
        ]) {
            return Ok(ConditionExpr::SubjectCheck {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
                kind: SubjectConditionKind::NotCastOrNoManaSpent,
            });
        }
        ts.restore(mcp);
        if ts.eat_phrase(&["at", "least"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["mana", "was", "spent", "to", "cast"])
            && eat_was_cast_target(ts)
        {
            return Ok(cmp_cond(
                ValueExpr::ManaPaidThisWay,
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(mcp);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "mana", "from"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && eat_was_cast_target(ts)
        {
            return Ok(cmp_cond(
                ValueExpr::ManaSpentFromFilter(Box::new(flt)),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(mcp);
        if let Some(mc) = ts.eat_mana_cost()
            && ts.eat_phrase(&["wasn't", "spent", "to", "cast"])
            && eat_was_cast_target(ts)
        {
            return Ok(not_cond(ConditionExpr::EventCheck(
                EventCheckKind::ManaCostSpent(mc),
            )));
        }
        ts.restore(mcp);
        if ts.eat_phrase(&["mana", "from"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_any_phrase(&[
                &["was", "spent", "to", "activate", "this", "ability"],
                &["was", "spent", "to", "cast", "it", "or", "activate", "it"],
            ])
        {
            return Ok(ConditionExpr::FilterCheck {
                filter: Box::new(flt),
                kind: FilterConditionKind::ManaSpentFrom,
                value: 1,
            });
        }
        ts.restore(mcp);
    }

    // Energy paid this way
    {
        let ecp = ts.checkpoint();
        let _ = ts.eat_phrase(&["you", "paid"]);
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            if ts.eat_any_phrase(&[&["this", "way"], &["was", "paid", "this", "way"]]) {
                return Ok(cmp_cond(
                    ValueExpr::PriorActionCount(PriorActionKind::EnergyPaidThisWay),
                    CompareOp::Ge,
                    ValueExpr::Const(n),
                ));
            }
        }
        ts.restore(ecp);
    }

    // Possessive stat comparisons
    {
        let scp = ts.checkpoint();
        const POSS_ANAPHORA: &[(&[&str], AnaphoraRef)] = &[
            (&["its"], AnaphoraRef::It),
            (&["his"], AnaphoraRef::It),
            (&["her"], AnaphoraRef::It),
            (&["the", "card's"], AnaphoraRef::ThatCard),
            (&["the", "discovered", "card's"], AnaphoraRef::ThatCard),
            (&["the", "revealed", "card's"], AnaphoraRef::ThatCard),
            (
                &["the", "sacrificed", "creature's"],
                AnaphoraRef::SacrificedObject,
            ),
            (
                &["the", "sacrificed", "permanent's"],
                AnaphoraRef::SacrificedObject,
            ),
            (&["that", "creature's"], AnaphoraRef::ThatCreature),
            (&["that", "card's"], AnaphoraRef::ThatCard),
            (&["that", "spell's"], AnaphoraRef::ThatSpell),
        ];
        if let Some(anaph) = eat_table(ts, POSS_ANAPHORA)
            && let Some(stat) = eat_stat_kind(ts)
            && ts.eat_any_word(&["is", "was"])
        {
            if ts.eat_phrase(&["greater", "than", "each", "other", "creature's", "power"]) {
                return Ok(ConditionExpr::Flag(
                    ConditionFlag::NoOtherCreatureHasGreaterPower,
                ));
            }
            let prefix_op = eat_prefix_cmp_op(ts, true, true);
            if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                let op = prefix_op.unwrap_or_else(|| eat_suffix_cmp_op(ts, true, true));
                return Ok(cmp_cond(
                    obj_stat_val(SubjectRef::Anaphora(anaph), stat),
                    op,
                    rhs,
                ));
            }
        }
        ts.restore(scp);
    }

    // Creature / permanent death & damage-dealt turn checks
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["no", "creatures", "died", "this", "turn"]) {
            return Ok(not_cond(ConditionExpr::Flag(
                ConditionFlag::PermanentDiedThisTurn,
            )));
        }
        if ts.eat_phrase(&["another", "creature", "died", "this", "turn"]) {
            return Ok(ConditionExpr::Flag(ConditionFlag::PermanentDiedThisTurn));
        }
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&[
                "or",
                "more",
                "creatures",
                "died",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::CreatureDied,
                filter: None,
                min_count: to_min_u16(n),
            });
        }
        ts.restore(dcp);
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&[
                "died",
                "under",
                "an",
                "opponent's",
                "control",
                "this",
                "turn",
            ])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::AnyOpponent,
                event: TurnEventKind::CreatureDied,
                filter: Some(Box::new(flt)),
                min_count: 1,
            });
        }
        ts.restore(dcp);
    }

    // Player / subject dealt damage this turn
    {
        let dmg_cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_any_phrase(&[&["was", "dealt"], &["were", "dealt"]])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "damage", "this", "turn"])
        {
            return Ok(cmp_cond(
                ValueExpr::DamageDealtToPlayerThisTurn(player),
                CompareOp::Ge,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(dmg_cp);
        let is_tilde = ts.eat_tilde();
        if (is_tilde || ts.eat_word("it")) && ts.eat_phrase(&["was", "dealt"]) {
            let combat_only = ts.eat_word("combat");
            if ts.eat_phrase(&["damage", "this", "turn"]) {
                return Ok(ConditionExpr::SubjectCheck {
                    subject: Box::new(if is_tilde {
                        SubjectRef::SelfObject
                    } else {
                        SubjectRef::Anaphora(AnaphoraRef::It)
                    }),
                    kind: SubjectConditionKind::DealtDamageThisTurn { combat_only },
                });
            }
        }
        ts.restore(dmg_cp);
    }

    // `you played a land this turn` / `you've cast both a <flt1> and a <flt2> this turn`
    if ts.eat_any_phrase(&[
        &["you", "played", "a", "land", "this", "turn"],
        &["you've", "played", "a", "land", "this", "turn"],
    ]) {
        return Ok(player_check(
            PlayerRef::You,
            PlayerConditionKind::PlayedLandThisTurn,
        ));
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["you've", "cast", "both"], &["you", "cast", "both"]])
            && let Ok(f1) = try_parse(ts, parse_object_filter)
            && ts.eat_word("and")
            && let Ok(f2) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::And(
                Box::new(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter: Some(Box::new(f1)),
                    min_count: 1,
                }),
                Box::new(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter: Some(Box::new(f2)),
                    min_count: 1,
                }),
            ));
        }
        ts.restore(ccp);
    }

    // Hand size / hand contents / comparative control & life
    if ts.eat_phrase(&["you", "have", "a", "card", "in", "hand"]) {
        return Ok(cmp_cond(
            player_stat_val(PlayerRef::You, PlayerStatKind::CardsInHand),
            CompareOp::Ge,
            ValueExpr::Const(1),
        ));
    }
    {
        let hcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "have", "no"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["in", "hand"])
        {
            return Ok(not_cond(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone: ZoneRef {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::You,
                },
                min_count: 1,
            }));
        }
        ts.restore(hcp);
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            let p_after = ts.checkpoint();
            if ts.eat_any_phrase(&[&["has", "more", "than"], &["have", "more", "than"]])
                && let Some(n) = ts.next_number(false)
                && ts.eat_any_phrase(&[&["cards", "in", "hand"], &["card", "in", "hand"]])
            {
                return Ok(cmp_cond(
                    player_stat_val(player, PlayerStatKind::CardsInHand),
                    CompareOp::Gt,
                    ValueExpr::Const(n),
                ));
            }
            ts.restore(p_after);
            if ts.eat_any_phrase(&[
                &[
                    "has", "more", "cards", "in", "hand", "than", "each", "other", "player",
                ],
                &[
                    "have", "more", "cards", "in", "hand", "than", "each", "other", "player",
                ],
            ]) {
                return Ok(cmp_cond(
                    player_stat_val(player, PlayerStatKind::CardsInHand),
                    CompareOp::Gt,
                    player_stat_val(PlayerRef::EachOtherPlayer, PlayerStatKind::CardsInHand),
                ));
            }
            ts.restore(p_after);
            if ts.eat_any_phrase(&[&["control", "fewer"], &["controls", "fewer"]])
                && let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_word("than")
                && let Ok(other) = try_parse(ts, parse_player_ref)
            {
                return Ok(ConditionExpr::ControlsMoreThan {
                    player: other,
                    other: player,
                    filter: Box::new(filter),
                });
            }
            ts.restore(p_after);
            if ts.eat_any_phrase(&[
                &["has", "less", "life", "than"],
                &["have", "less", "life", "than"],
            ]) && let Ok(other) = try_parse(ts, parse_player_ref)
            {
                return Ok(cmp_cond(
                    player_stat_val(player, PlayerStatKind::LifeTotal),
                    CompareOp::Lt,
                    player_stat_val(other, PlayerStatKind::LifeTotal),
                ));
            }
            ts.restore(p_after);
            if ts.eat_phrase(&[
                "has",
                "more",
                "life",
                "than",
                "another",
                "of",
                "your",
                "opponents",
            ]) {
                return Ok(cmp_cond(
                    player_stat_val(player, PlayerStatKind::LifeTotal),
                    CompareOp::Gt,
                    player_stat_val(PlayerRef::EachOtherOpponent, PlayerStatKind::LifeTotal),
                ));
            }
        }
        ts.restore(hcp);
    }

    // Cast / keyword cost / not-cast-from-hand
    {
        let ccp = ts.checkpoint();
        if (ts.eat_tilde() || ts.eat_word("it"))
            && ts.eat_phrase(&["wasn't", "cast", "from", "your", "hand"])
        {
            return Ok(not_cond(ConditionExpr::Flag(
                ConditionFlag::WasCastFromHand,
            )));
        }
        ts.restore(ccp);
    }
    {
        let kcp = ts.checkpoint();
        if ts.eat_any_word(&["his", "her"])
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
            && ts.eat_phrase(&["cost", "was", "paid"])
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(kcp);
        if ts.eat_phrase(&["they", "were", "cast", "using"])
            && let Ok(kw) = try_parse(ts, parse_keyword_kind)
        {
            let _ = ts.eat_word("cost");
            return Ok(ConditionExpr::EventCheck(EventCheckKind::KeywordCostPaid(
                kw,
            )));
        }
        ts.restore(kcp);
    }

    // `any of those cards remain(s) exiled` / `any of those cards shares a <prop> with <subj>`
    if ts.eat_any_phrase(&[
        &["any", "of", "those", "cards", "remain", "exiled"],
        &["any", "of", "them", "remain", "exiled"],
    ]) {
        return Ok(ConditionExpr::SubjectCheck {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
            kind: SubjectConditionKind::RemainsExiled,
        });
    }
    {
        let acp = ts.checkpoint();
        const SHARE_PROPS: &[(&[&str], SharedProperty)] = &[
            (&["card", "type"], SharedProperty::CardType),
            (&["creature", "type"], SharedProperty::CreatureType),
            (&["color"], SharedProperty::Color),
        ];
        if ts.eat_phrase(&["any", "of", "those", "cards", "shares", "a"])
            && let Some(kind) = eat_table(ts, SHARE_PROPS)
            && ts.eat_word("with")
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::SharesTypeWith {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThoseCards)),
                other: Box::new(other),
                kind,
            });
        }
        ts.restore(acp);
    }

    // `two [nonland] cards that share (a|all their) <prop> (were|are) (milled|discarded) this way`
    {
        let tcp = ts.checkpoint();
        if ts.eat_word("two") {
            let nonland = ts.eat_word("nonland");
            if ts.eat_phrase(&["cards", "that", "share"]) {
                let _ = ts.eat_any_phrase(&[&["a"], &["all", "their"]]);
                const TWO_SHARE_PROPS: &[(&[&str], SharedProperty)] = &[
                    (&["card", "type"], SharedProperty::CardType),
                    (&["card", "types"], SharedProperty::CardType),
                    (&["color"], SharedProperty::Color),
                    (&["colors"], SharedProperty::Color),
                ];
                const MILL_OR_DISCARD: &[(&[&str], PriorActionKind)] = &[
                    (&["milled", "this", "way"], PriorActionKind::MilledThisWay),
                    (
                        &["discarded", "this", "way"],
                        PriorActionKind::DiscardedThisWay,
                    ),
                ];
                if let Some(property) = eat_table(ts, TWO_SHARE_PROPS)
                    && ts.eat_any_word(&["were", "are"])
                    && let Some(action) = eat_table(ts, MILL_OR_DISCARD)
                {
                    let mut flt = ObjectFilter {
                        any_card: true,
                        predicates: vec![FilterPredicate::SharesPropertyWith {
                            property,
                            reference: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThoseCards)),
                            negated: false,
                        }],
                        ..Default::default()
                    };
                    if nonland {
                        flt.forbidden_types = CardTypeMask::LAND;
                    }
                    return Ok(ConditionExpr::PriorActionMatched {
                        action,
                        filter: Box::new(flt),
                    });
                }
            }
        }
        ts.restore(tcp);
    }

    // `this is the (first or second | fourth..sixth) time this ability has resolved this turn`
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["this", "is", "the"]) {
            if ts.eat_phrase(&[
                "first", "or", "second", "time", "this", "ability", "has", "resolved", "this",
                "turn",
            ]) {
                return Ok(ConditionExpr::Or(
                    Box::new(ConditionExpr::EventCheck(
                        EventCheckKind::ResolutionCountThisTurn(1),
                    )),
                    Box::new(ConditionExpr::EventCheck(
                        EventCheckKind::ResolutionCountThisTurn(2),
                    )),
                ));
            }
            const EXTRA_ORDS: &[(&[&str], u8)] =
                &[(&["fourth"], 4), (&["fifth"], 5), (&["sixth"], 6)];
            if let Some(n) = eat_table(ts, EXTRA_ORDS)
                && ts.eat_phrase(&["time", "this", "ability", "has", "resolved", "this", "turn"])
            {
                return Ok(ConditionExpr::EventCheck(
                    EventCheckKind::ResolutionCountThisTurn(n),
                ));
            }
        }
        ts.restore(rcp);
    }

    // `there's another <flt> on the battlefield`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["there's", "another"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["on", "the", "battlefield"])
                || flt.zones.iter().any(|z| z.zone == ZoneKind::Battlefield))
        {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone: ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                },
                min_count: 1,
            });
        }
        ts.restore(tcp);
    }

    // `no <flt> were revealed/removed/discarded this way` / `fewer than <N> <flt> were discarded this way`
    {
        let ncp = ts.checkpoint();
        if ts.eat_word("no") {
            if ts.eat_phrase(&["counters", "were", "removed", "this", "way"]) {
                return Ok(not_cond(prior_event(
                    PriorActionKind::CountersRemovedThisWay,
                )));
            }
            const NO_PRIOR_ACTS: &[(&[&str], PriorActionKind)] = &[
                (
                    &["revealed", "this", "way"],
                    PriorActionKind::RevealedThisWay,
                ),
                (
                    &["discarded", "this", "way"],
                    PriorActionKind::DiscardedThisWay,
                ),
                (&["exiled", "this", "way"], PriorActionKind::ExiledThisWay),
                (
                    &["destroyed", "this", "way"],
                    PriorActionKind::DestroyedThisWay,
                ),
            ];
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_any_word(&["were", "was"])
                && let Some(action) = eat_table(ts, NO_PRIOR_ACTS)
            {
                return Ok(not_cond(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(flt),
                }));
            }
        }
        ts.restore(ncp);
        if ts.eat_phrase(&["fewer", "than"])
            && let Some(n) = ts.next_number(false)
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["were", "discarded", "this", "way"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            return Ok(cmp_cond(
                ValueExpr::Count(Box::new(flt)),
                CompareOp::Lt,
                ValueExpr::Const(n),
            ));
        }
        ts.restore(ncp);
    }

    // `<player> [didn't] <verb> [no] <flt> this way`
    {
        let pcp = ts.checkpoint();
        const PLAYER_PRIOR_VERBS: &[(&[&str], PriorActionKind)] = &[
            (&["discard"], PriorActionKind::DiscardedThisWay),
            (&["discards"], PriorActionKind::DiscardedThisWay),
            (&["discarded"], PriorActionKind::DiscardedThisWay),
            (&["return"], PriorActionKind::ReturnedThisWay),
            (&["returns"], PriorActionKind::ReturnedThisWay),
            (&["returned"], PriorActionKind::ReturnedThisWay),
            (&["reveal"], PriorActionKind::RevealedThisWay),
            (&["reveals"], PriorActionKind::RevealedThisWay),
            (&["revealed"], PriorActionKind::RevealedThisWay),
            (&["sacrifice"], PriorActionKind::SacrificedThisWay),
            (&["sacrifices"], PriorActionKind::SacrificedThisWay),
            (&["sacrificed"], PriorActionKind::SacrificedThisWay),
            (&["mill"], PriorActionKind::MilledThisWay),
            (&["mills"], PriorActionKind::MilledThisWay),
            (&["milled"], PriorActionKind::MilledThisWay),
        ];
        if try_parse(ts, parse_player_ref).is_ok() {
            let neg = ts.eat_any_word(&["didn't", "don't", "doesn't"]);
            if let Some(action) = eat_table(ts, PLAYER_PRIOR_VERBS) {
                let is_no = ts.eat_word("no");
                if let Ok(flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_phrase(&["this", "way"])
                {
                    let base = ConditionExpr::PriorActionMatched {
                        action,
                        filter: Box::new(flt),
                    };
                    return Ok(if neg || is_no { not_cond(base) } else { base });
                }
            }
        }
        ts.restore(pcp);
    }

    // `<subj | flt> dies / destroyed / returned / sacrificed this way` / `<subj> is on the stack / has a single target`
    {
        let wcp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["dies", "this", "way"])
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::DestroyedThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(wcp);
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            const SUBJ_PRIOR_EVENTS: &[(&[&str], PriorActionKind)] = &[
                (
                    &["is", "destroyed", "this", "way"],
                    PriorActionKind::DestroyedThisWay,
                ),
                (
                    &["was", "destroyed", "this", "way"],
                    PriorActionKind::DestroyedThisWay,
                ),
                (
                    &[
                        "is", "returned", "to", "its", "owner's", "hand", "this", "way",
                    ],
                    PriorActionKind::ReturnedThisWay,
                ),
                (
                    &["is", "returned", "to", "your", "hand", "this", "way"],
                    PriorActionKind::ReturnedThisWay,
                ),
                (&["was", "sacrificed"], PriorActionKind::SacrificedThisWay),
            ];
            if let Some(kind) = eat_table(ts, SUBJ_PRIOR_EVENTS) {
                return Ok(prior_event(kind));
            }
            if ts.eat_phrase(&["is", "on", "the", "stack"]) {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone: ZoneRef {
                        zone: ZoneKind::Stack,
                        owner: ZoneOwner::Default,
                    },
                });
            }
            if ts.eat_phrase(&["has", "a", "single", "target"]) {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(ObjectFilter {
                        flags: FilterFlags::SINGLE_TARGET,
                        ..Default::default()
                    }),
                });
            }
        }
        ts.restore(wcp);
    }

    // `creatures you control have total toughness/power <N> or greater`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["creatures", "you", "control", "have", "total"]) {
            let stat = if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else if ts.eat_word("power") {
                Some(StatKind::Power)
            } else {
                None
            };
            if let Some(stat) = stat
                && let Some(n) = ts.next_number(false)
                && ts.eat_any_phrase(&[&["or", "greater"], &["or", "more"]])
            {
                let flt = ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    controller: Some(ControllerRelation::You),
                    ..Default::default()
                };
                return Ok(cmp_cond(
                    ValueExpr::AggregateStat {
                        op: AggregateOp::Sum,
                        stat,
                        filter: Box::new(flt),
                    },
                    CompareOp::Ge,
                    ValueExpr::Const(n),
                ));
            }
        }
        ts.restore(tcp);
    }

    super::extended_extra::parse_extended_extra(ts)
}
