//! Compositional top-down effect-sentence and verb-frame clause parser for `v2`.
//!
//! Grammar structure:
//! - `Sentence -> BindX | VillainousChoice | Conditional | DelayedTrigger | ScopedDuration | ForEach | ClauseSeq`
//! - `ClauseSeq -> Clause ((", then" | ", and" | "and") Clause)*` with explicit `active_actor` propagation
//! - `Clause -> [Actor] VerbFrame Objects Modifiers [Duration]`
//!
//! Strictly fail-closed: every sentence MUST consume all tokens (`expect_eof`).

use crate::FaceInput;
use crate::ast::{CardTypeMask, ColorMask};
use crate::v2::card::compile_face_v2;
use crate::v2::condition::parse_condition;
use crate::v2::cost::parse_cost_atom;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_dest, parse_zone_ref,
};
use crate::v2::ir::{
    ActivationRestriction, CharacteristicKind, ChoiceKind, CopyException, Cost, CountSpec,
    CounterKind, DelayedTriggerKind, Duration, Effect, EntryModifiers, ForEachDomain, LifeOp,
    ManaSpec, ManaSpendRule, ManaSymbol, ObjectFilter, ObjectMutation, ParseError, ParseStage,
    PlayerMutation, PlayerRef, PlayerRestrictionKind, PredefinedToken, Replacement,
    RuleRestrictionKind, StaticAbility, SubjectRef, TokenSpec, ValueExpr, VarName, ZoneDest,
    ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::{Token, TokenKind, tokenize};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::replacement::parse_replacement;
use crate::v2::static_ability::parse_static_ability;
use crate::v2::trigger::{parse_step_with_owner, parse_trigger_header};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};
use crate::vocab::{Vocabulary, lookup_card_type, lookup_color};

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

    // 2. Leading `If <player> do/does/don't/doesn't, <effect>`
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
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("when"))
            && let Ok(trig) = try_parse(ts, parse_trigger_header)
            && ts.eat_comma()
        {
            let body = parse_sentence_top(ts)?;
            return Ok(vec![Effect::DelayedTrigger {
                trigger: DelayedTriggerKind::Event(trig),
                body,
            }]);
        }
        ts.restore(cp);
    }

    // 6. Scoped leading duration: `Until end of turn, <body>` / `This turn and next turn, <body>`
    if ts.eat_phrase(&["this", "turn", "and", "next", "turn"]) && ts.eat_comma() {
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

    // 8. Activation restrictions (`Activate only ...`)
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

    // 10b. Trailing `unless <player> pays <cost>` / `unless <cond>` on any sentence
    if let Some(u_idx) = find_top_level_word(ts.tokens, ts.pos, "unless")
        && u_idx > ts.pos
    {
        let body_slice = &ts.tokens[ts.pos..u_idx];
        let tail_slice = &ts.tokens[u_idx + 1..];
        let mut tail_ts = ts.sub_stream(tail_slice, ParseStage::Effect);
        if let Ok(payer) = try_parse(&mut tail_ts, parse_player_ref)
            && (tail_ts.eat_word("pays") || tail_ts.eat_word("pay"))
            && let Ok(cost) = try_parse(&mut tail_ts, parse_cost_atom)
            && tail_ts.is_eof()
        {
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
        } else {
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
    }

    // 11. Trailing `at the beginning of <step>` delayed suffix on a sentence
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
    // A. Spend restriction (`Spend this mana only to cast spells from exile`)
    if ts.eat_phrase(&[
        "spend", "this", "mana", "only", "to", "cast", "spells", "from",
    ]) {
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
    if !ts.eat_phrase(&["activate", "only"]) {
        return Err(ts.err("expected `Activate only`"));
    }
    let mut out = Vec::new();
    loop {
        if ts.eat_phrase(&["as", "a", "sorcery"]) {
            out.push(ActivationRestriction::AsSorcery);
        } else if ts.eat_phrase(&["as", "an", "instant"]) {
            out.push(ActivationRestriction::AsInstant);
        } else if ts.eat_phrase(&["during", "your", "turn"]) {
            out.push(ActivationRestriction::DuringYourTurn);
        } else if ts.eat_phrase(&["during", "combat"]) {
            out.push(ActivationRestriction::DuringCombat);
        } else if ts.eat_phrase(&["once", "each", "turn"]) {
            out.push(ActivationRestriction::OnceEachTurn);
        } else if ts.eat_word("once") {
            out.push(ActivationRestriction::Once);
        } else {
            return Err(ts.err("expected timing restriction after `Activate only`"));
        }
        if ts.eat_phrase(&["and", "only"]) {
            continue;
        }
        break;
    }
    Ok(out)
}

/// Verb frames with a known player actor (`actor`).
fn parse_player_verb_frame(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    // 1. `Add <mana>`
    if ts.eat_word("add") || ts.eat_word("adds") {
        if ts.eat_phrase(&["one", "mana", "of", "any", "color"]) {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::AddMana(ManaSpec::AnyColor {
                        amount: ValueExpr::Const(1),
                        single_color: true,
                    }),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if let Some(mc) = ts.eat_mana_cost() {
            return Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::AddMana(ManaSpec::Exact(mc)),
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        return Err(ts.err("expected mana spec after `add`"));
    }

    // 2. `get(s) {E}...` / `get(s) an emblem with "..."`
    if ts.eat_word("get") || ts.eat_word("gets") {
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
        return Err(ts.err("expected emblem or energy after `get(s)`"));
    }

    // 3. `play(s) ...` (both `play lands and cast spells from <zone> [dur]`, `plays with their hand revealed`, and `play that card [dur]`)
    if ts.eat_word("play") || ts.eat_word("plays") {
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
                    effect: Box::new(StaticAbility::PlayFromZone {
                        player: actor,
                        zone,
                        top_only,
                        lands: true,
                        spells: true,
                    }),
                    duration: dur,
                }],
                (),
            ));
        }
        let subj = parse_subject_ref(ts)?;
        let dur = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::GrantPlayPermission {
                    cast_only: false,
                    subject: Box::new(subj),
                    free: false,
                },
                duration: dur,
            }],
            (),
        ));
    }

    // 3b. `can't cast spells [or activate abilities]`
    if ts.eat_phrase(&["can't", "cast", "spells"]) {
        let mut out = vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::PlayerRestriction {
                player: actor,
                restriction: PlayerRestrictionKind::CantCastSpells,
                condition: None,
            }),
            duration: Duration::UntilEndOfTurn,
        }];
        if ts.eat_phrase(&["or", "activate", "abilities"]) {
            out.push(Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player: actor,
                    restriction: PlayerRestrictionKind::CantActivateAbilities,
                    condition: None,
                }),
                duration: Duration::UntilEndOfTurn,
            });
        }
        return Ok((out, ()));
    }

    // 3c. `cast(s) <flt> as though they had flash`
    if ts.eat_word("cast") || ts.eat_word("casts") {
        let cp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["as", "though", "they", "had", "flash"])
        {
            return Ok((
                vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::CastAsThoughFlash {
                        player: actor,
                        filter,
                    }),
                    duration: Duration::UntilEndOfTurn,
                }],
                (),
            ));
        }
        ts.restore(cp);
    }

    // 4. `pay(s) <cost>`
    if ts.eat_word("pay") || ts.eat_word("pays") {
        let cost = if let Some(mc) = ts.eat_mana_cost() {
            Cost::Mana(mc)
        } else {
            let amt = parse_value_expr(ts)?;
            if ts.eat_word("life") {
                Cost::PayLife(amt)
            } else {
                return Err(ts.err("expected `life` or mana after `pay`"));
            }
        };
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::PayCost(cost),
                duration: Duration::Instant,
            }],
            (),
        ));
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

    // 7. `draw(s) ...`
    if ts.eat_word("draw") || ts.eat_word("draws") {
        let count = if ts.eat_phrase(&["a", "number", "of", "cards", "equal", "to"]) {
            parse_value_expr(ts)?
        } else {
            let base = if ts.eat_word("a") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let _ = ts.eat_word("additional");
            if !(ts.eat_word("card") || ts.eat_word("cards")) {
                return Err(ts.err("expected `card(s)` after `draw`"));
            }
            if let Some(fe) = opt(ts, parse_for_each_expr) {
                if base == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(base), Box::new(fe))
                }
            } else {
                base
            }
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Draw,
                subject: SubjectRef::Player(actor),
                from: vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                to: Some(ZoneDest {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::Default,
                    library_position: None,
                    library_order: None,
                    under_control: None,
                }),
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 8. `discard(s) ...`
    if ts.eat_word("discard") || ts.eat_word("discards") {
        if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Discard {
                        hand: true,
                        random: false,
                    },
                    subject: SubjectRef::Player(actor),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
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
        let up_to = ts.eat_phrase(&["up", "to"]);
        let cnt = if ts.eat_word("a") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        if !(ts.eat_word("card") || ts.eat_word("cards")) {
            return Err(ts.err("expected `card(s)` after `discard`"));
        }
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Discard {
                    hand: false,
                    random: false,
                },
                subject: SubjectRef::Player(actor),
                from: vec![ZoneRef {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::Default,
                }],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count: cnt,
                    up_to,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 9. `sacrifice(s) ...`
    if ts.eat_word("sacrifice") || ts.eat_word("sacrifices") {
        // Handle anaphoric / `~` / `all <flt>` subjects (`Sacrifice it`, `Sacrifice ~`, `sacrifices all permanents in the pile of their choice`)
        let is_explicit_subject = matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Tilde))
            || ts.peek_word().is_some_and(|w| {
                w.eq_ignore_ascii_case("it")
                    || w.eq_ignore_ascii_case("them")
                    || w.eq_ignore_ascii_case("all")
                    || w.eq_ignore_ascii_case("that")
                    || w.eq_ignore_ascii_case("those")
            });
        if is_explicit_subject {
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Sacrifice,
                    subject,
                    from: vec![ZoneRef {
                        zone: ZoneKind::Battlefield,
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
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let _ = ts.eat_phrase(&["of", "those"]);
        let flt = parse_object_filter(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Sacrifice,
                subject: SubjectRef::Group(Box::new(flt)),
                from: vec![ZoneRef {
                    zone: ZoneKind::Battlefield,
                    owner: ZoneOwner::Default,
                }],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 10. `create(s) <token>`
    if ts.eat_word("create") || ts.eat_word("creates") {
        let (count, spec, modifiers) = parse_token_creation(ts)?;
        return Ok((
            vec![Effect::CreateToken {
                controller: actor,
                count,
                spec,
                modifiers,
            }],
            (),
        ));
    }

    // 11. `destroy(s) <subj>`
    if ts.eat_word("destroy") || ts.eat_word("destroys") {
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Destroy {
                    can_regenerate: true,
                },
                subject,
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 12. `exile(s) ...`
    if ts.eat_word("exile") || ts.eat_word("exiles") {
        // `exile their hand` / `exile all cards from their hand [face down]`
        if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["your", "hand"]) {
            let face_down = ts.eat_phrase(&["face", "down"]);
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down,
                        counters: None,
                    },
                    subject: SubjectRef::All(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![ZoneRef {
                            zone: ZoneKind::Hand,
                            owner: ZoneOwner::ThatPlayer,
                        }],
                        ..Default::default()
                    })),
                    from: vec![ZoneRef {
                        zone: ZoneKind::Hand,
                        owner: ZoneOwner::ThatPlayer,
                    }],
                    to: None,
                    modifiers: EntryModifiers {
                        face_down,
                        ..EntryModifiers::default()
                    },
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        // `exile the top [N] card(s) of <owner>'s library`
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("card") || ts.eat_word("cards")) {
                    return Err(ts.err("expected `card(s)` after `the top <N>`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of` after `the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            let face_down = ts.eat_phrase(&["face", "down"]);
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Exile {
                        face_down,
                        counters: None,
                    },
                    subject: SubjectRef::Group(Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![zref],
                        ..Default::default()
                    })),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers {
                        face_down,
                        ..EntryModifiers::default()
                    },
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            ));
        }

        // Compound 3-way exile (`Exile all enchantments from the battlefield, all instant cards from all graveyards, and all artifact cards from all hands`)
        let first = parse_subject_ref(ts)?;
        let mut subjects = vec![first];
        loop {
            let cp = ts.checkpoint();
            if ts.eat_comma() {
                let _ = ts.eat_word("and");
                if ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("all"))
                    && let Ok(next_s) = try_parse(ts, parse_subject_ref)
                {
                    subjects.push(next_s);
                    continue;
                }
            }
            ts.restore(cp);
            break;
        }
        let subject = if subjects.len() == 1 {
            subjects.remove(0)
        } else {
            SubjectRef::Compound(subjects)
        };
        let mut from = Vec::new();
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        let face_down = ts.eat_phrase(&["face", "down"]);
        // Optional `with <N> <kind> counters on it` / `with a number of <kind> counters on it equal to <val>`
        let counters = if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let wcp = ts.checkpoint();
            ts.eat_word("with");
            if ts.eat_phrase(&["a", "number", "of"]) {
                let ck = parse_counter_kind(ts)?;
                if (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["on", "it", "equal", "to"])
                {
                    let val = parse_value_expr(ts)?;
                    Some((ck, val))
                } else {
                    ts.restore(wcp);
                    None
                }
            } else if let Ok(val) = try_parse(ts, parse_value_expr)
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
            {
                let _ = ts.eat_phrase(&["on", "it"]);
                Some((ck, val))
            } else {
                ts.restore(wcp);
                None
            }
        } else {
            None
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Exile {
                    face_down,
                    counters,
                },
                subject,
                from,
                to: None,
                modifiers: EntryModifiers {
                    face_down,
                    ..EntryModifiers::default()
                },
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 13. `return(s) ...` (both `return <subj> [except <exc>] to <dest>` AND `returns to their hand <subj>`)
    if ts.eat_word("return") || ts.eat_word("returns") {
        // Inverted destination-first: `returns to their hand each card they exiled this way`
        if ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("to")) {
            let (dest, modifiers) = parse_zone_dest(ts)?;
            let subject = parse_subject_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Move,
                    subject,
                    from: Vec::new(),
                    to: Some(dest),
                    modifiers,
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        let except = if ts.eat_word("except") {
            Some(Box::new(parse_subject_ref(ts)?))
        } else {
            None
        };
        let (dest, modifiers) = parse_zone_dest(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject,
                from: Vec::new(),
                to: Some(dest),
                modifiers,
                except,
                count: None,
            }],
            (),
        ));
    }

    // 14. `put(s) ...` (BOTH `put <cnt> <kind> counter(s) on <subj>` AND `put <subj> into/onto/on <dest>`)
    if ts.eat_word("put") || ts.eat_word("puts") {
        // 14a. `put a number of <kind> counters on <subj> equal to <val>`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["a", "number", "of"])
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
            {
                let subject = parse_subject_ref(ts)?;
                if ts.eat_phrase(&["equal", "to"]) {
                    let count = parse_value_expr(ts)?;
                    return Ok((
                        vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::PutCounters { kind, count },
                            duration: Duration::Instant,
                        }],
                        (),
                    ));
                }
            }
            ts.restore(cp);
        }
        // 14b. `put <cnt> <kind> counter(s) on <subj> [for each/every ...]`
        {
            let cp = ts.checkpoint();
            let base_cnt = if ts.eat_word("a") || ts.eat_word("an") {
                Some(ValueExpr::Const(1))
            } else {
                opt(ts, parse_value_expr)
            };
            if let Some(base) = base_cnt
                && let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
                && ts.eat_word("on")
            {
                let subject = parse_subject_ref(ts)?;
                let count = if let Some(fe) = opt(ts, parse_for_each_expr) {
                    if base == ValueExpr::Const(1) {
                        fe
                    } else {
                        ValueExpr::Mul(Box::new(base), Box::new(fe))
                    }
                } else {
                    base
                };
                return Ok((
                    vec![Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::PutCounters { kind, count },
                        duration: Duration::Instant,
                    }],
                    (),
                ));
            }
            ts.restore(cp);
        }
        // 14c. `put <subj> into/onto/on <dest> [and <subj2> into/onto/on <dest2>]`
        //      (NEVER greedily swallows a second `put ...` clause!)
        let subject = parse_subject_ref(ts)?;
        let (dest, modifiers) = parse_zone_dest(ts)?;
        let mut out = vec![Effect::ZoneOp {
            actor,
            op: ZoneOpKind::Move,
            subject,
            from: Vec::new(),
            to: Some(dest),
            modifiers,
            except: None,
            count: None,
        }];
        let cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(s2) = try_parse(ts, parse_subject_ref)
            && let Ok((d2, m2)) = try_parse(ts, parse_zone_dest)
        {
            out.push(Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Move,
                subject: s2,
                from: Vec::new(),
                to: Some(d2),
                modifiers: m2,
                except: None,
                count: None,
            });
        } else {
            ts.restore(cp);
        }
        return Ok((out, ()));
    }

    // 15. `move(s) <cnt> [<kind>] counter(s) from <src> onto/to <dst>` (NEVER stores `"a"`!)
    if ts.eat_word("move") || ts.eat_word("moves") {
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            parse_value_expr(ts)?
        };
        let kind = if ts.eat_word("counter") || ts.eat_word("counters") {
            CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !(ts.eat_word("counter") || ts.eat_word("counters")) {
                return Err(ts.err("expected `counter(s)` after counter kind"));
            }
            ck
        };
        if !ts.eat_word("from") {
            return Err(ts.err("expected `from` in move counters"));
        }
        let from = parse_subject_ref(ts)?;
        if !(ts.eat_word("onto") || ts.eat_word("to")) {
            return Err(ts.err("expected `onto` or `to` in move counters"));
        }
        let to = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject: from.clone(),
                mutation: ObjectMutation::MoveCounters {
                    from: Box::new(from),
                    to: Box::new(to),
                    kind,
                    count,
                },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 16. `distribute a number of <kind> counters equal to <val> among <subj>`
    if ts.eat_word("distribute") || ts.eat_word("distributes") {
        let count_first = if ts.eat_phrase(&["a", "number", "of"]) {
            None
        } else {
            Some(parse_value_expr(ts)?)
        };
        let kind = parse_counter_kind(ts)?;
        if !(ts.eat_word("counters") || ts.eat_word("counter")) {
            return Err(ts.err("expected `counters` in distribute"));
        }
        let count = match count_first {
            Some(c) => c,
            None => {
                if !ts.eat_phrase(&["equal", "to"]) {
                    return Err(ts.err("expected `equal to` in distribute"));
                }
                parse_value_expr(ts)?
            }
        };
        if !ts.eat_word("among") {
            return Err(ts.err("expected `among` in distribute"));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::DistributeCounters { kind, count },
                duration: Duration::Instant,
            }],
            (),
        ));
    }

    // 17. `scry(ies) <N>` / `surveil(s) <N>` / `shuffle`
    if ts.eat_word("scry") || ts.eat_word("scries") {
        let amt = parse_value_expr(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Scry(amt),
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("surveil") || ts.eat_word("surveils") {
        let amt = parse_value_expr(ts)?;
        return Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Surveil(amt),
                duration: Duration::Instant,
            }],
            (),
        ));
    }
    if ts.eat_word("shuffle") || ts.eat_word("shuffles") {
        let _ = ts.eat_phrase(&["your", "library"]) || ts.eat_phrase(&["their", "library"]);
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Shuffle,
                subject: SubjectRef::Player(actor),
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }

    // 18. `search your library for [exactly] <N> <flt>`
    if ts.eat_word("search") || ts.eat_word("searches") {
        let lib = parse_zone_ref(ts)?;
        if !ts.eat_word("for") {
            return Err(ts.err("expected `for` in search library"));
        }
        let exact = ts.eat_word("exactly");
        let count = if ts.eat_word("a") || ts.eat_word("an") {
            ValueExpr::Const(1)
        } else {
            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
        };
        let flt = parse_object_filter(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::SearchLibrary {
                    reveal: false,
                    shuffle: false,
                    exact,
                },
                subject: SubjectRef::Group(Box::new(flt)),
                from: vec![lib],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: !exact,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 19. `reveal(s) [the top <N> cards of <lib> | <subj>]` / `look(s) at the top <N> cards of <lib>`
    if ts.eat_word("reveal") || ts.eat_word("reveals") {
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                if !(ts.eat_word("cards") || ts.eat_word("card")) {
                    return Err(ts.err("expected `card(s)` after `reveal the top`"));
                }
                n
            };
            if !ts.eat_word("of") {
                return Err(ts.err("expected `of` after `reveal the top card(s)`"));
            }
            let zref = parse_zone_ref(ts)?;
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::LookTop { reveal: true },
                    subject: SubjectRef::Player(actor),
                    from: vec![zref],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: Some(CountSpec {
                        count,
                        up_to: false,
                        any_number: false,
                    }),
                }],
                (),
            ));
        }
        let subject = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::Reveal,
                subject,
                from: Vec::new(),
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: None,
            }],
            (),
        ));
    }
    if ts.eat_phrase(&["look", "at", "the", "top"]) || ts.eat_phrase(&["looks", "at", "the", "top"])
    {
        let count = parse_value_expr(ts)?;
        if !(ts.eat_word("cards") || ts.eat_word("card")) || !ts.eat_word("of") {
            return Err(ts.err("expected `cards of <library>` after `look at the top`"));
        }
        let zref = parse_zone_ref(ts)?;
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::LookTop { reveal: false },
                subject: SubjectRef::Player(actor),
                from: vec![zref],
                to: None,
                modifiers: EntryModifiers::default(),
                except: None,
                count: Some(CountSpec {
                    count,
                    up_to: false,
                    any_number: false,
                }),
            }],
            (),
        ));
    }

    // 20. `separate(s) them into a face-down pile and a face-up pile`
    if ts.eat_word("separate") || ts.eat_word("separates") {
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&[
            "into",
            "a",
            "face-down",
            "pile",
            "and",
            "a",
            "face-up",
            "pile",
        ]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 1,
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["into", "two", "piles"]) {
            return Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::SeparateIntoPiles {
                        pile_count: 2,
                        face_down_piles: 0,
                    },
                    subject,
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            ));
        }
        return Err(ts.err("expected pile specification after `separate`"));
    }

    // 21. `choose(s) <characteristic | target>`
    if ts.eat_word("choose") || ts.eat_word("chooses") {
        if ts.eat_phrase(&["a", "creature", "type"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CreatureType),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "card", "type"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CardType),
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["a", "color"]) {
            return Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Color),
                }],
                (),
            ));
        }
        // Optional leading count (`choose two cards in your hand drawn this turn`)
        let _ = opt(ts, parse_value_expr);
        let subj = parse_subject_ref(ts)?;
        return Ok((
            vec![Effect::PromptChoice {
                chooser: actor,
                kind: ChoiceKind::ChooseObject(subj),
            }],
            (),
        ));
    }

    // 22. `untap(s) <subj>` / `tap(s) <subj>`
    if ts.eat_word("untap") || ts.eat_word("untaps") {
        let subject = parse_subject_ref(ts)?;
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
                    mutation: ObjectMutation::TurnFaceUp,
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
                    mutation: ObjectMutation::TurnFaceUp,
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
        if ts.eat_phrase(&["face", "down"]) {
            return Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::TurnFaceDown,
                    duration: Duration::Instant,
                }],
                (),
            ));
        }
    }

    // 24. `counter <subj> [unless its controller pays <cost>]`
    if ts.eat_word("counter") {
        let subject = parse_subject_ref(ts)?;
        let unless_pay = if ts.eat_phrase(&["unless", "its", "controller", "pays"]) {
            Some(Box::new(parse_cost_atom(ts)?))
        } else {
            None
        };
        return Ok((
            vec![Effect::ZoneOp {
                actor,
                op: ZoneOpKind::CounterSpell { unless_pay },
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

    Err(ts.err("expected player verb frame"))
}

/// Subject-headed verb frames (`<src> deals ...`, `<subj> gets ...`, `<subj> gains ...`,
/// `<subj> becomes ...`, `<subj> is goaded ...`, `<subj> can't ...`).
fn parse_subject_verb_frame(ts: &mut TokenStream<'_>) -> PResult<Vec<Effect>> {
    if ts.eat_phrase(&["damage", "can't", "be", "prevented"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::RuleRestriction {
                subject: None,
                rule: RuleRestrictionKind::DamageCantBePrevented,
                condition: None,
            }),
            duration,
        }]);
    }
    let subject = parse_subject_ref(ts)?;

    // 1. `<src> deals <amt> damage [divided as <player> chooses among <tgt> | to <tgt> [, <amt2> damage to <tgt2>, and <amt3> damage to <tgt3>]]`
    if ts.eat_word("deals") || ts.eat_word("deal") {
        let mut out = Vec::new();
        loop {
            let amount = parse_value_expr(ts)?;
            if !ts.eat_word("damage") {
                return Err(ts.err("expected `damage` after amount"));
            }
            if ts.eat_phrase(&["divided", "as"]) {
                let divider = parse_player_ref(ts)?;
                if !(ts.eat_word("chooses") || ts.eat_word("choose")) || !ts.eat_word("among") {
                    return Err(ts.err("expected `chooses among` in divided damage"));
                }
                let target = parse_subject_ref(ts)?;
                out.push(Effect::DealDamage {
                    source: subject.clone(),
                    target,
                    amount,
                    divided_by: Some(divider),
                });
                break;
            }
            if !ts.eat_word("to") {
                return Err(ts.err("expected `to` after `damage`"));
            }
            let target = parse_subject_ref(ts)?;
            out.push(Effect::DealDamage {
                source: subject.clone(),
                target,
                amount,
                divided_by: None,
            });
            let cp = ts.checkpoint();
            if ts.eat_comma() {
                let _ = ts.eat_word("and");
                if opt(ts, parse_value_expr).is_some() {
                    ts.restore(cp);
                    ts.eat_comma();
                    let _ = ts.eat_word("and");
                    continue;
                }
            }
            ts.restore(cp);
            break;
        }
        return Ok(out);
    }

    // 2. `<subj> gets/get +X/+Y [and gains <kws>] [until end of turn] [for each <expr>]`
    if ts.eat_word("gets") || ts.eat_word("get") {
        let (power, toughness) = ts
            .next_pt_mod()
            .ok_or_else(|| ts.err("expected P/T modifier after `get(s)`"))?;
        let mut gained_kws = None;
        if ts.eat_phrase(&["and", "gains"]) || ts.eat_phrase(&["and", "gain"]) {
            let (kws, _) = parse_keyword_list(ts)?;
            gained_kws = Some(kws);
        }
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        let scale = opt(ts, parse_for_each_expr).map(Box::new);
        let mut out = vec![Effect::MutateObject {
            subject: subject.clone(),
            mutation: ObjectMutation::ModifyPT {
                power,
                toughness,
                scale,
            },
            duration: duration.clone(),
        }];
        if let Some(kws) = gained_kws {
            out.push(Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantKeywords(kws),
                duration,
            });
        }
        return Ok(out);
    }

    // 3. `<subj> gains/gain "<quoted ability>" [duration]` OR `<subj> gains/gain [your choice of] <kws> [duration]`
    //    CRITICAL RULE: Unqualified `gains <kw>` defaults to `Duration::Permanent`!
    if ts.eat_word("gains") || ts.eat_word("gain") {
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_face_v2(
                FaceInput {
                    name: "~",
                    type_line: "Permanent",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !compiled.is_ok() {
                return Err(compiled
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile granted ability text")));
            }
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::GrantAbility(Box::new(compiled)),
                duration,
            }]);
        }
        let is_choice = ts.eat_phrase(&["your", "choice", "of"]);
        let (kws, _) = parse_keyword_list(ts)?;
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        if is_choice {
            return Ok(vec![Effect::PromptChoice {
                chooser: PlayerRef::You,
                kind: ChoiceKind::ChooseKeywordAndGrant {
                    subject,
                    keywords: kws,
                    duration,
                },
            }]);
        }
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::GrantKeywords(kws),
            duration,
        }]);
    }

    // 4. `<subj> enters with <N> [additional] <kind> counters on it`
    if ts.eat_phrase(&["enters", "with"]) || ts.eat_phrase(&["enter", "with"]) {
        let count = parse_value_expr(ts)?;
        let additional = ts.eat_word("additional");
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_phrase(&["on", "it"]) {
            return Ok(vec![Effect::ApplyReplacement {
                replacement: Box::new(Replacement::EntersWithCounters {
                    subject: Some(subject),
                    kind,
                    count,
                    additional,
                }),
                duration: Duration::UntilEndOfTurn,
            }]);
        }
    }

    // 5. `<subj> becomes a creature in addition to its other types and has base power and base toughness each equal to <val>`
    if ts.eat_word("becomes") || ts.eat_word("become") {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let mut add_types = CardTypeMask::EMPTY;
        while let Some(w) = ts.peek_word() {
            if let Some(ct) = lookup_card_type(&w.to_ascii_lowercase()) {
                ts.next_word();
                add_types = add_types.union(ct);
            } else {
                break;
            }
        }
        let keep_existing_types = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
        if ts.eat_phrase(&[
            "and",
            "has",
            "base",
            "power",
            "and",
            "base",
            "toughness",
            "each",
            "equal",
            "to",
        ]) || ts.eat_phrase(&[
            "and",
            "has",
            "base",
            "power",
            "and",
            "toughness",
            "each",
            "equal",
            "to",
        ]) {
            let val = parse_value_expr(ts)?;
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
            return Ok(vec![Effect::MutateObject {
                subject,
                mutation: ObjectMutation::SetDynamicBasePT {
                    power: val.clone(),
                    toughness: val,
                    add_types,
                    keep_existing_types,
                },
                duration,
            }]);
        }
    }

    // 6. `<subj> is goaded [duration]`
    if ts.eat_phrase(&["is", "goaded"]) || ts.eat_phrase(&["are", "goaded"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation: ObjectMutation::Goad,
            duration,
        }]);
    }

    // 7. `<subj> can't attack [or block] / can't be blocked [duration]`
    if ts.eat_word("can't") {
        let mutation = if ts.eat_phrase(&["attack", "or", "block"]) {
            ObjectMutation::CantAttackOrBlock
        } else if ts.eat_word("attack") {
            ObjectMutation::CantAttack
        } else if ts.eat_word("block") {
            ObjectMutation::CantBlock
        } else if ts.eat_phrase(&["be", "blocked"]) {
            ObjectMutation::CantBeBlocked
        } else if ts.eat_phrase(&["be", "regenerated"]) {
            ObjectMutation::CantBeRegenerated
        } else {
            return Err(ts.err("expected combat restriction after `can't`"));
        };
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::MutateObject {
            subject,
            mutation,
            duration,
        }]);
    }

    Err(ts.err("expected verb predicate after subject"))
}

fn parse_token_creation(
    ts: &mut TokenStream<'_>,
) -> PResult<(ValueExpr, TokenSpec, EntryModifiers)> {
    let count = if ts.eat_word("a") || ts.eat_word("an") {
        ValueExpr::Const(1)
    } else {
        parse_value_expr(ts)?
    };

    // Copy token: `tokens that are copies of <src> [and that are tapped and attacking] [, except ...]`
    if (ts.eat_word("token") || ts.eat_word("tokens"))
        && (ts.eat_phrase(&["that", "are", "copies", "of"])
            || ts.eat_phrase(&["that's", "a", "copy", "of"]))
    {
        let source = parse_subject_ref(ts)?;
        let mut modifiers = EntryModifiers::default();
        if ts.eat_phrase(&["and", "that", "are", "tapped", "and", "attacking"])
            || ts.eat_phrase(&["that's", "tapped", "and", "attacking"])
        {
            modifiers.tapped = true;
            modifiers.attacking = true;
        }
        let mut exceptions = Vec::new();
        {
            let cp = ts.checkpoint();
            if ts.eat_comma() && ts.eat_word("except") {
                if ts.eat_phrase(&["it", "has"]) || ts.eat_phrase(&["they", "have"]) {
                    let (kws, _) = parse_keyword_list(ts)?;
                    exceptions.push(CopyException::AddKeywords(kws));
                } else if ts.eat_phrase(&["it", "isn't", "legendary"])
                    || ts.eat_phrase(&["they", "aren't", "legendary"])
                {
                    exceptions.push(CopyException::NotLegendary);
                } else {
                    ts.restore(cp);
                }
            } else {
                ts.restore(cp);
            }
        }
        return Ok((
            count,
            TokenSpec::CopyOf {
                source: Box::new(source),
                exceptions,
            },
            modifiers,
        ));
    }

    // Predefined tokens (`Treasure token`, `Clue token`, `Food token`, etc.)
    {
        let cp = ts.checkpoint();
        if let Some(w) = ts.next_word() {
            let predef = match w.to_ascii_lowercase().as_str() {
                "treasure" => Some(PredefinedToken::Treasure),
                "clue" => Some(PredefinedToken::Clue),
                "food" => Some(PredefinedToken::Food),
                "blood" => Some(PredefinedToken::Blood),
                "map" => Some(PredefinedToken::Map),
                "powerstone" => Some(PredefinedToken::Powerstone),
                "shard" => Some(PredefinedToken::Shard),
                _ => None,
            };
            if let Some(pt) = predef
                && (ts.eat_word("token") || ts.eat_word("tokens"))
            {
                return Ok((count, TokenSpec::Predefined(pt), EntryModifiers::default()));
            }
        }
        ts.restore(cp);
    }

    // Creature token: `<P>/<T> [colors] [subtypes...] [card_types...] token(s) [with <kws>] [that are <colors>]`
    let (p, t) = ts
        .next_base_pt()
        .ok_or_else(|| ts.err("expected P/T or predefined token name"))?;
    let mut colors = ColorMask::EMPTY;
    let mut card_types = CardTypeMask::EMPTY;
    let mut subtypes = Vec::new();
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if low == "token" || low == "tokens" {
            ts.next_word();
            break;
        }
        if low == "and" || low == "colorless" {
            ts.next_word();
            continue;
        }
        if let Some(c) = lookup_color(&low) {
            ts.next_word();
            colors = colors.union(c);
        } else if let Some(ct) = lookup_card_type(&low) {
            ts.next_word();
            card_types = card_types.union(ct);
        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
            ts.next_word();
            subtypes.push(sub.to_string());
        } else {
            return Err(ts.err(format!("unexpected token descriptor `{w}`")));
        }
    }

    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    if ts.eat_word("with") {
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_face_v2(
                FaceInput {
                    name: "~",
                    type_line: "Creature",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !compiled.is_ok() {
                return Err(compiled
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile token ability text")));
            }
            abilities.push(compiled);
        } else {
            let (kws, _) = parse_keyword_list(ts)?;
            keywords = kws;
        }
    }

    // Trailing `that are blue, black, and red`
    if ts.eat_phrase(&["that", "are"]) || ts.eat_phrase(&["that's"]) {
        loop {
            let _ = ts.eat_comma();
            let _ = ts.eat_word("and");
            if let Some(w) = ts.peek_word()
                && let Some(c) = lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                colors = colors.union(c);
                continue;
            }
            break;
        }
    }

    Ok((
        count,
        TokenSpec::Creature {
            power: ValueExpr::Const(p),
            toughness: ValueExpr::Const(t),
            colors,
            card_types,
            subtypes,
            keywords,
            abilities,
        },
        EntryModifiers::default(),
    ))
}

fn parse_trailing_duration(ts: &mut TokenStream<'_>) -> Option<Duration> {
    if ts.eat_phrase(&["until", "end", "of", "turn"]) || ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    if ts.eat_phrase(&["until", "end", "of", "combat"])
        || ts.eat_phrase(&["this", "combat"])
        || ts.eat_phrase(&["that", "combat"])
    {
        return Some(Duration::UntilEndOfCombat);
    }
    if ts.eat_phrase(&["until", "your", "next", "turn"]) {
        return Some(Duration::UntilYourNextTurn);
    }
    if ts.eat_phrase(&["until", "the", "end", "of", "your", "next", "turn"]) {
        return Some(Duration::UntilEndOfNextTurn);
    }
    if ts.eat_phrase(&["for", "as", "long", "as", "it", "remains", "exiled"]) {
        return Some(Duration::ForAsLongAsExiled);
    }
    if ts.eat_phrase(&["for", "as", "long", "as"]) {
        let cond = parse_condition(ts).ok()?;
        return Some(Duration::ForAsLongAs(Box::new(cond)));
    }
    if ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    None
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
