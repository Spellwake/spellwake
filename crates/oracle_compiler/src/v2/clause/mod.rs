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
pub(crate) mod tokens;
mod zone_verbs;

use crate::v2::clause::player_verbs::parse_player_verb_frame;
use crate::v2::clause::subject_verbs::parse_subject_verb_frame;
use crate::v2::condition::parse_condition;
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref};
use crate::v2::ir::{
    ActivationRestriction, ChoiceKind, Cost, DelayedTriggerKind, Duration, Effect, ForEachDomain,
    LevelMarker, ManaRuleKind, ManaSpendRule, ObjectFilter, ObjectMutation, ParseError, ParseStage,
    PlayerActionKind, PlayerMutation, PlayerRef, RuleRestrictionKind, StaticAbility, SubjectRef,
    SubjectStaticRule, VarName,
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
        let is_period = matches!(tok.kind, TokenKind::Period);
        let is_sentence_ending_quote = matches!(tok.kind, TokenKind::Quoted(q) if q.trim_end().ends_with('.'))
            && toks.get(idx + 1).is_some_and(|next| {
                matches!(
                    next.kind,
                    TokenKind::Word(w)
                        if w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                            && matches!(
                                w,
                                "It's"
                                    | "They're"
                                    | "Activate"
                                    | "Do"
                                    | "This"
                                    | "Exile"
                                    | "Sacrifice"
                                    | "Return"
                                    | "Draw"
                                    | "Put"
                                    | "You"
                                    | "Each"
                                    | "If"
                                    | "When"
                                    | "Whenever"
                                    | "At"
                                    | "Until"
                                    | "Then"
                                    | "Otherwise"
                                    | "The"
                                    | "Cast"
                                    | "Empower"
                            )
                )
            });
        if is_period || is_sentence_ending_quote {
            let end_idx = if is_period { idx } else { idx + 1 };
            if end_idx > cur_start {
                let mut ts =
                    TokenStream::new(body, &toks[cur_start..end_idx], vocab, ParseStage::Effect);
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
    // Strip optional leading `Then [,]` or `Instead [,]` sequential/replacement connector
    if ts.eat_word("then") {
        let _ = ts.eat_comma();
    }
    if ts.eat_word("instead") {
        let _ = ts.eat_comma();
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ReplacePrevious(body)]);
    }
    {
        let xcp = ts.checkpoint();
        if ts.eat_phrase(&["x", "can't", "be"]) {
            if ts.next_number(false) == Some(0) {
                return Ok(vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::Action(PlayerActionKind::XCantBeZero),
                    duration: Duration::Instant,
                }]);
            }
            if ts.eat_phrase(&["greater", "than"])
                && let Ok(bound) = try_parse(ts, parse_value_expr)
            {
                return Ok(vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::XMaxBound(bound),
                    duration: Duration::Instant,
                }]);
            }
        }
        ts.restore(xcp);
    }
    {
        let xcp = ts.checkpoint();
        if ts.eat_phrase(&["x", "is"])
            && let Ok(expr) = try_parse(ts, parse_value_expr)
            && ts.is_eof()
        {
            return Ok(vec![Effect::BindVariable {
                var: VarName::X,
                expr,
                body: Vec::new(),
            }]);
        }
        ts.restore(xcp);
    }
    if ts.eat_phrase(&["the", "game", "is", "a", "draw"]) {
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::GameIsDraw,
            duration: Duration::Instant,
        }]);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_word("the")
            && ts
                .next_quoted()
                .is_some_and(|q| q.eq_ignore_ascii_case("legend rule"))
            && ts.eat_phrase(&["doesn't", "apply"])
        {
            let filter = if ts.eat_word("to") {
                if ts.eat_word("them") || ts.eat_word("it") {
                    None
                } else {
                    opt(ts, parse_object_filter)
                }
            } else {
                None
            };
            let dur = crate::v2::clause::subject_verbs::parse_trailing_duration(ts)
                .unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::LegendRuleDoesntApply(filter)),
                duration: dur,
            }]);
        }
        ts.restore(lcp);
    }
    if ts.eat_phrase(&[
        "you", "may", "choose", "the", "same", "mode", "more", "than", "once",
    ]) {
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::Action(PlayerActionKind::ChooseSameModeMultiple),
            duration: Duration::Instant,
        }]);
    }
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["spend", "only"])
            && let Some(w) = ts.peek_word()
            && let Some(color) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&["mana", "on", "x"]) {
                return Ok(vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::ManaRule(ManaRuleKind::SpendOnlyColorOnX(color)),
                    duration: Duration::Instant,
                }]);
            }
        }
        ts.restore(scp);
    }
    if ts.eat_phrase(&["do", "this", "only", "once", "each", "turn"]) {
        return Ok(vec![Effect::Restriction(vec![
            ActivationRestriction::OnceEachTurn,
        ])]);
    }
    if ts.eat_phrase(&[
        "the",
        "flashback",
        "cost",
        "is",
        "equal",
        "to",
        "its",
        "mana",
        "cost",
    ]) {
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::Action(PlayerActionKind::FlashbackCostEqualsManaCost),
            duration: Duration::UntilEndOfTurn,
        }]);
    }

    // 0. `, where X is <ValueExpr>` binder (supports BOTH trailing and mid-sentence `, where X is <expr>, <rest>`)
    if let Some(where_idx) = find_where_x_is(ts.tokens, ts.pos) {
        let body_slice = &ts.tokens[ts.pos..where_idx];
        let after_where = &ts.tokens[where_idx + 4..];
        let mut expr_ts = ts.sub_stream(after_where, ParseStage::Effect);
        if let Ok(x_expr) = parse_value_expr(&mut expr_ts) {
            if expr_ts.is_eof() {
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
            if expr_ts.eat_comma() && !expr_ts.is_eof() {
                let mut combined = Vec::with_capacity(body_slice.len() + 1 + after_where.len());
                combined.extend_from_slice(body_slice);
                combined.push(after_where[expr_ts.pos - 1].clone());
                combined.extend_from_slice(&after_where[expr_ts.pos..]);
                let mut body_ts = ts.sub_stream(&combined, ParseStage::Effect);
                if let Ok(body) = parse_sentence_top(&mut body_ts)
                    && body_ts.is_eof()
                {
                    ts.pos = ts.tokens.len();
                    return Ok(vec![Effect::BindVariable {
                        var: VarName::X,
                        expr: x_expr,
                        body,
                    }]);
                }
            }
        }
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
        let icp = ts.checkpoint();
        if (ts.eat_phrase(&["if", "no", "one", "does"])
            || ts.eat_phrase(&["if", "no", "player", "does"])
            || ts.eat_phrase(&["if", "you", "don't", "cast", "it"])
            || ts.eat_phrase(&["if", "you", "don't", "play", "it"])
            || ts.eat_phrase(&["if", "you", "didn't", "cast", "it"])
            || ts.eat_phrase(&["if", "you", "didn't", "play", "it"])
            || ts.eat_phrase(&["if", "you", "didn't", "create", "a", "token", "this", "way"])
            || ts.eat_phrase(&["if", "you", "don't", "draw", "a", "card", "this", "way"]))
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::IfPrevious {
                succeeded: false,
                body,
            }]);
        }
        ts.restore(icp);
        if ts.eat_phrase(&["if", "it", "doesn't"]) && ts.eat_comma() {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::IfPrevious {
                succeeded: false,
                body,
            }]);
        }
        ts.restore(icp);
        if ts.eat_phrase(&["if", "it", "does"]) && ts.eat_comma() {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::IfPrevious {
                succeeded: true,
                body,
            }]);
        }
        ts.restore(icp);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_word("if") && opt(ts, parse_player_ref).is_some() {
            let succeeded = if ts.eat_word("do") || ts.eat_word("does") {
                Some(true)
            } else if ts.eat_word("don't")
                || ts.eat_word("doesn't")
                || ts.eat_word("can't")
                || ts.eat_word("didn't")
            {
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

    // 3. Mana spend rider: `If that mana is spent [to cast | on] <spell_flt>, <rider>` OR `When you spend this mana to cast <spell_flt>, <rider>`
    if ts.eat_phrase(&["if", "that", "mana", "is", "spent", "to", "cast"])
        || ts.eat_phrase(&["if", "that", "mana", "is", "spent", "on"])
        || ts.eat_phrase(&["when", "you", "spend", "this", "mana", "to", "cast"])
    {
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
    // 3b. Reflexive action trigger inside an effect body (`When you sacrifice/discard/exile/cast <...> [this way], <body>`)
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["when", "you"])
            && (ts.eat_word("sacrifice")
                || ts.eat_word("discard")
                || ts.eat_word("exile")
                || ts.eat_word("cast")
                || ts.eat_word("pay")
                || ts.eat_phrase(&["lose", "control", "of"]))
        {
            let _ = ts.eat_phrase(&["one", "or", "more"]);
            let _ = opt(ts, parse_subject_ref);
            let _ = ts.eat_phrase(&["this", "way"])
                || ts.eat_phrase(&["this", "turn"])
                || ts.eat_phrase(&["from", "your", "hand"]);
            if ts.eat_comma()
                && let Ok(body) = try_parse(ts, parse_sentence_top)
            {
                return Ok(vec![Effect::ReflexiveTrigger(body)]);
            }
        }
        ts.restore(rcp);
    }

    // 4. Leading conditional: `If <cond>, <then_body> [instead]` OR `Unless <player> pays/pay <cost>, <body>`
    {
        let ucp = ts.checkpoint();
        if ts.eat_word("unless")
            && let Ok(payer) = try_parse(ts, parse_player_ref)
            && (ts.eat_word("pay") || ts.eat_word("pays"))
            && let Ok(cost) = try_parse(ts, parse_cost_atom)
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::Unless {
                body,
                payer: Some(payer),
                cost: Some(cost),
                condition: None,
            }]);
        }
        ts.restore(ucp);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(cond) = try_parse(ts, parse_condition)
            && ts.eat_comma()
        {
            // Support trailing `instead` on conditional effects (`If ..., ~ deals 4 damage instead`)
            let remaining = &ts.tokens[ts.pos..];
            let has_trailing_instead = remaining.last().is_some_and(
                |t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")),
            );
            let then_body = if has_trailing_instead && remaining.len() > 1 {
                let mut sub = ts.sub_stream(&remaining[..remaining.len() - 1], ParseStage::Effect);
                let inner = parse_sentence_top(&mut sub)?;
                sub.expect_eof()?;
                ts.pos = ts.tokens.len();
                vec![Effect::ReplacePrevious(inner)]
            } else {
                parse_sentence_top(ts)?
            };
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

    // 6. Scoped leading duration: `[For] as long as <cond>, <body>` / `Until end of turn, <body>` / `Until the end of your next turn, <body>` / `This turn and next turn, <body>`
    {
        let acp = ts.checkpoint();
        if (ts.eat_phrase(&["for", "as", "long", "as"]) || ts.eat_phrase(&["as", "long", "as"]))
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
    if ts.eat_phrase(&["until", "your", "next", "end", "step"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ScopedDuration {
            duration: Duration::UntilYourNextEndStep,
            body,
        }]);
    }
    if ts.eat_phrase(&["until", "your", "next", "upkeep"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ScopedDuration {
            duration: Duration::UntilYourNextUpkeep,
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
    // General `For each <player who can't | 1 damage/life | opponent with ... | player | filter>, <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each"]) {
            if (ts.eat_phrase(&["1", "damage", "prevented", "this", "way"])
                || ts.eat_phrase(&["one", "damage", "prevented", "this", "way"]))
                && ts.eat_comma()
            {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachUnitOfValue(
                        crate::v2::ir::ValueExpr::PriorActionCount(
                            crate::v2::ir::PriorActionKind::DamagePreventedThisWay,
                        ),
                    ),
                    body,
                }]);
            }
            if (ts.eat_phrase(&["1", "life", "you", "lost"])
                || ts.eat_phrase(&["1", "life", "lost", "this", "way"]))
                && ts.eat_comma()
            {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachUnitOfValue(
                        crate::v2::ir::ValueExpr::EventCountThisTurn {
                            event: crate::v2::ir::TurnEventKind::LifeLost,
                            player: Some(PlayerRef::You),
                            exclude_first: false,
                        },
                    ),
                    body,
                }]);
            }
            {
                let ocp = ts.checkpoint();
                if ts.eat_phrase(&["opponent", "with"])
                    && let Some(n) = ts.next_number(false)
                    && ts.eat_phrase(&["or", "more"])
                    && let Ok(kind) = try_parse(ts, crate::v2::filter::parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_comma()
                {
                    let body = parse_sentence_top(ts)?;
                    return Ok(vec![Effect::ForEach {
                        over: ForEachDomain::EachOpponentWithCondition(
                            crate::v2::ir::ConditionExpr::PlayerHasCounters {
                                player: PlayerRef::EachOpponent,
                                kind,
                                min_count: u16::try_from(n.max(1)).unwrap_or(1),
                            },
                        ),
                        body,
                    }]);
                }
                ts.restore(ocp);
            }
            if ts.eat_phrase(&["opponent", "who", "does"]) && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachPlayerWhoDoes(PlayerRef::EachOpponent),
                    body,
                }]);
            }
            if ts.eat_phrase(&["player", "who", "does"]) && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachPlayerWhoDoes(PlayerRef::EachPlayer),
                    body,
                }]);
            }
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
            if (ts.eat_phrase(&["other", "player"])
                || ts.eat_phrase(&["other", "opponent"])
                || ts.eat_word("opponent")
                || ts.eat_word("player"))
                && ts.eat_comma()
            {
                let p = PlayerRef::EachPlayer;
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::Player(p),
                    body,
                }]);
            }
            if ts.eat_phrase(&["of", "them"]) && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::Object(Box::new(ObjectFilter {
                        any_card: true,
                        ..Default::default()
                    })),
                    body,
                }]);
            }
            if ts.eat_word("copy") && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::Object(Box::new(ObjectFilter {
                        any_spell: true,
                        ..Default::default()
                    })),
                    body,
                }]);
            }
            if ts.eat_phrase(&["card", "type"]) && ts.eat_comma() {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::CardType,
                    body,
                }]);
            }
            if ts.eat_phrase(&["color", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_comma()
            {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::ColorAmong(Box::new(flt)),
                    body,
                }]);
            }
            {
                let ccp = ts.checkpoint();
                if let Ok(kind) = try_parse(ts, crate::v2::filter::parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                    && ts.eat_comma()
                {
                    let body = parse_sentence_top(ts)?;
                    return Ok(vec![Effect::ForEach {
                        over: ForEachDomain::Count(crate::v2::ir::ValueExpr::CounterCount {
                            subject: Box::new(subject),
                            kind,
                        }),
                        body,
                    }]);
                }
                ts.restore(ccp);
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

    // 7b. `It's [still] a[n] [color] <subtypes/types> [in addition to its/his/her other [creature] types] [and has no abilities | and gains <kws>]`
    {
        let itcp = ts.checkpoint();
        if ts.eat_phrase(&["it's", "still", "a"])
            || ts.eat_phrase(&["it's", "still", "an"])
            || ts.eat_phrase(&["it's", "a"])
            || ts.eat_phrase(&["it's", "an"])
            || ts.eat_phrase(&["they're", "still"])
            || ts.eat_word("they're")
        {
            let mut add_types = crate::ast::CardTypeMask::EMPTY;
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(ct) = lookup_card_type(&low) {
                    ts.next_word();
                    add_types = add_types.union(ct);
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else if let Some(c) = crate::vocab::lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else {
                    break;
                }
            }
            if !add_types.is_empty() || !subtypes.is_empty() || !colors.is_empty() {
                let keep_existing = ts
                    .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "his", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "her", "other", "types"])
                    || ts.eat_phrase(&[
                        "in", "addition", "to", "their", "other", "creature", "types",
                    ])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                let mut out = Vec::new();
                if !colors.is_empty() {
                    out.push(Effect::MutateObject {
                        subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                        mutation: ObjectMutation::BecomeColors {
                            colors,
                            keep_existing_colors: keep_existing,
                        },
                        duration: Duration::Permanent,
                    });
                }
                if !subtypes.is_empty() || !add_types.is_empty() {
                    out.push(Effect::MutateObject {
                        subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                        mutation: ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types: keep_existing,
                            base_pt: None,
                            keywords: Vec::new(),
                        },
                        duration: Duration::Permanent,
                    });
                }
                if ts.eat_phrase(&["and", "has", "no", "abilities"])
                    || ts.eat_phrase(&["and", "loses", "all", "abilities"])
                {
                    out.push(Effect::MutateObject {
                        subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                        mutation: ObjectMutation::RemoveAllAbilities,
                        duration: Duration::Permanent,
                    });
                } else {
                    let gcp = ts.checkpoint();
                    if (ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]))
                        && let Ok((kws, _)) = try_parse(ts, crate::v2::keywords::parse_keyword_list)
                    {
                        out.push(Effect::MutateObject {
                            subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It),
                            mutation: ObjectMutation::GrantKeywords(kws),
                            duration: Duration::Permanent,
                        });
                    } else {
                        ts.restore(gcp);
                    }
                }
                if ts.is_eof() {
                    return Ok(out);
                }
            }
        }
        ts.restore(itcp);
    }
    if ts.eat_phrase(&["your", "life", "total", "can't", "change"]) {
        let mut out = vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::Action(PlayerActionKind::LifeTotalCantChange),
            duration: Duration::UntilYourNextTurn,
        }];
        let pcp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(more) = try_parse(ts, parse_sentence_top)
        {
            out.extend(more);
        } else {
            ts.restore(pcp);
        }
        return Ok(out);
    }
    // 7b1. `<player>'s life total becomes <val>` (`Repay in Kind`, `Biorhythm`, `Beacon of Immortality`)
    {
        let lcp = ts.checkpoint();
        let life_player = if ts.eat_phrase(&["each", "player's", "life", "total", "becomes"]) {
            Some(PlayerRef::EachPlayer)
        } else if ts.eat_phrase(&["each", "opponent's", "life", "total", "becomes"]) {
            Some(PlayerRef::EachOpponent)
        } else if ts.eat_phrase(&["target", "player's", "life", "total", "becomes"]) {
            Some(PlayerRef::TargetPlayer)
        } else if ts.eat_phrase(&["target", "opponent's", "life", "total", "becomes"]) {
            Some(PlayerRef::TargetOpponent)
        } else if ts.eat_phrase(&["that", "player's", "life", "total", "becomes"])
            || ts.eat_phrase(&["their", "life", "total", "becomes"])
        {
            Some(PlayerRef::ThatPlayer)
        } else if ts.eat_phrase(&["your", "life", "total", "becomes"]) {
            Some(PlayerRef::You)
        } else {
            None
        };
        if let Some(player) = life_player
            && let Ok(amount) = try_parse(ts, parse_value_expr)
        {
            return Ok(vec![Effect::AdjustLife {
                player,
                amount,
                op: crate::v2::ir::LifeOp::Set,
            }]);
        }
        ts.restore(lcp);
    }
    // 7b1b. `the damage can't be prevented` (`Demonfire`, `Urza's Rage`, `Skullcrack`)
    if ts.eat_phrase(&["the", "damage", "can't", "be", "prevented"]) {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::RuleRestriction {
                subject: None,
                rule: RuleRestrictionKind::DamageCantBePrevented,
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        }]);
    }
    // 7b2. `All creatures able to block <tgt> this turn do so`
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["all", "creatures", "able", "to", "block"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && (ts.eat_phrase(&["this", "turn", "do", "so"]) || ts.eat_phrase(&["do", "so"]))
        {
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::AllMustBlock,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            }]);
        }
        ts.restore(bcp);
    }

    // 7c. `This ability costs {M} less/more to activate [for each <expr>]` / Class `Level <N>`
    if ts.eat_word("level")
        && let Some(lvl) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::LevelStructure(LevelMarker::ClassLevel(lvl))),
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

    // 10. Trailing `if <cond>` on a sentence (`draw two cards if you control a Demon`,
    //     `Create five of those tokens instead if ...`, `... at the beginning of the next end step if ...`)
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
            if let Ok(body) = parse_sentence_top(&mut body_ts)
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
        // Fallback: `<body> unless <effect>` (`unless that player sacrifices it`, `unless that player discards two cards or sacrifices ...`, `unless you pay {1} for each ...`)
        let mut eff_ts = ts.sub_stream(tail_slice, ParseStage::Effect);
        if let Ok(unless) = parse_sentence_top(&mut eff_ts)
            && eff_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
            if let Ok(body) = parse_clause_seq(&mut body_ts, None)
                && body_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(vec![Effect::UnlessEffect { body, unless }]);
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
    // A0. `you may spend [colorless] mana as though it were mana of any color/type to cast that spell / those spells / it`
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "spend"]) {
            let _ = ts.eat_word("colorless");
            if (ts.eat_phrase(&[
                "mana", "as", "though", "it", "were", "mana", "of", "any", "color", "to", "cast",
            ]) || ts.eat_phrase(&[
                "mana", "as", "though", "it", "were", "mana", "of", "any", "type", "to", "cast",
            ])) && (ts.eat_phrase(&["that", "spell"])
                || ts.eat_phrase(&["those", "spells"])
                || ts.eat_word("it"))
            {
                return Ok((
                    vec![Effect::MutatePlayer {
                        player: PlayerRef::You,
                        mutation: PlayerMutation::Action(PlayerActionKind::SpendManaAsAnyType),
                        duration: Duration::UntilEndOfTurn,
                    }],
                    Some(PlayerRef::You),
                ));
            }
        }
        ts.restore(scp);
    }

    // A. Spend restriction (`Spend this mana only to ...` / `This mana can't be spent to ...`)
    if ts.eat_phrase(&["this", "mana", "can't", "be", "spent", "to"]) {
        if ts.eat_word("cast")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::ManaRule(ManaRuleKind::SpendForbidden(
                        crate::v2::ir::ManaSpendPurpose::CastSpells(Some(flt)),
                    )),
                    duration: Duration::Instant,
                }],
                Some(PlayerRef::You),
            ));
        }
        if ts.eat_phrase(&["activate", "abilities"])
            || ts.eat_phrase(&["activate", "an", "ability"])
        {
            let flt = if ts.eat_word("of") {
                let f = opt(ts, parse_object_filter);
                let _ = ts.eat_word("source") || ts.eat_word("sources");
                f
            } else {
                None
            };
            return Ok((
                vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::ManaRule(ManaRuleKind::SpendForbidden(
                        crate::v2::ir::ManaSpendPurpose::ActivateAbilities(flt),
                    )),
                    duration: Duration::Instant,
                }],
                Some(PlayerRef::You),
            ));
        }
        return Err(ts.err("expected mana spend purpose after `This mana can't be spent to`"));
    }
    if ts.eat_phrase(&["spend", "this", "mana", "only", "to"]) {
        if ts.eat_phrase(&["cast", "spells", "from"]) {
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::ManaRule(ManaRuleKind::SpendRestriction(
                        ManaSpendRule {
                            only_for: true,
                            cast_from_zone: Some(zref.zone),
                            spell_filter: None,
                        },
                    )),
                    duration: Duration::Instant,
                }],
                Some(PlayerRef::You),
            ));
        }
        let mut purposes = Vec::new();
        loop {
            let pcp = ts.checkpoint();
            if ts.eat_phrase(&["foretell", "a", "card", "from", "your", "hand"]) {
                purposes.push(crate::v2::ir::ManaSpendPurpose::Foretell);
            } else if ts.eat_phrase(&["activate", "abilities"])
                || ts.eat_phrase(&["activate", "an", "ability"])
            {
                let flt = if ts.eat_word("of") {
                    let f = opt(ts, parse_object_filter);
                    let _ = ts.eat_word("source") || ts.eat_word("sources");
                    f
                } else {
                    None
                };
                purposes.push(crate::v2::ir::ManaSpendPurpose::ActivateAbilities(flt));
            } else if ts.eat_word("cast") {
                if ts.eat_phrase(&["spells", "from"])
                    && let Ok(zref) = try_parse(ts, parse_zone_ref)
                {
                    purposes.push(crate::v2::ir::ManaSpendPurpose::CastFromZone(zref.zone));
                } else if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    purposes.push(crate::v2::ir::ManaSpendPurpose::CastSpells(Some(flt)));
                } else {
                    ts.restore(pcp);
                    break;
                }
            } else {
                ts.restore(pcp);
                break;
            }
            let or_cp = ts.checkpoint();
            let _ = ts.eat_comma();
            if !ts.eat_word("or") {
                ts.restore(or_cp);
                break;
            }
        }
        if purposes.len() == 1
            && let Some(crate::v2::ir::ManaSpendPurpose::CastSpells(Some(flt))) = purposes.first()
        {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::ManaRule(ManaRuleKind::SpendRestriction(
                        ManaSpendRule {
                            only_for: true,
                            cast_from_zone: None,
                            spell_filter: Some(flt.clone()),
                        },
                    )),
                    duration: Duration::Instant,
                }],
                Some(PlayerRef::You),
            ));
        }
        if !purposes.is_empty() {
            let purpose = if purposes.len() == 1 {
                purposes.remove(0)
            } else {
                crate::v2::ir::ManaSpendPurpose::AnyOf(purposes)
            };
            return Ok((
                vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::ManaRule(ManaRuleKind::SpendPurpose(purpose)),
                    duration: Duration::Instant,
                }],
                Some(PlayerRef::You),
            ));
        }
        return Err(ts.err("expected mana spend purpose after `Spend this mana only to`"));
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

    // Explicit `You and <player2> each <verb>` (`You and target opponent each create a Treasure token`)
    {
        let p2_cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "and"])
            && let Ok(other) = try_parse(ts, parse_player_ref)
            && ts.eat_word("each")
            && let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, other))
        {
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::Player(other),
                    body,
                }],
                Some(other),
            ));
        }
        ts.restore(p2_cp);
    }

    // Explicit player prefix + optional `may` + verb frame
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("may") {
                // `Each player may exile their hand and draw ...` or `You may ...`
                let (mut body, _) = parse_player_verb_frame(ts, player)?;
                // Consume chained `and [may] <verb>` INSIDE `Optional` when it's `may <v1> and [may] <v2>`!
                let and_cp = ts.checkpoint();
                if ts.eat_word("and") {
                    let _ = ts.eat_word("may");
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
    // `Activate no more than <N> times each turn` (`Manaforge Cinder`)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["activate", "no", "more", "than"])
            && ts.next_number(false).is_some()
            && ts.eat_phrase(&["times", "each", "turn"])
        {
            return Ok(vec![ActivationRestriction::OnceEachTurn]);
        }
        ts.restore(ncp);
    }
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
        } else if ts.eat_phrase(&["during", "the", "declare", "attackers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "the", "declare", "blockers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["during", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::CombatDamage,
            ));
        } else if ts.eat_phrase(&["during", "the", "end", "of", "combat", "step"]) {
            out.push(ActivationRestriction::DuringEndOfCombat);
        } else if ts.eat_phrase(&["during", "combat"]) {
            out.push(ActivationRestriction::DuringCombat);
        } else if ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
            out.push(ActivationRestriction::BeforeAttackers);
        } else if ts.eat_phrase(&["once", "each", "turn"]) {
            out.push(ActivationRestriction::OnceEachTurn);
        } else if ts.eat_word("once") {
            out.push(ActivationRestriction::Once);
        } else if ts.eat_phrase(&["if", "you've", "controlled"])
            && ts.eat_tilde()
            && ts.eat_phrase(&[
                "continuously",
                "since",
                "the",
                "beginning",
                "of",
                "your",
                "most",
                "recent",
                "turn",
            ])
        {
            out.push(ActivationRestriction::ControlledContinuously);
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
