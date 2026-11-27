//! Extended sentence-level and clause-level effect frames for `v2`.
//!
//! Keeps `clause/mod.rs` compact while providing strictly-typed compositional parsers for
//! reflexive triggers, scoped `During ...` clauses, `It's`/`They're` type predicates,
//! dynamic base P/T assignments, `For each` domains, player relative clauses, and
//! possessive compound player-actor clauses.

use crate::ast::{CardTypeMask, PlayerStatKind, Rounding, TurnStepAst};
use crate::v2::clause::player_verbs::parse_player_verb_frame;
use crate::v2::clause::subject_verbs::parse_trailing_duration;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, DelayedTriggerKind, Duration, Effect, ForEachDomain, LifeOp,
    ObjectFilter, ObjectMutation, ParseStage, PlayerActionKind, PlayerMutation, PlayerRef,
    PlayerRestrictionKind, PriorActionKind, Replacement, ReplacementRuleKind, RuleRestrictionKind,
    StaticAbility, StaticFlagKind, StepOwner, SubjectRef, TriggerEvent, TriggerSpec, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

use super::{
    apply_continuous, apply_replacement, compile_quoted_face, eat_rounding,
    eat_type_color_subtype_run, find_phrase_index, mutate_obj, mutate_player, parse_sentence_top,
    player_action, simple_zone_op,
};

/// Parse extended leading sentence forms before general clause dispatch.
pub(super) fn parse_leading_sentence_ext(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    // 1. Reflexive triggers: `When they do, <body>`, `When that player does, <body>`, ...
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["when", "they", "do"],
            &["when", "that", "player", "does"],
            &["when", "a", "player", "does"],
            &["when", "an", "opponent", "does"],
            &["when", "it", "connives", "this", "way"],
            &["when", "it", "explores", "this", "way"],
            &["when", "it", "deals", "excess", "damage", "this", "way"],
        ]) && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::ReflexiveTrigger(body)]);
        }
        ts.restore(cp);
    }

    // 2. Delayed dies trigger: `When it/that creature dies [under your control] this turn, <body>`
    {
        let cp = ts.checkpoint();
        let subj = if ts.eat_phrase(&["when", "it", "dies"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::It))
        } else if ts.eat_phrase(&["when", "that", "creature", "dies"]) {
            Some(SubjectRef::Anaphora(AnaphoraRef::ThatCreature))
        } else {
            None
        };
        if let Some(subject) = subj {
            let _ = ts.eat_any_phrase(&[
                &["under", "your", "control"],
                &["under", "its", "controller's", "control"],
            ]);
            if ts.eat_phrase(&["this", "turn"])
                && ts.eat_comma()
                && let Ok(body) = try_parse(ts, parse_sentence_top)
            {
                return Ok(vec![Effect::DelayedTrigger {
                    trigger: DelayedTriggerKind::Event(TriggerSpec {
                        events: vec![TriggerEvent::Dies { subject }],
                        intervening_if: None,
                    }),
                    body,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 3. Extended `At the beginning of ...` delayed steps
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["at", "the", "beginning", "of"]) {
            const DELAYED_STEPS: &[(&[&str], TurnStepAst, StepOwner)] = &[
                (
                    &[
                        "the", "end", "step", "of", "that", "player's", "next", "turn",
                    ],
                    TurnStepAst::EndStep,
                    StepOwner::ThatPlayer,
                ),
                (
                    &["the", "end", "step", "of", "their", "next", "turn"],
                    TurnStepAst::EndStep,
                    StepOwner::ThatPlayer,
                ),
                (
                    &["the", "next", "main", "phase", "this", "turn"],
                    TurnStepAst::MainPhase,
                    StepOwner::Next,
                ),
                (
                    &["the", "next", "main", "phase"],
                    TurnStepAst::MainPhase,
                    StepOwner::Next,
                ),
                (
                    &["that", "player's", "next", "declare", "attackers", "step"],
                    TurnStepAst::BeginningOfCombat,
                    StepOwner::ThatPlayer,
                ),
                (
                    &["their", "next", "declare", "attackers", "step"],
                    TurnStepAst::BeginningOfCombat,
                    StepOwner::ThatPlayer,
                ),
            ];
            let step_owner = DELAYED_STEPS
                .iter()
                .find_map(|&(p, step, owner)| ts.eat_phrase(p).then_some((step, owner)));
            if let Some((step, owner)) = step_owner
                && ts.eat_comma()
                && let Ok(body) = try_parse(ts, parse_sentence_top)
            {
                return Ok(vec![Effect::DelayedTrigger {
                    trigger: DelayedTriggerKind::Step { step, owner },
                    body,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 4. Leading `During ...` scoped turns and untap steps
    {
        let cp = ts.checkpoint();
        if ts.eat_word("during") {
            let untap_owner = if ts.eat_phrase(&["your", "next", "untap", "step"]) {
                Some(StepOwner::YourNext)
            } else if ts.eat_any_phrase(&[
                &["that", "player's", "next", "untap", "step"],
                &["their", "next", "untap", "step"],
            ]) {
                Some(StepOwner::ThatPlayer)
            } else {
                None
            };
            if let Some(owner) = untap_owner {
                let _ = ts.eat_comma();
                if ts.eat_phrase(&["as", "you", "untap", "your", "permanents"]) {
                    let _ = ts.eat_comma();
                }
                if let Ok(body) = try_parse(ts, parse_sentence_top) {
                    return Ok(vec![Effect::DelayedTrigger {
                        trigger: DelayedTriggerKind::Step {
                            step: TurnStepAst::UntapStep,
                            owner,
                        },
                        body,
                    }]);
                }
            }
            ts.restore(cp);
            if ts.eat_word("during") {
                let dur = if ts.eat_any_phrase(&[
                    &["target", "player's", "next", "turn"],
                    &["target", "opponent's", "next", "turn"],
                    &["that", "player's", "next", "turn"],
                    &["their", "next", "turn"],
                    &["your", "next", "turn"],
                ]) {
                    Some(Duration::UntilEndOfNextTurn)
                } else if ts.eat_phrase(&["that", "turn"]) {
                    Some(Duration::UntilEndOfTurn)
                } else {
                    None
                };
                if let Some(duration) = dur
                    && ts.eat_comma()
                    && let Ok(body) = try_parse(ts, parse_sentence_top)
                {
                    return Ok(vec![Effect::ScopedDuration { duration, body }]);
                }
            }
        }
        ts.restore(cp);
    }

    // 6. `The new target must be <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "new", "target", "must", "be"])
            && let Ok(new_tgt) = try_parse(ts, parse_subject_ref)
            && ts.is_eof()
        {
            return Ok(vec![mutate_player(
                PlayerRef::You,
                PlayerMutation::Retarget {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatSpell)),
                    new_target: Some(Box::new(new_tgt)),
                },
                Duration::Instant,
            )]);
        }
        ts.restore(cp);
    }

    // 7. `The player whose turn it is [may] <player_verb_frame>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "player", "whose", "turn", "it", "is"]) {
            let optional = ts.eat_word("may");
            if let Ok((body, ())) =
                try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::ActivePlayer))
            {
                let effs = if optional {
                    vec![Effect::Optional {
                        actor: PlayerRef::ActivePlayer,
                        body,
                    }]
                } else {
                    body
                };
                return Ok(effs);
            }
        }
        ts.restore(cp);
    }

    // 8. Extended `It's` / `They're` type/color/PT/ability assertions
    {
        let cp = ts.checkpoint();
        let subj_opt = if ts.eat_word("they're") {
            Some(SubjectRef::Anaphora(AnaphoraRef::Them))
        } else if ts.eat_word("it's") {
            Some(SubjectRef::Anaphora(AnaphoraRef::It))
        } else {
            None
        };
        if let Some(subj) = subj_opt {
            let is_still = ts.eat_word("still");
            let _ = ts.eat_any_word(&["a", "an"]);
            let base_pt = ts.next_base_pt();
            let (add_types, colors, subtypes) = eat_type_color_subtype_run(ts, true, true, true);
            if base_pt.is_some() || !add_types.is_empty() || !subtypes.is_empty() {
                if ts.eat_word("named") {
                    let mut nwords = 0usize;
                    while nwords < 4
                        && let Some(w) = ts.peek_word()
                        && w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    {
                        ts.next_word();
                        nwords += 1;
                    }
                }
                let mut inline_kws = Vec::new();
                if ts.eat_word("with")
                    && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                {
                    inline_kws = kws;
                }
                let keep_existing = is_still
                    || ts.eat_in_addition_to_other_types()
                    || ts.eat_any_phrase(&[
                        &["in", "addition", "to", "his", "other", "types"],
                        &["in", "addition", "to", "her", "other", "types"],
                    ]);
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                let mut out = Vec::new();
                if !colors.is_empty() {
                    out.push(mutate_obj(
                        subj.clone(),
                        ObjectMutation::BecomeColors {
                            colors,
                            keep_existing_colors: keep_existing,
                        },
                        duration.clone(),
                    ));
                }
                out.push(mutate_obj(
                    subj.clone(),
                    ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types: keep_existing,
                        base_pt,
                        keywords: inline_kws,
                    },
                    duration.clone(),
                ));
                let gcp = ts.checkpoint();
                if ts.eat_any_phrase(&[
                    &["and", "has", "no", "abilities"],
                    &["and", "have", "no", "abilities"],
                    &["and", "loses", "all", "abilities"],
                    &["and", "lose", "all", "abilities"],
                ]) {
                    out.push(mutate_obj(
                        subj,
                        ObjectMutation::RemoveAllAbilities,
                        duration,
                    ));
                } else if ts.eat_any_phrase(&[
                    &["and", "they", "gain"],
                    &["and", "it", "gains"],
                    &["and", "gains"],
                    &["and", "gain"],
                ]) && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                {
                    let gdur = parse_trailing_duration(ts).unwrap_or(duration);
                    out.push(mutate_obj(subj, ObjectMutation::GrantKeywords(kws), gdur));
                } else {
                    ts.restore(gcp);
                }
                if ts.is_eof() {
                    return Ok(out);
                }
            }
        }
        ts.restore(cp);
    }

    // 9. `<subj>'s base toughness / base power and toughness ...`
    {
        let cp = ts.checkpoint();
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("'s")
            && ts.eat_word("base")
        {
            let set_pt = |subj, power, toughness, duration| {
                mutate_obj(
                    subj,
                    ObjectMutation::SetDynamicBasePT {
                        power,
                        toughness,
                        add_types: CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    },
                    duration,
                )
            };
            if ts.eat_word("toughness")
                && ts.eat_any_phrase(&[
                    &["becomes", "equal", "to"],
                    &["become", "equal", "to"],
                    &["becomes"],
                    &["become"],
                ])
                && let Ok(toughness) = try_parse(ts, parse_value_expr)
            {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![set_pt(
                    subject.clone(),
                    ValueExpr::ObjectStat {
                        subject: Box::new(subject),
                        stat: crate::ast::StatKind::Power,
                    },
                    toughness,
                    duration,
                )]);
            }
            if ts.eat_phrase(&["power", "and", "toughness"])
                && ts.eat_any_word(&["become", "becomes"])
            {
                if let Some((p, t)) = ts.next_base_pt() {
                    let mut duration =
                        parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    let mut out = vec![set_pt(
                        subject.clone(),
                        ValueExpr::Const(p),
                        ValueExpr::Const(t),
                        duration.clone(),
                    )];
                    let gcp = ts.checkpoint();
                    if ts.eat_any_phrase(&[
                        &["and", "they", "gain"],
                        &["and", "it", "gains"],
                        &["and", "gains"],
                        &["and", "gain"],
                    ]) && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                    {
                        let gdur = parse_trailing_duration(ts).unwrap_or_else(|| duration.clone());
                        if gdur != duration {
                            duration = gdur.clone();
                            if let Effect::MutateObject { duration: d, .. } = &mut out[0] {
                                *d = duration;
                            }
                        }
                        out.push(mutate_obj(
                            subject,
                            ObjectMutation::GrantKeywords(kws),
                            gdur,
                        ));
                    } else {
                        ts.restore(gcp);
                    }
                    if ts.is_eof() {
                        return Ok(out);
                    }
                } else if ts.eat_phrase(&["equal", "to", "the", "power", "and", "toughness", "of"])
                    && let Ok(other) = try_parse(ts, parse_subject_ref)
                {
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok(vec![set_pt(
                        subject,
                        ValueExpr::ObjectStat {
                            subject: Box::new(other.clone()),
                            stat: crate::ast::StatKind::Power,
                        },
                        ValueExpr::ObjectStat {
                            subject: Box::new(other),
                            stat: crate::ast::StatKind::Toughness,
                        },
                        duration,
                    )]);
                }
            }
        }
        ts.restore(cp);
    }

    // 10. Extended `For each ...` leading domains
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each"]) {
            // `For each [of those] color(s), <body>`
            if ts.eat_any_phrase(&[&["of", "those", "colors"], &["color"]])
                && ts.eat_comma()
                && let Ok(body) = try_parse(ts, parse_sentence_top)
            {
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::ColorAmong(Box::default()),
                    body,
                }]);
            }
            ts.restore(cp);
            if ts.eat_phrase(&["for", "each"]) {
                const PRIOR_DOMAINS: &[(&[&str], PriorActionKind)] = &[
                    (&["card", "drawn", "this", "way"], PriorActionKind::ThatMany),
                    (
                        &["card", "milled", "this", "way"],
                        PriorActionKind::MilledThisWay,
                    ),
                    (
                        &["card", "discarded", "this", "way"],
                        PriorActionKind::DiscardedThisWay,
                    ),
                    (
                        &["card", "exiled", "this", "way"],
                        PriorActionKind::ExiledThisWay,
                    ),
                    (
                        &["permanent", "sacrificed", "this", "way"],
                        PriorActionKind::SacrificedThisWay,
                    ),
                    (
                        &["creature", "sacrificed", "this", "way"],
                        PriorActionKind::SacrificedThisWay,
                    ),
                ];
                if let Some(kind) = PRIOR_DOMAINS
                    .iter()
                    .find_map(|&(p, k)| ts.eat_phrase(p).then_some(k))
                    && ts.eat_comma()
                    && let Ok(body) = try_parse(ts, parse_sentence_top)
                {
                    return Ok(vec![Effect::ForEach {
                        over: ForEachDomain::Count(ValueExpr::PriorActionCount(kind)),
                        body,
                    }]);
                }
            }
            ts.restore(cp);
            if ts.eat_phrase(&["for", "each"])
                && try_parse(ts, parse_counter_kind).is_ok()
                && ts.eat_word("counter")
                && ts.eat_any_phrase(&[
                    &["you", "put", "on", "a", "creature", "this", "way"],
                    &["you", "put", "on", "a", "permanent", "this", "way"],
                    &["put", "on", "a", "creature", "this", "way"],
                ])
                && ts.eat_comma()
                && let Ok(body) = try_parse(ts, parse_sentence_top)
            {
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::Count(ValueExpr::PriorActionCount(
                        PriorActionKind::ThatMany,
                    )),
                    body,
                }]);
            }
            ts.restore(cp);
            if ts.eat_phrase(&["for", "each"]) {
                let _ = ts.eat_phrase(&["of", "those"]);
                let who_player = if ts.eat_any_word(&["opponents", "opponent"]) {
                    Some(PlayerRef::EachOpponent)
                } else if ts.eat_any_word(&["players", "player"]) {
                    Some(PlayerRef::EachPlayer)
                } else {
                    None
                };
                if let Some(p) = who_player
                    && ts.eat_any_phrase(&[&["who", "didn't"], &["who", "doesn't"]])
                    && let Some(comma_idx) = (ts.pos..ts.tokens.len())
                        .find(|&i| matches!(ts.tokens[i].kind, TokenKind::Comma))
                    && comma_idx > ts.pos
                {
                    let mut prior_end = comma_idx;
                    if prior_end >= ts.pos + 2
                        && matches!(ts.tokens[prior_end - 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("this"))
                        && matches!(ts.tokens[prior_end - 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("way"))
                    {
                        prior_end -= 2;
                    }
                    let prior_slice = &ts.tokens[ts.pos..prior_end];
                    let mut prior_ts = ts.sub_stream(prior_slice, ParseStage::Effect);
                    if parse_player_verb_frame(&mut prior_ts, p).is_ok() && prior_ts.is_eof() {
                        ts.commit_substream_eof(comma_idx + 1, &prior_ts);
                        if let Ok(body) = try_parse(ts, parse_sentence_top) {
                            return Ok(vec![Effect::ForEach {
                                over: ForEachDomain::EachPlayerWhoCant(p),
                                body,
                            }]);
                        }
                    }
                }
            }
            ts.restore(cp);
            if ts.eat_phrase(&["for", "each"]) {
                let _ = ts.next_number(false);
                let _ = opt(ts, parse_counter_kind);
                if ts.eat_any_word(&["counter", "counters"]) && ts.eat_word("removed") {
                    let _ = ts.eat_phrase(&["this", "way"]);
                    if ts.eat_comma()
                        && let Ok(body) = try_parse(ts, parse_sentence_top)
                    {
                        return Ok(vec![Effect::ForEach {
                            over: ForEachDomain::Count(ValueExpr::PriorActionCount(
                                PriorActionKind::CountersRemovedThisWay,
                            )),
                            body,
                        }]);
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 11. `The first player [may] <verb>` / `The second player [may] <verb>` / `If the first player does, <body>`
    //     (`Oath of Mages`, `Oath of Lieges`, `Oath of Druids`, `Oath of Scholars`, `Oath of Ghouls`, `Cruel Entertainment`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["if", "the", "first", "player", "does"])
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::IfPrevious {
                succeeded: true,
                body,
            }]);
        }
        ts.restore(cp);
        if ts.eat_phrase(&["the", "first", "player"]) || ts.eat_phrase(&["the", "second", "player"])
        {
            let optional = ts.eat_word("may");
            if let Ok((body, ())) =
                try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::ThatPlayer))
            {
                let mut effs = if optional {
                    vec![Effect::Optional {
                        actor: PlayerRef::ThatPlayer,
                        body,
                    }]
                } else {
                    body
                };
                // Allow chained `, <clause>` / `and <clause>` (e.g. `Oath of Lieges`, `Oath of Scholars`, `Cruel Entertainment`)
                let mcp = ts.checkpoint();
                if (ts.eat_comma() || ts.eat_word("and")) && !ts.is_eof() {
                    let _ = ts.eat_word("then") || ts.eat_word("and");
                    if let Ok(more) = try_parse(ts, parse_sentence_top) {
                        effs.extend(more);
                    } else if let Ok((more_v, ())) =
                        try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::ThatPlayer))
                    {
                        effs.extend(more_v);
                    } else {
                        ts.restore(mcp);
                    }
                }
                if ts.is_eof() {
                    return Ok(effs);
                }
            }
        }
        ts.restore(cp);
    }

    // 12. `Mana of any (type|color) can be spent to cast ...`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["mana", "of", "any"])
            && ts.eat_any_word(&["type", "color"])
            && ts.eat_phrase(&["can", "be", "spent", "to", "cast"])
            && ts.eat_any_phrase(&[
                &["a", "spell", "this", "way"],
                &["spells", "this", "way"],
                &["that", "spell"],
                &["those", "spells"],
                &["it"],
                &["them"],
            ])
            && ts.is_eof()
        {
            return Ok(vec![player_action(
                PlayerRef::You,
                PlayerActionKind::SpendManaAsAnyType,
                Duration::UntilEndOfTurn,
            )]);
        }
        ts.restore(cp);
    }

    // 13. `Until end of combat [on your next turn], <body>` (`Fire Lord Ozai`, `Avatar Roku`, `Brazen Cannonade`)
    //     AND `At end of combat, <body>` (`Defiant Vanguard`, `Time Elemental`, `Fortune, Loyal Steed`)
    //     AND `At the beginning of each combat this turn, <body>` (`Full Throttle`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["until", "end", "of", "combat"]) {
            let next_turn = ts.eat_phrase(&["on", "your", "next", "turn"]);
            if ts.eat_comma()
                && let Ok(body) = try_parse(ts, parse_sentence_top)
            {
                return Ok(vec![Effect::ScopedDuration {
                    duration: if next_turn {
                        Duration::UntilEndOfNextTurn
                    } else {
                        Duration::UntilEndOfCombat
                    },
                    body,
                }]);
            }
        }
        ts.restore(cp);
        if ts.eat_phrase(&["at", "end", "of", "combat"])
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Step {
                    step: TurnStepAst::EndOfCombat,
                    owner: StepOwner::Next,
                },
                body,
            }]);
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "at",
            "the",
            "beginning",
            "of",
            "each",
            "combat",
            "this",
            "turn",
        ]) && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Step {
                    step: TurnStepAst::BeginningOfCombat,
                    owner: StepOwner::Each,
                },
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 14. `When you next attack this turn, <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["when", "you", "next", "attack", "this", "turn"])
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Event(TriggerSpec {
                    events: vec![TriggerEvent::Attacks {
                        subject: None,
                        player: Some(PlayerRef::You),
                    }],
                    intervening_if: None,
                }),
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 15. `If (that spell | ...) would be put into ... graveyard, exile (it|them) instead`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("if") {
            let subj_ok = ts.eat_any_phrase(&[
                &["that", "spell"],
                &["those", "spells"],
                &["a", "spell", "cast", "this", "way"],
                &[
                    "a",
                    "spell",
                    "cast",
                    "from",
                    "your",
                    "graveyard",
                    "this",
                    "way",
                ],
            ]) || (ts.eat_phrase(&["you", "cast"])
                && (ts.eat_tilde() || ts.eat_word("it"))
                && ts.eat_phrase(&["this", "way", "and", "it"]));
            if subj_ok
                && ts.eat_phrase(&["would", "be", "put", "into"])
                && ts.eat_any_phrase(&[
                    &["your", "graveyard"],
                    &["a", "graveyard"],
                    &["its", "owner's", "graveyard"],
                    &["their", "owners'", "graveyards"],
                ])
                && ts.eat_comma()
                && ts.eat_any_phrase(&[&["exile", "it", "instead"], &["exile", "them", "instead"]])
                && ts.is_eof()
            {
                return Ok(vec![apply_replacement(
                    Replacement::Rule(ReplacementRuleKind::LeaveBattlefieldExileInstead),
                    Duration::UntilEndOfTurn,
                )]);
            }
        }
        ts.restore(cp);
    }

    // 18. `Rather than pay [...], you may <verb>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["rather", "than", "pay"])
            && (ts.eat_phrase(&[
                "the", "mana", "cost", "of", "the", "next", "spell", "you", "cast", "this", "turn",
            ]) || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"])))
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(body);
        }
        ts.restore(cp);
    }

    // 19. `For one spell this turn, <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "one", "spell", "this", "turn"])
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::ScopedDuration {
                duration: Duration::UntilEndOfTurn,
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 20. `Any number of target players may each <verb>` AND `You may then <verb>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["any", "number", "of", "target", "players", "may", "each"])
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::ForEach {
                over: ForEachDomain::Player(PlayerRef::TargetPlayer),
                body: vec![Effect::Optional {
                    actor: PlayerRef::ThatPlayer,
                    body,
                }],
            }]);
        }
        ts.restore(cp);
        if ts.eat_phrase(&["you", "may", "then"])
            && let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::You))
        {
            return Ok(vec![Effect::Optional {
                actor: PlayerRef::You,
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 21. `[Your | up to one target player's] life total becomes (half ... | that number)`
    {
        let cp = ts.checkpoint();
        let life_p = if ts.eat_phrase(&["your", "life", "total", "becomes"]) {
            Some(PlayerRef::You)
        } else if ts.eat_phrase(&[
            "up", "to", "one", "target", "player's", "life", "total", "becomes",
        ]) {
            Some(PlayerRef::TargetPlayer)
        } else {
            None
        };
        if let Some(player) = life_p {
            if ts.eat_phrase(&["that", "number"]) && ts.is_eof() {
                return Ok(vec![Effect::AdjustLife {
                    player,
                    amount: ValueExpr::Var(crate::v2::ir::VarName::X),
                    op: LifeOp::Set,
                }]);
            }
            if ts.eat_any_phrase(&[
                &["half", "your", "starting", "life", "total"],
                &["half", "their", "starting", "life", "total"],
            ]) {
                let rounding = eat_rounding(ts, Rounding::Down);
                if ts.is_eof() {
                    return Ok(vec![Effect::AdjustLife {
                        player,
                        amount: ValueExpr::Div {
                            expr: Box::new(ValueExpr::PlayerStat {
                                player,
                                stat: PlayerStatKind::LifeTotal,
                            }),
                            denom: 2,
                            rounding,
                        },
                        op: LifeOp::Set,
                    }]);
                }
            }
        }
        ts.restore(cp);
    }

    // 22. `It's an Aura enchantment with enchant <flt> and "..." [and it loses all other abilities]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["it's", "an", "aura", "enchantment", "with", "enchant"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_word("and")
            && let Some(quoted) = ts.next_quoted()
            && let Ok(compiled) =
                compile_quoted_face(ts, "~", "Enchantment - Aura", quoted.trim_end_matches(','))
        {
            let mut out = vec![
                mutate_obj(
                    SubjectRef::Anaphora(AnaphoraRef::It),
                    ObjectMutation::BecomeCreatureTypes {
                        subtypes: vec!["Aura".to_string()],
                        add_types: CardTypeMask::ENCHANTMENT,
                        keep_existing_types: false,
                        base_pt: None,
                        keywords: Vec::new(),
                    },
                    Duration::Permanent,
                ),
                mutate_obj(
                    SubjectRef::Anaphora(AnaphoraRef::It),
                    ObjectMutation::GrantAbility(Box::new(compiled)),
                    Duration::Permanent,
                ),
            ];
            let lcp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "it", "loses", "all", "other", "abilities"]) {
                out.push(mutate_obj(
                    SubjectRef::Anaphora(AnaphoraRef::It),
                    ObjectMutation::RemoveAllAbilities,
                    Duration::Permanent,
                ));
            } else {
                ts.restore(lcp);
            }
            if ts.is_eof() {
                return Ok(out);
            }
        }
        ts.restore(cp);
    }

    // 23. `You may pay <cost> to end this effect`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "pay"])
            && let Ok(cost) = try_parse(ts, crate::v2::cost::parse_cost_atom)
            && ts.eat_phrase(&["to", "end", "this", "effect"])
            && ts.is_eof()
        {
            return Ok(vec![Effect::Optional {
                actor: PlayerRef::You,
                body: vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::PayCost(cost),
                    Duration::Instant,
                )],
            }]);
        }
        ts.restore(cp);
    }

    // 25. `If it wasn't (sacrificed | put onto the battlefield with this ability), <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["if", "it", "wasn't", "sacrificed"],
            &[
                "if",
                "it",
                "wasn't",
                "put",
                "onto",
                "the",
                "battlefield",
                "with",
                "this",
                "ability",
            ],
        ]) && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::IfPrevious {
                succeeded: false,
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 26. Global flag / pile choice / retarget / opponent spell+loyalty restriction
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "a", "deck", "can", "have", "any", "number", "of", "cards", "named",
        ]) && ts.eat_tilde()
            && ts.is_eof()
        {
            return Ok(vec![apply_continuous(
                StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck),
                Duration::Permanent,
            )]);
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "one",
            "of",
            "their",
            "piles",
            "is",
            "chosen",
            "by",
            "one",
            "of",
            "their",
            "opponents",
            "of",
            "their",
            "choice",
        ]) && ts.is_eof()
        {
            return Ok(vec![simple_zone_op(
                PlayerRef::AnyOpponent,
                crate::v2::ir::ZoneOpKind::ChoosePile,
                SubjectRef::Anaphora(AnaphoraRef::OnePile),
                Vec::new(),
                None,
            )]);
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "the",
            "new",
            "targets",
            "can't",
            "be",
            "you",
            "or",
            "a",
            "permanent",
            "you",
            "control",
        ]) && ts.is_eof()
        {
            return Ok(vec![apply_continuous(
                StaticAbility::RuleRestriction {
                    subject: Some(SubjectRef::Player(PlayerRef::You)),
                    rule: RuleRestrictionKind::CantBeTargetedBy(Box::default()),
                    condition: None,
                },
                Duration::Instant,
            )]);
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "your",
            "opponents",
            "can't",
            "cast",
            "spells",
            "or",
            "activate",
            "planeswalkers'",
            "loyalty",
            "abilities",
        ]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            if ts.is_eof() {
                return Ok(vec![
                    apply_continuous(
                        StaticAbility::PlayerRestriction {
                            player: PlayerRef::EachOpponent,
                            restriction: PlayerRestrictionKind::CantCastSpells,
                            condition: None,
                        },
                        duration.clone(),
                    ),
                    apply_continuous(
                        StaticAbility::PlayerRestriction {
                            player: PlayerRef::EachOpponent,
                            restriction: PlayerRestrictionKind::CantActivateAbilities,
                            condition: None,
                        },
                        duration,
                    ),
                ]);
            }
        }
        ts.restore(cp);
    }

    // 27. Inline `If ... would ...` replacement + Haunt + Starting with
    if let Some(effs) = try_parse_inline_if_would(ts) {
        return Ok(effs);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["when", "the", "creature"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["haunts", "dies"])
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Event(TriggerSpec {
                    events: vec![TriggerEvent::SubjectState {
                        subject: SubjectRef::SelfObject,
                        event: crate::v2::ir::SubjectTriggerKind::DiesOrHauntedDies,
                    }],
                    intervening_if: None,
                }),
                body,
            }]);
        }
        ts.restore(cp);
        if ts.eat_any_phrase(&[
            &[
                "starting", "with", "the", "next", "opponent", "in", "turn", "order",
            ],
            &[
                "starting",
                "with",
                "you",
                "and",
                "proceeding",
                "in",
                "the",
                "chosen",
                "direction",
            ],
        ]) && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::StartingWith {
                player: PlayerRef::EachOpponent,
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 28. Compound possessive player actor
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["that", "permanent's", "controller", "or", "that", "player"])
            && let Ok((body, ())) =
                try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::ThatPlayer))
            && ts.is_eof()
        {
            return Ok(body);
        }
        ts.restore(cp);
    }

    Err(ts.err("not an extended leading sentence"))
}

/// Inline `If ... would ...` replacement combinator for effect sentences.
/// Strictly delegates to `crate::v2::replacement::parse_replacement` and fails closed on error.
fn try_parse_inline_if_would(ts: &mut TokenStream<'_>) -> Option<Vec<Effect>> {
    if !ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("if"))
        || !ts.tokens[ts.pos..]
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("would")))
    {
        return None;
    }
    let cp = ts.checkpoint();
    if let Ok(rep) = try_parse(ts, crate::v2::replacement::parse_replacement)
        && ts.is_eof()
    {
        return Some(vec![apply_replacement(rep, Duration::UntilEndOfTurn)]);
    }
    ts.restore(cp);
    None
}

/// Parse extended trailing `for each ...` / `once for each ...`
/// modifiers at the end of a sentence (called as fallback from `mod.rs` 11c).
pub(super) fn parse_extended_trailing_for_each(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    let cp = ts.checkpoint();
    let _ = ts.eat_word("once");
    if ts.eat_phrase(&["for", "each"]) {
        if let Ok(v) = try_parse(ts, |s| {
            // Re-run standard `parse_for_each_expr` when preceded by `once`
            s.restore(cp);
            let _ = s.eat_word("once");
            parse_for_each_expr(s)
        }) {
            return Ok(v);
        }
        if ts.eat_any_phrase(&[
            &["counter", "removed", "this", "way"],
            &["counter", "removed"],
        ]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::CountersRemovedThisWay,
            ));
        }
        if ts.eat_phrase(&["card", "in"])
            && let Ok(zr) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_card: true,
                zones: vec![zr],
                ..Default::default()
            })));
        }
    }
    ts.restore(cp);
    Err(ts.err("not an extended trailing for-each modifier"))
}

/// Try splitting a sentence on trailing `once for each ...` or `for each ...`
/// when standard clause sequencing leaves those modifiers unconsumed.
pub(super) fn try_parse_trailing_for_each_sentence(
    ts: &mut TokenStream<'_>,
) -> Option<Vec<Effect>> {
    let split_idx = find_phrase_index(ts.tokens, ts.pos, &["once", "for", "each"])
        .or_else(|| find_phrase_index(ts.tokens, ts.pos, &["for", "each"]))?;
    if split_idx <= ts.pos {
        return None;
    }
    let body_slice = &ts.tokens[ts.pos..split_idx];
    let fe_slice = &ts.tokens[split_idx..];
    let mut fe_ts = ts.sub_stream(fe_slice, ParseStage::Effect);
    let fe_cp = fe_ts.checkpoint();
    let scale = parse_for_each_expr(&mut fe_ts)
        .or_else(|_| {
            fe_ts.restore(fe_cp);
            parse_extended_trailing_for_each(&mut fe_ts)
        })
        .ok()?;
    if !fe_ts.is_eof() {
        return None;
    }
    let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
    let body = super::parse_clause_seq(&mut body_ts, None).ok()?;
    if !body_ts.is_eof() {
        return None;
    }
    ts.commit_substream_eof(ts.tokens.len(), &fe_ts);
    Some(vec![Effect::ForEach {
        over: ForEachDomain::Count(scale),
        body,
    }])
}

/// Parse `<player> who <relative> <verb>` forms inside `parse_single_clause`.
pub(super) fn parse_player_relative_clause(
    ts: &mut TokenStream<'_>,
    player: PlayerRef,
) -> PResult<(Vec<Effect>, Option<PlayerRef>)> {
    let cp = ts.checkpoint();

    // 1. `<player> who does/do <verb>`
    if (ts.eat_phrase(&["who", "does"]) || ts.eat_phrase(&["who", "do"]))
        && let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, player))
    {
        return Ok((
            vec![Effect::ForEach {
                over: ForEachDomain::EachPlayerWhoDoes(player),
                body,
            }],
            Some(player),
        ));
    }
    ts.restore(cp);

    // 2. `<player> who sacrificed/discarded <flt> [this way] <verb>`
    if ts.eat_phrase(&["who", "sacrificed"]) || ts.eat_phrase(&["who", "discarded"]) {
        if let Some(tw_idx) = find_phrase_index(ts.tokens, ts.pos, &["this", "way"])
            && tw_idx > ts.pos
            && tw_idx + 2 < ts.tokens.len()
        {
            let obj_slice = &ts.tokens[ts.pos..tw_idx];
            let mut obj_ts = ts.sub_stream(obj_slice, ParseStage::Effect);
            if parse_subject_ref(&mut obj_ts).is_ok() && obj_ts.is_eof() {
                ts.commit_substream_eof(tw_idx + 2, &obj_ts);
                if let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, player)) {
                    return Ok((
                        vec![Effect::ForEach {
                            over: ForEachDomain::EachPlayerWhoDoes(player),
                            body,
                        }],
                        Some(player),
                    ));
                }
            }
        }
        ts.restore(cp);
        if (ts.eat_phrase(&["who", "sacrificed"]) || ts.eat_phrase(&["who", "discarded"]))
            && try_parse(ts, parse_subject_ref).is_ok()
            && let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, player))
        {
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::EachPlayerWhoDoes(player),
                    body,
                }],
                Some(player),
            ));
        }
    }
    ts.restore(cp);

    // 3. `<player> who was dealt [combat] damage by <subj> this turn <verb>`
    if ts.eat_phrase(&["who", "was", "dealt"]) {
        let _ = ts.eat_word("combat");
        if ts.eat_phrase(&["damage", "by"])
            && let Ok(src_subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["this", "turn"])
            && let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, player))
        {
            let src_flt = match src_subj {
                SubjectRef::Group(f) | SubjectRef::All(f) | SubjectRef::Each(f) => f,
                _ => Box::default(),
            };
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::PlayersMatching(Box::new(
                        SubjectRef::PlayersDealtCombatDamageBy {
                            player,
                            source: src_flt,
                            this_game: false,
                        },
                    )),
                    body,
                }],
                Some(player),
            ));
        }
    }
    ts.restore(cp);

    // 4. `<player> who controls <N> or fewer <flt> [may] <verb>`
    if (ts.eat_phrase(&["who", "controls"]) || ts.eat_phrase(&["who", "control"]))
        && let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&["or", "fewer"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        let optional = ts.eat_word("may");
        if let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, player)) {
            let inner = if optional {
                vec![Effect::Optional {
                    actor: PlayerRef::ThatPlayer,
                    body,
                }]
            } else {
                body
            };
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::Player(player),
                    body: vec![Effect::BranchCondition {
                        condition: ConditionExpr::Compare {
                            lhs: Box::new(ValueExpr::Count(Box::new(flt))),
                            op: crate::ast::CompareOp::Le,
                            rhs: Box::new(ValueExpr::Const(n)),
                        },
                        then_body: inner,
                        else_body: Vec::new(),
                    }],
                }],
                Some(player),
            ));
        }
    }
    ts.restore(cp);

    Err(ts.err("not an extended player relative clause"))
}

/// Parse compound possessive player actors:
/// `Target artifact or creature's owner <verb>`, `that spell or ability's controller <verb>`.
pub(super) fn parse_possessive_player_clause(
    ts: &mut TokenStream<'_>,
) -> PResult<(Vec<Effect>, Option<PlayerRef>)> {
    let cp = ts.checkpoint();

    // `Target <type1> or <type2>'s owner/controller <verb>`
    if ts.eat_word("target")
        && let Ok(_flt) = try_parse(ts, parse_object_filter)
    {
        let actor_opt = if ts.eat_phrase(&["'s", "owner"]) {
            Some(PlayerRef::ItsOwner)
        } else if ts.eat_phrase(&["'s", "controller"]) {
            Some(PlayerRef::ItsController)
        } else {
            None
        };
        if let Some(actor) = actor_opt {
            let optional = ts.eat_word("may");
            if let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, actor)) {
                let effs = if optional {
                    vec![Effect::Optional { actor, body }]
                } else {
                    body
                };
                return Ok((effs, Some(actor)));
            }
        }
    }
    ts.restore(cp);

    // `that spell or ability's controller/owner [may] <verb>`
    let actor_opt = if ts.eat_phrase(&["that", "spell", "or", "ability's", "controller"]) {
        Some(PlayerRef::ThatSpellController)
    } else if ts.eat_phrase(&["that", "spell", "or", "ability's", "owner"])
        || ts.eat_phrase(&["target", "artifact", "or", "creature's", "owner"])
        || ts.eat_phrase(&["target", "nonland", "permanent's", "owner"])
    {
        Some(PlayerRef::ItsOwner)
    } else if ts.eat_phrase(&["target", "artifact", "or", "creature's", "controller"]) {
        Some(PlayerRef::ItsController)
    } else {
        None
    };
    if let Some(actor) = actor_opt {
        let optional = ts.eat_word("may");
        if let Ok((body, ())) = try_parse(ts, |s| parse_player_verb_frame(s, actor)) {
            let effs = if optional {
                vec![Effect::Optional { actor, body }]
            } else {
                body
            };
            return Ok((effs, Some(actor)));
        }
    }
    ts.restore(cp);

    // `All creatures able to block <subj> this turn do so` (as a clause inside compound sentences)
    if ts.eat_phrase(&["all", "creatures", "able", "to", "block"])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
        && ts.eat_any_phrase(&[&["this", "turn", "do", "so"], &["do", "so"]])
    {
        return Ok((
            vec![apply_continuous(
                StaticAbility::SubjectRule {
                    subject,
                    rule: crate::v2::ir::SubjectStaticRule::AllMustBlock,
                    condition: None,
                },
                Duration::UntilEndOfTurn,
            )],
            None,
        ));
    }
    ts.restore(cp);

    Err(ts.err("not a possessive player clause"))
}
