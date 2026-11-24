//! Compositional Wave E7/E8 lossless clause parsers built from atomic Magic
//! rules primitives so both existing cards and unseen future cards sharing the
//! same underlying mechanics parse into strongly-typed AST nodes automatically.
//!
//! Zero `StructuredRule` or untyped `GameStateFlag` catch-alls are emitted here.

use crate::ast::{
    CardTypeMask, CategoryIterationKind, CombatState, CompareOp, ConditionAst, ControllerRelation,
    CostAtomAst, DelayedStepScope, DurationAst, FilterPredicateAst, GameMarker, InstructionAst,
    ObjectFilterAst, ObjectFilterFlags, PlayerRefAst, PlayerStatKind, PriorActionRefKind,
    ReplacementEffectAst, StatKind, StaticAbilityAst, SubjectRefAst, TargetCountAst,
    TriggerEventAst, TriggerSpecAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::{
    StaticOrReplacement, parse_atomic_effect, parse_cost_atom, parse_effect_body,
};
use crate::filter::{
    parse_condition, parse_object_filter, parse_player_ref, parse_subject_or_target,
    parse_value_expr,
};
use crate::vocab::{Vocabulary, parse_number_word};

fn obj_type(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        ..Default::default()
    }
}

fn any_perm() -> ObjectFilterAst {
    ObjectFilterAst {
        any_permanent: true,
        ..Default::default()
    }
}

fn any_card_flt() -> ObjectFilterAst {
    ObjectFilterAst {
        any_card: true,
        ..Default::default()
    }
}

/// Compositional Wave E7/E8 effect parser invoked before legacy `E5_STRUCTURED_RULES`.
#[inline(never)]
pub(crate) fn parse_wave_e7_effects(
    s: &str,
    low: &str,
    _s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. All-targets-legal resolution wrapper ("If both/all targets are still legal as this spell/ability resolves, <eff>")
    if let Some(body) = parse_all_targets_legal_wrapper(s, low, vocab) {
        return Some(body);
    }
    // 2. Simultaneous execution combinator ("[<player>] simultaneously[,] <A> and <B>")
    if let Some(body) = parse_simultaneous_effect(low, vocab) {
        return Some(body);
    }
    // 3. Category iteration combinator ("For each color pair, <eff>" / "For each kind of counter among/on <flt>, <eff>")
    if let Some(body) = parse_category_iteration_effect(s, low, vocab) {
        return Some(body);
    }
    // 4. Extreme-stat winner and tie-breaker grammar (Timesifter, Psychic Battle, Sokenzan Renegade)
    if let Some(body) = parse_extreme_stat_winner_and_tie(s, low, vocab) {
        return Some(body);
    }
    // 5. Comparative opponent targeting ("<player> chooses target player who(se) ... and is their opponent")
    if let Some(body) = parse_comparative_opponent_choice(low, vocab) {
        return Some(body);
    }
    // 6. Legal-target check ("If <player> control(s) one or more <flt> that spell could target, choose one of those <noun>")
    if let Some(body) = parse_legal_target_check(low, vocab) {
        return Some(body);
    }
    // 7. Forced mana-ability activation & unspent-mana drain (Drain Power, Pygmy Hippo)
    if let Some(body) = parse_forced_mana_abilities(low, vocab) {
        return Some(body);
    }
    // 8. Delayed triggers, bounded repeats, next-cast copies, and temporary replacements
    if let Some(body) = parse_delayed_and_trigger_grammar(s, low, vocab) {
        return Some(body);
    }
    // 9. Capped life gain, coin-flip loops, ordered sacrifices, destroy-except, and orthogonal rules
    parse_compositional_combat_and_resource_grammar(s, low, dur, vocab)
}

/// Compositional Wave E7/E8 static and replacement parser.
#[inline(never)]
pub(crate) fn parse_wave_e7_statics(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    let low = low.trim_end_matches('.');

    // 1. Chained replacement effect with success/failure follow-up sentences (Chains of Mephistopheles)
    if let Some(sr) = parse_chained_draw_replacement(low, vocab) {
        return Some(sr);
    }
    // 2. As-enters exile two matching cards, copy one with additional counters equal to the other's stat (The Mimeoplasm)
    if let Some(sr) = parse_as_enters_exile_copy_counters(low, vocab) {
        return Some(sr);
    }
    // 3. As-enters pay any amount of life capped by a dynamic ValueExprAst (Nameless Race)
    if let Some(sr) = parse_as_enters_capped_life(low, vocab) {
        return Some(sr);
    }
    // 4. Conditional -X/-Y static with min-capped Y (Snowblind)
    if let Some(sr) = parse_conditional_min_modify_pt(low, vocab) {
        return Some(sr);
    }
    // 5. Untap-step single-type choice restriction (Storage Matrix)
    if let Some(sr) = parse_untap_step_type_choice(low) {
        return Some(sr);
    }
    let _ = s;
    None
}

fn parse_all_targets_legal_wrapper(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    for pref in [
        "if both targets are still legal as this ability resolves, ",
        "if both targets are still legal as this spell resolves, ",
        "if all targets are still legal as this ability resolves, ",
        "if all targets are still legal as this spell resolves, ",
        "if both targets are still legal, ",
        "if all targets are still legal, ",
    ] {
        if let Some(rest_low) = low.strip_prefix(pref) {
            let body_slice = &s[s.len() - rest_low.len()..];
            let (body, fails) = parse_effect_body(body_slice, vocab);
            if fails.is_empty() && !body.is_empty() {
                return Some(vec![InstructionAst::IfCondition {
                    condition: Box::new(ConditionAst::Marker(GameMarker::AllTargetsLegal)),
                    then_body: body,
                }]);
            }
        }
    }
    None
}

pub(crate) fn parse_simultaneous_effect(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let (actor_prefix, rest) = if let Some(after) = low
        .strip_prefix("simultaneously, ")
        .or_else(|| low.strip_prefix("simultaneously "))
    {
        (None, after)
    } else if let Some((who_raw, after)) = low.split_once(" simultaneously ")
        && parse_player_ref(who_raw.trim()).is_some()
    {
        (Some(who_raw.trim()), after)
    } else {
        return None;
    };

    // Split on top-level " and " where both halves compile as atomic effects (inheriting actor_prefix when present).
    for (idx, _) in rest.match_indices(" and ") {
        let left_raw = rest[..idx].trim();
        let right_raw = rest[idx + 5..].trim();
        if left_raw.is_empty() || right_raw.is_empty() || left_raw.ends_with(',') {
            continue;
        }
        let left_full = match actor_prefix {
            Some(who) => format!("{who} {left_raw}"),
            None => left_raw.to_string(),
        };
        let right_full = match actor_prefix {
            Some(who) => format!("{who} {right_raw}"),
            None => right_raw.to_string(),
        };
        if let (Ok(mut l), Ok(mut r)) = (
            parse_atomic_effect(&left_full, vocab),
            parse_atomic_effect(&right_full, vocab),
        ) {
            l.append(&mut r);
            return Some(vec![InstructionAst::Simultaneous(l)]);
        }
        // Also support actor_prefix only on left (when right already specifies its own subject) or bare actions
        if let (Ok(mut l), Ok(mut r)) = (
            parse_atomic_effect(left_raw, vocab),
            parse_atomic_effect(right_raw, vocab),
        ) {
            l.append(&mut r);
            return Some(vec![InstructionAst::Simultaneous(l)]);
        }
    }
    None
}

fn parse_category_iteration_effect(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let clean_low = low.strip_prefix("then ").unwrap_or(low);
    let clean_s = &s[s.len() - clean_low.len()..];

    // "For each color pair, <effect>" (Niv-Mizzet Reborn and any future guild/color-pair card)
    if let Some(after) = clean_low.strip_prefix("for each color pair, ") {
        let body_s = &clean_s[clean_s.len() - after.len()..];
        if let Some(body) = parse_category_body(after.trim(), body_s.trim(), vocab) {
            return Some(vec![InstructionAst::ForEachCategory {
                category: CategoryIterationKind::ColorPair,
                body,
            }]);
        }
    }

    // "For each kind of counter among/on <filter>, <effect>" (Exotic Pets, Bribe Taker, and future counter-kind cards)
    if let Some(after) = clean_low
        .strip_prefix("for each kind of counter among ")
        .or_else(|| clean_low.strip_prefix("for each kind of counter on "))
        && let Some((flt_raw, body_low)) = after.split_once(", ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let body_s = &clean_s[clean_s.len() - body_low.len()..];
        if let Some(body) = parse_category_body(body_low.trim(), body_s.trim(), vocab) {
            return Some(vec![InstructionAst::ForEachCategory {
                category: CategoryIterationKind::CounterKindAmong(Box::new(flt)),
                body,
            }]);
        }
    }

    None
}

fn parse_category_body(
    body_low: &str,
    body_s: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // "choose a <filter> that's exactly those colors from among them"
    if let Some(after_ch) = body_low.strip_prefix("choose ")
        && let Some(flt_raw) = after_ch
            .strip_suffix(" that's exactly those colors from among them")
            .or_else(|| after_ch.strip_suffix(" that is exactly those colors from among them"))
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.predicates
            .push(FilterPredicateAst::MatchesChosen { negated: false });
        return Some(vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::Group(Box::new(flt))),
        }]);
    }
    // "put a counter of that kind on <subj>"
    if let Some(subj_raw) = body_low.strip_prefix("put a counter of that kind on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: "that kind".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    // "you may put your choice of a <kind> counter or a counter of that kind on <subj>" (Bribe Taker)
    if let Some(after) = body_low.strip_prefix("you may put your choice of ")
        && let Some((left_cnt, subj_raw)) = after.split_once(" or a counter of that kind on ")
        && let Some(kind_raw) = left_cnt
            .trim()
            .strip_prefix("a ")
            .or_else(|| left_cnt.trim().strip_prefix("an "))
            .and_then(|r| r.strip_suffix(" counter"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::Choice(vec![
                vec![InstructionAst::PutCounters {
                    subject: Box::new(subj.clone()),
                    counter_kind: kind_raw.trim().to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                }],
                vec![InstructionAst::PutCounters {
                    subject: Box::new(subj),
                    counter_kind: "that kind".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                }],
            ]),
        ])]);
    }
    let (body, fails) = parse_effect_body(body_s, vocab);
    if fails.is_empty() && !body.is_empty() {
        return Some(body);
    }
    None
}

fn parse_extreme_stat_winner_and_tie(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // "The player who <exiled|reveals|revealed|discarded> the card with the <greatest|least|lowest> <stat> <effect>"
    if let Some(after_who) = low.strip_prefix("the player who ") {
        for (verb, action) in [
            (
                "exiled the card with the ",
                PriorActionRefKind::ExiledThisWay,
            ),
            (
                "reveals the card with the ",
                PriorActionRefKind::RevealedThisWay,
            ),
            (
                "revealed the card with the ",
                PriorActionRefKind::RevealedThisWay,
            ),
            (
                "discarded the card with the ",
                PriorActionRefKind::DiscardedThisWay,
            ),
        ] {
            if let Some(after_v) = after_who.strip_prefix(verb) {
                let (greatest, after_ext) = if let Some(r) = after_v
                    .strip_prefix("greatest ")
                    .or_else(|| after_v.strip_prefix("highest "))
                {
                    (true, r)
                } else if let Some(r) = after_v
                    .strip_prefix("least ")
                    .or_else(|| after_v.strip_prefix("lowest "))
                {
                    (false, r)
                } else {
                    continue;
                };
                for (stat_tok, stat) in [
                    ("mana value ", StatKind::ManaValue),
                    ("power ", StatKind::Power),
                    ("toughness ", StatKind::Toughness),
                ] {
                    if let Some(rest_eff) = after_ext.strip_prefix(stat_tok)
                        && let Some(body) = parse_winner_action(rest_eff.trim(), vocab)
                    {
                        return Some(vec![InstructionAst::ForEachSubjectPlayer {
                            players: Box::new(SubjectRefAst::PlayerMatching {
                                player: PlayerRefAst::AnyPlayer,
                                condition: Box::new(ConditionAst::Marker(
                                    GameMarker::PlayerActionExtremeStat {
                                        player: PlayerRefAst::ThatPlayer,
                                        action,
                                        stat,
                                        greatest,
                                        allow_tie: false,
                                    },
                                )),
                            }),
                            body,
                        }]);
                    }
                }
            }
        }

        // "The player who has the <most|fewest> <cards in hand|life> <effect>" (Sokenzan Renegade)
        for (phr, stat, greatest) in [
            (
                "has the most cards in hand ",
                PlayerStatKind::CardsInHand,
                true,
            ),
            (
                "has the fewest cards in hand ",
                PlayerStatKind::CardsInHand,
                false,
            ),
            ("has the most life ", PlayerStatKind::LifeTotal, true),
            ("has the least life ", PlayerStatKind::LifeTotal, false),
        ] {
            if let Some(rest_eff) = after_who.strip_prefix(phr)
                && let Some(body) = parse_winner_action(rest_eff.trim(), vocab)
            {
                return Some(vec![InstructionAst::ForEachSubjectPlayer {
                    players: Box::new(SubjectRefAst::PlayerMatching {
                        player: PlayerRefAst::AnyPlayer,
                        condition: Box::new(ConditionAst::Marker(
                            GameMarker::PlayerHasExtremePlayerStat {
                                player: PlayerRefAst::ThatPlayer,
                                stat,
                                greatest,
                            },
                        )),
                    }),
                    body,
                }]);
            }
        }
    }

    // "If two or more <players' cards|cards|players> are tied for <greatest|least|fewest>, <effect>"
    if let Some(after_if) = low.strip_prefix("if two or more ")
        && let Some((subj_and_metric, eff_low)) = after_if.split_once(", ")
        && let Some((tied_kind, metric_word)) = subj_and_metric.split_once(" are tied for ")
        && matches!(tied_kind.trim(), "players' cards" | "cards" | "players")
        && matches!(
            metric_word.trim(),
            "greatest" | "least" | "fewest" | "highest" | "lowest"
        )
    {
        let eff_s = &s[s.len() - eff_low.len()..];
        let then_body = if let Some(until_raw) = eff_low
            .strip_prefix("the tied players repeat this process until ")
            .or_else(|| eff_low.strip_prefix("repeat this process until "))
        {
            vec![InstructionAst::RepeatProcessExt {
                times: None,
                for_list: Vec::new(),
                until: Some(until_raw.trim().to_string()),
            }]
        } else if eff_low == "the target or targets remain unchanged"
            || eff_low == "the targets remain unchanged"
        {
            vec![InstructionAst::AbilityRestriction {
                restriction: "targets_remain_unchanged".to_string(),
                condition: None,
            }]
        } else {
            let (b, fails) = parse_effect_body(eff_s.trim(), vocab);
            if fails.is_empty() && !b.is_empty() {
                b
            } else {
                return None;
            }
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::PlayersTiedFor {
                metric: format!("{}:{}", tied_kind.trim(), metric_word.trim()),
                min_tied: 2,
            }),
            then_body,
        }]);
    }

    // "Changing targets this way doesn't trigger abilities of permanents named ~"
    if low == "changing targets this way doesn't trigger abilities of permanents named ~" {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: "doesnt_trigger_abilities_of_permanents_named_self".to_string(),
            condition: None,
        }]);
    }

    None
}

fn parse_winner_action(eff_low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    if eff_low == "may change the target or targets" || eff_low == "may change the targets" {
        return Some(vec![InstructionAst::OptionalPlayer {
            player: PlayerRefAst::ThatPlayer,
            body: vec![InstructionAst::ChangeTargets {
                target: Box::new(SubjectRefAst::Anaphora("the target or targets".to_string())),
                new_target: None,
                must_target: None,
                single_target_only: false,
            }],
        }]);
    }
    let with_player = format!("that player {eff_low}");
    if let Ok(ins) = parse_atomic_effect(&with_player, vocab) {
        return Some(ins);
    }
    parse_atomic_effect(eff_low, vocab).ok()
}

fn parse_comparative_opponent_choice(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // "<player> chooses target player who(se) <comparison> and is their opponent"
    // Covers Oath of Ghouls, Oath of Mages, Oath of Scholars, and future Oath-style cards.
    let (who_raw, rest) = low
        .split_once(" chooses target player ")
        .or_else(|| low.split_once(" choose target player "))?;
    parse_player_ref(who_raw.trim())?;
    let cmp_clause = rest.strip_suffix(" and is their opponent")?;

    let cond = if let Some(after_gy) = cmp_clause.strip_prefix("whose graveyard has ") {
        let (op, after_op) = if let Some(r) = after_gy.strip_prefix("fewer ") {
            (CompareOp::Lt, r)
        } else {
            let r = after_gy.strip_prefix("more ")?;
            (CompareOp::Gt, r)
        };
        let flt_raw = after_op.strip_suffix(" in it than their graveyard does")?;
        let mut opp_gy = parse_object_filter(flt_raw.trim(), vocab).ok()?;
        let mut own_gy = opp_gy.clone();
        opp_gy.zone = Some(ZoneFilter::TargetOpponentGraveyard);
        own_gy.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::Count(Box::new(opp_gy))),
            op,
            rhs: Box::new(ValueExprAst::Count(Box::new(own_gy))),
        }
    } else {
        let after_who = cmp_clause.strip_prefix("who has ")?;
        let (op, after_op) = if let Some(r) = after_who.strip_prefix("fewer ") {
            (CompareOp::Lt, r)
        } else if let Some(r) = after_who.strip_prefix("less ") {
            (CompareOp::Lt, r)
        } else {
            let r = after_who.strip_prefix("more ")?;
            (CompareOp::Gt, r)
        };
        let stat = match after_op.strip_suffix(" than they do")? {
            "life" => PlayerStatKind::LifeTotal,
            "cards in hand" => PlayerStatKind::CardsInHand,
            "cards in their graveyard" => PlayerStatKind::CardsInGraveyard,
            "cards in their library" => PlayerStatKind::CardsInLibrary,
            _ => return None,
        };
        ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::TargetOpponent,
                stat,
            }),
            op,
            rhs: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::ThatPlayer,
                stat,
            }),
        }
    };

    Some(vec![InstructionAst::ChooseFromGroup {
        count: Box::new(ValueExprAst::Const(1)),
        random: false,
        group: Box::new(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::TargetOpponent,
            condition: Box::new(cond),
        }),
    }])
}

fn parse_legal_target_check(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // "if <player> control(s) one or more <filter> that spell could target, choose one of those <noun>"
    // Covers Exterminator Magmarch, Beamsplitter Mage, and future spell-copy target checks.
    let after_if = low.strip_prefix("if ")?;
    let (cond_part, choose_part) = after_if.split_once(", choose one of ")?;
    let (who_raw, rest_ctrl) = cond_part
        .split_once(" controls one or more ")
        .or_else(|| cond_part.split_once(" control one or more "))?;
    let player = if who_raw.trim() == "another opponent" {
        PlayerRefAst::EachOtherOpponent
    } else {
        parse_player_ref(who_raw.trim())?
    };
    let flt_raw = rest_ctrl.strip_suffix(" that spell could target")?;
    let mut flt = parse_object_filter(flt_raw.trim(), vocab).ok()?;
    flt.predicates
        .push(FilterPredicateAst::CouldBeTargetedBy(Box::new(
            SubjectRefAst::Anaphora("that spell".to_string()),
        )));
    Some(vec![InstructionAst::IfCondition {
        condition: Box::new(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        }),
        then_body: vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::Anaphora(choose_part.trim().to_string())),
        }],
    }])
}

fn parse_forced_mana_abilities(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // "[you may have ]<player> activate(s) a mana ability of each <filter> they control[ and lose(s) all unspent mana]"
    // Covers Pygmy Hippo, Drain Power, and future forced-mana-activation effects.
    let (optional, core) = match low.strip_prefix("you may have ") {
        Some(r) => (true, r),
        None => (false, low),
    };
    let (core_act, lose_unspent) = match core
        .strip_suffix(" and lose all unspent mana")
        .or_else(|| core.strip_suffix(" and loses all unspent mana"))
    {
        Some(r) => (r, true),
        None => (core, false),
    };
    let (who_raw, flt_raw) = core_act
        .split_once(" activates a mana ability of each ")
        .or_else(|| core_act.split_once(" activate a mana ability of each "))?;
    let player = parse_player_ref(who_raw.trim())?;
    let flt_clean = flt_raw
        .trim()
        .strip_suffix(" they control")
        .unwrap_or(flt_raw.trim());
    let mut flt = parse_object_filter(flt_clean, vocab).ok()?;
    flt.controller = Some(match player {
        PlayerRefAst::DefendingPlayer => ControllerRelation::DefendingPlayer,
        PlayerRefAst::TargetPlayer => ControllerRelation::TargetPlayer,
        PlayerRefAst::TargetOpponent => ControllerRelation::TargetOpponent,
        PlayerRefAst::You => ControllerRelation::You,
        _ => ControllerRelation::ThatPlayer,
    });
    let mut seq = vec![InstructionAst::ActivateManaAbilities {
        player,
        filter: Box::new(flt),
    }];
    if lose_unspent {
        seq.push(InstructionAst::LoseUnspentMana { player });
    }
    Some(if optional {
        vec![InstructionAst::Optional(seq)]
    } else {
        seq
    })
}

fn parse_delayed_and_trigger_grammar(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "when <player> cast(s) their first spell of the game, <effect>" (Chancellor of the Annex and future pregame triggers)
    if let Some(after_w) = low
        .strip_prefix("when ")
        .or_else(|| low.strip_prefix("whenever "))
        && let Some((who_raw, rest)) = after_w
            .split_once(" casts their first spell of the game, ")
            .or_else(|| after_w.split_once(" cast their first spell of the game, "))
        && let Some(caster) = parse_player_ref(who_raw.trim())
    {
        let body_slice = &s[s.len() - rest.len()..];
        let (body, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !body.is_empty() {
            let mut first_spell = ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            };
            first_spell
                .predicates
                .push(FilterPredicateAst::NthSpellCastThisGame(1));
            return Some(vec![InstructionAst::DelayedEventTrigger {
                trigger: TriggerSpecAst {
                    events: vec![TriggerEventAst::SpellCast {
                        caster,
                        spell: Some(Box::new(first_spell)),
                        self_spell: false,
                        targets: None,
                        nth_each_turn: None,
                        include_copy: false,
                    }],
                    intervening_if: None,
                },
                body,
            }]);
        }
    }

    // 2. "when you next cast <event> this turn, for each other opponent, choose <choice>, copy that spell, and the copy targets <chosen>" (Zevlor)
    if let Some(after_wn) = low.strip_prefix("when you next ")
        && let Some((ev_raw, body_rest)) = after_wn.split_once(", for each other opponent, ")
        && let Some((choose_part, copy_tail)) =
            body_rest.split_once(", copy that spell, and the copy targets ")
        && let Some(choice_subj_raw) = choose_part.strip_prefix("choose ")
    {
        let mut opp_perm = any_perm();
        opp_perm.controller = Some(ControllerRelation::ThatPlayer);
        let group = if choice_subj_raw == "that player or a permanent they control" {
            SubjectRefAst::Compound(vec![
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
                SubjectRefAst::Group(Box::new(opp_perm)),
            ])
        } else {
            parse_subject_or_target(choice_subj_raw, vocab).ok()?
        };
        return Some(vec![InstructionAst::WhenYouNext {
            event: ev_raw.trim().to_string(),
            body: vec![InstructionAst::ForEachPlayer {
                player: PlayerRefAst::EachOtherOpponent,
                body: vec![
                    InstructionAst::ChooseFromGroup {
                        count: Box::new(ValueExprAst::Const(1)),
                        random: false,
                        group: Box::new(group),
                    },
                    InstructionAst::Copy {
                        subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                        exception: None,
                        count: None,
                    },
                    InstructionAst::ChangeTargets {
                        target: Box::new(SubjectRefAst::Anaphora("the copy".to_string())),
                        new_target: Some(Box::new(SubjectRefAst::Anaphora(
                            copy_tail.trim().to_string(),
                        ))),
                        must_target: None,
                        single_target_only: false,
                    },
                ],
            }],
            duration: if ev_raw.contains("this turn") {
                DurationAst::UntilEndOfTurn
            } else {
                DurationAst::Permanent
            },
        }]);
    }

    // 3. Bounded repeat loop: "<effect>, then repeat(s) this process until <cond> or <val> cards have been put into their graveyard this way, whichever comes first" (Helm of Obedience)
    if let Some((first_eff, after_rep)) = low
        .split_once(", then repeats this process until ")
        .or_else(|| low.split_once(", then repeat this process until "))
        && let Some(until_body) = after_rep.strip_suffix(", whichever comes first")
        && let Some((stop_flt_raw, cap_tail)) = until_body.split_once(" or ")
        && let Some(cap_raw) =
            cap_tail.strip_suffix(" cards have been put into their graveyard this way")
        && let (Ok(mut first_ins), Ok(cap_val)) = (
            parse_atomic_effect(first_eff.trim(), vocab),
            parse_value_expr(cap_raw.trim(), vocab),
        )
    {
        first_ins.push(InstructionAst::RepeatProcessExt {
            times: Some(Box::new(cap_val)),
            for_list: Vec::new(),
            until: Some(format!(
                "{} has been put into their graveyard this way",
                stop_flt_raw.trim()
            )),
        });
        return Some(first_ins);
    }

    // 4. Delayed additional damage unless cost paid before step (Quenchable Fire)
    if let Some((dmg_part, rest)) =
        low.split_once(" at the beginning of your next upkeep step unless ")
        && let Some((who_raw, pay_tail)) = rest.split_once(" pays ")
        && let Some(cost_raw) = pay_tail.strip_suffix(" before that step")
        && let Some(unless_player) = parse_player_ref(who_raw.trim())
        && let Ok(cost) = parse_cost_atom(&cost_raw.trim().to_ascii_uppercase(), vocab)
        && let Some((src_raw, amt_and_tgt)) = dmg_part
            .split_once(" deals an additional ")
            .or_else(|| dmg_part.split_once(" deals "))
        && let Some((amt_raw, tgt_raw)) = amt_and_tgt.split_once(" damage to ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::Upkeep,
            scope: DelayedStepScope::YourNext,
            body: vec![InstructionAst::Unless {
                body: vec![InstructionAst::DealDamage {
                    source: Box::new(src),
                    target: Box::new(tgt),
                    amount: Box::new(amt),
                }],
                unless_player: Some(unless_player),
                unless_cost: Some(cost),
                unless_action: None,
                unless_condition: None,
            }],
        }]);
    }

    // 5. Turn-scoped trigger on damage dealt: "if <src> deals damage to <rec> this turn, <eff>" (Runesword)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_part, body_raw)) = after_if.split_once(", ")
        && let Some((src_raw, rec_raw)) = cond_part
            .strip_suffix(" this turn")
            .and_then(|c| c.split_once(" deals damage to "))
        && let (Ok(src), Ok(rec)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_subject_or_target(rec_raw.trim(), vocab),
        )
    {
        let body =
            if let Some(no_regen_subj) = body_raw.strip_suffix(" can't be regenerated this turn") {
                vec![InstructionAst::CombatRuleMod {
                    subject: Box::new(SubjectRefAst::Anaphora(no_regen_subj.trim().to_string())),
                    rule: "cant_be_regenerated".to_string(),
                    duration: DurationAst::UntilEndOfTurn,
                }]
            } else {
                let (b, fails) = parse_effect_body(body_raw.trim(), vocab);
                if fails.is_empty() && !b.is_empty() {
                    b
                } else {
                    return None;
                }
            };
        return Some(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::DamageDealt {
                    combat_only: false,
                    noncombat_only: false,
                    source: Some(Box::new(src)),
                    recipient: Some(Box::new(rec)),
                    grouped: false,
                    excess_only: false,
                }],
                intervening_if: None,
            },
            body,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 6. Turn-scoped damage prevention replacement for targeting spell/ability (Silhouette)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((ev_part, rest)) =
            after_if.split_once(" would cause a source to deal damage to ")
        && let Some(tgt_raw) = rest.strip_suffix(" this turn, prevent that damage")
        && let Ok(tgt) = parse_subject_or_target(tgt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryReplacement {
            replacement: Box::new(ReplacementEffectAst::EventInstead {
                subject_or_event: ev_part.trim().to_string(),
                would: format!("cause a source to deal damage to {}", tgt_raw.trim()),
                instead: "prevent that damage".to_string(),
                instead_body: Some(vec![InstructionAst::PreventAllDamage {
                    combat_only: false,
                    target: Some(Box::new(tgt)),
                    duration: DurationAst::Instant,
                }]),
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 7. Remove all <kind> counters from a <filter> that a <kind> counter was put onto with ~ but not removed from with ~ (Cyclopean Tomb)
    if let Some(after_rem) = low.strip_prefix("remove all ")
        && let Some((kind_raw, rest)) = after_rem.split_once(" counters from ")
        && let Some((base_flt_raw, tail)) = rest.split_once(" that a ")
        && tail.ends_with(" counter has not been removed from with ~")
        && let Ok(mut flt) = parse_object_filter(base_flt_raw.trim(), vocab)
    {
        let kind = kind_raw.trim().to_string();
        flt.predicates
            .push(FilterPredicateAst::CounterPlacedNotRemovedBySelf {
                counter_kind: kind.clone(),
            });
        return Some(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            counter_kind: kind,
            all: true,
            count: None,
        }]);
    }

    None
}

#[allow(clippy::too_many_lines)]
fn parse_compositional_combat_and_resource_grammar(
    s: &str,
    low: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Token creation with inherited abilities ("create <token_spec> and all triggered/activated abilities of <src>") (The Book of Vile Darkness)
    if let Some(after_cr) = low.strip_prefix("create ")
        && let Some((tok_part, inh_part)) = after_cr.split_once(" and all ")
        && let Some((kind_word, src_desc)) = inh_part.split_once(" abilities of ")
        && matches!(kind_word.trim(), "triggered" | "activated")
        && let Ok(mut create_ins) =
            parse_atomic_effect(&format!("create {}", tok_part.trim()), vocab)
    {
        let mut src_flt = any_card_flt();
        if src_desc.contains("exiled") {
            src_flt.zone = Some(ZoneFilter::Exile);
            src_flt.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        }
        src_flt.predicates.push(FilterPredicateAst::HasAbilityKind {
            kind: kind_word.trim().to_string(),
            negated: false,
        });
        create_ins.push(InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::InheritAbilities {
                subject: Box::new(SubjectRefAst::Anaphora("the token".to_string())),
                source_filter: Box::new(src_flt),
                activated_only: kind_word.trim() == "activated",
                condition: None,
            }),
            duration: DurationAst::Permanent,
        });
        return Some(create_ins);
    }

    // 2. Extra loyalty activation reset ("for each <filter>, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn") (The Chain Veil)
    if let Some(after_fe) = low.strip_prefix("for each ")
        && let Some((flt_raw, tail)) = after_fe.split_once(", you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let _ = tail;
        return Some(vec![InstructionAst::ForEachObject {
            filter: Box::new(flt),
            body: vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                    rule: "extra_loyalty_activation_as_though_none_activated".to_string(),
                    filter: None,
                    condition: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            }],
        }]);
    }

    // 3. Per-mana alternative activation payment ("for each mana in this ability's activation cost, you may <cost> rather than pay that mana") (Heirloom Epic)
    if let Some(after_fe) =
        low.strip_prefix("for each mana in this ability's activation cost, you may ")
        && let Some(cost_raw) = after_fe.strip_suffix(" rather than pay that mana")
        && let Ok(cost_atom) = parse_cost_atom(cost_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeAbilityCost {
                cost: vec![cost_atom],
                ability_kind: "this_ability_convoke".to_string(),
                scope: "per_mana_in_activation_cost".to_string(),
                condition: None,
            }),
            duration: DurationAst::Instant,
        }]);
    }

    // 4. Coin flip branch with recursive flip-again decision ("if you win/lose the flip, <body...>, and <player> decides whether to flip again") (Game of Chaos)
    if let Some((outcome, after_if)) = low
        .strip_prefix("if you win the flip, ")
        .map(|r| ("win", r))
        .or_else(|| {
            low.strip_prefix("if you lose the flip, ")
                .map(|r| ("lose", r))
        })
        && let Some((body_raw, decider_raw)) = after_if
            .strip_suffix(" decides whether to flip again")
            .or_else(|| after_if.strip_suffix(" decide whether to flip again"))
            .and_then(|r| r.rsplit_once(", and "))
        && let Some(decider) = parse_player_ref(decider_raw.trim())
    {
        let (mut then_body, fails) = parse_effect_body(body_raw.trim(), vocab);
        if fails.is_empty() && !then_body.is_empty() {
            then_body.push(InstructionAst::OptionalPlayer {
                player: decider,
                body: vec![InstructionAst::RepeatProcess {
                    condition: None,
                    max_times: None,
                    any_number: false,
                }],
            });
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(ConditionAst::CoinFlipOutcome {
                    outcome: outcome.to_string(),
                }),
                then_body,
            }]);
        }
    }

    // 5. Pairwise turn-control exchange ("the first player controls the second player during ..., and the second player controls the first player during ...") (Cruel Entertainment)
    if let Some((left_part, right_part)) = low.split_once(", and ")
        && let Some((c1, t1)) = parse_player_controls_next_turn(left_part.trim())
        && let Some((c2, t2)) = parse_player_controls_next_turn(right_part.trim())
    {
        return Some(vec![
            InstructionAst::PlayerControlsPlayerTurn {
                controller: Box::new(c1),
                controlled: Box::new(t1),
            },
            InstructionAst::PlayerControlsPlayerTurn {
                controller: Box::new(c2),
                controlled: Box::new(t2),
            },
        ]);
    }

    // 6. Ordered multi-type sacrifices ("<player> sacrifices <n1> permanent(s) of their choice of the first type, sacrifices <n2> of their choice of the second type, then sacrifices <n3> of their choice of the third type") (Rite of Ruin)
    if let Some((who_raw, rest)) = low.split_once(" sacrifices ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((p1, rest2)) = rest.split_once(", sacrifices ")
        && let Some((p2, p3)) = rest2.split_once(", then sacrifices ")
        && let (Some(n1), Some(n2), Some(n3)) = (
            parse_ordered_type_sac_count(p1, "first type"),
            parse_ordered_type_sac_count(p2, "second type"),
            parse_ordered_type_sac_count(p3, "third type"),
        )
    {
        let nth_perm = |nth: u8| {
            let mut flt = any_perm();
            flt.predicates.push(FilterPredicateAst::NthChosenType(nth));
            Box::new(SubjectRefAst::Group(Box::new(flt)))
        };
        let sac_player = match player {
            PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent => PlayerRefAst::ThatPlayer,
            other => other,
        };
        let body = vec![
            InstructionAst::Sacrifice {
                player: sac_player,
                count: Some(Box::new(ValueExprAst::Const(n1))),
                subject: nth_perm(1),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            },
            InstructionAst::Sacrifice {
                player: sac_player,
                count: Some(Box::new(ValueExprAst::Const(n2))),
                subject: nth_perm(2),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            },
            InstructionAst::Sacrifice {
                player: sac_player,
                count: Some(Box::new(ValueExprAst::Const(n3))),
                subject: nth_perm(3),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            },
        ];
        return Some(
            if matches!(
                player,
                PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent
            ) {
                vec![InstructionAst::ForEachPlayer { player, body }]
            } else {
                body
            },
        );
    }

    // 7. Capped life gain ("you gain life equal to <base>, but not more [life] than [the amount of {<c>} spent on x, ]the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness") (Drain Life, Soul Burn)
    if let Some(after_gain) = low.strip_prefix("you gain life equal to ")
        && let Some((base_raw, cap_raw)) = after_gain
            .split_once(", but not more life than ")
            .or_else(|| after_gain.split_once(", but not more than "))
        && cap_raw.ends_with("the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness")
    {
        let base_val = if base_raw.trim() == "the damage dealt" {
            ValueExprAst::PriorResult
        } else {
            parse_value_expr(base_raw.trim(), vocab).ok()?
        };
        let mut delta = base_val;
        if let Some(mana_prefix) = cap_raw.strip_prefix("the amount of ")
            && let Some((mana_sym, _)) = mana_prefix.split_once(" spent on x, ")
        {
            delta = ValueExprAst::Min(
                Box::new(delta),
                Box::new(ValueExprAst::ManaSpentToCast {
                    mana_filter: Some(format!("{}_on_X", mana_sym.trim().to_ascii_uppercase())),
                }),
            );
        }
        delta = ValueExprAst::Min(Box::new(delta), Box::new(ValueExprAst::TargetPreDamageCap));
        return Some(vec![InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            delta: Box::new(delta),
            gain: true,
        }]);
    }

    // 8. Count colors of <subj>, then search your library for <filter> that's exactly that many colors plus <n> (Evolving Door)
    if let Some(after_cnt) = low.strip_prefix("count the colors of ")
        && let Some((subj_raw, search_part)) =
            after_cnt.split_once(", then search your library for ")
        && let Some((flt_raw, plus_raw)) =
            search_part.split_once(" that's exactly that many colors plus ")
        && let (Ok(mut search_flt), Some(plus_n)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_number_word(plus_raw.trim()),
        )
    {
        let mut src_flt = obj_type(CardTypeMask::CREATURE);
        if subj_raw.contains("sacrificed") {
            src_flt
                .predicates
                .push(FilterPredicateAst::AffectedByPriorAction(
                    PriorActionRefKind::SacrificedThisWay,
                ));
        }
        let target_colors = ValueExprAst::Add(
            Box::new(ValueExprAst::ColorsAmong(Box::new(src_flt))),
            Box::new(ValueExprAst::Const(plus_n)),
        );
        search_flt
            .predicates
            .push(FilterPredicateAst::ColorCountCompare {
                op: CompareOp::Eq,
                expr: Box::new(target_colors),
            });
        return Some(vec![InstructionAst::SearchLibrary {
            library: "your".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
            up_to: false,
            any_number: false,
            exact: false,
            filter: Box::new(search_flt),
            reveal: false,
            to_zone: "found_set".to_string(),
            shuffle: false,
            extra_zones: Vec::new(),
            secondary_filter: None,
            secondary_is_or: false,
            where_x: None,
            rest_to_zone: None,
            modifier: None,
            follow_up: Vec::new(),
        }]);
    }

    // 9. Destroy with explicit exception ("destroy <subj>, except for <filter>") (Total War and future destroy-except cards)
    if let Some(after_des) = low.strip_prefix("destroy ")
        && let Some((subj_raw, exc_raw)) = after_des.split_once(", except for ")
        && let (Ok(subj), Ok(exc)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(exc_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::Destroy {
            subject: Box::new(subj),
            random: false,
            except: Some(Box::new(exc)),
        }]);
    }

    // 10. Put X <kind> counters on target creature that <blocker> blocked this turn, where X is <val> (Glyph of Delusion)
    if let Some(after_put) = low.strip_prefix("put x ")
        && let Some((kind_raw, rest)) = after_put.split_once(" counters on target creature that ")
        && let Some((blocker_raw, x_raw)) = rest.split_once(" blocked this turn, where x is ")
        && let (Ok(blocker), Ok(x_expr)) = (
            parse_subject_or_target(blocker_raw.trim(), vocab),
            parse_value_expr(x_raw.trim(), vocab),
        )
    {
        let mut blocked_cr = obj_type(CardTypeMask::CREATURE);
        blocked_cr.predicates.push(FilterPredicateAst::BlockedBy {
            blocker: Box::new(blocker),
            this_turn: true,
        });
        return Some(vec![InstructionAst::BindX {
            x_expr: Box::new(x_expr),
            body: vec![InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(blocked_cr),
                    count: TargetCountAst::Exact(1),
                }),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::ChosenX),
            }],
        }]);
    }

    // 11. Reanimate from LKI graveyard of last controller when blocked by <blocker> (Glyph of Reincarnation)
    if let Some(after_fe) = low.strip_prefix("for each creature that died this way, put ")
        && let Some((card_raw, rest)) = after_fe.split_once(" from the graveyard of the player who controlled that creature the last time it became blocked by ")
        && let Some((blocker_raw, dest_raw)) = rest.split_once(" onto ")
        && let (Ok(mut gy_flt), Ok(blocker)) = (
            parse_object_filter(card_raw.trim(), vocab),
            parse_subject_or_target(blocker_raw.trim(), vocab),
        )
    {
        let mut died = obj_type(CardTypeMask::CREATURE);
        died.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::DestroyedThisWay,
            ));
        gy_flt.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        gy_flt.flags = ObjectFilterFlags::LKI_PAST_TENSE;
        gy_flt.predicates.push(FilterPredicateAst::BlockedBy {
            blocker: Box::new(blocker),
            this_turn: true,
        });
        return Some(vec![InstructionAst::ForEachObject {
            filter: Box::new(died),
            body: vec![InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(gy_flt))),
                to: dest_raw.trim().to_string(),
            }],
        }]);
    }

    // 12. Return <subj> to its owner's hand if it shares a color with the most common color among <scope> (Barrin's Unmaking)
    if let Some(after_ret) = low.strip_prefix("return ")
        && let Some((subj_raw, rest)) = after_ret.split_once(" to its owner's hand if ")
        && let Some((_, scope_tail)) =
            rest.split_once(" shares a color with the most common color among ")
        && let (allow_tie, scope_raw) =
            match scope_tail.strip_suffix(" or a color tied for most common") {
                Some(r) => (true, r),
                None => (false, scope_tail),
            }
        && let (Ok(subj), Ok(scope_subj)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(scope_raw.trim(), vocab),
        )
    {
        let scope_flt = match scope_subj {
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => *f,
            _ => any_perm(),
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(
                GameMarker::SharesMostCommonColorAmong {
                    subject: Box::new(subj.clone()),
                    scope: Box::new(scope_flt),
                    allow_tie,
                },
            )),
            then_body: vec![InstructionAst::ReturnZone {
                subject: Box::new(subj),
                from_zone: Some("battlefield".to_string()),
                to: "its owner's hand".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            }],
        }]);
    }

    // 13. Temporary shroud exemption ("until end of turn, <subj> can be the target of spells and abilities controlled by <player> as though it didn't have shroud") (Autumn Willow)
    if let Some(after_u) = low.strip_prefix("until end of turn, ")
        && let Some((subj_raw, rest)) =
            after_u.split_once(" can be the target of spells and abilities controlled by ")
        && let Some(who_raw) = rest.strip_suffix(" as though it didn't have shroud")
        && let (Ok(subj), Some(who)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_player_ref(who_raw.trim()),
        )
    {
        let ctrl = match who {
            PlayerRefAst::TargetPlayer => ControllerRelation::TargetPlayer,
            PlayerRefAst::TargetOpponent => ControllerRelation::TargetOpponent,
            PlayerRefAst::You => ControllerRelation::You,
            _ => ControllerRelation::ThatPlayer,
        };
        let sp_or_ab = ObjectFilterAst {
            any_spell: true,
            is_ability: true,
            controller: Some(ctrl),
            ..Default::default()
        };
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "ignore_shroud_for_controller".to_string(),
                filter: Some(Box::new(sp_or_ab)),
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 14. Unpreventable and unredirectable damage ("damage that would be dealt to <subj> this turn can't be prevented or dealt instead to another permanent or player") (Whippoorwill)
    if let Some(after_d) = low.strip_prefix("damage that would be dealt to ")
        && let Some(subj_raw) = after_d.strip_suffix(
            " this turn can't be prevented or dealt instead to another permanent or player",
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "damage_cant_be_prevented_or_redirected".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 15. Reveal hand then discard cards sharing name with cards spliced onto <subj> (Minamo's Meddling)
    if let Some((who_raw, rest)) = low.split_once(
        " reveals their hand, then discards each card with the same name as a card spliced onto ",
    ) && let (Some(player), Ok(spell_subj)) = (
        parse_player_ref(who_raw.trim()),
        parse_subject_or_target(rest.trim(), vocab),
    ) {
        let mut spliced_match = any_card_flt();
        spliced_match
            .predicates
            .push(FilterPredicateAst::SameNameAsSplicedOnto(Box::new(
                spell_subj,
            )));
        return Some(vec![
            InstructionAst::RevealHand { player },
            InstructionAst::Discard {
                player,
                count: None,
                filter: Some(Box::new(spliced_match)),
                anaphora: None,
                random: false,
            },
        ]);
    }

    // 16. Damage to controller of the last <filter> that dealt damage to you this turn (Suffocation)
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Some((amt_raw, flt_raw)) = rest
            .split_once(" damage to the controller of the last ")
            .and_then(|(a, r)| {
                r.strip_suffix(" that dealt damage to you this turn")
                    .map(|f| (a, f))
            })
        && let (Ok(src), Ok(amt), Ok(mut spell_flt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        spell_flt.flags = spell_flt
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
        spell_flt
            .predicates
            .push(FilterPredicateAst::LastMatchingThisTurn);
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(SubjectRefAst::ControllerOf(Box::new(SubjectRefAst::Group(
                Box::new(spell_flt),
            )))),
            amount: Box::new(amt),
        }]);
    }

    let _ = s;
    None
}

fn parse_player_controls_next_turn(part: &str) -> Option<(SubjectRefAst, SubjectRefAst)> {
    let (ctrl_raw, rest) = part.split_once(" controls ")?;
    let (tgt_raw, dur_raw) = rest.split_once(" during ")?;
    if !dur_raw.ends_with("'s next turn") {
        return None;
    }
    Some((
        SubjectRefAst::Anaphora(ctrl_raw.trim().to_string()),
        SubjectRefAst::Anaphora(tgt_raw.trim().to_string()),
    ))
}

fn parse_ordered_type_sac_count(part: &str, ordinal_suffix: &str) -> Option<i32> {
    let before_ord = part
        .trim()
        .strip_suffix(&format!(" of the {ordinal_suffix}"))?;
    let before_choice = before_ord
        .strip_suffix(" of their choice")
        .unwrap_or(before_ord)
        .trim();
    let cnt_tok = before_choice
        .strip_suffix(" permanents")
        .or_else(|| before_choice.strip_suffix(" permanent"))
        .unwrap_or(before_choice)
        .trim();
    parse_number_word(cnt_tok)
}

fn parse_chained_draw_replacement(low: &str, vocab: &Vocabulary) -> Option<StaticOrReplacement> {
    // "if a player would draw a card except <exc>, <act1> instead. if the player <did>, <act2>. if the player doesn't <did>, <act3>"
    let after_if = low.strip_prefix("if a player would draw a card except ")?;
    let (exc, rest1) = after_if.split_once(", ")?;
    let (act1_raw, rest2) = rest1.split_once(" instead. if the player ")?;
    let (_, after_did) = rest2.split_once(" this way, ")?;
    let (act2_raw, rest3) = after_did.split_once(". if the player doesn't ")?;
    let (_, act3_raw) = rest3.split_once(" this way, ")?;

    let (mut body, fails1) = parse_effect_body(act1_raw.trim(), vocab);
    let (succ_body, fails2) = parse_effect_body(act2_raw.trim(), vocab);
    let (fail_body, fails3) = parse_effect_body(act3_raw.trim(), vocab);
    if !fails1.is_empty() || !fails2.is_empty() || !fails3.is_empty() || body.is_empty() {
        return None;
    }
    body.push(InstructionAst::IfPreviousSucceeded(succ_body));
    body.push(InstructionAst::IfPreviousFailed(fail_body));
    Some(StaticOrReplacement::Replacement(
        ReplacementEffectAst::EventInstead {
            subject_or_event: "a player".to_string(),
            would: format!("draw a card except {}", exc.trim()),
            instead: "chained_draw_replacement".to_string(),
            instead_body: Some(body),
        },
    ))
}

fn parse_as_enters_exile_copy_counters(
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // "as ~ enters, you may exile two <filter>. if you do, it enters as a copy of one of those cards with a number of additional <kind> counters on it equal to the <stat> of the other card"
    let after = low.strip_prefix("as ~ enters, you may exile two ")?;
    let (flt_raw, rest) = after.split_once(
        ". if you do, it enters as a copy of one of those cards with a number of additional ",
    )?;
    let (kind_raw, stat_raw) = rest.split_once(" counters on it equal to the ")?;
    let stat = match stat_raw.strip_suffix(" of the other card")? {
        "power" => StatKind::Power,
        "toughness" => StatKind::Toughness,
        "mana value" => StatKind::ManaValue,
        _ => return None,
    };
    let src_flt = parse_object_filter(flt_raw.trim(), vocab).ok()?;
    let mut exiled_copy_flt = src_flt.clone();
    exiled_copy_flt.zone = None;
    exiled_copy_flt
        .predicates
        .push(FilterPredicateAst::AffectedByPriorAction(
            PriorActionRefKind::ExiledThisWay,
        ));
    Some(StaticOrReplacement::Replacement(
        ReplacementEffectAst::Compound(vec![
            ReplacementEffectAst::AsEntersAction(vec![InstructionAst::Optional(vec![
                InstructionAst::MoveCounted {
                    actor: Some(PlayerRefAst::You),
                    count: Some(Box::new(ValueExprAst::Const(2))),
                    up_to: false,
                    any_number: false,
                    subject: Box::new(SubjectRefAst::Group(Box::new(src_flt.clone()))),
                    to: "exile".to_string(),
                },
            ])]),
            ReplacementEffectAst::EntersAsCopy {
                optional: false,
                filter: Box::new(exiled_copy_flt),
                exception: Some("if_exiled_two_cards_as_enters".to_string()),
            },
            ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("the other card".to_string())),
                    stat,
                }),
                per: None,
                x_expr: None,
                condition: Some(Box::new(ConditionAst::Marker(
                    GameMarker::ZoneActionHappenedThisWay {
                        filter: Box::new(src_flt),
                        action: "exile".to_string(),
                    },
                ))),
            },
        ]),
    ))
}

fn parse_as_enters_capped_life(low: &str, vocab: &Vocabulary) -> Option<StaticOrReplacement> {
    // "as ~ enters, pay any amount of life. the amount you pay can't be more than <cap_expr>"
    let cap_raw = low.strip_prefix(
        "as ~ enters, pay any amount of life. the amount you pay can't be more than ",
    )?;
    let cap_expr = if let Some((lhs_raw, rhs_raw)) = cap_raw.split_once(" plus ") {
        let lhs = parse_value_expr(
            lhs_raw
                .trim()
                .strip_prefix("the total ")
                .unwrap_or(lhs_raw.trim()),
            vocab,
        )
        .ok()?;
        let rhs_clean = rhs_raw
            .trim()
            .strip_prefix("the total ")
            .unwrap_or(rhs_raw.trim())
            .replace("in their graveyards", "in your opponents' graveyards");
        let rhs = parse_value_expr(&rhs_clean, vocab).ok()?;
        ValueExprAst::Add(Box::new(lhs), Box::new(rhs))
    } else {
        parse_value_expr(cap_raw.trim(), vocab).ok()?
    };
    Some(StaticOrReplacement::Replacement(
        ReplacementEffectAst::AsEntersAction(vec![
            InstructionAst::PayCost(CostAtomAst::PayLife(Box::new(ValueExprAst::ChosenX))),
            InstructionAst::AbilityRestriction {
                restriction: "as_enters_life_cap".to_string(),
                condition: Some(Box::new(ConditionAst::Compare {
                    lhs: Box::new(ValueExprAst::ChosenX),
                    op: CompareOp::Le,
                    rhs: Box::new(cap_expr),
                })),
            },
        ]),
    ))
}

fn parse_conditional_min_modify_pt(low: &str, vocab: &Vocabulary) -> Option<StaticOrReplacement> {
    // "<subj> gets -x/-y. if that creature is <cond>, x is <e1>. otherwise, x is <e2>. y is equal to x or to <e3>, whichever is smaller"
    let (subj_raw, rest1) = low.split_once(" gets -x/-y. if ")?;
    let (cond_raw, rest2) = rest1.split_once(", x is ")?;
    let (e1_raw, rest3) = rest2.split_once(". otherwise, x is ")?;
    let (e2_raw, rest4) = rest3.split_once(". y is equal to x or to ")?;
    let e3_raw = rest4.strip_suffix(", whichever is smaller")?;

    let subj = parse_subject_or_target(subj_raw.trim(), vocab).ok()?;
    let cond = if cond_raw.trim() == "that creature is attacking" {
        let mut att = obj_type(CardTypeMask::CREATURE);
        att.combat = Some(CombatState::Attacking);
        ConditionAst::MatchesFilter {
            subject: Box::new(subj.clone()),
            filter: Box::new(att),
        }
    } else {
        parse_condition(cond_raw.trim(), vocab).ok()?
    };
    let e1 = parse_value_expr(e1_raw.trim(), vocab).ok()?;
    let e2 = parse_value_expr(e2_raw.trim(), vocab).ok()?;
    let e3 = if let Some(stat_part) = e3_raw.strip_suffix(" minus 1") {
        let base = parse_value_expr(stat_part.trim(), vocab).ok()?;
        ValueExprAst::Sub(Box::new(base), Box::new(ValueExprAst::Const(1)))
    } else {
        parse_value_expr(e3_raw.trim(), vocab).ok()?
    };
    let x_expr = ValueExprAst::Conditional {
        condition: Box::new(cond),
        then_expr: Box::new(e1),
        else_expr: Box::new(e2),
    };
    let y_expr = ValueExprAst::Min(Box::new(x_expr.clone()), Box::new(e3));
    Some(StaticOrReplacement::Static(
        StaticAbilityAst::DynamicModifyPT {
            subject: Box::new(subj),
            power_delta: Box::new(ValueExprAst::Sub(
                Box::new(ValueExprAst::Const(0)),
                Box::new(x_expr),
            )),
            toughness_delta: Box::new(ValueExprAst::Sub(
                Box::new(ValueExprAst::Const(0)),
                Box::new(y_expr),
            )),
            condition: None,
        },
    ))
}

fn parse_untap_step_type_choice(low: &str) -> Option<StaticOrReplacement> {
    let after = low.strip_prefix("each player chooses ")?;
    let (types_part, tail) = after.split_once(" during their untap step. ")?;
    if tail != "that player can untap only permanents of the chosen type this step" {
        return None;
    }
    let normalized = types_part.trim().replace(", or ", ",").replace(", ", ",");
    if normalized.is_empty() {
        return None;
    }
    Some(StaticOrReplacement::Static(
        StaticAbilityAst::RuleOrCombatRestriction {
            subject: None,
            rule: format!("untap_step_choose_type_only_untap:{normalized}"),
            filter: None,
            condition: None,
        },
    ))
}
