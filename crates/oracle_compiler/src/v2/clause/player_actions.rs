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

pub(super) fn parse_mana_addition(ts: &mut TokenStream<'_>) -> PResult<ManaSpec> {
    let _ = ts.eat_phrase(&["an", "additional"]);
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
        if (ts.eat_phrase(&["one", "mana", "of", "any", "type", "that"])
            || ts.eat_phrase(&["one", "mana", "of", "any", "color", "that"]))
            && try_parse(ts, parse_object_filter).is_ok()
            && (ts.eat_phrase(&["could", "produce"]) || ts.eat_word("produced"))
        {
            return Ok(ManaSpec::AnyColor {
                amount: ValueExpr::Const(1),
                single_color: true,
            });
        }
        ts.restore(tcp);
    }
    if ts.eat_phrase(&[
        "one", "mana", "of", "any", "type", "that", "land", "produced",
    ]) || ts.eat_phrase(&[
        "one",
        "mana",
        "of",
        "any",
        "type",
        "that",
        "permanent",
        "produced",
    ]) || ts.eat_phrase(&[
        "one", "mana", "of", "any", "of", "the", "exiled", "cards'", "colors",
    ]) || ts.eat_phrase(&["one", "mana", "of", "any", "color"])
    {
        let _ = ts.eat_phrase(&["in", "your", "commander's", "color", "identity"]);
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
            if ts.eat_phrase(&["of", "that", "color"]) {
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
    if let Ok(times) = try_parse(ts, parse_value_expr)
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
    if ts.eat_phrase(&["take", "the", "initiative"])
        || ts.eat_phrase(&["takes", "the", "initiative"])
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::TakeInitiative),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["open", "an", "attraction"]) || ts.eat_phrase(&["opens", "an", "attraction"])
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::OpenAttraction),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["repeat", "this", "process", "any", "number", "of", "times"])
        || ts.eat_phrase(&["repeat", "this", "process", "once"])
        || ts.eat_phrase(&["repeat", "this", "process"])
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::RepeatProcessOnce),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["win", "the", "game"]) || ts.eat_phrase(&["wins", "the", "game"]) {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::WinGame),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&[
        "don't", "lose", "this", "mana", "as", "steps", "and", "phases", "end",
    ]) || ts.eat_phrase(&[
        "doesn't", "lose", "this", "mana", "as", "steps", "and", "phases", "end",
    ]) {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::KeepManaAcrossSteps),
            duration: Duration::UntilEndOfTurn,
        });
    }
    if (ts.eat_word("skip") || ts.eat_word("skips"))
        && (ts.eat_phrase(&["your", "next"]) || ts.eat_phrase(&["their", "next"]))
    {
        if ts.eat_word("turn") {
            return Ok(Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::SkipNextTurn),
                duration: Duration::Instant,
            });
        }
        if ts.eat_phrase(&["draw", "step"]) {
            return Ok(Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::SkipNextStep(crate::ast::TurnStepAst::DrawStep),
                duration: Duration::Instant,
            });
        }
        if ts.eat_phrase(&["untap", "step"]) {
            return Ok(Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::SkipNextStep(crate::ast::TurnStepAst::UntapStep),
                duration: Duration::Instant,
            });
        }
        if ts.eat_phrase(&["combat", "phase"]) {
            return Ok(Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::SkipNextStep(crate::ast::TurnStepAst::BeginningOfCombat),
                duration: Duration::Instant,
            });
        }
    }
    if ts.eat_word("flip") || ts.eat_word("flips") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Flip),
            duration: Duration::Permanent,
        });
    }
    if ts.eat_word("recruit") || ts.eat_word("recruits") {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Recruit),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("earthbend") || ts.eat_word("earthbends") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Earthbend,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("monstrosity") {
        let amt = parse_value_expr(ts)?;
        return Ok(Effect::MutateObject {
            subject: SubjectRef::SelfObject,
            mutation: ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Monstrosity,
                amount: amt,
            },
            duration: Duration::Permanent,
        });
    }
    if (ts.eat_word("empower") || ts.eat_word("empowers"))
        && let Some(w) = ts.peek_word()
        && let Some(pw) = ts.vocab.lookup_subtype(&w.to_ascii_lowercase())
    {
        let planeswalker = pw.to_string();
        ts.next_word();
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Empower {
                planeswalker,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("learn") || ts.eat_word("learns") {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Learn),
            duration: Duration::Instant,
        });
    }
    if (ts.eat_word("clash") || ts.eat_word("clashes"))
        && ts.eat_phrase(&["with", "an", "opponent"])
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Clash),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("proliferate") || ts.eat_word("proliferates") {
        let eff = Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Proliferate),
            duration: Duration::Instant,
        };
        return Ok(wrap_optional_repeat_times(ts, eff));
    }
    if ts.eat_word("investigate") || ts.eat_word("investigates") {
        let amt = if ts.eat_word("twice") {
            ValueExpr::Const(2)
        } else {
            let cp = ts.checkpoint();
            if let Ok(times) = try_parse(ts, parse_value_expr)
                && ts.eat_word("times")
            {
                times
            } else {
                ts.restore(cp);
                ValueExpr::Const(1)
            }
        };
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Investigate,
                amount: amt,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("populate") || ts.eat_word("populates") {
        let eff = Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Populate),
            duration: Duration::Instant,
        };
        return Ok(wrap_optional_repeat_times(ts, eff));
    }
    if ts.eat_word("forage") || ts.eat_word("forages") {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Forage),
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("blight") || ts.eat_word("blights") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Blight,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("behold") || ts.eat_word("beholds") {
        let flt = parse_object_filter(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Behold(flt),
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
            mutation: ObjectMutation::KeywordAction {
                kind: ObjectKeywordAction::Adapt,
                amount: ValueExpr::Const(i32::from(n)),
            },
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
        let eff = Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Incubate,
                amount,
            },
            duration: Duration::Instant,
        };
        return Ok(wrap_optional_repeat_times(ts, eff));
    }
    if ts.eat_word("discover") || ts.eat_word("discovers") {
        let amount = parse_value_expr(ts)?;
        let eff = Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Discover,
                amount,
            },
            duration: Duration::Instant,
        };
        return Ok(wrap_optional_repeat_times(ts, eff));
    }
    if ts.eat_word("bolster") || ts.eat_word("bolsters") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Bolster,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("support") || ts.eat_word("supports") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Support,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("fateseal") || ts.eat_word("fateseals") {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::Fateseal,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["manifest", "dread"]) || ts.eat_phrase(&["manifests", "dread"]) {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::ManifestDread),
            duration: Duration::Instant,
        });
    }
    if (ts.eat_word("manifest") || ts.eat_word("manifests"))
        && (ts.eat_phrase(&["the", "top", "card", "of", "your", "library"])
            || ts.eat_phrase(&["the", "top", "card", "of", "their", "library"]))
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::Manifest),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["venture", "into", "the", "dungeon"])
        || ts.eat_phrase(&["ventures", "into", "the", "dungeon"])
    {
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::Action(PlayerActionKind::VentureIntoDungeon),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["the", "ring", "tempts", "you"]) {
        return Ok(Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::Action(PlayerActionKind::RingTemptsYou),
            duration: Duration::Instant,
        });
    }
    if ts.eat_phrase(&["collect", "evidence"]) {
        let amount = parse_value_expr(ts)?;
        return Ok(Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::ScaledAction {
                kind: ScaledPlayerAction::CollectEvidence,
                amount,
            },
            duration: Duration::Instant,
        });
    }
    if ts.eat_word("goad") || ts.eat_word("goads") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Goad),
            duration: Duration::UntilYourNextTurn,
        });
    }
    if ts.eat_word("detain") || ts.eat_word("detains") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Detain),
            duration: Duration::UntilYourNextTurn,
        });
    }
    if ts.eat_word("suspect") || ts.eat_word("suspects") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Suspect),
            duration: Duration::Permanent,
        });
    }
    if ts.eat_word("exert") || ts.eat_word("exerts") {
        let subject = parse_subject_ref(ts)?;
        return Ok(Effect::MutateObject {
            subject,
            mutation: ObjectMutation::StateAction(ObjectStateAction::Exert),
            duration: Duration::Instant,
        });
    }
    Err(ts.err("not a keyword action verb"))
}
