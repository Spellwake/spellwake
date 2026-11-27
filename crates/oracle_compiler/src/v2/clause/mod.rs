//! Compositional top-down effect-sentence and verb-frame clause parser for `v2`.
//!
//! Modular layout:
//! - `mod.rs`: sentence-level dispatch (BindX, VillainousChoice, Conditionals, DelayedTrigger,
//!   ScopedDuration, ForEach, Restrictions) and clause sequencing with player-actor propagation.
//! - `player_verbs.rs`: player-initiated verb frames, mana additions, damage prevention, keyword actions.
//! - `subject_verbs.rs`: object/subject-initiated verb frames and trailing duration parsing.
//! - `tokens.rs`: predefined, creature, and copy-token creation parser.

pub(crate) mod player_actions;
mod player_verbs;
mod player_verbs_ext;
mod restrictions;
mod sentence_ext;
mod subject_combat;
mod subject_verbs;
pub(crate) mod tokens;
mod zone_ext;
mod zone_verbs;

use crate::ast::{CardTypeMask, ColorMask};
use crate::v2::clause::player_verbs::parse_player_verb_frame;
use crate::v2::clause::restrictions::{
    parse_activation_restrictions, parse_additional_combat, parse_temporary_static_clause,
};
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
use crate::vocab::{Vocabulary, lookup_color};

pub(super) use restrictions::{
    all_zone_cards_subject, apply_continuous, apply_replacement, compile_quoted_face,
    compile_quoted_permanent, counted_zone_op, eat_rounding, eat_type_color_subtype_run,
    exact_count, exile_zone_op, move_zone_op, mutate_obj, mutate_player, parse_from_zone_list,
    player_action, prompt_choice, shuffled_zone_dest, simple_zone_dest, simple_zone_op,
    zone_cards_subject,
};

const SENTENCE_STARTERS: &[&str] = &[
    "It's",
    "They're",
    "It",
    "They",
    "Its",
    "Their",
    "Your",
    "That",
    "Those",
    "Activate",
    "Do",
    "This",
    "Exile",
    "Sacrifice",
    "Return",
    "Draw",
    "Put",
    "You",
    "Each",
    "If",
    "When",
    "Whenever",
    "At",
    "Until",
    "Then",
    "Otherwise",
    "The",
    "Cast",
    "Empower",
    "Creatures",
    "Permanents",
    "Players",
    "Opponents",
    "Search",
    "Only",
    "Create",
    "Choose",
    "Discard",
    "Tap",
    "Untap",
    "Destroy",
    "Target",
    "All",
    "For",
    "Gain",
    "Add",
    "Reveal",
    "Shuffle",
    "Look",
    "Scry",
    "Surveil",
    "Mill",
    "Copy",
    "Remove",
    "Move",
    "Prevent",
    "Flip",
    "Roll",
    "Repeat",
    "End",
    "Any",
    "An",
    "A",
];

/// Parse a multi-sentence effect body into strictly-typed `Vec<Effect>` (fail-closed).
pub fn parse_effect_body_str(
    body: &str,
    vocab: &Vocabulary,
) -> Result<Vec<Effect>, Vec<ParseError>> {
    let toks = tokenize(body).map_err(|e| vec![e])?;
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut parse_slice = |slice| {
        let mut ts = TokenStream::new(body, slice, vocab, ParseStage::Effect);
        match parse_effect_sentence(&mut ts).and_then(|res| {
            ts.expect_eof()?;
            Ok(res)
        }) {
            Ok(effs) => out.extend(effs),
            Err(e) => errors.push(e),
        }
    };
    let mut cur_start = 0usize;
    for (idx, tok) in toks.iter().enumerate() {
        let is_period = matches!(tok.kind, TokenKind::Period);
        let is_sentence_ending_quote = matches!(tok.kind, TokenKind::Quoted(q) if q.trim_end().ends_with('.'))
            && toks.get(idx + 1).is_some_and(|next| {
                matches!(
                    next.kind,
                    TokenKind::Word(w)
                        if w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                            && SENTENCE_STARTERS.contains(&w)
                )
            });
        if is_period || is_sentence_ending_quote {
            let end_idx = if is_period { idx } else { idx + 1 };
            if end_idx > cur_start {
                parse_slice(&toks[cur_start..end_idx]);
            }
            cur_start = idx + 1;
        }
    }
    if cur_start < toks.len() {
        parse_slice(&toks[cur_start..]);
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
    // Strip optional Saga chapter flavor/ability word prefix (`Word ... --`) OR Pawprint modal prefix (`{P}+ --`)
    {
        let aw_cp = ts.checkpoint();
        let mut sym_count = 0usize;
        while matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(crate::v2::ir::ManaSymbol::Pawprint))
        ) {
            ts.next_tok();
            sym_count += 1;
        }
        if !(sym_count >= 1 && ts.eat_em_dash()) {
            ts.restore(aw_cp);
            let mut word_count = 0usize;
            let mut found_dash = false;
            while word_count < 4 {
                match ts.peek().map(|t| &t.kind) {
                    Some(TokenKind::Word(_)) => {
                        ts.next_tok();
                        word_count += 1;
                    }
                    Some(TokenKind::EmDash) if word_count >= 1 => {
                        ts.next_tok();
                        found_dash = true;
                        break;
                    }
                    _ => break,
                }
            }
            if !found_dash {
                ts.restore(aw_cp);
            }
        }
    }
    // Strip optional leading `Then [,]`, `Also [,]`, or `Instead [,]` sequential/replacement connector
    if ts.eat_any_word(&["then", "also"]) {
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
                return Ok(vec![player_action(
                    PlayerRef::You,
                    PlayerActionKind::XCantBeZero,
                    Duration::Instant,
                )]);
            }
            if ts.eat_phrase(&["greater", "than"])
                && let Ok(bound) = try_parse(ts, parse_value_expr)
            {
                return Ok(vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::XMaxBound(bound),
                    Duration::Instant,
                )]);
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
    if ts.eat_phrase(&["repeat", "the", "following", "process"])
        && let Ok(cnt) = try_parse(ts, parse_value_expr)
        && ts.eat_word("times")
    {
        return Ok(vec![Effect::ForEach {
            over: ForEachDomain::Count(cnt),
            body: Vec::new(),
        }]);
    }
    if ts.eat_phrase(&["the", "game", "is", "a", "draw"]) {
        return Ok(vec![mutate_player(
            PlayerRef::You,
            PlayerMutation::GameIsDraw,
            Duration::Instant,
        )]);
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
                if ts.eat_any_word(&["them", "it"]) {
                    None
                } else {
                    opt(ts, parse_object_filter)
                }
            } else {
                None
            };
            let dur = crate::v2::clause::subject_verbs::parse_trailing_duration(ts)
                .unwrap_or(Duration::Permanent);
            return Ok(vec![apply_continuous(
                StaticAbility::LegendRuleDoesntApply(filter),
                dur,
            )]);
        }
        ts.restore(lcp);
    }
    if ts.eat_phrase(&[
        "you", "may", "choose", "the", "same", "mode", "more", "than", "once",
    ]) {
        return Ok(vec![player_action(
            PlayerRef::You,
            PlayerActionKind::ChooseSameModeMultiple,
            Duration::Instant,
        )]);
    }
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["spend", "only"])
            && let Some(w) = ts.peek_word()
            && let Some(color) = lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&["mana", "on", "x"]) {
                return Ok(vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::ManaRule(ManaRuleKind::SpendOnlyColorOnX(color)),
                    Duration::Instant,
                )]);
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
        return Ok(vec![player_action(
            PlayerRef::You,
            PlayerActionKind::FlashbackCostEqualsManaCost,
            Duration::UntilEndOfTurn,
        )]);
    }
    if let Ok(extra) = try_parse(ts, player_actions::parse_extra_sentence_effect)
        && ts.is_eof()
    {
        return Ok(extra);
    }
    if let Ok(ext) = try_parse(ts, |s| {
        let res = restrictions::parse_extra_restriction_or_cost_sentence(s)?;
        s.expect_eof()?;
        Ok(res)
    }) {
        return Ok(ext);
    }
    if let Ok(ext) = try_parse(ts, |s| {
        let res = sentence_ext::parse_leading_sentence_ext(s)?;
        s.expect_eof()?;
        Ok(res)
    }) {
        return Ok(ext);
    }

    // 0. `, where X is <ValueExpr>` binder (supports BOTH trailing and mid-sentence `, where X is <expr>, <rest>`, plus `"..., " where X is`)
    if let Some((body_end, expr_start)) = find_where_x_is(ts.tokens, ts.pos) {
        let body_slice = &ts.tokens[ts.pos..body_end];
        let after_where = &ts.tokens[expr_start..];
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

    // 2. Leading `When you do, <effect>` / `When it regenerates this way, <effect>` (reflexive trigger) OR `When/Whenever <trigger_header>, <effect>`
    if (ts.eat_phrase(&["when", "you", "do"])
        || ts.eat_phrase(&["when", "it", "regenerates", "this", "way"]))
        && ts.eat_comma()
    {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::ReflexiveTrigger(body)]);
    }
    if ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("when") || w.eq_ignore_ascii_case("whenever"))
    {
        let wcp = ts.checkpoint();
        if let Ok(trig) = try_parse(ts, parse_trigger_header)
            && ts.eat_comma()
            && let Ok(body) = try_parse(ts, parse_sentence_top)
        {
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Event(trig),
                body,
            }]);
        }
        ts.restore(wcp);
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
        const IF_PREV_FALSE: &[&[&str]] = &[
            &["if", "no", "one", "does"],
            &["if", "no", "player", "does"],
            &["if", "you", "don't", "cast", "it"],
            &["if", "you", "don't", "play", "it"],
            &["if", "you", "didn't", "cast", "it"],
            &["if", "you", "didn't", "play", "it"],
            &["if", "you", "didn't", "create", "a", "token", "this", "way"],
            &["if", "you", "don't", "draw", "a", "card", "this", "way"],
            &["if", "it", "doesn't"],
        ];
        if ts.eat_any_phrase(IF_PREV_FALSE) && ts.eat_comma() {
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
            let succeeded = if ts.eat_any_word(&["do", "does", "pay", "pays"]) {
                Some(true)
            } else if ts.eat_any_word(&["don't", "doesn't", "can't", "didn't"]) {
                Some(false)
            } else {
                None
            };
            if let Some(ok) = succeeded {
                let extra_or_cond = if ts.eat_phrase(&["or", "if"]) {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                if ts.eat_comma() {
                    let body = parse_sentence_top(ts)?;
                    if let Some(or_c) = extra_or_cond {
                        return Ok(vec![Effect::BranchCondition {
                            condition: or_c,
                            then_body: body.clone(),
                            else_body: vec![Effect::IfPrevious {
                                succeeded: ok,
                                body,
                            }],
                        }]);
                    }
                    return Ok(vec![Effect::IfPrevious {
                        succeeded: ok,
                        body,
                    }]);
                }
            }
        }
        ts.restore(cp);
    }

    // 3. Mana spend rider: `If/When that mana is spent [to cast | on] <spell_flt>, <rider>` OR `When you spend this mana to cast <spell_flt>, <rider>`
    const MANA_RIDER_PREFIXES: &[&[&str]] = &[
        &["if", "that", "mana", "is", "spent", "to", "cast"],
        &["if", "that", "mana", "is", "spent", "on"],
        &["when", "that", "mana", "is", "spent", "to", "cast"],
        &["when", "that", "mana", "is", "spent", "on"],
        &["when", "you", "spend", "this", "mana", "to", "cast"],
    ];
    if ts.eat_any_phrase(MANA_RIDER_PREFIXES) {
        let spell_filter = parse_object_filter(ts)?;
        let _ = ts.eat_phrase(&["or", "activate", "an", "ability"]);
        if !ts.eat_comma() {
            return Err(ts.err("expected `,` after mana spend rider condition"));
        }
        let rider = parse_sentence_top(ts)?;
        return Ok(vec![mutate_player(
            PlayerRef::You,
            PlayerMutation::AddManaSpendRider {
                spell_filter,
                rider,
            },
            Duration::UntilEndOfTurn,
        )]);
    }
    // 3b. Reflexive action trigger inside an effect body (`When you sacrifice/discard/exile/cast <...> [this way], <body>`)
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["when", "you"])
            && (ts.eat_any_word(&["sacrifice", "discard", "exile", "cast", "pay"])
                || ts.eat_phrase(&["lose", "control", "of"]))
        {
            let _ = ts.eat_phrase(&["one", "or", "more"]);
            let _ = opt(ts, parse_subject_ref);
            let _ = ts.eat_any_phrase(&[
                &["this", "way"],
                &["this", "turn"],
                &["from", "your", "hand"],
            ]);
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
            // Support trailing `instead` on conditional effects, while preserving full replacement sentences
            // (e.g. `If you win the flip, the next time ... deals double that damage instead`).
            let remaining = &ts.tokens[ts.pos..];
            let has_trailing_instead = remaining.last().is_some_and(
                |t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")),
            );
            let then_body = if has_trailing_instead && remaining.len() > 1 {
                let mut sub = ts.sub_stream(&remaining[..remaining.len() - 1], ParseStage::Effect);
                if let Ok(inner) = parse_sentence_top(&mut sub)
                    && sub.is_eof()
                {
                    ts.pos = ts.tokens.len();
                    vec![Effect::ReplacePrevious(inner)]
                } else {
                    parse_sentence_top(ts)?
                }
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

    if ts.eat_phrase(&["during", "any", "turn", "you", "attacked", "with"]) {
        if ts.next_number(false).is_some() {
            let _ = ts.eat_phrase(&["or", "more"]);
        }
        let _ = opt(ts, parse_object_filter);
        if ts.eat_comma() {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::BranchCondition {
                condition: crate::v2::ir::ConditionExpr::PlayerCheck {
                    player: PlayerRef::You,
                    kind: crate::v2::ir::PlayerConditionKind::AttackedThisTurn,
                },
                then_body: body,
                else_body: Vec::new(),
            }]);
        }
    }
    if ts.eat_phrase(&["at", "this", "turn's", "next", "end", "of", "combat"]) && ts.eat_comma() {
        let body = parse_sentence_top(ts)?;
        return Ok(vec![Effect::DelayedTrigger {
            trigger: DelayedTriggerKind::Step {
                step: crate::ast::TurnStepAst::EndOfCombat,
                owner: crate::v2::ir::StepOwner::Next,
            },
            body,
        }]);
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
    const SCOPED_DURATIONS: &[(&[&str], Duration)] = &[
        (
            &["this", "turn", "and", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &["until", "the", "end", "of", "your", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &["until", "your", "next", "end", "step"],
            Duration::UntilYourNextEndStep,
        ),
        (
            &["until", "your", "next", "upkeep"],
            Duration::UntilYourNextUpkeep,
        ),
        (
            &["until", "the", "beginning", "of", "your", "next", "upkeep"],
            Duration::UntilYourNextUpkeep,
        ),
        (
            &["until", "your", "next", "turn"],
            Duration::UntilYourNextTurn,
        ),
        (&["until", "end", "of", "turn"], Duration::UntilEndOfTurn),
        (&["this", "turn"], Duration::UntilEndOfTurn),
    ];
    for (phrase, duration) in SCOPED_DURATIONS {
        let scp = ts.checkpoint();
        if ts.eat_phrase(phrase) && ts.eat_comma() {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::ScopedDuration {
                duration: duration.clone(),
                body,
            }]);
        }
        ts.restore(scp);
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
            && (ts.eat_phrase(&["in", "it"])
                || flt
                    .predicates
                    .contains(&crate::v2::ir::FilterPredicate::AmongPriorCards))
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
            if ts.eat_phrase(&["kind", "of", "counter"])
                && (ts.eat_word("on") || ts.eat_word("among"))
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_comma()
            {
                let body = parse_sentence_top(ts)?;
                return Ok(vec![Effect::ForEach {
                    over: ForEachDomain::EachUnitOfValue(crate::v2::ir::ValueExpr::CounterCount {
                        subject: Box::new(subj),
                        kind: crate::v2::ir::CounterKind::Any,
                    }),
                    body,
                }]);
            }
            if ts.eat_any_phrase(&[
                &["1", "damage", "prevented", "this", "way"],
                &["one", "damage", "prevented", "this", "way"],
            ]) && ts.eat_comma()
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
            if ts.eat_any_phrase(&[
                &["1", "life", "you", "lost"],
                &["1", "life", "lost", "this", "way"],
            ]) && ts.eat_comma()
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
                if ts.eat_phrase(&["opponent", "with", "no", "cards", "in", "hand"])
                    && ts.eat_comma()
                {
                    let body = parse_sentence_top(ts)?;
                    return Ok(vec![Effect::ForEach {
                        over: ForEachDomain::EachOpponentWithCondition(
                            crate::v2::ir::ConditionExpr::ZoneMetric {
                                zone: crate::v2::ir::ZoneRef {
                                    zone: crate::v2::ir::ZoneKind::Hand,
                                    owner: crate::v2::ir::ZoneOwner::Opponent,
                                },
                                kind: crate::v2::ir::ZoneMetricKind::IsEmpty,
                                min_count: 0,
                            },
                        ),
                        body,
                    }]);
                }
                ts.restore(ocp);
                if ts.eat_phrase(&["opponent", "with"])
                    && let Some(n) = ts.next_number(false)
                    && ts.eat_phrase(&["or", "more"])
                    && let Ok(kind) = try_parse(ts, crate::v2::filter::parse_counter_kind)
                    && ts.eat_any_word(&["counters", "counter"])
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
            {
                let fcp = ts.checkpoint();
                let simple_domain = if ts.eat_phrase(&["opponent", "who", "does"]) {
                    Some(ForEachDomain::EachPlayerWhoDoes(PlayerRef::EachOpponent))
                } else if ts.eat_phrase(&["player", "who", "does"]) {
                    Some(ForEachDomain::EachPlayerWhoDoes(PlayerRef::EachPlayer))
                } else if ts.eat_phrase(&["opponent", "who", "can't"]) {
                    Some(ForEachDomain::EachPlayerWhoCant(PlayerRef::EachOpponent))
                } else if ts.eat_phrase(&["player", "who", "can't"]) {
                    Some(ForEachDomain::EachPlayerWhoCant(PlayerRef::EachPlayer))
                } else if ts.eat_any_phrase(&[
                    &["other", "player"],
                    &["other", "opponent"],
                    &["opponent"],
                    &["player"],
                ]) {
                    Some(ForEachDomain::Player(PlayerRef::EachPlayer))
                } else {
                    None
                };
                if let Some(over) = simple_domain
                    && ts.eat_comma()
                {
                    let body = parse_sentence_top(ts)?;
                    return Ok(vec![Effect::ForEach { over, body }]);
                }
                ts.restore(fcp);
            }
            {
                let dcp = ts.checkpoint();
                if ts.next_number(false) == Some(1)
                    && (ts.eat_phrase(&["damage", "prevented", "this", "way"])
                        || (ts.eat_phrase(&["damage", "dealt"]) && {
                            if ts.eat_word("to") {
                                let _ = opt(ts, parse_subject_ref);
                            }
                            let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "way"]]);
                            true
                        }))
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
                ts.restore(dcp);
            }
            {
                let fcp = ts.checkpoint();
                let simple_obj = if ts.eat_phrase(&["of", "them"]) {
                    Some(ForEachDomain::Object(Box::new(ObjectFilter {
                        any_card: true,
                        ..Default::default()
                    })))
                } else if ts.eat_word("copy") {
                    Some(ForEachDomain::Object(Box::new(ObjectFilter {
                        any_spell: true,
                        ..Default::default()
                    })))
                } else if ts.eat_phrase(&["card", "type"]) {
                    Some(ForEachDomain::CardType)
                } else {
                    None
                };
                if let Some(over) = simple_obj
                    && ts.eat_comma()
                {
                    let body = parse_sentence_top(ts)?;
                    return Ok(vec![Effect::ForEach { over, body }]);
                }
                ts.restore(fcp);
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

    if ts.eat_phrase(&["your", "life", "total", "can't", "change"]) {
        let mut out = vec![player_action(
            PlayerRef::You,
            PlayerActionKind::LifeTotalCantChange,
            Duration::UntilYourNextTurn,
        )];
        let pcp = ts.checkpoint();
        let _ = ts.eat_comma();
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
        if let Some(player) = eat_possessive_life_total_becomes(ts) {
            let _ = ts.eat_phrase(&["equal", "to"]);
            if let Ok(amount) = try_parse(ts, parse_value_expr) {
                return Ok(vec![Effect::AdjustLife {
                    player,
                    amount,
                    op: crate::v2::ir::LifeOp::Set,
                }]);
            }
        }
        ts.restore(lcp);
    }
    // 7b1b. `the damage can't be prevented` (`Demonfire`, `Urza's Rage`, `Skullcrack`)
    if ts.eat_phrase(&["the", "damage", "can't", "be", "prevented"]) {
        return Ok(vec![apply_continuous(
            StaticAbility::RuleRestriction {
                subject: None,
                rule: RuleRestrictionKind::DamageCantBePrevented,
                condition: None,
            },
            Duration::UntilEndOfTurn,
        )]);
    }
    // 7b1b2. `Its/Their/<subj>'s activated abilities can't be activated [unless they're mana abilities] [dur]` (`Koma, Cosmos Serpent`, `Deadlock Trap`)
    {
        let acp = ts.checkpoint();
        let subj_opt = if ts.eat_word("its") {
            Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It))
        } else if ts.eat_word("their") {
            Some(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::Them))
        } else if let Ok(s) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("'s")
        {
            Some(s)
        } else {
            None
        };
        if let Some(subj) = subj_opt
            && ts.eat_phrase(&["activated", "abilities", "can't", "be", "activated"])
        {
            let _ = ts.eat_any_phrase(&[
                &["unless", "they're", "mana", "abilities"],
                &["unless", "they", "are", "mana", "abilities"],
            ]);
            let duration = crate::v2::clause::subject_verbs::parse_trailing_duration(ts)
                .unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![mutate_obj(
                subj,
                ObjectMutation::ApplyRule(RuleRestrictionKind::ActivatedAbilitiesCantBeActivated),
                duration,
            )]);
        }
        ts.restore(acp);
    }
    // 7b1c. `<subj>'s base power [and toughness] become(s) equal to <val> [dur]` OR `the base power and toughness of <subj> become equal to <val> [dur]` (`Belligerent Yearling`, `PuPu UFO`, `Sita Varma`, `Halfdane`)
    {
        let bcp = ts.checkpoint();
        let base_pt_subj = if ts.eat_phrase(&["the", "base", "power", "and", "toughness", "of"]) {
            try_parse(ts, parse_subject_ref).ok()
        } else if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("'s")
            && ts.eat_word("base")
            && (ts.eat_phrase(&["power", "and", "toughness"]) || ts.eat_word("power"))
        {
            Some(subject)
        } else {
            None
        };
        if let Some(subject) = base_pt_subj
            && ts.eat_any_phrase(&[&["become", "equal", "to"], &["becomes", "equal", "to"]])
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            let duration = crate::v2::clause::subject_verbs::parse_trailing_duration(ts)
                .unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![mutate_obj(
                subject,
                ObjectMutation::SetDynamicBasePT {
                    power: val.clone(),
                    toughness: val,
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                duration,
            )]);
        }
        ts.restore(bcp);
    }
    // 7b2. `All creatures able to block <tgt> this turn do so`
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["all", "creatures", "able", "to", "block"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_any_phrase(&[&["this", "turn", "do", "so"], &["do", "so"]])
        {
            return Ok(vec![apply_continuous(
                StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::AllMustBlock,
                    condition: None,
                },
                Duration::UntilEndOfTurn,
            )]);
        }
        ts.restore(bcp);
    }

    // 7c. `This ability costs {M} less/more to activate [for each <expr>]` / Class `Level <N>`
    if ts.eat_word("level")
        && let Some(lvl) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        return Ok(vec![apply_continuous(
            StaticAbility::LevelStructure(LevelMarker::ClassLevel(lvl)),
            Duration::Permanent,
        )]);
    }
    if ts.peek_phrase(&["this", "ability", "costs"])
        && let Ok(st) = try_parse(ts, parse_static_ability)
    {
        return Ok(vec![apply_continuous(st, Duration::Permanent)]);
    }

    // 8. Activation / trigger / cast restrictions
    if ts.eat_any_phrase(&[
        &[
            "this", "ability", "triggers", "only", "once", "each", "turn",
        ],
        &[
            "this", "ability", "triggers", "only", "twice", "each", "turn",
        ],
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
            let mut restrs = vec![ActivationRestriction::AllowedPlayer(player)];
            let _ = ts.eat_comma();
            if ts.eat_any_phrase(&[
                &["but", "only", "as", "a", "sorcery"],
                &["only", "as", "a", "sorcery"],
            ]) {
                restrs.push(ActivationRestriction::AsSorcery);
            } else if ts.eat_any_phrase(&[&["but", "only", "during"], &["only", "during"]]) {
                if ts.eat_any_phrase(&[
                    &["their", "turn", "before", "the", "end", "step"],
                    &["their", "turn"],
                    &["your", "turn"],
                ]) {
                    restrs.push(ActivationRestriction::DuringYourTurn);
                } else if ts.eat_any_phrase(&[
                    &["any", "upkeep", "step"],
                    &["any", "upkeep"],
                    &["their", "upkeep"],
                ]) {
                    restrs.push(ActivationRestriction::DuringYourUpkeep);
                } else if ts
                    .eat_any_phrase(&[&["their", "draw", "step"], &["your", "draw", "step"]])
                {
                    restrs.push(ActivationRestriction::OnlyIf(Box::new(
                        crate::v2::ir::ConditionExpr::EventCheck(
                            crate::v2::ir::EventCheckKind::DuringStep(
                                crate::ast::TurnStepAst::DrawStep,
                            ),
                        ),
                    )));
                }
            }
            return Ok(vec![Effect::Restriction(restrs)]);
        }
        ts.restore(acp);
    }
    if let Some(restr) = opt(ts, parse_activation_restrictions) {
        return Ok(vec![Effect::Restriction(restr)]);
    }

    // 9. Trailing `instead` / `instead of one` replacement inside a spell/ability
    if ts.tokens.len() > ts.pos {
        let trim_len = if ts.tokens.len() >= ts.pos + 3
            && matches!(ts.tokens[ts.tokens.len() - 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("instead"))
            && matches!(ts.tokens[ts.tokens.len() - 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("of"))
            && match ts.tokens[ts.tokens.len() - 1].kind {
                TokenKind::Number(_) => true,
                TokenKind::Word(w) => {
                    let low = w.to_ascii_lowercase();
                    ["one", "two", "three", "it", "them"].contains(&low.as_str())
                }
                _ => false,
            } {
            Some(3)
        } else if ts.tokens.len() >= ts.pos + 4
            && matches!(ts.tokens[ts.tokens.len() - 4].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("instead"))
            && matches!(ts.tokens[ts.tokens.len() - 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("of"))
            && matches!(ts.tokens[ts.tokens.len() - 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("that"))
            && matches!(ts.tokens[ts.tokens.len() - 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("many"))
        {
            Some(4)
        } else if matches!(
            ts.tokens[ts.tokens.len() - 1].kind,
            TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        ) {
            Some(1)
        } else {
            None
        };
        if let Some(k) = trim_len {
            let inner_slice = &ts.tokens[ts.pos..ts.tokens.len() - k];
            let mut inner_ts = ts.sub_stream(inner_slice, ParseStage::Effect);
            if let Ok(inner) = parse_clause_seq(&mut inner_ts, None)
                && inner_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
                return Ok(vec![Effect::ReplacePrevious(inner)]);
            }
        }
    }

    // 10. Trailing `if <cond>` on a sentence (`draw two cards if you control a Demon`,
    //     `Create five of those tokens instead if ...`, `Destroy that creature if it has mana value 4 or less instead if a permanent left the battlefield under your control this turn`)
    //     Iterate rightmost-first so nested `<body> if <inner> instead if <outer>` composes cleanly!
    for if_idx in (ts.pos + 1..ts.tokens.len()).rev() {
        if !matches!(
            ts.tokens[if_idx].kind,
            TokenKind::Word(w) if w.eq_ignore_ascii_case("if")
        ) {
            continue;
        }
        let mut body_end = if_idx;
        if body_end > ts.pos && matches!(ts.tokens[body_end - 1].kind, TokenKind::Comma) {
            body_end -= 1;
        }
        let body_slice = &ts.tokens[ts.pos..body_end];
        let cond_slice = &ts.tokens[if_idx + 1..];
        let mut cond_ts = ts.sub_stream(cond_slice, ParseStage::Effect);
        let parsed_cond = parse_condition(&mut cond_ts)
            .ok()
            .filter(|_| cond_ts.is_eof())
            .or_else(|| {
                let mut e_ts = ts.sub_stream(cond_slice, ParseStage::Effect);
                sentence_ext::parse_extra_clause_condition(&mut e_ts)
                    .ok()
                    .filter(|_| e_ts.is_eof())
            });
        if let Some(cond) = parsed_cond {
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
        let payer_opt =
            if tail_ts.eat_phrase(&["that", "permanent's", "controller", "or", "that", "player"]) {
                Some(PlayerRef::ThatPlayer)
            } else {
                try_parse(&mut tail_ts, parse_player_ref).ok()
            };
        if let Some(payer) = payer_opt {
            let pcp = tail_ts.checkpoint();
            let parsed_cost = if let Ok(c) = try_parse(&mut tail_ts, parse_cost_atom)
                && tail_ts.is_eof()
            {
                Some(c)
            } else {
                tail_ts.restore(pcp);
                let _ = tail_ts.eat_any_word(&["pays", "pay"]);
                if let Ok(c) = try_parse(&mut tail_ts, parse_cost_atom) {
                    if tail_ts.eat_any_phrase(&[&["plus", "an", "additional"], &["plus"], &["or"]])
                    {
                        let _ = try_parse(&mut tail_ts, parse_cost_atom);
                    }
                    let _ = opt(&mut tail_ts, crate::v2::value::parse_for_each_expr);
                    let _ = tail_ts.eat_any_phrase(&[
                        &["before", "that", "draw", "step"],
                        &["before", "that", "upkeep", "step"],
                        &["before", "that", "step"],
                    ]);
                    tail_ts.is_eof().then_some(c)
                } else {
                    tail_ts.restore(pcp);
                    if tail_ts.eat_any_word(&["pays", "pay"])
                        && let Ok(amt) = try_parse(&mut tail_ts, parse_value_expr)
                        && (tail_ts.eat_word("life") || tail_ts.is_eof())
                        && tail_ts.is_eof()
                    {
                        Some(Cost::PayLife(amt))
                    } else {
                        tail_ts.restore(pcp);
                        if tail_ts.eat_any_word(&["pays", "pay"])
                            && tail_ts.eat_any_phrase(&[
                                &["its", "mana", "cost"],
                                &["that", "creature's", "mana", "cost"],
                                &["that", "card's", "mana", "cost"],
                            ])
                        {
                            if tail_ts.eat_phrase(&["reduced", "by"]) {
                                let _ = tail_ts.eat_mana_cost();
                            }
                            tail_ts.is_eof().then_some(Cost::PayAnyAmountMana)
                        } else {
                            None
                        }
                    }
                }
            };
            if let Some(cost) = parsed_cost {
                let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
                if let Ok(body) = parse_sentence_top(&mut body_ts)
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
        let parsed_cond = parse_condition(&mut cond_ts)
            .ok()
            .filter(|_| cond_ts.is_eof())
            .or_else(|| {
                let mut e_ts = ts.sub_stream(tail_slice, ParseStage::Effect);
                sentence_ext::parse_extra_clause_condition(&mut e_ts)
                    .ok()
                    .filter(|_| e_ts.is_eof())
            });
        if let Some(cond) = parsed_cond {
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

    // 11. Trailing `at the beginning of <step>` / `at end of combat` / `when ~ leaves the battlefield` delayed suffix
    if let Some(w_idx) = find_top_level_word(ts.tokens, ts.pos, "when")
        && w_idx > ts.pos
        && w_idx + 5 == ts.tokens.len()
        && matches!(ts.tokens[w_idx + 1].kind, TokenKind::Tilde)
        && matches!(ts.tokens[w_idx + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("leaves"))
        && matches!(ts.tokens[w_idx + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("the"))
        && matches!(ts.tokens[w_idx + 4].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("battlefield"))
    {
        let body_slice = &ts.tokens[ts.pos..w_idx];
        let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
        if let Ok(body) = parse_clause_seq(&mut body_ts, None)
            && body_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Event(crate::v2::ir::TriggerSpec {
                    events: vec![crate::v2::ir::TriggerEvent::ZoneChange {
                        from: Some(crate::v2::ir::ZoneKind::Battlefield),
                        to: None,
                        subject: SubjectRef::SelfObject,
                    }],
                    intervening_if: None,
                }),
                body,
            }]);
        }
    }
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
        if let Ok((step, owner)) = parse_step_with_owner(&mut step_ts).or_else(|e| {
            const EXTRA_STEPS: &[(&[&str], crate::ast::TurnStepAst, crate::v2::ir::StepOwner)] = &[
                (
                    &["that", "player's", "next", "declare", "attackers", "step"],
                    crate::ast::TurnStepAst::BeginningOfCombat,
                    crate::v2::ir::StepOwner::ThatPlayer,
                ),
                (
                    &["their", "next", "declare", "attackers", "step"],
                    crate::ast::TurnStepAst::BeginningOfCombat,
                    crate::v2::ir::StepOwner::ThatPlayer,
                ),
                (
                    &["your", "next", "main", "phase", "this", "turn"],
                    crate::ast::TurnStepAst::MainPhase,
                    crate::v2::ir::StepOwner::YourNext,
                ),
                (
                    &["your", "next", "main", "phase"],
                    crate::ast::TurnStepAst::MainPhase,
                    crate::v2::ir::StepOwner::YourNext,
                ),
                (
                    &["that", "player's", "next", "end", "step"],
                    crate::ast::TurnStepAst::EndStep,
                    crate::v2::ir::StepOwner::ThatPlayer,
                ),
                (
                    &["their", "next", "end", "step"],
                    crate::ast::TurnStepAst::EndStep,
                    crate::v2::ir::StepOwner::ThatPlayer,
                ),
            ];
            EXTRA_STEPS
                .iter()
                .find_map(|&(p, s, o)| step_ts.eat_phrase(p).then_some((s, o)))
                .ok_or(e)
        }) {
            let trailing_cond = if step_ts.eat_word("if") {
                parse_condition(&mut step_ts).ok()
            } else {
                None
            };
            let trailing_and = if trailing_cond.is_none() && step_ts.eat_word("and") {
                try_parse(&mut step_ts, parse_sentence_top).ok()
            } else {
                None
            };
            if step_ts.is_eof() {
                let mut body_ts = ts.sub_stream(body_slice, ParseStage::Effect);
                if let Ok(body) = parse_clause_seq(&mut body_ts, None)
                    && body_ts.is_eof()
                {
                    ts.pos = ts.tokens.len();
                    let delayed = Effect::DelayedTrigger {
                        trigger: DelayedTriggerKind::Step { step, owner },
                        body: if let Some(cond) = trailing_cond {
                            vec![Effect::BranchCondition {
                                condition: cond,
                                then_body: body,
                                else_body: Vec::new(),
                            }]
                        } else {
                            body
                        },
                    };
                    let mut out = vec![delayed];
                    if let Some(more) = trailing_and {
                        out.extend(more);
                    }
                    return Ok(out);
                }
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

    // 11c. Clause sequence with automatic player-actor propagation, plus trailing `, rounded up/down` and `for each <expr>` fallbacks!
    {
        let seq_cp = ts.checkpoint();
        if let Ok(effs) = parse_clause_seq(ts, None) {
            if ts.is_eof() {
                return Ok(effs);
            }
            let rcp = ts.checkpoint();
            if ts.eat_rounded_modifier().is_some() && ts.is_eof() {
                return Ok(effs);
            }
            ts.restore(rcp);
        }
        ts.restore(seq_cp);
        if let Some(effs) = sentence_ext::try_parse_trailing_for_each_sentence(ts) {
            return Ok(effs);
        }
        ts.restore(seq_cp);
    }

    // 12. Clause sequence with automatic player-actor propagation!
    parse_clause_seq(ts, None)
}

fn parse_villainous_choice(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let chooser = parse_player_ref(ts)?;
    if !ts.eat_any_phrase(&[
        &["faces", "a", "villainous", "choice"],
        &["face", "a", "villainous", "choice"],
    ]) {
        return Err(ts.err("expected `face(s) a villainous choice`"));
    }
    if !ts.eat_em_dash() {
        return Err(ts.err("expected em dash after `villainous choice`"));
    }
    parse_villainous_options(ts, chooser)
}

fn parse_villainous_options(ts: &mut TokenStream<'_>, chooser: PlayerRef) -> PResult<Effect> {
    // Split remaining tokens on `, or ` so BOTH options retain internal `, then` and `and` clauses!
    let or_idx = find_comma_or(ts.tokens, ts.pos)
        .ok_or_else(|| ts.err("expected `, or` separating villainous choice options"))?;
    let mut ts1 = ts.sub_stream(&ts.tokens[ts.pos..or_idx], ParseStage::Effect);
    let b1 = parse_sentence_top(&mut ts1)?;
    ts1.expect_eof()?;
    let mut ts2 = ts.sub_stream(&ts.tokens[or_idx + 2..], ParseStage::Effect);
    let b2 = parse_sentence_top(&mut ts2)?;
    ts2.expect_eof()?;
    ts.pos = ts.tokens.len();
    Ok(prompt_choice(
        chooser,
        ChoiceKind::VillainousChoice {
            options: vec![b1, b2],
        },
    ))
}

/// Parse a sequence of clauses joined by `, then`, `, and`, or `and`, propagating any explicit
/// non-`You` leading player actor forward to subsequent bare-verb clauses!
fn parse_clause_seq(
    ts: &mut TokenStream<'_>,
    inherited_actor: Option<PlayerRef>,
) -> PResult<Vec<Effect>> {
    let (first_effs, mut active_actor) = parse_single_clause(ts, inherited_actor)?;
    let mut effects = wrap_optional_trailing_if(ts, first_effs);

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
                effects.extend(wrap_optional_trailing_if(ts, next_effs));
                if next_actor.is_some() {
                    active_actor = next_actor;
                }
            }
            Err(_) => {
                // Allow a trailing sentence-level frame after `, then` / `, and` when it consumes to EOF
                // (e.g. `, and until your next turn, ...`, `, and at the beginning of ..., ...`,
                // `, then faces a villainous choice -- ...`).
                let rem_cp = ts.checkpoint();
                if let Ok(tail_effs) = try_parse(ts, parse_sentence_top)
                    && ts.is_eof()
                {
                    effects.extend(tail_effs);
                    break;
                }
                ts.restore(rem_cp);
                // Also support inherited-actor villainous choice after `, then`
                if let Some(actor) = active_actor
                    && ts.eat_any_phrase(&[
                        &["faces", "a", "villainous", "choice"],
                        &["face", "a", "villainous", "choice"],
                    ])
                    && ts.eat_em_dash()
                    && let Ok(vc) = try_parse(ts, |s| parse_villainous_options(s, actor))
                {
                    effects.push(vc);
                    break;
                }
                ts.restore(cp);
                break;
            }
        }
    }

    Ok(effects)
}

fn wrap_optional_trailing_if(ts: &mut TokenStream<'_>, effs: Vec<Effect>) -> Vec<Effect> {
    let cp = ts.checkpoint();
    if ts.eat_word("if")
        && let Ok(cond) = try_parse(ts, parse_condition)
    {
        return vec![Effect::BranchCondition {
            condition: cond,
            then_body: effs,
            else_body: Vec::new(),
        }];
    }
    ts.restore(cp);
    effs
}

/// Parse one atomic clause, returning `(Vec<Effect>, Option<PlayerRef>)` where the second element
/// is the clause's explicit player actor (for propagation across `and` / `, then`).
fn parse_single_clause(
    ts: &mut TokenStream<'_>,
    inherited_actor: Option<PlayerRef>,
) -> PResult<(Vec<Effect>, Option<PlayerRef>)> {
    // A0. `you may spend [colorless] mana as though it were mana of any color/type to cast that spell / those spells / it`
    //     OR `[you] [may] pay <cost> rather than pay [~'s | this spell's | its | that spell's] mana cost` (Zendikar Traps, `Bolas's Citadel`, `Xander's Pact`)
    {
        let acp = ts.checkpoint();
        let _ = ts.eat_word("you");
        let _ = ts.eat_word("may");
        if let Ok(costs) = try_parse(ts, crate::v2::cost::parse_cost_list)
            && ts.eat_any_phrase(&[&["rather", "than", "pay"], &["rather", "than", "paying"]])
            && (ts.eat_any_phrase(&[
                &["its", "mana", "cost"],
                &["that", "spell's", "mana", "cost"],
                &["this", "spell's", "mana", "cost"],
            ]) || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"])))
        {
            return Ok((
                vec![apply_continuous(
                    StaticAbility::AlternativeCost(costs),
                    Duration::Instant,
                )],
                Some(PlayerRef::You),
            ));
        }
        ts.restore(acp);
    }
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "spend"]) {
            let _ = ts.eat_word("colorless");
            if ts.eat_any_phrase(&[
                &[
                    "mana", "as", "though", "it", "were", "mana", "of", "any", "color", "to",
                    "cast",
                ],
                &[
                    "mana", "as", "though", "it", "were", "mana", "of", "any", "type", "to", "cast",
                ],
            ]) && (ts.eat_any_phrase(&[&["that", "spell"], &["those", "spells"]])
                || ts.eat_word("it"))
            {
                return Ok((
                    vec![player_action(
                        PlayerRef::You,
                        PlayerActionKind::SpendManaAsAnyType,
                        Duration::UntilEndOfTurn,
                    )],
                    Some(PlayerRef::You),
                ));
            }
        }
        ts.restore(scp);
    }

    // A. Spend restriction (`Spend this mana only to ...` / `This mana can't be spent to ...`)
    if ts.eat_phrase(&["this", "mana", "can't", "be", "spent", "to"]) {
        let forbid = |purpose| {
            Ok((
                vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::ManaRule(ManaRuleKind::SpendForbidden(purpose)),
                    Duration::Instant,
                )],
                Some(PlayerRef::You),
            ))
        };
        if ts.eat_word("cast")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return forbid(crate::v2::ir::ManaSpendPurpose::CastSpells(Some(flt)));
        }
        if ts.eat_any_phrase(&[&["activate", "abilities"], &["activate", "an", "ability"]]) {
            let flt = if ts.eat_word("of") {
                let f = opt(ts, parse_object_filter);
                let _ = ts.eat_any_word(&["source", "sources"]);
                f
            } else {
                None
            };
            return forbid(crate::v2::ir::ManaSpendPurpose::ActivateAbilities(flt));
        }
        if let Some(purpose) = restrictions::try_parse_extra_mana_spend_purpose(ts) {
            return forbid(purpose);
        }
        return Err(ts.err("expected mana spend purpose after `This mana can't be spent to`"));
    }
    if ts.eat_phrase(&[
        "spend", "this", "mana", "only", "on", "costs", "that", "contain",
    ]) && ts.eat_mana_cost().is_some()
    {
        return Ok((
            vec![mutate_player(
                PlayerRef::You,
                PlayerMutation::ManaRule(ManaRuleKind::SpendOnlyColorOnX(ColorMask::EMPTY)),
                Duration::Instant,
            )],
            Some(PlayerRef::You),
        ));
    }
    if ts.eat_phrase(&["spend", "this", "mana", "only", "to"]) {
        if ts.eat_phrase(&["cast", "spells", "from"]) {
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::ManaRule(ManaRuleKind::SpendRestriction(ManaSpendRule {
                        only_for: true,
                        cast_from_zone: Some(zref.zone),
                        spell_filter: None,
                    })),
                    Duration::Instant,
                )],
                Some(PlayerRef::You),
            ));
        }
        let mut purposes = Vec::new();
        loop {
            let pcp = ts.checkpoint();
            if let Some(purpose) = restrictions::try_parse_extra_mana_spend_purpose(ts) {
                purposes.push(purpose);
            } else if ts.eat_phrase(&["foretell", "a", "card", "from", "your", "hand"]) {
                purposes.push(crate::v2::ir::ManaSpendPurpose::Foretell);
            } else if ts
                .eat_any_phrase(&[&["activate", "abilities"], &["activate", "an", "ability"]])
            {
                let flt = if ts.eat_word("of") {
                    let f = opt(ts, parse_object_filter);
                    let _ = ts.eat_any_word(&["source", "sources"]);
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
            let _ = ts.eat_word("to");
        }
        if purposes.len() == 1
            && let Some(crate::v2::ir::ManaSpendPurpose::CastSpells(Some(flt))) = purposes.first()
        {
            return Ok((
                vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::ManaRule(ManaRuleKind::SpendRestriction(ManaSpendRule {
                        only_for: true,
                        cast_from_zone: None,
                        spell_filter: Some(flt.clone()),
                    })),
                    Duration::Instant,
                )],
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
                vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::ManaRule(ManaRuleKind::SpendPurpose(purpose)),
                    Duration::Instant,
                )],
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
            vec![apply_replacement(rep, Duration::UntilEndOfTurn)],
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

    // `Each opponent attacking <player> does the same` (`Curse of Disturbance`, `Curse of Vitality`, `Curse of Opulence`)
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["each", "opponent", "attacking"])
            && let Ok(p) = try_parse(ts, parse_player_ref)
            && ts.eat_phrase(&["does", "the", "same"])
        {
            return Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::Player(p),
                    body: vec![Effect::IfPrevious {
                        succeeded: true,
                        body: Vec::new(),
                    }],
                }],
                Some(PlayerRef::EachOpponent),
            ));
        }
        ts.restore(ccp);
    }

    // `the damage can't be prevented` as a clause (`Demonfire`, `Urza's Rage`, `Skullcrack`)
    if ts.eat_phrase(&["the", "damage", "can't", "be", "prevented"]) {
        return Ok((
            vec![apply_continuous(
                StaticAbility::RuleRestriction {
                    subject: None,
                    rule: RuleRestrictionKind::DamageCantBePrevented,
                    condition: None,
                },
                Duration::UntilEndOfTurn,
            )],
            inherited_actor,
        ));
    }
    // `The same is true for <kw_list>` (`Bleeding Effect`, `Thunderous Orator`)
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["the", "same", "is", "true", "for"])
            && try_parse(ts, crate::v2::filter::parse_keyword_list).is_ok()
        {
            return Ok((
                vec![apply_continuous(
                    StaticAbility::GlobalFlag(crate::v2::ir::StaticFlagKind::AppliesInAllZones),
                    Duration::UntilEndOfTurn,
                )],
                inherited_actor,
            ));
        }
        ts.restore(scp);
    }

    // `<player>'s life total becomes [equal to] <val>` as a clause (e.g. `exile ~ and your life total becomes 1`)
    {
        let lcp = ts.checkpoint();
        if let Some(player) = eat_possessive_life_total_becomes(ts) {
            let _ = ts.eat_phrase(&["equal", "to"]);
            if let Ok(amount) = try_parse(ts, parse_value_expr) {
                return Ok((
                    vec![Effect::AdjustLife {
                        player,
                        amount,
                        op: crate::v2::ir::LifeOp::Set,
                    }],
                    Some(player),
                ));
            }
        }
        ts.restore(lcp);
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

    // Compound possessive player actors and embedded blocking clauses
    if let Ok(res) = try_parse(ts, sentence_ext::parse_possessive_player_clause) {
        return Ok(res);
    }

    // Explicit player prefix + optional `each` + optional restrictive relative (`who can't`, `who didn't`, `with no cards in hand`) + optional `may` + verb frame
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            let _ = ts.eat_word("each");
            if let Ok(res) = try_parse(ts, |s| {
                sentence_ext::parse_player_relative_clause(s, player)
            }) {
                return Ok(res);
            }
            // `<player> who can't/didn't/doesn't [<prior_verb> this way] <verb>`
            {
                const WHO_NEG: &[&[&str]] =
                    &[&["who", "can't"], &["who", "didn't"], &["who", "doesn't"]];
                let wd_cp = ts.checkpoint();
                if ts.eat_any_phrase(WHO_NEG) {
                    if let Some(tw_idx) = find_phrase_index(ts.tokens, ts.pos, &["this", "way"])
                        && tw_idx > ts.pos
                        && tw_idx + 2 < ts.tokens.len()
                    {
                        let prior_slice = &ts.tokens[ts.pos..tw_idx];
                        let mut prior_ts = ts.sub_stream(prior_slice, ParseStage::Effect);
                        if parse_player_verb_frame(&mut prior_ts, player).is_ok()
                            && prior_ts.is_eof()
                        {
                            ts.pos = tw_idx + 2;
                            if let Ok((body, _)) =
                                try_parse(ts, |s| parse_player_verb_frame(s, player))
                            {
                                return Ok((
                                    vec![Effect::ForEach {
                                        over: ForEachDomain::EachPlayerWhoCant(player),
                                        body,
                                    }],
                                    Some(player),
                                ));
                            }
                        }
                    }
                    ts.restore(wd_cp);
                    if ts.eat_any_phrase(WHO_NEG)
                        && let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player))
                    {
                        return Ok((
                            vec![Effect::ForEach {
                                over: ForEachDomain::EachPlayerWhoCant(player),
                                body,
                            }],
                            Some(player),
                        ));
                    }
                }
                ts.restore(wd_cp);
            }
            // `<player> who controls [N or more] <flt> <verb>` (`Shatter the Sky`, `Depopulate`)
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["who", "controls"]) || ts.eat_phrase(&["who", "control"]) {
                    let min_count = ts
                        .next_number(false)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(1);
                    let _ = ts.eat_phrase(&["or", "more"]);
                    if let Ok(flt) = try_parse(ts, parse_object_filter)
                        && let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player))
                    {
                        return Ok((
                            vec![Effect::ForEach {
                                over: ForEachDomain::Player(player),
                                body: vec![Effect::BranchCondition {
                                    condition: crate::v2::ir::ConditionExpr::Controls {
                                        player: PlayerRef::ThatPlayer,
                                        filter: Box::new(flt),
                                        min_count,
                                    },
                                    then_body: body,
                                    else_body: Vec::new(),
                                }],
                            }],
                            Some(player),
                        ));
                    }
                }
                ts.restore(ccp);
            }
            // `<player> who has <N> or more <kind> counters <verb>` (`Feed the Infection`, `Ixhel`)
            {
                let wcp = ts.checkpoint();
                if ts.eat_phrase(&["who", "has"])
                    && let Some(n) = ts.next_number(false)
                    && ts.eat_phrase(&["or", "more"])
                    && let Ok(kind) = try_parse(ts, crate::v2::filter::parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player))
                {
                    return Ok((
                        vec![Effect::ForEach {
                            over: ForEachDomain::EachOpponentWithCondition(
                                crate::v2::ir::ConditionExpr::PlayerHasCounters {
                                    player,
                                    kind,
                                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                                },
                            ),
                            body,
                        }],
                        Some(player),
                    ));
                }
                ts.restore(wcp);
                // `<player> who lost [<N> or more] life this turn <verb>` (`Davros`, `Papalymo`)
                if ts.eat_phrase(&["who", "lost"]) {
                    let min_life = if let Some(n) = ts.next_number(false) {
                        let _ = ts.eat_phrase(&["or", "more"]);
                        Some(n)
                    } else {
                        None
                    };
                    if ts.eat_phrase(&["life", "this", "turn"]) {
                        let cond = if let Some(n) = min_life {
                            crate::v2::ir::ConditionExpr::PlayerTurnEvent {
                                player,
                                event: crate::v2::ir::TurnEventKind::LifeLost,
                                filter: None,
                                min_count: u16::try_from(n.max(1)).unwrap_or(1),
                            }
                        } else {
                            crate::v2::ir::ConditionExpr::PlayerCheck {
                                player,
                                kind: crate::v2::ir::PlayerConditionKind::LostLifeThisTurn,
                            }
                        };
                        if let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player))
                        {
                            return Ok((
                                vec![Effect::ForEach {
                                    over: ForEachDomain::EachOpponentWithCondition(cond),
                                    body,
                                }],
                                Some(player),
                            ));
                        }
                    }
                }
                ts.restore(wcp);
            }
            // `<player> with no cards in hand <verb>` (`Tinybones, Trinket Thief`)
            if ts.eat_phrase(&["with", "no", "cards", "in", "hand"])
                && let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, player))
            {
                return Ok((
                    vec![Effect::ForEach {
                        over: ForEachDomain::EachOpponentWithCondition(
                            crate::v2::ir::ConditionExpr::ZoneMetric {
                                zone: crate::v2::ir::ZoneRef {
                                    zone: crate::v2::ir::ZoneKind::Hand,
                                    owner: crate::v2::ir::ZoneOwner::Opponent,
                                },
                                kind: crate::v2::ir::ZoneMetricKind::IsEmpty,
                                min_count: 0,
                            },
                        ),
                        body,
                    }],
                    Some(player),
                ));
            }
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

    // F2. Quantified / modified player subjects parsed by `parse_subject_ref`
    //     (e.g. `Another target player creates ...`, `Up to one target player puts ...`, `Two target players exchange ...`)
    {
        let tp_cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            let player_actor = match subj {
                SubjectRef::TargetPlayer { opponent_only, .. } => Some(if opponent_only {
                    PlayerRef::TargetOpponent
                } else {
                    PlayerRef::TargetPlayer
                }),
                SubjectRef::Player(p) => Some(p),
                _ => None,
            };
            if let Some(p_actor) = player_actor {
                let optional = ts.eat_word("may");
                if let Ok((body, _)) = try_parse(ts, |s| parse_player_verb_frame(s, p_actor)) {
                    let effs = if optional {
                        vec![Effect::Optional {
                            actor: p_actor,
                            body,
                        }]
                    } else {
                        body
                    };
                    return Ok((effs, Some(p_actor)));
                }
            }
        }
        ts.restore(tp_cp);
    }

    // G. Subject-headed object verb frames:
    //    `<src> deals N damage to <tgt>`, `<subj> gets +X/+Y ...`, `<subj> gains <kws>`,
    //    `<subj> becomes ...`, `<subj> is goaded ...`, `<subj> can't ...`
    let effs = parse_subject_verb_frame(ts)?;
    Ok((effs, inherited_actor))
}

fn find_where_x_is(tokens: &[Token<'_>], start: usize) -> Option<(usize, usize)> {
    if tokens.len() < start + 5 {
        return None;
    }
    for i in start..tokens.len().saturating_sub(4) {
        if matches!(tokens[i].kind, TokenKind::Comma)
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("where"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x"))
            && matches!(tokens[i + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("is"))
        {
            return Some((i, i + 4));
        }
        if matches!(tokens[i].kind, TokenKind::Quoted(q) if q.ends_with(','))
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("where"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x"))
            && matches!(tokens[i + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("is"))
        {
            return Some((i + 1, i + 4));
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

pub(super) fn find_phrase_index(
    tokens: &[Token<'_>],
    start: usize,
    words: &[&str],
) -> Option<usize> {
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

fn eat_possessive_life_total_becomes(ts: &mut TokenStream<'_>) -> Option<PlayerRef> {
    const TABLE: &[(&[&str], PlayerRef)] = &[
        (
            &["each", "player's", "life", "total", "becomes"],
            PlayerRef::EachPlayer,
        ),
        (
            &["each", "opponent's", "life", "total", "becomes"],
            PlayerRef::EachOpponent,
        ),
        (
            &["target", "player's", "life", "total", "becomes"],
            PlayerRef::TargetPlayer,
        ),
        (
            &["target", "opponent's", "life", "total", "becomes"],
            PlayerRef::TargetOpponent,
        ),
        (
            &["that", "player's", "life", "total", "becomes"],
            PlayerRef::ThatPlayer,
        ),
        (
            &["their", "life", "total", "becomes"],
            PlayerRef::ThatPlayer,
        ),
        (&["your", "life", "total", "becomes"], PlayerRef::You),
    ];
    TABLE
        .iter()
        .find_map(|&(phrase, player)| ts.eat_phrase(phrase).then_some(player))
}
