//! Mana addition, damage prevention, and standalone keyword-action verb helpers for `v2` clauses.

use crate::v2::clause::subject_verbs::parse_trailing_duration;
use crate::v2::filter::{parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    Duration, Effect, ForEachDomain, ManaSpec, ObjectKeywordAction, ObjectMutation,
    ObjectStateAction, PlayerActionKind, PlayerMutation, PlayerRef, ScaledPlayerAction, SubjectRef,
    ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;

use super::{
    apply_continuous, apply_replacement, exact_count, exile_zone_op, move_zone_op, mutate_obj,
    mutate_player, player_action, prompt_choice, simple_zone_dest, simple_zone_op,
    zone_cards_subject,
};

pub(super) fn parse_mana_addition(ts: &mut TokenStream<'_>) -> PResult<ManaSpec> {
    let _ = ts.eat_phrase(&["an", "additional"]);
    {
        let tm_cp = ts.checkpoint();
        if ts.eat_phrase(&["twice", "that", "much"])
            && let Some(mc) = ts.eat_mana_cost()
        {
            return Ok(ManaSpec::Scaled {
                base: Box::new(ManaSpec::Exact(mc)),
                scale: ValueExpr::Mul(
                    Box::new(ValueExpr::Const(2)),
                    Box::new(ValueExpr::PriorActionCount(
                        crate::v2::ir::PriorActionKind::ThatMany,
                    )),
                ),
            });
        }
        ts.restore(tm_cp);
        if ts.eat_phrase(&["that", "much"])
            && let Some(mc) = ts.eat_mana_cost()
        {
            return Ok(ManaSpec::Scaled {
                base: Box::new(ManaSpec::Exact(mc)),
                scale: ValueExpr::PriorActionCount(
                    crate::v2::ir::PriorActionKind::DealtDamageThisWay,
                ),
            });
        }
        ts.restore(tm_cp);
    }
    {
        let amt_cp = ts.checkpoint();
        if ts.eat_phrase(&["an", "amount", "of", "mana", "of"]) {
            let chosen = ts.eat_phrase(&["the", "chosen", "color"]);
            let that_c = !chosen && ts.eat_phrase(&["that", "color"]);
            if (chosen || that_c)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(amount) = try_parse(ts, parse_value_expr)
            {
                return Ok(if chosen {
                    ManaSpec::ChosenColor(amount)
                } else {
                    ManaSpec::ThatColor(amount)
                });
            }
        }
        ts.restore(amt_cp);
    }
    if ts.eat_phrase(&["an", "amount", "of"])
        && let Some(mc) = ts.eat_mana_cost()
        && ts.eat_phrase(&["equal", "to"])
    {
        let scale = parse_value_expr(ts)?;
        return Ok(ManaSpec::Scaled {
            base: Box::new(ManaSpec::Exact(mc)),
            scale,
        });
    }
    if ts.eat_phrase(&["one", "mana", "of", "the", "chosen", "color"]) {
        return Ok(ManaSpec::ChosenColor(ValueExpr::Const(1)));
    }
    if ts.eat_phrase(&[
        "one", "mana", "of", "either", "of", "the", "circled", "colors",
    ]) {
        return Ok(ManaSpec::AnyColor {
            amount: ValueExpr::Const(1),
            single_color: true,
        });
    }
    if ts.eat_phrase(&["one", "mana", "of", "that", "color"]) {
        return Ok(ManaSpec::ThatColor(ValueExpr::Const(1)));
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["one", "mana", "of", "any", "of"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["'s", "colors"])
        {
            return Ok(ManaSpec::AnyColor {
                amount: ValueExpr::Const(1),
                single_color: true,
            });
        }
        ts.restore(ccp);
    }
    {
        let tcp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["one", "mana", "of", "any", "type", "that"],
            &["one", "mana", "of", "any", "color", "that"],
            &["one", "mana", "of", "any", "type"],
            &["one", "mana", "of", "any", "color"],
        ]) && (ts.eat_any_phrase(&[
            &["the", "sacrificed", "land"],
            &["the", "sacrificed", "permanent"],
            &["the", "exiled", "land"],
        ]) || try_parse(ts, parse_object_filter).is_ok())
            && (ts.eat_phrase(&["could", "produce"]) || ts.eat_word("produced"))
        {
            return Ok(ManaSpec::AnyColor {
                amount: ValueExpr::Const(1),
                single_color: true,
            });
        }
        ts.restore(tcp);
    }
    if ts.eat_any_phrase(&[
        &[
            "one", "mana", "of", "any", "type", "that", "land", "produced",
        ],
        &[
            "one",
            "mana",
            "of",
            "any",
            "type",
            "that",
            "permanent",
            "produced",
        ],
        &[
            "one", "mana", "of", "any", "of", "the", "exiled", "cards'", "colors",
        ],
        &["one", "mana", "of", "any", "color"],
    ]) {
        let _ = ts.eat_phrase(&["in", "your", "commander's", "color", "identity"]);
        let acp = ts.checkpoint();
        if ts.eat_word("among") && try_parse(ts, parse_object_filter).is_ok() {
            let ccp = ts.checkpoint();
            if !(ts.eat_word("and") && try_parse(ts, parse_object_filter).is_ok()) {
                ts.restore(ccp);
            }
        } else {
            ts.restore(acp);
        }
        return Ok(ManaSpec::AnyColor {
            amount: ValueExpr::Const(1),
            single_color: true,
        });
    }
    {
        let cp = ts.checkpoint();
        if let Ok(amount) = try_parse(ts, parse_value_expr) {
            if let Some(mc) = ts.eat_mana_cost() {
                let first = ManaSpec::Scaled {
                    base: Box::new(ManaSpec::Exact(mc)),
                    scale: amount,
                };
                let ocp = ts.checkpoint();
                if ts.eat_word("or")
                    && let Ok(amt2) = try_parse(ts, parse_value_expr)
                    && let Some(mc2) = ts.eat_mana_cost()
                {
                    return Ok(ManaSpec::Either(
                        Box::new(first),
                        Box::new(ManaSpec::Scaled {
                            base: Box::new(ManaSpec::Exact(mc2)),
                            scale: amt2,
                        }),
                    ));
                }
                ts.restore(ocp);
                return Ok(first);
            }
            if ts.eat_word("mana") {
                if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
                    return Ok(ManaSpec::ChosenColor(amount));
                }
                if ts.eat_phrase(&["of", "that", "color"]) || ts.eat_phrase(&["of", "that", "type"])
                {
                    return Ok(ManaSpec::ThatColor(amount));
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
                if ts.eat_phrase(&["in", "any", "combination", "of"])
                    && let Some(first_mc) = ts.eat_mana_cost()
                {
                    let mut opts = vec![first_mc];
                    loop {
                        let ocp = ts.checkpoint();
                        let _ = ts.eat_comma();
                        if (ts.eat_word("and/or") || ts.eat_word("or") || ts.eat_word("and"))
                            && let Some(next_mc) = ts.eat_mana_cost()
                        {
                            opts.push(next_mc);
                            continue;
                        }
                        ts.restore(ocp);
                        break;
                    }
                    return Ok(ManaSpec::Scaled {
                        base: Box::new(ManaSpec::Choice(opts)),
                        scale: amount,
                    });
                }
                if ts.eat_phrase(&["of", "different", "colors"]) {
                    return Ok(ManaSpec::DifferentColors(amount));
                }
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
        let base = if choices.len() == 1 {
            ManaSpec::Exact(choices.remove(0))
        } else {
            ManaSpec::Choice(choices)
        };
        let ocp = ts.checkpoint();
        if ts.eat_phrase(&["or", "one", "mana", "of", "the", "chosen", "color"]) {
            return Ok(ManaSpec::Either(
                Box::new(base),
                Box::new(ManaSpec::ChosenColor(ValueExpr::Const(1))),
            ));
        }
        ts.restore(ocp);
        return Ok(base);
    }
    Err(ts.err("expected mana spec after `add`"))
}

pub(super) fn parse_prevent_damage(ts: &mut TokenStream<'_>) -> PResult<Effect> {
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

    // `Prevent all [combat] damage a source of your choice / <subj> would deal [to <tgt>] this turn` (`Pay No Heed`, `Guard Dogs`, `Resistance Fighter`)
    if ts.eat_phrase(&["a", "source", "of", "your", "choice", "would", "deal"])
        || ts.eat_phrase(&["a", "source", "of", "their", "choice", "would", "deal"])
        || ts.eat_phrase(&[
            "that", "a", "source", "of", "your", "choice", "would", "deal",
        ])
    {
        source = Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatObject));
    } else {
        let scp = ts.checkpoint();
        let _ = ts.eat_word("that");
        if let Ok(src) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["would", "deal"])
        {
            source = Some(src);
        } else {
            ts.restore(scp);
        }
    }

    loop {
        if ts.eat_phrase(&["to", "and", "dealt", "by"]) {
            let subj = parse_subject_ref(ts)?;
            target = Some(subj.clone());
            source = Some(subj);
            continue;
        }
        if ts.eat_word("to") {
            let first = parse_subject_ref(ts)?;
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(second) = try_parse(ts, parse_subject_ref)
            {
                target = Some(SubjectRef::Compound(vec![first, second]));
            } else {
                ts.restore(acp);
                target = Some(first);
            }
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
        let dcp = ts.checkpoint();
        if ts.eat_comma() && ts.eat_phrase(&["divided", "as", "you", "choose"]) {
            continue;
        }
        ts.restore(dcp);
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

fn wrap_optional_repeat_times(ts: &mut TokenStream<'_>, eff: Effect) -> Effect {
    if ts.eat_word("twice") {
        return Effect::ForEach {
            over: ForEachDomain::Count(ValueExpr::Const(2)),
            body: vec![eff],
        };
    }
    let cp = ts.checkpoint();
    let times_opt = if let Some(n) = ts.next_number(false) {
        Some(ValueExpr::Const(n))
    } else if ts.eat_word("x") {
        Some(ValueExpr::Var(crate::v2::ir::VarName::X))
    } else {
        try_parse(ts, parse_value_expr).ok()
    };
    if let Some(times) = times_opt
        && ts.eat_word("times")
    {
        return Effect::ForEach {
            over: ForEachDomain::Count(times),
            body: vec![eff],
        };
    }
    ts.restore(cp);
    eff
}

pub(super) fn parse_keyword_action_verb(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<Effect> {
    const SIMPLE_PLAYER_ACTIONS: &[(&[&str], PlayerActionKind)] = &[
        (
            &["take", "the", "initiative"],
            PlayerActionKind::TakeInitiative,
        ),
        (
            &["takes", "the", "initiative"],
            PlayerActionKind::TakeInitiative,
        ),
        (
            &["open", "an", "attraction"],
            PlayerActionKind::OpenAttraction,
        ),
        (
            &["opens", "an", "attraction"],
            PlayerActionKind::OpenAttraction,
        ),
        (
            &["repeat", "this", "process", "any", "number", "of", "times"],
            PlayerActionKind::RepeatProcessOnce,
        ),
        (
            &["repeat", "this", "process", "once"],
            PlayerActionKind::RepeatProcessOnce,
        ),
        (
            &["repeat", "this", "process"],
            PlayerActionKind::RepeatProcessOnce,
        ),
        (&["win", "the", "game"], PlayerActionKind::WinGame),
        (&["wins", "the", "game"], PlayerActionKind::WinGame),
        (&["recruit"], PlayerActionKind::Recruit),
        (&["recruits"], PlayerActionKind::Recruit),
        (&["learn"], PlayerActionKind::Learn),
        (&["learns"], PlayerActionKind::Learn),
        (
            &["clash", "with", "an", "opponent"],
            PlayerActionKind::Clash,
        ),
        (
            &["clashes", "with", "an", "opponent"],
            PlayerActionKind::Clash,
        ),
        (&["forage"], PlayerActionKind::Forage),
        (&["forages"], PlayerActionKind::Forage),
        (&["manifest", "dread"], PlayerActionKind::ManifestDread),
        (&["manifests", "dread"], PlayerActionKind::ManifestDread),
        (
            &["manifest", "the", "top", "card", "of", "your", "library"],
            PlayerActionKind::Manifest,
        ),
        (
            &["manifest", "the", "top", "card", "of", "their", "library"],
            PlayerActionKind::Manifest,
        ),
        (
            &["manifests", "the", "top", "card", "of", "your", "library"],
            PlayerActionKind::Manifest,
        ),
        (
            &["manifests", "the", "top", "card", "of", "their", "library"],
            PlayerActionKind::Manifest,
        ),
        (
            &["venture", "into", "the", "dungeon"],
            PlayerActionKind::VentureIntoDungeon,
        ),
        (
            &["ventures", "into", "the", "dungeon"],
            PlayerActionKind::VentureIntoDungeon,
        ),
    ];
    for &(phrase, action) in SIMPLE_PLAYER_ACTIONS {
        if ts.eat_phrase(phrase) {
            return Ok(player_action(actor, action, Duration::Instant));
        }
    }
    if ts.eat_any_phrase(&[
        &[
            "don't", "lose", "this", "mana", "as", "steps", "and", "phases", "end",
        ],
        &[
            "doesn't", "lose", "this", "mana", "as", "steps", "and", "phases", "end",
        ],
        &["don't", "lose", "this", "mana", "as", "steps", "end"],
        &["doesn't", "lose", "this", "mana", "as", "steps", "end"],
    ]) {
        return Ok(player_action(
            actor,
            PlayerActionKind::KeepManaAcrossSteps,
            Duration::UntilEndOfTurn,
        ));
    }
    if ts.eat_any_phrase(&[&["end", "the", "turn"], &["ends", "the", "turn"]]) {
        return Ok(mutate_player(
            actor,
            PlayerMutation::SkipNextStep(crate::ast::TurnStepAst::CleanupStep),
            Duration::Instant,
        ));
    }
    if ts.eat_any_phrase(&[&["time", "travel"], &["time", "travels"]]) {
        let time_subj = SubjectRef::All(Box::new(crate::v2::ir::ObjectFilter {
            predicates: vec![crate::v2::ir::FilterPredicate::HasCounterKind(
                crate::v2::ir::CounterKind::Time,
            )],
            ..Default::default()
        }));
        let eff = prompt_choice(
            actor,
            crate::v2::ir::ChoiceKind::ChooseBranch(vec![
                vec![mutate_obj(
                    time_subj.clone(),
                    ObjectMutation::PutCounters {
                        kind: crate::v2::ir::CounterKind::Time,
                        count: ValueExpr::Const(1),
                    },
                    Duration::Instant,
                )],
                vec![mutate_obj(
                    time_subj,
                    ObjectMutation::RemoveCounters {
                        kind: crate::v2::ir::CounterKind::Time,
                        count: Some(ValueExpr::Const(1)),
                        all: false,
                    },
                    Duration::Instant,
                )],
            ]),
        );
        return Ok(wrap_optional_repeat_times(ts, eff));
    }
    if ts.eat_any_word(&["airbend", "airbends"]) {
        let any_number = ts.eat_phrase(&["any", "number", "of"]);
        let subject = parse_subject_ref(ts)?;
        return Ok(exile_zone_op(
            actor,
            subject,
            Vec::new(),
            false,
            None,
            any_number.then_some(crate::v2::ir::CountSpec {
                count: ValueExpr::Const(0),
                up_to: false,
                any_number: true,
            }),
        ));
    }
    if ts.eat_any_word(&["cloak", "cloaks"]) {
        let (subject, from, count) = if ts.eat_phrase(&["the", "top", "card", "of"]) {
            let zref = crate::v2::filter::parse_zone_ref(ts)?;
            (
                zone_cards_subject(zref),
                vec![zref],
                Some(exact_count(ValueExpr::Const(1))),
            )
        } else {
            let subj = parse_subject_ref(ts)?;
            let mut f = Vec::new();
            if ts.eat_word("from")
                && let Ok(zr) = try_parse(ts, crate::v2::filter::parse_zone_ref)
            {
                f.push(zr);
            }
            (subj, f, None)
        };
        return Ok(move_zone_op(
            actor,
            subject,
            from,
            simple_zone_dest(
                crate::v2::ir::ZoneKind::Battlefield,
                crate::v2::ir::ZoneOwner::Default,
            ),
            crate::v2::ir::EntryModifiers {
                face_down: true,
                ..crate::v2::ir::EntryModifiers::default()
            },
            count,
        ));
    }
    if ts.eat_any_word(&["skip", "skips"]) {
        const SKIP_NEXT_TURN_STEPS: &[(&[&str], crate::ast::TurnStepAst)] = &[
            (
                &["all", "combat", "phases", "of", "their", "next", "turn"],
                crate::ast::TurnStepAst::BeginningOfCombat,
            ),
            (
                &["all", "combat", "phases", "of", "your", "next", "turn"],
                crate::ast::TurnStepAst::BeginningOfCombat,
            ),
            (
                &["the", "untap", "step", "of", "that", "turn"],
                crate::ast::TurnStepAst::UntapStep,
            ),
            (
                &["the", "draw", "step", "of", "that", "turn"],
                crate::ast::TurnStepAst::DrawStep,
            ),
        ];
        for &(phrase, step) in SKIP_NEXT_TURN_STEPS {
            if ts.eat_phrase(phrase) {
                return Ok(mutate_player(
                    actor,
                    PlayerMutation::SkipNextStep(step),
                    Duration::UntilEndOfNextTurn,
                ));
            }
        }
        const SKIP_STEPS: &[(&[&str], crate::ast::TurnStepAst)] = &[
            (&["draw", "step"], crate::ast::TurnStepAst::DrawStep),
            (&["untap", "step"], crate::ast::TurnStepAst::UntapStep),
            (
                &["combat", "phase"],
                crate::ast::TurnStepAst::BeginningOfCombat,
            ),
        ];
        if ts.eat_any_phrase(&[&["your", "next"], &["their", "next"]]) {
            if ts.eat_word("turn") {
                return Ok(player_action(
                    actor,
                    PlayerActionKind::SkipNextTurn,
                    Duration::Instant,
                ));
            }
            if let Ok(cnt) = try_parse(ts, parse_value_expr)
                && ts.eat_word("turns")
            {
                return Ok(Effect::ForEach {
                    over: ForEachDomain::Count(cnt),
                    body: vec![player_action(
                        actor,
                        PlayerActionKind::SkipNextTurn,
                        Duration::Instant,
                    )],
                });
            }
            for &(phrase, step) in SKIP_STEPS {
                if ts.eat_phrase(phrase) {
                    return Ok(mutate_player(
                        actor,
                        PlayerMutation::SkipNextStep(step),
                        Duration::Instant,
                    ));
                }
            }
        }
        if ts.eat_any_word(&["your", "their"]) {
            for &(phrase, step) in SKIP_STEPS {
                if ts.eat_phrase(phrase) {
                    let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok(mutate_player(
                        actor,
                        PlayerMutation::SkipNextStep(step),
                        dur,
                    ));
                }
            }
        }
    }
    if ts.eat_word("monstrosity") {
        let amt = parse_value_expr(ts)?;
        return Ok(mutate_obj(
            SubjectRef::SelfObject,
            ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Monstrosity,
                amount: amt,
            },
            Duration::Permanent,
        ));
    }
    if ts.eat_any_word(&["empower", "empowers"])
        && let Some(w) = ts.peek_word()
        && let Some(pw) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase())
    {
        let planeswalker = pw.to_string();
        ts.next_word();
        let amount = parse_value_expr(ts)?;
        return Ok(mutate_player(
            actor,
            PlayerMutation::Empower {
                planeswalker,
                amount,
            },
            Duration::Instant,
        ));
    }
    if ts.eat_any_word(&["proliferate", "proliferates"]) {
        return Ok(wrap_optional_repeat_times(
            ts,
            player_action(actor, PlayerActionKind::Proliferate, Duration::Instant),
        ));
    }
    if ts.eat_any_word(&["investigate", "investigates"]) {
        let amt = if ts.eat_word("twice") {
            ValueExpr::Const(2)
        } else {
            let cp = ts.checkpoint();
            let v = ts
                .next_number(false)
                .map(ValueExpr::Const)
                .or_else(|| {
                    ts.eat_word("x")
                        .then_some(ValueExpr::Var(crate::v2::ir::VarName::X))
                })
                .or_else(|| try_parse(ts, parse_value_expr).ok());
            if let Some(times) = v
                && ts.eat_word("times")
            {
                times
            } else {
                ts.restore(cp);
                ValueExpr::Const(1)
            }
        };
        return Ok(mutate_player(
            actor,
            PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Investigate,
                amount: amt,
            },
            Duration::Instant,
        ));
    }
    if ts.eat_any_word(&["populate", "populates"]) {
        return Ok(wrap_optional_repeat_times(
            ts,
            player_action(actor, PlayerActionKind::Populate, Duration::Instant),
        ));
    }
    if ts.eat_any_word(&["behold", "beholds"]) {
        let flt = parse_object_filter(ts)?;
        return Ok(mutate_player(
            actor,
            PlayerMutation::Behold(flt),
            Duration::Instant,
        ));
    }
    if ts.eat_any_word(&["adapt", "adapts"]) {
        let n = ts
            .next_number(false)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| ts.err("expected number after `adapt`"))?;
        return Ok(mutate_obj(
            SubjectRef::SelfObject,
            ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Adapt,
                amount: ValueExpr::Const(i32::from(n)),
            },
            Duration::Instant,
        ));
    }
    if ts.eat_any_word(&["amass", "amasses"]) {
        let subtype = if let Some(w) = ts.peek_word()
            && let Some(sub) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase())
        {
            ts.next_word();
            sub.to_string()
        } else {
            "Army".to_string()
        };
        let amount = parse_value_expr(ts)?;
        return Ok(mutate_player(
            actor,
            PlayerMutation::Amass { subtype, amount },
            Duration::Instant,
        ));
    }
    if ts.eat_phrase(&["the", "ring", "tempts", "you"]) {
        return Ok(player_action(
            PlayerRef::You,
            PlayerActionKind::RingTemptsYou,
            Duration::Instant,
        ));
    }
    const SCALED_ACTIONS: &[(&[&str], ScaledPlayerAction, bool)] = &[
        (&["earthbend"], ScaledPlayerAction::Earthbend, false),
        (&["earthbends"], ScaledPlayerAction::Earthbend, false),
        (&["blight"], ScaledPlayerAction::Blight, false),
        (&["blights"], ScaledPlayerAction::Blight, false),
        (&["incubate"], ScaledPlayerAction::Incubate, true),
        (&["incubates"], ScaledPlayerAction::Incubate, true),
        (&["discover"], ScaledPlayerAction::Discover, true),
        (&["discovers"], ScaledPlayerAction::Discover, true),
        (&["bolster"], ScaledPlayerAction::Bolster, false),
        (&["bolsters"], ScaledPlayerAction::Bolster, false),
        (&["support"], ScaledPlayerAction::Support, false),
        (&["supports"], ScaledPlayerAction::Support, false),
        (&["fateseal"], ScaledPlayerAction::Fateseal, false),
        (&["fateseals"], ScaledPlayerAction::Fateseal, false),
        (
            &["collect", "evidence"],
            ScaledPlayerAction::CollectEvidence,
            false,
        ),
    ];
    for &(phrase, kind, repeat) in SCALED_ACTIONS {
        if ts.eat_phrase(phrase) {
            let amount = parse_value_expr(ts)?;
            let eff = mutate_player(
                actor,
                PlayerMutation::ScaledAction { kind, amount },
                Duration::Instant,
            );
            return Ok(if repeat {
                wrap_optional_repeat_times(ts, eff)
            } else {
                eff
            });
        }
    }
    const OBJECT_STATE_VERBS: &[(&[&str], ObjectStateAction, Duration)] = &[
        (
            &["flip", "flips"],
            ObjectStateAction::Flip,
            Duration::Permanent,
        ),
        (
            &["goad", "goads"],
            ObjectStateAction::Goad,
            Duration::UntilYourNextTurn,
        ),
        (
            &["detain", "detains"],
            ObjectStateAction::Detain,
            Duration::UntilYourNextTurn,
        ),
        (
            &["suspect", "suspects"],
            ObjectStateAction::Suspect,
            Duration::Permanent,
        ),
        (
            &["exert", "exerts"],
            ObjectStateAction::Exert,
            Duration::Instant,
        ),
    ];
    for (words, action, dur) in OBJECT_STATE_VERBS {
        if ts.eat_any_word(words) {
            let subject = parse_subject_ref(ts)?;
            return Ok(mutate_obj(
                subject,
                ObjectMutation::StateAction(*action),
                dur.clone(),
            ));
        }
    }
    Err(ts.err("not a keyword action verb"))
}

/// Parse a damage-redirection replacement sentence (`[All | The next <N>] [combat] damage that would be dealt ... is dealt to <dest> instead`
/// or `Excess damage is dealt to <dest> instead`).
pub(crate) fn parse_damage_redirect_replacement(
    ts: &mut TokenStream<'_>,
) -> PResult<(crate::v2::ir::Replacement, Duration)> {
    let cp = ts.checkpoint();
    // `Excess damage is dealt to <dest> instead` (`Flame Spill`, `Gandalf's Sanction`, `Ram Through`, `Ravenous Tyrannosaurus`)
    if ts.eat_phrase(&["excess", "damage", "is", "dealt", "to"])
        && let Ok(to) = try_parse(ts, parse_subject_ref)
        && ts.eat_word("instead")
    {
        return Ok((
            crate::v2::ir::Replacement::DamageRule(crate::v2::ir::DamageReplacement::RedirectAll {
                from: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatDamage),
                to,
            }),
            Duration::UntilEndOfTurn,
        ));
    }
    ts.restore(cp);

    // `[All | The next <val>] [combat] damage that would be dealt [this turn] [to <from>] [by <src>] [this turn] is dealt to <to> instead`
    // (`Shimian Night Stalker`, `Mirror Strike`, `Gideon's Sacrifice`, `Hazduhr the Abbot`, `Kor Dirge`, `Saving Grace`, `Weathered Bodyguards`)
    let next_amount = if ts.eat_phrase(&["the", "next"]) {
        match try_parse(ts, parse_value_expr) {
            Ok(v) => Some(v),
            Err(_) => {
                ts.restore(cp);
                return Err(ts.err("expected amount after `the next`"));
            }
        }
    } else {
        let _ = ts.eat_word("all");
        None
    };
    let _ = ts.eat_word("combat");
    if ts.eat_phrase(&["damage", "that", "would", "be", "dealt"])
        || ts.eat_phrase(&[
            "damage", "that", "a", "source", "of", "your", "choice", "would", "deal",
        ])
        || ts.eat_phrase(&[
            "damage", "that", "a", "source", "of", "their", "choice", "would", "deal",
        ])
    {
        let mut duration = Duration::Permanent;
        let mut from = SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatDamage);
        loop {
            if let Some(d) = parse_trailing_duration(ts) {
                duration = d;
                continue;
            }
            if ts.eat_word("to") {
                if let Ok(tgt) = try_parse(ts, parse_subject_ref) {
                    // Optional compound `and <subj2>` (`to you and permanents you control`)
                    let acp = ts.checkpoint();
                    if ts.eat_word("and")
                        && let Ok(tgt2) = try_parse(ts, parse_subject_ref)
                    {
                        from = SubjectRef::Compound(vec![tgt, tgt2]);
                    } else {
                        ts.restore(acp);
                        from = tgt;
                    }
                    continue;
                }
                break;
            }
            if ts.eat_word("by") {
                if ts.eat_phrase(&["a", "source", "of", "your", "choice"])
                    || ts.eat_phrase(&["a", "source", "of", "their", "choice"])
                    || try_parse(ts, parse_subject_ref).is_ok()
                {
                    continue;
                }
                break;
            }
            break;
        }
        if ts.eat_phrase(&["is", "dealt", "to"])
            && let Ok(to) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("instead")
        {
            let rep = match next_amount {
                Some(amount) => crate::v2::ir::Replacement::DamageRule(
                    crate::v2::ir::DamageReplacement::RedirectNext { amount, from, to },
                ),
                None => crate::v2::ir::Replacement::DamageRule(
                    crate::v2::ir::DamageReplacement::RedirectAll { from, to },
                ),
            };
            return Ok((rep, duration));
        }
    }
    ts.restore(cp);
    Err(ts.err("not a damage redirect sentence"))
}

/// Parse auxiliary top-level effect sentences without bloating `clause/mod.rs`.
pub(crate) fn parse_extra_sentence_effect(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    // 1. Damage redirection
    {
        let rcp = ts.checkpoint();
        if let Ok((replacement, duration)) = try_parse(ts, parse_damage_redirect_replacement) {
            return Ok(vec![apply_replacement(replacement, duration)]);
        }
        ts.restore(rcp);
    }
    // 2. `Its power is equal to <v1> and its toughness is equal to <v2>`
    {
        let pcp = ts.checkpoint();
        if ts.eat_phrase(&["its", "power", "is", "equal", "to"])
            && let Ok(power) = try_parse(ts, parse_value_expr)
            && ts.eat_phrase(&["and", "its", "toughness", "is", "equal", "to"])
            && let Ok(toughness) = try_parse(ts, parse_value_expr)
        {
            return Ok(vec![mutate_obj(
                SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                ObjectMutation::SetDynamicBasePT {
                    power,
                    toughness,
                    add_types: crate::ast::CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                Duration::Permanent,
            )]);
        }
        ts.restore(pcp);
    }
    // 3. `The escape/replicate cost is equal to [its | the card's] mana cost [plus <costs>]`
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["the", "escape", "cost", "is", "equal", "to"])
            && ts.eat_any_phrase(&[&["its", "mana", "cost"], &["the", "card's", "mana", "cost"]])
        {
            let mut costs = vec![crate::v2::ir::Cost::PayAnyAmountMana];
            if ts.eat_word("plus")
                && let Ok(extra) = try_parse(ts, crate::v2::cost::parse_cost_list)
            {
                costs.extend(extra);
            }
            return Ok(vec![apply_continuous(
                crate::v2::ir::StaticAbility::AlternativeCost(costs),
                Duration::UntilEndOfTurn,
            )]);
        }
        ts.restore(ecp);
        if ts.eat_any_word(&["the", "its"])
            && ts.peek_word().is_some_and(|w| {
                matches!(
                    w.to_ascii_lowercase().as_str(),
                    "replicate"
                        | "encore"
                        | "embalm"
                        | "harmonize"
                        | "unearth"
                        | "blitz"
                        | "dash"
                        | "disturb"
                        | "foretell"
                        | "plot"
                        | "warp"
                        | "madness"
                )
            })
        {
            ts.next_word();
            if ts.eat_phrase(&["cost", "is", "equal", "to", "its", "mana", "cost"]) {
                return Ok(vec![apply_continuous(
                    crate::v2::ir::StaticAbility::AlternativeCost(vec![
                        crate::v2::ir::Cost::PayAnyAmountMana,
                    ]),
                    Duration::Permanent,
                )]);
            }
        }
        ts.restore(ecp);
    }
    // 4. `copy the next <flt> you cast this turn when you cast it`
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["copy", "the", "next"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["you", "cast", "this", "turn", "when", "you", "cast", "it"])
        {
            return Ok(vec![Effect::DelayedTrigger {
                trigger: crate::v2::ir::DelayedTriggerKind::Event(crate::v2::ir::TriggerSpec {
                    events: vec![crate::v2::ir::TriggerEvent::SpellCast {
                        caster: PlayerRef::You,
                        spell: Some(flt),
                        from_outside_hand: false,
                    }],
                    intervening_if: None,
                }),
                body: vec![Effect::CopyObject {
                    actor: PlayerRef::You,
                    subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatSpell),
                    count: ValueExpr::Const(1),
                    modifiers: crate::v2::ir::EntryModifiers::default(),
                    exceptions: Vec::new(),
                }],
            }]);
        }
        ts.restore(ccp);
    }
    // 4b. `After [this | the second main] phase [this turn], there's/there is an additional combat phase [followed by an additional main phase]`
    {
        let acp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["after", "this", "phase"],
            &["after", "the", "second", "main", "phase", "this", "turn"],
            &["after", "the", "second", "main", "phase"],
        ]) && ts.eat_comma()
            && ts.eat_any_phrase(&[
                &["there", "is", "an", "additional", "combat", "phase"],
                &["there's", "an", "additional", "combat", "phase"],
            ])
        {
            let followed_by_main =
                ts.eat_phrase(&["followed", "by", "an", "additional", "main", "phase"]);
            return Ok(vec![mutate_player(
                PlayerRef::You,
                PlayerMutation::AdditionalCombat { followed_by_main },
                Duration::UntilEndOfTurn,
            )]);
        }
        ts.restore(acp);
    }
    // 5. `The next time ...`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next", "time"]) {
            // 5a. `damage would be dealt to <from> this turn, that damage is dealt to <to> instead`
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
                && let Ok(from) = try_parse(ts, parse_subject_ref)
            {
                let _ = ts.eat_phrase(&["and/or", "you"]);
                if ts.eat_phrase(&["this", "turn"])
                    && ts.eat_comma()
                    && ts.eat_phrase(&["that", "damage", "is", "dealt", "to"])
                    && let Ok(to) = try_parse(ts, parse_subject_ref)
                    && ts.eat_word("instead")
                {
                    return Ok(vec![apply_replacement(
                        crate::v2::ir::Replacement::DamageRule(
                            crate::v2::ir::DamageReplacement::RedirectNext {
                                amount: ValueExpr::Const(0),
                                from,
                                to,
                            },
                        ),
                        Duration::UntilEndOfTurn,
                    )]);
                }
            }
            ts.restore(dcp);
            // 5b. `<player> would draw a card this turn, instead <effect>`
            if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
                && ts.eat_phrase(&["would", "draw", "a", "card", "this", "turn"])
                && ts.eat_comma()
                && ts.eat_word("instead")
                && let Ok(instead) = try_parse(ts, super::parse_effect_sentence)
            {
                return Ok(vec![apply_replacement(
                    crate::v2::ir::Replacement::EventInstead {
                        event: crate::v2::ir::TriggerEvent::PlayerAction {
                            player,
                            action: crate::v2::ir::PlayerTriggerKind::CardDrawn,
                        },
                        instead,
                    },
                    Duration::UntilEndOfTurn,
                )]);
            }
            ts.restore(dcp);
            // 5b2. `<player> would roll one or more dice this turn, instead ...`
            if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
                && ts.eat_phrase(&["would", "roll", "one", "or", "more", "dice", "this", "turn"])
                && ts.eat_comma()
                && ts.eat_phrase(&[
                    "instead", "they", "roll", "that", "many", "dice", "plus", "one",
                ])
            {
                let _ = ts.eat_comma();
                let _ = ts.eat_any_word(&["then", "and"]);
                if ts.eat_phrase(&[
                    "you", "choose", "one", "of", "those", "rolls", "to", "ignore",
                ]) {
                    return Ok(vec![apply_replacement(
                        crate::v2::ir::Replacement::EventInstead {
                            event: crate::v2::ir::TriggerEvent::RolledDie { player, value: 0 },
                            instead: vec![mutate_player(
                                player,
                                PlayerMutation::RollDie { sides: 6, count: 2 },
                                Duration::Instant,
                            )],
                        },
                        Duration::UntilEndOfTurn,
                    )]);
                }
            }
            ts.restore(dcp);
            // 5b4. `one or more <flt> enter this turn, each enters with <val> additional <kind> counter(s) on it`
            if ts.eat_phrase(&["one", "or", "more"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["enter", "this", "turn"])
                && ts.eat_comma()
                && ts.eat_phrase(&["each", "enters", "with"])
                && let Ok(count) = try_parse(ts, parse_value_expr)
                && ts.eat_word("additional")
                && let Ok(kind) = try_parse(ts, crate::v2::filter::parse_counter_kind)
                && ts.eat_any_word(&["counters", "counter"])
                && ts.eat_phrase(&["on", "it"])
            {
                return Ok(vec![apply_replacement(
                    crate::v2::ir::Replacement::EntersWithCounters {
                        subject: Some(SubjectRef::Group(Box::new(flt))),
                        kind,
                        count,
                        additional: true,
                    },
                    Duration::UntilEndOfTurn,
                )]);
            }
            ts.restore(dcp);
            // 5b6. `you cast <flt> [from your hand] this turn, put that card into your hand instead of into your graveyard as it resolves`
            if ts.eat_phrase(&["you", "cast"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                let _ = ts.eat_phrase(&["from", "your", "hand"]);
                if ts.eat_phrase(&["this", "turn"])
                    && ts.eat_comma()
                    && ts.eat_phrase(&[
                        "put",
                        "that",
                        "card",
                        "into",
                        "your",
                        "hand",
                        "instead",
                        "of",
                        "into",
                        "your",
                        "graveyard",
                        "as",
                        "it",
                        "resolves",
                    ])
                {
                    return Ok(vec![apply_replacement(
                        crate::v2::ir::Replacement::EventInstead {
                            event: crate::v2::ir::TriggerEvent::SpellCast {
                                caster: PlayerRef::You,
                                spell: Some(flt),
                                from_outside_hand: false,
                            },
                            instead: vec![simple_zone_op(
                                PlayerRef::You,
                                crate::v2::ir::ZoneOpKind::Move,
                                SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatCard),
                                Vec::new(),
                                Some(simple_zone_dest(
                                    crate::v2::ir::ZoneKind::Hand,
                                    crate::v2::ir::ZoneOwner::You,
                                )),
                            )],
                        },
                        Duration::UntilEndOfTurn,
                    )]);
                }
            }
            ts.restore(dcp);
            // 5c. `<src | a source of your choice> would deal [combat] damage [to <tgt> [and/or <tgt2>]] this turn/combat, <instead>`
            let src_opt = if ts.eat_any_phrase(&[
                &["a", "source", "of", "your", "choice"],
                &["a", "source", "of", "their", "choice"],
            ]) {
                Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatSource))
            } else if let Ok(s) = try_parse(ts, parse_subject_ref) {
                let _ = ts.eat_any_phrase(&[&["of", "your", "choice"], &["of", "their", "choice"]]);
                Some(s)
            } else {
                None
            };
            if let Some(src) = src_opt
                && ts.eat_phrase(&["would", "deal"])
            {
                let combat_only = ts.eat_word("combat");
                if ts.eat_word("damage") {
                    let tgt = if ts.eat_word("to") {
                        let _ = ts.eat_phrase(&["one", "or", "more"]);
                        if let Ok(t1) = try_parse(ts, parse_subject_ref) {
                            let tcp = ts.checkpoint();
                            if ts.eat_any_word(&["and/or", "and"])
                                && let Ok(t2) = try_parse(ts, parse_subject_ref)
                            {
                                Some(SubjectRef::Compound(vec![t1, t2]))
                            } else {
                                ts.restore(tcp);
                                Some(t1)
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if ts.eat_any_phrase(&[&["this", "turn"], &["this", "combat"]])
                        && ts.eat_comma()
                    {
                        if ts.eat_phrase(&["prevent", "all", "but"])
                            && let Some(rem) =
                                ts.next_number(false).and_then(|n| u16::try_from(n).ok())
                            && ts.eat_phrase(&["of", "that", "damage"])
                        {
                            return Ok(vec![apply_replacement(
                                crate::v2::ir::Replacement::DamageRule(
                                    crate::v2::ir::DamageReplacement::PreventAllBut {
                                        source: None,
                                        target: tgt,
                                        remaining: rem,
                                    },
                                ),
                                Duration::UntilEndOfTurn,
                            )]);
                        }
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            return Ok(vec![Effect::PreventDamage {
                                amount: None,
                                combat_only,
                                source: Some(src),
                                target: tgt,
                                duration: Duration::UntilEndOfTurn,
                            }]);
                        }
                        if ts.eat_phrase(&["it", "deals", "double", "that", "damage"]) {
                            if ts.eat_word("to") {
                                let _ = try_parse(ts, parse_subject_ref);
                            }
                            if ts.eat_word("instead") {
                                return Ok(vec![apply_replacement(
                                    crate::v2::ir::Replacement::ModifyDamage {
                                        source: None,
                                        target: tgt,
                                        add: 0,
                                        factor: 2,
                                    },
                                    Duration::UntilEndOfTurn,
                                )]);
                            }
                        }
                        if ts.eat_any_phrase(&[
                            &["it", "deals", "that", "damage", "to"],
                            &["that", "spell", "deals", "that", "damage", "to"],
                        ]) && let Ok(to) = try_parse(ts, parse_subject_ref)
                            && ts.eat_word("instead")
                        {
                            return Ok(vec![apply_replacement(
                                crate::v2::ir::Replacement::DamageRule(
                                    crate::v2::ir::DamageReplacement::RedirectNext {
                                        amount: ValueExpr::Const(0),
                                        from: tgt.unwrap_or(src),
                                        to,
                                    },
                                ),
                                Duration::UntilEndOfTurn,
                            )]);
                        }
                    }
                }
            }
        }
        ts.restore(ncp);
    }
    Err(ts.err("not an auxiliary effect sentence"))
}
