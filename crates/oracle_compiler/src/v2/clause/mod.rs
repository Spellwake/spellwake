//! Compositional top-down effect-sentence and verb-frame clause parser for `v2`.
//!
//! Modular layout:
//! - `mod.rs`: sentence-level dispatch (BindX, VillainousChoice, Conditionals, DelayedTrigger,
//!   ScopedDuration, ForEach, Restrictions) and clause sequencing with player-actor propagation.
//! - `player_verbs.rs`: player-initiated verb frames, mana additions, damage prevention, keyword actions.
//! - `subject_verbs.rs`: object/subject-initiated verb frames and trailing duration parsing.
//! - `tokens.rs`: predefined, creature, and copy-token creation parser.

mod player_verbs;
mod subject_verbs;
mod tokens;

use crate::v2::clause::player_verbs::parse_player_verb_frame;
use crate::v2::clause::subject_verbs::parse_subject_verb_frame;
use crate::v2::condition::parse_condition;
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref};
use crate::v2::ir::{
    ActivationRestriction, ChoiceKind, Cost, DelayedTriggerKind, Duration, Effect, ForEachDomain,
    ManaSpendRule, ObjectFilter, ObjectMutation, ParseError, ParseStage, PlayerMutation, PlayerRef,
    RuleRestrictionKind, StaticAbility, SubjectRef, VarName,
};
use crate::v2::lexer::{Token, TokenKind, tokenize};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::replacement::parse_replacement;
use crate::v2::static_ability::parse_static_ability;
use crate::v2::trigger::{parse_step_with_owner, parse_trigger_header};
use crate::v2::value::parse_value_expr;
use crate::vocab::{Vocabulary, lookup_card_type};

/// Parse a multi-sentence effect body into strictly-typed `Vec<Effect>` (fail-closed).
pub fn parse_effect_body_str(
    body: &str,
    vocab: &Vocabulary,
) -> Result<Vec<Effect>, Vec<ParseError>> {
    let toks = tokenize(body).map_err(|e| vec![e])?;
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut cur_start = 0usize;
    for (idx, tok) in toks.iter().enumerate() {
        if matches!(tok.kind, TokenKind::Period) {
            if idx > cur_start {
                let mut ts =
                    TokenStream::new(body, &toks[cur_start..idx], vocab, ParseStage::Effect);
                match parse_effect_sentence(&mut ts).and_then(|res| {
                    ts.expect_eof()?;
                    Ok(res)
                }) {
                    Ok(effs) => out.extend(effs),
                    Err(e) => errors.push(e),
                }
            }
            cur_start = idx + 1;
        }
    }
    if cur_start < toks.len() {
        let mut ts = TokenStream::new(body, &toks[cur_start..], vocab, ParseStage::Effect);
        match parse_effect_sentence(&mut ts).and_then(|res| {
            ts.expect_eof()?;
            Ok(res)
        }) {
            Ok(effs) => out.extend(effs),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Parse one complete effect sentence from a [`TokenStream`] (strictly fail-closed).
pub fn parse_effect_sentence(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    // Strip trailing period on slice boundaries if present
    let end_trim = if matches!(ts.tokens.last().map(|t| &t.kind), Some(TokenKind::Period)) {
        ts.tokens.len() - 1
    } else {
        ts.tokens.len()
    };
    let mut ts_inner = ts.sub_stream(&ts.tokens[ts.pos..end_trim], ParseStage::Effect);
    let effects = parse_sentence_top(&mut ts_inner)?;
    ts_inner.expect_eof()?;
    ts.pos = ts.tokens.len();
    Ok(effects)
}

fn parse_sentence_top(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    // Strip optional leading `Then [,]` sequential connector (`Then draw a card.`, `Then if ...`)
    if ts.eat_word("then") {
        let _ = ts.eat_comma();
    }

    // 0. Trailing `, where X is <ValueExpr>` binder (split cleanly on top-level `, where X is`)
    if let Some(where_idx) = find_where_x_is(ts.tokens, ts.pos) {
        let body_slice = &ts.tokens[ts.pos..where_idx];
        let expr_slice = &ts.tokens[where_idx + 4..];
        let mut expr_ts = ts.sub_stream(expr_slice, ParseStage::Effect);
        let x_expr = parse_value_expr(&mut expr_ts)?;
        expr_ts.expect_eof()?;

        let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
        let body = parse_sentence_top(&mut body_ts)?;
        body_ts.expect_eof()?;

        ts.pos = ts.tokens.len();
        return Ok(vec![Effect::BindVariable {
            var: VarName::X,
            expr: x_expr,
            body,
        }]);
    }

    // 1. Villainous choice frame: `<player> face(s) a villainous choice — <opt1>, or <opt2>`
    //    Handled BEFORE any `, then` or `and` splitting so `, then ...` stays strictly inside Option 2!
    if let Some(vc) = opt(ts, parse_villainous_choice) {
        return Ok(vec![vc]);
    }

    // 2. Leading `When you do, <effect>` (reflexive trigger) OR `Otherwise, <effect>` OR `If <player> do/does/don't/doesn't, <effect>`
    if ts.eat_phrase(&["when", "you", "do"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ReflexiveTrigger(body)]);
    }
    if ts.eat_word("otherwise") && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::IfPrevious {
            succeeded: false,
            body,
        }]);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_word("if") && opt(ts, parse_player_ref).is_some() {
            let succeeded = if ts.eat_word("do") || ts.eat_word("does") {
                Some(true)
            } else if ts.eat_word("don't") || ts.eat_word("doesn't") {
                Some(false)
            } else {
                None
            };
            if let Some(ok) = succeeded
                && ts.eat_comma()
            {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::IfPrevious {
                    succeeded: ok,
                    body,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 3. Mana spend rider: `If that mana is spent to cast <spell_flt>, <rider>`
    if ts.eat_phrase(&["if", "that", "mana", "is", "spent", "to", "cast"]) {
        let spell_filter = parse_object_filter(ts)?;
        if !ts.eat_comma() {
            return Err(ts.err("expected `,` after mana spend rider condition"));
        }
        let rider = parse_sentence_top(ts)?;
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::AddManaSpendRider {
                spell_filter,
                rider,
            },
            duration: Duration::UntilEndOfTurn,
        }]);
    }

    // 4. Leading conditional: `If <cond>, <then_body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(cond) = try_parse(ts, parse_condition)
            && ts.eat_comma()
        {
            let then_body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::BranchCondition {
                condition: cond,
                then_body,
                else_body: Vec::new(),
            }]);
        }
        ts.restore(cp);
    }

    // 5. Delayed trigger prefix: `At the beginning of <step>, <body>` or `When <event>, <body>`
    {
        let cp = ts.checkpoint();
        if ts.peek_phrase(&["at", "the", "beginning", "of"])
            && let Ok((step, owner)) = try_parse(ts, |s| {
                s.eat_phrase(&["at", "the", "beginning", "of"]);
                parse_step_with_owner(s)
            })
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Step { step, owner },
                body,
            }]);
        }
        ts.restore(cp);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["when", "you", "next", "cast"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::DelayedTrigger {
                    trigger: DelayedTriggerKind::Event(crate::v2::ir::TriggerSpec {
                        events: vec![crate::v2::ir::TriggerEvent::SpellCast {
                            caster: PlayerRef::You,
                            spell: Some(flt),
                            from_outside_hand: false,
                        }],
                        intervening_if: None,
                    }),
                    body,
                }]);
            }
        }
        ts.restore(cp);
    }
    {
        let cp = ts.checkpoint();
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("when") || w.eq_ignore_ascii_case("whenever"))
            && let Ok(trig) = try_parse(ts, parse_trigger_header)
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::DelayedTrigger {
                    trigger: DelayedTriggerKind::Event(trig),
                    body,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 6. Scoped leading duration: `As long as <cond>, <body>` / `Until end of turn, <body>` / `Until the end of your next turn, <body>` / `This turn and next turn, <body>`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["as", "long", "as"])
            && let Ok(cond) = try_parse(ts, parse_condition)
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::ScopedDuration {
                duration: Duration::ForAsLongAs(Box::new(cond)),
                body,
            }]);
        }
        ts.restore(acp);
    }
    if (ts.eat_phrase(&["this", "turn", "and", "next", "turn"])
        || ts.eat_phrase(&["until", "the", "end", "of", "your", "next", "turn"]))
        && ts.eat_comma()
    {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ScopedDuration {
            duration: Duration::UntilEndOfNextTurn,
            body,
        }]);
    }
    if ts.eat_phrase(&["until", "your", "next", "turn"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ScopedDuration {
            duration: Duration::UntilYourNextTurn,
            body,
        }]);
    }
    if ts.eat_phrase(&["until", "end", "of", "turn"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ScopedDuration {
            duration: Duration::UntilEndOfTurn,
            body,
        }]);
    }
    if ts.eat_phrase(&["this", "turn"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ScopedDuration {
            duration: Duration::UntilEndOfTurn,
            body,
        }]);
    }

    // 6b. Leading `Starting with <player>, <body>`
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["starting", "with"])
            && let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::StartingWith { player, body }]);
        }
        ts.restore(scp);
    }

    // 7. Leading `For each graveyard with <flt> in it, <body>` / `For each of those <flt>, <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each", "graveyard", "with"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["in", "it"])
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::ForEach {
                over: ForEachDomain::GraveyardWith(Box::new(flt)),
                body,
            }]);
        }
        ts.restore(cp);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each", "of", "those"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::ForEach {
                over: ForEachDomain::Object(Box::new(flt)),
                body,
            }]);
        }
        ts.restore(cp);
    }
    // General `For each <player who can't | player | filter>, <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each"]) {
            if ts.eat_phrase(&["opponent", "who", "can't"]) && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachPlayerWhoCant(PlayerRef::EachOpponent),
                    body,
                }]);
            }
            if ts.eat_phrase(&["player", "who", "can't"]) && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachPlayerWhoCant(PlayerRef::EachPlayer),
                    body,
                }]);
            }
            if (ts.eat_word("opponent") || ts.eat_word("player")) && ts.eat_comma() {
                let p = PlayerRef::EachPlayer;
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::Player(p),
                    body,
                }]);
            }
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_comma()
            {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::Object(Box::new(flt)),
                    body,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 7b. `It's still a <type>` (`It's still a land`, `They're still lands`)
    if (ts.eat_phrase(&["it's", "still", "a"])
        || ts.eat_phrase(&["it's", "still", "an"])
        || ts.eat_phrase(&["they're", "still"]))
        && let Some(w) = ts.peek_word()
        && let Some(ct) = lookup_card_type(&w.to_ascii_lowercase())
    {
        ts.next_word();
        return Ok(vec![Effect::MutateObject {
            subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
            mutation: ObjectMutation::RetainTypes(ct),
            duration: Duration::Permanent,
        }]);
    }

    // 7c. `This ability costs {M} less/more to activate [for each <expr>]` / Class `Level <N>`
    if ts.eat_word("level")
        && let Some(lvl) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::ClassLevelHeader(lvl)),
            duration: Duration::Permanent,
        }]);
    }
    if ts.peek_phrase(&["this", "ability", "costs"])
        && let Ok(st) = try_parse(ts, parse_static_ability)
    {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(st),
            duration: Duration::Permanent,
        }]);
    }

    // 8. Activation / trigger / cast restrictions (`Activate only ...`, `This ability triggers only once each turn`, `Cast this spell only ...`)
    if ts.eat_phrase(&[
        "this", "ability", "triggers", "only", "once", "each", "turn",
    ]) {
        return Ok(vec![Effect::Restriction(vec![
            ActivationRestriction::OnceEachTurn,
        ])]);
    }
    {
        let acp = ts.checkpoint();
        let _ = ts.eat_word("only");
        if let Ok(player) = try_parse(ts, parse_player_ref)
            && ts.eat_phrase(&["may", "activate", "this", "ability"])
        {
            return Ok(vec![Effect::Restriction(vec![
                ActivationRestriction::AllowedPlayer(player),
            ])]);
        }
        ts.restore(acp);
    }
    if let Some(restr) = opt(ts, parse_activation_restrictions) {
        return Ok(vec![Effect::Restriction(restr)]);
    }

    // 9. Trailing `instead` replacement inside a spell/ability (`... instead`)
    if ts.tokens.len() > ts.pos
        && matches!(
            ts.tokens[ts.tokens.len() - 1].kind,
            TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        )
    {
        let inner_slice = &ts.tokens[ts.pos..ts.tokens.len() - 1];
        let mut inner_ts = ts.sub_stream(inner_slice, ParseStage::Effect);
        if let Ok(inner) = parse_clause_seq(&mut inner_ts, None)
            && inner_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(vec![Effect::ReplacePrevious(inner)]);
        }
    }

    // 10. Trailing `if <cond>` on a sentence (`draw two cards if you control a Demon`)
    if let Some(if_idx) = find_top_level_word(ts.tokens, ts.pos, "if")
        && if_idx > ts.pos
    {
        let body_slice = &ts.tokens[ts.pos..if_idx];
        let cond_slice = &ts.tokens[if_idx + 1..];
        let mut cond_ts = ts.sub_stream(cond_slice, ParseStage::Effect);
        if let Ok(cond) = parse_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
            if let Ok(body) = parse_clause_seq(&mut body_ts, None)
                && body_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(vec![Effect::BranchCondition {
                    condition: cond,
                    then_body: body,
                    else_body: Vec::new(),
                }]);
            }
        }
    }

    // 10b. Trailing `unless <player> [pays] <cost>` / `unless <cond>` on any sentence
    if let Some(u_idx) = find_top_level_word(ts.tokens, ts.pos, "unless")
        && u_idx > ts.pos
    {
        let body_slice = &ts.tokens[ts.pos..u_idx];
        let tail_slice = &ts.tokens[u_idx + 1..];
        let mut tail_ts = ts.sub_stream(tail_slice, ParseStage::Effect);
        if let Ok(payer) = try_parse(&mut tail_ts, parse_player_ref) {
            let pcp = tail_ts.checkpoint();
            let parsed_cost = if let Ok(c) = try_parse(&mut tail_ts, parse_cost_atom)
                && tail_ts.is_eof()
            {
                Some(c)
            } else {
                tail_ts.restore(pcp);
                let _ = tail_ts.eat_word("pays") || tail_ts.eat_word("pay");
                if let Ok(c) = try_parse(&mut tail_ts, parse_cost_atom)
                    && tail_ts.is_eof()
                {
                    Some(c)
                } else {
                    tail_ts.restore(pcp);
                    if (tail_ts.eat_word("pays") || tail_ts.eat_word("pay"))
                        && let Ok(amt) = try_parse(&mut tail_ts, parse_value_expr)
                        && tail_ts.eat_word("life")
                        && tail_ts.is_eof()
                    {
                        Some(Cost::PayLife(amt))
                    } else {
                        None
                    }
                }
            };
            if let Some(cost) = parsed_cost {
                let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
                if let Ok(body) = parse_clause_seq(&mut body_ts, None)
                    && body_ts.is_eof()
                {
                    ts.pos = ts.tokens.len();
                    return Ok(vec![Effect::Unless {
                        body,
                        payer: Some(payer),
                        cost: Some(cost),
                        condition: None,
                    }]);
                }
            }
        }
        let mut cond_ts = ts.sub_stream(tail_slice, ParseStage::Effect);
        if let Ok(cond) = parse_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
            if let Ok(body) = parse_clause_seq(&mut body_ts, None)
                && body_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(vec![Effect::Unless {
                    body,
                    payer: None,
                    cost: None,
                    condition: Some(cond),
                }]);
            }
        }
    }

    // 11. Trailing `at the beginning of <step>` / `at end of combat` delayed suffix on a sentence
    if let Some(eoc_idx) = find_phrase_index(ts.tokens, ts.pos, &["at", "end", "of", "combat"])
        && eoc_idx > ts.pos
        && eoc_idx + 4 == ts.tokens.len()
    {
        let body_slice = &ts.tokens[ts.pos..eoc_idx];
        let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
        if let Ok(body) = parse_clause_seq(&mut body_ts, None)
            && body_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(vec![Effect::ScopedDuration {
                duration: Duration::UntilEndOfCombat,
                body,
            }]);
        }
    }
    if let Some(at_idx) = find_phrase_index(ts.tokens, ts.pos, &["at", "the", "beginning", "of"])
        && at_idx > ts.pos
    {
        let body_slice = &ts.tokens[ts.pos..at_idx];
        let step_slice = &ts.tokens[at_idx + 4..];
        let mut step_ts = ts.sub_stream(step_slice, ParseStage::Effect);
        if let Ok((step, owner)) = parse_step_with_owner(&mut step_ts)
            && step_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
            if let Ok(body) = parse_clause_seq(&mut body_ts, None)
                && body_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(vec![Effect::DelayedTrigger {
                    trigger: DelayedTriggerKind::Step { step, owner },
                    body,
                }]);
            }
        }
    }

    // 11b. Binary choice `<opt1> or <opt2>` (`pay 4 life or put the card on top of your library`)
    if let Some(or_idx) = find_top_level_word(ts.tokens, ts.pos, "or")
        && or_idx > ts.pos
    {
        let left_slice = &ts.tokens[ts.pos..or_idx];
        let right_slice = &ts.tokens[or_idx + 1..];
        let mut l_ts = ts.sub_stream(left_slice, ParseStage::Effect);
        let mut r_ts = ts.sub_stream(right_slice, ParseStage::Effect);
        if let Ok(left_effs) = parse_clause_seq(&mut l_ts, None)
            && l_ts.is_eof()
            && let Ok(right_effs) = parse_clause_seq(&mut r_ts, None)
            && r_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(vec![Effect::PromptChoice {
                chooser: PlayerRef::You,
                kind: ChoiceKind::ChooseBranch(vec![left_effs, right_effs]),
            }]);
        }
    }

    // 12. Clause sequence with automatic player-actor propagation!
    parse_clause_seq(ts, None)
}

fn parse_villainous_choice(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let chooser = parse_player_ref(ts)?;
    if !(ts.eat_phrase(&["faces", "a", "villainous", "choice"])
        || ts.eat_phrase(&["face", "a", "villainous", "choice"]))
    {
        return Err(ts.err("expected `face(s) a villainous choice`"));
    }
    if !ts.eat_em_dash() {
        return Err(ts.err("expected em dash after `villainous choice`"));
    }
    // Split remaining tokens on `, or ` so BOTH options retain internal `, then` and `and` clauses!
    let or_idx = find_comma_or(ts.tokens, ts.pos)
        .ok_or_else(|| ts.err("expected `, or` separating villainous choice options"))?;
    let opt1_slice = &ts.tokens[ts.pos..or_idx];
    let opt2_slice = &ts.tokens[or_idx + 2..];

    let mut ts1 = ts.sub_stream(opt1_slice, ParseStage::Effect);
    let b1 = parse_sentence_top(&mut ts1)?;
    ts1.expect_eof()?;

    let mut ts2 = ts.sub_stream(opt2_slice, ParseStage::Effect);
    let b2 = parse_sentence_top(&mut ts2)?;
    ts2.expect_eof()?;

    ts.pos = ts.tokens.len();
    Ok(Effect::PromptChoice {
        chooser,
        kind: ChoiceKind::VillainousChoice {
            options: vec![b1, b2],
        },
    })
}

/// Parse a sequence of clauses joined by `, then`, `, and`, or `and`, propagating any explicit
/// non-`You` leading player actor forward to subsequent bare-verb clauses!
fn parse_clause_seq(
    ts: &mut TokenStream<'_>,
    inherited_actor: Option<PlayerRef>,
) -> PResult<Vec<Effect>> {
    let (mut effects, mut active_actor) = parse_single_clause(ts, inherited_actor)?;

    loop {
        if ts.is_eof() {
            break;
        }
        let cp = ts.checkpoint();
        let sep = if ts.eat_comma() {
            let _ = ts.eat_word("then") || ts.eat_word("and");
            true
        } else {
            ts.eat_word("and")
        };
        if !sep {
            ts.restore(cp);
            break;
        }
        // Optional `then` after `and`
        let _ = ts.eat_word("then");
        match try_parse(ts, |s| parse_single_clause(s, active_actor)) {
            Ok((next_effs, next_actor)) => {
                effects.extend(next_effs);
                if next_actor.is_some() {
                    active_actor = next_actor;
                }
            }
            Err(_) => {
                ts.restore(cp);
                break;
            }
        }
    }

    Ok(effects)
}

/// Parse one atomic clause, returning `(Vec<Effect>, Option<PlayerRef>)` where the second element
/// is the clause's explicit player actor (for propagation across `and` / `, then`).
fn parse_single_clause(
    ts: &mut TokenStream<'_>,
    inherited_actor: Option<PlayerRef>,
) -> PResult<(Vec<Effect>, Option<PlayerRef>)> {
    // A. Spend restriction (`Spend this mana only to cast <flt> [from <zone>]`)
    if ts.eat_phrase(&["spend", "this", "mana", "only", "to", "cast"]) {
        if ts.eat_phrase(&["spells", "from"]) {
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::AddManaSpendRestriction(ManaSpendRule {
                        only_for: true,
                        cast_from_zone: Some(zref.zone),
                        spell_filter: None,
                    }),
                    duration: Duration::Instant,
                }],
                Some(PlayerRef::You),
            ));
        }
        let flt = parse_object_filter(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: PlayerRef::You,
                mutation: PlayerMutation::AddManaSpendRestriction(ManaSpendRule {
                    only_for: true,
                    cast_from_zone: None,
                    spell_filter: Some(flt),
                }),
                duration: Duration::Instant,
            }],
            Some(PlayerRef::You),
        ));
    }

    // B. Standalone additional combat phase (`After this phase, there is an additional combat phase` /
    //    `there is an additional combat phase after this phase`)
    if let Some(ac) = opt(ts, parse_additional_combat) {
        return Ok((vec![ac], inherited_actor));
    }

    // C. Replacement sentence embedded inside a spell (`The next Construct spell you cast this turn enters with ...`)
    if let Ok(rep) = try_parse(ts, parse_replacement) {
        return Ok((
            vec![Effect::ApplyReplacement {
                replacement: Box::new(rep),
                duration: Duration::UntilEndOfTurn,
            }],
            inherited_actor,
        ));
    }

    // D. Static sentence embedded inside a spell (`The next enchantment spell you cast this turn can't be countered`,
    //    `creatures can't attack`, `players and permanents can't be the targets of ...`)
    if let Some(st_eff) = opt(ts, parse_temporary_static_clause) {
        return Ok((vec![st_eff], inherited_actor));
    }

    // E. Check for explicit leading player actor (`Each player`, `Each opponent`, `They`, `That player`,
    //    `Target opponent`, `An opponent`, `You`, `The player with the lowest life total`,
    //    `Each opponent dealt combat damage this game by <flt>`)
    {
        let cp = ts.checkpoint();
        // `Each opponent dealt combat damage this game by <flt> <action>`
        if let Ok(SubjectRef::PlayersDealtCombatDamageBy {
            player,
            source,
            this_game,
        }) = try_parse(ts, parse_subject_ref)
        {
            let (body, _) = parse_player_verb_frame(ts, PlayerRef::ThatPlayer)?;
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::PlayersMatching(Box::new(
                        SubjectRef::PlayersDealtCombatDamageBy {
                            player,
                            source,
                            this_game,
                        },
                    )),
                    body,
                }],
                Some(player),
            ));
        }
        ts.restore(cp);
    }

    // Explicit player prefix + optional `may` + verb frame
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("may") {
                // `Each player may exile their hand and draw ...` or `You may ...`
                let (mut body, _) = parse_player_verb_frame(ts, player)?;
                // Consume chained `and <verb>` INSIDE `Optional` when it's `may <v1> and <v2>`!
                let and_cp = ts.checkpoint();
                if ts.eat_word("and") {
                    if let Ok((more, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player)) {
                        body.extend(more);
                    } else {
                        ts.restore(and_cp);
                    }
                }
                return Ok((
                    vec![Effect::Optional {
                        actor: player,
                        body,
                    }],
                    Some(player),
                ));
            }
            if let Ok((effs, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player)) {
                return Ok((effs, Some(player)));
            }
        }
        ts.restore(cp);
    }

    // F. Bare verb frame (uses `inherited_actor.unwrap_or(PlayerRef::You)`)
    let actor = inherited_actor.unwrap_or(PlayerRef::You);
    if let Ok((effs, _)) = try_parse(ts, |s| parse_player_verb_frame(s, actor)) {
        return Ok((effs, inherited_actor));
    }

    // G. Subject-headed object verb frames:
    //    `<src> deals N damage to <tgt>`, `<subj> gets +X/+Y ...`, `<subj> gains <kws>`,
    //    `<subj> becomes ...`, `<subj> is goaded ...`, `<subj> can't ...`
    let effs = parse_subject_verb_frame(ts)?;
    Ok((effs, inherited_actor))
}

fn parse_additional_combat(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let _ = ts.eat_phrase(&["after", "this", "phase"])
        || ts.eat_phrase(&["after", "this", "main", "phase"])
        || ts.eat_phrase(&["after", "this", "combat", "phase"]);
    let _ = ts.eat_comma();
    if ts.eat_phrase(&["there", "is", "an", "additional", "combat", "phase"])
        || ts.eat_phrase(&["there's", "an", "additional", "combat", "phase"])
    {
        let _ = ts.eat_phrase(&["after", "this", "phase"]);
        let followed_by_main =
            ts.eat_phrase(&["followed", "by", "an", "additional", "main", "phase"]);
        return Ok(Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::AdditionalCombat { followed_by_main },
            duration: Duration::UntilEndOfTurn,
        });
    }
    Err(ts.err("expected additional combat phase clause"))
}

fn parse_temporary_static_clause(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    // `players and permanents can't be the targets of spells or activated abilities`
    if ts.eat_phrase(&[
        "players",
        "and",
        "permanents",
        "can't",
        "be",
        "the",
        "targets",
        "of",
    ]) {
        let src = parse_object_filter(ts)?;
        return Ok(Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::Compound(vec![
                    SubjectRef::Player(PlayerRef::EachPlayer),
                    SubjectRef::All(Box::new(ObjectFilter {
                        any_permanent: true,
                        ..Default::default()
                    })),
                ])),
                rule: RuleRestrictionKind::CantBeTargetedBy(Box::new(src)),
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        });
    }
    // `The next enchantment spell you cast this turn can't be countered`
    if ts.peek_phrase(&["the", "next"]) {
        let st = parse_static_ability(ts)?;
        return Ok(Effect::ApplyContinuous {
            effect: Box::new(st),
            duration: Duration::UntilEndOfTurn,
        });
    }
    Err(ts.err("not a temporary static clause"))
}

fn parse_activation_restrictions(ts: &mut TokenStream<'_>) -> PResult<Vec<ActivationRestriction>> {
    let valid_head = ts.eat_phrase(&["activate", "only"])
        || ts.eat_phrase(&["cast", "this", "spell", "only"])
        || {
            let cp = ts.checkpoint();
            if ts.eat_word("cast") && ts.eat_tilde() && ts.eat_word("only") {
                true
            } else {
                ts.restore(cp);
                false
            }
        };
    if !valid_head {
        return Err(ts.err("expected `Activate only` or `Cast ... only`"));
    }
    let mut out = Vec::new();
    loop {
        if ts.eat_phrase(&["as", "a", "sorcery"]) {
            out.push(ActivationRestriction::AsSorcery);
        } else if ts.eat_phrase(&["as", "an", "instant"]) {
            out.push(ActivationRestriction::AsInstant);
        } else if ts.eat_phrase(&["during", "your", "upkeep"]) {
            out.push(ActivationRestriction::DuringYourUpkeep);
        } else if ts.eat_phrase(&["during", "your", "turn"]) {
            out.push(ActivationRestriction::DuringYourTurn);
            if ts.eat_comma() && ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
                out.push(ActivationRestriction::BeforeAttackers);
            }
        } else if ts.eat_phrase(&["during", "combat"]) {
            out.push(ActivationRestriction::DuringCombat);
        } else if ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
            out.push(ActivationRestriction::BeforeAttackers);
        } else if ts.eat_phrase(&["once", "each", "turn"]) {
            out.push(ActivationRestriction::OnceEachTurn);
        } else if ts.eat_word("once") {
            out.push(ActivationRestriction::Once);
        } else if ts.eat_word("if") {
            let cond = parse_condition(ts)?;
            out.push(ActivationRestriction::OnlyIf(Box::new(cond)));
        } else {
            return Err(ts.err("expected timing restriction after `Activate only`"));
        }
        if ts.eat_phrase(&["and", "only"]) || ts.eat_word("and") {
            continue;
        }
        break;
    }
    Ok(out)
}

fn find_where_x_is(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    if tokens.len() < start + 5 {
        return None;
    }
    for i in start..tokens.len().saturating_sub(4) {
        if matches!(tokens[i].kind, TokenKind::Comma)
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("where"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x"))
            && matches!(tokens[i + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("is"))
        {
            return Some(i);
        }
    }
    None
}

fn find_comma_or(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    for i in start..tokens.len().saturating_sub(1) {
        if matches!(tokens[i].kind, TokenKind::Comma)
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("or"))
        {
            return Some(i);
        }
    }
    None
}

fn find_top_level_word(tokens: &[Token<'_>], start: usize, word: &str) -> Option<usize> {
    for (i, tok) in tokens.iter().enumerate().skip(start) {
        if matches!(tok.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case(word)) {
            return Some(i);
        }
    }
    None
}

fn find_phrase_index(tokens: &[Token<'_>], start: usize, words: &[&str]) -> Option<usize> {
    if words.is_empty() || tokens.len() < start + words.len() {
        return None;
    }
    for i in start..=tokens.len() - words.len() {
        let matched = words.iter().enumerate().all(|(off, &expected)| {
            matches!(tokens[i + off].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case(expected))
        });
        if matched {
            return Some(i);
        }
    }
    None
}
